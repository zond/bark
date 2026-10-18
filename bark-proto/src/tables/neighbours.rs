//! The neighbour table (spec "Neighbour table"): relays heard by heartbeat, keyed by
//! node ID, each with the position its heartbeat gave.
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
//! `neighbour_expiry_rotations` rotations after its last heartbeat (spec "Neighbour table":
//! 3, 390 s).
//!
//! A failed hand-over adds one to `fail_count`; at `demote_after_failures`
//! (spec "Neighbour table": 2) the neighbour is demoted: a next hop only when no other with
//! margin is left (spec "Next hop", roadmap D25). Demotion ends
//! when the neighbour ACKs a packet from this relay, which resets
//! `fail_count` because spec "Neighbour table" counts consecutive failures, or
//! `demotion_rotations` rotations (3, 390 s) after the failure that
//! demoted it; then `fail_count` starts again from zero. A heartbeat
//! refreshes the entry but leaves `fail_count` alone (roadmap D18): it
//! proves the neighbour is alive, not that it hears us.
//!
//! # Margin (roadmap D38)
//!
//! A neighbour has margin when its averaged RSSI, `rssi`, is high enough
//! for a full-length frame to get through a fade of [`FADE_DEPTH_DB`]
//! (spec "Next hop"). The average takes in every frame heard from the neighbour:
//! heartbeats, its DATA, and its ACKs, which are the best evidence that
//! its link carries frames. Each new frame counts half (spec "Relay choice"). A neighbour without margin stays in the
//! table: it is alive.
//!
//! [`FADE_DEPTH_DB`]: crate::radio::FADE_DEPTH_DB
//!
//! # Bans
//!
//! A neighbour that refuses a hand-over (spec "Receiving DATA") is banned for
//! [`BAN`]: no next hop at all until then (spec "Next hop", roadmap D29). A refusal
//! is no failure: it says the neighbour hears us.
//!
//! A failed hand-over bans it too, for [`hold`] of its `fail_count`: one
//! ban after the first failure in a row, doubling with each further one up
//! to the bucket's refill (roadmap D42). A refusal measures what the
//! receiver's bucket lacks; silence says only that the channel or the
//! receiver is full, and not how full, so the sender backs off further
//! each time. An ACK that takes a packet resets `fail_count`, and so the
//! ladder.
//!
//! [`BAN`]: crate::tables::budget::BAN
//! [`hold`]: crate::tables::budget::hold
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
//! relays (spec "Capacity and sizing"): about two heartbeats a second arrive on its channel, so
//! an entry would last about 64 / 2 = 30 s and every `fail_count` would be
//! forgotten within a minute. Keeping incumbents replaces exactly the
//! entries that are failing or not being heard. Whether a set spread
//! around the relay routes better is open item O4; [`victim`] is the one
//! function to replace if so.
//!
//! # Sleepy and anchorless neighbours
//!
//! A sleepy node's beat fills the table like any heartbeat, the entry
//! marked sleepy: between its windows it receives duty-cycled, so it is
//! sent to with the long preamble. One with an anchor is a next hop only
//! as a last resort ([`Neighbour::last_resort`], spec "Next hop"), and
//! its entry expires after two beat periods, since its beats come once a
//! period and only on its anchor's channel. As a newcomer to a full table
//! it takes only a free slot: eviction makes way for a node that is a
//! next hop like any other (spec "Neighbour table").
//!
//! A sleepy node without an anchor beats with the anchorless bit too, once
//! an anchorless beat period on the next channel each time: no anchor, a
//! next hop like any other, and its entry expires two of its worst-case
//! rotations and a beat's lag after its last beat. A rotation of its beats
//! is 13 minutes; with one lost beat in ten an expiry after one would drop
//! a live neighbour every tenth rotation, for a rotation, where it may be
//! the only way on. Two lose it one time in a hundred; a stale entry costs
//! a hand-over's attempts and a ban.
//!
//! A relay answers each beat of an anchorless neighbour with its own
//! heartbeat on the neighbour's channel, with the long preamble, so that
//! the neighbour hears it and takes it as its anchor: at once and every
//! fade after, for two beat periods ([`Neighbours::answer_due`]).
//!
//! # The anchor of a node that sleeps
//!
//! A sleeping node hears its anchor's heartbeat only when the rotation
//! lands in one of its windows, so it keeps the anchor's entry
//! ([`Neighbours::pin`]): the entry neither expires nor makes way for a
//! newcomer while pinned (spec "Relay choice").

use crate::geo::{LocalPlane, Point};
use crate::ids::NodeId;
use crate::node::{heartbeat_lag, Config};
use crate::position::Position;
use crate::radio::{full_frame_rssi_above_noise_db, Channel, CHANNELS};
use crate::sniff::ANCHORLESS_BEAT_PERIOD;
use crate::tables::budget::hold;
use crate::tables::heard::beat_period;
use crate::time::{Duration, Instant};
use crate::wire::Heartbeat;

/// Table capacity (spec "Neighbour table").
pub const NEIGHBOURS: usize = 64;

/// One neighbouring relay.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Neighbour {
    /// Its ID: the key.
    pub id: NodeId,
    /// Its position, rebuilt from its heartbeat's short form.
    pub position: Position,
    /// Its position in this node's local plane.
    pub local: Point,
    /// Its home channel, where to transmit to it.
    pub home: Channel,
    /// When its last heartbeat was heard.
    pub last_heard: Instant,
    /// When `fail_count` last reached the demotion threshold.
    pub demoted_at: Instant,
    /// Until when it is banned: it refused a hand-over, or its last ones
    /// failed.
    pub banned_until: Instant,
    /// Averaged packet RSSI of the frames heard from it, dBm.
    pub rssi: f32,
    /// Consecutive failed hand-overs to it.
    pub fail_count: u8,
    /// Whether it is sleepy (heartbeat 0x9 or 0xB): it receives
    /// duty-cycled between its windows, and is sent to with the long
    /// preamble.
    pub sleepy: bool,
    /// Whether it is sleepy and knows no node that is not (heartbeat 0xB):
    /// no anchor for a sleepy node.
    pub anchorless: bool,
    /// Heartbeats heard from it, up to [`BEATS`].
    pub beats: u8,
    /// When its last heartbeat heard started, and on which channel: with its
    /// fixed interval, when its next comes to any channel
    /// ([`Neighbours::next_beat`]).
    pub beat_at: Instant,
    /// See `beat_at`.
    pub beat_on: Channel,
    /// Of an anchorless neighbour, the answers sent since its last beat
    /// ([`Neighbours::answer_due`]).
    pub answered: u8,
}

impl Neighbour {
    /// Whether it is a sleepy node with an anchor: a next hop only where
    /// no other is (spec "Next hop").
    pub fn last_resort(&self) -> bool {
        self.sleepy && !self.anchorless
    }

    /// Whether it can be an anchor: it is not sleepy (spec "Relay choice").
    pub fn relay(&self) -> bool {
        !self.sleepy
    }
}

/// The heartbeats counted per neighbour: a sleepy node switches to a
/// stronger node only once it has heard two of its heartbeats
/// (spec "Relay choice").
pub const BEATS: u8 = 2;

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
    /// Age at which an anchorless entry expires.
    pub anchorless_expire_after: Duration,
    /// Age at which the entry of a sleepy node with an anchor expires: two
    /// beat periods.
    pub sleepy_expire_after: Duration,
}

impl NeighbourParams {
    /// From the protocol configuration; see the module documentation.
    pub const fn from_config(c: &Config) -> NeighbourParams {
        let mean_rotation = Duration::from_micros(
            (c.heartbeat_min.as_micros() + c.heartbeat_max.as_micros()) * CHANNELS as u64 / 2,
        );
        NeighbourParams {
            demote_after: c.demote_after_failures,
            demotion: mean_rotation.mul(c.demotion_rotations as u64),
            expire_after: mean_rotation.mul(c.neighbour_expiry_rotations as u64),
            stale_after: c.heartbeat_max.mul(CHANNELS as u64),
            anchorless_expire_after: Duration::from_micros(
                ANCHORLESS_BEAT_PERIOD.as_micros() * (2 * CHANNELS as u64)
                    + ANCHORLESS_BEAT_PERIOD.as_micros() / 2
                    + heartbeat_lag(c).as_micros(),
            ),
            sleepy_expire_after: beat_period(c.delivery_ttl).mul(2),
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
    /// The entry kept whatever its age (module documentation).
    pinned: Option<NodeId>,
}

impl Neighbours {
    /// An empty table.
    pub const fn new(params: NeighbourParams) -> Neighbours {
        Neighbours {
            slots: [None; NEIGHBOURS],
            params,
            pinned: None,
        }
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
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(i, n)| n.as_ref().map(|n| (i, n)))
    }

    /// All entries, in slot order.
    pub fn iter(&self) -> impl Iterator<Item = &Neighbour> {
        self.slots.iter().flatten()
    }

    /// The entry in `slot`.
    pub fn slot(&self, slot: usize) -> Option<&Neighbour> {
        self.slots.get(slot)?.as_ref()
    }

    /// The slot of `id`.
    pub fn slot_of(&self, id: NodeId) -> Option<usize> {
        self.slots().find(|(_, n)| n.id == id).map(|(i, _)| i)
    }

    /// The entry for `id`.
    pub fn get(&self, id: NodeId) -> Option<&Neighbour> {
        self.slot(self.slot_of(id)?)
    }

    /// The slot and entry of `id`.
    pub fn by_id(&self, id: NodeId) -> Option<(usize, &Neighbour)> {
        self.slots().find(|(_, n)| n.id == id)
    }

    /// Whether `n` is demoted at `now`.
    pub fn is_demoted(&self, n: &Neighbour, now: Instant) -> bool {
        n.fail_count >= self.params.demote_after && now.since(n.demoted_at) < self.params.demotion
    }

    /// The averaged RSSI, dBm, from which a neighbour has margin: a
    /// full-length frame reaches it through a fade, with the noise floor
    /// at `noise_floor_dbm` (spec "Next hop", roadmap D38).
    pub fn margin_dbm(noise_floor_dbm: i16) -> f32 {
        noise_floor_dbm as f32 + full_frame_rssi_above_noise_db()
    }

    /// Whether `n` has margin with the noise floor at `noise_floor_dbm`.
    pub fn has_margin(n: &Neighbour, noise_floor_dbm: i16) -> bool {
        n.rssi >= Neighbours::margin_dbm(noise_floor_dbm)
    }

    /// Whether `n` is banned at `now`.
    pub fn is_banned(&self, n: &Neighbour, now: Instant) -> bool {
        now < n.banned_until
    }

    /// Whether `n` has expired at `now`.
    pub fn is_expired(&self, n: &Neighbour, now: Instant) -> bool {
        Some(n.id) != self.pinned && now.since(n.last_heard) >= self.expire_after(n)
    }

    fn expire_after(&self, n: &Neighbour) -> Duration {
        if n.anchorless {
            self.params.anchorless_expire_after
        } else if n.sleepy {
            self.params.sleepy_expire_after
        } else {
            self.params.expire_after
        }
    }

    /// Keep the entry of `id`, if any, whatever its age; `None` keeps none.
    pub fn pin(&mut self, id: Option<NodeId>) {
        self.pinned = id;
    }

    /// Neighbours that work at `now`, with their slots: live and not
    /// demoted. The owner rule (spec "Owner") looks only at these.
    pub fn candidates(&self, now: Instant) -> impl Iterator<Item = (usize, &Neighbour)> {
        self.live(now)
            .filter(move |(_, n)| !self.is_demoted(n, now))
    }

    /// Entries not expired at `now`, demoted or not, with their slots.
    pub fn live(&self, now: Instant) -> impl Iterator<Item = (usize, &Neighbour)> {
        self.slots().filter(move |(_, n)| !self.is_expired(n, now))
    }

    /// A heartbeat from `hb.id` at `position` was heard at `now` with
    /// packet RSSI `rssi`. Refreshes its position, channel, flags, time and
    /// RSSI, or adds it; a sleepy node with an anchor only into a free
    /// slot (module documentation).
    pub fn heard(
        &mut self,
        hb: &Heartbeat,
        position: Position,
        rssi: i16,
        plane: &LocalPlane,
        now: Instant,
    ) -> Heard {
        if let Some(i) = self.slot_of(hb.id) {
            if let Some(n) = &mut self.slots[i] {
                if n.position != position {
                    n.position = position;
                    n.local = plane.point(position);
                }
                n.home = hb.home;
                n.sleepy = hb.sleepy;
                n.anchorless = hb.anchorless && hb.sleepy;
                n.answered = 0;
                n.beats = (n.beats + 1).min(BEATS);
                n.last_heard = now;
                n.rssi = average(n.rssi, rssi);
            }
            return Heard::Refreshed(i);
        }
        let new = Neighbour {
            id: hb.id,
            position,
            local: plane.point(position),
            home: hb.home,
            last_heard: now,
            demoted_at: Instant::ZERO,
            banned_until: Instant::ZERO,
            rssi: rssi as f32,
            fail_count: 0,
            sleepy: hb.sleepy,
            anchorless: hb.anchorless && hb.sleepy,
            beats: 1,
            beat_at: now,
            beat_on: hb.home,
            answered: 0,
        };
        if let Some(i) = self.slots.iter().position(Option::is_none) {
            self.slots[i] = Some(new);
            return Heard::Added(i);
        }
        if new.last_resort() {
            return Heard::Rejected;
        }
        match victim(self, now) {
            Some(i) => match self.slots[i].replace(new) {
                Some(old) => Heard::Replaced(i, old),
                None => Heard::Added(i),
            },
            None => Heard::Rejected,
        }
    }

    /// A heartbeat of `id` was heard on channel `on` at `now`.
    pub fn beat_heard(&mut self, id: NodeId, on: Channel, now: Instant) {
        if let Some(n) = self.entry_mut(id) {
            n.beat_at = now;
            n.beat_on = on;
        }
    }

    /// When the next heartbeat of `n`, which steps through the channels
    /// one a heartbeat at its fixed `interval` (spec "Heartbeat"), comes to
    /// channel `ch`, after the last one heard.
    pub fn next_beat(n: &Neighbour, interval: Duration, ch: Channel) -> Instant {
        let steps = (ch.index() + CHANNELS - n.beat_on.index()) % CHANNELS;
        let steps = if steps == 0 { CHANNELS } else { steps };
        n.beat_at + interval.mul(steps as u64)
    }

    /// The next answer due to an anchorless neighbour (spec "Sleepy nodes alone"):
    /// the `k`-th, counted from 0, `k` × `every` after the start of its last
    /// beat, for the first `answers`. Its ID, its home, where it listens,
    /// and when. The answers end long before the entry expires, so expiry
    /// is not checked.
    pub fn answer_due(&self, every: Duration, answers: u8) -> Option<(NodeId, Channel, Instant)> {
        self.iter()
            .filter(|n| n.anchorless && n.answered < answers)
            .map(|n| (n.id, n.home, n.beat_at + every.mul(n.answered as u64)))
            .min_by_key(|&(_, _, at)| at)
    }

    /// An answer to `id` went at `now`, or was given up: the next is due
    /// `every` after the start of its beat for each sent, and never before
    /// one `every` from now.
    pub fn answer_sent(&mut self, id: NodeId, now: Instant, every: Duration) {
        if let Some(n) = self.entry_mut(id) {
            let past = now.since(n.beat_at).as_micros() / every.as_micros().max(1);
            n.answered = n.answered.saturating_add(1).max(past.min(254) as u8 + 1);
        }
    }

    /// `id` took a packet from this relay at `now`: its failures are
    /// over, and it is alive.
    pub fn succeeded(&mut self, id: NodeId, now: Instant) {
        if let Some(n) = self.entry_mut(id) {
            n.fail_count = 0;
        }
        self.answered(id, now);
    }

    /// `id` answered a packet from this relay at `now`, taking or refusing
    /// it: it is alive, whether or not its heartbeats are heard
    /// (spec "The ACK").
    pub fn answered(&mut self, id: NodeId, now: Instant) {
        if let Some(n) = self.entry_mut(id) {
            n.last_heard = n.last_heard.max(now);
        }
    }

    /// A DATA or ACK frame from neighbour `id` was heard with packet RSSI
    /// `rssi`.
    pub fn frame_heard(&mut self, id: NodeId, rssi: i16) {
        if let Some(n) = self.entry_mut(id) {
            n.rssi = average(n.rssi, rssi);
        }
    }

    /// A packet from neighbour `id` gave its home channel in `aux` (spec
    /// "Packet formats"): that is where it listens now, newer than its last
    /// heartbeat if it has moved since.
    pub fn home_heard(&mut self, id: NodeId, home: Channel) {
        if let Some(n) = self.entry_mut(id) {
            n.home = home;
        }
    }

    /// `id` refused a hand-over: banned until `until`.
    pub fn refused(&mut self, id: NodeId, until: Instant) {
        if let Some(n) = self.entry_mut(id) {
            n.banned_until = until;
        }
    }

    /// A hand-over to `id` failed at `now`: it is banned, longer after each
    /// failure in a row. Returns whether it is now demoted, or `None` if it
    /// is not in the table.
    pub fn failed(&mut self, id: NodeId, now: Instant) -> Option<bool> {
        let params = self.params;
        let n = self.entry_mut(id)?;
        if n.fail_count >= params.demote_after && now.since(n.demoted_at) >= params.demotion {
            n.fail_count = 0;
        }
        n.fail_count = n.fail_count.saturating_add(1);
        if n.fail_count >= params.demote_after {
            n.demoted_at = now;
        }
        n.banned_until = n.banned_until.max(now + hold(n.fail_count));
        Some(n.fail_count >= params.demote_after)
    }

    /// Remove the entry of `id`, if any.
    pub fn forget(&mut self, id: NodeId) {
        if let Some(i) = self.slot_of(id) {
            self.slots[i] = None;
        }
    }

    /// Remove every entry expired at `now`.
    pub fn expire(&mut self, now: Instant) {
        for i in 0..NEIGHBOURS {
            if self.slots[i].is_some_and(|n| self.is_expired(&n, now)) {
                self.slots[i] = None;
            }
        }
    }

    /// When the next entry expires, if any.
    pub fn next_expiry(&self) -> Option<Instant> {
        self.iter()
            .filter(|n| Some(n.id) != self.pinned)
            .map(|n| n.last_heard + self.expire_after(n))
            .min()
    }

    /// Recompute every entry's local point after own position changed.
    pub fn replane(&mut self, plane: &LocalPlane) {
        for n in self.slots.iter_mut().flatten() {
            n.local = plane.point(n.position);
        }
    }

    fn entry_mut(&mut self, id: NodeId) -> Option<&mut Neighbour> {
        self.slots.iter_mut().flatten().find(|n| n.id == id)
    }
}

/// `avg` with a new reading `dbm` counting half.
fn average(avg: f32, dbm: i16) -> f32 {
    (avg + dbm as f32) / 2.0
}

/// The eviction policy (roadmap D12): which slot of a full table a
/// newcomer takes at `now`, if any. See the module documentation.
pub fn victim(table: &Neighbours, now: Instant) -> Option<usize> {
    let oldest = |demoted: bool| {
        table
            .slots()
            .filter(|(_, n)| Some(n.id) != table.pinned && table.is_demoted(n, now) == demoted)
            .min_by_key(|(_, n)| n.last_heard)
            .map(|(i, _)| i)
    };
    let stale = |i: &usize| {
        table
            .slot(*i)
            .is_some_and(|n| now.since(n.last_heard) >= table.params.stale_after)
    };
    oldest(true).or_else(|| oldest(false).filter(stale))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> NeighbourParams {
        NeighbourParams::from_config(&Config::default())
    }

    /// Neighbour `i`'s heartbeat, its position and ID following from `i`.
    fn hb(i: i32, home: u8) -> (Heartbeat, Position) {
        let hb = Heartbeat {
            sleepy: false,
            anchorless: false,
            home: Channel::new(home).unwrap(),
            id: id(i),
            short: pos(i).short(),
        };
        (hb, pos(i))
    }

    /// Hear anchorless node `i`'s heartbeat.
    fn hear_anchorless(t: &mut Neighbours, i: i32, rssi: i16, now: Instant) -> Heard {
        let (hb, p) = hb(i, 0);
        let hb = Heartbeat {
            sleepy: true,
            anchorless: true,
            ..hb
        };
        t.heard(&hb, p, rssi, &plane(), now)
    }

    /// A neighbour heard on channel 3 is next on channel 5 two intervals
    /// later, and on channel 3 itself a whole rotation later.
    #[test]
    fn the_next_heartbeat_follows_the_rotation() {
        let mut t = Neighbours::new(params());
        let (hb, p) = hb(1, 0);
        t.heard(&hb, p, -90, &plane(), Instant::ZERO);
        let at = Instant::from_micros(7_000_000);
        t.beat_heard(id(1), Channel::new(3).unwrap(), at);
        let n = *t.get(id(1)).unwrap();
        let interval = Duration::from_secs(10);
        let next = |c| Neighbours::next_beat(&n, interval, Channel::new(c).unwrap());
        assert_eq!(next(5), at + interval.mul(2));
        assert_eq!(next(3), at + interval.mul(CHANNELS as u64));
        assert_eq!(next(2), at + interval.mul(CHANNELS as u64 - 1));
    }

    fn id(i: i32) -> NodeId {
        NodeId((i as u32 + 1).to_be_bytes())
    }

    fn pos(i: i32) -> Position {
        Position { lat: i, lon: -i }
    }

    /// Hear neighbour `i`'s heartbeat on channel `home`.
    fn hear(t: &mut Neighbours, i: i32, home: u8, rssi: i16, now: Instant) -> Heard {
        let (hb, p) = hb(i, home);
        t.heard(&hb, p, rssi, &plane(), now)
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
        assert_eq!(
            p.demotion,
            Duration::from_secs(390),
            "three rotations of 13 × 10 s"
        );
        assert_eq!(
            p.expire_after,
            Duration::from_secs(390),
            "3 × 13 × 10 s ≈ the design's 400 s"
        );
        assert_eq!(p.stale_after, Duration::from_secs(195));
    }

    /// An anchorless entry expires two of its sender's worst-case
    /// rotations and a beat's lag after its last heartbeat: 26.5 beat
    /// periods, and a long full frame, the ACK timeout and two backoffs,
    /// 0.99 s.
    #[test]
    fn anchorless_entries_expire_after_two_rotations() {
        let mut t = Neighbours::new(params());
        assert_eq!(
            params().anchorless_expire_after,
            Duration::from_micros(1_590_992_864)
        );
        hear_anchorless(&mut t, 0, -80, at(0));
        hear(&mut t, 1, 0, -80, at(1300));
        assert!(t.get(id(0)).unwrap().anchorless);
        assert_eq!(t.live(at(1590)).count(), 2);
        assert_eq!(t.live(at(1591)).count(), 1);
        assert_eq!(
            t.next_expiry(),
            Some(Instant::ZERO + params().anchorless_expire_after)
        );
    }

    /// Hear sleepy node `i`'s beat, the sleepy bit alone: it has an anchor.
    fn hear_sleepy(t: &mut Neighbours, i: i32, rssi: i16, now: Instant) -> Heard {
        let (hb, p) = hb(i, 0);
        let hb = Heartbeat { sleepy: true, ..hb };
        t.heard(&hb, p, rssi, &plane(), now)
    }

    /// A sleepy node with an anchor is in the table, marked sleepy, a last
    /// resort, and expires two beat periods after its last beat, 600 s.
    #[test]
    fn sleepy_entries_expire_after_two_beat_periods() {
        let mut t = Neighbours::new(params());
        assert_eq!(params().sleepy_expire_after, Duration::from_secs(600));
        hear_sleepy(&mut t, 0, -80, at(0));
        let n = *t.get(id(0)).unwrap();
        assert!(n.sleepy && !n.anchorless && n.last_resort() && !n.relay());
        assert_eq!(t.live(at(599)).count(), 1);
        assert_eq!(t.live(at(600)).count(), 0);
        assert_eq!(t.next_expiry(), Some(at(600)));
        hear_anchorless(&mut t, 1, -80, at(0));
        let n = *t.get(id(1)).unwrap();
        assert!(n.sleepy && n.anchorless && !n.last_resort());
    }

    /// A sleepy node with an anchor takes a free slot but makes nobody
    /// give up theirs, where any other newcomer would.
    #[test]
    fn a_sleepy_newcomer_takes_only_a_free_slot() {
        let mut t = Neighbours::new(params());
        for i in 0..NEIGHBOURS as i32 - 1 {
            hear(&mut t, i, 0, -80, at(0));
        }
        assert_eq!(hear_sleepy(&mut t, 200, -80, at(1)), Heard::Added(63));
        assert_eq!(hear_sleepy(&mut t, 201, -80, at(300)), Heard::Rejected);
        assert!(matches!(
            hear_anchorless(&mut t, 202, -80, at(300)),
            Heard::Replaced(..)
        ));
    }

    /// A relay answers an anchorless neighbour's beat at once and a fade
    /// later, and starts again at its next beat.
    #[test]
    fn answers_follow_each_anchorless_beat() {
        let mut t = Neighbours::new(params());
        let fade = Duration::from_secs(60);
        hear_anchorless(&mut t, 0, -80, at(0));
        t.beat_heard(id(0), Channel::new(0).unwrap(), at(0));
        let due = |t: &Neighbours| t.answer_due(fade, 2).map(|a| a.2);
        assert_eq!(due(&t), Some(at(0)));
        t.answer_sent(id(0), at(1), fade);
        assert_eq!(due(&t), Some(at(60)));
        // Late, it never sends two at once.
        t.answer_sent(id(0), at(130), fade);
        assert_eq!(due(&t), None);
        hear_anchorless(&mut t, 0, -80, at(780));
        t.beat_heard(id(0), Channel::new(0).unwrap(), at(780));
        assert_eq!(due(&t), Some(at(780)));
    }

    #[test]
    fn heartbeats_are_counted_up_to_two() {
        let mut t = Neighbours::new(params());
        hear(&mut t, 1, 0, -80, at(0));
        assert_eq!(t.get(id(1)).unwrap().beats, 1);
        hear(&mut t, 1, 0, -80, at(130));
        hear(&mut t, 1, 0, -80, at(260));
        assert_eq!(t.get(id(1)).unwrap().beats, BEATS);
    }

    #[test]
    fn margin_follows_every_frame_heard() {
        // Noise floor −117 dBm: a full frame through a 4 dB fade needs a
        // packet RSSI of −117 + 4.1 dBm.
        let floor = -117;
        let mut t = Neighbours::new(params());
        hear(&mut t, 1, 0, -114, at(0));
        let n = *t.get(id(1)).unwrap();
        assert!(!Neighbours::has_margin(&n, floor), "a faint heartbeat");
        t.frame_heard(n.id, -110);
        let n = *t.get(id(1)).unwrap();
        assert_eq!(n.rssi, -112.0, "a new frame counts half");
        assert!(Neighbours::has_margin(&n, floor), "an ACK heard well");
        hear(&mut t, 1, 0, -118, at(130));
        assert!(!Neighbours::has_margin(t.get(id(1)).unwrap(), floor));
    }

    #[test]
    fn heard_adds_and_refreshes() {
        let mut t = Neighbours::new(params());
        assert_eq!(hear(&mut t, 1000, 3, -80, at(0)), Heard::Added(0));
        let n = *t.get(id(1000)).unwrap();
        assert_eq!(n.home.index(), 3);
        assert!((n.local.north - 1000.0 * crate::position::UNIT_M as f32).abs() < 0.01);
        t.failed(n.id, at(5));
        assert_eq!(hear(&mut t, 1000, 4, -80, at(10)), Heard::Refreshed(0));
        let n = *t.get(n.id).unwrap();
        assert_eq!(
            (n.home.index(), n.fail_count, n.last_heard),
            (4, 1, at(10)),
            "a heartbeat is not an ACK"
        );
        assert_eq!(t.len(), 1);
        t.home_heard(n.id, Channel::new(7).unwrap());
        assert_eq!(t.get(n.id).unwrap().home.index(), 7);
        assert_eq!(
            t.get(n.id).unwrap().last_heard,
            at(10),
            "a packet's channel is not a heartbeat"
        );
    }

    #[test]
    fn lookup_by_id() {
        let mut t = Neighbours::new(params());
        hear(&mut t, 7, 0, -80, at(0));
        hear(&mut t, 9, 0, -80, at(0));
        assert_eq!(
            t.by_id(id(9)).map(|(i, n)| (i, n.position)),
            Some((1, pos(9)))
        );
        assert_eq!(t.by_id(id(8)), None);
    }

    #[test]
    fn demotion_ends_on_ack_or_after_three_rotations_not_on_heartbeat() {
        let mut t = Neighbours::new(params());
        hear(&mut t, 1, 0, -80, at(0));
        let p = id(1);
        assert_eq!(t.candidates(at(1)).count(), 1);
        assert_eq!(t.failed(p, at(1)), Some(false));
        t.succeeded(p, at(1));
        assert_eq!(
            t.failed(p, at(1)),
            Some(false),
            "an ACK resets: failures are consecutive"
        );
        assert_eq!(t.failed(p, at(2)), Some(true));
        assert_eq!(t.candidates(at(2)).count(), 0);
        assert_eq!(t.failed(id(2), at(2)), None);

        // Heartbeats keep it alive but demoted (D18).
        hear(&mut t, 1, 0, -80, at(300));
        assert_eq!(
            t.candidates(at(300)).count(),
            0,
            "a heartbeat does not lift demotion"
        );
        // Three rotations after the demoting failure it is back.
        assert_eq!(t.candidates(at(391)).count(), 0);
        assert_eq!(t.candidates(at(392)).count(), 1);
        // And starts counting again from zero.
        assert_eq!(t.failed(p, at(393)), Some(false));
        assert_eq!(t.failed(p, at(394)), Some(true));
        // An ACK lifts it at once.
        t.succeeded(p, at(394));
        assert_eq!(t.candidates(at(395)).count(), 1);
    }

    #[test]
    fn expiry_frees_the_slot_and_keeps_the_others() {
        let mut t = Neighbours::new(params());
        hear(&mut t, 1, 0, -80, at(0));
        hear(&mut t, 2, 0, -80, at(100));
        assert_eq!(t.next_expiry(), Some(at(390)));
        assert_eq!(
            t.candidates(at(390)).count(),
            1,
            "expired entries are not candidates"
        );
        t.expire(at(389));
        assert_eq!(t.len(), 2);
        t.expire(at(390));
        assert_eq!(t.len(), 1);
        assert_eq!(t.slot_of(id(2)), Some(1), "slots do not move");
        assert_eq!(t.next_expiry(), Some(at(490)));
        assert_eq!(
            hear(&mut t, 3, 0, -80, at(400)),
            Heard::Added(0),
            "the free slot is reused"
        );
    }

    #[test]
    fn eviction_prefers_demoted_then_stale_then_keeps_incumbents() {
        let mut t = Neighbours::new(params());
        for i in 0..NEIGHBOURS as i32 {
            assert_eq!(
                hear(&mut t, i, 0, -80, at(i as u64)),
                Heard::Added(i as usize)
            );
        }
        // All live and recently heard: the newcomer is turned away.
        assert_eq!(hear(&mut t, 100, 0, -80, at(100)), Heard::Rejected);
        // A demoted entry goes first, even though others are older.
        t.failed(id(40), at(100));
        t.failed(id(40), at(100));
        match hear(&mut t, 101, 0, -80, at(101)) {
            Heard::Replaced(40, old) => assert_eq!(old.position, pos(40)),
            other => panic!("{other:?}"),
        }
        // Entry 0, last heard at 0 s, is stale at 195 s.
        assert_eq!(hear(&mut t, 102, 0, -80, at(194)), Heard::Rejected);
        match hear(&mut t, 103, 0, -80, at(195)) {
            Heard::Replaced(0, old) => assert_eq!(old.position, pos(0)),
            other => panic!("{other:?}"),
        }
        assert_eq!(t.len(), NEIGHBOURS);
    }

    #[test]
    fn a_lapsed_demotion_is_not_preferred_for_eviction() {
        let mut t = Neighbours::new(params());
        for i in 0..NEIGHBOURS as i32 {
            hear(&mut t, i, 0, -80, at(0));
        }
        t.failed(id(5), at(0));
        t.failed(id(5), at(0));
        for i in 0..NEIGHBOURS as i32 {
            hear(&mut t, i, 0, -80, at(390));
        }
        assert_eq!(hear(&mut t, 100, 0, -80, at(391)), Heard::Rejected);
    }
}
