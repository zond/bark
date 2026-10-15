//! Forwarding (design §9): the relay's part of the depth-first search, the
//! owner rule, and the per-hop transfer of its send queue.
//!
//! # Receiving a DATA packet (§9.2.4)
//!
//! [`Relay::data`] classifies the packet by the six cases of §9.2.4 in
//! order and decides whether acting on it is work: a forward, a return, a
//! packet sent straight back, or a delivery. Work to send on goes into the
//! send queue's regular room if it has a place and the bucket holds the
//! ACK, the airtime of everything queued with the new frame, and the reply
//! reserve (§9.2.4, roadmap D29); else into a free hop place numbered no
//! higher than the packet's count (roadmap D31); else it is refused with
//! BUSY. A new message also needs a place for its record, which the size
//! of the table guarantees (`tables/records.rs`). A delivery is refused when
//! the delivery queue is full, or when the bucket holds less than the
//! reply reserve and an ACK (`tables/budget.rs`). Otherwise the ACK is
//! owed before acting. A packet that gives no work (cases 2, 3 and 5) is
//! ACKed from the reply reserve: this relay already has the message, and
//! BUSY would send its sender on with a second copy. A relay that cannot
//! afford even the reply stays silent, and its sender retries.
//!
//! A relay owes one reply at a time (`transfer.rs`, roadmap D29). A packet
//! that arrives while it owes another one, its ACK still waiting for the
//! channel, is neither taken nor refused: the relay says nothing, and the
//! sender's retry brings the packet again. A copy of the packet the owed
//! reply answers is answered by it too.
//!
//! Three refinements the spec does not spell out:
//!
//! - A return from the last child while the send queue still holds the
//!   relay's searching copy (a forward or a return to someone else) is
//!   only ACKed. That happens when the hand-over to the child was given up
//!   (its ACKs were lost) and the relay has already moved on: like case
//!   5, the relay's search has moved on, and a second one would split the
//!   message.
//! - An offer from the last child after the relay has returned the
//!   message to its parent is sent straight back (case 6), not taken for a
//!   return (case 4); see [`Record::returned`].
//! - A message delivered here, or waiting in the delivery queue, is only
//!   ACKed whatever its record says: the delivered window and the
//!   delivery queue remember it, and its record may have made way for
//!   newer ones (§9.2.7).
//!
//! # The send queue (§9.3)
//!
//! Every packet this relay sends for a message goes through the send
//! queue, one hand-over at a time ([`Transfer`]). A forward is addressed
//! when its hand-over starts, by the one next-hop function of §9.2.3, and
//! readdressed by it after a failure; with no candidate left it becomes a
//! return to the parent, or at the origin a drop. The hop count a packet
//! carries is fixed when it is queued: what the relay received plus one
//! (§9.2.6).
//!
//! Where a frame goes is read at the moment of sending (roadmap D29 rule
//! 7): before each attempt its channel is the receiver's home as the
//! neighbour table has it, from the latest heartbeat or packet of that
//! neighbour, and a DELIVER's is this relay's home, with the preamble of
//! the leaf's receive mode as the leaf table has it. A neighbour that
//! moves home (§6.3) between attempts, or while a message waits to come
//! back to it, is followed.
//!
//! When listen-before-talk gives up on a forward's neighbour (§9.3 step
//! 2), the neighbour is set aside for this hand-over only (`skipped`):
//! it was not handed the message, so it stays a candidate for later
//! choices, but choosing again at once must not pick it again. A return,
//! a send-back and a DELIVER have nowhere else to go; for them a give-up
//! counts as one of the three attempts, and never as a failure against
//! the neighbour.
//!
//! A BUSY from the receiver of the front hand-over, for its message, bans
//! the receiver for [`BAN`], so no message takes it until then (§9.3,
//! roadmap D27, D29, D30). A forward treats it as that give-up, and since
//! the frame went out, the record's mark that the receiver was handed the
//! message is taken back. A return or a send-back has nowhere else to go:
//! it waits in its place for the ban to end, and the refused transmission
//! does not count as one of its attempts.
//!
//! A forward whose every remaining candidate is banned waits in its place
//! until the earliest ban ends, and is routed again then (roadmap D31). It
//! is not returned or dropped: overload makes a message wait, never end.
//! Only a search exhausted by tried and failed neighbours returns or drops.
//! Waiting entries keep their airtime in admission, and the queue hands
//! over the first entry that is due. The application's own message waits
//! the same way, longer after refusals in a row ([`hold`]); one for an own
//! leaf that finds the delivery queue full is set aside ([`Held`]) until
//! the queue has a place (§9.2.5). Either way the application is given no
//! other meanwhile.
//!
//! [`Transfer`]: crate::node::transfer::Transfer

use super::{Job, Relay};
use crate::event::{Action, AppEvent, DropReason, Frame, Trace, MAX_FRAME};
use crate::ids::{LeafId, MsgId};
use crate::node::mac;
use crate::node::transfer::{Check, Missed, Reply, Step};
use crate::position::{Position, ShortName};
use crate::radio::{time_on_air, Channel, MIN_PREAMBLE_SYMBOLS};
use crate::tables::budget::{hold, BAN, BUSY_AIRTIME, HEARTBEAT_RESERVE, REPLY_RESERVE};
use crate::tables::delivery::{Pushed, Queued};
use crate::tables::neighbours::Neighbour;
use crate::tables::records::{Record, HELD, MAX_HOPS};
use crate::tables::send_queue::{Kind, Outgoing, SEND_QUEUE};
use crate::time::{Duration, Instant};
use crate::wire::{Ack, AckFrom, Busy, Data, Packet, ACK_LEN, DATA_HEADER_LEN, MAX_PAYLOAD};

/// A relay within this distance of a destination that is not its own
/// position, with no live, undemoted neighbour closer, owns it (design
/// §9.2.7): 50 m is what §11 allows between two GPS fixes of a relay that
/// has not moved, so the addressed relay and this one are, for the radio,
/// in the same place.
pub const OWNER_RADIUS_M: f32 = 50.0;

/// What a received DATA packet is (§9.2.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Case {
    /// Case 1: no record.
    New,
    /// Cases 2, 3 and 5: ACK only.
    AckOnly,
    /// Case 4: back from the last child.
    FromLastChild,
    /// Case 6: offered by a relay that does not know this one has it.
    SendBack,
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

fn ack_airtime() -> Duration {
    time_on_air(ACK_LEN as u8, MIN_PREAMBLE_SYMBOLS)
}

fn data_airtime(payload: usize) -> Duration {
    time_on_air((DATA_HEADER_LEN + payload) as u8, MIN_PREAMBLE_SYMBOLS)
}

/// `d` as a frame on `channel`.
fn data_frame(d: &Data<'_>, channel: Channel) -> Frame {
    mac::frame(&Packet::Data(*d), channel, MIN_PREAMBLE_SYMBOLS)
}

impl Relay {
    /// This relay's short name (design §8.1).
    pub(super) fn me(&self) -> ShortName {
        self.config.position.short()
    }

    /// Whether forwarding, or an ACK that takes on work, may spend
    /// `airtime` now: it must leave the reply reserve (§9.4).
    pub(super) fn affordable(&self, airtime: Duration, now: Instant) -> bool {
        self.budget.would_allow_keeping(airtime, REPLY_RESERVE, now)
    }

    /// The place in the send queue for a frame of `airtime`, received with
    /// `hops`, after this relay spends `spend` (the ACK that takes the
    /// packet); 0 is the regular room. The regular room needs a place, and
    /// everything queued, the frame included, must fit in the bucket now
    /// with the reply reserve left (§9.2.4, §9.3). Failing that, the
    /// highest free hop place not above `hops` takes it, whatever the
    /// bucket holds (roadmap D31). `freed`: the front's hand-over ends
    /// first, giving up its place.
    pub(super) fn admit(
        &self,
        airtime: Duration,
        spend: Duration,
        hops: u8,
        freed: bool,
        now: Instant,
    ) -> Result<u8, DropReason> {
        let front = self.queue.front().filter(|_| freed);
        let front_air = front.map_or(Duration::ZERO, |o| mac::airtime(&o.frame));
        let queued = self.queue.airtime() - front_air + self.held_airtime();
        let regular = self.regular() - usize::from(front.is_some_and(|o| o.place == 0));
        let why = if regular >= SEND_QUEUE {
            DropReason::SendQueueFull
        } else if self.budget.available(now) < queued + airtime + spend + REPLY_RESERVE {
            DropReason::Budget
        } else {
            return Ok(0);
        };
        let freed_place = front.map(|o| o.place).filter(|&k| k > 0);
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
    /// heartbeat reserve (§9.4).
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

    // --- Owner and next hop (§9.2.3, §9.2.7) ---

    /// Whether this relay owns the destination `dst` (§9.2.7).
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

    /// The next hop for the message of `record` to `dst` (§9.2.3): the
    /// live neighbour nearest `dst` that is not in the tried set, not set
    /// aside and not banned, ties broken by short name; a demoted one only
    /// when no other is left (roadmap D25, D29).
    fn next_hop(&self, record: &Record, dst: Position, now: Instant) -> Option<(usize, Neighbour)> {
        let target = self.plane.point(dst);
        let parent = record.parent().map(|(name, _)| name);
        let skipped = self.skipped;
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
                demoted(a)
                    .cmp(&demoted(b))
                    .then(da.total_cmp(&db))
                    .then(a.position.short().0.cmp(&b.position.short().0))
            })
            .map(|(slot, n)| (slot, *n))
    }

    // --- Receiving (§9.2.4) ---

    /// A DATA packet addressed to this relay.
    pub(super) fn data(&mut self, d: Data<'_>, now: Instant, emit: &mut dyn FnMut(Action)) {
        let id = d.msg_id();
        let from_leaf = d.hops == 0;
        let ack = Ack {
            msg_id: id,
            from: AckFrom::from(self.me()),
        };
        let case = if self.delivered.contains(id) || self.delivery.iter().any(|q| q.msg_id == id) {
            Case::AckOnly
        } else {
            self.classify(&d, id, from_leaf)
        };
        let owner = match case {
            Case::New | Case::FromLastChild => self.owns(d.dst_pos, now),
            Case::AckOnly | Case::SendBack => false,
        };
        let sends = match case {
            Case::New | Case::FromLastChild => !owner,
            Case::SendBack => true,
            Case::AckOnly => false,
        };
        // A return from the child being handed the message ends that
        // hand-over, which frees its place in the queue.
        let completes = case == Case::FromLastChild && self.handing(id, d.src_hop);
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
            match self.admit(air, ack_airtime(), d.hops, completes, now) {
                Ok(k) => {
                    place = k;
                    None
                }
                Err(why) => Some(why),
            }
        } else if for_leaf && self.delivery.is_full() {
            Some(DropReason::DeliveryFull)
        } else if !self.affordable(ack_airtime(), now) {
            Some(DropReason::Budget)
        } else {
            None
        };
        let reply = match refusal {
            Some(_) => Reply::Busy(Busy::refusing(self.me(), id)),
            None => Reply::Ack(ack),
        };
        if !self.replies.can_owe(reply, d.sender_home, now) {
            // The packet could not be answered; its sender's retry
            // brings it again (§9.3, roadmap D29).
            return;
        }
        if let Some(reason) = refusal {
            emit(Action::Trace(Trace::Refused { msg_id: id, reason }));
            if self.can_reply(BUSY_AIRTIME, now) {
                self.replies.owe(reply, d.sender_home, deadline, now);
            }
            return;
        }
        if !self.can_reply(ack_airtime(), now) {
            emit(Action::Trace(Trace::Refused {
                msg_id: id,
                reason: DropReason::Budget,
            }));
            return;
        }
        self.replies.owe(reply, d.sender_home, deadline, now);

        match case {
            Case::New => {
                let parent = (!from_leaf).then_some((d.src_hop, d.sender_home));
                let mut record = Record::new(id, parent, d.hops, now);
                if owner {
                    record.set_owned();
                }
                if let Some(at) = record_at {
                    self.records.put(at, record);
                }
                if owner {
                    self.arrive(id, d.dst_pos, d.dst_leaf, d.payload, now, emit);
                } else {
                    self.send_on(&d, Kind::Forward, place, emit);
                }
            }
            Case::AckOnly => {
                if let Some(r) = self.records.get_mut(id) {
                    r.received(d.hops, now);
                }
                self.delivered.offered(id, now);
            }
            Case::FromLastChild => {
                if let Some(r) = self.records.get_mut(id) {
                    r.received(d.hops, now);
                    if owner {
                        r.set_owned();
                    }
                }
                if completes {
                    self.handover_done(now, emit);
                }
                if owner {
                    self.arrive(id, d.dst_pos, d.dst_leaf, d.payload, now, emit);
                } else {
                    self.send_on(&d, Kind::Forward, place, emit);
                }
            }
            Case::SendBack => {
                if let Some(r) = self.records.get_mut(id) {
                    r.received(d.hops, now);
                }
                self.send_on(&d, Kind::Bounce, place, emit);
            }
        }
    }

    /// The case of §9.2.4 for a DATA packet of a message this relay is not
    /// holding for delivery.
    fn classify(&self, d: &Data<'_>, id: MsgId, from_leaf: bool) -> Case {
        let Some(r) = self.records.get(id) else {
            return Case::New;
        };
        if r.owned() || d.hops <= r.highest_hops() {
            return Case::AckOnly;
        }
        // Past the copy test a leaf's packet (`hops` 0) cannot get.
        debug_assert!(!from_leaf);
        let sender = d.src_hop;
        if r.last_child() == Some(sender) {
            return if self.handing(id, sender) {
                Case::FromLastChild
            } else if self.searching(id) {
                Case::AckOnly
            } else if r.returned() {
                Case::SendBack
            } else {
                Case::FromLastChild
            };
        }
        // An earlier child may be offering its own searching copy, which
        // only a send-back keeps alive (§9.2.4 case 6).
        if r.parent().map(|(name, _)| name) == Some(sender) {
            return Case::AckOnly;
        }
        Case::SendBack
    }

    /// Whether the send queue holds this relay's searching copy of message
    /// `id`: a forward or a return.
    fn searching(&self, id: MsgId) -> bool {
        self.queue
            .iter()
            .any(|o| matches!(o.kind, Kind::Forward | Kind::Return) && o.msg_id() == Some(id))
    }

    /// Whether the front hand-over is of message `id` to `to`, sent at
    /// least once.
    fn handing(&self, id: MsgId, to: ShortName) -> bool {
        !self.handover.idle()
            && self.handover.transmitted()
            && self.handover.expect()
                == Ack {
                    msg_id: id,
                    from: AckFrom::from(to),
                }
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

    /// Queue `d` onward into `place`: a forward, addressed later, or
    /// straight back to its sender. Carries the received count plus one
    /// (§9.2.6).
    fn send_on(&mut self, d: &Data<'_>, kind: Kind, place: u8, emit: &mut dyn FnMut(Action)) {
        let id = d.msg_id();
        if d.hops == MAX_HOPS {
            self.drop_msg(id, DropReason::HopCap, emit);
            return;
        }
        let (dst_hop, channel) = match kind {
            Kind::Bounce => (d.src_hop, d.sender_home),
            _ => (ShortName(0), self.home),
        };
        let out = Data {
            sender_home: self.home,
            dst_hop,
            src_hop: self.me(),
            hops: d.hops + 1,
            ..*d
        };
        let out = Outgoing {
            place,
            ..Outgoing::new(data_frame(&out, channel), kind)
        };
        if self.queue.push(out).is_err() {
            self.drop_msg(id, DropReason::SendQueueFull, emit);
        }
    }

    /// The message has reached its owner, this relay (§9.2.7): deliver it
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
                // to refuse (§9.2.4 case 4).
                self.drop_msg(id, DropReason::DeliveryFull, emit);
                return;
            }
            Pushed::Queued => self.feed_due = true,
        }
        emit(Action::Trace(Trace::Queued { msg_id: id, leaf }));
        self.deliver_now(leaf, now);
    }

    // --- The relay's own application (§9.2) ---

    /// Whether the application may send now (roadmap D29 rule 6): the
    /// relay is running, holds no message of the application's, and its
    /// send queue takes a full frame on top of what it holds, as a forward
    /// would be taken but without the ACK.
    pub(super) fn accepts_send(&self, now: Instant) -> bool {
        !self.booting()
            && self.own.is_none()
            && self
                .no_room(data_airtime(MAX_PAYLOAD), Duration::ZERO, now)
                .is_none()
    }

    /// When the application may send again, if only the bucket keeps it
    /// waiting; anything else ends with an event.
    pub(super) fn send_ready_at(&self, now: Instant) -> Option<Instant> {
        if self.booting() || self.own.is_some() || self.regular() >= SEND_QUEUE {
            return None;
        }
        let need =
            self.queue.airtime() + self.held_airtime() + data_airtime(MAX_PAYLOAD) + REPLY_RESERVE;
        self.budget
            .allowed_at(Duration::ZERO, need, now)
            .filter(|&t| t > now)
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
    /// The application is the first sender and sends count 0 (§9.2.6), as
    /// a leaf does; this relay then sends 1, what it received plus one. It
    /// cannot put 0 on the air itself: a relay receiving count 0 takes the
    /// sender for a leaf and itself for the origin (§9.2.1). A message the
    /// relay already has a record of is in the network already, as is one
    /// delivered or queued here (§9.2.7).
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
            record.set_owned();
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
        self.send_on(&d, Kind::Forward, 0, emit);
    }

    // --- The hand-over (§9.3) ---

    /// Make sure the first due entry of the send queue, brought to the
    /// front, has a hand-over: address a forward, or drop what cannot be
    /// sent.
    pub(super) fn prepare(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        while self.handover.idle() {
            let Some(i) = self.queue.next_due(now) else {
                return;
            };
            if i > 0 {
                // What was set aside belongs to the old front.
                self.queue.to_front(i);
                self.skipped = 0;
            }
            let Some(out) = self.queue.front().copied() else {
                return;
            };
            let routed = match out.kind {
                Kind::Forward => self.route(out, now, emit),
                Kind::Return | Kind::Bounce => match Packet::decode(out.frame.payload()) {
                    Ok(Packet::Data(d)) => Routed::To(
                        out,
                        Ack {
                            msg_id: d.msg_id(),
                            from: d.dst_hop.into(),
                        },
                    ),
                    _ => Routed::Done,
                },
                Kind::Deliver => match Packet::decode(out.frame.payload()) {
                    Ok(Packet::Deliver(d)) => Routed::To(
                        out,
                        Ack {
                            msg_id: d.msg_id,
                            from: d.dst_leaf.into(),
                        },
                    ),
                    _ => Routed::Done,
                },
            };
            match routed {
                Routed::To(out, expect) => {
                    if let Some(front) = self.queue.front_mut() {
                        *front = out;
                    }
                    self.handover.start(expect, out.tries, now);
                }
                Routed::Done => {
                    self.queue.pop();
                    self.skipped = 0;
                    self.feed_due = true;
                }
                Routed::Hold => {
                    if let Some(out) = self.queue.pop() {
                        self.held = Some(Held { out });
                    }
                    self.skipped = 0;
                    self.feed_due = true;
                }
                Routed::Wait(t) => {
                    if let Some(front) = self.queue.front_mut() {
                        front.not_before = t;
                    }
                    self.skipped = 0;
                }
            }
        }
    }

    /// Address the forward `out` (§9.2.3), or let it wait for a ban to
    /// end, or return it to the parent (§9.2.5), or drop it at the origin.
    /// Ownership is checked again first: a closer neighbour may have been
    /// demoted since (§9.2.4 case 4).
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
                r.set_owned();
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
            return Routed::To(
                Outgoing { frame, ..out },
                Ack {
                    msg_id: id,
                    from: to.into(),
                },
            );
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
                if let Some(r) = self.records.get_mut(id) {
                    r.set_returned();
                }
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
                    Ack {
                        msg_id: id,
                        from: parent.into(),
                    },
                )
            }
            None => {
                self.drop_msg(id, DropReason::NoRoute, emit);
                Routed::Done
            }
        }
    }

    /// When the earliest ban ends among the neighbours that would be
    /// candidates for the message of `record` but for their ban; `None`
    /// if the search was exhausted by tried and failed ones alone.
    /// A BUSY also sets its sender aside for this hand-over, so being set
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

    /// Address the front frame as its receiver is now, before each attempt
    /// (§9.3): a neighbour that moved home since the frame was queued, or
    /// since the last attempt, listens elsewhere, and a leaf that changed
    /// its receive mode needs another preamble. A receiver no longer in its
    /// table keeps the address it had.
    fn readdress(&mut self, now: Instant) -> Option<Outgoing> {
        let mut out = self.queue.front().copied()?;
        let (channel, preamble) = match Packet::decode(out.frame.payload()) {
            Ok(Packet::Data(d)) => (self.home_of(d.dst_hop), None),
            // The leaf listens on this relay's home, wherever that is now.
            Ok(Packet::Deliver(d)) => (
                Some(self.home),
                self.leaves
                    .get(d.dst_leaf, now)
                    .map(|l| l.mode.preamble_symbols()),
            ),
            _ => (None, None),
        };
        out.frame.channel = channel.unwrap_or(out.frame.channel);
        out.frame.preamble_symbols = preamble.unwrap_or(out.frame.preamble_symbols);
        if let Some(front) = self.queue.front_mut() {
            *front = out;
        }
        Some(out)
    }

    /// Start listen-before-talk for the front hand-over if it is due and
    /// the budget lets it (§9.4: deferred, never dropped).
    pub(super) fn start_handover(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) -> bool {
        if !self.handover.due(now) {
            return false;
        }
        let Some(out) = self.readdress(now) else {
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

    /// A CAD or RSSI check of the front hand-over completed.
    pub(super) fn handover_check(
        &mut self,
        busy: bool,
        now: Instant,
        emit: &mut dyn FnMut(Action),
    ) {
        let step = self.handover.step();
        // The hand-over may have ended meanwhile (§9.2.4 case 4).
        if !matches!(step, Step::Cad | Step::Rssi) {
            return;
        }
        let Some(out) = self.queue.front().copied() else {
            return;
        };
        let backoff = self.config.hop_backoff_max;
        match self.handover.checked(busy, now, &mut self.rng, backoff) {
            Check::Clear if step == Step::Cad => {
                self.radio
                    .start(Job::HandOver, Action::Rssi(out.frame.channel), emit)
            }
            Check::Clear => self.transmit_front(out, now, emit),
            Check::Busy => {}
            Check::GaveUp => self.handover_gave_up(out, now, emit),
        }
    }

    /// Listen-before-talk is clear: transmit the front frame, with this
    /// relay's home channel as it is now (§14 `aux`). An owed ACK goes
    /// first, and the check is then repeated.
    fn transmit_front(&mut self, out: Outgoing, now: Instant, emit: &mut dyn FnMut(Action)) {
        if self.replies.wake_at().is_some_and(|t| t <= now) {
            self.handover.hold(now);
            return;
        }
        let mut frame = out.frame;
        if let Ok(Packet::Data(d)) = Packet::decode(out.frame.payload()) {
            frame = data_frame(
                &Data {
                    sender_home: self.home,
                    ..d
                },
                out.frame.channel,
            );
            let id = d.msg_id();
            if let Some(r) = self.records.get_mut(id) {
                match out.kind {
                    Kind::Forward => {
                        let slot = self
                            .neighbours
                            .by_short(d.dst_hop)
                            .map_or(usize::MAX, |(s, _)| s);
                        r.sent_to_child(d.dst_hop, slot, now);
                    }
                    _ => r.sent(now),
                }
            }
        }
        let airtime = mac::airtime(&frame);
        if !self.affordable(airtime, now) {
            // An owed reply spent the budget since the check began.
            self.defer_handover(&out, now, emit);
            return;
        }
        if let Some(front) = self.queue.front_mut() {
            front.frame = frame;
        }
        self.budget.record(airtime, now);
        self.handover.transmitting();
        self.radio
            .start(Job::HandOver, Action::Transmit(frame), emit);
    }

    /// Listen-before-talk gave up on the front hand-over's channel.
    fn handover_gave_up(&mut self, out: Outgoing, now: Instant, emit: &mut dyn FnMut(Action)) {
        match out.kind {
            Kind::Forward => {
                // Choose again, past this neighbour (§9.3 step 2).
                let to = self.handover.expect().from;
                if let Some((slot, _)) = self
                    .neighbours
                    .slots()
                    .find(|(_, n)| AckFrom::from(n.position.short()) == to)
                {
                    self.skipped |= 1 << slot;
                }
            }
            Kind::Return | Kind::Bounce | Kind::Deliver => {
                let (max, retry) = (self.config.hop_attempts, self.config.retry_backoff);
                if self.handover.lost_attempt(now, max, &mut self.rng, retry) == Missed::Failed {
                    self.handover_failed(out, false, now, emit);
                }
            }
        }
    }

    /// An ACK arrived.
    pub(super) fn ack_received(&mut self, ack: Ack, now: Instant, emit: &mut dyn FnMut(Action)) {
        if self.handover.acked(ack) {
            self.handover_done(now, emit);
        }
    }

    /// A BUSY arrived: if from the receiver of the front hand-over, it
    /// does not take the packet (§9.3); see the module documentation.
    pub(super) fn busy_received(&mut self, b: Busy, now: Instant, emit: &mut dyn FnMut(Action)) {
        if !self.handover.refused_by(b) {
            return;
        }
        let Some(out) = self.queue.front().copied() else {
            return;
        };
        let msg_id = out.msg_id().unwrap_or_default();
        emit(Action::Trace(Trace::TurnedAway { msg_id, by: b.from }));
        let slot = self
            .neighbours
            .by_short(b.from)
            .map(|(s, n)| (s, n.position));
        if let Some((_, position)) = slot {
            self.neighbours.refused(position, now + BAN);
        }
        match out.kind {
            Kind::Forward => {
                if let Some((slot, _)) = slot {
                    self.skipped |= 1 << slot;
                    if let Some(r) = self.records.get_mut(msg_id) {
                        r.not_handed(slot);
                    }
                }
                self.handover.cancel();
            }
            // No other receiver: wait for the ban, and let the queue move.
            Kind::Return | Kind::Bounce | Kind::Deliver => {
                let tries = self.handover.refused();
                self.skipped = 0;
                if let Some(front) = self.queue.front_mut() {
                    front.not_before = now + BAN;
                    front.tries = tries;
                }
            }
        }
    }

    fn neighbour_named(&self, name: ShortName) -> Option<Position> {
        self.neighbours.by_short(name).map(|(_, n)| n.position)
    }

    /// The front hand-over succeeded: its receiver has the packet.
    pub(super) fn handover_done(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        self.handover.cancel();
        self.skipped = 0;
        let Some(out) = self.queue.pop() else { return };
        self.feed_due = true;
        let d = match Packet::decode(out.frame.payload()) {
            Ok(Packet::Data(d)) => d,
            Ok(Packet::Deliver(d)) => return self.delivered_to(d, now, emit),
            _ => return,
        };
        let (id, to) = (d.msg_id(), d.dst_hop);
        if let Some(p) = self.neighbour_named(to) {
            self.neighbours.succeeded(p);
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
            Kind::Return => Trace::ReturnedToParent { msg_id: id, to },
            Kind::Bounce | Kind::Deliver => Trace::Bounced { msg_id: id, to },
        };
        debug_assert!(out.kind != Kind::Deliver, "a DELIVER is not a DATA");
        emit(Action::Trace(trace));
    }

    /// Every attempt of the front hand-over went unanswered (§9.3 step 6).
    /// `counted`: a failure is counted against the neighbour (§8.2).
    fn handover_failed(
        &mut self,
        out: Outgoing,
        counted: bool,
        now: Instant,
        emit: &mut dyn FnMut(Action),
    ) {
        let id = out.msg_id().unwrap_or_default();
        if counted {
            if let Ok(Packet::Data(d)) = Packet::decode(out.frame.payload()) {
                self.count_failure(d.dst_hop, now, emit);
            }
        }
        let reason = match out.kind {
            // The neighbour is the last child now; the next one in order is
            // taken when the front is prepared again.
            Kind::Forward => return,
            Kind::Return => Some(DropReason::ReturnFailed),
            Kind::Bounce => Some(DropReason::BounceFailed),
            Kind::Deliver => None,
        };
        if let Ok(Packet::Deliver(d)) = Packet::decode(out.frame.payload()) {
            self.leaves.set_missed(d.dst_leaf, true);
        }
        self.queue.pop();
        self.skipped = 0;
        self.feed_due = true;
        if let Some(reason) = reason {
            self.drop_msg(id, reason, emit);
        }
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

    // --- The owed reply (§9.3) ---

    /// Start sending the owed reply, if it is ready. Without budget
    /// for it the relay cannot reply (§9.4); the reply is abandoned.
    pub(super) fn start_reply(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) -> bool {
        let Some(o) = self.replies.due(now) else {
            return false;
        };
        let frame = mac::frame(&o.reply.packet(), o.channel, MIN_PREAMBLE_SYMBOLS);
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
                let frame = mac::frame(&o.reply.packet(), o.channel, MIN_PREAMBLE_SYMBOLS);
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
            if let Some(out) = self.queue.front().copied() {
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
