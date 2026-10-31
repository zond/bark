//! The node: medium access, forwarding and delivery to the sleepy nodes
//! it hears. Every node is a `Relay`, sleepy or not by its power
//! (`relay/power.rs`, spec "Sleepy nodes").
//!
//! This file holds the node and its loop: one event applied, the
//! deadlines fired, the radio's jobs scheduled. The first rotation and
//! the home channel are in `relay/boot.rs`
//! (spec "Home channel selection"), the busy sweep and the move rule in
//! `relay/sweep.rs` (spec "Busy estimate", "Channel moves"), the
//! heartbeat in `relay/heartbeat.rs` (spec "Heartbeat"), a sleepy node's
//! anchor, beat and duty-cycled receive in `relay/anchor.rs`
//! (spec "Sleepy nodes"), routing and the per-hop transfer in
//! `relay/forward.rs` (spec "Forwarding"), delivery to sleepy nodes in
//! `relay/delivery.rs`, the node's role by its power in
//! `relay/power.rs`, whether it moves in `relay/motion.rs` and its GPS
//! in `relay/gps.rs`.
//!
//! # Radio discipline
//!
//! Each use of the radio is a job: the reply (an ACK, taking or refusing)
//! this node owes, the heartbeat, the hand-over of one entry of the send
//! queue, and the sweep. Each is one excursion of the one shape
//! (`mac::Excursion`): a check of a channel and, if it is clear, a
//! transmission, or one reading; the job continues from the completion
//! event. The node keeps at most one excursion outstanding
//! (`docs/core-interface.md` §3 rule 1). When the radio is free it starts
//! the first job that is ready, in that order: an owed reply is worth
//! nothing once its sender stops waiting, 100 ms after the frame (spec
//! "The ACK"). The hand-over alone lets an owed reply in before it
//! transmits, since its frame can take 139 ms on air.
//! While a hand-over waits for its ACK, only an owed reply uses the radio:
//! the heartbeat and the sweep would take the node off its home channel,
//! where the ACK arrives (spec "Hand-over"). Jobs that are not ready say when
//! they will be, and the earliest of those is `next_wakeup`.

mod anchor;
mod boot;
mod delivery;
mod forward;
mod gps;
mod heartbeat;
mod motion;
mod power;
mod sweep;

use boot::random_channel;
use heartbeat::Beacon;
use sweep::Sweep;

use forward::{Forwarding, OwnMessage};
use power::Battery;

use crate::event::{Action, AppCommand, Event};
use crate::geo::LocalPlane;
use crate::ids::{MsgId, NodeId};
use crate::node::mac::Excursion;
use crate::node::transfer::Replies;
use crate::node::{Config, Core, Rng};
use crate::position::Position;
use crate::radio::{Channel, MIN_PREAMBLE_SYMBOLS};
use crate::sniff::LONG_PREAMBLE_SYMBOLS;
use crate::store::Stored;
use crate::tables::budget::Budget;
use crate::tables::busy::BusyEstimator;
use crate::tables::delivered::Delivered;
use crate::tables::delivery::{DeliveryQueue, DELIVERY_QUEUE};
use crate::tables::heard::{Heard, HEARD};
use crate::tables::neighbours::{Neighbour, NeighbourParams, Neighbours};
use crate::tables::records::Records;
use crate::tables::send_queue::{Kind, SendQueue, State};
use crate::time::{earlier, Duration, Instant};
use crate::wire::Packet;
use motion::Motion;

/// A use of the radio: what owns the outstanding excursion, and what the
/// scheduler starts (module documentation).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Job {
    Sweep,
    Heartbeat,
    HandOver,
    Reply,
}

/// The jobs in the order the scheduler serves them: an owed reply is
/// worth nothing once its sender stops waiting; the heartbeat keeps its
/// slot; a hand-over; the sweep's reading last.
const JOBS: [Job; 4] = [Job::Reply, Job::Heartbeat, Job::HandOver, Job::Sweep];

/// The radio's side: the one excursion outstanding, what each job keeps
/// between its excursions, and the airtime budget and the busy estimate
/// that gate them.
struct Radio {
    excursion: Excursion<Job>,
    /// The replies this node owes (`transfer.rs`).
    replies: Replies,
    /// The busy sweep (`relay/sweep.rs`).
    sweep: Sweep,
    /// The heartbeat (`relay/heartbeat.rs`).
    beacon: Beacon,
    busy: BusyEstimator,
    budget: Budget,
}

impl Radio {
    /// Before the first event: the sweep from `sweep`, the heartbeat's
    /// rotation from `rotation`.
    const fn new(config: &Config, sweep: Channel, rotation: Channel) -> Radio {
        Radio {
            excursion: Excursion::new(),
            replies: Replies::new(),
            sweep: Sweep::new(sweep),
            beacon: Beacon::new(rotation),
            busy: BusyEstimator::from_config(config),
            budget: Budget::new(),
        }
    }
}

/// A sleepy node's anchor: the node that is not sleepy whose home channel
/// it listens on and whose position is its address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Anchor {
    /// Its ID.
    pub id: NodeId,
    /// Its position.
    pub position: Position,
}

/// What the node is by its power (spec "Nodes", `relay/power.rs`), and
/// what each needs. The heartbeat's flags are read off it
/// (`heartbeat_frame`).
#[derive(Clone, Copy, Debug)]
enum Mode {
    /// Not sleepy: it holds mail and heartbeats on its slots.
    Relay(Power),
    /// Sleepy: it holds mail for nobody, beats to its anchor and
    /// receives duty-cycled between its windows (`relay/anchor.rs`).
    Sleepy(Sleep),
}

/// A relay's power (`relay/power.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Power {
    External,
    /// On battery since then: in its grace, or relaying on while it is
    /// fixed with battery levels.
    Battery(Instant),
    /// Its grace over or its battery low, since then: it flushes its
    /// delivery queue, and then turns sleepy.
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
    /// Its last check heard nothing at all: it is out of reach, and checks
    /// again only once it hears something.
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
    /// `Timers::boot_ends`.
    Booting,
    Running,
}

/// The node's deadlines, each named for what it times. [`Timers::next`]
/// is the earliest that needs the core of its own accord; the wake-up is
/// derived from it and from what the tables and the radio jobs say, in
/// `Node::due_at`, and nowhere else. The firing is `Node::fire`.
#[derive(Clone, Copy, Debug)]
struct Timers {
    /// When the first rotation began.
    boot_from: Instant,
    /// When it ends, while it runs (spec "Channel at boot").
    boot_ends: Option<Instant>,
    /// When the rotation a node that sleeps listens to check its anchor
    /// ends, while it does (spec "Relay choice").
    rescan_ends: Option<Instant>,
    /// Until when a sleepy node that has just taken its first anchor
    /// listens, a worst-case rotation, to take the strongest node it heard
    /// at its end (spec "Relay choice").
    choosing: Option<Instant>,
    /// When a node that sleeps expects its anchor's next heartbeat on its
    /// channel, and from when it expected the last.
    watch_at: Option<Instant>,
    watch_base: Instant,
    /// Until when a sleepy node listens after its beat or a message it
    /// took (spec "Delivery").
    awake_until: Instant,
    /// When the window it listens in ends, while it is awake.
    window_ends: Option<Instant>,
    /// Until when the driver was told the radio may receive duty-cycled
    /// (`relay/anchor.rs`).
    sniff_until: Option<Instant>,
    /// When the bucket will fit the DATA to a sleepy node the send queue
    /// last had no room for, as a hand-over the bucket cannot fit yet is
    /// deferred.
    feed_at: Option<Instant>,
    /// When queued deliveries are next retried (spec "Delivery queue":
    /// every 60 s).
    delivery_retry_at: Instant,
    /// When the bucket lets the application send, if only it is missing.
    app_ready_at: Option<Instant>,
    /// Before this the application is not offered a send: the wait the
    /// last refusal of its message asked for
    /// (spec "A sleepy node's own message").
    retry_at: Instant,
    /// When the bucket will have refilled what the senders it refused
    /// were told to come back for (spec "Bans").
    promised: Instant,
    /// The move rule (spec "Channel moves"): since when the home's excess
    /// has lasted, and when to move.
    excess_since: Option<Instant>,
    move_at: Option<Instant>,
}

impl Timers {
    const UNSET: Timers = Timers {
        boot_from: Instant::ZERO,
        boot_ends: None,
        rescan_ends: None,
        choosing: None,
        watch_at: None,
        watch_base: Instant::ZERO,
        awake_until: Instant::ZERO,
        window_ends: None,
        sniff_until: None,
        feed_at: None,
        delivery_retry_at: Instant::ZERO,
        app_ready_at: None,
        retry_at: Instant::ZERO,
        promised: Instant::ZERO,
        excess_since: None,
        move_at: None,
    };

    /// The earliest deadline armed that needs the core when it comes.
    /// Those that do not: `watch_at`, whose window `window_ends` carries;
    /// `sniff_until`, the driver's; `retry_at`, which `app_ready_at`
    /// carries; `promised` and `excess_since`, which are not waited for.
    fn next(&self) -> Option<Instant> {
        [
            self.boot_ends,
            self.rescan_ends,
            self.choosing,
            self.window_ends,
            self.feed_at,
            self.app_ready_at,
            self.move_at,
        ]
        .into_iter()
        .flatten()
        .min()
    }

    /// The move rule starts over.
    fn clear_move(&mut self) {
        self.excess_since = None;
        self.move_at = None;
    }
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
    /// Heard table entries, expired ones not yet removed included.
    pub heard: usize,
    /// Neighbour table entries, expired ones not yet removed included (spec "Neighbour table").
    pub neighbours: usize,
    /// Of `send_queue`, the frames in hop places (spec "Send queue").
    pub hop_places: usize,
    /// Of `send_queue`, the frames waiting for a ban to end (spec "Overload waits").
    pub waiting: usize,
    /// Of `send_queue`, returns to the parent (spec "Return to parent").
    pub returns: usize,
}

/// A node that is not sleepy: its heard table and delivery queue at the
/// design's sizes (spec "Heard table", "Delivery queue").
pub type Relay = Node<HEARD, DELIVERY_QUEUE>;

/// A node built only to be sleepy: no heard table and no delivery queue,
/// which a sleepy node does not keep (spec "Sleepy nodes"), so that a
/// driver of a node that never switches to a relay does not spend their
/// memory; it stays sleepy whatever its power (`relay/power.rs`). A
/// [`Relay`] on battery behaves the same and keeps them empty. The board
/// runs a [`Relay`], since its node turns a relay on USB.
pub type SleepyNode = Node<0, 0>;

/// The node, with room for `H` sleepy nodes heard and `Q` messages for
/// them (`docs/core-interface.md` §6).
///
/// Laid out as declared: what an event reads first, in its parts, the
/// tables after, each with its few fields before its items, so that an
/// event touches few pages of memory. A simulator holds tens of thousands
/// of nodes, and in a city-sized run the pages it touches, not its
/// instructions, set its speed.
#[repr(C)]
pub struct Node<const H: usize, const Q: usize> {
    stage: Stage,
    mode: Mode,
    /// What it knows of its battery (`relay/power.rs`).
    battery: Battery,
    config: Config,
    timers: Timers,
    radio: Radio,
    /// What forwarding keeps beside the send queue (`relay/forward.rs`).
    forwarding: Forwarding,
    /// The application's message (`relay/forward.rs`).
    own: OwnMessage,
    /// Whether it is fixed or carried (`relay/motion.rs`).
    motion: Motion,
    /// A sleepy node's searches for a fix (`relay/gps.rs`).
    gps: gps::Gps,
    /// What the node last told its driver to keep across a reboot, or
    /// was handed back at boot (`store.rs`).
    stored: Option<Stored>,
    /// This node's ID: `config.node_id`, or drawn from the seed.
    id: NodeId,
    home: Channel,
    plane: LocalPlane,
    rng: Rng,
    delivered: Delivered,
    /// The sleepy nodes heard, for delivery.
    heard: Heard<H>,
    delivery: DeliveryQueue<Q>,
    neighbours: Neighbours,
    queue: SendQueue,
    records: Records,
}

impl<const H: usize, const Q: usize> Node<H, Q> {
    /// A relay until its power says otherwise, unless it has no room for
    /// a relay's mail (`relay/power.rs`): then sleepy at `at`.
    const fn mode_at_boot() -> Mode {
        if Self::can_relay() {
            Mode::Relay(Power::External)
        } else {
            Mode::Sleepy(Sleep::new(false))
        }
    }

    /// A relay with empty tables and the default configuration, not set
    /// up: built at compile time, so that a driver can keep a relay in a
    /// static and set it up in place with [`Relay::reset`]. A relay is
    /// 150 KiB, and [`Relay::new`] needs about as much again on the stack
    /// (`docs/core-interface.md` §6).
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
            battery: Battery::at_boot(None),
            radio: Radio::new(&config, channel, channel),
            neighbours: Neighbours::new(NeighbourParams::from_config(&config)),
            records: Records::new(),
            queue: SendQueue::new(),
            forwarding: Forwarding::IDLE,
            heard: Heard::from_config(&config),
            motion: Motion::at_boot(&config),
            gps: gps::Gps::DEFAULT,
            delivery: DeliveryQueue::from_config(&config),
            delivered: Delivered::new(),
            own: OwnMessage::NONE,
            config,
            timers: Timers::UNSET,
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
        // large tables are emptied where they lie.
        let Node {
            config: c,
            timers,
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
            delivered,
            own,
        } = self;
        *stored = config.stored;
        *id = node_id;
        *plane = LocalPlane::new(config.position);
        *h = home;
        *stage = Stage::Off;
        *mode = Self::mode_at_boot();
        *battery = Battery::at_boot(config.stored.and_then(|s| s.battery_scale));
        *radio = Radio::new(
            &config,
            Channel::all().next().expect("the plan has channels"),
            rotation,
        );
        *neighbours = Neighbours::new(NeighbourParams::from_config(&config));
        records.clear();
        queue.clear();
        *forwarding = Forwarding::IDLE;
        *heard = Heard::from_config(&config);
        *motion = Motion::at_boot(&config);
        *gps = gps::Gps::DEFAULT;
        *delivery = DeliveryQueue::from_config(&config);
        *delivered = Delivered::new();
        *own = OwnMessage::NONE;
        *timers = Timers::UNSET;
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
        self.timers.retry_at
    }

    /// This node's ID (spec "Addresses").
    pub fn id(&self) -> NodeId {
        self.id
    }

    /// The position in this node's address (spec "Addresses"): its own, or
    /// for a sleepy node its anchor's while it has one. It is the position
    /// the node works by: it beats it, centres its plane on it and
    /// rebuilds the short positions it hears near it (spec "Beats").
    /// `Config::position` stays its own place, kept for when it is a
    /// relay again (`relay/motion.rs`).
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
    /// listen: it beats with the anchorless bit and the long preamble, and
    /// has no window (spec "Sleepy nodes alone").
    pub fn anchorless(&self) -> bool {
        self.sleep().is_some_and(|s| s.anchor.is_none()) && self.stage == Stage::Running
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
        self.radio.excursion.idle()
            && self.handover_idle()
            && !self.radio.replies.pending()
            && self.radio.beacon.current.is_none()
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

    /// The busy estimates and noise floor.
    pub fn busy(&self) -> &BusyEstimator {
        &self.radio.busy
    }

    /// The messages waiting in the delivery queue for their nodes (spec
    /// "Delivery queue"). For a driver to sample; reading it changes
    /// nothing.
    pub fn delivery_queue(&self) -> impl Iterator<Item = (MsgId, NodeId)> + '_ {
        self.delivery.iter().map(|q| (q.msg_id, q.node))
    }

    /// The messages this relay holds: queued to send, or waiting for
    /// their sleepy node. A driver uses it to tell a message still in the
    /// network from one lost without a trace.
    pub fn holding(&self) -> impl Iterator<Item = MsgId> + '_ {
        self.queue
            .iter()
            .filter_map(|o| o.msg_id())
            .chain(self.delivery.iter().map(|q| q.msg_id))
    }

    /// Whether this relay has a record of message `id`: it has received
    /// or originated it (spec "Message records"). For a driver to sample;
    /// reading it changes nothing.
    pub fn knows(&self, id: MsgId) -> bool {
        self.records.get(id).is_some()
    }

    /// How many records this relay keeps, and, once the table is full,
    /// how long its least recently active record has been idle at `now`:
    /// about how long a record lasts here (spec "Message records"). For a
    /// driver to sample; reading it changes nothing.
    pub fn records(&self, now: Instant) -> (usize, Option<Duration>) {
        let idle = self.records.least_recent().map(|t| now.since(t));
        (self.records.len(), idle)
    }

    /// The lowest count a packet from a relay may carry to be taken into
    /// a hop place now, if any is free (spec "Send queue"). For a driver to sample;
    /// reading it changes nothing.
    pub fn lowest_free_hop_place(&self) -> Option<u8> {
        self.queue.lowest_free_place()
    }

    /// The highest count this relay would send, of the forwards in its
    /// send queue that could go to the neighbour `to`: live, not handed the
    /// message, not its parent (spec "Next hop", "Hop count"). For a driver
    /// to sample; reading it changes nothing.
    pub fn highest_count_for(&self, to: NodeId, now: Instant) -> Option<u8> {
        let (slot, n) = self.neighbours.by_id(to)?;
        if self.neighbours.is_expired(n, now) {
            return None;
        }
        self.queue
            .iter()
            .filter(|o| o.kind == Kind::Forward && !o.held())
            .filter_map(|o| self.records.get(o.msg_id()?))
            .filter(|r| !r.handed(slot) && r.parent().map(|(p, _)| p) != Some(to))
            .map(|r| r.highest_hops().saturating_add(1))
            .max()
    }

    /// Whether the hand-over under way takes a message from the delivery
    /// queue to its node whether or not the node is in a window: the
    /// flush after a far fix, or to a node heard as a relay
    /// (`relay/delivery.rs`). For a driver to sample; reading it changes
    /// nothing.
    pub fn sending_direct(&self) -> bool {
        self.in_flight().is_some_and(|o| o.kind == Kind::Direct)
    }

    /// Whether a hand-over is under way: begun and not yet answered or
    /// given up, waits for a backoff, a retry or the bucket included. For
    /// a driver to sample; reading it changes nothing.
    pub fn handing_over(&self) -> bool {
        !self.handover_idle()
    }

    /// How loaded the relay is at `now`, for a driver to sample. Reading
    /// it changes nothing.
    pub fn load(&self, now: Instant) -> Load {
        Load {
            bucket: self.radio.budget.available(now),
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
        }
    }

    /// What the node keeps across a reboot now (`store.rs`). A carried
    /// node's place changes with each fix, and a carried sleepy node's
    /// anchor and home as it goes: it keeps the place it stood when last
    /// fixed, a sleepy one the home it had then, and no anchor, since one
    /// that boots where it has gone chooses afresh.
    fn to_keep(&self) -> Stored {
        let carried = self.motion.carried;
        let kept = self.stored.filter(|_| carried);
        Stored {
            home: match kept {
                Some(s) if self.sleepy() => s.home,
                _ => self.home,
            },
            place: kept.map_or(self.motion.place(), |s| s.place),
            carried,
            map_point: kept.map_or(self.motion.map_point, |s| s.map_point),
            anchor: self.anchor().filter(|_| !carried),
            // Kept only where it is read (`relay/power.rs`), so that a node
            // without battery levels writes no more than before.
            relay: !self.sleepy() && self.config.battery.is_some(),
            battery_scale: self.battery.scale,
        }
    }

    /// Tell the driver to keep what changed.
    fn keep(&mut self, emit: &mut dyn FnMut(Action)) {
        // A node with no place has none to keep: it boots without one.
        if !self.motion.placed {
            return;
        }
        let now = self.to_keep();
        if self.stored != Some(now) {
            self.stored = Some(now);
            emit(Action::Store(now));
        }
    }

    /// Fire what is due at `now`: the first rotation's end, the check of
    /// the anchor, the first choice, the anchor's heartbeat and the move
    /// rule, then the power's, the GPS's, the forwarding's and the
    /// delivery's deadlines, in that order.
    fn fire(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        if self.timers.boot_ends.is_some_and(|t| now >= t) {
            self.finish_boot(now, emit);
        }
        if let Some(ends) = self.timers.rescan_ends.filter(|&t| now >= t) {
            self.anchor_checked(ends - self.config.boot_listen, now, emit);
        }
        if self.timers.choosing.is_some_and(|t| now >= t) {
            self.reconsider_anchor(now, emit);
        }
        self.watch_anchor(now, emit);
        if self.timers.move_at.is_some_and(|t| now >= t) {
            self.timers.clear_move();
            // Re-check after the random wait (spec "Channel moves").
            if self.excess() {
                let old = self.home;
                let to = self.quietest();
                self.retune(to, emit);
                if self.home != old {
                    self.radio.beacon.announce = Some(old);
                    self.take_listeners(old, now);
                }
            }
        }
        self.power_timers(now, emit);
        self.gps_timers(now, emit);
        self.forward_timers(now, emit);
        self.delivery_timers(now);
    }

    /// When the core next needs a wakeup: the earliest of its deadlines,
    /// of what the power, the GPS, the forwarding and the delivery derive
    /// from their state, and of when a job is next ready for the radio.
    fn due_at(&self) -> Option<Instant> {
        let jobs = JOBS.iter().filter_map(|&job| self.ready_at(job));
        [
            self.timers.next(),
            self.grace_ends(),
            self.flush_ends(),
            self.gps_wakeup(),
            self.forward_deadline(),
            self.delivery_wakeup(),
        ]
        .into_iter()
        .flatten()
        .chain(jobs)
        .min()
    }

    /// Serve the radio: feed the send queue, then start the first job that
    /// is ready, in [`JOBS`]'s order, until one has taken the radio or
    /// none is ready. A job that was ready but needed no excursion lets
    /// the next be tried.
    fn schedule(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        self.release_held(now, emit);
        if self.forwarding.feed_due || self.timers.feed_at.is_some_and(|t| t <= now) {
            self.feed(false, now);
        }
        while self.radio.excursion.idle() {
            self.prepare(now, emit);
            if !JOBS.iter().any(|&job| self.start_job(job, now, emit)) {
                break;
            }
        }
    }

    /// Start `job` if it is ready. The heartbeat and the sweep would take
    /// the node off its home channel, where the ACK of a hand-over
    /// arrives, so neither starts while one is awaited.
    fn start_job(&mut self, job: Job, now: Instant, emit: &mut dyn FnMut(Action)) -> bool {
        match job {
            Job::Reply => self.start_reply(now, emit),
            Job::Heartbeat => {
                !self.awaiting_ack() && self.beacon_due(now) && self.start_heartbeat(now, emit)
            }
            Job::HandOver => self.start_handover(now, emit),
            Job::Sweep => !self.awaiting_ack() && self.sweep_due(now) && self.start_sweep(emit),
        }
    }

    /// When `job` is next ready of its own accord, if it will be. Jobs
    /// that need the radio free say so only while it is: its completion
    /// event wakes the core otherwise. The hand-over's ACK wait counts
    /// even while an owed reply holds the radio.
    fn ready_at(&self, job: Job) -> Option<Instant> {
        let free = self.radio.excursion.idle();
        match job {
            Job::Reply => self.radio.replies.wake_at().filter(|_| free),
            Job::Heartbeat => {
                if !free || self.awaiting_ack() || !self.heartbeats() {
                    return None;
                }
                let pending = self.radio.beacon.announce.is_some()
                    || self.radio.beacon.current.is_some()
                    || self.radio.beacon.change.is_some();
                let due = if pending {
                    self.radio.beacon.ready_at
                } else {
                    // A relay's answer may come before its next slot.
                    let answer = self.answer_due().map(|(_, _, at)| at);
                    answer.map_or(self.radio.beacon.next_at, |t| {
                        t.min(self.radio.beacon.next_at)
                    })
                };
                Some(due.max(self.radio.beacon.ready_at))
            }
            Job::HandOver => {
                let flight = self.flight_wake().filter(|_| self.awaiting_ack() || free);
                // A waiting entry, once nothing else is under way.
                let queued = self
                    .queue
                    .earliest_due()
                    .filter(|_| self.handover_idle() && free);
                earlier(flight, queued)
            }
            Job::Sweep => (free && !self.awaiting_ack() && self.sweeping())
                .then_some(self.radio.sweep.next_at),
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
            Ok(Packet::Heartbeat(hb)) => self.heard(hb, rssi, now, emit),
            Ok(Packet::Ack(ack)) => self.ack_received(ack, now, emit),
            Ok(Packet::Data(d)) if d.dst_hop == self.me() => self.data(d, now, emit),
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
            AppCommand::Battery { millivolts } => self.battery_read(millivolts, now),
            AppCommand::ClearMapPoint => self.clear_map_point(),
            AppCommand::Fix { position } => {
                self.gps_fixed();
                self.fix(position, now, emit);
                self.gps_after_fix();
            }
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
        // Any use of the radio ends a sniff (core-interface §3 rule 10).
        let woke = core::cell::Cell::new(false);
        let mut wake = |a: Action| {
            let radio = matches!(
                a,
                Action::Listen(_) | Action::Lbt(_) | Action::Rssi(_) | Action::Transmit(_)
            );
            woke.set(woke.get() || radio);
            emit(a)
        };
        let emit: &mut dyn FnMut(Action) = &mut wake;
        if self.stage == Stage::Off {
            self.start(now, emit);
        }
        let threshold = self.radio.busy.threshold_dbm();
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
                match self.radio.excursion.complete() {
                    Some(Job::Heartbeat) => self.heartbeat_check(busy, now, emit),
                    Some(Job::HandOver) => self.handover_check(busy, now, emit),
                    Some(Job::Reply) => self.reply_check(busy, now, emit),
                    Some(Job::Sweep) | None => {}
                }
            }
            Event::RssiSample { channel, dbm } => match self.radio.excursion.complete() {
                Some(Job::Sweep) => self.sweep_sample(channel, dbm, now),
                Some(Job::Heartbeat | Job::HandOver | Job::Reply) | None => {}
            },
            Event::TxDone => match self.radio.excursion.complete() {
                Some(Job::Heartbeat) => self.heartbeat_done(now, true),
                // Unless the hand-over was given up meanwhile (a displaced record).
                Some(Job::HandOver) => {
                    let timeout = self.config.ack_timeout;
                    if let Some((mut h, _)) = self.handover() {
                        if h.state() == State::Radio {
                            h.sent(now, timeout);
                        }
                    }
                }
                Some(Job::Reply) => {
                    let listens_on = self.radio.replies.sent();
                    if listens_on {
                        self.took(now);
                    }
                }
                _ => {}
            },
            Event::App(cmd) => self.app(cmd, now, emit),
        }
        self.fire(now, emit);
        self.schedule(now, emit);
        self.timers.app_ready_at = self.send_ready_at(now);
        if woke.get() {
            self.timers.sniff_until = None;
        }
        self.sniff(now, emit);
        self.keep(emit);
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
