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
//! [`Relay::data`] classifies the packet by the six cases of spec "Receiving DATA" in
//! order and decides whether acting on it is work: sending it on, which
//! may be a forward or a return, or a delivery. Work to send on first
//! passes the gate ([`Relay::gate`]): it is routed as it would be from
//! here now, and so is every message queued, and it is refused if those
//! that go to the same next hop take longer than [`HOP_WAIT`] to go, by
//! that neighbour's service time, and one has a count as high or higher,
//! told when its place in that neighbour's line will be within the budget
//! (spec "Send queue", "Bans"). It then goes into the
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
//! owed before acting. A packet that gives no work (cases 1, 3, 4 and 6)
//! is ACKed from the reply reserve: the message has arrived, the packet
//! was answered already, a copy held here goes on, or the copy is seen
//! (below), and a refusal would send its sender on with a second copy. A
//! relay that cannot afford even the reply stays silent, and its sender
//! retries.
//!
//! A relay owes one reply at a time (`transfer::REPLIES`, roadmap D29). A
//! packet that arrives while it owes another one, its ACK still waiting
//! for the channel, is neither taken nor refused: the relay says nothing,
//! and the sender's retry brings the packet again. A copy of the packet
//! the owed reply answers is answered by it too.
//!
//! The ACK goes at once, without listening first, and the take is final
//! only when it has gone out: the reply carries the DATA, and the relay
//! acts on it at the ACK's `TxDone` ([`Relay::taken`]); an ACK that
//! cannot go out, for want of budget, leaves the packet untaken,
//! reported as refused for `DropReason::Unanswered`, and the sender's
//! retry brings it again. Before D93 the relay acted on reception and
//! the ACK listened first, so an ACK that lapsed on a busy channel left
//! a copy the sender did not know of (spec "The ACK", roadmap D93).
//!
//! A packet is ACKed without being acted on only where that cannot end the
//! last copy (roadmap D35). A copy from a node this relay did not hand
//! the message to, while it holds none, is answered seen (case 6, roadmap
//! D90): the ACK's seen flag says the relay has the record and no copy,
//! and the sender keeps the copy, marks this relay tried and goes on, so
//! a split's copies do not start the search again at every relay they
//! reach, and no copy ends. The relay counts the sender as tried too: it
//! has the message, and searches on from its own record. The same packet
//! again, its seen answer lost, is answered seen again. A return, from a
//! node in the record's `handed` set or the one being handed to, is taken
//! on with this relay's own search (case 5), as is a copy for a
//! destination this relay owns now. A hand-over under way whose frame has
//! been on air does not count as a copy held here: its receiver may have
//! taken it, the ACK been lost, and the copy now offered be that one come
//! back. A message delivered here, or waiting in the delivery queue, is only
//! ACKed whatever its record says: the delivered window and the delivery
//! queue remember it, and its record may have made way for newer ones
//! (spec "Owner").
//!
//! # The send queue (spec "Send queue")
//!
//! Every packet this relay sends for a message goes through the send
//! queue, one hand-over in flight at a time ([`IN_FLIGHT`]); each place
//! carries its own hand-over ([`HandOver`]). A forward is addressed
//! when its hand-over starts, by the one next-hop function of spec "Next hop",
//! from the state as it is then; with no candidate left it becomes a
//! return to the parent, or at the origin a drop. Nothing of the choice
//! is kept once the hand-over ends: the next routing makes it afresh.
//! The hop count a packet
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
//! the message, taking it or having seen it (roadmap D36, D90). A seen
//! ACK says the neighbour has the message's record and no copy of it:
//! the forward is routed again at once from the record, past that
//! neighbour, by the same choice a return taken from a child gets; a
//! return answered seen is dropped, its parent having gone on without
//! it. A neighbour that failed a hand-over stays untried: an unanswered
//! frame is more often lost than its ACK, so the neighbour most likely
//! lacks the message; marked tried, it would end its own search if it
//! later got the message elsewhere and offered it here.
//!
//! A busy channel moves nothing (spec "Hand-over" step 2, roadmap D96).
//! When listen-before-talk gives up on a forward's neighbour, the forward
//! keeps the neighbour and its transmissions so far, waits a retry wait
//! and listens again: the channel says nothing about the neighbour, and
//! after a transmission the neighbour may hold the message, so offering
//! it to the next candidate would split it. A return and a DATA to a
//! sleepy node have nowhere else to go; for them a give-up counts as one
//! of the three attempts, and never as a failure against the neighbour.
//!
//! An ACK that refuses the current hand-over bans its receiver for the
//! wait it asks, at least [`BAN`], so no message takes it until then
//! (spec "Bans", roadmap D27, D29, D32). The refused message stays in its
//! place, and since a ban is a wait and not an exclusion (spec "Next hop")
//! the refuser is still its next hop: it waits for the ban and is
//! offered to the same neighbour again, a return as a forward, the
//! refused transmission not counting as an attempt. The neighbour
//! answers, and going round it would take the message the long way past
//! a relay that has said when it will have room.
//!
//! A hand-over whose attempts all go unanswered bans its receiver too,
//! longer after each failure in a row (`tables/neighbours.rs`, roadmap
//! D42): silence says the channel or the receiver is full, and offering
//! it the next message at once only adds frames there. The failed
//! forward waits for the ban and goes to the same neighbour again; a
//! return waits the same way. Going to the next candidate at once would
//! send the message the long way round just when the channel is full;
//! measured, it undoes the ban's gain (roadmap D42).
//!
//! Refused or failed, a forward waits for its neighbour while the
//! neighbour ranks first, however its ban has grown meanwhile; once it is
//! demoted it ranks behind the others and the message goes elsewhere,
//! and a return is dropped once its parent is demoted or gone (roadmap
//! D102, D106). The demotion, failed hand-overs in a row with no answer
//! to this relay between ([`DEMOTE_AFTER_FAILURES`]), is the one test
//! that the neighbour no longer hears this relay, and the neighbour table
//! holds it for every message alike: the message keeps nothing of it.
//!
//! A forward whose next hop is banned waits in its place until the ban
//! ends, and is routed again then (roadmap D31). It is not returned or
//! dropped: overload makes a message wait, never end (roadmap D106,
//! spec "Overload waits"). Only a search exhausted by neighbours tried
//! returns or drops.
//! Waiting entries keep their airtime in admission, and the queue hands
//! over the first entry that is due. The application's own message waits
//! the same way, longer after refusals in a row ([`hold`]); one for a node
//! this one holds for that finds the delivery queue full is set aside in
//! its place ([`State::Held`]) until the queue has a place
//! (spec "Return to parent"). Either way the application is given no
//! other meanwhile.
//!
//! A sleepy node with an anchor hands its own message to the anchor
//! (spec "A sleepy node's own message", "Packet sender") with count 0, so
//! that the node that takes it is the origin of its search, takes it only
//! into its regular room and only while not every neighbour of its is
//! banned. Overload then stops a new message where it enters (roadmap D31,
//! D34), and the search never returns to a node that sleeps. A sleepy
//! node's own message that finds no live neighbour at all waits for one,
//! routed again every `fade_retry`: a node in a pocket loses its
//! neighbours by going dark and finds them again.
//!
//! No message ends by age on its way (spec "Time to live", roadmap D106):
//! the refusals that hold it carry the load back to its origin instead.
//! The DATA's `time_left` byte is passed on as it came.
//!
//! [`HandOver`]: crate::node::transfer::HandOver
//! [`DEMOTE_AFTER_FAILURES`]: crate::tables::DEMOTE_AFTER_FAILURES
//! [`IN_FLIGHT`]: crate::node::transfer::IN_FLIGHT

use super::{Job, Node};
use crate::event::{Action, AppEvent, Delivery, DropReason, Frame, Trace, MAX_FRAME};
use crate::ids::{MsgId, NodeId};
use crate::node::transfer::{self, Check, Flight, HandOver, Missed, Owed, Reply, IN_FLIGHT};
use crate::node::Rng;
use crate::node::{heartbeat_interval, mac};
use crate::position::Position;
use crate::radio::{time_on_air, Channel, MIN_PREAMBLE_SYMBOLS};
use crate::sniff::LONG_PREAMBLE_SYMBOLS;
use crate::tables::budget::{
    hold, told, told_up, wait_units, ACK_AIRTIME, BAN, HEARTBEAT_RESERVE, LONGEST_TOLD,
    REPLY_RESERVE,
};
use crate::tables::delivery::{Pushed, Queued};
use crate::tables::neighbours::{Neighbour, Neighbours};
use crate::tables::records::{Record, HELD, MAX_HOPS};
use crate::tables::send_queue::{Kind, Outgoing, State, HOP_PLACES, HOP_WAIT, SEND_QUEUE};
use crate::time::{earlier, Duration, Instant};
use crate::wire::{Ack, Data, Packet, DATA_HEADER_LEN, MAX_PAYLOAD};

pub use crate::node::OWNER_RADIUS_M;

/// What a received DATA packet is (spec "Receiving DATA").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Case {
    /// Case 2: no record.
    New,
    /// Cases 1, 3 and 4: arrived, a retransmission of a packet taken, or a
    /// copy while this relay holds one. ACK only.
    AckOnly,
    /// Case 5: a return, or a copy for a destination this relay owns now,
    /// and it holds none. It goes on with its own search.
    Search,
    /// Case 6: a copy from a node this relay did not hand the message to,
    /// and it holds none; or a retransmission of such a copy. Answered
    /// seen: the copy stays with its sender.
    Seen,
}

/// What a received DATA gives this relay to do, and whether it takes or
/// refuses it (spec "Receiving DATA"), decided before the reply and acted
/// on after it where the take waits for the ACK.
#[derive(Clone, Copy, Debug)]
struct Verdict {
    case: Case,
    /// The sender's neighbour-table slot.
    slot: Option<usize>,
    /// For a copy of a message the relay has a record of: whether it came
    /// from a node that has the message from this relay, whether it
    /// merges with a copy held here, and whether it is answered seen.
    copy: Option<(bool, bool, bool)>,
    /// This relay owns the destination, and acts on the packet.
    owner: bool,
    /// A sleepy node's own message, count 0, whose search starts here.
    fresh: bool,
    /// Where a new record goes.
    record_at: Option<usize>,
    /// The send-queue place it takes; 0 the regular room.
    place: u8,
    /// Why it is refused, if it is.
    refusal: Option<DropReason>,
    /// The pushback its way on gave, for the wait a refusal tells.
    pushback: Duration,
    /// Its way on, for a refusal's place in that way's line.
    way: Option<NodeId>,
    /// What the gate found on that way.
    gate: Option<Gate>,
}

/// What the gate finds on a message's way on (spec "Send queue").
#[derive(Clone, Copy, Debug)]
struct Gate {
    /// How long the messages queued here for the way take to go, by its
    /// service time: the message's wait here before its hand-over.
    queue: Duration,
    /// When the longest waiting of them began to wait for the way.
    oldest: Option<Instant>,
    /// The wait passes [`HOP_WAIT`] and one of them has come as far or
    /// farther: the message is refused.
    over: bool,
}

impl Verdict {
    /// Whether the packet is work: sent on or delivered (cases 2 and 5).
    fn acts(&self) -> bool {
        matches!(self.case, Case::New | Case::Search)
    }
}

/// What became of a forward when it was addressed. Short-lived, so the
/// frame it carries by value costs nothing.
#[allow(clippy::large_enum_variant)]
enum Routed {
    /// Addressed: send this frame, whose receiver its ACK names.
    To(Outgoing),
    /// Dropped or delivered here; it leaves the queue.
    Done,
    /// The application's message, for a node this one holds for, waits
    /// set aside for a place in the delivery queue.
    Hold,
    /// It waits in its place until then, for the end of the first
    /// rotation or for a neighbour to be heard.
    Wait(Instant),
    /// Its next hop, or its parent, is banned until then: routed again
    /// once the ban ends.
    Banned(Instant),
}

/// Whether neighbour-table `slot` is tried for the message of `record`:
/// it has the message from this relay, or the queued entry held for it
/// through its demotion (`passed`, spec "Next hop").
fn tried(record: &Record, passed: u64, slot: usize) -> bool {
    record.handed(slot) || passed & (1u64 << slot) != 0
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

/// What forwarding keeps beside the send queue: the one hand-over in
/// flight, and whether the send queue may have room for the delivery
/// queue.
#[derive(Clone, Copy, Debug)]
pub(super) struct Forwarding {
    /// What the radio side keeps of the hand-over in flight, that of the
    /// send queue's current entry.
    pub(super) flight: Flight,
    /// The send queue may have room for the delivery queue: a hand-over
    /// ended or the delivery queue changed.
    pub(super) feed_due: bool,
    /// When the send queue was last looked at with nothing to send: the
    /// earliest end of a wait or a ban that held an entry back.
    pub(super) wake: Option<Instant>,
}

impl Forwarding {
    pub(super) const IDLE: Forwarding = Forwarding {
        flight: Flight::NONE,
        feed_due: false,
        wake: None,
    };
}

/// The application's own message, from when it is taken until its first
/// hop takes it, it arrives here, or it is dropped (roadmap D29).
#[derive(Clone, Copy, Debug)]
pub(super) struct OwnMessage {
    pub(super) id: Option<MsgId>,
    /// Refusals in a row by banned neighbours: each doubles the hold
    /// before it is offered again (`budget::hold`).
    pub(super) refusals: u8,
}

impl OwnMessage {
    pub(super) const NONE: OwnMessage = OwnMessage {
        id: None,
        refusals: 0,
    };
}

impl<const H: usize, const Q: usize> Node<H, Q> {
    /// This node's ID.
    pub(super) fn me(&self) -> NodeId {
        self.id
    }

    /// The ACK by which `from` takes this relay's offer of `msg_id`.
    fn expect(&self, msg_id: MsgId, from: NodeId) -> Ack {
        transfer::expect(msg_id, from, self.me())
    }

    /// The send queue's current entry while its hand-over is under way.
    pub(super) fn in_flight(&self) -> Option<&Outgoing> {
        self.queue.current().filter(|o| o.in_flight())
    }

    /// Hand-overs under way, against [`IN_FLIGHT`].
    fn flights(&self) -> usize {
        usize::from(self.in_flight().is_some())
    }

    pub(super) fn handover_idle(&self) -> bool {
        self.in_flight().is_none()
    }

    /// Whether the hand-over under way listens at home for its ACK.
    pub(super) fn awaiting_ack(&self) -> bool {
        self.in_flight().is_some_and(|o| o.state == State::AwaitAck)
    }

    /// The hand-over of the send queue's current entry, to advance, with
    /// the random source its waits draw from.
    pub(super) fn handover(&mut self) -> Option<(HandOver<'_>, &mut Rng)> {
        let me = self.id;
        let out = self.queue.current_mut()?;
        Some((
            HandOver::of(out, &mut self.forwarding.flight, me),
            &mut self.rng,
        ))
    }

    /// Give the hand-over under way up: its entry is queued again.
    pub(super) fn cancel_handover(&mut self) {
        if let Some((mut h, _)) = self.handover() {
            h.cancel();
        }
    }

    /// The wait a refusal for `reason` asks for (spec "Bans"): for want
    /// of room, until the bucket holds everything queued, a frame of
    /// `airtime` and its ACK with the reply reserve left, if nothing else
    /// comes; for want of the way on, `way`, at the gate or as fresh work
    /// while it is banned, a place in that neighbour's line; anything
    /// else frees in one ban; and never less than the pushback the
    /// message's way on gave (`pushback`), the rest of its next hop's
    /// ban, so a wait told at the centre is passed back hop by hop
    /// toward the origin.
    ///
    /// Every sender refused for room takes a place in line: the next is
    /// told one ban later than the last, the time the bucket takes to
    /// refill the frame and ACK a sender will bring back. So the senders
    /// refused in one moment come back one after the other, as the room
    /// frees, and not all at once. A sender refused for the way on takes
    /// a place in that way's line instead, spaced by the time this relay
    /// takes to get one message through to it, so that the senders come
    /// back as that way frees; `gate` is what the gate found on the way.
    #[allow(clippy::too_many_arguments)]
    fn refusal_wait(
        &mut self,
        reason: DropReason,
        airtime: Duration,
        pushback: Duration,
        way: Option<NodeId>,
        gate: Option<Gate>,
        now: Instant,
        emit: &mut dyn FnMut(Action),
    ) -> Duration {
        if matches!(reason, DropReason::SameHop | DropReason::Banned) {
            if let Some(wait) = way.and_then(|to| self.line_wait(to, gate, pushback, now, emit)) {
                return wait;
            }
        }
        let room = match reason {
            DropReason::Budget | DropReason::SendQueueFull => {
                // Past the bucket's depth, what is queued goes as the
                // bucket refills, so the time is the refill of all of it.
                let queued = self.queue.airtime();
                let need = airtime + queued + ACK_AIRTIME + REPLY_RESERVE;
                self.radio.budget.refilled_at(need, now).since(now)
            }
            _ => BAN,
        };
        // Only the room is a line; a wait for the way on passes on as it
        // is, so it cannot grow around a loop of relays.
        let slot = self.timers.promised.max(now + room);
        self.timers.promised = slot + BAN;
        slot.since(now).max(pushback)
    }

    /// The wait told a sender refused for want of the way on, `to`: until
    /// its place in `to`'s line is [`HOP_WAIT`] away, rounded up to a wait
    /// an ACK tells so that it never comes back before it, when it would
    /// only cost a frame and a refusal; and no less than `pushback`, the
    /// rest of `to`'s ban, which stands outside the line (spec "Bans").
    /// `gate`: the queue for `to` here. `None` if `to` is not in the table.
    fn line_wait(
        &mut self,
        to: NodeId,
        gate: Option<Gate>,
        pushback: Duration,
        now: Instant,
        emit: &mut dyn FnMut(Action),
    ) -> Option<Duration> {
        let (queue, oldest) = gate.map_or((Duration::ZERO, None), |g| (g.queue, g.oldest));
        let limit = HOP_WAIT + LONGEST_TOLD;
        let (place, service) = self
            .neighbours
            .place_in_line(to, queue, oldest, limit, now)?;
        let wait = told_up(place - HOP_WAIT).max(pushback);
        emit(Action::Trace(Trace::InLine {
            neighbour: to,
            wait,
            service,
            at_cap: !self.affordable(self.data_airtime(MAX_PAYLOAD), now),
        }));
        Some(wait)
    }

    /// Where the message of `record` to `dst` goes if routed now, and
    /// when that one's ban ends: its next hop, or its parent once none is
    /// left (spec "Next hop", "Return to parent").
    fn heads_to(
        &self,
        record: &Record,
        passed: u64,
        avoid: u64,
        dst: Position,
        now: Instant,
    ) -> Option<(NodeId, Instant)> {
        if let Some((_, n)) = self.next_hop(record, passed, avoid, dst, now) {
            return Some((n.id, n.banned_until));
        }
        let (parent, _) = record.parent()?;
        let ban = self
            .neighbours
            .get(parent)
            .map_or(Instant::ZERO, |n| n.banned_until);
        Some((parent, ban))
    }

    /// Where queued entry `o` goes now: the neighbour of the hand-over
    /// under way, or where it would be routed; `None` for a delivery,
    /// one set aside, or one this relay owns now.
    fn bound_for(&self, o: &Outgoing, now: Instant) -> Option<NodeId> {
        if o.kind.delivers() || o.held() {
            return None;
        }
        let Ok(Packet::Data(d)) = Packet::decode(o.frame.payload()) else {
            return None;
        };
        if o.in_flight() {
            return Some(d.dst_hop);
        }
        self.heading(o, &d, now).map(|(to, _, _)| to)
    }

    /// Where queued forward `o`, carrying `d`, goes if routed now, when
    /// that one's ban ends, and where its record is: `None` if this relay
    /// owns its destination now, or it has no record.
    fn heading(&self, o: &Outgoing, d: &Data<'_>, now: Instant) -> Option<(NodeId, Instant, u16)> {
        if d.dst_id == self.me() || self.owns(d.dst_pos, now) {
            return None;
        }
        let (at, record) = self.records.find(d.msg_id(), o.record)?;
        self.heads_to(record, o.passed, o.avoided(), d.dst_pos, now)
            .map(|(to, ban)| (to, ban, at))
    }

    /// The entry the next hand-over goes to (spec "Send queue"): every
    /// queued forward is routed from the state as it is now, and one
    /// whose next hop is banned is not sendable until the ban ends; of
    /// the rest the queue's order chooses. With none to send, the
    /// earliest end of what holds them back is kept for the wakeup.
    /// At most the queue's entries times the neighbour table's entries
    /// per call, and no allocation.
    fn pick(&mut self, now: Instant) -> Option<usize> {
        const N: usize = SEND_QUEUE + HOP_PLACES;
        let mut banned = [false; N];
        let mut hints = [None::<u16>; N];
        let mut toward = [0u8; N];
        let mut wake: Option<Instant> = None;
        let still_banned = if cfg!(feature = "detour-silence") {
            self.banned_slots(now)
        } else {
            0
        };
        for (i, o) in self.queue.iter().enumerate() {
            if o.held() {
                continue;
            }
            if o.not_before > now {
                wake = earlier(wake, Some(o.not_before));
                continue;
            }
            if o.kind != Kind::Forward {
                continue;
            }
            let Ok(Packet::Data(d)) = Packet::decode(o.frame.payload()) else {
                continue;
            };
            if let Some((to, ban, at)) = self.heading(o, &d, now) {
                hints[i] = Some(at);
                toward[i] = self.neighbours.slot_of(to).map_or(0, |s| s as u8 + 1);
                if ban > now {
                    banned[i] = true;
                    wake = earlier(wake, Some(ban));
                }
            }
        }
        for i in 0..self.queue.len() {
            if let Some(o) = self.queue.get_mut(i) {
                o.record = hints[i].unwrap_or(o.record);
                o.waited |= banned[i];
                o.keep_avoiding(still_banned);
                // Routed elsewhere than before, it begins to wait for
                // the new way now, for that way's service time.
                if toward[i] != 0 && o.toward != toward[i] {
                    if o.toward != 0 {
                        o.since = now;
                    }
                    o.toward = toward[i];
                }
            }
        }
        let pick = self.queue.next_due(now, |i, _| !banned[i]);
        self.forwarding.wake = if pick.is_some() { None } else { wake };
        pick
    }

    /// The gate of the send queue (spec "Send queue"): what a message
    /// received with `hops`, other than `id`, going to `to`, finds there.
    /// Its wait here is the service time to `to` for each queued message
    /// that goes to `to` now; it is refused if that passes [`HOP_WAIT`]
    /// and its count is not higher than every one of theirs.
    ///
    /// The count of a queued message is one less than the count it is
    /// sent with: the count it holds here. At most the queue's entries
    /// times the neighbour table's entries per call, and no allocation.
    fn gate(&self, id: MsgId, to: NodeId, hops: u8, now: Instant) -> Gate {
        let (mut ahead, mut barred, mut oldest) = (0, false, None::<Instant>);
        for o in self.queue.iter().filter(|o| o.msg_id() != Some(id)) {
            if self.bound_for(o, now) == Some(to) {
                ahead += 1;
                barred |= o.hops().saturating_sub(1) >= hops;
                oldest = Some(oldest.map_or(o.since, |t| t.min(o.since)));
            }
        }
        let service = self
            .neighbours
            .service_now(to, oldest, now)
            .unwrap_or(Duration::ZERO);
        let queue = service.mul(ahead);
        Gate {
            queue,
            oldest,
            over: barred && queue > HOP_WAIT,
        }
    }

    /// Whether forwarding, or an ACK that takes on work, may spend
    /// `airtime` now: it must leave the reply reserve (spec "Duty cycle cap").
    pub(super) fn affordable(&self, airtime: Duration, now: Instant) -> bool {
        self.radio
            .budget
            .would_allow_keeping(airtime, REPLY_RESERVE, now)
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
        let queued = self.queue.airtime() - current_air;
        let regular = self.regular() - usize::from(current.is_some_and(|o| o.place == 0));
        let why = if regular >= SEND_QUEUE {
            DropReason::SendQueueFull
        } else if self.radio.budget.available(now) < queued + airtime + spend + REPLY_RESERVE {
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

    /// Entries in the regular room, one set aside for the delivery queue
    /// counting as one.
    fn regular(&self) -> usize {
        self.queue.regular()
    }

    /// Whether a reply may spend `airtime` now: it must leave the
    /// heartbeat reserve (spec "Duty cycle cap").
    pub(super) fn can_reply(&self, airtime: Duration, now: Instant) -> bool {
        self.radio
            .budget
            .would_allow_keeping(airtime, HEARTBEAT_RESERVE, now)
    }

    fn drop_msg(&mut self, msg_id: MsgId, reason: DropReason, emit: &mut dyn FnMut(Action)) {
        if self.own.id == Some(msg_id) {
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

    /// Whether `n` is a last resort at `now` (spec "Next hop"): a sleepy
    /// neighbour with an anchor, or a carried one not heard for its own
    /// heartbeat interval, by when it has walked as far as a position is
    /// good to and its slot has told its place to another channel.
    fn last_resort(&self, n: &Neighbour, now: Instant) -> bool {
        n.last_resort()
            || (n.carried
                && n.relay()
                && now.since(n.last_heard) >= heartbeat_interval(n.id, &self.config))
    }

    /// Whether `n` is among the neighbours to choose from, `others_open`
    /// telling whether any that is no last resort is open: a last resort
    /// only where none is, banned or not (spec "Next hop"). A message
    /// whose other neighbours are banned waits for them
    /// (spec "Overload waits").
    fn tier(&self, n: &Neighbour, others_open: bool, now: Instant) -> bool {
        self.last_resort(n, now) != others_open
    }

    /// Whether any neighbour that is no last resort is a candidate for the
    /// message of `record` at `now` ([`Self::tier`]): live, and neither
    /// handed it nor its parent.
    fn others_open(&self, record: &Record, passed: u64, now: Instant) -> bool {
        let parent = record.parent().map(|(name, _)| name);
        self.neighbours.live(now).any(|(slot, n)| {
            !self.last_resort(n, now) && !tried(record, passed, slot) && Some(n.id) != parent
        })
    }

    /// The next hop for the message of `record` to `dst` (spec "Next hop"),
    /// from the state as it is at `now`: the live neighbour nearest `dst`
    /// that is not in the tried set, ties broken by ID; a demoted one only
    /// when no other with margin is left, and one without margin only when
    /// no other at all (roadmap D25, D29, D38). Bans do not enter: a banned
    /// next hop is waited for, not gone round (roadmap D106), but for the
    /// slots of `avoid`, gone round while banned where any other is left
    /// (research build `detour-silence`).
    fn next_hop(
        &self,
        record: &Record,
        passed: u64,
        avoid: u64,
        dst: Position,
        now: Instant,
    ) -> Option<(usize, Neighbour)> {
        let hop = self.choose(record, passed, dst, now);
        if avoid == 0 {
            return hop;
        }
        let round = avoid & self.banned_slots(now);
        match hop {
            Some((slot, n)) if round & (1u64 << slot) != 0 => self
                .choose(record, passed | round, dst, now)
                .filter(|(_, m)| !self.last_resort(m, now) || self.last_resort(&n, now))
                .or(hop),
            _ => hop,
        }
    }

    /// [`Self::next_hop`] with every slot of `passed` taken out.
    fn choose(
        &self,
        record: &Record,
        passed: u64,
        dst: Position,
        now: Instant,
    ) -> Option<(usize, Neighbour)> {
        let target = self.plane.point(dst);
        let parent = record.parent().map(|(name, _)| name);
        // Computed once: it takes a logarithm.
        let margin = Neighbours::margin_dbm(self.radio.busy.noise_floor_dbm());
        let faint = |n: &Neighbour| n.rssi < margin;
        let demoted = |n: &Neighbour| self.neighbours.is_demoted(n, now);
        let first = self.first_hop(now).filter(|_| record.parent().is_none());
        let others = self.others_open(record, passed, now);
        self.neighbours
            .live(now)
            .filter(|(slot, n)| {
                first.is_none_or(|a| n.id == a)
                    && (first.is_some() || self.tier(n, others, now))
                    && !tried(record, passed, *slot)
                    && Some(n.id) != parent
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

    /// The neighbour-table slots of the live neighbours banned at `now`.
    fn banned_slots(&self, now: Instant) -> u64 {
        self.neighbours
            .live(now)
            .filter(|(_, n)| self.neighbours.is_banned(n, now))
            .fold(0, |m, (slot, _)| m | 1u64 << slot)
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
        let repeat = self
            .records
            .get(id)
            .is_some_and(|r| r.repeats(d.src_hop, d.hops));
        // A packet from the relay this one is handing the message to shows
        // that it took it, and ends that hand-over, which frees its place.
        let completes = !repeat && self.handing(id, d.src_hop);
        let v = self.verdict(&d, ACK_AIRTIME, completes, now);
        let deadline = now + self.config.ack_timeout;
        let air = self.data_airtime(d.payload.len());
        if let Some(g) = v.gate {
            emit(Action::Trace(Trace::WayWait {
                wait: g.queue,
                refused: g.over,
            }));
        }
        let wait = v.refusal.map_or(0, |why| {
            let told = self.refusal_wait(why, air, v.pushback, v.way, v.gate, now, emit);
            wait_units(told)
        });
        let reply = Ack {
            msg_id: id,
            from: self.me(),
            to: d.src_hop,
            refused: v.refusal.is_some(),
            wait,
            seen: v.case == Case::Seen,
        };
        let sending = self.sending_reply();
        if !self
            .radio
            .replies
            .can_owe(reply, d.sender_home, sending, now)
        {
            // The packet could not be answered; its sender's retry
            // brings it again (spec "The ACK", roadmap D29).
            emit(Action::Trace(Trace::Silent {
                msg_id: id,
                owed: true,
            }));
            self.note_untaken(emit);
            return;
        }
        if let Some(reason) = v.refusal {
            emit(Action::Trace(Trace::Refused { msg_id: id, reason }));
            if self.can_reply(ACK_AIRTIME, now) {
                self.radio
                    .replies
                    .owe(reply, d.sender_home, deadline, false, None, sending, now);
                self.note_untaken(emit);
            } else {
                emit(Action::Trace(Trace::Silent {
                    msg_id: id,
                    owed: false,
                }));
            }
            return;
        }
        if !self.can_reply(ACK_AIRTIME, now) {
            emit(Action::Trace(Trace::Refused {
                msg_id: id,
                reason: DropReason::Budget,
            }));
            emit(Action::Trace(Trace::Silent {
                msg_id: id,
                owed: false,
            }));
            return;
        }
        // A sleepy node listens on once it has answered, for the next
        // (spec "Delivery").
        let renews = d.dst_id == self.me();
        // The reply carries a packet with work until it has gone out
        // (module documentation); one that is no work is acted on now:
        // nothing is lost if its ACK never goes.
        let later = v.acts();
        let takes = later.then(|| data_frame(&d, d.sender_home, MIN_PREAMBLE_SYMBOLS));
        self.radio
            .replies
            .owe(reply, d.sender_home, deadline, renews, takes, sending, now);
        self.note_untaken(emit);
        if completes {
            self.handover_done(now, emit);
        }
        if !later {
            self.act_on(&d, v, now, emit);
        }
    }

    /// Whether `d` is taken or refused, and what it gives this relay to do
    /// (spec "Receiving DATA"). `spend`: what the ACK that takes it will
    /// cost the bucket, nothing once it has gone, when the gate is not
    /// asked again; `completes`: it comes
    /// from the neighbour being handed the message, whose hand-over ends
    /// first, giving up its place.
    fn verdict(&self, d: &Data<'_>, spend: Duration, completes: bool, now: Instant) -> Verdict {
        let id = d.msg_id();
        // Count 0: a sleepy node's own message, fresh work that starts its
        // search here (spec "Packet sender", "Hop count").
        let fresh = d.hops == 0;
        let repeat = self
            .records
            .get(id)
            .is_some_and(|r| r.repeats(d.src_hop, d.hops));
        // From a neighbour that has the message from this relay: a return
        // (spec "Receiving DATA" case 5). The one being handed to has it
        // too, though its ACK has not come.
        let slot = self.neighbours.slot_of(d.src_hop);
        let returned = completes
            || self
                .records
                .get(id)
                .zip(slot)
                .is_some_and(|(r, s)| r.handed(s));
        let owner_here = d.dst_id == self.me() || self.owns(d.dst_pos, now);
        let case = self.classify(id, repeat, returned, owner_here);
        let copy = self
            .records
            .get(id)
            .filter(|r| !repeat && !r.arrived())
            .map(|_| (returned, case == Case::AckOnly, case == Case::Seen));
        let acts = matches!(case, Case::New | Case::Search);
        let owner = acts && owner_here;
        let sends = acts && !owner;
        let for_leaf = owner && d.dst_id != self.me();
        let record_at = match case {
            Case::New => self.record_place(),
            _ => None,
        };
        let mut place = 0;
        // Where it would go from here now, as the record it would have
        // routes it: its sender, a sleepy node handing over its own or a
        // child returning it, has it.
        let heads = if sends {
            let mut probe = match self.records.get(id) {
                Some(r) if case == Case::Search => *r,
                _ => Record::new(
                    id,
                    (!fresh).then_some((d.src_hop, d.sender_home)),
                    d.hops,
                    now,
                ),
            };
            if let Some(s) = slot.filter(|_| fresh || case == Case::Search) {
                probe.handed_to(s);
            }
            self.heads_to(&probe, 0, 0, d.dst_pos, now)
        } else {
            None
        };
        // The rest of the ban on its way on (spec "Bans"). Fresh work is
        // refused while it lasts, and told how long, as spec "Receiving DATA"
        // says. A message already in the network is not: refused for
        // that, its sender would send it round the pushback, and around
        // a ring of relays the refusals kept each other up.
        let pushback = heads.map_or(Duration::ZERO, |(_, ban)| ban.since(now));
        // Not again once the take is final: the sender is done with it.
        let gate = heads
            .filter(|_| spend > Duration::ZERO)
            .map(|(to, _)| self.gate(id, to, d.hops, now));
        let refusal = if !acts {
            None
        } else if case == Case::New && record_at.is_none() {
            Some(DropReason::NoRecord)
        } else if gate.is_some_and(|g| g.over) {
            Some(DropReason::SameHop)
        } else if sends {
            let air = self.data_airtime(d.payload.len());
            match self.admit(air, spend, d.hops, completes, now) {
                Ok(_) if fresh && self.bans_end(now).is_some() => Some(DropReason::Banned),
                Ok(_) if fresh && pushback > Duration::ZERO => Some(DropReason::Banned),
                Ok(k) => {
                    place = k;
                    None
                }
                Err(why) => Some(why),
            }
        } else if for_leaf && !self.delivery.has_room(now) {
            Some(DropReason::DeliveryFull)
        } else if !self.affordable(spend, now) {
            Some(DropReason::Budget)
        } else {
            None
        };
        Verdict {
            case,
            slot,
            copy,
            owner,
            fresh,
            record_at,
            place,
            refusal,
            pushback,
            way: heads.map(|(to, _)| to),
            gate,
        }
    }

    /// Act on `d`, answered with verdict `v`: its record, then its work.
    fn act_on(&mut self, d: &Data<'_>, v: Verdict, now: Instant, emit: &mut dyn FnMut(Action)) {
        let id = d.msg_id();
        if v.case == Case::New {
            let parent = (!v.fresh).then_some((d.src_hop, d.sender_home));
            let mut record = Record::new(id, parent, d.hops, now);
            if v.owner {
                record.set_arrived();
            }
            if let (true, Some(slot)) = (v.fresh, v.slot) {
                // The sleepy node it came from has it, and would end the
                // search there: it is tried.
                record.handed_to(slot);
            }
            if let Some(at) = v.record_at {
                self.records.put(at, record);
            }
        } else if let Some(r) = self.records.get_mut(id) {
            if v.case == Case::Seen {
                // The sender has the message and searches on from its own
                // record; it need not be offered this one.
                r.seen(d.src_hop, d.hops, now);
                if let Some(s) = v.slot {
                    r.handed_to(s);
                }
            } else {
                r.received(d.src_hop, d.hops, now);
            }
            if v.owner {
                r.set_arrived();
            }
        }
        if let Some((returned, merged, seen)) = v.copy {
            emit(Action::Trace(Trace::Copy {
                msg_id: id,
                returned,
                merged,
                seen,
            }));
        }
        match v.case {
            Case::AckOnly => {
                self.delivered.offered(id, now);
            }
            // The copy stays with its sender, which goes on.
            Case::Seen => {}
            Case::New | Case::Search if v.owner => {
                self.arrive(id, d, now, emit);
            }
            Case::New | Case::Search => self.send_on(d, v.place, now, emit),
        }
    }

    /// The ACK that takes the DATA in `frame` has left the antenna: the
    /// take is final, and the relay acts on it now (spec "The ACK").
    /// Nothing has changed since it was answered taken but the ACK's own
    /// airtime, spent; should the room be gone after all, the copy ends
    /// here, since its sender is done with it.
    pub(super) fn taken(&mut self, frame: Frame, now: Instant, emit: &mut dyn FnMut(Action)) {
        let Ok(Packet::Data(d)) = Packet::decode(frame.payload()) else {
            return;
        };
        let v = self.verdict(&d, Duration::ZERO, false, now);
        if let Some(why) = v.refusal {
            self.drop_msg(d.msg_id(), why, emit);
            return;
        }
        self.act_on(&d, v, now, emit);
    }

    /// Report a DATA that a reply given up left untaken: refused, for its
    /// sender retries it (spec "The ACK").
    fn note_untaken(&mut self, emit: &mut dyn FnMut(Action)) {
        if let Some(msg_id) = self.radio.replies.untaken() {
            emit(Action::Trace(Trace::Refused {
                msg_id,
                reason: DropReason::Unanswered,
            }));
        }
    }

    /// The case of spec "Receiving DATA" for a DATA packet of message `id`; `repeat`:
    /// from the sender and with the count of the last one received;
    /// `returned`: from a neighbour that has the message from this relay;
    /// `owner`: this relay owns the destination now.
    fn classify(&self, id: MsgId, repeat: bool, returned: bool, owner: bool) -> Case {
        if self.delivered.contains(id) || self.delivery.iter().any(|q| q.msg_id == id) {
            return Case::AckOnly;
        }
        let Some(r) = self.records.get(id) else {
            return Case::New;
        };
        if r.arrived() {
            return Case::AckOnly;
        }
        if repeat {
            // Answered as the first was (case 3).
            return if r.last_seen() {
                Case::Seen
            } else {
                Case::AckOnly
            };
        }
        if self.holds(id) {
            return Case::AckOnly;
        }
        if returned || owner {
            return Case::Search;
        }
        Case::Seen
    }

    /// Whether this relay holds a copy of message `id` that is sure to live
    /// on: in its send queue or set aside, but not the frame of a hand-over
    /// under way, once it has been on air. Its receiver may have taken it
    /// and only the ACK been lost, and the copy now offered here may be
    /// that one, come back.
    fn holds(&self, id: MsgId) -> bool {
        let sent = self.in_flight().is_some_and(|o| o.transmitted());
        let current = self.queue.current_index().filter(|_| sent);
        self.queue
            .iter()
            .enumerate()
            .any(|(i, o)| Some(i) != current && o.msg_id() == Some(id))
    }

    /// Whether the current hand-over is of message `id` to `to`, sent at
    /// least once.
    fn handing(&self, id: MsgId, to: NodeId) -> bool {
        self.in_flight()
            .filter(|o| o.transmitted())
            .and_then(|o| o.expect(self.me()))
            == Some(self.expect(id, to))
    }

    /// Where a new record goes: never in place of the record of a message
    /// this relay holds, in its send queue or set aside, which it needs to
    /// route that message (`tables/records.rs`). `None` only if every
    /// record is such, which the table's size rules out.
    pub(super) fn record_place(&self) -> Option<usize> {
        let mut held = [MsgId(0); HELD + 1];
        let mut n = 0;
        if self.records.is_full() {
            for id in self.queue.iter().filter_map(Outgoing::msg_id) {
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
    /// hand-over starts. Carries the
    /// received count plus one, which places it among waiting entries,
    /// until its hand-over stamps the count it is sent with (spec "Hop count").
    fn send_on(&mut self, d: &Data<'_>, place: u8, now: Instant, emit: &mut dyn FnMut(Action)) {
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
            since: now,
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
    /// node it is for.
    fn arrive(&mut self, id: MsgId, d: &Data<'_>, now: Instant, emit: &mut dyn FnMut(Action)) {
        let (node, payload) = (d.dst_id, d.payload);
        if self.own.id == Some(id) {
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
            Pushed::Queued => self.forwarding.feed_due = true,
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
        self.own.id.is_none()
            && now >= self.timers.retry_at
            && self
                .no_room(self.data_airtime(MAX_PAYLOAD), Duration::ZERO, now)
                .is_none()
            && self.bans_end(now).is_none()
    }

    /// When the application may send again, if only the bucket and bans
    /// keep it waiting; anything else ends with an event.
    pub(super) fn send_ready_at(&self, now: Instant) -> Option<Instant> {
        if self.own.id.is_some() || self.regular() >= SEND_QUEUE {
            return None;
        }
        let need = self.queue.airtime() + self.data_airtime(MAX_PAYLOAD) + REPLY_RESERVE;
        let bucket = self
            .radio
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
        self.own = OwnMessage {
            id: own,
            refusals: 0,
        };
    }

    /// Deliver the message set aside once the delivery queue has a place.
    pub(super) fn release_held(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        let Some(i) = self.queue.iter().position(Outgoing::held) else {
            return;
        };
        if !self.delivery.has_room(now) {
            return;
        }
        let Some(h) = self.queue.remove(i) else {
            return;
        };
        if let Ok(Packet::Data(d)) = Packet::decode(h.frame.payload()) {
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
        let d = Data {
            sender_home: self.home,
            dst_hop: NodeId::NONE,
            src_hop: self.me(),
            dst_pos,
            dst_id,
            hops: 0,
            time_left: Data::units(self.config.delivery_ttl),
            payload,
        };
        if dst_id == self.me() || self.owns(dst_pos, now) {
            let mut record = Record::new(id, None, 0, now);
            record.set_arrived();
            self.records.put(at, record);
            if dst_id != self.me() && !self.delivery.has_room(now) {
                // At its owner the delivery queue's rule ends it
                // (spec "Delivery queue"). Admission left it a place, as for any message of the
                // application's.
                let out = Outgoing {
                    state: State::Held,
                    since: now,
                    ..Outgoing::new(
                        data_frame(&d, self.home, self.forward_preamble()),
                        Kind::Forward,
                    )
                };
                let _ = self.queue.push(out);
            } else {
                self.arrive(id, &d, now, emit);
            }
            return;
        }
        let record = Record::new(id, None, 0, now);
        if let Some((to, ban)) = self.heads_to(&record, 0, 0, dst_pos, now) {
            // Fresh work, gated as a sleepy node's own is at its anchor
            // (spec "Receiving DATA"): refused, it goes back to the
            // application with a place in its way's line, as an ACK
            // tells.
            let gate = self.gate(id, to, 0, now);
            emit(Action::Trace(Trace::WayWait {
                wait: gate.queue,
                refused: gate.over,
            }));
            if gate.over || ban > now {
                let pushback = ban.since(now);
                let wait = self
                    .line_wait(to, Some(gate), pushback, now, emit)
                    .unwrap_or(pushback)
                    .max(BAN);
                self.timers.retry_at = now + wait;
                self.drop_msg(id, DropReason::Pushback, emit);
                emit(Action::App(AppEvent::GivenBack {
                    msg_id: id,
                    retry_at: self.timers.retry_at,
                }));
                return;
            }
        }
        self.records.put(at, record);
        self.send_on(&d, 0, now, emit);
    }

    // --- The hand-over (spec "Hand-over") ---

    /// Make sure the send queue's next due entry has a hand-over: address
    /// a forward, or drop what cannot be sent. The entry stays where it is
    /// in the queue.
    pub(super) fn prepare(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        self.make_way(now);
        while self.flights() < IN_FLIGHT {
            let Some(i) = self.pick(now) else {
                return;
            };
            if self.queue.current_index() != Some(i) {
                self.queue.select(i);
            }
            let Some(out) = self.queue.current().copied() else {
                return;
            };
            let routed = match out.kind {
                Kind::Forward => self.route(out, now, emit),
                Kind::Return => match Packet::decode(out.frame.payload()) {
                    Ok(Packet::Data(_)) => Routed::To(out),
                    _ => Routed::Done,
                },
                // Stays in the delivery queue while its node sleeps.
                Kind::Deliver => match Packet::decode(out.frame.payload()) {
                    Ok(Packet::Data(d)) if self.listening(d.dst_hop, now) || self.early(&out) => {
                        Routed::To(out)
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
                    Ok(Packet::Data(_)) => Routed::To(out),
                    _ => Routed::Done,
                },
            };
            match routed {
                Routed::To(out) => {
                    if let Some(current) = self.queue.current_mut() {
                        *current = out;
                    }
                    if let Some((mut h, _)) = self.handover() {
                        h.start(now);
                    }
                }
                Routed::Done => {
                    self.queue.take_current();
                    self.forwarding.feed_due = true;
                }
                Routed::Hold => {
                    if let Some(current) = self.queue.current_mut() {
                        current.state = State::Held;
                    }
                    self.queue.deselect();
                    self.forwarding.feed_due = true;
                }
                Routed::Wait(t) => {
                    if let Some(current) = self.queue.current_mut() {
                        current.not_before = t;
                    }
                }
                // Only if the state changed since the pick; the next
                // look at the queue sees it.
                Routed::Banned(t) => {
                    self.forwarding.wake = earlier(self.forwarding.wake, Some(t));
                    return;
                }
            }
        }
    }

    /// A DATA to a sleepy node goes first (spec "Delivery"): a hand-over
    /// that waits between its attempts, for the channel, a retry or the
    /// bucket, steps aside for it and keeps its place. A forward or a
    /// return is routed again when its turn comes, so it starts afresh.
    fn make_way(&mut self, now: Instant) {
        let Some(current) = self.queue.current().copied() else {
            return;
        };
        let waiting = matches!(current.state, State::Waiting(_)) && self.radio.excursion.idle();
        if !waiting || current.kind.delivers() || self.queue.delivery_due(now).is_none() {
            return;
        }
        if let Some(c) = self.queue.current_mut() {
            if c.kind == Kind::Return {
                c.kind = Kind::Forward;
            }
            if c.kind == Kind::Forward {
                c.tries = 0;
            }
            c.state = State::Queued;
        }
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

    /// Address the forward `out` from the state as it is now: to its next
    /// hop (spec "Next hop"), or back to its parent once none is left
    /// (spec "Return to parent"), or let it wait while that one is
    /// banned, or drop it at the origin. Nothing of an earlier choice is
    /// kept: a neighbour that refused or failed it is chosen again while
    /// it ranks first, and the message goes elsewhere once it does not,
    /// demoted or gone.
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
            if self.own.id == Some(id) && d.dst_id != self.me() && !self.delivery.has_room(now) {
                return Routed::Hold;
            }
            self.arrive(id, &d, now, emit);
            return Routed::Done;
        }
        if let Some((_, n)) = self.next_hop(&record, out.passed, out.avoided(), d.dst_pos, now) {
            if self.neighbours.is_banned(&n, now) {
                // A ban is a wait, not an exclusion (spec "Next hop").
                return Routed::Banned(n.banned_until);
            }
            let to = n.id;
            let frame = data_frame(&Data { dst_hop: to, ..d }, n.home, preamble(&n));
            // Addressed afresh, its attempts start afresh.
            return Routed::To(Outgoing {
                frame,
                kind: Kind::Forward,
                tries: 0,
                state: State::Queued,
                ..out
            });
        }
        if let Some(ends) = self.timers.boot_ends {
            if self.own.id == Some(id) && !self.neighbours.live(now).any(|(_, n)| n.relay()) {
                // Until it hears a relay, or its first rotation ends
                // (`heard`).
                return Routed::Wait(ends);
            }
        }
        let alone = self.neighbours.live(now).next().is_none();
        if self.sleepy() && self.own.id == Some(id) && alone {
            // Until its time runs out (`expire`).
            return Routed::Wait(now + self.config.fade_retry);
        }
        match record.parent() {
            Some((parent, stored)) => {
                if let Some(p) = self
                    .neighbours
                    .get(parent)
                    .filter(|p| self.neighbours.is_banned(p, now))
                {
                    return Routed::Banned(p.banned_until);
                }
                let home = self.home_of(parent).unwrap_or(stored);
                let frame = data_frame(
                    &Data {
                        dst_hop: parent,
                        ..d
                    },
                    home,
                    self.preamble_to(parent),
                );
                Routed::To(Outgoing {
                    frame,
                    kind: Kind::Return,
                    tries: 0,
                    state: State::Queued,
                    ..out
                })
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
        let others = self
            .neighbours
            .live(now)
            .any(|(_, n)| !self.last_resort(n, now));
        let mut until: Option<Instant> = None;
        for (_, n) in self
            .neighbours
            .live(now)
            .filter(|(_, n)| self.tier(n, others, now))
        {
            if !self.neighbours.is_banned(n, now) {
                return None;
            }
            until = Some(until.map_or(n.banned_until, |t| t.min(n.banned_until)));
        }
        until
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
        if !self.handover().is_some_and(|(h, _)| h.due(now)) {
            return false;
        }
        let Some(out) = self.readdress(now) else {
            return false;
        };
        if !self.affordable(mac::airtime(&out.frame), now) {
            self.defer_handover(&out, now, emit);
            return false;
        }
        if let Some((mut h, _)) = self.handover() {
            h.begin_lbt(now);
        }
        self.radio
            .excursion
            .check(Job::HandOver, out.frame.channel, emit);
        true
    }

    fn defer_handover(&mut self, out: &Outgoing, now: Instant, emit: &mut dyn FnMut(Action)) {
        let airtime = mac::airtime(&out.frame);
        let until = self
            .radio
            .budget
            .allowed_at(airtime, REPLY_RESERVE, now)
            .unwrap_or(now + self.config.heartbeat_max);
        if let Some((mut h, _)) = self.handover() {
            h.defer(until);
        }
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
        let Some(out) = self.queue.current().copied() else {
            return;
        };
        if out.state != State::Radio {
            return;
        }
        let backoff = self.config.hop_backoff_max;
        let Some((mut h, rng)) = self.handover() else {
            return;
        };
        match h.checked(busy, now, rng, backoff) {
            Check::Clear => self.transmit_current(out, now, emit),
            Check::Busy => {}
            Check::GaveUp => self.handover_gave_up(out, now, emit),
        }
    }

    /// Listen-before-talk is clear: transmit the current frame, with this
    /// relay's home channel as it is now (spec "Packet formats" `aux`).
    /// An owed ACK goes first, and the check is then repeated.
    fn transmit_current(&mut self, out: Outgoing, now: Instant, emit: &mut dyn FnMut(Action)) {
        let Some(frame) = self.stamp(out, now, emit) else {
            return;
        };
        let airtime = mac::airtime(&frame);
        if !self.affordable(airtime, now) {
            // An owed reply spent the budget since the check began.
            self.defer_handover(&out, now, emit);
            return;
        }
        if let Some(current) = self.queue.current_mut() {
            current.frame = frame;
        }
        self.radio.budget.record(airtime, now);
        self.radio.excursion.transmit(Job::HandOver, frame, emit);
    }

    /// The current frame as it goes now, its count stamped, or `None`
    /// where it is dropped at the cap, or held for an owed reply.
    fn stamp(
        &mut self,
        out: Outgoing,
        now: Instant,
        emit: &mut dyn FnMut(Action),
    ) -> Option<Frame> {
        if self.radio.replies.wake_at().is_some_and(|t| t <= now) {
            if let Some((mut h, _)) = self.handover() {
                h.hold(now);
            }
            return None;
        }
        let mut frame = out.frame;
        if let Ok(Packet::Data(d)) = Packet::decode(out.frame.payload()) {
            let id = d.msg_id();
            let Some(hops) = self.count(&d, id, &out) else {
                self.queue.take_current();
                self.forwarding.feed_due = true;
                self.drop_msg(id, DropReason::HopCap, emit);
                return None;
            };
            frame = data_frame(
                &Data {
                    sender_home: self.home,
                    hops,
                    ..d
                },
                out.frame.channel,
                out.frame.preamble_symbols,
            );
            if let Some(r) = self.records.get_mut(id) {
                r.sent(now);
            }
        }
        Some(frame)
    }

    /// The count the current hand-over of `d` carries (spec "Hop count"): stamped at
    /// its first transmission as the record's highest plus one, repeated by
    /// its retries. Stamped when queued, a frame would keep a count older
    /// than one this relay has since received or sent, and a relay that
    /// readdressed it would offer a count it had already used. `None` past
    /// the hop cap, but for a delivery: its search has ended, and the cap
    /// ends searches, so it goes at the cap.
    fn count(&self, d: &Data<'_>, id: MsgId, out: &Outgoing) -> Option<u8> {
        if out.transmitted() {
            return Some(d.hops);
        }
        debug_assert!(
            self.records.get(id).is_some(),
            "a held message keeps its record"
        );
        match self.records.get(id) {
            // A sleepy node hands its own message over as fresh work: the
            // node that takes it is its origin (module documentation).
            Some(r)
                if self.anchored_sleepy() && self.own.id == Some(id) && r.parent().is_none() =>
            {
                Some(0)
            }
            Some(r) if out.kind.delivers() => Some(r.highest_hops().saturating_add(1)),
            Some(r) => r.highest_hops().checked_add(1),
            None => Some(d.hops),
        }
    }

    /// Listen-before-talk gave up on the current hand-over's channel.
    fn handover_gave_up(&mut self, out: Outgoing, now: Instant, emit: &mut dyn FnMut(Action)) {
        let (msg_id, to) = out
            .expect(self.me())
            .map_or((MsgId::default(), NodeId::NONE), |e| (e.msg_id, e.from));
        match out.kind {
            Kind::Forward => {
                // The channel says nothing about the neighbour: the same
                // neighbour again, after a retry wait (spec "Hand-over"
                // step 2, roadmap D96).
                let unit = self.retry_unit();
                let Some((mut h, rng)) = self.handover() else {
                    return;
                };
                let until = h.channel_busy(now, rng, unit);
                emit(Action::Trace(Trace::ChannelBusy { msg_id, to, until }));
            }
            Kind::Return | Kind::Deliver | Kind::Direct => {
                let (max, retry) = (self.config.hop_attempts, self.retry_unit());
                let Some((mut h, rng)) = self.handover() else {
                    return;
                };
                let missed = h.lost_attempt(now, max, rng, retry);
                let until = self.forwarding.flight.until().max(now);
                emit(Action::Trace(Trace::ChannelBusy { msg_id, to, until }));
                if missed == Missed::Failed {
                    self.handover_failed(out, false, now, emit);
                }
            }
        }
    }

    /// An ACK arrived: if it answers the current hand-over, the receiver
    /// took the packet, refused it, or has seen it (spec "The ACK",
    /// "Bans"); see the module documentation.
    pub(super) fn ack_received(&mut self, ack: Ack, now: Instant, emit: &mut dyn FnMut(Action)) {
        if ack.refused {
            self.refused(ack, now, emit);
        } else if ack.seen {
            self.seen(ack, now, emit);
        } else if self.handover().is_some_and(|(mut h, _)| h.acked(ack)) {
            self.handover_done(now, emit);
        }
    }

    /// The receiver of the current hand-over has the message's record and
    /// no copy of it (spec "The ACK", roadmap D90). It is tried, as one
    /// that took the packet is; a forward is then routed again from the
    /// record when its turn comes, the same choice a return taken from a
    /// child gets, so the sender goes on to its next candidate as if the
    /// receiver had returned the message at once. Nothing is banned and
    /// no failure is counted. A return has nowhere else to go: the parent
    /// has gone on without this copy, and it ends here.
    fn seen(&mut self, ack: Ack, now: Instant, emit: &mut dyn FnMut(Action)) {
        if !self
            .in_flight()
            .is_some_and(|o| o.answered_by(ack, self.id))
        {
            return;
        }
        let Some(out) = self.queue.current().copied() else {
            return;
        };
        let Ok(Packet::Data(d)) = Packet::decode(out.frame.payload()) else {
            return;
        };
        let (id, to) = (d.msg_id(), d.dst_hop);
        emit(Action::Trace(Trace::Seen { msg_id: id, by: to }));
        self.tried(id, to, d.hops, now, emit);
        if out.kind == Kind::Forward {
            self.cancel_handover();
            return;
        }
        self.queue.take_current();
        self.forwarding.feed_due = true;
        self.drop_msg(id, DropReason::ReturnSeen, emit);
    }

    /// `id` answered this relay (spec "Neighbour table"): traced if it was
    /// demoted since its last answer, for counting how long a demoted
    /// neighbour stays silent.
    pub(super) fn answer_from(&mut self, id: NodeId, now: Instant, emit: &mut dyn FnMut(Action)) {
        let after = self.neighbours.params().demote_after;
        if self
            .neighbours
            .get(id)
            .is_some_and(|n| n.fail_count >= after)
        {
            emit(Action::Trace(Trace::Answered { neighbour: id }));
        }
        self.neighbours.answered(id, now);
    }

    /// The receiver `to` of a hand-over of `id` sent at count `hops`
    /// answered that it has the packet, taken or seen: it is alive and
    /// tried, and the count is spent (spec "The ACK", "Message records",
    /// "Hop count").
    fn tried(
        &mut self,
        id: MsgId,
        to: NodeId,
        hops: u8,
        now: Instant,
        emit: &mut dyn FnMut(Action),
    ) {
        let slot = self.neighbours.slot_of(to);
        if slot.is_some() {
            self.answer_from(to, now, emit);
        }
        if let Some(r) = self.records.get_mut(id) {
            r.handed_over(hops);
            // The parent that took a return has the message from this
            // relay as much as a child has (spec "Message records").
            if let Some(s) = slot {
                r.handed_to(s);
            }
        }
    }

    fn refused(&mut self, ack: Ack, now: Instant, emit: &mut dyn FnMut(Action)) {
        if !self
            .in_flight()
            .is_some_and(|o| o.answered_by(ack, self.id))
        {
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
            self.answer_from(by, now, emit);
            self.neighbours.refused(by, now + wait);
            self.neighbours.told(by, wait);
        }
        if self.sleepy() && self.own.id == Some(msg_id) && out.kind == Kind::Forward {
            // A sleepy node's own message goes back to its application,
            // which may offer it again once the wait is over: refused at
            // the origin, it does not age here (spec "A sleepy node's own message").
            self.queue.take_current();
            self.forwarding.feed_due = true;
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
            // Routed again when its turn comes: the refuser, banned now,
            // stays its next hop while it ranks first, so the message waits
            // for it, and going round it would only take the message the
            // long way past a relay that has said when it will have room
            // (spec "Bans", "Next hop", roadmap D106).
            Kind::Forward | Kind::Return => {
                if cfg!(feature = "detour-refusal") {
                    // Research build: gone round for the told wait.
                    self.go_round(by);
                }
                if self.own.id == Some(msg_id) {
                    // Each offer costs the refuser a reply and hides its ACKs.
                    self.own.refusals = self.own.refusals.saturating_add(1);
                    let until = now + wait.max(hold(self.own.refusals));
                    if let Some(current) = self.queue.current_mut() {
                        current.not_before = until;
                    }
                }
                self.cancel_handover();
                if let Some(current) = self.queue.current_mut() {
                    current.kind = Kind::Forward;
                    current.tries = 0;
                }
            }
            // No other receiver: wait for the ban, and let the queue move.
            Kind::Deliver | Kind::Direct => {
                let tries = self.handover().map_or(0, |(mut h, _)| h.refused());
                if let Some(current) = self.queue.current_mut() {
                    current.not_before = now + wait;
                    current.tries = tries;
                }
            }
        }
    }

    /// The current hand-over succeeded: its receiver has the packet.
    pub(super) fn handover_done(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        let Some(out) = self.queue.take_current() else {
            return;
        };
        self.forwarding.feed_due = true;
        let Ok(Packet::Data(d)) = Packet::decode(out.frame.payload()) else {
            return;
        };
        let (id, to) = (d.msg_id(), d.dst_hop);
        if out.kind.delivers() {
            self.answer_from(to, now, emit);
            let long = out.frame.preamble_symbols >= LONG_PREAMBLE_SYMBOLS;
            let by = match (out.kind, long) {
                (Kind::Direct, true) => Delivery::Flush,
                (Kind::Direct, false) => Delivery::Relay,
                (_, true) => Delivery::Long,
                (_, false) => Delivery::Window,
            };
            return self.delivered_to(id, to, by, now, emit);
        }
        self.neighbours.took(to, out.since, now);
        self.tried(id, to, d.hops, now, emit);
        if to == d.dst_id {
            // The node it is addressed to took it: it has arrived.
            if let Some(r) = self.records.get_mut(id) {
                r.set_arrived();
            }
        }
        let trace = match out.kind {
            Kind::Forward => {
                if self.own.id == Some(id) {
                    self.set_own(None);
                }
                Trace::Forwarded {
                    msg_id: id,
                    to,
                    hops: d.hops,
                    attempts: out.tries,
                }
            }
            Kind::Return | Kind::Deliver | Kind::Direct => Trace::ReturnedToParent {
                msg_id: id,
                to,
                attempts: out.tries,
            },
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
                self.count_failure(d.dst_hop, now, emit);
                let own_at_anchor = self.sleepy() && d.hops == 0;
                if out.kind == Kind::Forward && !own_at_anchor {
                    self.passed_if_demoted(d.dst_hop, now);
                    self.go_round(d.dst_hop);
                }
                let (passed, avoid) = self
                    .queue
                    .current()
                    .map_or((0, 0), |o| (o.passed, o.avoided()));
                let ban = self
                    .neighbours
                    .get(d.dst_hop)
                    .map(|n| n.banned_until)
                    .filter(|&t| t > now);
                // The neighbour now demoted or gone: a return ends, and a
                // forward goes where its next routing takes it.
                let gone = self
                    .neighbours
                    .get(d.dst_hop)
                    .is_none_or(|n| self.neighbours.is_demoted(n, now));
                let moves_on = match out.kind {
                    Kind::Forward => {
                        !own_at_anchor
                            && self
                                .goes_to(&d, passed, avoid, now)
                                .is_some_and(|to| to != d.dst_hop)
                    }
                    _ => gone,
                };
                emit(Action::Trace(Trace::HandOverFailed {
                    msg_id: id,
                    to: d.dst_hop,
                    moves_on,
                }));
                if own_at_anchor {
                    // A sleepy node's own message tries its anchor again a
                    // fade later, doubled for each silence in a row
                    // (spec "A sleepy node's own message"): the ban's ladder would
                    // offer it every few seconds to a node whose channel is
                    // full, and a fixed fade every minute for its whole TTL.
                    let ban = ban.unwrap_or(now);
                    let silences = self
                        .neighbours
                        .by_id(d.dst_hop)
                        .map_or(1, |(_, n)| n.fail_count.max(1));
                    let mut fade = self.config.fade_retry;
                    for _ in 1..silences {
                        fade = fade.mul(2).min(self.config.delivery_ttl);
                    }
                    if let Some(current) = self.queue.current_mut() {
                        current.not_before = ban.max(now + fade);
                        current.tries = 0;
                    }
                    return;
                }
                if out.kind == Kind::Forward || !gone {
                    // Routed again when its turn comes, with fresh
                    // attempts: banned now, the neighbour is waited for
                    // while it ranks first, and the message goes elsewhere
                    // once its demotion makes it a last resort (roadmap
                    // D42, D106). Most likely the frame, not the ACK, was
                    // lost, so the neighbour stays untried.
                    if let Some(current) = self.queue.current_mut() {
                        current.kind = Kind::Forward;
                        current.tries = 0;
                    }
                    return;
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
        self.forwarding.feed_due = true;
        if let Some(reason) = reason {
            self.drop_msg(id, reason, emit);
        }
    }

    /// The current forward, which failed at `to`, has held for `to`
    /// through its demotion: `to` counts as tried for it while it is
    /// queued here, so that its search goes on, to the next candidate or
    /// back to its parent. A link that works one way, the only candidate
    /// left, would otherwise hold the message for as long as its
    /// heartbeats keep its entry (spec "Next hop"). Not in the record:
    /// `to` does not have the message, and a copy it offers later is no
    /// return (spec "Receiving DATA").
    fn passed_if_demoted(&mut self, to: NodeId, now: Instant) {
        let Some((slot, n)) = self.neighbours.by_id(to) else {
            return;
        };
        if !self.neighbours.is_demoted(n, now) {
            return;
        }
        if let Some(o) = self.queue.current_mut() {
            o.passed |= 1u64 << slot;
        }
    }

    /// The current forward goes round `to` while `to` is banned (research
    /// builds `detour-silence` and `detour-refusal`; a no-op otherwise).
    fn go_round(&mut self, to: NodeId) {
        let Some(slot) = self.neighbours.slot_of(to) else {
            return;
        };
        if let Some(o) = self.queue.current_mut() {
            o.avoid(slot);
        }
    }

    /// Where the forward of `d` would go if routed now: its next hop, or
    /// its parent once none is left (spec "Next hop", "Return to parent").
    fn goes_to(&self, d: &Data<'_>, passed: u64, avoid: u64, now: Instant) -> Option<NodeId> {
        let record = self.records.get(d.msg_id())?;
        self.next_hop(record, passed, avoid, d.dst_pos, now)
            .map(|(_, n)| n.id)
            .or_else(|| record.parent().map(|(p, _)| p))
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

    /// Send the owed reply, if it is ready, at once: its sender is
    /// listening for it (spec "The ACK"). Without budget for it the relay
    /// cannot reply (spec "Duty cycle cap"); the reply is abandoned.
    pub(super) fn start_reply(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) -> bool {
        let due = self.radio.replies.due(now);
        self.note_untaken(emit);
        let Some(o) = due else {
            return false;
        };
        let frame = reply_frame(o);
        let airtime = mac::airtime(&frame);
        if !self.can_reply(airtime, now) {
            let Reply::Ack(a) = o.reply;
            emit(Action::Trace(Trace::Silent {
                msg_id: a.msg_id,
                owed: false,
            }));
            self.radio.replies.abandon();
            self.note_untaken(emit);
            return true;
        }
        self.radio.budget.record(airtime, now);
        self.radio.excursion.transmit(Job::Reply, frame, emit);
        true
    }

    /// Whether the owed reply is on the radio.
    fn sending_reply(&self) -> bool {
        self.radio.excursion.step_of(Job::Reply).is_some()
    }

    // --- Timers ---

    pub(super) fn forward_timers(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        // This runs on every event; only an ACK wait that ran out needs the
        // retry unit's two airtimes.
        if self.awaiting_ack() {
            let (max, retry) = (self.config.hop_attempts, self.retry_unit());
            let missed = self
                .handover()
                .and_then(|(mut h, rng)| h.timeout(now, max, rng, retry));
            if missed == Some(Missed::Failed) {
                if let Some(out) = self.queue.current().copied() {
                    self.handover_failed(out, true, now, emit);
                }
            }
        }
    }

    /// When the hand-over in flight next needs the core, if it waits.
    pub(super) fn flight_wake(&self) -> Option<Instant> {
        match self.in_flight()?.state {
            State::Waiting(_) | State::AwaitAck => Some(self.forwarding.flight.until()),
            _ => None,
        }
    }
}
