//! The board's GPS receiver, a Quectel L76K, and when a sleepy node runs
//! it (spec "Position", "Beats"; `docs/gps-schedule-2026-10-25.md`).
//!
//! The figures are the L76K Hardware Design V1.0 (Quectel, 2021-10-20),
//! Table 2 (page 11), at room temperature with every satellite at
//! −130 dBm, about the open-sky level. A sleepy node runs a search only
//! when its core asks for one ([`crate::Action::Locate`]), at the times
//! [`GpsSchedule`] gives.

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

/// The search limit of spec "Beats" as written before
/// `docs/gps-schedule-2026-10-25.md`: 10 minutes.
pub const BEAT_SEARCH_LIMIT: Duration = Duration::from_secs(600);

/// When a sleepy node runs its GPS. A research switch: the note measures
/// each, and the rejected ones go before a merge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpsSchedule {
    /// Spec "Beats" as written: a search before every beat, of up to
    /// [`BEAT_SEARCH_LIMIT`].
    EveryBeat,
    /// A search of up to [`SEARCH_LIMIT`] when the node turns sleepy, when
    /// it takes an anchor or changes it, when it turns anchorless, and
    /// once a delivery TTL while it stays anchorless; none for
    /// [`crate::Config::gps_backoff`] after one that found nothing.
    Events,
    /// As `Events`, and at each beat while the node's last two fixes were
    /// farther apart than two fixes of a still node may be (spec
    /// "Position"): it searches while it is seen to move, until a search
    /// finds nothing or a fix is near the last.
    Follow,
    /// As `Events`, but none for an anchor: only when the node turns
    /// anchorless and once a delivery TTL while it stays so.
    Anchorless,
}
