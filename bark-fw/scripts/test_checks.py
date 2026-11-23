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
    "stack_used={s} unserved=0 pieces_taken=0 update=idle"
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

    def test_a_line_logged_after_the_reboot(self):
        def log(at: float) -> checks.Log:
            return board_log("x", [(1000.0, at, "reboot"), (1001.0, 51.0, "gps: fix"), (1010.0, 0.3, "stored None")])

        self.assertEqual(checks.resets(log(51.0 - checks.REBOOT_LAG_S), []), [])
        self.assertEqual(len(checks.resets(log(50.9 - checks.REBOOT_LAG_S), [])), 1)

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

    def test_a_message_given_back_is_not_dropped(self):
        log = board_log("x", [(0.0, 1.0, "Dropped { msg_id: MsgId(3735928559), reason: GivenBack }")])
        self.assertEqual(checks.radio(log), [])


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

    def noisy(self, *lines: str) -> list[checks.Log]:
        """As `two`, B sending elsewhere, with A's own lines 0.1 s before
        its skip."""
        a, b = self.two("tx ch=4 airtime_us=14400", -0.2)
        skip = (1025.1, 25.0, "HeartbeatSkipped { channel: Channel(3) }")
        own = [(1025.0, 24.9, x) for x in lines]
        a = board_log("a", sorted(uptimes(1010.0, 10.0, 3) + own + [skip], key=lambda x: x[1]))
        return [a, b]

    def test_explained_by_noise_the_boards_checks_read(self):
        logs = self.noisy("lbt ch=3 preamble=false dbm=-84", "lbt ch=3 preamble=false dbm=-85")
        self.assertEqual(checks.heartbeats(logs), [])

    def test_a_check_without_a_reading_explains_nothing(self):
        for lines in [
            ("lbt ch=3 preamble=false dbm=-84", "lbt ch=3 preamble=false dbm=0"),
            ("lbt ch=3 preamble=false dbm=-84", "bench: no reading ch=3 wake_us=1"),
            ("lbt ch=5 preamble=false dbm=-84",),
        ]:
            got = checks.heartbeats(self.noisy(*lines))
            self.assertEqual([f.kind for f in got], [checks.FAIL], lines)

    def test_unchecked_while_no_board_logs_its_frames(self):
        got = checks.heartbeats(self.two(None, 0))
        self.assertEqual([f.kind for f in got], [checks.UNCHECKED])

    def test_the_host_clock_is_the_least_offset_nearby(self):
        log = board_log("x", [(1010.05, 10.0, "a"), (1012.0, 11.0, "b"), (1020.01, 20.0, "c")])
        clock = checks.HostClock(log.boots[0])
        self.assertAlmostEqual(clock.host(15.0), 1015.01)


class Anchor(unittest.TestCase):
    def watches(self, n: int, heard: int) -> checks.Log:
        """A sleepy node's `n` windows a rotation apart, `heard` of them
        with the heartbeat."""
        lines = []
        for i in range(n):
            h = "true, offset_us: -6226" if i < heard else "false, offset_us: 0"
            lines.append((1000.0 + 75 * i, 75.0 * (i + 1), f"AnchorWatch {{ heard: {h}, listened: Duration(30416) }}"))
        return board_log("s", lines)

    def test_one_lost_in_ten_passes(self):
        self.assertEqual(checks.anchor(self.watches(100, 80)), [])

    def test_fewer_caught_than_a_receivers_losses_explain_fails(self):
        got = checks.anchor(self.watches(100, 79))
        self.assertEqual([f.kind for f in got], [checks.FAIL])
        self.assertIn("caught 79 of 100", got[0].what)

    def test_no_windows_no_finding(self):
        self.assertEqual(checks.anchor(board_log("r", uptimes(1000.0, 10.0, 3))), [])

    def test_the_trace_is_the_cores(self):
        src = (HERE.parent.parent / "bark-proto/src/event.rs").read_text()
        body = re.search(r"\n    AnchorWatch \{(.*?)\n    \},", src, re.S).group(1)
        self.assertEqual(re.findall(r"(\w+):", body), ["heard", "offset_us", "listened"])
        self.assertIn("\n    AnchorCheck,", src)


class Clock(unittest.TestCase):
    def boot(self, ppm: float, hours: float, lfclk: str = "crystal, started in 105 ms",
             name: str = "c") -> checks.Log:
        """A boot whose uptime runs `ppm` fast of the host's clock, its
        lines read 1 to 3 ms late."""
        lines = [(1000.002, 0.3, f"lfclk: {lfclk}")]
        for i in range(1, int(hours * 360) + 1):
            host = 1000.0 + 10 * i
            lines.append((host + 0.001 + (i % 3) * 0.001, 0.3 + 10 * i * (1 + ppm * 1e-6), f"uptime {10 * i} s"))
        return board_log(name, lines)

    def test_the_rate_against_the_hosts_clock(self):
        (r,) = checks.rates(self.boot(130, 2))
        self.assertAlmostEqual(r.ppm, 130, delta=1)
        self.assertEqual(r.lfclk, "crystal, started in 105 ms")

    def test_boards_alike_far_from_the_host_pass(self):
        # The crystals of 2026-11-17: +114 and +130 ppm against the host.
        self.assertEqual(checks.clocks([self.boot(114, 2, name="a"), self.boot(130, 2, name="b")]), [])

    def test_a_board_on_the_rc_oscillator_fails_against_the_others(self):
        rc = self.boot(-543, 2, "RC oscillator, calibrated", name="s")
        got = checks.clocks([self.boot(114, 2, name="a"), self.boot(130, 2, name="b"), rc])
        self.assertEqual(sorted(f.board for f in got), ["a", "b", "s", "s"])
        self.assertTrue(all(f.kind == checks.FAIL for f in got))
        (f, _) = [f for f in got if f.board == "s"]
        self.assertIn("-543 ppm against the host, 657 from a board at +114, beyond 200", f.what)

    def test_the_mark_is_twice_the_cores_bound(self):
        self.assertEqual(checks.clocks([self.boot(-95, 2, name="a"), self.boot(95, 2, name="b")]), [])
        # Over 8 h: a line may wait 64 ms in the board's ring, which over 2 h
        # leaves each rate 18 ppm unsure, and 220 apart not surely past 200.
        self.assertEqual(len(checks.clocks([self.boot(-110, 8, name="a"), self.boot(110, 8, name="b")])), 2)

    def test_one_board_alone_is_not_judged(self):
        self.assertEqual(checks.clocks([self.boot(-543, 2, "RC oscillator, calibrated")]), [])

    def test_a_boot_under_an_hour_is_not_judged(self):
        self.assertEqual(checks.rates(self.boot(500, 0.9)), [])

    def test_a_capture_that_was_not_live_is_not_judged(self):
        self.assertEqual(checks.rates(self.boot(200_000, 2)), [])

    def test_the_report_line(self):
        (line,) = checks.rate_lines([self.boot(-543, 2, "RC oscillator, calibrated")], {"c": "S"})
        self.assertTrue(line.startswith("clock S: -543 ppm ±"), line)
        self.assertTrue(line.endswith("RC oscillator, calibrated"), line)

    def test_the_bound_is_the_cores(self):
        src = (HERE.parent.parent / "bark-proto/src/node/relay/anchor.rs").read_text()
        self.assertEqual(int(re.search(r"pub const CLOCK_DRIFT_PPM: u64 = (\d+);", src).group(1)), checks.CLOCK_PPM)


if __name__ == "__main__":
    unittest.main()
