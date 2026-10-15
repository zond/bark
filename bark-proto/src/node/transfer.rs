//! Per-hop transfer (design §9.3), shared by both roles: one frame handed
//! to one receiver until it answers or the sender gives up ([`Transfer`]),
//! and the one ACK, taking or refusing, this node can owe the sender of
//! what it received ([`Replies`]).
//!
//! Both only keep the state; the role owns the radio (`mac.rs`), the frame
//! and the duty-cycle budget, starts each excursion and reports its
//! completion here.
//!
//! # A hand-over
//!
//! 1. Listen before talking on the receiver's home channel: CAD, then RSSI.
//!    If either finds the channel busy, wait a random 0–`hop_backoff_max`
//!    (§9.3: 50 ms) and check again; once the checks of one attempt have
//!    found it busy for [`LBT_GIVE_UP`] (500 ms), give up ([`Check::GaveUp`]).
//! 2. Transmit, then listen home for the ACK for `ack_timeout` (100 ms).
//! 3. No ACK: wait a random time in `[0, retry_window]` and go back to 1,
//!    up to `hop_attempts` (3) transmissions; then the hand-over has
//!    failed. The window is twice the time one hand-over holds its
//!    receiver and doubles per attempt ([`retry_window`]), so that two
//!    senders that cannot hear each other and have collided at the
//!    receiver come apart (§9.3, roadmap D22).
//!
//! An ACK answers the hand-over only if it names the message, the
//! receiver as `from` and this node as `to` (roadmap D19, D32): another
//! sender may be offering the same message to the same receiver. One
//! that arrives after the timeout but before the next attempt still
//! counts: the receiver has the frame. Whether it takes or refuses the
//! frame, the role decides what follows (D27).

use crate::event::Action;
use crate::node::Rng;
use crate::radio::Channel;
use crate::tables::{retry_window, LBT_GIVE_UP};
use crate::time::{Duration, Instant};
use crate::wire::{Ack, AckName};

/// Where a hand-over is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    /// No hand-over.
    Idle,
    /// Start listen-before-talk at this instant: the first attempt, a
    /// busy backoff, a retry, or a wait for the duty-cycle budget.
    Wait(Instant),
    /// The CAD excursion is outstanding.
    Cad,
    /// The RSSI excursion is outstanding.
    Rssi,
    /// The frame is on air.
    Tx,
    /// Listening home for the ACK until this instant.
    AwaitAck(Instant),
}

/// What a listen-before-talk check led to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Check {
    /// The channel is clear: go on, to RSSI after CAD, to transmit after RSSI.
    Clear,
    /// Busy: the hand-over waits a random backoff.
    Busy,
    /// Busy for the whole give-up time; the hand-over is idle again.
    GaveUp,
}

/// What a missed ACK led to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Missed {
    /// Waiting to try again.
    Retry,
    /// Every attempt went unanswered; the hand-over is idle again.
    Failed,
}

/// One hand-over (design §9.3).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Transfer {
    step: Step,
    /// Transmissions so far.
    attempts: u8,
    /// When this attempt's listen-before-talk began; `None` until it does.
    lbt_since: Option<Instant>,
    expect: Ack,
}

impl Transfer {
    pub(crate) const fn new() -> Transfer {
        Transfer {
            step: Step::Idle,
            attempts: 0,
            lbt_since: None,
            expect: Ack {
                msg_id: crate::ids::MsgId(0),
                from: AckName([0; 4]),
                to: AckName([0; 4]),
                refused: false,
            },
        }
    }

    pub(crate) fn step(&self) -> Step {
        self.step
    }

    pub(crate) fn idle(&self) -> bool {
        self.step == Step::Idle
    }

    /// Whether the frame has been on air at least once.
    pub(crate) fn transmitted(&self) -> bool {
        self.attempts > 0
    }

    /// The ACK that takes this hand-over's frame.
    pub(crate) fn expect(&self) -> Ack {
        self.expect
    }

    pub(crate) fn awaiting_ack(&self) -> bool {
        matches!(self.step, Step::AwaitAck(_))
    }

    /// Begin a hand-over that ends with `expect`, ready at once, after
    /// `attempts` unanswered transmissions made before it waited.
    pub(crate) fn start(&mut self, expect: Ack, attempts: u8, now: Instant) {
        *self = Transfer {
            step: Step::Wait(now),
            attempts,
            lbt_since: None,
            expect,
        };
    }

    /// Give the hand-over up without a result.
    pub(crate) fn cancel(&mut self) {
        self.step = Step::Idle;
    }

    /// Whether listen-before-talk is due to start.
    pub(crate) fn due(&self, now: Instant) -> bool {
        matches!(self.step, Step::Wait(t) if t <= now)
    }

    /// The instant this hand-over next needs the core, if it is waiting.
    pub(crate) fn wake_at(&self) -> Option<Instant> {
        match self.step {
            Step::Wait(t) | Step::AwaitAck(t) => Some(t),
            _ => None,
        }
    }

    /// Hold the hand-over until `until`, without starting a new
    /// listen-before-talk period: to let an ACK this node owes go first.
    pub(crate) fn hold(&mut self, until: Instant) {
        self.step = Step::Wait(until);
    }

    /// Hold the hand-over until the duty-cycle budget allows it. The
    /// listen-before-talk give-up counts time the channel was found busy;
    /// a wait of minutes for the budget is not that, so the checks after
    /// it start a new period.
    pub(crate) fn defer(&mut self, until: Instant) {
        self.lbt_since = None;
        self.step = Step::Wait(until);
    }

    /// Listen-before-talk starts: the caller issues the CAD.
    pub(crate) fn begin_lbt(&mut self, now: Instant) {
        self.lbt_since.get_or_insert(now);
        self.step = Step::Cad;
    }

    /// A CAD or RSSI check completed.
    pub(crate) fn checked(
        &mut self,
        busy: bool,
        now: Instant,
        rng: &mut Rng,
        backoff_max: Duration,
    ) -> Check {
        if !busy {
            if self.step == Step::Cad {
                self.step = Step::Rssi;
            }
            return Check::Clear;
        }
        let since = self.lbt_since.unwrap_or(now);
        if now.since(since) >= LBT_GIVE_UP {
            self.lbt_since = None;
            self.step = Step::Idle;
            return Check::GaveUp;
        }
        self.step = Step::Wait(now + rng.duration(Duration::ZERO, backoff_max));
        Check::Busy
    }

    /// The caller is transmitting the frame.
    pub(crate) fn transmitting(&mut self) {
        self.step = Step::Tx;
    }

    /// The frame left the antenna: wait `ack_timeout` for the ACK.
    pub(crate) fn sent(&mut self, now: Instant, ack_timeout: Duration) {
        self.attempts = self.attempts.saturating_add(1);
        self.lbt_since = None;
        self.step = Step::AwaitAck(now + ack_timeout);
    }

    /// Whether `ack` answers this hand-over, taking or refusing a frame
    /// that has been sent.
    pub(crate) fn answered_by(&self, ack: Ack) -> bool {
        !self.idle() && self.transmitted() && ack.answers(&self.expect)
    }

    /// The receiver refused the hand-over, which gives it up: the
    /// unanswered transmissions before the refused one, to resume with.
    pub(crate) fn refused(&mut self) -> u8 {
        self.step = Step::Idle;
        self.attempts.saturating_sub(1)
    }

    /// An ACK arrived. Returns whether it takes this hand-over's frame,
    /// which ends the hand-over.
    pub(crate) fn acked(&mut self, ack: Ack) -> bool {
        let taken = self.answered_by(ack) && !ack.refused;
        if taken {
            self.step = Step::Idle;
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
        self.attempts = self.attempts.saturating_add(1);
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
        match self.step {
            Step::AwaitAck(deadline) if now >= deadline => {
                Some(self.after_attempt(now, max, rng, retry_backoff))
            }
            _ => None,
        }
    }

    fn after_attempt(
        &mut self,
        now: Instant,
        max: u8,
        rng: &mut Rng,
        retry_backoff: Duration,
    ) -> Missed {
        if self.attempts >= max {
            self.step = Step::Idle;
            Missed::Failed
        } else {
            self.lbt_since = None;
            let window = retry_window(retry_backoff, self.attempts);
            self.step = Step::Wait(now + rng.duration(Duration::ZERO, window));
            Missed::Retry
        }
    }
}

/// A reply this node owes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Owed {
    pub(crate) reply: Ack,
    /// The channel the sender listens on.
    pub(crate) channel: Channel,
    /// When the sender stops waiting; an ACK after that is useless.
    deadline: Instant,
    /// Not before this: a listen-before-talk backoff.
    ready: Instant,
}

/// Listen-before-talk progress of the reply being sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReplyStep {
    Idle,
    Cad,
    Rssi,
    Tx,
}

/// The one reply this node can owe (roadmap D29). It is sent with
/// listen-before-talk, as the receiver of §9.3 does, with the hand-over's
/// backoff, and abandoned once its sender has stopped waiting.
///
/// One place, because a node that owes a reply has the radio's next
/// transmission promised: a second packet it took or refused meanwhile
/// would be answered only after the first reply, and its sender may have
/// stopped waiting by then. A relay takes or refuses a packet only when
/// [`Replies::can_owe`] its reply, and a leaf takes a DELIVER only then;
/// otherwise the node stays silent and the sender's retry brings the
/// packet again.
#[derive(Clone)]
pub(crate) struct Replies {
    owed: Option<Owed>,
    step: ReplyStep,
}

impl Replies {
    pub(crate) const fn new() -> Self {
        Replies {
            owed: None,
            step: ReplyStep::Idle,
        }
    }

    /// Whether `reply` on `channel` can be owed now: the place is free, or
    /// holds that reply already, for an earlier copy of the packet.
    pub(crate) fn can_owe(&mut self, reply: Ack, channel: Channel, now: Instant) -> bool {
        self.forget_late(now);
        self.owed
            .is_none_or(|o| o.reply == reply && o.channel == channel)
    }

    /// Owe `reply` on `channel` until `deadline`. A reply owed already
    /// answers this copy too. Another one that has not started out gives
    /// way, as the one whose packet is older; one being sent does not, and
    /// `reply` is then not owed.
    pub(crate) fn owe(&mut self, reply: Ack, channel: Channel, deadline: Instant, now: Instant) {
        self.forget_late(now);
        if let Some(o) = self.owed {
            if (o.reply == reply && o.channel == channel) || self.step != ReplyStep::Idle {
                return;
            }
        }
        self.owed = Some(Owed {
            reply,
            channel,
            deadline,
            ready: now,
        });
    }

    /// Drop the owed reply if its sender has stopped waiting, unless it is
    /// being sent.
    fn forget_late(&mut self, now: Instant) {
        if self.step == ReplyStep::Idle && self.owed.is_some_and(|o| o.deadline <= now) {
            self.owed = None;
        }
    }

    /// The reply to start sending now, if it is ready and not in progress.
    pub(crate) fn due(&mut self, now: Instant) -> Option<Owed> {
        if self.step != ReplyStep::Idle {
            return None;
        }
        self.forget_late(now);
        self.owed.filter(|o| o.ready <= now)
    }

    /// When the owed reply is ready to send.
    pub(crate) fn wake_at(&self) -> Option<Instant> {
        if self.step != ReplyStep::Idle {
            return None;
        }
        self.owed.map(|o| o.ready)
    }

    #[cfg(test)]
    pub(crate) fn pending(&self) -> bool {
        self.owed.is_some()
    }

    /// Give up the owed reply: no budget for it.
    pub(crate) fn abandon(&mut self) {
        self.owed = None;
        self.step = ReplyStep::Idle;
    }

    /// The CAD of the owed reply starts.
    pub(crate) fn begin(&mut self) -> Option<Action> {
        let o = self.owed?;
        self.step = ReplyStep::Cad;
        Some(Action::Cad(o.channel))
    }

    /// A check of the reply being sent completed. Returns the next
    /// excursion's channel and whether it is the transmission, or `None`
    /// if the reply now waits or was abandoned.
    pub(crate) fn checked(
        &mut self,
        busy: bool,
        now: Instant,
        rng: &mut Rng,
        backoff_max: Duration,
    ) -> Option<(Owed, bool)> {
        let o = self.owed.as_mut()?;
        if busy {
            self.step = ReplyStep::Idle;
            let ready = now + rng.duration(Duration::ZERO, backoff_max);
            if ready >= o.deadline {
                self.owed = None;
            } else {
                o.ready = ready;
            }
            return None;
        }
        let o = *o;
        match self.step {
            ReplyStep::Cad => {
                self.step = ReplyStep::Rssi;
                Some((o, false))
            }
            ReplyStep::Rssi => {
                self.step = ReplyStep::Tx;
                Some((o, true))
            }
            ReplyStep::Idle | ReplyStep::Tx => None,
        }
    }

    /// The reply being sent left the antenna.
    pub(crate) fn sent(&mut self) {
        if self.step == ReplyStep::Tx {
            self.owed = None;
        }
        self.step = ReplyStep::Idle;
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
            from: AckName([1, 2, 3, 4]),
            to: AckName([5, 6, 7, 8]),
            refused: false,
        }
    }

    #[test]
    fn three_attempts_then_failure() {
        let mut rng = Rng::new(1);
        let mut t = Transfer::new();
        t.start(ack(7), 0, at(0));
        assert!(t.due(at(0)) && !t.transmitted());
        for attempt in 0..3u64 {
            let now = t.wake_at().unwrap();
            assert!(t.due(now));
            t.begin_lbt(now);
            assert_eq!(t.checked(false, now, &mut rng, ms(50)), Check::Clear);
            assert_eq!(t.step(), Step::Rssi);
            assert_eq!(t.checked(false, now, &mut rng, ms(50)), Check::Clear);
            t.transmitting();
            t.sent(now, ms(100));
            assert!(t.awaiting_ack());
            assert_eq!(t.timeout(now + ms(99), 3, &mut rng, ms(50)), None);
            let missed = t.timeout(now + ms(100), 3, &mut rng, ms(50)).unwrap();
            if attempt < 2 {
                assert_eq!(missed, Missed::Retry);
                let w = t.wake_at().unwrap();
                // Two units after the first transmission, four after the second.
                let window = ms(50 << (attempt + 1));
                assert!(
                    w >= now + ms(100) && w <= now + ms(100) + window,
                    "backoff 0–{} ms",
                    window.as_millis()
                );
            } else {
                assert_eq!(missed, Missed::Failed);
                assert!(t.idle());
            }
        }
    }

    #[test]
    fn ack_must_name_message_and_both_parties() {
        let mut rng = Rng::new(2);
        let mut t = Transfer::new();
        t.start(ack(7), 0, at(0));
        assert!(!t.acked(ack(7)), "nothing sent yet");
        t.begin_lbt(at(0));
        t.checked(false, at(0), &mut rng, ms(50));
        t.checked(false, at(1), &mut rng, ms(50));
        t.transmitting();
        t.sent(at(2), ms(100));
        assert!(!t.acked(ack(8)));
        assert!(
            !t.acked(Ack {
                from: AckName([9; 4]),
                ..ack(7)
            }),
            "same message, other receiver (D19)"
        );
        let to_another = Ack {
            to: AckName([9; 4]),
            ..ack(7)
        };
        assert!(
            !t.answered_by(to_another) && !t.acked(to_another),
            "same message and receiver, answering another sender (D32)"
        );
        let refused = Ack {
            refused: true,
            ..ack(7)
        };
        assert!(t.answered_by(refused) && !t.acked(refused), "a refusal");
        assert!(!t.idle(), "a refusal is the role's to act on");
        // Late, during the retry backoff: still the receiver's ACK.
        assert_eq!(t.timeout(at(102), 3, &mut rng, ms(50)), Some(Missed::Retry));
        assert!(t.acked(ack(7)));
        assert!(t.idle());
    }

    #[test]
    fn busy_channel_backs_off_and_gives_up_after_500_ms() {
        let mut rng = Rng::new(3);
        let mut t = Transfer::new();
        t.start(ack(1), 0, at(0));
        let mut now = at(0);
        let mut checks = 0;
        loop {
            t.begin_lbt(now);
            checks += 1;
            match t.checked(true, now, &mut rng, ms(50)) {
                Check::Busy => now = t.wake_at().unwrap(),
                Check::GaveUp => break,
                Check::Clear => unreachable!(),
            }
        }
        assert!(now >= at(500) && now < at(550), "{now:?}");
        assert!(checks > 10 && t.idle());
        // A retry starts a fresh 500 ms.
        t.start(ack(1), 0, now);
        t.begin_lbt(now);
        assert_eq!(
            t.checked(true, now + ms(499), &mut rng, ms(50)),
            Check::Busy
        );
    }

    #[test]
    fn a_budget_hold_starts_a_new_listen_before_talk_period() {
        let mut rng = Rng::new(5);
        let mut t = Transfer::new();
        t.start(ack(1), 0, at(0));
        t.begin_lbt(at(0));
        assert_eq!(t.checked(true, at(0), &mut rng, ms(50)), Check::Busy);
        t.defer(at(60_000));
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
        a.owe(ack(1), ch, at(100), at(0));
        assert!(a.can_owe(ack(1), ch, at(10)), "a copy is answered too");
        a.owe(ack(1), ch, at(110), at(10));
        assert!(!a.can_owe(ack(2), ch, at(10)), "one place");
        assert!(!a.can_owe(ack(1), Channel::new(4).unwrap(), at(10)));
        assert_eq!(a.due(at(10)).unwrap().reply, ack(1));
        assert_eq!(a.begin(), Some(Action::Cad(ch)));
        assert_eq!(a.due(at(10)), None, "one at a time");
        a.owe(ack(2), ch, at(111), at(11));
        assert_eq!(
            a.checked(false, at(11), &mut rng, ms(50)),
            Some((a.owed.unwrap(), false)),
            "the reply being sent keeps its place"
        );
        assert_eq!(a.owed.unwrap().reply, ack(1));
        assert_eq!(
            a.checked(false, at(12), &mut rng, ms(50)).map(|(_, tx)| tx),
            Some(true)
        );
        a.sent();
        assert!(!a.pending());
        assert!(a.can_owe(ack(2), ch, at(13)));
        // Busy until past the deadline: abandoned.
        a.owe(ack(2), ch, at(100), at(60));
        a.due(at(60)).unwrap();
        a.begin();
        let mut now = at(60);
        while a.pending() {
            assert_eq!(a.checked(true, now, &mut rng, ms(50)), None);
            if let Some(w) = a.wake_at() {
                assert!(w < at(100));
                now = w;
                a.due(now).unwrap();
                a.begin();
            }
        }
        // Not started: a newer one takes the place.
        a.owe(ack(3), ch, at(300), at(200));
        a.owe(ack(4), ch, at(301), at(200));
        assert_eq!(a.due(at(201)).unwrap().reply, ack(4));
        // A lapsed one frees the place.
        assert!(!a.can_owe(ack(5), ch, at(300)));
        assert!(a.can_owe(ack(5), ch, at(301)));
        assert_eq!(a.due(at(301)), None);
        // One refusal answers a retransmission too, but not another message.
        let busy = |id| Ack {
            refused: true,
            ..ack(id)
        };
        a.owe(busy(1), ch, at(500), at(400));
        assert!(a.can_owe(busy(1), ch, at(410)));
        assert!(!a.can_owe(busy(2), ch, at(410)));
        assert!(!a.can_owe(ack(1), ch, at(410)));
    }
}
