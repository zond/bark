//! The neighbour table (design §8.2): relays heard by heartbeat, keyed by
//! position, which is their identity (§8.1).
//!
//! # Liveness and demotion
//!
//! An entry expires `neighbour_expiry_rotations` rotations after its last
//! heartbeat (§8.2: 3, ≈400 s). A rotation is the time for a neighbour's
//! heartbeat to step through all channels and come back to ours:
//! `CHANNELS` heartbeats at the mean interval, 13 × 10 s = 130 s.
//!
//! A failed transfer adds one to `fail_count`; at `demote_after_failures`
//! (§8.2: 2) the neighbour is demoted, skipped as a next hop. A successful
//! transfer resets `fail_count`, because §8.2 counts *consecutive*
//! failures. A heartbeat also resets it: §8.2 says a demoted neighbour is
//! skipped "until its next heartbeat resets it", and §6.3/§9.3 rely on the
//! heartbeat to carry a moved neighbour's new channel, the usual cause of
//! a failure. Demotion is not stored separately: it is
//! `fail_count >= demote_after`, so the two can never disagree.
//!
//! # Eviction (design open item O4)
//!
//! Decision, 2026-10-14: when the table is full and an unknown relay is
//! heard, the newcomer replaces
//!
//! 1. the least recently heard demoted entry, if any; else
//! 2. the least recently heard entry, if it is *stale*: not heard for a
//!    full worst-case rotation (`CHANNELS × heartbeat_max`, 195 s), so it
//!    has certainly missed a heartbeat on our channel; else
//! 3. nothing: the newcomer is not added.
//!
//! Why not least-recently-heard, as §8.2 proposes: a rooftop relay hears
//! ≈270 relays (§13), ≈2 heartbeats per second on its channel. Every
//! unknown one would evict a live entry, so an entry would last about
//! 64 / 2 = 30 s, every `fail_count` would be forgotten within a minute,
//! and the table would hold a random 64 of the 270 anyway. Keeping
//! incumbents holds a random 64 too, keeps their demotion state, and
//! replaces exactly the entries that are failing or not being heard.
//! It does not aim for geographic diversity (the concern O4 raises); that
//! needs a rule tuned in the simulator, and [`victim`] is the one function
//! to replace when there is one.

use crate::geo::{LocalPlane, Point};
use crate::node::Config;
use crate::position::Position;
use crate::radio::{Channel, CHANNELS};
use crate::tables::fixed::FixedVec;
use crate::time::{Duration, Instant};
use crate::wire::Heartbeat;

/// Table capacity (design §8.2).
pub const NEIGHBOURS: usize = 64;

/// One neighbouring relay.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Neighbour {
    /// Its position: the key and its identity.
    pub position: Position,
    /// Its position in this node's local plane.
    pub local: Point,
    /// Its home channel, where to transmit to it.
    pub home: Channel,
    /// When its last heartbeat was heard.
    pub last_heard: Instant,
    /// Consecutive failed transfers to it.
    pub fail_count: u8,
    /// Whether its position is surveyed (heartbeat 0x1) rather than GPS (0x2).
    pub surveyed: bool,
}

impl Neighbour {
    const FILL: Neighbour = Neighbour {
        position: Position { lat: 0, lon: 0 },
        local: Point { east: 0.0, north: 0.0 },
        home: match Channel::new(0) {
            Some(c) => c,
            None => unreachable!(),
        },
        last_heard: Instant::ZERO,
        fail_count: 0,
        surveyed: false,
    };
}

/// Timing parameters of the table, derived from [`Config`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NeighbourParams {
    /// Failures at which a neighbour is demoted.
    pub demote_after: u8,
    /// Age at which an entry expires.
    pub expire_after: Duration,
    /// Age at which an entry may be evicted for a newcomer.
    pub stale_after: Duration,
}

impl NeighbourParams {
    /// From the protocol configuration; see the module documentation.
    pub fn from_config(c: &Config) -> NeighbourParams {
        let mean_rotation = (c.heartbeat_min + c.heartbeat_max).mul(CHANNELS as u64).as_micros() / 2;
        NeighbourParams {
            demote_after: c.demote_after_failures,
            expire_after: Duration::from_micros(mean_rotation * c.neighbour_expiry_rotations as u64),
            stale_after: c.heartbeat_max.mul(CHANNELS as u64),
        }
    }
}

/// What [`Neighbours::heard`] did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Heard {
    /// A known neighbour was refreshed.
    Refreshed,
    /// A new neighbour was added to free space.
    Added,
    /// A new neighbour replaced this one.
    Replaced(Neighbour),
    /// The table is full of live, working neighbours; the newcomer was not added.
    Rejected,
}

/// The neighbour table.
#[derive(Clone)]
pub struct Neighbours {
    entries: FixedVec<Neighbour, NEIGHBOURS>,
    params: NeighbourParams,
}

impl Neighbours {
    /// An empty table.
    pub const fn new(params: NeighbourParams) -> Neighbours {
        Neighbours { entries: FixedVec::new(Neighbour::FILL), params }
    }

    /// The parameters.
    pub fn params(&self) -> &NeighbourParams {
        &self.params
    }

    /// Number of entries, expired ones not yet removed included.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there are no entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// All entries, in no particular order.
    pub fn iter(&self) -> impl Iterator<Item = &Neighbour> {
        self.entries.as_slice().iter()
    }

    /// The entry for `position`.
    pub fn get(&self, position: Position) -> Option<&Neighbour> {
        self.iter().find(|n| n.position == position)
    }

    /// Whether `n` is demoted.
    pub fn is_demoted(&self, n: &Neighbour) -> bool {
        n.fail_count >= self.params.demote_after
    }

    /// Whether `n` has expired at `now`.
    pub fn is_expired(&self, n: &Neighbour, now: Instant) -> bool {
        now.since(n.last_heard) >= self.params.expire_after
    }

    /// Next-hop candidates at `now`: live and not demoted.
    pub fn candidates(&self, now: Instant) -> impl Iterator<Item = &Neighbour> {
        self.iter().filter(move |n| !self.is_demoted(n) && !self.is_expired(n, now))
    }

    /// A heartbeat from `hb.position` was heard at `now`. Refreshes its
    /// channel, flag and time and resets `fail_count`, or adds it.
    pub fn heard(&mut self, hb: &Heartbeat, plane: &LocalPlane, now: Instant) -> Heard {
        if let Some(n) = self.entries.as_mut_slice().iter_mut().find(|n| n.position == hb.position) {
            n.home = hb.home;
            n.surveyed = hb.surveyed;
            n.last_heard = now;
            n.fail_count = 0;
            return Heard::Refreshed;
        }
        let new = Neighbour {
            position: hb.position,
            local: plane.point(hb.position),
            home: hb.home,
            last_heard: now,
            fail_count: 0,
            surveyed: hb.surveyed,
        };
        if self.entries.push(new).is_ok() {
            return Heard::Added;
        }
        match victim(self.entries.as_slice(), &self.params, now) {
            Some(i) => {
                let old = self.entries.as_slice()[i];
                self.entries.as_mut_slice()[i] = new;
                Heard::Replaced(old)
            }
            None => Heard::Rejected,
        }
    }

    /// A transfer to `position` was acknowledged.
    pub fn succeeded(&mut self, position: Position) {
        if let Some(n) = self.entry_mut(position) {
            n.fail_count = 0;
        }
    }

    /// A transfer to `position` failed. Returns whether it is now demoted,
    /// or `None` if it is not in the table.
    pub fn failed(&mut self, position: Position) -> Option<bool> {
        let demote_after = self.params.demote_after;
        let n = self.entry_mut(position)?;
        n.fail_count = n.fail_count.saturating_add(1);
        Some(n.fail_count >= demote_after)
    }

    /// Remove every entry expired at `now`.
    pub fn expire(&mut self, now: Instant) {
        let expire_after = self.params.expire_after;
        self.entries.retain(|n| now.since(n.last_heard) < expire_after);
    }

    /// When the next entry expires, if any.
    pub fn next_expiry(&self) -> Option<Instant> {
        self.iter().map(|n| n.last_heard + self.params.expire_after).min()
    }

    /// Recompute every entry's local point after own position changed.
    pub fn replane(&mut self, plane: &LocalPlane) {
        for n in self.entries.as_mut_slice() {
            n.local = plane.point(n.position);
        }
    }

    fn entry_mut(&mut self, position: Position) -> Option<&mut Neighbour> {
        self.entries.as_mut_slice().iter_mut().find(|n| n.position == position)
    }
}

/// The eviction policy (design O4): which entry of a full table a newcomer
/// replaces at `now`, if any. See the module documentation.
pub fn victim(entries: &[Neighbour], params: &NeighbourParams, now: Instant) -> Option<usize> {
    let oldest = |demoted: bool| {
        entries
            .iter()
            .enumerate()
            .filter(|(_, n)| (n.fail_count >= params.demote_after) == demoted)
            .min_by_key(|(_, n)| n.last_heard)
            .map(|(i, _)| i)
    };
    oldest(true).or_else(|| oldest(false).filter(|&i| now.since(entries[i].last_heard) >= params.stale_after))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> NeighbourParams {
        NeighbourParams::from_config(&Config::default())
    }

    fn hb(i: i32, home: u8) -> Heartbeat {
        Heartbeat { surveyed: true, home: Channel::new(home).unwrap(), position: Position { lat: i, lon: -i } }
    }

    fn at(s: u64) -> Instant {
        Instant::from_micros(s * 1_000_000)
    }

    fn plane() -> LocalPlane {
        LocalPlane::new(Position::default())
    }

    #[test]
    fn params_follow_the_design() {
        let p = params();
        assert_eq!(p.demote_after, 2);
        assert_eq!(p.expire_after, Duration::from_secs(390), "3 × 13 × 10 s ≈ the design's 400 s");
        assert_eq!(p.stale_after, Duration::from_secs(195));
    }

    #[test]
    fn heard_adds_and_refreshes() {
        let mut t = Neighbours::new(params());
        assert_eq!(t.heard(&hb(1000, 3), &plane(), at(0)), Heard::Added);
        let n = *t.get(Position { lat: 1000, lon: -1000 }).unwrap();
        assert_eq!(n.home.index(), 3);
        assert!((n.local.north - 1000.0 * crate::position::UNIT_M as f32).abs() < 0.01);
        t.failed(n.position);
        assert_eq!(t.heard(&hb(1000, 4), &plane(), at(10)), Heard::Refreshed);
        let n = *t.get(n.position).unwrap();
        assert_eq!((n.home.index(), n.fail_count, n.last_heard), (4, 0, at(10)));
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn demotion_and_reset() {
        let mut t = Neighbours::new(params());
        t.heard(&hb(1, 0), &plane(), at(0));
        let p = Position { lat: 1, lon: -1 };
        assert_eq!(t.candidates(at(1)).count(), 1);
        assert_eq!(t.failed(p), Some(false));
        t.succeeded(p);
        assert_eq!(t.failed(p), Some(false), "success resets: failures are consecutive");
        assert_eq!(t.failed(p), Some(true));
        assert_eq!(t.candidates(at(1)).count(), 0);
        assert_eq!(t.failed(Position { lat: 2, lon: 2 }), None);
        t.heard(&hb(1, 0), &plane(), at(2));
        assert_eq!(t.candidates(at(2)).count(), 1, "a heartbeat lifts demotion");
    }

    #[test]
    fn expiry() {
        let mut t = Neighbours::new(params());
        t.heard(&hb(1, 0), &plane(), at(0));
        t.heard(&hb(2, 0), &plane(), at(100));
        assert_eq!(t.next_expiry(), Some(at(390)));
        assert_eq!(t.candidates(at(390)).count(), 1, "expired entries are not candidates");
        t.expire(at(389));
        assert_eq!(t.len(), 2);
        t.expire(at(390));
        assert_eq!(t.len(), 1);
        assert_eq!(t.next_expiry(), Some(at(490)));
    }

    #[test]
    fn eviction_prefers_demoted_then_stale_then_keeps_incumbents() {
        let mut t = Neighbours::new(params());
        for i in 0..NEIGHBOURS as i32 {
            assert_eq!(t.heard(&hb(i, 0), &plane(), at(i as u64)), Heard::Added);
        }
        // All live and recently heard: the newcomer is turned away.
        assert_eq!(t.heard(&hb(100, 0), &plane(), at(100)), Heard::Rejected);
        // A demoted entry goes first, even though others are older.
        let p = Position { lat: 40, lon: -40 };
        t.failed(p);
        t.failed(p);
        match t.heard(&hb(101, 0), &plane(), at(101)) {
            Heard::Replaced(old) => assert_eq!(old.position, p),
            other => panic!("{other:?}"),
        }
        // Entry 0, last heard at 0 s, is stale at 195 s.
        assert_eq!(t.heard(&hb(102, 0), &plane(), at(194)), Heard::Rejected);
        match t.heard(&hb(103, 0), &plane(), at(195)) {
            Heard::Replaced(old) => assert_eq!(old.position, Position { lat: 0, lon: 0 }),
            other => panic!("{other:?}"),
        }
        assert_eq!(t.len(), NEIGHBOURS);
    }
}
