//! Per-message records (spec "Message records", roadmap D11, D17): what a relay
//! remembers about each message it has received or originated, so that it
//! can run its part of the depth-first search and recognise a repeat.
//!
//! # Eviction
//!
//! A record has no lifetime: it is kept until the table needs its place.
//! A new record then takes the place of the least recently touched one,
//! touched meaning that the message was received or sent here, of those
//! whose message this relay does not hold. A message is held while it is
//! in the send queue, the hand-over included, or set aside for the
//! delivery queue; it needs its record to be routed when its turn comes,
//! so overload never takes it (roadmap D31). The table holds more records
//! than the relay can hold messages ([`HELD`]), so a new record always
//! finds a place.
//!
//! # Size
//!
//! A message sent on can come back to this relay, returned or offered by
//! a split copy, and its record must still be here then. On its way it
//! may wait at each relay as long as a full send queue takes to drain,
//! each entry waiting out one ban (spec "Bans", roadmap D31):
//!
//! ```text
//! (39 + 254) places × 1.57 s = 459.6 s
//! ```
//!
//! The most messages a relay can touch in that time within its duty
//! cycle (spec "Duty cycle cap"): each costs at least an ACK (17 bytes, 15.68 ms) and one
//! DATA packet with no payload (28 bytes, 22.08 ms), together 37.76 ms;
//! any 459.6 s carries at most a full bucket and its refill,
//! 6 s + 9.83 % × 459.6 s = 51.19 s of airtime ([`most_on_air`]), and
//! ⌊51.19 s / 37.76 ms⌋ = 1355. The records of held messages are not
//! evicted and may be older than that, so the table has room for them on
//! top: 1355 + 293 = 1648.
//!
//! # The tried set
//!
//! D17 defined the tried set as the parent, the last child, and every
//! neighbour before the last child in the order of spec "Next hop" (distance to the
//! destination, then ID), which is fixed for a message. The record
//! keeps instead the neighbours that took the message from this relay,
//! each by ACKing a hand-over of it, as a mask over neighbour-table slots
//! ([`Record::handed`]). With the table unchanged and every hand-over
//! answered, the two agree; the last child itself is no longer kept
//! (roadmap D35). They differ for a relay passed over because it was
//! demoted, expired or not in the table, and for one that refused the
//! message or never answered (roadmap D36): the order counts it as tried,
//! the mask as untried, so it stays a candidate for later choices. When a
//! slot changes hands its bit is cleared in every record
//! ([`Records::slot_reoccupied`]), so a newcomer is never taken for a
//! neighbour that was handed the message.

use crate::ids::MsgId;
use crate::ids::NodeId;
use crate::radio::{time_on_air, Channel, MIN_PREAMBLE_SYMBOLS};
use crate::tables::budget::{most_on_air, BAN};
use crate::tables::neighbours::NEIGHBOURS;
use crate::tables::send_queue::{HOP_PLACES, SEND_QUEUE};
use crate::time::{Duration, Instant};
use crate::wire::{ACK_LEN, DATA_HEADER_LEN};

/// The hop cap: the largest count the one-byte `hops` field holds (spec "Hop count").
pub const MAX_HOPS: u8 = u8::MAX;

/// How long a full send queue takes to drain with each entry waiting out
/// one ban; see the module documentation.
pub const DRAIN: Duration = BAN.mul((SEND_QUEUE + HOP_PLACES) as u64);

/// The most messages a relay holds at once: one per place in its send
/// queue. A message set aside for the delivery queue keeps its place
/// (`relay/forward.rs`).
pub const HELD: usize = SEND_QUEUE + HOP_PLACES;

/// The most messages a relay can touch in `span`; see the module
/// documentation.
pub const fn records_for(span: Duration) -> usize {
    let airtime = most_on_air(span).as_micros();
    let per_message = time_on_air(ACK_LEN as u8, MIN_PREAMBLE_SYMBOLS).as_micros()
        + time_on_air(DATA_HEADER_LEN as u8, MIN_PREAMBLE_SYMBOLS).as_micros();
    (airtime / per_message) as usize
}

/// Record bound; see the module documentation.
pub const RECORDS: usize = records_for(DRAIN) + HELD;

const _: () = assert!(RECORDS == 1648);
const _: () = assert!(RECORDS > HELD, "a new record always finds a place");
const _: () = assert!(
    NEIGHBOURS <= u64::BITS as usize,
    "the tried mask has a bit per slot"
);

const ARRIVED: u8 = 1;
const HAS_PARENT: u8 = 2;

/// Stands in for the parent's home at the origin, which has no parent.
const NO_HOME: Channel = match Channel::new(0) {
    Some(c) => c,
    None => panic!("the plan has channels"),
};

/// What a relay remembers about one message (spec "Message records").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    id: MsgId,
    parent: NodeId,
    /// The sender of the last packet of the message received here, with
    /// `last_hops`: the same pair again is a retransmission (spec "Receiving DATA").
    last_from: NodeId,
    /// The parent's home channel when the message arrived. A return goes
    /// to the parent's home as the neighbour table has it then, and to
    /// this one only if the parent has left the table (spec "Return to parent").
    parent_home: Channel,
    highest_hops: u8,
    last_hops: u8,
    flags: u8,
    handed: u64,
    last_active: Instant,
}

impl Record {
    /// A record for a message first received at `now` with `hops`, from
    /// `parent` (ID and home channel), or from a sleepy node's own message
    /// or this node's application (`None`): this node is the origin.
    pub fn new(id: MsgId, parent: Option<(NodeId, Channel)>, hops: u8, now: Instant) -> Record {
        let (name, home, flags) = match parent {
            Some((name, home)) => (name, home, HAS_PARENT),
            None => (NodeId::NONE, NO_HOME, 0),
        };
        Record {
            id,
            parent: name,
            last_from: name,
            parent_home: home,
            highest_hops: hops,
            last_hops: hops,
            flags,
            handed: 0,
            last_active: now,
        }
    }

    /// The message.
    pub fn id(&self) -> MsgId {
        self.id
    }

    /// The relay the message first came from and its home channel when
    /// the message arrived; `None` at the origin.
    pub fn parent(&self) -> Option<(NodeId, Channel)> {
        (self.flags & HAS_PARENT != 0).then_some((self.parent, self.parent_home))
    }

    /// The largest count received for the message, or carried by a
    /// hand-over of it from this relay that succeeded (spec "Hop count").
    pub fn highest_hops(&self) -> u8 {
        self.highest_hops
    }

    /// Whether the message has arrived: this relay owns the destination
    /// (spec "Owner"), or the relay it is addressed to took it from this one.
    pub fn arrived(&self) -> bool {
        self.flags & ARRIVED != 0
    }

    /// When the message was last received or sent here.
    pub fn last_active(&self) -> Instant {
        self.last_active
    }

    /// Whether the neighbour now in `slot` took the message from this
    /// relay.
    pub fn handed(&self, slot: usize) -> bool {
        slot < NEIGHBOURS && self.handed & (1 << slot) != 0
    }

    /// Mark the message as arrived.
    pub fn set_arrived(&mut self) {
        self.flags |= ARRIVED;
    }

    /// Whether a packet of the message from `from` with `hops` repeats
    /// the last one received: a retransmission whose ACK was lost
    /// (spec "Receiving DATA"). A sender sends a count only once but in retries
    /// (spec "Hop count"), so any other packet is another copy.
    pub fn repeats(&self, from: NodeId, hops: u8) -> bool {
        (from, hops) == (self.last_from, self.last_hops)
    }

    /// A packet of the message from `from` with `hops` was received at
    /// `now`.
    pub fn received(&mut self, from: NodeId, hops: u8, now: Instant) {
        self.last_active = now;
        self.last_from = from;
        self.last_hops = hops;
        self.highest_hops = self.highest_hops.max(hops);
    }

    /// A hand-over of the message with count `hops` succeeded. The next
    /// one starts above it, so this relay never sends the same count twice
    /// but as a retry (spec "Hop count").
    pub fn handed_over(&mut self, hops: u8) {
        self.highest_hops = self.highest_hops.max(hops);
    }

    /// The neighbour in neighbour-table `slot` acknowledged the message:
    /// it has it, and is tried.
    pub fn handed_to(&mut self, slot: usize) {
        if slot < NEIGHBOURS {
            self.handed |= 1 << slot;
        }
    }

    /// The message was sent at `now`.
    pub fn sent(&mut self, now: Instant) {
        self.last_active = now;
    }
}

/// The records of one relay.
///
/// Its few fields first, then the items: an event reads the first and
/// seldom the items ([`crate::Node`]'s layout).
#[derive(Clone)]
#[repr(C)]
pub struct Records {
    len: usize,
    entries: [Record; RECORDS],
}

impl Records {
    /// No records.
    pub const fn new() -> Records {
        let fill = Record {
            id: MsgId(0),
            parent: NodeId::NONE,
            last_from: NodeId::NONE,
            parent_home: NO_HOME,
            highest_hops: 0,
            last_hops: 0,
            flags: 0,
            handed: 0,
            last_active: Instant::ZERO,
        };
        Records {
            entries: [fill; RECORDS],
            len: 0,
        }
    }

    /// Number of records.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// No records again, in place.
    pub fn clear(&mut self) {
        self.len = 0;
    }

    /// When the least recently active record was last active, once the
    /// table is full: about how long a record lasts before it makes way.
    pub fn least_recent(&self) -> Option<Instant> {
        if !self.is_full() {
            return None;
        }
        self.entries.iter().map(|r| r.last_active).min()
    }

    /// Whether a new record must take an old one's place.
    pub fn is_full(&self) -> bool {
        self.len == RECORDS
    }

    /// The record for `id`.
    pub fn get(&self, id: MsgId) -> Option<&Record> {
        self.entries[..self.len].iter().find(|r| r.id == id)
    }

    /// Forget the record for `id`, if there is one: its message was given
    /// back to its application and may be offered again as new.
    pub fn forget(&mut self, id: MsgId) {
        if let Some(i) = self.entries[..self.len].iter().position(|r| r.id == id) {
            self.len -= 1;
            self.entries.swap(i, self.len);
        }
    }

    /// The record for `id`, to update.
    pub fn get_mut(&mut self, id: MsgId) -> Option<&mut Record> {
        self.entries[..self.len].iter_mut().find(|r| r.id == id)
    }

    /// Where a new record goes: free space, else the place of the least
    /// recently active record whose message is not `held`; `None` if
    /// every one is.
    pub fn place(&self, held: impl Fn(MsgId) -> bool) -> Option<usize> {
        if !self.is_full() {
            return Some(self.len);
        }
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, r)| !held(r.id))
            .min_by_key(|(_, r)| r.last_active)
            .map(|(i, _)| i)
    }

    /// Keep `record`, which must be for a message without one, at `at`
    /// from [`Records::place`]; returns the record it replaces.
    pub fn put(&mut self, at: usize, record: Record) -> Option<Record> {
        debug_assert!(self.get(record.id).is_none(), "one record per message");
        debug_assert!(at <= self.len && at < RECORDS);
        if at == self.len {
            self.entries[at] = record;
            self.len += 1;
            return None;
        }
        Some(core::mem::replace(&mut self.entries[at], record))
    }

    /// Neighbour-table `slot` has a new occupant, which no record has
    /// handed a message to.
    pub fn slot_reoccupied(&mut self, slot: usize) {
        if slot < NEIGHBOURS {
            for r in &mut self.entries[..self.len] {
                r.handed &= !(1 << slot);
            }
        }
    }
}

impl Default for Records {
    fn default() -> Self {
        Records::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(ms: u64) -> Instant {
        Instant::from_micros(ms * 1000)
    }

    fn ch(i: u8) -> Channel {
        Channel::new(i).unwrap()
    }

    #[test]
    fn constants_follow_the_design() {
        // 293 places × one ban, 1.57 s.
        assert_eq!(DRAIN, BAN.mul(293));
        assert_eq!(DRAIN.as_millis(), 459_583);
        assert_eq!(HELD, 293);
        assert_eq!(RECORDS, 1355 + 293);
        assert_eq!(core::mem::size_of::<Record>(), 40);
    }

    #[test]
    fn record_fields() {
        let p = NodeId([0, 0, 0, 0, 0xab, 0xcd]);
        let mut r = Record::new(MsgId(7), Some((p, ch(4))), 3, at(0));
        assert_eq!(r.parent(), Some((p, ch(4))));
        assert!(!r.arrived());
        assert!(r.repeats(p, 3), "the packet that created it, again");
        assert!(!r.repeats(p, 2) && !r.repeats(NodeId([5; 6]), 3));
        r.received(NodeId([5; 6]), 2, at(1));
        assert!(
            r.repeats(NodeId([5; 6]), 2) && !r.repeats(p, 3),
            "the last only"
        );
        assert_eq!((r.highest_hops(), r.last_active()), (3, at(1)));
        r.received(NodeId([5; 6]), 9, at(2));
        assert_eq!(r.highest_hops(), 9);
        r.handed_over(12);
        r.handed_over(10);
        assert_eq!(r.highest_hops(), 12, "what it sent counts too");
        r.handed_to(63);
        assert!(r.handed(63) && !r.handed(0) && !r.handed(64));
        r.handed_to(0);
        assert!(r.handed(63) && r.handed(0), "every child stays handed");
        r.sent(at(6));
        assert_eq!(r.last_active(), at(6));
        r.set_arrived();
        assert!(r.arrived());

        let origin = Record::new(MsgId(8), None, 0, at(0));
        assert_eq!(origin.parent(), None);
        assert!(
            origin.repeats(NodeId::NONE, 0),
            "the origin's packet, again"
        );
    }

    fn keep(t: &mut Records, r: Record) -> Option<Record> {
        let at = t.place(|_| false).unwrap();
        t.put(at, r)
    }

    #[test]
    fn records_have_no_lifetime() {
        let mut t = Records::new();
        keep(&mut t, Record::new(MsgId(1), None, 0, at(0)));
        assert!(t.get(MsgId(1)).is_some());
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn a_full_table_gives_up_the_least_recently_active_record_not_held() {
        let mut t = Records::new();
        for i in 0..RECORDS as u32 {
            let now = at(i as u64);
            assert_eq!(keep(&mut t, Record::new(MsgId(i), None, 0, now)), None);
        }
        assert!(t.is_full());
        let now = at(RECORDS as u64);
        // Touch the oldest; hold the second oldest: the third goes.
        t.get_mut(MsgId(0)).unwrap().sent(now);
        let i = t.place(|id| id == MsgId(1)).unwrap();
        let gone = t.put(i, Record::new(MsgId(10_000), None, 0, now)).unwrap();
        assert_eq!(gone.id(), MsgId(2));
        assert!(t.get(MsgId(0)).is_some() && t.get(MsgId(1)).is_some());
        assert!(t.get(MsgId(10_000)).is_some() && t.get(MsgId(2)).is_none());
        // Received counts as touched too.
        t.get_mut(MsgId(3))
            .unwrap()
            .received(NodeId([1; 6]), 1, now);
        let i = t.place(|_| false).unwrap();
        assert_eq!(
            t.put(i, Record::new(MsgId(10_001), None, 0, now))
                .unwrap()
                .id(),
            MsgId(1)
        );
        assert_eq!(t.place(|_| true), None, "every message held");
    }

    #[test]
    fn a_reoccupied_slot_is_not_handed() {
        let mut t = Records::new();
        for id in 0..3 {
            keep(&mut t, Record::new(MsgId(id), None, 0, at(0)));
            let r = t.get_mut(MsgId(id)).unwrap();
            r.handed_to(7);
            r.handed_to(8);
        }
        t.slot_reoccupied(7);
        for id in 0..3 {
            let r = t.get(MsgId(id)).unwrap();
            assert!(!r.handed(7) && r.handed(8));
        }
    }
}
