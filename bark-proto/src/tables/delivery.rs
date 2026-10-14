//! The delivery queue (design §10.4): messages for attached leaves that
//! could not be delivered yet, held for a while instead of failed at once.
//!
//! Each message lives the delivery TTL (§10.4: 10 minutes); oldest first.
//! The relay hands a leaf's messages to the send queue in arrival order
//! with [`DeliveryQueue::pop_for`], and drains
//! [`DeliveryQueue::pop_expired`] to fail the expired ones.
//!
//! A message stays long only for a leaf that does not answer. One that
//! listens, always or with a check of the channel every 500 ms, takes its
//! DELIVER at the first hand-over, whose 512 ms preamble spans the check
//! (§10.5). One that does not is a phone in a pocket, a basement or the
//! metro, and its messages wait for its first keepalive after it is back,
//! at most 60 s later (§10.2), or for the TTL. Leaves in one train or one
//! basement go dark together, so the queue holds what arrives in a TTL for
//! every leaf the relay can serve, its leaf table's [`LEAVES`] (§10.2),
//! at the [`TEXTS_PER_DAY`] each that the network carries (§13):
//! `256 × 23 × 10 min / 1 day = 40.9`, so 41.
//!
//! A message already queued is not queued twice (same `msg_id`): a
//! duplicate that slipped past the per-message records costs nothing here.
//! A message that finds the queue full is refused (§9.2.4): what is queued
//! stays, and the sender learns at once that this relay cannot take it.

use crate::ids::{LeafId, MsgId};
use crate::node::Config;
use crate::tables::fixed::FixedVec;
use crate::tables::leaves::LEAVES;
use crate::time::{Duration, Instant};
use crate::wire::MAX_PAYLOAD;

/// Texts a user receives a day when the network carries all it can
/// (design §13: ≈23 at 300 000 users).
pub const TEXTS_PER_DAY: u64 = 23;

/// Queue capacity for `leaves` leaves and messages that live `ttl`; see
/// the module documentation.
pub const fn queue_for(leaves: usize, ttl: Duration) -> usize {
    const DAY: u64 = 86_400_000_000;
    (leaves as u64 * TEXTS_PER_DAY * ttl.as_micros()).div_ceil(DAY) as usize
}

/// Queue capacity for the design's leaf table and 10-minute TTL (§10.4).
pub const DELIVERY_QUEUE: usize = queue_for(LEAVES, Duration::from_secs(600));

const _: () = assert!(DELIVERY_QUEUE == 41);

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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pushed {
    /// Queued.
    Queued,
    /// Already queued; nothing changed.
    Duplicate,
    /// The queue is full; nothing changed.
    Full,
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

    /// Whether a new message would be refused.
    pub fn is_full(&self) -> bool {
        self.entries.is_full()
    }

    /// Queued messages, oldest first.
    pub fn iter(&self) -> impl Iterator<Item = &Queued> {
        self.entries.as_slice().iter()
    }

    /// Number of messages queued for `leaf`.
    pub fn count_for(&self, leaf: LeafId) -> usize {
        self.iter().filter(|q| q.leaf == leaf).count()
    }

    /// Queue a message, unless it is queued already or the queue is full.
    pub fn push(&mut self, q: Queued) -> Pushed {
        if self.iter().any(|e| e.msg_id == q.msg_id) {
            return Pushed::Duplicate;
        }
        match self.entries.push(q) {
            Ok(()) => Pushed::Queued,
            Err(_) => Pushed::Full,
        }
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
    fn full_queue_refuses() {
        let mut q = DeliveryQueue::from_config(&Config::default());
        for i in 0..DELIVERY_QUEUE as u32 {
            assert_eq!(q.push(msg(i, i as u8, i as u64)), Pushed::Queued);
        }
        assert!(q.is_full());
        assert_eq!(q.push(msg(100, 200, 100)), Pushed::Full);
        assert_eq!(q.push(msg(0, 0, 100)), Pushed::Duplicate);
        assert_eq!(q.len(), DELIVERY_QUEUE);
        assert_eq!(q.iter().next().unwrap().msg_id, MsgId(0));
    }

    #[test]
    fn size_follows_the_leaf_table_and_ttl() {
        let c = Config::default();
        assert_eq!(queue_for(LEAVES, c.delivery_ttl), DELIVERY_QUEUE);
        // The delivered window's 64 is the same count for 400 leaves.
        assert_eq!(queue_for(400, c.delivery_ttl), 64);
    }
}
