//! The neighbour table (design §8.2): relays heard by heartbeat, keyed by
//! position, which is their identity (§8.1).
//!
//! # Slots
//!
//! An entry keeps its slot from when it is added until it expires or is
//! replaced, so a slot index names a neighbour for as long as it is in the
//! table. Per-message records mark the neighbours they have tried by slot
//! (`tables/records.rs`), and that needs the index to stay put.
//!
//! # Liveness and demotion
//!
//! A rotation is the time for a neighbour's heartbeat to step through all
//! channels and come back to ours: `CHANNELS` heartbeats at the mean
//! interval, 13 × 10 s = 130 s. An entry expires
//! `neighbour_expiry_rotations` rotations after its last heartbeat (§8.2:
//! 3, 390 s).
//!
//! A failed hand-over adds one to `fail_count`; at `demote_after_failures`
//! (§8.2: 2) the neighbour is demoted and is not a next hop. Demotion ends
//! when the neighbour ACKs a packet from this relay, which resets
//! `fail_count` because §8.2 counts consecutive failures, or
//! `demotion_rotations` rotations (3, 390 s) after the failure that
//! demoted it; then `fail_count` starts again from zero. A heartbeat
//! refreshes the entry but leaves `fail_count` alone (roadmap D18): it
//! proves the neighbour is alive, not that it hears us.
//!
//! # Eviction (roadmap D12)
//!
//! When the table is full and an unknown relay is heard, the newcomer
//! replaces
//!
//! 1. the least recently heard demoted entry, if any; else
//! 2. the least recently heard entry, if it is *stale*: not heard for a
//!    full worst-case rotation (`CHANNELS × heartbeat_max`, 195 s), so it
//!    has certainly missed a heartbeat on our channel; else
//! 3. nothing: the newcomer is not added.
//!
//! Least-recently-heard eviction fails on a rooftop relay that hears ≈270
//! relays (§13): about two heartbeats a second arrive on its channel, so
//! an entry would last about 64 / 2 = 30 s and every `fail_count` would be
//! forgotten within a minute. Keeping incumbents replaces exactly the
//! entries that are failing or not being heard. Whether a set spread
//! around the relay routes better is open item O4; [`victim`] is the one
//! function to replace if so.

use crate::geo::{LocalPlane, Point};
use crate::node::Config;
use crate::position::{Position, ShortName};
use crate::radio::{Channel, CHANNELS};
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
    /// When `fail_count` last reached the demotion threshold.
    pub demoted_at: Instant,
    /// Consecutive failed hand-overs to it.
    pub fail_count: u8,
    /// Whether its position is surveyed (heartbeat 0x1) rather than GPS (0x2).
    pub surveyed: bool,
}

/// Timing parameters of the table, derived from [`Config`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NeighbourParams {
    /// Failures at which a neighbour is demoted.
    pub demote_after: u8,
    /// How long a demotion lasts without an ACK.
    pub demotion: Duration,
    /// Age at which an entry expires.
    pub expire_after: Duration,
    /// Age at which an entry may be evicted for a newcomer.
    pub stale_after: Duration,
}

impl NeighbourParams {
    /// From the protocol configuration; see the module documentation.
    pub fn from_config(c: &Config) -> NeighbourParams {
        let mean_rotation = Duration::from_micros((c.heartbeat_min + c.heartbeat_max).mul(CHANNELS as u64).as_micros() / 2);
        NeighbourParams {
            demote_after: c.demote_after_failures,
            demotion: mean_rotation.mul(c.demotion_rotations as u64),
            expire_after: mean_rotation.mul(c.neighbour_expiry_rotations as u64),
            stale_after: c.heartbeat_max.mul(CHANNELS as u64),
        }
    }
}

/// What [`Neighbours::heard`] did. Slots are indices into the table.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Heard {
    /// A known neighbour was refreshed.
    Refreshed(usize),
    /// A new neighbour was added to a free slot.
    Added(usize),
    /// A new neighbour took the slot of this one.
    Replaced(usize, Neighbour),
    /// The table is full of live, working neighbours; the newcomer was not added.
    Rejected,
}

impl Heard {
    /// The slot that now holds a neighbour it did not hold before.
    pub fn new_occupant(self) -> Option<usize> {
        match self {
            Heard::Added(i) | Heard::Replaced(i, _) => Some(i),
            Heard::Refreshed(_) | Heard::Rejected => None,
        }
    }
}

/// The neighbour table.
#[derive(Clone)]
pub struct Neighbours {
    slots: [Option<Neighbour>; NEIGHBOURS],
    params: NeighbourParams,
}

impl Neighbours {
    /// An empty table.
    pub const fn new(params: NeighbourParams) -> Neighbours {
        Neighbours { slots: [None; NEIGHBOURS], params }
    }

    /// The parameters.
    pub fn params(&self) -> &NeighbourParams {
        &self.params
    }

    /// Number of entries, expired ones not yet removed included.
    pub fn len(&self) -> usize {
        self.iter().count()
    }

    /// Whether there are no entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// All entries with their slots, in slot order.
    pub fn slots(&self) -> impl Iterator<Item = (usize, &Neighbour)> {
        self.slots.iter().enumerate().filter_map(|(i, n)| n.as_ref().map(|n| (i, n)))
    }

    /// All entries, in slot order.
    pub fn iter(&self) -> impl Iterator<Item = &Neighbour> {
        self.slots.iter().flatten()
    }

    /// The entry in `slot`.
    pub fn slot(&self, slot: usize) -> Option<&Neighbour> {
        self.slots.get(slot)?.as_ref()
    }

    /// The slot of `position`.
    pub fn slot_of(&self, position: Position) -> Option<usize> {
        self.slots().find(|(_, n)| n.position == position).map(|(i, _)| i)
    }

    /// The entry for `position`.
    pub fn get(&self, position: Position) -> Option<&Neighbour> {
        self.slot(self.slot_of(position)?)
    }

    /// The slot and entry a short name (design §8.1) names. Every entry was
    /// heard, so lies within hearing range, where a short name is unique.
    pub fn by_short(&self, short: ShortName) -> Option<(usize, &Neighbour)> {
        self.slots().find(|(_, n)| n.position.short() == short)
    }

    /// Whether `n` is demoted at `now`.
    pub fn is_demoted(&self, n: &Neighbour, now: Instant) -> bool {
        n.fail_count >= self.params.demote_after && now.since(n.demoted_at) < self.params.demotion
    }

    /// Whether `n` has expired at `now`.
    pub fn is_expired(&self, n: &Neighbour, now: Instant) -> bool {
        now.since(n.last_heard) >= self.params.expire_after
    }

    /// Next-hop candidates at `now`, with their slots: live and not demoted.
    pub fn candidates(&self, now: Instant) -> impl Iterator<Item = (usize, &Neighbour)> {
        self.slots().filter(move |(_, n)| !self.is_demoted(n, now) && !self.is_expired(n, now))
    }

    /// A heartbeat from `hb.position` was heard at `now`. Refreshes its
    /// channel, flag and time, or adds it.
    pub fn heard(&mut self, hb: &Heartbeat, plane: &LocalPlane, now: Instant) -> Heard {
        if let Some(i) = self.slot_of(hb.position) {
            if let Some(n) = &mut self.slots[i] {
                n.home = hb.home;
                n.surveyed = hb.surveyed;
                n.last_heard = now;
            }
            return Heard::Refreshed(i);
        }
        let new = Neighbour {
            position: hb.position,
            local: plane.point(hb.position),
            home: hb.home,
            last_heard: now,
            demoted_at: Instant::ZERO,
            fail_count: 0,
            surveyed: hb.surveyed,
        };
        if let Some(i) = self.slots.iter().position(Option::is_none) {
            self.slots[i] = Some(new);
            return Heard::Added(i);
        }
        match victim(self, now) {
            Some(i) => match self.slots[i].replace(new) {
                Some(old) => Heard::Replaced(i, old),
                None => Heard::Added(i),
            },
            None => Heard::Rejected,
        }
    }

    /// `position` ACKed a packet from this relay: its failures are over.
    pub fn succeeded(&mut self, position: Position) {
        if let Some(n) = self.entry_mut(position) {
            n.fail_count = 0;
        }
    }

    /// A hand-over to `position` failed at `now`. Returns whether it is now
    /// demoted, or `None` if it is not in the table.
    pub fn failed(&mut self, position: Position, now: Instant) -> Option<bool> {
        let params = self.params;
        let n = self.entry_mut(position)?;
        if n.fail_count >= params.demote_after && now.since(n.demoted_at) >= params.demotion {
            n.fail_count = 0;
        }
        n.fail_count = n.fail_count.saturating_add(1);
        if n.fail_count >= params.demote_after {
            n.demoted_at = now;
        }
        Some(n.fail_count >= params.demote_after)
    }

    /// Remove every entry expired at `now`.
    pub fn expire(&mut self, now: Instant) {
        let expire_after = self.params.expire_after;
        for s in &mut self.slots {
            if s.is_some_and(|n| now.since(n.last_heard) >= expire_after) {
                *s = None;
            }
        }
    }

    /// When the next entry expires, if any.
    pub fn next_expiry(&self) -> Option<Instant> {
        self.iter().map(|n| n.last_heard + self.params.expire_after).min()
    }

    /// Recompute every entry's local point after own position changed.
    pub fn replane(&mut self, plane: &LocalPlane) {
        for n in self.slots.iter_mut().flatten() {
            n.local = plane.point(n.position);
        }
    }

    fn entry_mut(&mut self, position: Position) -> Option<&mut Neighbour> {
        self.slots.iter_mut().flatten().find(|n| n.position == position)
    }
}

/// The eviction policy (roadmap D12): which slot of a full table a
/// newcomer takes at `now`, if any. See the module documentation.
pub fn victim(table: &Neighbours, now: Instant) -> Option<usize> {
    let oldest = |demoted: bool| {
        table.slots().filter(|(_, n)| table.is_demoted(n, now) == demoted).min_by_key(|(_, n)| n.last_heard).map(|(i, _)| i)
    };
    let stale = |i: &usize| table.slot(*i).is_some_and(|n| now.since(n.last_heard) >= table.params.stale_after);
    oldest(true).or_else(|| oldest(false).filter(stale))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> NeighbourParams {
        NeighbourParams::from_config(&Config::default())
    }

    fn hb(i: i32, home: u8) -> Heartbeat {
        Heartbeat { surveyed: true, home: Channel::new(home).unwrap(), position: pos(i) }
    }

    fn pos(i: i32) -> Position {
        Position { lat: i, lon: -i }
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
        assert_eq!(p.demotion, Duration::from_secs(390), "three rotations of 13 × 10 s");
        assert_eq!(p.expire_after, Duration::from_secs(390), "3 × 13 × 10 s ≈ the design's 400 s");
        assert_eq!(p.stale_after, Duration::from_secs(195));
    }

    #[test]
    fn heard_adds_and_refreshes() {
        let mut t = Neighbours::new(params());
        assert_eq!(t.heard(&hb(1000, 3), &plane(), at(0)), Heard::Added(0));
        let n = *t.get(pos(1000)).unwrap();
        assert_eq!(n.home.index(), 3);
        assert!((n.local.north - 1000.0 * crate::position::UNIT_M as f32).abs() < 0.01);
        t.failed(n.position, at(5));
        assert_eq!(t.heard(&hb(1000, 4), &plane(), at(10)), Heard::Refreshed(0));
        let n = *t.get(n.position).unwrap();
        assert_eq!((n.home.index(), n.fail_count, n.last_heard), (4, 1, at(10)), "a heartbeat is not an ACK");
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn lookup_by_short_name() {
        let mut t = Neighbours::new(params());
        t.heard(&hb(7, 0), &plane(), at(0));
        t.heard(&hb(9, 0), &plane(), at(0));
        assert_eq!(t.by_short(pos(9).short()).map(|(i, n)| (i, n.position)), Some((1, pos(9))));
        assert_eq!(t.by_short(pos(8).short()), None);
    }

    #[test]
    fn demotion_ends_on_ack_or_after_three_rotations_not_on_heartbeat() {
        let mut t = Neighbours::new(params());
        t.heard(&hb(1, 0), &plane(), at(0));
        let p = pos(1);
        assert_eq!(t.candidates(at(1)).count(), 1);
        assert_eq!(t.failed(p, at(1)), Some(false));
        t.succeeded(p);
        assert_eq!(t.failed(p, at(1)), Some(false), "an ACK resets: failures are consecutive");
        assert_eq!(t.failed(p, at(2)), Some(true));
        assert_eq!(t.candidates(at(2)).count(), 0);
        assert_eq!(t.failed(pos(2), at(2)), None);

        // Heartbeats keep it alive but demoted (D18).
        t.heard(&hb(1, 0), &plane(), at(300));
        assert_eq!(t.candidates(at(300)).count(), 0, "a heartbeat does not lift demotion");
        // Three rotations after the demoting failure it is back.
        assert_eq!(t.candidates(at(391)).count(), 0);
        assert_eq!(t.candidates(at(392)).count(), 1);
        // And starts counting again from zero.
        assert_eq!(t.failed(p, at(393)), Some(false));
        assert_eq!(t.failed(p, at(394)), Some(true));
        // An ACK lifts it at once.
        t.succeeded(p);
        assert_eq!(t.candidates(at(395)).count(), 1);
    }

    #[test]
    fn expiry_frees_the_slot_and_keeps_the_others() {
        let mut t = Neighbours::new(params());
        t.heard(&hb(1, 0), &plane(), at(0));
        t.heard(&hb(2, 0), &plane(), at(100));
        assert_eq!(t.next_expiry(), Some(at(390)));
        assert_eq!(t.candidates(at(390)).count(), 1, "expired entries are not candidates");
        t.expire(at(389));
        assert_eq!(t.len(), 2);
        t.expire(at(390));
        assert_eq!(t.len(), 1);
        assert_eq!(t.slot_of(pos(2)), Some(1), "slots do not move");
        assert_eq!(t.next_expiry(), Some(at(490)));
        assert_eq!(t.heard(&hb(3, 0), &plane(), at(400)), Heard::Added(0), "the free slot is reused");
    }

    #[test]
    fn eviction_prefers_demoted_then_stale_then_keeps_incumbents() {
        let mut t = Neighbours::new(params());
        for i in 0..NEIGHBOURS as i32 {
            assert_eq!(t.heard(&hb(i, 0), &plane(), at(i as u64)), Heard::Added(i as usize));
        }
        // All live and recently heard: the newcomer is turned away.
        assert_eq!(t.heard(&hb(100, 0), &plane(), at(100)), Heard::Rejected);
        // A demoted entry goes first, even though others are older.
        t.failed(pos(40), at(100));
        t.failed(pos(40), at(100));
        match t.heard(&hb(101, 0), &plane(), at(101)) {
            Heard::Replaced(40, old) => assert_eq!(old.position, pos(40)),
            other => panic!("{other:?}"),
        }
        // Entry 0, last heard at 0 s, is stale at 195 s.
        assert_eq!(t.heard(&hb(102, 0), &plane(), at(194)), Heard::Rejected);
        match t.heard(&hb(103, 0), &plane(), at(195)) {
            Heard::Replaced(0, old) => assert_eq!(old.position, pos(0)),
            other => panic!("{other:?}"),
        }
        assert_eq!(t.len(), NEIGHBOURS);
    }

    #[test]
    fn a_lapsed_demotion_is_not_preferred_for_eviction() {
        let mut t = Neighbours::new(params());
        for i in 0..NEIGHBOURS as i32 {
            t.heard(&hb(i, 0), &plane(), at(0));
        }
        t.failed(pos(5), at(0));
        t.failed(pos(5), at(0));
        for i in 0..NEIGHBOURS as i32 {
            t.heard(&hb(i, 0), &plane(), at(390));
        }
        assert_eq!(t.heard(&hb(100, 0), &plane(), at(391)), Heard::Rejected);
    }
}
