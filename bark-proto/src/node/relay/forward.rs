//! Forwarding (spec "Forwarding"): the relay's part of the depth-first search, the
//! owner rule, and the per-hop transfer of its send queue.
//!
//! # Receiving a DATA packet (spec "Receiving DATA")
//!
//! [`Relay::data`] classifies the packet by the five cases of spec "Receiving DATA" in
//! order and decides whether acting on it is work: sending it on, which
//! may be a forward or a return, or a delivery. Work to send on goes into the
//! send queue's regular room if it has a place and the bucket holds the
//! ACK, the airtime of everything queued with the new frame, and the reply
//! reserve (spec "Receiving DATA", roadmap D29); else into a free hop place numbered no
//! higher than the packet's count (roadmap D31); else it is refused with
//! an ACK that says so. A leaf's packet is refused too while every
//! neighbour is banned (roadmap D34); one already in the network is not,
//! since the hop places keep relays from locking up only if it moves on.
//! A new message also needs a place for its record,
//! which the size of the table guarantees (`tables/records.rs`). A
//! delivery is refused when
//! the delivery queue is full, or when the bucket holds less than the
//! reply reserve and an ACK (`tables/budget.rs`). Otherwise the ACK is
//! owed before acting. A packet that gives no work (cases 1, 3 and 4) is
//! ACKed from the reply reserve: the message has arrived, the packet was
//! acted on already, or a copy held here goes on, and a refusal would send
//! its sender on with a second copy. A relay that cannot afford even the
//! reply stays silent, and its sender retries.
//!
//! A relay owes one reply at a time (`transfer.rs`, roadmap D29). A packet
//! that arrives while it owes another one, its ACK still waiting for the
//! channel, is neither taken nor refused: the relay says nothing, and the
//! sender's retry brings the packet again. A copy of the packet the owed
//! reply answers is answered by it too.
//!
//! A packet is ACKed without being acted on only where that cannot end the
//! last copy (roadmap D35). A hand-over under way whose frame has been on
//! air does not count as a copy held here: its receiver may have taken it,
//! the ACK been lost, and the copy now offered be that one come back. A
//! message delivered here, or waiting in the delivery queue, is only
//! ACKed whatever its record says: the delivered window and the delivery
//! queue remember it, and its record may have made way for newer ones
//! (spec "Owner").
//!
//! # The send queue (spec "Hand-over")
//!
//! Every packet this relay sends for a message goes through the send
//! queue, one hand-over at a time ([`Transfer`]). A forward is addressed
//! when its hand-over starts, by the one next-hop function of spec "Next hop", and
//! readdressed by it after a failure; with no candidate left it becomes a
//! return to the parent, or at the origin a drop. The hop count a packet
//! carries is stamped at each hand-over's first transmission, the record's
//! highest count plus one, and repeated by its retries (spec "Hop count").
//!
//! Where a frame goes is read at the moment of sending (roadmap D29 rule
//! 7): before each attempt its channel is the receiver's home as the
//! neighbour table has it, from the latest heartbeat or packet of that
//! neighbour, and a DELIVER's is this relay's home, with the preamble of
//! the leaf's receive mode as the leaf table has it. A neighbour that
//! moves home (spec "Channel moves") between attempts, or while a message waits to come
//! back to it, is followed.
//!
//! A neighbour is tried, marked in the record, only once it has ACKed
//! the message (roadmap D36). When listen-before-talk gives up on a
//! forward's neighbour (spec "Hand-over" step 2), the neighbour refuses it, or its
//! three attempts go unanswered (step 6), it is set aside for this
//! hand-over only (`skipped`): choosing again at once must not pick it
//! again, but it stays a candidate for later choices. An unanswered frame
//! is more often lost than its ACK, so the neighbour most likely lacks the
//! message; marked tried, it would end its own search if it later got the
//! message elsewhere and offered it here. A return and a DELIVER have
//! nowhere else to go; for them a give-up counts as one of the three
//! attempts, and never as a failure against the neighbour.
//!
//! An ACK that refuses the current hand-over bans its receiver for
//! [`BAN`], so no message takes it until then (spec "Hand-over", roadmap D27, D29,
//! D32). A forward treats it as that give-up. A return has nowhere else
//! to go: it waits in its place for the ban to end, and the refused
//! transmission does not count as one of its attempts.
//!
//! A hand-over whose attempts all go unanswered bans its receiver too,
//! longer after each failure in a row (`tables/neighbours.rs`, roadmap
//! D42): silence says the channel or the receiver is full, and offering
//! it the next message at once only adds frames there. The failed
//! forward waits for the ban and goes to the same neighbour again, and
//! moves on only once a failure has demoted it; a return waits the same
//! way, and is dropped only then. Going to the next candidate at once
//! would send the message the long way round just when the channel is
//! full; measured, it undoes the ban's gain (roadmap D42).
//!
//! A forward whose every remaining candidate is banned waits in its place
//! until the earliest ban ends, and is routed again then (roadmap D31). It
//! is not returned or dropped: overload makes a message wait, never end.
//! Only a search exhausted by neighbours tried or set aside returns or
//! drops.
//! Waiting entries keep their airtime in admission, and the queue hands
//! over the first entry that is due. The application's own message waits
//! the same way, longer after refusals in a row ([`hold`]); one for an own
//! leaf that finds the delivery queue full is set aside ([`Held`]) until
//! the queue has a place (spec "Return to parent"). Either way the application is given no
//! other meanwhile.
//!
//! [`Transfer`]: crate::node::transfer::Transfer

use super::{Job, Relay};
use crate::event::{Action, AppEvent, DropReason, Frame, Trace, MAX_FRAME};
use crate::ids::{LeafId, MsgId};
use crate::node::mac;
use crate::node::transfer::{Check, Missed, Step};
use crate::position::{Position, ShortName};
use crate::radio::{time_on_air, Channel, MIN_PREAMBLE_SYMBOLS};
use crate::tables::budget::{hold, ACK_AIRTIME, BAN, HEARTBEAT_RESERVE, REPLY_RESERVE};
use crate::tables::delivery::{Pushed, Queued};
use crate::tables::neighbours::{Neighbour, Neighbours};
use crate::tables::records::{Record, HELD, MAX_HOPS};
use crate::tables::send_queue::{Kind, Outgoing, SEND_QUEUE};
use crate::time::{Duration, Instant};
use crate::wire::{Ack, AckName, Data, Packet, DATA_HEADER_LEN, MAX_PAYLOAD};

/// A relay within this distance of a destination that is not its own
/// position, with no live, undemoted neighbour closer, owns it
/// (spec "Owner"): 50 m is what spec "Position and time" allows between two GPS fixes of a relay that
/// has not moved, so the addressed relay and this one are, for the radio,
/// in the same place.
pub const OWNER_RADIUS_M: f32 = 50.0;

/// What a received DATA packet is (spec "Receiving DATA").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Case {
    /// Case 2: no record.
    New,
    /// Cases 1, 3 and 4: arrived, a retransmission, or a copy while
    /// this relay holds one. ACK only.
    AckOnly,
    /// Case 5: a copy, and this relay holds none. It goes on with its own
    /// search.
    Search,
}

/// What became of a forward when it was addressed. Short-lived, so the
/// frame it carries by value costs nothing.
#[allow(clippy::large_enum_variant)]
enum Routed {
    /// Addressed: send this frame, expecting this ACK.
    To(Outgoing, Ack),
    /// Dropped or delivered here; it leaves the queue.
    Done,
    /// The application's message, for a leaf of this relay, waits set
    /// aside for a place in the delivery queue.
    Hold,
    /// Every candidate left is banned: it waits in its place until then.
    Wait(Instant),
}

/// The application's message for a leaf of this relay, set aside while
/// the delivery queue is full (roadmap D29). It keeps its place and
/// airtime in the send queue's admission.
#[derive(Clone, Copy, Debug)]
pub(super) struct Held {
    pub(super) out: Outgoing,
}

fn data_airtime(payload: usize) -> Duration {
    time_on_air((DATA_HEADER_LEN + payload) as u8, MIN_PREAMBLE_SYMBOLS)
}

/// `d` as a frame on `channel`.
fn data_frame(d: &Data<'_>, channel: Channel) -> Frame {
    mac::frame(&Packet::Data(*d), channel, MIN_PREAMBLE_SYMBOLS)
}

impl Relay {
    /// This relay's short name (spec "Relay identity").
    pub(super) fn me(&self) -> ShortName {
        self.config.position.short()
    }

    /// The ACK by which `from` takes this relay's offer of `msg_id`.
    fn expect(&self, msg_id: MsgId, from: AckName) -> Ack {
        Ack {
            msg_id,
            from,
            to: self.me().into(),
            refused: false,
        }
    }

    /// Whether forwarding, or an ACK that takes on work, may spend
    /// `airtime` now: it must leave the reply reserve (spec "Duty cycle cap").
    pub(super) fn affordable(&self, airtime: Duration, now: Instant) -> bool {
        self.budget.would_allow_keeping(airtime, REPLY_RESERVE, now)
    }

    /// The place in the send queue for a frame of `airtime`, received with
    /// `hops`, after this relay spends `spend` (the ACK that takes the
    /// packet); 0 is the regular room. The regular room needs a place, and
    /// everything queued, the frame included, must fit in the bucket now
    /// with the reply reserve left (spec "Receiving DATA", "Hand-over"). Failing that, the
    /// highest free hop place not above `hops` takes it, whatever the
    /// bucket holds (roadmap D31). `freed`: the current hand-over ends
    /// first, giving up its place.
    pub(super) fn admit(
        &self,
        airtime: Duration,
        spend: Duration,
        hops: u8,
        freed: bool,
        now: Instant,
    ) -> Result<u8, DropReason> {
        let current = self.queue.current().filter(|_| freed);
        let current_air = current.map_or(Duration::ZERO, |o| mac::airtime(&o.frame));
        let queued = self.queue.airtime() - current_air + self.held_airtime();
        let regular = self.regular() - usize::from(current.is_some_and(|o| o.place == 0));
        let why = if regular >= SEND_QUEUE {
            DropReason::SendQueueFull
        } else if self.budget.available(now) < queued + airtime + spend + REPLY_RESERVE {
            DropReason::Budget
        } else {
            return Ok(0);
        };
        let freed_place = current.map(|o| o.place).filter(|&k| k > 0);
        self.queue.free_place(hops, freed_place).ok_or(why)
    }

    /// Why the regular room cannot take a frame of `airtime`, if it cannot;
    /// for what enters the network here and has no hop place.
    pub(super) fn no_room(
        &self,
        airtime: Duration,
        spend: Duration,
        now: Instant,
    ) -> Option<DropReason> {
        self.admit(airtime, spend, 0, false, now).err()
    }

    /// Entries in the regular room, the held message counting as one.
    fn regular(&self) -> usize {
        self.queue.regular() + usize::from(self.held.is_some())
    }

    fn held_airtime(&self) -> Duration {
        self.held
            .as_ref()
            .map_or(Duration::ZERO, |h| mac::airtime(&h.out.frame))
    }

    /// Whether a reply may spend `airtime` now: it must leave the
    /// heartbeat reserve (spec "Duty cycle cap").
    pub(super) fn can_reply(&self, airtime: Duration, now: Instant) -> bool {
        self.budget
            .would_allow_keeping(airtime, HEARTBEAT_RESERVE, now)
    }

    fn drop_msg(&mut self, msg_id: MsgId, reason: DropReason, emit: &mut dyn FnMut(Action)) {
        if self.own == Some(msg_id) {
            self.set_own(None);
        }
        emit(Action::Trace(Trace::Dropped { msg_id, reason }));
    }

    // --- Owner and next hop (spec "Next hop", "Owner") ---

    /// Whether this relay owns the destination `dst` (spec "Owner").
    fn owns(&self, dst: Position, now: Instant) -> bool {
        if dst == self.config.position {
            return true;
        }
        let target = self.plane.point(dst);
        let mine = target.distance2(Default::default());
        mine <= OWNER_RADIUS_M * OWNER_RADIUS_M
            && !self
                .neighbours
                .candidates(now)
                .any(|(_, n)| n.local.distance2(target) < mine)
    }

    /// The next hop for the message of `record` to `dst` (spec "Next hop"): the
    /// live neighbour nearest `dst` that is not in the tried set, not set
    /// aside and not banned, ties broken by short name; a demoted one only
    /// when no other with margin is left, and one without margin only when
    /// no other at all (roadmap D25, D29, D38).
    fn next_hop(&self, record: &Record, dst: Position, now: Instant) -> Option<(usize, Neighbour)> {
        let target = self.plane.point(dst);
        let parent = record.parent().map(|(name, _)| name);
        let skipped = self.skipped;
        // Computed once: it takes a logarithm.
        let margin = Neighbours::margin_dbm(self.busy.noise_floor_dbm());
        let faint = |n: &Neighbour| n.rssi < margin;
        let demoted = |n: &Neighbour| self.neighbours.is_demoted(n, now);
        self.neighbours
            .live(now)
            .filter(|(slot, n)| {
                !record.handed(*slot)
                    && skipped & (1 << slot) == 0
                    && Some(n.position.short()) != parent
                    && !self.neighbours.is_banned(n, now)
            })
            .min_by(|(_, a), (_, b)| {
                let (da, db) = (a.local.distance2(target), b.local.distance2(target));
                faint(a)
                    .cmp(&faint(b))
                    .then(demoted(a).cmp(&demoted(b)))
                    .then(da.total_cmp(&db))
                    .then(a.position.short().0.cmp(&b.position.short().0))
            })
            .map(|(slot, n)| (slot, *n))
    }

    // --- Receiving (spec "Receiving DATA") ---

    /// A DATA packet addressed to this relay.
    pub(super) fn data(&mut self, d: Data<'_>, now: Instant, emit: &mut dyn FnMut(Action)) {
        let id = d.msg_id();
        let from_leaf = d.hops == 0;
        let repeat = self
            .records
            .get(id)
            .is_some_and(|r| r.repeats(d.src_hop, d.hops));
        // A packet from the relay this one is handing the message to shows
        // that it took it, and ends that hand-over, which frees its place.
        let completes = !repeat && self.handing(id, d.src_hop);
        let case = self.classify(id, repeat);
        let owner = case != Case::AckOnly && self.owns(d.dst_pos, now);
        let sends = case != Case::AckOnly && !owner;
        let deadline = now + self.config.ack_timeout;
        let for_leaf = owner && d.dst_leaf != LeafId::RELAY;
        let record_at = match case {
            Case::New => self.record_place(),
            _ => None,
        };
        let mut place = 0;
        let refusal = if case == Case::AckOnly {
            None
        } else if case == Case::New && record_at.is_none() {
            Some(DropReason::NoRecord)
        } else if sends {
            let air = data_airtime(d.payload.len());
            match self.admit(air, ACK_AIRTIME, d.hops, completes, now) {
                Ok(_) if from_leaf && self.bans_end(now).is_some() => Some(DropReason::Banned),
                Ok(k) => {
                    place = k;
                    None
                }
                Err(why) => Some(why),
            }
        } else if for_leaf && self.delivery.is_full() {
            Some(DropReason::DeliveryFull)
        } else if !self.affordable(ACK_AIRTIME, now) {
            Some(DropReason::Budget)
        } else {
            None
        };
        let reply = Ack {
            msg_id: id,
            from: self.me().into(),
            to: d.src_hop.into(),
            refused: refusal.is_some(),
        };
        if !self.replies.can_owe(reply, d.sender_home, now) {
            // The packet could not be answered; its sender's retry
            // brings it again (spec "Hand-over", roadmap D29).
            return;
        }
        if let Some(reason) = refusal {
            emit(Action::Trace(Trace::Refused { msg_id: id, reason }));
            if self.can_reply(ACK_AIRTIME, now) {
                self.replies.owe(reply, d.sender_home, deadline, now);
            }
            return;
        }
        if !self.can_reply(ACK_AIRTIME, now) {
            emit(Action::Trace(Trace::Refused {
                msg_id: id,
                reason: DropReason::Budget,
            }));
            return;
        }
        self.replies.owe(reply, d.sender_home, deadline, now);
        if completes {
            self.handover_done(now, emit);
        }
        if case == Case::New {
            let parent = (!from_leaf).then_some((d.src_hop, d.sender_home));
            let mut record = Record::new(id, parent, d.hops, now);
            if owner {
                record.set_arrived();
            }
            if let Some(at) = record_at {
                self.records.put(at, record);
            }
        } else if let Some(r) = self.records.get_mut(id) {
            r.received(d.src_hop, d.hops, now);
            if owner {
                r.set_arrived();
            }
        }
        match case {
            Case::AckOnly => {
                self.delivered.offered(id, now);
            }
            Case::New | Case::Search if owner => {
                self.arrive(id, d.dst_pos, d.dst_leaf, d.payload, now, emit);
            }
            Case::New | Case::Search => self.send_on(&d, place, emit),
        }
    }

    /// The case of spec "Receiving DATA" for a DATA packet of message `id`; `repeat`:
    /// from the sender and with the count of the last one received.
    fn classify(&self, id: MsgId, repeat: bool) -> Case {
        if self.delivered.contains(id) || self.delivery.iter().any(|q| q.msg_id == id) {
            return Case::AckOnly;
        }
        let Some(r) = self.records.get(id) else {
            return Case::New;
        };
        if r.arrived() || repeat || self.holds(id) {
            return Case::AckOnly;
        }
        Case::Search
    }

    /// Whether this relay holds a copy of message `id` that is sure to live
    /// on: in its send queue or set aside, but not the frame of a hand-over
    /// under way, once it has been on air. Its receiver may have taken it
    /// and only the ACK been lost, and the copy now offered here may be
    /// that one, come back.
    fn holds(&self, id: MsgId) -> bool {
        let sent = !self.handover.idle() && self.handover.transmitted();
        let current = self.queue.current_index().filter(|_| sent);
        self.queue
            .iter()
            .enumerate()
            .any(|(i, o)| Some(i) != current && o.msg_id() == Some(id))
            || self.held.and_then(|h| h.out.msg_id()) == Some(id)
    }

    /// Whether the current hand-over is of message `id` to `to`, sent at
    /// least once.
    fn handing(&self, id: MsgId, to: ShortName) -> bool {
        !self.handover.idle()
            && self.handover.transmitted()
            && self.handover.expect() == self.expect(id, to.into())
    }

    /// Where a new record goes: never in place of the record of a message
    /// this relay holds, in its send queue or set aside, which it needs to
    /// route that message (`tables/records.rs`). `None` only if every
    /// record is such, which the table's size rules out.
    pub(super) fn record_place(&self) -> Option<usize> {
        let mut held = [MsgId(0); HELD + 1];
        let mut n = 0;
        if self.records.is_full() {
            let aside = self.held.and_then(|h| h.out.msg_id());
            for id in self.queue.iter().filter_map(Outgoing::msg_id).chain(aside) {
                held[n] = id;
                n += 1;
            }
            // Admission keeps the held message within the send queue's places.
            debug_assert!(n <= HELD, "{n} messages held");
            held[..n].sort_unstable_by_key(|id| id.0);
        }
        let held = &held[..n];
        self.records
            .place(|id| held.binary_search_by_key(&id.0, |h| h.0).is_ok())
    }

    /// Queue `d` onward into `place`, a forward addressed when its
    /// hand-over starts. Carries the received count plus one, which places
    /// it among waiting entries, until its hand-over stamps the count it is
    /// sent with (spec "Hop count").
    fn send_on(&mut self, d: &Data<'_>, place: u8, emit: &mut dyn FnMut(Action)) {
        let id = d.msg_id();
        if d.hops == MAX_HOPS {
            self.drop_msg(id, DropReason::HopCap, emit);
            return;
        }
        let out = Data {
            sender_home: self.home,
            dst_hop: ShortName(0),
            src_hop: self.me(),
            hops: d.hops + 1,
            ..*d
        };
        let out = Outgoing {
            place,
            ..Outgoing::new(data_frame(&out, self.home), Kind::Forward)
        };
        if self.queue.push(out).is_err() {
            self.drop_msg(id, DropReason::SendQueueFull, emit);
        }
    }

    /// The message has reached its owner, this relay (spec "Owner"): deliver it
    /// to the application or queue it for the leaf.
    fn arrive(
        &mut self,
        id: MsgId,
        dst: Position,
        leaf: LeafId,
        payload: &[u8],
        now: Instant,
        emit: &mut dyn FnMut(Action),
    ) {
        if self.own == Some(id) {
            self.set_own(None);
        }
        if leaf == LeafId::RELAY {
            if dst != self.config.position {
                self.drop_msg(id, DropReason::RelayGone, emit);
                return;
            }
            self.delivered.insert(id, now);
            let mut buf = [0; MAX_FRAME];
            buf[..payload.len()].copy_from_slice(payload);
            emit(Action::App(AppEvent::Message {
                msg_id: id,
                len: payload.len() as u8,
                payload: buf,
            }));
            return;
        }
        let Some(q) = Queued::new(id, leaf, payload, now) else {
            self.drop_msg(id, DropReason::PayloadTooLong, emit);
            return;
        };
        match self.delivery.push(q) {
            Pushed::Duplicate => return,
            Pushed::Full => {
                // Only when ownership is found at the hand-over, too late
                // to refuse (spec "Receiving DATA" case 5).
                self.drop_msg(id, DropReason::DeliveryFull, emit);
                return;
            }
            Pushed::Queued => self.feed_due = true,
        }
        emit(Action::Trace(Trace::Queued { msg_id: id, leaf }));
        self.deliver_now(leaf, now);
    }

    // --- The relay's own application (spec "Routing") ---

    /// Whether the application may send now (roadmap D29 rule 6, D34): the
    /// relay is running, holds no message of the application's, its send
    /// queue takes a full frame on top of what it holds, as a leaf's would
    /// be taken but without the ACK, and not every neighbour is banned.
    pub(super) fn accepts_send(&self, now: Instant) -> bool {
        !self.booting()
            && self.own.is_none()
            && self
                .no_room(data_airtime(MAX_PAYLOAD), Duration::ZERO, now)
                .is_none()
            && self.bans_end(now).is_none()
    }

    /// When the application may send again, if only the bucket and bans
    /// keep it waiting; anything else ends with an event.
    pub(super) fn send_ready_at(&self, now: Instant) -> Option<Instant> {
        if self.booting() || self.own.is_some() || self.regular() >= SEND_QUEUE {
            return None;
        }
        let need =
            self.queue.airtime() + self.held_airtime() + data_airtime(MAX_PAYLOAD) + REPLY_RESERVE;
        let bucket = self
            .budget
            .allowed_at(Duration::ZERO, need, now)
            .filter(|&t| t > now);
        match (bucket, self.bans_end(now)) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        }
    }

    /// The application's message is now `own`; a new one, or none, has
    /// been refused no time yet.
    fn set_own(&mut self, own: Option<MsgId>) {
        self.own = own;
        self.own_refusals = 0;
    }

    /// Deliver the held message once the delivery queue has a place.
    pub(super) fn release_held(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        let Some(h) = self.held else { return };
        if self.delivery.is_full() {
            return;
        }
        self.held = None;
        if let Ok(Packet::Data(d)) = Packet::decode(h.out.frame.payload()) {
            self.arrive(d.msg_id(), d.dst_pos, d.dst_leaf, d.payload, now, emit);
        }
    }

    /// `AppCommand::Send`: this relay is the origin. Offered only when
    /// [`Relay::accepts_send`]; anything else is the driver's bug and is
    /// ignored. The message is the application's until its first hop
    /// takes it (`own`).
    ///
    /// The application is the first sender and sends count 0 (spec "Hop count"), as
    /// a leaf does; this relay then sends 1, what it received plus one. It
    /// cannot put 0 on the air itself: a relay receiving count 0 takes the
    /// sender for a leaf and itself for the origin (spec "Packet sender"). A message the
    /// relay already has a record of is in the network already, as is one
    /// delivered or queued here (spec "Owner").
    pub(super) fn originate(
        &mut self,
        dst_pos: Position,
        dst_leaf: LeafId,
        payload: &[u8],
        now: Instant,
        emit: &mut dyn FnMut(Action),
    ) {
        if payload.len() > MAX_PAYLOAD || !self.accepts_send(now) {
            return;
        }
        let id = MsgId::of(dst_pos, dst_leaf, payload);
        if self.records.get(id).is_some() || self.delivered.contains(id) {
            return;
        }
        // The table outnumbers what a relay holds (`tables/records.rs`).
        let Some(at) = self.record_place() else {
            return;
        };
        self.set_own(Some(id));
        let d = Data {
            sender_home: self.home,
            dst_hop: ShortName(0),
            src_hop: self.me(),
            dst_pos,
            dst_leaf,
            hops: 0,
            payload,
        };
        if self.owns(dst_pos, now) {
            let mut record = Record::new(id, None, 0, now);
            record.set_arrived();
            self.records.put(at, record);
            if dst_leaf != LeafId::RELAY && self.delivery.is_full() {
                let out = Outgoing::new(data_frame(&d, self.home), Kind::Forward);
                self.held = Some(Held { out });
            } else {
                self.arrive(id, dst_pos, dst_leaf, payload, now, emit);
            }
            return;
        }
        self.records.put(at, Record::new(id, None, 0, now));
        self.send_on(&d, 0, emit);
    }

    // --- The hand-over (spec "Hand-over") ---

    /// Make sure the send queue's next due entry has a hand-over: address
    /// a forward, or drop what cannot be sent. The entry stays where it is
    /// in the queue.
    pub(super) fn prepare(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        while self.handover.idle() {
            let Some(i) = self.queue.next_due(now) else {
                return;
            };
            if self.queue.current_index() != Some(i) {
                // What was set aside belongs to the entry chosen before.
                self.queue.select(i);
                self.skipped = 0;
            }
            let Some(out) = self.queue.current().copied() else {
                return;
            };
            let routed = match out.kind {
                Kind::Forward => self.route(out, now, emit),
                Kind::Return => match Packet::decode(out.frame.payload()) {
                    Ok(Packet::Data(d)) => {
                        Routed::To(out, self.expect(d.msg_id(), d.dst_hop.into()))
                    }
                    _ => Routed::Done,
                },
                Kind::Deliver => match Packet::decode(out.frame.payload()) {
                    Ok(Packet::Deliver(d)) => {
                        Routed::To(out, self.expect(d.msg_id, d.dst_leaf.into()))
                    }
                    _ => Routed::Done,
                },
            };
            match routed {
                Routed::To(out, expect) => {
                    if let Some(current) = self.queue.current_mut() {
                        *current = out;
                    }
                    self.handover.start(expect, out.tries, now);
                }
                Routed::Done => {
                    self.queue.take_current();
                    self.skipped = 0;
                    self.feed_due = true;
                }
                Routed::Hold => {
                    if let Some(out) = self.queue.take_current() {
                        self.held = Some(Held { out });
                    }
                    self.skipped = 0;
                    self.feed_due = true;
                }
                Routed::Wait(t) => {
                    if let Some(current) = self.queue.current_mut() {
                        current.not_before = t;
                    }
                    self.skipped = 0;
                }
            }
        }
    }

    /// Address the forward `out` (spec "Next hop"), or let it wait for a ban to
    /// end, or return it to the parent (spec "Return to parent"), or drop it at the origin.
    /// Ownership is checked again first: a closer neighbour may have been
    /// demoted since (spec "Receiving DATA" case 5).
    fn route(&mut self, out: Outgoing, now: Instant, emit: &mut dyn FnMut(Action)) -> Routed {
        let Ok(Packet::Data(d)) = Packet::decode(out.frame.payload()) else {
            return Routed::Done;
        };
        let id = d.msg_id();
        debug_assert!(
            self.records.get(id).is_some(),
            "a held message keeps its record"
        );
        let Some(record) = self.records.get(id).copied() else {
            self.drop_msg(id, DropReason::NoRecord, emit);
            return Routed::Done;
        };
        if self.owns(d.dst_pos, now) {
            if let Some(r) = self.records.get_mut(id) {
                r.set_arrived();
            }
            if self.own == Some(id) && d.dst_leaf != LeafId::RELAY && self.delivery.is_full() {
                return Routed::Hold;
            }
            self.arrive(id, d.dst_pos, d.dst_leaf, d.payload, now, emit);
            return Routed::Done;
        }
        if let Some((_, n)) = self.next_hop(&record, d.dst_pos, now) {
            let to = n.position.short();
            let frame = data_frame(&Data { dst_hop: to, ..d }, n.home);
            return Routed::To(Outgoing { frame, ..out }, self.expect(id, to.into()));
        }
        if let Some(t) = self.ban_ends(&record, now) {
            if self.own == Some(id) {
                // Each offer costs the refuser a reply and hides its ACKs.
                self.own_refusals = self.own_refusals.saturating_add(1);
                return Routed::Wait(t.max(now + hold(self.own_refusals)));
            }
            return Routed::Wait(t);
        }
        match record.parent() {
            Some((parent, stored)) => {
                let home = self.home_of(parent).unwrap_or(stored);
                let frame = data_frame(
                    &Data {
                        dst_hop: parent,
                        ..d
                    },
                    home,
                );
                Routed::To(
                    Outgoing {
                        frame,
                        kind: Kind::Return,
                        ..out
                    },
                    self.expect(id, parent.into()),
                )
            }
            None => {
                self.drop_msg(id, DropReason::NoRoute, emit);
                Routed::Done
            }
        }
    }

    /// When fresh work could next start moving, if every live neighbour is
    /// banned now: the earliest end of those bans. A message entering here
    /// has no tried set, so these are all the candidates spec "Next hop" would have
    /// for it, demoted ones and those without margin included as its last
    /// resort. Fresh work waits
    /// at its origin meanwhile, where its application sees it, rather than
    /// here for a ban, to be refused again (roadmap D34). With no live
    /// neighbour at all it is no ban: the search ends as before.
    fn bans_end(&self, now: Instant) -> Option<Instant> {
        let mut until: Option<Instant> = None;
        for (_, n) in self.neighbours.live(now) {
            if !self.neighbours.is_banned(n, now) {
                return None;
            }
            until = Some(until.map_or(n.banned_until, |t| t.min(n.banned_until)));
        }
        until
    }

    /// When the earliest ban ends among the neighbours that would be
    /// candidates for the message of `record` but for their ban; `None`
    /// if the search was exhausted by tried and failed ones alone.
    /// A refusal also sets its sender aside for this hand-over, so being set
    /// aside does not count against a banned neighbour.
    fn ban_ends(&self, record: &Record, now: Instant) -> Option<Instant> {
        self.neighbours
            .live(now)
            .filter(|(slot, n)| !record.handed(*slot) && self.neighbours.is_banned(n, now))
            .map(|(_, n)| n.banned_until)
            .min()
    }

    /// The home channel of the neighbour named `name`, as its latest
    /// heartbeat or packet gave it.
    fn home_of(&self, name: ShortName) -> Option<Channel> {
        self.neighbours.by_short(name).map(|(_, n)| n.home)
    }

    /// Address the current frame as its receiver is now, before each attempt
    /// (spec "Hand-over"): a neighbour that moved home since the frame was queued, or
    /// since the last attempt, listens elsewhere, and a leaf that changed
    /// its receive mode needs another preamble. A receiver no longer in its
    /// table keeps the address it had.
    fn readdress(&mut self) -> Option<Outgoing> {
        let mut out = self.queue.current().copied()?;
        let (channel, preamble) = match Packet::decode(out.frame.payload()) {
            Ok(Packet::Data(d)) => (self.home_of(d.dst_hop), None),
            // The leaf listens on this relay's home, wherever that is now.
            Ok(Packet::Deliver(d)) => (
                Some(self.home),
                self.leaves
                    .get(d.dst_leaf)
                    .map(|l| l.mode.preamble_symbols()),
            ),
            _ => (None, None),
        };
        out.frame.channel = channel.unwrap_or(out.frame.channel);
        out.frame.preamble_symbols = preamble.unwrap_or(out.frame.preamble_symbols);
        if let Some(current) = self.queue.current_mut() {
            *current = out;
        }
        Some(out)
    }

    /// Start listen-before-talk for the current hand-over if it is due and
    /// the budget lets it (spec "Duty cycle cap": deferred, never dropped).
    pub(super) fn start_handover(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) -> bool {
        if !self.handover.due(now) {
            return false;
        }
        let Some(out) = self.readdress() else {
            return false;
        };
        if !self.affordable(mac::airtime(&out.frame), now) {
            self.defer_handover(&out, now, emit);
            return false;
        }
        self.handover.begin_lbt(now);
        self.radio
            .start(Job::HandOver, Action::Cad(out.frame.channel), emit);
        true
    }

    fn defer_handover(&mut self, out: &Outgoing, now: Instant, emit: &mut dyn FnMut(Action)) {
        let airtime = mac::airtime(&out.frame);
        let until = self
            .budget
            .allowed_at(airtime, REPLY_RESERVE, now)
            .unwrap_or(now + self.config.heartbeat_max);
        self.handover.defer(until);
        let msg_id = out.msg_id().unwrap_or_default();
        emit(Action::Trace(Trace::HandOverDeferred { msg_id, until }));
    }

    /// A CAD or RSSI check of the current hand-over completed.
    pub(super) fn handover_check(
        &mut self,
        busy: bool,
        now: Instant,
        emit: &mut dyn FnMut(Action),
    ) {
        let step = self.handover.step();
        // The hand-over may have ended meanwhile (spec "Receiving DATA" case 5).
        if !matches!(step, Step::Cad | Step::Rssi) {
            return;
        }
        let Some(out) = self.queue.current().copied() else {
            return;
        };
        let backoff = self.config.hop_backoff_max;
        match self.handover.checked(busy, now, &mut self.rng, backoff) {
            Check::Clear if step == Step::Cad => {
                self.radio
                    .start(Job::HandOver, Action::Rssi(out.frame.channel), emit)
            }
            Check::Clear => self.transmit_current(out, now, emit),
            Check::Busy => {}
            Check::GaveUp => self.handover_gave_up(out, now, emit),
        }
    }

    /// Listen-before-talk is clear: transmit the current frame, with this
    /// relay's home channel as it is now (spec "Packet formats" `aux`). An owed ACK goes
    /// first, and the check is then repeated.
    fn transmit_current(&mut self, out: Outgoing, now: Instant, emit: &mut dyn FnMut(Action)) {
        if self.replies.wake_at().is_some_and(|t| t <= now) {
            self.handover.hold(now);
            return;
        }
        let mut frame = out.frame;
        if let Ok(Packet::Data(d)) = Packet::decode(out.frame.payload()) {
            let id = d.msg_id();
            let Some(hops) = self.count(&d, id) else {
                self.handover.cancel();
                self.queue.take_current();
                self.skipped = 0;
                self.feed_due = true;
                self.drop_msg(id, DropReason::HopCap, emit);
                return;
            };
            frame = data_frame(
                &Data {
                    sender_home: self.home,
                    hops,
                    ..d
                },
                out.frame.channel,
            );
            if let Some(r) = self.records.get_mut(id) {
                r.sent(now);
            }
        } else if let Ok(Packet::Deliver(d)) = Packet::decode(out.frame.payload()) {
            // Whether a sleeping leaf listens is a matter of now, not of
            // when the DELIVER was queued (spec "Leaf receive mode").
            frame.preamble_symbols = self.deliver_preamble(d.dst_leaf, d.msg_id, now);
        }
        let airtime = mac::airtime(&frame);
        if !self.affordable(airtime, now) {
            // An owed reply spent the budget since the check began.
            self.defer_handover(&out, now, emit);
            return;
        }
        if let Some(current) = self.queue.current_mut() {
            current.frame = frame;
        }
        self.budget.record(airtime, now);
        self.handover.transmitting();
        self.radio
            .start(Job::HandOver, Action::Transmit(frame), emit);
    }

    /// The count the current hand-over of `d` carries (spec "Hop count"): stamped at
    /// its first transmission as the record's highest plus one, repeated by
    /// its retries. Stamped when queued, a frame would keep a count older
    /// than one this relay has since received or sent, and a relay that
    /// readdressed it would offer a count it had already used. `None` past
    /// the hop cap.
    fn count(&self, d: &Data<'_>, id: MsgId) -> Option<u8> {
        if self.handover.transmitted() {
            return Some(d.hops);
        }
        debug_assert!(
            self.records.get(id).is_some(),
            "a held message keeps its record"
        );
        match self.records.get(id) {
            Some(r) => r.highest_hops().checked_add(1),
            None => Some(d.hops),
        }
    }

    /// Listen-before-talk gave up on the current hand-over's channel.
    fn handover_gave_up(&mut self, out: Outgoing, now: Instant, emit: &mut dyn FnMut(Action)) {
        match out.kind {
            Kind::Forward => {
                // Choose again, past this neighbour (spec "Hand-over" step 2).
                let to = self.handover.expect().from;
                if let Some((slot, _)) = self
                    .neighbours
                    .slots()
                    .find(|(_, n)| AckName::from(n.position.short()) == to)
                {
                    self.skipped |= 1 << slot;
                }
            }
            Kind::Return | Kind::Deliver => {
                let (max, retry) = (self.config.hop_attempts, self.config.retry_backoff);
                if self.handover.lost_attempt(now, max, &mut self.rng, retry) == Missed::Failed {
                    self.handover_failed(out, false, now, emit);
                }
            }
        }
    }

    /// An ACK arrived: if it answers the current hand-over, the receiver
    /// took the packet, or refused it (spec "Hand-over"); see the module documentation.
    pub(super) fn ack_received(&mut self, ack: Ack, now: Instant, emit: &mut dyn FnMut(Action)) {
        if ack.refused {
            self.refused(ack, now, emit);
        } else if self.handover.acked(ack) {
            self.handover_done(now, emit);
        }
    }

    fn refused(&mut self, ack: Ack, now: Instant, emit: &mut dyn FnMut(Action)) {
        if !self.handover.answered_by(ack) {
            return;
        }
        let Some(out) = self.queue.current().copied() else {
            return;
        };
        let msg_id = out.msg_id().unwrap_or_default();
        let by = ShortName(u32::from_be_bytes(ack.from.0));
        emit(Action::Trace(Trace::TurnedAway { msg_id, by }));
        let slot = self.neighbours.by_short(by).map(|(s, n)| (s, n.position));
        if let Some((_, position)) = slot {
            self.neighbours.answered(position, now);
            self.neighbours.refused(position, now + BAN);
        }
        match out.kind {
            Kind::Forward => {
                if let Some((slot, _)) = slot {
                    self.skipped |= 1 << slot;
                }
                self.handover.cancel();
            }
            // No other receiver: wait for the ban, and let the queue move.
            Kind::Return | Kind::Deliver => {
                let tries = self.handover.refused();
                self.skipped = 0;
                if let Some(current) = self.queue.current_mut() {
                    current.not_before = now + BAN;
                    current.tries = tries;
                }
            }
        }
    }

    /// The current hand-over succeeded: its receiver has the packet.
    pub(super) fn handover_done(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        self.handover.cancel();
        self.skipped = 0;
        let Some(out) = self.queue.take_current() else {
            return;
        };
        self.feed_due = true;
        let d = match Packet::decode(out.frame.payload()) {
            Ok(Packet::Data(d)) => d,
            Ok(Packet::Deliver(d)) => return self.delivered_to(d, now, emit),
            _ => return,
        };
        let (id, to) = (d.msg_id(), d.dst_hop);
        let slot = self.neighbours.by_short(to).map(|(s, n)| (s, n.position));
        let receiver = slot.map(|(_, p)| p);
        if let Some(p) = receiver {
            self.neighbours.succeeded(p, now);
        }
        if let Some(r) = self.records.get_mut(id) {
            r.handed_over(d.hops);
            if let (Kind::Forward, Some((s, _))) = (out.kind, slot) {
                r.handed_to(s);
            }
            if receiver == Some(d.dst_pos) {
                // The relay it is addressed to took it: it has arrived.
                r.set_arrived();
            }
        }
        let trace = match out.kind {
            Kind::Forward => {
                if self.own == Some(id) {
                    self.set_own(None);
                }
                Trace::Forwarded {
                    msg_id: id,
                    to,
                    hops: d.hops,
                }
            }
            Kind::Return | Kind::Deliver => Trace::ReturnedToParent { msg_id: id, to },
        };
        debug_assert!(out.kind != Kind::Deliver, "a DELIVER is not a DATA");
        emit(Action::Trace(trace));
    }

    /// Every attempt of the current hand-over went unanswered (spec "Hand-over" step 6).
    /// `counted`: a failure is counted against the neighbour (spec "Neighbour table").
    fn handover_failed(
        &mut self,
        out: Outgoing,
        counted: bool,
        now: Instant,
        emit: &mut dyn FnMut(Action),
    ) {
        let id = out.msg_id().unwrap_or_default();
        if let Ok(Packet::Data(d)) = Packet::decode(out.frame.payload()) {
            if counted {
                self.count_failure(d.dst_hop, now, emit);
                if let Some(t) = self.wait_for(d.dst_hop, now) {
                    // The same neighbour again once its ban ends, with
                    // fresh attempts (roadmap D42, W).
                    self.skipped = 0;
                    if let Some(current) = self.queue.current_mut() {
                        current.not_before = t;
                        current.tries = 0;
                    }
                    return;
                }
            }
            if out.kind == Kind::Forward {
                // Most likely the frame, not the ACK, was lost: the
                // neighbour stays untried, set aside for this choice only.
                if let Some((slot, _)) = self.neighbours.by_short(d.dst_hop) {
                    self.skipped |= 1 << slot;
                }
            }
        }
        let reason = match out.kind {
            Kind::Forward => return,
            Kind::Return => Some(DropReason::ReturnFailed),
            Kind::Deliver => None,
        };
        if let Ok(Packet::Deliver(d)) = Packet::decode(out.frame.payload()) {
            self.leaves.set_missed(d.dst_leaf, true);
        }
        self.queue.take_current();
        self.skipped = 0;
        self.feed_due = true;
        if let Some(reason) = reason {
            self.drop_msg(id, reason, emit);
        }
    }

    /// When to offer a failed hand-over to `name` again (roadmap D42):
    /// when its ban ends, until a failure demotes it. A demoted neighbour
    /// is likely gone or faded; a forward then takes its next candidate,
    /// and a return is dropped.
    fn wait_for(&self, name: ShortName, now: Instant) -> Option<Instant> {
        let (_, n) = self.neighbours.by_short(name)?;
        (!self.neighbours.is_demoted(n, now)).then_some(n.banned_until)
    }

    fn count_failure(&mut self, name: ShortName, now: Instant, emit: &mut dyn FnMut(Action)) {
        let Some((_, n)) = self.neighbours.by_short(name) else {
            return;
        };
        let (position, was) = (n.position, self.neighbours.is_demoted(n, now));
        if self.neighbours.failed(position, now) == Some(true) && !was {
            emit(Action::Trace(Trace::Demoted { neighbour: name }));
        }
    }

    // --- The owed reply (spec "Hand-over") ---

    /// Start sending the owed reply, if it is ready. Without budget
    /// for it the relay cannot reply (spec "Duty cycle cap"); the reply is abandoned.
    pub(super) fn start_reply(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) -> bool {
        let Some(o) = self.replies.due(now) else {
            return false;
        };
        let frame = self.reply_frame(o, now);
        if !self.can_reply(mac::airtime(&frame), now) {
            self.replies.abandon();
            return true;
        }
        match self.replies.begin() {
            Some(cad) => {
                self.radio.start(Job::Reply, cad, emit);
                true
            }
            None => false,
        }
    }

    pub(super) fn reply_check(&mut self, busy: bool, now: Instant, emit: &mut dyn FnMut(Action)) {
        let backoff = self.config.hop_backoff_max;
        match self.replies.checked(busy, now, &mut self.rng, backoff) {
            Some((o, false)) => self.radio.start(Job::Reply, Action::Rssi(o.channel), emit),
            Some((o, true)) => {
                let frame = self.reply_frame(o, now);
                let airtime = mac::airtime(&frame);
                if self.can_reply(airtime, now) {
                    self.budget.record(airtime, now);
                    self.radio.start(Job::Reply, Action::Transmit(frame), emit);
                } else {
                    self.replies.abandon();
                }
            }
            None => {}
        }
    }

    // --- Timers ---

    pub(super) fn forward_timers(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        let (max, retry) = (self.config.hop_attempts, self.config.retry_backoff);
        if self.handover.timeout(now, max, &mut self.rng, retry) == Some(Missed::Failed) {
            if let Some(out) = self.queue.current().copied() {
                self.handover_failed(out, true, now, emit);
            }
        }
    }

    /// When forwarding next needs the core. The ACK wait counts even while
    /// an owed reply holds the radio; the rest only once the radio is free.
    pub(super) fn forward_wakeup(&self) -> Option<Instant> {
        let mut w: Option<Instant> = None;
        let mut at = |t: Instant| w = Some(w.map_or(t, |w| w.min(t)));
        if self.handover.awaiting_ack() || self.radio.idle() {
            if let Some(t) = self.handover.wake_at() {
                at(t);
            }
        }
        if self.radio.idle() {
            if let Some(t) = self.replies.wake_at() {
                at(t);
            }
        }
        // A waiting entry, once nothing else is under way.
        if self.handover.idle() && self.radio.idle() {
            if let Some(t) = self.queue.earliest_due() {
                at(t);
            }
        }
        w
    }
}
