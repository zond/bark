//! The leaves a relay serves (design §10): ATTACH and keepalive into the
//! leaf table, and delivery from the delivery queue.
//!
//! Every message for a leaf goes through the delivery queue (§10.4): it is
//! queued when it arrives here, whether or not the leaf is attached, and
//! leaves the queue when the leaf ACKs its DELIVER or when the delivery TTL
//! runs out. A queued message for an attached leaf is handed to the send
//! queue as a DELIVER, one per leaf at a time and oldest first, when it
//! arrives, when the previous one is ACKed, on each keepalive of its leaf,
//! and every `delivery_retry` (§10.4: 60 s). A DELIVER whose hand-over
//! fails stays queued for those retries.
//!
//! An unknown leaf is queued like an absent one: §9.2.7 has the owner
//! queue when nobody knows the leaf, and it may attach within the TTL.
//!
//! The ACK of an ATTACH carries the leaf's ID as the message ID and the
//! relay's short name (§14). A relay whose leaf table is full of live
//! leaves does not ACK, so the leaf attaches elsewhere (`tables/leaves.rs`).

use super::Relay;
use crate::event::{Action, DropReason, Trace};
use crate::ids::{LeafId, MsgId};
use crate::node::mac;
use crate::radio::{time_on_air, MIN_PREAMBLE_SYMBOLS};
use crate::tables::send_queue::{Kind, Outgoing};
use crate::time::Instant;
use crate::wire::{Ack, AckFrom, Attach, Deliver, Packet, ACK_LEN};

impl Relay {
    /// An ATTACH or keepalive for this relay (§10.2).
    pub(super) fn attach(&mut self, a: Attach, now: Instant) {
        if !self.affordable(time_on_air(ACK_LEN as u8, MIN_PREAMBLE_SYMBOLS), now) {
            return;
        }
        if self.leaves.attached(a.leaf, a.mode, now).is_err() {
            return;
        }
        let ack = Ack { msg_id: MsgId(u32::from_be_bytes(a.leaf.0)), from: AckFrom::from(self.me()) };
        self.acks.owe(ack, self.home, now + self.config.ack_timeout, now);
        self.try_deliver(a.leaf, now);
    }

    /// Hand the oldest message queued for `leaf` to the send queue, if the
    /// leaf is attached and has no DELIVER there already.
    pub(super) fn try_deliver(&mut self, leaf: LeafId, now: Instant) {
        let Some(entry) = self.leaves.get(leaf, now).copied() else { return };
        let is_for = |o: &Outgoing| {
            o.kind == Kind::Deliver
                && matches!(Packet::decode(o.frame.payload()), Ok(Packet::Deliver(d)) if d.dst_leaf == leaf)
        };
        if self.queue.is_full() || self.queue.iter().any(is_for) {
            return;
        }
        let Some(q) = self.delivery.iter().find(|q| q.leaf == leaf) else { return };
        let deliver = Deliver { dst_leaf: leaf, msg_id: q.msg_id, payload: q.payload() };
        let frame = mac::frame(&Packet::Deliver(deliver), self.home, entry.mode.preamble_symbols());
        let _ = self.queue.push(Outgoing { frame, kind: Kind::Deliver });
    }

    /// The leaf ACKed the DELIVER at the front of the send queue.
    pub(super) fn delivered_to(&mut self, d: Deliver<'_>, now: Instant, emit: &mut dyn FnMut(Action)) {
        self.delivery.remove(d.msg_id);
        self.delivered.insert(d.msg_id, now);
        emit(Action::Trace(Trace::Delivered { msg_id: d.msg_id, leaf: d.dst_leaf }));
        self.try_deliver(d.dst_leaf, now);
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
            emit(Action::Trace(Trace::Dropped { msg_id: id, reason: DropReason::QueueExpired }));
        }
        if now >= self.delivery_retry_at {
            self.delivery_retry_at = now + self.config.delivery_retry;
            let mut i = 0;
            loop {
                let Some(leaf) = self.delivery.iter().nth(i).map(|q| q.leaf) else { break };
                if !self.delivery.iter().take(i).any(|q| q.leaf == leaf) {
                    self.try_deliver(leaf, now);
                }
                i += 1;
            }
        }
    }

    /// When the leaf service next needs the core.
    pub(super) fn service_wakeup(&self) -> Option<Instant> {
        let retry = (!self.delivery.is_empty()).then_some(self.delivery_retry_at);
        match (self.delivery.next_expiry(), retry) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
}
