//! The board's GPS receiver, a Quectel L76K, and when a node runs it
//! (spec "Position", "Beats"; `docs/gps-schedule-2026-10-25.md`,
//! `docs/relay-gps-2026-10-26.md`).
//!
//! The figures are the L76K Hardware Design V1.0 (Quectel, 2021-10-20),
//! Table 2 (page 11), at room temperature with every satellite at
//! −130 dBm, about the open-sky level. A sleepy node runs a search only
//! when its core asks for one ([`crate::Action::Locate`]), while it has
//! no anchor (spec "Beats", roadmap D80, D91); a relay's runs while its
//! core says so (`Node::gps_tracks`), and searches as a sleepy node's
//! when it rests (roadmap D82).

use crate::time::Duration;

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
/// It is the time to read a satellite's orbit, which its signal repeats
/// every 30 s (IS-GPS-200, §20.3.2: a frame of five 6 s subframes, the
/// orbit in the first three).
pub const COLD_START: Duration = Duration::from_secs(30);

/// How long a satellite's broadcast orbit serves for a hot start: the
/// four hours each set of orbit parameters is fitted over (IS-GPS-200,
/// §20.3.3.4.3.1, the fit interval). Older, the receiver reads it afresh.
pub const ORBIT_VALID: Duration = Duration::from_secs(4 * 3600);

/// How long a search may run before it is given up: two of the 30 s
/// frames that carry the orbits, so that a cold start under the open sky
/// survives one frame lost to a bit error. A search that has found
/// nothing by then is most likely indoors, below the −148 dBm the L76K
/// needs to acquire (Table 2), where a longer one finds nothing either.
pub const SEARCH_LIMIT: Duration = COLD_START.mul(2);

/// How long a sleepy node runs no search after one that found nothing:
/// 8 h. A search that finds nothing costs [`SEARCH_LIMIT`] at
/// [`GPS_CURRENT_UA`], 0.48 mAh; once in 8 h that is 0.060 mA, a tenth of
/// a sleepy node's 0.63 mA (spec "Delivery"), for a node that never sees
/// the sky (roadmap D80).
pub const BACKOFF: Duration = Duration::from_secs(8 * 3600);

/// A sleepy node's mean current with an anchor, µA: the radio's, S4's
/// figure (spec "Delivery"), which [`BACKOFF`] keeps the GPS within a
/// tenth of.
pub const SLEEPY_CURRENT_UA: u64 = 630;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_back_off_keeps_a_node_without_sky_within_a_tenth() {
        let search_uas = GPS_CURRENT_UA * SEARCH_LIMIT.as_micros() / 1_000_000;
        let mean_ua = search_uas as f64 / (BACKOFF.as_micros() as f64 / 1e6);
        assert!(mean_ua <= SLEEPY_CURRENT_UA as f64 / 10.0, "{mean_ua}");
        // And not much less: 7.7 h is the least that keeps it.
        let least_s = search_uas as f64 / (SLEEPY_CURRENT_UA as f64 / 10.0);
        assert!((least_s / 3600.0 - 7.7).abs() < 0.05, "{least_s}");
    }
}
