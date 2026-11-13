"""Scenarios on the bench's boards: builds, timed actions, and what passes them.

docs/bench-suite-2026-11-13.md. Run through scripts/bench.py from bark-fw/:

    scripts/bench.py run bench/a1-boot.toml A=f4f5 B=3112 C=c367 D=bac7
    scripts/bench.py check target/bench/runs/a1-boot-20261113-210000
    scripts/bench.py check LOGDIR bench/a1-boot.toml A=f4f5 ...

`run` builds an image for each board, every one under its own image
version, flashes each board with the loader and its image and checks the
version it reports, logs every board through one reader a port, does the
scenario's actions at their times, and then judges the logs: the standing
checks (checks.py) and the scenario's own conditions. It refuses to start
while another process holds a board's port. Everything goes to
target/bench/runs/<scenario>-<time>/: a log for each board, `run.json`
(the boards, their builds, the run's start and end) and `actions.jsonl`
(each action when it was done, and what its expected line held).

`check` judges a directory of logs again without a board: one a run wrote,
or any other with one log for each board (named so that its name holds the
board's ID, as `bench.py capture` writes them). Conditions that need the
host's clock or a run's actions are reported unchecked where the logs lack
them.

A scenario is a TOML file, as the simulator's are, which Python reads
without a package (tomllib):

    about = "one line"
    duration_s = 300.0          # from the run's start, once every board runs
    seed = 1                    # for actions at random times
    waits_for = ["..."]         # firmware it needs that main lacks: `run` refuses

    [[boards]]                  # in this order the builds take their versions
    name = "A"                  # the label on the command line, A=ID
    place_m = -150.0            # built in, metres east of the desk; else GPS
    deaf_to = "C"               # BARK_IGNORE: drops every frame of C
    env = { BARK_ON_BATTERY = "1" }
    features = ["bench"]
    serves = false              # holds its image to serve over the mesh
    silent = false              # kept in its bootloader for the run
    on_boot = ["..."]           # console commands typed at each boot

    [[actions]]
    at_s = 60.0
    board = "A"                 # or a variable naming a board
    do = "say"                  # say; watch (the latest line, else the next);
                                # off; on (copies the board's UF2 again)
    command = "text ${C.id} ${C.lat} ${C.lon} hello"
    name = "ac"                 # its time and its captures: ${ac.id}
    expect = '^bench: send id=(?P<id>[0-9a-f]{8}) .*taken'
    expect_s = 10.0             # a line not seen in time fails the run
    every_s = [600.0, 1800.0]   # again after a wait drawn from this range
    until_s = 14400.0           # until then
    boards = ["A", "B"]         # instead of board: one drawn each time

    [[pass]]                    # every one must hold
    board = "C"                 # or boards = [...], each must hold
    line = 'message id=${ac.id}'
    after = "ac"                # from that action; else the run's start
    within_s = 6.3              # for so long; else to the run's end
    count = 1                   # at least so many lines
    absent = false              # no such line instead
    last_boot = false           # in the log's last boot only
    # or: any = [{...}, {...}]  one of these
    # or: kept = { before = '...(?P<v>..)', after = '...(?P<v>..)', compare = "same" }
    #     at each reboot, the last `before` of the old boot against the
    #     first `after` of the new one, "same" or "not_less"; with
    #     unless = '...', not at a reboot whose old boot logged that

    [checks]
    skip = { heartbeat = "why" }  # a standing check the scenario defeats by design

The window of a condition starts, without `after`, when the run's setup
first touched a board, and the standing checks read each board's log from
its flash on: what came before is its old image's.

Variables are written ${name} in a command, a board or a line: for each
board <label>.id (twelve hex digits), <label>.id_dec (as the firmware's
NodeId prints, `186, 230, ...`), <label>.version, and <label>.lat and
<label>.lon in degrees, the desk's where no place is built in; for each named action
its expect's groups, <name>.<group>, and a hex group as decimal with
|dec, as a MsgId prints: ${ac.id|dec}. In a line they match literally.
"""

from __future__ import annotations

import argparse
import json
import random
import re
import subprocess
import sys
import threading
import time
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

import bench
import checks

RUNS = bench.OUT / "runs"
# The last image version a bench build took, so that no two builds ever
# share one: a board keeps the blocks it gathered and the versions it gave
# up across runs (bark-fw/src/update/mesh.rs).
VERSION_FILE = bench.OUT / "image-version"
# From the copy of a UF2 to the version in the board's log: the copy, the
# boot, and the first 10 s report, which repeats the banner's version for a
# host that reopened the port late (bark-fw/src/boot.rs); with room.
BOOT_S = 60.0
VERSION = re.compile(r"(?:image version |\bimage_version=)(\d+)\b")
VAR = re.compile(r"\$\{([A-Za-z_][\w]*(?:\.[\w]+)?)(\|dec)?\}")


class Bad(Exception):
    """A scenario file that says something wrong."""


# ---- the file --------------------------------------------------------------------


@dataclass
class Board:
    name: str
    place_m: float | None = None
    deaf_to: str | None = None
    env: dict[str, str] = field(default_factory=dict)
    features: list[str] = field(default_factory=list)
    serves: bool = False
    silent: bool = False
    on_boot: list[str] = field(default_factory=list)


@dataclass
class Action:
    at_s: float
    do: str
    board: str | None = None
    boards: list[str] = field(default_factory=list)
    command: str | None = None
    name: str | None = None
    expect: str | None = None
    expect_s: float = 10.0
    every_s: tuple[float, float] | None = None
    until_s: float | None = None


@dataclass
class Scenario:
    name: str
    about: str
    duration_s: float
    boards: list[Board]
    actions: list[Action]
    passes: list[dict]
    seed: int = 1
    waits_for: list[str] = field(default_factory=list)
    skip: dict[str, str] = field(default_factory=dict)

    def board(self, name: str) -> Board:
        return next(b for b in self.boards if b.name == name)


def take(d: dict, keys: set[str], what: str) -> None:
    unknown = set(d) - keys
    if unknown:
        raise Bad(f"{what}: unknown {', '.join(sorted(unknown))}")


# What the runner builds in from the board's own fields.
RUNNER_SETS = ("BARK_IMAGE_VERSION", "BARK_IGNORE", "BARK_LAT", "BARK_LON", "BARK_OPEN")
CONDITION_KEYS = {"board", "boards", "line", "after", "within_s", "count", "absent", "last_boot", "any", "kept", "why"}


def check_condition(c: dict, boards: set[str], actions: set[str], what: str) -> None:
    take(c, CONDITION_KEYS, what)
    if "any" in c:
        if set(c) - {"any", "why"} or not c["any"]:
            raise Bad(f"{what}: `any` stands alone and lists conditions")
        for i, sub in enumerate(c["any"]):
            check_condition(sub, boards, actions, f"{what}, any {i + 1}")
        return
    named = [c["board"]] if "board" in c else c.get("boards", [])
    if not named or ("board" in c and "boards" in c):
        raise Bad(f"{what}: give board or boards")
    for b in named:
        if b not in boards:
            raise Bad(f"{what}: no board {b}")
    if ("line" in c) == ("kept" in c):
        raise Bad(f"{what}: give line or kept")
    if "kept" in c:
        take(c["kept"], {"before", "after", "compare", "unless"}, f"{what}, kept")
        if c["kept"].get("compare", "same") not in ("same", "not_less"):
            raise Bad(f"{what}: compare is same or not_less")
        for k in ("before", "after"):
            if "(?P<v>" not in c["kept"].get(k, ""):
                raise Bad(f"{what}: kept's {k} needs a group (?P<v>...)")
    if c.get("after") not in (None, *actions):
        raise Bad(f"{what}: after names no action")
    for k in ("line",):
        if k in c:
            re.compile(VAR.sub("x", c[k]))


def load(path: str | Path) -> Scenario:
    p = Path(path)
    with open(p, "rb") as f:
        d = tomllib.load(f)
    take(d, {"name", "about", "duration_s", "seed", "waits_for", "boards", "actions", "pass", "checks"}, p.name)
    boards = []
    for b in d.get("boards", []):
        take(b, {"name", "place_m", "deaf_to", "env", "features", "serves", "silent", "on_boot"}, f"board {b.get('name')}")
        if not re.fullmatch(r"[A-Za-z_]\w*", b.get("name", "")):
            raise Bad("a board's name is a word")
        env = b.get("env", {})
        bad = [k for k in env if not k.startswith("BARK_") or k in RUNNER_SETS or not isinstance(env[k], str)]
        if bad:
            raise Bad(f"board {b['name']}: env {', '.join(bad)}: BARK_ variables as strings, none the runner sets")
        boards.append(Board(**b))
    names = {b.name for b in boards}
    if len(names) != len(boards) or not boards:
        raise Bad("boards: none, or two of one name")
    for b in boards:
        if b.deaf_to is not None and b.deaf_to not in names:
            raise Bad(f"board {b.name}: deaf to no board {b.deaf_to}")
    actions = []
    for i, a in enumerate(d.get("actions", [])):
        what = f"action {i + 1}"
        take(a, {"at_s", "do", "board", "boards", "command", "name", "expect", "expect_s", "every_s", "until_s"}, what)
        a = dict(a)
        if "every_s" in a:
            a["every_s"] = tuple(a["every_s"])
            if len(a["every_s"]) != 2 or not 0 < a["every_s"][0] <= a["every_s"][1] or "until_s" not in a:
                raise Bad(f"{what}: every_s is [least, most] with until_s")
        act = Action(**a)
        if act.do not in ("say", "watch", "off", "on"):
            raise Bad(f"{what}: do is say, watch, off or on")
        if (act.do == "say") != (act.command is not None):
            raise Bad(f"{what}: a command goes with say alone")
        if act.do == "watch" and (act.expect is None or act.name is None):
            raise Bad(f"{what}: watch takes expect and name")
        if (act.board is None) == (not act.boards):
            raise Bad(f"{what}: give board or boards")
        for b in act.boards + ([act.board] if act.board and not VAR.search(act.board) else []):
            if b not in names:
                raise Bad(f"{what}: no board {b}")
        if act.expect is not None:
            re.compile(VAR.sub("x", act.expect))
        actions.append(act)
    named = {a.name for a in actions if a.name}
    passes = d.get("pass", [])
    for i, c in enumerate(passes):
        check_condition(c, names, named, f"pass {i + 1}")
    c = d.get("checks", {})
    take(c, {"skip"}, "checks")
    skip = c.get("skip", {})
    unknown = set(skip) - {"reset", "radio", "stack", "gap", "heartbeat"}
    if unknown:
        raise Bad(f"checks: no standing check {', '.join(sorted(unknown))}")
    return Scenario(
        name=d.get("name", p.stem),
        about=d.get("about", ""),
        duration_s=float(d["duration_s"]),
        boards=boards,
        actions=actions,
        passes=passes,
        seed=d.get("seed", 1),
        waits_for=d.get("waits_for", []),
        skip=skip,
    )


def timeline(s: Scenario) -> list[tuple[float, Action, str]]:
    """Every action as (time from the start, action, board), in order, the
    repeated ones at their drawn times."""
    rng = random.Random(s.seed)
    out = []
    for a in s.actions:
        t = a.at_s
        while True:
            board = a.board if a.board is not None else rng.choice(a.boards)
            out.append((t, a, board))
            if a.every_s is None:
                break
            t += rng.uniform(*a.every_s)
            if t > a.until_s:
                break
    out.sort(key=lambda e: e[0])
    return out


# ---- variables -------------------------------------------------------------------


def id_dec(node: str) -> str:
    return ", ".join(str(int(node[i : i + 2], 16)) for i in range(0, 12, 2))


def board_vars(ids: dict[str, str], versions: dict[str, int], boards: list[Board] = []) -> dict[str, str]:
    out = {}
    for b in boards:
        # A board with no place built in is on the desk.
        out[f"{b.name}.lat"], out[f"{b.name}.lon"] = bench.place(b.place_m or 0.0)
    for label, node in ids.items():
        out[f"{label}.id"] = node
        out[f"{label}.id_dec"] = id_dec(node)
        if label in versions:
            out[f"{label}.version"] = str(versions[label])
    return out


def fill(text: str, vars: dict[str, str], escape: bool) -> str | None:
    """`text` with its variables filled; None if one is unknown."""
    missing = False

    def one(m: re.Match) -> str:
        nonlocal missing
        v = vars.get(m.group(1))
        if v is None:
            missing = True
            return ""
        if m.group(2):
            v = str(int(v, 16))
        return re.escape(v) if escape else v

    out = VAR.sub(one, text)
    return None if missing else out


# ---- judging the logs ------------------------------------------------------------


@dataclass
class Run:
    """What the judging knows of a run: each label's log, the variables,
    the actions' times by name, and on the host's clock (None for logs
    without it) the run's start, when its boards first left their old
    images, and its end."""

    logs: dict[str, checks.Log]
    vars: dict[str, str]
    acted: dict[str, float]
    start: float | None
    end: float | None


def lines_in(log: checks.Log, lo: float | None, hi: float | None) -> list[checks.Line]:
    if lo is None and hi is None:
        return log.lines
    return [x for x in log.lines if x.host is not None and (lo is None or x.host >= lo) and (hi is None or x.host <= hi)]


def judge(c: dict, run: Run) -> tuple[str, str]:
    """One condition: (PASS, FAIL or UNCHECKED, what was seen)."""
    if "any" in c:
        got = [judge(sub, run) for sub in c["any"]]
        if any(k == "pass" for k, _ in got):
            return "pass", next(w for k, w in got if k == "pass")
        kind = checks.UNCHECKED if any(k == checks.UNCHECKED for k, _ in got) else checks.FAIL
        return kind, "; ".join(w for _, w in got)
    boards = [c["board"]] if "board" in c else c["boards"]
    lo = run.start
    if "after" in c:
        lo = run.acted.get(c["after"])
        if lo is None:
            return checks.UNCHECKED, f"no time for the action {c['after']}"
    hi = run.end if "within_s" not in c else (lo + c["within_s"] if lo is not None else None)
    if "within_s" in c and lo is None:
        return checks.UNCHECKED, "a time limit, and logs without the host's clock"
    seen = []
    for b in boards:
        log = run.logs.get(b)
        if log is None:
            return checks.UNCHECKED, f"no log for {b}"
        if (lo is not None or hi is not None) and not log.host_time:
            return checks.UNCHECKED, f"{b}: a time window, and a log without the host's clock"
        if "kept" in c:
            kind, what = kept(c["kept"], log, lo, hi)
            if kind != "pass":
                return kind, f"{b}: {what}"
            seen.append(f"{b}: {what}")
            continue
        pattern = fill(c["line"], run.vars, escape=True)
        if pattern is None:
            return checks.UNCHECKED, f"{c['line']}: a variable no run gave"
        r = re.compile(pattern)
        lines = lines_in(log, lo, hi)
        if c.get("last_boot"):
            last = {id(x) for x in log.boots[-1].lines} if log.boots else set()
            lines = [x for x in lines if id(x) in last]
        found = [x for x in lines if r.search(x.text)]
        if c.get("absent"):
            if found:
                return checks.FAIL, f"{b} line {found[0].n}: {found[0].text[:100]}"
            seen.append(f"{b}: none")
        elif len(found) < c.get("count", 1):
            return checks.FAIL, f"{b}: {len(found)} of /{c['line']}/"
        else:
            first = found[0]
            when = f" after {first.host - lo:.1f} s" if lo is not None and first.host is not None else ""
            seen.append(f"{b}: {len(found)}{when}")
    return "pass", ", ".join(seen)


def kept(k: dict, log: checks.Log, lo: float | None, hi: float | None) -> tuple[str, str]:
    before, after = re.compile(k["before"]), re.compile(k["after"])
    unless = re.compile(k["unless"]) if "unless" in k else None
    less_ok = k.get("compare", "same") == "not_less"
    checked = 0
    for prev, boot in zip(log.boots, log.boots[1:]):
        t = boot.first_host()
        if (lo is not None or hi is not None) and (t is None or (lo is not None and t < lo) or (hi is not None and t > hi)):
            continue
        if unless is not None and any(unless.search(x.text) for x in prev.lines):
            continue
        old = [m.group("v") for x in prev.lines if (m := before.search(x.text))]
        if not old:
            continue
        new = next((m.group("v") for x in boot.lines if (m := after.search(x.text))), None)
        if new is None:
            return checks.FAIL, f"line {boot.lines[0].n}: a boot without /{k['after']}/"
        ok = int(new) >= int(old[-1]) if less_ok else new == old[-1]
        if not ok:
            return checks.FAIL, f"line {boot.lines[0].n}: {old[-1]} before the reboot, {new} after"
        checked += 1
    return "pass", f"{checked} reboots"


def evaluate(s: Scenario | None, run: Run, rooms: dict[str, int], asked: dict[str, list[float]] | None,
             silent_at_end: set[str]) -> tuple[bool, list[str]]:
    """The standing checks and the scenario's conditions: whether they
    pass, and the report."""
    labels = {log.name: label for label, log in run.logs.items()}
    findings = checks.standing(
        list(run.logs.values()),
        asked=None if asked is None else {run.logs[k].name: v for k, v in asked.items() if k in run.logs},
        rooms={run.logs[k].name: v for k, v in rooms.items() if k in run.logs},
        end=run.end,
        silent_at_end={run.logs[k].name for k in silent_at_end if k in run.logs},
    )
    skip = s.skip if s else {}
    report = []
    for check, why in skip.items():
        report.append(f"skipped {check}: {why}")
    findings = [f for f in findings if f.check not in skip]
    for f in findings:
        f.board = labels.get(f.board, f.board)
    report += checks.summary(findings)
    ok = not any(f.kind == checks.FAIL for f in findings)
    for i, c in enumerate(s.passes if s else [], 1):
        kind, what = judge(c, run)
        why = f" ({c['why']})" if "why" in c else ""
        word = {"pass": "pass", checks.FAIL: "FAIL", checks.UNCHECKED: "unchecked"}[kind]
        report.append(f"{word} {i}{why}: {what}")
        ok &= kind == "pass"
    return ok, report


# ---- check: a directory of logs --------------------------------------------------


def label_logs(d: Path, given: dict[str, str], quiet: set[str] = set()) -> dict[str, Path]:
    """Each label's log in `d`: the one whose name holds the ID it was
    given; without labels, every log under its own name. A board in
    `quiet`, in its bootloader for the whole run, may have none."""
    files = sorted(d.glob("*.log"))
    if not given:
        return {f.stem: f for f in files}
    out = {}
    for label, node in given.items():
        found = [f for f in files if node.lower() in f.stem.lower()]
        if not found and label in quiet:
            continue
        if len(found) != 1:
            sys.exit(f"{label}={node}: {len(found)} logs in {d} have it in their name")
        out[label] = found[0]
    return out


def read_since(path: Path, since: float | None) -> checks.Log:
    """A board's log from `since` on the host's clock: what its old image
    logged before the run's setup is not the run's. The lines before stay
    as blanks, so that the line numbers are the file's."""
    with open(path, encoding="utf-8", errors="replace", newline="\n") as f:
        raw = f.readlines()
    if since is not None:
        for i, line in enumerate(raw):
            h = checks.HOST.match(line)
            if h and float(h.group(1)) >= since:
                break
            raw[i] = ""
    return checks.parse(raw, path.stem)


def labels_arg(pairs: list[str]) -> dict[str, str]:
    out = {}
    for pair in pairs:
        label, eq, node = pair.partition("=")
        if not eq:
            sys.exit(f"{pair}: give LABEL=ID")
        out[label] = node
    return out


def judge_dir(d: Path, scenario: str | None, given: dict[str, str], stack_room: int | None) -> tuple[bool, list[str]]:
    """The checks over a directory of logs, a run's or any other."""
    meta = json.loads((d / "run.json").read_text()) if (d / "run.json").exists() else {}
    s = load(scenario) if scenario else (load(meta["scenario"]) if "scenario" in meta else None)
    acted: dict[str, float] = {}
    vars = board_vars(meta.get("ids", {}), meta.get("versions", {}), s.boards if s else [])
    asked: dict[str, list[float]] | None = None
    silent = set(meta.get("silent_at_end", []))
    flashed: dict[str, float] = {}
    failed = []
    if (d / "actions.jsonl").exists():
        asked = {}
        for line in (d / "actions.jsonl").read_text().splitlines():
            e = json.loads(line)
            board = e.get("board")
            if e.get("resets"):
                asked.setdefault(board, []).append(e["t"])
            if e.get("do") in ("off", "on") and board:
                (silent.add if e["do"] == "off" else silent.discard)(board)
                if e["do"] == "on":
                    flashed.setdefault(board, e["t"])
            if e.get("name") and "failed" not in e:
                acted[e["name"]] = e["t"]
                vars.update({f"{e['name']}.{k}": v for k, v in e.get("vars", {}).items()})
            if "failed" in e:
                failed.append(f"{board or ''}: {e['failed']}")
    quiet = {b.name for b in s.boards if b.silent} if s else set()
    files = label_logs(d, given or meta.get("ids", {}), quiet)
    # Each board from its flash: before it, its old image's lines.
    logs = {label: read_since(f, flashed.get(label, meta.get("setup"))) for label, f in files.items()}
    rooms = meta.get("stack_rooms", {})
    if stack_room:
        rooms = {label: stack_room for label in logs}
    run = Run(logs, vars, acted, meta.get("setup"), meta.get("end"))
    ok, report = evaluate(s, run, rooms, asked, silent)
    if failed:
        ok = False
        report = [f"FAIL action {f}" for f in failed] + report
    return ok, report


def check_cmd(a: argparse.Namespace) -> None:
    ok, report = judge_dir(Path(a.dir), a.scenario, labels_arg(a.boards), a.stack_room)
    print("\n".join(report))
    print("PASS" if ok else "FAIL")
    sys.exit(0 if ok else 1)


# ---- run: on the boards ----------------------------------------------------------


def next_versions(n: int, first: int | None) -> list[int]:
    """`n` image versions no bench build has had (VERSION_FILE)."""
    last = int(VERSION_FILE.read_text()) if VERSION_FILE.exists() else None
    if first is not None:
        if last is not None and first <= last:
            sys.exit(f"--first-version {first}: bench builds have reached {last}")
        last = first - 1
    elif last is None:
        sys.exit(
            f"no {VERSION_FILE}: give --first-version, above every version a board has run or gathered "
            "(their banners' `image version`)"
        )
    out = list(range(last + 1, last + 1 + n))
    VERSION_FILE.parent.mkdir(parents=True, exist_ok=True)
    VERSION_FILE.write_text(f"{out[-1]}\n")
    return out


def build_env(b: Board, ids: dict[str, str]) -> dict[str, str]:
    env = dict(b.env)
    if b.place_m is not None:
        env["BARK_LAT"], env["BARK_LON"] = bench.place(b.place_m)
    if b.deaf_to is not None:
        env["BARK_IGNORE"] = ids[b.deaf_to]
    return env


class Live:
    """A run on the boards: the reader, the actions' record."""

    def __init__(self, s: Scenario, ids: dict[str, str], out: Path, built: dict[str, bench.Built]):
        self.s, self.ids, self.out, self.built = s, ids, out, built
        self.vars = board_vars(ids, {k: v.version for k, v in built.items()}, s.boards)
        self.record = open(out / "actions.jsonl", "a", buffering=1)
        self.label = {node: label for label, node in ids.items()}
        self.booted: dict[str, float] = {}
        self.boots: list[str] = []
        self.lock = threading.Lock()
        self.cap = bench.Capture(out, set(ids.values()), say=self.say, on_line=self.line)

    def say(self, text: str) -> None:
        print(text, flush=True)

    def note(self, **e) -> None:
        e.setdefault("t", time.time())
        self.record.write(json.dumps(e) + "\n")

    def fail(self, why: str, **e) -> None:
        self.say(f"FAIL {why}")
        self.note(failed=why, **e)

    def line(self, node: str, t: float, text: str) -> None:
        """From the reader: a boot of a board with boot commands, its
        banner or its first report, whichever the reader saw first."""
        label = self.label[node]
        if not self.s.board(label).on_boot or not (checks.BANNER.match(text) or checks.BOOT_FACTS.match(text)):
            return
        with self.lock:
            if t - self.booted.get(label, -BOOT_S) >= BOOT_S:
                self.booted[label] = t
                self.boots.append(label)

    def on_boot(self) -> None:
        """The boot commands of the boards that booted since last asked."""
        with self.lock:
            due, self.boots = self.boots, []
        for label in due:
            for c in self.s.board(label).on_boot:
                try:
                    bench.say(self.ids[label], c)
                except (OSError, KeyError) as e:
                    self.fail(f"{c} at its boot: {e!r}", board=label, do="say")
                    break
                self.note(board=label, do="say", command=c, on_boot=True)

    def reset(self, label: str, do: str, name: str | None = None) -> None:
        node = self.ids[label]
        if do == "off":
            self.note(board=label, do=do, name=name, resets=True)
            bench.off(node)
        else:
            # Noted once copied: the judge reads the board's log from here,
            # and the old image's last lines come before it.
            bench.on(node, self.built[label].uf2)
            self.note(board=label, do=do, name=name, resets=True)

    def silence(self, b: Board) -> None:
        """Into the bootloader, unless it is there already."""
        if self.ids[b.name] in bench.ports():
            self.reset(b.name, "off")

    def flash(self, b: Board) -> None:
        """The board's UF2 copied to its bootloader, and its version read."""
        node = self.ids[b.name]
        t = time.time()
        self.reset(b.name, "on")
        want = self.built[b.name].version
        got = self.cap.wait(node, VERSION, t, BOOT_S)
        if got is None:
            sys.exit(f"{b.name}: no version in its log within {BOOT_S:.0f} s of the copy")
        if int(got[1].group(1)) != want:
            sys.exit(f"{b.name}: runs version {got[1].group(1)}, flashed with {want}: a stale UF2?")
        self.say(f"{b.name}: version {want} runs")

    def act(self, a: Action, label: str) -> None:
        """One action; one that cannot be done is a failure of the run,
        which goes on so that its logs say what followed."""
        try:
            self._act(a, label)
        except (OSError, KeyError, subprocess.CalledProcessError, SystemExit) as e:
            self.fail(f"{a.name or a.command or a.do} on {label}: {e!r}", board=label, do=a.do)

    def _act(self, a: Action, label: str) -> None:
        what = a.name or a.command or a.do
        target = fill(label, self.vars, escape=False)
        board = self.resolve(target) if target is not None else None
        if board is None:
            return self.fail(f"{what}: no board {label}", board=label, do=a.do)
        if a.do in ("off", "on"):
            return self.reset(board, a.do, a.name)
        command = fill(a.command, self.vars, escape=False) if a.command is not None else ""
        pattern = fill(a.expect, self.vars, escape=True) if a.expect is not None else ""
        if command is None or pattern is None:
            return self.fail(f"{what}: a variable no action gave", board=board, do=a.do)
        t = time.time()
        e = {"t": t, "board": board, "do": a.do, "name": a.name}
        got = None
        if a.do == "say":
            bench.say(self.ids[board], command)
            e.update(command=command, resets=command == "reboot")
            if a.expect is None:
                return self.note(**e)
        else:
            got = self.cap.last(self.ids[board], re.compile(pattern))
        got = got or self.cap.wait(self.ids[board], re.compile(pattern), t, a.expect_s)
        if got is None:
            return self.fail(f"{what}: no /{a.expect}/ within {a.expect_s} s", **e)
        e["vars"] = got[1].groupdict()
        if a.name:
            self.vars.update({f"{a.name}.{k}": v for k, v in e["vars"].items()})
        self.note(**e)

    def resolve(self, target: str) -> str | None:
        """A board's label from a label, an ID or the NodeId's bytes."""
        for label, node in self.ids.items():
            if target in (label, node, id_dec(node)):
                return label
        return None


def run_cmd(a: argparse.Namespace) -> None:
    s = load(a.scenario)
    if s.waits_for and not a.anyway:
        sys.exit(f"{s.name} waits for: {'; '.join(s.waits_for)}")
    given = labels_arg(a.boards)
    missing = [b.name for b in s.boards if b.name not in given]
    if missing:
        sys.exit(f"give each board as LABEL=ID: {', '.join(missing)} missing")
    present = bench.ports()
    # A board in its bootloader has no serial port: it is found by its
    # twelve digits, and its drive when it is flashed.
    ids = {label: bench.full_id(node, list(present)) for label, node in given.items() if label in {b.name for b in s.boards}}
    held = {label: bench.holders(present[node]) for label, node in ids.items() if node in present}
    held = {k: v for k, v in held.items() if v}
    if held:
        sys.exit("ports held: " + "; ".join(f"{k} by {', '.join(v)}" for k, v in held.items()))

    out = RUNS / f"{s.name}-{time.strftime('%Y%m%d-%H%M%S')}"
    out.mkdir(parents=True)
    versions = next_versions(len(s.boards), a.first_version)
    bench.build_loader()
    built = {}
    for b, v in zip(s.boards, versions):
        built[b.name] = bench.build_image(b.name, build_env(b, ids), b.features, v, out / "builds", b.serves)
    meta = {
        "scenario": str(Path(a.scenario).resolve()),
        "ids": ids,
        "versions": {k: v.version for k, v in built.items()},
        "stack_rooms": {k: v.stack_room for k, v in built.items()},
        "silent_at_end": [b.name for b in s.boards if b.silent],
    }
    (out / "run.json").write_text(json.dumps(meta, indent=1))
    if a.build_only:
        print(f"built in {out}/builds")
        return

    live = Live(s, ids, out, built)
    live.cap.start()
    try:
        # All silent first, so that no board runs its old image beside the
        # new ones, and the boots fall close together.
        meta["setup"] = time.time()
        for b in s.boards:
            live.silence(b)
        for b in s.boards:
            if not b.silent:
                live.flash(b)
        start = time.time()
        meta["start"] = start
        live.note(do="start", t=start)
        print(f"{s.name}: started; {s.duration_s:.0f} s; logs in {out}", flush=True)
        for at, action, board in timeline(s):
            while time.time() < start + min(at, s.duration_s):
                live.on_boot()
                time.sleep(0.2)
            if at > s.duration_s:
                break
            live.act(action, board)
        while time.time() < start + s.duration_s:
            live.on_boot()
            time.sleep(0.2)
    finally:
        meta["end"] = time.time()
        live.note(do="end", t=meta["end"])
        live.cap.stop()
        for t, node, note in live.cap.notes:
            live.note(t=t, board=live.label[node], failed=note)
        (out / "run.json").write_text(json.dumps(meta, indent=1))
    ok, report = judge_dir(out, None, {}, None)
    (out / "result.txt").write_text("\n".join(report + ["PASS" if ok else "FAIL"]) + "\n")
    print("\n".join(report))
    print("PASS" if ok else "FAIL")
    sys.exit(0 if ok else 1)


def add_commands(sub) -> None:
    r = sub.add_parser("run", help="a scenario on the boards (scripts/scenario.py)")
    r.add_argument("scenario")
    r.add_argument("boards", nargs="+", metavar="LABEL=ID", help="the board each label stands for")
    r.add_argument("--first-version", type=int, help="the first image version, if no bench build has one yet")
    r.add_argument("--build-only", action="store_true", help="build and stop before any board is touched")
    r.add_argument("--anyway", action="store_true", help="run a scenario that waits for firmware")
    r.set_defaults(fn=run_cmd)
    c = sub.add_parser("check", help="a scenario's checks over logs already written")
    c.add_argument("dir")
    c.add_argument("scenario", nargs="?")
    c.add_argument("boards", nargs="*", metavar="LABEL=ID")
    c.add_argument("--stack-room", type=int, help="the boards' stack room, bytes, if no run recorded it")
    c.set_defaults(fn=check_cmd)
