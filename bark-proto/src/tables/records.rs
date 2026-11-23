//! Per-message records (spec "Message records"): what a relay remembers
//! about each message it has received or originated, so that it can run
//! its part of the depth-first search and recognise a repeat. The table's
//! size is derived in spec "Bounds".
//!
//! The tried set is kept as a mask over neighbour-table slots
//! ([`Record::handed`]); when a slot changes hands its bit is cleared in
//! every record ([`Records::slot_reoccupied`]), so a newcomer is never
//! taken for a neighbour that was handed the message.

use crate::ids::MsgId;
use crate::ids::NodeId;
use crate::radio::{time_on_air, Channel, MIN_PREAMBLE_SYMBOLS};
use crate::tables::budget::{most_on_air, BAN};
use crate::tables::neighbours::NEIGHBOURS;
use crate::tables::send_queue::{HOP_SLOTS, SEND_QUEUE};
use crate::time::{Duration, Instant};
pub use crate::wire::{Hops, MAX_HOPS};
use crate::wire::{ACK_LEN, DATA_HEADER_LEN};

/// How long a full send queue takes to drain with each entry waiting out
/// one ban: 103 × 1.58 s = 162.9 s (spec "Bounds").
pub const DRAIN: Duration = BAN.mul((SEND_QUEUE + HOP_SLOTS) as u64);

/// The most messages a relay holds at once: one per place in its send
/// queue, which a message set aside for the delivery queue keeps.
pub const HELD: usize = SEND_QUEUE + HOP_SLOTS;

/// The most messages a relay can touch in `span`: its most airtime over
/// an ACK and an empty DATA each, ⌊22.02 s / 40.3 ms⌋ = 546 for
/// [`DRAIN`] (spec "Bounds").
pub const fn records_for(span: Duration) -> usize {
    let airtime = most_on_air(span).as_micros();
    let per_message = time_on_air(ACK_LEN as u8, MIN_PREAMBLE_SYMBOLS).as_micros()
        + time_on_air(DATA_HEADER_LEN as u8, MIN_PREAMBLE_SYMBOLS).as_micros();
    (airtime / per_message) as usize
}

/// Record bound: what a relay touches in a drain, and the records of held
/// messages, which are never evicted, on top (spec "Bounds").
pub const RECORDS: usize = records_for(DRAIN) + HELD;

const _: () = assert!(RECORDS == 649);
const _: () = assert!(RECORDS <= u16::MAX as usize, "a hint holds a place");
const _: () = assert!(RECORDS > HELD, "a new record always finds a place");
const _: () = assert!(
    NEIGHBOURS <= u64::BITS as usize,
    "the tried mask has a bit per slot"
);

const ARRIVED: u8 = 1;
const HAS_PARENT: u8 = 2;
// The last packet was answered seen, not taken (spec "Receiving DATA").
const LAST_SEEN: u8 = 4;
// This node's application sent it; the parent's first byte is its tag.
const OWN: u8 = 8;
// It came with count 0, a sleepy node's own (spec "Hop count").
const FROM_SLEEPY: u8 = 16;

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
    /// With `last_hops`, the last packet received (spec "Receiving DATA").
    last_from: NodeId,
    /// The parent's home channel when the message arrived, used only if
    /// the parent has left the table (spec "Return to parent").
    parent_home: Channel,
    /// The largest wrong-hop count received or sent (spec "Hop count").
    highest_wrong: u8,
    flags: u8,
    highest_hops: Hops,
    last_hops: Hops,
    handed: u64,
    last_active: Instant,
}

impl Record {
    /// A record for a message first received at `now` with `hops` and
    /// `wrong`, from `parent` (ID and home channel), or `None` for one
    /// whose search starts here without an application to go back to
    /// (spec "Sleepy nodes").
    pub fn new(
        id: MsgId,
        parent: Option<(NodeId, Channel)>,
        (hops, wrong): (Hops, u8),
        now: Instant,
    ) -> Record {
        let (name, home, flags) = match parent {
            Some((name, home)) if hops == 0 => (name, home, HAS_PARENT | FROM_SLEEPY),
            Some((name, home)) => (name, home, HAS_PARENT),
            None => (NodeId::NONE, NO_HOME, 0),
        };
        Record {
            id,
            parent: name,
            last_from: name,
            parent_home: home,
            highest_wrong: wrong,
            highest_hops: hops,
            last_hops: hops,
            flags,
            handed: 0,
            last_active: now,
        }
    }

    /// A record for this node's application's message, sent at `now` with
    /// `tag` ([`crate::AppCommand::Send`]): this node is its origin, and the
    /// tag stands in place of the parent (spec "Message records").
    pub fn own(id: MsgId, tag: u8, now: Instant) -> Record {
        let mut name = NodeId::NONE;
        name.0[0] = tag;
        Record {
            parent: name,
            last_from: NodeId::NONE,
            flags: OWN,
            ..Record::new(id, None, (0, 0), now)
        }
    }

    /// The message.
    pub fn id(&self) -> MsgId {
        self.id
    }

    /// The relay the message first came from and its home channel when
    /// the message arrived; `None` where its search began.
    pub fn parent(&self) -> Option<(NodeId, Channel)> {
        (self.flags & HAS_PARENT != 0).then_some((self.parent, self.parent_home))
    }

    /// At the origin, the tag its application sent it with: `None` where
    /// it has a parent, or no application to go back to.
    pub fn tag(&self) -> Option<u8> {
        (self.flags & OWN != 0).then_some(self.parent.0[0])
    }

    /// Whether its parent handed it over with count 0, as a sleepy node
    /// hands its own message to its anchor (spec "Hop count").
    pub fn from_sleepy(&self) -> bool {
        self.flags & FROM_SLEEPY != 0
    }

    /// The largest count received for the message, or carried by a
    /// hand-over of it from this relay that succeeded (spec "Hop count").
    pub fn highest_hops(&self) -> Hops {
        self.highest_hops
    }

    /// The largest wrong-hop count received for the message, or carried by
    /// a hand-over of it from this relay that was taken or seen.
    pub fn highest_wrong(&self) -> u8 {
        self.highest_wrong
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
    /// (spec "Receiving DATA", "Hop count").
    pub fn repeats(&self, from: NodeId, hops: Hops) -> bool {
        (from, hops) == (self.last_from, self.last_hops)
    }

    /// A packet of the message from `from` with `hops` and `wrong` was
    /// received at `now`, and taken.
    pub fn received(&mut self, from: NodeId, (hops, wrong): (Hops, u8), now: Instant) {
        self.flags &= !LAST_SEEN;
        self.last_active = now;
        self.last_from = from;
        self.last_hops = hops;
        self.highest_hops = self.highest_hops.max(hops);
        self.highest_wrong = self.highest_wrong.max(wrong);
    }

    /// A packet of the message from `from` with `hops` and `wrong` was
    /// received at `now` and answered seen (spec "Receiving DATA" case 6).
    pub fn seen(&mut self, from: NodeId, count: (Hops, u8), now: Instant) {
        self.received(from, count, now);
        self.flags |= LAST_SEEN;
    }

    /// Whether the last packet was answered seen.
    pub fn last_seen(&self) -> bool {
        self.flags & LAST_SEEN != 0
    }

    /// A hand-over of the message with counts `hops` and `wrong`
    /// succeeded or was answered seen (spec "Hop count").
    pub fn handed_over(&mut self, (hops, wrong): (Hops, u8)) {
        self.highest_hops = self.highest_hops.max(hops);
        self.highest_wrong = self.highest_wrong.max(wrong);
    }

    /// The neighbour in neighbour-table `slot` has had the message from
    /// this relay or offered it one (spec "Message records",
    /// "Receiving DATA" case 5).
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
            highest_wrong: 0,
            flags: 0,
            highest_hops: 0,
            last_hops: 0,
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

    /// The record for `id` and where it is, looked for at `hint` first:
    /// where it was last found, which saves the search while it has not
    /// moved.
    pub fn find(&self, id: MsgId, hint: u16) -> Option<(u16, &Record)> {
        let at = hint as usize;
        if at < self.len && self.entries[at].id == id {
            return Some((hint, &self.entries[at]));
        }
        let i = self.entries[..self.len].iter().position(|r| r.id == id)?;
        Some((i as u16, &self.entries[i]))
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

/// Bytes of a record kept across a reset ([`Records::kept`]).
pub const KEPT_LEN: usize = 37;

impl Records {
    /// Every record active within `ttl` of `now`, as kept across a reset,
    /// but those `left_out`, of a message offered again as new after it
    /// (spec "Updates over the mesh"). After the delivery TTL no copy of a
    /// message is left to come back, so an older record serves nothing.
    pub fn kept<'a>(
        &'a self,
        ttl: Duration,
        left_out: impl Fn(MsgId) -> bool + 'a,
        now: Instant,
    ) -> impl Iterator<Item = [u8; KEPT_LEN]> + 'a {
        use super::kept::{put, Round};
        self.entries[..self.len]
            .iter()
            .filter(move |r| now.since(r.last_active) < ttl && !left_out(r.id))
            .map(move |r| {
                let mut b = [0u8; KEPT_LEN];
                b[0..4].copy_from_slice(&r.id.0.to_be_bytes());
                b[4..10].copy_from_slice(&r.parent.0);
                b[10..16].copy_from_slice(&r.last_from.0);
                b[16] = r.parent_home.index();
                b[17..20].copy_from_slice(&r.highest_hops.to_be_bytes()[1..]);
                b[20..23].copy_from_slice(&r.last_hops.to_be_bytes()[1..]);
                b[23] = r.highest_wrong;
                b[24] = r.flags;
                b[25..33].copy_from_slice(&r.handed.to_be_bytes());
                put(&mut b[33..37], r.last_active, now, Round::Earlier);
                b
            })
    }

    /// Put back a record [`Records::kept`] gave, taken at `then`: whether
    /// it was, not if the message has a record, the table is full, or the
    /// message was last active `ttl` or more before `now` or before the
    /// clock's epoch. Its tried set names the slots the neighbour table
    /// keeps across the reset.
    pub fn restore(
        &mut self,
        b: &[u8; KEPT_LEN],
        then: super::kept::Then,
        ttl: Duration,
        now: Instant,
    ) -> bool {
        let Some(last_active) = super::kept::get(&b[33..37], then) else {
            return false;
        };
        let (Ok(parent), Ok(last_from), Ok(handed)) = (
            b[4..10].try_into(),
            b[10..16].try_into(),
            b[25..33].try_into(),
        ) else {
            return false;
        };
        let Some(parent_home) = Channel::new(b[16]) else {
            return false;
        };
        let r = Record {
            id: MsgId(u32::from_be_bytes([b[0], b[1], b[2], b[3]])),
            parent: NodeId(parent),
            last_from: NodeId(last_from),
            parent_home,
            highest_wrong: b[23],
            flags: b[24],
            highest_hops: Hops::from_be_bytes([0, b[17], b[18], b[19]]).min(MAX_HOPS),
            last_hops: Hops::from_be_bytes([0, b[20], b[21], b[22]]).min(MAX_HOPS),
            handed: u64::from_be_bytes(handed),
            last_active,
        };
        if now.since(last_active) >= ttl || self.is_full() || self.get(r.id).is_some() {
            return false;
        }
        self.put(self.len, r);
        true
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
        assert_eq!(DRAIN, BAN.mul(103));
        assert_eq!(DRAIN.as_millis(), 162_900);
        assert_eq!(HELD, 103);
        assert_eq!(RECORDS, 546 + 103);
        assert_eq!(core::mem::size_of::<Record>(), 48);
    }

    #[test]
    fn record_fields() {
        let p = NodeId([0, 0, 0, 0, 0xab, 0xcd]);
        let mut r = Record::new(MsgId(7), Some((p, ch(4))), (3, 1), at(0));
        assert_eq!(r.parent(), Some((p, ch(4))));
        assert!(!r.arrived());
        assert!(r.repeats(p, 3), "the packet that created it, again");
        assert!(!r.repeats(p, 2) && !r.repeats(NodeId([5; 6]), 3));
        r.received(NodeId([5; 6]), (2, 0), at(1));
        assert!(
            r.repeats(NodeId([5; 6]), 2) && !r.repeats(p, 3),
            "the last only"
        );
        assert_eq!((r.highest_hops(), r.last_active()), (3, at(1)));
        assert_eq!(r.highest_wrong(), 1);
        r.received(NodeId([5; 6]), (9, 4), at(2));
        assert_eq!((r.highest_hops(), r.highest_wrong()), (9, 4));
        assert!(!r.last_seen());
        r.seen(NodeId([6; 6]), (4, 2), at(3));
        assert!(r.repeats(NodeId([6; 6]), 4) && r.last_seen());
        assert_eq!((r.highest_hops(), r.last_active()), (9, at(3)));
        r.received(NodeId([6; 6]), (11, 4), at(4));
        assert!(!r.last_seen(), "taken: the answer changes with the packet");
        r.handed_over((300, 5));
        r.handed_over((10, 3));
        assert_eq!(r.highest_hops(), 300, "what it sent counts too");
        assert_eq!(r.highest_wrong(), 5);
        r.handed_to(63);
        assert!(r.handed(63) && !r.handed(0) && !r.handed(64));
        r.handed_to(0);
        assert!(r.handed(63) && r.handed(0), "every child stays handed");
        r.sent(at(6));
        assert_eq!(r.last_active(), at(6));
        r.set_arrived();
        assert!(r.arrived());

        let origin = Record::new(MsgId(8), None, (0, 0), at(0));
        assert_eq!((origin.parent(), origin.tag()), (None, None));
        assert!(
            origin.repeats(NodeId::NONE, 0),
            "the origin's packet, again"
        );
        assert!(!r.from_sleepy() && !origin.from_sleepy());

        for tag in [0, 7, 255] {
            let own = Record::own(MsgId(9), tag, at(0));
            assert_eq!((own.parent(), own.tag()), (None, Some(tag)));
            assert!(own.repeats(NodeId::NONE, 0) && !own.arrived());
        }
        let anchor = Record::new(MsgId(10), Some((p, ch(4))), (0, 0), at(0));
        assert!(anchor.from_sleepy() && anchor.tag().is_none());
        assert_eq!(anchor.parent(), Some((p, ch(4))));
    }

    fn keep(t: &mut Records, r: Record) -> Option<Record> {
        let at = t.place(|_| false).unwrap();
        t.put(at, r)
    }

    #[test]
    fn records_have_no_lifetime() {
        let mut t = Records::new();
        keep(&mut t, Record::new(MsgId(1), None, (0, 0), at(0)));
        assert!(t.get(MsgId(1)).is_some());
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn a_full_table_gives_up_the_least_recently_active_record_not_held() {
        let mut t = Records::new();
        for i in 0..RECORDS as u32 {
            let now = at(i as u64);
            assert_eq!(keep(&mut t, Record::new(MsgId(i), None, (0, 0), now)), None);
        }
        assert!(t.is_full());
        let now = at(RECORDS as u64);
        // Touch the oldest; hold the second oldest: the third goes.
        t.get_mut(MsgId(0)).unwrap().sent(now);
        let i = t.place(|id| id == MsgId(1)).unwrap();
        let gone = t
            .put(i, Record::new(MsgId(10_000), None, (0, 0), now))
            .unwrap();
        assert_eq!(gone.id(), MsgId(2));
        assert!(t.get(MsgId(0)).is_some() && t.get(MsgId(1)).is_some());
        assert!(t.get(MsgId(10_000)).is_some() && t.get(MsgId(2)).is_none());
        // Received counts as touched too.
        t.get_mut(MsgId(3))
            .unwrap()
            .received(NodeId([1; 6]), (1, 0), now);
        let i = t.place(|_| false).unwrap();
        assert_eq!(
            t.put(i, Record::new(MsgId(10_001), None, (0, 0), now))
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
            keep(&mut t, Record::new(MsgId(id), None, (0, 0), at(0)));
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
