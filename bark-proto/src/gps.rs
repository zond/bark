//! The board's GPS receiver, a Quectel L76K, and when a node runs it
//! (spec "Position", "Beats").
//!
//! The figures are the L76K Hardware Design V1.0 (Quectel, 2021-10-20),
//! Table 2 (page 11), at room temperature with every satellite at
//! −130 dBm, about the open-sky level.

use crate::time::Duration;

/// How far a fix may lie from the nearest relay the node knows before it
/// is dropped as unreliable, metres: the hearing radius, 3 km rooftop to
/// rooftop (spec "Physical layer", "Position"). No relay a node hears is
/// farther.
pub const HEARING_RADIUS_M: f32 = 3000.0;

/// The receiver's current while it acquires or tracks, µA: 29 mA in
/// either (Table 2).
pub const GPS_CURRENT_UA: u64 = 29_000;

/// The receiver's current in standby, µA: 20 µA (Table 2), with its RF
/// off and its RAM and clock kept (§3.3.3), which a hot start needs.
pub const GPS_STANDBY_UA: u64 = 20;

/// The time to a first fix of a hot start, with the satellites' orbits
/// and the time known: 2 s (Table 2).
pub const HOT_START: Duration = Duration::from_secs(2);

/// The time to a first fix of a cold start, without them: 30 s (Table 2).
/// The orbit repeats every 30 s (IS-GPS-200, §20.3.2: a frame of five 6 s
/// subframes).
pub const COLD_START: Duration = Duration::from_secs(30);

/// How long a satellite's broadcast orbit serves for a hot start: the
/// four hours each set of orbit parameters is fitted over (IS-GPS-200,
/// §20.3.3.4.3.1, the fit interval). Older, the receiver reads it afresh.
pub const ORBIT_VALID: Duration = Duration::from_secs(4 * 3600);

/// How long a search may run before it is given up: two 30 s orbit
/// frames, so a cold start survives one frame lost to a bit error. Past
/// that the node is most likely indoors, below the −148 dBm the L76K
/// needs to acquire (Table 2).
pub const SEARCH_LIMIT: Duration = COLD_START.mul(2);

/// How long a sleepy node runs no search after one that found nothing:
/// [`SEARCH_LIMIT`] at [`GPS_CURRENT_UA`] is 0.48 mAh, and once in 9 h,
/// 0.054 mA, keeps a node that never sees the sky within a tenth of
/// [`SLEEPY_CURRENT_UA`]; 8.6 h is the least, rounded up to the hour.
pub const BACKOFF: Duration = Duration::from_secs(9 * 3600);

/// How long a relay on external power stands still, fixed at a place it
/// took, before its GPS rests: a day, the owner's choice (roadmap D250).
/// Running, the GPS draws 38 mA, a node on USB 54 mA against 16 mA with
/// it at rest (`docs/solar-test-2026-11-24.md`): more than a small solar
/// unit gives back in winter.
pub const STILL_REST: Duration = Duration::from_secs(24 * 3600);

/// How often a still relay's resting GPS searches: every half hour in its
/// second still day, every hour after [`STILL_SLOWER`], the owner's
/// choices. At the measured 38 mA a search of [`SEARCH_LIMIT`], its worst
/// case, adds 1.3 and 0.6 mA.
pub const STILL_SEARCH_EVERY: [Duration; 2] =
    [Duration::from_secs(30 * 60), Duration::from_secs(3600)];

/// When a still relay's searches slow to the second of
/// [`STILL_SEARCH_EVERY`]: after two still days.
pub const STILL_SLOWER: Duration = STILL_REST.mul(2);

/// A sleepy node's mean current with an anchor, µA, as measured in the
/// simulator (spec "Delivery").
pub const SLEEPY_CURRENT_UA: u64 = 560;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_back_off_keeps_a_node_without_sky_within_a_tenth() {
        let search_uas = GPS_CURRENT_UA * SEARCH_LIMIT.as_micros() / 1_000_000;
        let mean_ua = search_uas as f64 / (BACKOFF.as_micros() as f64 / 1e6);
        assert!(mean_ua <= SLEEPY_CURRENT_UA as f64 / 10.0, "{mean_ua}");
        // 8.6 h is the least that keeps it.
        let least_s = search_uas as f64 / (SLEEPY_CURRENT_UA as f64 / 10.0);
        assert!((least_s / 3600.0 - 8.6).abs() < 0.05, "{least_s}");
    }

    /// A still relay's searches at their worst, each run to its limit at
    /// the GPS's 38 mA on USB (`docs/solar-test-2026-11-24.md`), cost 1.3
    /// and 0.6 mA.
    #[test]
    fn a_still_relay_s_searches_cost_little() {
        const MEASURED_UA: f64 = 38_000.0;
        let search_s = SEARCH_LIMIT.as_micros() as f64 / 1e6;
        let ma = STILL_SEARCH_EVERY
            .map(|every| MEASURED_UA * search_s / (every.as_micros() as f64 / 1e6) / 1e3);
        assert!((ma[0] - 1.27).abs() < 0.005, "{ma:?}");
        assert!((ma[1] - 0.63).abs() < 0.005, "{ma:?}");
    }
}
