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
use crate::links::{LinkTable, NodeRadio};
use crate::metrics::{DropReason, Metrics, Observer, Outcome};
use crate::phy::{self, Deaf, RadioParams};
use crate::rng::{stream_seed, Purpose, Stream};
use crate::scenario::{
    resolve, Destination, PlacedNode, RoleSpec, Scenario, ScriptAction, TrafficSpec,
};
use bark_proto::radio::{time_on_air, MIN_PREAMBLE_SYMBOLS};
use bark_proto::{
    Action, AppCommand, AppEvent, Channel, Config, Core, Duration, Event, Frame, Instant, Leaf,
    LeafId, MsgId, Position, Relay, MAX_FRAME,
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
    pub end: Instant,
    /// Worst instantaneous interference so far, mW.
    pub max_i_mw: f64,
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
    /// Neither delivered nor reported lost (in flight at the end, or lost
    /// without a report).
    Unaccounted,
    /// Reached its destination's application.
    Delivered,
    /// Failed locally at its origin.
    Failed,
    /// Dropped by a relay; the last drop reported wins.
    Dropped {
        /// Where.
        node: u32,
        /// Why.
        reason: DropReason,
    },
}

struct OriginRecord {
    dst: u32,
    t: Instant,
    fate: Fate,
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
    home_relay: Vec<u32>,
    availability: Option<(f64, f64)>,
    avail_rng: Vec<Stream>,
    origins: Vec<OriginRecord>,
    pending_tag: Option<u32>,
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
}

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
        let indoor: Vec<bool> = placed.iter().map(|p| p.indoor).collect();
        let links = LinkTable::build(
            &sc.links,
            &NodeRadio {
                xy: &xy,
                tx_dbm: &tx_dbm,
                indoor: &indoor,
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
            radio,
            sens_dbm,
            noise_mw: phy::mw(noise_dbm),
            rej,
            busy_signal_mw,
            links,
            names,
            xy,
            positions,
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
            home_relay: vec![u32::MAX; n],
            availability: None,
            avail_rng: Vec::new(),
            origins: Vec::new(),
            pending_tag: None,
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
        };
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
        for o in &sc.metrics.observers {
            let i = resolve(placed, o)?;
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
            }
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

    /// Run to the scenario end.
    pub fn run(&mut self) {
        self.run_until(self.end);
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
            Ev::Sample => {
                self.metrics.sample(self.now.as_micros());
                let c = self.cadence.expect("sampling on");
                self.push(self.now + c, Ev::Sample);
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
        let wake = if node < self.n_relays {
            let c = &mut self.relays[node as usize];
            c.handle(ev, now, &mut |a| buf.push(a));
            c.next_wakeup()
        } else {
            let c = &mut self.leaves[(node - self.n_relays) as usize];
            c.handle(ev, now, &mut |a| buf.push(a));
            c.next_wakeup()
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
    }

    /// Perform one action for `node`. `scripted` actions come from the
    /// scenario rather than the core; their completions go to the log.
    fn apply(&mut self, node: u32, a: Action, scripted: bool) {
        match a {
            Action::Listen(c) => self.listen(node, c),
            Action::Cad(c) => self.begin_excursion(node, ExKind::Cad, c, None, scripted),
            Action::Rssi(c) => self.begin_excursion(node, ExKind::Rssi, c, None, scripted),
            Action::Transmit(f) => {
                self.begin_excursion(node, ExKind::Transmit, f.channel, Some(f), scripted)
            }
            Action::App(e) => self.app_event(node, e),
            Action::Trace(_) => {}
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
            slot: None,
        };
        let first_reason = ex.reason_at(now);
        self.abort_lock(node, first_reason);
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
        st.lock = Lock::Idle;
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
    /// nowhere else.
    fn app_event(&mut self, node: u32, e: AppEvent) {
        let m = &mut self.metrics;
        match e {
            AppEvent::Accepted(MsgId(id)) => {
                m.app.accepted += 1;
                if let Some(tag) = self.pending_tag {
                    self.msg_tags.insert(id, tag);
                }
            }
            // The first hop acknowledged the message.
            AppEvent::Sent(MsgId(_)) => {
                m.app.sent += 1;
                m.nodes[node as usize].sent += 1;
            }
            // A local failure at the origin.
            AppEvent::Failed { msg_id, reason } => {
                *m.app.failed.entry(format!("{reason:?}")).or_insert(0) += 1;
                m.nodes[node as usize].failed += 1;
                if let Some(o) = self
                    .msg_tags
                    .get(&msg_id.0)
                    .and_then(|&t| self.origins.get_mut(t as usize))
                {
                    if o.fate == Fate::Unaccounted {
                        o.fate = Fate::Failed;
                    }
                }
            }
            AppEvent::Message { len, payload, .. } => {
                m.nodes[node as usize].messages_in += 1;
                if len < 4 {
                    m.app.misdelivered += 1;
                    return;
                }
                let tag = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
                match self.origins.get_mut(tag as usize) {
                    Some(o) if o.dst == node && o.fate != Fate::Delivered => {
                        o.fate = Fate::Delivered;
                        m.app.delivered += 1;
                        m.app.latencies_us.push(self.now.since(o.t).as_micros());
                    }
                    Some(o) if o.dst == node => m.app.duplicates += 1,
                    _ => m.app.misdelivered += 1,
                }
            }
        }
    }

    /// A core dropped message `msg_id` for `reason`. The protocol sends no
    /// failure notice, so this is the only record of where a lost message
    /// died. The core interface does not report drops yet; when it does,
    /// its report is translated into this call in [`Sim::app_event`].
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
            }
        }
    }

    /// Fate of every originated message, in origination order.
    pub fn fates(&self) -> impl Iterator<Item = Fate> + '_ {
        self.origins.iter().map(|o| o.fate)
    }

    /// The relay a destination is reached through: itself if a relay;
    /// for a leaf, the relay with the strongest link to it, else the
    /// nearest relay. The sender's application is assumed to know it
    /// (design §9.1: addresses are relay position plus leaf id).
    fn relay_of(&mut self, node: u32) -> u32 {
        if node < self.n_relays {
            return node;
        }
        if self.home_relay[node as usize] == u32::MAX {
            let best = self
                .links
                .row(node)
                .iter()
                .filter(|l| l.rx < self.n_relays)
                .max_by(|a, b| a.base_dbm.total_cmp(&b.base_dbm).then(b.rx.cmp(&a.rx)))
                .map(|l| l.rx);
            let r = best.or_else(|| {
                let p = self.xy[node as usize];
                (0..self.n_relays).min_by(|&a, &b| {
                    self.xy[a as usize]
                        .dist(p)
                        .total_cmp(&self.xy[b as usize].dist(p))
                })
            });
            self.home_relay[node as usize] = r.unwrap_or(0);
        }
        self.home_relay[node as usize]
    }

    /// Leaf id of a node: leaves are numbered from 1 (0 is the relay itself).
    pub fn leaf_id(&self, node: u32) -> LeafId {
        if node < self.n_relays {
            LeafId::RELAY
        } else {
            LeafId((node - self.n_relays + 1).to_be_bytes())
        }
    }

    fn originate(&mut self, src: u32, dst: u32, len: u8) {
        let st = &self.nodes[src as usize];
        if !st.alive || !st.present || self.n_relays == 0 {
            return;
        }
        let tag = self.origins.len() as u32;
        self.origins.push(OriginRecord {
            dst,
            t: self.now,
            fate: Fate::Unaccounted,
        });
        let mut payload = [0u8; MAX_FRAME];
        payload[..4].copy_from_slice(&tag.to_le_bytes());
        let relay = self.relay_of(dst);
        let cmd = AppCommand::Send {
            dst_pos: self.positions[relay as usize],
            dst_leaf: self.leaf_id(dst),
            payload: &payload[..len as usize],
        };
        self.metrics.app.originated += 1;
        self.metrics.nodes[src as usize].originated += 1;
        self.pending_tag = Some(tag);
        self.deliver(src, Event::App(cmd));
        self.pending_tag = None;
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
        if !self.nodes[node as usize].alive {
            return;
        }
        self.abort_lock(node, Deaf::Absent);
        self.clear_arrivals(node);
        let st = &mut self.nodes[node as usize];
        st.alive = false;
        st.resting = None;
        st.wake_gen = st.wake_gen.wrapping_add(1);
        st.wake_at = None;
        st.ex_gen = st.ex_gen.wrapping_add(1);
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
        let next = if present {
            // Going dark: out of reach of everything now on the air.
            self.abort_lock(n, Deaf::Absent);
            self.clear_arrivals(n);
            self.nodes[n as usize].present = false;
            self.avail_rng[li].exponential(mean_dark)
        } else {
            // Back in reach. Signals already on the air are not added: the
            // error is at most one packet's worth of interference.
            self.nodes[n as usize].present = true;
            self.avail_rng[li].exponential(mean_up)
        };
        self.push(self.now + secs(next), Ev::Availability(n));
    }

    // ---- Helpers for the reception model --------------------------------

    /// Abort a reception in progress at `node`, recording why.
    pub(crate) fn abort_lock(&mut self, node: u32, why: Deaf) {
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
        // Two messages from A to L, accepted as msg ids 7 and 8.
        for id in [7, 8] {
            let tag = sim.origins.len() as u32;
            sim.origins.push(OriginRecord {
                dst: 2,
                t: Instant::ZERO,
                fate: Fate::Unaccounted,
            });
            sim.pending_tag = Some(tag);
            sim.app_event(0, AppEvent::Accepted(MsgId(id)));
        }
        sim.pending_tag = None;
        sim.message_dropped(1, MsgId(7), DropReason::NoRoute);
        sim.message_dropped(1, MsgId(8), DropReason::QueueTtl);
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
            (s.app.lost_dropped.no_route, s.app.lost_dropped.queue_ttl),
            (1, 0)
        );
        assert_eq!(
            (s.app.drops.total(), s.app.delivered, s.app.unaccounted),
            (2, 1, 0)
        );
    }
}
