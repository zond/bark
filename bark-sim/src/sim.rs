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
use crate::metrics::{DropReason, FirstTry, Metrics, Observer, Outcome, AFTER_BEAT, AFTER_TAKE};
use crate::phy::{self, Deaf, ErrorCurve, Loss, RadioParams};
use crate::queue::{self, Events, Wakeups};
use crate::rng::{stream_seed, Purpose, Stream};
use crate::scenario::{
    resolve, Destination, Ids, PairKind, PairReach, PlacedNode, RoleSpec, Scenario, ScriptAction,
    SharedIds, TrafficSpec,
};
use crate::trace::{Names, Tracer};
use crate::walk::{MovingLink, Walks};
use bark_proto::radio::{time_on_air, MIN_PREAMBLE_SYMBOLS, SYMBOL_TIME};
use bark_proto::sniff::{DETECT_SYMBOLS, LONG_PREAMBLE_SYMBOLS};
use bark_proto::{
    Action, AppCommand, AppEvent, Channel, Config, Core, Duration, Event, Frame, Instant, MsgId,
    NodeId, Position, Relay, Trace, MAX_FRAME,
};
use std::collections::HashMap;

/// Something scheduled. Indices are node, slot, flow or script indices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ev {
    /// Power-on of a node.
    Boot(u32),
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
    /// An interferer that is on reaches the walkers where they are now
    /// (interferer, generation).
    InterfererMoves(u32, u32),
    /// Leaf availability toggles.
    Availability(u32),
    /// Scripted event.
    Script(u32),
    /// A check held during a reception starts (node, hold generation).
    Held(u32, u32),
    /// Time-series sample.
    Sample,
    /// A walker ends its stop or its walk (walker index).
    Leg(u32),
    /// A walking node takes a GPS fix (node).
    Fix(u32),
    /// A group of sleepy nodes goes dark, or comes back (group, dark).
    Group(u32, bool),
    /// A relay with a GPS fault takes a fix (node).
    Gps(u32),
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
    /// When a duty-cycled receiver detected its preamble, if it was one.
    pub caught: Option<Instant>,
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
    /// Dark by its own availability process, and in how many groups that
    /// are dark (`scenario::DarkGroup`): present only when neither.
    pub avail_dark: bool,
    pub group_dark: u32,
    pub resting: Option<Channel>,
    /// After a `Listen` retune, the radio listens from here.
    pub listen_from: Instant,
    pub ex: Option<Excursion>,
    pub ex_gen: u32,
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
    /// The last heartbeats to reach this node, for [`crate::metrics::Addressed`].
    pub beats: BeatRing,
    /// The radio receives duty-cycled (`Action::Sniff`): its cycle began
    /// then, with a receive window, and it ends then.
    pub sniff: Option<(Instant, Instant)>,
    /// Since when it has, for the count of that time.
    pub sniff_since: Instant,
}

impl NodeState {
    /// When the duty cycle of a radio receiving duty-cycled at `t` began.
    pub(crate) fn sniffing_at(&self, t: Instant) -> Option<Instant> {
        self.sniff.filter(|&(_, u)| t < u).map(|(t0, _)| t0)
    }
}

/// The last few heartbeats to reach a node: when each was on the air, its
/// channel, and whether a sleepy node sent it. A frame lasts at most a full
/// frame's 139 ms and a heartbeat 11.84 ms, so eight cover what a frame can
/// overlap at any heartbeat rate a node hears.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct BeatRing {
    beats: [(Instant, Instant, u8, bool); 8],
    next: usize,
}

impl BeatRing {
    /// A heartbeat reached the node.
    pub(crate) fn push(&mut self, start: Instant, end: Instant, channel: u8, sleepy: bool) {
        self.beats[self.next] = (start, end, channel, sleepy);
        self.next = (self.next + 1) % self.beats.len();
    }

    /// How many heartbeats, sleepy or not, were on the air on `channel`
    /// during `[start, end)`.
    pub(crate) fn count(&self, start: Instant, end: Instant, channel: u8) -> usize {
        self.beats
            .iter()
            .filter(|&&(s, e, c, _)| c == channel && s < end && e > start)
            .count()
    }

    /// Whether a heartbeat, a sleepy node's or another's, was on the air
    /// on `channel` during `[start, end)`.
    pub(crate) fn during(&self, start: Instant, end: Instant, channel: u8, sleepy: bool) -> bool {
        self.beats
            .iter()
            .any(|&(s, e, c, z)| c == channel && z == sleepy && s < end && e > start)
    }
}

/// A DATA on the air to a sleepy node (`Sim::to_sleepy`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct ToSleepy {
    /// The node.
    pub node: u32,
    /// It could not hear the frame when it started: it received
    /// duty-cycled and the frame's preamble was short.
    pub deaf: bool,
    /// It was dark or removed then.
    pub absent: bool,
    /// The sender, its holder.
    pub holder: u32,
    /// The message.
    pub msg: u32,
    /// The window it went in (`Sim::window_of`).
    pub window: usize,
    /// Why it was lost at the node, if it was.
    pub lost: Option<Outcome>,
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
    /// Its receivers over links with a walker at either end, from when it
    /// reached the air.
    pub extra: Vec<MovingLink>,
}

pub(crate) struct Interferer {
    pub channel: u8,
    pub lora: bool,
    pub mean_on_s: f64,
    pub mean_off_s: f64,
    /// Where it is and how strong, for the walkers it reaches.
    pub at: Xy,
    pub power_dbm: f64,
    /// The nodes that do not walk it reaches, and the power at each.
    pub reach: Vec<(u32, f32)>,
    /// The walkers it reaches while it is on, as of its last look.
    pub moving: Vec<(u32, f32)>,
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
    /// When a node other than its origin first sent a DATA of it: when it
    /// entered the network past its first hop's admission.
    entered_at: Option<Instant>,
    /// The time left the last DATA to its destination node carried: what
    /// that node's application learns of the message's way (spec
    /// "Receiving DATA").
    time_left: Option<Duration>,
    /// The node that queued it for its destination node
    /// (spec "Delivery queue"), the beats of that node it heard while the
    /// message was queued, and the DATA it sent it.
    holder: Option<u32>,
    holder_beats: u32,
    holder_tries: u32,
    /// What became of its destination node's beats at the holder while
    /// it was queued.
    watch: HolderWatch,
    /// Frames that carried it or answered it (DATA and their ACKs),
    /// and their time on air, µs.
    frames: u32,
    airtime_us: u64,
    /// The largest hand-over count a DATA frame of it carried (spec "Hop count").
    hops: u8,
    /// How far apart its origin and destination were when it was sent,
    /// and whether its destination was walking then.
    distance_m: f64,
    dst_walking: bool,
    /// The relays whose positions its origin's and its destination's
    /// addresses were when it was sent, if any.
    src_relay: Option<u32>,
    dst_relay: Option<u32>,
    /// It reached the application of a node with its destination's ID
    /// that is not its destination.
    to_twin: bool,
    /// Its origin was walking or driven when it was sent.
    src_moving: bool,
    /// Its destination's anchor then, if it had one.
    dst_anchor: Option<u32>,
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
    /// Straight-line distance from origin to destination when it was
    /// sent, metres.
    pub distance_m: f64,
    /// Whether its destination was walking when it was sent.
    pub dst_walking: bool,
    /// Origination to its core taking it, seconds.
    pub taken_s: Option<f64>,
    /// Origination to its drop, seconds.
    pub dropped_s: Option<f64>,
    /// The relays at its origin's and its destination's addresses.
    pub src_relay: Option<u32>,
    /// See `src_relay`.
    pub dst_relay: Option<u32>,
    /// It reached a node with its destination's ID that is not its
    /// destination.
    pub to_twin: bool,
    /// Its origin was walking or driven when it was sent.
    pub src_moving: bool,
    /// Its destination's anchor then, if it had one.
    pub dst_anchor: Option<u32>,
    /// Origination to the first DATA of it from a node other than its
    /// origin, seconds.
    pub entered_s: Option<f64>,
    /// The time left in the last DATA to its destination node, seconds.
    pub time_left_s: Option<f64>,
    /// Beats of its destination node its holder heard while it was
    /// queued, and the DATA the holder sent it.
    pub holder_beats: u32,
    /// See `holder_beats`.
    pub holder_tries: u32,
    /// Its destination node's beats at the holder while it was queued.
    pub watch: HolderWatch,
}

/// Fates of a sleepy node's beat at the holder of a message for it, by
/// index into [`HolderWatch::beats`].
pub mod beat_fate {
    /// Received.
    pub const RECEIVED: usize = 0;
    /// The node was dark: the beat reached nobody.
    pub const DARK: usize = 1;
    /// No link from the node to the holder above sensitivity on average.
    pub const FAR: usize = 2;
    /// The holder rested on another channel when the beat began.
    pub const OFF_CHANNEL: usize = 3;
    /// Lost to noise: a weak link or a fade.
    pub const WEAK: usize = 4;
    /// Lost to another frame: capture, interference, or the holder
    /// demodulating a corrupted one.
    pub const COLLIDED: usize = 5;
    /// The holder was transmitting, or turning its radio round for it.
    pub const TRANSMITTING: usize = 6;
    /// The holder was checking a channel or retuning.
    pub const CHECKING: usize = 7;
    /// Not received for another reason.
    pub const OTHER: usize = 8;
    /// The node's listen-before-talk found its channel busy each time and
    /// it skipped the beat.
    pub const SKIPPED: usize = 9;
    /// The node beat without an anchor: no window follows.
    pub const ANCHORLESS: usize = 10;
    /// Number of fates.
    pub const N: usize = 11;
    /// Column names.
    pub const NAMES: [&str; N] = [
        "rx",
        "dark",
        "far",
        "offch",
        "weak",
        "coll",
        "tx",
        "check",
        "other",
        "skip",
        "anchorless",
    ];
}

/// A message's wait at its holder, for [`MessageRow`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HolderWatch {
    /// When the holder queued it.
    pub queued_at: Option<Instant>,
    /// Its destination node was in the holder's heard table then.
    pub known: bool,
    /// Its destination's anchor was the holder then.
    pub anchored: bool,
    /// DATA of it the holder sent with the long preamble.
    pub long_tries: u32,
    /// Its destination's beats while it was queued, by [`beat_fate`].
    pub beats: [u32; beat_fate::N],
    /// Of the beats the holder did not receive, those another node that
    /// is not sleepy did.
    pub elsewhere: u32,
    /// For a node that does not walk, its link to the holder and to the
    /// strongest relay, dBm before fading, when the holder queued it.
    pub holder_dbm: Option<f32>,
    pub best_dbm: Option<f32>,
}

/// What a frame of a message was, for [`Sim::message_frame`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FrameKind {
    /// A DATA.
    Data,
    /// An ACK that takes it.
    Taken,
    /// An ACK that refuses it.
    Refused,
}

/// A message an application waits to hand to its core.
#[derive(Clone, Copy)]
struct Offer {
    tag: u32,
    dst_pos: Position,
    dst_id: NodeId,
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
    pub(crate) xy: Vec<Xy>,
    pub(crate) sites: Vec<Site>,
    /// The walking nodes, if any walk.
    pub(crate) walks: Option<Walks>,
    /// Where the local grid is on the Earth.
    origin: Origin,
    /// Whether the core being handed a GPS fix (`Ev::Fix`) is.
    fixing: bool,
    /// Each node's ID, assigned here (spec "Addresses"): one per node but
    /// for the pairs the scenario forces to share one.
    ids: Vec<NodeId>,
    /// The forced pairs, and whether each shares its ID (`SharedIds`).
    pub(crate) pairs: Vec<crate::metrics::Pair>,
    /// Per node, its GPS fault: from when, how, and in which direction;
    /// whether fixes are checked; the last fix handed to its core and when.
    gps_fault: Vec<Option<(Instant, crate::scenario::FaultKind, f64, Xy)>>,
    gps: crate::scenario::Gps,
    accepted: Vec<(Xy, Instant)>,
    /// Per node that is not sleepy, with `[gps] error_m`, the error of its
    /// fixes, when it was last drawn, and the stream it is drawn from.
    gps_error: Vec<(Xy, Instant, Stream)>,
    /// The texts held at a first far fix after the settle time (roadmap
    /// D70): whether their holder had turned fixed again before, and how
    /// it delivered them, an index of `metrics::Carried::held`.
    held_far: HashMap<u32, (bool, Option<usize>)>,
    /// Per relay, its last first far fix, and whether it has turned fixed
    /// again since it was first carried.
    far_at: HashMap<u32, (Instant, bool)>,
    /// Deliveries by the way they went, after the settle time
    /// (`metrics::Carried::delivered_by`).
    delivered_by: [u64; 4],
    /// Relay node by ID, as [`key`] gives it.
    pub(crate) relay_index: HashMap<u32, u32>,
    /// Whether the relay a core event is being delivered to was booting
    /// before it, so that its boot choice is not counted as a move.
    was_booting: bool,
    positions: Vec<Position>,
    /// Nodes `0..n_relays` are not sleepy; the rest, placed as leaves, are.
    n_relays: u32,
    /// Every node's core.
    relays: Vec<Relay>,
    pub(crate) nodes: Vec<NodeState>,
    pub(crate) now: Instant,
    heap: Events<Ev>,
    /// The cores' wakeups, apart from `heap` (`queue.rs`).
    wakeups: Wakeups,
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
    /// The people's nodes, in node order: relays placed as leaves, then the
    /// sleepy nodes. They send and receive the texts.
    people: Vec<u32>,
    /// Per node, its index in `people`, or `u32::MAX`.
    person_of: Vec<u32>,
    /// Each node's address as a sender last learned it: a relay's
    /// position, or the relay a leaf was last attached to.
    address: Vec<Option<Position>>,
    /// When each node is switched on, and whether it is still to be.
    boot_at: Vec<Instant>,
    pending_boot: Vec<bool>,
    /// Each node's clock.
    pub(crate) clocks: Vec<Clock>,
    /// When each sleepy node first took an anchor.
    first_anchor: Vec<Option<Instant>>,
    /// Since when each sleepy node has been without an anchor, past its
    /// boot listen; and every such spell that ended with an anchor, from
    /// when to when.
    anchorless_since: Vec<Option<Instant>>,
    spells: Vec<(Instant, Instant)>,
    /// When each text the application skipped was due.
    skipped_at: Vec<Instant>,
    /// Once a minute from the start: the time, s, the mean and the most
    /// anchorless entries in a relay's neighbour table, and the sleepy
    /// nodes without an anchor.
    anchorless: Vec<(u64, f64, u32, u32)>,
    availability: Option<(f64, f64)>,
    avail_rng: Vec<Stream>,
    /// Per leaf, when it came back into reach, until a relay hears it.
    back_at: Vec<Option<Instant>>,
    /// Leaf ID to node index, for the leaves that go dark.
    leaf_nodes: HashMap<[u8; 4], u32>,
    /// Sleepy node by ID, as [`key`] gives it.
    pub(crate) sleepy_index: HashMap<u32, u32>,
    /// DATA on air addressed to a sleepy node: key to the node, what the
    /// frame found at its start, and the message and its holder.
    pub(crate) to_sleepy: HashMap<u64, ToSleepy>,
    /// Heartbeats on air of nodes that are not sleepy, after the settle
    /// time: key to the receivers that lost them, and why.
    pub(crate) beat_losses: HashMap<u64, Vec<(u32, crate::metrics::Outcome)>>,
    /// Per sleepy node by index less the relays: the start of its last
    /// beat and the end of the last DATA for it that it received.
    pub(crate) beat_took: Vec<(Instant, Instant)>,
    /// A delivery's hand-over under way, by holder and message: the window
    /// it went in and what became of its first DATA
    /// (`SleepMetrics::waited`).
    pub(crate) first_try: HashMap<(u32, u32), (usize, FirstTry)>,
    /// Sleepy nodes whose anchor the scenario removed or restarted, and
    /// when; and, once an anchor hears them again, when that was.
    pub(crate) orphans: HashMap<u32, (Instant, Option<Instant>)>,
    origins: Vec<OriginRecord>,
    /// Per node, the message its application waits to send.
    offers: Vec<Option<Offer>>,
    /// Message id (kept across hops) to traffic tag.
    msg_tags: HashMap<u32, u32>,
    /// What each application offered, to offer again when given back.
    offered: HashMap<u32, Offer>,
    /// The count of the last DATA of each message to each node, to read
    /// the count a NACK answers.
    pub(crate) data_hops: HashMap<(u32, u32), u8>,
    /// Per minute and message tag, airtime by frame kind, when asked for.
    msg_minutes: Option<HashMap<(u32, u32), [u64; 4]>>,
    /// Messages queued, by holder and destination node's ID, for counting
    /// the beats the holder hears.
    queued_for: HashMap<(u32, u32), Vec<u32>>,
    /// The holders of messages queued for each node, by the node's ID.
    holders_of: HashMap<u32, Vec<u32>>,
    /// Beats on the air of a node with messages queued: key to each
    /// holder and the beat's fate there so far.
    pub(crate) beat_watch: HashMap<u64, Vec<(u32, Option<usize>)>>,
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
    /// DATA on air from a relay to a relay, with `tables_on`: key to the
    /// margin and distance bin of [`crate::metrics::Tables::data_by_reach`].
    pub(crate) reach_of: HashMap<u64, (usize, usize)>,
    /// DATA on air addressed to a relay: key to the ACK that would answer
    /// it, message ID and `to`.
    pub(crate) answer_of: HashMap<u64, (u32, u32)>,
    /// DATA a relay received and has not answered yet: (relay, message,
    /// `to`) to the answer's deadline and whether a leaf sent it.
    pub(crate) awaiting: HashMap<(u32, u32, u32), (Instant, bool)>,
    /// The relay DATA a relay is being handed now: (relay, message,
    /// sender's ID, count), for [`crate::metrics::HopPlaceRefusals`].
    rx_data: Option<(u32, u32, u32, u8)>,
    /// (relay, message) for every record a relay has kept, for
    /// [`crate::metrics::RecordsLost`].
    recorded: std::collections::HashSet<(u32, u32)>,
    /// Refused messages not yet sent again: (sender, message) to the
    /// refusal's time and whether a hop place was free for another.
    refused_at: HashMap<(u32, u32), (Instant, bool)>,
}

/// The ID the simulator gives node `i`: `i + 1` times an odd constant,
/// a bijection on 32 bits, so every node has its own and none is zero,
/// and the IDs spread over the range as random ones would.
fn node_id(i: u32) -> NodeId {
    NodeId((i + 1).wrapping_mul(0x9e37_79b1).to_be_bytes())
}

/// The 45° sector of the bearing to `p`, north first and clockwise.
fn sector(p: bark_proto::geo::Point) -> usize {
    let deg = p.east.atan2(p.north).to_degrees().rem_euclid(360.0);
    ((deg / 45.0) as usize).min(crate::metrics::SECTORS - 1)
}

/// The bin of [`crate::metrics::Tables`] of a neighbour at `p`.
pub(crate) fn reach_bin(p: bark_proto::geo::Point) -> usize {
    use crate::metrics::{REACH_BINS, REACH_BIN_M};
    ((p.distance_m(Default::default()) / REACH_BIN_M) as usize).min(REACH_BINS - 1)
}

/// A node ID as a map key.
pub(crate) fn key(id: NodeId) -> u32 {
    u32::from_be_bytes(id.0)
}

/// `n` node IDs drawn from the run's seed, each node from its own stream.
/// A draw that is zero, which names no node, or another node's ID is drawn
/// again: the simulator keys its own tables by ID. Among the 20 000 nodes
/// of a domain two share an ID about one run in twenty.
fn random_ids(seed: u64, n: usize) -> Vec<NodeId> {
    let mut seen = std::collections::HashSet::with_capacity(n);
    (0..n as u64)
        .map(|i| {
            let mut r = Stream::new(seed, Purpose::NodeId, i);
            loop {
                let id = r.next_u64() as u32;
                if id != 0 && seen.insert(id) {
                    return NodeId(id.to_be_bytes());
                }
            }
        })
        .collect()
}

/// The pairs of nodes `specs` force to share an ID, drawn from the run's
/// seed among the nodes that do not walk, whose links are stored; no node
/// is in two. Nodes `0..n_relays` are not sleepy. A pair in reach hears
/// each other both ways above `sens_dbm` before fading; one out of reach
/// has no link stored either way. Each pair also counts the relays that
/// hear both of its nodes above `sens_dbm`, which keep one entry for two.
fn draw_pairs(
    specs: &[SharedIds],
    seed: u64,
    n_relays: u32,
    walking: &[bool],
    links: &LinkTable,
    sens_dbm: f64,
) -> Result<Vec<crate::metrics::Pair>, String> {
    let n = walking.len() as u32;
    let mut used = vec![false; n as usize];
    let up = |a: u32, b: u32| {
        links
            .get(a, b)
            .is_some_and(|l| l.base_dbm as f64 >= sens_dbm)
    };
    let mut out = Vec::new();
    for (k, spec) in specs.iter().enumerate() {
        let mut r = Stream::new(seed, Purpose::SharedIds, k as u64);
        let (ga, gb) = match spec.kind {
            PairKind::RelayRelay => (0..n_relays, 0..n_relays),
            PairKind::RelaySleepy => (0..n_relays, n_relays..n),
            PairKind::SleepySleepy => (n_relays..n, n_relays..n),
        };
        let free = |i: u32, used: &[bool]| !used[i as usize] && !walking[i as usize];
        for _ in 0..spec.count {
            let mut found = None;
            for _ in 0..100_000 {
                if ga.is_empty() || gb.is_empty() {
                    break;
                }
                let a = r.range(ga.start as u64, ga.end as u64 - 1) as u32;
                if !free(a, &used) {
                    continue;
                }
                let b = match spec.reach {
                    PairReach::In => {
                        let c: Vec<u32> = links
                            .row(a)
                            .iter()
                            .map(|l| l.rx)
                            .filter(|&b| gb.contains(&b) && b != a && free(b, &used))
                            .filter(|&b| up(a, b) && up(b, a))
                            .collect();
                        if c.is_empty() {
                            continue;
                        }
                        c[r.range(0, c.len() as u64 - 1) as usize]
                    }
                    PairReach::Out => {
                        let b = r.range(gb.start as u64, gb.end as u64 - 1) as u32;
                        if b == a || !free(b, &used) || links.get(a, b).is_some() {
                            continue;
                        }
                        if links.get(b, a).is_some() {
                            continue;
                        }
                        b
                    }
                };
                found = Some((a, b));
                break;
            }
            let Some((a, b)) = found else {
                return Err(format!("shared_ids {k}: no pair found"));
            };
            used[a as usize] = true;
            used[b as usize] = true;
            let both = (0..n_relays)
                .filter(|&x| x != a && x != b && up(a, x) && up(b, x))
                .count() as u32;
            out.push(crate::metrics::Pair {
                kind: spec.kind,
                reach: spec.reach,
                a,
                b,
                shared: spec.share,
                relays_hearing_both: both,
            });
        }
    }
    Ok(out)
}

/// A node's clock: at true time `t` it reads `t + t × ppb / 10⁹`. The
/// simulator keeps true time and hands each core its own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Clock {
    /// How fast it runs, parts per billion; negative if slow.
    ppb: i64,
}

impl Clock {
    const BILLION: i128 = 1_000_000_000;

    /// A clock off by `ppm` parts per million.
    fn off_by(ppm: f64) -> Clock {
        Clock {
            ppb: (ppm * 1e3).round() as i64,
        }
    }

    /// What the clock reads at true time `t`.
    pub(crate) fn local(self, t: Instant) -> Instant {
        if self.ppb == 0 {
            return t;
        }
        let us = t.as_micros() as i128;
        let l = us + floor_ratio(us, self.ppb, Self::BILLION as i64);
        Instant::from_micros(l.clamp(0, u64::MAX as i128) as u64)
    }

    /// The earliest true time at which the clock reads `l` or later: a
    /// timer the core sets for `l` fires then, never before. At `t` the
    /// clock reads `⌊t (10⁹ + ppb) / 10⁹⌋`, `l` or more from
    /// `t = l × 10⁹ / (10⁹ + ppb)` on, and the first whole microsecond of
    /// that is `l − ⌊l × ppb / (10⁹ + ppb)⌋`.
    pub(crate) fn true_time(self, l: Instant) -> Instant {
        if self.ppb == 0 || l == Instant::MAX {
            return l;
        }
        let target = l.as_micros() as i128;
        let t = target - floor_ratio(target, self.ppb, Self::BILLION as i64 + self.ppb);
        Instant::from_micros(t.clamp(0, u64::MAX as i128) as u64)
    }
}

/// `⌊a × b / d⌋` for `d > 0`. In 64 bits when they hold the product, as
/// they do for a clock's gain over any run of a month or less: a 128-bit
/// division costs several times more, and a clock is read on every event.
fn floor_ratio(a: i128, b: i64, d: i64) -> i128 {
    let narrow = a >= i64::MIN as i128 && a <= i64::MAX as i128;
    match (a as i64).checked_mul(b).filter(|_| narrow) {
        Some(p) => p.div_euclid(d) as i128,
        None => (a * b as i128).div_euclid(d as i128),
    }
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
        let people: Vec<u32> = (0..n as u32)
            .filter(|&i| placed[i as usize].person)
            .collect();
        let mut person_of = vec![u32::MAX; n];
        for (k, &i) in people.iter().enumerate() {
            person_of[i as usize] = k as u32;
        }
        let relay_people: Vec<u32> = people.iter().copied().filter(|&i| i < n_relays).collect();
        let moves = sc.movement.fraction > 0.0
            || sc.vehicles.fraction > 0.0
            || (!relay_people.is_empty()
                && (sc.movement.relay_fraction > 0.0 || sc.vehicles.relay_fraction > 0.0));
        let walks = moves.then(|| {
            Walks::new(
                &sc.movement,
                &sc.vehicles,
                &sc.links,
                base.tx_power_dbm as f64,
                radio.cutoff_dbm(),
                &xy,
                &site,
                n_relays..n as u32,
                &relay_people,
                sc.seed,
            )
        });
        let walking: Vec<bool> = (0..n as u32)
            .map(|i| walks.as_ref().is_some_and(|w| w.walks(i)))
            .collect();
        let links = LinkTable::build(
            &sc.links,
            &NodeRadio {
                xy: &xy,
                tx_dbm: &tx_dbm,
                site: &site,
                names: &names,
                walks: &walking,
            },
            radio.cutoff_dbm(),
            sc.seed,
        )?;

        let mut ids: Vec<NodeId> = match sc.ids {
            Ids::Sequence => (0..n as u32).map(node_id).collect(),
            Ids::Random => random_ids(sc.seed, n),
        };
        let pairs = draw_pairs(
            &sc.shared_ids,
            sc.seed,
            n_relays,
            &walking,
            &links,
            radio.sensitivity_dbm(),
        )?;
        for p in &pairs {
            if p.shared {
                ids[p.b as usize] = ids[p.a as usize];
            }
        }
        let mut homes = vec![None; n];
        for h in &sc.homes {
            let c = Channel::new(h.channel)
                .ok_or_else(|| format!("home {}: no channel {}", h.node, h.channel))?;
            homes[resolve(&placed, &h.node)? as usize] = Some(c);
        }
        let config_for = |i: usize| Config {
            position: positions[i],
            node_id: ids[i],
            home_override: homes[i],
            ..base
        };
        let core_seed = |i: usize| stream_seed(sc.seed, Purpose::CoreSeed, i as u64);
        let relays: Vec<Relay> = (0..n)
            .map(|i| Relay::new(config_for(i), core_seed(i)))
            .collect();

        let nodes: Vec<NodeState> = (0..n)
            .map(|i| NodeState {
                alive: true,
                present: true,
                avail_dark: false,
                group_dark: 0,
                resting: None,
                listen_from: Instant::ZERO,
                ex: None,
                ex_gen: 0,
                wake_at: None,
                lock: Lock::Idle,
                arrivals: Vec::new(),
                observer: None,
                cad_rng: Stream::new(sc.seed, Purpose::CadMiss, i as u64),
                rx_rng: Stream::new(sc.seed, Purpose::Reception, i as u64),
                held: None,
                held_gen: 0,
                restarts: 0,
                beats: BeatRing::default(),
                sniff: None,
                sniff_since: Instant::ZERO,
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
            relay_index: (0..n_relays).map(|i| (key(ids[i as usize]), i)).collect(),
            sleepy_index: (n_relays..n as u32)
                .map(|i| (key(ids[i as usize]), i))
                .collect(),
            to_sleepy: HashMap::new(),
            beat_losses: HashMap::new(),
            beat_took: vec![(Instant::ZERO, Instant::ZERO); n - n_relays as usize],
            first_try: HashMap::new(),
            orphans: HashMap::new(),
            ids,
            pairs,
            gps_fault: vec![None; n],
            gps: sc.gps.clone(),
            accepted: Vec::new(),
            gps_error: Vec::new(),
            held_far: HashMap::new(),
            far_at: HashMap::new(),
            delivered_by: [0; 4],
            positions,
            sites: site,
            walks,
            origin,
            fixing: false,
            was_booting: false,
            n_relays,
            relays,
            nodes,
            now: Instant::ZERO,
            heap: Events::new(),
            wakeups: Wakeups::new(n),
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
            people,
            person_of,
            address: vec![None; n],
            boot_at: vec![Instant::ZERO; n],
            clocks: (0..n as u64)
                .map(|i| {
                    let drift = sc.clocks.drift_ppm;
                    if drift > 0.0 {
                        let u = Stream::new(sc.seed, Purpose::Clock, i).uniform();
                        Clock::off_by((2.0 * u - 1.0) * drift)
                    } else {
                        Clock::default()
                    }
                })
                .collect(),
            pending_boot: vec![true; n],
            first_anchor: vec![None; n],
            anchorless_since: vec![None; n],
            spells: Vec::new(),
            skipped_at: Vec::new(),
            anchorless: Vec::new(),
            availability: None,
            avail_rng: Vec::new(),
            back_at: Vec::new(),
            leaf_nodes: HashMap::new(),
            origins: Vec::new(),
            offers: (0..n).map(|_| None).collect(),
            msg_tags: HashMap::new(),
            offered: HashMap::new(),
            data_hops: HashMap::new(),
            msg_minutes: None,
            queued_for: HashMap::new(),
            holders_of: HashMap::new(),
            beat_watch: HashMap::new(),
            script: Vec::new(),
            script_log: Vec::new(),
            cadence: sc.metrics.cadence_s.map(secs),
            end: Instant::ZERO + secs(sc.duration_s),
            actions: Vec::new(),
            metrics: Metrics::new(n),
            scenario_name: sc.name.clone(),
            seed: sc.seed,
            per_node_summary: sc.metrics.per_node,
            settle: Instant::ZERO + secs(sc.settled_s()),
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
            reach_of: HashMap::new(),
            answer_of: HashMap::new(),
            awaiting: HashMap::new(),
            rx_data: None,
            recorded: std::collections::HashSet::new(),
            refused_at: HashMap::new(),
        };
        sim.component = sim.components();
        sim.setup(sc, &placed)?;
        Ok(sim)
    }

    fn setup(&mut self, sc: &Scenario, placed: &[PlacedNode]) -> Result<(), String> {
        let n = placed.len();
        // Switch-on: every node is off until its boot.
        for i in 0..n {
            // A person switches a relay on as a sleepy node.
            let window = if (i as u32) < self.n_relays && self.person_of[i] == u32::MAX {
                sc.boot.relays_s
            } else {
                sc.boot.sleepy_s
            };
            let at = if window > 0.0 {
                let mut r = Stream::new(sc.seed, Purpose::Boot, i as u64);
                Instant::ZERO + secs(r.uniform() * window)
            } else {
                Instant::ZERO
            };
            self.boot_at[i] = at;
            self.nodes[i].alive = false;
            self.push(at, Ev::Boot(i as u32));
        }
        // Observers.
        // The relays placed as such: those placed as people may be many.
        let all_relays: Vec<String> = (0..self.n_relays)
            .filter(|&i| self.person_of[i as usize] == u32::MAX)
            .map(|i| format!("relay:{i}"))
            .collect();
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
            let walks = self.walks.as_ref();
            let reach = self
                .xy
                .iter()
                .enumerate()
                .filter(|(i, _)| !walks.is_some_and(|w| w.walks(*i as u32)))
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
                at,
                power_dbm: s.power_dbm,
                reach,
                moving: Vec::new(),
                on: false,
                gen: 0,
                key: 0,
                end: Instant::ZERO,
                rng,
            });
            // Toggle at its start into the start state.
            let from = Instant::ZERO + secs(s.from_s.max(0.0));
            if start_on {
                self.push(from, Ev::Interferer(k as u32, 0));
            } else {
                let off = secs(self.interferers[k].rng.exponential(s.mean_off_s));
                self.push(from + off, Ev::Interferer(k as u32, 0));
            }
        }
        // Traffic.
        for t in &sc.traffic {
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
                    // Its own index among the flows, not among all traffic.
                    let i = self.flows.len() as u32;
                    self.flows.push(f);
                    self.push(Instant::ZERO + first, Ev::Flow(i));
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
                    self.leaf_xy = self.people.iter().map(|&i| self.xy[i as usize]).collect();
                    if let Destination::Within { radius_m } = destination {
                        self.leaf_grid = Some(Grid::new(&self.leaf_xy, *radius_m));
                    }
                    for k in 0..self.people.len() {
                        let i = self.people[k];
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
            // Each sleepy node's process starts at its switch-on.
            for i in self.n_relays..n as u32 {
                let r = Stream::new(sc.seed, Purpose::Availability, i as u64);
                self.avail_rng.push(r);
                self.leaf_nodes.insert(self.node_id(i).0, i);
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
            if e.action == ScriptAction::Start {
                // Off until then, whatever its switch-on time.
                self.pending_boot[node as usize] = false;
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
        // Walkers move whether their node is on or not.
        let legs: Vec<Instant> = self
            .walks
            .iter()
            .flat_map(|w| w.walkers.iter().map(|k| k.until))
            .collect();
        for (w, until) in legs.into_iter().enumerate() {
            self.push(until, Ev::Leg(w as u32));
        }
        // GPS faults.
        self.accepted = (0..n).map(|i| (self.xy[i], Instant::ZERO)).collect();
        for (k, f) in sc.gps.faults.iter().enumerate() {
            let mut r = Stream::new(sc.seed, Purpose::Gps, k as u64);
            let walks = self.walks.as_ref();
            let pool: Vec<u32> = if f.sleepy {
                (self.n_relays..n as u32)
                    .filter(|&i| !walks.is_some_and(|w| w.walks(i)))
                    .collect()
            } else {
                (0..self.n_relays)
                    .filter(|&i| !walks.is_some_and(|w| w.walks(i)))
                    .collect()
            };
            let mut pool: Vec<u32> = pool
                .into_iter()
                .filter(|&i| self.gps_fault[i as usize].is_none())
                .collect();
            if pool.len() < f.count as usize {
                return Err(format!("gps fault {k}: only {} nodes", pool.len()));
            }
            for _ in 0..f.count {
                let i = pool.swap_remove(r.range(0, pool.len() as u64 - 1) as usize);
                let a = r.uniform() * std::f64::consts::TAU;
                let size = match f.kind {
                    crate::scenario::FaultKind::Jump => f.distance_m,
                    crate::scenario::FaultKind::Drift => f.speed_m_s,
                };
                let dir = Xy {
                    x: a.cos(),
                    y: a.sin(),
                };
                let at = Instant::ZERO + secs(f.at_s);
                self.gps_fault[i as usize] = Some((at, f.kind, size, dir));
                self.metrics.research.gps_nodes[usize::from(f.sleepy)].push(i);
                if !f.sleepy {
                    self.push(at, Ev::Gps(i));
                }
            }
        }
        // Fixes of every node that is not sleepy, and their error.
        if sc.gps.error_m > 0.0 {
            self.gps_error = (0..self.n_relays)
                .map(|i| {
                    let mut r = Stream::new(sc.seed, Purpose::GpsError, i as u64);
                    let e = Xy {
                        x: r.normal() * sc.gps.error_m,
                        y: r.normal() * sc.gps.error_m,
                    };
                    (e, Instant::ZERO, r)
                })
                .collect();
        }
        if sc.gps.relays_every_s > 0.0 {
            for i in 0..self.n_relays {
                if self.gps_fault[i as usize].is_none() {
                    let at = self.boot_at[i as usize] + secs(sc.gps.relays_every_s);
                    self.push(at, Ev::Gps(i));
                }
            }
        }
        // Groups that go dark together.
        for (k, g) in sc.dark_groups.iter().enumerate() {
            let near = resolve(placed, &g.near)?;
            let at = self.xy[near as usize];
            let mut c: Vec<u32> = (self.n_relays..n as u32)
                .filter(|&i| !self.walks.as_ref().is_some_and(|w| w.walks(i)))
                .collect();
            c.sort_by(|&a, &b| {
                let d = |i: u32| self.xy[i as usize].dist(at);
                d(a).total_cmp(&d(b)).then(a.cmp(&b))
            });
            c.truncate(g.count as usize);
            if c.len() < g.count as usize {
                return Err(format!("dark group {k}: only {} sleepy nodes", c.len()));
            }
            self.metrics.research.groups.push(crate::metrics::Group {
                near,
                from_s: g.at_s,
                to_s: g.at_s + g.for_s,
                members: c,
            });
            self.push(Instant::ZERO + secs(g.at_s), Ev::Group(k as u32, true));
            self.push(
                Instant::ZERO + secs(g.at_s + g.for_s),
                Ev::Group(k as u32, false),
            );
        }
        if let Some(c) = self.cadence {
            self.push(Instant::ZERO + c, Ev::Sample);
        }
        Ok(())
    }

    pub(crate) fn push(&mut self, t: Instant, ev: Ev) {
        self.seq += 1;
        self.heap.push(queue::order(t, self.seq), ev);
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

    /// A node's core.
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
            ids: (0..n).map(|i| (self.node_id(i).0, i)).collect(),
        };
        self.tracer = Some(Tracer::new(out, names));
    }

    /// Run to the scenario end.
    pub fn run(&mut self) {
        self.run_until(self.end);
        for n in 0..self.node_count() as u32 {
            self.wake(n);
        }
        if let Some(t) = &mut self.tracer {
            t.flush();
        }
    }

    /// Process every event at or before `t`, then set the clock to `t`.
    pub fn run_until(&mut self, t: Instant) {
        loop {
            let event = self.heap.peek();
            let wakeup = self.wakeups.peek();
            let next = match (event, wakeup) {
                (Some(e), Some((w, _))) => e.min(w),
                (Some(e), None) => e,
                (None, Some((w, _))) => w,
                (None, None) => break,
            };
            if queue::time(next) > t {
                break;
            }
            self.now = queue::time(next);
            self.metrics.events += 1;
            if let Some((_, n)) = wakeup.filter(|&(w, _)| w == next) {
                // The wakeup stays queued while the core runs, so that the
                // one it sets next takes its place in one move.
                let st = &mut self.nodes[n as usize];
                if st.alive {
                    st.wake_at = None;
                    self.deliver(n, Event::Wakeup);
                }
                if self.wakeups.get(n) == Some(next) {
                    self.wakeups.set(n, None);
                }
            } else {
                let (_, ev) = self.heap.pop().expect("peeked");
                self.dispatch(ev);
            }
        }
        self.now = self.now.max(t);
    }

    fn dispatch(&mut self, ev: Ev) {
        match ev {
            Ev::Boot(n) => {
                if std::mem::take(&mut self.pending_boot[n as usize]) {
                    self.start(n);
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
            Ev::InterfererMoves(k, g) => {
                if self.interferers[k as usize].gen == g && self.interferers[k as usize].on {
                    self.interferer_reaches_walkers(k);
                    self.push(self.now + crate::walk::REGRID, Ev::InterfererMoves(k, g));
                }
            }
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
                    self.sample_anchors();
                    self.sample_anchorless();
                    self.sample_dense();
                    self.sample_tables();
                }
                let c = self.cadence.expect("sampling on");
                self.push(self.now + c, Ev::Sample);
            }
            Ev::Leg(w) => {
                let walks = self.walks.as_mut().expect("walkers");
                let until = walks.next_leg(w, self.now);
                if self.now >= self.settle {
                    let k = &walks.walkers[w as usize];
                    self.metrics.sleep.walks[usize::from(k.walking)] += 1;
                }
                self.push(until, Ev::Leg(w));
            }
            Ev::Group(k, dark) => {
                let members = self.metrics.research.groups[k as usize].members.clone();
                for n in members {
                    let st = &mut self.nodes[n as usize];
                    if dark {
                        st.group_dark += 1;
                    } else {
                        st.group_dark -= 1;
                    }
                    self.set_presence(n);
                }
            }
            Ev::Fix(n) => self.fix(n),
            Ev::Gps(n) => {
                self.fix(n);
                let every = match self.gps.relays_every_s {
                    s if s > 0.0 => secs(s),
                    _ => Duration::from_secs(1),
                };
                self.push(self.now + every, Ev::Gps(n));
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
        for (i, r) in self.relays[..self.n_relays as usize].iter().enumerate() {
            let l = r.load(self.clocks[i].local(self.now));
            let rec = r.records(self.clocks[i].local(self.now));
            let m = serde_json::to_string(&self.metrics.nodes[i]).expect("serialisable");
            s.push_str(&format!(
                "{{\"t\":{t},\"n\":{i},\"alive\":{},\"booting\":{},\"home\":{},\"busy\":{:.5},\"busy_min\":{:.5},\"bucket_us\":{},\"sq\":{},\"dq\":{},\"lv\":{},\"nb\":{},\"hp\":{},\"wt\":{},\"rt\":{},\"rec\":{},\"rec_idle_s\":{},\"m\":{m}}}\n",
                self.nodes[i].alive,
                r.booting(),
                r.home().index(),
                r.busy().fraction(r.home()) as f64 / bark_proto::tables::busy::ONE as f64,
                bark_proto::Channel::all()
                    .map(|c| r.busy().fraction(c))
                    .min()
                    .unwrap_or(0) as f64
                    / bark_proto::tables::busy::ONE as f64,
                l.bucket.as_micros(),
                l.send_queue,
                l.delivery_queue,
                l.heard,
                l.neighbours,
                l.hop_places,
                l.waiting,
                l.returns,
                rec.0,
                rec.1.map_or(-1, |d| (d.as_micros() / 1_000_000) as i64)
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
        for (k, leaf) in self.relays[self.n_relays as usize..].iter().enumerate() {
            let n = self.n_relays as usize + k;
            a.add(
                &self.metrics.nodes[n],
                leaf.anchor().is_some(),
                leaf.holding().next().is_some(),
                self.offers[n].is_some(),
                self.nodes[n].present,
            );
        }
        let app = &self.metrics.app;
        s.push_str(&format!(
            "{{\"t\":{t},\"leaves\":{},\"addressed\":{},\"sleep\":{},\"originated\":{},\"delivered\":{},\"skipped\":{}}}\n",
            serde_json::to_string(&a).expect("serialisable"),
            serde_json::to_string(&self.metrics.addressed).expect("serialisable"),
            serde_json::to_string(&self.metrics.sleep).expect("serialisable"),
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
        for (i, r) in self.relays[..self.n_relays as usize].iter().enumerate() {
            if !self.nodes[i].alive || r.booting() {
                continue;
            }
            let l = r.load(self.clocks[i].local(self.now));
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
        self.note_beat(node, &ev);
        if node < self.n_relays {
            self.rx_data = match ev {
                Event::Received { bytes, .. } => match bark_proto::wire::Packet::decode(bytes) {
                    Ok(bark_proto::wire::Packet::Data(d)) if d.hops > 0 => {
                        Some((node, d.msg_id().0, key(d.src_hop), d.hops))
                    }
                    _ => None,
                },
                _ => None,
            };
        }
        let clock = self.clocks[node as usize];
        let rx = self.rx_data.filter(|r| r.0 == node);
        if let Some((at, id, _, _)) = rx {
            if !self.relays[at as usize].knows(MsgId(id)) && self.recorded.contains(&(at, id)) {
                let l = &mut self.metrics.records_lost;
                l.packets += 1;
                l.tags.extend(self.msg_tags.get(&id));
            }
        }
        let c = &mut self.relays[node as usize];
        self.was_booting = c.booting();
        c.handle(ev, clock.local(now), &mut |a| buf.push(a));
        if let Some((at, id, _, _)) = rx {
            if c.knows(MsgId(id)) {
                self.recorded.insert((at, id));
            }
        }
        let wake = c.next_wakeup().map(|t| clock.true_time(t));
        if node >= self.n_relays {
            let anchor = c.anchor().map(|a| a.position);
            let alone = c.anchorless();
            let since = &mut self.anchorless_since[node as usize];
            match (*since, alone) {
                (None, true) => *since = Some(now),
                (Some(t), false) => {
                    if anchor.is_some() {
                        self.spells.push((t, now));
                        let s = |t: Instant| t.as_micros() as f64 / 1e6;
                        self.metrics.nodes[node as usize]
                            .anchored_s
                            .push((s(now), s(t)));
                    }
                    *since = None;
                }
                _ => {}
            }
            self.attached(node, anchor);
        }
        for a in buf.drain(..) {
            self.apply(node, a, false);
        }
        self.actions = buf;
        let st = &mut self.nodes[node as usize];
        if st.wake_at != wake {
            st.wake_at = wake;
            let order = wake.map(|t| {
                self.seq += 1;
                // Late is allowed, early is not: a past deadline fires now.
                queue::order(t.max(now), self.seq)
            });
            self.wakeups.set(node, order);
        }
        // The application asks again after every event (core-interface §4).
        if self.offers[node as usize].is_some() {
            self.offer(node);
        }
    }

    /// Sleepy nodes whose anchor was removed or restarted: how many, how
    /// many an anchor heard again and the median and 90th percentile of
    /// how long that took, s; and the messages for them originated after
    /// that, and of those delivered.
    pub fn orphans(&self) -> (u64, u64, Option<f64>, Option<f64>, u64, u64) {
        let mut back: Vec<u64> = self
            .orphans
            .values()
            .filter_map(|&(t, b)| b.map(|b| b.since(t).as_micros()))
            .collect();
        back.sort_unstable();
        let at = |p| crate::metrics::percentile(&back, p).map(|us| us as f64 / 1e6);
        let (mut sent, mut got) = (0, 0);
        for o in &self.origins {
            if let Some(&(t, _)) = self.orphans.get(&o.dst) {
                if o.t >= t && o.t + Duration::from_secs(600) <= self.end {
                    sent += 1;
                    got += u64::from(o.fate == Fate::Delivered);
                }
            }
        }
        let n = self.orphans.len() as u64;
        (n, back.len() as u64, at(50.0), at(90.0), sent, got)
    }

    /// The radio of `node` receives duty-cycled until `until`, its cycle
    /// starting now. A packet it was receiving is lost unless enough of
    /// its preamble is left for the first window to detect it.
    fn sniff(&mut self, node: u32, until: Instant) {
        self.wake(node);
        if until <= self.now {
            return;
        }
        let detect = SYMBOL_TIME.mul(DETECT_SYMBOLS);
        let st = &self.nodes[node as usize];
        let keeps = matches!(st.lock, Lock::Rx(r) if r.preamble_end >= self.now + detect);
        if !keeps {
            self.abort_lock(node, Deaf::Sniff);
        }
        let st = &mut self.nodes[node as usize];
        st.sniff = Some((self.now, until));
        st.sniff_since = self.now;
    }

    /// The radio of `node` listens in full from now: count the time it
    /// received duty-cycled after the settle time.
    fn wake(&mut self, node: u32) {
        let (now, settle) = (self.now, self.settle);
        let st = &mut self.nodes[node as usize];
        let m = &mut self.metrics.nodes[node as usize];
        if let Some((_, until)) = st.sniff.take() {
            let (from, to) = (st.sniff_since.max(settle), until.min(now));
            if to > from {
                m.sniff_us += to.since(from).as_micros();
            }
        }
    }

    /// Once a minute: anchorless entries in the relays' neighbour tables,
    /// and the sleepy nodes without an anchor.
    fn sample_anchorless(&mut self) {
        let n = self.n_relays as usize;
        if n == 0 {
            return;
        }
        let counts: Vec<u32> = self.relays[..n]
            .iter()
            .map(|r| r.neighbours().filter(|e| e.anchorless).count() as u32)
            .collect();
        let mean = counts.iter().sum::<u32>() as f64 / n as f64;
        let most = counts.iter().copied().max().unwrap_or(0);
        let alone = self.relays[n..]
            .iter()
            .filter(|r| r.anchor().is_none())
            .count() as u32;
        self.anchorless
            .push((self.now.as_micros() / 1_000_000, mean, most, alone));
    }

    /// The spells sleepy nodes spent without an anchor past their boot
    /// listen and that began from `from` on: how many ended with an
    /// anchor, how many were still open at the end, and the median and
    /// 90th percentile of the length of those that ended, s.
    pub fn anchorless_spells(&self, from: Instant) -> (u64, u64, Option<f64>, Option<f64>) {
        let mut len: Vec<u64> = self
            .spells
            .iter()
            .filter(|(t, _)| *t >= from)
            .map(|(t, u)| u.since(*t).as_micros())
            .collect();
        len.sort_unstable();
        let open = self
            .anchorless_since
            .iter()
            .flatten()
            .filter(|&&t| t >= from)
            .count() as u64;
        let p = |p| crate::metrics::percentile(&len, p).map(|us| us as f64 / 1e6);
        (len.len() as u64, open, p(50.0), p(90.0))
    }

    /// When the sleepy nodes first took an anchor, s from each one's
    /// switch-on: those that never did, and the median, 90th percentile and latest of the
    /// others.
    pub fn first_anchors(&self) -> (u64, Option<f64>, Option<f64>, Option<f64>) {
        let n = self.n_relays as usize;
        let mut at: Vec<u64> = self.first_anchor[n..]
            .iter()
            .zip(&self.boot_at[n..])
            .filter_map(|(t, b)| t.map(|t| t.since(*b).as_micros()))
            .collect();
        at.sort_unstable();
        let never = (self.first_anchor.len() - n - at.len()) as u64;
        let p = |p| crate::metrics::percentile(&at, p).map(|us| us as f64 / 1e6);
        (
            never,
            p(50.0),
            p(90.0),
            at.last().map(|&us| us as f64 / 1e6),
        )
    }

    /// Texts the application skipped from `t` on, while its node still
    /// held one ([`Sim::originate`]).
    pub fn skipped_since(&self, t: Instant) -> u64 {
        self.skipped_at.iter().filter(|&&at| at >= t).count() as u64
    }

    /// See `anchorless`.
    pub fn anchorless_series(&self) -> &[(u64, f64, u32, u32)] {
        &self.anchorless
    }

    /// Per relay, how many running relays keep it in their neighbour
    /// tables, and how many with margin.
    pub(crate) fn known_by(&self) -> Vec<(u32, u32)> {
        use bark_proto::tables::neighbours::Neighbours;
        let mut known = vec![(0u32, 0u32); self.n_relays as usize];
        for (i, r) in self.relays[..self.n_relays as usize].iter().enumerate() {
            if !self.nodes[i].alive || r.booting() {
                continue;
            }
            let floor = r.busy().noise_floor_dbm();
            for e in r.neighbours().filter(|e| e.relay()) {
                if let Some(&j) = self.relay_index.get(&key(e.id)) {
                    let k = &mut known[j as usize];
                    k.0 += 1;
                    k.1 += u32::from(Neighbours::has_margin(e, floor));
                }
            }
        }
        known
    }

    /// Whether relays are placed among the people, so that the dense
    /// instruments are kept.
    fn dense(&self) -> bool {
        self.people.first().is_some_and(|&i| i < self.n_relays)
    }

    /// Once a minute after the settle time, with relays among the people:
    /// the running relays no running relay keeps (`metrics::Dense`).
    fn sample_dense(&mut self) {
        if !self.dense() || self.now < self.settle {
            return;
        }
        let known = self.known_by();
        let (mut none, mut faint) = ([0u32; 2], [0u32; 2]);
        for (i, k) in known.iter().enumerate() {
            if !self.nodes[i].alive || self.relays[i].booting() {
                continue;
            }
            let person = usize::from(self.person_of[i] != u32::MAX);
            none[person] += u32::from(k.0 == 0);
            faint[person] += u32::from(k.1 == 0);
        }
        let t = self.now.as_micros() / 1_000_000;
        let d = self
            .metrics
            .research
            .dense
            .get_or_insert_with(Default::default);
        d.unknown.push((t, none, faint));
    }

    /// Sample what the neighbour tables hold (`metrics::Tables`), for the
    /// JSON summary.
    pub fn tables_on(&mut self) {
        self.metrics.research.tables = Some(crate::metrics::Tables {
            relays: vec![(0, 0, 0); self.n_relays as usize],
            ..Default::default()
        });
    }

    /// Once a minute after the settle time, with `tables_on`: what the
    /// running relays' tables hold; every five minutes, whether each
    /// sleepy node's anchor is the strongest relay it hears.
    fn sample_tables(&mut self) {
        use bark_proto::tables::neighbours::Neighbours;
        if self.metrics.research.tables.is_none() || self.now < self.settle {
            return;
        }
        let mut t = self.metrics.research.tables.take().expect("on");
        for i in 0..self.n_relays as usize {
            let r = &self.relays[i];
            if !self.nodes[i].alive || r.booting() {
                continue;
            }
            let floor = r.busy().noise_floor_dbm();
            let e = &mut t.relays[i];
            e.0 += 1;
            for n in r.neighbours() {
                let margin = Neighbours::has_margin(n, floor);
                e.1 += 1;
                e.2 += u64::from(margin && n.relay());
                let s = sector(n.local);
                t.sectors[0][s] += 1;
                t.sectors[1][s] += u64::from(margin);
                if margin && n.relay() {
                    t.margin_by_distance[reach_bin(n.local)] += 1;
                }
            }
        }
        if (self.now.as_micros() / SERIES_EVERY_US).is_multiple_of(5) {
            self.sample_anchor_fit(&mut t);
        }
        self.metrics.research.tables = Some(t);
    }

    /// See `sample_tables`.
    fn sample_anchor_fit(&self, t: &mut crate::metrics::Tables) {
        let n = self.n_relays as usize;
        if self.relays.len() == n {
            return;
        }
        // Per sleepy node, the strongest running relay it hears and its
        // anchor's link, dBm.
        let mut best = vec![(f32::NEG_INFINITY, u32::MAX); self.relays.len() - n];
        let mut anchor = vec![f32::NEG_INFINITY; self.relays.len() - n];
        let anchors: Vec<Option<u32>> = (n..self.relays.len())
            .map(|s| self.attached_relay(s as u32))
            .collect();
        for a in 0..n {
            if !self.nodes[a].alive || self.relays[a].booting() {
                continue;
            }
            for l in self.links.row(a as u32) {
                let Some(k) = (l.rx as usize).checked_sub(n) else {
                    continue;
                };
                if (l.base_dbm as f64) < self.sens_dbm {
                    continue;
                }
                if l.base_dbm > best[k].0 {
                    best[k] = (l.base_dbm, a as u32);
                }
                if anchors[k] == Some(a as u32) {
                    anchor[k] = l.base_dbm;
                }
            }
        }
        for k in 0..best.len() {
            let s = n + k;
            let walks = self.walks.as_ref().and_then(|w| w.walker(s as u32));
            if anchors[k].is_none()
                || !self.nodes[s].alive
                || !self.nodes[s].present
                || walks.is_some()
            {
                continue;
            }
            let (dbm, b) = best[k];
            if b == u32::MAX {
                continue;
            }
            let id = self.relays[b as usize].id();
            let held = self.relays[s].neighbours().any(|e| e.id == id);
            t.anchors.0 += 1;
            t.anchors.1 += u64::from(anchors[k] == Some(b));
            t.anchors.2 += u64::from(held);
            let gap = (dbm - anchor[k]).max(0.0);
            t.anchor_gap_db[(gap as usize).min(30)] += 1;
        }
    }

    /// Once a minute after the settle time: sleepy nodes whose anchor would
    /// not hear their beat, on another channel or not in its heard table.
    fn sample_anchors(&mut self) {
        if self.now < self.settle || self.n_relays == 0 {
            return;
        }
        let homes = &mut self.metrics.research.homes;
        homes.resize(
            self.n_relays as usize,
            [0; bark_proto::radio::CHANNELS as usize],
        );
        for (i, h) in homes.iter_mut().enumerate() {
            let r = &self.relays[i];
            if self.nodes[i].alive && !r.booting() {
                h[r.home().index() as usize] += 1;
            }
        }
        let s = &mut self.metrics.sleep;
        s.samples += 1;
        for n in self.n_relays as usize..self.relays.len() {
            let (leaf, st) = (&self.relays[n], &self.nodes[n]);
            let Some(a) = leaf.anchor() else { continue };
            if !st.alive {
                continue;
            }
            let Some(&ai) = self.relay_index.get(&key(a.id)) else {
                continue;
            };
            let anchor = &self.relays[ai as usize];
            s.anchored += 1;
            let off = anchor.home() != leaf.home();
            let unheard = anchor.heard_nodes().get(leaf.id()).is_none();
            s.off_channel += u64::from(off);
            s.unheard += u64::from(unheard);
            let hears = self.nodes[ai as usize].alive && !off && !unheard;
            if let Some(k) = self.walks.as_ref().and_then(|w| w.walker(n as u32)) {
                use crate::metrics::MOVER_STOPS_S;
                let bin = if k.walking {
                    0
                } else {
                    let at = self.now.since(k.since).as_micros() / 1_000_000;
                    1 + MOVER_STOPS_S.iter().take_while(|&&s| at >= s).count()
                };
                let m = &mut self.metrics.research.movers[usize::from(k.route.is_some())][bin];
                m.0 += 1;
                m.1 += u64::from(hears);
            }
            if let Some((_, back @ None)) = self.orphans.get_mut(&(n as u32)) {
                if hears {
                    *back = Some(self.now);
                }
            }
        }
    }

    /// A node hears the beat of a sleepy node that came back into reach,
    /// and is its anchor, or any node if it has none: the network knows the
    /// sleepy node is back.
    fn note_beat(&mut self, receiver: u32, ev: &Event<'_>) {
        if self.leaf_nodes.is_empty() {
            return;
        }
        let Event::Received { bytes, .. } = ev else {
            return;
        };
        let Ok(bark_proto::wire::Packet::Heartbeat(hb)) = bark_proto::wire::Packet::decode(bytes)
        else {
            return;
        };
        if let Some(tags) = self.queued_for.get_mut(&(receiver, key(hb.id))) {
            let origins = &mut self.origins;
            tags.retain(|&t| origins[t as usize].fate == Fate::Unaccounted);
            for &t in tags.iter() {
                origins[t as usize].holder_beats += 1;
            }
        }
        let Some(&leaf) = self.leaf_nodes.get(&hb.id.0) else {
            return;
        };
        let anchor = self.relays[leaf as usize].anchor().map(|a| a.id);
        if anchor.is_some_and(|a| a != self.relays[receiver as usize].id()) {
            return;
        }
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
        if matches!(
            a,
            Action::Listen(_) | Action::Cad(_) | Action::Rssi(_) | Action::Transmit(_)
        ) {
            // Any use of the radio ends a sniff (core-interface §3 rule 10).
            self.wake(node);
        }
        // The core's instants are on its own clock; a script's are true.
        let true_time = |t: Instant| {
            if scripted {
                t
            } else {
                self.clocks[node as usize].true_time(t)
            }
        };
        match a {
            Action::Sniff(until) => self.sniff(node, true_time(until)),
            Action::Listen(c) => self.listen(node, c),
            Action::Cad(c) => self.check(node, ExKind::Cad, c, scripted),
            Action::Rssi(c) => self.check(node, ExKind::Rssi, c, scripted),
            Action::Transmit(f) => {
                self.note_direct(node, &f);
                self.note_sent_again(node, &f);
                self.note_beat_fix(node, &f, scripted);
                self.begin_excursion(node, ExKind::Transmit, f.channel, Some(f), scripted)
            }
            Action::App(e) => self.app_event(node, e),
            Action::Trace(Trace::Dropped { msg_id, reason }) => {
                self.message_dropped(node, msg_id, reason)
            }
            Action::Trace(t) => {
                let c = &mut self.metrics.nodes[node as usize].core;
                match t {
                    Trace::HomeChannelMoved { from, to } if !self.was_booting => {
                        c.home_moves += 1;
                        if node < self.n_relays {
                            let t = self.now.as_micros() as f64 / 1e6;
                            let m = (t, node, from.index(), to.index());
                            self.metrics.research.moves.push(m);
                        }
                    }
                    Trace::HeartbeatSkipped { channel } => {
                        c.heartbeats_skipped += 1;
                        let r = &self.relays[node as usize];
                        if r.sleepy() && r.anchor().is_some() && channel == r.home() {
                            let id = key(r.id());
                            for h in self.holders_waiting(id) {
                                self.watch_note(h, id, beat_fate::SKIPPED, false);
                            }
                        }
                    }
                    Trace::HeartbeatDeferred { .. } => c.heartbeats_deferred += 1,
                    Trace::Refused { msg_id, reason } => {
                        c.refused.add(reason);
                        self.note_refusal(node, msg_id, reason);
                    }
                    Trace::TurnedAway { .. } => c.turned_away += 1,
                    Trace::HandOverDeferred { .. } => c.handovers_deferred += 1,
                    Trace::Demoted { .. } => c.demoted += 1,
                    Trace::AnchorCheck => {
                        c.anchor_checks += 1;
                        if self.fixing && self.now >= self.settle {
                            self.metrics.sleep.fix_checks += 1;
                        }
                    }
                    Trace::AnchorWatch {
                        heard,
                        offset_us,
                        listened,
                    } if self.now >= self.settle => {
                        let s = &mut self.metrics.sleep;
                        s.watches[usize::from(!heard)] += 1;
                        s.watch_listen_us += listened.as_micros();
                        if heard && offset_us < 0 {
                            s.watch_early += 1;
                        }
                        if heard {
                            s.watch_earliest_us = s.watch_earliest_us.min(offset_us as i64);
                        }
                    }
                    Trace::Delivered { msg_id, by, .. } => {
                        c.delivered += 1;
                        self.first_try.remove(&(node, msg_id.0));
                        self.note_delivered_by(msg_id, by);
                    }
                    Trace::FarFix => self.note_far_fix(node),
                    Trace::Carried { carried } => self.note_carried(node, carried),
                    Trace::AnchorCarried => {
                        let after = self.now >= self.settle;
                        self.carried_metrics().anchors_left += u64::from(after);
                    }
                    Trace::Queued { msg_id, node: to } => {
                        if let Some(&tag) = self.msg_tags.get(&msg_id.0) {
                            let r = &self.relays[node as usize];
                            let dst = self.origins[tag as usize].dst as usize;
                            let w = &mut self.origins[tag as usize].watch;
                            w.queued_at = Some(self.now);
                            w.known = r.heard_nodes().get(to).is_some();
                            w.anchored = self.relays[dst].anchor().is_some_and(|a| a.id == r.id());
                            let row = self.links.row(dst as u32);
                            let to_relay = row.iter().filter(|l| l.rx < self.n_relays);
                            w.best_dbm = to_relay.map(|l| l.base_dbm).reduce(f32::max);
                            w.holder_dbm = row.iter().find(|l| l.rx == node).map(|l| l.base_dbm);
                            let holders = self.holders_of.entry(key(to)).or_default();
                            if !holders.contains(&node) {
                                holders.push(node);
                            }
                            self.origins[tag as usize].holder = Some(node);
                            self.queued_for
                                .entry((node, key(to)))
                                .or_default()
                                .push(tag);
                        }
                    }
                    Trace::DeliveryWaits {
                        msg_id,
                        node: to,
                        tried,
                    } => {
                        if tried {
                            c.deliveries_failed += 1;
                        } else {
                            c.deliveries_late += 1;
                        }
                        self.delivery_waits(node, msg_id.0, key(to), tried)
                    }
                    Trace::Forwarded { hops, .. } => {
                        c.handed_over += 1;
                        c.originated_out += u64::from(hops == 1);
                    }
                    Trace::HandOverFailed { msg_id, to } => {
                        c.failed += 1;
                        let had = self
                            .relay_index
                            .get(&key(to))
                            .is_some_and(|&r| self.relays[r as usize].knows(msg_id));
                        c.failed_taken += u64::from(had);
                    }
                    Trace::Copy {
                        returned, merged, ..
                    } => {
                        if returned {
                            c.returns_in += 1;
                        } else {
                            c.copies += 1;
                            c.merged += u64::from(merged);
                        }
                    }
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

    /// What the nodes that move do (roadmap D70), from the first event
    /// that shows one.
    fn carried_metrics(&mut self) -> &mut crate::metrics::Carried {
        self.metrics
            .research
            .carried
            .get_or_insert_with(Default::default)
    }

    /// `relay` had the first far fix of a run: what it holds for its
    /// sleepy nodes is flushed.
    fn note_far_fix(&mut self, relay: u32) {
        let walks = self.walks.as_ref().is_some_and(|w| w.walks(relay));
        let t = self.now;
        let again = self.far_at.get(&relay).is_some_and(|&(_, a)| a);
        self.far_at.insert(relay, (t, again));
        let s = t.as_micros() as f64 / 1e6;
        self.carried_metrics().far_fixes.push((s, relay, walks));
        if t >= self.settle {
            let held: Vec<u32> = self.relays[relay as usize]
                .delivery_queue()
                .map(|(id, _)| id.0)
                .collect();
            for id in held {
                self.held_far.entry(id).or_insert((again, None));
            }
        }
    }

    /// `relay` turned carried, or fixed again.
    fn note_carried(&mut self, relay: u32, carried: bool) {
        let walks = self.walks.as_ref().is_some_and(|w| w.walks(relay));
        let t = self.now;
        let s = t.as_micros() as f64 / 1e6;
        if carried {
            let since = self.far_at.get(&relay).map_or(t, |&(f, _)| f);
            let run = t.since(since).as_micros() as f64 / 1e6;
            self.carried_metrics().carried.push((s, relay, walks, run));
        } else {
            self.far_at.entry(relay).or_insert((t, true)).1 = true;
            self.carried_metrics().fixed.push((s, relay, walks));
        }
    }

    /// A holder delivered `msg_id` to its node `by` that way.
    fn note_delivered_by(&mut self, msg_id: MsgId, by: bark_proto::Delivery) {
        use bark_proto::Delivery as D;
        let (i, fate) = match by {
            D::Window => (0, 1),
            D::Long => (1, 1),
            D::Flush => (2, 0),
            D::Relay => (3, 2),
        };
        if self.now >= self.settle {
            self.delivered_by[i] += 1;
        }
        if matches!(by, D::Flush | D::Relay) {
            self.carried_metrics();
        }
        if let Some((_, f)) = self.held_far.get_mut(&msg_id.0) {
            f.get_or_insert(fate);
        }
    }

    /// `node` sends `f`: if it is the flush or a delivery to a node as a
    /// relay, count its airtime.
    fn note_direct(&mut self, node: u32, f: &Frame) {
        if node >= self.n_relays || !self.relays[node as usize].sending_direct() {
            return;
        }
        let after = self.now >= self.settle;
        let air = time_on_air(f.len, f.preamble_symbols).as_micros() as f64 / 1e6;
        let c = self.carried_metrics();
        let to = if f.preamble_symbols >= LONG_PREAMBLE_SYMBOLS {
            &mut c.flush
        } else {
            &mut c.to_relay
        };
        if after {
            to.0 += 1;
            to.1 += air;
        }
    }

    /// The fates of the texts held at a first far fix
    /// (`metrics::Carried::held`).
    pub(crate) fn held_fates(&self) -> [[u64; 5]; 2] {
        let mut held = [[0; 5]; 2];
        for (id, &(again, fate)) in &self.held_far {
            let got = self
                .msg_tags
                .get(id)
                .is_some_and(|&tag| matches!(self.origins[tag as usize].fate, Fate::Delivered));
            let i = fate.unwrap_or(if got { 4 } else { 3 });
            held[usize::from(again)][i] += 1;
        }
        held
    }

    /// Deliveries by the way they went (`metrics::Carried::delivered_by`).
    pub(crate) fn delivered_by(&self) -> [u64; 4] {
        self.delivered_by
    }

    /// A delivery by `holder` of `msg` to the node `to` waits for its next
    /// beat: count it by its window and its first DATA.
    fn delivery_waits(&mut self, holder: u32, msg: u32, to: u32, tried: bool) {
        let first = self.first_try.remove(&(holder, msg));
        let Some(&n) = self.sleepy_index.get(&to) else {
            return;
        };
        if self.now < self.settle {
            return;
        }
        let (window, how) = match (first, tried) {
            (Some(f), _) => f,
            (None, t) => {
                let how = if t {
                    FirstTry::NotSent
                } else {
                    FirstTry::NotStarted
                };
                (self.window_of(n), how)
            }
        };
        self.metrics.sleep.waited[window][how as usize] += 1;
    }

    /// The window the sleepy node `n` listens in now: after its beat, or
    /// after a message it took since.
    pub(crate) fn window_of(&self, n: u32) -> usize {
        let (beat, took) = self.beat_took[(n - self.n_relays) as usize];
        if took > beat {
            AFTER_TAKE
        } else {
            AFTER_BEAT
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
        let Some(&sender) = self.relay_index.get(&from) else {
            return;
        };
        let me = self.relays[node as usize].id();
        let lowest = self.relays[node as usize].lowest_free_hop_place();
        let then = self.clocks[sender as usize].local(self.now);
        let held = self.relays[sender as usize].highest_count_for(me, then);
        let with = matches!((lowest, held), (Some(k), Some(h)) if h >= k);
        let h = &mut self.metrics.hop_places;
        h.refusals += 1;
        h.with_place += u64::from(with);
        self.refused_at.insert((sender, id), (self.now, with));
    }

    /// `node` takes a GPS fix: where it is, or where a fault says. With
    /// the checks on (spec "Position and time"), a fix farther from the
    /// last one handed to the core than the node can have moved since,
    /// 50 m for a relay and the carried speed's worth more for a sleepy
    /// node, or farther than the longest hearing radius from every
    /// neighbour it keeps, is not handed over. The check of the satellite
    /// signal-to-noise cannot be modelled: the simulator has no satellites.
    fn fix(&mut self, n: u32) {
        if !self.nodes[n as usize].alive {
            return;
        }
        let mut at = self.place(n);
        let fault = self.gps_fault[n as usize].filter(|f| self.now >= f.0);
        if let Some((from, kind, size, dir)) = fault {
            let off = match kind {
                crate::scenario::FaultKind::Jump => size,
                crate::scenario::FaultKind::Drift => {
                    size * self.now.since(from).as_micros() as f64 / 1e6
                }
            };
            at = Xy {
                x: at.x + dir.x * off,
                y: at.y + dir.y * off,
            };
        }
        if let Some((e, last, r)) = self.gps_error.get_mut(n as usize) {
            // A Gauss-Markov step from the last draw (`scenario::Gps`).
            let tau = self.gps.error_correlation_s.max(1e-9);
            let rho = (-(self.now.since(*last).as_micros() as f64 / 1e6) / tau).exp();
            let fresh = self.gps.error_m * (1.0 - rho * rho).sqrt();
            e.x = e.x * rho + fresh * r.normal();
            e.y = e.y * rho + fresh * r.normal();
            *last = self.now;
            at = Xy {
                x: at.x + e.x,
                y: at.y + e.y,
            };
        }
        let position = self.origin.position(at);
        let k = usize::from(n >= self.n_relays);
        if self.gps.checks {
            // Two fixes of a node that has not moved may differ by this
            // much (spec "Position and time").
            const STILL_M: f64 = 50.0;
            // The longest hearing radius, rooftop to rooftop (spec
            // "Physical layer").
            const HEARING_M: f32 = 3000.0;
            // Any node may be carried (roadmap D70); a fixed one keeps its
            // position in its core.
            let (last, when) = self.accepted[n as usize];
            let moved = STILL_M
                + self.gps.carried_speed_m_s * self.now.since(when).as_micros() as f64 / 1e6;
            let plane = bark_proto::geo::LocalPlane::new(position);
            let far = |p: Position| plane.point(p).distance_m(Default::default()) > HEARING_M;
            let mut heard = self.relays[n as usize]
                .neighbours()
                .map(|e| e.position)
                .peekable();
            let alone = heard.peek().is_some() && heard.all(far);
            if at.dist(last) > moved {
                if fault.is_some() {
                    self.metrics.research.fixes[k][1] += 1;
                }
                return;
            }
            if alone {
                if fault.is_some() {
                    self.metrics.research.fixes[k][2] += 1;
                }
                return;
            }
        }
        if fault.is_some() {
            self.metrics.research.fixes[k][0] += 1;
        }
        self.accepted[n as usize] = (at, self.now);
        self.fixing = true;
        self.deliver(n, Event::App(AppCommand::Fix { position }));
        self.fixing = false;
    }

    /// `node` sends `f`: if it walks, or its GPS has a fault, and `f` is
    /// its beat, it takes a GPS fix as it goes (spec "Beats"), which its
    /// core is handed next.
    fn note_beat_fix(&mut self, node: u32, f: &Frame, scripted: bool) {
        let walks = self.walks.as_ref().is_some_and(|w| w.walks(node));
        let faulty = node >= self.n_relays && self.gps_fault[node as usize].is_some();
        let own_fixes = node < self.n_relays && self.gps.relays_every_s > 0.0;
        if scripted || !(walks || faulty) || own_fixes {
            return;
        }
        if let Ok(bark_proto::wire::Packet::Heartbeat(_)) =
            bark_proto::wire::Packet::decode(f.payload())
        {
            self.push(self.now, Ev::Fix(node));
        }
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
        let receiving = match &st.lock {
            Lock::Rx(r) if !scripted && deaf => Some(r.end),
            _ => None,
        };
        if let Some(end) = receiving {
            if st.alive && st.ex.is_none() && st.held.is_none() {
                let longest = time_on_air(MAX_FRAME as u8, LONG_PREAMBLE_SYMBOLS);
                st.held = Some((kind, c));
                st.held_gen = st.held_gen.wrapping_add(1);
                let g = st.held_gen;
                self.push(end.min(now + longest), Ev::Held(node, g));
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
        // Only a broken rule needs the name.
        let label = || self.label(node);
        let st = &self.nodes[node as usize];
        if !st.alive {
            return;
        }
        if st.held.is_some() && !scripted {
            panic!(
                "{}: core started {kind:?} while a held check is outstanding \
                 (docs/core-interface.md §3 rule 1: one excursion outstanding)",
                label()
            );
        }
        if let Some(ex) = &st.ex {
            let who = if scripted { "scripted event" } else { "core" };
            panic!(
                "{}: {who} started {kind:?} on channel {} while a {}{:?} excursion is outstanding \
                 (docs/core-interface.md §3 rule 1: one excursion outstanding)",
                label(),
                c.index(),
                if ex.scripted { "scripted " } else { "" },
                ex.kind
            );
        }
        if let Some(f) = &frame {
            assert!(
                f.preamble_symbols >= MIN_PREAMBLE_SYMBOLS,
                "{}: transmit with a {}-symbol preamble, below the {MIN_PREAMBLE_SYMBOLS}-symbol minimum \
                 (docs/core-interface.md §3 rule 4)",
                label(),
                f.preamble_symbols
            );
            assert!(
                f.len as usize <= MAX_FRAME,
                "{}: frame longer than MAX_FRAME",
                label()
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
                    extra: Vec::new(),
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
    /// nowhere else: a message for this node's application, or its own
    /// message given back.
    fn app_event(&mut self, node: u32, e: AppEvent) {
        let (len, payload) = match e {
            AppEvent::Message { len, payload, .. } => (len, payload),
            AppEvent::GivenBack { msg_id, retry_at } => {
                if !self.give_back(node, msg_id, retry_at) {
                    self.mark_dropped(node, msg_id, DropReason::Pushback);
                }
                return;
            }
        };
        let m = &mut self.metrics;
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
            Some(o) if self.ids[o.dst as usize] == self.ids[node as usize] => {
                m.app.misdelivered += 1;
                m.research.to_twin += 1;
                o.to_twin = true;
            }
            _ => m.app.misdelivered += 1,
        }
    }

    /// A core dropped message `msg_id` for `reason`, reported in
    /// `Trace::Dropped`. The protocol sends no failure notice, so this is
    /// the only record of where a lost message died.
    pub fn message_dropped(&mut self, node: u32, msg_id: MsgId, reason: DropReason) {
        self.metrics.app.drops.add(reason);
        self.metrics.nodes[node as usize].drops.add(reason);
        if reason == DropReason::Pushback {
            // The message is its application's again, which decides its
            // fate on the `AppEvent::GivenBack` that follows.
            return;
        }
        let dst = self
            .msg_tags
            .get(&msg_id.0)
            .and_then(|&t| self.origins.get(t as usize))
            .map(|o| o.dst);
        if let (DropReason::QueueExpired, Some(dst), true) = (reason, dst, self.now >= self.settle)
        {
            let me = self.relays[node as usize].id();
            let anchor = self.relays[dst as usize].anchor().map(|a| a.id);
            let k = match (anchor == Some(me), self.nodes[dst as usize].present) {
                (true, true) => 0,
                (true, false) => 1,
                (false, _) => 2,
            };
            self.metrics.sleep.queue_expired[k] += 1;
        }
        self.mark_dropped(node, msg_id, reason);
    }

    /// Message `msg_id` ended at `node` for `reason`, unless it was
    /// delivered already.
    fn mark_dropped(&mut self, node: u32, msg_id: MsgId, reason: DropReason) {
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

    /// A frame of message `id` went on the air from `src`: it carried
    /// `hops` (DATA) and took `air_us`.
    /// A DATA of the message `msg` went from `src` with the long preamble.
    pub(crate) fn note_long_try(&mut self, msg: u32, src: u32) {
        if let Some(&tag) = self.msg_tags.get(&msg) {
            let o = &mut self.origins[tag as usize];
            if o.holder == Some(src) {
                o.watch.long_tries += 1;
            }
        }
    }

    /// The holders with messages for the node `id` not yet accounted for.
    pub(crate) fn holders_waiting(&mut self, id: u32) -> Vec<u32> {
        let Some(holders) = self.holders_of.get_mut(&id) else {
            return Vec::new();
        };
        let (queued, origins) = (&self.queued_for, &self.origins);
        holders.retain(|&h| {
            queued.get(&(h, id)).is_some_and(|tags| {
                tags.iter()
                    .any(|&t| origins[t as usize].fate == Fate::Unaccounted)
            })
        });
        holders.clone()
    }

    /// Count a beat of the node `id` with `fate` at `holder` for each
    /// message the holder holds for it.
    pub(crate) fn watch_note(&mut self, holder: u32, id: u32, fate: usize, elsewhere: bool) {
        if let Some(tags) = self.queued_for.get(&(holder, id)) {
            for &t in tags {
                let o = &mut self.origins[t as usize];
                if o.fate == Fate::Unaccounted {
                    o.watch.beats[fate] += 1;
                    o.watch.elsewhere += u32::from(elsewhere);
                }
            }
        }
    }

    pub(crate) fn message_frame(
        &mut self,
        id: MsgId,
        hops: u8,
        air_us: u64,
        src: u32,
        kind: FrameKind,
        time_left: Option<Duration>,
    ) {
        let Some(&tag) = self.msg_tags.get(&id.0) else {
            return;
        };
        let Some(o) = self.origins.get_mut(tag as usize) else {
            return;
        };
        o.frames += 1;
        o.airtime_us += air_us;
        o.hops = o.hops.max(hops);
        if time_left.is_some() {
            o.time_left = time_left;
            if o.holder == Some(src) {
                o.holder_tries += 1;
            }
        }
        let kind = match kind {
            FrameKind::Data if src == o.src => 0,
            FrameKind::Data => {
                o.entered_at.get_or_insert(self.now);
                1
            }
            FrameKind::Taken => 2,
            FrameKind::Refused => 3,
        };
        if let Some(mm) = &mut self.msg_minutes {
            let minute = (self.now.as_micros() / 60_000_000) as u32;
            mm.entry((minute, tag)).or_default()[kind] += air_us;
        }
    }

    /// Keep, per minute and message, the airtime of its frames by
    /// [`FrameKind`] (`bark-sim --message-minutes FILE`).
    pub fn message_minutes_on(&mut self) {
        self.msg_minutes = Some(HashMap::new());
    }

    /// `minute,tag,origin_data_us,data_us,ack_us,refused_us`, sorted:
    /// DATA from the message's origin, DATA from any other node, ACKs that
    /// take it and ACKs that refuse it.
    pub fn message_minutes_csv(&self) -> Option<String> {
        let mm = self.msg_minutes.as_ref()?;
        let mut rows: Vec<_> = mm.iter().collect();
        rows.sort_unstable_by_key(|(k, _)| **k);
        let mut s = String::from("minute,tag,origin_data_us,data_us,ack_us,refused_us\n");
        for ((m, t), a) in rows {
            s.push_str(&format!("{m},{t},{},{},{},{}\n", a[0], a[1], a[2], a[3]));
        }
        Some(s)
    }

    /// Whether relay `r`'s neighbour table holds `id`, and how many entries
    /// it has.
    pub(crate) fn table_holds(&self, r: u32, id: NodeId) -> (bool, usize) {
        let t = self.relays[r as usize].neighbours();
        t.fold((false, 0), |(held, n), e| (held || e.id == id, n + 1))
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
                distance_m: o.distance_m,
                dst_walking: o.dst_walking,
                taken_s: o.taken_at.map(|d| d.since(t).as_micros() as f64 / 1e6),
                dropped_s: o.dropped_at.map(|d| d.since(t).as_micros() as f64 / 1e6),
                src_relay: o.src_relay,
                dst_relay: o.dst_relay,
                to_twin: o.to_twin,
                src_moving: o.src_moving,
                dst_anchor: o.dst_anchor,
                entered_s: o.entered_at.map(|d| d.since(t).as_micros() as f64 / 1e6),
                time_left_s: o.time_left.map(|d| d.as_micros() as f64 / 1e6),
                holder_beats: o.holder_beats,
                holder_tries: o.holder_tries,
                watch: o.watch,
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
            "tag,src,dst,t_s,fate,drop_node,drop_reason,reachable,latency_s,frames,airtime_s,hops,distance_m,taken_s,dropped_s,dst_walking,src_relay,dst_relay,to_twin,src_moving,dst_anchor,src_x,dst_x,entered_s,time_left_s,holder_beats,holder_tries,queued_s,holder_knew,holder_anchor,holder_long,holder_dbm,best_dbm",
        );
        for n in beat_fate::NAMES {
            s.push_str(",b_");
            s.push_str(n);
        }
        s.push_str(",b_elsewhere\n");
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
            let entered = r.entered_s.map(|l| format!("{l:.6}")).unwrap_or_default();
            let left = r.time_left_s.map(|l| format!("{l:.1}")).unwrap_or_default();
            let relay = |r: Option<u32>| r.map(|r| r.to_string()).unwrap_or_default();
            s.push_str(&format!(
                "{},{},{},{:.6},{fate},{node},{reason},{},{latency},{},{:.6},{},{:.1},{taken},{dropped},{},{},{},{},{},{},{:.0},{:.0},{entered},{left},{},{}\n",
                r.tag,
                r.src,
                r.dst,
                r.t_s,
                r.reachable,
                r.frames,
                r.airtime_s,
                r.hops,
                r.distance_m,
                r.dst_walking,
                relay(r.src_relay),
                relay(r.dst_relay),
                r.to_twin,
                r.src_moving,
                relay(r.dst_anchor),
                self.xy(r.src).x,
                self.xy(r.dst).x,
                r.holder_beats,
                r.holder_tries
            ));
            s.pop();
            let w = &r.watch;
            let queued = w
                .queued_at
                .map(|q| format!("{:.6}", q.as_micros() as f64 / 1e6 - r.t_s))
                .unwrap_or_default();
            s.push_str(&format!(
                ",{queued},{},{},{}",
                w.known, w.anchored, w.long_tries
            ));
            let dbm = |d: Option<f32>| d.map(|d| format!("{d:.1}")).unwrap_or_default();
            s.push_str(&format!(",{},{}", dbm(w.holder_dbm), dbm(w.best_dbm)));
            for b in w.beats {
                s.push_str(&format!(",{b}"));
            }
            s.push_str(&format!(",{}\n", w.elsewhere));
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

    /// A sleepy node's anchor now, as a node index.
    pub fn attached_relay(&self, leaf: u32) -> Option<u32> {
        let a = self.relays[leaf as usize].anchor()?;
        self.relay_index.get(&key(a.id)).copied()
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
        let waiting = self
            .offers
            .iter()
            .enumerate()
            .filter(|(i, _)| alive(*i))
            .filter_map(|(_, o)| o.as_ref().map(|o| o.tag));
        relays
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
            relay_at(self.relays[src as usize].address())
        };
        // A relay that walks has no stored links to judge by.
        let walks = |r: u32| self.walks.as_ref().is_some_and(|w| w.walks(r));
        match (from, relay_at(dst)) {
            (Some(a), Some(b)) if !walks(a) && !walks(b) => {
                self.component[a as usize] == self.component[b as usize]
            }
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
                if self.now >= self.settle {
                    self.metrics.sleep.anchor_changes += 1;
                }
            } else {
                self.first_anchor[node as usize] = Some(self.now);
            }
            *last = Some(p);
        }
    }

    /// The address of `node` as a sender knows it (spec "Addresses"): a
    /// relay's own position; for a leaf, the relay it is attached to, or
    /// the one it was last attached to, as the sender would have learnt
    /// it from an earlier message; for a leaf without an anchor past its
    /// boot listen, its own position. `None` for a leaf that has had
    /// neither: nobody can have its address.
    fn address_of(&mut self, node: u32) -> Option<Position> {
        if node < self.n_relays {
            return Some(self.relay_position(node));
        }
        let leaf = &self.relays[node as usize];
        if self.n_relays == 0 {
            // No node that is not sleepy: a sleepy node's own position.
            return Some(leaf.address());
        }
        if leaf.anchorless() {
            return Some(leaf.address());
        }
        if let Some(a) = leaf.anchor() {
            self.address[node as usize] = Some(a.position);
        }
        self.address[node as usize]
    }

    /// Where a relay says it is: where it was placed, or for one that
    /// walks, the position its core holds.
    fn relay_position(&self, relay: u32) -> Position {
        if self.walks.as_ref().is_some_and(|w| w.walks(relay)) {
            self.relays[relay as usize].config().position
        } else {
            self.positions[relay as usize]
        }
    }

    /// When a node was switched on, if it has been.
    pub fn switched_on(&self, node: u32) -> Option<Instant> {
        let i = node as usize;
        (!self.pending_boot[i] && self.nodes[i].alive).then_some(self.boot_at[i])
    }

    /// Where a node is now.
    pub fn place(&self, node: u32) -> Xy {
        match self.walks.as_ref().and_then(|w| w.walker(node)) {
            Some(k) => k.at(self.now),
            None => self.xy[node as usize],
        }
    }

    /// A node's ID.
    pub fn node_id(&self, node: u32) -> NodeId {
        self.ids[node as usize]
    }

    /// The traffic model has a message from `src` to `dst`. Its
    /// application holds one at a time and hands it to the core the moment
    /// the core takes one (core-interface §4); a message due while the last
    /// still waits is skipped. Latency counts from here, so a busy network
    /// shows as latency.
    fn originate(&mut self, src: u32, dst: u32, len: u8) {
        let st = &self.nodes[src as usize];
        if !st.alive || !st.present {
            return;
        }
        if self.offers[src as usize].is_some() {
            self.metrics.app.skipped += 1;
            self.skipped_at.push(self.now);
            return;
        }
        let Some(dst_pos) = self.address_of(dst) else {
            self.metrics.app.unaddressed += 1;
            return;
        };
        let tag = self.origins.len() as u32;
        let reachable = self.reachable(src, dst_pos);
        let src_pos = if src < self.n_relays {
            self.relay_position(src)
        } else {
            self.relays[src as usize].address()
        };
        let relay_at = |p: Position| (0..self.n_relays).find(|&r| self.positions[r as usize] == p);
        let (src_relay, dst_relay) = (relay_at(src_pos), relay_at(dst_pos));
        self.origins.push(OriginRecord {
            src,
            dst,
            t: self.now,
            fate: Fate::Unaccounted,
            reachable,
            delivered_at: None,
            taken_at: None,
            dropped_at: None,
            entered_at: None,
            time_left: None,
            holder: None,
            holder_beats: 0,
            holder_tries: 0,
            watch: HolderWatch::default(),
            frames: 0,
            airtime_us: 0,
            hops: 0,
            distance_m: self.place(src).dist(self.place(dst)),
            dst_walking: self
                .walks
                .as_ref()
                .and_then(|w| w.walker(dst))
                .is_some_and(|k| k.walking),
            src_relay,
            dst_relay,
            to_twin: false,
            src_moving: self
                .walks
                .as_ref()
                .and_then(|w| w.walker(src))
                .is_some_and(|k| k.walking),
            dst_anchor: (dst >= self.n_relays)
                .then(|| self.attached_relay(dst))
                .flatten(),
        });
        self.metrics.app.originated += 1;
        self.metrics.nodes[src as usize].originated += 1;
        self.offers[src as usize] = Some(Offer {
            tag,
            dst_pos,
            dst_id: self.node_id(dst),
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

    /// A core gave its application's message back with a wait: the
    /// application offers it again, as long as it is younger than the TTL
    /// the network would have kept it for. Whether it did.
    fn give_back(&mut self, node: u32, msg_id: MsgId, retry_at: Instant) -> bool {
        let Some(&tag) = self.msg_tags.get(&msg_id.0) else {
            return false;
        };
        let patience = self.relays[node as usize].config().delivery_ttl;
        let local = self.clocks[node as usize].local(self.now);
        let wait = retry_at.since(local);
        let o = &self.origins[tag as usize];
        // Told to wait past its patience, the application gives up at once.
        if o.src != node
            || self.now.since(o.t) + wait >= patience
            || self.offers[node as usize].is_some()
        {
            return false;
        }
        let Some(offer) = self.offered.get(&tag).copied() else {
            return false;
        };
        self.offers[node as usize] = Some(offer);
        true
    }

    /// Hand `src`'s waiting message to its core, if the core takes one now.
    fn offer(&mut self, src: u32) {
        let now = self.now;
        let local = self.clocks[src as usize].local(now);
        let ready = self.nodes[src as usize].alive && self.relays[src as usize].accepts_send(local);
        if !ready {
            return;
        }
        let Some(o) = self.offers[src as usize].take() else {
            return;
        };
        self.offered.insert(o.tag, o);
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
        let id = MsgId::of(o.dst_pos, o.dst_id, payload);
        if self.msg_tags.insert(id.0, o.tag).is_some() {
            self.metrics.app.shared_ids += 1;
        }
        let cmd = AppCommand::Send {
            dst_pos: o.dst_pos,
            dst_id: o.dst_id,
            payload,
        };
        self.deliver(src, Event::App(cmd));
    }

    fn poisson_arrival(&mut self, n: u32) {
        let li = self.person_of[n as usize] as usize;
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
            self.originate(n, self.people[d as usize], len);
        }
        self.push(self.now + gap, Ev::Poisson(n));
    }

    // ---- Scripted events -------------------------------------------------

    fn run_script(&mut self, k: u32) {
        let e = &self.script[k as usize];
        let (node, action, channel, len, preamble) =
            (e.node, e.action, e.channel, e.len, e.preamble);
        if matches!(action, ScriptAction::Remove | ScriptAction::Restart) && node < self.n_relays {
            let id = self.relays[node as usize].id();
            for n in self.n_relays..self.node_count() as u32 {
                if self.relays[n as usize].anchor().is_some_and(|a| a.id == id) {
                    self.orphans.entry(n).or_insert((self.now, None));
                }
            }
        }
        match action {
            ScriptAction::Remove => self.remove(node),
            ScriptAction::Restart => self.restart(node),
            ScriptAction::Start => self.start(node),
            ScriptAction::Sniff => self.apply(node, Action::Sniff(self.end), true),
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

    /// Power on a node that has been off since the start: it boots.
    pub fn start(&mut self, node: u32) {
        if self.nodes[node as usize].alive {
            return;
        }
        self.nodes[node as usize].alive = true;
        self.boot_at[node as usize] = self.now;
        if let Some(mean_up) = self.availability.map(|a| a.0) {
            if node >= self.n_relays {
                let li = (node - self.n_relays) as usize;
                let first = secs(self.avail_rng[li].exponential(mean_up));
                self.push(self.now + first, Ev::Availability(node));
            }
        }
        self.power_on(node);
    }

    /// Power-on is delivered as a wakeup (docs/core-interface.md §2), and
    /// then the power the node has (§4).
    fn power_on(&mut self, node: u32) {
        self.deliver(node, Event::Wakeup);
        let external = self.external(node);
        self.deliver(node, Event::App(AppCommand::Power { external }));
    }

    /// Whether `node` is on external power now: the nodes placed as relays.
    fn external(&self, node: u32) -> bool {
        node < self.n_relays
    }

    /// Power a node off and on again: it boots afresh with every table
    /// lost and its ID and position kept. Its
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
        let r = &mut self.relays[node as usize];
        *r = Relay::new(*r.config(), seed);
        self.power_on(node);
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
        self.wake(node);
        let st = &mut self.nodes[node as usize];
        st.alive = false;
        st.resting = None;
        st.wake_at = None;
        self.wakeups.set(node, None);
        let st = &mut self.nodes[node as usize];
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
            let moving = std::mem::take(&mut i.moving);
            for &(n, _) in reach.iter().chain(&moving) {
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
            if self.walks.is_some() {
                self.interferer_reaches_walkers(k);
                self.push(now + crate::walk::REGRID, Ev::InterfererMoves(k, g));
            }
        }
    }

    /// Interferer `k`, which is on, reaches the walkers where they are
    /// now. Looked at again as often as the walkers' grids are built, a
    /// walk of 14 m, which moves the path loss of a walker 300 m away by
    /// under a decibel; a stop that begins or ends between two looks
    /// keeps its wall or its lack of one until the next.
    fn interferer_reaches_walkers(&mut self, k: u32) {
        let i = &self.interferers[k as usize];
        let (key, end, ch, lora) = (i.key, i.end, i.channel, i.lora);
        let floor = self.radio.cutoff_dbm();
        let now = self.now;
        let new = self
            .walks
            .as_ref()
            .expect("walkers")
            .reached_from(i.at, i.power_dbm, floor, now);
        let old = std::mem::replace(&mut self.interferers[k as usize].moving, new.clone());
        for (n, _) in old {
            self.remove_arrival(n, key);
        }
        for (n, dbm) in new {
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
    }

    fn availability_toggle(&mut self, n: u32) {
        let (mean_up, mean_dark) = self.availability.expect("availability configured");
        let li = (n - self.n_relays) as usize;
        if !self.nodes[n as usize].alive {
            return;
        }
        let dark = !self.nodes[n as usize].avail_dark;
        self.nodes[n as usize].avail_dark = dark;
        let next = if dark {
            self.avail_rng[li].exponential(mean_dark)
        } else {
            self.avail_rng[li].exponential(mean_up)
        };
        self.set_presence(n);
        self.push(self.now + secs(next), Ev::Availability(n));
    }

    /// `n` is in reach unless its availability process or a group has it
    /// dark: bring its radio in or out of reach to match.
    fn set_presence(&mut self, n: u32) {
        let st = &self.nodes[n as usize];
        let present = !st.avail_dark && st.group_dark == 0;
        if present == st.present || !st.alive {
            return;
        }
        if let Some(t) = &mut self.tracer {
            t.note(self.now, n, if present { "back" } else { "dark" });
        }
        let back = (n >= self.n_relays)
            .then(|| self.back_at.get_mut((n - self.n_relays) as usize))
            .flatten();
        if present {
            // Back in reach. Signals already on the air are not added: the
            // error is at most one packet's worth of interference.
            if let Some(b) = back {
                *b = Some(self.now);
            }
            self.nodes[n as usize].present = true;
            self.metrics.returns.returned += 1;
        } else {
            // Going dark: out of reach of everything now on the air.
            if let Some(b) = back {
                *b = None;
            }
            self.abort_lock(n, Deaf::Absent);
            self.clear_arrivals(n);
            self.nodes[n as usize].present = false;
        }
    }

    // ---- Helpers for the reception model --------------------------------

    /// Abort a reception in progress at `node`, recording why.
    pub(crate) fn abort_lock(&mut self, node: u32, why: Deaf) {
        self.settle(node);
        let st = &mut self.nodes[node as usize];
        if let Lock::Rx(r) = st.lock {
            self.metrics.outcome(node, r.channel, Outcome::Deaf(why));
            st.lock = Lock::Idle;
            self.note_beat_loss(r.key, node, Outcome::Deaf(why));
        }
        self.nodes[node as usize].lock = Lock::Idle;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A clock off by 50 ppm reads 50 µs more or less a second, and a
    /// timer set on it fires at the first true microsecond it reads the
    /// time set, never before.
    #[test]
    fn clocks_run_at_their_rate_and_timers_never_fire_early() {
        for ppm in [50.0, -50.0, 12.345, -0.001] {
            let c = Clock::off_by(ppm);
            let second = Instant::from_micros(1_000_000);
            let off = c.local(second).as_micros() as i64 - 1_000_000;
            assert_eq!(off, ppm.floor() as i64, "{ppm} ppm");
            for l in [0u64, 1, 999, 1_000_000, 3_600_000_000, 123_456_789_012] {
                let l = Instant::from_micros(l);
                let t = c.true_time(l);
                assert!(c.local(t) >= l, "{ppm} ppm: fires early for {l:?}");
                if t > Instant::ZERO {
                    let before = Instant::from_micros(t.as_micros() - 1);
                    assert!(c.local(before) < l, "{ppm} ppm: fires late for {l:?}");
                }
            }
        }
        let c = Clock::default();
        assert_eq!(c.local(Instant::from_micros(7)), Instant::from_micros(7));
        assert_eq!(c.true_time(Instant::MAX), Instant::MAX);
    }

    /// The closed forms meet their definitions: what the clock reads, and
    /// a timer that fires at the first microsecond the clock reads its
    /// time; and the 64-bit ratio is the 128-bit one, up to where the
    /// product no longer fits.
    #[test]
    fn clock_closed_forms_are_the_definitions() {
        let reads =
            |c: Clock, t: i128| (t * (1_000_000_000 + c.ppb as i128)).div_euclid(1_000_000_000);
        for ppm in [50.0, -50.0, 12.345, -0.001, 999.0, -999.0] {
            let c = Clock::off_by(ppm);
            let far = [
                999_999,
                1_000_000_001,
                3_600_000_000,
                123_456_789_012,
                (1 << 62) + 12_345,
            ];
            for l in (0..3000).chain(far) {
                let local = c.local(Instant::from_micros(l)).as_micros() as i128;
                assert_eq!(local, reads(c, l as i128), "{ppm} ppm at {l}");
                let t = c.true_time(Instant::from_micros(l)).as_micros() as i128;
                assert!(reads(c, t) >= l as i128, "{ppm} ppm: early for {l}");
                assert!(
                    t == 0 || reads(c, t - 1) < l as i128,
                    "{ppm} ppm: late for {l}"
                );
            }
        }
        for b in [1, -1, 50_000, -50_000, 12_345, i64::MAX, i64::MIN] {
            for a in [
                0,
                1,
                999_999_999,
                1_000_000_001,
                3_600_000_000,
                u64::MAX as i128,
            ] {
                for a in [a, -a] {
                    let d = 1_000_000_007;
                    let wide = (a * b as i128).div_euclid(d as i128);
                    assert_eq!(floor_ratio(a, b, d), wide, "{a} × {b}");
                }
            }
        }
    }

    /// Random IDs are distinct, never zero, the same for a seed, and share
    /// last bytes at the chance rate: n nodes have n² / 512 pairs on one
    /// byte, where the sequence spreads them evenly.
    #[test]
    fn random_ids_share_last_bytes_by_chance() {
        let n = 2048;
        let ids = random_ids(7, n);
        assert_eq!(ids, random_ids(7, n));
        assert_ne!(ids, random_ids(8, n));
        let set: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(set.len(), n);
        assert!(!set.contains(&NodeId::NONE));
        let pairs = |ids: &[NodeId]| {
            let mut count = [0u64; 256];
            for id in ids {
                count[id.0[3] as usize] += 1;
            }
            count
                .iter()
                .map(|c| c * c.saturating_sub(1) / 2)
                .sum::<u64>()
        };
        // Expected 2048 × 2047 / 512 = 8188, standard deviation about 370.
        let chance = pairs(&ids);
        assert!((7100..9300).contains(&chance), "{chance} pairs");
        let sequence: Vec<NodeId> = (0..n as u32).map(node_id).collect();
        assert_eq!(pairs(&sequence), 256 * 8 * 7 / 2);
    }

    /// A block of relays and sleepy nodes on the log-distance links of the
    /// scenarios (`scenarios/s2-block.toml`), for the research models.
    fn block(seed: u64, duration_s: f64) -> Scenario {
        block_of(1000.0, seed, duration_s)
    }

    /// [`block`], `side` metres square.
    fn block_of(side: f64, seed: u64, duration_s: f64) -> Scenario {
        let mut sc = Scenario::new("block", seed, duration_s)
            .place(crate::scenario::Placement::Density {
                role: RoleSpec::Relay,
                x0: 0.0,
                y0: 0.0,
                width_m: side,
                height_m: side,
                per_km2: 10.0,
                indoor: false,
                rooftop: false,
                relay_share: 0.0,
            })
            .place(crate::scenario::Placement::Density {
                role: RoleSpec::Leaf,
                x0: 0.0,
                y0: 0.0,
                width_m: side,
                height_m: side,
                per_km2: 200.0,
                indoor: true,
                rooftop: false,
                relay_share: 0.0,
            });
        sc.links.exponent = 3.6;
        sc.links.indoor_loss_db = 14.4;
        sc.links.shadowing_sigma_db = 6.0;
        sc
    }

    /// Relays placed as people send and receive texts like the sleepy
    /// nodes, and those that walk carry their position with them: their
    /// address is the position their core holds.
    #[test]
    fn people_s_relays_text_and_walk() {
        let mut sc = block(5, 1800.0);
        if let crate::scenario::Placement::Density { relay_share, .. } = &mut sc.nodes[1] {
            *relay_share = 0.1;
        }
        sc.movement.relay_fraction = 1.0;
        sc.movement.trip_m = 300.0;
        sc.movement.stop_s = 300.0;
        sc.traffic.push(TrafficSpec::Poisson {
            per_leaf_per_hour: 2.0,
            payload_min: 20,
            payload_max: 20,
            destination: Destination::Uniform,
        });
        let mut sim = Sim::new(&sc).unwrap();
        let people: Vec<u32> = (0..sim.n_relays)
            .filter(|&i| sim.person_of[i as usize] != u32::MAX)
            .collect();
        assert!(
            people.len() > 10,
            "{} relays among the people",
            people.len()
        );
        assert!(people.iter().all(|&i| sim.walks.as_ref().unwrap().walks(i)));
        assert!((0..sim.n_relays)
            .filter(|&i| sim.person_of[i as usize] == u32::MAX)
            .all(|i| !sim.walks.as_ref().unwrap().walks(i)));
        sim.run();
        let sent = people
            .iter()
            .map(|&i| sim.metrics.nodes[i as usize].originated);
        assert!(sent.sum::<u64>() > 0, "the people's relays sent nothing");
        let to_them = sim
            .origins
            .iter()
            .filter(|o| people.contains(&o.dst))
            .count();
        assert!(to_them > 0, "nothing was sent to the people's relays");
        let moved = people
            .iter()
            .filter(|&&i| sim.relay_position(i) != sim.positions[i as usize])
            .count();
        assert!(moved > 0, "no walking relay took a fix");
    }

    /// Forced pairs are of the kind and reach asked for, no node is in
    /// two, and they share an ID only when told to.
    #[test]
    fn forced_pairs_share_an_id() {
        use crate::scenario::{PairKind, PairReach};
        let mut sc = block_of(4000.0, 3, 1.0);
        for kind in [
            PairKind::RelayRelay,
            PairKind::RelaySleepy,
            PairKind::SleepySleepy,
        ] {
            for reach in [PairReach::In, PairReach::Out] {
                sc.shared_ids.push(SharedIds {
                    kind,
                    reach,
                    count: 2,
                    share: kind != PairKind::RelaySleepy,
                });
            }
        }
        let sim = Sim::new(&sc).unwrap();
        assert_eq!(sim.pairs.len(), 12);
        let mut seen = std::collections::HashSet::new();
        for p in &sim.pairs {
            assert!(seen.insert(p.a) && seen.insert(p.b), "{p:?} in two pairs");
            let relay = |i: u32| i < sim.n_relays;
            let kinds = match p.kind {
                PairKind::RelayRelay => (true, true),
                PairKind::RelaySleepy => (true, false),
                PairKind::SleepySleepy => (false, false),
            };
            assert_eq!((relay(p.a), relay(p.b)), kinds, "{p:?}");
            let up = |a, b| {
                sim.links
                    .get(a, b)
                    .is_some_and(|l| l.base_dbm as f64 >= sim.sens_dbm)
            };
            match p.reach {
                PairReach::In => assert!(up(p.a, p.b) && up(p.b, p.a), "{p:?}"),
                PairReach::Out => {
                    assert!(sim.links.get(p.a, p.b).is_none() && sim.links.get(p.b, p.a).is_none())
                }
            }
            assert_eq!(sim.node_id(p.a) == sim.node_id(p.b), p.shared, "{p:?}");
        }
    }

    /// A dark group is the sleepy nodes nearest its node; they go out of
    /// reach together for the spell and come back, whatever their own
    /// availability says.
    #[test]
    fn a_dark_group_goes_dark_together() {
        let mut sc = block(4, 400.0);
        sc.availability = Some(crate::scenario::Availability {
            mean_available_s: 100.0,
            mean_dark_s: 50.0,
        });
        sc.dark_groups.push(crate::scenario::DarkGroup {
            near: "relay:0".into(),
            count: 20,
            at_s: 100.0,
            for_s: 200.0,
        });
        let mut sim = Sim::new(&sc).unwrap();
        let g = sim.metrics.research.groups[0].clone();
        let at = sim.xy(0);
        let far = g
            .members
            .iter()
            .map(|&i| sim.xy(i).dist(at))
            .fold(0.0, f64::max);
        let nearer = (sim.n_relays..sim.node_count() as u32)
            .filter(|i| !g.members.contains(i))
            .filter(|&i| sim.xy(i).dist(at) < far)
            .count();
        assert_eq!((g.members.len(), nearer), (20, 0));
        sim.run_until(Instant::ZERO + secs(150.0));
        assert!(g.members.iter().all(|&i| !sim.nodes[i as usize].present));
        sim.run_until(Instant::ZERO + secs(350.0));
        for &i in &g.members {
            let st = &sim.nodes[i as usize];
            assert_eq!(st.present, !st.avail_dark, "node {i}");
        }
    }

    /// A relay whose GPS jumps is handed the fix unchecked, and with the
    /// checks the jump is refused; a drift slower than 50 m a second
    /// passes the jump check fix by fix.
    #[test]
    fn checks_refuse_a_jump_and_pass_a_slow_drift() {
        use crate::scenario::{FaultKind, GpsFault};
        let fault = |kind, checks| {
            let mut sc = block(5, 300.0);
            sc.gps.checks = checks;
            sc.gps.faults.push(GpsFault {
                kind,
                sleepy: false,
                count: 1,
                at_s: 100.0,
                distance_m: 5000.0,
                speed_m_s: 1.0,
            });
            let mut sim = Sim::new(&sc).unwrap();
            sim.run();
            let n = sim.metrics.research.gps_nodes[0][0];
            let moved = sim.relay(n).config().position != sim.positions[n as usize];
            (sim.metrics.research.fixes[0], moved)
        };
        let (f, moved) = fault(FaultKind::Jump, false);
        assert!(f[0] > 150 && f[1] + f[2] == 0 && moved, "{f:?}");
        let (f, moved) = fault(FaultKind::Jump, true);
        assert!(f[0] == 0 && f[1] > 150 && !moved, "{f:?}");
        let (f, moved) = fault(FaultKind::Drift, true);
        assert!(f[0] > 150 && f[1] + f[2] == 0 && moved, "{f:?}");
    }

    /// With an error on every relay's fixes, a relay whose fixes stray far
    /// from where it stands for long enough turns carried, and with a
    /// small error none does; with no fixes asked for, nothing of it is in
    /// the summary (roadmap D70).
    #[test]
    fn relays_with_a_large_gps_error_turn_carried() {
        let run = |error_m: f64, every_s: f64| {
            let mut sc = block(7, 900.0);
            sc.gps.checks = true;
            sc.gps.carried_speed_m_s = 2.0;
            sc.gps.relays_every_s = every_s;
            sc.gps.error_m = error_m;
            sc.gps.error_correlation_s = 300.0;
            let mut sim = Sim::new(&sc).unwrap();
            sim.run();
            sim.summary().research.carried
        };
        let c = run(200.0, 1.0).expect("far fixes");
        assert!(!c.far_fixes.is_empty() && !c.carried.is_empty(), "{c:?}");
        assert!(c
            .carried
            .iter()
            .all(|&(_, _, walks, run)| !walks && run >= 72.0));
        assert!(run(1.0, 1.0).is_none());
        assert!(run(0.0, 0.0).is_none());
    }

    /// An interferer reaches walkers where they are: one that walks out of
    /// its reach no longer hears it.
    #[test]
    fn interferers_reach_walkers_where_they_are() {
        let mut sc = block(6, 4000.0);
        sc.movement.fraction = 1.0;
        sc.movement.stop_s = 300.0;
        sc.interferers.push(crate::scenario::InterfererSpec {
            x: 500.0,
            y: 500.0,
            channel: 6,
            power_dbm: 10.0,
            lora: false,
            mean_on_s: 1.0,
            mean_off_s: 0.0,
            from_s: 0.0,
        });
        let mut sim = Sim::new(&sc).unwrap();
        let mut changed = 0;
        let mut last: Option<Vec<u32>> = None;
        for t in 1..40 {
            sim.run_until(Instant::ZERO + secs(t as f64 * 100.0));
            let i = &sim.interferers[0];
            assert!(i.reach.iter().all(|&(n, _)| n < sim.n_relays));
            let now: Vec<u32> = i.moving.iter().map(|m| m.0).collect();
            for &(n, dbm) in &i.moving {
                let w = sim.walks.as_ref().unwrap().walker(n).unwrap();
                let mut want = 10.0 - sc.links.path_loss_db(w.at(sim.now).dist(i.at));
                if !w.walking {
                    want -= sc.links.indoor_loss_db;
                }
                // Looked at again within the walkers' regrid interval.
                assert!((dbm as f64 - want).abs() < 3.0, "{dbm} against {want}");
            }
            if last.as_ref().is_some_and(|l| *l != now) {
                changed += 1;
            }
            last = Some(now);
        }
        assert!(changed > 10, "{changed} changes in the walkers reached");
    }

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
                entered_at: None,
                time_left: None,
                holder: None,
                holder_beats: 0,
                holder_tries: 0,
                watch: HolderWatch::default(),
                frames: 0,
                airtime_us: 0,
                hops: 0,
                distance_m: 0.0,
                dst_walking: false,
                src_relay: None,
                dst_relay: None,
                to_twin: false,
                src_moving: false,
                dst_anchor: None,
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
