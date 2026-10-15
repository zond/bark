//! The protocol core: configuration, roles, and the `Core` trait that the
//! driver programs against.
//!
//! The relay is in `node/relay.rs` and `node/relay/`, the leaf in
//! `node/leaf.rs`. Both use the radio discipline of `node/mac.rs` and the
//! per-hop transfer of `node/transfer.rs`.

use crate::event::{Action, Event};
use crate::ids::LeafId;
use crate::position::Position;
use crate::radio::Channel;
use crate::time::{Duration, Instant};

mod leaf;
mod mac;
mod relay;
mod transfer;

pub use leaf::Leaf;
pub use relay::Relay;

/// Which half of the protocol a node runs (design §1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Forwards traffic, sends heartbeats, serves leaves.
    Relay,
    /// Attaches to one relay; originates and receives application messages.
    Leaf,
}

/// Protocol parameters. Defaults are the design's values; the simulator
/// sweeps them (roadmap Phase 1 milestone 5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    /// Own position. Relay identity (design §8.1).
    pub position: Position,
    /// Whether `position` is surveyed (heartbeat type 0x1) or GPS-derived (0x2).
    pub surveyed: bool,
    /// A leaf's ID, which the app generates at random (design §8.1). A
    /// leaf given [`LeafId::RELAY`] draws one from its seed. A relay
    /// ignores it.
    pub leaf_id: LeafId,
    /// Transmit power in dBm, at most `radio::MAX_ERP_DBM` minus antenna gain.
    pub tx_power_dbm: i8,
    /// Heartbeat interval bounds, uniform random in between (design §7: 5–15 s).
    pub heartbeat_min: Duration,
    /// See `heartbeat_min`.
    pub heartbeat_max: Duration,
    /// Per-hop ACK timeout (design §9.3: 100 ms).
    pub ack_timeout: Duration,
    /// Per-hop transmit attempts before giving up on a neighbour (design §9.3: 3).
    pub hop_attempts: u8,
    /// Longest random wait of a hand-over or an ACK after a busy
    /// listen-before-talk check (design §9.3: 0–50 ms).
    pub hop_backoff_max: Duration,
    /// Unit of the random wait before a retry after a missing ACK: retry
    /// `k` waits up to `2^k` units (design §9.3: a full frame and its ACK,
    /// 152.96 ms; `tables::RETRY_BACKOFF`).
    pub retry_backoff: Duration,
    /// Failed transfers before a neighbour is demoted (design §8.2: 2).
    pub demote_after_failures: u8,
    /// Neighbour expiry in heartbeat rotations (design §8.2: 3, ≈400 s).
    pub neighbour_expiry_rotations: u8,
    /// Rotations a demotion lasts without an ACK (design §8.2: 3, 390 s).
    pub demotion_rotations: u8,
    /// Leaf keepalive interval (design §10.2: 60 s).
    pub keepalive: Duration,
    /// Delivery-queue entry lifetime (design §10.4: 10 min).
    pub delivery_ttl: Duration,
    /// Interval of the delivery retry for queued messages, besides the
    /// retry on each keepalive (design §10.4: 60 s).
    pub delivery_retry: Duration,
    /// Busy-estimate EMA coefficient as 1/N (design §6.1: 1/600).
    pub busy_ema_inverse: u16,
    /// Interval between 13-channel RSSI sweeps (design §6.1: 1 s).
    pub sweep_interval: Duration,
    /// Busy threshold above noise floor in dB (design §6.1: 6).
    pub busy_threshold_db: u8,
    /// Move if `busy[home] − min(busy)` exceeds this for `move_dwell` (design §6.3: 0.05, 10 min).
    pub move_threshold_percent: u8,
    /// See `move_threshold_percent`.
    pub move_dwell: Duration,
    /// Maximum random wait before moving (design §6.3: 5 min).
    pub move_wait_max: Duration,
    /// Heartbeat listen-before-talk checks before the heartbeat is skipped,
    /// with a random 0–`lbt_backoff_max` wait after each busy one (design
    /// §7: "retry ... up to three times, then skip").
    pub lbt_attempts: u8,
    /// See `lbt_attempts`.
    pub lbt_backoff_max: Duration,
    /// Listen on one channel this long before choosing home (design §6.2:
    /// one full rotation, ≈150 s).
    pub boot_listen: Duration,
    /// Fixed home channel for bench work instead of the measured choice
    /// (design §6.2). The move rule is off while it is set.
    pub home_override: Option<Channel>,
}

impl Default for Config {
    fn default() -> Self {
        Config::DEFAULT
    }
}

impl Config {
    /// The design's values, as a constant.
    pub const DEFAULT: Config = Config {
        position: Position { lat: 0, lon: 0 },
        surveyed: true,
        leaf_id: LeafId::RELAY,
        tx_power_dbm: 10,
        heartbeat_min: Duration::from_secs(5),
        heartbeat_max: Duration::from_secs(15),
        ack_timeout: Duration::from_millis(100),
        hop_attempts: 3,
        hop_backoff_max: Duration::from_millis(50),
        retry_backoff: crate::tables::RETRY_BACKOFF,
        demote_after_failures: 2,
        neighbour_expiry_rotations: 3,
        demotion_rotations: 3,
        keepalive: Duration::from_secs(60),
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
        boot_listen: Duration::from_secs(150),
        home_override: None,
    };
}

/// A protocol state machine. Both roles implement it; drivers program
/// against it and never against a role directly.
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

/// Either role, for a driver that switches role at run time (the firmware).
#[allow(clippy::large_enum_variant)]
pub enum Node {
    /// Running as a relay.
    Relay(Relay),
    /// Running as a leaf.
    Leaf(Leaf),
}

impl Node {
    /// Create a node in the given role.
    pub fn new(role: Role, config: Config, seed: u64) -> Node {
        match role {
            Role::Relay => Node::Relay(Relay::new(config, seed)),
            Role::Leaf => Node::Leaf(Leaf::new(config, seed)),
        }
    }

    /// Current role.
    pub fn role(&self) -> Role {
        match self {
            Node::Relay(_) => Role::Relay,
            Node::Leaf(_) => Role::Leaf,
        }
    }
}

impl Core for Node {
    fn handle(&mut self, event: Event<'_>, now: Instant, emit: &mut dyn FnMut(Action)) {
        match self {
            Node::Relay(r) => r.handle(event, now, emit),
            Node::Leaf(l) => l.handle(event, now, emit),
        }
    }

    fn next_wakeup(&self) -> Option<Instant> {
        match self {
            Node::Relay(r) => r.next_wakeup(),
            Node::Leaf(l) => l.next_wakeup(),
        }
    }

    fn accepts_send(&self, now: Instant) -> bool {
        match self {
            Node::Relay(r) => r.accepts_send(now),
            Node::Leaf(l) => l.accepts_send(now),
        }
    }
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
