//! Scenarios: a TOML file format and the builder API behind it.
//!
//! The file format is the stable interface (roadmap Q6): scenarios are
//! shared and diffed as files, and tests build the same [`Scenario`] value
//! in Rust with the builder methods. `scenarios/s1-line.toml` is an example
//! of every common section.
//!
//! Nodes are named or addressed by role and index: `"relay:3"` is the
//! fourth relay placed, `"leaf:0"` the first leaf, across all placements in
//! file order. A name given to a point node can be used anywhere a node is
//! referenced.
//!
//! A leaf is a sleepy node (spec "Sleepy nodes"): the files, the rate
//! `per_leaf_per_hour` and the metrics keep the name the format was made
//! with, and the simulator's code says sleepy node where it means the
//! protocol's thing.

use crate::geo::{Origin, Xy};
use crate::links::{LinkParams, Site};
use crate::phy::RadioParams;
use crate::rng::{Purpose, Stream};
use bark_proto::{Config, Duration};
use serde::{Deserialize, Serialize};

/// Node role in a scenario.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoleSpec {
    /// A node on external power at boot: a relay.
    Relay,
    /// A node on battery at boot: sleepy; the name is the file format's.
    Leaf,
}

/// How nodes are placed. Coordinates are metres east (`x`) and north (`y`)
/// of the origin.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "place", rename_all = "snake_case", deny_unknown_fields)]
pub enum Placement {
    /// One node.
    Point {
        /// Optional unique name.
        #[serde(default)]
        name: Option<String>,
        /// Role.
        role: RoleSpec,
        /// Metres east.
        x: f64,
        /// Metres north.
        y: f64,
        /// Indoor (adds the link model's indoor loss at this end).
        #[serde(default)]
        indoor: bool,
        /// On a roof (the link model's rooftop bonus to other rooftops).
        #[serde(default)]
        rooftop: bool,
    },
    /// A square lattice over a rectangle, each point jittered uniformly by
    /// up to `jitter_m` in x and y.
    Lattice {
        /// Role.
        role: RoleSpec,
        /// South-west corner, metres east.
        #[serde(default)]
        x0: f64,
        /// South-west corner, metres north.
        #[serde(default)]
        y0: f64,
        /// Width in metres.
        width_m: f64,
        /// Height in metres.
        height_m: f64,
        /// Lattice spacing in metres.
        spacing_m: f64,
        /// Jitter in metres.
        #[serde(default)]
        jitter_m: f64,
        /// Indoor.
        #[serde(default)]
        indoor: bool,
        /// On a roof.
        #[serde(default)]
        rooftop: bool,
    },
    /// Uniformly random over a rectangle, `round(per_km2 × area)` nodes.
    Density {
        /// Role.
        role: RoleSpec,
        /// South-west corner, metres east.
        #[serde(default)]
        x0: f64,
        /// South-west corner, metres north.
        #[serde(default)]
        y0: f64,
        /// Width in metres.
        width_m: f64,
        /// Height in metres.
        height_m: f64,
        /// Nodes per square kilometre.
        per_km2: f64,
        /// Indoor.
        #[serde(default)]
        indoor: bool,
        /// On a roof.
        #[serde(default)]
        rooftop: bool,
        /// Of leaves, the share that are relays, as a person's node is
        /// while it is on USB power: drawn apart from the positions, so
        /// that the people stand where they would with none. They stay
        /// people: they send and receive texts and may walk.
        #[serde(default)]
        relay_share: f64,
    },
}

/// Overrides of the protocol [`Config`]; absent fields keep the design's
/// defaults. Durations in milliseconds.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[allow(missing_docs)]
pub struct ProtocolOverrides {
    pub tx_power_dbm: Option<i8>,
    pub heartbeat_min_ms: Option<u64>,
    pub heartbeat_max_ms: Option<u64>,
    pub ack_timeout_ms: Option<u64>,
    pub hop_attempts: Option<u8>,
    pub demote_after_failures: Option<u8>,
    pub neighbour_expiry_rotations: Option<u8>,
    pub fade_retry_ms: Option<u64>,
    pub delivery_ttl_ms: Option<u64>,
    pub busy_ema_inverse: Option<u16>,
    pub sweep_interval_ms: Option<u64>,
    pub busy_threshold_db: Option<u8>,
    pub move_threshold_percent: Option<u8>,
    pub move_dwell_ms: Option<u64>,
    pub move_wait_max_ms: Option<u64>,
    pub lbt_attempts: Option<u8>,
    pub lbt_backoff_max_ms: Option<u64>,
    pub hop_backoff_max_ms: Option<u64>,
    pub retry_backoff_ms: Option<u64>,
    pub boot_listen_ms: Option<u64>,
    pub sleepy_listens: Option<bool>,
    pub fixed_after_ms: Option<u64>,
    pub grace_ms: Option<u64>,
    /// `Config::battery` at the design's levels: a fixed relay on battery
    /// relays until its battery is low.
    pub battery_levels: Option<bool>,
}

impl ProtocolOverrides {
    /// The design's defaults with these overrides applied.
    pub fn config(&self) -> Config {
        let mut c = Config::default();
        let ms = Duration::from_millis;
        macro_rules! set {
            ($f:ident) => {
                if let Some(v) = self.$f {
                    c.$f = v;
                }
            };
            ($f:ident, $src:ident, ms) => {
                if let Some(v) = self.$src {
                    c.$f = ms(v);
                }
            };
            ($f:ident, $src:ident, us) => {
                if let Some(v) = self.$src {
                    c.$f = Duration::from_micros(v);
                }
            };
        }
        set!(tx_power_dbm);
        set!(heartbeat_min, heartbeat_min_ms, ms);
        set!(heartbeat_max, heartbeat_max_ms, ms);
        set!(ack_timeout, ack_timeout_ms, ms);
        set!(hop_attempts);
        set!(demote_after_failures);
        set!(neighbour_expiry_rotations);
        set!(fade_retry, fade_retry_ms, ms);
        set!(delivery_ttl, delivery_ttl_ms, ms);
        set!(busy_ema_inverse);
        set!(sweep_interval, sweep_interval_ms, ms);
        set!(hop_backoff_max, hop_backoff_max_ms, ms);
        set!(retry_backoff, retry_backoff_ms, ms);
        set!(busy_threshold_db);
        set!(move_threshold_percent);
        set!(move_dwell, move_dwell_ms, ms);
        set!(move_wait_max, move_wait_max_ms, ms);
        set!(lbt_attempts);
        set!(lbt_backoff_max, lbt_backoff_max_ms, ms);
        set!(boot_listen, boot_listen_ms, ms);
        set!(sleepy_listens);
        set!(fixed_after, fixed_after_ms, ms);
        set!(grace, grace_ms, ms);
        if let Some(on) = self.battery_levels {
            c.battery = on.then_some(bark_proto::BatteryLevels::DESIGN);
        }
        c
    }
}

/// A foreign transmitter (key fob, LPD433 handheld) on one channel with an
/// on/off process: exponential on and off periods.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InterfererSpec {
    /// Metres east.
    pub x: f64,
    /// Metres north.
    pub y: f64,
    /// Channel index.
    pub channel: u8,
    /// Transmit power in dBm.
    pub power_dbm: f64,
    /// Whether CAD detects it (LoRa-like modulation).
    #[serde(default)]
    pub lora: bool,
    /// Mean on period, seconds.
    pub mean_on_s: f64,
    /// Mean off period, seconds; 0 means always on.
    #[serde(default)]
    pub mean_off_s: f64,
    /// Silent until then, seconds: a transmitter that appears among
    /// nodes that have settled.
    #[serde(default)]
    pub from_s: f64,
}

/// Destination choice for Poisson traffic.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Destination {
    /// Any other leaf, uniformly.
    Uniform,
    /// Any other leaf within `radius_m`, uniformly (most texts are local).
    Within {
        /// Radius in metres.
        radius_m: f64,
    },
}

/// Traffic sources. Every message carries a 4-byte tag in its payload so
/// delivery can be matched end to end; payloads are at least 4 bytes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TrafficSpec {
    /// One message from `from` to `to` every `interval_s`.
    Periodic {
        /// Origin node.
        from: String,
        /// Destination node.
        to: String,
        /// Interval, seconds.
        interval_s: f64,
        /// First message, seconds; defaults to one interval.
        #[serde(default)]
        start_s: Option<f64>,
        /// Payload bytes.
        payload_bytes: u8,
    },
    /// Every leaf originates messages as a Poisson process.
    Poisson {
        /// Mean messages per leaf per hour.
        per_leaf_per_hour: f64,
        /// Smallest payload, bytes.
        payload_min: u8,
        /// Largest payload, bytes (uniform in between).
        payload_max: u8,
        /// Destination distribution.
        destination: Destination,
    },
}

/// Leaf availability: alternating exponential available and dark periods.
/// A dark leaf is out of radio reach (pocket, basement, metro): nothing
/// reaches it, its transmissions reach nobody, and it originates nothing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Availability {
    /// Mean available period, seconds.
    pub mean_available_s: f64,
    /// Mean dark period, seconds.
    pub mean_dark_s: f64,
}

/// What a scripted event does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptAction {
    /// Remove the node for good (power off).
    Remove,
    /// Power the node off and on again at once: it loses every table and
    /// timer and boots afresh with the same ID and position, as a solar
    /// relay does after a night.
    Restart,
    /// Driver-level `Listen(channel)`, as if the core had asked.
    Listen,
    /// Driver-level transmit of `len` bytes with `preamble_symbols`; a test
    /// signal. The completion is logged, not given to the core.
    Transmit,
    /// Driver-level CAD; the result is logged, not given to the core.
    Cad,
    /// Driver-level RSSI sample; the result is logged, not given to the core.
    Rssi,
    /// Driver-level `Sniff` on the resting channel until the run ends, as
    /// if the core had asked; a test of duty-cycled receive.
    Sniff,
    /// Power on a node that is off: a node with a `start` event is off
    /// until it, and then boots, as a relay planted during the run does.
    Start,
    /// As `restart`, with the node's stored state erased: a fresh board
    /// in its place (roadmap D73).
    Wipe,
}

/// A scripted event.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptedEvent {
    /// When, seconds.
    pub at_s: f64,
    /// Which node.
    pub node: String,
    /// What.
    pub action: ScriptAction,
    /// Channel for listen, transmit, cad and rssi.
    #[serde(default)]
    pub channel: Option<u8>,
    /// Payload bytes for transmit.
    #[serde(default)]
    pub len: Option<u8>,
    /// Preamble symbols for transmit; default the minimum.
    #[serde(default)]
    pub preamble_symbols: Option<u16>,
}

/// Metrics options.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MetricsParams {
    /// Time-series interval, seconds; `None` for no time series.
    pub cadence_s: Option<f64>,
    /// Nodes at which per-channel busy time is measured.
    pub observers: Vec<String>,
    /// Include per-node counters in the JSON summary.
    pub per_node: bool,
}

/// When nodes are switched on: each at a time drawn uniformly from its
/// kind's window from the start of the run. Both windows 0 switch every
/// node on at once.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Boot {
    /// The window for nodes that are not sleepy, seconds.
    pub relays_s: f64,
    /// The window for sleepy nodes, seconds.
    pub sleepy_s: f64,
}

impl Boot {
    /// The latest a node is switched on, seconds.
    pub fn last_s(&self) -> f64 {
        self.relays_s.max(self.sleepy_s)
    }
}

/// How the simulator gives nodes their IDs (spec "Node identity").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ids {
    /// Node `i` gets `i + 1` times an odd constant: every node its own
    /// and the last bytes, which set heartbeat intervals, spread evenly.
    #[default]
    Sequence,
    /// Drawn from the run's seed, as the chip's factory ID is taken to be random:
    /// last bytes shared at their chance rate. Two nodes never get one ID.
    Random,
}

/// Which nodes of a forced pair share an ID ([`SharedIds`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PairKind {
    /// Two nodes that are not sleepy.
    RelayRelay,
    /// One that is not sleepy and one that is.
    RelaySleepy,
    /// Two sleepy nodes.
    SleepySleepy,
}

/// Whether the two nodes of a forced pair hear each other.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PairReach {
    /// Each hears the other above sensitivity before fading.
    In,
    /// No link is stored between them either way: neither even adds to
    /// the other's noise.
    Out,
}

/// Pairs of nodes forced to share an ID (spec "Node identity"), which a
/// real domain has about one run in twenty and the simulator otherwise
/// never makes. The pairs are drawn from the run's seed among nodes that
/// do not walk, no node in two. With `share = false` the same pairs are
/// drawn and marked but keep their own IDs, the comparison run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharedIds {
    /// What the two nodes are.
    pub kind: PairKind,
    /// Whether they hear each other.
    pub reach: PairReach,
    /// How many such pairs.
    pub count: u32,
    /// Whether they share the ID, or are only marked.
    #[serde(default = "yes")]
    pub share: bool,
}

fn yes() -> bool {
    true
}

/// Sleepy nodes that go dark together, as the people in one train or one
/// basement do (spec "Delivery queue"): the `count` that do not walk
/// nearest to the node `near`, from `at_s` for `for_s` seconds. Each goes
/// dark whatever its own availability process says, and back to it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DarkGroup {
    /// The node whose area it is, as a node reference.
    pub near: String,
    /// How many sleepy nodes.
    pub count: u32,
    /// When they go dark, seconds.
    pub at_s: f64,
    /// For how long, seconds.
    pub for_s: f64,
}

/// Node clocks. Each node's clock runs at its own rate, drawn uniformly
/// within `drift_ppm` of true time from the run's seed; the simulator's
/// time is true time. 0 keeps every clock true.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Clocks {
    /// The most a clock runs fast or slow, parts per million.
    pub drift_ppm: f64,
}

/// Walking sleepy nodes. A share of them walk: each stops indoors at a
/// point, then walks outdoors in a straight line to another, and so on.
/// Nodes that are not sleepy never move.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Movement {
    /// The share of sleepy nodes that walk; 0 for none.
    pub fraction: f64,
    /// The share of relays placed as leaves (`relay_share`) that walk:
    /// a person's node on a power bank. 0 for none.
    pub relay_fraction: f64,
    /// Walking speed, metres a second.
    pub speed_m_s: f64,
    /// Mean straight-line length of a walk, metres; exponential, in a
    /// random direction, folded back at the edges of the area the nodes
    /// were placed in.
    pub trip_m: f64,
    /// Mean time at a stop, seconds; exponential.
    pub stop_s: f64,
    /// The distance over which the shadowing of a walker's links changes,
    /// metres.
    pub shadow_decorrelation_m: f64,
}

impl Default for Movement {
    fn default() -> Self {
        Movement {
            fraction: 0.0,
            relay_fraction: 0.0,
            // A common pedestrian walking speed.
            speed_m_s: 1.4,
            trip_m: 1000.0,
            stop_s: 3600.0,
            // 3GPP TR 38.901, urban macro, not in line of sight.
            shadow_decorrelation_m: 50.0,
        }
    }
}

/// People's nodes whose power switches between USB and battery, and with
/// it their role (spec "Nodes", roadmap D71). The simulator reports
/// each change to the node's core. A node that does not switch keeps the
/// power its placement gives it: a relay on USB, a sleepy node on battery.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Power {
    /// The share of the people's nodes that neither walk nor ride whose
    /// power switches at random, from the power its placement gives it;
    /// 0 for none.
    pub fraction: f64,
    /// Mean time on USB, seconds; exponential.
    pub usb_s: f64,
    /// Mean time on battery, seconds; exponential.
    pub battery_s: f64,
    /// Every walking person's node is on battery while it walks and on USB
    /// at its stops, as people charge where they stop.
    pub walkers: bool,
}

impl Default for Power {
    fn default() -> Self {
        Power {
            fraction: 0.0,
            usb_s: 7200.0,
            battery_s: 1800.0,
            walkers: false,
        }
    }
}

/// Each node's battery, and the grid's outages, which take USB power
/// from every node on it (`battery.rs`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Battery {
    /// Model the batteries at all.
    pub on: bool,
    /// The nominal capacity, mAh.
    pub capacity_mah: f64,
    /// Each cell's capacity as a share of the nominal, drawn uniformly
    /// between the two.
    pub cell: [f64; 2],
    /// The charge of a node on USB at the start as a share of its cell's
    /// capacity, drawn uniformly between the two.
    pub charge_on_usb: [f64; 2],
    /// The same for a node on battery.
    pub charge_on_battery: [f64; 2],
    /// Each node's reading is off by a factor drawn uniformly within
    /// 1 ± this.
    pub gain_error: f64,
    /// The charging current on USB, mA.
    pub charge_ma: f64,
    /// The board's current besides the radio, µA, of a relay and of a
    /// sleepy node.
    pub relay_ua: f64,
    /// See `relay_ua`.
    pub sleepy_ua: f64,
    /// How often the driver reads the battery, seconds.
    pub read_s: f64,
    /// The grid's outages, `[from, to]` seconds, each after the settle
    /// time.
    pub outages: Vec<[f64; 2]>,
}

impl Default for Battery {
    fn default() -> Self {
        Battery {
            on: false,
            capacity_mah: 850.0,
            cell: [1.0, 1.0],
            charge_on_usb: [1.0, 1.0],
            charge_on_battery: [1.0, 1.0],
            gain_error: 0.0,
            charge_ma: 500.0,
            relay_ua: 0.0,
            sleepy_ua: 0.0,
            read_s: 60.0,
            outages: Vec::new(),
        }
    }
}

/// Sleepy nodes carried in vehicles. A share of the sleepy nodes that do
/// not walk ride: each stops indoors at a point of a straight route, then
/// is driven along the route to another point drawn uniformly on it, and
/// so on. Driven, its links are outdoor ones less the vehicle's body.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Vehicles {
    /// The share of sleepy nodes that ride; 0 for none.
    pub fraction: f64,
    /// The share of relays placed as leaves that do not walk and ride.
    pub relay_fraction: f64,
    /// Driving speed, metres a second.
    pub speed_m_s: f64,
    /// Mean time at a stop, seconds; exponential.
    pub stop_s: f64,
    /// The route's two ends, `[x0, y0, x1, y1]`, metres.
    pub route: [f64; 4],
    /// What the vehicle's body takes from every link of a node inside it
    /// while it drives, dB.
    pub loss_db: f64,
}

impl Default for Vehicles {
    fn default() -> Self {
        Vehicles {
            fraction: 0.0,
            relay_fraction: 0.0,
            speed_m_s: 10.0,
            stop_s: 600.0,
            route: [0.0; 4],
            // 3GPP TR 38.901 §7.4.3.2: the mean penetration loss of a car.
            loss_db: 9.0,
        }
    }
}

/// A node held on one home channel from its boot, for a scenario that
/// needs two nodes on one channel (`Config::home_override`): it neither
/// chooses nor moves its home, and a sleepy one does not follow its
/// anchor's.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomeSpec {
    /// The node, as a node reference.
    pub node: String,
    /// Its channel.
    pub channel: u8,
}

/// How a GPS fix goes wrong (`GpsFault`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FaultKind {
    /// From `at_s` every fix is `distance_m` off, in one direction: a
    /// spoofed position.
    Jump,
    /// From `at_s` the fixes walk away at `speed_m_s`, in one direction:
    /// a spoofer that pulls the position slowly.
    Drift,
}

/// Nodes whose GPS gives wrong fixes from a time on
/// (spec "Position"), drawn from the run's seed among the relays or among the sleepy
/// nodes that do not move, each in its own random direction.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GpsFault {
    /// How the fixes go wrong.
    pub kind: FaultKind,
    /// Sleepy nodes, or relays.
    pub sleepy: bool,
    /// How many nodes.
    pub count: u32,
    /// From when, seconds.
    pub at_s: f64,
    /// How far off a jump puts the fixes, metres.
    #[serde(default)]
    pub distance_m: f64,
    /// How fast a drift pulls them, metres a second.
    #[serde(default)]
    pub speed_m_s: f64,
}

/// GPS fixes that go wrong, and whether a node's driver applies the
/// plausibility checks of spec "Position" before handing a fix to its
/// core. A relay with a fault takes a fix every second, the receiver's
/// usual output rate; one that walks, one at each heartbeat; a sleepy node
/// one before each beat (spec "Beats"). Other nodes are handed no fixes,
/// since a true one changes nothing, unless `relays_every_s` or
/// `error_m` says otherwise.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Gps {
    /// The faults.
    pub faults: Vec<GpsFault>,
    /// Whether the checks are applied.
    pub checks: bool,
    /// Every node that is not sleepy takes a fix this often, seconds,
    /// walking or not, and none at its heartbeats; 0 for the default
    /// above.
    pub relays_every_s: f64,
    /// The error of a fix of a node that is not sleepy, metres: on each
    /// axis a Gauss-Markov process of this standard deviation and
    /// `error_correlation_s`, around where the node is. 0 for none. A
    /// research model of a receiver's wander: the L76K gives 2 m CEP
    /// under open sky (its specification), and nothing is measured for a
    /// windowsill.
    pub error_m: f64,
    /// See `error_m`, seconds.
    pub error_correlation_s: f64,
}

/// A complete scenario.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Scenario {
    /// Name, carried into the summary.
    pub name: String,
    /// Run seed.
    pub seed: u64,
    /// Simulated duration, seconds.
    pub duration_s: f64,
    /// How nodes get their IDs.
    pub ids: Ids,
    /// When nodes are switched on.
    pub boot: Boot,
    /// How their clocks run.
    pub clocks: Clocks,
    /// Which of them walk, and how.
    pub movement: Movement,
    /// Which of them ride in vehicles, and how.
    pub vehicles: Vehicles,
    /// Pairs forced to share an ID.
    pub shared_ids: Vec<SharedIds>,
    /// Groups of sleepy nodes that go dark together.
    pub dark_groups: Vec<DarkGroup>,
    /// GPS fixes that go wrong.
    pub gps: Gps,
    /// Whose power switches between USB and battery.
    pub power: Power,
    /// The nodes' batteries and the grid's outages.
    pub battery: Battery,
    /// How long a node takes to settle after it is switched on, seconds:
    /// the run has settled this long after the last switch-on
    /// ([`Scenario::settled_s`]), and messages originated before then are
    /// summarised apart, so start-up losses do not mask the steady state.
    /// 0 for no start-up phase.
    pub settle_s: f64,
    /// Origin of the local grid.
    pub origin: Origin,
    /// Radio parameters.
    pub radio: RadioParams,
    /// Link generator.
    pub links: LinkParams,
    /// Protocol parameter overrides.
    pub protocol: ProtocolOverrides,
    /// Node placements, in order.
    pub nodes: Vec<Placement>,
    /// Foreign interferers.
    pub interferers: Vec<InterfererSpec>,
    /// Traffic sources.
    pub traffic: Vec<TrafficSpec>,
    /// Leaf availability process; `None` keeps every leaf available.
    pub availability: Option<Availability>,
    /// Scripted events.
    pub events: Vec<ScriptedEvent>,
    /// Metrics options.
    pub metrics: MetricsParams,
    /// Nodes held on one home channel.
    pub homes: Vec<HomeSpec>,
}

impl Default for Scenario {
    fn default() -> Self {
        Scenario {
            name: "unnamed".into(),
            seed: 1,
            duration_s: 60.0,
            ids: Ids::default(),
            boot: Boot::default(),
            clocks: Clocks::default(),
            movement: Movement::default(),
            vehicles: Vehicles::default(),
            shared_ids: Vec::new(),
            dark_groups: Vec::new(),
            gps: Gps::default(),
            power: Power::default(),
            battery: Battery::default(),
            settle_s: 0.0,
            origin: Origin::default(),
            radio: RadioParams::default(),
            links: LinkParams::default(),
            protocol: ProtocolOverrides::default(),
            nodes: Vec::new(),
            interferers: Vec::new(),
            traffic: Vec::new(),
            availability: None,
            events: Vec::new(),
            homes: Vec::new(),
            metrics: MetricsParams {
                per_node: true,
                ..MetricsParams::default()
            },
        }
    }
}

/// A placed node.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedNode {
    /// Name, if given.
    pub name: Option<String>,
    /// Role.
    pub role: RoleSpec,
    /// Position.
    pub xy: Xy,
    /// Indoor.
    pub indoor: bool,
    /// On a roof.
    pub rooftop: bool,
    /// A person's node: a leaf, or a relay placed as a leaf
    /// (`relay_share`). People send and receive texts and may walk.
    pub person: bool,
}

impl PlacedNode {
    /// Where its antenna is, for the link generator.
    pub fn site(&self) -> Site {
        match (self.indoor, self.rooftop) {
            (true, _) => Site::Indoor,
            (false, true) => Site::Rooftop,
            (false, false) => Site::Outdoor,
        }
    }
}

impl Scenario {
    /// Parse TOML.
    pub fn from_toml(s: &str) -> Result<Scenario, String> {
        toml::from_str(s).map_err(|e| e.to_string())
    }

    /// Read and parse a TOML file.
    pub fn load(path: &std::path::Path) -> Result<Scenario, String> {
        let s = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Scenario::from_toml(&s).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// When the run has settled, seconds: [`Scenario::settle_s`] after the
    /// last node is switched on.
    pub fn settled_s(&self) -> f64 {
        self.settle_s.max(0.0) + self.boot.last_s()
    }

    /// Builder: a named scenario with a seed and duration.
    pub fn new(name: &str, seed: u64, duration_s: f64) -> Scenario {
        Scenario {
            name: name.into(),
            seed,
            duration_s,
            ..Scenario::default()
        }
    }

    /// Builder: add a named point node.
    pub fn point(mut self, name: &str, role: RoleSpec, x: f64, y: f64) -> Scenario {
        self.nodes.push(Placement::Point {
            name: Some(name.into()),
            role,
            x,
            y,
            indoor: false,
            rooftop: false,
        });
        self
    }

    /// Builder: add a placement.
    pub fn place(mut self, p: Placement) -> Scenario {
        self.nodes.push(p);
        self
    }

    /// Builder: add an explicit symmetric link of `dbm` (table model).
    pub fn link(mut self, a: &str, b: &str, dbm: f64) -> Scenario {
        self.links.model = crate::links::Model::Table;
        self.links.table.push(crate::links::TableLink {
            a: a.into(),
            b: b.into(),
            dbm,
            dbm_back: None,
            channel_offsets_db: None,
        });
        self
    }

    /// Builder: add a scripted event.
    pub fn script(
        mut self,
        at_s: f64,
        node: &str,
        action: ScriptAction,
        channel: Option<u8>,
    ) -> Scenario {
        self.events.push(ScriptedEvent {
            at_s,
            node: node.into(),
            action,
            channel,
            len: None,
            preamble_symbols: None,
        });
        self
    }

    /// Builder: add a scripted transmission.
    pub fn script_tx(
        mut self,
        at_s: f64,
        node: &str,
        channel: u8,
        len: u8,
        preamble: u16,
    ) -> Scenario {
        self.events.push(ScriptedEvent {
            at_s,
            node: node.into(),
            action: ScriptAction::Transmit,
            channel: Some(channel),
            len: Some(len),
            preamble_symbols: Some(preamble),
        });
        self
    }

    /// Expand placements into nodes, relays first then leaves, each group
    /// in file order. Deterministic in the seed.
    pub fn placed(&self) -> Result<Vec<PlacedNode>, String> {
        let mut out = Vec::new();
        for (pi, p) in self.nodes.iter().enumerate() {
            let mut rng = Stream::new(self.seed, Purpose::Placement, pi as u64);
            match p {
                Placement::Point {
                    name,
                    role,
                    x,
                    y,
                    indoor,
                    rooftop,
                } => out.push(PlacedNode {
                    name: name.clone(),
                    role: *role,
                    xy: Xy { x: *x, y: *y },
                    indoor: *indoor,
                    rooftop: *rooftop,
                    person: *role == RoleSpec::Leaf,
                }),
                Placement::Lattice {
                    role,
                    x0,
                    y0,
                    width_m,
                    height_m,
                    spacing_m,
                    jitter_m,
                    indoor,
                    rooftop,
                } => {
                    if *spacing_m <= 0.0 {
                        return Err("lattice spacing_m must be positive".into());
                    }
                    let nx = (width_m / spacing_m).floor() as usize + 1;
                    let ny = (height_m / spacing_m).floor() as usize + 1;
                    for j in 0..ny {
                        for i in 0..nx {
                            let jx = (rng.uniform() * 2.0 - 1.0) * jitter_m;
                            let jy = (rng.uniform() * 2.0 - 1.0) * jitter_m;
                            out.push(PlacedNode {
                                name: None,
                                role: *role,
                                xy: Xy {
                                    x: x0 + i as f64 * spacing_m + jx,
                                    y: y0 + j as f64 * spacing_m + jy,
                                },
                                indoor: *indoor,
                                rooftop: *rooftop,
                                person: *role == RoleSpec::Leaf,
                            });
                        }
                    }
                }
                Placement::Density {
                    role,
                    x0,
                    y0,
                    width_m,
                    height_m,
                    per_km2,
                    indoor,
                    rooftop,
                    relay_share,
                } => {
                    let n = (per_km2 * width_m * height_m / 1e6).round() as usize;
                    let mut share = Stream::new(self.seed, Purpose::RelayShare, pi as u64);
                    for _ in 0..n {
                        let relay = *role == RoleSpec::Leaf && share.chance(*relay_share);
                        out.push(PlacedNode {
                            name: None,
                            role: if relay { RoleSpec::Relay } else { *role },
                            xy: Xy {
                                x: x0 + rng.uniform() * width_m,
                                y: y0 + rng.uniform() * height_m,
                            },
                            indoor: *indoor,
                            rooftop: *rooftop,
                            person: *role == RoleSpec::Leaf,
                        });
                    }
                }
            }
        }
        if let Some(n) = out.iter().find(|n| n.indoor && n.rooftop) {
            return Err(format!("node at {:?} is both indoor and on a roof", n.xy));
        }
        // Stable partition: relays first.
        let (mut relays, leaves): (Vec<_>, Vec<_>) =
            out.into_iter().partition(|n| n.role == RoleSpec::Relay);
        relays.extend(leaves);
        let mut names: Vec<&str> = relays.iter().filter_map(|n| n.name.as_deref()).collect();
        names.sort_unstable();
        if let Some(w) = names.windows(2).find(|w| w[0] == w[1]) {
            return Err(format!("node name {:?} used twice", w[0]));
        }
        Ok(relays)
    }
}

/// Resolve a node reference (a name, `relay:N` or `leaf:N`) to a global index.
pub fn resolve(nodes: &[PlacedNode], r: &str) -> Result<u32, String> {
    if let Some(i) = nodes.iter().position(|n| n.name.as_deref() == Some(r)) {
        return Ok(i as u32);
    }
    let relays = nodes.iter().filter(|n| n.role == RoleSpec::Relay).count();
    let parse = |s: &str, count: usize, offset: usize| -> Result<u32, String> {
        let i: usize = s.parse().map_err(|_| format!("bad node reference {r:?}"))?;
        if i >= count {
            return Err(format!(
                "node reference {r:?} out of range ({count} placed)"
            ));
        }
        Ok((offset + i) as u32)
    };
    if let Some(s) = r.strip_prefix("relay:") {
        parse(s, relays, 0)
    } else if let Some(s) = r.strip_prefix("leaf:") {
        parse(s, nodes.len() - relays, relays)
    } else {
        Err(format!("unknown node {r:?}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placements_expand_relays_first() {
        let s = Scenario::new("t", 3, 1.0)
            .point("L", RoleSpec::Leaf, 0.0, 0.0)
            .point("R", RoleSpec::Relay, 10.0, 0.0)
            .place(Placement::Lattice {
                role: RoleSpec::Relay,
                x0: 0.0,
                y0: 0.0,
                width_m: 1000.0,
                height_m: 500.0,
                spacing_m: 500.0,
                jitter_m: 20.0,
                indoor: false,
                rooftop: false,
            })
            .place(Placement::Density {
                role: RoleSpec::Leaf,
                x0: 0.0,
                y0: 0.0,
                width_m: 1000.0,
                height_m: 1000.0,
                per_km2: 25.0,
                indoor: true,
                rooftop: false,
                relay_share: 0.0,
            });
        let n = s.placed().unwrap();
        assert_eq!(n.len(), 1 + 1 + 6 + 25);
        assert!(n[..7].iter().all(|n| n.role == RoleSpec::Relay));
        assert_eq!(resolve(&n, "R").unwrap(), 0);
        assert_eq!(resolve(&n, "L").unwrap(), 7);
        assert_eq!(resolve(&n, "relay:6").unwrap(), 6);
        assert_eq!(resolve(&n, "leaf:25").unwrap(), 32);
        assert!(resolve(&n, "leaf:26").is_err());
        assert_eq!(s.placed().unwrap(), n, "deterministic");
    }

    /// A share of a leaf placement are relays, drawn apart from where the
    /// people stand: the places are those of no share, and every node
    /// placed is still a person's.
    #[test]
    fn a_share_of_people_are_relays_where_they_stand() {
        let people = |share: f64| {
            Scenario::new("t", 3, 1.0)
                .place(Placement::Density {
                    role: RoleSpec::Leaf,
                    x0: 0.0,
                    y0: 0.0,
                    width_m: 1000.0,
                    height_m: 1000.0,
                    per_km2: 400.0,
                    indoor: true,
                    rooftop: false,
                    relay_share: share,
                })
                .placed()
                .unwrap()
        };
        let (none, some) = (people(0.0), people(0.25));
        let relays = some.iter().filter(|n| n.role == RoleSpec::Relay).count();
        assert!((80..120).contains(&relays), "{relays} relays of 400");
        assert!(some.iter().all(|n| n.person));
        let mut a: Vec<(i64, i64)> = none
            .iter()
            .map(|n| (n.xy.x as i64, n.xy.y as i64))
            .collect();
        let mut b: Vec<(i64, i64)> = some
            .iter()
            .map(|n| (n.xy.x as i64, n.xy.y as i64))
            .collect();
        a.sort_unstable();
        b.sort_unstable();
        assert_eq!(a, b);
    }

    #[test]
    fn overrides_apply() {
        let o = ProtocolOverrides {
            ack_timeout_ms: Some(250),
            tx_power_dbm: Some(7),
            ..Default::default()
        };
        let c = o.config();
        assert_eq!(c.ack_timeout, Duration::from_millis(250));
        assert_eq!(c.tx_power_dbm, 7);
        assert_eq!(c.heartbeat_min, Config::default().heartbeat_min);
    }

    #[test]
    fn toml_rejects_unknown_fields() {
        assert!(Scenario::from_toml("name = \"x\"\nbogus = 1\n").is_err());
        assert!(Scenario::from_toml("[radio]\ncapture_db = 6.0\n").is_ok());
        let e = Scenario::from_toml(
            "[[nodes]]\nplace = \"point\"\nrole = \"relay\"\nx = 0\ny = 0\nz = 1\n",
        );
        assert!(e.is_err());
    }
}
