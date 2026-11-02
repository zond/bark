//! The hand-over pace (spec "Bans"): how long one hand-over has taken
//! this relay lately, measured, so that a refusal for want of room can
//! tell the time the line really takes.
//!
//! The bucket admits by airtime: a packet is taken to send on while the
//! airtime of everything queued fits in the bucket. Where the bucket
//! binds, that is the right measure, since the bucket is what the queue
//! drains by. Where the channel binds, a relay's time goes to hand-overs
//! that go unanswered, their retry waits and the bans after them, while
//! its bucket stays near full: it then admits by airtime it has to spare
//! into a queue that drains at the pace the channel allows, far below the
//! bucket's, and the refusals it sends ask for the bucket's refill of what
//! it holds, a second or two, not the minutes the line takes. The pace
//! sees both: a hand-over held for the bucket takes the bucket's refill,
//! one lost to the channel takes its attempts and retries.
//!
//! **What is measured.** Each hand-over's span, from when it is started
//! to when it ends, taken, refused, seen or failed, every wait inside it
//! included; the sum of the spans over the count of the hand-overs is the
//! pace. One hand-over runs at a time (`node/transfer.rs`), so the sum is
//! the time the relay spent handing over, and the pace is its throughput
//! while it had work: an idle relay measures nothing, and is not slow.
//!
//! **The window.** The longest wait, 61 s, the refill of a whole bucket
//! (spec "Duty cycle cap"): the pace over the time a queue is allowed to
//! take, just past, is the estimate of the pace over that time to come.
//! The window is counted in bans, 1.58 s, the unit a refusal tells its
//! wait in and the shortest anything waits (spec "Bans"): ⌈61 s / 1.58 s⌉
//! = 39 bins, a count and a sum each, 312 bytes.

use crate::tables::budget::{BAN, LONGEST_WAIT};
use crate::time::{Duration, Instant};

/// Bins of one [`BAN`] over the [`LONGEST_WAIT`]: 39.
pub(crate) const BINS: usize = LONGEST_WAIT.as_micros().div_ceil(BAN.as_micros()) as usize;

const _: () = assert!(BINS == 39);

/// The hand-overs that ended in one ban, and the time they took.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Bin {
    /// Microseconds: a span is at most the longest wait, 61 s, and one
    /// hand-over runs at a time.
    busy: u32,
    ended: u16,
}

/// The pace, as the module documentation has it.
#[derive(Clone, Copy, Debug)]
pub struct Pace {
    bins: [Bin; BINS],
    /// The bin of `since`; the ones before it round the ring are older.
    head: usize,
    /// When the head bin began.
    since: Instant,
}

impl Pace {
    /// Nothing measured.
    pub const NONE: Pace = Pace {
        bins: [Bin { busy: 0, ended: 0 }; BINS],
        head: 0,
        since: Instant::ZERO,
    };

    /// Bins the window has moved past `since` by `now`, at most all.
    fn stale(&self, now: Instant) -> usize {
        (now.since(self.since).as_micros() / BAN.as_micros()).min(BINS as u64) as usize
    }

    /// A hand-over that took `span` ended at `now`.
    pub fn ended(&mut self, span: Duration, now: Instant) {
        let stale = self.stale(now);
        if stale >= BINS {
            *self = Pace::NONE;
            self.since = now;
        } else {
            for _ in 0..stale {
                self.head = (self.head + 1) % BINS;
                self.bins[self.head] = Bin::default();
                self.since += BAN;
            }
        }
        let b = &mut self.bins[self.head];
        b.busy = b
            .busy
            .saturating_add(span.as_micros().min(u32::MAX as u64) as u32);
        b.ended = b.ended.saturating_add(1);
    }

    /// The time one hand-over took on average over the window ending at
    /// `now`; `None` while none has ended in it.
    pub fn per_handover(&self, now: Instant) -> Option<Duration> {
        let stale = self.stale(now);
        if stale >= BINS {
            return None;
        }
        let (mut busy, mut ended) = (0u64, 0u64);
        for k in 0..BINS - stale {
            let b = self.bins[(self.head + BINS - k) % BINS];
            busy += b.busy as u64;
            ended += b.ended as u64;
        }
        (ended > 0).then(|| Duration::from_micros(busy / ended))
    }

    /// The time `n` hand-overs would take at the pace measured by `now`;
    /// `None` without a measurement.
    pub fn time_for(&self, n: usize, now: Instant) -> Option<Duration> {
        self.per_handover(now).map(|t| t.mul(n as u64))
    }
}

impl Default for Pace {
    fn default() -> Self {
        Pace::NONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: u64) -> Instant {
        Instant::ZERO + Duration::from_secs(s)
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn the_window_is_the_longest_wait_in_bans() {
        assert_eq!(BINS, 39);
        let window = BAN.mul(BINS as u64);
        assert!(window >= LONGEST_WAIT && window < LONGEST_WAIT + BAN);
        assert_eq!(core::mem::size_of::<Pace>(), BINS * 8 + 16);
    }

    #[test]
    fn nothing_measured_is_none() {
        let p = Pace::NONE;
        assert_eq!(p.per_handover(at(1000)), None);
        assert_eq!(p.time_for(5, at(1000)), None);
    }

    #[test]
    fn the_average_over_the_window() {
        let mut p = Pace::NONE;
        p.ended(ms(300), at(100));
        p.ended(ms(3100), at(101));
        assert_eq!(p.per_handover(at(101)), Some(ms(1700)));
        assert_eq!(p.time_for(10, at(101)), Some(ms(17_000)));
        // Still in the window just under the longest wait later.
        assert_eq!(
            p.per_handover(at(100) + LONGEST_WAIT - ms(1)),
            Some(ms(1700))
        );
    }

    #[test]
    fn the_window_forgets() {
        let mut p = Pace::NONE;
        p.ended(ms(3000), at(100));
        // A ban-wide bin at a time: the old sample leaves once its bin does.
        let mut t = at(100);
        while p.per_handover(t) == Some(ms(3000)) {
            t += BAN;
        }
        assert!(t.since(at(100)) >= LONGEST_WAIT);
        assert!(t.since(at(100)) <= LONGEST_WAIT + BAN.mul(2));
        assert_eq!(p.per_handover(t), None);
        // Reading changed nothing; a new sample after a long silence
        // stands alone.
        p.ended(ms(500), at(10_000));
        assert_eq!(p.per_handover(at(10_000)), Some(ms(500)));
        // Quick hand-overs push an old slow one out of the average as the
        // window moves on.
        let mut p = Pace::NONE;
        p.ended(ms(3000), at(200));
        for k in 1..=40u64 {
            p.ended(ms(200), at(200) + BAN.mul(k));
        }
        assert_eq!(p.per_handover(at(200) + BAN.mul(40)), Some(ms(200)));
    }
}
