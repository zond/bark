//! Duty-cycled receive and the long preamble (spec "Physical layer",
//! "Sleepy nodes").
//!
//! A sleepy node keeps its receiver, between its windows, in the SX1262's
//! duty-cycled receive (SX1261/2 datasheet Rev 1.2, §13.1.7,
//! SetRxDutyCycle): the radio receives for [`SNIFF_RX`], sleeps for
//! [`SNIFF_SLEEP`], wakes, and so on. A frame is caught only if its
//! preamble covers enough of a receive window to be detected, so frames to
//! such a node carry the [`LONG_PREAMBLE_SYMBOLS`] preamble. Every number
//! here follows from the datasheet, the symbol time and the anchorless
//! beat period; the tests below check each step.
//!
//! # The two conditions
//!
//! The radio goes round a cycle of `C = rx + sleep + wake`: receive for
//! `rx`, about 1 ms to save its state and later restore it, the sleep, and
//! the TCXO's start before the next receive (§13.1.7, Figures 13-2 and
//! 13-3). A preamble of length `L` starting at an arbitrary time is
//! detected if `D` of it falls inside one receive window. The worst start
//! is just too late for one window, so the next must still hold `D` of it:
//!
//! ```text
//! L ≥ C − rx + 2D = sleep + wake + 2D                          (caught)
//! ```
//!
//! Once it detects a preamble the radio waits `2 rx + sleep` for the header
//! before it goes back to its cycle (§13.1.7: "Tpreamble + Theader ≤ 2 *
//! rxPeriod + sleepPeriod"):
//!
//! ```text
//! L + T_header ≤ 2 rx + sleep                                  (kept)
//! ```
//!
//! With `L` at its least, the two together need
//! `rx ≥ (wake + 2D + T_header) / 2`, whatever the sleep.
//!
//! # The cycle
//!
//! The node pays for its receive windows, `I_rx (rx + wake) / C`, and for
//! its own beats, each a long preamble of about `C` every beat period `P`:
//! `I_tx C / P`. The sum is least at `C = √(I_rx (rx + wake) P / I_tx)`,
//! where the two are equal. Longer sleeps make every frame to the node
//! longer; shorter ones make it listen more.

use crate::radio::{header_end, CHANNELS, SYMBOL_TIME};
use crate::time::Duration;

/// Preamble symbols the receiver needs inside a receive window to detect
/// a preamble: 4, the time spec "Busy estimate" takes for the modem to
/// lock on. To be measured in bring-up step 1.
pub const DETECT_SYMBOLS: u64 = 4;

/// From the end of a sleep to the start of the receive window: about
/// 1 ms to save and restore the radio's state (datasheet §13.1.7, Figure
/// 13-2: "around 1 ms"), and the TCXO's start, which the chip adds before
/// each receive (Figure 13-3 note), 5 ms as the firmware programs it
/// (`bark-fw/src/radio.rs`, `TCXO_START`).
pub const SNIFF_WAKE: Duration = Duration::from_millis(6);

/// The step of the duty cycle's periods that is a whole number of
/// microseconds: the datasheet counts them in 15.625 µs (§13.1.7), and
/// eight of those are 125 µs.
pub const SNIFF_STEP: Duration = Duration::from_micros(125);

/// The radio's current while it receives, µA: the figure the spec and the
/// notes use for listening (spec "Delivery"), above the datasheet's
/// 5.3 mA in boosted receive (Table 3-5). The wake is counted at it too.
/// To be replaced by the bench's current measurement.
pub const RX_CURRENT_UA: u64 = 5_800;

/// The radio's current while it transmits, µA: 32 mA, the least the
/// datasheet gives for the SX1262 at 434 MHz (Table 3-6, +14 dBm with its
/// optimal settings); at the +10 dBm we send it is less.
pub const TX_CURRENT_UA: u64 = 32_000;

/// The radio's current asleep between receive windows, nA: 1.2 µA with
/// its state kept and its 64 kHz clock running (Table 3-5).
pub const SLEEP_CURRENT_NA: u64 = 1_200;

/// How often a sleepy node without an anchor beats, at most (spec
/// "Sleepy nodes alone"): the delivery TTL over the channels, 46.2 s, so
/// that beats on the next channel each time visit every channel within a
/// TTL, before anything the network holds for the node runs out.
pub const fn anchorless_beat_period(delivery_ttl: Duration) -> Duration {
    delivery_ttl.div(CHANNELS as u64)
}

/// The beat interval the cycle is set for
/// (spec "Duty-cycled receive and the long preamble"): the anchorless
/// beat period it was first derived with. One cycle serves every sleepy node; at the mean anchorless
/// interval of 34.6 s the least would be 274 ms, and this one costs 4 %
/// more there.
pub const CYCLE_BEAT_INTERVAL: Duration = Duration::from_secs(60);

const fn ceil_to(d: u64, step: u64) -> u64 {
    d.div_ceil(step) * step
}

const fn isqrt(n: u128) -> u64 {
    // Newton's method from above; exact for every u128 we pass.
    if n < 2 {
        return n as u64;
    }
    let mut x = n;
    let mut y = x.div_ceil(2);
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x as u64
}

/// `D`, the preamble detected inside a window.
const DETECT: u64 = DETECT_SYMBOLS * SYMBOL_TIME.as_micros();

/// From the end of the preamble to the end of the header, 6.25 symbols of
/// sync and the 8 of the header block (§6.1.4).
const HEADER: u64 = header_end(0).as_micros();

/// The receive window: half the wake, two detections and the header,
/// and half a symbol, so that a preamble rounded up to a whole symbol
/// still meets the second condition (module documentation); in steps.
pub const SNIFF_RX: Duration = Duration::from_micros(ceil_to(
    (SNIFF_WAKE.as_micros() + 2 * DETECT + HEADER + SYMBOL_TIME.as_micros()).div_ceil(2),
    SNIFF_STEP.as_micros(),
));

/// The sleep between windows: the cycle at which the receive windows cost
/// what the node's own beats do (module documentation), less the window
/// and the wake, in whole steps.
pub const SNIFF_SLEEP: Duration = {
    let on = (SNIFF_RX.as_micros() + SNIFF_WAKE.as_micros()) as u128;
    let cycle = isqrt(
        RX_CURRENT_UA as u128 * on * CYCLE_BEAT_INTERVAL.as_micros() as u128
            / TX_CURRENT_UA as u128,
    );
    let sleep = cycle - SNIFF_RX.as_micros() - SNIFF_WAKE.as_micros();
    Duration::from_micros(sleep / SNIFF_STEP.as_micros() * SNIFF_STEP.as_micros())
};

/// One cycle of duty-cycled receive: window, sleep and wake.
pub const SNIFF_CYCLE: Duration =
    Duration::from_micros(SNIFF_RX.as_micros() + SNIFF_SLEEP.as_micros() + SNIFF_WAKE.as_micros());

/// The preamble of every frame to a sleepy node outside its windows, and
/// of the beats of one without an anchor: the shortest that a duty-cycled
/// receiver always catches,
/// the sleep, the wake and two detections, in whole symbols.
pub const LONG_PREAMBLE_SYMBOLS: u16 =
    (SNIFF_SLEEP.as_micros() + SNIFF_WAKE.as_micros() + 2 * DETECT)
        .div_ceil(SYMBOL_TIME.as_micros()) as u16;

/// The radio's mean current in duty-cycled receive, nA: the window and
/// the wake at [`RX_CURRENT_UA`], the sleep at [`SLEEP_CURRENT_NA`].
pub const SNIFF_CURRENT_NA: u64 =
    ((SNIFF_RX.as_micros() + SNIFF_WAKE.as_micros()) * RX_CURRENT_UA * 1000
        + SNIFF_SLEEP.as_micros() * SLEEP_CURRENT_NA)
        / SNIFF_CYCLE.as_micros();

/// Whether a duty-cycled receiver whose cycle began at `t0` (its first
/// window opening then) detects a preamble of `preamble` that starts at
/// `start`, and when: once [`DETECT_SYMBOLS`] of it have been inside one
/// window. All times in µs.
pub fn detects(t0: u64, start: u64, preamble: Duration) -> Option<u64> {
    let (cycle, rx) = (SNIFF_CYCLE.as_micros(), SNIFF_RX.as_micros());
    let end = start + preamble.as_micros();
    // The first window that still has room for a detection at or after
    // the start: its close is at least `DETECT` past the start.
    let need = (start + DETECT).max(t0 + rx) - rx;
    let k = (need - t0).div_ceil(cycle);
    let open = t0 + k * cycle;
    let at = open.max(start) + DETECT;
    (at <= end && at <= open + rx).then_some(at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::radio::{time_on_air, MIN_PREAMBLE_SYMBOLS};

    #[test]
    fn the_numbers() {
        assert_eq!(SNIFF_RX.as_micros(), 6_000);
        assert_eq!(SNIFF_SLEEP.as_micros(), 349_125);
        assert_eq!(SNIFF_CYCLE.as_micros(), 361_125);
        assert_eq!(LONG_PREAMBLE_SYMBOLS, 1396);
        // 357.4 ms of preamble; a heartbeat 366.1 ms, a full frame 492.9 ms.
        assert_eq!(
            SYMBOL_TIME.mul(LONG_PREAMBLE_SYMBOLS as u64).as_micros(),
            357_376
        );
        assert_eq!(time_on_air(9, LONG_PREAMBLE_SYMBOLS).as_micros(), 366_144);
        assert_eq!(time_on_air(255, LONG_PREAMBLE_SYMBOLS).as_micros(), 492_864);
        // 0.194 mA: 30 times less than listening.
        assert_eq!(SNIFF_CURRENT_NA, 193_891);
    }

    #[test]
    fn both_conditions_hold() {
        let l = SYMBOL_TIME.mul(LONG_PREAMBLE_SYMBOLS as u64).as_micros();
        let (rx, sleep, wake) = (
            SNIFF_RX.as_micros(),
            SNIFF_SLEEP.as_micros(),
            SNIFF_WAKE.as_micros(),
        );
        assert!(l >= sleep + wake + 2 * DETECT, "caught");
        assert!(l + HEADER <= 2 * rx + sleep, "kept");
        // Every period is a whole number of the radio's 15.625 µs steps.
        for p in [rx, sleep] {
            assert_eq!(p * 64 % 1000, 0, "{p}");
        }
        // A transmission stays under the second the regulations allow it
        // (spec "Regulatory limits").
        assert!(time_on_air(255, LONG_PREAMBLE_SYMBOLS) < Duration::from_secs(1));
    }

    #[test]
    fn the_cycle_balances_windows_against_beats() {
        let c = SNIFF_CYCLE.as_micros() as f64;
        let on = (SNIFF_RX + SNIFF_WAKE).as_micros() as f64;
        let p = CYCLE_BEAT_INTERVAL.as_micros() as f64;
        let cost = |c: f64| RX_CURRENT_UA as f64 * on / c + TX_CURRENT_UA as f64 * c / p;
        assert!(cost(c) <= cost(c * 0.9) && cost(c) <= cost(c * 1.1));
        // Within 7 % of the least between 250 and 520 ms: the choice is flat.
        for t in [250e3, 520e3] {
            assert!(cost(t) < cost(c) * 1.07, "{t}");
        }
        // At the mean anchorless interval, three quarters of the period:
        // the least there is 274 ms, and this cycle costs 4 % more.
        let ttl = crate::node::Config::default().delivery_ttl;
        let p = anchorless_beat_period(ttl).as_micros() as f64 * 0.75;
        let cost = |c: f64| RX_CURRENT_UA as f64 * on / c + TX_CURRENT_UA as f64 * c / p;
        let least = (RX_CURRENT_UA as f64 * on * p / TX_CURRENT_UA as f64).sqrt();
        assert!((least / 1e3 - 274.0).abs() < 1.0, "{least}");
        assert!((cost(c) / cost(least) - 1.04).abs() < 0.005);
    }

    #[test]
    fn a_long_preamble_is_caught_wherever_it_starts() {
        let pre = SYMBOL_TIME.mul(LONG_PREAMBLE_SYMBOLS as u64);
        let t0 = 1_000_000;
        for off in (0..2 * SNIFF_CYCLE.as_micros()).step_by(97) {
            let at = detects(t0, t0 + off, pre).expect("caught");
            assert!(at >= t0 + off + DETECT && at <= t0 + off + pre.as_micros());
        }
    }

    #[test]
    fn a_short_preamble_is_caught_only_in_a_window() {
        let pre = SYMBOL_TIME.mul(MIN_PREAMBLE_SYMBOLS as u64);
        let t0 = 0;
        let cycle = SNIFF_CYCLE.as_micros();
        let caught = (0..cycle)
            .step_by(5)
            .filter(|&s| detects(t0, cycle + s, pre).is_some())
            .count() as f64;
        // A window of 6 ms and 3.07 ms of preamble: caught when it starts
        // from 2.05 ms before a window opens to 1.02 ms before it closes.
        let expected = (SNIFF_RX.as_micros() + pre.as_micros() - 2 * DETECT) as f64 / cycle as f64;
        assert!((caught / (cycle / 5) as f64 - expected).abs() < 0.001);
    }
}
