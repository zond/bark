//! Forwarding (spec "Forwarding"): what a node does with a DATA addressed
//! to it, and the hand-overs of its send queue.
//!
//! First the types a received DATA is judged by, and three pure functions
//! that say where a queued entry stands after a failed hand-over
//! (`failure`), a refused one (`refusal`) and a routing (`way`), with
//! their table tests at the end of the file. Then the node's methods, in
//! sections:
//!
//! - the send queue's bookkeeping: the hand-over in flight, the wait a
//!   refusal tells, the gate and admission (spec "Send queue", "Bans");
//! - the owner and the next hop (spec "Owner", "Next hop");
//! - receiving DATA, judged when it arrives and checked again when its
//!   take is final, and messages that enter the send queue from the
//!   delivery queue (spec "Receiving DATA", "The ACK", "Delivery queue");
//! - the application's own message (spec "Addresses", "Receiving DATA");
//! - the hand-over: routing an entry, stamping its count, and what an
//!   answer or a silence makes of it (spec "Hand-over", "Hop count",
//!   "Return to parent", "Overload waits");
//! - the owed reply (spec "The ACK") and the timers.
//!
//! The radio steps of one hand-over are in `node/transfer.rs`, the
//! queue's order in `tables/send_queue.rs`, and delivery to sleepy nodes
//! in `relay/delivery.rs`.

use super::{Job, Node};
use crate::event::{Action, AppEvent, Delivery, DropReason, Frame, Trace, MAX_FRAME};
use crate::ids::{MsgId, NodeId};
use crate::node::transfer::{self, Check, Flight, HandOver, Missed, Owed, Reply, Take, IN_FLIGHT};
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
use crate::tables::send_queue::{Kind, Message, Outgoing, State, HOP_SLOTS, HOP_WAIT, SEND_QUEUE};
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

/// What a received DATA is to this relay (spec "Receiving DATA").
#[derive(Clone, Copy, Debug)]
struct Arrival {
    case: Case,
    /// The sender's neighbour-table slot.
    slot: Option<usize>,
    /// A copy of a message this relay has a record of that has not arrived
    /// here, other than its last packet again.
    copy: bool,
    /// From a node that has the message from this relay: a return.
    returned: bool,
    /// This relay owns the destination, and acts on the packet.
    owner: bool,
    /// A sleepy node's own message, count 0, whose search starts here.
    fresh: bool,
}

impl Arrival {
    /// Whether the packet is work: sent on or delivered (cases 2 and 5).
    fn acts(&self) -> bool {
        matches!(self.case, Case::New | Case::Search)
    }
}

/// Where a packet taken goes.
#[derive(Clone, Copy, Debug, Default)]
struct Room {
    /// Where a new record goes.
    record_at: Option<usize>,
    /// The send-queue place of one to send on; 0 the regular room.
    place: u8,
}

/// Whether a DATA is taken, decided when it arrives (spec "Receiving DATA").
#[derive(Clone, Copy, Debug)]
struct Verdict {
    arrival: Arrival,
    /// Why it is refused, if it is.
    refusal: Option<DropReason>,
    /// Its way on, for a refusal's place in that way's line.
    way: Option<NodeId>,
    /// The rest of the ban on its way on, for the wait a refusal tells.
    pushback: Duration,
    /// What the gate found on its way on.
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

/// Where an entry whose hand-over failed stands (spec "Hand-over" step 6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Failure {
    /// A sleepy node's own message, handed to its anchor.
    OwnAtAnchor,
    /// A forward.
    Forward,
    /// A return whose parent is a neighbour not demoted.
    Return,
    /// A return whose parent is demoted or gone.
    ReturnWithoutParent,
    /// A delivery in its node's window.
    Deliver,
    /// A delivery whether or not its node is in a window.
    Direct,
}

/// Where an entry of `kind` stands once its hand-over failed:
/// `own_at_anchor`, a sleepy node's own message; `gone`, its receiver is
/// demoted or no longer in the neighbour table.
fn failure(kind: Kind, own_at_anchor: bool, gone: bool) -> Failure {
    match kind {
        Kind::Deliver => Failure::Deliver,
        Kind::Direct => Failure::Direct,
        _ if own_at_anchor => Failure::OwnAtAnchor,
        Kind::Forward => Failure::Forward,
        Kind::Return if gone => Failure::ReturnWithoutParent,
        Kind::Return => Failure::Return,
    }
}

/// What a refusal of its hand-over makes of an entry (spec "Bans").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Refusal {
    /// Waits in the delivery queue for its refuser, which is leaving the
    /// air and where it may end.
    WaitsForRefuser,
    /// A sleepy node's own message, given back to its application.
    GivenBack,
    /// A forward or a return, which goes round its refuser.
    GoesRound,
    /// A delivery, which has no other receiver: it waits.
    Waits,
}

/// What a refusal makes of an entry of `kind`: `waits_for_refuser`, its
/// refuser is leaving the air where it may end and the delivery queue
/// has a place; `sleepy_own`, a sleepy node's own message.
fn refusal(kind: Kind, waits_for_refuser: bool, sleepy_own: bool) -> Refusal {
    match kind {
        Kind::Deliver | Kind::Direct => Refusal::Waits,
        _ if waits_for_refuser => Refusal::WaitsForRefuser,
        Kind::Forward if sleepy_own => Refusal::GivenBack,
        Kind::Forward | Kind::Return => Refusal::GoesRound,
    }
}

/// Where routing sends a queued forward now: `N` its next hop, `P` its
/// parent (spec "Next hop", "Return to parent").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Way<N, P> {
    /// It ends here.
    EndsHere,
    /// It ends here, the application's own message, with no place here
    /// yet: set aside until there is.
    SetAside,
    /// Its next hop, or its parent, is banned until then.
    Banned(Instant),
    /// To its next hop.
    Next(N),
    /// Its origin holds it until then.
    WaitsUntil(Instant),
    /// Back to its parent.
    Parent(P),
    /// No candidate is left and it has no parent: its search ends.
    Exhausted,
}

/// Where routing sends a queued forward at `now`: `ends_here`, this node
/// owns its destination; `room_here`, it has a place here if it ends
/// here; `next`, its next hop and when that one's ban ends; `waits`,
/// until when its origin holds it with no next hop; `parent`, its parent
/// and when that one's ban ends.
fn way<N, P>(
    ends_here: bool,
    room_here: bool,
    next: Option<(N, Instant)>,
    waits: Option<Instant>,
    parent: Option<(P, Instant)>,
    now: Instant,
) -> Way<N, P> {
    if ends_here {
        return match room_here {
            true => Way::EndsHere,
            false => Way::SetAside,
        };
    }
    match (next, waits, parent) {
        (Some((_, ban)), _, _) if ban > now => Way::Banned(ban),
        (Some((n, _)), _, _) => Way::Next(n),
        (None, Some(t), _) => Way::WaitsUntil(t),
        (None, None, Some((_, ban))) if ban > now => Way::Banned(ban),
        (None, None, Some((p, _))) => Way::Parent(p),
        (None, None, None) => Way::Exhausted,
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

/// A full DATA frame with the long preamble, 372.8 ms
/// (spec "Duty-cycled receive and the long preamble"): what a relay must
/// spare to send a message at once to a sleepy node, whose length the
/// routing does not know (spec "Next hop").
const LONG_FRAME: Duration = time_on_air(MAX_FRAME as u8, LONG_PREAMBLE_SYMBOLS);

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
    /// The second it began to drain for a reboot, rounded up, taking no message
    /// (`relay/drain.rs`), or [`u32::MAX`] while it does not: whole
    /// seconds, which fit beside `feed_due`, are fine enough for a bound
    /// of minutes.
    pub(super) drain_from_s: u32,
    /// When the send queue was last looked at with nothing to send: the
    /// earliest end of a wait or a ban that held an entry back.
    pub(super) wake: Option<Instant>,
    /// When the bucket will fit the DATA to a sleepy node the send queue
    /// last had no room for, as a hand-over the bucket cannot fit yet is
    /// deferred (`relay/delivery.rs`).
    pub(super) feed_at: Option<Instant>,
    /// When queued deliveries are next retried, every 60 s, under the
    /// `sleepy_listens` research switch.
    pub(super) delivery_retry_at: Instant,
    /// When the bucket will have refilled what the senders it refused
    /// were told to come back for (spec "Bans").
    pub(super) promised: Instant,
}

impl Forwarding {
    pub(super) const IDLE: Forwarding = Forwarding {
        flight: Flight::NONE,
        feed_due: false,
        drain_from_s: u32::MAX,
        wake: None,
        feed_at: None,
        delivery_retry_at: Instant::ZERO,
        promised: Instant::ZERO,
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
    /// Before this the application is not offered a send: the wait the
    /// last refusal of its message asked for
    /// (spec "A sleepy node's own message").
    pub(super) retry_at: Instant,
    /// When the bucket lets the application send, if only it is missing.
    pub(super) app_ready_at: Option<Instant>,
}

impl OwnMessage {
    pub(super) const NONE: OwnMessage = OwnMessage {
        id: None,
        refusals: 0,
        retry_at: Instant::ZERO,
        app_ready_at: None,
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
                self.radio.budget().refilled_at(need, now).since(now)
            }
            _ => BAN,
        };
        // Only the room is a line; a wait for the way on passes on as it
        // is, so it cannot grow around a loop of relays.
        let slot = self.forwarding.promised.max(now + room);
        self.forwarding.promised = slot + BAN;
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
        dst: (NodeId, Position),
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
        if o.in_flight() {
            return Some(o.to);
        }
        self.heading(o, now).map(|(to, _, _)| to)
    }

    /// Where queued forward `o` goes if routed now, when that one's ban
    /// ends, and where its record is: `None` if this relay owns its
    /// destination now, or it has no record.
    fn heading(&self, o: &Outgoing, now: Instant) -> Option<(NodeId, Instant, u16)> {
        let (at, record) = self.records.find(o.msg_id(), o.record)?;
        let dst = (o.message.dst_id, o.message.dst_pos);
        if self.ends_here(dst, Some(record), o.passed, None, now) {
            return None;
        }
        self.heads_to(record, o.passed, o.avoid, dst, now)
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
        const N: usize = SEND_QUEUE + HOP_SLOTS;
        let mut banned = [false; N];
        let mut hints = [None::<u16>; N];
        let mut toward = [0u8; N];
        let mut wake: Option<Instant> = None;
        let still_banned = self.banned_slots(now);
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
            if let Some((to, ban, at)) = self.heading(o, now) {
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
        for o in self.queue.iter().filter(|o| o.msg_id() != id) {
            if self.bound_for(o, now) == Some(to) {
                ahead += 1;
                barred |= o.hops.saturating_sub(1) >= hops;
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
            .budget()
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
        let current_air = current.map_or(Duration::ZERO, Outgoing::airtime);
        let queued = self.queue.airtime() - current_air;
        let regular = self.regular() - usize::from(current.is_some_and(|o| o.place == 0));
        let why = if regular >= SEND_QUEUE {
            DropReason::SendQueueFull
        } else if self.radio.budget().available(now) < queued + airtime + spend + REPLY_RESERVE {
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
            .budget()
            .would_allow_keeping(airtime, HEARTBEAT_RESERVE, now)
    }

    fn drop_msg(&mut self, msg_id: MsgId, reason: DropReason, emit: &mut dyn FnMut(Action)) {
        if self.own.id == Some(msg_id) {
            self.set_own(None);
        }
        emit(Action::Trace(Trace::Dropped { msg_id, reason }));
    }

    // --- Owner and next hop (spec "Next hop", "Owner") ---

    /// Whether the message of `record`, if it has one here, to `dst_id`
    /// at `dst_pos`, ends here: addressed to this node, or its position
    /// owned here while the addressed node is no live neighbour it can
    /// be handed to ([`Self::addressed_open`]). `from` is the node it
    /// comes from now, if any.
    fn ends_here(
        &self,
        (dst_id, dst_pos): (NodeId, Position),
        record: Option<&Record>,
        passed: u64,
        from: Option<NodeId>,
        now: Instant,
    ) -> bool {
        dst_id == self.me()
            || (self.owns(dst_pos, now) && !self.addressed_open(dst_id, record, passed, from, now))
    }

    /// Whether `dst_id` is a live neighbour that is no sleepy node with an
    /// anchor and that the message of `record` may still be handed to:
    /// not tried, not its parent, not `from`. The owner of the position
    /// then hands it over at once rather than queue it for the node's
    /// beat on its channel (spec "Owner", "Next hop").
    fn addressed_open(
        &self,
        dst_id: NodeId,
        record: Option<&Record>,
        passed: u64,
        from: Option<NodeId>,
        now: Instant,
    ) -> bool {
        let parent = record.and_then(|r| r.parent()).map(|(name, _)| name);
        self.neighbours.live(now).any(|(slot, n)| {
            n.id == dst_id
                && !n.last_resort()
                && Some(n.id) != parent
                && Some(n.id) != from
                && !record.is_some_and(|r| tried(r, passed, slot))
        })
    }

    /// Whether this relay owns the destination `dst` of another node's
    /// message (spec "Owner"). A sleepy node owns none, nor does a
    /// carried one, nor one leaving to turn sleepy (`relay/power.rs`):
    /// they hold mail for nobody, and a neighbour that is carried keeps
    /// no other from owning.
    fn owns(&self, dst: Position, now: Instant) -> bool {
        if self.sleepy() || self.motion.carried() || self.leaving() {
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

    /// Whether the sleepy neighbour `n` with an anchor, a message's
    /// destination, is sent the message now with the long preamble
    /// rather than reached through its anchor (spec "Next hop"): it has
    /// not left such a hand-over unanswered since it was last heard, and
    /// this node's bucket spares a full long frame ([`Self::spares_long`]).
    fn reach_now(&self, n: &Neighbour, now: Instant) -> bool {
        !n.unanswered && self.spares_long(LONG_FRAME, None, now)
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

    /// The next hop for the message of `record` to node and position `dst`
    /// (spec "Next hop"), from the state as it is at `now`: the addressed
    /// node itself if it is a live neighbour not in the tried set, a stale
    /// carried one too, a sleepy one with an anchor only while this relay
    /// spares the long frame ([`Self::reach_now`]), else as a last resort;
    /// otherwise the live neighbour nearest the position
    /// that is not in the tried set, ties broken by ID; a demoted one only
    /// when no other with margin is left, and one without margin only when
    /// no other at all (roadmap D25, D29, D38). A banned next hop is waited
    /// for, not gone round (roadmap D106), unless it refused this message:
    /// the slots of `avoid` are gone round while banned where another is
    /// left that is no worse a last resort (roadmap D110). A neighbour
    /// about to leave the air is gone round the same way, unless the
    /// message may end there ([`Self::ends_at`], roadmap D181).
    fn next_hop(
        &self,
        record: &Record,
        passed: u64,
        avoid: u64,
        dst: (NodeId, Position),
        now: Instant,
    ) -> Option<(usize, Neighbour)> {
        let hop = self.choose(record, passed, dst, now);
        let (slot, n) = hop?;
        let refused = avoid & (1u64 << slot) != 0 && self.neighbours.is_banned(&n, now);
        if !refused && !(n.leaving && !self.ends_at(&n, dst)) {
            return hop;
        }
        let round = (avoid & self.banned_slots(now)) | self.leaving_slots(dst, now);
        self.choose(record, passed | round, dst, now)
            .filter(|(_, m)| !self.last_resort(m, now) || self.last_resort(&n, now))
            .or(hop)
    }

    /// [`Self::next_hop`] with every slot of `passed` taken out.
    fn choose(
        &self,
        record: &Record,
        passed: u64,
        (dst_id, dst): (NodeId, Position),
        now: Instant,
    ) -> Option<(usize, Neighbour)> {
        let target = self.plane.point(dst);
        // Only a carried node itself can end the search for it, and a
        // frame to it costs little against a search through the city:
        // first even when stale (roadmap D116a). A sleepy node with an
        // anchor is first only while this relay spares the long frame;
        // otherwise its anchor delivers it (roadmap D134).
        let addressed = |n: &Neighbour| n.id == dst_id;
        let parent = record.parent().map(|(name, _)| name);
        // Computed once: it takes a logarithm.
        let margin = Neighbours::margin_dbm(self.radio.busy().noise_floor_dbm());
        let faint = |n: &Neighbour| n.rssi < margin;
        let demoted = |n: &Neighbour| self.neighbours.is_demoted(n, now);
        let first = self.first_hop(now).filter(|_| record.parent().is_none());
        let others = self.others_open(record, passed, now);
        self.neighbours
            .live(now)
            .filter(|(slot, n)| {
                first.is_none_or(|a| n.id == a)
                    && (first.is_some()
                        || (addressed(n) && (!n.last_resort() || self.reach_now(n, now)))
                        || self.tier(n, others, now))
                    && !tried(record, passed, *slot)
                    && Some(n.id) != parent
            })
            .min_by(|(_, a), (_, b)| {
                let (da, db) = (a.local.distance2(target), b.local.distance2(target));
                addressed(b)
                    .cmp(&addressed(a))
                    .then(faint(a).cmp(&faint(b)))
                    .then(demoted(a).cmp(&demoted(b)))
                    .then(da.total_cmp(&db))
                    .then(a.id.0.cmp(&b.id.0))
            })
            .map(|(slot, n)| (slot, *n))
    }

    /// The neighbour-table slots of the live neighbours about to leave the
    /// air at `now`, but for those where a message to `dst` may end.
    fn leaving_slots(&self, dst: (NodeId, Position), now: Instant) -> u64 {
        self.neighbours
            .live(now)
            .filter(|(_, n)| n.leaving && !self.ends_at(n, dst))
            .fold(0, |m, (slot, _)| m | 1u64 << slot)
    }

    /// Whether a message to `dst` may end at `n`: it is the destination, or
    /// it holds mail within the owner radius of the position, so may own it
    /// (spec "Owner"). Gone round such a node, a message would only wander
    /// among its neighbours, each passing it over in turn, to the hop cap.
    fn ends_at(&self, n: &Neighbour, (dst_id, dst): (NodeId, Position)) -> bool {
        n.id == dst_id
            || (n.holds_mail()
                && n.local.distance2(self.plane.point(dst)) <= OWNER_RADIUS_M * OWNER_RADIUS_M)
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
        let v = self.verdict(&d, completes, now);
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
            seen: v.arrival.case == Case::Seen,
        };
        if !self.radio.can_owe(reply, d.sender_home, now) {
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
                    .owe(reply, d.sender_home, deadline, false, None, now);
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
        // (spec "The ACK"); one that is no work is acted on now: nothing
        // is lost if its ACK never goes.
        let later = v.arrival.acts();
        let takes = later.then(|| Take::of(&d));
        self.radio
            .owe(reply, d.sender_home, deadline, renews, takes, now);
        self.note_untaken(emit);
        if completes {
            self.handover_done(now, emit);
        }
        if !later {
            self.act_on(&d, v.arrival, Room::default(), now, emit);
        }
    }

    /// Whether `d`, just arrived, is taken or refused, and what it is
    /// (spec "Receiving DATA"), with the ACK that would take it still to
    /// pay for. `completes`: it comes from the neighbour being handed the
    /// message, whose hand-over ends first, giving up its place.
    fn verdict(&self, d: &Data<'_>, completes: bool, now: Instant) -> Verdict {
        let arrival = self.arrival(d, completes, now);
        let heads = self.way_on(d, &arrival, now);
        let gate = heads.map(|(to, _)| self.gate(d.msg_id(), to, d.hops, now));
        let refusal = if !arrival.acts() {
            None
        } else if self.draining() {
            Some(DropReason::Draining)
        } else {
            self.room(d, &arrival, heads, gate, ACK_AIRTIME, completes, now)
                .err()
        };
        Verdict {
            arrival,
            refusal,
            way: heads.map(|(to, _)| to),
            pushback: heads.map_or(Duration::ZERO, |(_, ban)| ban.since(now)),
            gate,
        }
    }

    /// What remains to be checked of `d` once its take is final, its ACK
    /// gone out (spec "The ACK"): what it is now, since a hand-over may
    /// have ended or the message arrived while the ACK waited for the
    /// radio, and whether the room it takes is still there. Not the gate
    /// nor the drain: its sender is done with it.
    fn when_taken(&self, d: &Data<'_>, now: Instant) -> Result<(Arrival, Room), DropReason> {
        let arrival = self.arrival(d, false, now);
        if !arrival.acts() {
            return Ok((arrival, Room::default()));
        }
        let heads = self.way_on(d, &arrival, now);
        self.room(d, &arrival, heads, None, Duration::ZERO, false, now)
            .map(|room| (arrival, room))
    }

    /// What `d` is to this relay: its case of spec "Receiving DATA" and
    /// what that was decided from. `completes`: as for [`Self::verdict`].
    fn arrival(&self, d: &Data<'_>, completes: bool, now: Instant) -> Arrival {
        let id = d.msg_id();
        let record = self.records.get(id);
        let repeat = record.is_some_and(|r| r.repeats(d.src_hop, d.hops));
        // From a neighbour that has the message from this relay: a return
        // (spec "Receiving DATA" case 5). The one being handed to has it
        // too, though its ACK has not come.
        let slot = self.neighbours.slot_of(d.src_hop);
        let returned = completes || record.zip(slot).is_some_and(|(r, s)| r.handed(s));
        let owner = self.ends_here((d.dst_id, d.dst_pos), record, 0, Some(d.src_hop), now);
        let case = self.classify(id, repeat, returned, owner);
        Arrival {
            case,
            slot,
            copy: record.is_some_and(|r| !repeat && !r.arrived()),
            returned,
            owner: owner && matches!(case, Case::New | Case::Search),
            // Count 0: a sleepy node's own message, fresh work that starts
            // its search here (spec "Packet sender", "Hop count").
            fresh: d.hops == 0,
        }
    }

    /// Where the message of `d`, to send on, would go from here now, and
    /// when that one's ban ends: routed as the record it would have, whose
    /// sender, a sleepy node handing over its own or a child returning
    /// it, has it.
    fn way_on(&self, d: &Data<'_>, a: &Arrival, now: Instant) -> Option<(NodeId, Instant)> {
        if !a.acts() || a.owner {
            return None;
        }
        let id = d.msg_id();
        let mut probe = match self.records.get(id) {
            Some(r) if a.case == Case::Search => *r,
            _ => Record::new(
                id,
                (!a.fresh).then_some((d.src_hop, d.sender_home)),
                d.hops,
                now,
            ),
        };
        if let Some(s) = a.slot.filter(|_| a.fresh || a.case == Case::Search) {
            probe.handed_to(s);
        }
        self.heads_to(&probe, 0, 0, (d.dst_id, d.dst_pos), now)
    }

    /// The room a packet with work takes, or why there is none (spec
    /// "Receiving DATA"): a place for a new record; for one to send on,
    /// past the gate, a send-queue place, and for fresh work no ban on its
    /// way on, `heads`; for one to deliver, a place in the delivery queue
    /// or the inbox and the budget. `spend`: the ACK that takes it, if it
    /// has not gone out; `gate`: asked only then; `completes`: as for
    /// [`Self::verdict`].
    #[allow(clippy::too_many_arguments)]
    fn room(
        &self,
        d: &Data<'_>,
        a: &Arrival,
        heads: Option<(NodeId, Instant)>,
        gate: Option<Gate>,
        spend: Duration,
        completes: bool,
        now: Instant,
    ) -> Result<Room, DropReason> {
        let record_at = match a.case {
            Case::New => self.record_place(),
            _ => None,
        };
        if a.case == Case::New && record_at.is_none() {
            return Err(DropReason::NoRecord);
        }
        if gate.is_some_and(|g| g.over) {
            return Err(DropReason::SameHop);
        }
        if a.owner {
            if !self.has_room_for(d.dst_id, now) {
                return Err(DropReason::DeliveryFull);
            }
            if !self.affordable(spend, now) {
                return Err(DropReason::Budget);
            }
            return Ok(Room {
                record_at,
                place: 0,
            });
        }
        let air = self.data_airtime(d.payload.len());
        let place = self.admit(air, spend, d.hops, completes, now)?;
        // Fresh work is refused while its way on is banned, and told how
        // long (spec "Receiving DATA"). A message already in the network
        // is not: refused for that, its sender would send it round the
        // pushback, and around a ring of relays the refusals kept each
        // other up.
        let banned = heads.is_some_and(|(_, ban)| ban > now);
        if a.fresh && (self.bans_end(now).is_some() || banned) {
            return Err(DropReason::Banned);
        }
        Ok(Room { record_at, place })
    }

    /// Act on `d`, which is `a` to this relay and taken into `room`: its
    /// record, then its work.
    fn act_on(
        &mut self,
        d: &Data<'_>,
        a: Arrival,
        room: Room,
        now: Instant,
        emit: &mut dyn FnMut(Action),
    ) {
        let id = d.msg_id();
        if a.case == Case::New {
            let parent = (!a.fresh).then_some((d.src_hop, d.sender_home));
            let mut record = Record::new(id, parent, d.hops, now);
            if a.owner {
                record.set_arrived();
            }
            if let (true, Some(slot)) = (a.fresh, a.slot) {
                // The sleepy node it came from has it, and would end the
                // search there: it is tried.
                record.handed_to(slot);
            }
            if let Some(at) = room.record_at {
                self.records.put(at, record);
            }
        } else if let Some(r) = self.records.get_mut(id) {
            if a.case == Case::Seen {
                // The sender has the message and searches on from its own
                // record; it need not be offered this one.
                r.seen(d.src_hop, d.hops, now);
                if let Some(s) = a.slot {
                    r.handed_to(s);
                }
            } else {
                r.received(d.src_hop, d.hops, now);
            }
            if a.owner {
                r.set_arrived();
            }
        }
        if a.copy {
            emit(Action::Trace(Trace::Copy {
                msg_id: id,
                returned: a.returned,
                merged: a.case == Case::AckOnly,
                seen: a.case == Case::Seen,
            }));
        }
        match a.case {
            Case::AckOnly => {
                self.delivered.offered(id, now);
            }
            // The copy stays with its sender, which goes on.
            Case::Seen => {}
            Case::New | Case::Search if a.owner => {
                self.arrive(id, d, now, emit);
            }
            Case::New | Case::Search => self.send_on(d, room.place, now, emit),
        }
    }

    /// The ACK that takes the DATA of `take` has left the antenna: the
    /// take is final, and the relay acts on it now (spec "The ACK").
    /// Nothing has changed since it was answered taken but the ACK's own
    /// airtime, spent; should the room be gone after all, the copy ends
    /// here, since its sender is done with it.
    pub(super) fn taken(&mut self, take: Take, now: Instant, emit: &mut dyn FnMut(Action)) {
        let d = take.data(self.me());
        match self.when_taken(&d, now) {
            Ok((a, room)) => self.act_on(&d, a, room, now, emit),
            Err(why) => self.drop_msg(d.msg_id(), why, emit),
        }
    }

    /// Report a DATA that a reply given up left untaken: refused, for its
    /// sender retries it (spec "The ACK").
    fn note_untaken(&mut self, emit: &mut dyn FnMut(Action)) {
        if let Some(msg_id) = self.radio.untaken() {
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
            .any(|(i, o)| Some(i) != current && o.msg_id() == id)
    }

    /// Whether the current hand-over is of message `id` to `to`, sent at
    /// least once.
    fn handing(&self, id: MsgId, to: NodeId) -> bool {
        self.in_flight()
            .filter(|o| o.transmitted())
            .map(|o| o.expect(self.me()))
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
            for id in self.queue.iter().map(Outgoing::msg_id) {
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
        let out = Outgoing {
            place,
            since: now,
            ..self.onward(Message::of(d), d.hops)
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
        let Some(mut q) = Queued::new(id, d.dst_pos, node, payload, now) else {
            self.drop_msg(id, DropReason::PayloadTooLong, emit);
            return;
        };
        q.hops = d.hops;
        if node == self.me() {
            self.delivered.insert(id, now);
            // The inbox keeps it until the application confirms it, by the
            // delivery queue's rule (spec "Delivery queue").
            if let Some(gone) = self.inbox.make_room(now) {
                emit(Action::Trace(Trace::Dropped {
                    msg_id: gone.msg_id,
                    reason: DropReason::QueueExpired,
                }));
            }
            match self.inbox.push(q) {
                Pushed::Queued => emit(Action::App(AppEvent::Message { msg_id: id })),
                Pushed::Duplicate => {}
                // Only when ownership is found at the hand-over, as below.
                Pushed::Full => self.drop_msg(id, DropReason::DeliveryFull, emit),
            }
            return;
        }
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

    /// A node that turned sleepy holds mail for nobody: what its delivery
    /// queue still holds goes into its send queue, oldest first, as
    /// forwards from here toward their addresses, admitted as any forward
    /// received with their count; what finds no room waits for it here
    /// (spec "Sleepy nodes"). Each starts a search here, with no parent,
    /// so that a copy that comes back is searched on and not taken for
    /// delivered.
    pub(super) fn hand_on(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        loop {
            let Some(q) = self.delivery.iter().next() else {
                return;
            };
            let (id, hops) = (q.msg_id, q.hops);
            if hops == MAX_HOPS {
                self.delivery.remove(id);
                self.drop_msg(id, DropReason::HopCap, emit);
                continue;
            }
            let out = self.onward(q.message(), hops);
            let Ok(place) = self.admit(out.airtime(), Duration::ZERO, hops, false, now) else {
                return;
            };
            self.records.forget(id);
            // The table outnumbers what a node holds (`tables/records.rs`).
            let Some(at) = self.record_place() else {
                return;
            };
            self.records.put(at, Record::new(id, None, hops, now));
            if !self.push_forward(out, place, now) {
                return;
            }
            self.delivery.remove(id);
            emit(Action::Trace(Trace::HandedOn { msg_id: id }));
        }
    }

    /// Whether the forward `out`, refused by `by`, waits for it in the
    /// delivery queue: `by` is leaving the air and the message may end
    /// there ([`Self::ends_at`]); sent round it, it would only wander
    /// among its neighbours to the hop cap (spec "Delivery queue"). Not
    /// at a node that is sleepy or leaving to turn sleepy, which holds
    /// mail for nobody, nor where the queue has no place; then it goes
    /// round as any refused forward.
    fn waits_for_refuser(&self, out: &Outgoing, by: NodeId, now: Instant) -> bool {
        if self.sleepy() || self.leaving() {
            return false;
        }
        let m = &out.message;
        let ends = self
            .neighbours
            .get(by)
            .is_some_and(|n| n.leaving && self.ends_at(n, (m.dst_id, m.dst_pos)));
        let id = m.id();
        ends && (self.delivery.has_room(now) || self.delivery.iter().any(|q| q.msg_id == id))
    }

    /// The current forward `out` leaves the send queue to wait for `by`
    /// in the delivery queue ([`Self::waits_for_refuser`]).
    fn wait_for(&mut self, out: &Outgoing, by: NodeId, now: Instant, emit: &mut dyn FnMut(Action)) {
        let m = &out.message;
        let id = m.id();
        let Some(mut q) = Queued::new(id, m.dst_pos, m.dst_id, m.payload(), now) else {
            return;
        };
        // The count it arrived with, which it goes on from (`release`).
        q.hops = self
            .records
            .get(id)
            .map_or(out.hops.saturating_sub(1), Record::highest_hops);
        q.waits_for = by;
        self.make_room(now, emit);
        let pushed = self.delivery.push(q);
        debug_assert!(pushed != Pushed::Full, "a place was found for it");
        self.cancel_handover();
        self.queue.take_current();
        self.forwarding.feed_due = true;
        if self.own.id == Some(id) {
            // The node keeps it now, as a first hop would.
            self.set_own(None);
        }
        emit(Action::Trace(Trace::Waits {
            msg_id: id,
            node: by,
        }));
    }

    /// Whether a message that waits for `id` waits no longer at `now`:
    /// `id`'s last heartbeat no longer says it is leaving, as after its
    /// swap, or it is no live neighbour.
    fn waited(&self, id: NodeId, now: Instant) -> bool {
        self.neighbours
            .live(now)
            .find(|(_, n)| n.id == id)
            .is_none_or(|(_, n)| !n.leaving)
    }

    /// Each message that waits for a neighbour that has come back, or is
    /// gone, goes into the send queue as a forward from here, admitted as
    /// any forward received with its count, and is routed as any: to that
    /// neighbour, where it ends, or on as the state now says. What finds
    /// no room waits for it (spec "Delivery queue", roadmap D186).
    pub(super) fn release(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        if self.sleepy() {
            // What it held goes on by `hand_on`.
            return;
        }
        loop {
            let Some(q) = self
                .delivery
                .iter()
                .find(|q| q.waits() && self.waited(q.waits_for, now))
                .copied()
            else {
                return;
            };
            let (id, hops) = (q.msg_id, q.hops);
            if hops == MAX_HOPS {
                self.delivery.remove(id);
                self.drop_msg(id, DropReason::HopCap, emit);
                continue;
            }
            let out = self.onward(q.message(), hops);
            let Ok(place) = self.admit(out.airtime(), Duration::ZERO, hops, false, now) else {
                return;
            };
            if self.records.get(id).is_none() {
                // Its record made way meanwhile: its search starts here.
                let Some(at) = self.record_place() else {
                    return;
                };
                self.records.put(at, Record::new(id, None, hops, now));
            }
            if !self.push_forward(out, place, now) {
                return;
            }
            self.delivery.remove(id);
            emit(Action::Trace(Trace::Released { msg_id: id }));
        }
    }

    /// `message` as a forward this node sends on, going on from count
    /// `hops`, which places it among waiting entries until its hand-over
    /// stamps the count it goes with (spec "Hop count"), addressed when
    /// its hand-over starts.
    pub(super) fn onward(&self, message: Message, hops: u8) -> Outgoing {
        Outgoing::new(
            message,
            hops.saturating_add(1),
            NodeId::NONE,
            self.home,
            self.forward_preamble(),
            Kind::Forward,
        )
    }

    /// Queue the forward `out`, whose record is in the table, into
    /// `place`: whether it was.
    pub(super) fn push_forward(&mut self, out: Outgoing, place: u8, now: Instant) -> bool {
        let out = Outgoing {
            place,
            since: now,
            ..out
        };
        let pushed = self.queue.push(out).is_ok();
        self.forwarding.feed_due |= pushed;
        pushed
    }

    // --- The relay's own application (spec "Routing") ---

    /// Whether the application may send now (roadmap D29 rule 6, D34): the
    /// relay is running, holds no message of the application's, its send
    /// queue takes a full frame on top of what it holds, as a sleepy node's
    /// would be taken but without the ACK, and not every neighbour is
    /// banned; for a sleepy node with a live anchor, not the anchor.
    pub(super) fn accepts_send(&self, now: Instant) -> bool {
        self.own.id.is_none()
            && !self.draining()
            && now >= self.own.retry_at
            && self
                .no_room(self.data_airtime(MAX_PAYLOAD), Duration::ZERO, now)
                .is_none()
            && self.bans_end(now).is_none()
    }

    /// When the application may send again, if only the bucket and bans
    /// keep it waiting; anything else ends with an event.
    pub(super) fn send_ready_at(&self, now: Instant) -> Option<Instant> {
        if self.own.id.is_some() || self.draining() || self.regular() >= SEND_QUEUE {
            return None;
        }
        let need = self.queue.airtime() + self.data_airtime(MAX_PAYLOAD) + REPLY_RESERVE;
        let bucket = self
            .radio
            .budget()
            .allowed_at(Duration::ZERO, need, now)
            .filter(|&t| t > now);
        let retry = Some(self.own.retry_at).filter(|&t| t > now);
        [bucket, self.bans_end(now), retry]
            .into_iter()
            .flatten()
            .max()
    }

    /// The application's message is now `own`; a new one, or none, has
    /// been refused no time yet.
    fn set_own(&mut self, own: Option<MsgId>) {
        self.own.id = own;
        self.own.refusals = 0;
    }

    /// Deliver the message set aside once the delivery queue has a place.
    pub(super) fn release_held(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        let Some(i) = self.queue.iter().position(Outgoing::held) else {
            return;
        };
        let room = self
            .queue
            .get(i)
            .is_none_or(|h| self.has_room_for(h.message.dst_id, now));
        if !room {
            return;
        }
        let Some(h) = self.queue.remove(i) else {
            return;
        };
        self.arrive(h.msg_id(), &h.data(self.me(), self.home), now, emit);
    }

    /// Whether a message for `node` that ends here at `now` finds a place:
    /// in the inbox if it is for this node, else in the delivery queue
    /// (spec "Delivery queue").
    pub(super) fn has_room_for(&self, node: NodeId, now: Instant) -> bool {
        if node == self.me() {
            self.inbox.has_room(now)
        } else {
            self.delivery.has_room(now)
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
            payload,
        };
        if self.ends_here((dst_id, dst_pos), None, 0, None, now) {
            let mut record = Record::new(id, None, 0, now);
            record.set_arrived();
            self.records.put(at, record);
            if !self.has_room_for(dst_id, now) {
                // At its owner the delivery queue's rule ends it
                // (spec "Delivery queue"). Admission left it a place, as for any message of the
                // application's.
                let out = Outgoing {
                    state: State::Held,
                    since: now,
                    ..Outgoing::new(
                        Message::of(&d),
                        d.hops,
                        NodeId::NONE,
                        self.home,
                        self.forward_preamble(),
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
        if let Some((to, ban)) = self.heads_to(&record, 0, 0, (dst_id, dst_pos), now) {
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
                self.own.retry_at = now + wait;
                self.drop_msg(id, DropReason::Pushback, emit);
                emit(Action::App(AppEvent::GivenBack {
                    msg_id: id,
                    retry_at: self.own.retry_at,
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
                Kind::Return => Routed::To(out),
                // Stays in the delivery queue while its node sleeps.
                Kind::Deliver if self.listening(out.to, now) || self.early(&out) => Routed::To(out),
                Kind::Deliver => {
                    emit(Action::Trace(Trace::DeliveryWaits {
                        msg_id: out.msg_id(),
                        node: out.to,
                        tried: false,
                    }));
                    Routed::Done
                }
                // Goes whether or not its node is in a window
                // (`relay/delivery.rs`).
                Kind::Direct => Routed::To(out),
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
        let waiting = matches!(current.state, State::Waiting(_)) && self.radio.idle();
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
    /// long preamble, to a node not known to be out of reach (spec
    /// "Delivery", `relay/delivery.rs`).
    fn early(&self, out: &Outgoing) -> bool {
        out.preamble >= LONG_PREAMBLE_SYMBOLS && self.heard.get(out.to).is_some_and(|n| !n.missed)
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
        let id = out.msg_id();
        let dst = (out.message.dst_id, out.message.dst_pos);
        debug_assert!(
            self.records.get(id).is_some(),
            "a held message keeps its record"
        );
        let Some(record) = self.records.get(id).copied() else {
            self.drop_msg(id, DropReason::NoRecord, emit);
            return Routed::Done;
        };
        let next = self
            .next_hop(&record, out.passed, out.avoid, dst, now)
            .map(|(_, n)| (n, n.banned_until));
        let parent = record.parent().map(|(p, stored)| {
            let ban = self
                .neighbours
                .get(p)
                .map_or(Instant::ZERO, |n| n.banned_until);
            ((p, stored), ban)
        });
        let found = way(
            self.ends_here(dst, Some(&record), out.passed, None, now),
            self.own.id != Some(id) || self.has_room_for(out.message.dst_id, now),
            next,
            self.origin_waits(&record, id, now),
            parent,
            now,
        );
        match found {
            Way::EndsHere => {
                if let Some(r) = self.records.get_mut(id) {
                    r.set_arrived();
                }
                self.arrive(id, &out.data(self.me(), self.home), now, emit);
                Routed::Done
            }
            Way::SetAside => {
                if let Some(r) = self.records.get_mut(id) {
                    r.set_arrived();
                }
                Routed::Hold
            }
            // A ban is a wait, not an exclusion (spec "Next hop").
            Way::Banned(t) => Routed::Banned(t),
            // Addressed afresh, its attempts start afresh.
            Way::Next(n) => Routed::To(Outgoing {
                to: n.id,
                channel: n.home,
                preamble: preamble(&n),
                kind: Kind::Forward,
                tries: 0,
                state: State::Queued,
                ..out
            }),
            Way::WaitsUntil(t) => Routed::Wait(t),
            Way::Parent((parent, stored)) => Routed::To(Outgoing {
                to: parent,
                channel: self.home_of(parent).unwrap_or(stored),
                preamble: self.preamble_to(parent),
                kind: Kind::Return,
                tries: 0,
                state: State::Queued,
                ..out
            }),
            Way::Exhausted => {
                self.drop_msg(id, DropReason::NoRoute, emit);
                Routed::Done
            }
        }
    }

    /// Until when an origin holds message `id` of `record` while it has
    /// no candidate, rather than end its search: its application's own
    /// in its first rotation until it hears a relay or the rotation ends
    /// (spec "Return to parent", `heard`); at a sleepy node, its own or
    /// one it held when it turned sleepy, while it hears no neighbour,
    /// routed again every fade (spec "A sleepy node's own message",
    /// "Sleepy nodes").
    fn origin_waits(&self, record: &Record, id: MsgId, now: Instant) -> Option<Instant> {
        if let Some(ends) = self.boot.ends() {
            if self.own.id == Some(id) && !self.neighbours.live(now).any(|(_, n)| n.relay()) {
                return Some(ends);
            }
        }
        let alone = self.neighbours.live(now).next().is_none();
        (self.sleepy() && record.parent().is_none() && alone).then(|| now + self.config.fade_retry)
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
            .map_or(MIN_PREAMBLE_SYMBOLS, |o| o.preamble);
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
        let to = out.to;
        let (channel, preamble) = match out.kind {
            // Its preamble is chosen when it is queued.
            Kind::Direct => (self.home_of(to), None),
            Kind::Forward | Kind::Return => {
                (self.home_of(to), self.neighbours.get(to).map(preamble))
            }
            // Its holder moves its sleepy nodes with it (`take_listeners`);
            // one that listens now needs no long preamble.
            Kind::Deliver => (
                self.heard.get(to).map(|n| n.home),
                self.listening(to, now).then_some(MIN_PREAMBLE_SYMBOLS),
            ),
        };
        out.channel = channel.unwrap_or(out.channel);
        out.preamble = preamble.unwrap_or(out.preamble);
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
        if !self.affordable(out.airtime(), now) {
            self.defer_handover(&out, now, emit);
            return false;
        }
        if let Some((mut h, _)) = self.handover() {
            h.begin_lbt(now);
        }
        self.radio.check(Job::HandOver, out.channel, emit);
        true
    }

    fn defer_handover(&mut self, out: &Outgoing, now: Instant, emit: &mut dyn FnMut(Action)) {
        let airtime = out.airtime();
        let until = self
            .radio
            .budget()
            .allowed_at(airtime, REPLY_RESERVE, now)
            .unwrap_or(now + self.config.heartbeat_max);
        if let Some((mut h, _)) = self.handover() {
            h.defer(until);
        }
        emit(Action::Trace(Trace::HandOverDeferred {
            msg_id: out.msg_id(),
            until,
        }));
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
        let Some(out) = self.stamp(out, now, emit) else {
            return;
        };
        let airtime = out.airtime();
        if !self.affordable(airtime, now) {
            // An owed reply spent the budget since the check began.
            self.defer_handover(&out, now, emit);
            return;
        }
        if let Some(current) = self.queue.current_mut() {
            current.hops = out.hops;
        }
        let frame = out.frame(self.me(), self.home);
        self.radio.send(Job::HandOver, frame, now, emit);
    }

    /// The current entry as it goes now, its count stamped, or `None`
    /// where it is dropped at the cap, or held for an owed reply.
    fn stamp(
        &mut self,
        out: Outgoing,
        now: Instant,
        emit: &mut dyn FnMut(Action),
    ) -> Option<Outgoing> {
        if self.radio.replies().wake_at().is_some_and(|t| t <= now) {
            if let Some((mut h, _)) = self.handover() {
                h.hold(now);
            }
            return None;
        }
        let id = out.msg_id();
        let Some(hops) = self.count(id, &out) else {
            self.queue.take_current();
            self.forwarding.feed_due = true;
            self.drop_msg(id, DropReason::HopCap, emit);
            return None;
        };
        if let Some(r) = self.records.get_mut(id) {
            r.sent(now);
        }
        Some(Outgoing { hops, ..out })
    }

    /// The count the current hand-over of `out`, of message `id`, carries
    /// (spec "Hop count"): stamped at its first transmission as the
    /// record's highest plus one, repeated by its retries. Stamped when
    /// queued, a frame would keep a count older than one this relay has
    /// since received or sent, and a relay that readdressed it would offer
    /// a count it had already used. `None` past the hop cap, but for a
    /// delivery: its search has ended, and the cap ends searches, so it
    /// goes at the cap.
    fn count(&self, id: MsgId, out: &Outgoing) -> Option<u8> {
        if out.transmitted() {
            return Some(out.hops);
        }
        debug_assert!(
            self.records.get(id).is_some(),
            "a held message keeps its record"
        );
        match self.records.get(id) {
            // A sleepy node hands its own message over as fresh work: the
            // node that takes it is its origin (spec "Hop count").
            Some(r)
                if self.anchored_sleepy() && self.own.id == Some(id) && r.parent().is_none() =>
            {
                Some(0)
            }
            Some(r) if out.kind.delivers() => Some(r.highest_hops().saturating_add(1)),
            Some(r) => r.highest_hops().checked_add(1),
            None => Some(out.hops),
        }
    }

    /// Listen-before-talk gave up on the current hand-over's channel.
    fn handover_gave_up(&mut self, out: Outgoing, now: Instant, emit: &mut dyn FnMut(Action)) {
        let (msg_id, to) = (out.msg_id(), out.to);
        match out.kind {
            Kind::Forward | Kind::Return => {
                // The channel says nothing about the neighbour: the same
                // neighbour again, after a retry wait (spec "Hand-over"
                // step 2, roadmap D96, D111).
                let unit = self.retry_unit();
                let Some((mut h, rng)) = self.handover() else {
                    return;
                };
                let until = h.channel_busy(now, rng, unit);
                emit(Action::Trace(Trace::ChannelBusy { msg_id, to, until }));
            }
            // A failed delivery stays in the delivery queue and waits for
            // its node's next beat, so a give-up counts as an attempt.
            Kind::Deliver | Kind::Direct => {
                let (max, retry) = (self.config.hop_attempts, self.retry_unit());
                let Some((mut h, rng)) = self.handover() else {
                    return;
                };
                let missed = h.lost_attempt(now, max, rng, retry);
                let until = self.forwarding.flight.until().max(now);
                emit(Action::Trace(Trace::ChannelBusy { msg_id, to, until }));
                if missed == Missed::Failed {
                    self.handover_failed(out, now, emit);
                }
            }
        }
    }

    /// An ACK arrived: if it answers the current hand-over, the receiver
    /// took the packet, refused it, or has seen it (spec "The ACK",
    /// "Bans").
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
        let (id, to) = (out.msg_id(), out.to);
        emit(Action::Trace(Trace::Seen { msg_id: id, by: to }));
        self.tried(id, to, out.hops, now, emit);
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

    /// The receiver of the current hand-over refused it (spec "Bans").
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
        let msg_id = out.msg_id();
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
        let waits = !out.kind.delivers() && self.waits_for_refuser(&out, by, now);
        let own = self.own.id == Some(msg_id);
        match refusal(out.kind, waits, self.sleepy() && own) {
            // Leaves for the delivery queue, to wait for the refuser.
            Refusal::WaitsForRefuser => self.wait_for(&out, by, now, emit),
            // Leaves and goes back to its application, which may offer it
            // again once the wait is over: refused at the origin, it does
            // not age here.
            Refusal::GivenBack => {
                self.queue.take_current();
                self.forwarding.feed_due = true;
                self.own.retry_at = now + wait;
                self.records.forget(msg_id);
                self.drop_msg(msg_id, DropReason::Pushback, emit);
                emit(Action::App(AppEvent::GivenBack {
                    msg_id,
                    retry_at: self.own.retry_at,
                }));
            }
            // Stays, to be routed again when its turn comes, round the
            // refuser for the wait it told: its queue that way is full,
            // and a neighbour with room may sit beside it.
            Refusal::GoesRound => {
                self.go_round(by);
                if own {
                    // Every refusal in a row counts, gone round or not:
                    // each is a frame and a reply on the full channels
                    // round the origin, so the origin offers no faster
                    // for having more neighbours to be refused by.
                    self.own.refusals = self.own.refusals.saturating_add(1);
                    if !self.goes_elsewhere(by, now) {
                        // Each offer costs the refuser a reply and hides its ACKs.
                        let until = now + wait.max(hold(self.own.refusals));
                        if let Some(current) = self.queue.current_mut() {
                            current.not_before = until;
                        }
                    }
                }
                self.cancel_handover();
                if let Some(current) = self.queue.current_mut() {
                    current.kind = Kind::Forward;
                    current.tries = 0;
                }
            }
            // Stays, waiting for the told wait with the transmissions it
            // has made: it has no other receiver.
            Refusal::Waits => {
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
        let (id, to) = (out.msg_id(), out.to);
        if out.kind.delivers() {
            self.answer_from(to, now, emit);
            let long = out.preamble >= LONG_PREAMBLE_SYMBOLS;
            let by = match (out.kind, long) {
                (Kind::Direct, true) => Delivery::Flush,
                (Kind::Direct, false) => Delivery::Relay,
                (_, true) => Delivery::Long,
                (_, false) => Delivery::Window,
            };
            return self.delivered_to(id, to, by, now, emit);
        }
        self.neighbours.took(to, out.since, now);
        self.tried(id, to, out.hops, now, emit);
        if to == out.message.dst_id {
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
                    hops: out.hops,
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
    fn handover_failed(&mut self, out: Outgoing, now: Instant, emit: &mut dyn FnMut(Action)) {
        let (id, to) = (out.msg_id(), out.to);
        let own_at_anchor = self.sleepy() && out.hops == 0;
        let mut gone = false;
        if !out.kind.delivers() {
            if out.preamble >= LONG_PREAMBLE_SYMBOLS && to == out.message.dst_id {
                // Not again at once before it beats: its anchor delivers
                // it (spec "Next hop").
                self.neighbours.unanswered(to);
            }
            self.count_failure(to, now, emit);
            if out.kind == Kind::Forward && !own_at_anchor {
                self.passed_if_demoted(to, now);
            }
            gone = self
                .neighbours
                .get(to)
                .is_none_or(|n| self.neighbours.is_demoted(n, now));
            let moves_on = match out.kind {
                Kind::Forward => {
                    let (passed, avoid) =
                        self.queue.current().map_or((0, 0), |o| (o.passed, o.avoid));
                    !own_at_anchor
                        && self
                            .goes_to(&out.message, passed, avoid, now)
                            .is_some_and(|next| next != to)
                }
                _ => gone,
            };
            emit(Action::Trace(Trace::HandOverFailed {
                msg_id: id,
                to,
                moves_on,
            }));
        }
        match failure(out.kind, own_at_anchor, gone) {
            // Waits a fade, doubled for each silence in a row, and tries
            // its anchor again: the ban's ladder would offer it every few
            // seconds to a node whose channel is full, and a fixed fade
            // every minute for as long as it waits.
            Failure::OwnAtAnchor => {
                let neighbour = self.neighbours.by_id(to).map(|(_, n)| *n);
                let ban = neighbour
                    .map(|n| n.banned_until)
                    .filter(|&t| t > now)
                    .unwrap_or(now);
                let silences = neighbour.map_or(1, |n| n.fail_count.max(1));
                let mut fade = self.config.fade_retry;
                for _ in 1..silences {
                    fade = fade.mul(2).min(self.config.delivery_ttl);
                }
                if let Some(current) = self.queue.current_mut() {
                    current.not_before = ban.max(now + fade);
                    current.tries = 0;
                }
            }
            // Stays, to be routed again when its turn comes, with fresh
            // attempts: banned now, the neighbour is waited for while it
            // ranks first, and the message goes elsewhere once its
            // demotion makes it a last resort. Most likely the frame, not
            // the ACK, was lost, so the neighbour stays untried.
            Failure::Forward | Failure::Return => {
                if let Some(current) = self.queue.current_mut() {
                    current.kind = Kind::Forward;
                    current.tries = 0;
                }
            }
            // Leaves, and ends.
            Failure::ReturnWithoutParent => {
                self.queue.take_current();
                self.forwarding.feed_due = true;
                self.drop_msg(id, DropReason::ReturnFailed, emit);
            }
            // Leaves; the delivery queue keeps it, for its node's next beat.
            Failure::Deliver => {
                self.heard.missed(to);
                emit(Action::Trace(Trace::DeliveryWaits {
                    msg_id: id,
                    node: to,
                    tried: true,
                }));
                self.queue.take_current();
                self.forwarding.feed_due = true;
            }
            // Leaves; the delivery queue keeps it, for its node heard again.
            Failure::Direct => {
                self.direct_failed(to);
                self.queue.take_current();
                self.forwarding.feed_due = true;
            }
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

    /// Whether the current forward, refused by `by`, would go elsewhere
    /// if routed now.
    fn goes_elsewhere(&self, by: NodeId, now: Instant) -> bool {
        let Some(o) = self.queue.current() else {
            return false;
        };
        self.goes_to(&o.message, o.passed, o.avoid, now)
            .is_some_and(|to| to != by)
    }

    /// The current forward goes round `to` while `to` is banned.
    fn go_round(&mut self, to: NodeId) {
        let Some(slot) = self.neighbours.slot_of(to) else {
            return;
        };
        if let Some(o) = self.queue.current_mut() {
            o.go_round(slot);
        }
    }

    /// Where the forward of `m` would go if routed now: its next hop, or
    /// its parent once none is left (spec "Next hop", "Return to parent").
    fn goes_to(&self, m: &Message, passed: u64, avoid: u64, now: Instant) -> Option<NodeId> {
        let record = self.records.get(m.id())?;
        self.next_hop(record, passed, avoid, (m.dst_id, m.dst_pos), now)
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
        let due = self.radio.reply_due(now);
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
            self.radio.abandon_reply();
            self.note_untaken(emit);
            return true;
        }
        self.radio.send(Job::Reply, frame, now, emit);
        true
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
                    self.handover_failed(out, now, emit);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_hand_over() {
        use Failure::*;
        use Kind::{Deliver as D, Direct as X, Forward as F, Return as R};
        // (rule, kind, a sleepy node's own message, receiver demoted or gone, outcome)
        let rows = [
            (
                "spec \"Hand-over\": a forward keeps its place",
                F,
                false,
                false,
                Forward,
            ),
            (
                "spec \"Hand-over\": it goes elsewhere once demoted",
                F,
                false,
                true,
                Forward,
            ),
            (
                "spec \"A sleepy node's own message\"",
                F,
                true,
                false,
                OwnAtAnchor,
            ),
            (
                "spec \"A sleepy node's own message\", demoted",
                F,
                true,
                true,
                OwnAtAnchor,
            ),
            (
                "spec \"Return to parent\": a parent is waited for",
                R,
                false,
                false,
                Return,
            ),
            (
                "spec \"Return to parent\": demoted or gone, dropped",
                R,
                false,
                true,
                ReturnWithoutParent,
            ),
            (
                "spec \"Delivery queue\": kept whatever its hand-over does",
                D,
                false,
                false,
                Deliver,
            ),
            (
                "spec \"Delivery queue\": kept, demoted or not",
                D,
                false,
                true,
                Deliver,
            ),
            (
                "spec \"Delivery\": a node heard as a relay",
                X,
                false,
                false,
                Direct,
            ),
            ("spec \"Delivery\": the flush", X, false, true, Direct),
        ];
        for (rule, kind, own, gone, then) in rows {
            assert_eq!(failure(kind, own, gone), then, "{rule}");
        }
    }

    #[test]
    fn a_refused_hand_over() {
        use Kind::{Deliver as D, Direct as X, Forward as F, Return as R};
        use Refusal::*;
        // (rule, kind, the refuser leaving where it may end, a sleepy node's own message, outcome)
        let rows = [
            (
                "spec \"Bans\": a refused forward goes round",
                F,
                false,
                false,
                GoesRound,
            ),
            (
                "spec \"Bans\": a refused return goes on as a forward",
                R,
                false,
                false,
                GoesRound,
            ),
            (
                "spec \"Next hop\": waits for a leaving neighbour",
                F,
                true,
                false,
                WaitsForRefuser,
            ),
            (
                "spec \"Next hop\": a return too",
                R,
                true,
                false,
                WaitsForRefuser,
            ),
            (
                "spec \"A sleepy node's own message\"",
                F,
                false,
                true,
                GivenBack,
            ),
            (
                "spec \"Next hop\": waiting goes before giving back",
                F,
                true,
                true,
                WaitsForRefuser,
            ),
            (
                "spec \"Bans\": a refused delivery waits the told wait",
                D,
                false,
                false,
                Waits,
            ),
            ("spec \"Bans\": whatever its refuser", D, true, false, Waits),
            ("spec \"Bans\": a direct delivery too", X, true, true, Waits),
        ];
        for (rule, kind, leaving, sleepy_own, then) in rows {
            assert_eq!(refusal(kind, leaving, sleepy_own), then, "{rule}");
        }
    }

    #[test]
    fn where_routing_sends_a_forward() {
        let t = |s| Instant::ZERO + Duration::from_secs(s);
        let now = t(10);
        let (free, banned) = (t(5), t(20));
        type W = Way<&'static str, &'static str>;
        // (rule, ends here, room here, next hop, origin waits, parent, outcome)
        let rows: [(&str, bool, bool, _, _, _, W); 10] = [
            (
                "spec \"Owner\"",
                true,
                true,
                Some(("n", free)),
                None,
                Some(("p", free)),
                Way::EndsHere,
            ),
            (
                "spec \"Receiving DATA\": kept aside",
                true,
                false,
                None,
                None,
                None,
                Way::SetAside,
            ),
            (
                "spec \"Next hop\"",
                false,
                true,
                Some(("n", free)),
                None,
                Some(("p", free)),
                Way::Next("n"),
            ),
            (
                "spec \"Next hop\": a ban is a wait",
                false,
                true,
                Some(("n", banned)),
                None,
                None,
                Way::Banned(banned),
            ),
            (
                "spec \"Next hop\": a ban that ended",
                false,
                true,
                Some(("n", now)),
                None,
                None,
                Way::Next("n"),
            ),
            (
                "spec \"Return to parent\"",
                false,
                true,
                None,
                None,
                Some(("p", free)),
                Way::Parent("p"),
            ),
            (
                "spec \"Return to parent\": waits while banned",
                false,
                true,
                None,
                None,
                Some(("p", banned)),
                Way::Banned(banned),
            ),
            (
                "spec \"Return to parent\": the origin drops",
                false,
                true,
                None,
                None,
                None,
                Way::Exhausted,
            ),
            (
                "spec \"Return to parent\": the first rotation",
                false,
                true,
                None,
                Some(banned),
                None,
                Way::WaitsUntil(banned),
            ),
            (
                "spec \"A sleepy node's own message\": alone",
                false,
                true,
                None,
                Some(t(70)),
                None,
                Way::WaitsUntil(t(70)),
            ),
        ];
        for (rule, ends, room, next, waits, parent, then) in rows {
            assert_eq!(way(ends, room, next, waits, parent, now), then, "{rule}");
        }
    }
}
