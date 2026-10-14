//! Per-message records (design §9.2.2, roadmap D11, D17): what a relay
//! remembers about each message it has received or originated, so that it
//! can run its part of the depth-first search and recognise a repeat.
//!
//! # Lifetime
//!
//! A record dies [`record_idle`] after the message was last received or
//! sent here; each receipt or send restarts the clock. That is how long
//! the longest search the hop cap allows takes when every hand-over
//! succeeds at the first attempt but waits the longest for the channel
//! and carries a full frame:
//!
//! ```text
//! 255 hand-overs (§9.2.6) × (500 ms listen-before-talk give-up
//!                            + 138.6 ms airtime of a 255-byte frame
//!                            + 100 ms ACK timeout, §9.3)  = 188.3 s
//! ```
//!
//! So in a search without failures no record expires while the message is
//! away exploring beyond it. While the message waits here for the
//! duty-cycle budget, which can take minutes, its record is kept alive
//! ([`Record::held`]).
//!
//! # Bound
//!
//! At most [`RECORDS`] records; when full, a new one replaces a dead one,
//! else the least recently active. The bound is the most messages a relay
//! can handle in one record lifetime within its duty cycle (§3): each costs
//! at least an ACK (9 bytes, 11.84 ms) and one DATA packet with no payload
//! (21 bytes, 18.24 ms), together 30.08 ms; 10 % of 188.33 s is 18.83 s of
//! airtime, and ⌊18.83 s / 30.08 ms⌋ = 626. (§9.2.2 rounds the two
//! airtimes to 11.8 and 18.2 ms and gets 627.)
//!
//! # The tried set
//!
//! D17 defines the tried set as the parent, the last child, and every
//! neighbour before the last child in the order of §9.2.3 (distance to the
//! destination, then short name), which is fixed for a message. Choosing
//! the next hop needs nothing more: the candidates are the live, undemoted
//! neighbours after the last child in that order. The order also stays
//! right when the table changes, since it never goes back.
//!
//! Recognising a repeat (§9.2.4, cases 5 and 6) needs to know whom this
//! relay actually handed the message to, which the record keeps as a mask
//! over neighbour-table slots ([`Record::handed`]). With the table
//! unchanged and every neighbour before the last child a candidate when
//! passed over, the mask and D17's set agree. They differ for a relay
//! passed over because it was demoted, expired or not in the table: D17
//! counts it as tried, so its offer of the message would be ACKed and
//! dropped (case 5), ending the one copy that is searching; the mask
//! counts it as untried, so the message goes straight back to it (case 6)
//! and its search goes on, which is what a depth-first search does on
//! meeting a visited node. When a slot changes hands its bit is cleared in
//! every record ([`Records::slot_reoccupied`]), so a newcomer is never
//! taken for a neighbour that was handed the message; a returning
//! neighbour that was is then sent the message straight back, and drops
//! it as a repeat from its parent, which is safe. The spec text may follow
//! this.

use crate::ids::MsgId;
use crate::node::Config;
use crate::position::ShortName;
use crate::radio::{time_on_air, Channel, DUTY_CYCLE_PERCENT, MIN_PREAMBLE_SYMBOLS};
use crate::tables::attempt_span;
use crate::tables::neighbours::NEIGHBOURS;
use crate::time::{Duration, Instant};
use crate::wire::{ACK_LEN, DATA_HEADER_LEN};

/// The hop cap: the largest count the one-byte `hops` field holds (§9.2.6).
pub const MAX_HOPS: u8 = u8::MAX;

/// How long a record lives without the message being received or sent,
/// for a relay with this ACK timeout; see the module documentation.
pub const fn record_idle(ack_timeout: Duration) -> Duration {
    attempt_span(ack_timeout).mul(MAX_HOPS as u64)
}

/// The most records a relay keeps; see the module documentation.
pub const fn records_for(idle: Duration) -> usize {
    let airtime = idle.as_micros() * DUTY_CYCLE_PERCENT as u64 / 100;
    let per_message = time_on_air(ACK_LEN as u8, MIN_PREAMBLE_SYMBOLS).as_micros()
        + time_on_air(DATA_HEADER_LEN as u8, MIN_PREAMBLE_SYMBOLS).as_micros();
    (airtime / per_message) as usize
}

/// Record lifetime for the design's 100 ms ACK timeout (§9.3): 188.3 s.
pub const RECORD_IDLE: Duration = record_idle(Duration::from_millis(100));

/// Record bound for [`RECORD_IDLE`]: 626.
pub const RECORDS: usize = records_for(RECORD_IDLE);

const _: () = assert!(RECORD_IDLE.as_micros() == 188_332_800);
const _: () = assert!(RECORDS == 626);
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
    /// The parent's home channel; `None` at the origin, which has no parent.
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

    /// The relay the message first came from and its home channel; `None`
    /// at the origin.
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

    /// Whether the record is still alive at `now`, with records dying
    /// `idle` after their last activity.
    pub fn alive(&self, idle: Duration, now: Instant) -> bool {
        now.since(self.last_active) < idle
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

    /// The message was sent at `now` somewhere that does not change the
    /// search: back to the parent, or straight back to a relay that
    /// offered it (§9.2.4 case 6).
    pub fn sent(&mut self, now: Instant) {
        self.last_active = now;
    }

    /// The message is still in this relay's send queue at `now`, held back
    /// by the duty-cycle budget (§9.4). The lifetime covers a message away
    /// exploring beyond this relay; one still here must keep its record,
    /// or it could not be routed when its turn comes.
    pub fn held(&mut self, now: Instant) {
        self.last_active = now;
    }
}

/// The records of one relay.
#[derive(Clone)]
pub struct Records {
    entries: [Record; RECORDS],
    len: usize,
    idle: Duration,
}

impl Records {
    /// No records; each dies `idle` after its last activity.
    pub const fn new(idle: Duration) -> Records {
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
            idle,
        }
    }

    /// No records, with the lifetime for `config.ack_timeout`.
    pub fn from_config(config: &Config) -> Records {
        Records::new(record_idle(config.ack_timeout))
    }

    /// How long a record lives without activity.
    pub fn idle(&self) -> Duration {
        self.idle
    }

    /// Records alive at `now`.
    pub fn live(&self, now: Instant) -> usize {
        self.entries[..self.len]
            .iter()
            .filter(|r| r.alive(self.idle, now))
            .count()
    }

    /// The live record for `id`.
    pub fn get(&self, id: MsgId, now: Instant) -> Option<&Record> {
        self.entries[..self.len]
            .iter()
            .find(|r| r.id == id && r.alive(self.idle, now))
    }

    /// The live record for `id`, to update.
    pub fn get_mut(&mut self, id: MsgId, now: Instant) -> Option<&mut Record> {
        let idle = self.idle;
        self.entries[..self.len]
            .iter_mut()
            .find(|r| r.id == id && r.alive(idle, now))
    }

    /// Keep `record`, which must be for a message without a live record.
    /// It takes free space, else a dead record's place, else the least
    /// recently active one's, which is returned.
    pub fn insert(&mut self, record: Record, now: Instant) -> Option<Record> {
        debug_assert!(self.get(record.id, now).is_none(), "one record per message");
        if self.len < RECORDS {
            self.entries[self.len] = record;
            self.len += 1;
            return None;
        }
        let idle = self.idle;
        let live = self.entries.iter().enumerate();
        let (i, victim) = match live.clone().find(|(_, r)| !r.alive(idle, now)) {
            Some((i, _)) => (i, None),
            None => {
                let i = live
                    .min_by_key(|(_, r)| r.last_active)
                    .map_or(0, |(i, _)| i);
                (i, Some(self.entries[i]))
            }
        };
        self.entries[i] = record;
        victim
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
        let c = Config::default();
        assert_eq!(record_idle(c.ack_timeout), RECORD_IDLE);
        // 255 × (500 + 138.56 + 100) ms.
        assert_eq!(RECORD_IDLE, Duration::from_micros(255 * 738_560));
        assert_eq!(RECORDS, 626);
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

    #[test]
    fn idle_expiry_and_refresh() {
        let idle = Duration::from_millis(100);
        let mut t = Records::new(idle);
        t.insert(Record::new(MsgId(1), None, 0, at(0)), at(0));
        assert!(t.get(MsgId(1), at(99)).is_some());
        t.get_mut(MsgId(1), at(99)).unwrap().received(1, at(99));
        assert!(
            t.get(MsgId(1), at(198)).is_some(),
            "receipt restarts the clock"
        );
        t.get_mut(MsgId(1), at(198)).unwrap().sent(at(198));
        assert!(t.get(MsgId(1), at(297)).is_some(), "so does a send");
        assert!(t.get(MsgId(1), at(298)).is_none());
        assert!(t.get_mut(MsgId(1), at(298)).is_none());
        assert_eq!(t.live(at(298)), 0);
        // A dead record makes way for a new one for the same message.
        assert_eq!(
            t.insert(Record::new(MsgId(1), None, 5, at(300)), at(300)),
            None
        );
        assert_eq!(t.get(MsgId(1), at(300)).unwrap().highest_hops(), 5);
    }

    #[test]
    fn full_table_takes_dead_then_least_recently_active() {
        let mut t = Records::new(RECORD_IDLE);
        for i in 0..RECORDS as u32 {
            assert_eq!(
                t.insert(Record::new(MsgId(i), None, 0, at(i as u64)), at(i as u64)),
                None
            );
        }
        let now = at(RECORDS as u64);
        // Touch the oldest, so the second oldest is least recently active.
        t.get_mut(MsgId(0), now).unwrap().sent(now);
        let gone = t
            .insert(Record::new(MsgId(1000), None, 0, now), now)
            .unwrap();
        assert_eq!(gone.id(), MsgId(1));
        assert!(t.get(MsgId(0), now).is_some() && t.get(MsgId(1000), now).is_some());
        // Once records die, they go first, and nothing live is displaced.
        let later = now + RECORD_IDLE;
        t.get_mut(MsgId(1000), now).unwrap().sent(later);
        assert_eq!(
            t.insert(Record::new(MsgId(1001), None, 0, later), later),
            None
        );
        assert_eq!(t.live(later), 2, "1000 and 1001; the rest died");
    }

    #[test]
    fn a_reoccupied_slot_is_not_handed() {
        let mut t = Records::new(RECORD_IDLE);
        for id in 0..3 {
            t.insert(Record::new(MsgId(id), None, 0, at(0)), at(0));
            let r = t.get_mut(MsgId(id), at(0)).unwrap();
            r.sent_to_child(ShortName(1), 7, at(0));
            r.sent_to_child(ShortName(2), 8, at(0));
        }
        t.slot_reoccupied(7);
        for id in 0..3 {
            let r = t.get(MsgId(id), at(0)).unwrap();
            assert!(!r.handed(7) && r.handed(8));
            assert_eq!(r.last_child(), Some(ShortName(2)));
        }
    }
}
