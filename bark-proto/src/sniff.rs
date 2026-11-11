//! Duty-cycled receive and the long preamble
//! (spec "Duty-cycled receive and the long preamble"): the periods of the
//! SX1262's duty-cycled receive (SX1261/2 datasheet Rev 1.2, §13.1.7,
//! SetRxDutyCycle) and the preamble a receiver in it always catches. The
//! tests check each number.
//!
//! A preamble of length `L` is caught if `D` of it falls inside one
//! window wherever it starts, and once detected the radio waits
//! `2 rx + sleep` for the header (§13.1.7):
//!
//! ```text
//! L ≥ sleep + wake + 2D                                        (caught)
//! L + T_header ≤ 2 rx + sleep                                  (kept)
//! ```

use crate::radio::{header_end, CHANNELS, SYMBOL_TIME};
use crate::time::Duration;

/// Preamble symbols a receive window must hold to detect a preamble: the
/// modem's lock time of spec "Busy estimate". Not measured yet.
pub const DETECT_SYMBOLS: u64 = 4;

/// From the end of a sleep to the start of the receive window, less the
/// oscillator's start: about 1 ms to save and restore the radio's state
/// (datasheet §13.1.7, Figure 13-2: "around 1 ms"). Not measured on the
/// board.
pub const SNIFF_RESTORE: Duration = Duration::from_millis(1);

/// The oscillator's start the cycle is derived for (Figure 13-3 note): the
/// least the boot probe (`bark-driver/src/tcxo.rs`) can land on, a
/// quarter-millisecond step and one of margin, where the bench's board
/// lands at room temperature. A longer delay runs [`periods`].
pub const SNIFF_TCXO_START: Duration = Duration::from_micros(500);

/// The wake the cycle is derived for: the state restored and the
/// oscillator started.
pub const SNIFF_WAKE: Duration =
    Duration::from_micros(SNIFF_RESTORE.as_micros() + SNIFF_TCXO_START.as_micros());

/// The step of the duty cycle's periods that is a whole number of
/// microseconds: the datasheet counts them in 15.625 µs (§13.1.7), and
/// eight of those are 125 µs.
pub const SNIFF_STEP: Duration = Duration::from_micros(125);

/// The radio's current while it receives and wakes, µA: the listening
/// figure of spec "Delivery", above the datasheet's 5.3 mA in boosted
/// receive (Table 3-5). Not measured yet.
pub const RX_CURRENT_UA: u64 = 5_800;

/// The radio's current while it transmits, µA: 32 mA, the least the
/// datasheet gives for the SX1262 at 434 MHz (Table 3-6, +14 dBm with its
/// optimal settings); at the +10 dBm we send it is less.
pub const TX_CURRENT_UA: u64 = 32_000;

/// The radio's current asleep between receive windows, nA: 1.2 µA with
/// its state kept and its 64 kHz clock running (Table 3-5).
pub const SLEEP_CURRENT_NA: u64 = 1_200;

/// How often a sleepy node without an anchor beats, at most (spec
/// "Sleepy nodes alone"): the delivery TTL over the channels, so its beats
/// visit every channel before anything held for it runs out.
pub const fn anchorless_beat_period(delivery_ttl: Duration) -> Duration {
    delivery_ttl.div(CHANNELS as u64)
}

/// The beat interval the cycle is set for
/// (spec "Duty-cycled receive and the long preamble"). One cycle serves
/// every sleepy node; at the mean anchorless interval it costs 4 % more
/// than the least there.
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

/// The receive window for a wake: half the wake, two detections and the
/// header, and half a symbol, so that a preamble rounded up to a whole
/// symbol is still kept (module documentation); in steps.
const fn window(wake: Duration) -> Duration {
    Duration::from_micros(ceil_to(
        (wake.as_micros() + 2 * DETECT + HEADER + SYMBOL_TIME.as_micros()).div_ceil(2),
        SNIFF_STEP.as_micros(),
    ))
}

/// The receive window at the wake the cycle is derived for.
pub const SNIFF_RX: Duration = window(SNIFF_WAKE);

/// The sleep between windows: the cycle at which the windows cost what
/// the node's own beats do, less the window and the wake, in whole steps.
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

/// How long a receiver is deaf between two windows, the sleep and the
/// wake: what the long preamble spans, the same for every node whatever
/// its oscillator's start.
pub const SNIFF_GAP: Duration =
    Duration::from_micros(SNIFF_SLEEP.as_micros() + SNIFF_WAKE.as_micros());

/// The window and the sleep for an oscillator delay of `tcxo_start`: the
/// excess over [`SNIFF_TCXO_START`] comes off the sleep, so the gap stays
/// [`SNIFF_GAP`] (caught), and half of it widens the window (kept). A
/// shorter delay takes the derived periods: the gap may shrink, the
/// preamble does not.
pub const fn periods(tcxo_start: Duration) -> (Duration, Duration) {
    if tcxo_start.as_micros() <= SNIFF_TCXO_START.as_micros() {
        return (SNIFF_RX, SNIFF_SLEEP);
    }
    let wake = SNIFF_RESTORE.as_micros() + tcxo_start.as_micros();
    let sleep = SNIFF_GAP.as_micros().saturating_sub(wake) / SNIFF_STEP.as_micros()
        * SNIFF_STEP.as_micros();
    (
        window(Duration::from_micros(wake)),
        Duration::from_micros(sleep),
    )
}

/// The preamble of every frame to a sleepy node outside its windows, and
/// of the beats of one without an anchor: the shortest that a duty-cycled
/// receiver always catches, the gap and two detections, in whole symbols.
pub const LONG_PREAMBLE_SYMBOLS: u16 =
    (SNIFF_GAP.as_micros() + 2 * DETECT).div_ceil(SYMBOL_TIME.as_micros()) as u16;

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
        assert_eq!(SNIFF_WAKE.as_micros(), 1_500);
        assert_eq!(SNIFF_RX.as_micros(), 3_750);
        assert_eq!(SNIFF_SLEEP.as_micros(), 233_625);
        assert_eq!(SNIFF_CYCLE.as_micros(), 238_875);
        assert_eq!(LONG_PREAMBLE_SYMBOLS, 927);
        // 237.3 ms of preamble; a heartbeat 248.6 ms, a full frame 372.8 ms.
        assert_eq!(
            SYMBOL_TIME.mul(LONG_PREAMBLE_SYMBOLS as u64).as_micros(),
            237_312
        );
        assert_eq!(
            time_on_air(crate::wire::HEARTBEAT_LEN as u8, LONG_PREAMBLE_SYMBOLS).as_micros(),
            248_640
        );
        assert_eq!(time_on_air(255, LONG_PREAMBLE_SYMBOLS).as_micros(), 372_800);
        // 0.129 mA: 45 times less than listening.
        assert_eq!(SNIFF_CURRENT_NA, 128_646);
    }

    /// The window is bound by the header's wait and the two detections,
    /// not by the wake: without any wake it would still be 3 ms.
    #[test]
    fn the_window_is_bound_by_the_header_not_the_wake() {
        assert_eq!(window(Duration::ZERO).as_micros(), 3_000);
        assert_eq!(window(SNIFF_WAKE).as_micros(), 3_750);
    }

    /// Both conditions hold at every delay the probe can land on, in its
    /// quarter-millisecond steps up to 4 ms.
    #[test]
    fn a_longer_oscillator_start_keeps_the_gap_and_both_conditions() {
        let l = SYMBOL_TIME.mul(LONG_PREAMBLE_SYMBOLS as u64).as_micros();
        for step in 1..=16u64 {
            let delay = Duration::from_micros(250 * step);
            let (rx, sleep) = periods(delay);
            let wake =
                SNIFF_RESTORE.as_micros() + delay.as_micros().max(SNIFF_TCXO_START.as_micros());
            assert_eq!(sleep.as_micros() + wake, SNIFF_GAP.as_micros(), "{step}");
            assert!(l >= sleep.as_micros() + wake + 2 * DETECT, "caught {step}");
            assert!(
                l + HEADER <= 2 * rx.as_micros() + sleep.as_micros(),
                "kept {step}"
            );
            for p in [rx, sleep] {
                assert_eq!(p.as_micros() * 64 % 1000, 0, "{step}");
            }
        }
        assert_eq!(periods(SNIFF_TCXO_START), (SNIFF_RX, SNIFF_SLEEP));
        assert_eq!(periods(Duration::from_micros(250)), (SNIFF_RX, SNIFF_SLEEP));
        let (rx, sleep) = periods(Duration::from_millis(4));
        assert_eq!((rx.as_micros(), sleep.as_micros()), (5_500, 230_125));
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
        // Within 7 % of the least between 165 and 345 ms: the choice is flat.
        for t in [165e3, 345e3] {
            assert!(cost(t) < cost(c) * 1.07, "{t}");
        }
        // At the mean anchorless interval, three quarters of the period:
        // the least there is 181 ms, and this cycle costs 4 % more.
        let ttl = crate::node::Config::default().delivery_ttl;
        let p = anchorless_beat_period(ttl).as_micros() as f64 * 0.75;
        let cost = |c: f64| RX_CURRENT_UA as f64 * on / c + TX_CURRENT_UA as f64 * c / p;
        let least = (RX_CURRENT_UA as f64 * on * p / TX_CURRENT_UA as f64).sqrt();
        assert!((least / 1e3 - 181.0).abs() < 1.0, "{least}");
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
        // A window of 3.75 ms and 3.07 ms of preamble: caught when it
        // starts from 2.05 ms before a window opens to 1.02 ms before it
        // closes, 2.0 % of the cycle.
        let expected = (SNIFF_RX.as_micros() + pre.as_micros() - 2 * DETECT) as f64 / cycle as f64;
        assert!((caught / (cycle / 5) as f64 - expected).abs() < 0.001);
    }
}
