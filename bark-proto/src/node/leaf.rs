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
//! The leaf listens on one channel and attaches to the first relay whose
//! heartbeat it hears there, moving to that relay's home channel. It keeps
//! the averaged RSSI of its relay and of one challenger, the strongest
//! other relay heard lately, and switches to the challenger once two of
//! its heartbeats in a row are [`SWITCH_MARGIN_DB`] stronger than its own
//! relay. The average is over the last two heartbeats, the same span the
//! switch rule looks at. It attaches to the new relay at once and leaves
//! the old one to time it out.
//!
//! The relay is lost, and the challenger taken if there is one, when its
//! heartbeats stop for as long as a relay's neighbour entry would last
//! (§8.2: three rotations), when it moves channel without the leaf hearing
//! the announcement and so stops answering, or when an ATTACH goes
//! unanswered. A relay that ACKs is alive however its heartbeats fare, so
//! an ACK counts as hearing it. A relay's announcement of a new home
//! channel (§6.3) moves the leaf with it, and the leaf attaches again.
//!
//! # Sending
//!
//! A message from the application goes to the relay as DATA with `hops`
//! and `src_hop` zero (§14), with the per-hop transfer of §9.3:
//! `Accepted`, then `Sent` at the relay's ACK, or `Failed` with
//! [`FailReason::NoAck`] after every attempt, or [`FailReason::NoRelay`]
//! if the leaf has no relay. The leaf holds one message at a time; a
//! second while it is busy fails with [`FailReason::QueueFull`]. A failed
//! hand-over makes the leaf attach again at once, so a dead relay is found
//! within one more hand-over.
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
use crate::tables::budget::LeafBudget;
use crate::tables::busy::NOMINAL_NOISE_FLOOR_DBM;
use crate::tables::neighbours::NeighbourParams;
use crate::time::{Duration, Instant};
use crate::wire::{Ack, AckFrom, Attach, Data, Deliver, Heartbeat, Packet, RxMode, MAX_PAYLOAD};

/// How much stronger another relay must be to take a leaf (§10.1: 6 dB).
pub const SWITCH_MARGIN_DB: i16 = 6;

/// Consecutive stronger heartbeats before a leaf switches (§10.1: 2).
pub const SWITCH_HEARTBEATS: u8 = 2;

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
    /// Average of the last two heartbeats' RSSI, dBm.
    rssi: i16,
    last_heard: Instant,
    /// Heartbeats in a row stronger than the leaf's relay by the margin.
    streak: u8,
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
    challenger: Option<Heard>,
    /// The relay has ACKed an ATTACH since the leaf chose it.
    attached: bool,
    /// When the next keepalive is due (§10.2).
    attach_at: Instant,
    transfer: Transfer,
    sending: Sending,
    /// The message being sent, as DATA; readdressed for each hand-over.
    outgoing: Option<Frame>,
    acks: Acks<1>,
    budget: LeafBudget,
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
            challenger: None,
            attached: false,
            attach_at: Instant::ZERO,
            transfer: Transfer::new(),
            sending: Sending::Attach,
            outgoing: None,
            acks: Acks::new(),
            budget: LeafBudget::new(),
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

    /// RSSI above which listen-before-talk finds the channel busy: the
    /// nominal noise floor plus the busy margin (§6.1). A leaf runs no
    /// sweep to calibrate its floor.
    fn busy_dbm(&self) -> i16 {
        NOMINAL_NOISE_FLOOR_DBM + self.config.busy_threshold_db as i16
    }

    // --- Relay choice (§10.1) ---

    fn heartbeat(&mut self, hb: Heartbeat, rssi: i16, now: Instant, emit: &mut dyn FnMut(Action)) {
        let heard = Heard {
            position: hb.position,
            home: hb.home,
            rssi,
            last_heard: now,
            streak: 0,
        };
        let Some(mut relay) = self.relay else {
            self.switch(heard, now, emit);
            return;
        };
        if relay.position == hb.position {
            relay.rssi = average(relay.rssi, rssi);
            relay.last_heard = now;
            let moved = relay.home != hb.home;
            relay.home = hb.home;
            self.relay = Some(relay);
            if moved {
                // The announcement of a move (§6.3): follow, attach again.
                self.switch(relay, now, emit);
            }
            return;
        }
        let stronger = rssi >= relay.rssi + SWITCH_MARGIN_DB;
        let challenger = match self.challenger {
            Some(c) if c.position == hb.position => Heard {
                rssi: average(c.rssi, rssi),
                streak: if stronger { c.streak + 1 } else { 0 },
                ..heard
            },
            Some(c) if now.since(c.last_heard) < self.silence() && c.rssi >= rssi => return,
            _ => Heard {
                streak: u8::from(stronger),
                ..heard
            },
        };
        if challenger.streak >= SWITCH_HEARTBEATS {
            self.switch(challenger, now, emit);
        } else {
            self.challenger = Some(challenger);
        }
    }

    /// Take `relay` and attach to it now.
    fn switch(&mut self, relay: Heard, now: Instant, emit: &mut dyn FnMut(Action)) {
        if self
            .challenger
            .is_some_and(|c| c.position == relay.position)
        {
            self.challenger = None;
        }
        self.relay = Some(Heard { streak: 0, ..relay });
        self.attached = false;
        self.attach_at = now;
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
        self.transfer.cancel();
        match self.challenger.take() {
            Some(c) if now.since(c.last_heard) < self.silence() => self.switch(c, now, emit),
            _ => {
                // Stay on this channel and wait for a heartbeat (§10.1).
                if let Some(id) = self.outgoing_id() {
                    self.outgoing = None;
                    emit(Action::App(AppEvent::Failed {
                        msg_id: id,
                        reason: FailReason::NoRelay,
                    }));
                }
            }
        }
    }

    // --- Sending (§9.3, §10.2) ---

    fn outgoing_id(&self) -> Option<MsgId> {
        match Packet::decode(self.outgoing.as_ref()?.payload()) {
            Ok(Packet::Data(d)) => Some(d.msg_id()),
            _ => None,
        }
    }

    fn app(&mut self, cmd: AppCommand<'_>, emit: &mut dyn FnMut(Action)) {
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
        if self.relay.is_none() {
            fail(FailReason::NoRelay, emit);
            return;
        }
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
    }

    /// Start a hand-over if none is running: a due ATTACH first, then the
    /// message.
    fn prepare(&mut self, now: Instant) {
        if !self.transfer.idle() {
            return;
        }
        let Some(relay) = self.relay else { return };
        let from = AckFrom::from(relay.position.short());
        if !self.attached || now >= self.attach_at {
            self.sending = Sending::Attach;
            self.transfer.start(
                Ack {
                    msg_id: MsgId(u32::from_be_bytes(self.id.0)),
                    from,
                },
                now,
            );
        } else if let Some(msg_id) = self.outgoing_id() {
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
                .next_change(airtime, now)
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
                        .next_change(airtime, now)
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

    fn send_failed(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        match self.sending {
            Sending::Attach => self.lose_relay(now, emit),
            Sending::Data => {
                if let Some(msg_id) = self.outgoing_id() {
                    emit(Action::App(AppEvent::Failed {
                        msg_id,
                        reason: FailReason::NoAck,
                    }));
                }
                self.outgoing = None;
                self.attached = false;
            }
        }
    }

    fn acked(&mut self, ack: Ack, now: Instant, emit: &mut dyn FnMut(Action)) {
        if !self.transfer.acked(ack) {
            return;
        }
        if let Some(r) = &mut self.relay {
            r.last_heard = now;
        }
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

/// The mean of two RSSI readings.
fn average(a: i16, b: i16) -> i16 {
    ((a as i32 + b as i32) / 2) as i16
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
            Event::App(cmd) => self.app(cmd, emit),
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
        if let Some(r) = self.relay {
            at(r.last_heard + self.silence());
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
