#!/usr/bin/env python3
"""Read a node's discharge log and fit its cell's voltage curve (roadmap D174).

    scripts/discharge.py /dev/ttyACM0 --csv run.csv     # a console image's port
    scripts/discharge.py transcript.txt --csv run.csv   # or a saved transcript

The node image keeps a sample of its cell an hour apart on battery, and
one at each boot and change of power or mode, on the external flash
(bark-fw/README.md, "Discharge log"). A console image prints them with
its `discharge` command, a line each, oldest first; given a port, this
script sends the command and reads the answer, given a file, it reads
the lines from it. It writes every sample to the CSV, if asked, and
fits the curve over the last discharge: the last run of samples on
battery, from the one where USB went, or the boot on battery, to the
last reading before USB came back or the node stopped.

What it assumes:

- The current is constant in each mode, as roadmap D169 seeds the
  battery estimate: a relay 6.41 mA, a sleepy node 0.59 mA, and 29 mA
  more while the GPS runs (`bark_proto::drain`; spec "Sleepy nodes").
  The microcontroller and the display are not in them. A run in one mode
  does not depend on its current at all, as the fraction of the charge
  left is a ratio of times; across modes, a relay that turns sleepy at
  its low level, the ratio of the currents places the turn. `--relay-ma`,
  `--sleepy-ma` and `--gps-ma` take better ones, such as the drains the
  node learned (`status`, `drain_relay` and `drain_sleepy`, in
  ten-thousandths of a percent of 850 mAh an hour).
- The run starts full and ends empty: the fraction left at a sample is
  one less the charge used up to it over the charge used in the whole
  run. The summary says whether the run looks it: a first reading near
  a full cell's and a last near the end of the curve, the node stopping
  rather than being plugged in. The charge drawn after the last sample,
  up to an hour's, is not counted.
- Readings are calibrated by the scale the node kept (`scale=` in the
  header), as the core reads them; without one they are as read, up to
  3 % off. The load's drop across the cell's resistance, a few mV, and
  the cell's temperature are in them.

It prints the voltage at 21 points, every 5 % of the charge from 0 to
100, beside the voltage the typical cell of Maxim AN4189, Table 1, has
there and the percent the core's present mapping shows for the fitted
voltage. With such a table the core's `battery_percent` would read the
level off it by straight lines between neighbouring points, in place of
the three points of `BatteryLevels::DESIGN` and `BATTERY_FULL_MV`, the
levels being the table's 20 % and 30 % and the full voltage its 100 %;
the battery estimate's error from the bends between them would go
(roadmap D169, Limits). The script changes no code.

Standard library only. Run with python3 -I.
"""

from __future__ import annotations

import argparse
import csv
import os
import re
import select
import stat
import sys
import termios
import time
import tty

# The currents of roadmap D169, mA: a relay's radio, divider and GPS in
# standby; a sleepy node's; the GPS running (bark-proto/src/drain.rs).
RELAY_MA = 6.412
SLEEPY_MA = 0.592
GPS_MA = 29.0
# The cell, mAh (LILYGO T-Echo README).
CELL_MAH = 850.0
# One, in the units of the node's calibration (bark_proto::BATTERY_SCALE_ONE).
SCALE_ONE = 1 << 15

# A typical Li-ion cell's open-circuit voltage against its charge
# (Maxim AN4189, Table 1), percent to mV.
AN4189 = [(0, 3306), (5, 3675), (10, 3687), (15, 3710), (20, 3741), (30, 3775), (40, 3793),
          (50, 3821), (60, 3884), (70, 3945), (80, 4008), (90, 4086), (100, 4177)]
# The core's present mapping (bark_proto::node::battery_percent).
LOW_MV, HIGH_MV, FULL_MV = 3741, 3775, 4177
# A run that starts within this of a full cell and ends at or below the
# curve's last bend, 5 % (AN4189), looks like a whole discharge.
FULL_WITHIN_MV = 60
EMPTY_MV = 3675

# How long the console takes to print the log at most: 2048 lines paced
# at 3 ms (bark-fw/src/discharge.rs), with room.
READ_S = 30

LINE = re.compile(
    r"discharge seq=(\d+) utc_s=(\d+) uptime_s=(\d+) mv=(\d+) external=([01]) "
    r"sleepy=([01]) gps=([01]) why=(\w+)")
HEADER = re.compile(r"discharge slots=(\d+) every_s=(\d+) scale=(\w+)")
END = re.compile(r"discharge end samples=(\d+)")


def from_port(port: str) -> list[str]:
    """Ask the console for the log and return its lines."""
    fd = os.open(port, os.O_RDWR | os.O_NOCTTY)
    tty.setraw(fd)
    termios.tcflush(fd, termios.TCIOFLUSH)
    # A bare line end first closes whatever the tty echoed into the
    # console's line (bark-fw/README.md, "Log").
    os.write(fd, b"\r\ndischarge\r\n")
    lines, buf = [], b""
    deadline = time.monotonic() + READ_S
    while True:
        left = deadline - time.monotonic()
        if left <= 0 or not select.select([fd], [], [], left)[0]:
            sys.exit(f"no end of the log within {READ_S} s; is it a console image?")
        buf += os.read(fd, 4096)
        *done, buf = buf.split(b"\n")
        for line in done:
            text = line.decode(errors="replace").strip()
            lines.append(text)
            if END.search(text):
                os.close(fd)
                return lines


def parse(lines: list[str]) -> tuple[int | None, list[dict]]:
    """The calibration scale, if the header gave one, and the samples."""
    scale, samples = None, []
    for text in lines:
        if text.startswith(">"):
            continue
        if m := HEADER.search(text):
            scale = None if m.group(3) == "none" else int(m.group(3))
            samples = []  # a later printing replaces an earlier one
        elif m := LINE.search(text):
            seq, utc, up, mv, ext, sleepy, gps, why = m.groups()
            samples.append({"seq": int(seq), "utc_s": int(utc), "uptime_s": int(up), "mv": int(mv),
                            "external": ext == "1", "sleepy": sleepy == "1", "gps": gps == "1",
                            "why": why})
    return scale, samples


def between(a: dict, b: dict) -> float | None:
    """Seconds from sample a to the next, b, where they tell it, as the
    node's summary counts them (bark-image/src/discharge.rs)."""
    if b["why"] != "boot" and b["uptime_s"] >= a["uptime_s"]:
        return b["uptime_s"] - a["uptime_s"]
    if a["utc_s"] and b["utc_s"] >= a["utc_s"]:
        return b["utc_s"] - a["utc_s"]
    return None


def runs(samples: list[dict]) -> list[list[dict]]:
    """Runs on battery: consecutive samples off USB, each closed by the
    sample where USB came, whose reading is the last on battery."""
    out, run = [], []
    for s in samples:
        if not s["external"]:
            run.append(s)
            continue
        if run and s["why"] == "plugged" and s["mv"]:
            run.append(s)
        if len(run) >= 2:
            out.append(run)
        run = []
    if len(run) >= 2:
        out.append(run)
    return out


def current_ma(s: dict, a: argparse.Namespace) -> float:
    return (a.sleepy_ma if s["sleepy"] else a.relay_ma) + (a.gps_ma if s["gps"] else 0.0)


def mapping(mv: float) -> str:
    """The percent the core shows for mv (bark_proto::node::battery_percent)."""
    if mv <= LOW_MV:
        return "low"
    if mv < HIGH_MV:
        return str(round(20 + (mv - LOW_MV) * 10 / (HIGH_MV - LOW_MV)))
    return str(round(30 + (min(mv, FULL_MV) - HIGH_MV) * 70 / (FULL_MV - HIGH_MV)))


def interp(points: list[tuple[float, float]], x: float) -> float:
    """y at x on the straight lines through points, sorted by x."""
    for (x0, y0), (x1, y1) in zip(points, points[1:]):
        if x0 <= x <= x1:
            return y0 if x1 == x0 else y0 + (y1 - y0) * (x - x0) / (x1 - x0)
    return points[0][1] if x < points[0][0] else points[-1][1]


def median(xs: list[float]) -> float:
    s = sorted(xs)
    n = len(s)
    return s[n // 2] if n % 2 else (s[n // 2 - 1] + s[n // 2]) / 2


def fit(run: list[dict], following: dict | None, scale: int | None, a: argparse.Namespace) -> None:
    """Print the curve over one run, and how far it can be trusted."""
    k = (scale or SCALE_ONE) / SCALE_ONE
    used, gaps, by_mode, seconds = [0.0], 0, {}, 0.0
    for s, t in zip(run, run[1:]):
        dt = between(s, t)
        if dt is None:
            # A reset on battery with no time known: the uptime since it
            # is all that is known.
            dt, gaps = t["uptime_s"], gaps + 1
        seconds += dt
        q = current_ma(s, a) * dt / 3600
        mode = ("sleepy" if s["sleepy"] else "relay") + (" + gps" if s["gps"] else "")
        by_mode[mode] = by_mode.get(mode, 0.0) + q
        used.append(used[-1] + q)
    total = used[-1]
    if total <= 0:
        sys.exit("the run spans no time")
    hours = seconds / 3600
    # The fraction left at each sample, and its reading, calibrated; the
    # readings a median of five apart from the run's ends, against noise.
    left = [100 * (1 - q / total) for q in used]
    mv = [s["mv"] * k for s in run]
    smooth = [median(mv[max(0, i - 2):i + 3]) if 2 <= i < len(mv) - 2 else mv[i] for i in range(len(mv))]
    points = sorted(zip(left, smooth))

    first, last = mv[0], mv[-1]
    if run[-1]["why"] == "plugged":
        ended = "plugged in"
    elif following is not None and following["why"] == "boot":
        ended = "the node stopped"
    else:
        ended = "the log ends on battery"
    full = first >= FULL_MV - FULL_WITHIN_MV
    empty = last <= EMPTY_MV
    print(f"run: {len(run)} samples, seq {run[0]['seq']} to {run[-1]['seq']}, {hours:.1f} h "
          f"({hours / 24:.1f} days)")
    print(f"readings: {first:.0f} mV to {last:.0f} mV, "
          + (f"calibrated by {scale}/{SCALE_ONE}" if scale else "uncalibrated: no scale kept"))
    print("charge used: " + ", ".join(f"{m} {q:.0f} mAh" for m, q in by_mode.items())
          + f"; {total:.0f} mAh in all, {100 * total / CELL_MAH:.0f} % of the {CELL_MAH:.0f} mAh cell")
    print(f"whole discharge: started full {'yes' if full else 'no'}, ended empty "
          f"{'yes' if empty else 'no'}, ended as {ended}")
    if gaps:
        print(f"warning: {gaps} reset(s) on battery with no time known; the time before each is lost")
    if not (full and empty):
        print("warning: not a whole discharge; the fractions below are of this run's charge, not the cell's")
    print()
    print("percent  fitted_mv  an4189_mv  mapping_now")
    table = []
    for p in range(0, 101, 5):
        v = interp(points, p)
        table.append(round(v))
        print(f"{p:7d}  {v:9.0f}  {interp(AN4189, p):9.0f}  {mapping(v):>11}")
    print()
    print("as bark-proto would take it, mV at 0, 5, ..., 100 %:")
    print("[" + ", ".join(str(v) for v in table) + "]")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("source", help="a console image's serial port, or a transcript of `discharge`")
    ap.add_argument("--csv", help="write every sample here")
    ap.add_argument("--run", type=int, default=-1, help="which run on battery to fit, from 0; the last by default")
    ap.add_argument("--relay-ma", type=float, default=RELAY_MA)
    ap.add_argument("--sleepy-ma", type=float, default=SLEEPY_MA)
    ap.add_argument("--gps-ma", type=float, default=GPS_MA)
    a = ap.parse_args()

    if stat.S_ISCHR(os.stat(a.source).st_mode):
        lines = from_port(a.source)
    else:
        with open(a.source, errors="replace") as f:
            lines = f.read().splitlines()
    scale, samples = parse(lines)
    if not samples:
        sys.exit("no samples: the log is empty, or this is not a `discharge` printing")
    k = (scale or SCALE_ONE) / SCALE_ONE
    if a.csv:
        with open(a.csv, "w", newline="") as f:
            w = csv.writer(f)
            w.writerow(["seq", "utc_s", "uptime_s", "mv", "mv_calibrated", "external", "sleepy", "gps", "why"])
            for s in samples:
                w.writerow([s["seq"], s["utc_s"], s["uptime_s"], s["mv"], round(s["mv"] * k),
                            int(s["external"]), int(s["sleepy"]), int(s["gps"]), s["why"]])
        print(f"{len(samples)} samples written to {a.csv}")
    found = runs(samples)
    print(f"{len(samples)} samples, {len(found)} run(s) on battery")
    if not found:
        return 0
    run = found[a.run]
    end = samples.index(run[-1])
    following = samples[end + 1] if end + 1 < len(samples) else None
    fit(run, following, scale, a)
    return 0


if __name__ == "__main__":
    sys.exit(main())
