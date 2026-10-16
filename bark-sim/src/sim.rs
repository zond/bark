//! The engine: one event heap, the cores, and the driver side of
//! `docs/core-interface.md`.
//!
//! Everything that happens is a heap entry `(Instant, sequence, event)`:
//! boots, core wakeups, transmissions reaching the air and leaving it, RSSI
//! sample instants, excursion completions, traffic, fading steps,
//! interferer and availability toggles, scripted events, metric samples.
//! The sequence number breaks ties in insertion order, so a run is a pure
//! function of its scenario. Nothing iterates over all nodes per event.
//!
//! Each core has at most one wakeup entry in the heap that counts: entries
//! carry the core's wakeup generation and stale ones are dropped on pop.
//! Excursion events are invalidated the same way.
//!
//! The reception model is in `radio.rs`.

use crate::geo::{Grid, Origin, Xy};
use crate::links::{LinkTable, NodeRadio, Site};
use crate::metrics::{DropReason, Metrics, Observer, Outcome};
use crate::phy::{self, Deaf, ErrorCurve, Loss, RadioParams};
use crate::rng::{stream_seed, Purpose, Stream};
use crate::scenario::{
    resolve, Destination, PlacedNode, RoleSpec, Scenario, ScriptAction, TrafficSpec,
};
use crate::trace::{Names, Tracer};
use bark_proto::radio::{time_on_air, MIN_PREAMBLE_SYMBOLS};
use bark_proto::{
    Action, AppCommand, AppEvent, Channel, Config, Core, Duration, Event, Frame, Instant, Leaf,
    LeafId, MsgId, Position, Relay, Trace, MAX_FRAME,
};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

/// Something scheduled. Indices are node, slot, flow or script indices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ev {
    /// Power-on of a node.
    Boot(u32),
    /// Core wakeup, valid if the generation matches.
    Wakeup(u32, u32),
    /// A transmission's first symbol reaches the air (slot, slot generation).
    AirStart(u32, u32),
    /// A transmission's last symbol leaves the air (slot, slot generation).
    AirEnd(u32, u32),
    /// The instant an RSSI excursion reads the register (node, excursion generation).
    RssiRead(u32, u32),
    /// Excursion complete (node, excursion generation).
    ExcursionEnd(u32, u32),
    /// Periodic flow fires.
    Flow(u32),
    /// Poisson arrival at a leaf (global node index).
    Poisson(u32),
    /// Fading step.
    FadeTick,
    /// Interferer toggles (interferer, generation).
    Interferer(u32, u32),
    /// Leaf availability toggles.
    Availability(u32),
    /// Scripted event.
    Script(u32),
    /// A check held during a reception starts (node, hold generation).
    Held(u32, u32),
    /// Time-series sample.
    Sample,
}

struct Entry {
    t: Instant,
    seq: u64,
    ev: Ev,
}

impl PartialEq for Entry {
    fn eq(&self, o: &Self) -> bool {
        (self.t, self.seq) == (o.t, o.seq)
    }
}
impl Eq for Entry {}
impl PartialOrd for Entry {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Entry {
    /// Reversed: `BinaryHeap` is a max-heap and we want the earliest.
    fn cmp(&self, o: &Self) -> Ordering {
        (o.t, o.seq).cmp(&(self.t, self.seq))
    }
}

/// Kind of excursion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExKind {
    Cad,
    Rssi,
    Transmit,
}

/// An excursion in progress.
#[derive(Clone, Debug)]
pub(crate) struct Excursion {
    pub kind: ExKind,
    pub channel: Channel,
    /// End of each phase.
    pub bounds: [Instant; 3],
    /// Why the node is deaf in each phase.
    pub reasons: [Deaf; 3],
    /// Started by a scripted event: complete into the log, not the core.
    pub scripted: bool,
    /// CAD: a qualifying signal overlapped the sampling window.
    pub cad_seen: bool,
    /// RSSI: the value read.
    pub rssi_dbm: Option<i16>,
    /// Whether the node stops receiving for it. An RSSI reading on the
    /// resting channel does not (docs/core-interface.md §3 rule 7).
    pub deaf: bool,
    /// Transmit: the air slot.
    pub slot: Option<u32>,
}

impl Excursion {
    /// Deaf reason at `t`.
    pub fn reason_at(&self, t: Instant) -> Deaf {
        for (b, r) in self.bounds.iter().zip(self.reasons) {
            if t < *b {
                return r;
            }
        }
        self.reasons[2]
    }
}

/// A signal on the air at a node: a packet or an interferer.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Arrival {
    /// Unique per transmission or interferer burst.
    pub key: u64,
    pub channel: u8,
    pub dbm: f64,
    pub mw: f64,
    pub end: Instant,
    /// CAD can see it.
    pub lora: bool,
}

/// What a receiver's demodulator is doing.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Lock {
    /// Searching for a preamble.
    Idle,
    /// Locked to a packet.
    Rx(Rx),
    /// Demodulating a corrupted packet; deaf until then.
    Corrupt(Instant),
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Rx {
    pub key: u64,
    pub channel: u8,
    pub dbm: f64,
    pub mw: f64,
    pub preamble_end: Instant,
    /// Until here a lost packet frees the receiver: the header has not
    /// yet told it the packet's length.
    pub header_end: Instant,
    pub end: Instant,
    /// Weight of a preamble symbol in the hazard (`radio::Packet`).
    pub preamble_weight: f64,
    /// Worst instantaneous interference so far, mW.
    pub max_i_mw: f64,
    /// Error hazard accrued up to `since`, and the hazard at which the
    /// packet is lost, drawn at lock.
    pub spent: f64,
    pub budget: f64,
    /// Since when `rate` (hazard per symbol) and `cause` hold.
    pub since: Instant,
    pub rate: f64,
    pub cause: Loss,
    /// Whether the threshold rule would pass the packet so far.
    pub threshold_ok: bool,
    /// The interference `rate` was computed for, co-channel and adjacent
    /// mW, so that a change elsewhere in the band costs no recomputation.
    pub rated_for: Option<(f64, f64)>,
}

/// Driver state of one node.
pub(crate) struct NodeState {
    pub alive: bool,
    /// In radio reach (false while a leaf is dark).
    pub present: bool,
    pub resting: Option<Channel>,
    /// After a `Listen` retune, the radio listens from here.
    pub listen_from: Instant,
    pub ex: Option<Excursion>,
    pub ex_gen: u32,
    pub wake_gen: u32,
    pub wake_at: Option<Instant>,
    pub lock: Lock,
    pub arrivals: Vec<Arrival>,
    pub observer: Option<u32>,
    pub cad_rng: Stream,
    pub rx_rng: Stream,
    /// A CAD or RSSI held while a packet is being received (core-interface
    /// spec "Regulatory limits" rule 8), and its generation.
    pub held: Option<(ExKind, Channel)>,
    pub held_gen: u32,
    /// Times the node was restarted (`ScriptAction::Restart`).
    pub restarts: u32,
}

/// A transmission, from excursion start until it leaves the air.
pub(crate) struct AirTx {
    pub src: u32,
    pub key: u64,
    pub gen: u32,
    pub frame: Frame,
    pub on_air: bool,
    pub start: Instant,
    pub end: Instant,
}

pub(crate) struct Interferer {
    pub channel: u8,
    pub lora: bool,
    pub mean_on_s: f64,
    pub mean_off_s: f64,
    pub reach: Vec<(u32, f32)>,
    pub on: bool,
    pub gen: u32,
    pub key: u64,
    pub end: Instant,
    pub rng: Stream,
}

struct Flow {
    src: u32,
    dst: u32,
    interval: Duration,
    len: u8,
}

struct PoissonTraffic {
    mean_gap_s: f64,
    min: u8,
    max: u8,
    dest: Destination,
}

/// What became of a message, as far as the simulator has seen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fate {
    /// Still held at the end: waiting for its origin's core to take it,
    /// queued at a relay, waiting for its leaf, or held by the leaf that
    /// sent it.
    InFlight,
    /// Neither delivered, nor reported lost, nor held by any core: lost
    /// without a trace.
    Unaccounted,
    /// Reached its destination's application.
    Delivered,
    /// Dropped by a core; the last drop reported wins.
    Dropped {
        /// Where.
        node: u32,
        /// Why.
        reason: DropReason,
    },
}

struct OriginRecord {
    src: u32,
    dst: u32,
    /// When the traffic model first had it for its origin.
    t: Instant,
    fate: Fate,
    /// Whether the destination could be reached from the origin at the
    /// start of the run; see [`Sim::reachable`].
    reachable: bool,
    /// When it first reached its destination's application.
    delivered_at: Option<Instant>,
    /// When its origin's core took it from the application.
    taken_at: Option<Instant>,
    /// When a node dropped it.
    dropped_at: Option<Instant>,
    /// Frames that carried it or answered it (DATA, DELIVER, their ACKs),
    /// and their time on air, µs.
    frames: u32,
    airtime_us: u64,
    /// The largest hand-over count a DATA frame of it carried (spec "Hop count").
    hops: u8,
}

/// One originated message, for the per-message table (`Sim::message_rows`).
#[derive(Clone, Debug, PartialEq)]
pub struct MessageRow {
    /// Traffic tag, in origination order.
    pub tag: u32,
    /// Origin node.
    pub src: u32,
    /// Destination node.
    pub dst: u32,
    /// When the traffic model had it, seconds.
    pub t_s: f64,
    /// What became of it.
    pub fate: Fate,
    /// Whether its destination was reachable ([`Sim::reachable`]).
    pub reachable: bool,
    /// Origination to delivery, seconds.
    pub latency_s: Option<f64>,
    /// Frames that carried or answered it.
    pub frames: u32,
    /// Time on air of those frames, seconds.
    pub airtime_s: f64,
    /// The largest hand-over count it carried.
    pub hops: u8,
    /// Straight-line distance from origin to destination, metres.
    pub distance_m: f64,
    /// Origination to its core taking it, seconds.
    pub taken_s: Option<f64>,
    /// Origination to its drop, seconds.
    pub dropped_s: Option<f64>,
}

/// A message an application waits to hand to its core.
struct Offer {
    tag: u32,
    dst_pos: Position,
    dst_leaf: LeafId,
    len: u8,
}

/// A scripted excursion's result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptResult {
    /// CAD finished.
    Cad {
        /// Node.
        node: u32,
        /// When.
        at: Instant,
        /// Channel.
        channel: u8,
        /// Detected.
        busy: bool,
    },
    /// RSSI read.
    Rssi {
        /// Node.
        node: u32,
        /// When.
        at: Instant,
        /// Channel.
        channel: u8,
        /// Value.
        dbm: i16,
    },
    /// Transmission finished.
    TxDone {
        /// Node.
        node: u32,
        /// When.
        at: Instant,
    },
}

impl ScriptResult {
    /// When the excursion completed.
    pub fn at(&self) -> Instant {
        match *self {
            ScriptResult::Cad { at, .. }
            | ScriptResult::Rssi { at, .. }
            | ScriptResult::TxDone { at, .. } => at,
        }
    }
}

struct ScriptEntry {
    node: u32,
    action: ScriptAction,
    channel: Option<Channel>,
    len: u8,
    preamble: u16,
}

/// The simulator.
pub struct Sim {
    pub(crate) radio: RadioParams,
    pub(crate) curve: ErrorCurve,
    pub(crate) sens_dbm: f64,
    pub(crate) noise_mw: f64,
    /// Linear rejection factor by channel offset 0, 1, 2.
    pub(crate) rej: [f64; 3],
    /// Busy threshold for observers: signal power above which RSSI exceeds
    /// the noise floor by the protocol's busy threshold, mW.
    pub(crate) busy_signal_mw: f64,
    pub(crate) links: LinkTable,
    names: Vec<Option<String>>,
    xy: Vec<Xy>,
    sites: Vec<Site>,
    /// Relay node by short name (spec "Relay identity").
    pub(crate) relay_index: HashMap<u32, u32>,
    /// Whether the relay a core event is being delivered to was booting
    /// before it, so that its boot choice is not counted as a move.
    was_booting: bool,
    positions: Vec<Position>,
    n_relays: u32,
    relays: Vec<Relay>,
    leaves: Vec<Leaf>,
    pub(crate) nodes: Vec<NodeState>,
    pub(crate) now: Instant,
    heap: BinaryHeap<Entry>,
    seq: u64,
    pub(crate) txs: Vec<Option<AirTx>>,
    free_slots: Vec<u32>,
    slot_gen: Vec<u32>,
    next_key: u64,
    pub(crate) interferers: Vec<Interferer>,
    flows: Vec<Flow>,
    poisson: Option<PoissonTraffic>,
    poisson_rng: Vec<Stream>,
    leaf_grid: Option<Grid>,
    leaf_xy: Vec<Xy>,
    /// Each node's address as a sender last learned it: a relay's
    /// position, or the relay a leaf was last attached to.
    address: Vec<Option<Position>>,
    availability: Option<(f64, f64)>,
    avail_rng: Vec<Stream>,
    /// Per leaf, when it came back into reach, until a relay hears it.
    back_at: Vec<Option<Instant>>,
    /// Leaf ID to node index, for the leaves that go dark.
    leaf_nodes: HashMap<[u8; 4], u32>,
    origins: Vec<OriginRecord>,
    /// Per node, the message its application waits to send.
    offers: Vec<Option<Offer>>,
    /// Message id (kept across hops) to traffic tag.
    msg_tags: HashMap<u32, u32>,
    script: Vec<ScriptEntry>,
    /// Results of scripted excursions, in completion order.
    pub script_log: Vec<ScriptResult>,
    cadence: Option<Duration>,
    end: Instant,
    actions: Vec<Action>,
    /// Counters.
    pub metrics: Metrics,
    /// Scenario name and seed, for the summary.
    pub(crate) scenario_name: String,
    pub(crate) seed: u64,
    pub(crate) per_node_summary: bool,
    /// Phase boundaries for the summary: the scenario's settle time, and
    /// the first scripted removal.
    pub(crate) settle: Instant,
    pub(crate) removal: Option<Instant>,
    /// The event trace, when asked for (`Sim::trace_to`).
    tracer: Option<Tracer>,
    /// Per relay, its connected component among the relays at the start
    /// of the run; see [`Sim::reachable`].
    component: Vec<u32>,
    /// The per-minute series, when asked for (`Sim::series_on`).
    series: Option<String>,
    /// Frames on air addressed to a relay: key to (relay, kind, state at
    /// the start), for [`crate::metrics::Addressed`].
    pub(crate) addressed: HashMap<u64, (u32, u8, u8)>,
    /// DATA on air addressed to a relay: key to the ACK that would answer
    /// it, message ID and `to`.
    pub(crate) answer_of: HashMap<u64, (u32, u32)>,
    /// DATA a relay received and has not answered yet: (relay, message,
    /// `to`) to the answer's deadline and whether a leaf sent it.
    pub(crate) awaiting: HashMap<(u32, u32, u32), (Instant, bool)>,
    /// The relay DATA a relay is being handed now: (relay, message,
    /// sender's short name, count), for [`crate::metrics::HopPlaceRefusals`].
    rx_data: Option<(u32, u32, u32, u8)>,
    /// Relays by short name, filled as refusals name them.
    by_short: HashMap<u32, u32>,
    /// Refused messages not yet sent again: (sender, message) to the
    /// refusal's time and whether a hop place was free for another.
    refused_at: HashMap<(u32, u32), (Instant, bool)>,
}

/// The interval of the series, µs.
const SERIES_EVERY_US: u64 = 60_000_000;

fn secs(s: f64) -> Duration {
    Duration::from_micros((s * 1e6).round() as u64)
}

impl Sim {
    /// Build a simulator from a scenario.
    pub fn new(sc: &Scenario) -> Result<Sim, String> {
        let placed = sc.placed()?;
        let n = placed.len();
        if n == 0 {
            return Err("scenario has no nodes".into());
        }
        let n_relays = placed.iter().filter(|p| p.role == RoleSpec::Relay).count() as u32;
        let base = sc.protocol.config();
        let radio = sc.radio.clone();
        let origin: Origin = sc.origin;
        let xy: Vec<Xy> = placed.iter().map(|p| p.xy).collect();
        let positions: Vec<Position> = xy.iter().map(|&p| origin.position(p)).collect();
        let names: Vec<Option<String>> = placed.iter().map(|p| p.name.clone()).collect();
        let tx_dbm = vec![base.tx_power_dbm as f64; n];
        let site: Vec<Site> = placed.iter().map(PlacedNode::site).collect();
        let links = LinkTable::build(
            &sc.links,
            &NodeRadio {
                xy: &xy,
                tx_dbm: &tx_dbm,
                site: &site,
                names: &names,
            },
            radio.cutoff_dbm(),
            sc.seed,
        )?;

        let config_for = |i: usize| Config {
            position: positions[i],
            ..base
        };
        let core_seed = |i: usize| stream_seed(sc.seed, Purpose::CoreSeed, i as u64);
        let relays: Vec<Relay> = (0..n_relays as usize)
            .map(|i| Relay::new(config_for(i), core_seed(i)))
            .collect();
        let leaves: Vec<Leaf> = (n_relays as usize..n)
            .map(|i| Leaf::new(config_for(i), core_seed(i)))
            .collect();

        let nodes: Vec<NodeState> = (0..n)
            .map(|i| NodeState {
                alive: true,
                present: true,
                resting: None,
                listen_from: Instant::ZERO,
                ex: None,
                ex_gen: 0,
                wake_gen: 0,
                wake_at: None,
                lock: Lock::Idle,
                arrivals: Vec::new(),
                observer: None,
                cad_rng: Stream::new(sc.seed, Purpose::CadMiss, i as u64),
                rx_rng: Stream::new(sc.seed, Purpose::Reception, i as u64),
                held: None,
                held_gen: 0,
                restarts: 0,
            })
            .collect();

        let noise_dbm = radio.noise_floor_dbm();
        let sens_dbm = radio.sensitivity_dbm();
        let rej = [0u8, 1, 2].map(|o| {
            phy::mw(
                -radio
                    .rejection_db(o)
                    .expect("offsets 0..=2 have a rejection"),
            )
        });
        let busy_signal_mw = phy::mw(noise_dbm) * (phy::mw(base.busy_threshold_db as f64) - 1.0);

        let mut sim = Sim {
            curve: ErrorCurve::new(&radio),
            radio,
            sens_dbm,
            noise_mw: phy::mw(noise_dbm),
            rej,
            busy_signal_mw,
            links,
            names,
            xy,
            relay_index: (0..n_relays)
                .map(|i| (positions[i as usize].short().0, i))
                .collect(),
            positions,
            sites: site,
            was_booting: false,
            n_relays,
            relays,
            leaves,
            nodes,
            now: Instant::ZERO,
            heap: BinaryHeap::new(),
            seq: 0,
            txs: Vec::new(),
            free_slots: Vec::new(),
            slot_gen: Vec::new(),
            next_key: 0,
            interferers: Vec::new(),
            flows: Vec::new(),
            poisson: None,
            poisson_rng: Vec::new(),
            leaf_grid: None,
            leaf_xy: Vec::new(),
            address: vec![None; n],
            availability: None,
            avail_rng: Vec::new(),
            back_at: Vec::new(),
            leaf_nodes: HashMap::new(),
            origins: Vec::new(),
            offers: (0..n).map(|_| None).collect(),
            msg_tags: HashMap::new(),
            script: Vec::new(),
            script_log: Vec::new(),
            cadence: sc.metrics.cadence_s.map(secs),
            end: Instant::ZERO + secs(sc.duration_s),
            actions: Vec::new(),
            metrics: Metrics::new(n),
            scenario_name: sc.name.clone(),
            seed: sc.seed,
            per_node_summary: sc.metrics.per_node,
            settle: Instant::ZERO + secs(sc.settle_s.max(0.0)),
            removal: sc
                .events
                .iter()
                .filter(|e| e.action == ScriptAction::Remove)
                .map(|e| Instant::ZERO + secs(e.at_s))
                .min(),
            tracer: None,
            component: Vec::new(),
            series: None,
            addressed: HashMap::new(),
            answer_of: HashMap::new(),
            awaiting: HashMap::new(),
            rx_data: None,
            by_short: HashMap::new(),
            refused_at: HashMap::new(),
        };
        sim.component = sim.components();
        sim.setup(sc, &placed)?;
        Ok(sim)
    }

    fn setup(&mut self, sc: &Scenario, placed: &[PlacedNode]) -> Result<(), String> {
        let n = placed.len();
        // Boots.
        for i in 0..n {
            let at = if sc.boot_spread_s > 0.0 {
                let mut r = Stream::new(sc.seed, Purpose::Boot, i as u64);
                secs(r.uniform() * sc.boot_spread_s)
            } else {
                Duration::ZERO
            };
            self.push(Instant::ZERO + at, Ev::Boot(i as u32));
        }
        // Observers.
        let all_relays: Vec<String> = (0..self.n_relays).map(|i| format!("relay:{i}")).collect();
        let observers = sc.metrics.observers.iter().flat_map(|o| {
            if o == "relay:*" {
                all_relays.clone()
            } else {
                vec![o.clone()]
            }
        });
        for o in observers {
            let i = resolve(placed, &o)?;
            self.nodes[i as usize].observer = Some(self.metrics.observers.len() as u32);
            self.metrics.observers.push(Observer {
                node: i,
                ..Observer::default()
            });
        }
        // Interferers.
        for (k, s) in sc.interferers.iter().enumerate() {
            if Channel::new(s.channel).is_none() {
                return Err(format!("interferer {k}: channel {} out of plan", s.channel));
            }
            if s.mean_on_s <= 0.0 {
                return Err(format!("interferer {k}: mean_on_s must be positive"));
            }
            let at = Xy { x: s.x, y: s.y };
            let floor = self.radio.cutoff_dbm();
            let reach = self
                .xy
                .iter()
                .enumerate()
                .filter_map(|(i, &p)| {
                    let mut dbm = s.power_dbm - sc.links.path_loss_db(at.dist(p));
                    if placed[i].indoor {
                        dbm -= sc.links.indoor_loss_db;
                    }
                    (dbm >= floor).then_some((i as u32, dbm as f32))
                })
                .collect();
            let mut rng = Stream::new(sc.seed, Purpose::Interferer, k as u64);
            // Start in the stationary state.
            let p_on = if s.mean_off_s > 0.0 {
                s.mean_on_s / (s.mean_on_s + s.mean_off_s)
            } else {
                1.0
            };
            let start_on = rng.chance(p_on) || s.mean_off_s <= 0.0;
            self.interferers.push(Interferer {
                channel: s.channel,
                lora: s.lora,
                mean_on_s: s.mean_on_s,
                mean_off_s: s.mean_off_s,
                reach,
                on: false,
                gen: 0,
                key: 0,
                end: Instant::ZERO,
                rng,
            });
            // Toggle at time zero into the start state.
            if start_on {
                self.push(Instant::ZERO, Ev::Interferer(k as u32, 0));
            } else {
                let off = secs(self.interferers[k].rng.exponential(s.mean_off_s));
                self.push(Instant::ZERO + off, Ev::Interferer(k as u32, 0));
            }
        }
        // Traffic.
        for (k, t) in sc.traffic.iter().enumerate() {
            match t {
                TrafficSpec::Periodic {
                    from,
                    to,
                    interval_s,
                    start_s,
                    payload_bytes,
                } => {
                    if *payload_bytes < 4 {
                        return Err("payload_bytes must be at least 4 (the delivery tag)".into());
                    }
                    if *interval_s <= 0.0 {
                        return Err("interval_s must be positive".into());
                    }
                    let f = Flow {
                        src: resolve(placed, from)?,
                        dst: resolve(placed, to)?,
                        interval: secs(*interval_s),
                        len: *payload_bytes,
                    };
                    let first = secs(start_s.unwrap_or(*interval_s));
                    self.flows.push(f);
                    self.push(Instant::ZERO + first, Ev::Flow(k as u32));
                }
                TrafficSpec::Poisson {
                    per_leaf_per_hour,
                    payload_min,
                    payload_max,
                    destination,
                } => {
                    if self.poisson.is_some() {
                        return Err("at most one poisson traffic source".into());
                    }
                    if *payload_min < 4 || payload_max < payload_min {
                        return Err("poisson payload needs 4 <= payload_min <= payload_max".into());
                    }
                    if *per_leaf_per_hour <= 0.0 {
                        continue;
                    }
                    let mean_gap_s = 3600.0 / per_leaf_per_hour;
                    self.poisson = Some(PoissonTraffic {
                        mean_gap_s,
                        min: *payload_min,
                        max: *payload_max,
                        dest: destination.clone(),
                    });
                    self.leaf_xy = self.xy[self.n_relays as usize..].to_vec();
                    if let Destination::Within { radius_m } = destination {
                        self.leaf_grid = Some(Grid::new(&self.leaf_xy, *radius_m));
                    }
                    for i in self.n_relays..n as u32 {
                        let mut r = Stream::new(sc.seed, Purpose::Traffic, (1u64 << 32) | i as u64);
                        let gap = secs(r.exponential(mean_gap_s));
                        self.poisson_rng.push(r);
                        self.push(Instant::ZERO + gap, Ev::Poisson(i));
                    }
                }
            }
        }
        // Availability.
        if let Some(a) = &sc.availability {
            if a.mean_available_s <= 0.0 || a.mean_dark_s <= 0.0 {
                return Err("availability periods must be positive".into());
            }
            self.availability = Some((a.mean_available_s, a.mean_dark_s));
            for i in self.n_relays..n as u32 {
                let mut r = Stream::new(sc.seed, Purpose::Availability, i as u64);
                let first = secs(r.exponential(a.mean_available_s));
                self.avail_rng.push(r);
                self.push(Instant::ZERO + first, Ev::Availability(i));
                self.leaf_nodes.insert(self.leaf_id(i).0, i);
            }
            self.back_at = vec![None; self.avail_rng.len()];
        }
        // Scripted events.
        for (k, e) in sc.events.iter().enumerate() {
            let node = resolve(placed, &e.node)?;
            let channel = match e.channel {
                Some(c) => Some(
                    Channel::new(c).ok_or_else(|| format!("event {k}: channel {c} out of plan"))?,
                ),
                None => None,
            };
            if matches!(
                e.action,
                ScriptAction::Listen
                    | ScriptAction::Transmit
                    | ScriptAction::Cad
                    | ScriptAction::Rssi
            ) && channel.is_none()
            {
                return Err(format!("event {k}: {:?} needs a channel", e.action));
            }
            self.script.push(ScriptEntry {
                node,
                action: e.action,
                channel,
                len: e.len.unwrap_or(16),
                preamble: e.preamble_symbols.unwrap_or(MIN_PREAMBLE_SYMBOLS),
            });
            self.push(Instant::ZERO + secs(e.at_s), Ev::Script(k as u32));
        }
        if let Some(step) = self.links.fading_step() {
            self.push(Instant::ZERO + step, Ev::FadeTick);
        }
        if let Some(c) = self.cadence {
            self.push(Instant::ZERO + c, Ev::Sample);
        }
        Ok(())
    }

    pub(crate) fn push(&mut self, t: Instant, ev: Ev) {
        self.seq += 1;
        self.heap.push(Entry {
            t,
            seq: self.seq,
            ev,
        });
    }

    /// Current simulated time.
    pub fn now(&self) -> Instant {
        self.now
    }

    /// Scenario end.
    pub fn end(&self) -> Instant {
        self.end
    }

    /// Number of nodes.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Number of relays (nodes `0..relays()` are relays, the rest leaves).
    pub fn relays(&self) -> u32 {
        self.n_relays
    }

    /// The link table.
    pub fn links(&self) -> &LinkTable {
        &self.links
    }

    /// A node's resting channel.
    pub fn resting(&self, node: u32) -> Option<Channel> {
        self.nodes[node as usize].resting
    }

    /// A relay core.
    pub fn relay(&self, i: u32) -> &Relay {
        &self.relays[i as usize]
    }

    /// Human label for a node, for messages.
    pub fn label(&self, node: u32) -> String {
        let role = if node < self.n_relays {
            "relay"
        } else {
            "leaf"
        };
        match &self.names[node as usize] {
            Some(n) => format!("{role} {node} ({n})"),
            None => format!("{role} {node}"),
        }
    }

    /// Write the per-node event trace (`trace.rs`) to `out` from now on.
    pub fn trace_to(&mut self, out: Box<dyn std::io::Write>) {
        let n = self.nodes.len() as u32;
        let names = Names {
            labels: (0..n).map(|i| self.label(i)).collect(),
            relays: (0..self.n_relays)
                .map(|i| (self.positions[i as usize].short().0, i))
                .collect(),
            leaves: (self.n_relays..n).map(|i| (self.leaf_id(i).0, i)).collect(),
        };
        self.tracer = Some(Tracer::new(out, names));
    }

    /// Run to the scenario end.
    pub fn run(&mut self) {
        self.run_until(self.end);
        if let Some(t) = &mut self.tracer {
            t.flush();
        }
    }

    /// Process every event at or before `t`, then set the clock to `t`.
    pub fn run_until(&mut self, t: Instant) {
        while self.heap.peek().is_some_and(|e| e.t <= t) {
            let e = self.heap.pop().expect("peeked");
            self.now = e.t;
            self.metrics.events += 1;
            self.dispatch(e.ev);
        }
        self.now = self.now.max(t);
    }

    fn dispatch(&mut self, ev: Ev) {
        match ev {
            Ev::Boot(n) => {
                if self.nodes[n as usize].alive {
                    // The contract has no boot event; power-on is delivered
                    // as a wakeup (see docs/core-interface.md §2).
                    self.deliver(n, Event::Wakeup);
                }
            }
            Ev::Wakeup(n, g) => {
                let st = &mut self.nodes[n as usize];
                if st.alive && st.wake_gen == g {
                    st.wake_at = None;
                    self.deliver(n, Event::Wakeup);
                }
            }
            Ev::AirStart(slot, g) => self.air_start(slot, g),
            Ev::AirEnd(slot, g) => self.air_end(slot, g, false),
            Ev::RssiRead(n, g) => self.rssi_read(n, g),
            Ev::ExcursionEnd(n, g) => self.excursion_end(n, g),
            Ev::Flow(k) => {
                let f = &self.flows[k as usize];
                let (src, dst, len, interval) = (f.src, f.dst, f.len, f.interval);
                self.originate(src, dst, len);
                self.push(self.now + interval, Ev::Flow(k));
            }
            Ev::Poisson(n) => self.poisson_arrival(n),
            Ev::FadeTick => {
                self.links.fading_tick();
                let step = self.links.fading_step().expect("fading on");
                self.push(self.now + step, Ev::FadeTick);
            }
            Ev::Interferer(k, g) => self.interferer_toggle(k, g),
            Ev::Availability(n) => self.availability_toggle(n),
            Ev::Script(k) => self.run_script(k),
            Ev::Held(n, g) => {
                let st = &mut self.nodes[n as usize];
                if st.alive && st.held_gen == g {
                    if let Some((kind, c)) = st.held.take() {
                        self.begin_excursion(n, kind, c, None, false);
                    }
                }
            }
            Ev::Sample => {
                self.metrics.sample(self.now.as_micros());
                self.sample_load();
                if self.now.as_micros().is_multiple_of(SERIES_EVERY_US) {
                    self.series_rows();
                }
                let c = self.cadence.expect("sampling on");
                self.push(self.now + c, Ev::Sample);
            }
        }
    }

    /// Keep the per-minute series (`bark-sim --series FILE`); needs the
    /// metrics cadence to divide a minute.
    pub fn series_on(&mut self) {
        let mut s = String::new();
        for i in 0..self.n_relays {
            let xy = self.xy(i);
            s.push_str(&format!(
                "{{\"relay\":{i},\"x\":{:.0},\"y\":{:.0},\"site\":\"{:?}\"}}\n",
                xy.x,
                xy.y,
                self.site(i)
            ));
        }
        self.series = Some(s);
    }

    /// The series so far, one JSON object a line: each relay once with
    /// its place, then per minute each relay's counters and load, and the
    /// leaves' counters summed.
    pub fn series(&self) -> Option<&str> {
        self.series.as_deref()
    }

    fn series_rows(&mut self) {
        let Some(mut s) = self.series.take() else {
            return;
        };
        let t = self.now.as_micros() / 1_000_000;
        for (i, r) in self.relays.iter().enumerate() {
            let l = r.load(self.now);
            let m = serde_json::to_string(&self.metrics.nodes[i]).expect("serialisable");
            s.push_str(&format!(
                "{{\"t\":{t},\"n\":{i},\"alive\":{},\"booting\":{},\"busy\":{:.5},\"bucket_us\":{},\"sq\":{},\"dq\":{},\"lv\":{},\"nb\":{},\"m\":{m}}}\n",
                self.nodes[i].alive,
                r.booting(),
                r.busy().fraction(r.home()) as f64 / bark_proto::tables::busy::ONE as f64,
                l.bucket.as_micros(),
                l.send_queue,
                l.delivery_queue,
                l.leaves,
                l.neighbours
            ));
        }
        let now = self.now;
        let replies = &mut self.metrics.addressed.replies;
        self.awaiting.retain(|_, (deadline, leaf)| {
            // An ACK may still come late; give it a second.
            let gone = now > *deadline + Duration::from_secs(1);
            if gone {
                replies[*leaf as usize][3] += 1;
            }
            !gone
        });
        let mut a = crate::metrics::LeafSums::default();
        for (k, leaf) in self.leaves.iter().enumerate() {
            let n = self.n_relays as usize + k;
            a.add(
                &self.metrics.nodes[n],
                leaf.relay().is_some(),
                leaf.holding().is_some(),
                self.offers[n].is_some(),
                self.nodes[n].present,
            );
        }
        let app = &self.metrics.app;
        s.push_str(&format!(
            "{{\"t\":{t},\"leaves\":{},\"addressed\":{},\"originated\":{},\"delivered\":{},\"skipped\":{}}}\n",
            serde_json::to_string(&a).expect("serialisable"),
            serde_json::to_string(&self.metrics.addressed).expect("serialisable"),
            app.originated,
            app.delivered,
            app.skipped
        ));
        self.series = Some(s);
    }

    /// Sample every running relay's bucket, queues and tables.
    fn sample_load(&mut self) {
        use bark_proto::tables::budget::REPLY_RESERVE;
        use bark_proto::tables::neighbours::NEIGHBOURS;
        let full_frame = time_on_air(MAX_FRAME as u8, MIN_PREAMBLE_SYMBOLS);
        for (i, r) in self.relays.iter().enumerate() {
            if !self.nodes[i].alive || r.booting() {
                continue;
            }
            let l = r.load(self.now);
            self.metrics.nodes[i]
                .load
                .get_or_insert_with(Default::default)
                .add(&l, REPLY_RESERVE + full_frame, REPLY_RESERVE, NEIGHBOURS);
            if r.handing_over() {
                if let Some(load) = &mut self.metrics.nodes[i].load {
                    load.handing_over += 1;
                }
            }
        }
    }

    // ---- Core boundary -------------------------------------------------

    /// Hand one event to a core, then perform the actions it emitted, in
    /// order, and re-read its wakeup.
    pub(crate) fn deliver(&mut self, node: u32, ev: Event<'_>) {
        let mut buf = std::mem::take(&mut self.actions);
        buf.clear();
        let now = self.now;
        if let Some(t) = &mut self.tracer {
            t.event(now, node, &ev, &self.msg_tags);
        }
        if node < self.n_relays {
            self.note_attach(node, &ev);
            self.rx_data = match ev {
                Event::Received { bytes, .. } => match bark_proto::wire::Packet::decode(bytes) {
                    Ok(bark_proto::wire::Packet::Data(d)) if d.hops > 0 => {
                        Some((node, d.msg_id().0, d.src_hop.0, d.hops))
                    }
                    _ => None,
                },
                _ => None,
            };
        }
        let wake = if node < self.n_relays {
            let c = &mut self.relays[node as usize];
            self.was_booting = c.booting();
            c.handle(ev, now, &mut |a| buf.push(a));
            c.next_wakeup()
        } else {
            let c = &mut self.leaves[(node - self.n_relays) as usize];
            c.handle(ev, now, &mut |a| buf.push(a));
            let attached = c.relay();
            let wake = c.next_wakeup();
            self.attached(node, attached);
            wake
        };
        for a in buf.drain(..) {
            self.apply(node, a, false);
        }
        self.actions = buf;
        let st = &mut self.nodes[node as usize];
        if st.wake_at != wake {
            st.wake_gen = st.wake_gen.wrapping_add(1);
            st.wake_at = wake;
            if let Some(t) = wake {
                let g = st.wake_gen;
                // Late is allowed, early is not: a past deadline fires now.
                self.push(t.max(now), Ev::Wakeup(node, g));
            }
        }
        // The application asks again after every event (core-interface §4).
        if self.offers[node as usize].is_some() {
            self.offer(node);
        }
    }

    /// A relay receives an ATTACH addressed to it from a leaf that came
    /// back into reach: the network knows the leaf is back.
    fn note_attach(&mut self, relay: u32, ev: &Event<'_>) {
        if self.leaf_nodes.is_empty() {
            return;
        }
        let Event::Received { bytes, .. } = ev else {
            return;
        };
        let Ok(bark_proto::wire::Packet::Attach(a)) = bark_proto::wire::Packet::decode(bytes)
        else {
            return;
        };
        if a.relay != self.relays[relay as usize].config().position.short() {
            return;
        }
        let Some(&leaf) = self.leaf_nodes.get(&a.leaf.0) else {
            return;
        };
        let li = (leaf - self.n_relays) as usize;
        if let Some(t) = self.back_at[li].take() {
            let r = &mut self.metrics.returns;
            r.known += 1;
            r.latencies_us.push(self.now.since(t).as_micros());
        }
    }

    /// Perform one action for `node`. `scripted` actions come from the
    /// scenario rather than the core; their completions go to the log.
    fn apply(&mut self, node: u32, a: Action, scripted: bool) {
        let traced = (!scripted && self.tracer.is_some()).then_some(a);
        match a {
            Action::Listen(c) => self.listen(node, c),
            Action::Cad(c) => self.check(node, ExKind::Cad, c, scripted),
            Action::Rssi(c) => self.check(node, ExKind::Rssi, c, scripted),
            Action::Transmit(f) => {
                self.note_sent_again(node, &f);
                self.begin_excursion(node, ExKind::Transmit, f.channel, Some(f), scripted)
            }
            Action::App(e) => self.app_event(node, e),
            Action::Trace(Trace::Dropped { msg_id, reason }) => {
                self.message_dropped(node, msg_id, reason)
            }
            Action::Trace(t) => {
                let c = &mut self.metrics.nodes[node as usize].core;
                match t {
                    Trace::HomeChannelMoved { .. } if !self.was_booting => c.home_moves += 1,
                    Trace::HeartbeatSkipped { .. } => c.heartbeats_skipped += 1,
                    Trace::HeartbeatDeferred { .. } => c.heartbeats_deferred += 1,
                    Trace::Refused { msg_id, reason } => {
                        c.refused.add(reason);
                        self.note_refusal(node, msg_id, reason);
                    }
                    Trace::TurnedAway { .. } => c.turned_away += 1,
                    Trace::HandOverDeferred { .. } => c.handovers_deferred += 1,
                    Trace::Demoted { .. } => c.demoted += 1,
                    Trace::PollSkipped { .. } => c.polls_skipped += 1,
                    Trace::LeafDropped { .. } => c.leaves_dropped += 1,
                    Trace::Attaching { .. } => c.attaches_sent += 1,
                    Trace::AttachFailed { .. } => c.attaches_failed += 1,
                    Trace::Forwarded { .. } => c.handed_over += 1,
                    Trace::ReturnedToParent { .. } => {
                        c.handed_over += 1;
                        c.returned += 1;
                    }
                    _ => {}
                }
            }
        }
        if let (Some(a), Some(t)) = (traced, &mut self.tracer) {
            t.action(self.now, node, &a, &self.msg_tags);
        }
    }

    /// A relay refused the relay DATA it is being handed, if it is that:
    /// see [`crate::metrics::HopPlaceRefusals`].
    fn note_refusal(&mut self, node: u32, msg_id: MsgId, reason: bark_proto::DropReason) {
        use bark_proto::DropReason as R;
        if !matches!(reason, R::Budget | R::SendQueueFull) {
            return;
        }
        let Some((at, id, from, _)) = self.rx_data else {
            return;
        };
        if at != node || id != msg_id.0 {
            return;
        }
        let Some(sender) = self.relay_named(from) else {
            return;
        };
        let me = self.relays[node as usize].config().position.short();
        let lowest = self.relays[node as usize].lowest_free_hop_place();
        let held = self.relays[sender as usize].highest_count_for(me, self.now);
        let with = matches!((lowest, held), (Some(k), Some(h)) if h >= k);
        let h = &mut self.metrics.hop_places;
        h.refusals += 1;
        h.with_place += u64::from(with);
        self.refused_at.insert((sender, id), (self.now, with));
    }

    /// The relay named `short`, if any.
    fn relay_named(&mut self, short: u32) -> Option<u32> {
        if let Some(&i) = self.by_short.get(&short) {
            return Some(i);
        }
        let i = (0..self.n_relays)
            .find(|&i| self.relays[i as usize].config().position.short().0 == short)?;
        self.by_short.insert(short, i);
        Some(i)
    }

    /// `node` sends `f`: if it is a DATA of a message refused to it, the
    /// wait since ends.
    fn note_sent_again(&mut self, node: u32, f: &Frame) {
        if node >= self.n_relays || self.refused_at.is_empty() {
            return;
        }
        let Ok(bark_proto::wire::Packet::Data(d)) = bark_proto::wire::Packet::decode(f.payload())
        else {
            return;
        };
        if let Some((t, with)) = self.refused_at.remove(&(node, d.msg_id().0)) {
            let w = self.now.since(t).as_micros();
            let h = &mut self.metrics.hop_places;
            h.wait_us += w;
            if with {
                h.wait_with_place_us += w;
            }
        }
    }

    fn listen(&mut self, node: u32, c: Channel) {
        let now = self.now;
        let retune = self.radio.retune();
        let boot = Duration::from_micros(self.radio.stby_to_rx_us);
        let st = &mut self.nodes[node as usize];
        if st.resting == Some(c) {
            return;
        }
        let first = st.resting.is_none();
        st.resting = Some(c);
        if !first {
            self.metrics.nodes[node as usize].home_moves += 1;
        }
        if st.ex.is_none() {
            // Not an excursion, but the synthesiser still takes time to
            // settle; nothing that starts before then can be locked.
            st.listen_from = now + if first { boot } else { retune };
            self.abort_lock(node, Deaf::Retune);
        }
    }

    /// A CAD or RSSI reading. One that would take the node off a packet it
    /// is receiving waits for the packet's end, at most one longest frame
    /// (docs/core-interface.md §3 rule 8; spec "Busy estimate": a reading is taken
    /// only when no packet is in progress). Scripted checks are not held.
    fn check(&mut self, node: u32, kind: ExKind, c: Channel, scripted: bool) {
        self.settle(node);
        let now = self.now;
        let st = &mut self.nodes[node as usize];
        let deaf = !(kind == ExKind::Rssi && st.resting == Some(c));
        if let (false, true, Lock::Rx(r)) = (scripted, deaf, st.lock) {
            if st.alive && st.ex.is_none() && st.held.is_none() {
                let longest = time_on_air(MAX_FRAME as u8, MIN_PREAMBLE_SYMBOLS);
                st.held = Some((kind, c));
                st.held_gen = st.held_gen.wrapping_add(1);
                let g = st.held_gen;
                self.push(r.end.min(now + longest), Ev::Held(node, g));
                return;
            }
        }
        self.begin_excursion(node, kind, c, None, scripted);
    }

    fn begin_excursion(
        &mut self,
        node: u32,
        kind: ExKind,
        c: Channel,
        frame: Option<Frame>,
        scripted: bool,
    ) {
        let now = self.now;
        let label = self.label(node);
        let st = &self.nodes[node as usize];
        if !st.alive {
            return;
        }
        if st.held.is_some() && !scripted {
            panic!(
                "{label}: core started {kind:?} while a held check is outstanding \
                 (docs/core-interface.md §3 rule 1: one excursion outstanding)"
            );
        }
        if let Some(ex) = &st.ex {
            let who = if scripted { "scripted event" } else { "core" };
            panic!(
                "{label}: {who} started {kind:?} on channel {} while a {}{:?} excursion is outstanding \
                 (docs/core-interface.md §3 rule 1: one excursion outstanding)",
                c.index(),
                if ex.scripted { "scripted " } else { "" },
                ex.kind
            );
        }
        if let Some(f) = &frame {
            assert!(
                f.preamble_symbols >= MIN_PREAMBLE_SYMBOLS,
                "{label}: transmit with a {}-symbol preamble, below the {MIN_PREAMBLE_SYMBOLS}-symbol minimum \
                 (docs/core-interface.md §3 rule 4)",
                f.preamble_symbols
            );
            assert!(
                f.len as usize <= MAX_FRAME,
                "{label}: frame longer than MAX_FRAME"
            );
        }
        let off = st.resting != Some(c);
        let phases = match kind {
            ExKind::Cad => self.radio.cad_phases(off),
            ExKind::Rssi => self.radio.rssi_phases(off),
            ExKind::Transmit => {
                let f = frame.as_ref().expect("transmit has a frame");
                self.radio
                    .tx_phases(off, time_on_air(f.len, f.preamble_symbols))
            }
        };
        let mut bounds = [now; 3];
        let mut t = now;
        for (b, p) in bounds.iter_mut().zip(phases) {
            t += p.0;
            *b = t;
        }
        let reasons = phases.map(|p| p.1);
        let mut ex = Excursion {
            kind,
            channel: c,
            bounds,
            reasons,
            scripted,
            cad_seen: false,
            rssi_dbm: None,
            deaf: !(kind == ExKind::Rssi && !off),
            slot: None,
        };
        if ex.deaf {
            let first_reason = ex.reason_at(now);
            self.abort_lock(node, first_reason);
        }
        let st = &mut self.nodes[node as usize];
        st.ex_gen = st.ex_gen.wrapping_add(1);
        let g = st.ex_gen;
        match kind {
            ExKind::Cad => {
                self.metrics.nodes[node as usize].cad += 1;
                // Signals already on the air that overlap the sampling window.
                let sens = self.sens_dbm;
                ex.cad_seen = st.arrivals.iter().any(|a| {
                    a.lora && a.channel == c.index() && a.dbm >= sens && a.end > bounds[0]
                });
            }
            ExKind::Rssi => {
                self.metrics.nodes[node as usize].rssi += 1;
                self.push(bounds[1], Ev::RssiRead(node, g));
            }
            ExKind::Transmit => {
                let f = frame.expect("transmit has a frame");
                let slot = self.alloc_slot(AirTx {
                    src: node,
                    key: 0,
                    gen: 0,
                    frame: f,
                    on_air: false,
                    start: bounds[0],
                    end: bounds[1],
                });
                ex.slot = Some(slot);
                let sg = self.slot_gen[slot as usize];
                self.push(bounds[0], Ev::AirStart(slot, sg));
            }
        }
        self.nodes[node as usize].ex = Some(ex);
        self.push(bounds[2], Ev::ExcursionEnd(node, g));
    }

    fn alloc_slot(&mut self, mut tx: AirTx) -> u32 {
        let slot = match self.free_slots.pop() {
            Some(s) => s,
            None => {
                self.txs.push(None);
                self.slot_gen.push(0);
                (self.txs.len() - 1) as u32
            }
        };
        self.slot_gen[slot as usize] = self.slot_gen[slot as usize].wrapping_add(1);
        tx.gen = self.slot_gen[slot as usize];
        tx.key = self.next_key;
        self.next_key += 1;
        self.txs[slot as usize] = Some(tx);
        slot
    }

    pub(crate) fn free_slot(&mut self, slot: u32) {
        self.txs[slot as usize] = None;
        self.slot_gen[slot as usize] = self.slot_gen[slot as usize].wrapping_add(1);
        self.free_slots.push(slot);
    }

    pub(crate) fn slot_valid(&self, slot: u32, g: u32) -> bool {
        self.slot_gen.get(slot as usize) == Some(&g) && self.txs[slot as usize].is_some()
    }

    fn rssi_read(&mut self, node: u32, g: u32) {
        let st = &self.nodes[node as usize];
        if !st.alive || st.ex_gen != g {
            return;
        }
        let ex = st.ex.as_ref().expect("RSSI read during an RSSI excursion");
        let c = ex.channel.index();
        let mw = self.noise_mw + self.in_channel_mw(node, c, None);
        let dbm = phy::dbm(mw).round() as i16;
        self.nodes[node as usize]
            .ex
            .as_mut()
            .expect("checked")
            .rssi_dbm = Some(dbm);
    }

    fn excursion_end(&mut self, node: u32, g: u32) {
        let st = &mut self.nodes[node as usize];
        if !st.alive || st.ex_gen != g {
            return;
        }
        let ex = st.ex.take().expect("excursion end without excursion");
        // Back on the resting channel; the radio restarts its preamble search.
        if ex.deaf {
            st.lock = Lock::Idle;
        }
        let c = ex.channel;
        let result = match ex.kind {
            ExKind::Cad => {
                let miss = ex.cad_seen && st.cad_rng.chance(self.radio.cad_miss_probability);
                Event::CadDone {
                    channel: c,
                    busy: ex.cad_seen && !miss,
                }
            }
            ExKind::Rssi => Event::RssiSample {
                channel: c,
                dbm: ex.rssi_dbm.expect("RSSI read before end"),
            },
            ExKind::Transmit => Event::TxDone,
        };
        if ex.scripted {
            let at = self.now;
            self.script_log.push(match result {
                Event::CadDone { channel, busy } => ScriptResult::Cad {
                    node,
                    at,
                    channel: channel.index(),
                    busy,
                },
                Event::RssiSample { channel, dbm } => ScriptResult::Rssi {
                    node,
                    at,
                    channel: channel.index(),
                    dbm,
                },
                _ => ScriptResult::TxDone { node, at },
            });
        } else {
            self.deliver(node, result);
        }
    }

    // ---- Application ---------------------------------------------------

    /// Every application event the core can emit is interpreted here and
    /// nowhere else: a message for this node's application.
    fn app_event(&mut self, node: u32, e: AppEvent) {
        let m = &mut self.metrics;
        let AppEvent::Message { len, payload, .. } = e;
        m.nodes[node as usize].messages_in += 1;
        if len < 4 {
            m.app.misdelivered += 1;
            return;
        }
        let tag = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
        match self.origins.get_mut(tag as usize) {
            Some(o) if o.dst == node && o.fate != Fate::Delivered => {
                o.fate = Fate::Delivered;
                o.delivered_at = Some(self.now);
                m.app.delivered += 1;
                m.app.latencies_us.push(self.now.since(o.t).as_micros());
            }
            Some(o) if o.dst == node => m.app.duplicates += 1,
            _ => m.app.misdelivered += 1,
        }
    }

    /// A core dropped message `msg_id` for `reason`, reported in
    /// `Trace::Dropped`. The protocol sends no failure notice, so this is
    /// the only record of where a lost message died.
    pub fn message_dropped(&mut self, node: u32, msg_id: MsgId, reason: DropReason) {
        self.metrics.app.drops.add(reason);
        self.metrics.nodes[node as usize].drops.add(reason);
        if let Some(o) = self
            .msg_tags
            .get(&msg_id.0)
            .and_then(|&t| self.origins.get_mut(t as usize))
        {
            if o.fate != Fate::Delivered {
                o.fate = Fate::Dropped { node, reason };
                o.dropped_at = Some(self.now);
            }
        }
    }

    /// A frame of message `id` went on the air: it carried `hops` (DATA)
    /// and took `air_us`.
    pub(crate) fn message_frame(&mut self, id: MsgId, hops: u8, air_us: u64) {
        if let Some(o) = self
            .msg_tags
            .get(&id.0)
            .and_then(|&t| self.origins.get_mut(t as usize))
        {
            o.frames += 1;
            o.airtime_us += air_us;
            o.hops = o.hops.max(hops);
        }
    }

    /// Whether relay `r`'s neighbour table holds `p`, and how many entries
    /// it has.
    pub(crate) fn table_holds(&self, r: u32, p: Position) -> (bool, usize) {
        let t = self.relays[r as usize].neighbours();
        t.fold((false, 0), |(held, n), e| (held || e.position == p, n + 1))
    }

    /// Fate of every originated message, in origination order.
    pub fn fates(&self) -> impl Iterator<Item = Fate> + '_ {
        self.originated().map(|(_, f)| f)
    }

    /// When each message was originated, and its fate, in origination order.
    pub fn originated(&self) -> impl Iterator<Item = (Instant, Fate)> + '_ {
        self.messages().map(|(t, f, _)| (t, f))
    }

    /// When each message was originated, its fate, and whether its
    /// destination was reachable ([`Sim::reachable`]), in origination
    /// order. A message neither delivered nor reported lost is in flight
    /// if a core still holds it, and unaccounted otherwise.
    pub fn messages(&self) -> impl Iterator<Item = (Instant, Fate, bool)> + '_ {
        let held = self.held_tags();
        self.origins.iter().enumerate().map(move |(tag, o)| {
            let fate = match o.fate {
                Fate::Unaccounted if held.contains(&(tag as u32)) => Fate::InFlight,
                f => f,
            };
            (o.t, fate, o.reachable)
        })
    }

    /// Every originated message, in origination order.
    pub fn message_rows(&self) -> Vec<MessageRow> {
        self.messages()
            .zip(&self.origins)
            .enumerate()
            .map(|(tag, ((t, fate, reachable), o))| MessageRow {
                tag: tag as u32,
                src: o.src,
                dst: o.dst,
                t_s: t.as_micros() as f64 / 1e6,
                fate,
                reachable,
                latency_s: o.delivered_at.map(|d| d.since(t).as_micros() as f64 / 1e6),
                frames: o.frames,
                airtime_s: o.airtime_us as f64 / 1e6,
                hops: o.hops,
                distance_m: self.xy[o.src as usize].dist(self.xy[o.dst as usize]),
                taken_s: o.taken_at.map(|d| d.since(t).as_micros() as f64 / 1e6),
                dropped_s: o.dropped_at.map(|d| d.since(t).as_micros() as f64 / 1e6),
            })
            .collect()
    }

    /// Links between relays as `a,b,dbm`: the base receive power at `b`
    /// when `a` transmits at the configured power, before fading.
    pub fn relay_links_csv(&self) -> String {
        let mut s = String::from("a,b,dbm\n");
        for a in 0..self.n_relays {
            for l in self.links.row(a) {
                if l.rx < self.n_relays {
                    s.push_str(&format!("{},{},{:.2}\n", a, l.rx, l.base_dbm));
                }
            }
        }
        s
    }

    /// The per-message table as CSV.
    pub fn messages_csv(&self) -> String {
        let mut s = String::from(
            "tag,src,dst,t_s,fate,drop_node,drop_reason,reachable,latency_s,frames,airtime_s,hops,distance_m,taken_s,dropped_s\n",
        );
        for r in self.message_rows() {
            let (fate, node, reason) = match r.fate {
                Fate::Delivered => ("delivered", String::new(), String::new()),
                Fate::InFlight => ("in_flight", String::new(), String::new()),
                Fate::Unaccounted => ("unaccounted", String::new(), String::new()),
                Fate::Dropped { node, reason } => {
                    ("dropped", node.to_string(), format!("{reason:?}"))
                }
            };
            let latency = r.latency_s.map(|l| format!("{l:.6}")).unwrap_or_default();
            let taken = r.taken_s.map(|l| format!("{l:.6}")).unwrap_or_default();
            let dropped = r.dropped_s.map(|l| format!("{l:.6}")).unwrap_or_default();
            s.push_str(&format!(
                "{},{},{},{:.6},{fate},{node},{reason},{},{latency},{},{:.6},{},{:.1},{taken},{dropped}\n",
                r.tag,
                r.src,
                r.dst,
                r.t_s,
                r.reachable,
                r.frames,
                r.airtime_s,
                r.hops,
                r.distance_m
            ));
        }
        s
    }

    /// Position on the local grid.
    pub fn xy(&self, node: u32) -> Xy {
        self.xy[node as usize]
    }

    /// Where a node's antenna is.
    pub fn site(&self, node: u32) -> Site {
        self.sites[node as usize]
    }

    /// The relay a leaf is attached to now, as a node index.
    pub fn attached_relay(&self, leaf: u32) -> Option<u32> {
        let p = self.leaves[(leaf - self.n_relays) as usize].relay()?;
        self.relay_index.get(&p.short().0).copied()
    }

    /// Traffic tags of the messages some live core still holds, or its
    /// application still waits to hand it.
    fn held_tags(&self) -> std::collections::HashSet<u32> {
        let alive = |i: usize| self.nodes[i].alive;
        let relays = self
            .relays
            .iter()
            .enumerate()
            .filter(|(i, _)| alive(*i))
            .flat_map(|(_, r)| r.holding());
        let leaves = self
            .leaves
            .iter()
            .enumerate()
            .filter(|(i, _)| alive(self.n_relays as usize + i))
            .filter_map(|(_, l)| l.holding());
        let waiting = self
            .offers
            .iter()
            .enumerate()
            .filter(|(i, _)| alive(*i))
            .filter_map(|(_, o)| o.as_ref().map(|o| o.tag));
        relays
            .chain(leaves)
            .filter_map(|id| self.msg_tags.get(&id.0).copied())
            .chain(waiting)
            .collect()
    }

    /// Per relay, a label of its connected component: relays joined by
    /// links that both ends hear above sensitivity without fading.
    fn components(&self) -> Vec<u32> {
        let n = self.n_relays as usize;
        let mut comp: Vec<u32> = (0..n as u32).collect();
        fn root(comp: &mut [u32], mut i: u32) -> u32 {
            while comp[i as usize] != i {
                comp[i as usize] = comp[comp[i as usize] as usize];
                i = comp[i as usize];
            }
            i
        }
        for a in 0..self.n_relays {
            for l in self.links.row(a) {
                if l.rx < self.n_relays && self.hears_both(a, l.rx) {
                    let (ra, rb) = (root(&mut comp, a), root(&mut comp, l.rx));
                    comp[ra.max(rb) as usize] = ra.min(rb);
                }
            }
        }
        (0..n as u32).map(|i| root(&mut comp, i)).collect()
    }

    /// Whether `a` and `b` hear each other above sensitivity, before fading.
    fn hears_both(&self, a: u32, b: u32) -> bool {
        let up = |x, y| {
            self.links
                .get(x, y)
                .is_some_and(|l| l.base_dbm as f64 >= self.sens_dbm)
        };
        up(a, b) && up(b, a)
    }

    /// Whether a message from `src` to the relay at `dst` could arrive,
    /// judged from the links between relays at the start of the run: the
    /// origin relay, `src` itself or the relay the leaf `src` is attached
    /// to, is joined to the relay at `dst` by relays that hear each other.
    /// A message to a relay that no route reaches is lost whatever the
    /// protocol does, and is counted apart. A leaf without a relay, or an
    /// address no relay has, counts as reachable: that loss is the
    /// protocol's.
    pub fn reachable(&self, src: u32, dst: Position) -> bool {
        let relay_at = |p: Position| {
            (0..self.n_relays).find(|&r| self.relays[r as usize].config().position == p)
        };
        let from = if src < self.n_relays {
            Some(src)
        } else {
            self.leaves[(src - self.n_relays) as usize]
                .relay()
                .and_then(relay_at)
        };
        match (from, relay_at(dst)) {
            (Some(a), Some(b)) => self.component[a as usize] == self.component[b as usize],
            _ => true,
        }
    }

    /// Leaf `node` is attached to `relay` after an event: keep its address
    /// and count a change of relay.
    fn attached(&mut self, node: u32, relay: Option<Position>) {
        let Some(p) = relay else { return };
        let last = &mut self.address[node as usize];
        if *last != Some(p) {
            if last.is_some() {
                self.metrics.nodes[node as usize].relay_changes += 1;
            }
            *last = Some(p);
        }
    }

    /// The address of `node` as a sender knows it (spec "Addresses"): a
    /// relay's own position; for a leaf, the relay it is attached to, or
    /// the one it was last attached to, as the sender would have learnt
    /// it from an earlier message. `None` for a leaf that never attached:
    /// nobody can have its address.
    fn address_of(&mut self, node: u32) -> Option<Position> {
        if node < self.n_relays {
            return Some(self.positions[node as usize]);
        }
        let leaf = &self.leaves[(node - self.n_relays) as usize];
        if let Some(p) = leaf.relay() {
            self.address[node as usize] = Some(p);
        }
        self.address[node as usize]
    }

    /// Leaf id of a node: the leaf core's own, `LeafId::RELAY` for a relay.
    pub fn leaf_id(&self, node: u32) -> LeafId {
        if node < self.n_relays {
            LeafId::RELAY
        } else {
            self.leaves[(node - self.n_relays) as usize].id()
        }
    }

    /// The traffic model has a message from `src` to `dst`. Its
    /// application holds one at a time and hands it to the core the moment
    /// the core takes one (core-interface §4); a message due while the last
    /// still waits is skipped. Latency counts from here, so a busy network
    /// shows as latency.
    fn originate(&mut self, src: u32, dst: u32, len: u8) {
        let st = &self.nodes[src as usize];
        if !st.alive || !st.present || self.n_relays == 0 {
            return;
        }
        if self.offers[src as usize].is_some() {
            self.metrics.app.skipped += 1;
            return;
        }
        let Some(dst_pos) = self.address_of(dst) else {
            self.metrics.app.unaddressed += 1;
            return;
        };
        let tag = self.origins.len() as u32;
        let reachable = self.reachable(src, dst_pos);
        self.origins.push(OriginRecord {
            src,
            dst,
            t: self.now,
            fate: Fate::Unaccounted,
            reachable,
            delivered_at: None,
            taken_at: None,
            dropped_at: None,
            frames: 0,
            airtime_us: 0,
            hops: 0,
        });
        self.metrics.app.originated += 1;
        self.metrics.nodes[src as usize].originated += 1;
        self.offers[src as usize] = Some(Offer {
            tag,
            dst_pos,
            dst_leaf: self.leaf_id(dst),
            len,
        });
        self.offer(src);
    }

    /// How long each message its application still waits to hand over
    /// has waited so far.
    pub(crate) fn waiting(&self) -> impl Iterator<Item = Duration> + '_ {
        self.offers
            .iter()
            .flatten()
            .map(|o| self.now.since(self.origins[o.tag as usize].t))
    }

    /// Hand `src`'s waiting message to its core, if the core takes one now.
    fn offer(&mut self, src: u32) {
        let now = self.now;
        let ready = self.nodes[src as usize].alive
            && if src < self.n_relays {
                self.relays[src as usize].accepts_send(now)
            } else {
                self.leaves[(src - self.n_relays) as usize].accepts_send(now)
            };
        if !ready {
            return;
        }
        let Some(o) = self.offers[src as usize].take() else {
            return;
        };
        self.origins[o.tag as usize].taken_at = Some(now);
        let wait = now.since(self.origins[o.tag as usize].t);
        if wait > Duration::ZERO {
            let app = &mut self.metrics.app;
            app.waited += 1;
            app.longest_wait_us = app.longest_wait_us.max(wait.as_micros());
        }
        let mut payload = [0u8; MAX_FRAME];
        payload[..4].copy_from_slice(&o.tag.to_le_bytes());
        let payload = &payload[..o.len as usize];
        // The ID the core gives it (core-interface §7), to follow its
        // traces and drops.
        let id = MsgId::of(o.dst_pos, o.dst_leaf, payload);
        self.msg_tags.insert(id.0, o.tag);
        let cmd = AppCommand::Send {
            dst_pos: o.dst_pos,
            dst_leaf: o.dst_leaf,
            payload,
        };
        self.deliver(src, Event::App(cmd));
    }

    fn poisson_arrival(&mut self, n: u32) {
        let li = (n - self.n_relays) as usize;
        let p = self.poisson.as_ref().expect("poisson configured");
        let (mean, min, max) = (p.mean_gap_s, p.min, p.max);
        let dest = p.dest.clone();
        let leaves = self.leaf_xy.len() as u64;
        let r = &mut self.poisson_rng[li];
        let len = r.range(min as u64, max as u64) as u8;
        let dst = match dest {
            Destination::Uniform if leaves > 1 => {
                let k = r.range(0, leaves - 2);
                Some(if k >= li as u64 { k + 1 } else { k })
            }
            Destination::Within { radius_m } => {
                let grid = self.leaf_grid.as_ref().expect("grid for within");
                let me = self.leaf_xy[li];
                let mut cands = Vec::new();
                grid.neighbourhood(me, |j| {
                    if j as usize != li && self.leaf_xy[j as usize].dist(me) <= radius_m {
                        cands.push(j);
                    }
                });
                cands.sort_unstable();
                (!cands.is_empty())
                    .then(|| cands[r.range(0, cands.len() as u64 - 1) as usize] as u64)
            }
            _ => None,
        };
        let gap = secs(r.exponential(mean));
        if let Some(d) = dst {
            self.originate(n, self.n_relays + d as u32, len);
        }
        self.push(self.now + gap, Ev::Poisson(n));
    }

    // ---- Scripted events -------------------------------------------------

    fn run_script(&mut self, k: u32) {
        let e = &self.script[k as usize];
        let (node, action, channel, len, preamble) =
            (e.node, e.action, e.channel, e.len, e.preamble);
        match action {
            ScriptAction::Remove => self.remove(node),
            ScriptAction::Restart => self.restart(node),
            ScriptAction::Listen => self.listen(node, channel.expect("validated")),
            ScriptAction::Cad => self.apply(node, Action::Cad(channel.expect("validated")), true),
            ScriptAction::Rssi => self.apply(node, Action::Rssi(channel.expect("validated")), true),
            ScriptAction::Transmit => {
                let mut bytes = [0u8; MAX_FRAME];
                bytes[..4].copy_from_slice(&k.to_le_bytes());
                let f = Frame {
                    channel: channel.expect("validated"),
                    preamble_symbols: preamble,
                    len,
                    bytes,
                };
                self.apply(node, Action::Transmit(f), true);
            }
        }
    }

    /// Power a node off for good: its excursion, transmission, reception
    /// and timers end now.
    pub fn remove(&mut self, node: u32) {
        self.power_off(node, "removed");
    }

    /// Power a node off and on again: it boots afresh with every table
    /// lost and its identity kept, a relay's position or a leaf's ID. Its
    /// new core draws from a new random stream, as a board's hardware
    /// generator would after a reboot.
    pub fn restart(&mut self, node: u32) {
        if !self.nodes[node as usize].alive {
            return;
        }
        self.power_off(node, "restarted");
        let st = &mut self.nodes[node as usize];
        st.alive = true;
        st.restarts += 1;
        let index = (st.restarts as u64) << 32 | node as u64;
        let seed = stream_seed(self.seed, Purpose::CoreSeed, index);
        if node < self.n_relays {
            let r = &mut self.relays[node as usize];
            *r = Relay::new(*r.config(), seed);
        } else {
            let l = &mut self.leaves[(node - self.n_relays) as usize];
            let config = Config {
                leaf_id: l.id(),
                ..*l.config()
            };
            *l = Leaf::new(config, seed);
        }
        // Power-on is delivered as a wakeup (docs/core-interface.md §2).
        self.deliver(node, Event::Wakeup);
    }

    fn power_off(&mut self, node: u32, why: &str) {
        if !self.nodes[node as usize].alive {
            return;
        }
        if let Some(t) = &mut self.tracer {
            t.note(self.now, node, why);
        }
        self.abort_lock(node, Deaf::Absent);
        self.clear_arrivals(node);
        let st = &mut self.nodes[node as usize];
        st.alive = false;
        st.resting = None;
        st.wake_gen = st.wake_gen.wrapping_add(1);
        st.wake_at = None;
        st.ex_gen = st.ex_gen.wrapping_add(1);
        st.held = None;
        st.held_gen = st.held_gen.wrapping_add(1);
        if let Some(ex) = st.ex.take() {
            if let Some(slot) = ex.slot {
                let tx = self.txs[slot as usize]
                    .as_ref()
                    .expect("slot of live excursion");
                if tx.on_air {
                    let g = tx.gen;
                    self.air_end(slot, g, true);
                } else {
                    self.free_slot(slot);
                }
            }
        }
    }

    // ---- Interferers and availability -------------------------------------

    fn interferer_toggle(&mut self, k: u32, g: u32) {
        let now = self.now;
        let i = &mut self.interferers[k as usize];
        if i.gen != g {
            return;
        }
        i.gen = i.gen.wrapping_add(1);
        let g = i.gen;
        if i.on {
            i.on = false;
            let key = i.key;
            let off = secs(i.rng.exponential(i.mean_off_s));
            let reach = std::mem::take(&mut i.reach);
            for &(n, _) in &reach {
                self.remove_arrival(n, key);
            }
            self.interferers[k as usize].reach = reach;
            self.push(now + off, Ev::Interferer(k, g));
        } else {
            i.on = true;
            i.key = (1u64 << 63) | self.next_key;
            self.next_key += 1;
            let always = i.mean_off_s <= 0.0;
            i.end = if always {
                Instant::from_micros(u64::MAX)
            } else {
                now + secs(i.rng.exponential(i.mean_on_s))
            };
            let (key, end, ch, lora) = (i.key, i.end, i.channel, i.lora);
            let reach = std::mem::take(&mut i.reach);
            for &(n, dbm) in &reach {
                let dbm = dbm as f64;
                self.arrive(
                    n,
                    Arrival {
                        key,
                        channel: ch,
                        dbm,
                        mw: phy::mw(dbm),
                        end,
                        lora,
                    },
                    None,
                );
            }
            self.interferers[k as usize].reach = reach;
            if !always {
                self.push(end, Ev::Interferer(k, g));
            }
        }
    }

    fn availability_toggle(&mut self, n: u32) {
        let (mean_up, mean_dark) = self.availability.expect("availability configured");
        let li = (n - self.n_relays) as usize;
        if !self.nodes[n as usize].alive {
            return;
        }
        let present = self.nodes[n as usize].present;
        if let Some(t) = &mut self.tracer {
            t.note(self.now, n, if present { "dark" } else { "back" });
        }
        let next = if present {
            // Going dark: out of reach of everything now on the air.
            self.abort_lock(n, Deaf::Absent);
            self.clear_arrivals(n);
            self.nodes[n as usize].present = false;
            self.back_at[li] = None;
            self.avail_rng[li].exponential(mean_dark)
        } else {
            // Back in reach. Signals already on the air are not added: the
            // error is at most one packet's worth of interference.
            self.nodes[n as usize].present = true;
            self.back_at[li] = Some(self.now);
            self.metrics.returns.returned += 1;
            self.avail_rng[li].exponential(mean_up)
        };
        self.push(self.now + secs(next), Ev::Availability(n));
    }

    // ---- Helpers for the reception model --------------------------------

    /// Abort a reception in progress at `node`, recording why.
    pub(crate) fn abort_lock(&mut self, node: u32, why: Deaf) {
        self.settle(node);
        let st = &mut self.nodes[node as usize];
        if let Lock::Rx(r) = st.lock {
            self.metrics.outcome(node, r.channel, Outcome::Deaf(why));
        }
        st.lock = Lock::Idle;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drops are attributed to the node and reason the core reports, and a
    /// delivery of another copy overrides them.
    #[test]
    fn drops_are_attributed_to_node_and_reason() {
        let sc = Scenario::new("drops", 1, 10.0)
            .point("A", RoleSpec::Relay, 0.0, 0.0)
            .point("B", RoleSpec::Relay, 400.0, 0.0)
            .point("L", RoleSpec::Leaf, 450.0, 0.0)
            .link("A", "B", -100.0);
        let mut sim = Sim::new(&sc).unwrap();
        // Two messages from A to L, with msg ids 7 and 8.
        for id in [7, 8] {
            let tag = sim.origins.len() as u32;
            sim.origins.push(OriginRecord {
                src: 0,
                dst: 2,
                t: Instant::ZERO,
                fate: Fate::Unaccounted,
                reachable: true,
                delivered_at: None,
                taken_at: None,
                dropped_at: None,
                frames: 0,
                airtime_us: 0,
                hops: 0,
            });
            sim.msg_tags.insert(id, tag);
        }
        sim.message_dropped(1, MsgId(7), DropReason::NoRoute);
        sim.message_dropped(1, MsgId(8), DropReason::QueueExpired);
        let mut payload = [0u8; MAX_FRAME];
        payload[..4].copy_from_slice(&1u32.to_le_bytes());
        let msg = AppEvent::Message {
            msg_id: MsgId(8),
            len: 4,
            payload,
        };
        sim.app_event(2, msg);
        let fates: Vec<Fate> = sim.fates().collect();
        let no_route = Fate::Dropped {
            node: 1,
            reason: DropReason::NoRoute,
        };
        assert_eq!(fates, vec![no_route, Fate::Delivered]);
        assert_eq!(sim.metrics.nodes[1].drops.total(), 2);
        let s = sim.summary();
        assert_eq!(
            (
                s.app.lost_dropped.no_route,
                s.app.lost_dropped.queue_expired
            ),
            (1, 0)
        );
        assert_eq!(
            (s.app.drops.total(), s.app.delivered, s.app.unaccounted),
            (2, 1, 0)
        );
    }
}
