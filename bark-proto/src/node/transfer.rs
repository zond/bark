//! Per-hop transfer (spec "Hand-over"): one frame handed to one receiver
//! until it answers or the sender gives up ([`HandOver`]), and the replies
//! this node owes the senders of what it received ([`Replies`]): an ACK
//! each, taking or refusing.
//!
//! A hand-over lives in its send-queue place ([`Outgoing`]): where it is,
//! its transmissions so far and, in its frame, its receiver, which the ACK
//! it expects names. What the radio side keeps of the one in flight is
//! [`Flight`]. The node owns the radio (`mac.rs`), the frame and the
//! duty-cycle budget, starts each excursion and reports its completion
//! here.
//!
//! # A hand-over
//!
//! 1. Listen before talking on the receiver's home channel: one check, a
//!    CAD with the RSSI read while it samples ([`Action::Lbt`]). If it
//!    finds the channel busy, wait a random 0–`hop_backoff_max`
//!    (spec "Hand-over": 50 ms) and check again; once the checks of one attempt have
//!    found it busy for [`LBT_GIVE_UP`] (500 ms), give up ([`Check::GaveUp`]).
//! 2. Transmit, then listen home for the ACK for `ack_timeout` (100 ms).
//! 3. No ACK: wait a random time in `[0, retry_window]` and go back to 1,
//!    up to `hop_attempts` (3) transmissions; then the hand-over has
//!    failed. The window is twice the time one hand-over holds its
//!    receiver and doubles per attempt ([`retry_window`]), so that two
//!    senders that cannot hear each other and have collided at the
//!    receiver come apart (spec "Retry wait", roadmap D22).
//!
//! An ACK answers the hand-over only if it names the message, the
//! receiver as `from` and this node as `to` (roadmap D19, D32): another
//! sender may be offering the same message to the same receiver. One
//! that arrives after the timeout but before the next attempt still
//! counts: the receiver has the frame. Whether it takes or refuses the
//! frame, the role decides what follows (D27).

use crate::ids::NodeId;
use crate::node::Rng;
use crate::radio::Channel;
use crate::tables::fixed::FixedVec;
use crate::tables::send_queue::{Outgoing, State, Wait};
use crate::tables::{retry_window, LBT_GIVE_UP};
use crate::time::{Duration, Instant};
use crate::wire::{Ack, Packet};

/// How many hand-overs the node has in flight at most: one, so that the
/// radio listens at home for the ACK of the frame it sent, and the
/// scheduler starts no other meanwhile (spec "Hand-over"). Raising it is
/// a decision of its own.
pub(crate) const IN_FLIGHT: usize = 1;

/// What the radio side keeps of the hand-over in flight: until when it
/// waits, and since when the checks of its attempt have found the channel
/// busy.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Flight {
    until: Instant,
    /// `None` until a check of this attempt.
    lbt_since: Option<Instant>,
}

impl Flight {
    pub(crate) const NONE: Flight = Flight {
        until: Instant::ZERO,
        lbt_since: None,
    };

    /// Until when the hand-over in flight waits, or its ACK is due.
    pub(crate) fn until(&self) -> Instant {
        self.until
    }
}

/// What a listen-before-talk check led to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Check {
    /// The channel is clear: transmit.
    Clear,
    /// Busy: the hand-over waits a random backoff.
    Busy,
    /// Busy for the whole give-up time; the place is queued again.
    GaveUp,
}

/// What a missed ACK led to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Missed {
    /// Waiting to try again.
    Retry,
    /// Every attempt went unanswered; the place is queued again.
    Failed,
}

/// The hand-over of one place, advanced through the place's state and
/// the flight (spec "Hand-over").
pub(crate) struct HandOver<'a> {
    out: &'a mut Outgoing,
    flight: &'a mut Flight,
    /// This node, which the ACK names as `to`.
    me: NodeId,
}

/// The ACK by which `from` takes `me`'s offer of `msg_id`.
pub(crate) fn expect(msg_id: crate::ids::MsgId, from: NodeId, me: NodeId) -> Ack {
    Ack {
        msg_id,
        from,
        to: me,
        refused: false,
        wait: 0,
        seen: false,
    }
}

impl Outgoing {
    /// The ACK that takes this place's frame from `me`: the message and
    /// the receiver are in the frame.
    pub(crate) fn expect(&self, me: NodeId) -> Option<Ack> {
        match Packet::decode(self.frame.payload()) {
            Ok(Packet::Data(d)) => Some(expect(d.msg_id(), d.dst_hop, me)),
            _ => None,
        }
    }

    /// Whether the frame has been on air at least once in this hand-over.
    pub(crate) fn transmitted(&self) -> bool {
        self.tries > 0
    }

    /// Whether `ack` answers this place's hand-over, taking or refusing a
    /// frame that has been sent.
    pub(crate) fn answered_by(&self, ack: Ack, me: NodeId) -> bool {
        self.in_flight() && self.transmitted() && self.expect(me).is_some_and(|e| ack.answers(&e))
    }
}

impl<'a> HandOver<'a> {
    pub(crate) fn of(out: &'a mut Outgoing, flight: &'a mut Flight, me: NodeId) -> Self {
        HandOver { out, flight, me }
    }

    pub(crate) fn state(&self) -> State {
        self.out.state
    }

    pub(crate) fn awaiting_ack(&self) -> bool {
        self.out.state == State::AwaitAck
    }

    /// Begin the hand-over of this place, ready at once, after the
    /// unanswered transmissions it kept while it waited.
    pub(crate) fn start(&mut self, now: Instant) {
        *self.flight = Flight {
            until: now,
            lbt_since: None,
        };
        self.out.state = State::Waiting(Wait::Turn);
    }

    /// Give the hand-over up without a result: the place is queued again.
    pub(crate) fn cancel(&mut self) {
        self.out.state = State::Queued;
    }

    /// Whether listen-before-talk is due to start.
    pub(crate) fn due(&self, now: Instant) -> bool {
        matches!(self.out.state, State::Waiting(_)) && self.flight.until <= now
    }

    /// Hold the hand-over until `until`, without starting a new
    /// listen-before-talk period: to let an ACK this node owes go first.
    pub(crate) fn hold(&mut self, until: Instant) {
        self.wait(Wait::Reply, until);
    }

    /// Hold the hand-over until the duty-cycle budget allows it. The
    /// listen-before-talk give-up counts time the channel was found busy;
    /// a wait of minutes for the budget is not that, so the checks after
    /// it start a new period.
    pub(crate) fn defer(&mut self, until: Instant) {
        self.flight.lbt_since = None;
        self.wait(Wait::Budget, until);
    }

    fn wait(&mut self, why: Wait, until: Instant) {
        self.flight.until = until;
        self.out.state = State::Waiting(why);
    }

    /// Listen-before-talk starts: the caller issues the check.
    pub(crate) fn begin_lbt(&mut self, now: Instant) {
        self.flight.lbt_since.get_or_insert(now);
        self.out.state = State::Radio;
    }

    /// The listen-before-talk check completed.
    pub(crate) fn checked(
        &mut self,
        busy: bool,
        now: Instant,
        rng: &mut Rng,
        backoff_max: Duration,
    ) -> Check {
        if !busy {
            return Check::Clear;
        }
        let since = self.flight.lbt_since.unwrap_or(now);
        if now.since(since) >= LBT_GIVE_UP {
            self.flight.lbt_since = None;
            self.out.state = State::Queued;
            return Check::GaveUp;
        }
        self.wait(
            Wait::Backoff,
            now + rng.duration(Duration::ZERO, backoff_max),
        );
        Check::Busy
    }

    /// The frame left the antenna: wait `ack_timeout` for the ACK. An
    /// attempt is a transmission, counted here and nowhere else.
    pub(crate) fn sent(&mut self, now: Instant, ack_timeout: Duration) {
        self.out.tries = self.out.tries.saturating_add(1);
        self.flight.lbt_since = None;
        self.flight.until = now + ack_timeout;
        self.out.state = State::AwaitAck;
    }

    /// Whether `ack` answers this hand-over, taking or refusing a frame
    /// that has been sent.
    pub(crate) fn answered_by(&self, ack: Ack) -> bool {
        self.out.answered_by(ack, self.me)
    }

    /// The receiver refused the hand-over, which gives it up: the
    /// unanswered transmissions before the refused one, to resume with.
    pub(crate) fn refused(&mut self) -> u8 {
        self.out.state = State::Queued;
        self.out.tries.saturating_sub(1)
    }

    /// An ACK arrived. Returns whether it takes this hand-over's frame,
    /// which ends the hand-over.
    pub(crate) fn acked(&mut self, ack: Ack) -> bool {
        let taken = self.answered_by(ack) && !ack.refused;
        if taken {
            self.out.state = State::Queued;
        }
        taken
    }

    /// Count an attempt that could not be made, as if it had gone
    /// unanswered: for a hand-over that has nowhere else to go when
    /// listen-before-talk gives up.
    pub(crate) fn lost_attempt(
        &mut self,
        now: Instant,
        max: u8,
        rng: &mut Rng,
        retry_backoff: Duration,
    ) -> Missed {
        self.out.tries = self.out.tries.saturating_add(1);
        self.after_attempt(now, max, rng, retry_backoff)
    }

    /// If the ACK wait has run out at `now`, retry or fail.
    pub(crate) fn timeout(
        &mut self,
        now: Instant,
        max: u8,
        rng: &mut Rng,
        retry_backoff: Duration,
    ) -> Option<Missed> {
        if self.awaiting_ack() && now >= self.flight.until {
            Some(self.after_attempt(now, max, rng, retry_backoff))
        } else {
            None
        }
    }

    fn after_attempt(
        &mut self,
        now: Instant,
        max: u8,
        rng: &mut Rng,
        retry_backoff: Duration,
    ) -> Missed {
        if self.out.tries >= max {
            self.out.state = State::Queued;
            Missed::Failed
        } else {
            self.flight.lbt_since = None;
            let window = retry_window(retry_backoff, self.out.tries);
            self.wait(Wait::Retry, now + rng.duration(Duration::ZERO, window));
            Missed::Retry
        }
    }
}

/// What a node replies with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reply {
    /// An ACK, taking or refusing (spec "The ACK").
    Ack(Ack),
}

impl From<Ack> for Reply {
    fn from(a: Ack) -> Reply {
        Reply::Ack(a)
    }
}

/// A reply this node owes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Owed {
    pub(crate) reply: Reply,
    /// The channel the sender listens on.
    pub(crate) channel: Channel,
    /// When the sender stops waiting; an ACK after that is useless.
    deadline: Instant,
    /// Not before this: a listen-before-talk backoff.
    ready: Instant,
}

/// How many replies a node can owe at once (roadmap D29). One, because
/// a node that owes a reply has the radio's next transmission promised:
/// a second packet it took or refused meanwhile would be answered only
/// after the first reply, and its sender may have stopped waiting by
/// then. A relay takes or refuses a packet only when [`Replies::can_owe`]
/// its reply; otherwise the node stays silent and the sender's retry
/// brings the packet again.
pub(crate) const REPLIES: usize = 1;

/// The replies this node owes, in the order it took them on, sent with
/// listen-before-talk, as the receiver of spec "The ACK" does, with the
/// hand-over's backoff. One is abandoned once its sender has stopped
/// waiting: a rule of this queue, not of its length.
///
/// `sending` says whether the first owed reply is on the radio now, which
/// the node knows from its excursion: a packet can arrive meanwhile, as
/// the driver may hold a check while a reception ends (core-interface §3
/// rule 8).
#[derive(Clone)]
pub(crate) struct Replies {
    owed: FixedVec<Owed, REPLIES>,
    /// The reply being sent takes a DATA for this node: once it has gone,
    /// the node listens one hand-over for the next (spec "Delivery").
    renew: bool,
}

impl Replies {
    pub(crate) const fn new() -> Self {
        let none = Owed {
            reply: Reply::Ack(Ack {
                msg_id: crate::ids::MsgId(0),
                from: NodeId::NONE,
                to: NodeId::NONE,
                refused: false,
                wait: 0,
                seen: false,
            }),
            channel: match Channel::new(0) {
                Some(c) => c,
                None => unreachable!(),
            },
            deadline: Instant::ZERO,
            ready: Instant::ZERO,
        };
        Replies {
            owed: FixedVec::new(none),
            renew: false,
        }
    }

    /// Whether `reply` on `channel` can be owed now: the queue has a
    /// place, or holds that reply already, for an earlier copy of the
    /// packet.
    pub(crate) fn can_owe(
        &mut self,
        reply: impl Into<Reply>,
        channel: Channel,
        sending: bool,
        now: Instant,
    ) -> bool {
        let reply = reply.into();
        self.forget_late(sending, now);
        !self.owed.is_full() || self.owes(reply, channel)
    }

    fn owes(&self, reply: Reply, channel: Channel) -> bool {
        self.owed
            .as_slice()
            .iter()
            .any(|o| o.reply == reply && o.channel == channel)
    }

    /// Owe `reply` on `channel` until `deadline`, `renews` saying whether
    /// it takes a DATA for this node. A reply owed already answers this
    /// copy too. With no place left, the oldest one that has not started
    /// out gives way, as the one whose packet is older; if that is the one
    /// being sent, `reply` is not owed.
    pub(crate) fn owe(
        &mut self,
        reply: impl Into<Reply>,
        channel: Channel,
        deadline: Instant,
        renews: bool,
        sending: bool,
        now: Instant,
    ) {
        let reply = reply.into();
        self.renew = renews;
        self.forget_late(sending, now);
        if self.owes(reply, channel) {
            return;
        }
        if self.owed.is_full() {
            if sending {
                return;
            }
            self.owed.remove(0);
        }
        let _ = self.owed.push(Owed {
            reply,
            channel,
            deadline,
            ready: now,
        });
    }

    /// Drop the owed replies whose senders have stopped waiting, but the
    /// one being sent.
    fn forget_late(&mut self, sending: bool, now: Instant) {
        let mut i = 0;
        self.owed.retain(|o| {
            let keep = (sending && i == 0) || o.deadline > now;
            i += 1;
            keep
        });
    }

    /// The reply to start sending now, if the first owed is ready; asked
    /// only while the radio is free.
    pub(crate) fn due(&mut self, now: Instant) -> Option<Owed> {
        self.forget_late(false, now);
        self.owed
            .as_slice()
            .first()
            .filter(|o| o.ready <= now)
            .copied()
    }

    /// When the first owed reply is ready to send, once the radio is free.
    pub(crate) fn wake_at(&self) -> Option<Instant> {
        self.owed.as_slice().first().map(|o| o.ready)
    }

    /// Whether a reply is owed.
    pub(crate) fn pending(&self) -> bool {
        self.owed.len() > 0
    }

    /// Give up the first owed reply: no budget for it.
    pub(crate) fn abandon(&mut self) {
        self.renew = false;
        if self.pending() {
            self.owed.remove(0);
        }
    }

    /// The check of the reply being sent completed. Returns the reply to
    /// transmit, or `None` if it now waits or was abandoned.
    pub(crate) fn checked(
        &mut self,
        busy: bool,
        now: Instant,
        rng: &mut Rng,
        backoff_max: Duration,
    ) -> Option<Owed> {
        let o = self.owed.as_mut_slice().first_mut()?;
        if busy {
            let ready = now + rng.duration(Duration::ZERO, backoff_max);
            if ready >= o.deadline {
                self.owed.remove(0);
            } else {
                o.ready = ready;
            }
            return None;
        }
        Some(*o)
    }

    /// The reply being sent left the antenna. Returns whether the node
    /// listens on for the next DATA.
    pub(crate) fn sent(&mut self) -> bool {
        if self.pending() {
            self.owed.remove(0);
        }
        core::mem::take(&mut self.renew)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::MsgId;

    fn at(ms: u64) -> Instant {
        Instant::from_micros(ms * 1000)
    }

    fn ms(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    fn ack(id: u32) -> Ack {
        Ack {
            msg_id: MsgId(id),
            from: NodeId([1, 2, 3, 4, 5, 6]),
            to: NodeId([7, 8, 9, 10, 11, 12]),
            refused: false,
            wait: 0,
            seen: false,
        }
    }

    /// A place whose frame is a DATA of message `id` to the receiver
    /// `ack(id).from`, and the flight: a hand-over to drive.
    fn place(id: u32) -> (Outgoing, Flight) {
        use crate::event::{Frame, MAX_FRAME};
        use crate::position::Position;
        use crate::radio::MIN_PREAMBLE_SYMBOLS;
        use crate::tables::send_queue::Kind;
        use crate::wire::Data;
        let d = Data {
            sender_home: Channel::new(3).unwrap(),
            dst_hop: ack(id).from,
            src_hop: ack(id).to,
            dst_pos: Position { lat: 0, lon: 0 },
            dst_id: NodeId([5; 6]),
            hops: 1,
            time_left: 1,
            payload: &[id as u8],
        };
        let mut bytes = [0; MAX_FRAME];
        let len = Packet::Data(d).encode(&mut bytes).unwrap() as u8;
        let frame = Frame {
            channel: d.sender_home,
            preamble_symbols: MIN_PREAMBLE_SYMBOLS,
            len,
            bytes,
        };
        (Outgoing::new(frame, Kind::Forward), Flight::NONE)
    }

    /// The expected ACK names the message and the receiver of the frame
    /// (roadmap D15).
    fn names(id: u32) -> Ack {
        let (out, _) = place(id);
        let e = out.expect(ack(id).to).unwrap();
        assert_eq!(e.msg_id, out.msg_id().unwrap());
        assert_eq!((e.from, e.to), (ack(id).from, ack(id).to));
        e
    }

    #[test]
    fn three_attempts_then_failure() {
        let mut rng = Rng::new(1);
        let (mut out, mut flight) = place(7);
        let expect = names(7);
        let mut t = HandOver::of(&mut out, &mut flight, expect.to);
        t.start(at(0));
        assert!(t.due(at(0)) && !t.out.transmitted());
        for attempt in 0..3u64 {
            let now = t.flight.until;
            assert!(t.due(now));
            t.begin_lbt(now);
            assert_eq!(t.state(), State::Radio);
            assert_eq!(t.checked(false, now, &mut rng, ms(50)), Check::Clear);
            t.sent(now, ms(100));
            assert!(t.awaiting_ack());
            assert_eq!(
                t.out.tries,
                attempt as u8 + 1,
                "an attempt is a transmission"
            );
            assert_eq!(t.timeout(now + ms(99), 3, &mut rng, ms(50)), None);
            let missed = t.timeout(now + ms(100), 3, &mut rng, ms(50)).unwrap();
            if attempt < 2 {
                assert_eq!(missed, Missed::Retry);
                assert_eq!(t.state(), State::Waiting(Wait::Retry));
                let w = t.flight.until;
                // Two units after the first transmission, four after the second.
                let window = ms(50 << (attempt + 1));
                assert!(
                    w >= now + ms(100) && w <= now + ms(100) + window,
                    "backoff 0–{} ms",
                    window.as_millis()
                );
            } else {
                assert_eq!(missed, Missed::Failed);
                assert!(!t.out.in_flight());
            }
        }
    }

    #[test]
    fn ack_must_name_message_and_both_parties() {
        let mut rng = Rng::new(2);
        let (mut out, mut flight) = place(7);
        let expect = names(7);
        let mut t = HandOver::of(&mut out, &mut flight, expect.to);
        t.start(at(0));
        assert!(!t.acked(expect), "nothing sent yet");
        t.begin_lbt(at(0));
        t.checked(false, at(0), &mut rng, ms(50));
        t.checked(false, at(1), &mut rng, ms(50));
        t.sent(at(2), ms(100));
        assert!(!t.acked(ack(8)));
        assert!(
            !t.acked(Ack {
                from: NodeId([9; 6]),
                ..expect
            }),
            "same message, other receiver (D19)"
        );
        let to_another = Ack {
            to: NodeId([9; 6]),
            ..expect
        };
        assert!(
            !t.answered_by(to_another) && !t.acked(to_another),
            "same message and receiver, answering another sender (D32)"
        );
        let refused = Ack {
            refused: true,
            ..expect
        };
        assert!(t.answered_by(refused) && !t.acked(refused), "a refusal");
        assert!(t.out.in_flight(), "a refusal is the role's to act on");
        // Late, during the retry backoff: still the receiver's ACK.
        assert_eq!(t.timeout(at(102), 3, &mut rng, ms(50)), Some(Missed::Retry));
        assert!(t.acked(expect));
        assert!(!t.out.in_flight());
    }

    #[test]
    fn busy_channel_backs_off_and_gives_up_after_500_ms() {
        let mut rng = Rng::new(3);
        let (mut out, mut flight) = place(1);
        let me = ack(1).to;
        let mut t = HandOver::of(&mut out, &mut flight, me);
        t.start(at(0));
        let mut now = at(0);
        let mut checks = 0;
        loop {
            t.begin_lbt(now);
            checks += 1;
            match t.checked(true, now, &mut rng, ms(50)) {
                Check::Busy => {
                    assert_eq!(t.state(), State::Waiting(Wait::Backoff));
                    now = t.flight.until;
                }
                Check::GaveUp => break,
                Check::Clear => unreachable!(),
            }
        }
        assert!(now >= at(500) && now < at(550), "{now:?}");
        assert!(checks > 10 && !t.out.in_flight());
        assert!(!t.out.transmitted(), "a busy channel is no attempt");
        // A retry starts a fresh 500 ms.
        t.start(now);
        t.begin_lbt(now);
        assert_eq!(
            t.checked(true, now + ms(499), &mut rng, ms(50)),
            Check::Busy
        );
    }

    #[test]
    fn a_budget_hold_starts_a_new_listen_before_talk_period() {
        let mut rng = Rng::new(5);
        let (mut out, mut flight) = place(1);
        let me = ack(1).to;
        let mut t = HandOver::of(&mut out, &mut flight, me);
        t.start(at(0));
        t.begin_lbt(at(0));
        assert_eq!(t.checked(true, at(0), &mut rng, ms(50)), Check::Busy);
        t.defer(at(60_000));
        assert_eq!(t.state(), State::Waiting(Wait::Budget));
        t.begin_lbt(at(60_000));
        assert_eq!(
            t.checked(true, at(60_000), &mut rng, ms(50)),
            Check::Busy,
            "a minute of waiting for the budget is not a busy channel"
        );
    }

    #[test]
    fn replies_wait_out_backoff_and_lapse_at_the_deadline() {
        let mut rng = Rng::new(4);
        let ch = Channel::new(3).unwrap();
        let mut a = Replies::new();
        a.owe(ack(1), ch, at(100), false, false, at(0));
        assert!(
            a.can_owe(ack(1), ch, false, at(10)),
            "a copy is answered too"
        );
        a.owe(ack(1), ch, at(110), false, false, at(10));
        assert!(!a.can_owe(ack(2), ch, false, at(10)), "one place");
        assert!(!a.can_owe(ack(1), Channel::new(4).unwrap(), false, at(10)));
        assert_eq!(a.due(at(10)).unwrap().reply, Reply::Ack(ack(1)));
        // Its check is on the radio: a packet that arrives meanwhile is
        // answered after it, or not at all.
        a.owe(ack(2), ch, at(111), false, true, at(11));
        let sent = a.checked(false, at(11), &mut rng, ms(50)).unwrap();
        assert_eq!(
            sent.reply,
            Reply::Ack(ack(1)),
            "the reply being sent keeps its place"
        );
        assert!(!a.sent());
        assert!(!a.pending());
        assert!(a.can_owe(ack(2), ch, false, at(13)));
        // Busy until past the deadline: abandoned.
        a.owe(ack(2), ch, at(100), false, false, at(60));
        a.due(at(60)).unwrap();
        let mut now = at(60);
        while a.pending() {
            assert_eq!(a.checked(true, now, &mut rng, ms(50)), None);
            if let Some(w) = a.wake_at() {
                assert!(w < at(100));
                now = w;
                a.due(now).unwrap();
            }
        }
        // Not started: a newer one takes the place.
        a.owe(ack(3), ch, at(300), false, false, at(200));
        a.owe(ack(4), ch, at(301), true, false, at(200));
        assert_eq!(a.due(at(201)).unwrap().reply, Reply::Ack(ack(4)));
        // A lapsed one frees the place.
        assert!(!a.can_owe(ack(5), ch, false, at(300)));
        assert!(a.can_owe(ack(5), ch, false, at(301)));
        assert_eq!(a.due(at(301)), None);
        // One refusal answers a retransmission too, but not another message.
        let busy = |id| Ack {
            refused: true,
            ..ack(id)
        };
        a.owe(busy(1), ch, at(500), false, false, at(400));
        assert!(a.can_owe(busy(1), ch, false, at(410)));
        assert!(!a.can_owe(busy(2), ch, false, at(410)));
        assert!(!a.can_owe(ack(1), ch, false, at(410)));
        // Sent, a reply that took a DATA for this node has it listen on.
        a.due(at(410)).unwrap();
        assert!(!a.sent(), "a refusal took nothing");
        a.owe(ack(6), ch, at(600), true, false, at(510));
        a.due(at(510)).unwrap();
        assert!(a.sent());
    }
}
