//! The delivery queue and the inbox (spec "Delivery queue"): messages
//! that have reached their owner and are for a sleepy node it hears, or
//! wait for a neighbour leaving the air, kept oldest first. A stale
//! message stays until a full queue needs its place
//! ([`DeliveryQueue::make_room`]).
//!
//! A message already queued is not queued twice (same `msg_id`): a
//! duplicate that slipped past the message records costs nothing here.
//! The 6 bytes of [`Queued::waits_for`] fit in what the entry's alignment
//! left free.

use crate::ids::{MsgId, NodeId};
use crate::node::Config;
use crate::position::Position;
use crate::radio::Channel;
use crate::tables::fixed::FixedVec;
use crate::tables::heard::HEARD;
use crate::tables::send_queue::Message;
use crate::time::{Duration, Instant};
use crate::wire::MAX_PAYLOAD;

/// Texts a user receives a day at the design rate, ≈23 at 300 000 users
/// (spec "Capacity and sizing").
pub const TEXTS_PER_DAY: u64 = 23;

/// Queue capacity for `nodes` sleepy nodes and messages that live `ttl`:
/// what arrives for them in a TTL at [`TEXTS_PER_DAY`].
pub const fn queue_for(nodes: usize, ttl: Duration) -> usize {
    const DAY: u64 = 86_400_000_000;
    (nodes as u64 * TEXTS_PER_DAY * ttl.as_micros()).div_ceil(DAY) as usize
}

/// Queue capacity for the heard table and the 10-minute TTL (spec "Delivery queue").
pub const DELIVERY_QUEUE: usize = queue_for(HEARD, Duration::from_secs(600));

const _: () = assert!(DELIVERY_QUEUE == 41);

/// The inbox's capacity, the messages a node keeps for its own
/// application until it confirms each: what one node receives in a day,
/// for a phone that connects once a day (spec "Receiving").
pub const INBOX: usize = queue_for(1, Duration::from_secs(86_400));

const _: () = assert!(INBOX == 23);

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
    /// The neighbour leaving the air it waits for; [`NodeId::NONE`] for a
    /// message to deliver from here.
    pub waits_for: NodeId,
    /// When it was queued.
    pub enqueued: Instant,
    /// It went once with the long preamble after a far fix (spec
    /// "Delivery"): it does not again.
    pub flushed: bool,
    /// The home channel its node named in a heartbeat as a node that is
    /// not sleepy, since a delivery to it went unanswered: it goes there
    /// directly (spec "Delivery").
    pub relay_home: Option<Channel>,
    /// The count it arrived with, which it goes on from if the node turns
    /// sleepy before it is delivered (spec "Hop count", "Sleepy nodes").
    pub hops: u8,
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
        now: Instant,
    ) -> Option<Queued> {
        if payload.len() > MAX_PAYLOAD {
            return None;
        }
        let mut q = Queued {
            msg_id,
            dst_pos,
            node,
            waits_for: NodeId::NONE,
            enqueued: now,
            flushed: false,
            relay_home: None,
            hops: 0,
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

    /// The message.
    pub fn message(&self) -> Message {
        Message::new(self.dst_pos, self.node, self.payload())
    }

    /// Whether it waits for a neighbour leaving the air, not for
    /// delivery from here.
    pub fn waits(&self) -> bool {
        self.waits_for != NodeId::NONE
    }
}

impl core::fmt::Debug for Queued {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Queued")
            .field("msg_id", &self.msg_id)
            .field("node", &self.node)
            .field("waits_for", &self.waits_for)
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
/// sleepy node keeps only what it held when it turned sleepy, until the
/// send queue takes it (spec "Sleepy nodes").
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
    /// An empty queue whose messages turn stale after `ttl`.
    pub const fn new(ttl: Duration) -> DeliveryQueue<N> {
        let fill = Queued {
            msg_id: MsgId(0),
            dst_pos: Position { lat: 0, lon: 0 },
            node: NodeId::NONE,
            waits_for: NodeId::NONE,
            enqueued: Instant::ZERO,
            flushed: false,
            relay_home: None,
            hops: 0,
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

    /// What [`DeliveryQueue::from_config`] builds, made where the queue
    /// lies, with no copy of it on the stack.
    pub fn reset(&mut self, config: &Config) {
        self.ttl = config.delivery_ttl;
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
        NodeId([i; 6])
    }

    fn msg(id: u32, to: u8, t: u64) -> Queued {
        Queued::new(
            MsgId(id),
            Position::default(),
            leaf(to),
            &id.to_be_bytes(),
            at(t),
        )
        .unwrap()
    }

    #[test]
    fn payload_bound() {
        let p = Position::default();
        assert!(Queued::new(MsgId(1), p, leaf(1), &[0; MAX_PAYLOAD], at(0)).is_some());
        assert!(Queued::new(MsgId(1), p, leaf(1), &[0; MAX_PAYLOAD + 1], at(0)).is_none());
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
