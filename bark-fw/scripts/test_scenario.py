"""Tests of the scenario files and their judging (scenario.py): python3 -m unittest, from bark-fw/scripts.

No board is touched: runs are made here as the runner writes them, a log
for each board, run.json and actions.jsonl."""

from __future__ import annotations

import json
import re
import sys
import tempfile
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import scenario  # noqa: E402

SCENARIOS = sorted((HERE.parent / "bench").glob("*.toml"))
A = "bae6e6cbf4f5"
C = "7572482fc367"


def write(path: Path, text: str) -> Path:
    path.write_text(text)
    return path


def bad(text: str) -> str:
    with tempfile.TemporaryDirectory() as d:
        with unittest.TestCase().assertRaises(scenario.Bad) as e:
            scenario.load(write(Path(d) / "s.toml", text))
        return str(e.exception)


BOARDS = '[[boards]]\nname = "A"\n[[boards]]\nname = "C"\n'


class Files(unittest.TestCase):
    def test_every_scenario_loads_and_its_lines_compile(self):
        self.assertGreaterEqual(len(SCENARIOS), 7)
        for path in SCENARIOS:
            s = scenario.load(path)
            ids = {b.name: f"{i:012x}" for i, b in enumerate(s.boards, 1)}
            vars = scenario.board_vars(ids, {b.name: 7 for b in s.boards}, s.boards)
            for a in s.actions:
                for g in re.findall(r"\(\?P<(\w+)>", a.expect or ""):
                    vars[f"{a.name}.{g}"] = "0a0b0c0d" if g == "id" else "1, 2, 3, 4, 5, 6"

            def conditions(c):
                yield from (x for sub in c.get("any", []) for x in conditions(sub))
                if "line" in c:
                    yield c

            for c in [x for p in s.passes for x in conditions(p)]:
                filled = scenario.fill(c["line"], vars, escape=True)
                self.assertIsNotNone(filled, f"{path.name}: {c['line']}")
                re.compile(filled)
            for _, a, board in scenario.timeline(s):
                if a.command:
                    self.assertIsNotNone(scenario.fill(a.command, vars, escape=False), f"{path.name}: {a.command}")

    def test_mistakes_are_named(self):
        self.assertIn("unknown colour", bad('duration_s = 1\ncolour = 1\n' + BOARDS))
        self.assertIn("deaf to no board", bad('duration_s = 1\n[[boards]]\nname = "A"\ndeaf_to = "Q"\n'))
        self.assertIn("none the runner sets", bad('duration_s = 1\n[[boards]]\nname = "A"\nenv = { BARK_IGNORE = "x" }\n'))
        self.assertIn("no board Q", bad(f'duration_s = 1\n{BOARDS}[[pass]]\nboard = "Q"\nline = "x"\n'))
        self.assertIn("after names no action", bad(f'duration_s = 1\n{BOARDS}[[pass]]\nboard = "A"\nline = "x"\nafter = "z"\n'))
        self.assertIn("a command goes with say", bad(f'duration_s = 1\n{BOARDS}[[actions]]\nat_s = 1\nboard = "A"\ndo = "say"\n'))
        self.assertIn("needs a group", bad(f'duration_s = 1\n{BOARDS}[[pass]]\nboard = "A"\nkept = {{ before = "x", after = "y" }}\n'))


class Times(unittest.TestCase):
    def test_a_repeated_action_at_drawn_times_within_its_range(self):
        with tempfile.TemporaryDirectory() as d:
            s = scenario.load(write(Path(d) / "s.toml", f"""duration_s = 100.0
{BOARDS}
[[actions]]
at_s = 10.0
every_s = [5.0, 15.0]
until_s = 90.0
boards = ["A", "C"]
do = "off"
"""))
        t = [e[0] for e in scenario.timeline(s)]
        self.assertEqual(t[0], 10.0)
        self.assertTrue(all(5.0 <= b - a <= 15.0 for a, b in zip(t, t[1:])))
        self.assertLessEqual(t[-1], 90.0)
        self.assertEqual(t, [e[0] for e in scenario.timeline(s)])

    def test_variables_filled_escaped_and_as_decimal(self):
        vars = {"ac.id": "deadbeef", "A.id_dec": "186, 230"}
        self.assertEqual(scenario.fill("MsgId(${ac.id|dec})", vars, escape=False), "MsgId(3735928559)")
        self.assertEqual(scenario.fill(r"\[${A.id_dec}\]", vars, escape=True), r"\[186,\ 230\]")
        self.assertIsNone(scenario.fill("${zz.id}", vars, escape=False))
        self.assertEqual(scenario.id_dec(A), "186, 230, 230, 203, 244, 245")

    def test_versions_never_given_twice(self):
        with tempfile.TemporaryDirectory() as d:
            old = scenario.VERSION_FILE
            scenario.VERSION_FILE = Path(d) / "image-version"
            try:
                with self.assertRaises(SystemExit):
                    scenario.next_versions(2, None)
                self.assertEqual(scenario.next_versions(2, 50), [50, 51])
                self.assertEqual(scenario.next_versions(1, None), [52])
                with self.assertRaises(SystemExit):
                    scenario.next_versions(1, 52)
            finally:
                scenario.VERSION_FILE = old


def line(host: float, board: float, text: str) -> str:
    return f"{host:.3f} [{board:10.3f} INFO ] {text}\n"


class Judge(unittest.TestCase):
    """A2's first way on a run made here: A sends, C's application has it."""

    def run_dir(self, d: Path, at_c: float | None) -> Path:
        s = write(d / "s.toml", f"""duration_s = 60.0
{BOARDS}
[[actions]]
at_s = 10.0
board = "A"
do = "say"
command = "text ${{C.id}} hi"
name = "ac"
expect = '^text id=(?P<id>[0-9a-f]{{8}}) '

[[pass]]
board = "C"
line = '^message id=${{ac.id}}$'
after = "ac"
within_s = 6.3
""")
        start = 1000.0
        a = [line(start + t, 5.0 + t, f"uptime {int(5 + t)} s") for t in range(0, 61, 10)]
        a.append(line(start + 10.1, 15.1, "text id=0a0b0c0d to 7572482fc367 len=2 taken"))
        c = [line(start + t, 7.0 + t, f"uptime {int(7 + t)} s") for t in range(0, 61, 10)]
        if at_c is not None:
            c.append(line(start + at_c, 7.0 + at_c, "message id=0a0b0c0d"))
        write(d / f"{A}.log", "".join(sorted(a, key=lambda x: float(x.split()[2].rstrip("]")))))
        write(d / f"{C}.log", "".join(sorted(c, key=lambda x: float(x.split()[2].rstrip("]")))))
        meta = {"scenario": str(s), "ids": {"A": A, "C": C}, "versions": {"A": 1, "C": 2},
                "stack_rooms": {"A": 63384, "C": 63384}, "silent_at_end": [], "setup": start - 1, "end": start + 60.0}
        write(d / "run.json", json.dumps(meta))
        events = [
            {"t": start - 1, "board": "A", "do": "on", "resets": True},
            {"t": start - 1, "board": "C", "do": "on", "resets": True},
            {"t": start + 10.0, "board": "A", "do": "say", "name": "ac", "vars": {"id": "0a0b0c0d"}},
        ]
        write(d / "actions.jsonl", "".join(json.dumps(e) + "\n" for e in events))
        return d

    def test_delivered_in_time(self):
        with tempfile.TemporaryDirectory() as d:
            ok, report = scenario.judge_dir(self.run_dir(Path(d), 13.0), None, {}, None)
        self.assertTrue(ok, report)
        self.assertEqual(report[-1], "pass 1: C: 1 after 3.0 s")

    def test_late_or_never(self):
        for at in (17.0, None):
            with tempfile.TemporaryDirectory() as d:
                ok, report = scenario.judge_dir(self.run_dir(Path(d), at), None, {}, None)
            self.assertFalse(ok)
            self.assertTrue(report[-1].startswith("FAIL 1: C: 0 of"), report)

    def test_an_action_that_failed_fails_the_run(self):
        with tempfile.TemporaryDirectory() as d:
            p = self.run_dir(Path(d), 13.0)
            with open(p / "actions.jsonl", "a") as f:
                f.write(json.dumps({"t": 1020.0, "board": "A", "failed": "no /x/ within 10 s"}) + "\n")
            ok, report = scenario.judge_dir(p, None, {}, None)
        self.assertFalse(ok)
        self.assertEqual(report[0], "FAIL action A: no /x/ within 10 s")

    def test_unchecked_without_the_host_clock(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d)
            s = self.run_dir(p, 13.0) / "s.toml"
            for f in (p / "run.json", p / "actions.jsonl"):
                f.unlink()
            for f in p.glob("*.log"):
                f.write_text("".join(x.split(" ", 1)[1] for x in f.read_text().splitlines(keepends=True)))
            ok, report = scenario.judge_dir(p, str(s), {"A": "f4f5", "C": "c367"}, None)
        self.assertFalse(ok)
        self.assertTrue(report[-1].startswith("unchecked 1: no time for the action ac"), report)


class Kept(unittest.TestCase):
    def test_compared_across_each_reboot(self):
        rule = {"before": r"home: Channel\((?P<v>\d+)\)", "after": r"^stored home: Channel\((?P<v>\d+)\)"}
        text = [
            "[ 1.000 INFO ] stored home: Channel(4)",
            "[ 9.000 INFO ] store home: Channel(5)",
            "[ 1.000 INFO ] stored home: Channel(5)",
            "[ 9.000 INFO ] reboot",
            "[ 1.000 INFO ] stored home: Channel(6)",
        ]
        log = scenario.checks.parse(text[:3], "x")
        self.assertEqual(scenario.kept(rule, log, None, None), ("pass", "1 reboots"))
        log = scenario.checks.parse(text, "x")
        kind, what = scenario.kept(rule, log, None, None)
        self.assertEqual((kind, what), ("fail", "line 5: 5 before the reboot, 6 after"))
        self.assertEqual(scenario.kept({**rule, "unless": "^reboot$"}, log, None, None), ("pass", "1 reboots"))


if __name__ == "__main__":
    unittest.main()
