//! The radio's oscillator start, probed (roadmap D88).
//!
//! The SX1262 powers the board's TCXO each time it leaves STDBY_RC or
//! sleep, waits out a programmed delay, and only then goes on; it has no
//! ready signal, but sets `XOSC_START_ERR` (`GetDeviceErrors`, SX1261/2
//! datasheet §13.3.6) when the oscillator was not up by the delay's end.
//! A delay of zero hangs the chip. So the board measures its own start:
//! a binary search over the delay at boot, each probe a cold start with
//! the error word read after it, then confirmation probes at the chosen
//! delay, since the flag flickers at the edge, plus a margin; in use the
//! flag read after every cold start, the delay stepped up at once when it
//! is set, which covers a board getting cold; and a probe below the delay
//! now and then as the die's temperature moves, which recovers the margin
//! as it warms (`docs/bench-2026-10-30.md`, "The oscillator's start,
//! probed").
//!
//! Restarted within a millisecond of stopping, the oscillator is still
//! running and no delay is too short, so the restart after a CAD, which
//! ends in STDBY_RC a few hundred microseconds before the next command,
//! is programmed short ([`SHORT`]); every other start is a cold one and
//! takes the probed delay.

use crate::{Duration, Instant};

/// The search's step. The bench probed the flag at 50, 100, 200 and
/// 500 µs: it flickered between 50 and 200 and never showed at 500, so
/// the start is known to about a quarter millisecond and a finer step
/// would resolve nothing. A step this wide also holds the whole band in
/// which the flag flickers, so one step of margin clears it.
pub const STEP: Duration = Duration::from_micros(250);

/// The shortest delay probed: one step. Zero hangs the chip.
pub const MIN: Duration = STEP;

/// The longest delay probed, 16 steps: a factor eight over the 500 µs the
/// bench found at room temperature, where roadmap D87's 2 ms kept a
/// factor four; a cold board is unmeasured. A board whose oscillator
/// takes longer runs at this delay with the error flag set, which costs
/// a few dB at the start of each frame, not the frame.
pub const MAX: Duration = Duration::from_micros(16 * STEP.as_micros());

/// The margin over the lowest delay that passed its confirmation probes:
/// one step, the width of the band in which the flag flickers.
pub const MARGIN: Duration = STEP;

/// Probes at a delay after the search lands on it, all of which must pass,
/// since at the edge about half the probes fail: four passes at a delay
/// where half fail happen one time in sixteen.
pub const CONFIRM: u8 = 4;

/// The delay programmed for a restart within [`STILL_RUNNING`] of the
/// oscillator stopping, where the bench saw no error at 50 µs in 90
/// probes: twice that, since zero hangs the chip and the cost of a step
/// is a few microseconds.
pub const SHORT: Duration = Duration::from_micros(100);

/// How long after stopping the oscillator still runs: a restart within a
/// millisecond needed no delay on the bench, and one after a millisecond
/// was a cold start.
pub const STILL_RUNNING: Duration = Duration::from_millis(1);

/// How long a probe keeps the oscillator off before its start: twice
/// [`STILL_RUNNING`], so that every probe is a cold start.
pub const COLD_WAIT: Duration = Duration::from_millis(2);

/// The least time between probes in use. A probe deafens the radio for a
/// few milliseconds; once an hour it costs nothing, and the delay only
/// has to follow the day's temperature.
pub const REPROBE_FLOOR: Duration = Duration::from_secs(3600);

/// How far the die's temperature must have moved since the last probe,
/// in the nRF52840 TEMP peripheral's quarter degrees: 3 °C. The sensor is
/// uncalibrated and only its change is used; its resolution is a quarter
/// degree, so a move of twelve steps is a real one, and a start time that
/// a move this small would change is within the margin anyway. No bench
/// number sets it: the in-use check steps the delay up on any error,
/// whatever the temperature did, so this only paces the recovery.
pub const REPROBE_TEMP_QUARTERS: i16 = 12;

/// How often the die's temperature is read once the floor has passed.
pub const TEMP_EVERY: Duration = Duration::from_secs(60);

/// A delay in whole steps.
fn steps(d: Duration) -> u8 {
    (d.as_micros() / STEP.as_micros()) as u8
}

fn delay(steps: u8) -> Duration {
    Duration::from_micros(u64::from(steps) * STEP.as_micros())
}

/// Where a search is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// Binary search between the two steps, inclusive, for the lowest
    /// that passes.
    Between(u8, u8),
    /// Confirmation probes at this step, this many left.
    Confirm(u8, u8),
    /// The outcome: the lowest step that passed its confirmation, or
    /// `None` if none did up to [`MAX`].
    Done(Option<u8>),
}

/// A search for the lowest delay that starts the oscillator: at boot a
/// binary search over [`MIN`]..=[`MAX`], in use a confirmation at one
/// step. The caller probes the delay [`Search::next`] asks for and
/// reports [`Search::result`], until [`Search::outcome`] is `Some`.
#[derive(Clone, Copy, Debug)]
pub struct Search {
    phase: Phase,
}

impl Search {
    /// The search at boot.
    pub fn at_boot() -> Search {
        Search {
            phase: Phase::Between(steps(MIN), steps(MAX)),
        }
    }

    /// The confirmation probes at `d` alone, in use.
    pub fn confirm(d: Duration) -> Search {
        Search {
            phase: Phase::Confirm(steps(d).max(steps(MIN)), CONFIRM),
        }
    }

    /// The delay to probe next, or `None` once the search is over.
    pub fn next(&self) -> Option<Duration> {
        match self.phase {
            Phase::Between(lo, hi) => Some(delay(lo.midpoint(hi))),
            Phase::Confirm(at, _) => Some(delay(at)),
            Phase::Done(_) => None,
        }
    }

    /// The probe at the delay [`Search::next`] asked for started the
    /// oscillator without an error, or not.
    pub fn result(&mut self, ok: bool) {
        let max = steps(MAX);
        self.phase = match self.phase {
            Phase::Between(lo, hi) => {
                let mid = lo.midpoint(hi);
                let (lo, hi) = if ok { (lo, mid) } else { (mid + 1, hi) };
                if lo < hi {
                    Phase::Between(lo, hi)
                } else if lo > max {
                    Phase::Done(None)
                } else {
                    Phase::Confirm(lo, CONFIRM)
                }
            }
            Phase::Confirm(at, left) => match (ok, left) {
                (true, 1) => Phase::Done(Some(at)),
                (true, n) => Phase::Confirm(at, n - 1),
                // One failure and the step is not safe: the next one up,
                // confirmed afresh.
                (false, _) if at < max => Phase::Confirm(at + 1, CONFIRM),
                (false, _) => Phase::Done(None),
            },
            Phase::Done(d) => Phase::Done(d),
        };
    }

    /// The lowest delay that passed its confirmation, once the search is
    /// over: `Some(None)` if none did up to [`MAX`].
    pub fn outcome(&self) -> Option<Option<Duration>> {
        match self.phase {
            Phase::Done(d) => Some(d.map(delay)),
            _ => None,
        }
    }
}

/// The last probe made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Probe {
    /// The delay it tried.
    pub delay: Duration,
    /// Whether the oscillator started without an error.
    pub ok: bool,
    /// When.
    pub at: Instant,
    /// The die's temperature then, quarter degrees, if the board reads it.
    pub temp: Option<i16>,
}

/// The delay in use and how it got there.
#[derive(Clone, Copy, Debug)]
pub struct Delay {
    /// The delay cold starts are programmed with: the lowest confirmed
    /// delay plus [`MARGIN`].
    pub delay: Duration,
    /// Probes made since boot.
    pub probes: u32,
    /// Cold starts in use that set the error flag.
    pub errors: u32,
    /// The last probe.
    pub last: Option<Probe>,
    /// When the delay was last probed or stepped, and the die's
    /// temperature then.
    settled_at: Instant,
    settled_temp: Option<i16>,
    /// When the die's temperature was last read for the schedule.
    temp_read_at: Instant,
}

impl Delay {
    /// Before the search: [`MAX`], safe for the chip's calibration.
    pub fn new(now: Instant) -> Delay {
        Delay {
            delay: MAX,
            probes: 0,
            errors: 0,
            last: None,
            settled_at: now,
            settled_temp: None,
            temp_read_at: now,
        }
    }

    /// Note a probe made.
    pub fn probed(&mut self, probe: Probe) {
        self.probes += 1;
        self.last = Some(probe);
    }

    /// The search's outcome, at boot or in use: the delay is the lowest
    /// confirmed delay plus the margin, or [`MAX`] if none was confirmed.
    pub fn settle(&mut self, lowest: Option<Duration>, now: Instant, temp: Option<i16>) {
        self.delay = match lowest {
            Some(d) => Duration::from_micros(d.as_micros() + MARGIN.as_micros()).min(MAX),
            None => MAX,
        };
        self.settled_at = now;
        self.settled_temp = temp;
    }

    /// A cold start in use set the error flag: one step up at once.
    pub fn step_up(&mut self, now: Instant, temp: Option<i16>) {
        self.errors += 1;
        self.delay = Duration::from_micros(self.delay.as_micros() + STEP.as_micros()).min(MAX);
        self.settled_at = now;
        self.settled_temp = temp;
    }

    /// The delay a probe below the one in use tests: two steps down, so
    /// that a pass lowers the delay by one and keeps the margin. `None`
    /// at the bottom.
    pub fn probe_below(&self) -> Option<Duration> {
        let below = self.delay.as_micros().checked_sub(2 * STEP.as_micros())?;
        (below >= MIN.as_micros()).then(|| Duration::from_micros(below))
    }

    /// Whether the die's temperature is to be read now for the schedule:
    /// not before the floor, then every [`TEMP_EVERY`].
    pub fn temp_due(&self, now: Instant) -> bool {
        now >= self.settled_at + REPROBE_FLOOR && now >= self.temp_read_at + TEMP_EVERY
    }

    /// Whether a probe below the delay is due, given the die's
    /// temperature read now: the floor has passed and the temperature has
    /// moved by [`REPROBE_TEMP_QUARTERS`] since the delay settled, or was
    /// never read then.
    pub fn probe_due(&mut self, now: Instant, temp: i16) -> bool {
        self.temp_read_at = now;
        if now < self.settled_at + REPROBE_FLOOR || self.probe_below().is_none() {
            return false;
        }
        match self.settled_temp {
            Some(t) => (temp - t).abs() >= REPROBE_TEMP_QUARTERS,
            None => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn us(n: u64) -> Duration {
        Duration::from_micros(n)
    }

    /// Run a search against a board whose oscillator starts in `start`,
    /// the flag flickering (failing every other probe) within `flicker`
    /// below it. Returns the outcome and the probes made.
    fn run(mut s: Search, start: Duration, flicker: Duration) -> (Option<Duration>, Vec<Duration>) {
        let mut probes = Vec::new();
        let mut toggle = false;
        while let Some(d) = s.next() {
            probes.push(d);
            let ok = if d >= start {
                true
            } else if d.as_micros() + flicker.as_micros() >= start.as_micros() {
                toggle = !toggle;
                toggle
            } else {
                false
            };
            s.result(ok);
        }
        (s.outcome().expect("over"), probes)
    }

    #[test]
    fn the_steps_are_what_the_bench_resolved() {
        assert_eq!(STEP, us(250));
        assert_eq!(MAX, ms(4));
        assert_eq!(steps(MAX), 16);
        assert_eq!(delay(steps(MAX)), MAX);
        assert!(SHORT < STILL_RUNNING && SHORT > Duration::ZERO);
    }

    #[test]
    fn a_room_temperature_board_lands_one_step_up_from_the_lowest_pass() {
        // The bench's board: starts within 500 µs, flickers below 200.
        let (lowest, probes) = run(Search::at_boot(), us(250), us(150));
        assert_eq!(lowest, Some(us(250)));
        // Four binary probes, then four confirmations at the answer.
        assert_eq!(probes.len(), 4 + CONFIRM as usize);
        assert!(probes[4..].iter().all(|&d| d == us(250)));
        let mut d = Delay::new(Instant::ZERO);
        d.settle(lowest, Instant::ZERO, Some(100));
        assert_eq!(d.delay, us(500));
    }

    #[test]
    fn a_slow_board_lands_above_its_start() {
        for start in [us(750), us(1250), ms(2), us(3250), ms(4)] {
            let (lowest, _) = run(Search::at_boot(), start, us(150));
            let lowest = lowest.expect("found");
            assert!(lowest >= start, "{start:?}: {lowest:?}");
            assert!(lowest.as_micros() < start.as_micros() + STEP.as_micros());
        }
    }

    #[test]
    fn a_flickering_edge_is_climbed_by_the_confirmation() {
        // The oscillator takes 1.1 ms; the flag flickers from 0.6 ms up.
        // A lucky binary probe at 1.0 ms may pass; the confirmation then
        // sees a failure and climbs to 1.25 ms.
        let (lowest, probes) = run(Search::at_boot(), us(1100), us(500));
        assert_eq!(lowest, Some(us(1250)), "{probes:?}");
    }

    #[test]
    fn a_board_that_never_passes_gets_the_longest_delay() {
        let (lowest, _) = run(Search::at_boot(), ms(10), us(150));
        assert_eq!(lowest, None);
        let mut d = Delay::new(Instant::ZERO);
        d.settle(lowest, Instant::ZERO, None);
        assert_eq!(d.delay, MAX);
        // Stepping up stays at the longest.
        d.step_up(Instant::ZERO, None);
        assert_eq!(d.delay, MAX);
    }

    #[test]
    fn in_use_the_delay_steps_up_on_an_error_and_down_on_a_probe() {
        let t0 = Instant::ZERO;
        let mut d = Delay::new(t0);
        d.settle(Some(us(250)), t0, Some(80));
        assert_eq!(d.delay, us(500));
        d.step_up(t0 + ms(1), Some(40));
        assert_eq!((d.delay, d.errors), (us(750), 1));
        // Not before the floor, then only once the temperature has moved.
        assert!(!d.temp_due(t0 + REPROBE_FLOOR - ms(1)));
        assert!(d.temp_due(t0 + ms(1) + REPROBE_FLOOR));
        let later = t0 + ms(1) + REPROBE_FLOOR;
        assert!(!d.probe_due(later, 40 + REPROBE_TEMP_QUARTERS - 1));
        assert!(!d.temp_due(later + TEMP_EVERY - ms(1)));
        assert!(d.temp_due(later + TEMP_EVERY));
        assert!(d.probe_due(later + TEMP_EVERY, 40 + REPROBE_TEMP_QUARTERS));
        // The probe below tests two steps down; passing lowers by one.
        assert_eq!(d.probe_below(), Some(us(250)));
        let (lowest, probes) = run(Search::confirm(us(250)), us(250), us(150));
        assert_eq!((lowest, probes.len()), (Some(us(250)), CONFIRM as usize));
        d.settle(lowest, later, Some(52));
        assert_eq!(d.delay, us(500));
        // At the bottom there is nothing to probe.
        assert_eq!(d.probe_below(), None);
        assert!(!d.probe_due(later + REPROBE_FLOOR, 0));
        // A failed probe below leaves the delay where it is.
        d.settle(Some(us(500)), later, Some(52));
        assert_eq!(d.delay, us(750));
        let (lowest, _) = run(Search::confirm(us(250)), us(600), us(100));
        assert_eq!(lowest, Some(us(750)));
    }
}
