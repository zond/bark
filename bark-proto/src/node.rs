//! The protocol core: configuration, the node, and the `Core` trait that
//! the driver programs against.
//!
//! There is one node, [`Relay`], in `node/relay.rs` and `node/relay/`,
//! sleepy or a relay by its power (spec "Nodes", "Sleepy nodes",
//! roadmap D46, D71). It uses the
//! radio discipline of `node/mac.rs` and the per-hop transfer of
//! `node/transfer.rs`.

use crate::event::{Action, Event, MAX_FRAME};
use crate::ids::NodeId;
use crate::position::Position;
use crate::radio::{time_on_air, Channel, CHANNELS};
use crate::sniff::LONG_PREAMBLE_SYMBOLS;
use crate::store::Stored;
use crate::time::{Duration, Instant};
use crate::wire::HEARTBEAT_LEN;

mod mac;
mod relay;
mod transfer;

pub use relay::{Anchor, Load, Node, Relay, SleepyNode};

/// Protocol parameters. Defaults are the design's values; the simulator
/// sweeps them (roadmap Phase 1 milestone 5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    /// Own position.
    pub position: Position,
    /// The node's ID: on the board from the chip's hardware ID, in the
    /// simulator one per node. A node given [`NodeId::NONE`] draws one from
    /// its seed.
    pub node_id: NodeId,
    /// Research switch: holders send to a sleepy node whenever they hold a
    /// message for it, as if it listened all the time, instead of only
    /// while it listens after a beat. The simulator's sleepy nodes listen
    /// all the time; this separates what sending on the beat costs from
    /// the rest. Off in the design.
    pub sleepy_listens: bool,
    /// Transmit power in dBm, at most `radio::MAX_ERP_DBM` minus antenna gain.
    pub tx_power_dbm: i8,
    /// Heartbeat interval bounds: each node's fixed interval lies in between
    /// ([`heartbeat_interval`], spec "Heartbeat": 5–15 s).
    pub heartbeat_min: Duration,
    /// See `heartbeat_min`.
    pub heartbeat_max: Duration,
    /// Per-hop ACK timeout (spec "Hand-over": 100 ms).
    pub ack_timeout: Duration,
    /// Per-hop transmit attempts before giving up on a neighbour (spec "Hand-over": 3).
    pub hop_attempts: u8,
    /// Longest random wait of a hand-over or an ACK after a busy
    /// listen-before-talk check (spec "Hand-over": 0–50 ms).
    pub hop_backoff_max: Duration,
    /// Unit of the random wait before a retry after a missing ACK: retry
    /// `k` waits up to `2^k` units (spec "Retry wait": a full frame and its ACK,
    /// 155.52 ms; `tables::RETRY_BACKOFF`).
    pub retry_backoff: Duration,
    /// Failed transfers before a neighbour is demoted (spec "Neighbour table": 2).
    pub demote_after_failures: u8,
    /// Neighbour expiry in heartbeat rotations (spec "Neighbour table": 3, ≈400 s).
    pub neighbour_expiry_rotations: u8,
    /// Rotations a demotion lasts without an ACK (spec "Neighbour table": 3, 390 s).
    pub demotion_rotations: u8,
    /// How long a sleepy node's own message waits before it is routed
    /// again while the node has no live neighbour, and after its anchor did
    /// not answer (spec "A sleepy node's own message": 60 s, the length of
    /// a fade).
    pub fade_retry: Duration,
    /// Delivery-queue entry lifetime, the delivery TTL (spec "Sleepy nodes": 10 min).
    pub delivery_ttl: Duration,
    /// Interval of the delivery retry for queued messages to a sleepy node
    /// that listens all the time (`sleepy_listens`), besides the retry on
    /// each of its beats (spec "Delivery queue": 60 s).
    pub delivery_retry: Duration,
    /// Busy-estimate EMA coefficient as 1/N (spec "Busy estimate": 1/600).
    pub busy_ema_inverse: u16,
    /// Interval between 13-channel RSSI sweeps (spec "Busy estimate": 1 s).
    pub sweep_interval: Duration,
    /// Busy threshold above noise floor in dB (spec "Busy estimate": 6).
    pub busy_threshold_db: u8,
    /// Move if `busy[home] − min(busy)` exceeds this for `move_dwell` (spec "Channel moves": 0.05, 10 min).
    pub move_threshold_percent: u8,
    /// See `move_threshold_percent`.
    pub move_dwell: Duration,
    /// Maximum random wait before moving (spec "Channel moves": 5 min).
    pub move_wait_max: Duration,
    /// Heartbeat listen-before-talk checks before the heartbeat is skipped,
    /// with a random 0–`lbt_backoff_max` wait after each busy one
    /// (spec "Heartbeat": "up to three checks ... if every check is busy, skip").
    pub lbt_attempts: u8,
    /// See `lbt_attempts`.
    pub lbt_backoff_max: Duration,
    /// A node's first rotation: it chooses its home at the end, by when
    /// every relay in range has been heard, unless it has a stored one;
    /// a sleepy node that has heard no relay in two is anchorless (spec
    /// "Channel at boot": the worst-case rotation, 13 × 15 s = 195 s).
    pub boot_listen: Duration,
    /// What the node stored before its last reboot, from its driver
    /// (`store.rs`, spec "Implementation notes"); `None` for a fresh board.
    pub stored: Option<Stored>,
    /// Fixed home channel for bench work instead of the measured choice
    /// (spec "Channel at boot"). The move rule is off while it is set.
    pub home_override: Option<Channel>,
    /// How long the fixes of a carried node must stay within the GPS
    /// allowance of one place before it is fixed again
    /// (spec "Position": 4 h, the longer of the two measured,
    /// roadmap D70). Long, so that a node that stops for a while does not
    /// take mail it would strand when it moves on.
    pub fixed_after: Duration,
    /// How long a relay whose external power has gone keeps relaying,
    /// holding and owning, before it flushes its delivery queue and turns
    /// sleepy (spec "Sleepy nodes", roadmap D71). Listening draws
    /// [`crate::sniff::RX_CURRENT_UA`], 5.8 mA, so an hour of it is 0.68 %
    /// of the board's 850 mAh battery (spec "Hardware"); a shorter grace
    /// would turn a node sleepy at every brief unplug.
    pub grace: Duration,
    /// The battery levels between which a fixed relay on battery relays
    /// past its grace, its readings calibrated (`relay/power.rs`, spec
    /// "Sleepy nodes"). `None` for a driver that reports no
    /// battery, as the simulator's scenarios without batteries: every
    /// relay then turns sleepy at its grace's end, since a fixed one would
    /// otherwise relay until its battery browns out.
    pub battery: Option<BatteryLevels>,
}

/// A battery's open-circuit voltage, millivolts, at which a fixed relay
/// on battery turns sleepy, `low`, and at which a node its battery turned
/// sleepy relays again, `high` (spec "Sleepy nodes").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatteryLevels {
    /// Turn sleepy at or below this.
    pub low_mv: u16,
    /// Relay again at or above this.
    pub high_mv: u16,
}

impl BatteryLevels {
    /// A typical Li-ion cell's open-circuit voltage at 20 % and 30 % of its
    /// charge, 3741 and 3775 mV (Maxim Integrated, Application Note 4189,
    /// Table 1). The load moves the reading by its current times the
    /// cell's resistance, a few millivolts at a relay's; the gap is several
    /// times that and a reading's noise, so that the role does not flap.
    pub const DESIGN: BatteryLevels = BatteryLevels {
        low_mv: 3741,
        high_mv: 3775,
    };
}

/// A full cell's open-circuit voltage, millivolts, read after an hour's
/// rest (Maxim AN4189, Table 1): what a calibrated reading of a cell that
/// has finished charging and rested is (spec "Sleepy nodes").
pub const BATTERY_FULL_MV: u16 = 4177;

/// How long on external power a cell is full: the charger's 500 mA
/// (LILYGO T-Echo, "Electrical Parameters") brings an empty 850 mAh cell
/// to its end voltage within 1.7 h, and its constant-voltage end is
/// assumed to take under 1.3 h more.
pub const BATTERY_CHARGED_AFTER: Duration = Duration::from_secs(3 * 3600);

/// How long a full cell rests on battery before its reading calibrates:
/// the hour AN4189's readings rest.
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

/// The heartbeat intervals a node can have: 256, one ID byte's worth, over
/// the range of spec "Heartbeat", 39.06 ms apart (spec "Heartbeat"). Two
/// nodes whose intervals differ by at least two heartbeats' airtime,
/// 2 × 14.40 ms, slide past each other by more than an overlap at every
/// heartbeat, so that two heartbeats of theirs meet at most once in a row;
/// 10 s / 28.80 ms = 347 intervals would do, and the largest power of two
/// below it is 256.
pub const INTERVALS: u64 = 256;

/// Node `id`'s heartbeat interval: fixed, from the last byte of its ID,
/// one of [`INTERVALS`] steps over `heartbeat_min` to `heartbeat_max`
/// (spec "Heartbeat"). A neighbour that hears one heartbeat of it knows
/// when the next comes to its channel.
pub const fn heartbeat_interval(id: NodeId, config: &Config) -> Duration {
    let min = config.heartbeat_min.as_micros();
    let span = config.heartbeat_max.as_micros().saturating_sub(min);
    Duration::from_micros(min + span * id.last() as u64 / INTERVALS)
}

/// The longest a heartbeat waits behind its slot: a hand-over attempt it
/// may find under way, a full frame with the long preamble, to a sleepy
/// node, and the ACK timeout, 592.86 ms, which is longer than
/// a reception its check waits for, and two listen-before-talk backoffs
/// before its last check, 400 ms (spec "Heartbeat", "Hand-over"):
/// 992.86 ms.
pub const fn heartbeat_lag(config: &Config) -> Duration {
    let attempt = time_on_air(MAX_FRAME as u8, LONG_PREAMBLE_SYMBOLS).as_micros()
        + config.ack_timeout.as_micros();
    let backoffs =
        config.lbt_backoff_max.as_micros() * config.lbt_attempts.saturating_sub(1) as u64;
    Duration::from_micros(attempt + backoffs)
}

/// How far apart a node's heartbeats go in its first rotation (spec
/// "Heartbeat"): as long as the answers to one can take to arrive, so
/// that the next says whether one did. An answer goes within a
/// listen-before-talk backoff of the heartbeat, then waits behind its
/// slot as any heartbeat may, [`heartbeat_lag`], and is on the air for a
/// heartbeat's airtime: 200 + 992.86 + 14.40 ms = 1207.26 ms.
pub const fn burst_gap(config: &Config) -> Duration {
    let airtime = time_on_air(HEARTBEAT_LEN as u8, crate::radio::MIN_PREAMBLE_SYMBOLS);
    Duration::from_micros(
        config.lbt_backoff_max.as_micros()
            + heartbeat_lag(config).as_micros()
            + airtime.as_micros(),
    )
}

/// A relay within this distance of a destination that is not its own
/// position, with no live, undemoted neighbour closer, owns it
/// (spec "Owner"): 50 m is what spec "Position" allows between two GPS fixes of a relay that
/// has not moved, so the addressed relay and this one are, for the radio,
/// in the same place.
pub const OWNER_RADIUS_M: f32 = 50.0;

/// A common walking speed, metres a second: the comfortable gait of
/// adults, about 1.3 to 1.5 m/s (Bohannon, "Comfortable and maximum
/// walking speed of adults aged 20–79 years", Age and Ageing 26, 1997).
pub const WALKING_SPEED_M_S: f32 = 1.4;

/// How often the GPS gives a fix: once a second, the Quectel L76K's
/// default output rate (L76K hardware design and specification).
pub const FIX_INTERVAL: Duration = Duration::from_secs(1);

/// How long a fixed node's fixes must all be farther than the GPS
/// allowance, [`OWNER_RADIUS_M`], from the place it stands before it is carried
/// (spec "Position"): as long as a walker takes to cross the
/// whole width of the allowance, 2 × 50 m at [`WALKING_SPEED_M_S`],
/// 71.4 s, in whole fixes of [`FIX_INTERVAL`]: 72 far fixes in a row. A
/// still node's fix that strays past the allowance comes back within it;
/// one that walks keeps going.
pub const CARRY_AFTER: Duration = {
    let walk_us = (2.0 * OWNER_RADIUS_M / WALKING_SPEED_M_S * 1e6) as u64;
    let fix_us = FIX_INTERVAL.as_micros();
    Duration::from_micros(walk_us.div_ceil(fix_us) * fix_us)
};

const _: () = assert!(CARRY_AFTER.as_micros() == 72_000_000);

/// Longest heartbeat interval (spec "Heartbeat": 15 s); the first rotation derives from it.
const HEARTBEAT_MAX: Duration = Duration::from_secs(15);

impl Config {
    /// The design's values, as a constant.
    pub const DEFAULT: Config = Config {
        position: Position { lat: 0, lon: 0 },
        node_id: NodeId::NONE,
        sleepy_listens: false,
        tx_power_dbm: 10,
        heartbeat_min: Duration::from_secs(5),
        heartbeat_max: HEARTBEAT_MAX,
        ack_timeout: Duration::from_millis(100),
        hop_attempts: 3,
        hop_backoff_max: Duration::from_millis(50),
        retry_backoff: crate::tables::RETRY_BACKOFF,
        demote_after_failures: 2,
        neighbour_expiry_rotations: 3,
        demotion_rotations: 3,
        fade_retry: Duration::from_secs(60),
        delivery_ttl: Duration::from_secs(600),
        delivery_retry: Duration::from_secs(60),
        busy_ema_inverse: 600,
        sweep_interval: Duration::from_secs(1),
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

    /// Whether the core would take an `AppCommand::Send` at `now`. The
    /// driver offers its application's message only while this holds, and
    /// asks again after every `handle` call; until then the application
    /// waits, which is how a busy network pushes back on its user. A
    /// `Send` offered while this is false is a driver bug, and the core
    /// ignores it (`docs/core-interface.md` §4).
    fn accepts_send(&self, now: Instant) -> bool;
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
