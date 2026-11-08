#!/usr/bin/env python3
"""Resets at chosen moments of swaps on one board (roadmap Phase 2, "Resets at chosen moments of a swap").

Python 3 and its `cryptography` package, to sign the images with the
bench key; run from bark-fw/, the board on USB:

    scripts/cutbench.py ID
    scripts/cutbench.py --list

ID is the board's node ID, twelve hex digits or its last four. The board
gets the bench loader and a console image of version 1, as from
scripts/swapbench.py. Then one round for each moment of `--list`, in
turn, each staged with scripts/image.py, which sends `cut swap N|none`
and `cut back N|none` after `commit` and before `reboot`:

    swap   a console image one version up, its swap stopped in operation
           N: the next boot ends the swap, and the new version confirms;
    back   a `panic` image one version up, its swap not stopped and its
           swap back stopped in operation N: the previous version is back.

A moment is an operation's index in the phase's erases and writes on both
flashes at the boot that starts it, each page of an erase one, as the
bench loader counts them and as bark-image's host test counts its power
cuts (bark_image::boot::cut::operations); the loader does that operation
half way and resets. The moments walked are the operations around the
progress words of each phase's first and last pair of copies, the one
before each word and the one after, and those outside the copies: the
record's word before the swap, and the state page's word, erase and word
after the swap back. The list is derived here from the layout, read from
the firmware's ELF, and embassy-boot's order of operations; bark-image's
test `the_bench_script_names_its_moments_as_the_host_counts_them` runs
`--list --layout` and checks each against the host's count.

A round passes when the version expected runs and the firmware logs the
reset in the operation named, on its flash, erase or write, and page, and
the other phase's as not reached. The first round that fails stops the
run and says the moment and what was seen. Every line the board sends goes
to target/cutbench/<time>.log, after the host's clock.
"""

from __future__ import annotations

import argparse
import os
import re
import sys
import time
from dataclasses import dataclass

sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import bench  # noqa: E402  (beside this script: the board's ID)
import image  # noqa: E402  (beside this script: stage)
import swapbench  # noqa: E402  (beside this script: the builds, the setup, the boots)
import uf2  # noqa: E402  (beside this script: the ELF's symbols)

OUT = bench.FW / "target/cutbench"

# The erase unit, the internal flash's page and the external flash's
# sector alike (nRF52840 Product Specification, NVMC; bark-image/src/layout.rs).
PAGE = 4096
# The staged slot starts the external flash (bark-image/src/layout.rs,
# external::DFU), one sector longer than the running slot.
DFU_START = 0

CUT = re.compile(r"update: bench reset in the (swap|swap back) at operation (\d+): (.*)$")
FELL = re.compile(r"(internal|external) (erase|write) of page (\d+), [\d.]+ % through")


@dataclass(frozen=True)
class Op:
    side: str  # internal or external
    kind: str  # erase or write
    page: int  # on that flash
    word: int | None = None  # the progress word it writes, if it does
    what: str = ""


@dataclass(frozen=True)
class Moment:
    phase: str  # swap or back
    n: int
    op: Op
    why: str

    def line(self) -> str:
        """As `--list` prints it, and bark-image's test reads it."""
        return f"{self.phase} {self.n} {self.op.side} {self.op.kind} {self.op.page}: {self.op.what}, {self.why}"


def operations(phase: str, active: tuple[int, int], state: int, record: int) -> list[Op]:
    """The erases and writes of `phase` at the boot that starts it, in
    embassy-boot's order (`BootLoader::prepare_boot`, read at 0.7.0): the
    swap copies the running slot's pages from the last, each out a sector
    up in the staged slot and the staged page in, and the swap back each
    page from the first, out to its own sector and the previous image in
    from the sector above. A copy is an erase, a page's write and its
    progress word, the state page's word 2 + its index."""
    pages = (active[1] - active[0]) // PAGE
    first, state_page = active[0] // PAGE, state // PAGE
    out: list[Op] = []

    def copy(index: int, side: str, page: int, what: str) -> None:
        out.append(Op(side, "erase", page, what=f"erase of {what}"))
        out.append(Op(side, "write", page, what=f"write of {what}"))
        out.append(Op("internal", "write", state_page, index, f"progress word {index}"))

    if phase == "swap":
        out.append(Op("internal", "write", record // PAGE, what="the record's word that notes the image checked"))
        for j in range(pages):
            copy(2 * j, "external", DFU_START // PAGE + pages - j, f"staged sector {pages - j}")
            copy(2 * j + 1, "internal", first + pages - 1 - j, f"running page {pages - 1 - j}")
    else:
        for j in range(pages):
            copy(2 * pages + 2 * j, "external", DFU_START // PAGE + j, f"staged sector {j}")
            copy(2 * pages + 2 * j + 1, "internal", first + j, f"running page {j}")
        out.append(Op("internal", "write", state_page, what="the state page's word that marks the progress invalid"))
        out.append(Op("internal", "erase", state_page, what="the state page's erase"))
        out.append(Op("internal", "write", state_page, what="the state page's word that ends the swap back"))
    return out


def moments(layout: tuple[tuple[int, int], int, int]) -> list[Moment]:
    """The moments walked, the swap's then the swap back's."""
    found = []
    for phase in ("swap", "back"):
        ops = operations(phase, *layout)
        words = [i for i, op in enumerate(ops) if op.word is not None]
        chosen: dict[int, str] = {}
        # Around each progress word of the first pair of copies and the
        # last: the copy whole but not marked, and marked but the next not
        # whole. The swap's last word has none after it at this boot, so
        # it is taken itself.
        for w in words[:2] + words[-2:]:
            word = ops[w].word
            chosen.setdefault(w - 1, f"before progress word {word}")
            if w + 1 < len(ops):
                chosen.setdefault(w + 1, f"after progress word {word}")
            else:
                chosen.setdefault(w, f"the {phase}'s last operation")
        # Outside the copies: before the first copy, whose erase is two
        # operations before its progress word, and after the last.
        for i in range(len(ops)):
            if i < words[0] - 2:
                chosen.setdefault(i, "before the copies")
            elif i > words[-1]:
                chosen.setdefault(i, "after the copies")
        found += [Moment(phase, i, ops[i], chosen[i]) for i in sorted(chosen)]
    return found


def elf_layout() -> tuple[tuple[int, int], int, int]:
    """The running slot, and the state page's and the record's starts, from
    the firmware's ELF."""
    if not swapbench.ELF.exists():
        sys.exit(f"no {swapbench.ELF}: build the firmware first, or give --layout")
    syms = uf2.symbols(swapbench.ELF.read_bytes())
    active = uf2.region(syms, "__bootloader_active")
    return active, uf2.region(syms, "__bootloader_state")[0], uf2.region(syms, "__bark_record")[0]


def fell(lines: list[str]) -> dict[str, tuple[int, str]]:
    """Each phase's chosen moment and where it fell, from the last boot's
    record: `swap` and `back` to (operation, what the firmware says)."""
    out = {}
    for x in lines:
        m = CUT.search(x)
        if m:
            phase, n, where = m.groups()
            out["back" if phase == "swap back" else "swap"] = (int(n), where)
    return out


def round_(m: Moment, node: str, running: int, log) -> tuple[bool, int, dict]:
    """The round for moment `m` on a board running `running`: whether it
    went as expected, the version running after it, and what it saw."""
    kind = "up" if m.phase == "swap" else "panic"
    version = running + 1
    img = swapbench.build_image(kind, version)
    log.write(f"# {m.line()}\n")
    other = "back" if m.phase == "swap" else "swap"
    image.stage(str(img), swapbench.PORT.format(node), True, after=(f"cut {m.phase} {m.n}", f"cut {other} none"))
    t0 = time.monotonic()
    sessions = swapbench.boots(node, t0 + swapbench.ROUND_S, log, swapbench.GONE_S)
    row = {"moment": m, "version": version}
    if not sessions or not any(swapbench.READY in x for x in sessions[-1]["lines"]):
        row["why"] = f"no console within {swapbench.ROUND_S:.0f} s of `reboot`"
        return False, running, row
    last = sessions[-1]
    row["port_s"] = last["t"] - t0
    row["boots"] = len(sessions)
    runs = swapbench.banner(last)
    text = "\n".join(last["lines"])
    seen = fell(last["lines"])
    row["seen"] = seen
    if kind == "up":
        ok = runs == version and f"update: version {version} confirmed" in text
        want = f"version {version} confirmed"
        after = version if ok else running
    else:
        ok = runs == running and not swapbench.CONFIRMED.search(text)
        want = f"version {running} back"
        after = running
    why = []
    if not ok:
        why.append(f"expected {want}, version {runs} runs")
    at = seen.get(m.phase)
    if at is None:
        why.append(f"no bench reset logged for the {m.phase}: is the bench loader on the board?")
    else:
        n, where = at
        f = FELL.fullmatch(where)
        named = (m.op.side, m.op.kind, m.op.page)
        if n != m.n or f is None or (f[1], f[2], int(f[3])) != named:
            why.append(f"expected the reset at operation {m.n} in the {' '.join(map(str, named))}, logged at {n}: {where}")
    if seen.get(other, (None, "not reached"))[1] != "not reached":
        why.append(f"the {other} was reset too, at {seen[other][0]}: {seen[other][1]}")
    if why:
        row["why"] = "; ".join(why)
    return not why, after, row


def show(i: int, row: dict) -> str:
    m = row["moment"]
    head = f"{i:3} {m.phase:<4} op {m.n:<5} {m.op.side} {m.op.kind} of page {m.op.page} ({m.why})"
    if "port_s" not in row:
        return f"{head}  FAIL: {row['why']}"
    verdict = f"FAIL: {row['why']}" if "why" in row else "ok"
    return f"{head}  {row['port_s']:.1f} s, {row['boots']} boots  {verdict}"


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("id", nargs="?", help="the board's node ID, twelve hex digits or its last four")
    ap.add_argument("--list", action="store_true", help="print the moments walked and stop")
    ap.add_argument(
        "--layout",
        nargs=4,
        type=int,
        metavar=("ACTIVE_START", "ACTIVE_END", "STATE_START", "RECORD_START"),
        help="with --list: the layout, instead of the built firmware's",
    )
    a = ap.parse_args()
    if a.list:
        layout = elf_layout() if a.layout is None else ((a.layout[0], a.layout[1]), a.layout[2], a.layout[3])
        for m in moments(layout):
            print(m.line())
        return
    if a.id is None:
        ap.error("the board's ID, or --list")
    node = bench.full_id(a.id)
    swapbench.OUT.mkdir(parents=True, exist_ok=True)
    for old in swapbench.OUT.glob("*.img"):
        old.unlink()
    OUT.mkdir(parents=True, exist_ok=True)
    path = OUT / time.strftime("%Y-%m-%d-%H%M%S.log")
    print(f"log: {path}", flush=True)
    with open(path, "a", buffering=1) as log:
        swapbench.setup(node, log)
        walk = moments(elf_layout())
        print(f"{len(walk)} moments: {sum(m.phase == 'swap' for m in walk)} in the swap, "
              f"{sum(m.phase == 'back' for m in walk)} in the swap back", flush=True)
        running = 1
        for i, m in enumerate(walk, 1):
            ok, running, row = round_(m, node, running, log)
            print(show(i, row), flush=True)
            log.write(f"# {show(i, row)}\n")
            if not ok:
                sys.exit(1)
    print(f"all {len(walk)} moments: the expected version ran after each")


if __name__ == "__main__":
    main()
