//! The leaves a relay serves (design §10): ATTACH and keepalive into the
//! leaf table, and delivery from the delivery queue.
//!
//! Every message for a leaf goes through the delivery queue (§10.4): it is
//! queued when it arrives here, whether or not the leaf is attached, and
//! leaves the queue when the leaf ACKs its DELIVER or when the delivery TTL
//! runs out. A queued message for an attached leaf is handed to the send
//! queue as a DELIVER, one per leaf at a time and oldest first, when it
//! arrives, when the previous one is ACKed, on each keepalive of its leaf,
//! every `delivery_retry` (§10.4: 60 s), and whenever a hand-over ends and
//! frees room in the send queue. It needs room as a forward does: a place,
//! and its airtime with everything queued in the bucket (§9.3). One the
//! bucket cannot fit yet is offered again the moment it will, as a
//! deferred hand-over is started then (`feed_at`). A DELIVER
//! whose hand-over fails stays queued, and its leaf is marked as not
//! listening: freed room does not go to it, only its next keepalive and
//! the 60 s retry do, or a DELIVER would follow DELIVER to a dark phone.
//!
//! An unknown leaf is queued like an absent one: §9.2.7 has the owner
//! queue when nobody knows the leaf, and it may attach within the TTL.
//!
//! The ACK of an ATTACH carries the leaf's ID as the message ID and the
//! relay's short name (§14). A relay whose leaf table is full of live
//! leaves does not ACK, so the leaf attaches elsewhere (`tables/leaves.rs`).
//! The ACK of an ATTACH is a reply and may spend the reply reserve (§9.4):
//! a busy relay still keeps its leaves.

use super::Relay;
use crate::event::{Action, DropReason, Trace};
use crate::ids::{LeafId, MsgId};
use crate::node::mac;
use crate::node::transfer::Reply;
use crate::radio::{time_on_air, MIN_PREAMBLE_SYMBOLS};
use crate::tables::budget::REPLY_RESERVE;
use crate::tables::send_queue::{Kind, Outgoing};
use crate::time::{Duration, Instant};
use crate::wire::{Ack, AckFrom, Attach, Deliver, Packet, ACK_LEN};

impl Relay {
    /// An ATTACH or keepalive for this relay (§10.2).
    pub(super) fn attach(&mut self, a: Attach, now: Instant) {
        let ack = Reply::Ack(Ack {
            msg_id: MsgId(u32::from_be_bytes(a.leaf.0)),
            from: AckFrom::from(self.me()),
        });
        // Taken only if it can be answered, as a DATA packet is (§9.3).
        if !self.can_reply(time_on_air(ACK_LEN as u8, MIN_PREAMBLE_SYMBOLS), now)
            || !self.replies.can_owe(ack, self.home, now)
        {
            return;
        }
        if self.leaves.attached(a.leaf, a.mode, now).is_err() {
            return;
        }
        self.replies
            .owe(ack, self.home, now + self.config.ack_timeout, now);
        self.deliver_now(a.leaf, now);
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
        let Some(entry) = self.leaves.get(leaf, now).copied() else {
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
        let frame = mac::frame(
            &Packet::Deliver(deliver),
            self.home,
            entry.mode.preamble_symbols(),
        );
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

    /// The leaf ACKed the DELIVER at the front of the send queue.
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

    /// Expire and retry the delivery queue (§10.4).
    pub(super) fn service_timers(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        while let Some(q) = self.delivery.pop_expired(now) {
            // A DELIVER not yet on its way goes with it.
            let id = q.msg_id;
            let active = !self.handover.idle();
            let mut first = true;
            self.queue.retain(|o| {
                let keep = (first && active) || o.kind != Kind::Deliver || o.msg_id() != Some(id);
                first = false;
                keep
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
        [self.delivery.next_expiry(), retry, self.feed_at]
            .into_iter()
            .flatten()
            .min()
    }
}
