//! The radio's oscillator start, probed (roadmap D88, D193).
//!
//! The SX1262 powers the board's TCXO each time it leaves STDBY_RC or
//! sleep, waits out a programmed delay, and only then goes on; it has no
//! ready signal, but sets `XOSC_START_ERR` (`GetDeviceErrors`, SX1261/2
//! datasheet §13.3.6) when the oscillator was not up by the delay's end.
//! A delay of zero hangs the chip. So the board measures its own start.
//!
//! There are two starts. A cold one, after the oscillator has been off
//! for [`STILL_RUNNING`] or more. A restart sooner than that, which is
//! the return after every CAD, since a CAD ends in STDBY_RC: the
//! oscillator is then still running, or nearly, and needs far less. Each
//! has its own [`Delay`], probed the same way: a binary search at boot,
//! each probe a start with the error word read after it, then
//! confirmation probes at the chosen delay, since the flag flickers at
//! the edge, plus one step of margin; in use the flag read after every
//! start and the delay stepped up at once when it is set; and a probe
//! below the delay now and then, when the die's temperature has moved or
//! a day has passed without an error, which brings it back down
//! (`docs/bench-2026-10-30.md`, "The oscillator's start, probed";
//! `docs/radio-recovery-2026-11-10.md`). The restart's delay never
//! exceeds the cold one, which is its ceiling.

use crate::{Duration, Instant};

/// The cold delay's step. The bench probed the flag at 50, 100, 200 and
/// 500 µs: it flickered between 50 and 200 and never showed at 500, so
/// the start is known to about a quarter millisecond and a finer step
/// would resolve nothing. A step this wide also holds the whole band in
/// which the flag flickers, so one step of margin clears it.
pub const STEP: Duration = Duration::from_micros(250);

/// The shortest cold delay probed: one step. Zero hangs the chip.
pub const MIN: Duration = STEP;

/// The longest cold delay probed, 16 steps: a factor eight over the
/// 500 µs the bench found at room temperature, where roadmap D87's 2 ms
/// kept a factor four; a cold board is unmeasured. A board whose
/// oscillator takes longer runs at this delay with the error flag set,
/// which costs a few dB at the start of each frame, not the frame.
pub const MAX: Duration = Duration::from_micros(16 * STEP.as_micros());

/// The margin over the lowest cold delay that passed its confirmation
/// probes: one step, the width of the band in which the flag flickers.
pub const MARGIN: Duration = STEP;

/// The restart's step, and its floor. The bench restarted the
/// oscillator within a millisecond of stopping 90 times at 50 µs without
/// an error and tried nothing shorter, so 50 µs is the least delay known
/// to pass; zero hangs the chip. The chip's own step, 15.625 µs, would
/// resolve starts nobody has measured. One step up costs 50 µs at each
/// return after a CAD, nothing beside the CAD's 2 ms, and from the floor
/// to the bench's cold delay of 500 µs is nine steps.
pub const SHORT_STEP: Duration = Duration::from_micros(50);

/// Probes at a delay after the search lands on it, all of which must pass,
/// since at the edge about half the probes fail: four passes at a delay
/// where half fail happen one time in sixteen.
pub const CONFIRM: u8 = 4;

/// How long after stopping the oscillator still runs: a restart within a
/// millisecond needed no delay on the bench, and one after a millisecond
/// was a cold start.
pub const STILL_RUNNING: Duration = Duration::from_millis(1);

/// How long a cold probe keeps the oscillator off before its start: twice
/// [`STILL_RUNNING`], so that every such probe is a cold start.
pub const COLD_WAIT: Duration = Duration::from_millis(2);

/// The least time between a delay settling and a probe below it. A probe
/// deafens the radio for a few milliseconds; once an hour it costs
/// nothing, and the delay only has to follow the day's temperature.
pub const REPROBE_FLOOR: Duration = Duration::from_secs(3600);

/// How long a delay is kept without a start error before a probe below it
/// is due whatever the temperature did: a day, the period of the
/// temperature a board lives in. A delay that a day of starts at every
/// temperature of the day never faulted was set by one unlucky start, or
/// by an error blamed on the wrong start, and not by the board; a probe
/// then brings it down a step. An hour would be the floor, a probe every
/// hour on every board, each one a chance for an error in use where the
/// delay sits at the edge; a week would leave a needless step in place for
/// a week. A board that reads no temperature recovers by this alone.
pub const REPROBE_AFTER: Duration = Duration::from_secs(24 * 3600);

/// How far the die's temperature must have moved since a delay settled,
/// in the nRF52840 TEMP peripheral's quarter degrees: 3 °C. The sensor is
/// uncalibrated and only its change is used; its resolution is a quarter
/// degree, so a move of twelve steps is a real one, and a start time that
/// a move this small would change is within the margin anyway. No bench
/// number sets it: the in-use check steps the delay up on any error,
/// whatever the temperature did, so this only paces the recovery.
pub const REPROBE_TEMP_QUARTERS: i16 = 12;

/// How often the schedule reads the die's temperature once the floor has
/// passed.
pub const TEMP_EVERY: Duration = Duration::from_secs(60);

/// Where a search is, in whole steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// Binary search between the two steps, inclusive, for the lowest
    /// that passes.
    Between(u8, u8),
    /// Confirmation probes at this step, this many left.
    Confirm(u8, u8),
    /// The outcome: the lowest step that passed its confirmation, or
    /// `None` if none did up to the ceiling.
    Done(Option<u8>),
}

/// A search for the lowest delay that starts the oscillator: at boot a
/// binary search from one step up to a ceiling, in use a confirmation at
/// one step. The caller probes the delay [`Search::next`] asks for and
/// reports [`Search::result`], until [`Search::outcome`] is `Some`.
#[derive(Clone, Copy, Debug)]
pub struct Search {
    step: Duration,
    /// The ceiling, in steps.
    max: u8,
    phase: Phase,
}

impl Search {
    /// A delay in whole steps, rounded down.
    fn steps(&self, d: Duration) -> u8 {
        (d.as_micros() / self.step.as_micros()).min(u8::MAX.into()) as u8
    }

    fn delay(&self, steps: u8) -> Duration {
        self.step.mul(steps.into())
    }

    /// The delay to probe next, or `None` once the search is over.
    pub fn next(&self) -> Option<Duration> {
        match self.phase {
            Phase::Between(lo, hi) => Some(self.delay(lo.midpoint(hi))),
            Phase::Confirm(at, _) => Some(self.delay(at)),
            Phase::Done(_) => None,
        }
    }

    /// The probe at the delay [`Search::next`] asked for started the
    /// oscillator without an error, or not.
    pub fn result(&mut self, ok: bool) {
        let max = self.max;
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
    /// over: `Some(None)` if none did up to the ceiling.
    pub fn outcome(&self) -> Option<Option<Duration>> {
        match self.phase {
            Phase::Done(d) => Some(d.map(|s| self.delay(s))),
            _ => None,
        }
    }
}

/// A probe made.
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
    /// For a restart, how long the oscillator had been off.
    pub gap: Option<Duration>,
}

/// One probed delay, the cold start's or the restart's, and how it got
/// there.
#[derive(Clone, Copy, Debug)]
pub struct Delay {
    /// The search's step, its floor and its margin.
    pub step: Duration,
    /// The delay starts are programmed with: the lowest confirmed delay
    /// plus one step, and a step more for each error since.
    pub delay: Duration,
    /// Probes made since boot.
    pub probes: u32,
    /// Starts in use that set the error flag.
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
    /// Before the search: `ceiling`, safe.
    pub fn new(step: Duration, ceiling: Duration, now: Instant) -> Delay {
        Delay {
            step,
            delay: ceiling,
            probes: 0,
            errors: 0,
            last: None,
            settled_at: now,
            settled_temp: None,
            temp_read_at: now,
        }
    }

    /// The search at boot, from one step up to `ceiling`.
    pub fn search(&self, ceiling: Duration) -> Search {
        let mut s = Search {
            step: self.step,
            max: 0,
            phase: Phase::Done(None),
        };
        s.max = s.steps(ceiling).max(1);
        s.phase = Phase::Between(1, s.max);
        s
    }

    /// The confirmation probes at `d` alone, in use, climbing to
    /// `ceiling` at most.
    pub fn confirm(&self, d: Duration, ceiling: Duration) -> Search {
        let mut s = self.search(ceiling);
        s.phase = Phase::Confirm(s.steps(d).clamp(1, s.max), CONFIRM);
        s
    }

    /// Note a probe made.
    pub fn probed(&mut self, probe: Probe) {
        self.probes += 1;
        self.last = Some(probe);
    }

    /// A search's outcome, at boot or in use: the delay is the lowest
    /// confirmed delay plus one step, or `ceiling` if none was confirmed.
    pub fn settle(
        &mut self,
        lowest: Option<Duration>,
        ceiling: Duration,
        now: Instant,
        temp: Option<i16>,
    ) {
        self.delay = lowest.map_or(ceiling, |d| (d + self.step).min(ceiling));
        self.settled_at = now;
        self.settled_temp = temp;
    }

    /// A start in use set the error flag: one step up at once.
    pub fn step_up(&mut self, ceiling: Duration, now: Instant, temp: Option<i16>) {
        self.errors += 1;
        self.delay = (self.delay + self.step).min(ceiling);
        self.settled_at = now;
        self.settled_temp = temp;
    }

    /// The delay a probe below the one in use tests: two steps down, so
    /// that a pass lowers the delay by one and keeps the margin. `None`
    /// at the bottom.
    pub fn probe_below(&self) -> Option<Duration> {
        let below = self
            .delay
            .as_micros()
            .checked_sub(2 * self.step.as_micros())?;
        (below >= self.step.as_micros()).then(|| Duration::from_micros(below))
    }

    /// Whether the schedule is to be looked at now, with the die's
    /// temperature read for it: not before the floor, then every
    /// [`TEMP_EVERY`].
    pub fn check_due(&self, now: Instant) -> bool {
        now >= self.settled_at + REPROBE_FLOOR && now >= self.temp_read_at + TEMP_EVERY
    }

    /// Whether a probe below the delay is due, given the die's
    /// temperature read now, if the board reads it: the floor has passed,
    /// and [`REPROBE_AFTER`] has too, or the temperature has moved by
    /// [`REPROBE_TEMP_QUARTERS`] since the delay settled, or was not read
    /// then.
    pub fn probe_due(&mut self, now: Instant, temp: Option<i16>) -> bool {
        self.temp_read_at = now;
        if now < self.settled_at + REPROBE_FLOOR || self.probe_below().is_none() {
            return false;
        }
        if now >= self.due_by() {
            return true;
        }
        match (self.settled_temp, temp) {
            (Some(then), Some(now)) => (now - then).abs() >= REPROBE_TEMP_QUARTERS,
            (None, Some(_)) => true,
            (_, None) => false,
        }
    }

    /// From when a move of the temperature makes a probe due.
    pub fn due_from(&self) -> Instant {
        self.settled_at + REPROBE_FLOOR
    }

    /// When a probe is due whatever the temperature does.
    pub fn due_by(&self) -> Instant {
        self.settled_at + REPROBE_AFTER
    }

    /// The die's temperature when the delay settled.
    pub fn settled_temp(&self) -> Option<i16> {
        self.settled_temp
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

    fn cold() -> Delay {
        Delay::new(STEP, MAX, Instant::ZERO)
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
        assert_eq!(cold().search(MAX).max, 16);
        // The restart's floor is what passed 90 probes on the bench, and
        // with its margin it is the 100 µs D88 programmed.
        assert_eq!(SHORT_STEP, us(50));
        let (lowest, _) = run(
            Delay::new(SHORT_STEP, us(500), Instant::ZERO).search(us(500)),
            Duration::ZERO,
            Duration::ZERO,
        );
        assert_eq!(lowest, Some(SHORT_STEP));
        assert!(REPROBE_FLOOR < REPROBE_AFTER);
    }

    #[test]
    fn a_room_temperature_board_lands_one_step_up_from_the_lowest_pass() {
        // The bench's board: starts within 500 µs, flickers below 200.
        let (lowest, probes) = run(cold().search(MAX), us(250), us(150));
        assert_eq!(lowest, Some(us(250)));
        // Four binary probes, then four confirmations at the answer.
        assert_eq!(probes.len(), 4 + CONFIRM as usize);
        assert!(probes[4..].iter().all(|&d| d == us(250)));
        let mut d = cold();
        d.settle(lowest, MAX, Instant::ZERO, Some(100));
        assert_eq!(d.delay, us(500));
    }

    #[test]
    fn a_slow_board_lands_above_its_start() {
        for start in [us(750), us(1250), ms(2), us(3250), ms(4)] {
            let (lowest, _) = run(cold().search(MAX), start, us(150));
            let lowest = lowest.expect("found");
            assert!(lowest >= start, "{start:?}: {lowest:?}");
            assert!(lowest.as_micros() < start.as_micros() + STEP.as_micros());
        }
    }

    #[test]
    fn a_restart_lands_above_its_start_under_the_cold_delay() {
        let short = Delay::new(SHORT_STEP, us(500), Instant::ZERO);
        for start in [us(60), us(120), us(260), us(450)] {
            let (lowest, _) = run(short.search(us(500)), start, us(30));
            let lowest = lowest.expect("found");
            assert!(lowest >= start, "{start:?}: {lowest:?}");
            assert!(lowest < start + SHORT_STEP);
        }
        // One that needs more than a cold start gets the cold delay.
        let (lowest, _) = run(short.search(us(500)), us(700), us(30));
        let mut d = short;
        d.settle(lowest, us(500), Instant::ZERO, None);
        assert_eq!((lowest, d.delay), (None, us(500)));
    }

    #[test]
    fn a_flickering_edge_is_climbed_by_the_confirmation() {
        // The oscillator takes 1.1 ms; the flag flickers from 0.6 ms up.
        // A lucky binary probe at 1.0 ms may pass; the confirmation then
        // sees a failure and climbs to 1.25 ms.
        let (lowest, probes) = run(cold().search(MAX), us(1100), us(500));
        assert_eq!(lowest, Some(us(1250)), "{probes:?}");
    }

    #[test]
    fn a_board_that_never_passes_gets_the_ceiling() {
        let (lowest, _) = run(cold().search(MAX), ms(10), us(150));
        assert_eq!(lowest, None);
        let mut d = cold();
        d.settle(lowest, MAX, Instant::ZERO, None);
        assert_eq!(d.delay, MAX);
        // Stepping up stays at the ceiling.
        d.step_up(MAX, Instant::ZERO, None);
        assert_eq!(d.delay, MAX);
    }

    #[test]
    fn in_use_the_delay_steps_up_on_an_error_and_down_on_a_probe() {
        let t0 = Instant::ZERO;
        let mut d = cold();
        d.settle(Some(us(250)), MAX, t0, Some(80));
        assert_eq!(d.delay, us(500));
        d.step_up(MAX, t0 + ms(1), Some(40));
        assert_eq!((d.delay, d.errors), (us(750), 1));
        // Not before the floor, then only once the temperature has moved.
        assert!(!d.check_due(t0 + REPROBE_FLOOR - ms(1)));
        assert!(d.check_due(t0 + ms(1) + REPROBE_FLOOR));
        let later = t0 + ms(1) + REPROBE_FLOOR;
        assert!(!d.probe_due(later, Some(40 + REPROBE_TEMP_QUARTERS - 1)));
        assert!(!d.check_due(later + TEMP_EVERY - ms(1)));
        assert!(d.check_due(later + TEMP_EVERY));
        assert!(d.probe_due(later + TEMP_EVERY, Some(40 + REPROBE_TEMP_QUARTERS)));
        // The probe below tests two steps down; passing lowers by one.
        assert_eq!(d.probe_below(), Some(us(250)));
        let (lowest, probes) = run(d.confirm(us(250), MAX), us(250), us(150));
        assert_eq!((lowest, probes.len()), (Some(us(250)), CONFIRM as usize));
        d.settle(lowest, MAX, later, Some(52));
        assert_eq!(d.delay, us(500));
        // At the bottom there is nothing to probe.
        assert_eq!(d.probe_below(), None);
        assert!(!d.probe_due(later + REPROBE_AFTER, Some(0)));
        // A failed probe below leaves the delay where it is.
        d.settle(Some(us(500)), MAX, later, Some(52));
        assert_eq!(d.delay, us(750));
        let (lowest, _) = run(d.confirm(us(250), MAX), us(600), us(100));
        assert_eq!(lowest, Some(us(750)));
    }

    #[test]
    fn a_day_without_an_error_makes_a_probe_due_at_a_steady_temperature() {
        let t0 = Instant::ZERO;
        let mut d = cold();
        d.settle(Some(us(500)), MAX, t0, Some(80));
        assert_eq!(d.due_by(), t0 + REPROBE_AFTER);
        assert!(!d.probe_due(t0 + REPROBE_AFTER - ms(1), Some(80)));
        assert!(d.probe_due(t0 + REPROBE_AFTER, Some(80)));
        // An error starts the day again.
        d.step_up(MAX, t0 + REPROBE_AFTER - ms(1), Some(80));
        assert!(!d.probe_due(t0 + REPROBE_AFTER, Some(80)));
        // Without a temperature reading only the day counts.
        let mut d = cold();
        d.settle(Some(us(500)), MAX, t0, None);
        assert!(!d.probe_due(t0 + REPROBE_FLOOR, None));
        assert!(d.probe_due(t0 + REPROBE_AFTER, None));
    }
}
