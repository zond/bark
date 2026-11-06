#!/usr/bin/env python3
"""Bring-up steps 4 and 5 on the bench: builds, logs, and what the logs say.

No dependencies beyond Python 3. The procedure is docs/bringup-2026-11-06.md;
run from bark-fw/.

    scripts/bench.py build line A B C      # step 4: three relays in a line
    scripts/bench.py build sleepy R S      # step 5: a relay, a sleepy node, a console
    scripts/bench.py capture DIR           # log every board, host time on each line
    scripts/bench.py say ID 'preamble long'  # type a console command into a board
    scripts/bench.py off ID                # the board into its bootloader: silent
    scripts/bench.py on ID UF2             # copy a UF2 to the board's bootloader drive
    scripts/bench.py texts DIR             # each text's path and timing
    scripts/bench.py catch DIR S K         # long-preamble frames S caught of K's
    scripts/bench.py watch DIR S           # S's anchor and its watches

ID is a board's node ID: twelve hex digits for build and on, and for the
other commands its last four as well.
"""

from __future__ import annotations

import argparse
import glob
import math
import os
import re
import shutil
import subprocess
import sys
import termios
import threading
import time
import tty
from dataclasses import dataclass, field
from pathlib import Path

FW = Path(__file__).resolve().parent.parent
ELF = FW / "target/thumbv7em-none-eabihf/release/bark-fw"
OUT = FW / "target/bench"
PORTS = "/dev/serial/by-id/usb-bark_bark_T-Echo_*-if00"

# The places of scenarios/line.toml: 400 m apart east of its origin, on the
# simulator's projection (bark-sim/src/geo.rs). Far from any bench, so that
# a true fix there is beyond every relay a board knows and is dropped
# (bark-fw/README.md).
ORIGIN = (50.45, 30.52)
EARTH_RADIUS_M = 6_371_000.0


def place(x_m: float) -> tuple[str, str]:
    lat, lon = ORIGIN
    lon += math.degrees(x_m / (EARTH_RADIUS_M * math.cos(math.radians(lat))))
    return f"{lat:.6f}", f"{lon:.6f}"


# ---- boards and ports --------------------------------------------------


def ports() -> dict[str, str]:
    """Node ID to serial port, for every bark board on the host."""
    out = {}
    for p in glob.glob(PORTS):
        m = re.search(r"T-Echo_([0-9a-f]{12})-if00$", p)
        if m:
            out[m.group(1)] = p
    return out


def full_id(short: str, known: list[str] | None = None) -> str:
    """A board's twelve-digit ID from its twelve or last four digits."""
    short = short.lower()
    if re.fullmatch(r"[0-9a-f]{12}", short):
        return short
    found = [i for i in (known if known is not None else ports()) if i.endswith(short)]
    if len(found) != 1:
        sys.exit(f"{short}: {len(found)} boards end so; give all twelve digits")
    return found[0]


# ---- build ---------------------------------------------------------------


def cargo(env: dict[str, str], console: bool, out: Path) -> None:
    e = {k: v for k, v in os.environ.items() if not k.startswith("BARK_")}
    e.update(env)
    cmd = ["cargo", "build", "--release"] + (["--features", "console"] if console else [])
    print(" ".join(f"{k}={v}" for k, v in env.items()), " ".join(cmd), flush=True)
    subprocess.run(cmd, cwd=FW, env=e, check=True)
    subprocess.run([sys.executable, FW / "scripts/uf2.py", ELF, out], cwd=FW, check=True)
    size = subprocess.run(["size", ELF], capture_output=True, text=True)
    text, data = size.stdout.splitlines()[-1].split()[:2]
    print(f"{out.name}: image {int(text) + int(data)} B of the 401 408 B slot\n", flush=True)


def build(a: argparse.Namespace) -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    ids = [full_id(i, []) if len(i) == 12 else i for i in a.ids]
    if any(len(i) != 12 for i in ids):
        sys.exit("build: give each board's twelve hex digits (its banner's id=)")
    if a.what == "line":
        if len(ids) != 3:
            sys.exit("build line: A B C")
        ends = {0: ids[2], 2: ids[0]}
        for k, (name, i) in enumerate(zip("ABC", ids)):
            lat, lon = place(400.0 * k)
            env = {"BARK_LAT": lat, "BARK_LON": lon}
            if k in ends:
                env["BARK_IGNORE"] = ends[k]
            cargo(env, False, OUT / f"line-{name}-{i}.uf2")
    else:
        if len(ids) != 2:
            sys.exit("build sleepy: R S")
        lat, lon = place(0.0)
        cargo({"BARK_LAT": lat, "BARK_LON": lon}, False, OUT / f"sleepy-R-{ids[0]}.uf2")
        lat, lon = place(400.0)
        env = {"BARK_LAT": lat, "BARK_LON": lon, "BARK_ON_BATTERY": "1"}
        cargo(env, False, OUT / f"sleepy-S-{ids[1]}.uf2")
        cargo({}, True, OUT / "console.uf2")


# ---- the boards on the host ----------------------------------------------


def capture(a: argparse.Namespace) -> None:
    """Every board's log into DIR/<id>.log, each line after the host's clock
    in seconds. A board that leaves (its bootloader, a reset) is logged
    again when it comes back."""
    out = Path(a.dir)
    out.mkdir(parents=True, exist_ok=True)
    running: dict[str, threading.Thread] = {}

    def follow(node: str, port: str) -> None:
        with open(out / f"{node}.log", "a", buffering=1) as log:
            try:
                fd = os.open(port, os.O_RDONLY | os.O_NOCTTY)
            except OSError as e:
                print(f"{node}: {e}", file=sys.stderr)
                return
            try:
                # Raw: no echo back into the board, lines split here.
                tty.setraw(fd, termios.TCSANOW)
                rest = b""
                while True:
                    chunk = os.read(fd, 4096)
                    if not chunk:
                        break
                    now = time.time()
                    rest += chunk
                    *lines, rest = rest.split(b"\n")
                    for line in lines:
                        text = line.rstrip(b"\r").decode("utf-8", "replace")
                        log.write(f"{now:.3f} {text}\n")
            except OSError:
                pass
            finally:
                os.close(fd)
        print(f"{time.strftime('%H:%M:%S')} {node[-4:]} gone", flush=True)

    print(f"logging to {out}/, ^C to stop", flush=True)
    while True:
        for node, port in ports().items():
            t = running.get(node)
            if t is None or not t.is_alive():
                print(f"{time.strftime('%H:%M:%S')} {node[-4:]} on {port}", flush=True)
                t = threading.Thread(target=follow, args=(node, port), daemon=True)
                t.start()
                running[node] = t
        time.sleep(0.5)


def say(a: argparse.Namespace) -> None:
    fd = os.open(ports()[full_id(a.id)], os.O_WRONLY | os.O_NOCTTY)
    try:
        os.write(fd, a.command.encode() + b"\r\n")
    finally:
        os.close(fd)


def off(a: argparse.Namespace) -> None:
    """The 1200-baud touch (bark-fw/README.md, Flash): the board resets into
    its bootloader and its radio is silent until a UF2 is copied."""
    node = full_id(a.id)
    subprocess.run(["stty", "-F", ports()[node], "1200"], check=True)
    print(f"{node[-4:]}: into its bootloader")


def mounts() -> dict[str, Path]:
    """Mounted TECHOBOOT drives by their block device."""
    out = {}
    with open("/proc/mounts") as m:
        for line in m:
            dev, target = line.split()[:2]
            target = target.replace("\\040", " ")
            if Path(target).name.startswith("TECHOBOOT"):
                out[os.path.realpath(dev)] = Path(target)
    return out


def drive(node: str) -> Path | None:
    """The bootloader drive of the board `node`: the one whose USB serial,
    the chip's device ID, ends in the node ID (bark-fw/src/main.rs,
    `node_id`), else the only one there is."""
    by_id = glob.glob("/dev/disk/by-id/usb-*")
    devs = sorted({os.path.realpath(p) for p in by_id if node.upper() in p.upper()})
    mounted = mounts()
    for dev in devs:
        if dev in mounted:
            return mounted[dev]
    for dev in devs:
        subprocess.run(["udisksctl", "mount", "-b", dev], capture_output=True)
        if dev in mounts():
            return mounts()[dev]
    if not devs and len(mounted) == 1:
        return next(iter(mounted.values()))
    return None


def on(a: argparse.Namespace) -> None:
    """Copy a UF2 to the board's bootloader drive; the board boots it."""
    node = full_id(a.id, [])
    for _ in range(40):
        d = drive(node)
        if d is not None:
            break
        time.sleep(0.5)
    else:
        sys.exit(f"{node[-4:]}: no TECHOBOOT drive of its own: double-tap its reset button")
    shutil.copyfile(a.uf2, d / Path(a.uf2).name)
    os.sync()
    print(f"{node[-4:]}: {Path(a.uf2).name} copied to {d}")


# ---- reading the logs ----------------------------------------------------


@dataclass
class Line:
    t: float  # the host's clock, else the board's uptime
    node: str
    text: str


@dataclass
class Logs:
    lines: list[Line] = field(default_factory=list)
    host_time: bool = True

    def of(self, node: str) -> list[Line]:
        return [x for x in self.lines if x.node == node]


HOST = re.compile(r"^(\d+\.\d+) (.*)$")
BOARD = re.compile(r"^\[\s*(\d+\.\d+)\s+(\w+)\s*\]\s?(.*)$")


def read(d: str) -> Logs:
    logs = Logs()
    for f in sorted(Path(d).glob("*.log")):
        node = f.stem
        for raw in f.read_text(errors="replace").splitlines():
            h = HOST.match(raw)
            t = float(h.group(1)) if h else None
            body = h.group(2) if h else raw
            b = BOARD.match(body)
            if not b:
                continue
            if t is None:
                logs.host_time = False
                t = float(b.group(1))
            logs.lines.append(Line(t, node, b.group(3)))
    logs.lines.sort(key=lambda x: x.t)
    if not logs.lines:
        sys.exit(f"{d}: no log lines")
    if not logs.host_time:
        print("note: lines without the host's time; the times are each board's own uptime\n")
    return logs


def node_hex(s: str) -> str:
    """`NodeId([186, 230, ...])` or `[ba, e6, ...]` as twelve hex digits."""
    m = re.search(r"\[([0-9a-fx, ]+)\]", s)
    if not m:
        return "?"
    parts = [p.strip() for p in m.group(1).split(",")]
    hexed = all(re.fullmatch(r"[0-9a-f]{2}", p) for p in parts) and "NodeId" not in s
    return "".join(p if hexed else f"{int(p):02x}" for p in parts)


def msg_hex(s: str) -> str | None:
    m = re.search(r"MsgId\((\d+)\)", s)
    return f"{int(m.group(1)):08x}" if m else None


def name_of(names: dict[str, str], node: str) -> str:
    return next((v for k, v in names.items() if node.endswith(k)), node[-4:])


def names_arg(a: argparse.Namespace) -> dict[str, str]:
    """`--name A=f4f5`: the label A for the board whose ID ends in f4f5."""
    return dict(reversed(pair.split("=", 1)) for pair in a.name or [])


TRACE = re.compile(r"^(\w+) \{(.*)\}$")
NODE_FIELD = re.compile(r"(\w+): NodeId\(\[[\d, ]+\]\)")


def texts(a: argparse.Namespace) -> None:
    """Each text's path: where it was taken from a phone, each hand-over,
    return and drop, and where it reached an application, in the host's
    time from the take. Then the events that name no text: demotions, a
    turn to carried, the role."""
    logs = read(a.dir)
    names = names_arg(a)
    n = lambda node: name_of(names, node)  # noqa: E731
    events: dict[str, list[tuple[float, str, str]]] = {}
    other: list[tuple[float, str, str]] = []
    for x in logs.lines:
        m = re.match(r"ble: send id=([0-9a-f]{8}) len=(\d+) taken", x.text)
        if m:
            events.setdefault(m.group(1), []).append((x.t, x.node, f"taken from the phone, {m.group(2)} B"))
            continue
        m = re.match(r"message id=([0-9a-f]{8})", x.text)
        if m:
            events.setdefault(m.group(1), []).append((x.t, x.node, "for this node's application"))
            continue
        m = TRACE.match(x.text)
        if not m:
            continue
        kind, body = m.groups()
        fields = dict(re.findall(r"(\w+): (-?\w+)(?=\s*(?:,|$))", body))
        for f, rest in re.findall(r"(\w+): (NodeId\(\[[\d, ]+\]\))", body):
            fields[f] = n(node_hex(rest))
        msg = msg_hex(body)
        told = {
            "Forwarded": lambda: f"forwarded to {fields['to']}, hops {fields['hops']}, attempt {fields['attempts']}",
            "ReturnedToParent": lambda: f"returned to its parent {fields['to']}, attempt {fields['attempts']}",
            "HandOverFailed": lambda: f"hand-over to {fields['to']} unanswered, moves on {fields['moves_on']}",
            "ChannelBusy": lambda: f"channel to {fields['to']} busy",
            "Copy": lambda: "a copy came back" if fields.get("returned") == "true" else f"a copy: {body.strip()}",
            "Seen": lambda: f"{fields['by']} answered seen",
            "TurnedAway": lambda: f"refused by {fields['by']}",
            "Dropped": lambda: f"dropped, {fields['reason']}",
            "Refused": lambda: f"refused here, {fields['reason']}",
            "Queued": lambda: f"queued for {fields['node']}",
            "Delivered": lambda: f"delivered to {fields['node']} by {fields['by']}",
            "DeliveryWaits": lambda: f"waits for {fields['node']}'s beat, tried {fields['tried']}",
            "HandOverDeferred": lambda: "held by the duty-cycle budget",
            "Silent": lambda: "not answered",
        }.get(kind)
        if msg and told:
            events.setdefault(msg, []).append((x.t, x.node, told()))
        elif kind in ("Demoted", "Carried", "Role"):
            named = NODE_FIELD.sub(lambda f: f"{f.group(1)}: {n(node_hex(f.group(0)))}", body)
            other.append((x.t, x.node, f"{kind} {{{named}}}"))
    # Lines of their own: the traces without fields, and the boot's.
    alone = ("FarFix", "AnchorCheck", "AnchorCarried", "GraceEnded", "BatteryLow", "bench build", "node id=")
    other += [(x.t, x.node, x.text) for x in logs.lines if x.text.startswith(alone)]
    t0 = logs.lines[0].t
    for msg, ev in sorted(events.items(), key=lambda kv: kv[1][0][0]):
        ev.sort(key=lambda e: e[0])
        start = ev[0][0]
        print(f"text {msg}, first seen at {ev[0][0] - t0:.1f} s on {n(ev[0][1])}")
        for t, node, what in ev:
            print(f"  {t - start:+9.3f} s  {n(node):>6}  {what}")
        path = [n(ev[0][1])]
        for _, node, what in ev:
            m = re.match(r"(?:forwarded|returned to its parent) (?:to )?(\S+),", what)
            if m:
                path.append(m.group(1).rstrip(","))
        app = [t for t, _, what in ev if what == "for this node's application"]
        end = f", at the application after {app[0] - start:.3f} s" if app else ", at no application"
        print(f"  path {' -> '.join(path)}{end}\n")
    if other:
        print("events that name no text:")
        for t, node, what in sorted(other):
            print(f"  {t - t0:9.1f} s  {n(node):>6}  {what}")
        if any("carried: true" in w for _, _, w in other):
            print("\nwarning: a board turned carried; it owns no position until it is reflashed")


def driver_frames(lines: list[Line]) -> list[tuple[float, int]]:
    out = []
    for x in lines:
        m = re.match(r"driver events=\d+ frames=(\d+)", x.text)
        if m:
            out.append((x.t, int(m.group(1))))
    return out


def catch(a: argparse.Namespace) -> None:
    """For each beacon run of the console K (`beacon` ... `stop`), the frames
    K sent and the frames the sleepy node S counted meanwhile: its driver
    line's `frames`, the last before the run and the first after it. Only
    K transmits on S's channel during a run, so the difference is what S
    caught (scripts/measure.md 5.1)."""
    logs = read(a.dir)
    known = sorted({x.node for x in logs.lines})
    s, k = full_id(a.sleepy, known), full_id(a.console, known)
    sl, kl = logs.of(s), logs.of(k)
    homes = [
        m.group(1)
        for x in sl
        if (m := re.search(r"(?:^stored Some\(Stored \{ home|^HomeChannelMoved \{.*to): Channel\((\d+)\)", x.text))
    ]
    if homes:
        print(f"{s[-4:]} receives on channel {homes[-1]}")
    frames = driver_frames(sl)
    runs, cur, symbols = [], None, None
    for x in kl:
        m = re.search(r"symbols=(\d+)", x.text)
        if m and "preamble" in x.text:
            symbols = int(m.group(1))
        if x.text.startswith("beacon ch="):
            if cur is None:
                cur = {"symbols": symbols, "times": [], "ch": re.search(r"ch=(\d+)", x.text).group(1)}
                runs.append(cur)
            cur["times"].append(x.t)
        elif x.text.startswith("> stop"):
            cur = None
    if not runs:
        sys.exit(f"{k[-4:]}: no beacon lines")
    for r in runs:
        first, last = r["times"][0], r["times"][-1]
        before = [f for f in frames if f[0] < first]
        after = [f for f in frames if f[0] > last + 1.0]
        sent = len(r["times"])
        head = f"run on channel {r['ch']}, {r['symbols']} preamble symbols: {sent} sent"
        if not before or not after:
            print(f"{head}; no driver line of {s[-4:]} {'before' if not before else 'after'} it")
            continue
        caught = after[0][1] - before[-1][1]
        print(f"{head}, {caught} counted by {s[-4:]} ({caught / sent:.0%})")
        print(
            f"  driver lines at {before[-1][0] - first:+.1f} s and "
            f"{after[0][0] - last:+.1f} s from the run's ends"
        )


def watch(a: argparse.Namespace) -> None:
    """The sleepy node's anchor as it stored it, the watches for the anchor's
    heartbeat (`AnchorWatch`), its checks, and what it was given."""
    logs = read(a.dir)
    names = names_arg(a)
    known = sorted({x.node for x in logs.lines})
    s = full_id(a.sleepy, known)
    t0 = logs.lines[0].t
    heard, missed, offsets = 0, 0, []
    for x in logs.of(s):
        line = None
        if x.text.startswith("store ") and "anchor" in x.text:
            m = re.search(r"anchor: Some\(Anchor \{ id: (NodeId\(\[[\d, ]+\]\))", x.text)
            line = f"anchor {name_of(names, node_hex(m.group(1)))}" if m else "anchor None"
        elif x.text.startswith("AnchorWatch"):
            ok = "heard: true" in x.text
            off = re.search(r"offset_us: (-?\d+)", x.text)
            heard += ok
            missed += not ok
            if ok and off:
                offsets.append(int(off.group(1)))
            line = x.text
        elif x.text.startswith(("AnchorCheck", "Role", "HomeChannelMoved", "message id=", "boot relays=")):
            line = x.text
        if line:
            print(f"  {x.t - t0:9.1f} s  {line}")
    print(f"\nwatches: {heard} heard, {missed} missed", end="")
    if offsets:
        print(f"; the heartbeat {min(offsets)} to {max(offsets)} us from the time expected")
    else:
        print()


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    b = sub.add_parser("build", help="UF2s for step 4 (line A B C) or 5 (sleepy R S)")
    b.add_argument("what", choices=["line", "sleepy"])
    b.add_argument("ids", nargs="+", help="twelve hex digits each")
    b.set_defaults(fn=build)
    c = sub.add_parser("capture", help="log every board, host time on each line")
    c.add_argument("dir")
    c.set_defaults(fn=capture)
    s = sub.add_parser("say", help="type a console command into a board")
    s.add_argument("id")
    s.add_argument("command")
    s.set_defaults(fn=say)
    o = sub.add_parser("off", help="the board into its bootloader: silent")
    o.add_argument("id")
    o.set_defaults(fn=off)
    o = sub.add_parser("on", help="copy a UF2 to the board's bootloader drive")
    o.add_argument("id", help="twelve hex digits")
    o.add_argument("uf2")
    o.set_defaults(fn=on)
    t = sub.add_parser("texts", help="each text's path and timing")
    t.add_argument("dir")
    t.add_argument("--name", action="append", help="LABEL=ID, as A=f4f5")
    t.set_defaults(fn=texts)
    k = sub.add_parser("catch", help="long-preamble frames the sleepy node caught")
    k.add_argument("dir")
    k.add_argument("sleepy")
    k.add_argument("console")
    k.set_defaults(fn=catch)
    w = sub.add_parser("watch", help="the sleepy node's anchor and its watches")
    w.add_argument("dir")
    w.add_argument("sleepy")
    w.add_argument("--name", action="append", help="LABEL=ID, as R=f4f5")
    w.set_defaults(fn=watch)
    a = ap.parse_args()
    a.fn(a)


if __name__ == "__main__":
    main()
