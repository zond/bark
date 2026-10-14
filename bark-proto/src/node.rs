//! The protocol core: configuration, roles, and the `Core` trait that the
//! driver programs against.
//!
//! Phase 0 implements the state machines behind [`Relay`] and [`Leaf`]; this
//! file fixes their outward shape so the simulator and the firmware can be
//! built against it in parallel.

use crate::event::{Action, Event};
use crate::position::Position;
use crate::radio::{Channel, CHANNELS};
use crate::time::{Duration, Instant};

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
    /// Failed transfers before a neighbour is demoted (design §8.2: 2).
    pub demote_after_failures: u8,
    /// Neighbour expiry in heartbeat rotations (design §8.2: 3, ≈400 s).
    pub neighbour_expiry_rotations: u8,
    /// Leaf keepalive interval (design §10.2: 60 s).
    pub keepalive: Duration,
    /// Delivery-queue entry lifetime (design §10.4: 10 min).
    pub delivery_ttl: Duration,
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
    /// Listen-before-talk retries with random 0–`lbt_backoff_max` waits (design §7: 3, 200 ms).
    pub lbt_attempts: u8,
    /// See `lbt_attempts`.
    pub lbt_backoff_max: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            position: Position::default(),
            surveyed: true,
            tx_power_dbm: 10,
            heartbeat_min: Duration::from_secs(5),
            heartbeat_max: Duration::from_secs(15),
            ack_timeout: Duration::from_millis(100),
            hop_attempts: 3,
            demote_after_failures: 2,
            neighbour_expiry_rotations: 3,
            keepalive: Duration::from_secs(60),
            delivery_ttl: Duration::from_secs(600),
            busy_ema_inverse: 600,
            sweep_interval: Duration::from_secs(1),
            busy_threshold_db: 6,
            move_threshold_percent: 5,
            move_dwell: Duration::from_secs(600),
            move_wait_max: Duration::from_secs(300),
            lbt_attempts: 3,
            lbt_backoff_max: Duration::from_millis(200),
        }
    }
}

/// A protocol state machine. Both roles implement it; drivers program
/// against it and never against a role directly.
pub trait Core {
    /// Deliver one event at time `now`. The core calls `emit` zero or more
    /// times, in the order the actions must be performed, and returns.
    ///
    /// `now` never decreases between calls. `Event::Wakeup` is delivered when
    /// `now >= next_wakeup()`, possibly later than that instant, never earlier.
    fn handle(&mut self, event: Event<'_>, now: Instant, emit: &mut dyn FnMut(Action));

    /// When the core next needs `Event::Wakeup`, if ever. May change after
    /// every `handle` call; the driver re-reads it after each call.
    fn next_wakeup(&self) -> Option<Instant>;
}

/// Relay role. Capacity for 64 neighbours, 256 leaves and 64 queued
/// deliveries (design §8.2, §10.2, §10.4), fixed at compile time.
pub struct Relay {
    config: Config,
    home: Channel,
    rng: Rng,
    wakeup: Option<Instant>,
    started: bool,
}

/// Leaf role. Small: the simulator holds twenty thousand of these.
pub struct Leaf {
    config: Config,
    rng: Rng,
    wakeup: Option<Instant>,
}

/// Either role, for a driver that switches role at run time (the firmware).
pub enum Node {
    /// Running as a relay.
    Relay(Relay),
    /// Running as a leaf.
    Leaf(Leaf),
}

impl Relay {
    /// Create a relay. `seed` drives every random choice the relay makes;
    /// two relays with the same config and seed behave identically.
    pub fn new(config: Config, seed: u64) -> Relay {
        let mut rng = Rng::new(seed);
        let home = Channel::new((rng.next_u64() % CHANNELS as u64) as u8).expect("in range");
        Relay { config, home, rng, wakeup: None, started: false }
    }

    /// Current home channel.
    pub fn home(&self) -> Channel {
        self.home
    }

    /// Configuration.
    pub fn config(&self) -> &Config {
        &self.config
    }
}

impl Leaf {
    /// Create a leaf; see [`Relay::new`] for `seed`.
    pub fn new(config: Config, seed: u64) -> Leaf {
        Leaf { config, rng: Rng::new(seed), wakeup: None }
    }

    /// Configuration.
    pub fn config(&self) -> &Config {
        &self.config
    }
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

// Phase 0 stubs: the state machines are implemented behind these. Until
// then a core does nothing but start listening on its home channel.

impl Core for Relay {
    fn handle(&mut self, event: Event<'_>, _now: Instant, emit: &mut dyn FnMut(Action)) {
        if let Event::Wakeup = event {
            self.wakeup = None;
        }
        if !self.started {
            self.started = true;
            emit(Action::Listen(self.home));
        }
        let _ = &mut self.rng;
    }

    fn next_wakeup(&self) -> Option<Instant> {
        self.wakeup
    }
}

impl Core for Leaf {
    fn handle(&mut self, event: Event<'_>, _now: Instant, _emit: &mut dyn FnMut(Action)) {
        if let Event::Wakeup = event {
            self.wakeup = None;
        }
        let _ = (&mut self.rng, &self.config);
    }

    fn next_wakeup(&self) -> Option<Instant> {
        self.wakeup
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
}

/// SplitMix64: 64 bits of state, full-period, every bit of output usable.
/// Used for all protocol randomness so a run is a function of its seeds.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    /// Seed.
    pub fn new(seed: u64) -> Rng {
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
}
