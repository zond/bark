//! What a reset would lose and still be true after it, kept across the
//! reboot into a new image (spec "Updates over the mesh", the install;
//! roadmap D180): the neighbour table, without which a node just back
//! knows no way on for what it carries, and the message records, which a
//! return to it needs. Beside what the node holds ([`Node::held`]),
//! which it offers again.
//!
//! Each instant is kept as a time before the moment the tables were
//! taken, and put back that long before that moment on the clock after
//! the reset, the time away given by the driver: each entry ages as if
//! the node had listened and heard nothing meanwhile, and nothing is
//! taken as fresher than it is.

use super::Node;
use crate::ids::MsgId;
use crate::node::{heartbeat_interval, Config};
use crate::tables::kept::Then;
use crate::tables::neighbours::{NeighbourParams, KEPT_LEN as NEIGHBOUR_LEN};
use crate::tables::records::KEPT_LEN as RECORD_LEN;
use crate::time::{Duration, Instant};

/// One entry kept across a reset, in the core's own encoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kept {
    /// A neighbour-table entry, with its slot.
    Neighbour([u8; NEIGHBOUR_LEN]),
    /// A message record.
    Record([u8; RECORD_LEN]),
}

/// The longest a kept entry counts for after it was last renewed: a
/// neighbour's expiry, the longest an anchorless sleepy one's, 1 200.9 s
/// with the design's delivery TTL (spec "Neighbour table"), or a
/// record's delivery TTL. A driver whose clock starts at its reset sets
/// the core's epoch at least this, the time away and the time it takes
/// to write the tables, before it, so that every entry still alive has
/// an instant.
pub const fn remembered_within(config: &Config) -> Duration {
    let p = NeighbourParams::from_config(config);
    let mut d = config.delivery_ttl.as_micros();
    let expiries = [
        p.expire_after,
        p.sleepy_expire_after,
        p.anchorless_expire_after,
    ];
    let mut k = 0;
    while k < expiries.len() {
        if expiries[k].as_micros() > d {
            d = expiries[k].as_micros();
        }
        k += 1;
    }
    Duration::from_micros(d)
}

impl<const H: usize, const Q: usize> Node<H, Q> {
    /// What a reset at `now` would lose and still be true after it: every
    /// neighbour entry, then every message record within the delivery
    /// TTL of its message's last activity, but those of the messages the
    /// node holds, which it offers again as its own and new.
    pub fn remembered(&self, now: Instant) -> impl Iterator<Item = Kept> + '_ {
        let held = move |id: MsgId| {
            self.holding()
                .chain(self.inbox.iter().map(|q| q.msg_id))
                .any(|h| h == id)
        };
        self.neighbours.kept(now).map(Kept::Neighbour).chain(
            self.records
                .kept(self.config.delivery_ttl, held, now)
                .map(Kept::Record),
        )
    }

    /// Put back an entry [`Node::remembered`] gave, taken `ago` before
    /// `now`, after the reset and before the first event: whether it was
    /// kept. An entry is dropped that has expired by `now`, a record past
    /// the delivery TTL, and an entry from before the clock's epoch. A
    /// neighbour's heartbeats due on this node's channel meanwhile pass
    /// unheard with no reading, as any the radio was away for (spec
    /// "Heartbeat").
    pub fn remember(&mut self, kept: &Kept, ago: Duration, now: Instant) -> bool {
        let then = Then::before(now, ago);
        match kept {
            Kept::Record(b) => self.records.restore(b, then, self.config.delivery_ttl, now),
            Kept::Neighbour(b) => {
                let Some(mut n) = self.neighbours.restore(b, then, &self.plane, now).copied()
                else {
                    return false;
                };
                let interval = heartbeat_interval(n.id, &self.config);
                while self.slot_window(&n).is_some_and(|(from, _)| from <= now) {
                    let next =
                        crate::tables::neighbours::Neighbours::next_beat(&n, interval, self.home);
                    self.neighbours.slot_passed(n.id, next, self.home);
                    n = *self.neighbours.get(n.id).expect("just put back");
                }
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::ids::NodeId;
    use crate::position::Position;
    use crate::radio::Channel;
    use crate::tables::neighbours::NEIGHBOURS;
    use crate::tables::records::{Record, RECORDS};
    use crate::wire::Heartbeat;
    use crate::Relay;
    use std::vec::Vec;

    fn s(secs: u64) -> Duration {
        Duration::from_secs(secs)
    }

    fn at(secs: u64) -> Instant {
        Instant::ZERO + s(secs)
    }

    /// Neighbour `i` heard at `now`, a relay on channel `i` mod 13.
    fn hear(r: &mut Relay, i: u32, now: Instant) {
        let position = Position {
            lat: 1_000 * i as i32,
            lon: -700 * i as i32,
        };
        let hb = Heartbeat {
            sleepy: i % 5 == 1,
            anchorless: false,
            carried: i % 7 == 2,
            leaving: false,
            home: Channel::new((i % 13) as u8).unwrap(),
            format: crate::wire::FORMAT,
            map_point: i.is_multiple_of(3),
            id: NodeId::from_u64(100 + i as u64),
            short: position.short(),
        };
        let plane = r.plane;
        r.neighbours
            .heard(&hb, position, -90 + i as i16 % 20, &plane, now);
    }

    fn record(r: &mut Relay, id: u32, now: Instant) {
        let parent = id
            .is_multiple_of(2)
            .then(|| (NodeId::from_u64(id as u64), Channel::new(3).unwrap()));
        let mut rec = Record::new(MsgId(id), parent, (id % 9) as u8, now);
        rec.handed_to(id as usize % NEIGHBOURS);
        rec.received(NodeId::from_u64(7 + id as u64), 4, now);
        let at = r.records.place(|_| false).unwrap();
        r.records.put(at, rec);
    }

    fn taken(r: &Relay, now: Instant) -> Vec<Kept> {
        r.remembered(now).collect()
    }

    fn put_back(r: &mut Relay, kept: &[Kept], ago: Duration, now: Instant) -> usize {
        kept.iter().filter(|k| r.remember(k, ago, now)).count()
    }

    #[test]
    fn a_full_table_and_every_record_come_back() {
        let config = Config::default();
        let mut r = Relay::new(config, 1);
        let now = at(1_000);
        for i in 0..NEIGHBOURS as u32 {
            hear(&mut r, i, now - s(u64::from(i)));
        }
        let id = NodeId::from_u64(100);
        r.neighbours.failed(id, now);
        // A whole millisecond, so that its rounding later changes nothing.
        r.neighbours.refused(id, now + s(2));
        r.neighbours.refused(NodeId::from_u64(101), now + s(20));
        r.neighbours.told(NodeId::from_u64(102), s(5));
        for k in 0..RECORDS as u32 {
            record(&mut r, k, now - Duration::from_millis(u64::from(k) * 100));
        }
        let kept = taken(&r, now);
        assert_eq!(kept.len(), NEIGHBOURS + RECORDS);
        let mut back = Relay::new(config, 2);
        assert_eq!(put_back(&mut back, &kept, Duration::ZERO, now), kept.len());
        for slot in 0..NEIGHBOURS {
            let (a, b) = (r.neighbours.slot(slot), back.neighbours.slot(slot));
            assert_eq!(a, b, "slot {slot}");
        }
        for k in 0..RECORDS as u32 {
            assert_eq!(r.records.get(MsgId(k)), back.records.get(MsgId(k)));
        }
    }

    #[test]
    fn every_instant_moves_by_the_time_away() {
        let config = Config::default();
        let mut r = Relay::new(config, 3);
        let now = at(5_000);
        hear(&mut r, 0, now - s(100));
        let id = NodeId::from_u64(100);
        r.neighbours.refused(id, now + s(30));
        // Its slots are known: one was heard 100 s before.
        let home = r.home;
        r.neighbours.beat_heard(id, home, now - s(100));
        let rssi = r.neighbours.get(id).unwrap().rssi;
        record(&mut r, 1, now - s(50));
        let kept = taken(&r, now);
        // The clock after the reset reads 2 000 s; the tables were taken
        // 60 s before.
        let mut back = Relay::new(config, 4);
        let later = at(2_000);
        assert_eq!(put_back(&mut back, &kept, s(60), later), 2);
        let n = back.neighbours.get(id).unwrap();
        assert_eq!(n.last_heard, later - s(160));
        assert_eq!(n.banned_until, later - s(30), "the ban ran on");
        assert_eq!(
            back.records.get(MsgId(1)).unwrap().last_active(),
            later - s(110)
        );
        // Its heartbeat came due on this channel while the node was off:
        // it passed with no reading, and the next is ahead.
        assert_eq!(n.rssi, rssi);
        let (from, _) = back.slot_window(n).unwrap();
        assert!(from > later);
        assert!(n.beat_at > later - s(160));
    }

    #[test]
    fn an_entry_that_expires_meanwhile_is_dropped() {
        let config = Config::default();
        let expiry = NeighbourParams::from_config(&config).expire_after;
        assert_eq!(expiry, s(390));
        let mut r = Relay::new(config, 5);
        let now = at(5_000);
        hear(&mut r, 0, now - s(300));
        let kept = taken(&r, now);
        let mut back = Relay::new(config, 6);
        assert_eq!(put_back(&mut back, &kept, s(89), now), 1, "301 s + 89 s");
        let mut back = Relay::new(config, 6);
        assert_eq!(put_back(&mut back, &kept, s(90), now), 0, "300 s + 90 s");
        assert!(back.neighbours.is_empty());
    }

    #[test]
    fn a_record_past_the_delivery_ttl_is_not_kept() {
        let config = Config::default();
        assert_eq!(config.delivery_ttl, s(600));
        let mut r = Relay::new(config, 7);
        let now = at(5_000);
        record(&mut r, 1, now - s(600));
        record(&mut r, 2, now - s(590));
        let kept = taken(&r, now);
        assert_eq!(kept.len(), 1, "600 s idle is past the TTL");
        let mut back = Relay::new(config, 8);
        assert_eq!(put_back(&mut back, &kept, s(9), now), 1);
        let mut back = Relay::new(config, 8);
        assert_eq!(put_back(&mut back, &kept, s(10), now), 0, "590 s + 10 s");
        assert!(back.records.is_empty());
    }

    #[test]
    fn an_entry_from_before_the_epoch_is_dropped() {
        let config = Config::default();
        let mut r = Relay::new(config, 9);
        let now = at(5_000);
        hear(&mut r, 0, now - s(10));
        record(&mut r, 1, now - s(10));
        let kept = taken(&r, now);
        // A clock that started 30 s ago, the tables taken 25 s before it.
        let mut back = Relay::new(config, 10);
        assert_eq!(put_back(&mut back, &kept, s(55), at(30)), 0);
    }

    #[test]
    fn the_longest_a_kept_entry_counts() {
        // An anchorless neighbour's: 26 beat periods of 600 s / 13 and a
        // heartbeat's lag (spec "Neighbour table").
        let d = remembered_within(&Config::DEFAULT);
        assert_eq!(d.as_millis(), 1_200_000 + 872);
    }
}
