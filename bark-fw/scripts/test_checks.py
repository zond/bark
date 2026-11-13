"""Tests of the standing checks (checks.py): python3 -m unittest, from bark-fw/scripts.

The fixtures are cut from logs of the bench's boards, positions and phone
addresses zeroed; the logs a runner writes, with the host's clock on each
line, are made here."""

from __future__ import annotations

import re
import sys
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import checks  # noqa: E402

FIXTURES = HERE / "fixtures"


def fixture(name: str) -> checks.Log:
    return checks.read(str(FIXTURES / name))


def board_log(name: str, lines: list[tuple[float, float, str]], level: str = "INFO") -> checks.Log:
    """A runner's log from (host time, uptime, text)."""
    return checks.parse([f"{h:.3f} [{b:10.3f} {level} ] {t}\n" for h, b, t in lines], name)


def uptimes(host0: float, board0: float, n: int, every: float = 10.0) -> list[tuple[float, float, str]]:
    return [(host0 + i * every, board0 + i * every, f"uptime {int(board0 + i * every)} s") for i in range(n)]


DRIVER = (
    "driver events=22 frames=0 failed=0 failed_header=0 failed_crc=0 failed_timeout=0 held=0 radio_errors={e} "
    "recoveries=0 sniffs=0 longest_handle_us=488 longest_away_us=214 empty_readies=0 own_readies=0 resets=0 "
    "configured=1 cold_starts=0 tcxo_us=500 tcxo_errors=0 restarts=0 restart_us=0 restart_errors=0 "
    "stack_used={s} unserved=0 update=idle"
)


class Parse(unittest.TestCase):
    def test_a_whole_log_is_one_boot_with_nothing_broken(self):
        log = fixture("clean.log")
        self.assertEqual(len(log.boots), 1)
        self.assertEqual(log.broken, [])
        self.assertFalse(log.host_time)
        self.assertEqual(log.lines[0].text, "display: shown in 2091 ms, asleep")

    def test_two_records_joined_are_two_lines_and_a_break(self):
        log = fixture("start-garbled.log")
        whys = {n: why for n, _, why in log.broken}
        # The first line is cut where the reader opened the port.
        self.assertNotIn(1, whys)
        self.assertEqual(whys[7], "two records on one line, the first cut")
        joined = [x for x in log.lines if x.n == 7]
        self.assertEqual([x.text for x in joined], ["drive", "uptime 130 s"])

    def test_a_panic_message_goes_on_without_a_header(self):
        log = fixture("panic-boot.log")
        self.assertEqual(log.broken, [])
        self.assertEqual(log.boots[-1].kind, "node")
        self.assertEqual(log.boots[-1].version, 9)

    def test_a_boot_starts_at_a_banner_or_where_the_uptime_goes_back(self):
        log = fixture("stack-high.log")
        self.assertEqual(len(log.boots), 3)
        self.assertEqual(log.boots[1].lines[0].text, "HeartbeatSkipped { channel: Channel(7) }")

    def test_the_host_clock_is_read(self):
        log = board_log("x", [(1000.5, 3.25, "uptime 3 s")])
        self.assertEqual((log.lines[0].host, log.lines[0].board), (1000.5, 3.25))


class Driver(unittest.TestCase):
    def test_a_whole_line(self):
        f = checks.driver_fields(DRIVER.format(e=0, s=100))
        self.assertEqual(f["stack_used"], "100")

    def test_the_update_phase_holds_a_space(self):
        f = checks.driver_fields(DRIVER.format(e=0, s=100).replace("update=idle", "update=holds:9 serving:0:0"))
        self.assertEqual(f["update"], "holds:9 serving:0:0")

    def test_a_joined_line_is_refused(self):
        for bad in [
            DRIVER.format(e="04482600", s=100),
            DRIVER.format(e=0, s=100).replace("resets=0", "resets=0 configured=1 cold_starts=0"),
            DRIVER.format(e=0, s=100).replace("stack_used", "stack_uses"),
            "driver events=12 frames=3 failed=2 held=0 radio_errorm=Gps",
        ]:
            self.assertIsNone(checks.driver_fields(bad), bad)

    def test_the_fields_are_those_the_firmware_logs(self):
        src = (HERE.parent / "src/node.rs").read_text()
        fmt = re.search(r'"driver (events=.*?)",\s*s\.events', src, re.S).group(1)
        keys = re.findall(r"(\w+)=\{\}", fmt.replace("\\\n", ""))
        self.assertEqual(keys, checks.DRIVER_FIELDS)


class Gaps(unittest.TestCase):
    def test_none_in_a_whole_log(self):
        self.assertEqual(checks.gaps(fixture("clean.log")), [])

    def test_a_join_and_the_uptime_lines_it_took(self):
        got = [(f.line, f.what) for f in checks.gaps(fixture("start-garbled.log"))]
        self.assertIn((7, "30.0 s since the last uptime line"), got)
        self.assertIn((7, "two records on one line, the first cut"), got)

    def test_the_boards_own_count_of_records_dropped(self):
        log = board_log("x", [(0.0, 60.0, "log: 3 records dropped")], "WARN")
        self.assertEqual([f.what for f in checks.gaps(log)], ["the board dropped 3 records"])

    def test_nothing_read_on_the_host_for_longer_than_the_period(self):
        lines = uptimes(1000.0, 10.0, 3) + [(1030.5, 30.5, "gps: rest")]
        self.assertEqual(checks.gaps(board_log("x", lines)), [])
        late = uptimes(1000.0, 10.0, 2) + [(1021.0, 30.0, "uptime 30 s")]
        self.assertEqual([f.what for f in checks.gaps(board_log("x", late))], ["nothing read for 11.0 s"])

    def test_silent_at_the_end(self):
        log = board_log("x", uptimes(1000.0, 10.0, 3))
        self.assertEqual(checks.gaps(log, end=1030.0), [])
        self.assertEqual(len(checks.gaps(log, end=1040.0)), 1)
        self.assertEqual(checks.gaps(log, end=1040.0, silent_at_end=True), [])


class Resets(unittest.TestCase):
    def boots(self, tail: str) -> checks.Log:
        return board_log("x", [(1000.0, 50.0, "uptime 50 s"), (1001.0, 51.0, tail), (1010.0, 0.3, "stored None")])

    def test_after_the_firmwares_own_reboot(self):
        self.assertEqual(checks.resets(self.boots("reboot"), []), [])

    def test_a_touch_counts_only_where_no_run_says(self):
        log = self.boots("touch: 1200 baud, resetting into the bootloader")
        self.assertEqual(checks.resets(log, None), [])
        self.assertEqual(len(checks.resets(log, [])), 1)

    def test_after_an_action_of_the_host(self):
        log = self.boots("gps: rest")
        self.assertEqual(checks.resets(log, [1005.0]), [])
        self.assertEqual(len(checks.resets(log, [1010.0 - checks.RESET_AFTER_S - 1])), 1)

    def test_a_panic_is_never_asked_for(self):
        got = checks.resets(fixture("panic-boot.log"), [1e12])
        self.assertEqual([f.line for f in got], [81])


class Radio(unittest.TestCase):
    def test_an_error_and_the_counters_it_moved(self):
        got = [(f.line, f.what) for f in checks.radio(fixture("radio-error.log"))]
        self.assertEqual(got, [(19, "radio: NotDone(4)"), (22, "radio_errors=1"), (22, "resets=1")])

    def test_none_in_a_quiet_log(self):
        self.assertEqual(checks.radio(fixture("clean.log")), [])

    def test_a_message_dropped(self):
        log = board_log("x", [(0.0, 1.0, "Dropped { msg_id: MsgId(3735928559), reason: NoRoute }")])
        self.assertEqual([f.what for f in checks.radio(log)], ["message deadbeef dropped, NoRoute"])


class Stack(unittest.TestCase):
    def test_under_the_floor_that_packing_guarantees(self):
        got = checks.stack(fixture("stack-high.log"))
        self.assertEqual(len(got), 3)
        self.assertIn("35748 B used of 24576 B", got[0].what)

    def test_within_the_room_the_image_leaves(self):
        self.assertEqual(checks.stack(fixture("stack-high.log"), room=63384), [])
        log = board_log("x", [(0.0, 10.0, DRIVER.format(e=0, s=63384 - checks.STACK_MARGIN + 1))])
        self.assertEqual(len(checks.stack(log, room=63384)), 1)


class Heartbeats(unittest.TestCase):
    def two(self, tx: str | None, offset: float) -> list[checks.Log]:
        """A skips at 1025 s on the host's clock, its uptime 25 s; B, up
        for 500 s more, sends `offset` from then, the line read late."""
        skip = (1025.1, 25.0, "HeartbeatSkipped { channel: Channel(3) }")
        a = board_log("a", sorted(uptimes(1010.0, 10.0, 3) + [skip], key=lambda x: x[1]))
        b_lines = uptimes(1010.0, 510.0, 3)
        if tx is not None:
            b_lines.append((1030.0, 525.0 + offset, tx))
        b = board_log("b", sorted(b_lines, key=lambda x: x[1]))
        return [a, b]

    def test_explained_by_another_boards_frame_on_the_channel(self):
        logs = self.two("tx ch=3 airtime_us=14400", -0.2)
        self.assertEqual(checks.heartbeats(logs), [])

    def test_a_frame_elsewhere_or_long_before_explains_nothing(self):
        for tx, offset in [("tx ch=4 airtime_us=14400", -0.2), ("tx ch=3 airtime_us=14400", -1.0)]:
            got = checks.heartbeats(self.two(tx, offset))
            self.assertEqual([f.kind for f in got], [checks.FAIL], tx)

    def test_unchecked_while_no_board_logs_its_frames(self):
        got = checks.heartbeats(self.two(None, 0))
        self.assertEqual([f.kind for f in got], [checks.UNCHECKED])

    def test_the_host_clock_is_the_least_offset_nearby(self):
        log = board_log("x", [(1010.05, 10.0, "a"), (1012.0, 11.0, "b"), (1020.01, 20.0, "c")])
        clock = checks.HostClock(log.boots[0])
        self.assertAlmostEqual(clock.host(15.0), 1015.01)


if __name__ == "__main__":
    unittest.main()
