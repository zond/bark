//! Per-message records (design §9.2.2, roadmap D11, D17): what a relay
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
//! each entry waiting out one ban (§9.3, roadmap D31):
//!
//! ```text
//! (40 + 254) places × 1.53 s = 449.7 s
//! ```
//!
//! The most messages a relay can touch in that time within its duty
//! cycle (§3): each costs at least an ACK (9 bytes, 11.84 ms) and one DATA
//! packet with no payload (21 bytes, 18.24 ms), together 30.08 ms; 10 % of
//! 449.7 s is 44.97 s of airtime, and ⌊44.97 s / 30.08 ms⌋ = 1494. The
//! records of held messages are not evicted and may be older than that,
//! so the table has room for them on top: 1494 + 294 = 1788.
//!
//! # The tried set
//!
//! D17 defined the tried set as the parent, the last child, and every
//! neighbour before the last child in the order of §9.2.3 (distance to the
//! destination, then short name), which is fixed for a message. The record
//! keeps instead the neighbours this relay actually handed the message to,
//! as a mask over neighbour-table slots ([`Record::handed`]). With the
//! table unchanged and every neighbour before the last child a candidate
//! when passed over, the two agree. They differ for a relay passed over
//! because it was demoted, expired or not in the table: the order counts
//! it as tried, the mask as untried, so it stays a candidate for later
//! choices. When a slot changes hands its bit is cleared in every record
//! ([`Records::slot_reoccupied`]), so a newcomer is never taken for a
//! neighbour that was handed the message.

use crate::ids::MsgId;
use crate::position::ShortName;
use crate::radio::{time_on_air, Channel, DUTY_CYCLE_PERCENT, MIN_PREAMBLE_SYMBOLS};
use crate::tables::budget::BAN;
use crate::tables::neighbours::NEIGHBOURS;
use crate::tables::send_queue::{HOP_PLACES, SEND_QUEUE};
use crate::time::{Duration, Instant};
use crate::wire::{ACK_LEN, DATA_HEADER_LEN};

/// The hop cap: the largest count the one-byte `hops` field holds (§9.2.6).
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
    let airtime = span.as_micros() * DUTY_CYCLE_PERCENT as u64 / 100;
    let per_message = time_on_air(ACK_LEN as u8, MIN_PREAMBLE_SYMBOLS).as_micros()
        + time_on_air(DATA_HEADER_LEN as u8, MIN_PREAMBLE_SYMBOLS).as_micros();
    (airtime / per_message) as usize
}

/// Record bound; see the module documentation.
pub const RECORDS: usize = records_for(DRAIN) + HELD;

const _: () = assert!(RECORDS == 1788);
const _: () = assert!(RECORDS > HELD, "a new record always finds a place");
const _: () = assert!(
    NEIGHBOURS <= u64::BITS as usize,
    "the tried mask has a bit per slot"
);

const OWNED: u8 = 1;
const HAS_CHILD: u8 = 2;
const RETURNED: u8 = 4;

/// What a relay remembers about one message (§9.2.2).
///
/// The last child's distance to the destination, which §9.2.2 also lists,
/// is not stored: the record is consulted only with a packet of the
/// message at hand, and the distance follows from that packet's `dst_pos`
/// and the child's short name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    id: MsgId,
    parent: ShortName,
    last_child: ShortName,
    /// The parent's home channel when the message arrived; `None` at the
    /// origin, which has no parent. A return goes to the parent's home as
    /// the neighbour table has it then, and to this one only if the parent
    /// has left the table (§9.2.5).
    parent_home: Option<Channel>,
    highest_hops: u8,
    flags: u8,
    handed: u64,
    last_active: Instant,
}

impl Record {
    /// A record for a message first received at `now` with `hops`, from
    /// `parent` (short name and home channel), or originated here (`None`).
    pub fn new(id: MsgId, parent: Option<(ShortName, Channel)>, hops: u8, now: Instant) -> Record {
        let (name, home) = match parent {
            Some((name, home)) => (name, Some(home)),
            None => (ShortName(0), None),
        };
        Record {
            id,
            parent: name,
            last_child: ShortName(0),
            parent_home: home,
            highest_hops: hops,
            flags: 0,
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
    pub fn parent(&self) -> Option<(ShortName, Channel)> {
        self.parent_home.map(|home| (self.parent, home))
    }

    /// The neighbour this relay last sent the message to.
    pub fn last_child(&self) -> Option<ShortName> {
        (self.flags & HAS_CHILD != 0).then_some(self.last_child)
    }

    /// The largest `hops` received for the message.
    pub fn highest_hops(&self) -> u8 {
        self.highest_hops
    }

    /// Whether this relay owns the destination (§9.2.7).
    pub fn owned(&self) -> bool {
        self.flags & OWNED != 0
    }

    /// When the message was last received or sent here.
    pub fn last_active(&self) -> Instant {
        self.last_active
    }

    /// Whether the neighbour now in `slot` was handed the message.
    pub fn handed(&self, slot: usize) -> bool {
        slot < NEIGHBOURS && self.handed & (1 << slot) != 0
    }

    /// Whether this relay has given the message back to its parent: its
    /// part of the search is over.
    ///
    /// §9.2.2 does not list this. Without it a finished relay takes a
    /// later offer from its last child for that child's return (§9.2.4
    /// case 4) and returns the message to its parent a second time; the
    /// parent, which has moved on, only ACKs it (case 5), and the message
    /// is lost without a drop. With O, P, Q all neighbours: O → P → Q → O,
    /// O sends it straight back, Q and then P return it, O tries Q, and Q,
    /// whose last child is O, returns it to P again. A finished relay sends
    /// such an offer straight back (case 6), as the search needs: it is a
    /// visit to a relay already searched.
    pub fn returned(&self) -> bool {
        self.flags & RETURNED != 0
    }

    /// Mark the message as given back to the parent.
    pub fn set_returned(&mut self) {
        self.flags |= RETURNED;
    }

    /// Mark this relay as owning the destination.
    pub fn set_owned(&mut self) {
        self.flags |= OWNED;
    }

    /// A packet of the message with `hops` was received at `now`. Returns
    /// whether `hops` is higher than any before; if not, the packet is a
    /// copy (§9.2.4 case 3).
    pub fn received(&mut self, hops: u8, now: Instant) -> bool {
        self.last_active = now;
        let higher = hops > self.highest_hops;
        self.highest_hops = self.highest_hops.max(hops);
        higher
    }

    /// The message was sent at `now` to `child`, in neighbour-table `slot`:
    /// it is now the last child.
    pub fn sent_to_child(&mut self, child: ShortName, slot: usize, now: Instant) {
        self.last_child = child;
        self.flags |= HAS_CHILD;
        if slot < NEIGHBOURS {
            self.handed |= 1 << slot;
        }
        self.last_active = now;
    }

    /// The neighbour in `slot` refused the message it was sent (BUSY,
    /// §9.3): it was not handed it. It stays the last child, so a later
    /// offer from it still counts as its return (§9.2.4 case 4).
    pub fn not_handed(&mut self, slot: usize) {
        if slot < NEIGHBOURS {
            self.handed &= !(1 << slot);
        }
    }

    /// The message was sent at `now` somewhere that does not change the
    /// search: back to the parent, or straight back to a relay that
    /// offered it (§9.2.4 case 6).
    pub fn sent(&mut self, now: Instant) {
        self.last_active = now;
    }
}

/// The records of one relay.
#[derive(Clone)]
pub struct Records {
    entries: [Record; RECORDS],
    len: usize,
}

impl Records {
    /// No records.
    pub const fn new() -> Records {
        let fill = Record {
            id: MsgId(0),
            parent: ShortName(0),
            last_child: ShortName(0),
            parent_home: None,
            highest_hops: 0,
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

    /// Whether a new record must take an old one's place.
    pub fn is_full(&self) -> bool {
        self.len == RECORDS
    }

    /// The record for `id`.
    pub fn get(&self, id: MsgId) -> Option<&Record> {
        self.entries[..self.len].iter().find(|r| r.id == id)
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
        // 294 places × one ban, 1.53 s.
        assert_eq!(DRAIN, BAN.mul(294));
        assert_eq!(DRAIN.as_millis(), 449_670);
        assert_eq!(HELD, 294);
        assert_eq!(RECORDS, 1494 + 294);
        assert_eq!(core::mem::size_of::<Record>(), 32);
    }

    #[test]
    fn record_fields() {
        let mut r = Record::new(MsgId(7), Some((ShortName(0xabcd), ch(4))), 3, at(0));
        assert_eq!(r.parent(), Some((ShortName(0xabcd), ch(4))));
        assert_eq!(r.last_child(), None);
        assert!(!r.owned());
        assert!(!r.received(3, at(1)), "same hops: a copy");
        assert!(!r.received(2, at(2)), "lower hops: a copy");
        assert!(r.received(9, at(3)), "higher hops: news");
        assert_eq!((r.highest_hops(), r.last_active()), (9, at(3)));
        r.sent_to_child(ShortName(5), 63, at(4));
        assert_eq!(r.last_child(), Some(ShortName(5)));
        assert!(r.handed(63) && !r.handed(0) && !r.handed(64));
        r.sent_to_child(ShortName(6), 0, at(5));
        assert_eq!(r.last_child(), Some(ShortName(6)));
        assert!(r.handed(63) && r.handed(0), "every child stays handed");
        r.sent(at(6));
        assert_eq!(r.last_active(), at(6));
        r.set_owned();
        assert!(r.owned());
        assert!(!r.returned());
        r.set_returned();
        assert!(r.returned() && r.owned());

        let origin = Record::new(MsgId(8), None, 0, at(0));
        assert_eq!(origin.parent(), None);
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
        t.get_mut(MsgId(3)).unwrap().received(1, now);
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
            r.sent_to_child(ShortName(1), 7, at(0));
            r.sent_to_child(ShortName(2), 8, at(0));
        }
        t.slot_reoccupied(7);
        for id in 0..3 {
            let r = t.get(MsgId(id)).unwrap();
            assert!(!r.handed(7) && r.handed(8));
            assert_eq!(r.last_child(), Some(ShortName(2)));
        }
    }
}
