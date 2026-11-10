//! The drain before a reboot into a new image (spec "Send queue",
//! "Updates over the mesh", the install): the node takes no message, so
//! that what it holds goes on to its neighbours and its sleepy nodes, and
//! the driver reboots once it holds nothing or the bound has passed. What
//! it still holds then, the driver keeps across the reboot and offers
//! again as the node's own ([`Node::held`]).

use super::Node;
use crate::ids::{MsgId, NodeId};
use crate::position::Position;
use crate::time::Instant;
use crate::wire::Packet;

/// A message the node holds, as a driver keeps it across a reboot: what
/// [`MsgId::of`] is taken of, so that offered again it is the same
/// message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Held<'a> {
    /// Its ID.
    pub msg_id: MsgId,
    /// Its destination's position.
    pub dst_pos: Position,
    /// Its destination node.
    pub dst_id: NodeId,
    /// Its payload.
    pub payload: &'a [u8],
}

impl<const H: usize, const Q: usize> Node<H, Q> {
    /// `AppCommand::Drain`: take no message from now, or take them again.
    pub(super) fn drain(&mut self, on: bool, now: Instant) {
        let s = &mut self.forwarding.drain_from_s;
        *s = match on {
            // Rounded up, so that the bound is never shorter.
            true => (*s).min(now.as_micros().div_ceil(1_000_000) as u32),
            false => u32::MAX,
        };
    }

    /// Whether it drains for a reboot: it takes no message.
    pub fn draining(&self) -> bool {
        self.forwarding.drain_from_s != u32::MAX
    }

    /// Whether the node, draining, may reboot at `now`: it holds nothing
    /// a reset would lose, in its send queue, its delivery queue, its
    /// inbox or a reply that takes a message, or it has drained for the
    /// delivery TTL from the whole second it began, the wait the delivery
    /// queue gives a node that sleeps or is dark, by when every message
    /// in it has waited that long for its node (spec "Delivery queue")
    /// and a forward has had twenty hops' budgets (spec "Send queue").
    pub fn drained(&self, now: Instant) -> bool {
        if !self.draining() {
            return false;
        }
        let since = Instant::from_micros(u64::from(self.forwarding.drain_from_s) * 1_000_000);
        let empty = self.holding().next().is_none()
            && self.inbox.is_empty()
            && !self.radio.replies.taking();
        empty || now.since(since) >= self.config.delivery_ttl
    }

    /// Every message the node holds, each once: in its send queue, its
    /// delivery queue and its inbox. A message on its way to a sleepy node
    /// is the delivery queue's.
    pub fn held(&self) -> impl Iterator<Item = Held<'_>> + '_ {
        let queued = self
            .queue
            .iter()
            .filter(|o| !o.kind.delivers())
            .filter_map(|o| match Packet::decode(o.frame.payload()) {
                Ok(Packet::Data(d)) => Some(Held {
                    msg_id: d.msg_id(),
                    dst_pos: d.dst_pos,
                    dst_id: d.dst_id,
                    payload: d.payload,
                }),
                _ => None,
            });
        let waiting = self.delivery.iter().chain(self.inbox.iter()).map(|q| Held {
            msg_id: q.msg_id,
            dst_pos: q.dst_pos,
            dst_id: q.node,
            payload: q.payload(),
        });
        queued.chain(waiting)
    }
}
