//! The duty-cycle budget (spec "Regulatory limits", "Duty cycle cap"): a token bucket that keeps a
//! node within [`DUTY_CYCLE_PERCENT`] of every [`DUTY_CYCLE_WINDOW`], and
//! no single transmission longer than [`MAX_TRANSMISSION`].
//!
//! # The bucket
//!
//! The bucket holds at most [`DEPTH`] of airtime, one minute at the 10 %
//! cap, 6 s, and fills at the rate `r = (B − DEPTH) / W`, with `B` the
//! [`BUDGET`] of 360 s and `W` the one-hour window: 354 s an hour, 9.83 %.
//! A transmission may start only if the bucket holds its airtime, which it
//! then takes out.
//!
//! **Legality.** Take any interval `[T − L, T]`. If no transmission
//! straddles its start, everything on the air in it started in it and was
//! taken from the bucket: at most what the bucket held at `T − L`, at most
//! `DEPTH`, plus what flowed in meanwhile, `r·L`. If one transmission of
//! airtime `a` started at `s < T − L` and runs into the interval, count
//! from `s` instead: the bucket held at most `DEPTH − a` after it, so what
//! started in `(s, T]` is at most `DEPTH − a + r·(T − s)`, and the
//! straddler adds `s + a − (T − L)` inside the interval; the sum is
//! `DEPTH + L − (1 − r)·(T − s)`, at most `DEPTH + r·L` since
//! `T − s > L`. A transmission running past `T` only lowers the count. So
//! any interval of length `L` holds at most `DEPTH + r·L` on air, and any
//! hour at most `DEPTH + r·W = B`: 10 %, the limit of ERC/REC 70-03
//! Annex 1 band g1, which that recommendation measures over one hour.
//! The rate is below 10 % by `DEPTH / W` exactly so that the allowance
//! for a burst cannot carry an hour over the limit.
//!
//! **Silence.** A node that has emptied its bucket may send a transmission
//! of airtime `a` after `a / r`: 1.4 s for a full frame, never more than
//! `DEPTH / r`, 61 s.
//!
//! # Heartbeat reserve
//!
//! A relay out of budget stops forwarding but keeps sending heartbeats
//! (spec "Duty cycle cap"), so everything but a heartbeat leaves [`HEARTBEAT_RESERVE`] in
//! the bucket: one heartbeat's airtime, 13.12 ms. That suffices while
//! heartbeats are at least the reserve's refill time, 13.12 ms / r = 133 ms,
//! apart (spec "Heartbeat": at least 5 s): a heartbeat finds the reserve, takes
//! it, and the bucket refills it before the next, since nothing else may
//! spend below it.
//!
//! # Reply reserve
//!
//! Below [`REPLY_RESERVE`] a relay takes no new work: it answers every
//! packet offered to it with an ACK that refuses, or the ACK of a copy it
//! already has, and sends nothing else but heartbeats (spec "Receiving DATA", "Duty cycle cap",
//! roadmap D27, D32). So forwarding and the ACKs that accept work leave
//! the reply reserve in the bucket, and replies leave the heartbeat
//! reserve.
//!
//! The reserve holds the refusals the bucket refills while a sender
//! retries a hand-over that goes unanswered, 2.40 s (spec "Hand-over"):
//! `r × 2.40 s = 236.3 ms`, ⌊236.3 / 15.68⌋ = [`REPLIES`] = 15 ACKs of
//! 15.68 ms, and the heartbeat reserve on top, 248.3 ms in all: a relay at
//! the reserve can refuse, from the refill alone, as many senders as would
//! otherwise have spent that time retrying it.
//!
//! # Ban
//!
//! A relay refuses a packet to send on when the packet's frame and the ACK
//! that takes it do not fit in its bucket on top of what it has queued
//! (spec "Receiving DATA", roadmap D29). The sender bans it for [`BAN`], the time the
//! bucket takes to refill a full frame and its ACK, 138.56 + 15.68 =
//! 154.24 ms at 9.83 %: 1.57 s: by then the bucket has refilled what the
//! refusal lacked, unless the relay has spent it on what it already held.

use crate::event::MAX_FRAME;
use crate::radio::{time_on_air, DUTY_CYCLE_PERCENT, DUTY_CYCLE_WINDOW, MIN_PREAMBLE_SYMBOLS};
use crate::tables::{retrying_span, RETRY_BACKOFF};
use crate::time::{Duration, Instant};
use crate::wire::{ACK_LEN, AUX_MASK, HEARTBEAT_LEN};

/// Airtime allowed in any window: 10 % of an hour, 360 s.
pub const BUDGET: Duration =
    Duration::from_micros(DUTY_CYCLE_WINDOW.as_micros() * DUTY_CYCLE_PERCENT as u64 / 100);

/// The most airtime the bucket holds: one minute at the cap, 6 s, about
/// 39 full frames and their ACKs; the send queue's length follows from it
/// (spec "Send queue").
pub const DEPTH: Duration = Duration::from_micros(BUDGET.as_micros() / 60);

/// Longest single transmission (spec "Regulatory limits": 1 s; SF5 frames are ≤140 ms).
pub const MAX_TRANSMISSION: Duration = Duration::from_secs(1);

const fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

const G: u64 = gcd(
    BUDGET.as_micros() - DEPTH.as_micros(),
    DUTY_CYCLE_WINDOW.as_micros(),
);

/// The fill rate `(BUDGET − DEPTH) / W` as `RATE_NUM / RATE_DEN`: 59 / 600.
/// The bucket counts in units of `1 / RATE_DEN` µs, so it fills by
/// `RATE_NUM` units a microsecond and the arithmetic is exact.
const RATE_NUM: u64 = (BUDGET.as_micros() - DEPTH.as_micros()) / G;
const RATE_DEN: u64 = DUTY_CYCLE_WINDOW.as_micros() / G;

const _: () = assert!(RATE_NUM == 59 && RATE_DEN == 600);
const _: () = assert!(MAX_TRANSMISSION.as_micros() <= DEPTH.as_micros());

/// Airtime a relay keeps in the bucket for its heartbeats; see the module
/// documentation.
pub const HEARTBEAT_RESERVE: Duration = time_on_air(HEARTBEAT_LEN as u8, MIN_PREAMBLE_SYMBOLS);

/// Airtime of an ACK, taken or refused, 15.68 ms.
pub const ACK_AIRTIME: Duration = time_on_air(ACK_LEN as u8, MIN_PREAMBLE_SYMBOLS);

/// The airtime the bucket refills in `d`.
const fn refilled(d: Duration) -> Duration {
    Duration::from_micros(d.as_micros() * RATE_NUM / RATE_DEN)
}

/// The most a node may have on air in any interval of length `span`,
/// `DEPTH + r·span`: what a full bucket holds at its start and what flows
/// in meanwhile; see the module documentation.
pub const fn most_on_air(span: Duration) -> Duration {
    Duration::from_micros(DEPTH.as_micros() + refilled(span).as_micros())
}

/// The time the bucket takes to refill `d`.
const fn refill_time(d: Duration) -> Duration {
    Duration::from_micros((d.as_micros() * RATE_DEN).div_ceil(RATE_NUM))
}

/// Refusals the reply reserve holds: those the bucket refills while a
/// sender with the design's 100 ms ACK timeout and 3 attempts retries
/// (spec "Hand-over"); see the module documentation.
pub const REPLIES: u64 = refilled(retrying_span(Duration::from_millis(100), 3, RETRY_BACKOFF))
    .as_micros()
    / ACK_AIRTIME.as_micros();

/// Airtime below which a relay takes no new work; see the module
/// documentation.
pub const REPLY_RESERVE: Duration =
    Duration::from_micros(REPLIES * ACK_AIRTIME.as_micros() + HEARTBEAT_RESERVE.as_micros());

/// How long a sender refused bans the relay (spec "Bans"); see the module
/// documentation.
pub const BAN: Duration = refill_time(Duration::from_micros(
    time_on_air(MAX_FRAME as u8, MIN_PREAMBLE_SYMBOLS).as_micros() + ACK_AIRTIME.as_micros(),
));

const _: () = assert!(REPLIES == 15);

/// How long a sender holds a message after the `refusals`-th refusal in a
/// row before offering it again (spec "Bans"): one [`BAN`] after the
/// first, doubling after each further one, up to the refill of the whole
/// bucket, 61 s, the longest anything waits for a bucket. Offered once a
/// ban, a full frame every 1.57 s would be a 9 % probe on the refuser's
/// channel and spend the holder's bucket as fast as it refills.
pub const fn hold(refusals: u8) -> Duration {
    let cap = refill_time(DEPTH).as_micros();
    let doublings = refusals.saturating_sub(1);
    let mut wait = BAN.as_micros();
    let mut k = 0;
    while k < doublings && wait < cap {
        wait *= 2;
        k += 1;
    }
    Duration::from_micros(if wait < cap { wait } else { cap })
}

/// The wait a refusal asking for `bans` means (spec "Bans"): never
/// less than one [`BAN`].
pub fn told(bans: u8) -> Duration {
    BAN.mul(bans.max(1) as u64)
}

/// `wait` in whole [`BAN`]s for a refusal, rounded down, so that a wait
/// passed on from relay to relay only shrinks; at least 1, since 0 in an
/// ACK means taken, and at most what `aux` holds, 63 bans, 98.8 s. A
/// longer wait is told as the longest, and its sender may come back
/// early and be refused again.
pub fn wait_units(wait: Duration) -> u8 {
    (wait.as_micros() / BAN.as_micros()).clamp(1, AUX_MASK as u64) as u8
}

/// The duty-cycle token bucket.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget {
    /// Content at `at`, in `1 / RATE_DEN` µs. Negative only after a
    /// transmission the bucket did not allow was recorded.
    units: i64,
    at: Instant,
}

/// `d` in bucket units.
const fn units(d: Duration) -> i64 {
    (d.as_micros() * RATE_DEN) as i64
}

const FULL: i64 = units(DEPTH);

impl Budget {
    /// A full bucket.
    pub const fn new() -> Self {
        Budget {
            units: FULL,
            at: Instant::ZERO,
        }
    }

    /// Content at `now`, in bucket units.
    fn content(&self, now: Instant) -> i64 {
        let elapsed = now.as_micros().saturating_sub(self.at.as_micros()) as u128;
        let filled =
            self.units as i128 + (elapsed * RATE_NUM as u128).min(u64::MAX as u128) as i128;
        filled.min(FULL as i128) as i64
    }

    /// Whether a transmission of `airtime` may start at `now`.
    pub fn would_allow(&self, airtime: Duration, now: Instant) -> bool {
        self.would_allow_keeping(airtime, Duration::ZERO, now)
    }

    /// Whether a transmission of `airtime` may start at `now` and leave
    /// `reserve` in the bucket, as everything but a heartbeat must leave
    /// [`HEARTBEAT_RESERVE`].
    pub fn would_allow_keeping(&self, airtime: Duration, reserve: Duration, now: Instant) -> bool {
        airtime <= MAX_TRANSMISSION && self.content(now) >= units(airtime + reserve)
    }

    /// The first instant at or after `now` when a transmission of
    /// `airtime` that leaves `reserve` will be allowed, if nothing else is
    /// sent meanwhile; `None` if it never can be.
    pub fn allowed_at(
        &self,
        airtime: Duration,
        reserve: Duration,
        now: Instant,
    ) -> Option<Instant> {
        let need = airtime + reserve;
        if airtime > MAX_TRANSMISSION || need > DEPTH {
            return None;
        }
        let short = units(need) - self.content(now);
        if short <= 0 {
            return Some(now);
        }
        Some(now + Duration::from_micros((short as u64).div_ceil(RATE_NUM)))
    }

    /// Record a transmission of `airtime` that started at `now`. Records it
    /// whether or not it was allowed: what went on air counts.
    pub fn record(&mut self, airtime: Duration, now: Instant) {
        self.units = self.content(now) - units(airtime);
        self.at = self.at.max(now);
    }

    /// Airtime in the bucket at `now`. Any transmission no longer than
    /// this, and than [`MAX_TRANSMISSION`], is allowed.
    pub fn available(&self, now: Instant) -> Duration {
        Duration::from_micros(self.content(now).max(0) as u64 / RATE_DEN)
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

    /// The most airtime in any interval of length `w`, from the exact
    /// intervals, which are in order and do not overlap. The worst interval
    /// ends at a transmission's end, so only those are tried.
    fn worst(txs: &[(u64, u64)], w: u64) -> u64 {
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

    fn bound(w: u64) -> u64 {
        most_on_air(Duration::from_micros(w)).as_micros()
    }

    #[test]
    fn constants() {
        assert_eq!(BUDGET, Duration::from_secs(360));
        assert_eq!(DEPTH, Duration::from_secs(6));
        assert_eq!(bound(DUTY_CYCLE_WINDOW.as_micros()), BUDGET.as_micros());
        assert_eq!(core::mem::size_of::<Budget>(), 16);
        let r = RATE_NUM as f64 / RATE_DEN as f64;
        assert!((r - 354.0 / 3600.0).abs() < 1e-12);
        // Worst silence: a full bucket's refill.
        let refill = DEPTH.as_micros() as f64 / r / 1e6;
        assert!((refill - 61.0).abs() < 0.1, "{refill}");
    }

    #[test]
    fn heartbeat_reserve_is_one_heartbeat() {
        assert_eq!(HEARTBEAT_RESERVE, Duration::from_micros(13_120));
    }

    #[test]
    fn the_wait_in_bans() {
        assert_eq!(wait_units(Duration::ZERO), 1, "a refusal is never 0");
        assert_eq!(wait_units(BAN.mul(2) - Duration::from_micros(1)), 1);
        assert_eq!(wait_units(BAN.mul(2)), 2, "rounded down");
        assert_eq!(wait_units(BAN.mul(63)), AUX_MASK);
        assert_eq!(wait_units(Duration::from_secs(1000)), AUX_MASK);
        let longest = told(AUX_MASK).as_micros();
        assert!((98_800_000..98_900_000).contains(&longest), "{longest}");
        assert_eq!(told(0), BAN, "never less than a ban");
    }

    #[test]
    fn reply_reserve_and_ban() {
        assert_eq!(ACK_AIRTIME, Duration::from_micros(15_680));
        assert_eq!(REPLY_RESERVE, Duration::from_micros(248_320));
        let ban = BAN.as_micros();
        assert!((1_568_000..1_569_000).contains(&ban), "{ban} µs");
        // The ban refills exactly a full frame and its ACK.
        let mut b = Budget::new();
        let t = Instant::from_micros(1_000_000);
        b.record(b.available(t), t);
        assert_eq!(
            b.allowed_at(crate::tables::RETRY_BACKOFF, Duration::ZERO, t),
            Some(t + BAN)
        );
    }

    #[test]
    fn the_reserve_is_left_for_heartbeats() {
        let reserve = HEARTBEAT_RESERVE;
        let mut b = Budget::new();
        let mut t = Instant::ZERO + Duration::from_secs(1000);
        // Spend all but the reserve and a little, in 100 ms frames.
        while b.would_allow_keeping(ms(100), reserve, t) {
            b.record(ms(100), t);
            t += ms(100);
        }
        assert!(b.available(t) >= reserve && b.available(t) < reserve + ms(100));
        assert!(
            !b.would_allow_keeping(ms(100), reserve, t),
            "forwarding stops"
        );
        assert!(b.would_allow(reserve, t), "a heartbeat still goes");
        b.record(reserve, t);
        // It is back well before the next heartbeat (spec "Heartbeat": 5 s at the least).
        let back = b.allowed_at(ms(0), reserve, t).unwrap();
        assert!(back.since(t) < ms(121), "{:?}", back.since(t));
    }

    #[test]
    fn allowed_at_is_exact() {
        let mut b = Budget::new();
        let mut t = Instant::from_micros(7_777_777);
        while b.would_allow(ms(900), t) {
            b.record(ms(900), t);
            t += ms(1000);
        }
        b.record(b.available(t), t);
        for (a, reserve) in [(ms(500), Duration::ZERO), (ms(139), ms(12)), (ms(1), ms(0))] {
            let at = b.allowed_at(a, reserve, t).unwrap();
            assert!(at > t);
            assert!(b.would_allow_keeping(a, reserve, at));
            let before = Instant::from_micros(at.as_micros() - 1);
            assert!(!b.would_allow_keeping(a, reserve, before));
        }
        assert_eq!(b.allowed_at(ms(1001), Duration::ZERO, t), None);
        assert_eq!(b.allowed_at(ms(1000), DEPTH, t), None);
        assert_eq!(b.allowed_at(ms(0), Duration::ZERO, t), Some(t));
    }

    #[test]
    fn spends_a_burst_and_refills_at_the_rate() {
        let mut b = Budget::new();
        let t0 = Instant::from_micros(10 * 3_600_000_000);
        assert_eq!(b.available(t0), DEPTH);
        assert!(!b.would_allow(ms(1001), t0), "single transmission cap");
        // A burst of the whole bucket, and what flows in meanwhile, back to
        // back.
        let mut t = t0;
        let mut sent = 0;
        while b.would_allow(ms(1000), t) {
            b.record(ms(1000), t);
            t += ms(1000);
            sent += 1;
        }
        assert_eq!(sent, 6);
        b.record(b.available(t), t);
        assert!(!b.would_allow(ms(1), t));
        // A full frame after 1.4 s, the whole bucket after 61 s.
        let frame = ms(139);
        let wait = b.allowed_at(frame, Duration::ZERO, t).unwrap().since(t);
        assert!(wait > ms(1400) && wait < ms(1420), "{wait:?}");
        let full = t + Duration::from_micros(DEPTH.as_micros() * RATE_DEN / RATE_NUM + 1);
        assert_eq!(b.available(full), DEPTH);
    }

    #[test]
    fn a_steady_sender_gets_the_rate() {
        // Send 140 ms frames whenever allowed for three hours: every hour
        // and every minute stays within the bound, the last hour carries
        // the rate's 354 s less a frame, and no wait is longer than a
        // frame's refill.
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
        for w in [60_000_000, DUTY_CYCLE_WINDOW.as_micros()] {
            assert!(worst(&txs, w) <= bound(w), "window {w} µs");
        }
        let last_hour = end.as_micros() - DUTY_CYCLE_WINDOW.as_micros();
        let spent: u64 = txs
            .iter()
            .filter(|&&(s, _)| s >= last_hour)
            .map(|&(_, a)| a)
            .sum();
        let floor = (BUDGET - DEPTH).as_micros() - frame.as_micros();
        assert!(spent >= floor, "spent {spent} µs, floor {floor}");
        let gap = txs
            .windows(2)
            .map(|w| w[1].0 - (w[0].0 + w[0].1))
            .max()
            .unwrap();
        assert!(gap <= 1_430_000, "{gap} µs");
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
            // budget allows is sent. No hour holds more than 10 %, and no
            // interval more than the bucket plus the rate.
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
            prop_assert!(worst(&txs, DUTY_CYCLE_WINDOW.as_micros()) <= BUDGET.as_micros());
            for w in [1_000_000, 60_000_000, 600_000_000] {
                prop_assert!(worst(&txs, w) <= bound(w));
            }
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

    #[test]
    fn a_hold_doubles_up_to_the_bucket_s_refill() {
        assert_eq!(hold(1), BAN);
        assert_eq!(hold(2), BAN.mul(2));
        assert_eq!(hold(3), BAN.mul(4));
        assert_eq!(hold(6), BAN.mul(32));
        assert_eq!(hold(7), refill_time(DEPTH));
        assert_eq!(hold(255), refill_time(DEPTH));
    }
}
