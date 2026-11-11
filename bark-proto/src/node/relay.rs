//! The node and its loop: one event applied, the deadlines fired, the
//! radio's jobs scheduled. Every node is a `Relay`, sleepy or not by its
//! power (spec "Nodes", "Sleepy nodes").
//!
//! The parts live in `relay/`: `boot.rs` (spec "Home channel selection"),
//! `sweep.rs` (spec "Busy estimate", "Channel moves"), `heartbeat.rs`
//! (spec "Heartbeat"), `anchor.rs` (spec "Sleepy nodes"), `forward.rs`
//! (spec "Forwarding"), `delivery.rs`, `power.rs`, `motion.rs`, `gps.rs`.
//!
//! Each use of the radio is a job, one excursion at a time
//! (`mac::Excursion`, `docs/core-interface.md` §3 rule 1). When the radio
//! is free the first ready job in `JOBS` starts. While a hand-over waits
//! for its ACK only an owed reply uses the radio: the others could take the
//! node off its home channel, where the ACK arrives.

mod anchor;
mod battery;
mod boot;
mod delivery;
mod drain;
mod forward;
mod gps;
mod heartbeat;
mod motion;
mod power;
mod radio;
mod remember;
mod status;
mod sweep;
mod update;

use anchor::Listen;
use boot::{random_channel, Boot};
pub use drain::{Held, HeldAt};
use radio::Radio;
pub use remember::{remembered_within, Kept};
use update::Spread;

use battery::Battery;
use forward::{Forwarding, OwnMessage};
pub use power::Forced;

use crate::event::{Action, AppCommand, Event};
use crate::geo::LocalPlane;
use crate::ids::{MsgId, NodeId};
use crate::node::{Config, Core, Rng};
use crate::position::Position;
use crate::radio::{time_on_air, Channel, MIN_PREAMBLE_SYMBOLS};
use crate::sniff::LONG_PREAMBLE_SYMBOLS;
use crate::store::Stored;
use crate::tables::busy::BusyEstimator;
use crate::tables::delivered::Delivered;
use crate::tables::delivery::{DeliveryQueue, DELIVERY_QUEUE, INBOX};
use crate::tables::heard::{Heard, HEARD};
use crate::tables::neighbours::{Neighbour, NeighbourParams, Neighbours};
use crate::tables::records::Records;
use crate::tables::send_queue::{Kind, SendQueue, State};
use crate::time::{earlier, Duration, Instant};
use crate::wire::Packet;
use motion::Motion;

/// A use of the radio: what owns the outstanding excursion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Job {
    Sweep,
    Heartbeat,
    HandOver,
    Reply,
    Update,
    Status,
}

/// The jobs in the order the scheduler serves them: an owed reply is
/// worth nothing once its sender stops waiting; the heartbeat keeps its
/// slot; a status, which a person waits for, after the texts; an update
/// last, unless it has waited behind enough hand-overs (`relay/update.rs`).
const JOBS: [Job; 6] = [
    Job::Reply,
    Job::Heartbeat,
    Job::HandOver,
    Job::Status,
    Job::Sweep,
    Job::Update,
];

/// The jobs when an update frame goes before the next hand-over.
const JOBS_UPDATE_FIRST: [Job; 6] = [
    Job::Reply,
    Job::Heartbeat,
    Job::Update,
    Job::HandOver,
    Job::Status,
    Job::Sweep,
];

/// A sleepy node's anchor: the node that is not sleepy whose home channel
/// it listens on and whose position is its address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Anchor {
    /// Its ID.
    pub id: NodeId,
    /// Its position.
    pub position: Position,
}

/// What the node is by its power (spec "Nodes"); the heartbeat's flags are
/// read off it.
#[derive(Clone, Copy, Debug)]
enum Mode {
    Relay(Power),
    Sleepy(Sleep),
}

/// A relay's power (`relay/power.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Power {
    External,
    /// On battery and fixed since then: in its grace, or relaying on
    /// with battery levels.
    Battery(Instant),
    /// Its grace over, its battery low or carried on battery, since then:
    /// it flushes its delivery queue, and then turns sleepy.
    Leaving(Instant),
}

/// What a sleepy node keeps.
#[derive(Clone, Copy, Debug)]
struct Sleep {
    /// Its anchor, kept while it hears no better one.
    anchor: Option<Anchor>,
    /// It turned sleepy by its battery, so relays again when that is high.
    by_battery: bool,
    /// The rotations since it last heard its anchor's heartbeat where it
    /// expected it.
    misses: u8,
    /// Its last check heard nothing at all: out of reach, it checks again
    /// only once it hears something.
    dark: bool,
}

impl Sleep {
    /// Sleepy, with no anchor yet.
    const fn new(by_battery: bool) -> Sleep {
        Sleep {
            anchor: None,
            by_battery,
            misses: 0,
            dark: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    /// Not started: the driver's first `Wakeup` starts it.
    Off,
    /// In its first rotation (spec "Channel at boot"), until
    /// `Boot::ends`.
    Booting,
    Running,
}

/// A snapshot of a relay's tables and budget ([`Relay::load`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Load {
    /// Airtime in the duty-cycle bucket (spec "Duty cycle cap").
    pub bucket: Duration,
    /// Frames in the send queue, hop places and a place held for the
    /// delivery queue included (spec "Send queue").
    pub send_queue: usize,
    /// Messages waiting for a sleepy node (spec "Delivery queue").
    pub delivery_queue: usize,
    /// Heard table entries, expired ones included.
    pub heard: usize,
    /// Neighbour table entries, expired ones included.
    pub neighbours: usize,
    /// Of `send_queue`, the frames in hop places (spec "Send queue").
    pub hop_places: usize,
    /// Of `send_queue`, the frames waiting for a ban to end (spec "Overload waits").
    pub waiting: usize,
    /// Of `send_queue`, returns to the parent (spec "Return to parent").
    pub returns: usize,
    /// Live neighbours that are not a last resort: the next hops a message
    /// chooses among (spec "Next hop").
    pub candidates: usize,
    /// Of `candidates`, the banned ones (spec "Bans").
    pub banned: usize,
}

/// A node that is not sleepy: its heard table and delivery queue at the
/// design's sizes (spec "Heard table", "Delivery queue").
pub type Relay = Node<HEARD, DELIVERY_QUEUE>;

/// A node's live neighbours by what it knows of each
/// ([`Node::neighbour_counts`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NeighbourCounts {
    /// Whose heartbeats it hears.
    pub heard: u8,
    /// Of them, heard with margin: a full frame reaches this node through
    /// a fade, so one from it likely reaches them; not proven.
    pub strong: u8,
    /// Of them, those that have taken a message from it since it first
    /// heard them: proven to hear it.
    pub both_ways: u8,
}

/// A node built only to be sleepy, without the memory of a heard table and
/// delivery queue; it stays sleepy whatever its power. A [`Relay`] on
/// battery behaves the same and keeps them empty.
pub type SleepyNode = Node<0, 0>;

/// The node, with room for `H` sleepy nodes heard and `Q` messages for
/// them (`docs/core-interface.md` §6).
///
/// Laid out as declared, what an event reads first and the tables after,
/// so an event touches few memory pages: in a city-sized simulation the
/// pages touched, not the instructions, set the speed.
#[repr(C)]
pub struct Node<const H: usize, const Q: usize> {
    stage: Stage,
    mode: Mode,
    battery: Battery,
    config: Config,
    boot: Boot,
    listen: Listen,
    radio: Radio,
    forwarding: Forwarding,
    own: OwnMessage,
    spread: Spread,
    motion: Motion,
    gps: gps::Gps,
    /// What the node last told its driver to keep across a reboot, or was
    /// handed back at boot.
    stored: Option<Stored>,
    id: NodeId,
    home: Channel,
    plane: LocalPlane,
    rng: Rng,
    delivered: Delivered,
    heard: Heard<H>,
    delivery: DeliveryQueue<Q>,
    /// The messages for its own application, until the application
    /// confirms each (spec "Receiving DATA", "Phone link").
    inbox: DeliveryQueue<INBOX>,
    neighbours: Neighbours,
    queue: SendQueue,
    records: Records,
}

impl<const H: usize, const Q: usize> Node<H, Q> {
    /// A relay until its power says otherwise, unless it has no room for
    /// a relay's mail (`relay/power.rs`): then sleepy, with no anchor yet.
    const fn mode_at_boot() -> Mode {
        if Self::can_relay() {
            Mode::Relay(Power::External)
        } else {
            Mode::Sleepy(Sleep::new(false))
        }
    }

    /// A relay with empty tables, not set up, built at compile time so a
    /// driver can keep it in a static and set it up in place with
    /// [`Relay::reset`]: a relay is 150 KiB, and [`Relay::new`] needs as much
    /// again on the stack (`docs/core-interface.md` §6).
    pub const UNSET: Self = {
        let config = Config::DEFAULT;
        let channel = match Channel::new(0) {
            Some(c) => c,
            None => unreachable!(),
        };
        Node {
            stored: None,
            id: NodeId::NONE,
            plane: LocalPlane::ORIGIN,
            home: channel,
            stage: Stage::Off,
            mode: Self::mode_at_boot(),
            battery: Battery::UNSET,
            radio: Radio::new(&config, channel, channel),
            neighbours: Neighbours::new(NeighbourParams::from_config(&config)),
            records: Records::new(),
            queue: SendQueue::new(),
            forwarding: Forwarding::IDLE,
            heard: Heard::from_config(&config),
            motion: Motion::at_boot(&config),
            gps: gps::Gps::DEFAULT,
            delivery: DeliveryQueue::from_config(&config),
            inbox: DeliveryQueue::from_config(&config),
            delivered: Delivered::new(),
            own: OwnMessage::NONE,
            spread: Spread::new(),
            config,
            boot: Boot::UNSET,
            listen: Listen::UNSET,
            rng: Rng::new(0),
        }
    };

    /// Create a relay. `seed` drives every random choice the relay makes;
    /// two relays with the same config and seed behave identically.
    pub fn new(config: Config, seed: u64) -> Self {
        let mut relay = Self::UNSET;
        relay.reset(config, seed);
        relay
    }

    /// Make this relay, in place, what [`Relay::new`] would build from
    /// `config` and `seed`: everything it held is gone.
    pub fn reset(&mut self, config: Config, seed: u64) {
        let mut rng = Rng::new(seed);
        let mut node_id = config.node_id;
        while node_id == NodeId::NONE {
            node_id = NodeId::from_u64(rng.next_u64());
        }
        let mut config = config;
        // A fixed node stands where it stood before the reboot (spec
        // "Position").
        if let Some(s) = config.stored.filter(|s| !s.carried) {
            config.position = s.place;
        }
        let home = config
            .home_override
            .or(config.stored.map(|s| s.home))
            .unwrap_or_else(|| random_channel(&mut rng));
        let rotation = random_channel(&mut rng);
        // Every field by name, so that a new one cannot be left out. The
        // large tables are emptied in place: built and assigned, each would
        // pass through the stack (`bark-fw/README.md`).
        let Node {
            config: c,
            boot,
            listen,
            stored,
            id,
            plane,
            rng: r,
            home: h,
            stage,
            mode,
            battery,
            radio,
            neighbours,
            records,
            queue,
            forwarding,
            heard,
            motion,
            gps,
            delivery,
            inbox,
            delivered,
            own,
            spread,
        } = self;
        *stored = config.stored;
        *id = node_id;
        *plane = LocalPlane::new(config.position);
        *h = home;
        *stage = Stage::Off;
        *mode = Self::mode_at_boot();
        *battery = Battery::at_boot(config.stored);
        *radio = Radio::new(
            &config,
            Channel::all().next().expect("the plan has channels"),
            rotation,
        );
        neighbours.reset(NeighbourParams::from_config(&config));
        records.clear();
        queue.clear();
        *forwarding = Forwarding::IDLE;
        heard.reset(&config);
        *motion = Motion::at_boot(&config);
        *gps = gps::Gps::DEFAULT;
        delivery.reset(&config);
        inbox.reset(&config);
        *delivered = Delivered::new();
        *own = OwnMessage::NONE;
        *spread = Spread::new();
        *boot = Boot::UNSET;
        *listen = Listen::UNSET;
        *c = config;
        *r = rng;
    }

    /// Current home channel.
    pub fn home(&self) -> Channel {
        self.home
    }

    /// Configuration.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Before when its application should not offer a message again: the
    /// wait the last refusal of its message asked for.
    pub fn retry_at(&self) -> Instant {
        self.own.retry_at
    }

    /// This node's ID (spec "Addresses").
    pub fn id(&self) -> NodeId {
        self.id
    }

    /// The position in this node's address and the one it works by (spec
    /// "Addresses", "Beats"): its own, or a sleepy node's anchor's.
    /// `Config::position` stays its own place, for when it relays again.
    pub fn address(&self) -> Position {
        self.anchor().map_or(self.config.position, |a| a.position)
    }

    /// A sleepy node's anchor, if it has chosen one.
    pub fn anchor(&self) -> Option<Anchor> {
        self.sleep()?.anchor
    }

    /// What the node keeps as a sleepy node, if it is one.
    fn sleep(&self) -> Option<&Sleep> {
        match &self.mode {
            Mode::Sleepy(s) => Some(s),
            Mode::Relay(_) => None,
        }
    }

    fn sleep_mut(&mut self) -> Option<&mut Sleep> {
        match &mut self.mode {
            Mode::Sleepy(s) => Some(s),
            Mode::Relay(_) => None,
        }
    }

    /// Whether this is a sleepy node with an anchor: it beats on its
    /// anchor's channel and listens in the windows after its beats.
    fn anchored_sleepy(&self) -> bool {
        self.anchor().is_some()
    }

    /// Whether this node is a sleepy node without an anchor, past its boot
    /// listen (spec "Sleepy nodes alone").
    pub fn anchorless(&self) -> bool {
        self.sleep().is_some_and(|s| s.anchor.is_none()) && self.stage == Stage::Running
    }

    /// Whether its position is a map point a person set in the app (spec
    /// "Position").
    pub fn on_map_point(&self) -> bool {
        self.motion.map_point()
    }

    /// The preamble the frames this node forwards are counted with before
    /// their next hop is chosen: the long one at an anchorless node, whose
    /// neighbours are sleepy, else the shortest.
    pub(super) fn forward_preamble(&self) -> u16 {
        if self.anchorless() {
            LONG_PREAMBLE_SYMBOLS
        } else {
            MIN_PREAMBLE_SYMBOLS
        }
    }

    /// Whether the radio is free of anything the node does: no excursion,
    /// hand-over, owed reply or heartbeat under way.
    fn radio_free(&self) -> bool {
        self.radio.idle()
            && self.handover_idle()
            && !self.radio.replies().pending()
            && !self.radio.beacon.under_way()
    }

    /// The heard table.
    pub fn heard_nodes(&self) -> &Heard<H> {
        &self.heard
    }

    /// Whether the first rotation (spec "Channel at boot") is still running.
    pub fn booting(&self) -> bool {
        !matches!(self.stage, Stage::Running)
    }

    /// The neighbour table.
    pub fn neighbours(&self) -> impl Iterator<Item = &Neighbour> {
        self.neighbours.iter()
    }

    /// The neighbour table with its slots and its rules: which entries
    /// are live, demoted, banned.
    pub fn neighbour_table(&self) -> &Neighbours {
        &self.neighbours
    }

    /// The busy estimates and noise floor.
    pub fn busy(&self) -> &BusyEstimator {
        self.radio.busy()
    }

    /// The messages waiting in the delivery queue for their nodes.
    pub fn delivery_queue(&self) -> impl Iterator<Item = (MsgId, NodeId)> + '_ {
        self.delivery.iter().map(|q| (q.msg_id, q.node))
    }

    /// The messages this relay holds, queued to send or for a sleepy node:
    /// to tell a message still in the network from one lost.
    pub fn holding(&self) -> impl Iterator<Item = MsgId> + '_ {
        self.queue
            .iter()
            .map(|o| o.msg_id())
            .chain(self.delivery.iter().map(|q| q.msg_id))
    }

    /// Whether this relay has a record of message `id`
    /// (spec "Message records").
    pub fn knows(&self, id: MsgId) -> bool {
        self.records.get(id).is_some()
    }

    /// How many records this relay keeps, and, once the table is full, how
    /// long its least recently active one has been idle: about how long a
    /// record lasts here (spec "Message records").
    pub fn records(&self, now: Instant) -> (usize, Option<Duration>) {
        let idle = self.records.least_recent().map(|t| now.since(t));
        (self.records.len(), idle)
    }

    /// The lowest count a packet may carry to be taken into a free hop
    /// place now (spec "Send queue").
    pub fn lowest_free_hop_place(&self) -> Option<u8> {
        self.queue.lowest_free_place()
    }

    /// The highest count this relay would send to the neighbour `to`, of
    /// the forwards that could go to it (spec "Next hop", "Hop count").
    pub fn highest_count_for(&self, to: NodeId, now: Instant) -> Option<u8> {
        let (slot, n) = self.neighbours.by_id(to)?;
        if self.neighbours.is_expired(n, now) {
            return None;
        }
        self.queue
            .iter()
            .filter(|o| o.kind == Kind::Forward && !o.held())
            .filter_map(|o| self.records.get(o.msg_id()))
            .filter(|r| !r.handed(slot) && r.parent().map(|(p, _)| p) != Some(to))
            .map(|r| r.highest_hops().saturating_add(1))
            .max()
    }

    /// Whether the hand-over under way takes a message from the delivery
    /// queue to its node whether or not it is in a window
    /// (`relay/delivery.rs`).
    pub fn sending_direct(&self) -> bool {
        self.in_flight().is_some_and(|o| o.kind == Kind::Direct)
    }

    /// Whether a hand-over is under way, its waits included.
    pub fn handing_over(&self) -> bool {
        !self.handover_idle()
    }

    /// The send queue's entries in its regular room and in hop places.
    pub fn queue_use(&self) -> (usize, usize) {
        let places = self.queue.iter().filter(|o| o.place > 0).count();
        (self.queue.len() - places, places)
    }

    /// How loaded the relay is at `now`.
    pub fn load(&self, now: Instant) -> Load {
        Load {
            bucket: self.radio.budget().available(now),
            send_queue: self.queue.len(),
            delivery_queue: self.delivery.len(),
            heard: self.heard.len(),
            neighbours: self.neighbours.len(),
            hop_places: self.queue.iter().filter(|o| o.place > 0).count(),
            waiting: self.queue.iter().filter(|o| o.not_before > now).count(),
            returns: self
                .queue
                .iter()
                .filter(|o| o.kind == crate::tables::send_queue::Kind::Return)
                .count(),
            candidates: self
                .neighbours
                .live(now)
                .filter(|(_, n)| !n.last_resort())
                .count(),
            banned: self
                .neighbours
                .live(now)
                .filter(|(_, n)| !n.last_resort() && self.neighbours.is_banned(n, now))
                .count(),
        }
    }

    /// What the node keeps across a reboot now. A carried node keeps the
    /// place and, if sleepy, the home it had when last fixed, and no anchor:
    /// one that boots elsewhere chooses afresh.
    fn to_keep(&self) -> Stored {
        let (battery_scale, drain) = self.battery.kept();
        let carried = self.motion.carried();
        let kept = self.stored.filter(|_| carried);
        Stored {
            home: match kept {
                Some(s) if self.sleepy() => s.home,
                _ => self.home,
            },
            place: kept.map_or(self.motion.place(), |s| s.place),
            carried,
            map_point: kept.map_or(self.motion.map_point(), |s| s.map_point),
            anchor: self.anchor().filter(|_| !carried),
            // Kept only where it is read, so a node without battery levels
            // writes no more often.
            relay: !self.sleepy() && self.config.battery.is_some(),
            battery_scale,
            drain,
        }
    }

    /// Tell the driver to keep what changed.
    fn keep(&mut self, emit: &mut dyn FnMut(Action)) {
        // A node with no place has none to keep: it boots without one.
        if !self.motion.placed() {
            return;
        }
        let now = self.to_keep();
        if self.stored != Some(now) {
            self.stored = Some(now);
            emit(Action::Store(now));
        }
    }

    /// Fire what is due at `now`.
    fn fire(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        if self.boot.due(now) {
            self.finish_boot(now, emit);
        }
        if let Some(ends) = self.listen.check_due(now) {
            self.anchor_checked(ends - self.config.boot_listen, now, emit);
        }
        if self.listen.choice_due(now) {
            self.reconsider_anchor(now, emit);
        }
        self.watch_anchor(now, emit);
        if self.radio.move_due(now) {
            self.radio.clear_move();
            // Re-check after the random wait (spec "Channel moves").
            if self.excess() {
                let old = self.home;
                let to = self.quietest();
                self.retune(to, emit);
                if self.home != old {
                    self.radio.beacon.moved_from(old);
                    self.take_listeners(old, now);
                }
            }
        }
        self.power_timers(now, emit);
        let gps = self.gps_situation();
        self.gps.fire(&gps, &self.config, now, emit);
        self.forward_timers(now, emit);
        self.delivery_timers(now);
    }

    /// When the core next needs a wakeup: the earliest deadline of any part
    /// or job.
    fn due_at(&self) -> Option<Instant> {
        let jobs = JOBS.iter().filter_map(|&job| self.ready_at(job));
        [
            self.boot.next_wakeup(),
            self.listen.next_wakeup(),
            self.radio.move_at(),
            self.power_wakeup(),
            self.gps.next_wakeup(&self.gps_situation(), &self.config),
            self.delivery_wakeup(),
            self.own.app_ready_at,
            self.next_slot_edge(),
        ]
        .into_iter()
        .flatten()
        .chain(jobs)
        .min()
    }

    /// Serve the radio: feed the send queue, then start ready jobs in order
    /// until one takes the radio. A job that needed no excursion lets the
    /// next be tried. Not inlined: in `handle` it took 0.9 KB more of the
    /// node image's flash, and no measurable time.
    #[inline(never)]
    fn schedule(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        self.release_held(now, emit);
        self.release(now, emit);
        if self.forwarding.feed_due || self.forwarding.feed_at.is_some_and(|t| t <= now) {
            self.feed(false, now);
        }
        while self.radio.idle() {
            self.prepare(now, emit);
            let jobs = if self.spread.first() {
                JOBS_UPDATE_FIRST
            } else {
                JOBS
            };
            if !jobs.iter().any(|&job| self.start_job(job, now, emit)) {
                break;
            }
        }
    }

    /// Start `job` if it is ready; none but the reply and the hand-over
    /// while an ACK is awaited.
    fn start_job(&mut self, job: Job, now: Instant, emit: &mut dyn FnMut(Action)) -> bool {
        match job {
            Job::Reply => self.start_reply(now, emit),
            Job::Heartbeat => {
                !self.awaiting_ack() && self.beacon_due(now) && self.start_heartbeat(now, emit)
            }
            Job::HandOver => self.start_handover(now, emit),
            Job::Sweep => !self.awaiting_ack() && self.sweep_due(now) && self.start_sweep(emit),
            Job::Update => !self.awaiting_ack() && self.start_update(now, emit),
            Job::Status => !self.awaiting_ack() && self.start_status(now, emit),
        }
    }

    /// When `job` is next ready of its own accord. Jobs that need the radio
    /// free say so only while it is: its completion event wakes the core
    /// otherwise. The ACK wait counts even while an owed reply holds the
    /// radio.
    fn ready_at(&self, job: Job) -> Option<Instant> {
        let free = self.radio.idle();
        match job {
            Job::Reply => self.radio.replies().wake_at().filter(|_| free),
            Job::Heartbeat => {
                if !free || self.awaiting_ack() || !self.heartbeats() {
                    return None;
                }
                Some(self.heartbeat_ready_at())
            }
            Job::HandOver => {
                let flight = self.flight_wake().filter(|_| self.awaiting_ack() || free);
                // A waiting entry, once nothing else is under way.
                let queued = self
                    .forwarding
                    .wake
                    .filter(|_| self.handover_idle() && free);
                earlier(flight, queued)
            }
            Job::Sweep => {
                (free && !self.awaiting_ack() && self.sweeping()).then_some(self.radio.sweep_at())
            }
            Job::Update => {
                if !free || self.awaiting_ack() {
                    return None;
                }
                self.update_ready_at()
            }
            Job::Status => {
                if !free || self.awaiting_ack() {
                    return None;
                }
                self.status_ready_at()
            }
        }
    }

    // --- Reception ---

    fn received(&mut self, bytes: &[u8], rssi: i16, now: Instant, emit: &mut dyn FnMut(Action)) {
        self.set_aside_own(bytes, now);
        if let Some(s) = self.sleep_mut().filter(|s| s.dark) {
            // Back in reach: its anchor's rotations count from here.
            s.dark = false;
            s.misses = 0;
        }
        let decoded = Packet::decode(bytes);
        // A decoded frame proves this image works with another node's
        // (spec "Updates over the mesh"); a piece names no sender, but no
        // node hears its own.
        if decoded.as_ref().is_ok_and(|p| p.sender() != Some(self.id)) && self.radio.first_heard() {
            emit(Action::App(crate::AppEvent::Heard));
        }
        // A node's DATA says where it listens now (spec "Packet sender"),
        // a sleepy node's that it is still in reach (spec "Heard table"). Its
        // DATA and ACKs show how well it is heard (spec "Neighbour table").
        match decoded {
            Ok(Packet::Data(d)) => {
                self.neighbours.home_heard(d.src_hop, d.sender_home);
                self.neighbours.frame_heard(d.src_hop, rssi);
                self.heard.sent(d.src_hop, d.sender_home, now);
            }
            Ok(Packet::Ack(a)) => self.neighbours.frame_heard(a.from, rssi),
            _ => {}
        }
        match decoded {
            Ok(Packet::Heartbeat(hb)) => {
                self.spread.beat_heard(now);
                self.heard(hb, rssi, now, emit);
                if let Some(section) = crate::wire::decode_section(bytes) {
                    self.section_heard(hb.id, section, now, emit);
                }
            }
            Ok(Packet::Ack(ack)) => self.ack_received(ack, now, emit),
            Ok(Packet::Update(u)) => self.update_heard(u, now, emit),
            Ok(Packet::Status(f)) => self.status_heard(f, now, emit),
            Ok(Packet::Data(d)) if d.dst_hop == self.me() => {
                // It chose this node from heartbeats it heard: it hears us
                // (spec "Neighbour table").
                self.answer_from(d.src_hop, now, emit);
                self.data(d, now, emit)
            }
            _ => {}
        }
    }

    fn set_position(&mut self, position: Position) {
        self.config.position = position;
        self.replane();
    }

    /// The plane is centred on the position the node works by, its
    /// address, and every neighbour's point is on it.
    pub(super) fn replane(&mut self) {
        self.plane = LocalPlane::new(self.address());
        self.neighbours.replane(&self.plane);
    }

    fn app(&mut self, cmd: AppCommand<'_>, now: Instant, emit: &mut dyn FnMut(Action)) {
        match cmd {
            AppCommand::Send {
                dst_pos,
                dst_id,
                payload,
            } => self.originate(dst_pos, dst_id, payload, now, emit),
            AppCommand::Power { external } => self.power(external, now, emit),
            AppCommand::ForceRole => self.force(now, emit),
            AppCommand::Battery { millivolts } => self.battery_read(millivolts, now, emit),
            AppCommand::ClearMapPoint => self.clear_map_point(),
            AppCommand::Fix { position } => {
                self.gps.fixed(self.sleepy());
                self.fix(position, now, emit);
                self.gps.after_fix(self.sleepy(), self.far());
            }
            AppCommand::NoFix => self.motion.no_fix(),
            AppCommand::Holds(image) => self.holds_image(image, now),
            AppCommand::Gather { image, from } => self.gather(image, from, now),
            AppCommand::Block { index, ok } => self.spread.block_done(index, ok),
            AppCommand::Ask { neighbour } => self.ask(neighbour, now),
            AppCommand::Drain(on) => self.drain(on, now),
            AppCommand::SetPosition { position } => {
                // A sleepy node's position is no news, but a change of
                // its carried flag is.
                if self.map_point(position, now) || !self.sleepy() {
                    self.changed();
                }
            }
        }
    }
}

impl<const H: usize, const Q: usize> Core for Node<H, Q> {
    fn handle(&mut self, event: Event<'_>, now: Instant, emit: &mut dyn FnMut(Action)) {
        // Any use of the radio ends a sniff (core-interface §3 rule 10),
        // and an excursion takes it away from listening.
        let woke = core::cell::Cell::new(false);
        let away = core::cell::Cell::new(false);
        let mut wake = |a: Action| {
            let excursion = matches!(
                a,
                Action::Lbt(_) | Action::Rssi(_) | Action::Transmit(_) | Action::Serve(_)
            );
            woke.set(woke.get() || excursion || matches!(a, Action::Listen(_)));
            away.set(away.get() || excursion);
            emit(a)
        };
        let emit: &mut dyn FnMut(Action) = &mut wake;
        if self.stage == Stage::Off {
            self.start(now, emit);
        }
        // A frame received now took the radio since it started; a
        // heartbeat due meanwhile is counted first, against that.
        if let Event::Received { bytes, .. } = event {
            let started = now - time_on_air(bytes.len() as u8, MIN_PREAMBLE_SYMBOLS);
            self.radio.received_from(started);
        }
        self.count_missed_heartbeats(now);
        let threshold = self.radio.busy().threshold_dbm();
        match event {
            Event::Wakeup => {}
            Event::Received {
                bytes, rssi_dbm, ..
            } => self.received(bytes, rssi_dbm, now, emit),
            // The channel is clear only if neither the CAD saw a preamble
            // nor the reading was above the busy threshold (spec
            // "Heartbeat", "Busy estimate").
            Event::LbtDone { busy, dbm, .. } => {
                let busy = busy || dbm > threshold;
                match self.radio.complete() {
                    Some(Job::Heartbeat) => self.heartbeat_check(busy, now, emit),
                    Some(Job::HandOver) => self.handover_check(busy, now, emit),
                    Some(Job::Update) => self.update_check(busy, now, emit),
                    Some(Job::Status) => self.status_check(busy, now, emit),
                    // A reply goes without a check (spec "The ACK").
                    Some(Job::Sweep | Job::Reply) | None => {}
                }
            }
            Event::RssiSample { channel, dbm } => match self.radio.complete() {
                Some(Job::Sweep) => self.sweep_sample(channel, dbm, now),
                Some(Job::Heartbeat | Job::HandOver | Job::Reply | Job::Update | Job::Status)
                | None => {}
            },
            Event::TxDone => match self.radio.complete() {
                Some(Job::Heartbeat) => self.heartbeat_done(now, true),
                // Unless the hand-over was given up meanwhile (a displaced record).
                Some(Job::HandOver) => {
                    self.update_passed(now);
                    let timeout = self.config.ack_timeout;
                    if let Some((mut h, _)) = self.handover() {
                        if h.state() == State::Radio {
                            h.sent(now, timeout);
                        }
                    }
                }
                Some(Job::Reply) => {
                    let (listens_on, taken) = self.radio.reply_sent();
                    if listens_on {
                        self.took(now);
                    }
                    if let Some(take) = taken {
                        self.taken(take, now, emit);
                    }
                }
                _ => {}
            },
            Event::App(cmd) => self.app(cmd, now, emit),
        }
        if !matches!(event, Event::Wakeup | Event::App(_)) {
            // The excursion or the reception is over, unless a held
            // excursion is still out (core-interface §3 rule 8).
            self.radio.back(now);
        }
        self.fire(now, emit);
        self.schedule(now, emit);
        if away.get() {
            self.radio.went(now);
        }
        self.own.app_ready_at = self.send_ready_at(now);
        if woke.get() {
            self.listen.radio_used();
        }
        self.sniff(now, emit);
        self.keep(emit);
    }

    fn inbox(&self) -> Option<(MsgId, &[u8])> {
        self.inbox.iter().next().map(|q| (q.msg_id, q.payload()))
    }

    fn inbox_len(&self) -> usize {
        self.inbox.len()
    }

    fn confirm(&mut self, msg_id: MsgId) -> bool {
        if self.inbox.iter().next().is_some_and(|q| q.msg_id == msg_id) {
            self.inbox.remove(msg_id);
            true
        } else {
            false
        }
    }

    fn gps_tracks(&self) -> bool {
        Node::gps_tracks(self)
    }

    fn fix_in_reach(&self, position: Position) -> bool {
        Node::fix_in_reach(self, position)
    }

    fn next_wakeup(&self) -> Option<Instant> {
        if self.stage == Stage::Off {
            return Some(Instant::ZERO);
        }
        self.due_at()
    }

    fn accepts_send(&self, now: Instant) -> bool {
        Self::accepts_send(self, now)
    }
}

#[cfg(test)]
mod tests;
