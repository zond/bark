//! The leaf (design §10): a carried node that attaches to one relay,
//! hands it the messages its application sends, and takes delivery of
//! the messages for it. It sends no heartbeats and forwards nothing.
//!
//! # Receive mode
//!
//! Only the always-listening mode of §10.5 is implemented: the leaf
//! listens on its relay's home channel all the time and declares
//! [`RxMode::Listening`] in its ATTACH. The duty-cycled and hybrid modes,
//! which sleep between CADs and need the relay's long preamble, are
//! declared on the wire (`wire.rs`) and served by the relay, but no leaf
//! runs them yet.
//!
//! # Relay choice (§10.1)
//!
//! The leaf keeps the relays it hears in a small table ([`CANDIDATES`]),
//! each with the mean RSSI of its last two heartbeats. It attaches to the
//! strongest once it has listened for a worst-case rotation
//! (13 × `heartbeat_max`, 195 s) from the first heartbeat it heard, by
//! which time every relay in range has visited its channel; a leaf with a
//! message to send does not wait, and takes the strongest heard so far.
//!
//! Attached, it moves to another relay only when that relay has been heard
//! over at least [`SWITCH_HEARTBEATS`] heartbeats and is stronger by
//! [`SWITCH_MARGIN_DB`]. It attaches to the new relay at once and leaves
//! the old one to time it out.
//!
//! # Keeping the relay (§10.2)
//!
//! A hand-over to the relay that fails, keepalive or message, is tried
//! again one keepalive period later, with an ATTACH first. The relay is
//! given up only after failures in two periods in a row, or when it has
//! not been heard for as long as a neighbour entry lasts (§8.2: three
//! rotations, 390 s), whichever comes first. One period is 60 s, as long
//! as the fading of a link lasts (correlation time 60 s in the scenarios),
//! so one fade costs a keepalive, not the relay. A relay that ACKs is
//! alive however its heartbeats fare, so an ACK counts as hearing it. A
//! relay's announcement of a new home channel (§6.3) moves the leaf with
//! it, and the leaf attaches again. A leaf that has lost its relay takes
//! the strongest other one it has heard, as above.
//!
//! # Sending
//!
//! A message from the application goes to the relay as DATA with `hops`
//! and `src_hop` zero (§14), with the per-hop transfer of §9.3:
//! `Accepted`, then `Sent` at the relay's ACK. The leaf holds one message
//! at a time; a second while it is busy fails with
//! [`FailReason::QueueFull`]. A message the leaf cannot hand over, because
//! it has no relay or its relay does not answer, is held and tried again
//! as above for up to the delivery TTL (§10.4: 10 minutes), the time the
//! network itself would keep it; then it fails with [`FailReason::NoRelay`]
//! or [`FailReason::NoAck`].
//!
//! # Receiving
//!
//! A DELIVER for this leaf is ACKed with the leaf's ID (§14) and surfaced
//! once. A relay re-sends the oldest message it holds for a leaf before
//! any other (`relay/service.rs`), so a repeat follows its first copy with
//! nothing from that relay in between; remembering the last [`SEEN`]
//! messages covers the relay and the one the leaf has just left.

use crate::event::{Action, AppCommand, AppEvent, Event, FailReason, Frame, MAX_FRAME};
use crate::ids::{LeafId, MsgId};
use crate::node::mac::{self, Radio};
use crate::node::transfer::{Acks, Check, Missed, Step, Transfer};
use crate::node::{Config, Core, Rng};
use crate::position::Position;
use crate::radio::{Channel, CHANNELS, MIN_PREAMBLE_SYMBOLS};
use crate::tables::budget::Budget;
use crate::tables::busy::NOMINAL_NOISE_FLOOR_DBM;
use crate::tables::neighbours::NeighbourParams;
use crate::time::{Duration, Instant};
use crate::wire::{Ack, AckFrom, Attach, Data, Deliver, Heartbeat, Packet, RxMode, MAX_PAYLOAD};

/// How much stronger, in its mean over two heartbeats, another relay must
/// be to take a leaf from its relay (§10.1).
///
/// A heartbeat's RSSI is the link's mean plus fading with σ = 4 dB in the
/// scenarios (`scenarios/s2-block.toml`, after typical indoor multipath).
/// A relay's heartbeats reach the leaf's channel once per rotation, ≈130 s
/// apart against a fading correlation time of 60 s, so they are close to
/// independent: a mean of two has σ/√2, and the difference of two such
/// means σ√(1/2 + 1/2) = σ = 4 dB. Two standard deviations, 8 dB, keeps
/// the leaf where it is on all but 2.3 % of comparisons with a relay that
/// is no better.
pub const SWITCH_MARGIN_DB: i16 = 8;

/// Heartbeats a relay must have been heard over before a leaf switches to
/// it (§10.1: 2); see [`SWITCH_MARGIN_DB`].
pub const SWITCH_HEARTBEATS: u8 = 2;

/// Relays a leaf keeps track of besides its own. An indoor leaf reaches
/// relays within ≈300 m (§4); at the design's indoor density of ≈9.6
/// relays/km² (§13) that is π × 0.3² × 9.6 ≈ 2.7 relays. Where there are
/// more, the weakest is forgotten first, so the table always holds the
/// strongest.
pub const CANDIDATES: usize = 4;

/// Consecutive keepalive periods with a failed hand-over after which a
/// leaf gives its relay up (§10.2).
pub const FAILED_PERIODS: u8 = 2;

/// DELIVERs a leaf remembers; see the module documentation.
pub const SEEN: usize = 2;

/// Who started the outstanding excursion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Job {
    Send,
    Ack,
}

/// What the hand-over carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sending {
    Attach,
    Data,
}

/// A relay the leaf has heard.
#[derive(Clone, Copy, Debug)]
struct Heard {
    position: Position,
    home: Channel,
    /// Mean RSSI of the last two heartbeats, dBm; the one heartbeat until
    /// there are two.
    rssi: i16,
    /// RSSI of the last heartbeat, dBm.
    last: i16,
    /// Heartbeats heard, up to [`SWITCH_HEARTBEATS`].
    heartbeats: u8,
    last_heard: Instant,
}

impl Heard {
    fn new(hb: &Heartbeat, rssi: i16, now: Instant) -> Heard {
        Heard {
            position: hb.position,
            home: hb.home,
            rssi,
            last: rssi,
            heartbeats: 1,
            last_heard: now,
        }
    }

    /// Another heartbeat from this relay.
    fn heard(&mut self, hb: &Heartbeat, rssi: i16, now: Instant) {
        self.rssi = (self.last as i32 + rssi as i32).div_euclid(2) as i16;
        self.last = rssi;
        self.heartbeats = (self.heartbeats + 1).min(SWITCH_HEARTBEATS);
        self.home = hb.home;
        self.last_heard = now;
    }
}

/// Leaf role. Small: the simulator holds twenty thousand of these.
pub struct Leaf {
    config: Config,
    rng: Rng,
    id: LeafId,
    started: bool,
    /// The channel the leaf listens on: its relay's home.
    home: Channel,
    radio: Radio<Job>,
    relay: Option<Heard>,
    /// Other relays heard, strongest kept.
    candidates: [Option<Heard>; CANDIDATES],
    /// When the leaf first heard a heartbeat since it last knew no relay at
    /// all: a rotation later it has heard every relay in range.
    listening_since: Option<Instant>,
    /// The relay has ACKed an ATTACH since the leaf chose it, and no
    /// hand-over to it has failed since.
    attached: bool,
    /// When the next ATTACH is due: a keepalive (§10.2), or the retry a
    /// period after a failure.
    attach_at: Instant,
    /// Keepalive periods in a row in which a hand-over to the relay failed.
    failed_periods: u8,
    transfer: Transfer,
    sending: Sending,
    /// The message being sent, as DATA; readdressed for each hand-over.
    outgoing: Option<Frame>,
    /// When the message is given up if it has not been handed over.
    outgoing_until: Instant,
    acks: Acks<1>,
    budget: Budget,
    seen: [MsgId; SEEN],
    seen_next: u8,
}

impl Leaf {
    /// Create a leaf; see [`crate::Relay::new`] for `seed`. Its ID is
    /// `config.leaf_id`, or one drawn from `seed` if that is zero.
    pub fn new(config: Config, seed: u64) -> Leaf {
        let mut rng = Rng::new(seed);
        let mut id = config.leaf_id;
        while id == LeafId::RELAY {
            id = LeafId((rng.next_u64() as u32).to_be_bytes());
        }
        let first = Channel::all().next().expect("the plan has channels");
        let random = Channel::new(rng.range(0, CHANNELS as u64 - 1) as u8).unwrap_or(first);
        Leaf {
            home: config.home_override.unwrap_or(random),
            config,
            rng,
            id,
            started: false,
            radio: Radio::new(),
            relay: None,
            candidates: [None; CANDIDATES],
            listening_since: None,
            attached: false,
            attach_at: Instant::ZERO,
            failed_periods: 0,
            transfer: Transfer::new(),
            sending: Sending::Attach,
            outgoing: None,
            outgoing_until: Instant::ZERO,
            acks: Acks::new(),
            budget: Budget::new(),
            seen: [MsgId(0); SEEN],
            seen_next: 0,
        }
    }

    /// Configuration.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// This leaf's ID.
    pub fn id(&self) -> LeafId {
        self.id
    }

    /// The relay this leaf is attached to: chosen, and its ATTACH
    /// acknowledged. Its position plus [`Leaf::id`] is the leaf's address
    /// (§9.1).
    pub fn relay(&self) -> Option<Position> {
        self.relay.filter(|_| self.attached).map(|r| r.position)
    }

    /// The channel the leaf listens on.
    pub fn home(&self) -> Channel {
        self.home
    }

    /// How long a relay may go unheard before the leaf gives it up: the
    /// life of a neighbour entry (§8.2).
    fn silence(&self) -> Duration {
        NeighbourParams::from_config(&self.config).expire_after
    }

    /// How long a leaf listens before it has heard every relay in range: a
    /// worst-case rotation, 13 × `heartbeat_max` (§7).
    fn rotation(&self) -> Duration {
        NeighbourParams::from_config(&self.config).stale_after
    }

    /// RSSI above which listen-before-talk finds the channel busy: the
    /// nominal noise floor plus the busy margin (§6.1). A leaf runs no
    /// sweep to calibrate its floor.
    fn busy_dbm(&self) -> i16 {
        NOMINAL_NOISE_FLOOR_DBM + self.config.busy_threshold_db as i16
    }

    // --- Relay choice (§10.1) ---

    fn heartbeat(&mut self, hb: Heartbeat, rssi: i16, now: Instant, emit: &mut dyn FnMut(Action)) {
        self.listening_since.get_or_insert(now);
        if let Some(mut relay) = self.relay.filter(|r| r.position == hb.position) {
            let moved = relay.home != hb.home;
            relay.heard(&hb, rssi, now);
            self.relay = Some(relay);
            if moved {
                // The announcement of a move (§6.3): follow, attach again.
                self.switch(relay, now, emit);
            }
        } else {
            self.remember(&hb, rssi, now);
        }
        self.choose(now, emit);
    }

    /// Note a heartbeat from a relay other than the leaf's own.
    fn remember(&mut self, hb: &Heartbeat, rssi: i16, now: Instant) {
        if let Some(c) = self
            .candidates
            .iter_mut()
            .flatten()
            .find(|c| c.position == hb.position)
        {
            c.heard(hb, rssi, now);
            return;
        }
        let silence = self.silence();
        let free = self
            .candidates
            .iter()
            .position(|c| c.is_none_or(|c| now.since(c.last_heard) >= silence));
        let slot = free.or_else(|| {
            let (i, weakest) = self
                .candidates
                .iter()
                .enumerate()
                .filter_map(|(i, c)| c.map(|c| (i, c)))
                .min_by_key(|(_, c)| c.rssi)?;
            (weakest.rssi < rssi).then_some(i)
        });
        if let Some(i) = slot {
            self.candidates[i] = Some(Heard::new(hb, rssi, now));
        }
    }

    /// The strongest candidate heard within the relay silence, and its slot.
    fn strongest(&self, now: Instant) -> Option<(usize, Heard)> {
        let silence = self.silence();
        self.candidates
            .iter()
            .enumerate()
            .filter_map(|(i, c)| c.map(|c| (i, c)))
            .filter(|(_, c)| now.since(c.last_heard) < silence)
            .max_by_key(|(_, c)| c.rssi)
    }

    /// Attach to a relay, or move to a better one, if the rules of §10.1
    /// say so now.
    fn choose(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        let Some((slot, best)) = self.strongest(now) else {
            if self.relay.is_none() {
                // Nothing heard lately: the next heartbeat starts a new
                // rotation of listening.
                self.listening_since = None;
            }
            return;
        };
        let take = match self.relay {
            None => {
                self.outgoing.is_some()
                    || self
                        .listening_since
                        .is_some_and(|t| now.since(t) >= self.rotation())
            }
            Some(r) => {
                best.heartbeats >= SWITCH_HEARTBEATS && best.rssi >= r.rssi + SWITCH_MARGIN_DB
            }
        };
        if take {
            self.candidates[slot] = self.relay.take();
            self.switch(best, now, emit);
        }
    }

    /// Take `relay` and attach to it now.
    fn switch(&mut self, relay: Heard, now: Instant, emit: &mut dyn FnMut(Action)) {
        self.relay = Some(relay);
        self.attached = false;
        self.attach_at = now;
        self.failed_periods = 0;
        // A hand-over in progress was for the old relay or channel.
        self.transfer.cancel();
        if relay.home != self.home {
            self.home = relay.home;
            emit(Action::Listen(relay.home));
        }
    }

    /// The relay stopped answering or was not heard for too long.
    fn lose_relay(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        self.relay = None;
        self.attached = false;
        self.failed_periods = 0;
        self.transfer.cancel();
        self.choose(now, emit);
    }

    // --- Sending (§9.3, §10.2) ---

    fn outgoing_id(&self) -> Option<MsgId> {
        match Packet::decode(self.outgoing.as_ref()?.payload()) {
            Ok(Packet::Data(d)) => Some(d.msg_id()),
            _ => None,
        }
    }

    fn app(&mut self, cmd: AppCommand<'_>, now: Instant, emit: &mut dyn FnMut(Action)) {
        let AppCommand::Send {
            dst_pos,
            dst_leaf,
            payload,
        } = cmd
        else {
            return;
        };
        let msg_id = crate::ids::MsgId::of(dst_pos, dst_leaf, payload);
        let fail = |reason, emit: &mut dyn FnMut(Action)| {
            emit(Action::App(AppEvent::Failed { msg_id, reason }))
        };
        if payload.len() > MAX_PAYLOAD {
            fail(FailReason::PayloadTooLong, emit);
            return;
        }
        emit(Action::App(AppEvent::Accepted(msg_id)));
        if self.outgoing.is_some() {
            fail(FailReason::QueueFull, emit);
            return;
        }
        // Addressed when its hand-over starts.
        let d = Data {
            sender_home: self.home,
            dst_hop: Default::default(),
            src_hop: Default::default(),
            dst_pos,
            dst_leaf,
            hops: 0,
            payload,
        };
        self.outgoing = Some(mac::frame(
            &Packet::Data(d),
            self.home,
            MIN_PREAMBLE_SYMBOLS,
        ));
        self.outgoing_until = now + self.config.delivery_ttl;
        if self.relay.is_none() {
            self.choose(now, emit);
        }
    }

    /// The message could not be handed over within the delivery TTL.
    fn give_up_message(&mut self, emit: &mut dyn FnMut(Action)) {
        let Some(msg_id) = self.outgoing_id() else {
            return;
        };
        if self.sending == Sending::Data {
            self.transfer.cancel();
        }
        self.outgoing = None;
        let reason = if self.relay.is_some() {
            FailReason::NoAck
        } else {
            FailReason::NoRelay
        };
        emit(Action::App(AppEvent::Failed { msg_id, reason }));
    }

    /// Start a hand-over if none is running: a due ATTACH first, then the
    /// message once the relay has ACKed one.
    fn prepare(&mut self, now: Instant) {
        if !self.transfer.idle() {
            return;
        }
        let Some(relay) = self.relay else { return };
        let from = AckFrom::from(relay.position.short());
        if now >= self.attach_at {
            self.sending = Sending::Attach;
            self.transfer.start(
                Ack {
                    msg_id: MsgId(u32::from_be_bytes(self.id.0)),
                    from,
                },
                now,
            );
        } else if let Some(msg_id) = self.outgoing_id().filter(|_| self.attached) {
            self.sending = Sending::Data;
            self.transfer.start(Ack { msg_id, from }, now);
        }
    }

    /// The frame of the hand-over, addressed to the relay as it is now.
    fn frame(&self) -> Option<Frame> {
        let relay = self.relay?.position.short();
        let packet = match self.sending {
            Sending::Attach => Packet::Attach(Attach {
                leaf: self.id,
                relay,
                mode: RxMode::Listening,
            }),
            Sending::Data => {
                let Ok(Packet::Data(d)) = Packet::decode(self.outgoing.as_ref()?.payload()) else {
                    return None;
                };
                let addressed = Data {
                    sender_home: self.home,
                    dst_hop: relay,
                    ..d
                };
                return Some(mac::frame(
                    &Packet::Data(addressed),
                    self.home,
                    MIN_PREAMBLE_SYMBOLS,
                ));
            }
        };
        Some(mac::frame(&packet, self.home, MIN_PREAMBLE_SYMBOLS))
    }

    fn start_send(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) -> bool {
        if !self.transfer.due(now) {
            return false;
        }
        let Some(frame) = self.frame() else {
            self.transfer.cancel();
            return false;
        };
        let airtime = mac::airtime(&frame);
        if !self.budget.would_allow(airtime, now) {
            let until = self
                .budget
                .allowed_at(airtime, Duration::ZERO, now)
                .unwrap_or(now + self.config.keepalive);
            self.transfer.defer(until);
            return false;
        }
        self.transfer.begin_lbt(now);
        self.radio.start(Job::Send, Action::Cad(self.home), emit);
        true
    }

    fn send_check(&mut self, busy: bool, now: Instant, emit: &mut dyn FnMut(Action)) {
        let step = self.transfer.step();
        if !matches!(step, Step::Cad | Step::Rssi) {
            return;
        }
        let backoff = self.config.hop_backoff_max;
        match self.transfer.checked(busy, now, &mut self.rng, backoff) {
            Check::Clear if step == Step::Cad => {
                self.radio.start(Job::Send, Action::Rssi(self.home), emit)
            }
            Check::Clear => {
                if self.acks.wake_at().is_some_and(|t| t <= now) {
                    self.transfer.hold(now);
                    return;
                }
                let Some(frame) = self.frame() else {
                    self.transfer.cancel();
                    return;
                };
                let airtime = mac::airtime(&frame);
                if !self.budget.would_allow(airtime, now) {
                    let until = self
                        .budget
                        .allowed_at(airtime, Duration::ZERO, now)
                        .unwrap_or(now + self.config.keepalive);
                    self.transfer.defer(until);
                    return;
                }
                self.budget.record(airtime, now);
                self.transfer.transmitting();
                self.radio.start(Job::Send, Action::Transmit(frame), emit);
            }
            Check::Busy => {}
            Check::GaveUp => {
                let (max, retry) = (self.config.hop_attempts, self.config.retry_backoff);
                if self.transfer.lost_attempt(now, max, &mut self.rng, retry) == Missed::Failed {
                    self.send_failed(now, emit);
                }
            }
        }
    }

    /// Every attempt of a hand-over to the relay went unanswered: try again
    /// a keepalive period later, or give the relay up after failures in
    /// [`FAILED_PERIODS`] periods in a row. A message stays held.
    fn send_failed(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        self.failed_periods = self.failed_periods.saturating_add(1);
        if self.failed_periods >= FAILED_PERIODS {
            self.lose_relay(now, emit);
        } else {
            self.attached = false;
            self.attach_at = now + self.config.keepalive;
        }
    }

    fn acked(&mut self, ack: Ack, now: Instant, emit: &mut dyn FnMut(Action)) {
        if !self.transfer.acked(ack) {
            return;
        }
        if let Some(r) = &mut self.relay {
            r.last_heard = now;
        }
        self.failed_periods = 0;
        match self.sending {
            Sending::Attach => {
                self.attached = true;
                self.attach_at = now + self.config.keepalive;
            }
            Sending::Data => {
                if let Some(msg_id) = self.outgoing_id() {
                    emit(Action::App(AppEvent::Sent(msg_id)));
                }
                self.outgoing = None;
            }
        }
    }

    // --- Receiving (§10.3) ---

    fn deliver(&mut self, d: Deliver<'_>, now: Instant, emit: &mut dyn FnMut(Action)) {
        let ack = Ack {
            msg_id: d.msg_id,
            from: AckFrom::from(self.id),
        };
        self.acks
            .owe(ack, self.home, now + self.config.ack_timeout, now);
        if self.seen.contains(&d.msg_id) {
            return;
        }
        self.seen[self.seen_next as usize] = d.msg_id;
        self.seen_next = ((self.seen_next as usize + 1) % SEEN) as u8;
        let mut payload = [0; MAX_FRAME];
        payload[..d.payload.len()].copy_from_slice(d.payload);
        emit(Action::App(AppEvent::Message {
            msg_id: d.msg_id,
            len: d.payload.len() as u8,
            payload,
        }));
    }

    fn start_ack(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) -> bool {
        let Some(o) = self.acks.due(now) else {
            return false;
        };
        let frame = mac::frame(&Packet::Ack(o.ack), o.channel, MIN_PREAMBLE_SYMBOLS);
        if !self.budget.would_allow(mac::airtime(&frame), now) {
            self.acks.abandon();
            return true;
        }
        match self.acks.begin() {
            Some(cad) => {
                self.radio.start(Job::Ack, cad, emit);
                true
            }
            None => false,
        }
    }

    fn ack_check(&mut self, busy: bool, now: Instant, emit: &mut dyn FnMut(Action)) {
        let backoff = self.config.hop_backoff_max;
        match self.acks.checked(busy, now, &mut self.rng, backoff) {
            Some((o, false)) => self.radio.start(Job::Ack, Action::Rssi(o.channel), emit),
            Some((o, true)) => {
                let frame = mac::frame(&Packet::Ack(o.ack), o.channel, MIN_PREAMBLE_SYMBOLS);
                let airtime = mac::airtime(&frame);
                if self.budget.would_allow(airtime, now) {
                    self.budget.record(airtime, now);
                    self.radio.start(Job::Ack, Action::Transmit(frame), emit);
                } else {
                    self.acks.abandon();
                }
            }
            None => {}
        }
    }

    fn received(&mut self, bytes: &[u8], rssi: i16, now: Instant, emit: &mut dyn FnMut(Action)) {
        match Packet::decode(bytes) {
            Ok(Packet::Heartbeat(hb)) => self.heartbeat(hb, rssi, now, emit),
            Ok(Packet::Ack(a)) => self.acked(a, now, emit),
            Ok(Packet::Deliver(d)) if d.dst_leaf == self.id => self.deliver(d, now, emit),
            _ => {}
        }
    }

    fn timers(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        let (max, retry) = (self.config.hop_attempts, self.config.retry_backoff);
        if self.transfer.timeout(now, max, &mut self.rng, retry) == Some(Missed::Failed) {
            self.send_failed(now, emit);
        }
        if self
            .relay
            .is_some_and(|r| now.since(r.last_heard) >= self.silence())
        {
            self.lose_relay(now, emit);
        }
        if self.relay.is_none() {
            // The end of the first rotation of listening.
            self.choose(now, emit);
        }
        if self.outgoing.is_some() && now >= self.outgoing_until {
            self.give_up_message(emit);
        }
    }

    fn pump(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        while self.radio.idle() {
            self.prepare(now);
            if !(self.start_ack(now, emit) || self.start_send(now, emit)) {
                break;
            }
        }
    }
}

impl Core for Leaf {
    fn handle(&mut self, event: Event<'_>, now: Instant, emit: &mut dyn FnMut(Action)) {
        if !self.started {
            self.started = true;
            emit(Action::Listen(self.home));
        }
        let busy_dbm = self.busy_dbm();
        match event {
            Event::Wakeup => {}
            Event::Received {
                rssi_dbm, bytes, ..
            } => self.received(bytes, rssi_dbm, now, emit),
            Event::CadDone { busy, .. } => match self.radio.complete() {
                Some(Job::Send) => self.send_check(busy, now, emit),
                Some(Job::Ack) => self.ack_check(busy, now, emit),
                None => {}
            },
            Event::RssiSample { dbm, .. } => match self.radio.complete() {
                Some(Job::Send) => self.send_check(dbm > busy_dbm, now, emit),
                Some(Job::Ack) => self.ack_check(dbm > busy_dbm, now, emit),
                None => {}
            },
            Event::TxDone => match self.radio.complete() {
                Some(Job::Send) if self.transfer.step() == Step::Tx => {
                    self.transfer.sent(now, self.config.ack_timeout)
                }
                Some(Job::Ack) => self.acks.sent(),
                _ => {}
            },
            Event::App(cmd) => self.app(cmd, now, emit),
        }
        self.timers(now, emit);
        self.pump(now, emit);
    }

    fn next_wakeup(&self) -> Option<Instant> {
        if !self.started {
            return Some(Instant::ZERO);
        }
        let mut w: Option<Instant> = None;
        let mut at = |t: Instant| w = Some(w.map_or(t, |w| w.min(t)));
        match self.relay {
            Some(r) => at(r.last_heard + self.silence()),
            None => {
                if let Some(t) = self.listening_since {
                    at(t + self.rotation());
                }
            }
        }
        if self.outgoing.is_some() {
            at(self.outgoing_until);
        }
        if self.transfer.awaiting_ack() || self.radio.idle() {
            if let Some(t) = self.transfer.wake_at() {
                at(t);
            }
        }
        if self.radio.idle() {
            if let Some(t) = self.acks.wake_at() {
                at(t);
            }
            if self.transfer.idle() && self.relay.is_some() {
                at(self.attach_at);
            }
        }
        w
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size() {
        // docs/core-interface.md §6: under 1 KiB.
        let leaf = core::mem::size_of::<Leaf>();
        assert!(leaf < 1024, "{leaf} B");
    }
}
