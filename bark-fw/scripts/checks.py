"""The standing checks: faults no bench run may show, as functions of the boards' logs.

docs/bench-suite-2026-11-13.md, "The standing checks". Each check takes
parsed logs (`parse`) and returns findings: one of kind FAIL fails the run,
one of kind UNCHECKED says what the logs lack for the check to decide.
No dependencies beyond Python 3; tested by test_checks.py beside this file.

A log is what `bench.py capture` and the scenario runner write: each line
the board sent, after the host's clock in seconds when the host has one.
Every line the firmware logs starts `[ <uptime s> <LEVEL> ]`
(bark-fw/src/usb_log.rs).
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from typing import Iterable

FAIL = "fail"
UNCHECKED = "unchecked"

# The uptime line's period: 20 half-periods of the LED's 500 ms blink
# (bark-fw/src/main.rs, UPTIME_EVERY), logged while on USB, which every
# board on the bench is.
STATUS_PERIOD_S = 10.0
# A USB power event restarts the blink's half-period without counting it
# (main.rs, `on_usb_only`), so a line may come up to one half-period late.
STATUS_SLACK_S = 0.5
# The longest a line waits in the board before the host can read it: the
# log's 2048-byte ring drained at one 64-byte packet per 1 ms USB frame
# (bark-fw/src/usb_log.rs, RING and PIECE; update.rs, LOG_DRAIN).
LOG_DRAIN_S = 2048 / 64 * 0.001

# The stack: what a node image must keep free beyond the most it has used,
# for what the disassembly cannot show (bark-fw/README.md, "Tasks and the
# stack"), and the least room any packed image leaves it
# (scripts/uf2.py, STACK_FLOOR).
STACK_MARGIN = 6 * 1024
STACK_FLOOR = 24 * 1024

# A heartbeat's listen-before-talk: three checks, each busy one followed
# by a random wait of up to 200 ms (spec "Heartbeat"; bark-proto Config,
# lbt_attempts and lbt_backoff_max): the first check is at most 400 ms
# before the skip.
LBT_SPAN_S = (3 - 1) * 0.200
# The error in mapping a board's uptime to the host's clock (`HostClock`):
# a calibrated RC oscillator's 250 ppm (nRF52840 Product Specification,
# LFRC) over the three status periods an estimate spans, and one USB frame.
CLOCK_SLACK_S = 3 * STATUS_PERIOD_S * 250e-6 + 0.001


# ---- the log -----------------------------------------------------------------

# The host's clock in front of a line (bench.py, `capture`).
HOST = re.compile(r"^(\d+\.\d+) (.*)$")
# The firmware's line header (usb_log.rs); found anywhere, so that two
# records the host joined into one line are told apart.
HEADER = re.compile(r"\[\s*(\d+\.\d+)\s+(TRACE|DEBUG|INFO|WARN|ERROR)\s*\]\s?")
BANNER = re.compile(r"^bark-fw \S+ on .*?, (node|console) image version (\d+), id=([0-9a-f]{12})")
BOOT_FACTS = re.compile(r"^boot: .*\bimage_version=(\d+)")
# Lines that are no record and no fault: the console's echo of what was
# typed, and a reader's notes of its own.
NOT_RECORDS = ("> ", "=== ", "--- ", "# ")


@dataclass
class Line:
    n: int  # the line's number in its file, 1 first
    host: float | None  # the host's clock when it read the line
    board: float  # the board's uptime when it logged it
    level: str
    text: str


@dataclass
class Boot:
    lines: list[Line] = field(default_factory=list)
    kind: str | None = None  # "node" or "console", from the banner
    version: int | None = None

    def first_host(self) -> float | None:
        return next((x.host for x in self.lines if x.host is not None), None)

    def last_host(self) -> float | None:
        return next((x.host for x in reversed(self.lines) if x.host is not None), None)


@dataclass
class Log:
    name: str
    boots: list[Boot] = field(default_factory=list)
    # Lines that are no whole record: (line number, what it held, why).
    broken: list[tuple[int, str, str]] = field(default_factory=list)

    @property
    def lines(self) -> list[Line]:
        return [x for b in self.boots for x in b.lines]

    @property
    def host_time(self) -> bool:
        return any(x.host is not None for b in self.boots for x in b.lines)


def parse(raw: Iterable[str], name: str) -> Log:
    """A board's log from its lines, split into boots.

    A boot starts at a banner, or where the uptime goes back: lines are
    logged in order, so within one boot it never does. A banner can be
    missed, since the boot logs before the host reopens the port
    (bark-fw/src/boot.rs). A line that holds two headers is two records
    whose join lost the end of the first; one with text before its header,
    or none, is a piece of a record whose start was lost, except the lines
    of an error's message (a panic's), which follow it."""
    log = Log(name)
    boot = Boot()
    log.boots.append(boot)
    continues = False
    for n, line in enumerate(raw, 1):
        line = line.rstrip("\r\n").replace("\r", "")
        h = HOST.match(line)
        host = float(h.group(1)) if h else None
        body = h.group(2) if h else line
        heads = list(HEADER.finditer(body))
        if not heads:
            if body.strip() and not continues and not body.startswith(NOT_RECORDS):
                log.broken.append((n, body, "a line without its start"))
            continue
        lead = body[: heads[0].start()]
        if lead.strip() and not lead.startswith(NOT_RECORDS):
            log.broken.append((n, lead, "a line without its start"))
        if len(heads) > 1:
            log.broken.append((n, body, "two records on one line, the first cut"))
        for i, m in enumerate(heads):
            end = heads[i + 1].start() if i + 1 < len(heads) else len(body)
            x = Line(n, host, float(m.group(1)), m.group(2), body[m.end() : end].rstrip())
            banner = BANNER.match(x.text)
            prev = boot.lines[-1] if boot.lines else None
            if prev is not None and (banner or x.board < prev.board):
                boot = Boot()
                log.boots.append(boot)
            if banner:
                boot.kind, boot.version = banner.group(1), int(banner.group(2))
            facts = BOOT_FACTS.match(x.text)
            if facts and boot.version is None:
                boot.version = int(facts.group(1))
            boot.lines.append(x)
        continues = heads[-1].group(2) == "ERROR"
    if not log.boots[0].lines:
        log.boots.pop(0)
    return log


def read(path: str, name: str | None = None) -> Log:
    from pathlib import Path

    p = Path(path)
    with open(p, encoding="utf-8", errors="replace", newline="\n") as f:
        return parse(f, name or p.stem)


@dataclass
class Finding:
    check: str
    kind: str  # FAIL or UNCHECKED
    board: str
    what: str
    line: int | None = None  # the line's number in the board's log

    def __str__(self) -> str:
        at = f" line {self.line}" if self.line is not None else ""
        return f"{self.check}: {self.board}{at}: {self.what}"


# ---- the driver's 10 s line ----------------------------------------------------

# Its fields (bark-fw/src/node.rs, `Report::log_if_due`): the fixed ones,
# all integers but `update`, then the clock's and the battery's when known.
# test_checks.py holds this list to the format string in node.rs.
DRIVER_FIELDS = (
    "events frames failed failed_header failed_crc failed_timeout held radio_errors recoveries sniffs "
    "longest_handle_us longest_away_us empty_readies own_readies resets configured cold_starts tcxo_us "
    "tcxo_errors restarts restart_us restart_errors stack_used unserved update"
).split()
DRIVER_TAIL = "utc_s offset_min from battery_left_min drain_relay drain_sleepy".split()
NUMBER = re.compile(r"0|[1-9]\d*")
# The counters that count the radio's errors (bark-driver: `Stats` in
# lib.rs and sx1262.rs, `Delay` in tcxo.rs): a failed operation, the radio
# brought back after reading every channel busy, the chip reset after an
# error, and an oscillator start that set the error flag, cold or warm.
RADIO_COUNTERS = ("radio_errors", "recoveries", "resets", "tcxo_errors", "restart_errors")


def driver_fields(text: str) -> dict[str, str] | None:
    """The fields of a whole driver line; None for one the host cut and
    joined, whose fields are unknown, doubled, or not numbers. A log from
    an older build lacks some fields, which is no fault."""
    if not text.startswith("driver events="):
        return None
    out: dict[str, str] = {}
    key = None
    for token in text[len("driver ") :].split(" "):
        if "=" not in token and key == "update":
            # The update's phase holds a space: `holds:9 serving:0:0`.
            out[key] += " " + token
            continue
        key, eq, value = token.partition("=")
        if not eq or key in out or key not in DRIVER_FIELDS + DRIVER_TAIL:
            return None
        if key in DRIVER_FIELDS and key != "update" and not NUMBER.fullmatch(value):
            return None
        out[key] = value
    return out


# ---- the checks ----------------------------------------------------------------


def gaps(log: Log, end: float | None = None, silent_at_end: bool = False) -> list[Finding]:
    """A gap in the log: lines lost on the board or on the host.

    - a line broken by the host (`parse`), or a driver line it joined;
    - the firmware's own count of records it had no room for
      (`log: N records dropped`, usb_log.rs);
    - two uptime lines of one boot further apart than the period allows;
    - two lines of one boot, or the last line and `end`, the run's end,
      further apart on the host's clock than the period and the log's
      drain allow: a board or a reader that stopped. Not at the end for a
      board the run left silent."""
    out = [Finding("gap", FAIL, log.name, why, n) for n, _, why in log.broken]
    longest = STATUS_PERIOD_S + STATUS_SLACK_S
    for boot in log.boots:
        last_up = None
        last_host = None
        for x in boot.lines:
            if x.text.startswith("driver events=") and driver_fields(x.text) is None:
                out.append(Finding("gap", FAIL, log.name, "a driver line cut and joined", x.n))
            m = re.match(r"log: (\d+) records dropped", x.text)
            if m:
                out.append(Finding("gap", FAIL, log.name, f"the board dropped {m.group(1)} records", x.n))
            if re.fullmatch(r"uptime \d+ s", x.text):
                if last_up is not None and x.board - last_up > longest:
                    out.append(
                        Finding("gap", FAIL, log.name, f"{x.board - last_up:.1f} s since the last uptime line", x.n)
                    )
                last_up = x.board
            if x.host is not None:
                if last_host is not None and x.host - last_host > longest + LOG_DRAIN_S:
                    out.append(
                        Finding("gap", FAIL, log.name, f"nothing read for {x.host - last_host:.1f} s", x.n)
                    )
                last_host = x.host
    if end is not None and not silent_at_end and log.boots:
        last = log.boots[-1].last_host()
        if last is not None and end - last > longest + LOG_DRAIN_S:
            out.append(Finding("gap", FAIL, log.name, f"silent for the last {end - last:.1f} s of the run"))
    return out


# What the firmware logs as it resets on purpose: every reboot it chooses
# goes through `update::reboot` (bark-fw/src/update.rs), and the host's
# 1200-baud touch through touch.rs.
REBOOT = re.compile(r"^reboot$")
TOUCH = re.compile(r"^touch: 1200 baud")
PANIC = re.compile(r"^previous boot panicked")
# How long after an action of the host that resets a board its next boot
# may come: the bootloader's copy and reset, and a swap of the loader's,
# about 50 s (bark-fw/README.md, "Updates"), with room.
RESET_AFTER_S = 120.0


def resets(log: Log, asked: list[float] | None = None) -> list[Finding]:
    """A reset the scenario did not ask for.

    Each boot after the log's first must follow the firmware's own
    `reboot`, or an action of the host that resets the board, in `asked`,
    its host times, within RESET_AFTER_S. Without `asked`, a log not
    written by the runner, the 1200-baud touch counts as asked. A panic
    is never asked for."""
    out = []
    for prev, boot in zip(log.boots, log.boots[1:]):
        first = boot.lines[0]
        panic = next((x for x in boot.lines if PANIC.match(x.text)), None)
        if panic is not None:
            out.append(Finding("reset", FAIL, log.name, panic.text, panic.n))
            continue
        tail = prev.lines[-1].text if prev.lines else ""
        if REBOOT.match(tail):
            continue
        t = boot.first_host()
        if asked is None:
            if TOUCH.match(tail):
                continue
        elif t is not None and any(0 <= t - a <= RESET_AFTER_S for a in asked):
            continue
        why = f"a reset after `{tail[:60]}`" if tail else "a reset"
        out.append(Finding("reset", FAIL, log.name, why, first.n))
    return out


DROPPED = re.compile(r"^Dropped \{ msg_id: MsgId\((\d+)\), reason: (\w+)")


def radio(log: Log) -> list[Finding]:
    """A radio error or a message dropped: a radio counter of the driver's
    line above zero in a boot (they count from the boot), a `radio:`
    warning, or the core's `Dropped` trace."""
    out = []
    for boot in log.boots:
        worst: dict[str, tuple[int, int]] = {}
        for x in boot.lines:
            f = driver_fields(x.text)
            if f:
                for k in RADIO_COUNTERS:
                    v = int(f.get(k, "0"))
                    if v > worst.get(k, (0, 0))[0]:
                        worst[k] = (v, x.n)
            elif x.level == "WARN" and x.text.startswith("radio: "):
                out.append(Finding("radio", FAIL, log.name, x.text, x.n))
            m = DROPPED.match(x.text)
            if m:
                out.append(Finding("radio", FAIL, log.name, f"message {int(m.group(1)):08x} dropped, {m.group(2)}", x.n))
        for k, (v, n) in worst.items():
            out.append(Finding("radio", FAIL, log.name, f"{k}={v}", n))
    return out


def stack(log: Log, room: int | None = None) -> list[Finding]:
    """The stack's free room below its margin: the stack's room, from the
    board's image (`room`, scripts/uf2.py's `check_stack`) or else the
    floor every packed image leaves, less the most a node boot used
    (`stack_used`, bark-fw/src/stack.rs)."""
    room = STACK_FLOOR if room is None else room
    out = []
    for boot in log.boots:
        used = [(int(f["stack_used"]), x.n) for x in boot.lines if (f := driver_fields(x.text)) and "stack_used" in f]
        if not used:
            continue
        most, n = max(used)
        if room - most < STACK_MARGIN:
            out.append(
                Finding(
                    "stack", FAIL, log.name, f"{most} B used of {room} B: {room - most} B free, under {STACK_MARGIN}", n
                )
            )
    return out


# ---- across boards -------------------------------------------------------------


class HostClock:
    """A boot's uptime on the host's clock. A line reaches the host after
    the board logs it, never before, so the least of `host - board` over
    nearby lines is the offset; nearby, within a status period either
    side, keeps the oscillator's drift out (CLOCK_SLACK_S)."""

    def __init__(self, boot: Boot):
        self.least: dict[int, float] = {}
        for x in boot.lines:
            if x.host is not None:
                k = int(x.board // STATUS_PERIOD_S)
                d = x.host - x.board
                self.least[k] = min(d, self.least.get(k, d))

    def __bool__(self) -> bool:
        return bool(self.least)

    def host(self, board: float) -> float | None:
        k = int(board // STATUS_PERIOD_S)
        near = [self.least[j] for j in (k - 1, k, k + 1) if j in self.least]
        if not near:
            near = [self.least[min(self.least, key=lambda j: abs(j - k))]]
        return board + min(near)


SKIPPED = re.compile(r"^HeartbeatSkipped \{ channel: Channel\((\d+)\) \}")
# PROPOSED, not yet logged by the firmware: a line as each transmission
# starts, its channel and its airtime, from the driver.
TX = re.compile(r"^tx ch=(\d+) airtime_us=(\d+)$")


def transmissions(log: Log) -> list[tuple[float, float, int]]:
    """Every transmission of the log as (start, end, channel), on the
    host's clock."""
    out = []
    for boot in log.boots:
        clock = HostClock(boot)
        if not clock:
            continue
        for x in boot.lines:
            m = TX.match(x.text)
            if m:
                t = clock.host(x.board)
                out.append((t, t + int(m.group(2)) / 1e6, int(m.group(1))))
    return out


def heartbeats(logs: list[Log]) -> list[Finding]:
    """A heartbeat skipped for a busy channel while no other board sent on
    it: no transmission of another board on that channel overlaps the
    skip's checks, the LBT_SPAN_S before it.

    Needs every board's transmissions (TX, a line the firmware does not
    log yet) and the host's clock."""
    skips = [(log, x, boot) for log in logs for boot in log.boots for x in boot.lines if SKIPPED.match(x.text)]
    if not skips:
        return []
    if not any(TX.match(x.text) for log in logs for x in log.lines):
        return [Finding("heartbeat", UNCHECKED, "all", f"{len(skips)} skips, and no board logs its transmissions")]
    if not all(log.host_time for log in logs):
        return [Finding("heartbeat", UNCHECKED, "all", "a log without the host's clock")]
    sent = {log.name: transmissions(log) for log in logs}
    out = []
    clocks: dict[int, HostClock] = {}
    for log, x, boot in skips:
        clock = clocks.setdefault(id(boot), HostClock(boot))
        t = clock.host(x.board)
        ch = int(SKIPPED.match(x.text).group(1))
        lo, hi = t - LBT_SPAN_S - CLOCK_SLACK_S, t + CLOCK_SLACK_S
        busy = any(c == ch and s < hi and e > lo for name, txs in sent.items() if name != log.name for s, e, c in txs)
        if not busy:
            out.append(Finding("heartbeat", FAIL, log.name, f"skipped on channel {ch}, which no other board sent on", x.n))
    return out


def standing(
    logs: list[Log],
    asked: dict[str, list[float]] | None = None,
    rooms: dict[str, int] | None = None,
    end: float | None = None,
    silent_at_end: set[str] | frozenset[str] = frozenset(),
) -> list[Finding]:
    """Every standing check over the logs of one run. `asked`: each
    board's resetting actions, host times; `rooms`: each board's stack room;
    `end`: the run's end on the host's clock; `silent_at_end`: the boards
    the run left in their bootloader."""
    out = []
    for log in logs:
        out += resets(log, None if asked is None else asked.get(log.name, []))
        out += radio(log)
        out += stack(log, (rooms or {}).get(log.name))
        out += gaps(log, end, log.name in silent_at_end)
    out += heartbeats(logs)
    return out


def summary(findings: list[Finding], each: int = 5) -> list[str]:
    """The findings by check and board, the first few of each in full."""
    groups: dict[tuple[str, str, str], list[Finding]] = {}
    for f in findings:
        groups.setdefault((f.check, f.board, f.kind), []).append(f)
    out = []
    for (check, board, kind), fs in groups.items():
        word = "FAIL" if kind == FAIL else "unchecked"
        out.append(f"{word} {check} {board}: {len(fs)}")
        out += [f"    {f}" for f in fs[:each]]
        if len(fs) > each:
            out.append(f"    ... {len(fs) - each} more")
    return out
