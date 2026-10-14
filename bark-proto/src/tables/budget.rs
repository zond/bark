//! The duty-cycle budget (design §3, §9.4): at most
//! [`DUTY_CYCLE_PERCENT`] of any [`DUTY_CYCLE_WINDOW`] on air, and no
//! single transmission longer than [`MAX_TRANSMISSION`].
//!
//! # Accounting
//!
//! Airtime is summed into [`BUCKET`]-long buckets by the instant each
//! transmission *ends*. A transmission of length `a` starting at `now` is
//! allowed if the airtime in every bucket that overlaps the window ending
//! when it ends, `[now + a − W, now + a]`, plus `a`, is within the cap.
//!
//! That is sufficient for legality, at every instant:
//!
//! - Any earlier transmission with airtime inside that window ends inside
//!   it, so its bucket overlaps the window and is counted, in full.
//! - The airtime in a window ending at `T` grows with `T` while this
//!   transmission is on air (it gains this transmission at full rate and
//!   loses old airtime at most at that rate), so the window ending when it
//!   ends is the worst one; later windows are checked by later
//!   transmissions.
//!
//! The error is one-sided: the oldest counted bucket may lie mostly
//! outside the window and is counted whole. The budget is therefore
//! underestimated by at most the airtime of one bucket: about
//! `BUCKET × 10 % = 6 s` (1.7 % of the 360 s budget) for a node that
//! transmits steadily at the cap, and at most one bucket of continuous
//! transmission, 60 s, after a burst, which the budget itself never
//! allows to recur within the hour.

use crate::radio::{DUTY_CYCLE_PERCENT, DUTY_CYCLE_WINDOW};
use crate::time::{Duration, Instant};

/// Bucket length. One minute: 62 buckets for the hour.
pub const BUCKET: Duration = Duration::from_secs(60);

/// Airtime allowed in any window: 10 % of an hour, 360 s.
pub const BUDGET: Duration = Duration::from_micros(DUTY_CYCLE_WINDOW.as_micros() * DUTY_CYCLE_PERCENT as u64 / 100);

/// Longest single transmission (design §3: 1 s; SF5 frames are ≤140 ms).
pub const MAX_TRANSMISSION: Duration = Duration::from_secs(1);

/// Buckets kept: the window spans `W / BUCKET` buckets, plus one partial
/// at its start, plus one that a transmission ending up to
/// [`MAX_TRANSMISSION`] after `now` can reach.
const SLOTS: usize = (DUTY_CYCLE_WINDOW.as_micros() / BUCKET.as_micros()) as usize + 2;

const _: () = assert!(DUTY_CYCLE_WINDOW.as_micros().is_multiple_of(BUCKET.as_micros()));
const _: () = assert!(MAX_TRANSMISSION.as_micros() <= BUCKET.as_micros());
// Transmissions ending in one bucket do not overlap, so they started at
// most MAX_TRANSMISSION before it: their airtime fits u32 µs.
const _: () = assert!(BUCKET.as_micros() + MAX_TRANSMISSION.as_micros() <= u32::MAX as u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Slot {
    /// Bucket number: end instant / BUCKET.
    bucket: u64,
    /// Airtime in µs ending in this bucket.
    airtime: u32,
}

/// The rolling airtime budget.
#[derive(Clone, Debug)]
pub struct Budget {
    slots: [Slot; SLOTS],
}

impl Budget {
    /// A budget with nothing spent.
    pub const fn new() -> Budget {
        Budget { slots: [Slot { bucket: 0, airtime: 0 }; SLOTS] }
    }

    fn bucket(t: u64) -> u64 {
        t / BUCKET.as_micros()
    }

    /// Airtime in the buckets overlapping a window that starts at `start` µs.
    fn spent_since(&self, start: u64) -> u64 {
        let first = Budget::bucket(start);
        self.slots.iter().filter(|s| s.bucket >= first).map(|s| s.airtime as u64).sum()
    }

    /// Whether a transmission of `airtime` may start at `now`.
    pub fn would_allow(&self, airtime: Duration, now: Instant) -> bool {
        if airtime > MAX_TRANSMISSION {
            return false;
        }
        let end = now.as_micros() + airtime.as_micros();
        let start = end.saturating_sub(DUTY_CYCLE_WINDOW.as_micros());
        self.spent_since(start) + airtime.as_micros() <= BUDGET.as_micros()
    }

    /// Record a transmission of `airtime` that started at `now`. Records it
    /// whether or not it was allowed: what went on air counts.
    pub fn record(&mut self, airtime: Duration, now: Instant) {
        let bucket = Budget::bucket(now.as_micros() + airtime.as_micros());
        let slot = &mut self.slots[(bucket % SLOTS as u64) as usize];
        if slot.bucket != bucket {
            *slot = Slot { bucket, airtime: 0 };
        }
        let airtime = u32::try_from(airtime.as_micros()).unwrap_or(u32::MAX);
        slot.airtime = slot.airtime.saturating_add(airtime);
    }

    /// Airtime available at `now`: the cap less what the window ending now
    /// has spent. Any transmission no longer than this is allowed.
    pub fn available(&self, now: Instant) -> Duration {
        let start = now.as_micros().saturating_sub(DUTY_CYCLE_WINDOW.as_micros());
        Duration::from_micros(BUDGET.as_micros().saturating_sub(self.spent_since(start)))
    }
}

impl Default for Budget {
    fn default() -> Self {
        Budget::new()
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use proptest::prelude::*;
    use std::vec::Vec;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// The most airtime in any window, from the exact intervals, which are
    /// in order and do not overlap. The worst window ends at a
    /// transmission's end (module documentation), so only those are tried.
    fn worst_window(txs: &[(u64, u64)]) -> u64 {
        let w = DUTY_CYCLE_WINDOW.as_micros();
        let (mut first, mut sum, mut worst) = (0, 0, 0);
        for &(s, a) in txs {
            let end = s + a;
            let start = end.saturating_sub(w);
            sum += a;
            while txs[first].0 + txs[first].1 <= start {
                sum -= txs[first].1;
                first += 1;
            }
            let clipped = start.saturating_sub(txs[first].0);
            worst = worst.max(sum - clipped);
        }
        worst
    }

    #[test]
    fn constants() {
        assert_eq!(BUDGET, Duration::from_secs(360));
        assert_eq!(SLOTS, 62);
    }

    #[test]
    fn spends_and_recovers() {
        let mut b = Budget::new();
        let t0 = Instant::from_micros(10 * 3_600_000_000);
        assert_eq!(b.available(t0), BUDGET);
        assert!(!b.would_allow(ms(1001), t0), "single transmission cap");
        // Spend the whole budget in one-second frames.
        let mut t = t0;
        for _ in 0..360 {
            assert!(b.would_allow(ms(1000), t));
            b.record(ms(1000), t);
            t += ms(1000);
        }
        assert!(!b.would_allow(ms(1), t));
        assert_eq!(b.available(t), Duration::ZERO);
        // An hour after the burst ended everything is back.
        let later = t + DUTY_CYCLE_WINDOW + BUCKET;
        assert_eq!(b.available(later), BUDGET);
        assert!(b.would_allow(ms(1000), later));
    }

    #[test]
    fn straddling_transmission_counts_where_it_ends() {
        // One second from 59.5 s to 60.5 s, then 359 s from 120 s. A further
        // second ending at 3660.25 s puts 0.25 + 359 + 1 s in the window
        // [60.25 s, 3660.25 s]: over the cap. Bucketing by start instant
        // would miss the first transmission and allow it.
        let mut b = Budget::new();
        b.record(ms(1000), Instant::from_micros(59_500_000));
        for i in 0..359 {
            b.record(ms(1000), Instant::from_micros(120_000_000 + i * 1_000_000));
        }
        assert!(!b.would_allow(ms(1000), Instant::from_micros(3_659_250_000)));
        assert!(b.would_allow(ms(1000), Instant::from_micros(3_720_000_000)));
    }

    #[test]
    fn steady_sender_gets_nearly_the_whole_budget() {
        // Send 140 ms frames whenever allowed for three hours; the last hour
        // must carry at least the budget less one bucket at 10 %.
        let mut b = Budget::new();
        let mut txs = Vec::new();
        let frame = ms(140);
        let mut t = Instant::ZERO;
        let end = Instant::ZERO + DUTY_CYCLE_WINDOW.mul(3);
        while t < end {
            if b.would_allow(frame, t) {
                b.record(frame, t);
                txs.push((t.as_micros(), frame.as_micros()));
                t += frame;
            } else {
                t += ms(10);
            }
        }
        assert!(worst_window(&txs) <= BUDGET.as_micros());
        let last_hour = end.as_micros() - DUTY_CYCLE_WINDOW.as_micros();
        let spent: u64 = txs.iter().filter(|&&(s, _)| s >= last_hour).map(|&(_, a)| a).sum();
        let floor = BUDGET.as_micros() - BUCKET.as_micros() / 10 - frame.as_micros();
        assert!(spent >= floor, "spent {spent} µs, floor {floor}");
    }

    fn gap() -> impl Strategy<Value = u64> {
        prop_oneof![50 => 0u64..1_000_000, 1 => 0u64..1_200_000_000]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn never_exceeds_the_cap(steps in proptest::collection::vec((gap(), 1u64..=1_000_000), 1..10_000)) {
            // Demand well above the cap (mostly sub-second gaps, airtimes up to
            // 1 s) with occasional long silences; every transmission the
            // budget allows is sent.
            let mut b = Budget::new();
            let mut txs = Vec::new();
            let mut t = 0;
            for (gap, airtime) in steps {
                t += gap;
                let a = Duration::from_micros(airtime);
                let now = Instant::from_micros(t);
                if b.would_allow(a, now) {
                    b.record(a, now);
                    txs.push((t, airtime));
                    t += airtime;
                }
            }
            prop_assert!(worst_window(&txs) <= BUDGET.as_micros());
        }

        #[test]
        fn available_is_allowed(spent in proptest::collection::vec((0u64..5_000_000, 1u64..=1_000_000), 0..500), probe in 1u64..=1_000_000) {
            let mut b = Budget::new();
            let mut t = 0;
            for (gap, airtime) in spent {
                t += gap;
                b.record(Duration::from_micros(airtime), Instant::from_micros(t));
                t += airtime;
            }
            let now = Instant::from_micros(t);
            let a = Duration::from_micros(probe);
            if a <= b.available(now) {
                prop_assert!(b.would_allow(a, now));
            }
        }
    }
}
