//! Forwarding (spec "Forwarding"): the node's part of the depth-first search, the
//! owner rule, and the per-hop transfer of its send queue.
//!
//! A node owns a message addressed to its own ID wherever it is (spec
//! "Owner"). A message that reaches the owner of its position and is for
//! another node waits in the delivery queue until that node, a sleepy one
//! this node hears, listens (`relay/delivery.rs`); it goes to it as DATA
//! in the send queue, a hand-over like any other. A sleepy node holds mail
//! for nobody, so it owns no other node's message and forwards it (spec
//! "Sleepy nodes"); nor does a sleepy neighbour closer to a destination
//! keep the owner rule from holding here. The same holds for a carried
//! node, which also holds mail for nobody (`relay/motion.rs`).
//!
//! A sleepy neighbour with an anchor is a next hop only as a last resort,
//! where no other neighbour is left that could take the message but for
//! a ban, and every DATA to a sleepy neighbour carries the long preamble
//! (spec "Next hop").
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
//! an ACK that says so. A sleepy node's own message, count 0, is refused
//! too while every neighbour is banned (roadmap D34); one already in the
//! network is not, since the hop places keep nodes from locking up only
//! if it moves on.
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
//! A relay owes one reply at a time (`transfer::REPLIES`, roadmap D29). A
//! packet that arrives while it owes another one, its ACK still waiting
//! for the channel, is neither taken nor refused: the relay says nothing,
//! and the sender's retry brings the packet again. A copy of the packet
//! the owed reply answers is answered by it too.
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
//! # The send queue (spec "Send queue")
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
//! neighbour, or the heard table for a sleepy node it delivers to. A neighbour that
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
//! message elsewhere and offered it here. A return and a DATA to a sleepy node have
//! nowhere else to go; for them a give-up counts as one of the three
//! attempts, and never as a failure against the neighbour.
//!
//! An ACK that refuses the current hand-over bans its receiver for
//! [`BAN`], so no message takes it until then (spec "Bans", roadmap D27, D29,
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
//! is not returned or dropped: overload makes a message wait, never end;
//! only its time does (below).
//! Only a search exhausted by neighbours tried or set aside returns or
//! drops.
//! Waiting entries keep their airtime in admission, and the queue hands
//! over the first entry that is due. The application's own message waits
//! the same way, longer after refusals in a row ([`hold`]); one for a node
//! this one holds for that finds the delivery queue full is set aside ([`Held`]) until
//! the queue has a place (spec "Return to parent"). Either way the application is given no
//! other meanwhile.
//!
//! A sleepy node with an anchor hands its own message to the anchor
//! (spec "A sleepy node's own message", "Packet sender") with count 0, so
//! that the node that takes it is the origin of its search, takes it only
//! into its regular room and only while not every neighbour of its is
//! banned. Overload then stops a new message where it enters (roadmap D31,
//! D34), and the search never returns to a node that sleeps. A sleepy
//! node's own message that finds no live neighbour at all waits for one,
//! routed again every `fade_retry`, until its time runs out (below): a
//! node in a pocket loses its neighbours by going dark and finds them again.
//!
//! # Time left (spec "Time to live")
//!
//! A queued DATA keeps when its time runs out, from the packet it came in:
//! the time it carried less its time on air, which passed after the
//! sender stamped it. Each attempt carries what is left then, rounded
//! down to whole [`TTL_UNIT`]s. With no more than one unit left it cannot
//! be sent with any, and it is dropped wherever it waits, as soon as that
//! happens, so its place frees at once. Without an end by time a message
//! the network has taken lives as long as hand-overs to someone succeed,
//! which past the capacity domain's collapse was up to an hour (roadmap
//! D44). A packet that comes with its time gone is taken as no work, and
//! ends here. At its owner the delivery queue takes over.
//!
//! [`Transfer`]: crate::node::transfer::Transfer

use super::{Job, Node};
use crate::event::{Action, AppEvent, Delivery, DropReason, Frame, Trace, MAX_FRAME};
use crate::ids::{MsgId, NodeId};
use crate::node::mac;
use crate::node::transfer::{Check, Missed, Owed, Reply, Step};
use crate::position::Position;
use crate::radio::{time_on_air, Channel, MIN_PREAMBLE_SYMBOLS};
use crate::sniff::LONG_PREAMBLE_SYMBOLS;
use crate::tables::budget::{
    hold, told, wait_units, ACK_AIRTIME, BAN, HEARTBEAT_RESERVE, REPLY_RESERVE,
};
use crate::tables::delivery::{Pushed, Queued};
use crate::tables::neighbours::{Neighbour, Neighbours};
use crate::tables::records::{Record, HELD, MAX_HOPS};
use crate::tables::send_queue::{Kind, Outgoing, SEND_QUEUE};
use crate::time::{Duration, Instant};
use crate::wire::{Ack, Data, Packet, DATA_HEADER_LEN, MAX_PAYLOAD, TTL_UNIT};

pub use crate::node::OWNER_RADIUS_M;

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
    /// The application's message, for a node this one holds for, waits
    /// set aside for a place in the delivery queue.
    Hold,
    /// Every candidate left is banned: it waits in its place until then.
    Wait(Instant),
}

/// The application's message for a node this one holds for, set aside while
/// the delivery queue is full (roadmap D29). It keeps its place and
/// airtime in the send queue's admission.
#[derive(Clone, Copy, Debug)]
pub(super) struct Held {
    pub(super) out: Outgoing,
}

/// The frame of the owed reply. Its receiver is listening for it.
fn reply_frame(o: Owed) -> Frame {
    match o.reply {
        Reply::Ack(a) => mac::frame(&Packet::Ack(a), o.channel, MIN_PREAMBLE_SYMBOLS),
    }
}

/// `d` as a frame on `channel` with `preamble` symbols.
fn data_frame(d: &Data<'_>, channel: Channel, preamble: u16) -> Frame {
    mac::frame(&Packet::Data(*d), channel, preamble)
}

/// The preamble a DATA to `n` needs: the long one to a sleepy node, which
/// receives duty-cycled between its windows (spec "Sleepy nodes").
fn preamble(n: &Neighbour) -> u16 {
    if n.sleepy {
        LONG_PREAMBLE_SYMBOLS
    } else {
        MIN_PREAMBLE_SYMBOLS
    }
}

impl<const H: usize, const Q: usize> Node<H, Q> {
    /// This node's ID.
    pub(super) fn me(&self) -> NodeId {
        self.id
    }

    /// The ACK by which `from` takes this relay's offer of `msg_id`.
    fn expect(&self, msg_id: MsgId, from: NodeId) -> Ack {
        Ack {
            msg_id,
            from,
            to: self.me(),
            refused: false,
            wait: 0,
        }
    }

    /// How long a message of `record` to `dst` would wait here for its
    /// way on (spec "Bans"): the rest of the ban on the neighbour it
    /// would go to were no neighbour banned, which is the pushback that
    /// neighbour gave. Kept per neighbour, so a direction that pushes back
    /// does not hold up the others, and it ends with the ban.
    fn pushback(&self, record: &Record, dst: Position, now: Instant) -> Duration {
        self.preferred_hop(record, dst, now)
            .filter(|(_, n)| self.neighbours.is_banned(n, now))
            .map_or(Duration::ZERO, |(_, n)| n.banned_until.since(now))
    }

    /// The wait a refusal for `reason` asks for (spec "Bans"): for want
    /// of room, until the bucket holds everything queued, a frame of
    /// `airtime` and its ACK with the reply reserve left, if nothing else
    /// comes; anything else frees in one ban; and never less than the
    /// pushback the message's way on gave (`pushback`), so a wait told
    /// at the centre is passed back hop by hop toward the origin.
    ///
    /// Every sender refused takes a place in line: the next is told one
    /// ban later than the last, the time the bucket takes to refill the
    /// frame and ACK a sender will bring back. So the senders refused in
    /// one moment come back one after the other, as the room frees, and
    /// not all at once.
    fn refusal_wait(
        &mut self,
        reason: DropReason,
        airtime: Duration,
        pushback: Duration,
        now: Instant,
    ) -> Duration {
        let room = match reason {
            DropReason::Budget | DropReason::SendQueueFull => {
                let queued = self.queue.airtime() + self.held_airtime();
                self.budget
                    .allowed_at(airtime, queued + ACK_AIRTIME + REPLY_RESERVE, now)
                    .map_or(self.config.delivery_ttl, |t| t.since(now))
            }
            _ => BAN,
        };
        // Only the room is a line; a pushback passes on as it is, so it
        // cannot grow around a loop of relays.
        let slot = self.timers.promised.max(now + room);
        self.timers.promised = slot + BAN;
        slot.since(now).max(pushback)
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
    /// with the reply reserve left (spec "Receiving DATA", "Send queue"). Failing that, the
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

    /// Why a DATA to a sleepy node in its window, of `airtime`, cannot enter
    /// the regular room now, if it cannot (spec "Delivery queue"). It goes
    /// ahead of everything queued (spec "Delivery"), so it needs a place and
    /// its own frame in the bucket with the reply reserve left, not the
    /// airtime of what it goes ahead of.
    pub(super) fn no_room_to_deliver(&self, airtime: Duration, now: Instant) -> Option<DropReason> {
        if self.regular() >= SEND_QUEUE {
            Some(DropReason::SendQueueFull)
        } else if !self.affordable(airtime, now) {
            Some(DropReason::Budget)
        } else {
            None
        }
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

    /// Whether this relay owns the destination `dst` of another node's
    /// message (spec "Owner"). A sleepy node owns none (module
    /// documentation), nor does a carried one, nor one leaving after its
    /// grace on battery (`relay/power.rs`): they hold mail for nobody, and
    /// a neighbour that is carried keeps no other from owning.
    fn owns(&self, dst: Position, now: Instant) -> bool {
        if self.sleepy() || self.motion.carried || self.leaving() {
            return false;
        }
        if dst == self.config.position {
            return true;
        }
        if self.booting() {
            // Its table may not yet hold a relay within the radius that is
            // nearer (spec "Owner").
            return false;
        }
        let target = self.plane.point(dst);
        let mine = target.distance2(Default::default());
        mine <= OWNER_RADIUS_M * OWNER_RADIUS_M
            && !self
                .neighbours
                .candidates(now)
                .any(|(_, n)| n.holds_mail() && n.local.distance2(target) < mine)
    }

    /// Whether `n` is among the neighbours to choose from, `others_open`
    /// telling whether any that is no last resort is open: a sleepy
    /// neighbour with an anchor only where none is, banned or not (spec
    /// "Next hop"). A message whose other neighbours are banned waits for
    /// them (spec "Overload waits").
    fn tier(n: &Neighbour, others_open: bool) -> bool {
        n.last_resort() != others_open
    }

    /// Whether any neighbour that is no last resort is open for the
    /// message of `record` at `now` ([`Self::tier`]): live, neither handed
    /// it nor its parent, and a candidate or one but for a ban. One set
    /// aside for this hand-over and not banned, which listen-before-talk
    /// gave up on, is not; with `blocks` false neither counts.
    fn others_open(&self, record: &Record, blocks: bool, now: Instant) -> bool {
        let parent = record.parent().map(|(name, _)| name);
        self.neighbours.live(now).any(|(slot, n)| {
            !n.last_resort()
                && !record.handed(slot)
                && Some(n.id) != parent
                && (!blocks || self.skipped & (1 << slot) == 0 || self.neighbours.is_banned(n, now))
        })
    }

    /// The next hop for the message of `record` to `dst` (spec "Next hop"): the
    /// live neighbour nearest `dst` that is not in the tried set, not set
    /// aside and not banned, ties broken by ID; a demoted one only
    /// when no other with margin is left, and one without margin only when
    /// no other at all (roadmap D25, D29, D38).
    fn next_hop(&self, record: &Record, dst: Position, now: Instant) -> Option<(usize, Neighbour)> {
        self.hop(record, dst, true, now)
    }

    /// The neighbour [`Self::next_hop`] would choose for the message of
    /// `record` were none banned and none set aside: the way the message
    /// would go.
    fn preferred_hop(
        &self,
        record: &Record,
        dst: Position,
        now: Instant,
    ) -> Option<(usize, Neighbour)> {
        self.hop(record, dst, false, now)
    }

    /// [`Self::next_hop`], and with `blocks` false [`Self::preferred_hop`]:
    /// one choice, so that the way a message would go and the way it goes
    /// cannot part.
    fn hop(
        &self,
        record: &Record,
        dst: Position,
        blocks: bool,
        now: Instant,
    ) -> Option<(usize, Neighbour)> {
        let target = self.plane.point(dst);
        let parent = record.parent().map(|(name, _)| name);
        let skipped = if blocks { self.skipped } else { 0 };
        // Computed once: it takes a logarithm.
        let margin = Neighbours::margin_dbm(self.busy.noise_floor_dbm());
        let faint = |n: &Neighbour| n.rssi < margin;
        let demoted = |n: &Neighbour| self.neighbours.is_demoted(n, now);
        let first = self.first_hop(now).filter(|_| record.parent().is_none());
        let others = self.others_open(record, blocks, now);
        self.neighbours
            .live(now)
            .filter(|(slot, n)| {
                first.is_none_or(|a| n.id == a)
                    && (first.is_some() || Self::tier(n, others))
                    && !record.handed(*slot)
                    && skipped & (1 << slot) == 0
                    && Some(n.id) != parent
                    && (!blocks || !self.neighbours.is_banned(n, now))
            })
            .min_by(|(_, a), (_, b)| {
                let (da, db) = (a.local.distance2(target), b.local.distance2(target));
                faint(a)
                    .cmp(&faint(b))
                    .then(demoted(a).cmp(&demoted(b)))
                    .then(da.total_cmp(&db))
                    .then(a.id.0.cmp(&b.id.0))
            })
            .map(|(slot, n)| (slot, *n))
    }

    /// Where a sleepy node hands its own message: its anchor while that is
    /// live (spec "A sleepy node's own message"). Sent to
    /// the neighbour nearest the destination instead, the first hop was
    /// often a faint, far one, and at twice the spec "Capacity and sizing"
    /// rate the relays reached their cap and the domain collapsed
    /// (`docs/one-node-type-2026-10-16.md`).
    pub(super) fn first_hop(&self, now: Instant) -> Option<NodeId> {
        let a = self.anchor()?;
        self.neighbours
            .live(now)
            .any(|(_, n)| n.id == a.id)
            .then_some(a.id)
    }

    // --- Receiving (spec "Receiving DATA") ---

    /// A DATA packet addressed to this relay.
    pub(super) fn data(&mut self, d: Data<'_>, now: Instant, emit: &mut dyn FnMut(Action)) {
        let id = d.msg_id();
        // Count 0: a sleepy node's own message, fresh work that starts its
        // search here (spec "Packet sender", "Hop count").
        let fresh = d.hops == 0;
        let repeat = self
            .records
            .get(id)
            .is_some_and(|r| r.repeats(d.src_hop, d.hops));
        // A packet from the relay this one is handing the message to shows
        // that it took it, and ends that hand-over, which frees its place.
        let completes = !repeat && self.handing(id, d.src_hop);
        let case = self.classify(id, repeat);
        let copy = self
            .records
            .get(id)
            .filter(|r| !repeat && !r.arrived())
            .map(|r| {
                let returned = self
                    .neighbours
                    .slot_of(d.src_hop)
                    .is_some_and(|s| r.handed(s));
                (returned, case == Case::AckOnly)
            });
        let owner = case != Case::AckOnly && (d.dst_id == self.me() || self.owns(d.dst_pos, now));
        let sends = case != Case::AckOnly && !owner;
        // The time left was stamped as the frame went on air. Gone, the
        // packet is no work, and the message ends here.
        let expires = now + d.time_left() - self.data_airtime(d.payload.len());
        let expired = sends && now >= expires - TTL_UNIT;
        let deadline = now + self.config.ack_timeout;
        let for_leaf = owner && d.dst_id != self.me();
        let record_at = match case {
            Case::New => self.record_place(),
            _ => None,
        };
        let mut place = 0;
        // Fresh work is refused while the way it would go on from here
        // pushes back, and told how long (spec "Receiving DATA"). A message
        // already in the network is not: refused for that, its sender
        // would send it round the pushback, and around a ring of relays
        // the refusals kept each other up.
        let pushback = if case == Case::New && sends && fresh {
            let probe = Record::new(id, None, d.hops, now);
            self.pushback(&probe, d.dst_pos, now)
        } else {
            Duration::ZERO
        };
        let refusal = if case == Case::AckOnly || expired {
            None
        } else if case == Case::New && record_at.is_none() {
            Some(DropReason::NoRecord)
        } else if sends {
            let air = self.data_airtime(d.payload.len());
            match self.admit(air, ACK_AIRTIME, d.hops, completes, now) {
                Ok(_) if fresh && self.bans_end(now).is_some() => Some(DropReason::Banned),
                Ok(_) if pushback > Duration::ZERO => Some(DropReason::Banned),
                Ok(k) => {
                    place = k;
                    None
                }
                Err(why) => Some(why),
            }
        } else if for_leaf && !self.delivery.has_room(now) {
            Some(DropReason::DeliveryFull)
        } else if !self.affordable(ACK_AIRTIME, now) {
            Some(DropReason::Budget)
        } else {
            None
        };
        let air = self.data_airtime(d.payload.len());
        let wait = refusal.map_or(0, |why| {
            wait_units(self.refusal_wait(why, air, pushback, now))
        });
        let reply = Ack {
            msg_id: id,
            from: self.me(),
            to: d.src_hop,
            refused: refusal.is_some(),
            wait,
        };
        let sending = self.sending_reply();
        if !self.replies.can_owe(reply, d.sender_home, sending, now) {
            // The packet could not be answered; its sender's retry
            // brings it again (spec "The ACK", roadmap D29).
            return;
        }
        if let Some(reason) = refusal {
            emit(Action::Trace(Trace::Refused { msg_id: id, reason }));
            if self.can_reply(ACK_AIRTIME, now) {
                self.replies
                    .owe(reply, d.sender_home, deadline, false, sending, now);
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
        // A sleepy node listens on once it has answered, for the next
        // (spec "Delivery").
        let renews = d.dst_id == self.me();
        self.replies
            .owe(reply, d.sender_home, deadline, renews, sending, now);
        if completes {
            self.handover_done(now, emit);
        }
        if case == Case::New {
            let parent = (!fresh).then_some((d.src_hop, d.sender_home));
            let mut record = Record::new(id, parent, d.hops, now);
            if owner {
                record.set_arrived();
            }
            if let (true, Some(slot)) = (fresh, self.neighbours.slot_of(d.src_hop)) {
                // The sleepy node it came from has it, and would end the
                // search there: it is tried.
                record.handed_to(slot);
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
        if let Some((returned, merged)) = copy {
            emit(Action::Trace(Trace::Copy {
                msg_id: id,
                returned,
                merged,
            }));
        }
        match case {
            Case::AckOnly => {
                self.delivered.offered(id, now);
            }
            Case::New | Case::Search if owner => {
                self.arrive(id, &d, now, emit);
            }
            Case::New | Case::Search if expired => {
                self.drop_msg(id, DropReason::Expired, emit);
            }
            Case::New | Case::Search => self.send_on(&d, place, expires, emit),
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
    fn handing(&self, id: MsgId, to: NodeId) -> bool {
        !self.handover.idle()
            && self.handover.transmitted()
            && self.handover.expect() == self.expect(id, to)
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
    /// hand-over starts, whose time is gone at `expires`. Carries the
    /// received count plus one, which places it among waiting entries,
    /// until its hand-over stamps the count it is sent with (spec "Hop count").
    fn send_on(&mut self, d: &Data<'_>, place: u8, expires: Instant, emit: &mut dyn FnMut(Action)) {
        let id = d.msg_id();
        if d.hops == MAX_HOPS {
            self.drop_msg(id, DropReason::HopCap, emit);
            return;
        }
        let out = Data {
            sender_home: self.home,
            dst_hop: NodeId::NONE,
            src_hop: self.me(),
            hops: d.hops + 1,
            ..*d
        };
        let out = Outgoing {
            place,
            expires,
            ..Outgoing::new(
                data_frame(&out, self.home, self.forward_preamble()),
                Kind::Forward,
            )
        };
        if self.queue.push(out).is_err() {
            self.drop_msg(id, DropReason::SendQueueFull, emit);
        }
    }

    /// The message has reached its owner, this relay (spec "Owner"): deliver it
    /// to the application if it is for this node, or queue it for the
    /// node it is for, with the time it has left now.
    fn arrive(&mut self, id: MsgId, d: &Data<'_>, now: Instant, emit: &mut dyn FnMut(Action)) {
        let (node, payload) = (d.dst_id, d.payload);
        if self.own == Some(id) {
            self.set_own(None);
        }
        if node == self.me() {
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
        let Some(q) = Queued::new(id, d.dst_pos, node, payload, d.time_left, now) else {
            self.drop_msg(id, DropReason::PayloadTooLong, emit);
            return;
        };
        if !self.delivery.iter().any(|e| e.msg_id == id) {
            self.make_room(now, emit);
        }
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
        emit(Action::Trace(Trace::Queued { msg_id: id, node }));
        self.deliver_now(node, now);
    }

    // --- The relay's own application (spec "Routing") ---

    /// Whether the application may send now (roadmap D29 rule 6, D34): the
    /// relay is running, holds no message of the application's, its send
    /// queue takes a full frame on top of what it holds, as a sleepy node's
    /// would be taken but without the ACK, and not every neighbour is
    /// banned; for a sleepy node with a live anchor, not the anchor.
    pub(super) fn accepts_send(&self, now: Instant) -> bool {
        self.own.is_none()
            && now >= self.timers.retry_at
            && self
                .no_room(self.data_airtime(MAX_PAYLOAD), Duration::ZERO, now)
                .is_none()
            && self.bans_end(now).is_none()
    }

    /// When the application may send again, if only the bucket and bans
    /// keep it waiting; anything else ends with an event.
    pub(super) fn send_ready_at(&self, now: Instant) -> Option<Instant> {
        if self.own.is_some() || self.regular() >= SEND_QUEUE {
            return None;
        }
        let need = self.queue.airtime()
            + self.held_airtime()
            + self.data_airtime(MAX_PAYLOAD)
            + REPLY_RESERVE;
        let bucket = self
            .budget
            .allowed_at(Duration::ZERO, need, now)
            .filter(|&t| t > now);
        let retry = Some(self.timers.retry_at).filter(|&t| t > now);
        [bucket, self.bans_end(now), retry]
            .into_iter()
            .flatten()
            .max()
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
        if !self.delivery.has_room(now) {
            return;
        }
        self.held = None;
        if let Ok(Packet::Data(d)) = Packet::decode(h.out.frame.payload()) {
            self.arrive(d.msg_id(), &d, now, emit);
        }
    }

    /// `AppCommand::Send`: this relay is the origin. Offered only when
    /// [`Relay::accepts_send`]; anything else is the driver's bug and is
    /// ignored. The message is the application's until its first hop
    /// takes it (`own`).
    ///
    /// The application is the first sender and sends count 0 (spec "Hop count");
    /// this node then sends 1, what it received plus one. It cannot put 0
    /// on the air itself: a node receiving count 0 takes the sender for a
    /// sleepy node handing over its own and itself for the origin
    /// (spec "Packet sender"). A sleepy node with an anchor does exactly that
    /// (`count`). A message the
    /// relay already has a record of is in the network already, as is one
    /// delivered or queued here (spec "Owner").
    pub(super) fn originate(
        &mut self,
        dst_pos: Position,
        dst_id: NodeId,
        payload: &[u8],
        now: Instant,
        emit: &mut dyn FnMut(Action),
    ) {
        if payload.len() > MAX_PAYLOAD || !self.accepts_send(now) {
            return;
        }
        let id = MsgId::of(dst_pos, dst_id, payload);
        if self.records.get(id).is_some() || self.delivered.contains(id) {
            return;
        }
        // The table outnumbers what a relay holds (`tables/records.rs`).
        let Some(at) = self.record_place() else {
            return;
        };
        self.set_own(Some(id));
        // This node is the message's first holder: its time starts now.
        let ttl = self.config.delivery_ttl;
        let expires = now + ttl;
        let d = Data {
            sender_home: self.home,
            dst_hop: NodeId::NONE,
            src_hop: self.me(),
            dst_pos,
            dst_id,
            hops: 0,
            time_left: Data::units(ttl),
            payload,
        };
        if dst_id == self.me() || self.owns(dst_pos, now) {
            let mut record = Record::new(id, None, 0, now);
            record.set_arrived();
            self.records.put(at, record);
            if dst_id != self.me() && !self.delivery.has_room(now) {
                // At its owner: the delivery queue, not its time left,
                // ends it from here (spec "Delivery queue").
                let out = Outgoing::new(
                    data_frame(&d, self.home, self.forward_preamble()),
                    Kind::Forward,
                );
                self.held = Some(Held { out });
            } else {
                self.arrive(id, &d, now, emit);
            }
            return;
        }
        self.records.put(at, Record::new(id, None, 0, now));
        self.send_on(&d, 0, expires, emit);
    }

    // --- The hand-over (spec "Hand-over") ---

    /// Make sure the send queue's next due entry has a hand-over: address
    /// a forward, or drop what cannot be sent. The entry stays where it is
    /// in the queue.
    pub(super) fn prepare(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        self.make_way(now);
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
                    Ok(Packet::Data(d)) => Routed::To(out, self.expect(d.msg_id(), d.dst_hop)),
                    _ => Routed::Done,
                },
                // Stays in the delivery queue while its node sleeps.
                Kind::Deliver => match Packet::decode(out.frame.payload()) {
                    Ok(Packet::Data(d)) if self.listening(d.dst_hop, now) || self.early(&out) => {
                        Routed::To(out, self.expect(d.msg_id(), d.dst_hop))
                    }
                    Ok(Packet::Data(d)) => {
                        emit(Action::Trace(Trace::DeliveryWaits {
                            msg_id: d.msg_id(),
                            node: d.dst_hop,
                            tried: false,
                        }));
                        Routed::Done
                    }
                    _ => Routed::Done,
                },
                // Goes whether or not its node is in a window
                // (`relay/delivery.rs`).
                Kind::Direct => match Packet::decode(out.frame.payload()) {
                    Ok(Packet::Data(d)) => Routed::To(out, self.expect(d.msg_id(), d.dst_hop)),
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

    /// A DATA to a sleepy node goes first (spec "Delivery"): a hand-over
    /// that waits between its attempts, for the channel, a retry or the
    /// bucket, steps aside for it and keeps its place. A return keeps its
    /// unanswered attempts; a forward is addressed again when its turn
    /// comes, so it starts afresh.
    fn make_way(&mut self, now: Instant) {
        let waiting = matches!(self.handover.step(), Step::Wait(_)) && self.radio.idle();
        let Some(current) = self.queue.current().copied() else {
            return;
        };
        if !waiting || current.kind.delivers() || self.queue.delivery_due(now).is_none() {
            return;
        }
        let tries = match current.kind {
            Kind::Forward => 0,
            _ => self.handover.attempts(),
        };
        if let Some(c) = self.queue.current_mut() {
            c.tries = tries;
        }
        self.handover.cancel();
        self.skipped = 0;
    }

    /// Whether the delivery `out` goes outside its node's window, with the
    /// long preamble, to a node not known to be out of reach
    /// (`relay/delivery.rs`).
    fn early(&self, out: &Outgoing) -> bool {
        let to = match Packet::decode(out.frame.payload()) {
            Ok(Packet::Data(d)) => d.dst_hop,
            _ => return false,
        };
        out.frame.preamble_symbols >= LONG_PREAMBLE_SYMBOLS
            && self.heard.get(to).is_some_and(|n| !n.missed)
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
        if d.dst_id == self.me() || self.owns(d.dst_pos, now) {
            if let Some(r) = self.records.get_mut(id) {
                r.set_arrived();
            }
            if self.own == Some(id) && d.dst_id != self.me() && !self.delivery.has_room(now) {
                return Routed::Hold;
            }
            let time_left = Data::units(out.expires.since(now));
            self.arrive(id, &Data { time_left, ..d }, now, emit);
            return Routed::Done;
        }
        if let Some((_, n)) = self.next_hop(&record, d.dst_pos, now) {
            let to = n.id;
            let frame = data_frame(&Data { dst_hop: to, ..d }, n.home, preamble(&n));
            return Routed::To(Outgoing { frame, ..out }, self.expect(id, to));
        }
        if let Some(t) = self.ban_ends(&record, now) {
            if self.own == Some(id) {
                // Each offer costs the refuser a reply and hides its ACKs.
                self.own_refusals = self.own_refusals.saturating_add(1);
                return Routed::Wait(t.max(now + hold(self.own_refusals)));
            }
            return Routed::Wait(t);
        }
        if let Some(ends) = self.timers.boot_ends {
            if self.own == Some(id) && !self.neighbours.live(now).any(|(_, n)| n.relay()) {
                // Until it hears a relay, or its first rotation ends
                // (`heard`).
                return Routed::Wait(ends);
            }
        }
        let alone = self.neighbours.live(now).next().is_none();
        if self.sleepy() && self.own == Some(id) && alone {
            // Until its time runs out (`expire`).
            return Routed::Wait(now + self.config.fade_retry);
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
                    self.preamble_to(parent),
                );
                Routed::To(
                    Outgoing {
                        frame,
                        kind: Kind::Return,
                        ..out
                    },
                    self.expect(id, parent),
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
        if let Some((_, a)) = self.first_hop(now).and_then(|a| self.neighbours.by_id(a)) {
            return self.neighbours.is_banned(a, now).then_some(a.banned_until);
        }
        let others = self.neighbours.live(now).any(|(_, n)| !n.last_resort());
        let mut until: Option<Instant> = None;
        for (_, n) in self
            .neighbours
            .live(now)
            .filter(|(_, n)| Self::tier(n, others))
        {
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
        let others = self.others_open(record, true, now);
        self.neighbours
            .live(now)
            .filter(|(slot, n)| {
                Self::tier(n, others) && !record.handed(*slot) && self.neighbours.is_banned(n, now)
            })
            .map(|(_, n)| n.banned_until)
            .min()
    }

    /// The home channel of neighbour `id`, as its latest heartbeat or
    /// packet gave it.
    fn home_of(&self, id: NodeId) -> Option<Channel> {
        self.neighbours
            .by_id(id)
            .map(|(_, n)| n.home)
            .or_else(|| self.heard.get(id).map(|n| n.home))
    }

    /// The preamble of a DATA to `id`: the long one while the neighbour
    /// table has it sleepy, else the shortest.
    fn preamble_to(&self, id: NodeId) -> u16 {
        self.neighbours
            .get(id)
            .map_or(MIN_PREAMBLE_SYMBOLS, preamble)
    }

    /// The airtime of a DATA with `payload` bytes as this node's next hops
    /// need it, or as it came to this node: with the long preamble at an
    /// anchorless node, else the shortest.
    pub(super) fn data_airtime(&self, payload: usize) -> Duration {
        time_on_air((DATA_HEADER_LEN + payload) as u8, self.forward_preamble())
    }

    /// The unit of the retry wait of the current hand-over: one hand-over
    /// of a full frame with its preamble, so a hand-over with the long one
    /// waits by as much more as its frame is longer (spec "Retry wait").
    fn retry_unit(&self) -> Duration {
        let preamble = self
            .queue
            .current()
            .map_or(MIN_PREAMBLE_SYMBOLS, |o| o.frame.preamble_symbols);
        let longer = time_on_air(MAX_FRAME as u8, preamble)
            .as_micros()
            .saturating_sub(time_on_air(MAX_FRAME as u8, MIN_PREAMBLE_SYMBOLS).as_micros());
        self.config.retry_backoff + Duration::from_micros(longer)
    }

    /// Address the current frame as its receiver is now, before each attempt
    /// (spec "Hand-over"): a neighbour that moved home since the frame was queued, or
    /// since the last attempt, listens elsewhere, and one whose heartbeat
    /// has changed between sleepy and not changes its preamble. A receiver no longer in
    /// its table keeps the address it had.
    fn readdress(&mut self, now: Instant) -> Option<Outgoing> {
        let mut out = self.queue.current().copied()?;
        let (channel, preamble) = match Packet::decode(out.frame.payload()) {
            // Its preamble is chosen when it is queued.
            Ok(Packet::Data(d)) if out.kind == Kind::Direct => (self.home_of(d.dst_hop), None),
            Ok(Packet::Data(d)) if out.kind != Kind::Deliver => (
                self.home_of(d.dst_hop),
                self.neighbours.get(d.dst_hop).map(preamble),
            ),
            // Its holder moves its sleepy nodes with it (`take_listeners`);
            // one that listens now needs no long preamble.
            Ok(Packet::Data(d)) => (
                self.heard.get(d.dst_hop).map(|n| n.home),
                self.listening(d.dst_hop, now)
                    .then_some(MIN_PREAMBLE_SYMBOLS),
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
        let Some(out) = self.readdress(now) else {
            return false;
        };
        if !self.affordable(mac::airtime(&out.frame), now) {
            self.defer_handover(&out, now, emit);
            return false;
        }
        self.handover.begin_lbt(now);
        self.radio.check(Job::HandOver, out.frame.channel, emit);
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

    /// The listen-before-talk check of the current hand-over completed.
    pub(super) fn handover_check(
        &mut self,
        busy: bool,
        now: Instant,
        emit: &mut dyn FnMut(Action),
    ) {
        // The hand-over may have ended meanwhile (spec "Receiving DATA" case 5).
        if self.handover.step() != Step::Radio {
            return;
        }
        let Some(out) = self.queue.current().copied() else {
            return;
        };
        let backoff = self.config.hop_backoff_max;
        match self.handover.checked(busy, now, &mut self.rng, backoff) {
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
            // A DATA to a sleepy node keeps the time it arrived with: the
            // delivery queue, not its time left, ends it there.
            let time_left = match out.expires {
                Instant::MAX => d.time_left,
                ends => Data::units(ends.since(now)),
            };
            let (Some(hops), false) = (self.count(&d, id, out.kind), gone(&out, now)) else {
                let reason = match gone(&out, now) {
                    true => DropReason::Expired,
                    false => DropReason::HopCap,
                };
                self.handover.cancel();
                self.queue.take_current();
                self.skipped = 0;
                self.feed_due = true;
                self.drop_msg(id, reason, emit);
                return;
            };
            // Each attempt carries the time as it is when it goes, so a
            // retry never claims more than is left.
            frame = data_frame(
                &Data {
                    sender_home: self.home,
                    hops,
                    time_left,
                    ..d
                },
                out.frame.channel,
                out.frame.preamble_symbols,
            );
            if let Some(r) = self.records.get_mut(id) {
                r.sent(now);
            }
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
        self.radio.transmit(Job::HandOver, frame, emit);
    }

    /// The count the current hand-over of `d` carries (spec "Hop count"): stamped at
    /// its first transmission as the record's highest plus one, repeated by
    /// its retries. Stamped when queued, a frame would keep a count older
    /// than one this relay has since received or sent, and a relay that
    /// readdressed it would offer a count it had already used. `None` past
    /// the hop cap, but for a delivery: its search has ended, and the cap
    /// ends searches, so it goes at the cap.
    fn count(&self, d: &Data<'_>, id: MsgId, kind: Kind) -> Option<u8> {
        if self.handover.transmitted() {
            return Some(d.hops);
        }
        debug_assert!(
            self.records.get(id).is_some(),
            "a held message keeps its record"
        );
        match self.records.get(id) {
            // A sleepy node hands its own message over as fresh work: the
            // node that takes it is its origin (module documentation).
            Some(r) if self.anchored_sleepy() && self.own == Some(id) && r.parent().is_none() => {
                Some(0)
            }
            Some(r) if kind.delivers() => Some(r.highest_hops().saturating_add(1)),
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
                if let Some(slot) = self.neighbours.slot_of(to) {
                    self.skipped |= 1 << slot;
                }
            }
            Kind::Return | Kind::Deliver | Kind::Direct => {
                let (max, retry) = (self.config.hop_attempts, self.retry_unit());
                if self.handover.lost_attempt(now, max, &mut self.rng, retry) == Missed::Failed {
                    self.handover_failed(out, false, now, emit);
                }
            }
        }
    }

    /// An ACK arrived: if it answers the current hand-over, the receiver
    /// took the packet, or refused it (spec "The ACK", "Bans"); see the module documentation.
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
        let by = ack.from;
        emit(Action::Trace(Trace::TurnedAway { msg_id, by }));
        // As long as the refuser asks: its pushback, learned per neighbour
        // and ending with the ban (spec "Bans").
        let wait = told(ack.wait);
        let slot = self.neighbours.slot_of(by);
        if slot.is_some() {
            self.neighbours.answered(by, now);
            self.neighbours.refused(by, now + wait);
        }
        if self.sleepy() && self.own == Some(msg_id) && out.kind == Kind::Forward {
            // A sleepy node's own message goes back to its application,
            // which may offer it again once the wait is over: refused at
            // the origin, it does not age here (spec "A sleepy node's own message").
            self.handover.cancel();
            self.queue.take_current();
            self.skipped = 0;
            self.feed_due = true;
            self.timers.retry_at = now + wait;
            self.records.forget(msg_id);
            self.drop_msg(msg_id, DropReason::Pushback, emit);
            emit(Action::App(AppEvent::GivenBack {
                msg_id,
                retry_at: self.timers.retry_at,
            }));
            return;
        }
        match out.kind {
            Kind::Forward => {
                if let Some(slot) = slot {
                    self.skipped |= 1 << slot;
                }
                self.handover.cancel();
            }
            // No other receiver: wait for the ban, and let the queue move.
            Kind::Return | Kind::Deliver | Kind::Direct => {
                let tries = self.handover.refused();
                self.skipped = 0;
                if let Some(current) = self.queue.current_mut() {
                    current.not_before = now + wait;
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
        let Ok(Packet::Data(d)) = Packet::decode(out.frame.payload()) else {
            return;
        };
        let (id, to) = (d.msg_id(), d.dst_hop);
        if out.kind.delivers() {
            self.neighbours.answered(to, now);
            let long = out.frame.preamble_symbols >= LONG_PREAMBLE_SYMBOLS;
            let by = match (out.kind, long) {
                (Kind::Direct, true) => Delivery::Flush,
                (Kind::Direct, false) => Delivery::Relay,
                (_, true) => Delivery::Long,
                (_, false) => Delivery::Window,
            };
            return self.delivered_to(id, to, by, now, emit);
        }
        let slot = self.neighbours.slot_of(to);
        if slot.is_some() {
            self.neighbours.succeeded(to, now);
        }
        if let Some(r) = self.records.get_mut(id) {
            r.handed_over(d.hops);
            if let (Kind::Forward, Some(s)) = (out.kind, slot) {
                r.handed_to(s);
            }
            if to == d.dst_id {
                // The node it is addressed to took it: it has arrived.
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
            Kind::Return | Kind::Deliver | Kind::Direct => {
                Trace::ReturnedToParent { msg_id: id, to }
            }
        };
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
        let data = Packet::decode(out.frame.payload());
        if let (Ok(Packet::Data(d)), true) = (data, !out.kind.delivers()) {
            if counted {
                emit(Action::Trace(Trace::HandOverFailed {
                    msg_id: id,
                    to: d.dst_hop,
                }));
                self.count_failure(d.dst_hop, now, emit);
                if self.sleepy() && d.hops == 0 {
                    // A sleepy node's own message tries its anchor again a
                    // fade later, doubled for each silence in a row
                    // (spec "A sleepy node's own message"): the ban's ladder would
                    // offer it every few seconds to a node whose channel is
                    // full, and a fixed fade every minute for its whole TTL.
                    let ban = self.wait_for(d.dst_hop, now).unwrap_or(now);
                    let silences = self
                        .neighbours
                        .by_id(d.dst_hop)
                        .map_or(1, |(_, n)| n.fail_count.max(1));
                    let mut fade = self.config.fade_retry;
                    for _ in 1..silences {
                        fade = fade.mul(2).min(self.config.delivery_ttl);
                    }
                    self.skipped = 0;
                    if let Some(current) = self.queue.current_mut() {
                        current.not_before = ban.max(now + fade);
                        current.tries = 0;
                    }
                    return;
                }
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
                if let Some(slot) = self.neighbours.slot_of(d.dst_hop) {
                    self.skipped |= 1 << slot;
                }
            }
        }
        let reason = match out.kind {
            Kind::Forward => return,
            Kind::Return => Some(DropReason::ReturnFailed),
            Kind::Deliver | Kind::Direct => None,
        };
        if let (Ok(Packet::Data(d)), Kind::Direct) = (data, out.kind) {
            // Unanswered as a relay: again once it is heard again.
            self.direct_failed(d.dst_hop);
        }
        if let (Ok(Packet::Data(d)), Kind::Deliver) = (data, out.kind) {
            // Asleep: its messages wait for its next beat.
            self.heard.missed(d.dst_hop);
            emit(Action::Trace(Trace::DeliveryWaits {
                msg_id: id,
                node: d.dst_hop,
                tried: true,
            }));
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
    fn wait_for(&self, id: NodeId, now: Instant) -> Option<Instant> {
        let (_, n) = self.neighbours.by_id(id)?;
        (!self.neighbours.is_demoted(n, now)).then_some(n.banned_until)
    }

    fn count_failure(&mut self, id: NodeId, now: Instant, emit: &mut dyn FnMut(Action)) {
        let Some((_, n)) = self.neighbours.by_id(id) else {
            return;
        };
        let was = self.neighbours.is_demoted(n, now);
        if self.neighbours.failed(id, now) == Some(true) && !was {
            emit(Action::Trace(Trace::Demoted { neighbour: id }));
        }
    }

    // --- The owed reply (spec "The ACK") ---

    /// Start sending the owed reply, if it is ready. Without budget
    /// for it the relay cannot reply (spec "Duty cycle cap"); the reply is abandoned.
    pub(super) fn start_reply(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) -> bool {
        let Some(o) = self.replies.due(now) else {
            return false;
        };
        let frame = reply_frame(o);
        if !self.can_reply(mac::airtime(&frame), now) {
            self.replies.abandon();
            return true;
        }
        self.radio.check(Job::Reply, o.channel, emit);
        true
    }

    /// Whether the owed reply is on the radio.
    fn sending_reply(&self) -> bool {
        self.radio.step_of(Job::Reply).is_some()
    }

    pub(super) fn reply_check(&mut self, busy: bool, now: Instant, emit: &mut dyn FnMut(Action)) {
        let backoff = self.config.hop_backoff_max;
        if let Some(o) = self.replies.checked(busy, now, &mut self.rng, backoff) {
            let frame = reply_frame(o);
            let airtime = mac::airtime(&frame);
            if self.can_reply(airtime, now) {
                self.budget.record(airtime, now);
                self.radio.transmit(Job::Reply, frame, emit);
            } else {
                self.replies.abandon();
            }
        }
    }

    // --- Timers ---

    pub(super) fn forward_timers(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        // This runs on every event; only an ACK wait that ran out needs the
        // retry unit's two airtimes.
        if matches!(self.handover.step(), Step::AwaitAck(deadline) if now >= deadline) {
            let (max, retry) = (self.config.hop_attempts, self.retry_unit());
            if self.handover.timeout(now, max, &mut self.rng, retry) == Some(Missed::Failed) {
                if let Some(out) = self.queue.current().copied() {
                    self.handover_failed(out, true, now, emit);
                }
            }
        }
        self.expire(now, emit);
    }

    /// Drop every message in the send queue whose time is gone (spec "Time to live"):
    /// it could no longer be sent with a whole unit left. Waiting for a
    /// ban, in a hop place or a return alike; it frees its place at once
    /// rather than when its turn comes. One whose frame is on air or waits
    /// for its ACK ends at its next attempt instead (`transmit_current`).
    /// A message set aside for the delivery queue is at its owner, where
    /// that queue ends it.
    fn expire(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        if self.time_gone_at().is_none_or(|t| t > now) {
            return;
        }
        loop {
            let under_way = self.under_way();
            let found = self
                .queue
                .iter()
                .enumerate()
                .position(|(i, o)| Some(i) != under_way && gone(o, now));
            let Some(i) = found else { break };
            let id = self.queue.iter().nth(i).and_then(Outgoing::msg_id);
            if self.queue.current_index() == Some(i) {
                // Not yet on air: waiting its turn, for the bucket or to retry.
                self.handover.cancel();
                self.skipped = 0;
            }
            let mut k = 0;
            self.queue.retain(|_, _| {
                k += 1;
                k != i + 1
            });
            self.feed_due = true;
            self.drop_msg(id.unwrap_or_default(), DropReason::Expired, emit);
        }
    }

    /// The send queue's entry whose frame is on air or waiting for its ACK.
    fn under_way(&self) -> Option<usize> {
        let on_air = self.handover.awaiting_ack()
            || self.radio.step_of(Job::HandOver) == Some(mac::Step::Tx);
        self.queue.current_index().filter(|_| on_air)
    }

    /// When the earliest message in the send queue, but for one on air or
    /// waiting for its ACK, can no longer be sent with a whole unit of time
    /// left.
    fn time_gone_at(&self) -> Option<Instant> {
        // Asked on every event; mostly there is nothing queued.
        if self.queue.is_empty() {
            return None;
        }
        let under_way = self.under_way();
        self.queue
            .iter()
            .enumerate()
            .filter(|(i, _)| Some(*i) != under_way)
            .map(|(_, o)| o.expires)
            .min()
            .filter(|&t| t != Instant::MAX)
            .map(|t| t - TTL_UNIT)
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
        if let Some(t) = self.time_gone_at() {
            at(t);
        }
        w
    }
}

/// Whether `out`'s time is gone: no more than one unit is left, and a
/// DATA carries only whole ones, rounded down (spec "Time to live").
fn gone(out: &Outgoing, now: Instant) -> bool {
    out.expires != Instant::MAX && now >= out.expires - TTL_UNIT
}
