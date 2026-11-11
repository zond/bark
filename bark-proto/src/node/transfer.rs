//! Per-hop transfer (spec "Hand-over", "Retry wait", "The ACK"): one frame
//! handed to one receiver until it answers or the sender gives up
//! ([`HandOver`]), and the replies this node owes the senders of what it
//! received ([`Replies`]).
//!
//! A hand-over lives in its send-queue place ([`Outgoing`]); what the radio
//! side keeps of the one in flight is [`Flight`]. The node owns the radio
//! (`mac.rs`) and the duty-cycle budget, starts each excursion and reports
//! its completion here. The role decides what a listen-before-talk give-up,
//! a refusal or a failure leads to.

use crate::ids::{MsgId, NodeId};
use crate::node::Rng;
use crate::radio::Channel;
use crate::tables::fixed::FixedVec;
use crate::tables::send_queue::{Message, Outgoing, State, Wait};
use crate::tables::{retry_window, LBT_GIVE_UP};
use crate::time::{Duration, Instant};
use crate::wire::{Ack, Data};

/// How many hand-overs the node has in flight at most: one, so the radio
/// listens at home for the ACK of the frame it sent (spec "Hand-over").
pub(crate) const IN_FLIGHT: usize = 1;

/// What the radio side keeps of the hand-over in flight.
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
    /// The ACK by which this place's receiver takes its message from `me`.
    pub(crate) fn expect(&self, me: NodeId) -> Ack {
        expect(self.msg_id(), self.to, me)
    }

    /// Whether the frame has been on air at least once in this hand-over.
    pub(crate) fn transmitted(&self) -> bool {
        self.tries > 0
    }

    /// Whether `ack` answers this place's hand-over, taking, refusing or
    /// having seen a frame that has been sent.
    pub(crate) fn answered_by(&self, ack: Ack, me: NodeId) -> bool {
        self.in_flight() && self.transmitted() && ack.answers(&self.expect(me))
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

    /// Begin the hand-over of this place, ready at once, keeping the
    /// unanswered transmissions it had.
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

    /// Hold the hand-over until the duty-cycle budget allows it. A wait for
    /// the budget is not a busy channel, so the checks after it start a new
    /// give-up period.
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

    /// Whether `ack` answers this hand-over, taking, refusing or having
    /// seen a frame that has been sent.
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
        let taken = self.answered_by(ack) && !ack.refused && !ack.seen;
        if taken {
            self.out.state = State::Queued;
        }
        taken
    }

    /// Listen-before-talk gave up on the receiver's channel: wait the retry
    /// window of the transmissions so far and listen again; returns when.
    /// A channel busy longer than any frame is held by one hand-over after
    /// another, so the wait is in their unit and grows like the retry wait,
    /// so that senders waiting for the channel come apart
    /// (spec "Retry wait").
    pub(crate) fn channel_busy(
        &mut self,
        now: Instant,
        rng: &mut Rng,
        retry_backoff: Duration,
    ) -> Instant {
        self.flight.lbt_since = None;
        let window = retry_window(retry_backoff, self.out.tries);
        let until = now + rng.duration(Duration::ZERO, window);
        self.wait(Wait::Channel, until);
        until
    }

    /// Count an attempt that could not be made, as if it had gone
    /// unanswered: for a delivery when listen-before-talk gives up.
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
    /// An ACK, taking, refusing or seen (spec "The ACK").
    Ack(Ack),
}

impl From<Ack> for Reply {
    fn from(a: Ack) -> Reply {
        Reply::Ack(a)
    }
}

/// A DATA a reply takes, acted on once the reply has gone out, where the
/// take is final (spec "The ACK").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Take {
    /// Its message.
    pub(crate) message: Message,
    /// Its sender.
    pub(crate) from: NodeId,
    /// Its sender's home channel.
    pub(crate) home: Channel,
    /// Its count.
    pub(crate) hops: u8,
}

impl Take {
    /// What `d` gives to take.
    pub(crate) fn of(d: &Data<'_>) -> Take {
        Take {
            message: Message::of(d),
            from: d.src_hop,
            home: d.sender_home,
            hops: d.hops,
        }
    }

    /// The DATA as it came to `me`.
    pub(crate) fn data(&self, me: NodeId) -> Data<'_> {
        Data {
            sender_home: self.home,
            dst_hop: me,
            src_hop: self.from,
            dst_pos: self.message.dst_pos,
            dst_id: self.message.dst_id,
            hops: self.hops,
            payload: self.message.payload(),
        }
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
    /// When it was owed; a reply is ready at once (spec "The ACK").
    ready: Instant,
    /// The DATA the reply takes.
    takes: Option<Take>,
}

impl Owed {
    /// The message a reply that was not sent leaves untaken, if it took one.
    fn untaken(&self) -> Option<MsgId> {
        match (self.takes, self.reply) {
            (Some(_), Reply::Ack(a)) => Some(a.msg_id),
            _ => None,
        }
    }
}

/// How many replies a node can owe at once. One: a second packet would be
/// answered only after the first reply, when its sender may have stopped
/// waiting. A relay takes or refuses a packet only when [`Replies::can_owe`]
/// its reply; otherwise it stays silent and the sender's retry brings it
/// again.
pub(crate) const REPLIES: usize = 1;

/// The replies this node owes, in order (spec "The ACK"). One is abandoned
/// once its sender has stopped waiting; one that took a DATA then leaves it
/// untaken ([`Replies::untaken`]).
///
/// `sending` says whether the first owed reply is on the radio now: a
/// packet can arrive meanwhile, as the driver may hold a check while a
/// reception ends (core-interface §3 rule 8).
#[derive(Clone)]
pub(crate) struct Replies {
    owed: FixedVec<Owed, REPLIES>,
    /// The reply being sent takes a DATA for this node, so the node listens
    /// on for the next (spec "Delivery").
    renew: bool,
    /// The message of a reply abandoned since the node last asked, whose
    /// DATA it left untaken.
    untaken: Option<MsgId>,
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
            takes: None,
        };
        Replies {
            owed: FixedVec::new(none),
            renew: false,
            untaken: None,
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

    /// Owe `reply` on `channel` until `deadline`; `renews` says whether it
    /// takes a DATA for this node, `takes` is the DATA to act on once it has
    /// gone out. A reply owed already answers this copy too. With no place
    /// left, the oldest not yet started gives way; if that one is being
    /// sent, `reply` is not owed.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn owe(
        &mut self,
        reply: impl Into<Reply>,
        channel: Channel,
        deadline: Instant,
        renews: bool,
        takes: Option<Take>,
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
            self.give_up(0);
        }
        let _ = self.owed.push(Owed {
            reply,
            channel,
            deadline,
            ready: now,
            takes,
        });
    }

    /// Drop the owed replies whose senders have stopped waiting, but the
    /// one being sent.
    fn forget_late(&mut self, sending: bool, now: Instant) {
        let mut i = 0;
        while i < self.owed.len() {
            let o = self.owed.as_slice()[i];
            if (sending && i == 0) || o.deadline > now {
                i += 1;
            } else {
                self.give_up(i);
            }
        }
    }

    /// Remove the `i`-th owed reply unsent, noting the DATA it leaves
    /// untaken.
    fn give_up(&mut self, i: usize) {
        if let Some(id) = self.owed.as_slice().get(i).and_then(Owed::untaken) {
            self.untaken = Some(id);
        }
        self.owed.remove(i);
    }

    /// The message of a reply abandoned since last asked, whose DATA the
    /// node did not take.
    pub(crate) fn untaken(&mut self) -> Option<MsgId> {
        self.untaken.take()
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

    /// Whether an owed reply takes a DATA, which the node acts on once it
    /// has gone out.
    pub(crate) fn taking(&self) -> bool {
        self.owed.as_slice().iter().any(|o| o.takes.is_some())
    }

    /// Give up the first owed reply: no budget for it.
    pub(crate) fn abandon(&mut self) {
        self.renew = false;
        if self.pending() {
            self.give_up(0);
        }
    }

    /// The reply being sent left the antenna. Returns whether the node
    /// listens on for the next DATA, and the DATA the reply took, to act
    /// on now.
    pub(crate) fn sent(&mut self) -> (bool, Option<Take>) {
        let takes = if self.pending() {
            self.owed.remove(0).takes
        } else {
            None
        };
        (core::mem::take(&mut self.renew), takes)
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

    /// A hand-over of message `id` to the receiver `ack(id).from`.
    fn place(id: u32) -> (Outgoing, Flight) {
        use crate::position::Position;
        use crate::radio::MIN_PREAMBLE_SYMBOLS;
        use crate::tables::send_queue::Kind;
        let message = Message::new(Position { lat: 0, lon: 0 }, NodeId([5; 6]), &[id as u8]);
        let out = Outgoing::new(
            message,
            1,
            ack(id).from,
            Channel::new(3).unwrap(),
            MIN_PREAMBLE_SYMBOLS,
            Kind::Forward,
        );
        (out, Flight::NONE)
    }

    /// The expected ACK names the message and the receiver of the frame.
    fn names(id: u32) -> Ack {
        let (out, _) = place(id);
        let e = out.expect(ack(id).to);
        assert_eq!(e.msg_id, out.msg_id());
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
        let seen = Ack {
            seen: true,
            ..expect
        };
        assert!(t.answered_by(seen) && !t.acked(seen), "seen, not taken");
        assert!(t.out.in_flight(), "seen is the role's to act on");
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
    fn a_busy_channel_keeps_the_receiver_and_waits_the_retry_window() {
        let mut rng = Rng::new(4);
        let (mut out, mut flight) = place(1);
        let me = ack(1).to;
        let expect = names(1);
        let mut t = HandOver::of(&mut out, &mut flight, me);
        t.start(at(0));
        let give_up = |t: &mut HandOver<'_>, rng: &mut Rng, from: Instant| {
            let mut now = from;
            loop {
                t.begin_lbt(now);
                match t.checked(true, now, rng, ms(50)) {
                    Check::Busy => now = t.flight.until,
                    Check::GaveUp => return now,
                    Check::Clear => unreachable!(),
                }
            }
        };
        // Before any transmission: one unit.
        let now = give_up(&mut t, &mut rng, at(0));
        let until = t.channel_busy(now, &mut rng, ms(100));
        assert_eq!(t.state(), State::Waiting(Wait::Channel));
        assert!(t.out.in_flight() && !t.out.transmitted());
        assert!(until >= now && until <= now + ms(100), "{until:?}");
        assert!(!t.due(now) && t.due(until));
        assert_eq!(t.flight.until, until);
        // A fresh 500 ms once it listens again.
        t.begin_lbt(until);
        assert_eq!(
            t.checked(true, until + ms(499), &mut rng, ms(50)),
            Check::Busy
        );
        t.checked(false, until + ms(500), &mut rng, ms(50));
        t.sent(until + ms(500), ms(100));
        assert_eq!(
            t.timeout(until + ms(600), 3, &mut rng, ms(50)),
            Some(Missed::Retry)
        );
        // After one transmission: two units, and the transmission kept.
        let from = t.flight.until;
        let now = give_up(&mut t, &mut rng, from);
        let until = t.channel_busy(now, &mut rng, ms(100));
        assert!(until >= now && until <= now + ms(200), "{until:?}");
        assert_eq!(t.out.tries, 1, "a give-up is no attempt");
        assert!(t.answered_by(expect), "the receiver may still answer");
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
    fn replies_lapse_at_the_deadline() {
        let ch = Channel::new(3).unwrap();
        let mut a = Replies::new();
        a.owe(ack(1), ch, at(100), false, None, false, at(0));
        assert!(
            a.can_owe(ack(1), ch, false, at(10)),
            "a copy is answered too"
        );
        a.owe(ack(1), ch, at(110), false, None, false, at(10));
        assert!(!a.can_owe(ack(2), ch, false, at(10)), "one place");
        assert!(!a.can_owe(ack(1), Channel::new(4).unwrap(), false, at(10)));
        assert_eq!(a.due(at(10)).unwrap().reply, Reply::Ack(ack(1)));
        // It is on the radio: a packet that arrives meanwhile is
        // answered after it, or not at all.
        a.owe(ack(2), ch, at(111), false, None, true, at(11));
        assert_eq!(
            a.due(at(11)).unwrap().reply,
            Reply::Ack(ack(1)),
            "the reply being sent keeps its place"
        );
        assert!(!a.sent().0);
        assert!(!a.pending());
        assert!(a.can_owe(ack(2), ch, false, at(13)));
        // Not started: a newer one takes the place.
        a.owe(ack(3), ch, at(300), false, None, false, at(200));
        a.owe(ack(4), ch, at(301), true, None, false, at(200));
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
        a.owe(busy(1), ch, at(500), false, None, false, at(400));
        assert!(a.can_owe(busy(1), ch, false, at(410)));
        assert!(!a.can_owe(busy(2), ch, false, at(410)));
        assert!(!a.can_owe(ack(1), ch, false, at(410)));
        // Sent, a reply that took a DATA for this node has it listen on.
        a.due(at(410)).unwrap();
        assert!(!a.sent().0, "a refusal took nothing");
        a.owe(ack(6), ch, at(600), true, None, false, at(510));
        a.due(at(510)).unwrap();
        assert!(a.sent().0);
    }

    /// A reply that takes a DATA hands it over once sent, and leaves it
    /// untaken, reported once, when it is given up.
    #[test]
    fn a_take_waits_for_its_reply() {
        let ch = Channel::new(3).unwrap();
        let (out, _) = place(9);
        let take = Take {
            message: out.message,
            from: ack(9).from,
            home: out.channel,
            hops: out.hops,
        };
        let mut a = Replies::new();
        a.owe(ack(9), ch, at(100), false, Some(take), false, at(0));
        assert_eq!(a.untaken(), None);
        a.due(at(0)).unwrap();
        assert_eq!(a.sent(), (false, Some(take)), "sent: act on it");
        assert_eq!(a.untaken(), None);
        // Not sent by the deadline: the DATA is not taken.
        a.owe(ack(9), ch, at(200), false, Some(take), false, at(100));
        assert_eq!(a.due(at(200)), None);
        assert_eq!(a.untaken(), Some(MsgId(9)));
        assert_eq!(a.untaken(), None, "reported once");
        // Lapsed unsent, and abandoned for budget, alike.
        a.owe(ack(9), ch, at(300), false, Some(take), false, at(250));
        assert!(!a.can_owe(ack(10), ch, false, at(299)));
        assert_eq!(a.untaken(), None);
        assert!(
            a.can_owe(ack(10), ch, false, at(300)),
            "lapsed: the place is free"
        );
        assert_eq!(a.untaken(), Some(MsgId(9)));
        a.owe(ack(9), ch, at(400), false, Some(take), false, at(350));
        a.abandon();
        assert_eq!(a.untaken(), Some(MsgId(9)));
        // A refusal takes nothing to leave.
        let busy = Ack {
            refused: true,
            ..ack(9)
        };
        a.owe(busy, ch, at(500), false, None, false, at(450));
        a.abandon();
        assert_eq!(a.untaken(), None);
    }
}
