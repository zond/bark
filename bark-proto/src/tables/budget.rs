//! The duty-cycle budget (spec "Regulatory limits", "Duty cycle cap"): a
//! token bucket that keeps a node within [`DUTY_CYCLE_PERCENT`] of every
//! [`DUTY_CYCLE_WINDOW`], and no single transmission longer than
//! [`MAX_TRANSMISSION`]; its reserves; and the bans and told waits that
//! follow from its refill (spec "Bans").
//!
//! **Legality.** The bucket holds at most [`DEPTH`] and fills at
//! `r = (B − DEPTH) / W`, with `B` the [`BUDGET`] and `W` the window. Take
//! any interval `[T − L, T]`. If no transmission straddles its start,
//! everything on the air in it started in it and was taken from the
//! bucket: at most what the bucket held at `T − L`, at most `DEPTH`, plus
//! what flowed in meanwhile, `r·L`. If one transmission of airtime `a`
//! started at `s < T − L` and runs into the interval, count from `s`
//! instead: the bucket held at most `DEPTH − a` after it, so what started
//! in `(s, T]` is at most `DEPTH − a + r·(T − s)`, and the straddler adds
//! `s + a − (T − L)` inside the interval; the sum is
//! `DEPTH + L − (1 − r)·(T − s)`, at most `DEPTH + r·L` since `T − s > L`.
//! A transmission running past `T` only lowers the count. So any interval
//! of length `L` holds at most `DEPTH + r·L` on air, and any hour at most
//! `DEPTH + r·W = B`: 10 %, the limit of ERC/REC 70-03 Annex 1 band g1,
//! measured over one hour.

use crate::event::MAX_FRAME;
use crate::radio::{time_on_air, DUTY_CYCLE_PERCENT, DUTY_CYCLE_WINDOW, MIN_PREAMBLE_SYMBOLS};
use crate::tables::{retrying_span, ACK_TIMEOUT, HOP_ATTEMPTS, RETRY_BACKOFF};
use crate::time::{Duration, Instant};
use crate::wire::{ACK_LEN, AUX_MASK, HEARTBEAT_LEN};

/// Airtime allowed in any window: 10 % of an hour, 360 s.
pub const BUDGET: Duration =
    Duration::from_micros(DUTY_CYCLE_WINDOW.as_micros() * DUTY_CYCLE_PERCENT as u64 / 100);

/// The most airtime the bucket holds: one minute at the cap, 6 s (spec
/// "Duty cycle cap").
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

/// Airtime everything but a heartbeat leaves in the bucket: one
/// heartbeat's, 14.40 ms (spec "Duty cycle cap").
pub const HEARTBEAT_RESERVE: Duration = time_on_air(HEARTBEAT_LEN as u8, MIN_PREAMBLE_SYMBOLS);

/// Airtime of an ACK, taken or refused, 16.96 ms.
pub const ACK_AIRTIME: Duration = time_on_air(ACK_LEN as u8, MIN_PREAMBLE_SYMBOLS);

/// The airtime the bucket refills in `d`.
const fn refilled(d: Duration) -> Duration {
    Duration::from_micros(d.as_micros() * RATE_NUM / RATE_DEN)
}

/// The most a node may have on air in any interval of length `span`,
/// `DEPTH + r·span` (proof in the module documentation).
pub const fn most_on_air(span: Duration) -> Duration {
    Duration::from_micros(DEPTH.as_micros() + refilled(span).as_micros())
}

/// The time the bucket takes to refill `d`.
pub const fn refill_time(d: Duration) -> Duration {
    Duration::from_micros((d.as_micros() * RATE_DEN).div_ceil(RATE_NUM))
}

/// The longest ban, and the longest anything waits for a bucket: its
/// whole refill, 61.0 s (spec "Duty cycle cap", "Bans").
pub const LONGEST_BAN: Duration = refill_time(DEPTH);

/// The shortest time between a relay's heartbeats (spec "Heartbeat").
pub const HEARTBEAT_EVERY: Duration = Duration::from_secs(5);

/// Refusals the reply reserve holds: what a relay at its cap spends on
/// answering a DATA every frame-and-ACK time, and on its heartbeats,
/// beyond its refill, over [`LONGEST_BAN`]: 49 (spec "Duty cycle cap").
pub const REPLIES: u64 = {
    let span = LONGEST_BAN.as_micros();
    let beats = span.div_ceil(HEARTBEAT_EVERY.as_micros());
    let beat_air = beats * HEARTBEAT_RESERVE.as_micros();
    let per_data =
        time_on_air(MAX_FRAME as u8, MIN_PREAMBLE_SYMBOLS).as_micros() + ACK_AIRTIME.as_micros();
    let data = (span - beat_air) / per_data;
    let spent = data * ACK_AIRTIME.as_micros() + beat_air;
    let short = spent - refilled(LONGEST_BAN).as_micros();
    short.div_ceil(ACK_AIRTIME.as_micros())
};

/// Replies the bucket refills while a sender retries a hand-over that
/// goes unanswered, 2.41 s: 13 of 16.96 ms (`tables::DEMOTE_AFTER_FAILURES`).
pub const RETRY_REPLIES: u64 = refilled(retrying_span(ACK_TIMEOUT, HOP_ATTEMPTS, RETRY_BACKOFF))
    .as_micros()
    / ACK_AIRTIME.as_micros();

/// Airtime below which a relay takes no new work, [`REPLIES`] and a
/// heartbeat: 845.4 ms (spec "Duty cycle cap", "Receiving DATA").
pub const REPLY_RESERVE: Duration =
    Duration::from_micros(REPLIES * ACK_AIRTIME.as_micros() + HEARTBEAT_RESERVE.as_micros());

/// The shortest ban, and the unit of the wait a refusal tells: the refill
/// of a full frame and its ACK, 155.52 ms at 9.83 %, 1.58 s (spec "Bans").
pub const BAN: Duration = refill_time(Duration::from_micros(
    time_on_air(MAX_FRAME as u8, MIN_PREAMBLE_SYMBOLS).as_micros() + ACK_AIRTIME.as_micros(),
));

const _: () = assert!(REPLIES == 49 && RETRY_REPLIES == 13);

/// How long a sender holds a message after the `refusals`-th refusal in a
/// row before offering it again: one [`BAN`], doubling up to
/// [`LONGEST_BAN`] (spec "Bans").
pub const fn hold(refusals: u8) -> Duration {
    let cap = LONGEST_BAN.as_micros();
    let doublings = refusals.saturating_sub(1);
    let mut wait = BAN.as_micros();
    let mut k = 0;
    while k < doublings && wait < cap {
        wait *= 2;
        k += 1;
    }
    Duration::from_micros(if wait < cap { wait } else { cap })
}

/// The step of a told wait above the first codes, a part of the wait
/// below it (spec "Bans").
const STEP_PARTS: u64 = 8;

/// How many bans the ACK's wait code `v` means (spec "Bans"). Steps of an
/// eighth reach 3384 bans, 89 min, more than an hour; a ninth would reach
/// only 44 min.
const TOLD_BANS: [u64; AUX_MASK as usize + 1] = {
    let mut t = [0; AUX_MASK as usize + 1];
    let mut v = 1;
    while v <= AUX_MASK as usize {
        let last = t[v - 1];
        let step = last / STEP_PARTS;
        t[v] = if (v as u64) <= STEP_PARTS || step == 0 {
            last + 1
        } else {
            last + step
        };
        v += 1;
    }
    t
};

const _: () = assert!(TOLD_BANS[16] == 16 && TOLD_BANS[AUX_MASK as usize] == 3384);

/// The wait a refusal telling code `code` means (spec "Bans"): never
/// less than one [`BAN`].
pub fn told(code: u8) -> Duration {
    BAN.mul(TOLD_BANS[code.clamp(1, AUX_MASK) as usize])
}

/// The longest wait a refusal tells: code 63, 3384 bans, 89 min.
pub const LONGEST_TOLD: Duration = BAN.mul(TOLD_BANS[AUX_MASK as usize]);

/// The code of the longest told wait no longer than `wait`, so that a
/// wait passed on from relay to relay only shrinks; at least 1, since 0
/// in an ACK means taken.
pub fn wait_units(wait: Duration) -> u8 {
    let bans = wait.as_micros() / BAN.as_micros();
    let below = TOLD_BANS.iter().rposition(|&b| b <= bans).unwrap_or(1);
    below.clamp(1, AUX_MASK as usize) as u8
}

/// The shortest told wait no shorter than `wait`, so that a sender given a
/// place never comes back before it; at most [`LONGEST_TOLD`].
pub fn told_up(wait: Duration) -> Duration {
    let bans = wait.as_micros().div_ceil(BAN.as_micros());
    let code = TOLD_BANS
        .iter()
        .position(|&b| b >= bans)
        .unwrap_or(AUX_MASK as usize);
    told(code as u8)
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

    /// When the bucket will have refilled `need` beyond what it holds at
    /// `now`, its depth aside: by then a node that sends as the bucket
    /// allows has sent that much, if nothing else comes.
    pub fn refilled_at(&self, need: Duration, now: Instant) -> Instant {
        let short = units(need) - self.content(now);
        if short <= 0 {
            return now;
        }
        now + Duration::from_micros((short as u64).div_ceil(RATE_NUM))
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
        assert_eq!(HEARTBEAT_RESERVE, Duration::from_micros(14_400));
    }

    /// Past the bucket's depth, what a node needs is refilled at the
    /// bucket's rate all the same: twice the depth takes two refills from
    /// empty, less what the bucket holds.
    #[test]
    fn a_need_past_the_depth_is_a_longer_refill() {
        let mut b = Budget::new();
        let t = Instant::ZERO;
        assert_eq!(b.refilled_at(DEPTH, t), t, "a full bucket holds its depth");
        b.record(DEPTH, t);
        let one = b.refilled_at(DEPTH, t).since(t).as_micros();
        let two = b.refilled_at(DEPTH.mul(2), t).since(t).as_micros();
        assert!(one.abs_diff(LONGEST_BAN.as_micros()) <= 1 && two.abs_diff(2 * one) <= 1);
        assert_eq!(b.allowed_at(ms(0), DEPTH, t), Some(b.refilled_at(DEPTH, t)));
    }

    #[test]
    fn the_wait_in_bans() {
        assert_eq!(wait_units(Duration::ZERO), 1, "a refusal is never 0");
        assert_eq!(wait_units(BAN.mul(2) - Duration::from_micros(1)), 1);
        assert_eq!(wait_units(BAN.mul(2)), 2, "rounded down");
        assert_eq!(wait_units(BAN.mul(16)), 16, "one ban apart to 16");
        assert_eq!(wait_units(BAN.mul(17)), 16, "then an eighth apart");
        assert_eq!(wait_units(BAN.mul(18)), 17);
        assert_eq!(wait_units(LONGEST_TOLD), AUX_MASK);
        assert_eq!(wait_units(Duration::from_secs(100_000)), AUX_MASK);
        let longest = told(AUX_MASK).as_micros() / 1_000_000;
        assert!((5340..5360).contains(&longest), "{longest}");
        assert_eq!(told(0), BAN, "never less than a ban");
        assert_eq!(told_up(Duration::ZERO), BAN);
        assert_eq!(told_up(BAN.mul(17)), BAN.mul(18), "rounded up");
        assert_eq!(told_up(Duration::from_secs(100_000)), LONGEST_TOLD);
        // A told wait is told as itself, and each code says more.
        for v in 1..=AUX_MASK {
            assert_eq!(wait_units(told(v)), v);
            assert_eq!(told_up(told(v)), told(v));
            if v > 1 {
                let (a, b) = (told(v - 1).as_micros(), told(v).as_micros());
                assert!(b > a && b - a <= (a / 8).max(BAN.as_micros()));
            }
        }
    }

    #[test]
    fn reply_reserve_and_ban() {
        assert_eq!(ACK_AIRTIME, Duration::from_micros(16_960));
        assert_eq!(REPLY_RESERVE, Duration::from_micros(845_440));
        let ban = BAN.as_micros();
        assert!((1_581_000..1_582_000).contains(&ban), "{ban} µs");
        // The ban refills exactly a full frame and its ACK.
        let mut b = Budget::new();
        let t = Instant::from_micros(1_000_000);
        b.record(b.available(t), t);
        assert_eq!(
            b.allowed_at(crate::tables::RETRY_BACKOFF, Duration::ZERO, t),
            Some(t + BAN)
        );
    }

    /// A relay at its cap, the bucket at the reply reserve, receives a DATA
    /// every full frame and ACK and sends a heartbeat every 5 s: it
    /// answers every one for the longest ban, and not much longer.
    #[test]
    fn the_reply_reserve_lasts_a_longest_ban_under_siege() {
        let per_data = time_on_air(MAX_FRAME as u8, MIN_PREAMBLE_SYMBOLS) + ACK_AIRTIME;
        let start = Instant::ZERO + Duration::from_secs(1000);
        let mut b = Budget::new();
        b.record(b.available(start) - REPLY_RESERVE, start);
        let siege = |b: &mut Budget, span: Duration| {
            let mut t = start;
            let mut next_beat = start + HEARTBEAT_EVERY;
            while t < start + span {
                t += per_data;
                if t >= next_beat {
                    assert!(b.would_allow(HEARTBEAT_RESERVE, t), "heartbeat");
                    b.record(HEARTBEAT_RESERVE, t);
                    next_beat += HEARTBEAT_EVERY;
                }
                if !b.would_allow_keeping(ACK_AIRTIME, HEARTBEAT_RESERVE, t) {
                    return false;
                }
                b.record(ACK_AIRTIME, t);
            }
            true
        };
        assert!(siege(&mut b.clone(), LONGEST_BAN), "silent within the ban");
        assert!(
            !siege(&mut b, LONGEST_BAN.mul(2)),
            "a reserve that never ends"
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
        // Back well before the next heartbeat, 5 s at the least.
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
            // Demand well above the cap with occasional long silences: no
            // hour holds more than 10 %, no interval more than the bound.
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
