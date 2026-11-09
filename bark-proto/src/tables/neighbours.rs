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
//! (spec "Neighbour table": 3) the neighbour is demoted: a next hop only when no other with
//! margin is left (spec "Next hop", roadmap D25). Demotion ends
//! when the neighbour answers this relay, an ACK to it that takes,
//! refuses or has seen a packet, or a DATA it addresses to it, which
//! resets `fail_count`, since failures count only in a row with no answer
//! between (roadmap D106); or `demotion_rotations` rotations (3, 390 s)
//! after the failure that demoted it; then `fail_count` starts again from
//! zero. A heartbeat, or a frame it sends to another node, refreshes the
//! entry but leaves `fail_count` alone (roadmap D18): it proves the
//! neighbour is alive, not that it hears us.
//!
//! # Margin (roadmap D38)
//!
//! A neighbour has margin when its averaged RSSI, `rssi`, is high enough
//! for a full-length frame to get through a fade of [`FADE_DEPTH_DB`]
//! (spec "Next hop"). The average takes in every reading of the link,
//! each new one counting half (spec "Relay choice"): every frame heard
//! from the neighbour, heartbeats, its DATA and its ACKs, which are the
//! best evidence that its link carries frames; and each heartbeat of the
//! neighbour that was due on this node's channel and not heard, as a
//! reading at the floor weighted by the chance the radio would have
//! heard it, the share of the heartbeat's window it listened through
//! ([`Neighbours::missed`]), since an average of the frames that got
//! through alone stands above the link (spec "Heartbeat"). A neighbour
//! without margin stays in the table: it is alive.
//!
//! [`FADE_DEPTH_DB`]: crate::radio::FADE_DEPTH_DB
//!
//! # Bans
//!
//! A neighbour that refuses a hand-over (spec "Receiving DATA") is banned for
//! the wait it tells, at least [`BAN`], in place of any ban it had; the
//! refused message goes round it meanwhile (spec "Bans", "Next hop",
//! roadmap D110). A refusal is no failure: it says the neighbour hears us.
//!
//! A failed hand-over bans it too, for [`hold`] of its `fail_count`: one
//! ban after the first failure in a row, doubling with each further one up
//! to the bucket's refill (roadmap D42). A refusal measures what the
//! receiver's bucket lacks; silence says only that the channel or the
//! receiver is full, and not how full, so the sender backs off further
//! each time. Any answer to this relay resets `fail_count`, and so the
//! doubling.
//!
//! [`BAN`]: crate::tables::budget::BAN
//! [`hold`]: crate::tables::budget::hold
//!
//! # Service time
//!
//! Each entry keeps how long this relay takes to get one message through
//! to the neighbour ([`Neighbours::took`], spec "Neighbour table"): what
//! a sender refused for want of that way can be told it will wait.
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
//! 3. for a relay within the owner radius (spec "Owner"), the least
//!    recently heard entry beyond it, since the owner rule reads from the
//!    table that no relay that near is closer (roadmap D68); else
//! 4. nothing: the newcomer is not added.
//!
//! Least-recently-heard eviction fails on a rooftop relay that hears ≈270
//! relays (spec "Capacity and sizing"): about two heartbeats a second arrive on its channel, so
//! an entry would last about 64 / 2 = 30 s and every `fail_count` would be
//! forgotten within a minute. Keeping incumbents replaces exactly the
//! entries that are failing or not being heard. A set spread around the
//! relay by bearing routes no better (roadmap D69).
//!
//! # Sleepy and anchorless neighbours
//!
//! A sleepy node's beat fills the table like any heartbeat, the entry
//! marked sleepy: between its windows it receives duty-cycled, so it is
//! sent to with the long preamble. One with an anchor is a next hop only
//! as a last resort ([`Neighbour::last_resort`], spec "Next hop"), but for
//! a message addressed to it while the relay spares the long frame, and
//! its entry expires after two beat periods, since its beats come once a
//! period and only on its anchor's channel. As a newcomer to a full table
//! it takes only a free slot: eviction makes way for a node that is a
//! next hop like any other (spec "Neighbour table").
//!
//! An anchorless entry counts the consecutive rotations of its sender's
//! beats in which one was heard here ([`Neighbour::stays_anchorless`]).
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
//! fade after, for two beat periods ([`Neighbours::answer_due`]). A relay
//! flags the heartbeats of its burst anchorless too
//! ([`Neighbour::anchorless`]); it listens all the time, and is answered
//! once, with the ordinary preamble.
//!
//! # The anchor of a node that sleeps
//!
//! A sleeping node hears its anchor's heartbeat only when the rotation
//! lands in one of its windows, so it keeps the anchor's entry
//! ([`Neighbours::pin`]): the entry neither expires nor makes way for a
//! newcomer while pinned (spec "Relay choice").

use crate::geo::{LocalPlane, Point};
use crate::ids::NodeId;
use crate::node::{heartbeat_lag, Config, OWNER_RADIUS_M};
use crate::position::Position;
use crate::radio::{full_frame_rssi_above_noise_db, Channel, CHANNELS};
use crate::sniff::anchorless_beat_period;
use crate::tables::budget::{hold, BAN, LONGEST_BAN, LONGEST_TOLD};
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
    /// The newest wire format it speaks, from its last heartbeat (spec
    /// "Neighbour table"); shown to a person, used by no rule while
    /// format 1 is the only one.
    pub format: u8,
    /// Whether its position is a map point a person set, by its last
    /// heartbeat (spec "Neighbour table"); shown, used by no rule yet.
    pub map_point: bool,
    /// What the last heartbeat that carried a section said of its images
    /// (spec "Updates over the mesh"), its image kept once in the table
    /// ([`Neighbours::section`]).
    pub update: Told,
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
    /// Whether it is sleepy (the heartbeat's sleepy flag): it receives
    /// duty-cycled between its windows, and is sent to with the long
    /// preamble.
    pub sleepy: bool,
    /// Whether its last heartbeat had the anchorless flag: it
    /// knows no node that holds mail, and a relay that hears it answers
    /// ([`Neighbours::answer_due`]). A sleepy one is in anchorless mode
    /// ([`Neighbour::alone`]).
    pub anchorless: bool,
    /// Whether it moves (the heartbeat's carried flag): it holds mail for
    /// nobody, so it is no anchor and does not keep this node from owning
    /// a destination it is nearer (spec "Owner", "Relay choice").
    pub carried: bool,
    /// Heartbeats heard from it, up to [`BEATS`]; of an anchorless one,
    /// the consecutive rotations of its beats in which one was heard here,
    /// up to [`BEATS`] ([`Neighbour::stays_anchorless`]).
    pub beats: u8,
    /// When its last heartbeat heard in a slot started, or a slot of its
    /// that passed unheard ([`Neighbours::slot_passed`]), and on which
    /// channel: with its fixed interval, when its next comes to any channel
    /// ([`Neighbours::next_beat`]).
    pub beat_at: Instant,
    /// See `beat_at`.
    pub beat_on: Channel,
    /// Whether `beat_at` is from a heartbeat heard in a slot, so that its
    /// slots can be timed; not from a burst, which is in no slot (spec
    /// "Heartbeat").
    pub slotted: bool,
    /// How long the radio had been away from listening, in all, when the
    /// window of its next heartbeat on this channel opened, once it has
    /// (`relay/heartbeat.rs`, `count_missed_heartbeats`): what it has been
    /// away since then is the share of the window it may have missed.
    pub away_base: Option<u32>,
    /// Of an anchorless neighbour, the answers sent since its last beat
    /// ([`Neighbours::answer_due`]).
    pub answered: u8,
    /// Its service time ([`Neighbours::took`]), in [`SERVICE_UNIT`]s.
    pub service: u32,
    /// The waits its refusals told this relay since its last take, in
    /// [`SERVICE_UNIT`]s ([`Neighbours::told`]).
    pub told: u32,
    /// When it last took a hand-over from this relay.
    pub taken_at: Instant,
    /// When the line of senders this relay refused for it ends
    /// ([`Neighbours::place_in_line`]).
    pub line_end: Instant,
    /// Of a sleepy one with an anchor, whether a hand-over to it with the
    /// long preamble went unanswered since it was last heard: it is not
    /// sent to at once again until it beats (spec "Next hop").
    pub unanswered: bool,
    /// This node's application asked for its status, and the question
    /// has not gone (spec "Neighbour status").
    pub ask: bool,
    /// It asked for this node's status, and the answer has not gone.
    pub tell: bool,
}

impl Neighbour {
    /// Whether it is a sleepy node with an anchor: a next hop only where
    /// no other is (spec "Next hop").
    pub fn last_resort(&self) -> bool {
        self.sleepy && !self.anchorless
    }

    /// Whether it is a sleepy node without an anchor (sleepy and anchorless): no
    /// anchor for a sleepy node, and a next hop like any other.
    pub fn alone(&self) -> bool {
        self.sleepy && self.anchorless
    }

    /// Its service time: the average time this relay takes to get one
    /// message through to it ([`Neighbours::took`]).
    pub fn service(&self) -> Duration {
        SERVICE_UNIT.mul(self.service as u64)
    }

    /// Whether it is not sleepy: it listens all the time.
    pub fn relay(&self) -> bool {
        !self.sleepy
    }

    /// Whether it holds mail: neither sleepy nor carried. Only such a
    /// node is an anchor (spec "Relay choice"), or keeps another from
    /// owning a destination it is nearer (spec "Owner").
    pub fn holds_mail(&self) -> bool {
        !self.sleepy && !self.carried
    }

    /// Whether it is anchorless and was heard so in two consecutive
    /// rotations of its beats: not switching on, about to anchor, but in a
    /// pocket with no relay (spec "Beats").
    pub fn stays_anchorless(&self) -> bool {
        self.alone() && self.beats >= BEATS
    }
}

/// A neighbour's section as its entry keeps it: the image it names is
/// kept once in the table, among [`IMAGES`], by its place there. In 8
/// bytes, so that the entry is 16 bytes less than with the section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Told {
    /// A heartbeat with a section was heard.
    heard: bool,
    runs: u32,
    /// The image's place in the table, [`NO_IMAGE`] for none.
    image: u8,
    complete: u8,
    have: u8,
}

const NO_IMAGE: u8 = u8::MAX;

impl Told {
    /// No section heard.
    pub const NONE: Told = Told {
        heard: false,
        runs: 0,
        image: NO_IMAGE,
        complete: 0,
        have: 0,
    };
}

/// Distinct images the neighbours' sections name that a node keeps at
/// once. A section names the newest image its node holds or gathers: the
/// release spreading, the one before it, run by those that have not heard
/// of it yet, and the older one of a node that was away through a
/// release; a release follows the last no sooner than the day it takes to
/// reach every relay (roadmap D151), so a fourth is room for an image of
/// another key, a bench's. A fifth takes the place of the lowest version,
/// which no neighbour that runs a newer one gathers
/// (spec "Updates over the mesh").
pub const IMAGES: usize = 4;

/// The unit of a service time: 1/1024 of a ban, 1.5 ms, far below the
/// ban in which waits are told (spec "Neighbour table").
pub const SERVICE_UNIT: Duration = BAN.div(1024);

/// A service time before any take: one ban, the least wait a refusal
/// tells (spec "Bans").
const FIRST_SERVICE: u32 = 1024;

/// How many takes the service time remembers: as many as a neighbour at
/// its cap takes while its bucket refills from empty, one a ban for
/// [`LONGEST_BAN`], 39 (spec "Neighbour table").
pub const SERVICE_TAKES: u64 = LONGEST_BAN.as_micros().div_ceil(BAN.as_micros());

const _: () = assert!(SERVICE_TAKES == 39);
const _: () = assert!(
    LONGEST_TOLD.as_micros() / SERVICE_UNIT.as_micros() <= u32::MAX as u64,
    "the longest told wait fits a service time"
);

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
    /// The longest rotation of an anchorless node's beats: 13 beats, one on
    /// each channel, each at most an anchorless beat period after the
    /// last. Two of its beats heard here come in consecutive rotations when
    /// they are at least half of this apart, each beat at least half a
    /// period after the last, and at most this and a heartbeat's lag
    /// ([`NeighbourParams::anchorless_rotations`]).
    pub anchorless_rotation: Duration,
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
                anchorless_beat_period(c.delivery_ttl).as_micros() * (2 * CHANNELS as u64)
                    + heartbeat_lag(c).as_micros(),
            ),
            sleepy_expire_after: beat_period(c.delivery_ttl).mul(2),
            anchorless_rotation: anchorless_beat_period(c.delivery_ttl).mul(CHANNELS as u64),
        }
    }

    /// The least and the most time between two anchorless beats heard in
    /// consecutive rotations of their sender's beats; the lag is what the
    /// expiry adds to two rotations.
    pub fn anchorless_rotations(&self) -> (Duration, Duration) {
        let rotation = self.anchorless_rotation;
        let lag = self.anchorless_expire_after - rotation.mul(2);
        (rotation.div(2), rotation + lag)
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
    /// The images the entries' sections name ([`Told`]).
    images: [Option<crate::update::Image>; IMAGES],
    params: NeighbourParams,
    /// The entry kept whatever its age (module documentation).
    pinned: Option<NodeId>,
}

impl Neighbours {
    /// An empty table.
    pub const fn new(params: NeighbourParams) -> Neighbours {
        Neighbours {
            slots: [None; NEIGHBOURS],
            images: [None; IMAGES],
            params,
            pinned: None,
        }
    }

    /// What [`Neighbours::new`] builds, made where the table lies, with
    /// no copy of it on the stack.
    pub fn reset(&mut self, params: NeighbourParams) {
        self.slots.fill(None);
        self.images.fill(None);
        self.params = params;
        self.pinned = None;
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

    /// A heartbeat of `id` carried `section`.
    pub fn section_heard(&mut self, id: NodeId, section: crate::update::Section) {
        let Some(i) = self.slot_of(id) else {
            return;
        };
        // Its own image no longer counts as named while its new one is placed.
        if let Some(n) = &mut self.slots[i] {
            n.update = Told::NONE;
        }
        let image = section.image.map_or(NO_IMAGE, |m| self.place(m));
        if let Some(n) = &mut self.slots[i] {
            n.update = Told {
                heard: true,
                runs: section.runs,
                image,
                complete: section.complete,
                have: section.have,
            };
        }
    }

    /// The place of `image` among the images kept: its own, a free one, or
    /// the lowest version's if that is below it, whose entries then name
    /// none; else none.
    fn place(&mut self, image: crate::update::Image) -> u8 {
        if let Some(k) = self.images.iter().position(|m| *m == Some(image)) {
            return k as u8;
        }
        let named = |k: usize| {
            self.slots
                .iter()
                .flatten()
                .any(|n| n.update.heard && usize::from(n.update.image) == k)
        };
        let free = (0..IMAGES).find(|&k| self.images[k].is_none() || !named(k));
        let k = match free {
            Some(k) => k,
            None => {
                let (k, lowest) = self
                    .images
                    .iter()
                    .enumerate()
                    .filter_map(|(k, m)| m.map(|m| (k, m.version)))
                    .min_by_key(|x| x.1)
                    .unwrap_or((0, 0));
                if lowest >= image.version {
                    return NO_IMAGE;
                }
                for n in self.slots.iter_mut().flatten() {
                    if usize::from(n.update.image) == k {
                        n.update.image = NO_IMAGE;
                    }
                }
                k
            }
        };
        self.images[k] = Some(image);
        k as u8
    }

    /// The section `n`'s last heartbeat that carried one said.
    pub fn section(&self, n: &Neighbour) -> Option<crate::update::Section> {
        let t = n.update;
        t.heard.then_some(crate::update::Section {
            runs: t.runs,
            image: self.images.get(usize::from(t.image)).copied().flatten(),
            complete: t.complete,
            have: t.have,
        })
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
        if n.alone() {
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
                n.format = hb.format;
                n.map_point = hb.map_point;
                n.sleepy = hb.sleepy;
                n.carried = hb.carried;
                let anchorless = hb.anchorless && hb.sleepy;
                let (min, max) = self.params.anchorless_rotations();
                let gap = now.since(n.beat_at);
                n.beats = if anchorless && !n.alone() {
                    1
                } else if anchorless && gap < min {
                    // On this channel twice in one rotation: no new one.
                    n.beats
                } else if anchorless && gap > max {
                    // A rotation missed: not consecutive.
                    1
                } else {
                    (n.beats + 1).min(BEATS)
                };
                n.anchorless = hb.anchorless;
                n.answered = 0;
                n.unanswered = false;
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
            format: hb.format,
            map_point: hb.map_point,
            update: Told::NONE,
            last_heard: now,
            demoted_at: Instant::ZERO,
            banned_until: Instant::ZERO,
            rssi: rssi as f32,
            fail_count: 0,
            sleepy: hb.sleepy,
            anchorless: hb.anchorless,
            carried: hb.carried,
            beats: 1,
            beat_at: now,
            beat_on: hb.home,
            slotted: false,
            away_base: None,
            answered: 0,
            service: FIRST_SERVICE,
            told: 0,
            taken_at: Instant::ZERO,
            line_end: Instant::ZERO,
            unanswered: false,
            ask: false,
            tell: false,
        };
        if let Some(i) = self.slots.iter().position(Option::is_none) {
            self.slots[i] = Some(new);
            return Heard::Added(i);
        }
        if new.last_resort() {
            return Heard::Rejected;
        }
        match victim(self, now).or_else(|| self.near_victim(&new)) {
            Some(i) => match self.slots[i].replace(new) {
                Some(old) => Heard::Replaced(i, old),
                None => Heard::Added(i),
            },
            None => Heard::Rejected,
        }
    }

    /// A heartbeat of `id` in one of its slots was heard on channel `on`,
    /// starting at `now`.
    pub fn beat_heard(&mut self, id: NodeId, on: Channel, now: Instant) {
        if let Some(n) = self.entry_mut(id) {
            n.beat_at = now;
            n.beat_on = on;
            n.slotted = true;
            n.away_base = None;
        }
    }

    /// A heartbeat of `id`'s burst was heard: it has just switched on, and
    /// its slots are not known until one is heard (spec "Heartbeat").
    pub fn burst_heard(&mut self, id: NodeId) {
        if let Some(n) = self.entry_mut(id) {
            n.slotted = false;
        }
    }

    /// The slot of `id`'s heartbeat on this node's channel, due at `at`,
    /// has passed: its next comes a rotation of its `interval` later.
    pub fn slot_passed(&mut self, id: NodeId, at: Instant, on: Channel) {
        if let Some(n) = self.entry_mut(id) {
            n.beat_at = at;
            n.beat_on = on;
            n.away_base = None;
        }
    }

    /// The window of `id`'s next heartbeat on this channel opened with the
    /// radio away `away_us` in all so far.
    pub fn window_opened(&mut self, id: NodeId, away_us: u32) {
        if let Some(n) = self.entry_mut(id) {
            n.away_base = Some(away_us);
        }
    }

    /// A heartbeat of `id` was due on this node's channel and not heard:
    /// a reading at the floor, `floor_dbm`, with the weight of a frame
    /// heard times `heard_chance`, the chance the radio would have heard
    /// it, the share of its window the radio listened through (spec
    /// "Heartbeat").
    pub fn missed(&mut self, id: NodeId, floor_dbm: i16, heard_chance: f32) {
        if let Some(n) = self.entry_mut(id) {
            n.rssi += (floor_dbm as f32 - n.rssi) * heard_chance.clamp(0.0, 1.0) / 2.0;
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

    /// The next answer due to a neighbour whose last heartbeat had the
    /// anchorless flag (spec "Heartbeat", "Sleepy nodes alone"): the
    /// `k`-th, counted from 0, `k` × `every` after the start of its last
    /// heartbeat, for the first `answers` to a sleepy one, and once to
    /// one that listens all the time. Its ID, its home, where it listens,
    /// and when. The answers end long before the entry expires, so expiry
    /// is not checked.
    pub fn answer_due(&self, every: Duration, answers: u8) -> Option<(NodeId, Channel, Instant)> {
        self.iter()
            .filter(|n| n.anchorless && n.answered < if n.sleepy { answers } else { 1 })
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

    /// `id` answered this relay at `now`: an ACK to it that takes,
    /// refuses or has seen a packet, or a DATA addressed to it. It hears
    /// this relay and is alive, whether or not its heartbeats are heard,
    /// so its failures in a row are over and any demotion lifts
    /// (spec "Neighbour table", "The ACK").
    pub fn answered(&mut self, id: NodeId, now: Instant) {
        if let Some(n) = self.entry_mut(id) {
            n.fail_count = 0;
            n.unanswered = false;
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

    /// `id` took a hand-over from this relay at `now` of a message that
    /// began to wait for it here at `since`. The time since the later of
    /// that and its last take is one sample of its service time, which
    /// folds in whatever held the message: refusals and their bans,
    /// failed attempts, this relay's bucket and its other hand-overs. The
    /// average takes each sample at a weight of one in
    /// [`SERVICE_TAKES`], and stays within [`LONGEST_TOLD`] (spec
    /// "Neighbour table").
    pub fn took(&mut self, id: NodeId, since: Instant, now: Instant) {
        if let Some(n) = self.entry_mut(id) {
            let told = SERVICE_UNIT.mul(n.told as u64);
            let sample = (now.since(since.max(n.taken_at)) - told).min(LONGEST_TOLD);
            let sample = (sample.as_micros() / SERVICE_UNIT.as_micros()) as i64;
            n.told = 0;
            let old = n.service as i64;
            let step = (sample - old) / SERVICE_TAKES as i64;
            n.service = (old + step).clamp(0, u32::MAX as i64) as u32;
            n.taken_at = now;
        }
    }

    /// How long this relay takes to get one message through to `id` now:
    /// its service time, or the time the message waiting
    /// for `id` here since `oldest` has waited, its refusals' waits aside,
    /// if that is longer, so that a rising load is seen before the next
    /// take (spec "Bans"). At most [`LONGEST_TOLD`]. `None` if `id` is not
    /// in the table.
    pub fn service_now(
        &self,
        id: NodeId,
        oldest: Option<Instant>,
        now: Instant,
    ) -> Option<Duration> {
        let n = self.get(id)?;
        // What its refusals made the message wait is its own line, told
        // here: not this relay's service time.
        let told = SERVICE_UNIT.mul(n.told as u64).min(LONGEST_TOLD) - n.banned_until.since(now);
        let waited = oldest.map_or(Duration::ZERO, |t| now.since(t.max(n.taken_at)) - told);
        Some(n.service().max(waited).min(LONGEST_TOLD))
    }

    /// A refusal for want of `id` as the way on, told at `now`, the
    /// messages queued for it here taking `queue` to go: the place it
    /// gives its sender in `id`'s line, from now, when its hand-over would
    /// begin, and the service time that spaced it. The place is one
    /// service time ([`Neighbours::service_now`]) after the last place
    /// given, and at least a ban, the unit a refusal tells, since places
    /// closer than that would be told the same wait and come back
    /// together; or the end of `queue` if that is later (spec "Bans"). Time
    /// passing shortens the line; a place is at most `limit` from now.
    /// `None` if `id` is not in the table.
    pub fn place_in_line(
        &mut self,
        id: NodeId,
        queue: Duration,
        oldest: Option<Instant>,
        limit: Duration,
        now: Instant,
    ) -> Option<(Duration, Duration)> {
        let service = self.service_now(id, oldest, now)?;
        let n = self.entry_mut(id)?;
        let place = (n.line_end.max(now) + service.max(BAN)).max(now + queue);
        n.line_end = place.min(now + limit);
        Some((n.line_end.since(now), service))
    }

    /// Set `id`'s service time, for tests of what it decides.
    #[cfg(test)]
    pub(crate) fn set_service(&mut self, id: NodeId, service: Duration) {
        if let Some(n) = self.entry_mut(id) {
            n.service = (service.as_micros() / SERVICE_UNIT.as_micros()) as u32;
        }
    }

    /// `id` refused a hand-over, telling this relay to wait `wait`: not
    /// part of its service time ([`Neighbours::took`]).
    pub fn told(&mut self, id: NodeId, wait: Duration) {
        if let Some(n) = self.entry_mut(id) {
            let units = wait.as_micros() / SERVICE_UNIT.as_micros();
            n.told = (n.told as u64 + units).min(u32::MAX as u64) as u32;
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

    /// A hand-over to the sleepy `id` with the long preamble went
    /// unanswered ([`Neighbour::unanswered`]).
    pub fn unanswered(&mut self, id: NodeId) {
        if let Some(n) = self.entry_mut(id) {
            n.unanswered = true;
        }
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

    /// Each position rebuilt from its short form near `near`, for a node
    /// that has just learned where it is (spec "Positions on the wire").
    pub fn rebuild(&mut self, near: Position) {
        for n in self.slots.iter_mut().flatten() {
            n.position = Position::from_short(n.position.short(), near);
        }
    }

    /// Recompute every entry's local point after own position changed.
    pub fn replane(&mut self, plane: &LocalPlane) {
        for n in self.slots.iter_mut().flatten() {
            n.local = plane.point(n.position);
        }
    }

    /// Mark live neighbour `id` to be asked for its status, `tell` false,
    /// or told this node's, `tell` true (spec "Neighbour status"); whether it is
    /// one.
    pub fn status_due(&mut self, id: NodeId, tell: bool, now: Instant) -> bool {
        let live = self.get(id).is_some_and(|n| !self.is_expired(n, now));
        match self.entry_mut(id).filter(|_| live) {
            Some(n) if tell => n.tell = true,
            Some(n) => n.ask = true,
            None => return false,
        }
        true
    }

    /// The next status frame due: to a live neighbour, an answer before a
    /// question, as `(id, tell)`. Marks of neighbours expired are
    /// cleared.
    pub fn next_status(&mut self, now: Instant) -> Option<(NodeId, bool)> {
        let mut next = None;
        for i in 0..NEIGHBOURS {
            let Some(n) = self.slots[i].as_ref().filter(|n| n.ask || n.tell) else {
                continue;
            };
            if self.is_expired(n, now) {
                let n = self.slots[i].as_mut().expect("an entry");
                (n.ask, n.tell) = (false, false);
                continue;
            }
            if n.tell {
                return Some((n.id, true));
            }
            next = next.or(Some((n.id, false)));
        }
        next
    }

    /// The status frame to `id` went, or was given up.
    pub fn status_gone(&mut self, id: NodeId, tell: bool) {
        if let Some(n) = self.entry_mut(id) {
            match tell {
                true => n.tell = false,
                false => n.ask = false,
            }
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

impl Neighbours {
    /// For a relay `new` within the owner radius of this node (spec
    /// "Owner"), the slot of the least recently heard relay beyond it: the
    /// owner rule reads from the table that no relay that near is closer to
    /// a destination, so one turned away would have its messages claimed
    /// by its neighbour (roadmap D68).
    fn near_victim(&self, new: &Neighbour) -> Option<usize> {
        let r2 = OWNER_RADIUS_M * OWNER_RADIUS_M;
        let near = |n: &Neighbour| n.local.distance2(Default::default()) <= r2;
        if !new.holds_mail() || !near(new) {
            return None;
        }
        self.slots()
            .filter(|(_, n)| Some(n.id) != self.pinned && !near(n))
            .min_by_key(|(_, n)| n.last_heard)
            .map(|(i, _)| i)
    }
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
    use crate::tables::DEMOTE_AFTER_FAILURES;

    fn params() -> NeighbourParams {
        NeighbourParams::from_config(&Config::default())
    }

    /// Neighbour `i`'s heartbeat, its position and ID following from `i`.
    fn hb(i: i32, home: u8) -> (Heartbeat, Position) {
        let hb = Heartbeat {
            sleepy: false,
            anchorless: false,
            carried: false,
            home: Channel::new(home).unwrap(),
            format: crate::wire::FORMAT,
            map_point: false,
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
            carried: false,
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
        NodeId::from_u64(i as u64 + 1)
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

    /// A full table of live relays turns a newcomer away, but not one
    /// within the owner radius: it takes the place of the least recently
    /// heard relay beyond it (spec "Neighbour table", "Owner").
    #[test]
    fn a_relay_within_the_owner_radius_finds_a_place() {
        let mut t = Neighbours::new(params());
        // Far ones, a count of latitude and longitude about 0.85 m apart.
        for k in 0..NEIGHBOURS as i32 {
            assert!(hear(&mut t, 2000 + k, 0, -90, at(1 + k as u64))
                .new_occupant()
                .is_some());
        }
        assert_eq!(hear(&mut t, 3000, 0, -90, at(100)), Heard::Rejected);
        let near = hear(&mut t, 10, 0, -90, at(100));
        assert!(
            matches!(near, Heard::Replaced(_, old) if old.id == id(2000)),
            "{near:?}"
        );
        // Near ones are not made way for.
        for k in 11..20 {
            hear(&mut t, k, 0, -90, at(101));
        }
        assert!(t.get(id(10)).is_some());
        assert_eq!(hear(&mut t, 3001, 0, -90, at(102)), Heard::Rejected);
    }

    #[test]
    fn params_follow_the_design() {
        let p = params();
        assert_eq!(p.demote_after, 3);
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
    /// rotations and a beat's lag after its last heartbeat: 26 beat
    /// periods of 600 / 13 s, and a long full frame, the ACK timeout and
    /// two backoffs, 0.87 s.
    #[test]
    fn anchorless_entries_expire_after_two_rotations() {
        let mut t = Neighbours::new(params());
        assert_eq!(
            params().anchorless_expire_after,
            Duration::from_micros(1_200_872_796)
        );
        hear_anchorless(&mut t, 0, -80, at(0));
        hear(&mut t, 1, 0, -80, at(1000));
        assert!(t.get(id(0)).unwrap().anchorless);
        assert_eq!(t.live(at(1200)).count(), 2);
        assert_eq!(t.live(at(1201)).count(), 1);
        assert_eq!(
            t.next_expiry(),
            Some(Instant::ZERO + params().anchorless_expire_after)
        );
    }

    /// An anchorless neighbour stays anchorless once heard so in two
    /// consecutive rotations of its beats: not twice within half a
    /// rotation, 300 s, not across a rotation missed, more than 600 s and a
    /// heartbeat's lag, and no more once heard with the sleepy bit alone.
    #[test]
    fn an_anchorless_neighbour_stays_anchorless_over_two_rotations() {
        let mut t = Neighbours::new(params());
        let (min, max) = params().anchorless_rotations();
        assert_eq!(min, Duration::from_micros(299_999_999));
        assert_eq!(max, Duration::from_micros(600_872_798));
        let beat = |t: &mut Neighbours, s: u64| {
            hear_anchorless(t, 0, -80, at(s));
            t.beat_heard(id(0), Channel::new(0).unwrap(), at(s));
            t.get(id(0)).unwrap().stays_anchorless()
        };
        assert!(!beat(&mut t, 0), "once");
        assert!(!beat(&mut t, 200), "twice in one rotation");
        assert!(beat(&mut t, 500), "a rotation later");
        assert!(beat(&mut t, 1000));
        hear_sleepy(&mut t, 0, -80, at(1100));
        assert!(!t.get(id(0)).unwrap().stays_anchorless(), "anchored");
        assert!(!beat(&mut t, 1200), "anchorless again: once");
        assert!(!beat(&mut t, 1900), "a rotation missed");
        assert!(beat(&mut t, 2400));
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

    /// The service time starts at a ban and takes each take's interval
    /// at a weight of one in 39, counted from the later of the last take
    /// and when the message began to wait, less what the neighbour's
    /// refusals told (spec "Neighbour table").
    #[test]
    fn the_service_time_averages_the_takes() {
        let mut t = Neighbours::new(params());
        hear(&mut t, 1, 0, -80, at(0));
        let service = |t: &Neighbours| t.get(id(1)).unwrap().service();
        assert!(BAN - service(&t) < SERVICE_UNIT.mul(1024 / 1000 + 1));
        // Waited 40 bans from its queueing, nothing told.
        let start = at(10);
        let took = start + BAN.mul(40);
        t.took(id(1), start, took);
        let after = service(&t);
        let want = BAN + (BAN.mul(40) - BAN).div(SERVICE_TAKES);
        assert!(after.as_micros().abs_diff(want.as_micros()) < 2 * SERVICE_UNIT.as_micros());
        // Idle since that take: the next counts from its queueing, and a
        // refusal's wait is not this relay's time.
        t.told(id(1), BAN.mul(10));
        let queued = took + Duration::from_secs(600);
        t.took(id(1), queued, queued + BAN.mul(10));
        let unchanged = after - (after - Duration::ZERO).div(SERVICE_TAKES);
        assert!(
            service(&t).as_micros().abs_diff(unchanged.as_micros()) < 2 * SERVICE_UNIT.as_micros()
        );
        // Never past the longest told wait.
        for k in 0..400 {
            t.took(
                id(1),
                at(1000 + k),
                at(1000 + k) + Duration::from_secs(3600),
            );
        }
        assert!(service(&t) <= LONGEST_TOLD);
    }

    /// Senders refused for one way stand in its line, each a service time
    /// after the one before, a ban at least, or after the queue ahead of
    /// them if that ends later, the message that bars them raising it when
    /// it has waited longer; time shortens the line, and no place passes
    /// its limit (spec "Bans").
    #[test]
    fn places_in_a_line_are_a_service_time_apart() {
        let mut t = Neighbours::new(params());
        hear(&mut t, 1, 0, -80, at(0));
        let now = at(10);
        let limit = BAN.mul(100);
        let place = |t: &mut Neighbours, queue, oldest, now| {
            t.place_in_line(id(1), queue, oldest, limit, now).unwrap().0
        };
        let first = place(&mut t, Duration::ZERO, None, now);
        assert_eq!(first, BAN.max(t.get(id(1)).unwrap().service()));
        let second = place(&mut t, Duration::ZERO, None, now);
        assert_eq!(second, first + BAN);
        // Two bans later the line has moved up by two: it is empty.
        let later = now + BAN.mul(2);
        assert_eq!(place(&mut t, Duration::ZERO, None, later), first);
        // The message barring the way has waited 5 bans: that spaces it.
        let (third, spaced) = t
            .place_in_line(
                id(1),
                Duration::ZERO,
                Some(later - BAN.mul(5)),
                limit,
                later,
            )
            .unwrap();
        assert_eq!(spaced, BAN.mul(5));
        assert_eq!(third, first + BAN.mul(5));
        // A queue that ends later than the line places the sender after it.
        assert_eq!(place(&mut t, BAN.mul(40), None, later), BAN.mul(40));
        assert_eq!(place(&mut t, Duration::ZERO, None, later), BAN.mul(41));
        for _ in 0..100 {
            place(&mut t, Duration::ZERO, None, later);
        }
        assert_eq!(place(&mut t, Duration::ZERO, None, later), limit);
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
        assert_eq!(t.failed(p, at(1)), Some(false));
        t.answered(p, at(1));
        assert_eq!(
            t.failed(p, at(1)),
            Some(false),
            "an ACK resets: failures are consecutive"
        );
        assert_eq!(t.failed(p, at(2)), Some(false));
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
        assert_eq!(t.failed(p, at(393)), Some(false));
        assert_eq!(t.failed(p, at(394)), Some(true));
        // An ACK lifts it at once.
        t.answered(p, at(394));
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
        for _ in 0..DEMOTE_AFTER_FAILURES {
            t.failed(id(40), at(100));
        }
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

    /// The sections' images are kept once, at most [`IMAGES`]: a newer one
    /// takes the place of the lowest version, whose namers then name none,
    /// and an older one than all kept is kept by no entry.
    #[test]
    fn the_images_named_are_kept_once_and_at_most_four() {
        use crate::update::{Image, ImageTag, Section};
        let image = |v: u32| Image {
            name: ImageTag([v as u8; 4]),
            version: v,
            blocks: 26,
            last: 61,
            key: 1,
        };
        let section = |v: u32| Section {
            runs: 1,
            image: Some(image(v)),
            complete: 3,
            have: 9,
        };
        let mut t = Neighbours::new(params());
        for i in 1..=7 {
            hear(&mut t, i, 1, -80, at(0));
        }
        let named = |t: &Neighbours, i: i32| t.section(t.get(id(i)).unwrap()).and_then(|s| s.image);
        assert_eq!(t.section(t.get(id(1)).unwrap()), None);
        for (i, v) in [(1, 5), (2, 5), (3, 6), (4, 7), (5, 8)] {
            t.section_heard(id(i), section(v));
        }
        assert_eq!(t.section(t.get(id(1)).unwrap()), Some(section(5)));
        assert_eq!(named(&t, 2), Some(image(5)));
        // A fifth, newer: version 5's place, and its namers name none.
        t.section_heard(id(6), section(9));
        assert_eq!(named(&t, 6), Some(image(9)));
        assert_eq!(named(&t, 1), None);
        assert_eq!(named(&t, 2), None);
        assert_eq!(t.section(t.get(id(1)).unwrap()).unwrap().complete, 3);
        // An older one than every image kept is kept by none.
        t.section_heard(id(7), section(4));
        assert_eq!(named(&t, 7), None);
        // A place no entry names is free again.
        t.section_heard(id(3), section(8));
        t.section_heard(id(7), section(4));
        assert_eq!(named(&t, 7), Some(image(4)));
        assert_eq!(named(&t, 3), Some(image(8)));
    }
}
