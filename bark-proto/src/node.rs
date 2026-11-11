//! The protocol core: configuration, the node, and the `Core` trait that
//! the driver programs against.
//!
//! The one node type, [`Relay`], is in `node/relay.rs` and `node/relay/`
//! (spec "Nodes", "Sleepy nodes"); the radio discipline is in `node/mac.rs`,
//! the per-hop transfer in `node/transfer.rs`.

use crate::event::{Action, Event, MAX_FRAME};
use crate::ids::{MsgId, NodeId};
use crate::position::Position;
use crate::radio::{time_on_air, Channel, CHANNELS};
use crate::sniff::LONG_PREAMBLE_SYMBOLS;
use crate::store::Stored;
use crate::time::{Duration, Instant};
use crate::wire::HEARTBEAT_LEN;

mod mac;
mod relay;
mod transfer;

pub use relay::{
    remembered_within, Anchor, Forced, Held, HeldAt, Kept, Load, NeighbourCounts, Node, Relay,
    SleepyNode,
};

/// Protocol parameters. Defaults are the design's values; the simulator
/// sweeps them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    /// Own position.
    pub position: Position,
    /// Whether `position` is the node's place, as on a bench board or in the
    /// simulator. Without it, and without a stored place, the node has none
    /// until its first accepted fix or a map point (spec "Position").
    pub placed: bool,
    /// The node's ID; a node given [`NodeId::NONE`] draws one from its seed.
    pub node_id: NodeId,
    /// Research switch, off in the design: holders send to a sleepy node
    /// whenever they hold a message for it, as if it listened all the time,
    /// to separate what sending on the beat costs.
    pub sleepy_listens: bool,
    /// Transmit power in dBm, at most `radio::MAX_ERP_DBM` minus antenna gain.
    pub tx_power_dbm: i8,
    /// Heartbeat interval bounds ([`heartbeat_interval`], spec "Heartbeat").
    pub heartbeat_min: Duration,
    /// See `heartbeat_min`.
    pub heartbeat_max: Duration,
    /// Per-hop ACK timeout (spec "Hand-over").
    pub ack_timeout: Duration,
    /// Per-hop transmit attempts before giving up on a neighbour (spec
    /// "Hand-over").
    pub hop_attempts: u8,
    /// Longest random wait of a hand-over after a busy listen-before-talk
    /// check (spec "Hand-over").
    pub hop_backoff_max: Duration,
    /// Unit of the random wait before a retry after a missing ACK: retry
    /// `k` waits up to `2^k` units (spec "Retry wait").
    pub retry_backoff: Duration,
    /// Failed hand-overs in a row before a neighbour is demoted (spec
    /// "Neighbour table").
    pub demote_after_failures: u8,
    /// Neighbour expiry in heartbeat rotations (spec "Neighbour table").
    pub neighbour_expiry_rotations: u8,
    /// Rotations a demotion lasts without an ACK (spec "Neighbour table").
    pub demotion_rotations: u8,
    /// How long a sleepy node's own message waits before it is routed again
    /// when it has no live neighbour or its anchor did not answer
    /// (spec "A sleepy node's own message": the length of a fade).
    pub fade_retry: Duration,
    /// The delivery queue's stale age and the least the network keeps a
    /// node's mail (spec "Sleepy nodes").
    pub delivery_ttl: Duration,
    /// Delivery retry interval under `sleepy_listens`, besides the retry on
    /// each beat; for that research switch only.
    pub delivery_retry: Duration,
    /// Busy-estimate EMA coefficient as 1/N (spec "Busy estimate").
    pub busy_ema_inverse: u16,
    /// Interval between 13-channel RSSI sweeps (spec "Busy estimate").
    pub sweep_interval: Duration,
    /// Busy threshold above noise floor in dB (spec "Busy estimate").
    pub busy_threshold_db: u8,
    /// Move if `busy[home] − min(busy)` exceeds this for `move_dwell` (spec "Channel moves").
    pub move_threshold_percent: u8,
    /// See `move_threshold_percent`.
    pub move_dwell: Duration,
    /// Maximum random wait before moving (spec "Channel moves").
    pub move_wait_max: Duration,
    /// Heartbeat listen-before-talk checks before the heartbeat is skipped,
    /// with a random 0–`lbt_backoff_max` wait after each busy one (spec
    /// "Heartbeat").
    pub lbt_attempts: u8,
    /// See `lbt_attempts`.
    pub lbt_backoff_max: Duration,
    /// A node's first rotation, by whose end every relay in range has been
    /// heard (spec "Channel at boot": 13 × 15 s = 195 s).
    pub boot_listen: Duration,
    /// What the node stored before its last reboot
    /// (spec "Implementation notes"); `None` for a fresh board.
    pub stored: Option<Stored>,
    /// Fixed home channel for bench work; the move rule is off while set.
    pub home_override: Option<Channel>,
    /// How long a carried node's fixes must stay within [`CARRY_RADIUS_M`]
    /// of one place before it is fixed again (spec "Position"). Long, so a
    /// node that stops a while does not take mail it would strand.
    pub fixed_after: Duration,
    /// How long a fixed relay that lost external power keeps relaying
    /// before it turns sleepy (spec "Sleepy nodes"). An hour of listening at
    /// [`crate::sniff::RX_CURRENT_UA`] is 0.68 % of the 850 mAh battery
    /// (spec "Hardware"); a shorter grace would turn a node sleepy at every
    /// brief unplug.
    pub grace: Duration,
    /// The battery levels between which a fixed relay relays past its grace
    /// (spec "Sleepy nodes"). `None` for a driver that reports no battery:
    /// the relay then turns sleepy at its grace's end rather than relay
    /// until it browns out.
    pub battery: Option<BatteryLevels>,
    /// The firmware image version its status tells
    /// (spec "Neighbour status"); 0 where none is known.
    pub version: u32,
}

/// A calibrated battery reading, millivolts, at which a fixed relay on
/// battery turns sleepy, `low`, and at which a node its battery turned
/// sleepy relays again, `high` (spec "Sleepy nodes").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatteryLevels {
    /// Turn sleepy at or below this.
    pub low_mv: u16,
    /// Relay again at or above this.
    pub high_mv: u16,
}

impl BatteryLevels {
    /// The cell's curve at [`BATTERY_LOW_PERCENT`] and
    /// [`BATTERY_HIGH_PERCENT`], 3504 and 3611 mV. The gap is many times
    /// the readings' noise and the few millivolts a change of load moves
    /// them, so the role does not flap.
    pub const DESIGN: BatteryLevels = BatteryLevels {
        low_mv: cell_mv(BATTERY_LOW_PERCENT),
        high_mv: cell_mv(BATTERY_HIGH_PERCENT),
    };

    /// Whether a node may start swapping in a new image (spec
    /// "Implementation notes"). `millivolts` is a calibrated reading; 0, no
    /// reading yet, is below the low level.
    pub fn swap_allowed(self, external_power: bool, millivolts: u16) -> bool {
        external_power || millivolts > self.low_mv
    }
}

/// What a calibrated reading of a full cell is, millivolts: the curve's
/// 100 % (spec "Sleepy nodes"). The curve and the calibration so share one
/// scale, the board's whose run fitted the curve; a board calibrated to
/// it also has its lighter load's higher reading scaled away near full.
pub const BATTERY_FULL_MV: u16 = cell_mv(100);

/// The charge at the low level, percent: a fixed relay keeps a fifth of
/// its cell for its owner when it turns sleepy (spec "Sleepy nodes").
pub const BATTERY_LOW_PERCENT: u8 = 20;

/// The charge at which a node its battery turned sleepy relays again.
pub const BATTERY_HIGH_PERCENT: u8 = 30;

/// The cell's curve, charge left in millionths of a percent against `u`,
/// mV above [`CURVE_FROM_MV`]: `[a, b, c]` of a + b·u + c·u², that is
/// 5 % + 60 %/V + 66 %/V². A least-squares fit to the T-Echo cell's
/// discharge at a constant 35.4 mA, its readings at 3 % and above
/// (`docs/data/discharge-f4f5-2026-11-11.csv`, reproduced by
/// `bark-fw/scripts/discharge.py`; `docs/discharge-2026-11-11.md`),
/// rounded to two figures, which costs nothing: against the run's 131
/// samples it is within 1.1 % rms and 3.1 % at most, the ADC's 1.76 mV
/// step being 0.1 to 0.3 % of the charge.
const CURVE: [i64; 3] = [5_000_000, 60_000, 66];

/// The curve's origin, mV.
const CURVE_FROM_MV: i64 = 3300;

/// A whole percent in [`CURVE`]'s units.
const PERCENT: i64 = 1_000_000;

/// How much of its charge the cell has left at reading `mv`, calibrated,
/// in whole percent, on the cell's curve ([`CURVE`]), 0 to 100.
pub const fn cell_percent(mv: u16) -> u8 {
    let [a, b, c] = CURVE;
    let u = mv as i64 - CURVE_FROM_MV;
    // Below its lowest point the parabola rises again; it is below 0 there.
    if b + 2 * c * u <= 0 {
        return 0;
    }
    let p = a + b * u + c * u * u;
    if p <= 0 {
        0
    } else if p >= 100 * PERCENT {
        100
    } else {
        ((p + PERCENT / 2) / PERCENT) as u8
    }
}

/// The reading, mV, at which the cell has `percent` of its charge left:
/// the inverse of [`cell_percent`], rounded to the millivolt.
pub const fn cell_mv(percent: u8) -> u16 {
    let [a, b, c] = CURVE;
    let p = (if percent > 100 { 100 } else { percent }) as i64 * PERCENT;
    let root = (b * b + 4 * c * (p - a)) as u64;
    let u = (root.isqrt() as i64 - b + c).div_euclid(2 * c);
    (CURVE_FROM_MV + u) as u16
}

/// About how much charge is left at `mv`, a calibrated reading, in percent
/// ([`cell_percent`]); `None` at or below the low level, where a node shows
/// it low rather than a number.
pub const fn battery_percent(mv: u16) -> Option<u8> {
    if mv <= BatteryLevels::DESIGN.low_mv {
        None
    } else {
        Some(cell_percent(mv))
    }
}

/// How long on external power a cell is full: the charger's 500 mA
/// (LILYGO T-Echo, "Electrical Parameters") brings an empty 850 mAh cell
/// to its end voltage within 1.7 h, and its constant-voltage end is
/// assumed to take under 1.3 h more.
pub const BATTERY_CHARGED_AFTER: Duration = Duration::from_secs(3 * 3600);

/// How long a full cell rests on battery before its reading calibrates:
/// in the cell's measured run the charger's lift, some 25 mV beyond what
/// the charge used explains, was gone within 40 minutes
/// (`docs/discharge-2026-11-11.md`).
pub const BATTERY_REST: Duration = Duration::from_secs(3600);

/// A calibration that would scale the readings by more than this,
/// thousandths, is not taken: the SAADC's gain error at gain 1/6 is ±3 %
/// (nRF52840 Product Specification, SAADC), so a reading further off is
/// not of a full cell.
pub const BATTERY_GAIN_BOUND_PERMILLE: u32 = 30;

/// One, in the units of a battery reading's scale: a calibrated reading
/// is the driver's times the scale over this (`store::Stored`).
pub const BATTERY_SCALE_ONE: u32 = 1 << 15;

impl Default for Config {
    fn default() -> Self {
        Config::DEFAULT
    }
}

/// The heartbeat intervals a node can have, one ID byte's worth (spec
/// "Heartbeat"). Intervals at least two heartbeats' airtime apart
/// (2 × 14.40 ms) meet at most once in a row; 10 s / 28.80 ms = 347 would
/// do, and the largest power of two below it is 256.
pub const INTERVALS: u64 = 256;

/// Node `id`'s fixed heartbeat interval, from the last byte of its ID (spec
/// "Heartbeat"), so a neighbour that hears one knows when the next comes.
pub const fn heartbeat_interval(id: NodeId, config: &Config) -> Duration {
    let min = config.heartbeat_min.as_micros();
    let span = config.heartbeat_max.as_micros().saturating_sub(min);
    Duration::from_micros(min + span * id.last() as u64 / INTERVALS)
}

/// The longest a heartbeat waits behind its slot (spec "Heartbeat",
/// "Hand-over"): a hand-over attempt under way, a full long-preamble frame
/// and the ACK timeout, 472.8 ms (longer than any reception its check waits
/// for), plus two listen-before-talk backoffs, 400 ms: 872.8 ms.
pub const fn heartbeat_lag(config: &Config) -> Duration {
    let attempt = time_on_air(MAX_FRAME as u8, LONG_PREAMBLE_SYMBOLS).as_micros()
        + config.ack_timeout.as_micros();
    let backoffs =
        config.lbt_backoff_max.as_micros() * config.lbt_attempts.saturating_sub(1) as u64;
    Duration::from_micros(attempt + backoffs)
}

/// How far apart a node's heartbeats go in its first rotation (spec
/// "Heartbeat"): as long as an answer can take, so the next says whether
/// one came. A backoff, [`heartbeat_lag`] and a heartbeat's airtime:
/// 200 + 872.8 + 14.4 ms = 1087.2 ms.
pub const fn burst_gap(config: &Config) -> Duration {
    let airtime = time_on_air(HEARTBEAT_LEN as u8, crate::radio::MIN_PREAMBLE_SYMBOLS);
    Duration::from_micros(
        config.lbt_backoff_max.as_micros()
            + heartbeat_lag(config).as_micros()
            + airtime.as_micros(),
    )
}

/// A relay within this distance of a destination, with no live, undemoted
/// neighbour closer, owns it (spec "Owner"): what spec "Position" allows
/// between two fixes of a relay that has not moved.
pub const OWNER_RADIUS_M: f32 = 50.0;

/// A common walking speed, metres a second: the comfortable gait of
/// adults, about 1.3 to 1.5 m/s (Bohannon, "Comfortable and maximum
/// walking speed of adults aged 20–79 years", Age and Ageing 26, 1997).
pub const WALKING_SPEED_M_S: f32 = 1.4;

/// How often the GPS gives a fix: once a second, the Quectel L76K's
/// default output rate (L76K hardware design and specification).
pub const FIX_INTERVAL: Duration = Duration::from_secs(1);

/// How far a fixed node's fix must lie from its place to count towards
/// making it carried (spec "Position"): beyond a still board's wander, at
/// most 218 m measured (`docs/gps-bench-2026-11-06.md`). A place is one fix,
/// so the radius must hold the wander's whole width, not its distance from
/// a mean.
pub const CARRY_RADIUS_M: f32 = 5.0 * OWNER_RADIUS_M;

/// How long a fixed node's fixes must all lie beyond [`CARRY_RADIUS_M`]
/// before it is carried (spec "Position"): a walker crossing the owner
/// allowance, 2 × 50 m at [`WALKING_SPEED_M_S`], in whole fixes: 72 s.
/// A still node's stray fix comes back within the radius.
pub const CARRY_AFTER: Duration = {
    let walk_us = (2.0 * OWNER_RADIUS_M / WALKING_SPEED_M_S * 1e6) as u64;
    let fix_us = FIX_INTERVAL.as_micros();
    Duration::from_micros(walk_us.div_ceil(fix_us) * fix_us)
};

const _: () = assert!(CARRY_AFTER.as_micros() == 72_000_000);

/// Longest heartbeat interval (spec "Heartbeat": 15 s); the first rotation derives from it.
const HEARTBEAT_MAX: Duration = Duration::from_secs(15);

/// One channel read a second, so each channel every 13 s: a reading of
/// another channel deafens the node for 1.9 ms (spec "Busy estimate").
const SWEEP_INTERVAL: Duration = Duration::from_secs(CHANNELS as u64);

/// The busy estimate's window (spec "Busy estimate": 30 minutes), as the
/// samples a channel gets in it.
const BUSY_EMA_INVERSE: u16 =
    (Duration::from_secs(30 * 60).as_micros() / SWEEP_INTERVAL.as_micros()) as u16;

const _: () = assert!(BUSY_EMA_INVERSE == 138);

impl Config {
    /// The design's values, as a constant.
    pub const DEFAULT: Config = Config {
        position: Position { lat: 0, lon: 0 },
        placed: true,
        node_id: NodeId::NONE,
        sleepy_listens: false,
        tx_power_dbm: 10,
        heartbeat_min: crate::tables::budget::HEARTBEAT_EVERY,
        heartbeat_max: HEARTBEAT_MAX,
        ack_timeout: crate::tables::ACK_TIMEOUT,
        hop_attempts: crate::tables::HOP_ATTEMPTS,
        hop_backoff_max: Duration::from_millis(50),
        retry_backoff: crate::tables::RETRY_BACKOFF,
        demote_after_failures: crate::tables::DEMOTE_AFTER_FAILURES,
        neighbour_expiry_rotations: 3,
        demotion_rotations: 3,
        fade_retry: Duration::from_secs(60),
        delivery_ttl: Duration::from_secs(600),
        delivery_retry: Duration::from_secs(60),
        busy_ema_inverse: BUSY_EMA_INVERSE,
        sweep_interval: SWEEP_INTERVAL,
        busy_threshold_db: 6,
        move_threshold_percent: 5,
        move_dwell: Duration::from_secs(600),
        move_wait_max: Duration::from_secs(300),
        lbt_attempts: 3,
        lbt_backoff_max: Duration::from_millis(200),
        boot_listen: HEARTBEAT_MAX.mul(CHANNELS as u64),
        stored: None,
        home_override: None,
        fixed_after: Duration::from_secs(4 * 3600),
        grace: Duration::from_secs(3600),
        battery: None,
        version: 0,
    };
}

/// A protocol state machine. The node implements it; drivers program
/// against it.
pub trait Core {
    /// Deliver one event at time `now`. The core calls `emit` zero or more
    /// times, in the order the actions must be performed, and returns.
    ///
    /// The first call is `Event::Wakeup`, at start. After that `now` never
    /// decreases between calls, and `Event::Wakeup` is delivered when
    /// `now >= next_wakeup()`, possibly later than that instant, never earlier.
    fn handle(&mut self, event: Event<'_>, now: Instant, emit: &mut dyn FnMut(Action));

    /// When the core next needs `Event::Wakeup`, if ever. May change after
    /// every `handle` call; the driver re-reads it after each call.
    fn next_wakeup(&self) -> Option<Instant>;

    /// Whether the core would take an `AppCommand::Send` at `now`; until it
    /// does the application waits, which is how a busy network pushes back
    /// on its user. A `Send` offered while false is ignored
    /// (`docs/core-interface.md` §4).
    fn accepts_send(&self, now: Instant) -> bool;

    /// The oldest unconfirmed message for this node's application: its ID
    /// and payload (`docs/core-interface.md` §4).
    fn inbox(&self) -> Option<(MsgId, &[u8])>;

    /// Messages in the inbox.
    fn inbox_len(&self) -> usize;

    /// The application has the oldest message, `msg_id`, and the node may
    /// forget it; another ID changes nothing and returns false.
    fn confirm(&mut self, msg_id: MsgId) -> bool;

    /// Whether the GPS is to run all the time now; the driver re-reads it
    /// after each call (`docs/core-interface.md` §4).
    fn gps_tracks(&self) -> bool;

    /// Whether a fix at `position` passes the check against the relays
    /// the node knows; the driver hands only such a fix
    /// (`docs/core-interface.md` §4).
    fn fix_in_reach(&self, position: Position) -> bool;
}

/// SplitMix64: 64 bits of state, full-period, every bit of output usable.
/// Used for all protocol randomness so a run is a function of its seeds.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    /// Seed.
    pub const fn new(seed: u64) -> Rng {
        Rng(seed)
    }

    /// Next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `[lo, hi]` (inclusive), via the high bits.
    pub fn range(&mut self, lo: u64, hi: u64) -> u64 {
        let span = hi - lo + 1;
        lo + ((self.next_u64() as u128 * span as u128) >> 64) as u64
    }

    /// A duration uniform in `[lo, hi]`; `lo` if `hi < lo`.
    pub fn duration(&mut self, lo: Duration, hi: Duration) -> Duration {
        Duration::from_micros(self.range(lo.as_micros(), hi.as_micros().max(lo.as_micros())))
    }
}

#[cfg(test)]
mod battery_percent_tests {
    use super::*;

    #[test]
    fn the_levels_on_the_curve() {
        let levels = BatteryLevels::DESIGN;
        assert_eq!((levels.low_mv, levels.high_mv), (3504, 3611));
        assert_eq!(BATTERY_FULL_MV, 4128);
        assert_eq!(battery_percent(levels.low_mv), None);
        assert_eq!(battery_percent(levels.low_mv + 1), Some(20));
        assert_eq!(battery_percent(levels.high_mv), Some(30));
        assert_eq!(battery_percent(BATTERY_FULL_MV), Some(100));
        assert_eq!(battery_percent(4300), Some(100));
        // The old low level, 3741 mV, is about half the charge.
        assert_eq!(battery_percent(3741), Some(44));
    }

    /// The inverse is the curve's: each percent's reading reads back as
    /// it, and the curve never falls.
    #[test]
    fn the_inverse_and_the_curve_agree() {
        for p in 0..=100 {
            assert_eq!(cell_percent(cell_mv(p)), p, "{p} %");
        }
        assert_eq!(cell_mv(0), 3207);
        assert_eq!(cell_percent(0), 0);
        assert_eq!(cell_percent(3300), 5);
        let mut last = 0;
        for mv in 0..=u16::MAX {
            let p = cell_percent(mv);
            assert!(p >= last, "{mv} mV");
            last = p;
        }
    }

    #[test]
    fn a_swap_waits_for_power_above_the_low_level() {
        let levels = BatteryLevels::DESIGN;
        assert!(!levels.swap_allowed(false, 0));
        assert!(!levels.swap_allowed(false, levels.low_mv));
        assert!(levels.swap_allowed(false, levels.low_mv + 1));
        assert!(levels.swap_allowed(true, 0));
    }
}
