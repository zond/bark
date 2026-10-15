//! The relay: medium access, forwarding and the leaves it serves.
//!
//! This file holds the medium access: boot listen and home channel
//! (design §6), the busy sweep (§6.1), the heartbeat (§7), the neighbour
//! table (§8) and the duty-cycle budget (§3, §9.4). Routing and the
//! per-hop transfer (§9) are in `relay/forward.rs`, the leaves this relay
//! serves (§10) in `relay/service.rs`.
//!
//! # Radio discipline
//!
//! Each use of the radio is a job with its own small state machine: the
//! reply (an ACK, taking or refusing, or an ATTACHED) this relay owes, the
//! poll of a leaf, the heartbeat, the hand-over of one entry of the send
//! queue, and the sweep. The relay keeps at most one excursion outstanding
//! (`docs/core-interface.md` §3 rule 1). When the radio is free it starts
//! the first job that is ready, in that order: an owed reply is worth
//! nothing once its sender stops waiting, 100 ms after the frame (§9.3),
//! and a poll is skipped once it is 500 ms late (§10.2). A job that has begun a sequence of excursions (CAD,
//! RSSI, transmit) continues it from each completion event, so sequences
//! never interleave; the hand-over alone lets an owed reply in before it
//! transmits, since its frame can take 139 ms on air. While a hand-over
//! waits for its ACK, only an owed reply uses the radio: the heartbeat and
//! the sweep would take the relay off its home channel, where the ACK
//! arrives (§9.3), and a poll would make it deaf to it. Jobs that are not ready say when they will be, and the earliest
//! of those is `next_wakeup`.

mod forward;
mod service;

use crate::event::{Action, AppCommand, Event, Frame, Trace};
use crate::geo::LocalPlane;
use crate::ids::MsgId;
use crate::node::mac::{self, Radio};
use crate::node::transfer::{Replies, Step, Transfer};
use crate::node::{Config, Core, Rng};
use crate::position::Position;
use crate::radio::{time_on_air, Channel, CHANNELS, MIN_PREAMBLE_SYMBOLS};
use crate::tables::budget::Budget;
use crate::tables::busy::{percent, BusyEstimator, ONE};
use crate::tables::delivered::Delivered;
use crate::tables::delivery::DeliveryQueue;
use crate::tables::leaves::Leaves;
use crate::tables::neighbours::{Neighbour, NeighbourParams, Neighbours};
use crate::tables::records::Records;
use crate::tables::send_queue::SendQueue;
use crate::time::{Duration, Instant};
use crate::wire::{Heartbeat, Packet};

/// Who started the outstanding excursion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Job {
    Sweep,
    Heartbeat,
    HandOver,
    Reply,
    Poll,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    /// Not started: the driver's first `Wakeup` starts it.
    Off,
    /// Listening before choosing a home channel (design §6.2), until then.
    Booting(Instant),
    Running,
}

/// Listen-before-talk progress of one heartbeat or poll.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Check {
    Idle,
    Cad,
    Rssi,
    Tx,
}

/// The busy sweep (design §6.1): one RSSI reading at a time, channels in
/// turn, spread over the sweep interval.
struct Sweep {
    next_at: Instant,
    /// The channel the next reading takes.
    channel: Channel,
    /// When the home channel was last read.
    last_home: Instant,
    /// Time spent since `last_home` receiving packets for other nodes on
    /// the home channel.
    foreign: Duration,
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

/// The rotating heartbeat (design §7) and the one-off announcement on the
/// old channel after a move (§6.3).
struct Beacon {
    next_at: Instant,
    /// Not before this: a listen-before-talk backoff or a budget deferral.
    ready_at: Instant,
    rotation: Channel,
    /// The old home, to announce the new one on, after a move.
    announce: Option<Channel>,
    /// The heartbeat in progress: its channel, and whether it is the announcement.
    current: Option<(Channel, bool)>,
    busy_checks: u8,
    check: Check,
}

/// The move rule's timers (design §6.3).
#[derive(Default)]
struct Mover {
    excess_since: Option<Instant>,
    move_at: Option<Instant>,
}

/// A snapshot of a relay's tables and budget ([`Relay::load`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Load {
    /// Airtime in the duty-cycle bucket (design §9.4).
    pub bucket: Duration,
    /// Frames in the send queue, hop places included (§9.3).
    pub send_queue: usize,
    /// Messages waiting for a leaf (§10.4).
    pub delivery_queue: usize,
    /// Leaf table entries, expired ones not yet removed included (§10.2).
    pub leaves: usize,
    /// Neighbour table entries, expired ones not yet removed included (§8.2).
    pub neighbours: usize,
}

/// Relay role.
pub struct Relay {
    config: Config,
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
    /// attempts went unanswered (§9.3); not tried.
    skipped: u64,
    replies: Replies,
    leaves: Leaves,
    poller: service::Poller,
    delivery: DeliveryQueue,
    delivered: Delivered,
    /// When queued deliveries are next retried (§10.4: every 60 s).
    delivery_retry_at: Instant,
    /// When the bucket lets the application send, if only it is missing.
    app_ready_at: Option<Instant>,
    /// The application's message, from when it is taken until its first
    /// hop takes it, it arrives here, or it is dropped (roadmap D29).
    own: Option<MsgId>,
    /// Refusals in a row of `own` by banned neighbours: each doubles the
    /// hold before it is offered again (`budget::hold`).
    own_refusals: u8,
    /// The application's message for a leaf of this relay, set aside
    /// while the delivery queue is full.
    held: Option<forward::Held>,
    /// The send queue may have room for the delivery queue: a hand-over
    /// ended or the delivery queue changed.
    feed_due: bool,
    /// When the bucket will fit the DELIVER the send queue last had no
    /// room for, as a hand-over the bucket cannot fit yet is deferred.
    feed_at: Option<Instant>,
}

impl Relay {
    /// A relay with empty tables and the default configuration, not set
    /// up: built at compile time, so that a driver can keep a relay in a
    /// static and set it up in place with [`Relay::reset`]. A relay is
    /// 150 KiB, and [`Relay::new`] needs about as much again on the stack
    /// (`docs/core-interface.md` §6).
    pub const UNSET: Relay = {
        let config = Config::DEFAULT;
        let channel = match Channel::new(0) {
            Some(c) => c,
            None => unreachable!(),
        };
        Relay {
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
                last_home: Instant::ZERO,
                foreign: Duration::ZERO,
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
            leaves: Leaves::from_config(&config),
            poller: service::Poller::IDLE,
            delivery: DeliveryQueue::from_config(&config),
            delivered: Delivered::new(),
            delivery_retry_at: Instant::ZERO,
            app_ready_at: None,
            own: None,
            own_refusals: 0,
            held: None,
            feed_due: false,
            feed_at: None,
            config,
            rng: Rng::new(0),
        }
    };

    /// Create a relay. `seed` drives every random choice the relay makes;
    /// two relays with the same config and seed behave identically.
    pub fn new(config: Config, seed: u64) -> Relay {
        let mut relay = Relay::UNSET;
        relay.reset(config, seed);
        relay
    }

    /// Make this relay, in place, what [`Relay::new`] would build from
    /// `config` and `seed`: everything it held is gone.
    pub fn reset(&mut self, config: Config, seed: u64) {
        let mut rng = Rng::new(seed);
        let home = config
            .home_override
            .unwrap_or_else(|| random_channel(&mut rng));
        let rotation = random_channel(&mut rng);
        // Every field by name, so that a new one cannot be left out. The
        // large tables are emptied where they lie.
        let Relay {
            config: c,
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
            leaves,
            poller,
            delivery,
            delivered,
            delivery_retry_at,
            app_ready_at,
            own,
            own_refusals,
            held,
            feed_due,
            feed_at,
        } = self;
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
            last_home: Instant::ZERO,
            foreign: Duration::ZERO,
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
        };
        *mover = Mover::default();
        records.clear();
        queue.clear();
        *handover = Transfer::new();
        *skipped = 0;
        *replies = Replies::new();
        *leaves = Leaves::from_config(&config);
        *poller = service::Poller::IDLE;
        *delivery = DeliveryQueue::from_config(&config);
        *delivered = Delivered::new();
        *delivery_retry_at = Instant::ZERO;
        *app_ready_at = None;
        *own = None;
        *own_refusals = 0;
        *held = None;
        *feed_due = false;
        *feed_at = None;
        *c = config;
        *r = rng;
    }

    /// Current home channel.
    pub fn home(&self) -> Channel {
        self.home
    }

    /// Configuration. `position` is the relay's identity, which the boot
    /// listen may have moved by a metre (design §8.1).
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Whether the boot listen (design §6.2) is still running.
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
    /// their leaf. A driver uses it to tell a message still in the
    /// network from one lost without a trace.
    pub fn holding(&self) -> impl Iterator<Item = MsgId> + '_ {
        self.queue
            .iter()
            .filter_map(|o| o.msg_id())
            .chain(self.delivery.iter().map(|q| q.msg_id))
            .chain(self.held.as_ref().and_then(|h| h.out.msg_id()))
    }

    /// How loaded the relay is at `now`, for a driver to sample. Reading
    /// it changes nothing.
    pub fn load(&self, now: Instant) -> Load {
        Load {
            bucket: self.budget.available(now),
            send_queue: self.queue.len(),
            delivery_queue: self.delivery.len(),
            leaves: self.leaves.len(),
            neighbours: self.neighbours.len(),
        }
    }

    fn start(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        self.stage = Stage::Booting(now + self.config.boot_listen);
        self.sweep.next_at = now;
        self.sweep.last_home = now;
        emit(Action::Listen(self.home));
    }

    /// End of the boot listen: choose home (design §6.2), start heartbeats.
    fn finish_boot(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        self.stage = Stage::Running;
        if self.config.home_override.is_none() {
            let home = self.quietest();
            self.retune(home, emit);
        }
        self.beacon.next_at = now + self.heartbeat_interval();
    }

    fn retune(&mut self, to: Channel, emit: &mut dyn FnMut(Action)) {
        if to != self.home {
            // What was heard on the old channel says nothing of the new.
            self.sweep.pending = None;
            self.sweep.foreign = Duration::ZERO;
            emit(Action::Trace(Trace::HomeChannelMoved {
                from: self.home,
                to,
            }));
            self.home = to;
            emit(Action::Listen(to));
        }
    }

    /// `argmin busy[c]`, ties within 0.01 broken at random (design §6.2),
    /// so relays that hear nothing do not all pick channel 0.
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

    fn heartbeat_interval(&mut self) -> Duration {
        self.rng
            .duration(self.config.heartbeat_min, self.config.heartbeat_max)
    }

    fn timers(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        if let Stage::Booting(until) = self.stage {
            if now >= until {
                self.finish_boot(now, emit);
            }
        }
        if self.mover.move_at.is_some_and(|t| now >= t) {
            self.mover = Mover::default();
            // Re-check after the random wait (design §6.3).
            if self.excess() {
                let old = self.home;
                let to = self.quietest();
                self.retune(to, emit);
                if self.home != old {
                    self.beacon.announce = Some(old);
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
            let running = self.stage == Stage::Running;
            let started = self.start_reply(now, emit)
                || (!awaiting && running && self.poll_due(now) && self.start_poll(now, emit))
                || (!awaiting && self.beacon_due(now) && self.start_heartbeat(now, emit))
                || self.start_handover(now, emit)
                || (!awaiting && self.sweep_due(now) && self.start_sweep(emit));
            if !started {
                break;
            }
        }
    }

    // --- Sweep (design §6.1) ---

    fn sweep_due(&self, now: Instant) -> bool {
        now >= self.sweep.next_at
    }

    /// Each channel once per sweep interval, one reading at a time (§6.1,
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
    /// than any packet it can fall in, and then counts only energy that
    /// would stay on the channel if this relay left it (§6.1).
    fn sweep_sample(&mut self, channel: Channel, dbm: i16, now: Instant) {
        if channel == self.home {
            if let Some(p) = self.sweep.pending.take() {
                let interval = now.since(self.sweep.last_home);
                self.busy
                    .sample_home(p.channel, p.dbm, self.sweep.foreign, interval);
            }
            self.sweep.foreign = Duration::ZERO;
            self.sweep.last_home = now;
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
        if self.stage == Stage::Running && self.config.home_override.is_none() {
            self.move_rule(now);
        }
    }

    /// `busy[home] − min busy` above the move threshold (design §6.3).
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

    // --- Heartbeat (design §7) ---

    fn beacon_due(&self, now: Instant) -> bool {
        self.stage == Stage::Running
            && self.beacon.check == Check::Idle
            && now >= self.beacon.ready_at
            && (self.beacon.current.is_some()
                || self.beacon.announce.is_some()
                || now >= self.beacon.next_at)
    }

    /// The heartbeat in progress. It carries the home at the moment it is
    /// sent, so a move during its backoff still announces the new channel.
    fn heartbeat_frame(&self) -> Frame {
        let (channel, _) = self.beacon.current.unwrap_or((self.beacon.rotation, false));
        let hb = Heartbeat {
            surveyed: self.config.surveyed,
            home: self.home,
            position: self.config.position,
        };
        mac::frame(&Packet::Heartbeat(hb), channel, MIN_PREAMBLE_SYMBOLS)
    }

    /// Check the budget before the channel: there is no point listening for
    /// a slot the budget will not let us use. Nothing else transmits
    /// between this check and the transmission, and the bucket only fills
    /// meanwhile, so the budget cannot have shrunk by then.
    fn start_heartbeat(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) -> bool {
        if self.beacon.current.is_none() {
            self.beacon.current = Some(match self.beacon.announce {
                Some(old) => (old, true),
                None => (self.beacon.rotation, false),
            });
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

    /// The heartbeat went out, or was skipped (design §7). A skipped one
    /// keeps its channel, so the neighbours parked there are not left out
    /// for a whole rotation.
    fn heartbeat_done(&mut self, now: Instant, sent: bool) {
        self.beacon.check = Check::Idle;
        self.beacon.busy_checks = 0;
        if let Some((channel, true)) = self.beacon.current.take() {
            // A second move meanwhile has a newer old channel to announce on.
            if self.beacon.announce == Some(channel) {
                self.beacon.announce = None;
            }
            return;
        }
        if sent {
            self.beacon.rotation = self.beacon.rotation.next();
        }
        self.beacon.next_at = now + self.heartbeat_interval();
    }

    // --- Reception ---

    fn received(&mut self, bytes: &[u8], now: Instant, emit: &mut dyn FnMut(Action)) {
        self.count_busy(bytes, now);
        // A booting relay has no home channel yet to be reached on, nor
        // neighbours to forward to: it takes no traffic (§6.2).
        let running = self.stage == Stage::Running;
        let decoded = Packet::decode(bytes);
        // A relay's DATA says where it listens now (§9.2.1); a leaf's
        // (`hops` 0) names no relay.
        if let Ok(Packet::Data(d)) = decoded {
            if d.hops > 0 {
                self.neighbours.home_heard(d.src_hop, d.sender_home);
            }
        }
        match decoded {
            Ok(Packet::Heartbeat(hb)) => self.heard(hb, now, emit),
            Ok(Packet::Ack(ack)) => self.ack_received(ack, now, emit),
            Ok(Packet::Data(d)) if running && d.dst_hop == self.me() => self.data(d, now, emit),
            Ok(Packet::Attach(a)) if running && a.relay == self.me() => self.attach(a, now),
            _ => {}
        }
    }

    /// Account a reception on the home channel to the busy estimate (§6.1).
    /// Packets addressed to this relay, and the replies to its own
    /// hand-overs, follow it to any channel it moves to, so they are not
    /// counted, and a home reading taken while one was on the air is set
    /// aside.
    /// Everything else, heartbeats included, stays behind and counts.
    fn count_busy(&mut self, bytes: &[u8], now: Instant) {
        // The driver does not report the preamble a sender used. Assuming
        // the minimum undercounts only frames for sleeping leaves, which
        // are rare (design §10.5).
        let len = bytes.len().min(u8::MAX as usize) as u8;
        let airtime = time_on_air(len, MIN_PREAMBLE_SYMBOLS);
        let me = self.me();
        let own = match Packet::decode(bytes) {
            Ok(Packet::Data(d)) => d.dst_hop == me,
            Ok(Packet::Attach(a)) => a.relay == me,
            Ok(Packet::Ack(a)) => self.handover.answered_by(a),
            _ => false,
        };
        if !own {
            self.sweep.foreign = self.sweep.foreign + airtime;
            return;
        }
        if let Some(p) = &mut self.sweep.pending {
            if now.since(p.at) <= airtime {
                p.dbm = None;
            }
        }
    }

    fn heard(&mut self, hb: Heartbeat, now: Instant, emit: &mut dyn FnMut(Action)) {
        if hb.position != self.config.position {
            if let Some(slot) = self.neighbours.heard(&hb, &self.plane, now).new_occupant() {
                self.records.slot_reoccupied(slot);
                self.skipped &= !(1 << slot);
            }
            return;
        }
        if let Stage::Booting(_) = self.stage {
            // Another relay holds this identity: step two counts (≈1.2 m)
            // north and listen again (design §8.1).
            let to = Position {
                lat: hb.position.lat + 2,
                ..hb.position
            };
            self.set_position(to);
            self.stage = Stage::Booting(now + self.config.boot_listen);
            emit(Action::Trace(Trace::PositionShifted { to }));
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
                dst_leaf,
                payload,
            } => self.originate(dst_pos, dst_leaf, payload, now, emit),
            AppCommand::SetPosition { position, surveyed } => {
                self.config.surveyed = surveyed;
                self.set_position(position);
            }
        }
    }
}

fn random_channel(rng: &mut Rng) -> Channel {
    let fallback = Channel::all().next().expect("the plan has channels");
    Channel::new(rng.range(0, CHANNELS as u64 - 1) as u8).unwrap_or(fallback)
}

impl Core for Relay {
    fn handle(&mut self, event: Event<'_>, now: Instant, emit: &mut dyn FnMut(Action)) {
        if self.stage == Stage::Off {
            self.start(now, emit);
        }
        let threshold = self.busy.threshold_dbm();
        match event {
            Event::Wakeup => {}
            Event::Received { bytes, .. } => self.received(bytes, now, emit),
            Event::CadDone { busy, .. } => match self.radio.complete() {
                Some(Job::Heartbeat) => self.heartbeat_check(busy, now, emit),
                Some(Job::HandOver) => self.handover_check(busy, now, emit),
                Some(Job::Reply) => self.reply_check(busy, now, emit),
                Some(Job::Poll) => self.poll_check(busy, now, emit),
                Some(Job::Sweep) | None => {}
            },
            Event::RssiSample { channel, dbm } => match self.radio.complete() {
                Some(Job::Sweep) => self.sweep_sample(channel, dbm, now),
                Some(Job::Heartbeat) => self.heartbeat_check(dbm > threshold, now, emit),
                Some(Job::HandOver) => self.handover_check(dbm > threshold, now, emit),
                Some(Job::Reply) => self.reply_check(dbm > threshold, now, emit),
                Some(Job::Poll) => self.poll_check(dbm > threshold, now, emit),
                None => {}
            },
            Event::TxDone => match self.radio.complete() {
                Some(Job::Heartbeat) => self.heartbeat_done(now, true),
                // Unless the hand-over was given up meanwhile (a displaced record).
                Some(Job::HandOver) if self.handover.step() == Step::Tx => {
                    self.handover.sent(now, self.config.ack_timeout)
                }
                Some(Job::Reply) => self.replies.sent(),
                Some(Job::Poll) => self.poll_sent(),
                _ => {}
            },
            Event::App(cmd) => self.app(cmd, now, emit),
        }
        self.timers(now, emit);
        self.forward_timers(now, emit);
        self.service_timers(now, emit);
        self.pump(now, emit);
        self.app_ready_at = self.send_ready_at(now);
    }

    fn next_wakeup(&self) -> Option<Instant> {
        let mut w: Option<Instant> = None;
        let mut at = |t: Instant| w = Some(w.map_or(t, |w| w.min(t)));
        match self.stage {
            Stage::Off => return Some(Instant::ZERO),
            Stage::Booting(until) => at(until),
            Stage::Running => {}
        }
        if let Some(t) = self.mover.move_at {
            at(t);
        }
        if let Some(t) = self.forward_wakeup() {
            at(t);
        }
        if let Some(t) = self.service_wakeup() {
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
                    self.beacon.next_at
                };
                at(due.max(self.beacon.ready_at));
                if let Some(t) = self.poll_wakeup() {
                    at(t);
                }
            }
            at(self.sweep.next_at);
        }
        w
    }

    fn accepts_send(&self, now: Instant) -> bool {
        Relay::accepts_send(self, now)
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::tables::budget::BAN;
    use std::vec::Vec;

    #[test]
    fn sizes() {
        // docs/core-interface.md §6: the tables, and around them the medium
        // access, the hand-over, one owed reply and one held message in
        // under 1 KiB.
        let relay = core::mem::size_of::<Relay>();
        let leaf = core::mem::size_of::<crate::node::Leaf>();
        let tables = crate::tables::sizes::tables();
        std::println!("Relay {relay} B, tables {tables} B, Leaf {leaf} B");
        assert!(relay <= tables + 1024, "{relay}");
        // It fits the board's RAM: 255 KiB for the application
        // (bark-fw/memory.x), less the BLE controller's buffer, under 5 KiB
        // (docs/implementation-research-2026-10-14.md), and Embassy and the
        // stacks, about 16 KiB (design §15).
        let ram = (255 - 5 - 16) * 1024;
        assert!(relay <= ram, "{relay} of {ram}");
        assert!(leaf < 1024, "{leaf}");
    }

    /// Drive `r` from its own wakeups until `until`, completing every radio
    /// action at once on a quiet channel. Returns the frames it sent.
    fn drive(r: &mut Relay, now: &mut Instant, until: Instant) -> Vec<(Instant, Frame)> {
        drive_from(r, now, until, Vec::new())
    }

    /// [`drive`], with `pending` the actions `r` has asked for already.
    fn drive_from(
        r: &mut Relay,
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

    /// A hybrid leaf, which hears only long preambles while it sleeps,
    /// is polled with the short one at the time its last ATTACHED named,
    /// within the poll's lateness, so that it can listen for it then; a
    /// DELIVER to it keeps the long preamble (protocol §10.2, §10.5).
    #[test]
    fn a_hybrid_leaf_is_polled_at_its_slot_with_a_short_preamble() {
        use crate::ids::{LeafId, MsgId};
        use crate::tables::delivery::Queued;
        use crate::tables::leaves::{Polls, POLL_AIRTIME, POLL_LATE};
        use crate::wire::{Attach, RxMode};
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
        let leaf = LeafId([9; 4]);
        let (me, home) = (r.me(), r.home);
        let attach = |reply| {
            let a = Attach {
                leaf,
                relay: me,
                mode: RxMode::Hybrid,
                reply,
            };
            let f = mac::frame(&Packet::Attach(a), home, MIN_PREAMBLE_SYMBOLS);
            f.bytes[..f.len as usize].to_vec()
        };
        let receive = |r: &mut Relay, bytes: &[u8], now: Instant| {
            let mut pending = Vec::new();
            let event = Event::Received {
                channel: r.home,
                rssi_dbm: -80,
                snr_db: 10,
                bytes,
            };
            r.handle(event, now, &mut |a| pending.push(a));
            pending
        };
        let pending = receive(&mut r, &attach(false), now);
        let period = Polls::from_config(&config).period;
        let until = now + period + Duration::from_secs(2);
        let sent = drive_from(&mut r, &mut now, until, pending);
        let polls: Vec<_> = sent
            .iter()
            .filter_map(|(t, f)| match Packet::decode(f.payload()) {
                Ok(Packet::Attached(a)) => Some((*t, *f, a)),
                _ => None,
            })
            .collect();
        assert!(polls.len() >= 2, "{polls:?}");
        for w in polls.windows(2) {
            let ((t0, _, a0), (t1, f1, _)) = (w[0], w[1]);
            let slot = t0 + POLL_AIRTIME + a0.next_poll();
            assert!(t1 + Duration::from_millis(5) >= slot && t1 <= slot + POLL_LATE);
            assert_eq!(f1.preamble_symbols, MIN_PREAMBLE_SYMBOLS);
        }
        // The answer to the poll keeps the leaf; a DELIVER to it has the
        // long preamble.
        let pending = receive(&mut r, &attach(true), now);
        let at = now;
        drive_from(&mut r, &mut now, at, pending);
        assert_eq!(r.leaves.get(leaf).unwrap().unanswered, 0);
        r.delivery
            .push(Queued::new(MsgId(7), leaf, b"hi", now).unwrap());
        r.feed_due = true;
        let mut pending = Vec::new();
        r.handle(Event::Wakeup, now, &mut |a| pending.push(a));
        let until = now + Duration::from_secs(1);
        let sent = drive_from(&mut r, &mut now, until, pending);
        let deliver = sent
            .iter()
            .find(|(_, f)| matches!(Packet::decode(f.payload()), Ok(Packet::Deliver(_))))
            .expect("a DELIVER");
        assert_eq!(
            deliver.1.preamble_symbols,
            RxMode::Hybrid.preamble_symbols()
        );
    }

    /// A relay reset in place, after it has run and holds messages, does
    /// what a new one does.
    #[test]
    fn a_reset_relay_is_a_new_one() {
        use crate::ids::LeafId;
        let config = Config {
            boot_listen: Duration::from_secs(1),
            ..Config::default()
        };
        let mut used = Relay::new(config, 1);
        let mut now = Instant::ZERO;
        drive(&mut used, &mut now, Instant::ZERO + Duration::from_secs(30));
        let send = AppCommand::Send {
            dst_pos: config.position,
            dst_leaf: LeafId([1; 4]),
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

    /// A relay whose bucket is spent holds one message for a listening
    /// leaf, and nothing else happens: it wakes when its bucket will fit
    /// the DELIVER with the reply reserve, and sends it then, not at the
    /// 60 s retry (protocol §9.3, §10.4).
    #[test]
    fn a_deliver_waits_for_the_bucket_not_the_retry() {
        use crate::ids::{LeafId, MsgId};
        use crate::tables::budget::REPLY_RESERVE;
        use crate::tables::delivery::Queued;
        use crate::wire::{Deliver, RxMode};
        let hour = Duration::from_secs(3600);
        let config = Config {
            boot_listen: Duration::from_secs(1),
            home_override: Channel::new(3),
            sweep_interval: hour,
            heartbeat_min: hour,
            heartbeat_max: hour,
            // No poll within the test.
            poll_interval: hour,
            ..Config::default()
        };
        let mut r = Relay::new(config, 1);
        let mut now = Instant::ZERO;
        drive(&mut r, &mut now, Instant::ZERO + Duration::from_secs(2));
        assert!(!r.booting());
        now = Instant::ZERO + Duration::from_secs(10);
        let leaf = LeafId([7; 4]);
        r.leaves
            .attached(leaf, RxMode::Listening, Duration::ZERO, now)
            .unwrap();
        r.budget.record(r.budget.available(now), now);
        let q = Queued::new(MsgId(42), leaf, b"later", now).unwrap();
        r.delivery.push(q);
        r.feed_due = true;
        r.handle(Event::Wakeup, now, &mut |_| {});
        assert!(r.queue.is_empty(), "taken with a spent bucket");
        let deliver = Deliver {
            dst_leaf: leaf,
            msg_id: MsgId(42),
            payload: b"later",
        };
        let frame = mac::frame(&Packet::Deliver(deliver), r.home, MIN_PREAMBLE_SYMBOLS);
        let fits = r
            .budget
            .allowed_at(mac::airtime(&frame), REPLY_RESERVE, now)
            .unwrap();
        assert_eq!(r.next_wakeup(), Some(fits));
        let retry = r.delivery_retry_at;
        assert!(
            retry > fits + Duration::from_secs(10),
            "the retry is far off"
        );
        let sent = drive(
            &mut r,
            &mut now,
            Instant::from_micros(retry.as_micros() - 1),
        );
        let delivers: Vec<_> = sent
            .iter()
            .filter(|(_, f)| matches!(Packet::decode(f.payload()), Ok(Packet::Deliver(_))))
            .collect();
        assert_eq!(
            delivers.len(),
            3,
            "three attempts at a leaf that never answers"
        );
        let first = delivers[0].0;
        assert!(first >= fits, "sent before the bucket fit it");
        assert!(
            first.since(fits) < Duration::from_millis(1),
            "sent {:?} after the bucket fit it",
            first.since(fits)
        );
    }

    /// Two packets arrive within one reply time. The second finds the one
    /// owed-reply place holding the first one's ACK, which waits out a
    /// busy channel: it is neither taken nor refused, and gets no answer.
    /// Its sender's retry, after the first ACK is gone, is taken (roadmap
    /// D29 rule 4).
    #[test]
    fn a_packet_that_cannot_be_answered_is_not_taken() {
        use crate::event::AppEvent;
        use crate::ids::LeafId;
        use crate::position::ShortName;
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
            src_hop: ShortName(from),
            dst_pos: r.config.position,
            dst_leaf: LeafId::RELAY,
            hops: 1,
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

        // B's retry, after the retry wait (§9.3).
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
    /// can. A send offered anyway is ignored. A message for a leaf of this
    /// relay that finds the delivery queue full is held until it has a
    /// place.
    #[test]
    fn the_application_waits_for_room() {
        use crate::ids::LeafId;
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

        // Messages for a leaf of this relay that is not attached fill the
        // delivery queue.
        let me = r.config.position;
        let send = |r: &mut Relay, k: u8, now| {
            let cmd = AppCommand::Send {
                dst_pos: me,
                dst_leaf: LeafId([1; 4]),
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
        // The oldest expire after the delivery TTL; the held one takes a
        // place, and the application may send again.
        let ttl = r.config.delivery_ttl;
        let until = now + ttl + Duration::from_secs(1);
        drive(&mut r, &mut now, until);
        assert!(r.held.is_none());
        assert_eq!(r.holding().count(), 1, "the held one, now queued");
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
    /// two hops takes hop place 2, then 1, whatever the bucket holds; one
    /// from a leaf, or a third from two hops out, is refused. A place frees
    /// when its frame leaves (roadmap D31).
    #[test]
    fn a_packet_that_has_travelled_takes_a_hop_place() {
        use crate::event::DropReason;
        use crate::ids::LeafId;
        use crate::position::ShortName;
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
                src_hop: ShortName(1),
                dst_pos: far,
                dst_leaf: LeafId::RELAY,
                hops,
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
    /// queue keeps its record however old (protocol §9.2.2, roadmap D31).
    #[test]
    fn a_waiting_message_keeps_its_record() {
        use crate::ids::{LeafId, MsgId};
        use crate::position::ShortName;
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
                src_hop: ShortName(1),
                dst_pos: far,
                dst_leaf: LeafId::RELAY,
                hops,
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
        use crate::ids::{LeafId, MsgId};
        use crate::position::ShortName;
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
            surveyed: true,
            home: b_home,
            position: b,
        };
        hear(&mut r, now, &Packet::Heartbeat(hb));
        // Four forwards for B, queued in this order.
        let mut ids = Vec::new();
        for tag in 0..4u8 {
            let payload = [tag];
            let d = Data {
                sender_home: home,
                dst_hop: ShortName(0),
                src_hop: r.me(),
                dst_pos: far,
                dst_leaf: LeafId::RELAY,
                hops: 2,
                payload: &payload,
            };
            let parent = Some((ShortName(7), Channel::new(6).unwrap()));
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
                            from: b.short().into(),
                            to: r.me().into(),
                            refused: offers.len() <= refusals,
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
        use crate::ids::LeafId;
        use crate::position::ShortName;
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
            surveyed: true,
            home: Channel::new(5).unwrap(),
            position: d_pos,
        };
        hear(&mut r, now, &Packet::Heartbeat(hb));
        let data = Data {
            sender_home: home,
            dst_hop: ShortName(0),
            src_hop: r.me(),
            dst_pos: far,
            dst_leaf: LeafId::RELAY,
            hops: 2,
            payload: b"one message, two senders",
        };
        let id = data.msg_id();
        let parent = Some((ShortName(7), Channel::new(6).unwrap()));
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
            from: d_pos.short().into(),
            to: r.me().into(),
            refused: false,
        };
        let to_c = Ack {
            to: ShortName(0x0c0c_0c0c).into(),
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
    /// in the table past the 390 s a heartbeat alone keeps it (§8.2, §9.3).
    /// Without this, a relay whose only neighbour's heartbeats were lost
    /// for a few rotations dropped it while it was carrying every message.
    #[test]
    fn a_neighbour_that_answers_stays_live() {
        use crate::ids::LeafId;
        use crate::position::ShortName;
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
                surveyed: true,
                home: Channel::new(5).unwrap(),
                position: d_pos,
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
                dst_hop: ShortName(0),
                src_hop: r.me(),
                dst_pos: far,
                dst_leaf: LeafId::RELAY,
                hops: 2,
                payload: &payload,
            };
            let id = data.msg_id();
            let parent = Some((ShortName(7), Channel::new(6).unwrap()));
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
                from: d_pos.short().into(),
                to: r.me().into(),
                refused,
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
            let hb = Heartbeat {
                surveyed: true,
                home: Channel::new(5).unwrap(),
                position: Position {
                    lat: me.lat + 5_000 * k,
                    ..me
                },
            };
            r.neighbours.heard(&hb, &r.plane, now);
            r.neighbours.refused(hb.position, now + BAN);
        }
        let far = Position {
            lat: me.lat + 100_000,
            ..me
        };
        (r, now, far)
    }

    /// Offer `r` a full DATA to `dst` received with `hops`, from a leaf at
    /// 0, and send its reply. Returns the place it took and why it was
    /// refused, if it was.
    fn offer_data(
        r: &mut Relay,
        now: &mut Instant,
        dst: Position,
        tag: u8,
        hops: u8,
    ) -> (Option<u8>, Option<crate::event::DropReason>) {
        use crate::ids::LeafId;
        use crate::position::ShortName;
        use crate::wire::{Data, MAX_PAYLOAD};
        let payload = [tag; MAX_PAYLOAD];
        let d = Data {
            sender_home: Channel::new(5).unwrap(),
            dst_hop: r.me(),
            src_hop: ShortName(if hops == 0 { 0 } else { 1 }),
            dst_pos: dst,
            dst_leaf: LeafId::RELAY,
            hops,
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
        let until = *now + Duration::from_millis(100);
        drive_from(r, now, until, pending);
        let place = r
            .queue
            .iter()
            .find(|o| o.msg_id() == Some(d.msg_id()))
            .map(|o| o.place);
        (place, refused)
    }

    /// A leaf's message is refused while every neighbour that could take it
    /// on is banned, and taken once a ban has ended (roadmap D34): it waits
    /// at the leaf, not here.
    #[test]
    fn a_leaf_s_message_waits_for_a_way_on() {
        use crate::event::DropReason;
        let (mut r, start, far) = with_every_neighbour_banned();
        let mut now = start;
        assert_eq!(
            offer_data(&mut r, &mut now, far, 1, 0),
            (None, Some(DropReason::Banned))
        );
        now = start + BAN;
        assert_eq!(offer_data(&mut r, &mut now, far, 1, 0), (Some(0), None));
    }

    /// A packet already in the network is taken under the same bans, into
    /// the regular room and, with the bucket spent, a hop place: the hop
    /// places' argument against gridlock needs every such packet to move
    /// on (§9.3, roadmap D31, D34).
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
    /// one that succeeds raises the highest to its count (§9.2.6, roadmap
    /// D35). Stamped when queued, the frame would carry 2 throughout.
    #[test]
    fn a_hand_over_stamps_its_count_at_its_first_transmission() {
        use crate::ids::LeafId;
        use crate::position::ShortName;
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
            dst_hop: ShortName(0),
            src_hop: r.me(),
            dst_pos: far,
            dst_leaf: LeafId::RELAY,
            hops: 2,
            payload: b"count",
        };
        let id = data.msg_id();
        let parent = Some((ShortName(7), Channel::new(6).unwrap()));
        let at = r.record_place().unwrap();
        r.records.put(at, Record::new(id, parent, 1, now));
        let f = mac::frame(&Packet::Data(data), r.home, MIN_PREAMBLE_SYMBOLS);
        r.queue.push(Outgoing::new(f, Kind::Forward)).unwrap();
        // A copy with a higher count arrived since it was queued.
        r.records
            .get_mut(id)
            .unwrap()
            .received(ShortName(8), 6, now);

        let counts = |sent: &[(Instant, Frame)], to: Position| -> Vec<u8> {
            sent.iter()
                .filter_map(|(_, f)| match Packet::decode(f.payload()) {
                    Ok(Packet::Data(d)) if d.dst_hop == to.short() => Some(d.hops),
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
        r.records
            .get_mut(id)
            .unwrap()
            .received(ShortName(8), 9, now);
        while counts(&sent, near).is_empty() {
            let until = now + Duration::from_millis(10);
            sent.extend(drive(&mut r, &mut now, until));
            now = now.max(until);
            assert!(now < start + Duration::from_secs(10), "not readdressed");
        }
        assert_eq!(counts(&sent, near), [10], "readdressed: stamped again");
        let ack = Ack {
            msg_id: id,
            from: near.short().into(),
            to: r.me().into(),
            refused: false,
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
    /// taken; so is another copy while this relay holds one (§9.2.4 cases
    /// 3 and 4, roadmap D35). Here the relay holds the message, waiting
    /// for its banned neighbours.
    #[test]
    fn a_retransmission_or_a_copy_while_holding_is_only_acked() {
        use crate::ids::LeafId;
        use crate::position::ShortName;
        use crate::wire::Data;
        let (mut r, mut now, far) = with_every_neighbour_banned();
        let offer = |r: &mut Relay, now: &mut Instant, from: u32, hops: u8| {
            let d = Data {
                sender_home: Channel::new(5).unwrap(),
                dst_hop: r.me(),
                src_hop: ShortName(from),
                dst_pos: far,
                dst_leaf: LeafId::RELAY,
                hops,
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
    /// it comes from (§9.2.4 case 1, roadmap D35).
    #[test]
    fn a_copy_after_the_addressed_relay_took_it_is_only_acked() {
        use crate::ids::LeafId;
        use crate::position::ShortName;
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
            dst_hop: ShortName(0),
            src_hop: r.me(),
            dst_pos: addressed,
            dst_leaf: LeafId::RELAY,
            hops: 2,
            payload: b"arrived",
        };
        let id = data.msg_id();
        let parent = Some((ShortName(7), Channel::new(6).unwrap()));
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
            Ok(Packet::Data(d)) if d.dst_hop == addressed.short()
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
            from: addressed.short().into(),
            to: r.me().into(),
            refused: false,
        };
        hear(&mut r, &mut now, &Packet::Ack(ack));
        assert!(r.records.get(id).unwrap().arrived());
        assert!(r.queue.is_empty());
        let copy = Data {
            sender_home: Channel::new(5).unwrap(),
            dst_hop: r.me(),
            src_hop: ShortName(9),
            hops: 5,
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
