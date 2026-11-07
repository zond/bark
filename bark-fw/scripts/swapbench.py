#!/usr/bin/env python3
"""Resets in the middle of swaps on one board (roadmap Phase 2, "Resets in a swap", D143).

Python 3 and its `cryptography` package, to sign the images with the
bench key; run from bark-fw/, the board on USB:

    scripts/swapbench.py ID 200

ID is the board's node ID, twelve hex digits or its last four. The script
builds the bench loader (BARK_LOADER_BENCH=reset), which resets the chip
once in each swap and once in each swap back at a moment it draws, and a
console image of version 1; puts the board in its bootloader with the
1200-baud touch and copies the two to it. Then it runs the rounds, in
turn:

    up     a console image one version above the running one: the swap is
           cut once, the next boot ends it, and the new version confirms;
    panic  an image built with BARK_UPDATE_BENCH=panic, one version above:
           the swap is cut once, the image panics, the swap back is cut
           once, and the running version is back.

Each round stages its image with scripts/image.py and reboots, then reads
every boot that brings the port back, each with one open (README, "Log"),
until the console is ready. A round prints the time from `reboot` to the
port of its last boot, where each cut fell as the firmware logs it from
the record (`bench reset in the swap at ... us: external erase of page
113, 37.0 % through`, or `not reached`), and whether the version expected
runs. The first round that fails stops the run. Every line the board
sends goes to target/swapbench/<time>.log, after the host's clock.
"""

from __future__ import annotations

import argparse
import os
import re
import select
import subprocess
import sys
import termios
import time
import tty
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import bench  # noqa: E402  (beside this script: ports, the touch, the copy)
import image  # noqa: E402  (beside this script: pack, stage)

FW = bench.FW
LOADER = FW.parent / "bark-loader"
ELF = bench.ELF
LOADER_ELF = LOADER / "target/thumbv7em-none-eabihf/release/bark-loader"
OUT = FW / "target/swapbench"
PORT = "/dev/serial/by-id/usb-bark_bark_T-Echo_{}-if00"

# How long one round may take from `reboot`: two swaps of about 50 s
# (README, "Updates") and the boots between them, with room; the trial's
# watchdog, 600 s, would end a hang within it.
ROUND_S = 900.0
# From the port's appearance to `console ready`, past the radio's start
# and the trial's confirmation.
BOOT_S = 60.0
# The console's `reboot` drains the log, 32 ms, before it resets.
GONE_S = 15.0

BANNER = re.compile(r"image version (\d+), id=")
CONFIRMED = re.compile(r"update: version (\d+) confirmed")
BACK = re.compile(r"update: the new image did not confirm itself; version (\d+) is back")
CUT = re.compile(r"update: bench reset in the (swap|swap back) at (\d+) us: (.*)$")
READY = "console ready"


def build_image(kind: str, version: int) -> Path:
    """The console image of `kind`, up or panic, at `version`, packed."""
    out = OUT / f"{kind}-v{version}.img"
    if not out.exists():
        env = {"BARK_OPEN": "1", "BARK_IMAGE_VERSION": str(version)}
        if kind == "panic":
            env["BARK_UPDATE_BENCH"] = "panic"
        cargo(FW, env, ["--features", "console"])
        data, v = image.pack(ELF.read_bytes())
        assert v == version
        out.write_bytes(data)
    # Signed by the bench key, which an open build holds; one packed
    # before images were signed is signed now.
    if not any(key_id for key_id, _ in image.slots(out.read_bytes()[: image.HEADER_LEN])):
        image.sign(str(out), image.BENCH_KEY, image.bench_key_id(), None)
    return out


def cargo(where: Path, env: dict[str, str], args: list[str]) -> None:
    # As scripts/bench.py builds: no BARK_ variable from the shell.
    e = {k: v for k, v in os.environ.items() if not k.startswith("BARK_")}
    e.update(env)
    cmd = ["cargo", "build", "--release", *args]
    print(" ".join(f"{k}={v}" for k, v in env.items()), " ".join(cmd), f"({where.name})", flush=True)
    subprocess.run(cmd, cwd=where, env=e, check=True, stdout=subprocess.DEVNULL)


def setup(node: str, log) -> None:
    """The bench loader and a console image of version 1 on the board."""
    cargo(LOADER, {"BARK_OPEN": "1", "BARK_LOADER_BENCH": "reset"}, [])
    cargo(FW, {"BARK_OPEN": "1", "BARK_IMAGE_VERSION": "1"}, ["--features", "console"])
    uf2 = OUT / "base.uf2"
    subprocess.run([sys.executable, "-I", FW / "scripts/uf2.py", "--loader", LOADER_ELF, ELF, uf2], check=True)
    bench.off(argparse.Namespace(id=node))
    bench.on(argparse.Namespace(id=node, uf2=str(uf2)))
    sessions = boots(node, time.monotonic() + ROUND_S, log, None)
    if not sessions or banner(sessions[-1]) != 1:
        sys.exit("setup: the board did not come back running version 1")


def boots(node: str, deadline: float, log, expect_gone: float | None) -> list[dict]:
    """Each boot that brings the port back until one reaches the console,
    as {"t": when its port appeared, "lines": what it sent}. With
    `expect_gone`, wait that long for the old port to go first."""
    port = PORT.format(node)
    if expect_gone is not None:
        gone = time.monotonic() + expect_gone
        while os.path.exists(port):
            if time.monotonic() > gone:
                sys.exit("the port did not go after `reboot`")
            time.sleep(0.02)
    sessions = []
    while time.monotonic() < deadline:
        if not os.path.exists(port):
            time.sleep(0.02)
            continue
        t = time.monotonic()
        lines = read_boot(port, log)
        sessions.append({"t": t, "lines": lines})
        if any(READY in x for x in lines):
            return sessions
        while os.path.exists(port) and time.monotonic() < deadline:
            time.sleep(0.02)
    return sessions


def read_boot(port: str, log) -> list[str]:
    """One boot's lines from one open, until the console is ready, the
    port goes, or BOOT_S passes."""
    for _ in range(100):
        try:
            fd = os.open(port, os.O_RDONLY | os.O_NOCTTY)
            break
        except OSError:
            time.sleep(0.05)
    else:
        return []
    lines: list[str] = []
    rest = b""
    end = time.monotonic() + BOOT_S
    try:
        tty.setraw(fd, termios.TCSANOW)
        while time.monotonic() < end:
            if not select.select([fd], [], [], 0.5)[0]:
                continue
            chunk = os.read(fd, 4096)
            if not chunk:
                break
            rest += chunk
            *got, rest = rest.split(b"\n")
            for raw in got:
                text = raw.rstrip(b"\r").decode("utf-8", "replace")
                log.write(f"{time.time():.3f} {text}\n")
                lines.append(text)
                if READY in text:
                    return lines
    except OSError:
        pass
    finally:
        os.close(fd)
    return lines


def banner(session: dict) -> int | None:
    for x in session["lines"]:
        m = BANNER.search(x)
        if m:
            return int(m.group(1))
    return None


def cuts(lines: list[str]) -> dict[str, str]:
    """Where each phase's reset fell, from the last boot's record."""
    out = {}
    for x in lines:
        m = CUT.search(x)
        if m:
            phase, _, fell = m.groups()
            fell = re.sub(r" of page (\d+), ([\d.]+) % through", r" p\1 \2 %", fell)
            out["back" if phase == "swap back" else "swap"] = fell
    return out


def round_(i: int, node: str, running: int, log) -> tuple[bool, int, dict]:
    """Round `i`, 0 first, on a board running `running`: whether it went
    as expected, the version running after it, and what it printed."""
    kind = "up" if i % 2 == 0 else "panic"
    version = running + 1
    img = build_image(kind, version)
    log.write(f"# round {i + 1} {kind} v{version}\n")
    image.stage(str(img), PORT.format(node), True)
    t0 = time.monotonic()
    sessions = boots(node, t0 + ROUND_S, log, GONE_S)
    row = {"round": i + 1, "kind": kind, "version": version}
    if not sessions or not any(READY in x for x in sessions[-1]["lines"]):
        row["why"] = f"no console within {ROUND_S:.0f} s of `reboot`"
        return False, running, row
    last = sessions[-1]
    row["port_s"] = last["t"] - t0
    row["boots"] = len(sessions)
    row["cuts"] = cuts(last["lines"])
    runs = banner(last)
    text = "\n".join(last["lines"])
    if kind == "up":
        ok = runs == version and f"update: version {version} confirmed" in text
        want = ["swap"]
        after = version if ok else running
    else:
        ok = runs == running and not CONFIRMED.search(text)
        want = ["swap", "back"]
        after = running
        if ok and not BACK.search(text):
            # A cut in the swap back's last word leaves the state page
            # as with no swap asked for: the old image boots plainly.
            row["note"] = "back without its line"
    if not ok:
        row["why"] = f"version {runs} runs"
    missing = [p for p in want if p not in row["cuts"]]
    if ok and missing:
        ok = False
        row["why"] = f"no bench reset noted for the {' and '.join(missing)}: is the bench loader on the board?"
    return ok, after, row


def show(row: dict, ok: bool) -> str:
    head = f"{row['round']:4} {row['kind']:<5} v{row['version']:<4}"
    if "port_s" not in row:
        return f"{head} FAIL: {row['why']}"
    c = row["cuts"]
    where = "; ".join(f"{p}: {c[p]}" for p in ("swap", "back") if p in c) or "no reset noted"
    verdict = "ok" if ok else f"FAIL: {row['why']}"
    if ok and row.get("note"):
        verdict += f" ({row['note']})"
    return f"{head} {row['port_s']:6.1f} s  {row['boots']} boots  {where}  {verdict}"


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("id", help="the board's node ID, twelve hex digits or its last four")
    ap.add_argument("rounds", type=int, help="rounds, up and panic in turn: 200 for a hundred of each")
    a = ap.parse_args()
    node = bench.full_id(a.id)
    OUT.mkdir(parents=True, exist_ok=True)
    for old in OUT.glob("*.img"):
        old.unlink()
    path = OUT / time.strftime("%Y-%m-%d-%H%M%S.log")
    print(f"log: {path}", flush=True)
    with open(path, "a", buffering=1) as log:
        setup(node, log)
        running = 1
        rows = []
        for i in range(a.rounds):
            ok, running, row = round_(i, node, running, log)
            rows.append(row)
            print(show(row, ok), flush=True)
            log.write(f"# {show(row, ok)}\n")
            if not ok:
                break
    done = [r for r in rows if "port_s" in r]
    for kind in ("up", "panic"):
        rs = [r for r in done if r["kind"] == kind]
        if not rs:
            continue
        fired = sum(1 for r in rs for p, f in r["cuts"].items() if f != "not reached")
        noted = sum(len(r["cuts"]) for r in rs)
        times = sorted(r["port_s"] for r in rs)
        print(
            f"{kind}: {len(rs)} rounds, {fired} of {noted} resets fired, "
            f"reboot to port {times[0]:.1f} / {times[len(times) // 2]:.1f} / {times[-1]:.1f} s (min / median / max)"
        )
    if len(rows) < a.rounds or any("why" in r for r in rows):
        sys.exit(1)


if __name__ == "__main__":
    main()
