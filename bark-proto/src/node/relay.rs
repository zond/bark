//! The node: medium access, forwarding and delivery to the sleepy nodes
//! it hears. Every node is a `Relay`; [`Config::sleepy`] changes what it
//! does (spec "Sleepy nodes").
//!
//! This file holds the medium access: boot listen and home channel
//! (spec "Home channel selection"), the busy sweep (spec "Busy estimate"), the heartbeat (spec "Heartbeat"), the neighbour
//! table (spec "Neighbours"), the duty-cycle budget (spec "Regulatory limits", "Duty cycle cap"), and a sleepy
//! node's home and beat (below). Routing and the per-hop transfer (spec "Forwarding")
//! are in `relay/forward.rs`, delivery to sleepy nodes in
//! `relay/delivery.rs`.
//!
//! # Radio discipline
//!
//! Each use of the radio is a job with its own small state machine: the
//! reply (an ACK, taking or refusing) this node owes, the heartbeat, the
//! hand-over of one entry of the send queue, and the sweep. The node keeps
//! at most one excursion outstanding (`docs/core-interface.md` §3 rule 1).
//! When the radio is free it starts the first job that is ready, in that
//! order: an owed reply is worth nothing once its sender stops waiting,
//! 100 ms after the frame (spec "The ACK"). A job that has begun a sequence
//! of excursions (CAD, RSSI, transmit) continues it from each completion
//! event, so sequences never interleave; the hand-over alone lets an owed
//! reply in before it transmits, since its frame can take 139 ms on air.
//! While a hand-over waits for its ACK, only an owed reply uses the radio:
//! the heartbeat and the sweep would take the node off its home channel,
//! where the ACK arrives (spec "Hand-over"). Jobs that are not ready say when
//! they will be, and the earliest of those is `next_wakeup`.
//!
//! # A sleepy node's home and beat (spec "Sleepy nodes")
//!
//! It takes as its anchor the first node it hears that is neither sleepy
//! nor anchorless, at once, and listens on the anchor's home channel; its
//! address is the anchor's position and its own ID (spec "Addresses").
//! It stays awake for a worst-case rotation after that and then takes the
//! strongest node it heard, without a margin; after that it moves only to
//! one heard over two heartbeats and [`SWITCH_MARGIN_DB`] stronger (spec
//! "Relay choice"). Back from the dark, it keeps its anchor. It
//! follows its anchor to a new home channel at the anchor's next heartbeat
//! it hears, and beats there within a heartbeat interval; it does not move
//! by the busy estimate.
//!
//! With an anchor it heartbeats on its home channel only, with the sleepy
//! bit, each beat a random half to one beat period after the last
//! (`tables::heard::beat_period`, 300 s): at most a period apart, so that
//! two beats missed are a delivery TTL; spread, so that two nodes that met
//! once do not meet again. After it chooses or
//! changes its anchor it beats at once and once more within a heartbeat
//! interval, so that one lost beat does not hide it from its new holder for
//! a period. It sweeps the channels only while it boots: receiving
//! duty-cycled it could not.
//!
//! Between its windows it receives duty-cycled, anchor or not, so any node
//! can reach it with the long preamble (`crate::sniff`). It holds mail for
//! nobody: it keeps no heard table and no delivery queue
//! ([`SleepyNode`]), owns only messages addressed to its ID, and forwards
//! what it is given at once (spec "Sleepy nodes"). Its neighbours keep it
//! as a last resort (spec "Next hop"); where its neighbour table holds
//! anchorless nodes it sends each beat on each of their channels too,
//! with the long preamble, so that they learn where it listens (spec
//! "Beats").
//!
//! Without an anchor it is anchorless ([`Node::anchorless`], spec
//! "Sleepy nodes alone"): it has no window, so every frame to it carries
//! the long preamble. It beats with the anchorless bit and the long
//! preamble, each beat a random half to one anchorless beat period, 46.2 s,
//! after the last, on the next channel each time, so that a relay on any
//! channel hears it within a rotation; nobody takes it as an anchor, since
//! it leaves the mode at the first relay it hears. A relay answers each
//! such beat it hears with a heartbeat on the node's channel with the long
//! preamble, at once and a fade later, which the node takes as the first
//! relay heard. Its home is the quietest channel at the end of its boot
//! listen, kept.
//!
//! # Duty-cycled receive (`docs/core-interface.md` §3 rule 10)
//!
//! A sleepy node tells its driver the radio may receive duty-cycled
//! ([`Action::Sniff`]) whenever it has nothing under way and is neither
//! in a window, after its beat or a message it took (spec "Delivery"),
//! nor listening for its anchor's heartbeat or to choose or check its
//! anchor (spec "Relay choice"), until it next needs the radio, for its
//! next beat or the next offer of a message it waits to send, unless the
//! core starts an excursion sooner. With [`Config::sleepy_listens`], a
//! research switch, it never does.

mod delivery;
mod forward;

use crate::event::{Action, AppCommand, Event, Frame, Trace};
use crate::geo::LocalPlane;
use crate::ids::{MsgId, NodeId};
use crate::node::mac::{self, Radio};
use crate::node::transfer::{Replies, Step, Transfer};
use crate::node::{heartbeat_interval, heartbeat_lag, Config, Core, Rng};
use crate::position::Position;
use crate::radio::{time_on_air, Channel, CHANNELS, MIN_PREAMBLE_SYMBOLS};
use crate::sniff::{anchorless_beat_period, LONG_PREAMBLE_SYMBOLS};
use crate::tables::budget::Budget;
use crate::tables::busy::{percent, BusyEstimator, ONE};
use crate::tables::delivered::Delivered;
use crate::tables::delivery::{DeliveryQueue, DELIVERY_QUEUE};
use crate::tables::heard::{beat_period, Heard, HEARD};
use crate::tables::neighbours::{Neighbour, NeighbourParams, Neighbours, BEATS};
use crate::tables::records::Records;
use crate::tables::send_queue::{Kind, SendQueue};
use crate::time::{earlier, Duration, Instant};
use crate::wire::{Heartbeat, Packet, HEARTBEAT_LEN};
use forward::OWNER_RADIUS_M;

/// Who started the outstanding excursion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Job {
    Sweep,
    Heartbeat,
    HandOver,
    Reply,
}

/// How much stronger, in the averaged RSSI of the neighbour table, another
/// node must be to take a sleepy node from its anchor
/// (spec "Relay choice"): two standard deviations of the difference of two means of
/// two heartbeats each under the scenarios' 4 dB fading.
/// The table's average lets each new frame count half, which varies less
/// than a mean of two (σ²/3 against σ²/2), so 8 dB is a little more than
/// two standard deviations here.
pub const SWITCH_MARGIN_DB: f32 = 8.0;

/// The most a node's clock may run fast or slow, parts per million: the
/// tolerance of a common 32.768 kHz watch crystal, which the board's
/// low-frequency clock runs from (nRF52840 product specification, LFXO).
/// To be measured on the boards.
pub const CLOCK_DRIFT_PPM: u64 = 50;

/// How far two clocks, each off by up to [`CLOCK_DRIFT_PPM`], drift apart
/// over `d`.
fn drift(d: Duration) -> Duration {
    Duration::from_micros(d.as_micros() * 2 * CLOCK_DRIFT_PPM / 1_000_000)
}

/// The path-loss exponent of the scenarios, fitted to the ranges of spec
/// "Physical layer" (`scenarios/s4-capacity.toml`).
pub const PATH_LOSS_EXPONENT: f32 = 3.6;

/// How far a node that sleeps may move from where it chose or confirmed
/// its anchor, `reach_m` away then, before it listens a rotation to check
/// it (spec "Relay choice"): as far as takes the anchor's signal down by
/// [`SWITCH_MARGIN_DB`] when moving straight away from it,
/// `reach × (10^(margin / 10n) − 1)`, 0.67 of the distance to it; and no
/// less than two GPS fixes of a node that has not moved may differ by
/// ([`OWNER_RADIUS_M`]).
pub fn move_bound_m(reach_m: f32) -> f32 {
    let ratio = libm::powf(10.0, SWITCH_MARGIN_DB / (10.0 * PATH_LOSS_EXPONENT));
    (reach_m * (ratio - 1.0)).max(OWNER_RADIUS_M)
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    /// Not started: the driver's first `Wakeup` starts it.
    Off,
    /// Listening before choosing a home channel (spec "Channel at boot"), until then.
    Booting(Instant),
    Running,
}

/// Listen-before-talk progress of one heartbeat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Check {
    Idle,
    Cad,
    Rssi,
    Tx,
}

/// The busy sweep (spec "Busy estimate"): one RSSI reading at a time, channels in
/// turn, spread over the sweep interval.
struct Sweep {
    next_at: Instant,
    /// The channel the next reading takes.
    channel: Channel,
    /// The last home reading, held until any reception it fell in has
    /// ended and shown whether the energy it read was this relay's own.
    pending: Option<HomeReading>,
}

/// A reading of the home channel awaiting [`Sweep::pending`]'s verdict.
#[derive(Clone, Copy)]
struct HomeReading {
    channel: Channel,
    at: Instant,
    /// `None` once a packet addressed to this relay turned out to be on
    /// the air when it was taken.
    dbm: Option<i16>,
}

/// The rotating heartbeat (spec "Heartbeat") and the one-off announcement on the
/// old channel after a move (spec "Channel moves").
struct Beacon {
    next_at: Instant,
    /// Not before this: a listen-before-talk backoff or a budget deferral.
    ready_at: Instant,
    rotation: Channel,
    /// The old home, to announce the new one on, after a move.
    announce: Option<Channel>,
    /// The heartbeat in progress: its channel, and which it is.
    current: Option<(Channel, Beat)>,
    busy_checks: u8,
    check: Check,
    /// A sleepy node's beats still to come at the heartbeat interval after
    /// a change of anchor.
    quick: u8,
    /// Of a sleepy node with an anchor, the channels its anchorless
    /// neighbours listen on, as a mask, taken when its beat came due (spec
    /// "Beats"), and those of them still to beat on with the long preamble
    /// before the beat itself goes.
    reach_all: Option<u16>,
    reach_left: u16,
}

/// Which heartbeat the beacon sends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Beat {
    /// The one of its slot: on the rotation's channel, or a sleeping
    /// node's beat on its home.
    Slot,
    /// The one after a move, on the old channel (spec "Channel moves").
    Announce,
    /// A relay's answer to an anchorless node's beat, on that node's
    /// channel with the long preamble (spec "Sleepy nodes alone").
    Answer(NodeId),
    /// A sleepy node's beat, with the long preamble, on a channel its
    /// anchorless neighbours listen on (spec "Beats").
    Reach,
}

/// The move rule's timers (spec "Channel moves").
#[derive(Default)]
struct Mover {
    excess_since: Option<Instant>,
    move_at: Option<Instant>,
}

/// A snapshot of a relay's tables and budget ([`Relay::load`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Load {
    /// Airtime in the duty-cycle bucket (spec "Duty cycle cap").
    pub bucket: Duration,
    /// Frames in the send queue, hop places included (spec "Send queue").
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
/// driver of a sleepy node does not spend their memory. A [`Relay`] set up
/// sleepy behaves the same and keeps them empty.
pub type SleepyNode = Node<0, 0>;

/// The node, with room for `H` sleepy nodes heard and `Q` messages for
/// them (`docs/core-interface.md` §6).
pub struct Node<const H: usize, const Q: usize> {
    config: Config,
    /// This node's ID: `config.node_id`, or drawn from the seed.
    id: NodeId,
    plane: LocalPlane,
    rng: Rng,
    home: Channel,
    stage: Stage,
    radio: Radio<Job>,
    busy: BusyEstimator,
    budget: Budget,
    neighbours: Neighbours,
    sweep: Sweep,
    beacon: Beacon,
    mover: Mover,
    records: Records,
    queue: SendQueue,
    /// The hand-over of `queue`'s current entry.
    handover: Transfer,
    /// Neighbour slots the current forward passed over because
    /// listen-before-talk gave up on them, they refused it, or its
    /// attempts went unanswered (spec "Hand-over"); not tried.
    skipped: u64,
    replies: Replies,
    /// The sleepy nodes heard, for delivery.
    heard: Heard<H>,
    /// A sleepy node's anchor, kept while it hears no better one.
    anchor: Option<Anchor>,
    delivery: DeliveryQueue<Q>,
    delivered: Delivered,
    /// When queued deliveries are next retried (spec "Delivery queue": every 60 s).
    delivery_retry_at: Instant,
    /// When the bucket lets the application send, if only it is missing.
    app_ready_at: Option<Instant>,
    /// The application's message, from when it is taken until its first
    /// hop takes it, it arrives here, or it is dropped (roadmap D29).
    own: Option<MsgId>,
    /// Refusals in a row of `own` by banned neighbours: each doubles the
    /// hold before it is offered again (`budget::hold`).
    own_refusals: u8,
    /// Before this the application is not offered a send: the wait the
    /// last refusal of its message asked for (spec "A sleepy node's own message").
    retry_at: Instant,
    /// When the bucket will have refilled what the senders it refused
    /// were told to come back for (spec "Refusal").
    promised: Instant,
    /// The application's message for a sleepy node this relay holds for, set aside
    /// while the delivery queue is full.
    held: Option<forward::Held>,
    /// The send queue may have room for the delivery queue: a hand-over
    /// ended or the delivery queue changed.
    feed_due: bool,
    /// When the bucket will fit the DATA to a sleepy node the send queue last had no
    /// room for, as a hand-over the bucket cannot fit yet is deferred.
    feed_at: Option<Instant>,
    /// Until when the driver was told the radio may receive duty-cycled
    /// (module documentation).
    sniffing: Option<Instant>,
    /// Until when a sleepy node listens after its beat or a message it
    /// took (spec "Delivery").
    awake_until: Instant,
    /// Until when a sleepy node that has just taken its first anchor
    /// listens, a worst-case rotation, to take the strongest node it heard
    /// at its end (spec "Relay choice").
    choosing: Option<Instant>,
    /// The owed reply takes a DATA for this node: once it is sent, the
    /// node listens one hand-over for the next.
    renew: bool,
    /// Since when a node that sleeps listens a rotation to check its
    /// anchor.
    rescan: Option<Instant>,
    /// When the window it listens in ends, while it is awake.
    window_ends: Option<Instant>,
    /// When a node that sleeps expects its anchor's next heartbeat on its
    /// channel, from when it expected the last, and the rotations since it
    /// last heard one there.
    watch_at: Option<Instant>,
    watch_base: Instant,
    misses: u8,
    /// Its last check heard nothing at all: it is out of reach, and checks
    /// again only once it hears something.
    dark: bool,
    /// Where the node was when it chose or last confirmed its anchor, and
    /// how far the anchor was then, metres.
    chosen_from: Position,
    reach_m: f32,
}

impl<const H: usize, const Q: usize> Node<H, Q> {
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
            id: NodeId::NONE,
            plane: LocalPlane::ORIGIN,
            home: channel,
            stage: Stage::Off,
            radio: Radio::new(),
            busy: BusyEstimator::from_config(&config),
            budget: Budget::new(),
            neighbours: Neighbours::new(NeighbourParams::from_config(&config)),
            sweep: Sweep {
                next_at: Instant::ZERO,
                channel,
                pending: None,
            },
            beacon: Beacon {
                next_at: Instant::ZERO,
                ready_at: Instant::ZERO,
                rotation: channel,
                announce: None,
                current: None,
                busy_checks: 0,
                check: Check::Idle,
                quick: 0,
                reach_all: None,
                reach_left: 0,
            },
            mover: Mover {
                excess_since: None,
                move_at: None,
            },
            records: Records::new(),
            queue: SendQueue::new(),
            handover: Transfer::new(),
            skipped: 0,
            replies: Replies::new(),
            heard: Heard::from_config(&config),
            anchor: None,
            delivery: DeliveryQueue::from_config(&config),
            delivered: Delivered::new(),
            delivery_retry_at: Instant::ZERO,
            app_ready_at: None,
            own: None,
            own_refusals: 0,
            retry_at: Instant::ZERO,
            promised: Instant::ZERO,
            held: None,
            feed_due: false,
            feed_at: None,
            sniffing: None,
            awake_until: Instant::ZERO,
            choosing: None,
            renew: false,
            rescan: None,
            window_ends: None,
            watch_at: None,
            watch_base: Instant::ZERO,
            misses: 0,
            dark: false,
            chosen_from: Position { lat: 0, lon: 0 },
            reach_m: 0.0,
            config,
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
        assert!(
            config.sleepy || (H > 0 && Q > 0),
            "a node that is not sleepy keeps a heard table and a delivery queue"
        );
        let mut rng = Rng::new(seed);
        let mut node_id = config.node_id;
        while node_id == NodeId::NONE {
            node_id = NodeId((rng.next_u64() as u32).to_be_bytes());
        }
        let home = config
            .home_override
            .unwrap_or_else(|| random_channel(&mut rng));
        let rotation = random_channel(&mut rng);
        // Every field by name, so that a new one cannot be left out. The
        // large tables are emptied where they lie.
        let Node {
            config: c,
            id,
            plane,
            rng: r,
            home: h,
            stage,
            radio,
            busy,
            budget,
            neighbours,
            sweep,
            beacon,
            mover,
            records,
            queue,
            handover,
            skipped,
            replies,
            heard,
            anchor,
            delivery,
            delivered,
            delivery_retry_at,
            app_ready_at,
            own,
            own_refusals,
            retry_at,
            promised,
            held,
            feed_due,
            feed_at,
            sniffing,
            awake_until,
            choosing,
            renew,
            rescan,
            window_ends,
            watch_at,
            watch_base,
            misses,
            dark,
            chosen_from,
            reach_m,
        } = self;
        *id = node_id;
        *plane = LocalPlane::new(config.position);
        *h = home;
        *stage = Stage::Off;
        *radio = Radio::new();
        *busy = BusyEstimator::from_config(&config);
        *budget = Budget::new();
        *neighbours = Neighbours::new(NeighbourParams::from_config(&config));
        *sweep = Sweep {
            next_at: Instant::ZERO,
            channel: Channel::all().next().expect("the plan has channels"),
            pending: None,
        };
        *beacon = Beacon {
            next_at: Instant::ZERO,
            ready_at: Instant::ZERO,
            rotation,
            announce: None,
            current: None,
            busy_checks: 0,
            check: Check::Idle,
            quick: 0,
            reach_all: None,
            reach_left: 0,
        };
        *mover = Mover::default();
        records.clear();
        queue.clear();
        *handover = Transfer::new();
        *skipped = 0;
        *replies = Replies::new();
        *heard = Heard::from_config(&config);
        *anchor = None;
        *delivery = DeliveryQueue::from_config(&config);
        *delivered = Delivered::new();
        *delivery_retry_at = Instant::ZERO;
        *app_ready_at = None;
        *own = None;
        *own_refusals = 0;
        *retry_at = Instant::ZERO;
        *promised = Instant::ZERO;
        *held = None;
        *feed_due = false;
        *feed_at = None;
        *sniffing = None;
        *awake_until = Instant::ZERO;
        *choosing = None;
        *renew = false;
        *rescan = None;
        *window_ends = None;
        *watch_at = None;
        *watch_base = Instant::ZERO;
        *misses = 0;
        *dark = false;
        *chosen_from = config.position;
        *reach_m = 0.0;
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
        self.retry_at
    }

    /// This node's ID (spec "Addresses").
    pub fn id(&self) -> NodeId {
        self.id
    }

    /// The position in this node's address (spec "Addresses"): its own, or
    /// for a sleepy node its anchor's while it has one.
    pub fn address(&self) -> Position {
        match (self.config.sleepy, self.anchor) {
            (true, Some(a)) => a.position,
            _ => self.config.position,
        }
    }

    /// A sleepy node's anchor, if it has chosen one.
    pub fn anchor(&self) -> Option<Anchor> {
        self.anchor
    }

    /// Whether this is a sleepy node with an anchor: it beats on its
    /// anchor's channel and listens in the windows after its beats.
    fn anchored_sleepy(&self) -> bool {
        self.config.sleepy && self.anchor.is_some()
    }

    /// How often a sleepy node without an anchor beats, at most (spec
    /// "Sleepy nodes alone").
    fn anchorless_period(&self) -> Duration {
        anchorless_beat_period(self.config.delivery_ttl)
    }

    /// Whether this node is a sleepy node without an anchor, past its boot
    /// listen: it beats with the anchorless bit and the long preamble, and
    /// has no window (spec "Sleepy nodes alone").
    pub fn anchorless(&self) -> bool {
        self.config.sleepy && self.anchor.is_none() && self.stage == Stage::Running
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
            && self.handover.idle()
            && !self.replies.pending()
            && self.beacon.check == Check::Idle
            && self.beacon.current.is_none()
    }

    /// Whether the radio may receive duty-cycled at `now` (module
    /// documentation). Only a node with an anchor has windows: one
    /// without is sent to with the long preamble whenever.
    fn may_sniff(&self, now: Instant) -> bool {
        let window = self.anchored_sleepy() && now < self.awake_until;
        self.config.sleepy
            && !self.config.sleepy_listens
            && self.stage == Stage::Running
            && !window
            && self.choosing.is_none()
            && self.rescan.is_none()
            && self.radio_free()
            && !self
                .watch()
                .is_some_and(|(from, to)| now >= from && now < to)
    }

    /// When a sleepy node next needs its radio of its own accord: its
    /// next beat, or the next offer of a message it waits to send.
    fn radio_due(&self) -> Instant {
        let beat = self.beacon.next_at.max(self.beacon.ready_at);
        let due = self.queue.earliest_due().map_or(beat, |t| t.min(beat));
        self.watch().map_or(due, |(from, _)| due.min(from))
    }

    /// When a node that sleeps listens for its anchor's next heartbeat on
    /// its channel, if it does (module documentation): from as early as it
    /// can come to as late as it can. `watch_at` is when it comes if it
    /// starts in its slot, from the time expected of the last one heard
    /// (`anchor_heard`). It comes earlier by as much as the two clocks
    /// drift apart, [`CLOCK_DRIFT_PPM`] each, since; and by twice as much
    /// again, since the last one may have been late by what keeps a
    /// heartbeat behind its slot ([`heartbeat_lag`]) and moved the time
    /// expected later by the drift when its slot was as early as the
    /// drift allowed. It may come later by the drift, and by that lag.
    fn watch(&self) -> Option<(Instant, Instant)> {
        let at = self.watch_at?;
        let drift = drift(at.since(self.watch_base));
        let airtime = time_on_air(HEARTBEAT_LEN as u8, MIN_PREAMBLE_SYMBOLS);
        Some((
            at - drift.mul(3),
            at + drift + heartbeat_lag(&self.config) + airtime,
        ))
    }

    /// Follow the anchor's heartbeat: a node that sleeps expects it on its
    /// channel a rotation of the anchor's interval after the last
    /// (module documentation); once two rotations have passed without it,
    /// the anchor is gone, and the node listens a rotation to choose again
    /// (spec "Relay choice").
    fn watch_anchor(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        let keeps = self.anchored_sleepy() && !self.config.sleepy_listens && self.rescan.is_none();
        let Some(a) = self.anchor.filter(|_| keeps) else {
            self.watch_at = None;
            return;
        };
        let interval = heartbeat_interval(a.id, &self.config);
        if self.watch_at.is_none() {
            self.misses = 0;
            if let Some((_, n)) = self.neighbours.by_id(a.id) {
                self.watch_base = n.beat_at;
                self.watch_at = Some(Neighbours::next_beat(n, interval, self.home));
            }
        }
        while let Some((from, to)) = self.watch().filter(|&(_, to)| now >= to) {
            emit(Action::Trace(Trace::AnchorWatch {
                heard: false,
                offset_us: 0,
                listened: to.since(from),
            }));
            self.misses = self.misses.saturating_add(1);
            if self.misses >= 2 && !self.dark {
                self.watch_at = None;
                self.check_anchor(now, emit);
                return;
            }
            self.watch_at = self.watch_at.map(|t| t + interval.mul(CHANNELS as u64));
        }
    }

    /// The anchor's heartbeat that started at `now` was heard: it is alive, and its next
    /// comes to this node's channel when the rotation brings it there. A
    /// heartbeat may come late behind its slot, never early, so the time
    /// expected is kept from the earliest heard, moved later only by as
    /// much as the clocks may drift apart.
    /// A heartbeat of the anchor outside its slots was heard: the anchor
    /// is alive, and the time its next is expected stays as it was.
    fn anchor_alive(&mut self) {
        if self.rescan.is_some() {
            self.anchored();
        }
        self.misses = 0;
    }

    fn anchor_heard(&mut self, now: Instant) {
        let Some(a) = self.anchor else { return };
        if self.rescan.is_some() {
            // Heard while checking: it is there (spec "Relay choice").
            self.anchored();
        }
        self.misses = 0;
        let base = match (self.watch_at, self.watch()) {
            (Some(at), Some((from, to))) if now >= from && now < to => {
                now.min(at + drift(at.since(self.watch_base)))
            }
            _ => now,
        };
        let interval = heartbeat_interval(a.id, &self.config);
        if let Some((_, n)) = self.neighbours.by_id(a.id) {
            let n = Neighbour {
                beat_at: base,
                ..*n
            };
            self.watch_base = base;
            self.watch_at = Some(Neighbours::next_beat(&n, interval, self.home));
        }
    }

    /// Tell the driver the radio may receive duty-cycled, if it may and
    /// has not been told.
    fn sniff(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        if self.sniffing.is_some_and(|t| t > now) {
            return;
        }
        // The ends of the windows it listens in, and the start of the next
        // for its anchor's heartbeat, need the core, to say so.
        self.window_ends = None;
        if self.anchored_sleepy() {
            let ahead = |t: Option<Instant>| t.filter(|&t| t > now);
            let watch = self.watch();
            self.window_ends = earlier(
                earlier(ahead(Some(self.awake_until)), ahead(self.choosing)),
                earlier(ahead(watch.map(|w| w.0)), ahead(watch.map(|w| w.1))),
            );
        }
        if !self.may_sniff(now) {
            return;
        }
        let until = self.radio_due();
        if until > now {
            self.sniffing = Some(until);
            emit(Action::Sniff(until));
        }
    }

    /// This node has answered a DATA addressed to it: it listens one
    /// hand-over for the next (spec "Delivery").
    fn took(&mut self, now: Instant) {
        self.awake_until = now + self.heard.params().linger;
    }

    /// The heard table.
    pub fn heard_nodes(&self) -> &Heard<H> {
        &self.heard
    }

    /// Whether the boot listen (spec "Channel at boot") is still running.
    pub fn booting(&self) -> bool {
        !matches!(self.stage, Stage::Running)
    }

    /// The neighbour table.
    pub fn neighbours(&self) -> impl Iterator<Item = &Neighbour> {
        self.neighbours.iter()
    }

    /// The busy estimates and noise floor.
    pub fn busy(&self) -> &BusyEstimator {
        &self.busy
    }

    /// The messages this relay holds: queued to send, or waiting for
    /// their sleepy node. A driver uses it to tell a message still in the
    /// network from one lost without a trace.
    pub fn holding(&self) -> impl Iterator<Item = MsgId> + '_ {
        self.queue
            .iter()
            .filter_map(|o| o.msg_id())
            .chain(self.delivery.iter().map(|q| q.msg_id))
            .chain(self.held.as_ref().and_then(|h| h.out.msg_id()))
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
            .filter(|o| o.kind == Kind::Forward)
            .filter_map(|o| self.records.get(o.msg_id()?))
            .filter(|r| !r.handed(slot) && r.parent().map(|(p, _)| p) != Some(to))
            .map(|r| r.highest_hops().saturating_add(1))
            .max()
    }

    /// Whether a hand-over is under way: begun and not yet answered or
    /// given up, waits for a backoff, a retry or the bucket included. For
    /// a driver to sample; reading it changes nothing.
    pub fn handing_over(&self) -> bool {
        !self.handover.idle()
    }

    /// How loaded the relay is at `now`, for a driver to sample. Reading
    /// it changes nothing.
    pub fn load(&self, now: Instant) -> Load {
        Load {
            bucket: self.budget.available(now),
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

    fn start(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        // A sleepy node listens a rotation more: a relay switched on with
        // it heartbeats only after a boot listen of its own, and once the
        // node receives duty-cycled it hears relays only by their answers
        // (spec "Channel at boot").
        let rotations = if self.config.sleepy { 2 } else { 1 };
        self.stage = Stage::Booting(now + self.config.boot_listen.mul(rotations));
        self.sweep.next_at = now;
        emit(Action::Listen(self.home));
    }

    /// End of the boot listen: choose home (spec "Channel at boot"), or for
    /// a sleepy node an anchor, and start heartbeats.
    fn finish_boot(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        self.stage = Stage::Running;
        if self.config.sleepy {
            if self.config.home_override.is_none() {
                let home = self.quietest();
                self.retune(home, emit);
            }
            if !self.reconsider_anchor(now, emit) {
                // At a random time within a beat period, so that nodes that
                // boot together do not beat together.
                self.beacon.next_at =
                    now + self.rng.duration(Duration::ZERO, self.anchorless_period());
            }
            return;
        }
        if self.config.home_override.is_none() {
            let home = self.quietest();
            self.retune(home, emit);
        }
        // At a random phase of its interval: nodes that boot together, after
        // a power cut, would otherwise heartbeat in step whenever their
        // intervals have a small common multiple.
        let interval = heartbeat_interval(self.id, &self.config);
        self.beacon.next_at = now + self.rng.duration(Duration::ZERO, interval);
    }

    fn retune(&mut self, to: Channel, emit: &mut dyn FnMut(Action)) {
        if to != self.home {
            // What was heard on the old channel says nothing of the new.
            self.sweep.pending = None;
            emit(Action::Trace(Trace::HomeChannelMoved {
                from: self.home,
                to,
            }));
            self.home = to;
            emit(Action::Listen(to));
        }
    }

    /// `argmin busy[c]`, ties within 0.01 broken at random (spec "Channel at boot"),
    /// so nodes that hear nothing do not all pick channel 0.
    fn quietest(&mut self) -> Channel {
        let min = Channel::all()
            .map(|c| self.busy.fraction(c))
            .min()
            .unwrap_or(ONE);
        let busy = &self.busy;
        let near = |c: &Channel| busy.fraction(*c) <= min.saturating_add(percent(1));
        let count = Channel::all().filter(near).count() as u64;
        let pick = self.rng.range(0, count.saturating_sub(1)) as usize;
        Channel::all().filter(near).nth(pick).unwrap_or(self.home)
    }

    /// The time to a sleepy node's next beat: a random half to one beat
    /// period, the anchorless one without an anchor. A relay heartbeats on its fixed interval instead
    /// (`heartbeat_done`).
    fn beat_interval(&mut self) -> Duration {
        if self.beacon.quick > 0 {
            self.beacon.quick -= 1;
            return self
                .rng
                .duration(self.config.heartbeat_min, self.config.heartbeat_max);
        }
        let period = if self.anchored_sleepy() {
            beat_period(self.config.delivery_ttl)
        } else {
            self.anchorless_period()
        };
        self.rng.duration(period.div(2), period)
    }

    // --- A sleepy node's anchor (module documentation) ---

    /// Choose or keep a sleepy node's anchor among the live neighbours
    /// that are not sleepy, and follow its home channel. Returns whether
    /// the anchor changed.
    fn reconsider_anchor(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) -> bool {
        if !self.config.sleepy || self.stage != Stage::Running {
            return false;
        }
        let current = self
            .anchor
            .and_then(|a| self.neighbours.live(now).find(|(_, n)| n.id == a.id))
            .map(|(_, n)| *n);
        // Any node that is not sleepy or anchorless can hold a sleepy node's
        // messages: without an anchor it takes the first it hears
        // (spec "Relay choice").
        let strongest = |except: Option<NodeId>, beats: u8| {
            self.neighbours
                .live(now)
                .map(|(_, n)| n)
                .filter(|n| n.relay() && Some(n.id) != except && n.beats >= beats)
                .max_by(|a, b| a.rssi.total_cmp(&b.rssi))
                .copied()
        };
        let first = self.anchor.is_none();
        // The choice after the first anchor is made when its time comes,
        // whether or not the anchor is still in the table: left for later,
        // its time would wake the node at once, again and again.
        let chosen = self.choosing.is_some_and(|t| now >= t);
        if chosen {
            self.choosing = None;
        }
        let to = match current {
            None => strongest(None, 0),
            // A rotation after its first anchor, by when every node in range
            // has visited its channel, and while few can have its address:
            // the strongest heard, without the margin.
            Some(a) if chosen => strongest(Some(a.id), 0).filter(|b| b.rssi > a.rssi),
            Some(a) => strongest(Some(a.id), BEATS).filter(|b| b.rssi >= a.rssi + SWITCH_MARGIN_DB),
        };
        if let (Some(a), None) = (current, to) {
            // Kept: follow it to a new home, and to a new position.
            self.anchor = Some(Anchor {
                id: a.id,
                position: a.position,
            });
            if a.home != self.home && self.config.home_override.is_none() {
                self.retune(a.home, emit);
                // Its holder learns the new channel only from a beat there,
                // and every node of the anchor follows on the same
                // heartbeat: each beats at a random time within a heartbeat
                // interval (spec "Beats").
                let soon = now + self.rng.duration(Duration::ZERO, self.config.heartbeat_max);
                self.beacon.next_at = self.beacon.next_at.min(soon);
            }
        }
        let Some(to) = to else {
            return false;
        };
        if first && !self.config.sleepy_listens {
            self.choosing = Some(now + self.config.boot_listen);
        }
        self.take_anchor(&to, now, emit);
        true
    }

    /// This node has moved its home from `old`: the sleepy nodes there that
    /// listen now hear the heartbeat it sends on `old` and follow it, so it
    /// sends to them on its new home. The others still sleep there and
    /// say where they are with their next beat (spec "Channel moves").
    fn take_listeners(&mut self, old: Channel, now: Instant) {
        let home = self.home;
        for i in 0..self.heard.len() {
            let Some(n) = self.heard.iter().nth(i).copied() else {
                break;
            };
            if n.home == old && self.listening(n.id, now) {
                self.heard.moved(n.id, home);
                self.neighbours.home_heard(n.id, home);
            }
        }
    }

    /// Make `to` the anchor: listen on its home, and beat there at once.
    fn take_anchor(&mut self, to: &Neighbour, now: Instant, emit: &mut dyn FnMut(Action)) {
        self.anchor = Some(Anchor {
            id: to.id,
            position: to.position,
        });
        if self.config.home_override.is_none() {
            self.retune(to.home, emit);
        }
        if self.sniffing.is_some() {
            // It receives duty-cycled no longer, and listens now to choose.
            emit(Action::Listen(self.home));
        }
        // Heard at once and again soon by the new holder.
        self.beacon.next_at = now;
        self.beacon.quick = 1;
        self.anchored();
    }

    /// The anchor was chosen or confirmed: a node that sleeps keeps it
    /// whether or not it hears it, and measures its moves from here
    /// (module documentation).
    fn anchored(&mut self) {
        self.rescan = None;
        self.watch_at = None;
        self.chosen_from = self.config.position;
        self.reach_m = self.anchor.map_or(0.0, |a| {
            self.plane.point(a.position).distance_m(Default::default())
        });
        let keep = self.anchored_sleepy() && !self.config.sleepy_listens;
        self.neighbours
            .pin(self.anchor.filter(|_| keep).map(|a| a.id));
    }

    /// Listen a worst-case rotation, from `now`, to check the anchor of a
    /// node that sleeps (module documentation).
    pub(super) fn check_anchor(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        if !self.anchored_sleepy() || self.config.sleepy_listens || self.rescan.is_some() {
            return;
        }
        self.rescan = Some(now);
        emit(Action::Trace(Trace::AnchorCheck));
    }

    /// The rotation that began at `since` is over, the anchor not heard in
    /// it: take the strongest node heard that is not sleepy, or keep the
    /// anchor if none was (spec "Relay choice").
    fn anchor_checked(&mut self, since: Instant, now: Instant, emit: &mut dyn FnMut(Action)) {
        let heard = |n: &&Neighbour| n.relay() && n.last_heard >= since;
        let anchor = self.anchor.map(|a| a.id);
        if self
            .neighbours
            .iter()
            .filter(heard)
            .any(|n| Some(n.id) == anchor)
        {
            self.anchored();
            return;
        }
        let best = self
            .neighbours
            .iter()
            .filter(heard)
            .max_by(|a, b| a.rssi.total_cmp(&b.rssi))
            .copied();
        match best {
            Some(b) => self.take_anchor(&b, now, emit),
            // Nothing at all heard: the node is out of reach, in a pocket or
            // a basement, not its anchor gone; it keeps it, and checks again
            // only once it hears anything again.
            None => {
                self.anchored();
                self.dark = true;
            }
        }
    }

    fn timers(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        if let Stage::Booting(until) = self.stage {
            if now >= until {
                self.finish_boot(now, emit);
            }
        }
        if let Some(since) = self.rescan {
            if now >= since + self.config.boot_listen {
                self.anchor_checked(since, now, emit);
            }
        }
        if self.choosing.is_some_and(|t| now >= t) {
            self.reconsider_anchor(now, emit);
        }
        self.watch_anchor(now, emit);
        if self.sweeping() && self.mover.move_at.is_some_and(|t| now >= t) {
            self.mover = Mover::default();
            // Re-check after the random wait (spec "Channel moves").
            if self.excess() {
                let old = self.home;
                let to = self.quietest();
                self.retune(to, emit);
                if self.home != old {
                    self.beacon.announce = Some(old);
                    self.take_listeners(old, now);
                }
            }
        }
    }

    /// Start whatever is ready until the radio is busy or nothing is.
    fn pump(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        self.release_held(now, emit);
        if self.feed_due || self.feed_at.is_some_and(|t| t <= now) {
            self.feed(false, now);
        }
        while self.radio.idle() {
            self.prepare(now, emit);
            let awaiting = self.handover.awaiting_ack();
            let started = self.start_reply(now, emit)
                || (!awaiting && self.beacon_due(now) && self.start_heartbeat(now, emit))
                || self.start_handover(now, emit)
                || (!awaiting && self.sweep_due(now) && self.start_sweep(emit));
            if !started {
                break;
            }
        }
    }

    // --- Sweep (spec "Busy estimate") ---

    fn sweep_due(&self, now: Instant) -> bool {
        self.sweeping() && now >= self.sweep.next_at
    }

    /// Whether the node sweeps the channels: a relay, and a sleepy node
    /// while it boots; receiving duty-cycled it could not (module
    /// documentation).
    fn sweeping(&self) -> bool {
        !(self.config.sleepy && self.stage == Stage::Running)
    }

    /// Each channel once per sweep interval, one reading at a time (spec "Busy estimate",
    /// roadmap D13): one reading is away for less than the preamble leaves
    /// the modem to lock on after it, so it costs no packet; thirteen in a
    /// burst would.
    fn sweep_step(&self) -> Duration {
        self.config.sweep_interval.div(CHANNELS as u64)
    }

    fn start_sweep(&mut self, emit: &mut dyn FnMut(Action)) -> bool {
        self.sweep.next_at += self.sweep_step();
        self.radio
            .start(Job::Sweep, Action::Rssi(self.sweep.channel), emit);
        true
    }

    /// A reading of the home channel is held for one sweep interval, longer
    /// than any packet it can fall in, and then counts as a reading of any
    /// other channel does, unless energy that would follow this relay
    /// was on the air when it was taken (spec "Busy estimate").
    fn sweep_sample(&mut self, channel: Channel, dbm: i16, now: Instant) {
        if channel == self.home {
            // A reading that fell in this relay's own packet says nothing
            // of what would stay behind, and is not taken.
            if let Some((c, dbm)) = self
                .sweep
                .pending
                .take()
                .and_then(|p| p.dbm.map(|d| (p.channel, d)))
            {
                self.busy.sample(c, dbm);
            }
            self.sweep.pending = Some(HomeReading {
                channel,
                at: now,
                dbm: Some(dbm),
            });
        } else {
            self.busy.sample(channel, dbm);
        }
        self.sweep.channel = channel.next();
        // A reading held up by receptions (core-interface rule 8) or by
        // a hand-over must not leave a backlog to take back to back.
        if self.sweep.next_at <= now {
            self.sweep.next_at = now + self.sweep_step();
        }
        if channel.index() + 1 < CHANNELS {
            return;
        }
        // Once per round of the channels.
        self.neighbours.expire(now);
        if self.stage == Stage::Running && self.config.home_override.is_none() && self.sweeping() {
            self.move_rule(now);
        }
    }

    /// `busy[home] − min busy` above the move threshold (spec "Channel moves").
    fn excess(&self) -> bool {
        let min = Channel::all()
            .map(|c| self.busy.fraction(c))
            .min()
            .unwrap_or(0);
        self.busy.fraction(self.home).saturating_sub(min)
            > percent(self.config.move_threshold_percent)
    }

    fn move_rule(&mut self, now: Instant) {
        if !self.excess() {
            self.mover = Mover::default();
            return;
        }
        let since = *self.mover.excess_since.get_or_insert(now);
        if self.mover.move_at.is_none() && now.since(since) >= self.config.move_dwell {
            self.mover.move_at =
                Some(now + self.rng.duration(Duration::ZERO, self.config.move_wait_max));
        }
    }

    // --- Heartbeat (spec "Heartbeat") ---

    fn beacon_due(&self, now: Instant) -> bool {
        self.stage == Stage::Running
            && self.beacon.check == Check::Idle
            && now >= self.beacon.ready_at
            && (self.beacon.current.is_some()
                || self.beacon.announce.is_some()
                || now >= self.beacon.next_at
                || self.answer_due().is_some_and(|(_, _, at)| now >= at))
    }

    /// How many times a relay answers a beat of an anchorless node: once
    /// a fade for two of its beat periods (spec "Sleepy nodes alone").
    fn answers(&self) -> u8 {
        let span = self.anchorless_period().mul(2).as_micros();
        span.div_ceil(self.config.fade_retry.as_micros().max(1)) as u8
    }

    /// The next answer a relay owes an anchorless neighbour, if any: its
    /// ID, the channel it listens on, and when.
    fn answer_due(&self) -> Option<(NodeId, Channel, Instant)> {
        if self.config.sleepy {
            return None;
        }
        self.neighbours
            .answer_due(self.config.fade_retry, self.answers())
    }

    /// The heartbeat in progress. It carries the home at the moment it is
    /// sent, so a move during its backoff still announces the new channel.
    /// A sleepy node with an anchor beats on its home; a node without an
    /// anchor, a relay answering one, and a sleepy node's beat on its
    /// anchorless neighbours' channels, its home among them, send with the
    /// long preamble.
    fn heartbeat_frame(&self) -> Frame {
        let (channel, beat) = self
            .beacon
            .current
            .unwrap_or((self.beacon.rotation, Beat::Slot));
        let anchored = self.anchored_sleepy();
        let channel = match beat {
            Beat::Slot if anchored => self.home,
            _ => channel,
        };
        let hb = Heartbeat {
            sleepy: self.config.sleepy,
            anchorless: self.config.sleepy && self.anchor.is_none(),
            home: self.home,
            id: self.id,
            short: self.config.position.short(),
        };
        let reaches_home = self.beacon.reach_all.unwrap_or(0) & channel_bit(self.home) != 0;
        let preamble = match beat {
            Beat::Answer(_) | Beat::Reach => LONG_PREAMBLE_SYMBOLS,
            _ if hb.anchorless => LONG_PREAMBLE_SYMBOLS,
            // An anchorless neighbour listens on its home too.
            Beat::Slot if anchored && reaches_home => LONG_PREAMBLE_SYMBOLS,
            _ => MIN_PREAMBLE_SYMBOLS,
        };
        mac::frame(&Packet::Heartbeat(hb), channel, preamble)
    }

    /// Check the budget before the channel: there is no point listening for
    /// a slot the budget will not let us use. Nothing else transmits
    /// between this check and the transmission, and the bucket only fills
    /// meanwhile, so the budget cannot have shrunk by then. The
    /// announcement after a move goes first, then the heartbeat of the
    /// slot, after a sleepy node's copies of it on its anchorless
    /// neighbours' channels, then an answer.
    fn start_heartbeat(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) -> bool {
        if self.beacon.current.is_none() {
            if now >= self.beacon.next_at && self.beacon.reach_all.is_none() {
                let all = self.reach_channels(now);
                self.beacon.reach_all = Some(all);
                self.beacon.reach_left = all & !channel_bit(self.home);
            }
            let reach = Channel::all().find(|c| self.beacon.reach_left & channel_bit(*c) != 0);
            self.beacon.current = match (self.beacon.announce, self.answer_due()) {
                (Some(old), _) => Some((old, Beat::Announce)),
                _ if now >= self.beacon.next_at => match reach {
                    Some(c) => Some((c, Beat::Reach)),
                    None => Some((self.beacon.rotation, Beat::Slot)),
                },
                (None, Some((id, home, at))) if now >= at => Some((home, Beat::Answer(id))),
                _ => return false,
            };
        }
        let f = self.heartbeat_frame();
        let airtime = mac::airtime(&f);
        if !self.budget.would_allow(airtime, now) {
            let until = self
                .budget
                .allowed_at(airtime, Duration::ZERO, now)
                .unwrap_or(now + self.config.heartbeat_max);
            self.beacon.ready_at = until;
            emit(Action::Trace(Trace::HeartbeatDeferred { until }));
            return false;
        }
        self.beacon.check = Check::Cad;
        self.radio
            .start(Job::Heartbeat, Action::Cad(f.channel), emit);
        true
    }

    /// The channels the anchorless neighbours of a sleepy node with an
    /// anchor listen on at `now`, as a mask: each of its beats goes on
    /// each of them too, with the long preamble, once it has heard the
    /// neighbour anchorless in two consecutive rotations of its beats
    /// (spec "Beats"). One heard so once may be switching on, and anchor
    /// within minutes.
    fn reach_channels(&self, now: Instant) -> u16 {
        if !self.anchored_sleepy() {
            return 0;
        }
        self.neighbours
            .live(now)
            .filter(|(_, n)| n.stays_anchorless())
            .fold(0, |m, (_, n)| m | channel_bit(n.home))
    }

    fn heartbeat_check(&mut self, busy: bool, now: Instant, emit: &mut dyn FnMut(Action)) {
        let f = self.heartbeat_frame();
        if busy {
            self.beacon.check = Check::Idle;
            self.beacon.busy_checks += 1;
            if self.beacon.busy_checks >= self.config.lbt_attempts {
                emit(Action::Trace(Trace::HeartbeatSkipped {
                    channel: f.channel,
                }));
                self.heartbeat_done(now, false);
            } else {
                self.beacon.ready_at = now
                    + self
                        .rng
                        .duration(Duration::ZERO, self.config.lbt_backoff_max);
            }
            return;
        }
        if self.beacon.check == Check::Cad {
            self.beacon.check = Check::Rssi;
            self.radio
                .start(Job::Heartbeat, Action::Rssi(f.channel), emit);
        } else {
            self.beacon.check = Check::Tx;
            self.budget.record(mac::airtime(&f), now);
            self.radio.start(Job::Heartbeat, Action::Transmit(f), emit);
        }
    }

    /// The heartbeat went out, or was skipped (spec "Heartbeat"). On a fixed
    /// interval a skipped one gives up its channel for the rotation, so
    /// that the next comes where and when its neighbours expect it; a
    /// sleeping node's beat, always on its home, has no rotation to keep.
    fn heartbeat_done(&mut self, now: Instant, sent: bool) {
        self.beacon.check = Check::Idle;
        self.beacon.busy_checks = 0;
        match self.beacon.current.take() {
            Some((channel, Beat::Announce)) => {
                // A second move meanwhile has a newer old channel to announce on.
                if self.beacon.announce == Some(channel) {
                    self.beacon.announce = None;
                }
                return;
            }
            Some((_, Beat::Answer(id))) => {
                self.neighbours.answer_sent(id, now, self.config.fade_retry);
                return;
            }
            Some((channel, Beat::Reach)) => {
                self.beacon.reach_left &= !channel_bit(channel);
                return;
            }
            _ => {}
        }
        // The next beat takes its anchorless neighbours' channels afresh.
        self.beacon.reach_all = None;
        if sent && self.anchored_sleepy() {
            // The window after its beat (spec "Delivery").
            self.awake_until = now + self.heard.params().awake;
        }
        if self.config.sleepy {
            // Without an anchor, the next channel each beat whether or not
            // this one went; with one, the rotation is not used.
            self.beacon.rotation = self.beacon.rotation.next();
            self.beacon.next_at = now + self.beat_interval();
        } else {
            // On its fixed interval, a channel a slot whether or not the
            // heartbeat went, so that a neighbour that heard one knows when
            // the next comes to its channel (spec "Heartbeat").
            let interval = heartbeat_interval(self.id, &self.config);
            let mut next = self.beacon.next_at + interval;
            self.beacon.rotation = self.beacon.rotation.next();
            while next <= now {
                next += interval;
                self.beacon.rotation = self.beacon.rotation.next();
            }
            self.beacon.next_at = next;
        }
        if self.config.sleepy {
            // Its own beat stands in for the sweep's round of the tables.
            self.neighbours.expire(now);
        }
    }

    // --- Reception ---

    fn received(&mut self, bytes: &[u8], rssi: i16, now: Instant, emit: &mut dyn FnMut(Action)) {
        self.set_aside_own(bytes, now);
        if core::mem::take(&mut self.dark) {
            // Back in reach: its anchor's rotations count from here.
            self.misses = 0;
        }
        // A booting relay has no home channel yet to be reached on, nor
        // neighbours to forward to: it takes no traffic (spec "Channel at boot").
        let running = self.stage == Stage::Running;
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
            Ok(Packet::Data(d)) if running && d.dst_hop == self.me() => self.data(d, now, emit),
            _ => {}
        }
    }

    /// Set aside a home reading taken while a reception that follows this
    /// relay to any channel was on the air: a packet addressed to it, or
    /// the reply to its own hand-over (spec "Busy estimate"). Everything
    /// else stays behind, and the reading counts it as the sweep counts
    /// any channel's.
    fn set_aside_own(&mut self, bytes: &[u8], now: Instant) {
        let me = self.me();
        let own = match Packet::decode(bytes) {
            Ok(Packet::Data(d)) => d.dst_hop == me,
            Ok(Packet::Ack(a)) => self.handover.answered_by(a),
            _ => false,
        };
        if !own {
            return;
        }
        // A frame's preamble is not reported: one addressed to a node
        // that sweeps has the shortest.
        let len = bytes.len().min(u8::MAX as usize) as u8;
        let airtime = time_on_air(len, MIN_PREAMBLE_SYMBOLS);
        if let Some(p) = &mut self.sweep.pending {
            if now.since(p.at) <= airtime {
                p.dbm = None;
            }
        }
    }

    /// A heartbeat. Every heartbeat fills the neighbour table, a sleepy
    /// node's marked sleepy (spec "Neighbour table"). At a node that is not
    /// sleepy, the beat of a sleepy node with an anchor fills the heard
    /// table too: the node listens now, and gets what is held for it; one
    /// without an anchor has no window to deliver in
    /// (spec "Sleepy nodes alone"). A sleepy node's anchor follows the
    /// heartbeats of nodes that are not sleepy (spec "Relay choice").
    fn heard(&mut self, hb: Heartbeat, rssi: i16, now: Instant, emit: &mut dyn FnMut(Action)) {
        if hb.id == self.id {
            return;
        }
        let position = hb.position(self.config.position);
        let heard = self.neighbours.heard(&hb, position, rssi, &self.plane, now);
        // When it started: a heartbeat's slot is its start, give or take
        // the listen-before-talk.
        let started = now - heartbeat_airtime(&hb);
        // A heartbeat of the anchor that a sleeping node hears before its
        // watch opens is not in a slot: an answer to a node without an
        // anchor, or the one after a move (spec "Heartbeat"). It shows the
        // anchor alive but says nothing of its slots.
        let off_slot = self.anchor.is_some_and(|a| a.id == hb.id)
            && self.watch_at.is_some()
            && self.watch().is_some_and(|(from, _)| started < from);
        if !off_slot {
            self.neighbours.beat_heard(hb.id, self.home, started);
        }
        if let Some(slot) = heard.new_occupant() {
            self.records.slot_reoccupied(slot);
            self.skipped &= !(1 << slot);
        }
        if hb.sleepy {
            if !self.config.sleepy && !hb.anchorless && self.heard.heard(hb.id, hb.home, rssi, now)
            {
                self.deliver_now(hb.id, now);
            }
            return;
        }
        if self.config.sleepy && self.booting() {
            // An anchor ends the boot listen, which chooses a home
            // for a node that hears none (spec "Channel at boot").
            self.finish_boot(now, emit);
        }
        self.reconsider_anchor(now, emit);
        if self.anchor.is_some_and(|a| a.id == hb.id) {
            if let (Some(at), Some((from, to))) = (self.watch_at, self.watch()) {
                if started >= from && started < to {
                    let offset = started.as_micros() as i64 - at.as_micros() as i64;
                    emit(Action::Trace(Trace::AnchorWatch {
                        heard: true,
                        offset_us: offset as i32,
                        listened: now.since(from),
                    }));
                }
            }
            if off_slot {
                self.anchor_alive();
            } else {
                self.anchor_heard(started);
            }
        }
    }

    fn set_position(&mut self, position: Position) {
        self.config.position = position;
        self.plane = LocalPlane::new(position);
        self.neighbours.replane(&self.plane);
    }

    fn app(&mut self, cmd: AppCommand<'_>, now: Instant, emit: &mut dyn FnMut(Action)) {
        match cmd {
            AppCommand::Send {
                dst_pos,
                dst_id,
                payload,
            } => self.originate(dst_pos, dst_id, payload, now, emit),
            AppCommand::SetPosition { position } => {
                self.set_position(position);
                let moved = self
                    .plane
                    .point(self.chosen_from)
                    .distance_m(Default::default());
                if moved > move_bound_m(self.reach_m) {
                    self.check_anchor(now, emit);
                }
            }
        }
    }
}

/// How long a heartbeat heard was on the air: an anchorless node's beat
/// with the long preamble, any other as if with the shortest. A relay's
/// answer to an anchorless node also has the long one, but cannot be told
/// from its other heartbeats.
fn heartbeat_airtime(hb: &Heartbeat) -> Duration {
    let preamble = if hb.anchorless {
        LONG_PREAMBLE_SYMBOLS
    } else {
        MIN_PREAMBLE_SYMBOLS
    };
    time_on_air(HEARTBEAT_LEN as u8, preamble)
}

/// The bit of channel `c` in a mask of channels.
fn channel_bit(c: Channel) -> u16 {
    1 << c.index()
}

fn random_channel(rng: &mut Rng) -> Channel {
    let fallback = Channel::all().next().expect("the plan has channels");
    Channel::new(rng.range(0, CHANNELS as u64 - 1) as u8).unwrap_or(fallback)
}

impl<const H: usize, const Q: usize> Core for Node<H, Q> {
    fn handle(&mut self, event: Event<'_>, now: Instant, emit: &mut dyn FnMut(Action)) {
        // Any use of the radio ends a sniff (core-interface §3 rule 10).
        let woke = core::cell::Cell::new(false);
        let mut wake = |a: Action| {
            let radio = matches!(
                a,
                Action::Listen(_) | Action::Cad(_) | Action::Rssi(_) | Action::Transmit(_)
            );
            woke.set(woke.get() || radio);
            emit(a)
        };
        let emit: &mut dyn FnMut(Action) = &mut wake;
        if self.stage == Stage::Off {
            self.start(now, emit);
        }
        let threshold = self.busy.threshold_dbm();
        match event {
            Event::Wakeup => {}
            Event::Received {
                bytes, rssi_dbm, ..
            } => self.received(bytes, rssi_dbm, now, emit),
            Event::CadDone { busy, .. } => match self.radio.complete() {
                Some(Job::Heartbeat) => self.heartbeat_check(busy, now, emit),
                Some(Job::HandOver) => self.handover_check(busy, now, emit),
                Some(Job::Reply) => self.reply_check(busy, now, emit),
                Some(Job::Sweep) | None => {}
            },
            Event::RssiSample { channel, dbm } => match self.radio.complete() {
                Some(Job::Sweep) => self.sweep_sample(channel, dbm, now),
                Some(Job::Heartbeat) => self.heartbeat_check(dbm > threshold, now, emit),
                Some(Job::HandOver) => self.handover_check(dbm > threshold, now, emit),
                Some(Job::Reply) => self.reply_check(dbm > threshold, now, emit),
                None => {}
            },
            Event::TxDone => match self.radio.complete() {
                Some(Job::Heartbeat) => self.heartbeat_done(now, true),
                // Unless the hand-over was given up meanwhile (a displaced record).
                Some(Job::HandOver) if self.handover.step() == Step::Tx => {
                    self.handover.sent(now, self.config.ack_timeout)
                }
                Some(Job::Reply) => {
                    self.replies.sent();
                    if core::mem::take(&mut self.renew) {
                        self.took(now);
                    }
                }
                _ => {}
            },
            Event::App(cmd) => self.app(cmd, now, emit),
        }
        self.timers(now, emit);
        self.forward_timers(now, emit);
        self.delivery_timers(now);
        self.pump(now, emit);
        self.app_ready_at = self.send_ready_at(now);
        if woke.get() {
            self.sniffing = None;
        }
        self.sniff(now, emit);
    }

    fn next_wakeup(&self) -> Option<Instant> {
        let mut w: Option<Instant> = None;
        let mut at = |t: Instant| w = Some(w.map_or(t, |w| w.min(t)));
        match self.stage {
            Stage::Off => return Some(Instant::ZERO),
            Stage::Booting(until) => at(until),
            Stage::Running => {}
        }
        if let (Some(t), false) = (self.mover.move_at, !self.sweeping()) {
            at(t);
        }
        if let Some(t) = self.forward_wakeup() {
            at(t);
        }
        if let (Some(t), true) = (self.replies.wake_at(), self.radio.idle()) {
            at(t);
        }
        if let Some(t) = self.delivery_wakeup() {
            at(t);
        }
        if let Some(t) = self.rescan {
            at(t + self.config.boot_listen);
        }
        if let Some(t) = self.choosing {
            at(t);
        }
        if let Some(t) = self.window_ends {
            at(t);
        }

        // The application waits for the bucket (core-interface §4).
        if let Some(t) = self.app_ready_at {
            at(t);
        }
        let awaiting = self.handover.awaiting_ack();
        // Radio jobs only once the radio is free: their completion event
        // wakes the core otherwise.
        if self.radio.idle() && !awaiting {
            if self.stage == Stage::Running && self.beacon.check == Check::Idle {
                let pending = self.beacon.announce.is_some() || self.beacon.current.is_some();
                let due = if pending {
                    self.beacon.ready_at
                } else {
                    // A relay's answer may come before its next slot.
                    let answer = self.answer_due().map(|(_, _, at)| at);
                    answer.map_or(self.beacon.next_at, |t| t.min(self.beacon.next_at))
                };
                at(due.max(self.beacon.ready_at));
            }
            if self.sweeping() {
                at(self.sweep.next_at);
            }
        }
        w
    }

    fn accepts_send(&self, now: Instant) -> bool {
        Self::accepts_send(self, now)
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::tables::budget::BAN;
    use std::vec::Vec;

    /// The ID a test gives the relay heard at `p`.
    fn id_of(p: Position) -> NodeId {
        NodeId(p.short().0.to_be_bytes())
    }

    /// The ID numbered `n`.
    fn name(n: u32) -> NodeId {
        NodeId(n.to_be_bytes())
    }

    /// The ID of a node no test builds.
    const NOBODY: NodeId = NodeId([9; 4]);

    #[test]
    fn sizes() {
        // docs/core-interface.md §6: the tables, and around them the medium
        // access, the hand-over, one owed reply and one held message in
        // under 1 KiB.
        let relay = core::mem::size_of::<Relay>();
        let tables = crate::tables::sizes::tables();
        std::println!("Relay {relay} B, tables {tables} B");
        assert!(relay <= tables + 1024, "{relay}");
        // It fits the board's RAM: 255 KiB for the application
        // (bark-fw/memory.x), less the BLE controller's buffer, under 5 KiB
        // (docs/implementation-research-2026-10-14.md), and Embassy and the
        // stacks, about 16 KiB (spec "Implementation notes").
        let ram = (255 - 5 - 16) * 1024;
        assert!(relay <= ram, "{relay} of {ram}");
        // A node built only to be sleepy spends nothing on the heard table
        // and the delivery queue (spec "Sleepy nodes").
        let sleepy = core::mem::size_of::<SleepyNode>();
        let mailbox = core::mem::size_of::<Heard>() + core::mem::size_of::<DeliveryQueue>();
        std::println!("SleepyNode {sleepy} B, mailbox {mailbox} B");
        assert!(
            sleepy + mailbox <= relay + 64,
            "{sleepy} + {mailbox} of {relay}"
        );
    }

    /// A sleepy node built without the mailbox does what a full one set
    /// up sleepy does.
    #[test]
    fn a_sleepy_node_without_the_mailbox_is_a_sleepy_relay() {
        let config = Config {
            boot_listen: Duration::from_secs(1),
            sleepy: true,
            ..Config::default()
        };
        let mut full = Relay::new(config, 5);
        let mut bare = SleepyNode::new(config, 5);
        let until = Instant::ZERO + Duration::from_secs(900);
        let (mut t_full, mut t_bare) = (Instant::ZERO, Instant::ZERO);
        let sent_full = drive(&mut full, &mut t_full, until);
        let sent_bare = drive(&mut bare, &mut t_bare, until);
        assert!(sent_full.len() > 5);
        assert_eq!(sent_full, sent_bare);
    }

    /// A sleepy node fills its neighbour table from the beats it hears,
    /// marked sleepy, and keeps no heard table (spec "Neighbour table",
    /// "Heard table").
    #[test]
    fn a_sleepy_node_keeps_no_heard_table() {
        let config = Config {
            boot_listen: Duration::from_secs(1),
            sleepy: true,
            ..Config::default()
        };
        let mut r = Relay::new(config, 6);
        let mut now = Instant::ZERO;
        drive(&mut r, &mut now, Instant::ZERO + Duration::from_secs(5));
        let other = name(77);
        let beat = Heartbeat {
            sleepy: true,
            anchorless: false,
            home: r.home(),
            id: other,
            short: config.position.short(),
        };
        let mut buf = [0; crate::event::MAX_FRAME];
        let len = Packet::Heartbeat(beat).encode(&mut buf).unwrap();
        let event = Event::Received {
            channel: r.home(),
            rssi_dbm: -80,
            snr_db: 5,
            bytes: &buf[..len],
        };
        r.handle(event, now, &mut |_| {});
        assert!(r.heard_nodes().is_empty());
        let n = r.neighbours().find(|n| n.id == other).copied();
        assert!(n.is_some_and(|n| n.sleepy && n.last_resort()), "{n:?}");
    }

    /// A sleepy node holds mail for nobody: a message for another node at
    /// its own position is not its to deliver, and waits to go on (spec
    /// "Sleepy nodes", "Owner").
    #[test]
    fn a_sleepy_node_owns_no_other_node_s_message() {
        let config = Config {
            boot_listen: Duration::from_secs(1),
            sleepy: true,
            ..Config::default()
        };
        let mut r = Relay::new(config, 7);
        let mut now = Instant::ZERO;
        drive(&mut r, &mut now, Instant::ZERO + Duration::from_secs(5));
        assert!(r.anchorless());
        let send = AppCommand::Send {
            dst_pos: config.position,
            dst_id: name(78),
            payload: b"for another",
        };
        r.handle(Event::App(send), now, &mut |_| {});
        assert_eq!(r.holding().count(), 1);
        assert_eq!(r.load(now).delivery_queue, 0);
    }

    /// Drive `r` from its own wakeups until `until`, completing every radio
    /// action at once on a quiet channel. Returns the frames it sent.
    fn drive<const H: usize, const Q: usize>(
        r: &mut Node<H, Q>,
        now: &mut Instant,
        until: Instant,
    ) -> Vec<(Instant, Frame)> {
        drive_from(r, now, until, Vec::new())
    }

    /// [`drive`], with `pending` the actions `r` has asked for already.
    fn drive_from<const H: usize, const Q: usize>(
        r: &mut Node<H, Q>,
        now: &mut Instant,
        until: Instant,
        mut pending: Vec<Action>,
    ) -> Vec<(Instant, Frame)> {
        let mut sent = Vec::new();
        loop {
            let event = match pending.pop() {
                Some(Action::Cad(channel)) => Event::CadDone {
                    channel,
                    busy: false,
                },
                Some(Action::Rssi(channel)) => Event::RssiSample { channel, dbm: -130 },
                Some(Action::Transmit(f)) => {
                    sent.push((*now, f));
                    *now += mac::airtime(&f);
                    Event::TxDone
                }
                Some(_) => continue,
                None => match r.next_wakeup() {
                    Some(t) if t <= until => {
                        *now = (*now).max(t);
                        Event::Wakeup
                    }
                    _ => return sent,
                },
            };
            r.handle(event, *now, &mut |a| match a {
                Action::Cad(_) | Action::Rssi(_) | Action::Transmit(_) => pending.push(a),
                _ => {}
            });
        }
    }

    /// A sleepy node whose anchor is not in its table when the rotation of
    /// its first choice ends makes the choice at its time and does not
    /// wake at once again. Before D58 a sleepy node beating with the
    /// anchor's ID removed the entry; now the entry is pinned, so the test
    /// forgets it by hand to keep the invariant: a timer that is due is
    /// cleared.
    #[test]
    fn the_first_choice_ends_without_the_anchor() {
        use crate::wire::Heartbeat;
        let config = Config {
            sleepy: true,
            boot_listen: Duration::from_secs(10),
            ..Config::default()
        };
        let mut s = Relay::new(config, 5);
        let at = |ms: u64| Instant::ZERO + Duration::from_millis(ms);
        s.handle(Event::Wakeup, at(0), &mut |_| {});
        let anchor = name(77);
        let beat = || {
            let hb = Heartbeat {
                sleepy: false,
                anchorless: false,
                home: Channel::new(4).unwrap(),
                id: anchor,
                short: config.position.short(),
            };
            let mut bytes = [0u8; crate::event::MAX_FRAME];
            let n = Packet::Heartbeat(hb).encode(&mut bytes).unwrap();
            (bytes, n)
        };
        let hear = |s: &mut Relay, t: Instant| {
            let (bytes, n) = beat();
            let ev = Event::Received {
                channel: Channel::new(4).unwrap(),
                rssi_dbm: -90,
                snr_db: 0,
                bytes: &bytes[..n],
            };
            s.handle(ev, t, &mut |_| {});
        };
        hear(&mut s, at(1000));
        assert_eq!(s.anchor().map(|a| a.id), Some(anchor));
        s.neighbours.pin(None);
        s.neighbours.forget(anchor);
        assert!(s.neighbours().all(|n| n.id != anchor));
        // Run it, completing its radio actions at once, for as many
        // events as a minute takes it; stuck, it would spend them all at
        // the end of the choice.
        let mut now = at(2000);
        let mut pending: Vec<Action> = Vec::new();
        for _ in 0..10_000 {
            let event = match pending.pop() {
                Some(Action::Cad(channel)) => Event::CadDone {
                    channel,
                    busy: false,
                },
                Some(Action::Rssi(channel)) => Event::RssiSample { channel, dbm: -130 },
                Some(Action::Transmit(f)) => {
                    now += mac::airtime(&f);
                    Event::TxDone
                }
                Some(_) => continue,
                None => match s.next_wakeup() {
                    Some(t) => {
                        now = now.max(t);
                        Event::Wakeup
                    }
                    None => break,
                },
            };
            if now > at(60_000) {
                break;
            }
            s.handle(event, now, &mut |a| match a {
                Action::Cad(_) | Action::Rssi(_) | Action::Transmit(_) => pending.push(a),
                _ => {}
            });
        }
        assert!(now > at(60_000), "stuck at {now:?}");
    }

    /// A relay reset in place, after it has run and holds messages, does
    /// what a new one does.
    #[test]
    fn a_reset_relay_is_a_new_one() {
        use crate::ids::NodeId;
        let config = Config {
            boot_listen: Duration::from_secs(1),
            ..Config::default()
        };
        let mut used = Relay::new(config, 1);
        let mut now = Instant::ZERO;
        drive(&mut used, &mut now, Instant::ZERO + Duration::from_secs(30));
        let send = AppCommand::Send {
            dst_pos: config.position,
            dst_id: NodeId([1; 4]),
            payload: b"queued",
        };
        used.handle(Event::App(send), now, &mut |_| {});
        assert_eq!(used.holding().count(), 1);

        let other = Config {
            position: Position::from_degrees(50.45, 30.52),
            ..config
        };
        used.reset(other, 2);
        let mut fresh = Relay::new(other, 2);
        assert_eq!(used.holding().count(), 0);
        let until = Instant::ZERO + Duration::from_secs(60);
        let (mut t_used, mut t_fresh) = (Instant::ZERO, Instant::ZERO);
        let sent_used = drive(&mut used, &mut t_used, until);
        let sent_fresh = drive(&mut fresh, &mut t_fresh, until);
        assert!(!sent_fresh.is_empty());
        assert_eq!(sent_used, sent_fresh);
        assert_eq!(used.home(), fresh.home());
    }

    /// A node whose bucket is nearly spent holds one message for a sleepy
    /// node that has just beaten, and nothing else happens: it wakes when
    /// its bucket will fit the DATA with the reply reserve, within the
    /// node's window, and sends it then. The node never answers, and its message
    /// waits for its next beat (`relay/delivery.rs`).
    #[test]
    fn a_delivery_waits_for_the_bucket_within_the_window() {
        use crate::ids::MsgId;
        use crate::tables::budget::REPLY_RESERVE;
        use crate::tables::delivery::Queued;
        use crate::wire::Data;
        let hour = Duration::from_secs(3600);
        let config = Config {
            boot_listen: Duration::from_secs(1),
            home_override: Channel::new(3),
            sweep_interval: hour,
            heartbeat_min: hour,
            heartbeat_max: hour,
            ..Config::default()
        };
        let mut r = Relay::new(config, 1);
        let mut now = Instant::ZERO;
        drive(&mut r, &mut now, Instant::ZERO + Duration::from_secs(2));
        assert!(!r.booting());
        now = Instant::ZERO + Duration::from_secs(10);
        let node = NodeId([7; 4]);
        r.heard.heard(node, r.home, -80, now);
        // Short of the DATA and the reply reserve by what the bucket refills
        // well within the window.
        let dst = r.config.position;
        let short = Duration::from_millis(20);
        let data = Data {
            sender_home: r.home,
            dst_hop: node,
            src_hop: r.me(),
            dst_pos: dst,
            dst_id: node,
            hops: 1,
            time_left: u8::MAX,
            payload: b"later",
        };
        let frame = mac::frame(&Packet::Data(data), r.home, MIN_PREAMBLE_SYMBOLS);
        let need = mac::airtime(&frame) + REPLY_RESERVE - short;
        r.budget.record(r.budget.available(now) - need, now);
        let id = MsgId::of(dst, node, b"later");
        let mut record = crate::tables::records::Record::new(id, None, 0, now);
        record.set_arrived();
        let at = r.record_place().unwrap();
        r.records.put(at, record);
        let q = Queued::new(id, dst, node, b"later", u8::MAX, now).unwrap();
        r.delivery.push(q);
        r.feed_due = true;
        r.handle(Event::Wakeup, now, &mut |_| {});
        assert!(r.queue.is_empty(), "taken with a spent bucket");
        let fits = r
            .budget
            .allowed_at(mac::airtime(&frame), REPLY_RESERVE, now)
            .unwrap();
        assert_eq!(r.next_wakeup(), Some(fits));
        let window = r.heard.get(node).unwrap().awake_until;
        assert!(fits < window, "the bucket fits it in the window");
        let until = now + Duration::from_secs(60);
        let sent = drive(&mut r, &mut now, until);
        let datas: Vec<_> = sent
            .iter()
            .filter(|(_, f)| matches!(Packet::decode(f.payload()), Ok(Packet::Data(d)) if d.dst_hop == node))
            .collect();
        assert_eq!(
            datas.len(),
            3,
            "three attempts at a node that never answers"
        );
        let first = datas[0].0;
        assert!(
            first >= fits && first.since(fits) < Duration::from_millis(1),
            "sent {:?} after the bucket fit it",
            first.since(fits)
        );
        assert!(!r.heard.awake(node, now), "asleep until its next beat");
        assert_eq!(r.delivery.len(), 1, "it waits for the next beat");
    }

    /// A DATA to a sleepy node goes ahead of everything in the send queue
    /// (spec "Delivery"), so it needs only its own frame and the reply
    /// reserve in the bucket, not the airtime of the forwards it goes ahead
    /// of: a holder at its cap whose queued forwards outweigh its bucket
    /// still delivers in the window (spec "Delivery queue", roadmap D55).
    #[test]
    fn a_delivery_goes_ahead_of_the_queue_in_the_bucket_too() {
        use crate::ids::MsgId;
        use crate::tables::budget::REPLY_RESERVE;
        use crate::tables::delivery::Queued;
        use crate::tables::send_queue::{Kind, Outgoing};
        use crate::wire::{Data, MAX_PAYLOAD};
        let hour = Duration::from_secs(3600);
        let config = Config {
            boot_listen: Duration::from_secs(1),
            home_override: Channel::new(3),
            sweep_interval: hour,
            heartbeat_min: hour,
            heartbeat_max: hour,
            ..Config::default()
        };
        let mut r = Relay::new(config, 1);
        let mut now = Instant::ZERO;
        drive(&mut r, &mut now, Instant::ZERO + Duration::from_secs(2));
        now = Instant::ZERO + Duration::from_secs(10);
        let node = NodeId([7; 4]);
        r.heard.heard(node, r.home, -80, now);
        let dst = r.config.position;
        // Forwards waiting for a ban, a quarter of the bucket's depth.
        let mut queued = Duration::ZERO;
        for k in 0..10u8 {
            let mut payload = [0u8; MAX_PAYLOAD];
            payload[0] = k;
            let d = Data {
                sender_home: r.home,
                dst_hop: NodeId::NONE,
                src_hop: r.me(),
                dst_pos: dst,
                dst_id: name(99),
                hops: 2,
                time_left: u8::MAX,
                payload: &payload,
            };
            let frame = mac::frame(&Packet::Data(d), r.home, MIN_PREAMBLE_SYMBOLS);
            queued = queued + mac::airtime(&frame);
            let out = Outgoing {
                not_before: now + hour,
                expires: now + hour,
                ..Outgoing::new(frame, Kind::Forward)
            };
            r.queue.push(out).unwrap();
        }
        let data = Data {
            sender_home: r.home,
            dst_hop: node,
            src_hop: r.me(),
            dst_pos: dst,
            dst_id: node,
            hops: 0,
            time_left: u8::MAX,
            payload: b"ahead",
        };
        let frame = mac::frame(&Packet::Data(data), r.home, MIN_PREAMBLE_SYMBOLS);
        // The bucket holds the delivery and the reply reserve, but not the
        // forwards as well.
        let need = mac::airtime(&frame) + REPLY_RESERVE + Duration::from_millis(1);
        assert!(need < queued);
        r.budget.record(r.budget.available(now) - need, now);
        let id = MsgId::of(dst, node, b"ahead");
        let mut record = crate::tables::records::Record::new(id, None, 0, now);
        record.set_arrived();
        let at = r.record_place().unwrap();
        r.records.put(at, record);
        let q = Queued::new(id, dst, node, b"ahead", u8::MAX, now).unwrap();
        r.delivery.push(q);
        r.feed_due = true;
        let mut checked = None;
        r.handle(Event::Wakeup, now, &mut |a| {
            if let Action::Cad(c) = a {
                checked = Some(c);
            }
        });
        assert!(
            r.queue.iter().any(|o| o.kind == Kind::Deliver),
            "held back by the forwards behind it"
        );
        assert_eq!(checked, Some(r.home), "its hand-over starts at once");
    }

    /// Two packets arrive within one reply time. The second finds the one
    /// owed-reply place holding the first one's ACK, which waits out a
    /// busy channel: it is neither taken nor refused, and gets no answer.
    /// Its sender's retry, after the first ACK is gone, is taken (roadmap
    /// D29 rule 4).
    #[test]
    fn a_packet_that_cannot_be_answered_is_not_taken() {
        use crate::event::AppEvent;
        use crate::wire::Data;
        let hour = Duration::from_secs(3600);
        let config = Config {
            boot_listen: Duration::from_secs(1),
            home_override: Channel::new(3),
            sweep_interval: hour,
            heartbeat_min: hour,
            heartbeat_max: hour,
            ..Config::default()
        };
        let mut r = Relay::new(config, 1);
        let mut now = Instant::ZERO;
        drive(&mut r, &mut now, Instant::ZERO + Duration::from_secs(2));
        assert!(!r.booting());
        let (a_home, b_home) = (Channel::new(5).unwrap(), Channel::new(6).unwrap());
        let packet = |home: Channel, from: u32, text: &'static [u8]| Data {
            sender_home: home,
            dst_hop: r.me(),
            src_hop: name(from),
            dst_pos: r.config.position,
            dst_id: r.me(),
            hops: 1,
            time_left: u8::MAX,
            payload: text,
        };
        let (a, b) = (packet(a_home, 1, b"first"), packet(b_home, 2, b"second"));
        let bytes = |d: &Data<'_>| {
            let f = mac::frame(&Packet::Data(*d), r.home, MIN_PREAMBLE_SYMBOLS);
            f.bytes[..f.len as usize].to_vec()
        };
        let (a_bytes, b_bytes) = (bytes(&a), bytes(&b));
        let received = |bytes| Event::Received {
            channel: Channel::new(3).unwrap(),
            rssi_dbm: -80,
            snr_db: 10,
            bytes,
        };
        let taken = |actions: &[Action], d: &Data<'_>| {
            actions.iter().any(
                |x| matches!(x, Action::App(AppEvent::Message { msg_id, .. }) if *msg_id == d.msg_id()),
            )
        };
        let acks = |sent: &[(Instant, Frame)], d: &Data<'_>, home| {
            sent.iter()
                .filter(|(_, f)| {
                    f.channel == home
                        && matches!(Packet::decode(f.payload()), Ok(Packet::Ack(k)) if k.msg_id == d.msg_id())
                })
                .count()
        };

        now = Instant::ZERO + Duration::from_secs(10);
        let mut actions = Vec::new();
        r.handle(received(&a_bytes), now, &mut |x| actions.push(x));
        assert!(taken(&actions, &a));
        assert!(actions.contains(&Action::Cad(a_home)), "{actions:?}");
        // A's channel is busy: its ACK waits a backoff.
        now += Duration::from_millis(1);
        r.handle(
            Event::CadDone {
                channel: a_home,
                busy: true,
            },
            now,
            &mut |_| {},
        );
        assert!(r.replies.pending());

        now += Duration::from_millis(1);
        actions.clear();
        r.handle(received(&b_bytes), now, &mut |x| actions.push(x));
        assert!(actions.is_empty(), "{actions:?}");
        assert!(r.records.get(b.msg_id()).is_none(), "B's packet was taken");

        let until = now + Duration::from_millis(100);
        let sent = drive(&mut r, &mut now, until);
        assert_eq!(acks(&sent, &a, a_home), 1);
        assert_eq!(sent.len(), 1, "B's packet was answered");

        // B's retry, after the retry wait (spec "Retry wait").
        now += Duration::from_millis(200);
        actions.clear();
        r.handle(received(&b_bytes), now, &mut |x| actions.push(x));
        assert!(taken(&actions, &b));
        let until = now + Duration::from_millis(100);
        let sent = drive_from(&mut r, &mut now, until, actions);
        assert_eq!(acks(&sent, &b, b_home), 1);
    }

    /// The application may send only while the relay can take a message
    /// (roadmap D29 rule 6): not while it boots, not while it holds one of
    /// the application's, and not while its bucket cannot hold a full
    /// frame on top of what it has queued; then the relay wakes when it
    /// can. A send offered anyway is ignored. A message for a sleepy node
    /// this relay holds for that finds the delivery queue full is held
    /// until it has a place.
    #[test]
    fn the_application_waits_for_room() {
        use crate::ids::NodeId;
        use crate::tables::budget::DEPTH;
        use crate::tables::delivery::DELIVERY_QUEUE;
        let hour = Duration::from_secs(3600);
        let config = Config {
            boot_listen: Duration::from_secs(1),
            home_override: Channel::new(3),
            sweep_interval: hour,
            heartbeat_min: hour,
            heartbeat_max: hour,
            ..Config::default()
        };
        let mut r = Relay::new(config, 1);
        let mut now = Instant::ZERO;
        r.handle(Event::Wakeup, now, &mut |_| {});
        assert!(!r.accepts_send(now), "booting");
        drive(&mut r, &mut now, Instant::ZERO + Duration::from_secs(2));
        assert!(r.accepts_send(now));

        // Messages for a sleepy node at this relay's position that is not
        // heard fill the delivery queue.
        let me = r.config.position;
        let send = |r: &mut Relay, k: u8, now| {
            let cmd = AppCommand::Send {
                dst_pos: me,
                dst_id: NodeId([1; 4]),
                payload: &[k],
            };
            r.handle(Event::App(cmd), now, &mut |_| {});
        };
        for k in 0..=DELIVERY_QUEUE as u8 {
            assert!(r.accepts_send(now), "message {k}");
            send(&mut r, k, now);
        }
        assert_eq!(r.holding().count(), DELIVERY_QUEUE + 1, "one held");
        assert!(!r.accepts_send(now));
        send(&mut r, 254, now);
        assert_eq!(
            r.holding().count(),
            DELIVERY_QUEUE + 1,
            "taken while holding"
        );
        // A delivery TTL on the oldest is stale and makes way for the held
        // one; the others stay, and the application may send again.
        let ttl = r.config.delivery_ttl;
        let until = now + ttl + Duration::from_secs(1);
        drive(&mut r, &mut now, until);
        assert!(r.held.is_none());
        assert_eq!(
            r.holding().count(),
            DELIVERY_QUEUE,
            "the held one, now queued"
        );
        assert!(r.accepts_send(now));

        // A bucket spent down to a full frame keeps the application
        // waiting until it has refilled that much, and wakes the relay then.
        let mut r = Relay::new(config, 1);
        let mut now = Instant::ZERO;
        drive(&mut r, &mut now, Instant::ZERO + Duration::from_secs(2));
        r.budget.record(DEPTH, now);
        r.handle(Event::Wakeup, now, &mut |_| {});
        assert!(!r.accepts_send(now));
        let ready = r.next_wakeup().expect("a wakeup for the application");
        assert!(ready > now);
        assert!(!r.accepts_send(Instant::from_micros(ready.as_micros() - 1)));
        assert!(r.accepts_send(ready));
    }

    /// With the regular room closed by the bucket, a packet that has come
    /// two hops takes hop place 2, then 1, whatever the bucket holds; a
    /// sleepy node's own, or a third from two hops out, is refused. A place frees
    /// when its frame leaves (roadmap D31).
    #[test]
    fn a_packet_that_has_travelled_takes_a_hop_place() {
        use crate::event::DropReason;
        use crate::wire::{Data, MAX_PAYLOAD};
        let hour = Duration::from_secs(3600);
        let config = Config {
            boot_listen: Duration::from_secs(1),
            home_override: Channel::new(3),
            sweep_interval: hour,
            heartbeat_min: hour,
            heartbeat_max: hour,
            ..Config::default()
        };
        let mut r = Relay::new(config, 1);
        let mut now = Instant::ZERO;
        drive(&mut r, &mut now, Instant::ZERO + Duration::from_secs(2));
        now = Instant::ZERO + Duration::from_secs(10);
        // Room for replies, not for a full frame on top of the reserve.
        let spend = r.budget.available(now) - Duration::from_millis(300);
        r.budget.record(spend, now);
        let far = Position {
            lat: r.config.position.lat + 100_000,
            ..r.config.position
        };
        let home = Channel::new(5).unwrap();
        let offer = |r: &mut Relay, now: &mut Instant, tag: u8, hops: u8| {
            let payload = [tag; MAX_PAYLOAD];
            let d = Data {
                sender_home: home,
                dst_hop: r.me(),
                src_hop: name(1),
                dst_pos: far,
                dst_id: NOBODY,
                hops,
                time_left: u8::MAX,
                payload: &payload,
            };
            let f = mac::frame(&Packet::Data(d), r.home, MIN_PREAMBLE_SYMBOLS);
            let event = Event::Received {
                channel: r.home,
                rssi_dbm: -80,
                snr_db: 10,
                bytes: &f.bytes[..f.len as usize],
            };
            let mut refused = None;
            let mut pending = Vec::new();
            r.handle(event, *now, &mut |a| match a {
                Action::Trace(Trace::Refused { reason, .. }) => refused = Some(reason),
                Action::Cad(_) | Action::Rssi(_) | Action::Transmit(_) => pending.push(a),
                _ => {}
            });
            // Send the reply, so the next packet can be answered.
            let until = *now + Duration::from_millis(100);
            drive_from(r, now, until, pending);
            let place = r
                .queue
                .iter()
                .find(|o| o.msg_id() == Some(d.msg_id()))
                .map(|o| o.place);
            (place, refused)
        };
        assert_eq!(
            offer(&mut r, &mut now, 1, 0),
            (None, Some(DropReason::Budget))
        );
        assert_eq!(offer(&mut r, &mut now, 2, 2), (Some(2), None));
        assert_eq!(offer(&mut r, &mut now, 3, 2), (Some(1), None));
        assert_eq!(
            offer(&mut r, &mut now, 4, 2),
            (None, Some(DropReason::Budget))
        );
        r.queue.retain(|_, o| o.place != 1);
        assert_eq!(offer(&mut r, &mut now, 5, 2), (Some(1), None));
        assert_eq!(r.queue.regular(), 0);
    }

    /// A full record table gives up the least recently active record of a
    /// message this relay does not hold: a message waiting in the send
    /// queue keeps its record however old (spec "Message records", roadmap D31).
    #[test]
    fn a_waiting_message_keeps_its_record() {
        use crate::ids::MsgId;
        use crate::tables::records::{Record, HELD, RECORDS};
        use crate::wire::{Data, MAX_PAYLOAD};
        let hour = Duration::from_secs(3600);
        let config = Config {
            boot_listen: Duration::from_secs(1),
            home_override: Channel::new(3),
            sweep_interval: hour,
            heartbeat_min: hour,
            heartbeat_max: hour,
            ..Config::default()
        };
        let mut r = Relay::new(config, 1);
        let mut now = Instant::ZERO;
        drive(&mut r, &mut now, Instant::ZERO + Duration::from_secs(2));
        now = Instant::ZERO + Duration::from_secs(10);
        // Room for replies only: what is taken waits in a hop place.
        let spend = r.budget.available(now) - Duration::from_millis(300);
        r.budget.record(spend, now);
        let far = Position {
            lat: r.config.position.lat + 100_000,
            ..r.config.position
        };
        let offer = |r: &mut Relay, now: &mut Instant, tag: u8, hops: u8| {
            let payload = [tag; MAX_PAYLOAD];
            let d = Data {
                sender_home: Channel::new(5).unwrap(),
                dst_hop: r.me(),
                src_hop: name(1),
                dst_pos: far,
                dst_id: NOBODY,
                hops,
                time_left: u8::MAX,
                payload: &payload,
            };
            let f = mac::frame(&Packet::Data(d), r.home, MIN_PREAMBLE_SYMBOLS);
            let event = Event::Received {
                channel: r.home,
                rssi_dbm: -80,
                snr_db: 10,
                bytes: &f.bytes[..f.len as usize],
            };
            let mut pending = Vec::new();
            r.handle(event, *now, &mut |a| {
                if matches!(a, Action::Cad(_) | Action::Rssi(_) | Action::Transmit(_)) {
                    pending.push(a)
                }
            });
            let until = *now + Duration::from_millis(100);
            drive_from(r, now, until, pending);
            d.msg_id()
        };
        let waiting = offer(&mut r, &mut now, 1, 2);
        assert!(r.queue.iter().any(|o| o.msg_id() == Some(waiting)));
        // Every other record newer.
        let later = now + Duration::from_secs(1);
        for i in 1..RECORDS as u32 {
            let at = r.records.place(|_| false).unwrap();
            r.records.put(at, Record::new(MsgId(i), None, 0, later));
        }
        assert!(r.records.is_full());
        now = later + Duration::from_secs(1);
        let next = offer(&mut r, &mut now, 2, 3);
        assert!(
            r.records.get(waiting).is_some(),
            "the waiting message's record went"
        );
        assert!(r.records.get(next).is_some());
        assert!(
            r.records.get(MsgId(1)).is_none(),
            "the oldest not held went"
        );
        // What a relay holds stays within its places, so a record place
        // is always found.
        assert!(r.queue.len() + usize::from(r.held.is_some()) <= HELD);
        assert!(r.record_place().is_some());
    }

    /// Entries waiting for one busy neighbour keep their arrival order
    /// through bans that end and recur, and of equals the oldest is
    /// offered first each time (roadmap D31 rule 4). Choosing an entry
    /// used to move it to the front, which reversed the waiting entries
    /// at every ban and left the oldest waiting for minutes.
    #[test]
    fn waiting_entries_keep_arrival_order_through_bans() {
        use crate::ids::MsgId;
        use crate::tables::budget::BAN;
        use crate::tables::records::Record;
        use crate::tables::send_queue::{Kind, Outgoing};
        use crate::wire::{Ack, Data, Heartbeat};
        let hour = Duration::from_secs(3600);
        let config = Config {
            boot_listen: Duration::from_secs(1),
            home_override: Channel::new(3),
            sweep_interval: hour,
            heartbeat_min: hour,
            heartbeat_max: hour,
            ..Config::default()
        };
        let mut r = Relay::new(config, 1);
        let mut now = Instant::ZERO;
        drive(&mut r, &mut now, Instant::ZERO + Duration::from_secs(2));
        let me = r.config.position;
        let b = Position {
            lat: me.lat + 5_000,
            ..me
        };
        let far = Position {
            lat: me.lat + 100_000,
            ..me
        };
        let (home, b_home) = (r.home, Channel::new(5).unwrap());
        let hear = |r: &mut Relay, now: Instant, p: &Packet<'_>| {
            let f = mac::frame(p, home, MIN_PREAMBLE_SYMBOLS);
            let event = Event::Received {
                channel: home,
                rssi_dbm: -80,
                snr_db: 10,
                bytes: &f.bytes[..f.len as usize],
            };
            let mut pending = Vec::new();
            r.handle(event, now, &mut |a| pending.push(a));
            pending
        };
        let hb = Heartbeat {
            sleepy: false,
            anchorless: false,
            home: b_home,
            id: id_of(b),
            short: b.short(),
        };
        hear(&mut r, now, &Packet::Heartbeat(hb));
        // Four forwards for B, queued in this order.
        let mut ids = Vec::new();
        for tag in 0..4u8 {
            let payload = [tag];
            let d = Data {
                sender_home: home,
                dst_hop: name(0),
                src_hop: r.me(),
                dst_pos: far,
                dst_id: NOBODY,
                hops: 2,
                time_left: u8::MAX,
                payload: &payload,
            };
            let parent = Some((name(7), Channel::new(6).unwrap()));
            let at = r.record_place().unwrap();
            r.records.put(at, Record::new(d.msg_id(), parent, 1, now));
            let f = mac::frame(&Packet::Data(d), home, MIN_PREAMBLE_SYMBOLS);
            r.queue.push(Outgoing::new(f, Kind::Forward)).unwrap();
            ids.push(d.msg_id());
        }
        // B refuses the first `refusals` offers, then takes everything.
        let refusals = 3;
        let mut offers: Vec<(Instant, MsgId)> = Vec::new();
        let until = now + Duration::from_secs(20);
        let mut pending: Vec<Action> = Vec::new();
        r.handle(Event::Wakeup, now, &mut |a| pending.push(a));
        loop {
            let event = match pending.pop() {
                Some(Action::Cad(channel)) => Event::CadDone {
                    channel,
                    busy: false,
                },
                Some(Action::Rssi(channel)) => Event::RssiSample { channel, dbm: -130 },
                Some(Action::Transmit(f)) => {
                    now += mac::airtime(&f);
                    r.handle(Event::TxDone, now, &mut |a| pending.push(a));
                    if let Ok(Packet::Data(d)) = Packet::decode(f.payload()) {
                        let id = d.msg_id();
                        offers.push((now, id));
                        now += Duration::from_millis(5);
                        let reply = Packet::Ack(Ack {
                            msg_id: id,
                            from: id_of(b),
                            to: r.me(),
                            refused: offers.len() <= refusals,
                            wait: 0,
                        });
                        pending.extend(hear(&mut r, now, &reply));
                    }
                    continue;
                }
                Some(_) => continue,
                None => match r.next_wakeup() {
                    Some(t) if t <= until => {
                        now = now.max(t);
                        Event::Wakeup
                    }
                    _ => break,
                },
            };
            r.handle(event, now, &mut |a| pending.push(a));
            if offers.len() == refusals {
                // Every ban has ended and recurred: the queue is as it came.
                let queued: Vec<_> = r.queue.iter().filter_map(|o| o.msg_id()).collect();
                assert_eq!(queued, ids, "arrival order");
            }
        }
        let offered: Vec<_> = offers.iter().map(|(_, id)| *id).collect();
        // The oldest is offered at every ban's end, then each in turn.
        let mut expect = std::vec![ids[0]; refusals];
        expect.extend(&ids);
        assert_eq!(offered, expect);
        for w in offers[..=refusals].windows(2) {
            assert!(w[1].0.since(w[0].0) >= BAN, "offered within the ban");
        }
        assert!(r.queue.is_empty());
    }

    /// Two relays offer one message to the same neighbour D at once, as a
    /// retry after a lost ACK and a return can (roadmap D32). D's answers
    /// to the other sender, C, taking or refusing, neither end this
    /// relay's hand-over nor ban D here; only D's answer to this relay
    /// does. When an ACK named only the message and D, D's ACK to C ended
    /// this hand-over too, and the message, which D had not taken from
    /// this relay, was lost.
    #[test]
    fn an_answer_to_another_sender_is_not_ours() {
        use crate::tables::records::Record;
        use crate::tables::send_queue::{Kind, Outgoing};
        use crate::wire::{Ack, Data, Heartbeat};
        let hour = Duration::from_secs(3600);
        let config = Config {
            boot_listen: Duration::from_secs(1),
            home_override: Channel::new(3),
            sweep_interval: hour,
            heartbeat_min: hour,
            heartbeat_max: hour,
            ..Config::default()
        };
        let mut r = Relay::new(config, 1);
        let mut now = Instant::ZERO;
        drive(&mut r, &mut now, Instant::ZERO + Duration::from_secs(2));
        let me = r.config.position;
        let d_pos = Position {
            lat: me.lat + 5_000,
            ..me
        };
        let far = Position {
            lat: me.lat + 100_000,
            ..me
        };
        let home = r.home;
        let hear = |r: &mut Relay, now: Instant, p: &Packet<'_>| {
            let f = mac::frame(p, home, MIN_PREAMBLE_SYMBOLS);
            let event = Event::Received {
                channel: home,
                rssi_dbm: -80,
                snr_db: 10,
                bytes: &f.bytes[..f.len as usize],
            };
            let mut actions = Vec::new();
            r.handle(event, now, &mut |a| actions.push(a));
            actions
        };
        let hb = Heartbeat {
            sleepy: false,
            anchorless: false,
            home: Channel::new(5).unwrap(),
            id: id_of(d_pos),
            short: d_pos.short(),
        };
        hear(&mut r, now, &Packet::Heartbeat(hb));
        let data = Data {
            sender_home: home,
            dst_hop: name(0),
            src_hop: r.me(),
            dst_pos: far,
            dst_id: NOBODY,
            hops: 2,
            time_left: u8::MAX,
            payload: b"one message, two senders",
        };
        let id = data.msg_id();
        let parent = Some((name(7), Channel::new(6).unwrap()));
        let at = r.record_place().unwrap();
        r.records.put(at, Record::new(id, parent, 1, now));
        let f = mac::frame(&Packet::Data(data), home, MIN_PREAMBLE_SYMBOLS);
        r.queue.push(Outgoing::new(f, Kind::Forward)).unwrap();

        // Run until the offer to D has gone out.
        let mut pending = Vec::new();
        r.handle(Event::Wakeup, now, &mut |a| pending.push(a));
        loop {
            let event = match pending.pop().expect("the offer is sent") {
                Action::Cad(channel) => Event::CadDone {
                    channel,
                    busy: false,
                },
                Action::Rssi(channel) => Event::RssiSample { channel, dbm: -130 },
                Action::Transmit(f) => {
                    assert!(matches!(Packet::decode(f.payload()), Ok(Packet::Data(_))));
                    now += mac::airtime(&f);
                    r.handle(Event::TxDone, now, &mut |_| {});
                    break;
                }
                _ => continue,
            };
            r.handle(event, now, &mut |a| pending.push(a));
        }
        let to_me = Ack {
            msg_id: id,
            from: id_of(d_pos),
            to: r.me(),
            refused: false,
            wait: 0,
        };
        let to_c = Ack {
            to: name(0x0c0c_0c0c),
            ..to_me
        };
        let banned = |r: &Relay, now| {
            r.neighbours()
                .any(|n| n.position == d_pos && r.neighbours.is_banned(n, now))
        };
        now += Duration::from_millis(5);
        for answer in [
            to_c,
            Ack {
                refused: true,
                ..to_c
            },
        ] {
            let actions = hear(&mut r, now, &Packet::Ack(answer));
            assert!(r.handover.awaiting_ack(), "{answer:?} ended the hand-over");
            assert!(!banned(&r, now), "{answer:?} banned D");
            assert!(!actions.iter().any(|a| matches!(a, Action::Trace(_))));
        }
        let actions = hear(&mut r, now, &Packet::Ack(to_me));
        assert!(r.handover.idle() && r.queue.is_empty());
        assert!(actions.iter().any(|a| matches!(
            a,
            Action::Trace(Trace::Forwarded { msg_id, .. }) if *msg_id == id
        )));
    }

    /// A neighbour whose heartbeats this relay stops hearing, but which
    /// answers its hand-overs, taking or refusing them, is alive and stays
    /// in the table past the 390 s a heartbeat alone keeps it (spec "Neighbour table", "The ACK").
    /// Without this, a relay whose only neighbour's heartbeats were lost
    /// for a few rotations dropped it while it was carrying every message.
    #[test]
    fn a_neighbour_that_answers_stays_live() {
        use crate::tables::records::Record;
        use crate::tables::send_queue::{Kind, Outgoing};
        use crate::wire::{Ack, Data};
        let hour = Duration::from_secs(3600);
        let config = Config {
            boot_listen: Duration::from_secs(1),
            home_override: Channel::new(3),
            sweep_interval: hour,
            ..Config::default()
        };
        let mut r = Relay::new(config, 1);
        let mut now = Instant::ZERO;
        drive(&mut r, &mut now, Instant::ZERO + Duration::from_secs(2));
        let me = r.config.position;
        let d_pos = Position {
            lat: me.lat + 5_000,
            ..me
        };
        let far = Position {
            lat: me.lat + 100_000,
            ..me
        };
        let home = r.home;
        let hear = |r: &mut Relay, now: Instant, p: &Packet<'_>| {
            let f = mac::frame(p, home, MIN_PREAMBLE_SYMBOLS);
            let event = Event::Received {
                channel: home,
                rssi_dbm: -80,
                snr_db: 10,
                bytes: &f.bytes[..f.len as usize],
            };
            let mut actions = Vec::new();
            r.handle(event, now, &mut |a| actions.push(a));
            actions
        };
        let heard_at = now;
        hear(
            &mut r,
            now,
            &Packet::Heartbeat(Heartbeat {
                sleepy: false,
                anchorless: false,
                home: Channel::new(5).unwrap(),
                id: id_of(d_pos),
                short: d_pos.short(),
            }),
        );
        let expiry = r.neighbours.params().expire_after;
        assert_eq!(expiry, Duration::from_secs(390));
        let live = |r: &Relay, now| r.neighbours.live(now).any(|(_, n)| n.position == d_pos);

        // Hand D a message and let D answer, taking it, then refusing one.
        let answer = |r: &mut Relay, now: &mut Instant, tag: u8, refused: bool| {
            let payload = [tag];
            let data = Data {
                sender_home: home,
                dst_hop: name(0),
                src_hop: r.me(),
                dst_pos: far,
                dst_id: NOBODY,
                hops: 2,
                time_left: u8::MAX,
                payload: &payload,
            };
            let id = data.msg_id();
            let parent = Some((name(7), Channel::new(6).unwrap()));
            let at = r.record_place().unwrap();
            r.records.put(at, Record::new(id, parent, 1, *now));
            let f = mac::frame(&Packet::Data(data), home, MIN_PREAMBLE_SYMBOLS);
            r.queue.push(Outgoing::new(f, Kind::Forward)).unwrap();
            let until = *now + Duration::from_millis(100);
            let mut pending = Vec::new();
            r.handle(Event::Wakeup, *now, &mut |a| pending.push(a));
            let sent = drive_from(r, now, until, pending);
            assert!(sent
                .iter()
                .any(|(_, f)| f.channel == Channel::new(5).unwrap()));
            let ack = Ack {
                msg_id: id,
                from: id_of(d_pos),
                to: r.me(),
                refused,
                wait: 0,
            };
            let pending = hear(r, *now, &Packet::Ack(ack));
            r.queue.retain(|_, _| false);
            r.handover.cancel();
            let until = *now + Duration::from_millis(100);
            drive_from(r, now, until, pending);
        };
        now = heard_at + (expiry - Duration::from_secs(10));
        answer(&mut r, &mut now, 1, false);
        let taken_at = now;
        assert!(live(&r, heard_at + expiry), "taken: alive");
        now = taken_at + (expiry - Duration::from_secs(10));
        answer(&mut r, &mut now, 2, true);
        let refused_at = now;
        assert!(live(&r, taken_at + expiry), "refused: alive");
        assert!(!live(&r, refused_at + expiry), "silent: expired");
    }

    /// A running relay with two neighbours, both banned for one [`BAN`]
    /// from the returned instant, and a destination far beyond them.
    fn with_every_neighbour_banned() -> (Relay, Instant, Position) {
        let hour = Duration::from_secs(3600);
        let config = Config {
            boot_listen: Duration::from_secs(1),
            home_override: Channel::new(3),
            sweep_interval: hour,
            heartbeat_min: hour,
            heartbeat_max: hour,
            ..Config::default()
        };
        let mut r = Relay::new(config, 1);
        let mut now = Instant::ZERO;
        drive(&mut r, &mut now, Instant::ZERO + Duration::from_secs(2));
        let me = r.config.position;
        for k in 1..=2 {
            let at = Position {
                lat: me.lat + 5_000 * k,
                ..me
            };
            let hb = Heartbeat {
                sleepy: false,
                anchorless: false,
                home: Channel::new(5).unwrap(),
                id: id_of(at),
                short: at.short(),
            };
            r.neighbours.heard(&hb, at, -80, &r.plane, now);
            r.neighbours.refused(hb.id, now + BAN);
        }
        let far = Position {
            lat: me.lat + 100_000,
            ..me
        };
        (r, now, far)
    }

    /// Offer `r` a full DATA to `dst` received with `hops`, a sleepy node's
    /// own at 0, and send its reply. Returns the place it took and why it was
    /// refused, if it was.
    fn offer_data(
        r: &mut Relay,
        now: &mut Instant,
        dst: Position,
        tag: u8,
        hops: u8,
    ) -> (Option<u8>, Option<crate::event::DropReason>) {
        let (place, refused, _) = offer_timed(r, now, dst, tag, hops, u8::MAX);
        (place, refused)
    }

    /// [`offer_data`] with `time_left` units left; also returns why the
    /// message was dropped, if it was.
    fn offer_timed(
        r: &mut Relay,
        now: &mut Instant,
        dst: Position,
        tag: u8,
        hops: u8,
        time_left: u8,
    ) -> (
        Option<u8>,
        Option<crate::event::DropReason>,
        Option<crate::event::DropReason>,
    ) {
        use crate::wire::{Data, MAX_PAYLOAD};
        let payload = [tag; MAX_PAYLOAD];
        let d = Data {
            sender_home: Channel::new(5).unwrap(),
            dst_hop: r.me(),
            src_hop: name(1),
            dst_pos: dst,
            dst_id: NOBODY,
            hops,
            time_left,
            payload: &payload,
        };
        let f = mac::frame(&Packet::Data(d), r.home, MIN_PREAMBLE_SYMBOLS);
        let event = Event::Received {
            channel: r.home,
            rssi_dbm: -80,
            snr_db: 10,
            bytes: &f.bytes[..f.len as usize],
        };
        let (mut refused, mut dropped) = (None, None);
        let mut pending = Vec::new();
        r.handle(event, *now, &mut |a| match a {
            Action::Trace(Trace::Refused { reason, .. }) => refused = Some(reason),
            Action::Trace(Trace::Dropped { reason, .. }) => dropped = Some(reason),
            Action::Cad(_) | Action::Rssi(_) | Action::Transmit(_) => pending.push(a),
            _ => {}
        });
        let until = *now + Duration::from_millis(100);
        drive_from(r, now, until, pending);
        let place = r
            .queue
            .iter()
            .find(|o| o.msg_id() == Some(d.msg_id()))
            .map(|o| o.place);
        (place, refused, dropped)
    }

    /// A packet whose time is gone is work like any other: the network
    /// has taken the message, and only its origin ends it by age
    /// (spec "Receiving DATA").
    #[test]
    fn a_packet_whose_time_is_gone_is_taken_and_sent_on() {
        let (mut r, mut now, far) = with_every_neighbour_banned();
        assert_eq!(
            offer_timed(&mut r, &mut now, far, 1, 2, 0),
            (Some(0), None, None)
        );
        assert_eq!(
            offer_timed(&mut r, &mut now, far, 2, 2, 1),
            (Some(0), None, None)
        );
    }

    /// A message waiting here carries its time as it is at each attempt,
    /// rounded down, down to none, and waits on past it, in the regular
    /// room or a hop place (spec "Receiving DATA").
    #[test]
    fn a_waiting_message_outlives_its_time() {
        use crate::wire::{Data, TTL_UNIT};
        let (mut r, start, far) = with_every_neighbour_banned();
        let mut now = start;
        let units = 2;
        let (place, _, _) = offer_timed(&mut r, &mut now, far, 1, 2, units);
        assert_eq!(place, Some(0));
        let deadline = start + TTL_UNIT.mul(units as u64);
        // The neighbours do not answer: the message waits for their bans.
        let until = deadline + Duration::from_secs(20);
        let sent = drive(&mut r, &mut now, until);
        let offers: Vec<_> = sent
            .iter()
            .filter_map(|(t, f)| match Packet::decode(f.payload()) {
                Ok(Packet::Data(d)) => Some((*t, d.time_left)),
                _ => None,
            })
            .collect();
        assert!(offers.iter().any(|(t, _)| *t > deadline), "{offers:?}");
        for (t, left) in &offers {
            assert!(
                *left <= Data::units(deadline.since(*t)),
                "{left} units at {t:?}"
            );
        }
        assert_eq!(r.holding().count(), 1, "ended by its time");

        // In a hop place, the same.
        let spend = r.budget.available(now) - Duration::from_millis(300);
        r.budget.record(spend, now);
        let (place, _, _) = offer_timed(&mut r, &mut now, far, 2, 2, 1);
        assert_eq!(place, Some(2));
        let until = now + TTL_UNIT.mul(4);
        drive(&mut r, &mut now, until);
        assert_eq!(r.holding().count(), 2);
    }

    /// A packet already in the network is taken under the same bans, into
    /// the regular room and, with the bucket spent, a hop place: the hop
    /// places' argument against gridlock needs every such packet to move
    /// on (spec "Overload waits", roadmap D31, D34).
    #[test]
    fn a_packet_in_the_network_is_taken_whatever_the_bans() {
        let (mut r, mut now, far) = with_every_neighbour_banned();
        assert_eq!(offer_data(&mut r, &mut now, far, 1, 2), (Some(0), None));
        let spend = r.budget.available(now) - Duration::from_millis(300);
        r.budget.record(spend, now);
        assert_eq!(offer_data(&mut r, &mut now, far, 2, 2), (Some(2), None));
    }

    /// A hand-over carries the record's highest count plus one, stamped at
    /// its first transmission and repeated by its retries; readdressed
    /// after a failure it carries the count of its new first transmission;
    /// one that succeeds raises the highest to its count (spec "Hop count", roadmap
    /// D35). Stamped when queued, the frame would carry 2 throughout.
    #[test]
    fn a_hand_over_stamps_its_count_at_its_first_transmission() {
        use crate::tables::records::Record;
        use crate::tables::send_queue::{Kind, Outgoing};
        use crate::wire::{Ack, Data};
        let (mut r, start, far) = with_every_neighbour_banned();
        let mut now = start + BAN;
        let me = r.config.position;
        let [near, nearest] = [1, 2].map(|k| Position {
            lat: me.lat + 5_000 * k,
            ..me
        });
        let data = Data {
            sender_home: r.home,
            dst_hop: name(0),
            src_hop: r.me(),
            dst_pos: far,
            dst_id: NOBODY,
            hops: 2,
            time_left: u8::MAX,
            payload: b"count",
        };
        let id = data.msg_id();
        let parent = Some((name(7), Channel::new(6).unwrap()));
        let at = r.record_place().unwrap();
        r.records.put(at, Record::new(id, parent, 1, now));
        let f = mac::frame(&Packet::Data(data), r.home, MIN_PREAMBLE_SYMBOLS);
        r.queue.push(Outgoing::new(f, Kind::Forward)).unwrap();
        // A copy with a higher count arrived since it was queued.
        r.records.get_mut(id).unwrap().received(name(8), 6, now);

        let counts = |sent: &[(Instant, Frame)], to: Position| -> Vec<u8> {
            sent.iter()
                .filter_map(|(_, f)| match Packet::decode(f.payload()) {
                    Ok(Packet::Data(d)) if d.dst_hop == id_of(to) => Some(d.hops),
                    _ => None,
                })
                .collect()
        };
        let mut pending = Vec::new();
        r.handle(Event::Wakeup, now, &mut |a| pending.push(a));
        let until = now;
        let mut sent = drive_from(&mut r, &mut now, until, pending);
        while counts(&sent, nearest).len() < 3 {
            let until = now + Duration::from_millis(10);
            sent.extend(drive(&mut r, &mut now, until));
            now = now.max(until);
            assert!(now < start + Duration::from_secs(10), "no third attempt");
        }
        assert_eq!(counts(&sent, nearest), [7, 7, 7], "stamped, then repeated");
        // Another copy arrives during the last attempt.
        r.records.get_mut(id).unwrap().received(name(8), 9, now);
        while counts(&sent, near).is_empty() {
            let until = now + Duration::from_millis(10);
            sent.extend(drive(&mut r, &mut now, until));
            now = now.max(until);
            assert!(now < start + Duration::from_secs(10), "not readdressed");
        }
        assert_eq!(counts(&sent, near), [10], "readdressed: stamped again");
        let ack = Ack {
            msg_id: id,
            from: id_of(near),
            to: r.me(),
            refused: false,
            wait: 0,
        };
        let f = mac::frame(&Packet::Ack(ack), r.home, MIN_PREAMBLE_SYMBOLS);
        let event = Event::Received {
            channel: r.home,
            rssi_dbm: -80,
            snr_db: 10,
            bytes: &f.bytes[..f.len as usize],
        };
        r.handle(event, now, &mut |_| {});
        assert_eq!(r.records.get(id).unwrap().highest_hops(), 10);
    }

    /// A retransmission, the same sender and count again, is ACKed and not
    /// taken; so is another copy while this relay holds one (spec "Receiving DATA" cases
    /// 3 and 4, roadmap D35). Here the relay holds the message, waiting
    /// for its banned neighbours.
    #[test]
    fn a_retransmission_or_a_copy_while_holding_is_only_acked() {
        use crate::wire::Data;
        let (mut r, mut now, far) = with_every_neighbour_banned();
        let offer = |r: &mut Relay, now: &mut Instant, from: u32, hops: u8| {
            let d = Data {
                sender_home: Channel::new(5).unwrap(),
                dst_hop: r.me(),
                src_hop: name(from),
                dst_pos: far,
                dst_id: NOBODY,
                hops,
                time_left: u8::MAX,
                payload: b"again",
            };
            let f = mac::frame(&Packet::Data(d), r.home, MIN_PREAMBLE_SYMBOLS);
            let event = Event::Received {
                channel: r.home,
                rssi_dbm: -80,
                snr_db: 10,
                bytes: &f.bytes[..f.len as usize],
            };
            let mut pending = Vec::new();
            r.handle(event, *now, &mut |a| pending.push(a));
            let until = *now + Duration::from_millis(100);
            let acks: Vec<_> = drive_from(r, now, until, pending)
                .into_iter()
                .filter_map(|(_, f)| match Packet::decode(f.payload()) {
                    Ok(Packet::Ack(a)) => Some(a),
                    _ => None,
                })
                .collect();
            let held = r.queue.iter().filter(|o| o.msg_id() == Some(d.msg_id()));
            (acks, held.count())
        };
        let taken = |acks: &[crate::wire::Ack]| acks.len() == 1 && !acks[0].refused;
        let (acks, held) = offer(&mut r, &mut now, 1, 2);
        assert!(taken(&acks) && held == 1, "the first is taken");
        let (acks, held) = offer(&mut r, &mut now, 1, 2);
        assert!(
            taken(&acks) && held == 1,
            "its retransmission is only ACKed"
        );
        let (acks, held) = offer(&mut r, &mut now, 9, 2);
        assert!(taken(&acks) && held == 1, "another copy too, while held");
    }

    /// Once the relay a message is addressed to has taken it from this
    /// one, the message has arrived: a later copy is only ACKed, wherever
    /// it comes from (spec "Receiving DATA" case 1, roadmap D35).
    #[test]
    fn a_copy_after_the_addressed_relay_took_it_is_only_acked() {
        use crate::tables::records::Record;
        use crate::tables::send_queue::{Kind, Outgoing};
        use crate::wire::{Ack, Data};
        let (mut r, start, _) = with_every_neighbour_banned();
        let mut now = start + BAN;
        let me = r.config.position;
        let addressed = Position {
            lat: me.lat + 10_000,
            ..me
        };
        let data = Data {
            sender_home: r.home,
            dst_hop: name(0),
            src_hop: r.me(),
            dst_pos: addressed,
            dst_id: id_of(addressed),
            hops: 2,
            time_left: u8::MAX,
            payload: b"arrived",
        };
        let id = data.msg_id();
        let parent = Some((name(7), Channel::new(6).unwrap()));
        let at = r.record_place().unwrap();
        r.records.put(at, Record::new(id, parent, 1, now));
        let f = mac::frame(&Packet::Data(data), r.home, MIN_PREAMBLE_SYMBOLS);
        r.queue.push(Outgoing::new(f, Kind::Forward)).unwrap();
        let mut pending = Vec::new();
        r.handle(Event::Wakeup, now, &mut |a| pending.push(a));
        let until = now + Duration::from_millis(50);
        let sent = drive_from(&mut r, &mut now, until, pending);
        assert!(sent.iter().any(|(_, f)| matches!(
            Packet::decode(f.payload()),
            Ok(Packet::Data(d)) if d.dst_hop == id_of(addressed)
        )));
        let hear = |r: &mut Relay, now: &mut Instant, p: &Packet<'_>| {
            let f = mac::frame(p, r.home, MIN_PREAMBLE_SYMBOLS);
            let event = Event::Received {
                channel: r.home,
                rssi_dbm: -80,
                snr_db: 10,
                bytes: &f.bytes[..f.len as usize],
            };
            let mut pending = Vec::new();
            r.handle(event, *now, &mut |a| pending.push(a));
            let until = *now + Duration::from_millis(100);
            drive_from(r, now, until, pending)
        };
        let ack = Ack {
            msg_id: id,
            from: id_of(addressed),
            to: r.me(),
            refused: false,
            wait: 0,
        };
        hear(&mut r, &mut now, &Packet::Ack(ack));
        assert!(r.records.get(id).unwrap().arrived());
        assert!(r.queue.is_empty());
        let copy = Data {
            sender_home: Channel::new(5).unwrap(),
            dst_hop: r.me(),
            src_hop: name(9),
            hops: 5,
            time_left: u8::MAX,
            ..data
        };
        let sent = hear(&mut r, &mut now, &Packet::Data(copy));
        let acks: Vec<_> = sent
            .iter()
            .filter_map(|(_, f)| match Packet::decode(f.payload()) {
                Ok(Packet::Ack(a)) => Some(a),
                _ => None,
            })
            .collect();
        assert!(acks.len() == 1 && !acks[0].refused, "{acks:?}");
        assert!(r.queue.is_empty(), "taken");
    }

    /// The relay's own application waits while every neighbour is banned,
    /// and the relay wakes when the first ban ends (roadmap D34).
    #[test]
    fn the_application_waits_while_every_neighbour_is_banned() {
        let (mut r, now, _) = with_every_neighbour_banned();
        r.handle(Event::Wakeup, now, &mut |_| {});
        assert!(!r.accepts_send(now));
        assert_eq!(r.next_wakeup(), Some(now + BAN));
        assert!(r.accepts_send(now + BAN));
    }
}
