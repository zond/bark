//! The leaves a relay serves (design §10): ATTACH into the leaf table,
//! the polls that keep it, and delivery from the delivery queue.
//!
//! # Polls (§10.2)
//!
//! An ATTACH that attaches is answered with ATTACHED, which names the
//! leaf's slot. At each slot the relay sends the leaf an ATTACHED, its
//! poll, and the leaf answers with an ATTACH marked as a reply, which is
//! not answered. A poll goes after an owed reply and before the heartbeat
//! and the hand-over; one that cannot start within [`POLL_LATE`] of its
//! slot is skipped, so that a sleeping leaf's window bounds it. Each
//! ATTACHED counts the messages queued for the leaf, and while a sleeping
//! leaf listens for those after answering, their DELIVERs have the short
//! preamble (§10.5). A leaf that leaves its polls unanswered for a
//! delivery TTL is dropped (`tables/leaves.rs`).
//!
//! Every message for a leaf goes through the delivery queue (§10.4): it is
//! queued when it arrives here, whether or not the leaf is attached, and
//! leaves the queue when the leaf ACKs its DELIVER or when the delivery TTL
//! runs out. A queued message for an attached leaf is handed to the send
//! queue as a DELIVER, one per leaf at a time and oldest first, when it
//! arrives, when the previous one is ACKed, on each answer to a poll,
//! every `delivery_retry` (§10.4: 60 s), and whenever a hand-over ends and
//! frees room in the send queue. It needs room as a forward does: a place,
//! and its airtime with everything queued in the bucket (§9.3). One the
//! bucket cannot fit yet is offered again the moment it will, as a
//! deferred hand-over is started then (`feed_at`). A DELIVER
//! whose hand-over fails stays queued, and its leaf is marked as not
//! listening: freed room does not go to it, only its next answer to a
//! poll and the 60 s retry do, or a DELIVER would follow DELIVER to a dark
//! phone.
//!
//! An unknown leaf is queued like an absent one: §9.2.7 has the owner
//! queue when nobody knows the leaf, and it may attach within the TTL.
//!
//! A relay whose leaf table is full does not answer, so the leaf attaches
//! elsewhere (`tables/leaves.rs`). ATTACHED and polls are replies and may
//! spend the reply reserve (§9.4): a busy relay still keeps its leaves.

use super::{Check, Job, Relay};
use crate::event::{Action, DropReason, Frame, Trace};
use crate::ids::{LeafId, MsgId};
use crate::node::mac;
use crate::node::transfer::{Owed, Reply};
use crate::radio::MIN_PREAMBLE_SYMBOLS;
use crate::tables::budget::REPLY_RESERVE;
use crate::tables::leaves::{POLL_AIRTIME, POLL_LATE};
use crate::tables::send_queue::{Kind, Outgoing};
use crate::time::{Duration, Instant};
use crate::wire::{Attach, Attached, Deliver, Packet, RxMode, WAITING_MAX};

/// The poll in progress (design §10.2).
pub(super) struct Poller {
    /// The leaf and its slot.
    pub(super) current: Option<(LeafId, Instant)>,
    /// Not before this: a listen-before-talk backoff.
    pub(super) ready_at: Instant,
    pub(super) check: Check,
    /// The poll sent last, while its answer may still come: the leaf and
    /// the end of the wait.
    pub(super) answer: Option<(LeafId, Instant)>,
}

impl Poller {
    pub(super) const IDLE: Poller = Poller {
        current: None,
        ready_at: Instant::ZERO,
        check: Check::Idle,
        answer: None,
    };
}

impl Relay {
    /// An ATTACH for this relay (§10.2): one that attaches is answered with
    /// ATTACHED; one that answers a poll is not answered.
    pub(super) fn attach(&mut self, a: Attach, now: Instant) {
        if a.reply {
            if self.poller.answer.is_some_and(|(l, _)| l == a.leaf) {
                self.poller.answer = None;
            }
            if self.leaves.answered(a.leaf, a.mode, now) {
                self.leaves.measured(a.leaf, self.leaf_loss());
                self.deliver_now(a.leaf, now);
            }
            return;
        }
        let reply = Reply::Attached(a.leaf);
        // Taken only if it can be answered, as a DATA packet is (§9.3).
        if !self.can_reply(POLL_AIRTIME, now) || !self.replies.can_owe(reply, self.home, now) {
            return;
        }
        let waiting = self.waiting(a.leaf);
        if self.leaves.attached(a.leaf, a.mode, waiting, now).is_err() {
            return;
        }
        self.leaves.measured(a.leaf, self.leaf_loss());
        self.reply_power = self.reply_power_for(None);
        self.replies
            .owe(reply, self.home, now + self.config.ack_timeout, now);
        self.deliver_now(a.leaf, now);
    }

    /// Messages queued for `leaf`, as ATTACHED counts them.
    fn waiting(&self, leaf: LeafId) -> u8 {
        self.delivery.count_for(leaf).min(WAITING_MAX as usize) as u8
    }

    /// ATTACHED for `leaf`, sent at `now`, naming its next slot.
    fn attached_frame(&self, leaf: LeafId, now: Instant) -> Frame {
        let entry = self.leaves.get(leaf);
        let next = entry.map_or(Duration::ZERO, |e| e.next_poll.since(now + POLL_AIRTIME));
        let waiting = entry.map_or(0, |e| e.waiting);
        let a = Attached {
            leaf,
            next_poll: Attached::units(next),
            waiting,
        };
        mac::frame(&Packet::Attached(a), self.home, MIN_PREAMBLE_SYMBOLS)
    }

    /// The frame of the owed reply, as sent at `now`.
    pub(super) fn reply_frame(&self, o: Owed, now: Instant) -> Frame {
        match o.reply {
            Reply::Ack(a) => mac::frame(&Packet::Ack(a), o.channel, MIN_PREAMBLE_SYMBOLS),
            Reply::Attached(leaf) => self.attached_frame(leaf, now),
            Reply::Attach(a) => mac::frame(&Packet::Attach(a), o.channel, MIN_PREAMBLE_SYMBOLS),
        }
    }

    // --- Polls (§10.2) ---

    /// Whether a poll may start now: one has begun, or a slot has come.
    pub(super) fn poll_due(&self, now: Instant) -> bool {
        self.poller.check == Check::Idle
            && match self.poller.current {
                Some(_) => now >= self.poller.ready_at,
                None => self.leaves.poll_due(now).is_some(),
            }
    }

    /// The poll in progress cannot go out within its slot's lateness.
    fn skip_poll(&mut self, emit: &mut dyn FnMut(Action)) {
        if let Some((leaf, _)) = self.poller.current.take() {
            self.leaves.polled(leaf);
            emit(Action::Trace(Trace::PollSkipped { leaf }));
        }
        self.poller.check = Check::Idle;
    }

    /// Start listen-before-talk for the due poll, or skip it if it is too
    /// late or the bucket cannot pay for it.
    pub(super) fn start_poll(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) -> bool {
        if self.poller.current.is_none() {
            self.poller.current = self.leaves.poll_due(now);
            self.poller.ready_at = now;
        }
        let Some((_, slot)) = self.poller.current else {
            return false;
        };
        if now > slot + POLL_LATE || !self.can_reply(POLL_AIRTIME, now) {
            self.skip_poll(emit);
            return false;
        }
        self.poller.check = Check::Cad;
        self.radio.start(Job::Poll, Action::Cad(self.home), emit);
        true
    }

    /// A CAD or RSSI check of the poll completed.
    pub(super) fn poll_check(&mut self, busy: bool, now: Instant, emit: &mut dyn FnMut(Action)) {
        let Some((leaf, slot)) = self.poller.current else {
            self.poller.check = Check::Idle;
            return;
        };
        let late = slot + POLL_LATE;
        if busy {
            self.poller.check = Check::Idle;
            let backoff = self.config.hop_backoff_max;
            self.poller.ready_at = now + self.rng.duration(Duration::ZERO, backoff);
            if self.poller.ready_at > late {
                self.skip_poll(emit);
            }
            return;
        }
        if self.poller.check == Check::Cad {
            self.poller.check = Check::Rssi;
            self.radio.start(Job::Poll, Action::Rssi(self.home), emit);
            return;
        }
        // An owed reply goes first; the poll checks the channel again.
        if self.replies.wake_at().is_some_and(|t| t <= now) {
            self.poller.check = Check::Idle;
            self.poller.ready_at = now;
            return;
        }
        if now > late || !self.can_reply(POLL_AIRTIME, now) || self.leaves.get(leaf).is_none() {
            self.skip_poll(emit);
            return;
        }
        self.leaves.polled(leaf);
        self.leaves.told(leaf, self.waiting(leaf), now);
        let mut frame = self.attached_frame(leaf, now);
        frame.power_dbm = self.leaf_power(leaf);
        self.budget.record(mac::airtime(&frame), now);
        self.poller.check = Check::Tx;
        self.radio.start(Job::Poll, Action::Transmit(frame), emit);
    }

    /// The poll left the antenna.
    pub(super) fn poll_sent(&mut self, now: Instant) {
        let answer = self
            .poller
            .current
            .map(|(leaf, _)| (leaf, now + self.config.ack_timeout));
        self.poller = Poller {
            answer,
            ..Poller::IDLE
        };
    }

    /// Until when the relay keeps listening for the answer to its last
    /// poll, as it does for the ACK of a hand-over (§9.3): a frame of its
    /// own would make it deaf to it. Cleared once that is past.
    pub(super) fn answer_wait(&mut self, now: Instant) -> Option<Instant> {
        if self.poller.answer.is_some_and(|(_, t)| t <= now) {
            self.poller.answer = None;
        }
        self.poller.answer.map(|(_, t)| t)
    }

    /// When the poll in progress, or the next slot, needs the radio.
    pub(super) fn poll_wakeup(&self) -> Option<Instant> {
        if let Some((_, t)) = self.poller.answer {
            return Some(t);
        }
        match self.poller.current {
            Some(_) if self.poller.check == Check::Idle => Some(self.poller.ready_at),
            Some(_) => None,
            None => self.leaves.next_poll(),
        }
    }

    /// Hand the oldest message queued for `leaf` to the send queue, if the
    /// leaf is attached, listening or `retry`, and has no DELIVER there
    /// already. If the send queue has no room for it: `Err(Some(t))` when
    /// the bucket will fit it at `t`, `Err(None)` when only a hand-over
    /// ending will make room.
    pub(super) fn try_deliver(
        &mut self,
        leaf: LeafId,
        retry: bool,
        now: Instant,
    ) -> Result<(), Option<Instant>> {
        let Some(entry) = self.leaves.get(leaf).copied() else {
            return Ok(());
        };
        let is_for = |o: &Outgoing| {
            o.kind == Kind::Deliver
                && matches!(Packet::decode(o.frame.payload()), Ok(Packet::Deliver(d)) if d.dst_leaf == leaf)
        };
        if (entry.missed && !retry) || self.queue.iter().any(is_for) {
            return Ok(());
        }
        let Some(q) = self.delivery.iter().find(|q| q.leaf == leaf) else {
            return Ok(());
        };
        let deliver = Deliver {
            dst_leaf: leaf,
            msg_id: q.msg_id,
            payload: q.payload(),
        };
        let preamble = self.deliver_preamble(leaf, q.msg_id, now);
        let frame = mac::frame(&Packet::Deliver(deliver), self.home, preamble);
        let airtime = mac::airtime(&frame);
        match self.no_room(airtime, Duration::ZERO, now) {
            None => {}
            Some(DropReason::SendQueueFull) => return Err(None),
            // With what is queued in the bucket too; `None` if that is more
            // than the bucket holds, and then the queue must drain first.
            Some(_) => {
                let queued = self.queue.airtime() + REPLY_RESERVE;
                return Err(self.budget.allowed_at(airtime, queued, now));
            }
        }
        let _ = self.queue.push(Outgoing::new(frame, Kind::Deliver));
        Ok(())
    }

    /// The preamble of a DELIVER of `msg_id` to `leaf` sent at `now`: the
    /// leaf's mode's, unless the leaf sleeps and listens with the ordinary
    /// receiver for what its last answered poll counted (§10.5).
    pub(super) fn deliver_preamble(&self, leaf: LeafId, msg_id: MsgId, now: Instant) -> u16 {
        let mode = self.leaves.get(leaf).map_or(RxMode::Listening, |e| e.mode);
        let counted = self
            .delivery
            .iter()
            .find(|q| q.msg_id == msg_id)
            .is_some_and(|q| self.leaves.awake_for(leaf, q.enqueued, now));
        if counted {
            MIN_PREAMBLE_SYMBOLS
        } else {
            mode.preamble_symbols()
        }
    }

    /// Offer the send queue the oldest queued message of each leaf, oldest
    /// first, until it has no room, and note when to offer again: when the
    /// bucket will fit the DELIVER that did not, or else when a hand-over
    /// ends (`feed_due`).
    pub(super) fn feed(&mut self, retry: bool, now: Instant) {
        self.feed_due = false;
        self.feed_at = None;
        let mut i = 0;
        loop {
            let Some(leaf) = self.delivery.iter().nth(i).map(|q| q.leaf) else {
                return;
            };
            let first = !self.delivery.iter().take(i).any(|q| q.leaf == leaf);
            if first {
                if let Err(at) = self.try_deliver(leaf, retry, now) {
                    self.feed_at = at;
                    return;
                }
            }
            i += 1;
        }
    }

    /// Hand `leaf`'s oldest message to the send queue as [`Self::feed`]
    /// would, for a leaf that is listening or not.
    pub(super) fn deliver_now(&mut self, leaf: LeafId, now: Instant) {
        if let Err(Some(at)) = self.try_deliver(leaf, true, now) {
            self.feed_at = Some(self.feed_at.map_or(at, |t| t.min(at)));
        }
    }

    /// The leaf ACKed the DELIVER being handed over.
    pub(super) fn delivered_to(
        &mut self,
        d: Deliver<'_>,
        now: Instant,
        emit: &mut dyn FnMut(Action),
    ) {
        self.delivery.remove(d.msg_id);
        self.delivered.insert(d.msg_id, now);
        self.leaves.set_missed(d.dst_leaf, false);
        emit(Action::Trace(Trace::Delivered {
            msg_id: d.msg_id,
            leaf: d.dst_leaf,
        }));
        self.deliver_now(d.dst_leaf, now);
    }

    /// Drop leaves that stopped answering (§10.2); expire and retry the
    /// delivery queue (§10.4).
    pub(super) fn service_timers(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        self.leaves
            .expire(now, |leaf| emit(Action::Trace(Trace::LeafDropped { leaf })));
        while let Some(q) = self.delivery.pop_expired(now) {
            // A DELIVER not yet on its way goes with it.
            let id = q.msg_id;
            let active = !self.handover.idle();
            self.queue.retain(|current, o| {
                (current && active) || o.kind != Kind::Deliver || o.msg_id() != Some(id)
            });
            self.feed_due = true;
            emit(Action::Trace(Trace::Dropped {
                msg_id: id,
                reason: DropReason::QueueExpired,
            }));
        }
        if now >= self.delivery_retry_at {
            self.delivery_retry_at = now + self.config.delivery_retry;
            self.feed(true, now);
        }
    }

    /// When the leaf service next needs the core.
    pub(super) fn service_wakeup(&self) -> Option<Instant> {
        let retry = (!self.delivery.is_empty()).then_some(self.delivery_retry_at);
        [
            self.delivery.next_expiry(),
            retry,
            self.feed_at,
            self.leaves.next_drop(),
        ]
        .into_iter()
        .flatten()
        .min()
    }
}
