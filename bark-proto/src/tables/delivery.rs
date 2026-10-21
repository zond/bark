//! The delivery queue (spec "Delivery queue"): messages that have reached
//! their owner and are for another node, a sleepy one it hears, held until
//! that node wakes instead of failed at once.
//!
//! Messages are kept oldest first. The node hands a sleepy node's messages
//! to the send queue in arrival order while that node listens, after each
//! of its beats (`tables/heard.rs`). One queued the delivery TTL ago or
//! longer (spec "Sleepy nodes": 10 minutes) is stale: it stays, and is
//! still delivered, until a message arrives at a full queue, when the
//! oldest stale one makes way ([`DeliveryQueue::make_room`]).
//!
//! A message waits at most a beat period for its node, unless that node
//! is a phone in a pocket, a basement or the metro, and then for its first
//! beat after it is back, or for the TTL. Nodes in one train or one
//! basement go dark together, so the queue holds what arrives in a TTL for
//! every sleepy node a node can serve, its heard table's [`HEARD`], at the
//! [`TEXTS_PER_DAY`] each that the network carries (spec "Capacity and sizing"):
//! `256 × 23 × 10 min / 1 day = 40.9`, so 41.
//!
//! A message already queued is not queued twice (same `msg_id`): a
//! duplicate that slipped past the per-message records costs nothing here.
//! A message that finds the queue full of messages that are not stale is
//! refused (spec "Receiving DATA"): what is queued stays, and the sender
//! learns at once that this relay cannot take it.

use crate::ids::{MsgId, NodeId};
use crate::node::Config;
use crate::position::Position;
use crate::radio::Channel;
use crate::tables::fixed::FixedVec;
use crate::tables::heard::HEARD;
use crate::time::{Duration, Instant};
use crate::wire::MAX_PAYLOAD;

/// Texts a user receives a day when the network carries all it can
/// (spec "Capacity and sizing": ≈23 at 300 000 users).
pub const TEXTS_PER_DAY: u64 = 23;

/// Queue capacity for `nodes` sleepy nodes and messages that live `ttl`;
/// see the module documentation.
pub const fn queue_for(nodes: usize, ttl: Duration) -> usize {
    const DAY: u64 = 86_400_000_000;
    (nodes as u64 * TEXTS_PER_DAY * ttl.as_micros()).div_ceil(DAY) as usize
}

/// Queue capacity for the heard table and the 10-minute TTL (spec "Delivery queue").
pub const DELIVERY_QUEUE: usize = queue_for(HEARD, Duration::from_secs(600));

const _: () = assert!(DELIVERY_QUEUE == 41);

/// A queued message.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Queued {
    /// The message.
    pub msg_id: MsgId,
    /// Its destination position, which with `node` and the payload makes
    /// its ID.
    pub dst_pos: Position,
    /// The node it is for.
    pub node: NodeId,
    /// The time it had left when it arrived, in DATA's units, which the
    /// DATA to the node carries: here the queue's own TTL ends it, so its
    /// time left stands still (spec "Delivery queue").
    pub time_left: u8,
    /// When it was queued.
    pub enqueued: Instant,
    /// It went once with the long preamble after a far fix (spec
    /// "Delivery queue"): it does not again.
    pub flushed: bool,
    /// The home channel its node's heartbeat named, heard as a node that
    /// is not sleepy since the last hand-over of its messages to it went
    /// unanswered: it goes to it there directly (spec "Delivery queue").
    pub relay_home: Option<Channel>,
    len: u8,
    payload: [u8; MAX_PAYLOAD],
}

impl Queued {
    /// A message, or `None` if `payload` exceeds [`MAX_PAYLOAD`].
    pub fn new(
        msg_id: MsgId,
        dst_pos: Position,
        node: NodeId,
        payload: &[u8],
        time_left: u8,
        now: Instant,
    ) -> Option<Queued> {
        if payload.len() > MAX_PAYLOAD {
            return None;
        }
        let mut q = Queued {
            msg_id,
            dst_pos,
            node,
            time_left,
            enqueued: now,
            flushed: false,
            relay_home: None,
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
            .field("node", &self.node)
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

/// The delivery queue, in arrival order, of up to `N` messages: a
/// sleepy node keeps none (spec "Sleepy nodes").
///
/// Its few fields first, then the items: an event reads the first and
/// seldom the items ([`crate::Node`]'s layout).
#[derive(Clone)]
#[repr(C)]
pub struct DeliveryQueue<const N: usize = DELIVERY_QUEUE> {
    ttl: Duration,
    entries: FixedVec<Queued, N>,
}

impl<const N: usize> DeliveryQueue<N> {
    /// An empty queue whose messages live `ttl`.
    pub const fn new(ttl: Duration) -> DeliveryQueue<N> {
        let fill = Queued {
            msg_id: MsgId(0),
            dst_pos: Position { lat: 0, lon: 0 },
            time_left: 0,
            node: NodeId::NONE,
            enqueued: Instant::ZERO,
            flushed: false,
            relay_home: None,
            len: 0,
            payload: [0; MAX_PAYLOAD],
        };
        DeliveryQueue {
            entries: FixedVec::new(fill),
            ttl,
        }
    }

    /// An empty queue with `config.delivery_ttl`.
    pub const fn from_config(config: &Config) -> DeliveryQueue<N> {
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

    /// Number of messages queued for `node`.
    pub fn count_for(&self, node: NodeId) -> usize {
        self.iter().filter(|q| q.node == node).count()
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

    /// Queued messages, oldest first, to change their marks.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Queued> {
        self.entries.as_mut_slice().iter_mut()
    }

    /// Empty the queue.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Take the oldest message for `node`.
    pub fn pop_for(&mut self, node: NodeId) -> Option<Queued> {
        let i = self.iter().position(|q| q.node == node)?;
        Some(self.entries.remove(i))
    }

    /// Take the message `msg_id`, e.g. once its delivery is acknowledged.
    pub fn remove(&mut self, msg_id: MsgId) -> Option<Queued> {
        let i = self.iter().position(|q| q.msg_id == msg_id)?;
        Some(self.entries.remove(i))
    }

    /// Whether a message arriving at `now` finds a place: a free one, or
    /// one a stale message, queued a TTL ago or longer, would make.
    pub fn has_room(&self, now: Instant) -> bool {
        !self.is_full() || self.iter().next().is_some_and(|q| self.stale(q, now))
    }

    /// Make a place for a message arriving at `now` in a full queue: take
    /// the oldest message if it is stale.
    pub fn make_room(&mut self, now: Instant) -> Option<Queued> {
        let oldest = self.iter().next()?;
        if self.is_full() && self.stale(oldest, now) {
            Some(self.entries.remove(0))
        } else {
            None
        }
    }

    /// When the oldest message turns stale, if any.
    pub fn stale_from(&self) -> Option<Instant> {
        self.iter().next().map(|q| q.enqueued + self.ttl)
    }

    fn stale(&self, q: &Queued, now: Instant) -> bool {
        now.since(q.enqueued) >= self.ttl
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: u64) -> Instant {
        Instant::from_micros(s * 1_000_000)
    }

    fn leaf(i: u8) -> NodeId {
        NodeId([i; 4])
    }

    fn msg(id: u32, to: u8, t: u64) -> Queued {
        Queued::new(
            MsgId(id),
            Position::default(),
            leaf(to),
            &id.to_be_bytes(),
            0,
            at(t),
        )
        .unwrap()
    }

    #[test]
    fn payload_bound() {
        let p = Position::default();
        assert!(Queued::new(MsgId(1), p, leaf(1), &[0; MAX_PAYLOAD], 0, at(0)).is_some());
        assert!(Queued::new(MsgId(1), p, leaf(1), &[0; MAX_PAYLOAD + 1], 0, at(0)).is_none());
        assert_eq!(msg(7, 1, 0).payload(), &[0, 0, 0, 7]);
    }

    #[test]
    fn fifo_per_node_and_dedup() {
        let mut q = DeliveryQueue::<DELIVERY_QUEUE>::from_config(&Config::default());
        assert_eq!(q.push(msg(1, 1, 0)), Pushed::Queued);
        assert_eq!(q.push(msg(2, 2, 1)), Pushed::Queued);
        assert_eq!(q.push(msg(3, 1, 2)), Pushed::Queued);
        assert_eq!(q.push(msg(3, 1, 3)), Pushed::Duplicate);
        assert_eq!(q.count_for(leaf(1)), 2);
        assert_eq!(q.pop_for(leaf(1)).unwrap().msg_id, MsgId(1));
        assert_eq!(q.pop_for(leaf(1)).unwrap().msg_id, MsgId(3));
        assert_eq!(q.pop_for(leaf(1)), None);
        assert_eq!(q.remove(MsgId(2)).unwrap().node, leaf(2));
        assert!(q.is_empty());
    }

    #[test]
    fn a_stale_message_stays_until_its_place_is_needed() {
        let mut q = DeliveryQueue::<4>::from_config(&Config::default());
        for i in 0..4 {
            q.push(msg(i, i as u8, 100 * i as u64));
        }
        // Not full: nothing goes, stale or not.
        q.remove(MsgId(3));
        assert!(q.has_room(at(10_000)));
        assert_eq!(q.make_room(at(10_000)), None);
        q.push(msg(3, 3, 300));
        // Full, the oldest not yet stale: no room.
        assert!(!q.has_room(at(599)));
        assert_eq!(q.make_room(at(599)), None);
        // The oldest stale: it makes way, and only it.
        assert!(q.has_room(at(600)));
        assert_eq!(q.make_room(at(800)).unwrap().msg_id, MsgId(0));
        assert_eq!(q.make_room(at(800)), None);
        assert_eq!(q.len(), 3);
    }

    #[test]
    fn full_queue_refuses() {
        let mut q = DeliveryQueue::<DELIVERY_QUEUE>::from_config(&Config::default());
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
    fn size_follows_the_heard_table_and_ttl() {
        let c = Config::default();
        assert_eq!(queue_for(HEARD, c.delivery_ttl), DELIVERY_QUEUE);
        // The delivered window's 64 is the same count for 400 leaves.
        assert_eq!(queue_for(400, c.delivery_ttl), 64);
    }
}
