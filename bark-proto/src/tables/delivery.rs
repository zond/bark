//! The delivery queue (design §10.4): messages for attached leaves that
//! could not be delivered yet, held for a while instead of failed at once.
//!
//! Up to 64 messages, each with a TTL (§10.4: 10 minutes); oldest first.
//! The relay retries delivery on the leaf's next keepalive and every 60 s,
//! taking a leaf's messages in arrival order with [`DeliveryQueue::pop_for`],
//! and drains [`DeliveryQueue::pop_expired`] to fail the expired ones.
//!
//! A message already queued is not queued twice (same `msg_id`): a
//! duplicate that slipped past the per-message records costs nothing here.
//!
//! When the queue is full, the newcomer displaces the oldest message of
//! the leaf with the most queued messages, the newcomer counted (decision
//! 2026-10-14; §10.4 does not say). One absent leaf with a busy
//! conversation then cannot flush everyone else's messages; among leaves
//! with equal shares the oldest message, nearest its expiry, goes. The
//! displaced message is returned so the relay can fail it as it would an
//! expired one.

use crate::ids::{LeafId, MsgId};
use crate::node::Config;
use crate::tables::fixed::FixedVec;
use crate::time::{Duration, Instant};
use crate::wire::MAX_PAYLOAD;

/// Queue capacity (design §10.4).
pub const DELIVERY_QUEUE: usize = 64;

/// A queued message.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Queued {
    /// The message.
    pub msg_id: MsgId,
    /// The leaf it is for.
    pub leaf: LeafId,
    /// When it was queued.
    pub enqueued: Instant,
    len: u8,
    payload: [u8; MAX_PAYLOAD],
}

impl Queued {
    /// A message, or `None` if `payload` exceeds [`MAX_PAYLOAD`].
    pub fn new(msg_id: MsgId, leaf: LeafId, payload: &[u8], now: Instant) -> Option<Queued> {
        if payload.len() > MAX_PAYLOAD {
            return None;
        }
        let mut q = Queued {
            msg_id,
            leaf,
            enqueued: now,
            len: payload.len() as u8,
            payload: [0; MAX_PAYLOAD],
        };
        q.payload[..payload.len()].copy_from_slice(payload);
        Some(q)
    }

    /// The payload.
    pub fn payload(&self) -> &[u8] {
        &self.payload[..self.len as usize]
    }
}

impl core::fmt::Debug for Queued {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Queued")
            .field("msg_id", &self.msg_id)
            .field("leaf", &self.leaf)
            .field("enqueued", &self.enqueued)
            .field("payload", &self.payload())
            .finish()
    }
}

/// What [`DeliveryQueue::push`] did.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pushed {
    /// Queued in free space.
    Queued,
    /// Already queued; nothing changed.
    Duplicate,
    /// Queued, displacing this message.
    Displaced(Queued),
}

/// The delivery queue, in arrival order.
#[derive(Clone)]
pub struct DeliveryQueue {
    entries: FixedVec<Queued, DELIVERY_QUEUE>,
    ttl: Duration,
}

impl DeliveryQueue {
    /// An empty queue whose messages live `ttl`.
    pub const fn new(ttl: Duration) -> DeliveryQueue {
        let fill = Queued {
            msg_id: MsgId(0),
            leaf: LeafId::RELAY,
            enqueued: Instant::ZERO,
            len: 0,
            payload: [0; MAX_PAYLOAD],
        };
        DeliveryQueue {
            entries: FixedVec::new(fill),
            ttl,
        }
    }

    /// An empty queue with `config.delivery_ttl`.
    pub fn from_config(config: &Config) -> DeliveryQueue {
        DeliveryQueue::new(config.delivery_ttl)
    }

    /// Number of queued messages.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Queued messages, oldest first.
    pub fn iter(&self) -> impl Iterator<Item = &Queued> {
        self.entries.as_slice().iter()
    }

    /// Number of messages queued for `leaf`.
    pub fn count_for(&self, leaf: LeafId) -> usize {
        self.iter().filter(|q| q.leaf == leaf).count()
    }

    /// Queue a message; see the module documentation for a full queue.
    pub fn push(&mut self, q: Queued) -> Pushed {
        if self.iter().any(|e| e.msg_id == q.msg_id) {
            return Pushed::Duplicate;
        }
        if self.entries.push(q).is_ok() {
            return Pushed::Queued;
        }
        let share = |leaf: LeafId| self.count_for(leaf) + usize::from(leaf == q.leaf);
        // First in arrival order among the largest shares: that leaf's oldest.
        let mut victim = 0;
        let mut most = 0;
        for (i, e) in self.iter().enumerate() {
            let s = share(e.leaf);
            if s > most {
                most = s;
                victim = i;
            }
        }
        let displaced = self.entries.remove(victim);
        let _ = self.entries.push(q);
        Pushed::Displaced(displaced)
    }

    /// Take the oldest message for `leaf`.
    pub fn pop_for(&mut self, leaf: LeafId) -> Option<Queued> {
        let i = self.iter().position(|q| q.leaf == leaf)?;
        Some(self.entries.remove(i))
    }

    /// Take the message `msg_id`, e.g. once its delivery is acknowledged.
    pub fn remove(&mut self, msg_id: MsgId) -> Option<Queued> {
        let i = self.iter().position(|q| q.msg_id == msg_id)?;
        Some(self.entries.remove(i))
    }

    /// Take the oldest message if it has expired at `now`. Call until `None`.
    pub fn pop_expired(&mut self, now: Instant) -> Option<Queued> {
        let oldest = self.iter().next()?;
        if now.since(oldest.enqueued) >= self.ttl {
            Some(self.entries.remove(0))
        } else {
            None
        }
    }

    /// When the oldest message expires, if any.
    pub fn next_expiry(&self) -> Option<Instant> {
        self.iter().next().map(|q| q.enqueued + self.ttl)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: u64) -> Instant {
        Instant::from_micros(s * 1_000_000)
    }

    fn leaf(i: u8) -> LeafId {
        LeafId([i; 4])
    }

    fn msg(id: u32, to: u8, t: u64) -> Queued {
        Queued::new(MsgId(id), leaf(to), &id.to_be_bytes(), at(t)).unwrap()
    }

    #[test]
    fn payload_bound() {
        assert!(Queued::new(MsgId(1), leaf(1), &[0; MAX_PAYLOAD], at(0)).is_some());
        assert!(Queued::new(MsgId(1), leaf(1), &[0; MAX_PAYLOAD + 1], at(0)).is_none());
        assert_eq!(msg(7, 1, 0).payload(), &[0, 0, 0, 7]);
    }

    #[test]
    fn fifo_per_leaf_and_dedup() {
        let mut q = DeliveryQueue::from_config(&Config::default());
        assert_eq!(q.push(msg(1, 1, 0)), Pushed::Queued);
        assert_eq!(q.push(msg(2, 2, 1)), Pushed::Queued);
        assert_eq!(q.push(msg(3, 1, 2)), Pushed::Queued);
        assert_eq!(q.push(msg(3, 1, 3)), Pushed::Duplicate);
        assert_eq!(q.count_for(leaf(1)), 2);
        assert_eq!(q.pop_for(leaf(1)).unwrap().msg_id, MsgId(1));
        assert_eq!(q.pop_for(leaf(1)).unwrap().msg_id, MsgId(3));
        assert_eq!(q.pop_for(leaf(1)), None);
        assert_eq!(q.remove(MsgId(2)).unwrap().leaf, leaf(2));
        assert!(q.is_empty());
    }

    #[test]
    fn ttl() {
        let mut q = DeliveryQueue::from_config(&Config::default());
        q.push(msg(1, 1, 0));
        q.push(msg(2, 2, 100));
        assert_eq!(q.next_expiry(), Some(at(600)));
        assert_eq!(q.pop_expired(at(599)), None);
        assert_eq!(q.pop_expired(at(700)).unwrap().msg_id, MsgId(1));
        assert_eq!(q.pop_expired(at(700)).unwrap().msg_id, MsgId(2));
        assert_eq!(q.pop_expired(at(700)), None);
        assert_eq!(q.next_expiry(), None);
    }

    #[test]
    fn full_queue_displaces_from_the_largest_share() {
        let mut q = DeliveryQueue::from_config(&Config::default());
        // Leaf 9 holds half the queue, the others one message each.
        for i in 0..DELIVERY_QUEUE as u32 {
            let to = if i % 2 == 1 { 9 } else { i as u8 + 100 };
            assert_eq!(q.push(msg(i, to, i as u64)), Pushed::Queued);
        }
        // Leaf 9's oldest goes, not the oldest message overall (0, for leaf 100).
        assert_eq!(q.push(msg(100, 1, 100)), Pushed::Displaced(msg(1, 9, 1)));
        // Equal shares: the oldest message among them goes.
        let mut q = DeliveryQueue::from_config(&Config::default());
        for i in 0..DELIVERY_QUEUE as u32 {
            q.push(msg(i, i as u8, i as u64));
        }
        assert_eq!(q.push(msg(100, 200, 100)), Pushed::Displaced(msg(0, 0, 0)));
        // The newcomer counts: leaf 5 now has two with it, so its own oldest goes.
        assert_eq!(q.push(msg(101, 5, 101)), Pushed::Displaced(msg(5, 5, 5)));
        assert_eq!(q.len(), DELIVERY_QUEUE);
        assert_eq!(q.iter().last().unwrap().msg_id, MsgId(101));
    }
}
