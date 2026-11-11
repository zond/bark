#!/usr/bin/env python3
"""Read a node's discharge log and fit its cell's voltage curve (roadmap D178).

    scripts/discharge.py /dev/ttyACM0 --csv run.csv     # a console image's port
    scripts/discharge.py transcript.txt --csv run.csv   # or a saved transcript
    scripts/discharge.py run.csv                        # or a CSV it wrote

The node image keeps a sample of its cell an hour apart on battery, and
one at each boot and change of power or mode, on the external flash
(bark-fw/README.md, "Discharge log"). A discharge bench build (roadmap
D190) samples every ten minutes and marks its samples `bench=1`: a relay
at any battery level with its GPS tracking, one mode from full to flat
in about a day. A console image prints them with
its `discharge` command, a line each, oldest first; given a port, this
script sends the command and reads the answer, given a file, it reads
the lines from it. It writes every sample to the CSV, if asked, and
fits the curve over the last discharge: the last run of samples on
battery, from the one where USB went, or the boot on battery, to the
last reading before USB came back or the node stopped.

What it trusts: each sample's line ends in `crc=`, the CRC-16 of the
text before it (`bark_image::discharge::Line`); a line whose check
fails or is missing, cut or spliced on its way, is rejected and counted.
A transcript from before the check, with none on any line, is read
unchecked, with a warning. It says where sequence numbers are missing, whether the end
line's count matches, and where a run's samples are further apart than
their interval and a minute, the driver reading the battery once a
minute.

What it assumes:

- The current is constant in each mode, as roadmap D169 seeds the
  battery estimate: a relay 6.41 mA, a sleepy node 0.59 mA, and 29 mA
  more while the GPS runs (`bark_proto::drain`; spec "Sleepy nodes"); a
  bench sample 35.4 mA, a relay's and the GPS's (`--bench-ma`).
  The microcontroller and the display are not in them. A run in one mode
  does not depend on its current at all, as the fraction of the charge
  left is a ratio of times; across modes, a relay that turns sleepy at
  its low level, the ratio of the currents places the turn. `--relay-ma`,
  `--sleepy-ma` and `--gps-ma` take better ones, such as the drains the
  node learned (`status`, `drain_relay` and `drain_sleepy`, in
  ten-thousandths of a percent an hour of the 767 mAh the core counts).
  A bench run is one mode, so its fit is independent of the current
  estimates.
- The run starts full and ends empty: the fraction left at a sample is
  one less the charge used up to it over the charge used in the whole
  run. The summary says whether the run looks it: a first reading near
  a full cell's and a last below the curve's 5 %, the node stopping
  rather than being plugged in. The charge drawn after the last sample,
  up to an interval's, is not counted.
- Readings are calibrated by the scale the node kept (`scale=` in the
  header), as the core reads them; without one they are as read, up to
  3 % off. The load's drop across the cell's resistance and the cell's
  temperature are in them (docs/discharge-2026-11-11.md).

It fits charge left = a + b u + c u², u the volts above 3.300 V, by least
squares over the run's samples at 3 % and above, where the voltage
falls off its last bend faster than a quadratic follows, and prints a,
b and c, as `bark_proto::node`'s curve takes them, and the fit's errors
and the core's curve's against the run. It prints the voltage at every
5 % of the charge from 0 to 100 beside the core's curve's. The script
changes no code.

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
# A discharge bench build's one mode: a relay with its GPS tracking.
BENCH_MA = RELAY_MA + GPS_MA
# The cell's nominal charge, mAh (LILYGO T-Echo README).
CELL_MAH = 850.0
# One, in the units of the node's calibration (bark_proto::BATTERY_SCALE_ONE).
SCALE_ONE = 1 << 15

# The core's curve (bark_proto::node, CURVE): charge left, percent, at u
# volts above CURVE_FROM_V, a + b u + c u².
CURVE = (5.0, 60.0, 66.0)
CURVE_FROM_V = 3.3
# The fit's samples: those with at least this much charge left, percent.
FIT_FROM_PERCENT = 3.0
# A run that starts within this of a full cell and ends at or below the
# curve's 5 % looks like a whole discharge.
FULL_WITHIN_MV = 60

# How long the console takes to print the log at most: 2048 lines, each
# as soon as the log has room (bark-fw/src/discharge.rs), with room.
READ_S = 30

LINE = re.compile(
    r"(discharge seq=(\d+) utc_s=(\d+) uptime_s=(\d+) mv=(\d+) external=([01]) "
    r"sleepy=([01]) gps=([01]) bench=([01]) why=(\w+))(?: crc=([0-9a-f]{4}))?$")
HEADER = re.compile(r"discharge slots=(\d+) every_s=(\d+) bench_every_s=(\d+) scale=(\w+)")
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


def crc16(data: bytes) -> int:
    """CRC-16/CCITT from 0xffff, as bark_image::discharge checks a line."""
    c = 0xFFFF
    for x in data:
        c ^= x << 8
        for _ in range(8):
            c = ((c << 1) ^ 0x1021) & 0xFFFF if c & 0x8000 else (c << 1) & 0xFFFF
    return c


# The line bark_image::discharge's test prints, and its check.
assert crc16(b"discharge seq=49 utc_s=1794322819 uptime_s=1204 mv=4153 external=0 sleepy=0 gps=1 "
             b"bench=1 why=unplugged") == 0x57CF


def sample(seq, utc, up, mv, ext, sleepy, gps, bench, why) -> dict:
    return {"seq": int(seq), "utc_s": int(utc), "uptime_s": int(up), "mv": int(mv),
            "external": ext == "1", "sleepy": sleepy == "1", "gps": gps == "1",
            "bench": bench == "1", "why": why}


def parse(lines: list[str]) -> tuple[int | None, dict, list[dict], dict]:
    """The calibration scale, if the header gave one, the intervals
    between samples, a node image's and a bench build's, the samples, and
    what the reading found: lines rejected, lines unchecked, the count the
    end line gave."""
    scale, every, samples = None, {False: 3600, True: 600}, []
    found = {"rejected": 0, "unchecked": 0, "end": None}
    # Where any line has a check, one without was cut.
    checked = any(" crc=" in text for text in lines)
    for text in lines:
        if text.startswith(">"):
            continue
        if m := HEADER.search(text):
            scale = None if m.group(4) == "none" else int(m.group(4))
            every = {False: int(m.group(2)), True: int(m.group(3))}
            samples = []  # a later printing replaces an earlier one
            found = {"rejected": 0, "unchecked": 0, "end": None}
        elif m := END.search(text):
            found["end"] = int(m.group(1))
        elif "discharge seq=" in text:
            m = LINE.search(text)
            if m is None or (checked and m.group(11) is None) or (
                    m.group(11) and crc16(m.group(1).encode()) != int(m.group(11), 16)):
                found["rejected"] += 1
                continue
            found["unchecked"] += m.group(11) is None
            samples.append(sample(*m.groups()[1:10]))
    return scale, every, samples, found


def from_csv(path: str) -> tuple[int | None, list[dict]]:
    """The samples of a CSV this script wrote, and the scale its
    calibrated column shows."""
    with open(path, newline="") as f:
        rows = list(csv.DictReader(f))
    samples = [sample(r["seq"], r["utc_s"], r["uptime_s"], r["mv"], r["external"], r["sleepy"],
                      r["gps"], r["bench"], r["why"]) for r in rows]
    top = max(rows, key=lambda r: int(r["mv"]), default=None)
    scale = None
    if top and int(top["mv"]) and int(top["mv_calibrated"]) != int(top["mv"]):
        scale = round(SCALE_ONE * int(top["mv_calibrated"]) / int(top["mv"]))
    return scale, samples


def check(samples: list[dict], found: dict) -> None:
    """Say what of the log is missing or cannot be trusted."""
    if found["rejected"]:
        print(f"warning: {found['rejected']} sample line(s) rejected: their check failed, "
              "cut or spliced on the way")
    if found["unchecked"]:
        print(f"warning: {found['unchecked']} sample line(s) carry no check (an older console); "
              "read unchecked")
    if found["end"] is not None and found["end"] != len(samples) + found["rejected"]:
        print(f"warning: the end line counts {found['end']} samples, {len(samples)} read "
              f"and {found['rejected']} rejected")
    gaps = [(a["seq"], b["seq"]) for a, b in zip(samples, samples[1:]) if b["seq"] != a["seq"] + 1]
    for a, b in gaps:
        print(f"warning: samples missing between seq {a} and {b}")
    if not gaps and samples:
        print(f"sequence complete: seq {samples[0]['seq']} to {samples[-1]['seq']}")


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
    if s["bench"]:
        return a.bench_ma
    return (a.sleepy_ma if s["sleepy"] else a.relay_ma) + (a.gps_ma if s["gps"] else 0.0)


def core_percent(mv: float) -> float:
    """The charge left the core's curve gives a calibrated reading,
    percent, unrounded (bark_proto::node::cell_percent)."""
    a, b, c = CURVE
    u = mv / 1000 - CURVE_FROM_V
    if b + 2 * c * u <= 0:
        return 0.0
    return min(100.0, max(0.0, a + b * u + c * u * u))


def core_mv(percent: float) -> float:
    """The reading at which the core's curve gives percent
    (bark_proto::node::cell_mv), unrounded."""
    a, b, c = CURVE
    return 1000 * (CURVE_FROM_V + (-b + (b * b + 4 * c * (percent - a)) ** 0.5) / (2 * c))


def quadratic(points: list[tuple[float, float]]) -> tuple[float, float, float]:
    """Least squares of percent = a + b u + c u² over (u, percent)."""
    m = [[sum(u ** (j + k) for u, _ in points) for k in range(3)] for j in range(3)]
    v = [sum(p * u ** j for u, p in points) for j in range(3)]
    for col in range(3):
        pivot = max(range(col, 3), key=lambda r: abs(m[r][col]))
        m[col], m[pivot], v[col], v[pivot] = m[pivot], m[col], v[pivot], v[col]
        for r in range(3):
            if r != col:
                f = m[r][col] / m[col][col]
                m[r] = [x - f * y for x, y in zip(m[r], m[col])]
                v[r] -= f * v[col]
    return tuple(v[j] / m[j][j] for j in range(3))


def errors(f, left: list[float], mv: list[float]) -> tuple[float, float]:
    """The rms and the largest of f(mv) less the charge left, percent."""
    e = [f(v) - p for p, v in zip(left, mv)]
    return (sum(x * x for x in e) / len(e)) ** 0.5, max(abs(x) for x in e)


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


def fit(run: list[dict], following: dict | None, scale: int | None, every: dict,
        a: argparse.Namespace) -> None:
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
        mode = "bench" if s["bench"] else ("sleepy" if s["sleepy"] else "relay") + (" + gps" if s["gps"] else "")
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
    late = [(s, t, between(s, t)) for s, t in zip(run, run[1:])
            if between(s, t) is not None and between(s, t) > every[s["bench"]] + 60]
    for s, t, dt in late:
        print(f"warning: {dt} s from seq {s['seq']} to {t['seq']}, more than the interval and a "
              "minute: samples missing")
    if not late:
        print(f"evenly spaced: no two samples more than {every[run[0]['bench']]} s and a minute apart")
    if run[-1]["why"] == "plugged":
        ended = "plugged in"
    elif following is not None and following["why"] == "boot":
        ended = "the node stopped"
    else:
        ended = "the log ends on battery"
    full = first >= core_mv(100) - FULL_WITHIN_MV
    empty = last <= core_mv(5)
    bench = all(s["bench"] for s in run)
    print(f"run: {len(run)} samples, seq {run[0]['seq']} to {run[-1]['seq']}, {hours:.1f} h "
          f"({hours / 24:.1f} days), a sample every {every[bench]} s"
          + (f"; a discharge bench run, one mode at {a.bench_ma:.1f} mA, so the fit does not "
             "depend on the current" if bench else ""))
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
    if bench:
        print("note: readings under the bench's load, some 10 to 20 mV below a relay's "
              "(docs/discharge-2026-11-11.md); the last readings, before the board stopped, mark its true end")
    elif any(s["bench"] for s in run):
        print("warning: bench and node samples in one run; the fit mixes their currents")
    print()
    # The fit, over the samples above the last bend, by their raw
    # readings; its errors and the core's curve's over every sample.
    fit_points = [(v / 1000 - CURVE_FROM_V, p) for p, v in zip(left, mv) if p >= FIT_FROM_PERCENT]
    a_, b_, c_ = quadratic(fit_points)
    fitted = lambda v: min(100.0, max(0.0, a_ + b_ * (v / 1000 - CURVE_FROM_V)
                                      + c_ * (v / 1000 - CURVE_FROM_V) ** 2))
    above = [(p, v) for p, v in zip(left, mv) if p >= FIT_FROM_PERCENT]
    print(f"fit over {len(fit_points)} samples at {FIT_FROM_PERCENT:.0f} % and above: percent = "
          f"{a_:.2f} + {b_:.2f} u + {c_:.2f} u², u the volts above {CURVE_FROM_V:.3f} V")
    for name, f in (("fit", fitted), ("core's curve " + "/".join(f"{x:g}" for x in CURVE), core_percent),
                    ("core's, whole percent", lambda v: round(core_percent(v)))):
        rms, top = errors(f, left, mv)
        rms3, top3 = errors(f, *zip(*above))
        print(f"{name}: rms {rms:.2f} %, largest {top:.2f} % over every sample; "
              f"{rms3:.2f} % and {top3:.2f} % at {FIT_FROM_PERCENT:.0f} % and above")
    print()
    print("percent  measured_mv  core_mv  core_percent_of_measured")
    for p in range(0, 101, 5):
        v = interp(points, p)
        print(f"{p:7d}  {v:11.0f}  {core_mv(p):7.0f}  {core_percent(v):24.0f}")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("source", help="a console image's serial port, or a transcript of `discharge`")
    ap.add_argument("--csv", help="write every sample here")
    ap.add_argument("--run", type=int, default=-1, help="which run on battery to fit, from 0; the last by default")
    ap.add_argument("--relay-ma", type=float, default=RELAY_MA)
    ap.add_argument("--sleepy-ma", type=float, default=SLEEPY_MA)
    ap.add_argument("--gps-ma", type=float, default=GPS_MA)
    ap.add_argument("--bench-ma", type=float, default=BENCH_MA, help="a discharge bench sample's current")
    a = ap.parse_args()

    if stat.S_ISCHR(os.stat(a.source).st_mode):
        lines = from_port(a.source)
    else:
        with open(a.source, errors="replace") as f:
            lines = f.read().splitlines()
    if lines and lines[0].startswith("seq,"):
        scale, samples = from_csv(a.source)
        every, found = {False: 3600, True: 600}, {"rejected": 0, "unchecked": 0, "end": None}
    else:
        scale, every, samples, found = parse(lines)
    check(samples, found)
    if not samples:
        sys.exit("no samples: the log is empty, or this is not a `discharge` printing")
    k = (scale or SCALE_ONE) / SCALE_ONE
    if a.csv:
        with open(a.csv, "w", newline="") as f:
            w = csv.writer(f)
            w.writerow(["seq", "utc_s", "uptime_s", "mv", "mv_calibrated", "external", "sleepy", "gps",
                        "bench", "why"])
            for s in samples:
                w.writerow([s["seq"], s["utc_s"], s["uptime_s"], s["mv"], round(s["mv"] * k),
                            int(s["external"]), int(s["sleepy"]), int(s["gps"]), int(s["bench"]), s["why"]])
        print(f"{len(samples)} samples written to {a.csv}")
    found = runs(samples)
    print(f"{len(samples)} samples, {len(found)} run(s) on battery")
    if not found:
        return 0
    run = found[a.run]
    end = samples.index(run[-1])
    following = samples[end + 1] if end + 1 < len(samples) else None
    fit(run, following, scale, every, a)
    return 0


if __name__ == "__main__":
    sys.exit(main())
