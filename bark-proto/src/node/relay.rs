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
//! replies (ACK or BUSY) this relay owes, the heartbeat, the hand-over at
//! the front of the send queue, and the sweep. The relay keeps at most one
//! excursion outstanding (`docs/core-interface.md` §3 rule 1). When the
//! radio is free it starts the first job that is ready, in that order: an
//! owed reply is worth nothing once its sender stops waiting, 100 ms after
//! the frame (§9.3). A job that has begun a sequence of excursions (CAD,
//! RSSI, transmit) continues it from each completion event, so sequences
//! never interleave; the hand-over alone lets an owed reply in before it
//! transmits, since its frame can take 139 ms on air. While a hand-over
//! waits for its ACK, only owed replies use the radio: the heartbeat and
//! the sweep would take the relay off its home channel, where the ACK
//! arrives (§9.3). Jobs that are not ready say when they will be, and the earliest
//! of those is `next_wakeup`.

mod forward;
mod service;

use crate::event::{Action, AppCommand, Event, Frame, Trace};
use crate::geo::LocalPlane;
use crate::ids::MsgId;
use crate::node::mac::{self, Radio};
use crate::node::transfer::{Replies, Step, Transfer, RELAY_REPLIES};
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    /// Not started: the driver's first `Wakeup` starts it.
    Off,
    /// Listening before choosing a home channel (design §6.2), until then.
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
    /// The hand-over of the front of `queue`.
    handover: Transfer,
    /// Neighbour slots the front forward passed over because
    /// listen-before-talk gave up on them (§9.3 step 2); not tried.
    skipped: u64,
    replies: Replies<RELAY_REPLIES>,
    leaves: Leaves,
    delivery: DeliveryQueue,
    delivered: Delivered,
    /// When queued deliveries are next retried (§10.4: every 60 s).
    delivery_retry_at: Instant,
    /// The send queue may have room for the delivery queue: a hand-over
    /// ended or the delivery queue changed.
    feed_due: bool,
    /// When the bucket will fit the DELIVER the send queue last had no
    /// room for, as a hand-over the bucket cannot fit yet is deferred.
    feed_at: Option<Instant>,
}

impl Relay {
    /// Create a relay. `seed` drives every random choice the relay makes;
    /// two relays with the same config and seed behave identically.
    pub fn new(config: Config, seed: u64) -> Relay {
        let mut rng = Rng::new(seed);
        let home = config
            .home_override
            .unwrap_or_else(|| random_channel(&mut rng));
        let rotation = random_channel(&mut rng);
        Relay {
            plane: LocalPlane::new(config.position),
            home,
            stage: Stage::Off,
            radio: Radio::new(),
            busy: BusyEstimator::from_config(&config),
            budget: Budget::new(),
            neighbours: Neighbours::new(NeighbourParams::from_config(&config)),
            sweep: Sweep {
                next_at: Instant::ZERO,
                channel: Channel::all().next().expect("the plan has channels"),
                last_home: Instant::ZERO,
                foreign: Duration::ZERO,
                pending: None,
            },
            beacon: Beacon {
                next_at: Instant::ZERO,
                ready_at: Instant::ZERO,
                rotation,
                announce: None,
                current: None,
                busy_checks: 0,
                check: Check::Idle,
            },
            mover: Mover::default(),
            records: Records::from_config(&config),
            queue: SendQueue::new(),
            handover: Transfer::new(),
            skipped: 0,
            replies: Replies::new(),
            leaves: Leaves::from_config(&config),
            delivery: DeliveryQueue::from_config(&config),
            delivered: Delivered::from_config(&config),
            delivery_retry_at: Instant::ZERO,
            feed_due: false,
            feed_at: None,
            config,
            rng,
        }
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
        self.leaves.expire(now);
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
        match Packet::decode(bytes) {
            Ok(Packet::Heartbeat(hb)) => self.heard(hb, now, emit),
            Ok(Packet::Ack(ack)) => self.ack_received(ack, now, emit),
            Ok(Packet::Busy(b)) => self.busy_received(b, now, emit),
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
            Ok(Packet::Busy(b)) => self.handover.refused_by(b),
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
                Some(Job::Reply) => self.replies.sent(),
                _ => {}
            },
            Event::App(cmd) => self.app(cmd, now, emit),
        }
        self.timers(now, emit);
        self.forward_timers(now, emit);
        self.service_timers(now, emit);
        self.pump(now, emit);
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
            }
            at(self.sweep.next_at);
        }
        w
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    #[test]
    fn sizes() {
        // docs/core-interface.md §6: under 50 KiB with every table; 50 344 B
        // with a 40-place send queue and a 41-place delivery queue.
        let relay = core::mem::size_of::<Relay>();
        let leaf = core::mem::size_of::<crate::node::Leaf>();
        std::println!("Relay {relay} B, Leaf {leaf} B");
        assert!(relay < 50 * 1024, "{relay}");
        assert!(leaf < 1024, "{leaf}");
    }

    /// Drive `r` from its own wakeups until `until`, completing every radio
    /// action at once on a quiet channel. Returns the frames it sent.
    fn drive(r: &mut Relay, now: &mut Instant, until: Instant) -> Vec<(Instant, Frame)> {
        let mut sent = Vec::new();
        let mut pending: Vec<Action> = Vec::new();
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
            ..Config::default()
        };
        let mut r = Relay::new(config, 1);
        let mut now = Instant::ZERO;
        drive(&mut r, &mut now, Instant::ZERO + Duration::from_secs(2));
        assert!(!r.booting());
        now = Instant::ZERO + Duration::from_secs(10);
        let leaf = LeafId([7; 4]);
        r.leaves.attached(leaf, RxMode::Listening, now).unwrap();
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
}
