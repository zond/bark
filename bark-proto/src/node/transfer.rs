//! Per-hop transfer (design §9.3), shared by both roles: one frame handed
//! to one receiver until it ACKs or the sender gives up ([`Transfer`]), and
//! the replies, ACK or BUSY, this node owes the senders of what it
//! received ([`Replies`]).
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
//! An ACK matches only if it names both the message and the receiver
//! (roadmap D19). An ACK that arrives after the timeout but before the
//! next attempt still counts: the receiver has the frame. A BUSY matches
//! if it names the receiver (D27); the role decides what follows.

use crate::event::Action;
use crate::node::Rng;
use crate::radio::{time_on_air, Channel, MIN_PREAMBLE_SYMBOLS};
use crate::tables::fixed::FixedVec;
use crate::tables::{retry_window, LBT_GIVE_UP};
use crate::time::{Duration, Instant};
use crate::wire::{Ack, AckFrom, Busy, Packet, ATTACH_LEN};

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
                from: AckFrom([0; 4]),
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

    /// The ACK that ends this hand-over.
    pub(crate) fn expect(&self) -> Ack {
        self.expect
    }

    pub(crate) fn awaiting_ack(&self) -> bool {
        matches!(self.step, Step::AwaitAck(_))
    }

    /// Begin a hand-over that ends with `expect`, ready at once.
    pub(crate) fn start(&mut self, expect: Ack, now: Instant) {
        *self = Transfer {
            step: Step::Wait(now),
            attempts: 0,
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

    /// Whether `ack` ends this hand-over.
    pub(crate) fn answered_by(&self, ack: Ack) -> bool {
        !self.idle() && self.transmitted() && ack == self.expect
    }

    /// Whether a BUSY from `from` refuses this hand-over: it names the
    /// receiver of a frame that has been sent.
    pub(crate) fn refused_by(&self, from: AckFrom) -> bool {
        !self.idle() && self.transmitted() && from == self.expect.from
    }

    /// The receiver refused a hand-over that has no other receiver to go
    /// to: the transmission counts as unanswered.
    pub(crate) fn refused(
        &mut self,
        now: Instant,
        max: u8,
        rng: &mut Rng,
        retry_backoff: Duration,
    ) -> Missed {
        self.after_attempt(now, max, rng, retry_backoff)
    }

    /// An ACK arrived. Returns whether it ends this hand-over, which it
    /// then does.
    pub(crate) fn acked(&mut self, ack: Ack) -> bool {
        let matched = self.answered_by(ack);
        if matched {
            self.step = Step::Idle;
        }
        matched
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

/// How many replies a node can owe at once with the design's 100 ms ACK
/// timeout (§9.3). A reply is owed from the end of the frame it answers
/// until the sender stops waiting, `ack_timeout` later; frames on one
/// channel do not overlap, and the shortest frame that is answered,
/// ATTACH, takes 11.84 ms, so ⌈100 / 11.84⌉ = 9 can be owed at once.
pub(crate) const fn replies_for(ack_timeout: Duration) -> usize {
    ack_timeout
        .as_micros()
        .div_ceil(time_on_air(ATTACH_LEN as u8, MIN_PREAMBLE_SYMBOLS).as_micros()) as usize
}

/// Replies a relay can owe at once: [`replies_for`] the design's 100 ms.
pub(crate) const RELAY_REPLIES: usize = replies_for(Duration::from_millis(100));

const _: () = assert!(RELAY_REPLIES == 9);

/// What a node answers a packet with (§9.3, §9.2.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reply {
    Ack(Ack),
    Busy(Busy),
}

impl Reply {
    pub(crate) fn packet(self) -> Packet<'static> {
        match self {
            Reply::Ack(a) => Packet::Ack(a),
            Reply::Busy(b) => Packet::Busy(b),
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

/// The replies this node owes, oldest first. Each is sent with
/// listen-before-talk, as the receiver of §9.3 does, with the hand-over's
/// backoff, and abandoned once its sender has stopped waiting.
///
/// A node configured with a longer ACK timeout than the one `N` was sized
/// for can owe more than `N`; then the oldest is abandoned, as the one
/// whose sender stops waiting first.
#[derive(Clone)]
pub(crate) struct Replies<const N: usize> {
    owed: FixedVec<Owed, N>,
    step: ReplyStep,
}

impl<const N: usize> Replies<N> {
    pub(crate) const fn new() -> Self {
        let fill = Owed {
            reply: Reply::Ack(Ack {
                msg_id: crate::ids::MsgId(0),
                from: AckFrom([0; 4]),
            }),
            channel: match Channel::new(0) {
                Some(c) => c,
                None => unreachable!(),
            },
            deadline: Instant::ZERO,
            ready: Instant::ZERO,
        };
        Replies {
            owed: FixedVec::new(fill),
            step: ReplyStep::Idle,
        }
    }

    /// Owe `reply` on `channel` until `deadline`.
    pub(crate) fn owe(&mut self, reply: Reply, channel: Channel, deadline: Instant, now: Instant) {
        self.forget_late(now);
        let o = Owed {
            reply,
            channel,
            deadline,
            ready: now,
        };
        if self
            .owed
            .as_slice()
            .iter()
            .any(|e| e.reply == reply && e.channel == channel)
        {
            // Already owed for an earlier copy, or a BUSY for another
            // sender on the channel: one reply answers both.
            return;
        }
        if self.owed.push(o).is_err() {
            let first = usize::from(self.step != ReplyStep::Idle);
            if first < self.owed.len() {
                self.owed.remove(first);
            }
            let _ = self.owed.push(o);
        }
    }

    /// Drop owed replies whose sender has stopped waiting, except the one
    /// being sent.
    fn forget_late(&mut self, now: Instant) {
        let busy = self.step != ReplyStep::Idle;
        let mut i = usize::from(busy);
        while i < self.owed.len() {
            if self.owed.as_slice()[i].deadline <= now {
                self.owed.remove(i);
            } else {
                i += 1;
            }
        }
    }

    /// The reply to start sending now, if one is ready and none is in
    /// progress.
    pub(crate) fn due(&mut self, now: Instant) -> Option<Owed> {
        if self.step != ReplyStep::Idle {
            return None;
        }
        self.forget_late(now);
        self.owed
            .as_slice()
            .first()
            .filter(|o| o.ready <= now)
            .copied()
    }

    /// When the next owed reply is ready to send.
    pub(crate) fn wake_at(&self) -> Option<Instant> {
        if self.step != ReplyStep::Idle {
            return None;
        }
        self.owed.as_slice().first().map(|o| o.ready)
    }

    #[cfg(test)]
    pub(crate) fn pending(&self) -> bool {
        !self.owed.as_slice().is_empty()
    }

    /// Give up the first owed reply: no budget for it.
    pub(crate) fn abandon(&mut self) {
        if !self.owed.as_slice().is_empty() {
            self.owed.remove(0);
        }
        self.step = ReplyStep::Idle;
    }

    /// The CAD of the first owed reply starts.
    pub(crate) fn begin(&mut self) -> Option<Action> {
        let o = self.owed.as_slice().first()?;
        self.step = ReplyStep::Cad;
        Some(Action::Cad(o.channel))
    }

    /// A check of the reply being sent completed. Returns the next
    /// excursion's channel and whether it is the transmission, or `None`
    /// if the ACK now waits or was abandoned.
    pub(crate) fn checked(
        &mut self,
        busy: bool,
        now: Instant,
        rng: &mut Rng,
        backoff_max: Duration,
    ) -> Option<(Owed, bool)> {
        let o = *self.owed.as_slice().first()?;
        if busy {
            self.step = ReplyStep::Idle;
            let ready = now + rng.duration(Duration::ZERO, backoff_max);
            if ready >= o.deadline {
                self.owed.remove(0);
            } else {
                self.owed.as_mut_slice()[0].ready = ready;
            }
            return None;
        }
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
        if self.step == ReplyStep::Tx && !self.owed.as_slice().is_empty() {
            self.owed.remove(0);
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
            from: AckFrom([1, 2, 3, 4]),
        }
    }

    #[test]
    fn three_attempts_then_failure() {
        let mut rng = Rng::new(1);
        let mut t = Transfer::new();
        t.start(ack(7), at(0));
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
    fn ack_must_name_message_and_sender() {
        let mut rng = Rng::new(2);
        let mut t = Transfer::new();
        t.start(ack(7), at(0));
        assert!(!t.acked(ack(7)), "nothing sent yet");
        t.begin_lbt(at(0));
        t.checked(false, at(0), &mut rng, ms(50));
        t.checked(false, at(1), &mut rng, ms(50));
        t.transmitting();
        t.sent(at(2), ms(100));
        assert!(!t.acked(ack(8)));
        assert!(
            !t.acked(Ack {
                msg_id: MsgId(7),
                from: AckFrom([9; 4])
            }),
            "same message, other relay (D19)"
        );
        // Late, during the retry backoff: still the receiver's ACK.
        assert_eq!(t.timeout(at(102), 3, &mut rng, ms(50)), Some(Missed::Retry));
        assert!(t.acked(ack(7)));
        assert!(t.idle());
    }

    #[test]
    fn busy_channel_backs_off_and_gives_up_after_500_ms() {
        let mut rng = Rng::new(3);
        let mut t = Transfer::new();
        t.start(ack(1), at(0));
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
        t.start(ack(1), now);
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
        t.start(ack(1), at(0));
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
    fn acks_wait_out_backoff_and_lapse_at_the_deadline() {
        let mut rng = Rng::new(4);
        let ch = Channel::new(3).unwrap();
        let mut a: Replies<2> = Replies::new();
        let ack = |id| Reply::Ack(ack(id));
        a.owe(ack(1), ch, at(100), at(0));
        a.owe(ack(1), ch, at(110), at(10));
        assert_eq!(a.owed.len(), 1, "one ACK answers a retransmission too");
        assert_eq!(a.due(at(10)).unwrap().reply, ack(1));
        assert_eq!(a.begin(), Some(Action::Cad(ch)));
        assert_eq!(a.due(at(10)), None, "one at a time");
        assert_eq!(
            a.checked(false, at(11), &mut rng, ms(50)).map(|(_, tx)| tx),
            Some(false)
        );
        assert_eq!(
            a.checked(false, at(12), &mut rng, ms(50)).map(|(_, tx)| tx),
            Some(true)
        );
        a.sent();
        assert!(!a.pending());
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
        // Full: the oldest gives way.
        a.owe(ack(3), ch, at(300), at(200));
        a.owe(ack(4), ch, at(301), at(200));
        a.owe(ack(5), ch, at(302), at(201));
        assert_eq!(a.due(at(201)).unwrap().reply, ack(4));
        // Lapsed ones are forgotten.
        assert_eq!(a.due(at(301)).unwrap().reply, ack(5));
        assert_eq!(replies_for(Duration::from_millis(100)), 9);
        // One BUSY tells every sender on a channel.
        let busy = Reply::Busy(Busy {
            from: crate::position::ShortName(9),
        });
        a.owe(busy, ch, at(500), at(400));
        a.owe(busy, ch, at(510), at(410));
        assert_eq!(a.owed.len(), 1);
    }
}
