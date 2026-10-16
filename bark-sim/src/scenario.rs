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
    /// Relay core.
    Relay,
    /// Leaf core.
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
    },
}

/// Overrides of the protocol [`Config`]; absent fields keep the design's
/// defaults. Durations in milliseconds.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[allow(missing_docs)]
pub struct ProtocolOverrides {
    pub tx_power_dbm: Option<i8>,
    pub surveyed: Option<bool>,
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
        set!(surveyed);
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
    /// Nodes boot uniformly at random in `[0, boot_spread_s]`.
    pub boot_spread_s: f64,
    /// When the network should have settled, seconds: messages originated
    /// before it are summarised apart, so start-up losses do not mask the
    /// steady state. 0 for no start-up phase.
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
}

impl Default for Scenario {
    fn default() -> Self {
        Scenario {
            name: "unnamed".into(),
            seed: 1,
            duration_s: 60.0,
            boot_spread_s: 0.0,
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
                } => {
                    let n = (per_km2 * width_m * height_m / 1e6).round() as usize;
                    for _ in 0..n {
                        out.push(PlacedNode {
                            name: None,
                            role: *role,
                            xy: Xy {
                                x: x0 + rng.uniform() * width_m,
                                y: y0 + rng.uniform() * height_m,
                            },
                            indoor: *indoor,
                            rooftop: *rooftop,
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
