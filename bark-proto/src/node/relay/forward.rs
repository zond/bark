//! Forwarding (design §9): the relay's part of the depth-first search, the
//! owner rule, and the per-hop transfer of its send queue.
//!
//! # Receiving a DATA packet (§9.2.4)
//!
//! [`Relay::data`] classifies the packet by the six cases of §9.2.4 in
//! order and decides whether acting on it is work: a forward, a return, a
//! packet sent straight back, or a delivery. Work to send on is refused
//! with BUSY unless the send queue has a place for it and the bucket holds
//! the ACK, the airtime of everything queued with the new frame, and the
//! reply reserve (§9.2.4, roadmap D29). So the queue never holds more than
//! the bucket can send now, and a packet waits in it for the hand-overs
//! ahead of it, not for the bucket to refill. A delivery is refused when
//! the delivery queue is full, or when the bucket holds less than the
//! reply reserve and an ACK (`tables/budget.rs`). Otherwise the ACK is
//! owed before acting. A packet that gives no work (cases 2, 3 and 5) is
//! ACKed from the reply reserve: this relay already has the message, and
//! BUSY would send its sender on with a second copy. A relay that cannot
//! afford even the reply stays silent, and its sender retries.
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
//!   ACKed whatever its record says: the record may have expired (188 s)
//!   well within the delivery TTL (10 min) the owner must remember it for
//!   (§9.2.7).
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
//! When listen-before-talk gives up on a forward's neighbour (§9.3 step
//! 2), the neighbour is set aside for this hand-over only (`skipped`):
//! it was not handed the message, so it stays a candidate for later
//! choices, but choosing again at once must not pick it again. A return,
//! a send-back and a DELIVER have nowhere else to go; for them a give-up
//! counts as one of the three attempts, and never as a failure against
//! the neighbour.
//!
//! A BUSY from the receiver of the front hand-over, for its message, is
//! handled as that give-up (§9.3, roadmap D27, D30), with two
//! differences: the receiver is also marked busy for [`BUSY_HOLD`], so
//! other messages take it only as the last resort, and since the frame
//! went out, the record's mark that the receiver was handed the message
//! is taken back. A return, a
//! send-back or a DELIVER refused this way has used one of its attempts.
//!
//! [`Transfer`]: crate::node::transfer::Transfer

use super::{Job, Relay};
use crate::event::{Action, AppEvent, DropReason, FailReason, Frame, Trace, MAX_FRAME};
use crate::ids::{LeafId, MsgId};
use crate::node::mac;
use crate::node::transfer::{Check, Missed, Reply, Step};
use crate::position::{Position, ShortName};
use crate::radio::{time_on_air, Channel, MIN_PREAMBLE_SYMBOLS};
use crate::tables::budget::{BUSY_AIRTIME, BUSY_HOLD, HEARTBEAT_RESERVE, REPLY_RESERVE};
use crate::tables::delivery::{Pushed, Queued};
use crate::tables::neighbours::Neighbour;
use crate::tables::records::{Record, MAX_HOPS};
use crate::tables::send_queue::{Kind, Outgoing};
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

    /// Why the send queue cannot take a frame of `airtime` after this
    /// relay spends `spend` (the ACK that takes the packet), if it cannot:
    /// it needs a place, and everything queued, the frame included, must
    /// fit in the bucket now with the reply reserve left (§9.2.4, §9.3).
    /// `freed`: the front's hand-over ends first, giving up its place.
    pub(super) fn no_room(
        &self,
        airtime: Duration,
        spend: Duration,
        freed: bool,
        now: Instant,
    ) -> Option<DropReason> {
        let front = match (freed, self.queue.front()) {
            (true, Some(o)) => mac::airtime(&o.frame),
            _ => Duration::ZERO,
        };
        let queued = self.queue.airtime() - front;
        if self.queue.is_full() && !freed {
            Some(DropReason::SendQueueFull)
        } else if self.budget.available(now) < queued + airtime + spend + REPLY_RESERVE {
            Some(DropReason::Budget)
        } else {
            None
        }
    }

    /// Whether a reply may spend `airtime` now: it must leave the
    /// heartbeat reserve (§9.4).
    pub(super) fn can_reply(&self, airtime: Duration, now: Instant) -> bool {
        self.budget
            .would_allow_keeping(airtime, HEARTBEAT_RESERVE, now)
    }

    fn drop_msg(&mut self, msg_id: MsgId, reason: DropReason, emit: &mut dyn FnMut(Action)) {
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
    /// live neighbour nearest `dst` that is not in the tried set and not
    /// set aside, ties broken by short name; a demoted one only when no
    /// other is left (roadmap D25), and a busy one only after those (D27).
    fn next_hop(&self, record: &Record, dst: Position, now: Instant) -> Option<(usize, Neighbour)> {
        let target = self.plane.point(dst);
        let parent = record.parent().map(|(name, _)| name);
        let skipped = self.skipped;
        let demoted = |n: &Neighbour| self.neighbours.is_demoted(n, now);
        let busy = |n: &Neighbour| self.neighbours.is_busy(n, now);
        self.neighbours
            .live(now)
            .filter(|(slot, n)| {
                !record.handed(*slot)
                    && skipped & (1 << slot) == 0
                    && Some(n.position.short()) != parent
            })
            .min_by(|(_, a), (_, b)| {
                let (da, db) = (a.local.distance2(target), b.local.distance2(target));
                busy(a)
                    .cmp(&busy(b))
                    .then(demoted(a).cmp(&demoted(b)))
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
        let case =
            if self.delivered.contains(id, now) || self.delivery.iter().any(|q| q.msg_id == id) {
                Case::AckOnly
            } else {
                self.classify(&d, id, from_leaf, now)
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
        let refusal = if case == Case::AckOnly {
            None
        } else if sends {
            self.no_room(data_airtime(d.payload.len()), ack_airtime(), completes, now)
        } else if for_leaf && self.delivery.is_full() {
            Some(DropReason::DeliveryFull)
        } else if !self.affordable(ack_airtime(), now) {
            Some(DropReason::Budget)
        } else {
            None
        };
        if let Some(reason) = refusal {
            emit(Action::Trace(Trace::Refused { msg_id: id, reason }));
            if self.can_reply(BUSY_AIRTIME, now) {
                let busy = Reply::Busy(Busy::refusing(self.me(), id));
                self.replies.owe(busy, d.sender_home, deadline, now);
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
        self.replies
            .owe(Reply::Ack(ack), d.sender_home, deadline, now);

        match case {
            Case::New => {
                let parent = (!from_leaf).then_some((d.src_hop, d.sender_home));
                let mut record = Record::new(id, parent, d.hops, now);
                if owner {
                    record.set_owned();
                }
                self.keep(record, now, emit);
                if owner {
                    self.arrive(id, d.dst_pos, d.dst_leaf, d.payload, now, emit);
                } else {
                    self.send_on(&d, Kind::Forward { report: false }, emit);
                }
            }
            Case::AckOnly => {
                if let Some(r) = self.records.get_mut(id, now) {
                    r.received(d.hops, now);
                }
            }
            Case::FromLastChild => {
                if let Some(r) = self.records.get_mut(id, now) {
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
                    self.send_on(&d, Kind::Forward { report: false }, emit);
                }
            }
            Case::SendBack => {
                if let Some(r) = self.records.get_mut(id, now) {
                    r.received(d.hops, now);
                }
                self.send_on(&d, Kind::Bounce, emit);
            }
        }
    }

    /// The case of §9.2.4 for a DATA packet of a message this relay is not
    /// holding for delivery.
    fn classify(&self, d: &Data<'_>, id: MsgId, from_leaf: bool, now: Instant) -> Case {
        let Some(r) = self.records.get(id, now) else {
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
        self.queue.iter().any(|o| {
            matches!(o.kind, Kind::Forward { .. } | Kind::Return) && o.msg_id() == Some(id)
        })
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

    /// Keep a new record. One that it displaces from a full table takes the
    /// message's queued packets with it: without a record they cannot be
    /// routed on.
    fn keep(&mut self, record: Record, now: Instant, emit: &mut dyn FnMut(Action)) {
        let Some(victim) = self.records.insert(record, now) else {
            return;
        };
        let gone = victim.id();
        let routed = |o: &Outgoing| o.kind != Kind::Deliver && o.msg_id() == Some(gone);
        let front = self.queue.front().is_some_and(routed);
        let before = self.queue.len();
        self.queue.retain(|o| !routed(o));
        if self.queue.len() != before {
            self.feed_due = true;
            if front {
                self.handover.cancel();
                self.skipped = 0;
            }
            self.drop_msg(gone, DropReason::RecordLost, emit);
        }
    }

    /// Queue `d` onward: a forward, addressed later, or straight back to
    /// its sender. Carries the received count plus one (§9.2.6).
    fn send_on(&mut self, d: &Data<'_>, kind: Kind, emit: &mut dyn FnMut(Action)) {
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
            frame: data_frame(&out, channel),
            kind,
        };
        if self.queue.push(out).is_err() {
            self.drop_msg(id, DropReason::SendQueueFull, emit);
        }
    }

    /// The message has reached its owner, this relay (§9.2.7): deliver it
    /// to the application or queue it for the leaf. Returns whether it was.
    fn arrive(
        &mut self,
        id: MsgId,
        dst: Position,
        leaf: LeafId,
        payload: &[u8],
        now: Instant,
        emit: &mut dyn FnMut(Action),
    ) -> bool {
        if leaf == LeafId::RELAY {
            if dst != self.config.position {
                self.drop_msg(id, DropReason::RelayGone, emit);
                return false;
            }
            self.delivered.insert(id, now);
            let mut buf = [0; MAX_FRAME];
            buf[..payload.len()].copy_from_slice(payload);
            emit(Action::App(AppEvent::Message {
                msg_id: id,
                len: payload.len() as u8,
                payload: buf,
            }));
            return true;
        }
        let Some(q) = Queued::new(id, leaf, payload, now) else {
            self.drop_msg(id, DropReason::PayloadTooLong, emit);
            return false;
        };
        match self.delivery.push(q) {
            Pushed::Duplicate => return true,
            Pushed::Full => {
                // Only when ownership is found at the hand-over, too late
                // to refuse (§9.2.4 case 4).
                self.drop_msg(id, DropReason::DeliveryFull, emit);
                return false;
            }
            Pushed::Queued => self.feed_due = true,
        }
        emit(Action::Trace(Trace::Queued { msg_id: id, leaf }));
        self.deliver_now(leaf, now);
        true
    }

    // --- The relay's own application (§9.2) ---

    /// `AppCommand::Send`: this relay is the origin.
    ///
    /// The application is the first sender and sends count 0 (§9.2.6), as
    /// a leaf does; this relay then sends 1, what it received plus one. It
    /// cannot put 0 on the air itself: a relay receiving count 0 takes the
    /// sender for a leaf and itself for the origin (§9.2.1). A message the
    /// relay already has a record of is in the network already; it is
    /// reported sent. So is one delivered or queued here (§9.2.7).
    pub(super) fn originate(
        &mut self,
        dst_pos: Position,
        dst_leaf: LeafId,
        payload: &[u8],
        now: Instant,
        emit: &mut dyn FnMut(Action),
    ) {
        let id = MsgId::of(dst_pos, dst_leaf, payload);
        let fail = |reason, emit: &mut dyn FnMut(Action)| {
            emit(Action::App(AppEvent::Failed { msg_id: id, reason }))
        };
        if payload.len() > MAX_PAYLOAD {
            fail(FailReason::PayloadTooLong, emit);
            self.drop_msg(id, DropReason::PayloadTooLong, emit);
            return;
        }
        emit(Action::App(AppEvent::Accepted(id)));
        if self.records.get(id, now).is_some() || self.delivered.contains(id, now) {
            emit(Action::App(AppEvent::Sent(id)));
            return;
        }
        if self.owns(dst_pos, now) {
            if dst_leaf != LeafId::RELAY && self.delivery.is_full() {
                fail(FailReason::QueueFull, emit);
                self.drop_msg(id, DropReason::DeliveryFull, emit);
                return;
            }
            let mut record = Record::new(id, None, 0, now);
            record.set_owned();
            self.keep(record, now, emit);
            if self.arrive(id, dst_pos, dst_leaf, payload, now, emit) {
                emit(Action::App(AppEvent::Sent(id)));
            } else {
                fail(FailReason::NoRoute, emit);
            }
            return;
        }
        let airtime = data_airtime(payload.len());
        if let Some(reason) = self.no_room(airtime, Duration::ZERO, false, now) {
            let f = match reason {
                DropReason::SendQueueFull => FailReason::QueueFull,
                _ => FailReason::Budget,
            };
            fail(f, emit);
            self.drop_msg(id, reason, emit);
            return;
        }
        self.keep(Record::new(id, None, 0, now), now, emit);
        let d = Data {
            sender_home: self.home,
            dst_hop: ShortName(0),
            src_hop: self.me(),
            dst_pos,
            dst_leaf,
            hops: 0,
            payload,
        };
        self.send_on(&d, Kind::Forward { report: true }, emit);
    }

    // --- The hand-over (§9.3) ---

    /// Make sure the front of the send queue has a hand-over: address a
    /// forward, or drop what cannot be sent.
    pub(super) fn prepare(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        while self.handover.idle() {
            let Some(out) = self.queue.front().copied() else {
                return;
            };
            let routed = match out.kind {
                Kind::Forward { report } => self.route(out, report, now, emit),
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
                    self.handover.start(expect, now);
                }
                Routed::Done => {
                    self.queue.pop();
                    self.skipped = 0;
                    self.feed_due = true;
                }
            }
        }
    }

    /// Address the forward `out` (§9.2.3), or return it to the parent
    /// (§9.2.5), or drop it at the origin. Ownership is checked again
    /// first: a closer neighbour may have been demoted since (§9.2.4 case 4).
    fn route(
        &mut self,
        out: Outgoing,
        report: bool,
        now: Instant,
        emit: &mut dyn FnMut(Action),
    ) -> Routed {
        let Ok(Packet::Data(d)) = Packet::decode(out.frame.payload()) else {
            return Routed::Done;
        };
        let id = d.msg_id();
        let fail = |reason, emit: &mut dyn FnMut(Action)| {
            if report {
                emit(Action::App(AppEvent::Failed { msg_id: id, reason }));
            }
        };
        let Some(record) = self.records.get(id, now).copied() else {
            fail(FailReason::NoRoute, emit);
            self.drop_msg(id, DropReason::RecordLost, emit);
            return Routed::Done;
        };
        if self.owns(d.dst_pos, now) {
            if let Some(r) = self.records.get_mut(id, now) {
                r.set_owned();
            }
            let delivered = self.arrive(id, d.dst_pos, d.dst_leaf, d.payload, now, emit);
            if report {
                emit(Action::App(if delivered {
                    AppEvent::Sent(id)
                } else {
                    AppEvent::Failed {
                        msg_id: id,
                        reason: FailReason::NoRoute,
                    }
                }));
            }
            return Routed::Done;
        }
        if let Some((_, n)) = self.next_hop(&record, d.dst_pos, now) {
            let to = n.position.short();
            let frame = data_frame(&Data { dst_hop: to, ..d }, n.home);
            return Routed::To(
                Outgoing {
                    frame,
                    kind: out.kind,
                },
                Ack {
                    msg_id: id,
                    from: to.into(),
                },
            );
        }
        match record.parent() {
            Some((parent, home)) => {
                if let Some(r) = self.records.get_mut(id, now) {
                    r.set_returned();
                }
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
                    },
                    Ack {
                        msg_id: id,
                        from: parent.into(),
                    },
                )
            }
            None => {
                fail(FailReason::NoRoute, emit);
                self.drop_msg(id, DropReason::NoRoute, emit);
                Routed::Done
            }
        }
    }

    /// Start listen-before-talk for the front hand-over if it is due and
    /// the budget lets it (§9.4: deferred, never dropped).
    pub(super) fn start_handover(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) -> bool {
        if !self.handover.due(now) {
            return false;
        }
        let Some(out) = self.queue.front().copied() else {
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
        // Everything queued waits with the front.
        for id in self.queue.iter().filter_map(|o| o.msg_id()) {
            if let Some(r) = self.records.get_mut(id, now) {
                r.held(now);
            }
        }
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
        if out.kind == Kind::Deliver {
            // The leaf listens on this relay's home, wherever that is now.
            frame.channel = self.home;
        }
        if let Ok(Packet::Data(d)) = Packet::decode(out.frame.payload()) {
            frame = data_frame(
                &Data {
                    sender_home: self.home,
                    ..d
                },
                out.frame.channel,
            );
            let id = d.msg_id();
            if let Some(r) = self.records.get_mut(id, now) {
                match out.kind {
                    Kind::Forward { .. } => {
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
            // Owed replies spent the budget since the check began.
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
            Kind::Forward { .. } => {
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
            self.neighbours.refused(position, now + BUSY_HOLD);
        }
        match out.kind {
            Kind::Forward { .. } => {
                if let Some((slot, _)) = slot {
                    self.skipped |= 1 << slot;
                    if let Some(r) = self.records.get_mut(msg_id, now) {
                        r.not_handed(slot);
                    }
                }
                self.handover.cancel();
            }
            Kind::Return | Kind::Bounce | Kind::Deliver => {
                let (max, retry) = (self.config.hop_attempts, self.config.retry_backoff);
                if self.handover.refused(now, max, &mut self.rng, retry) == Missed::Failed {
                    self.handover_failed(out, false, now, emit);
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
            Kind::Forward { report } => {
                if report {
                    emit(Action::App(AppEvent::Sent(id)));
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
            Kind::Forward { .. } => return,
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

    // --- Owed replies (§9.3) ---

    /// Start sending the first owed reply, if one is ready. Without budget
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
        w
    }
}
