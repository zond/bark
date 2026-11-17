//! Firmware images spreading over the mesh (spec "Updates over the mesh"):
//! the heartbeat's section, gathering an image's pieces, and streaming the
//! blocks this node holds to the neighbours that lack them. `Spread` holds
//! the state and reads the rest of the node through a `View`; the node's
//! methods at the end wire it in.

use super::radio::Radio;
use super::{Job, Node, Stage};
use crate::event::{Action, Frame, SlotPiece, Trace};
use crate::geo::Point;
use crate::ids::NodeId;
use crate::node::{heartbeat_interval, heartbeat_lag, mac};
use crate::node::{Config, Rng};
use crate::position::Position;
use crate::radio::{Channel, CHANNELS, MIN_PREAMBLE_SYMBOLS};
use crate::status::Gathering;
use crate::tables::budget::REPLY_RESERVE;
use crate::tables::heard::beat_period;
use crate::tables::neighbours::{Neighbour, Neighbours};
use crate::time::{Duration, Instant};
use crate::update::{
    fed_within, sweep_pieces, unproven_within, Image, Piece, Section, FULL_FRAME, GAP,
    LEASE_ROTATIONS, MAX_BLOCKS, PACE, PASS, PIECES, PROOF_BLOCKS, SECTION_EVERY, SERVED_CHANNELS,
    SLOT_LISTEN,
};
use crate::wire::{Packet, Update, PIECE_BYTES};

/// How long after its slot heartbeat, or a piece of its run, a relay may
/// still start the check for the next frame for its sleepy nodes: at once.
/// The bench's 8.0 to 19.4 ms from one piece's end to the next's start were
/// of checks started in the event that ended the transmission; a later
/// start adds to them, and [`SLOT_LISTEN`]'s 25 ms leaves 5.6 ms beyond the
/// longest. 1 ms is well within that.
const SLOT_START: Duration = Duration::from_millis(1);

/// Other senders' pieces a node remembers, to count them against its own
/// pace on its home channel.
const OTHERS: usize = 8;

/// Versions a node holds off at once: one given up unproven or whose
/// proof failed, and those it left unproven for another while it found
/// the one whose proof comes.
const HELD_OFF: usize = 4;

/// What a section tells that starts a rotation of it when it changes: the
/// version the node runs, its newest image and the blocks of it held from
/// the first.
type Told = (u32, Option<u32>, u8);

const ALL_CHANNELS: u16 = (1 << CHANNELS) - 1;

fn bit(c: Channel) -> u16 {
    1 << c.index()
}

/// What a node that gathers an image keeps.
#[derive(Clone, Copy, Debug)]
struct Gather {
    image: Image,
    /// The first block it lacks.
    block: u8,
    /// The pieces of each block handed to the driver, a bit each.
    seen: [[u8; PIECES.div_ceil(8)]; MAX_BLOCKS],
    /// The blocks that decoded and passed their hash, a bit each.
    done: u64,
    /// When it began or last took a piece, for [`fed_within`] and
    /// [`unproven_within`].
    fed: Instant,
}

impl Gather {
    fn holds(&self, b: u8) -> bool {
        b < 64 && self.done >> b & 1 == 1
    }

    /// Whether it holds the blocks that prove the image, which its driver
    /// checked before it said the second of them passed.
    fn proven(&self) -> bool {
        (0..PROOF_BLOCKS).all(|b| self.holds(b))
    }

    /// Whether piece `index` of block `b` is new for a block it lacks; it
    /// marks it.
    fn take(&mut self, b: u8, index: u8) -> bool {
        if b >= self.image.blocks || usize::from(b) >= MAX_BLOCKS || self.holds(b) {
            return false;
        }
        let (byte, bit) = (usize::from(index / 8), 1 << (index % 8));
        let seen = &mut self.seen[usize::from(b)];
        let new = seen[byte] & bit == 0;
        seen[byte] |= bit;
        new
    }

    /// Block `b` decoded, or failed its hash (`!ok`) and its pieces are
    /// taken again.
    fn decoded(&mut self, b: u8, ok: bool) {
        if b >= self.image.blocks || usize::from(b) >= MAX_BLOCKS || self.holds(b) {
            return;
        }
        if !ok {
            self.seen[usize::from(b)] = [0; PIECES.div_ceil(8)];
            return;
        }
        self.done |= 1 << b;
        while self.block < self.image.blocks && self.holds(self.block) {
            self.block += 1;
        }
    }
}

/// What the node keeps of the images spreading
/// (spec "Updates over the mesh", `docs/core-interface.md`, "Memory").
pub(super) struct Spread {
    own: Option<Image>,
    /// The version of the last image offered to the driver.
    offered: Option<u32>,
    gather: Option<Gather>,
    /// Versions offered no more until a time: given up unproven, left
    /// unproven for another, or refused by the driver.
    held_off: [Option<(u32, Instant)>; HELD_OFF],
    /// The last channels it sent a piece on, and when: the only ones that
    /// can still be within their pace, since a pace holds at most this
    /// many.
    sent: [Option<(Channel, Instant)>; SERVED_CHANNELS],
    /// Frames of the send queue sent while a piece was due.
    passed: u8,
    /// On each channel, the block its sweep is at (`u8::MAX` before the
    /// first), the pieces of it sent, and the pieces of it to send.
    cur: [u8; CHANNELS as usize],
    sent_cur: [u8; CHANNELS as usize],
    target: [u8; CHANNELS as usize],
    /// The piece of each block it streams next: from a random start,
    /// climbing, so a block's pieces repeat only after [`PIECES`].
    index: [u8; MAX_BLOCKS],
    /// The channels, a bit each, whose next heartbeat carries its section.
    section_to: u16,
    /// When its heartbeats next carry it for a rotation, if nothing changes.
    section_at: Instant,
    /// When the last such rotation began: an answer starts one at most a
    /// rotation after it.
    rotation_at: Option<Instant>,
    /// A neighbour's section named nothing as new as what it streams: its
    /// next heartbeat starts a rotation of its section.
    answer: bool,
    told: Option<Told>,
    /// The neighbour it last took a piece from, as far as it can tell: a
    /// piece does not name its sender. Its section goes on that one's
    /// home channel every rotation.
    helper: Option<NodeId>,
    /// Until when the frame after its slot heartbeat on its home channel
    /// may start, where its sleepy nodes listen.
    slot_open: Option<Instant>,
    /// The pieces still to go in the run after its slot heartbeat, the one
    /// under way among them.
    run_left: u8,
    /// When its last slot heartbeat went on its home channel.
    slot_at: Instant,
    /// When it last heard a heartbeat, or a piece that followed one in a
    /// run: a piece that ends within [`SLOT_LISTEN`] of it goes on the run.
    beat_heard: Instant,
    /// A sleepy node heard its anchor's slot heartbeat, and has yet to
    /// decide whether it listens for the run.
    slot_heard: bool,
    /// When it last heard other nodes' pieces on its home channel, the
    /// last few: they share the channel's pace with it.
    others: [Instant; OTHERS],
    /// The channel of the piece under way, and its checks.
    current: Option<Channel>,
    busy_checks: u8,
    /// Not before this: a listen-before-talk backoff or the bucket.
    ready_at: Instant,
}

impl Spread {
    pub(super) const fn new() -> Spread {
        Spread {
            own: None,
            offered: None,
            gather: None,
            held_off: [None; HELD_OFF],
            sent: [None; SERVED_CHANNELS],
            passed: 0,
            cur: [u8::MAX; CHANNELS as usize],
            sent_cur: [0; CHANNELS as usize],
            target: [0; CHANNELS as usize],
            index: [0; MAX_BLOCKS],
            section_to: 0,
            section_at: Instant::ZERO,
            rotation_at: None,
            answer: false,
            told: None,
            helper: None,
            slot_open: None,
            run_left: 0,
            slot_at: Instant::ZERO,
            beat_heard: Instant::ZERO,
            slot_heard: false,
            others: [Instant::ZERO; OTHERS],
            current: None,
            busy_checks: 0,
            ready_at: Instant::ZERO,
        }
    }
}

/// What a channel needs of what this node holds: the lowest block a
/// neighbour heard there lacks, how many lack something, and how many of
/// them are sleepy.
#[derive(Clone, Copy, Debug, Default)]
struct Need {
    from: u8,
    count: u8,
    sleepy: u8,
}

/// What the update reads of the rest of the node.
#[derive(Clone, Copy)]
pub(super) struct View<'a> {
    pub(super) neighbours: &'a Neighbours,
    pub(super) config: &'a Config,
    pub(super) home: Channel,
    /// Its own position: a sleepy node whose address it is, is its own.
    pub(super) position: Position,
    pub(super) id: NodeId,
    /// It streams: a relay past its first rotation.
    pub(super) spreads: bool,
    pub(super) sleepy: bool,
    /// Its send queue is empty.
    pub(super) idle: bool,
}

/// The [`View`] of `node`, borrowing only the fields it reads, so that
/// the spread can be changed beside it.
macro_rules! view {
    ($node:expr) => {
        View {
            neighbours: &$node.neighbours,
            config: &$node.config,
            home: $node.home,
            position: $node.config.position,
            id: $node.id,
            spreads: $node.stage == Stage::Running && !$node.sleepy(),
            sleepy: $node.sleepy(),
            idle: $node.queue.is_empty(),
        }
    };
}

impl Spread {
    pub(super) fn image(&self) -> Option<Image> {
        self.own
    }

    pub(super) fn gathering(&self) -> Option<(Image, u8)> {
        self.gather.map(|g| (g.image, g.block))
    }

    /// Its section; `version` is its build's, 0 for none, when the image
    /// it holds says what it runs.
    fn section(&self, version: u32) -> Option<Section> {
        let runs = match version {
            0 => self.own.map_or(0, |o| o.version),
            v => v,
        };
        let (image, complete, have) = match (self.gather, self.own) {
            (Some(g), _) => (Some(g.image), g.block, Self::have(&g)),
            (None, Some(o)) => (Some(o), o.blocks, 0),
            (None, None) if runs == 0 => return None,
            (None, None) => (None, 0, 0),
        };
        Some(Section {
            runs,
            image,
            complete,
            have,
        })
    }

    fn have(g: &Gather) -> u8 {
        let have: u32 = g
            .seen
            .get(usize::from(g.block))
            .map_or(0, |b| b.iter().map(|x| x.count_ones()).sum());
        have.min(255) as u8
    }

    pub(super) fn gathering_status(&self) -> Gathering {
        self.gather.map_or(Gathering::default(), |g| Gathering {
            version: g.image.version,
            blocks: g.image.blocks,
            complete: g.block,
            have: Self::have(&g),
        })
    }

    /// The image it streams: the one it gathers once it is proven, else
    /// the one it holds whole. Nothing of an image that may be forged goes
    /// on.
    fn streams(&self) -> Option<Image> {
        self.proven().map(|g| g.image).or(self.own)
    }

    fn proven(&self) -> Option<Gather> {
        self.gather.filter(Gather::proven)
    }

    fn holds_block(&self, b: u8) -> bool {
        match self.proven() {
            Some(g) => g.holds(b),
            None => self.own.is_some_and(|o| b < o.blocks),
        }
    }

    /// What its section tells that starts a rotation of it when it
    /// changes; the pieces of the next block may be stale, which the
    /// loss margin covers.
    fn told_now(&self, version: u32) -> Option<Told> {
        self.section(version)
            .map(|t| (t.runs, t.image.map(|i| i.version), t.complete))
    }

    fn rotation_due(&self, version: u32, now: Instant) -> bool {
        self.told_now(version) != self.told || now >= self.section_at || self.answer
    }

    /// The section a heartbeat sent on `channel` at `now` carries, if any
    /// (spec "Updates over the mesh"). A heartbeat skipped for a busy
    /// channel leaves its channel's bit for the next.
    pub(super) fn heartbeat_section(
        &self,
        v: &View<'_>,
        now: Instant,
        channel: Channel,
    ) -> Option<Section> {
        let version = v.config.version;
        if self.serves_sleepy(v, now, channel) {
            return self.streamed(version);
        }
        let due = self.rotation_due(version, now)
            || self.section_to & bit(channel) != 0
            || self.restates_on(v, now) == Some(channel);
        self.section(version).filter(|_| due)
    }

    /// Its section as the image it streams names it: the image it holds
    /// whole in place of a gather not yet proven, which its sleepy nodes
    /// could take nothing of.
    fn streamed(&self, version: u32) -> Option<Section> {
        match (self.gather, self.own) {
            (Some(g), Some(own)) if !g.proven() => Some(Section {
                image: Some(own),
                complete: own.blocks,
                have: 0,
                ..self.section(version)?
            }),
            _ => self.section(version),
        }
    }

    /// Whether a heartbeat on `channel` is the one its sleepy nodes hear
    /// while one of them lacks a block it holds: it then says what it
    /// streams them, since they hear no other of its heartbeats and listen
    /// for the piece after this one only if they know.
    fn serves_sleepy(&self, v: &View<'_>, now: Instant, channel: Channel) -> bool {
        channel == v.home && self.needs(v, now)[usize::from(v.home.index())].sleepy > 0
    }

    /// A heartbeat with the section went on `channel`: a rotation of them
    /// starts now if one is due. A sleepy node's rotation is its one beat,
    /// which its anchor hears.
    pub(super) fn section_sent(
        &mut self,
        version: u32,
        sleepy: bool,
        now: Instant,
        channel: Channel,
    ) {
        if self.rotation_due(version, now) {
            self.told = self.told_now(version);
            self.section_at = now + SECTION_EVERY;
            self.section_to = ALL_CHANNELS;
            self.rotation_at = Some(now);
            self.answer = false;
        }
        self.section_to &= match sleepy {
            true => 0,
            false => !bit(channel),
        };
    }

    fn rotation(v: &View<'_>) -> Duration {
        heartbeat_interval(v.id, v.config).mul(u64::from(CHANNELS))
    }

    /// Where a node that gathers says so again each rotation: on the home
    /// channel of the helper it last took a piece from, or before its
    /// first piece of the best holder it hears; a sleepy node on its
    /// anchor's, where all its beats go.
    fn restates_on(&self, v: &View<'_>, now: Instant) -> Option<Channel> {
        let g = self.gather.filter(|g| g.block < g.image.blocks)?;
        if v.sleepy {
            return Some(v.home);
        }
        let helper = self
            .helper
            .and_then(|id| v.neighbours.get(id))
            .or_else(|| self.holder_of(v, g.block, now))?;
        Some(helper.home)
    }

    /// The neighbour whose stream carries block `b` of the image it
    /// gathers to it, by the helpers' own rule: a relay at home on its
    /// channel that holds the block, else the best holder elsewhere, most
    /// blocks from the first, then the lowest ID.
    fn holder_of<'a>(&self, v: &View<'a>, b: u8, now: Instant) -> Option<&'a Neighbour> {
        let image = self.gather?.image;
        v.neighbours
            .live(now)
            .map(|(_, m)| m)
            .filter(|m| !m.sleepy && Self::complete_of(v, m, image) > b)
            .max_by_key(|m| {
                (
                    m.home == v.home,
                    Self::complete_of(v, m, image),
                    core::cmp::Reverse(m.id.0),
                )
            })
    }

    /// `AppCommand::Holds`.
    pub(super) fn holds_image(&mut self, image: Image, rng: &mut Rng, now: Instant) {
        if self.own == Some(image) {
            return;
        }
        self.random_starts(rng);
        self.own = Some(image);
        if self
            .gather
            .is_some_and(|g| g.image.version <= image.version)
        {
            self.gather = None;
        }
        self.section_at = now;
    }

    /// `AppCommand::Gather`: the node gathers `image`, holding the blocks
    /// before `from` already, proven if they are the proof's.
    pub(super) fn gather(
        &mut self,
        config: &Config,
        image: Image,
        from: u8,
        rng: &mut Rng,
        now: Instant,
    ) {
        if usize::from(image.blocks) > MAX_BLOCKS {
            return;
        }
        // An unproven image left for an older one is held off, or the two
        // would take turns.
        if let Some(old) = self.gather.filter(|g| !g.proven()) {
            if old.image.version > image.version {
                self.hold_off(config, old.image.version, now);
            }
        }
        let mut g = Gather {
            image,
            block: 0,
            seen: [[0; PIECES.div_ceil(8)]; MAX_BLOCKS],
            done: 0,
            fed: now,
        };
        for b in 0..from.min(image.blocks) {
            g.decoded(b, true);
        }
        self.random_starts(rng);
        self.gather = Some(g);
        self.section_at = now;
    }

    /// Each block's pieces from a random start, so that two holders of it
    /// seldom send the same, and every sweep from the first block.
    fn random_starts(&mut self, rng: &mut Rng) {
        self.cur = [u8::MAX; CHANNELS as usize];
        self.sent_cur = [0; CHANNELS as usize];
        self.target = [0; CHANNELS as usize];
        for index in &mut self.index {
            *index = rng.range(0, PIECES as u64 - 1) as u8;
        }
    }

    pub(super) fn block_done(&mut self, index: u8, ok: bool) {
        if let Some(g) = self.gather.as_mut() {
            g.decoded(index, ok);
        }
    }

    /// The node gathers the image of `version` no more, and it may be
    /// offered again.
    fn drop_gather(&mut self, version: u32, now: Instant) {
        if self.gather.is_some_and(|g| g.image.version == version) {
            self.gather = None;
            self.helper = None;
            self.section_at = now;
        }
        if self.offered == Some(version) {
            self.offered = None;
        }
    }

    /// `AppCommand::Rejected`: its proof or its staging failed, and it takes
    /// no offer of it for [`unproven_within`].
    pub(super) fn rejected(&mut self, config: &Config, version: u32, now: Instant) {
        self.drop_gather(version, now);
        self.hold_off(config, version, now);
    }

    /// When the gather, if unproven, is given up.
    pub(super) fn expires_at(&self, config: &Config) -> Option<Instant> {
        let g = self.gather.filter(|g| !g.proven())?;
        Some(g.fed + unproven_within(config))
    }

    /// An unproven gather that has taken no piece for [`unproven_within`]
    /// is given up and held off, and the driver told to forget its pieces:
    /// a section can name an image nobody holds.
    pub(super) fn expire(&mut self, config: &Config, now: Instant, emit: &mut dyn FnMut(Action)) {
        let Some(g) = self
            .gather
            .filter(|g| !g.proven() && now >= g.fed + unproven_within(config))
        else {
            return;
        };
        let version = g.image.version;
        self.drop_gather(version, now);
        self.hold_off(config, version, now);
        emit(Action::Unproven(version));
    }

    fn hold_off(&mut self, config: &Config, version: u32, now: Instant) {
        let until = now + unproven_within(config);
        let slot = self
            .held_off
            .iter()
            .position(|h| h.is_none_or(|(v, end)| v == version || end <= now))
            .unwrap_or_else(|| {
                let first = self.held_off.iter().flatten().map(|h| h.1).min();
                self.held_off
                    .iter()
                    .position(|h| h.map(|h| h.1) == first)
                    .unwrap_or(0)
            });
        self.held_off[slot] = Some((version, until));
        // Offered again once the hold-off ends.
        if self.offered == Some(version) {
            self.offered = None;
        }
    }

    fn held_off(&self, version: u32, now: Instant) -> bool {
        self.held_off
            .iter()
            .flatten()
            .any(|&(v, end)| v == version && now < end)
    }

    /// A heartbeat carried `section` (spec "Updates over the mesh"): an
    /// image its sender holds the proof of is offered to the driver's
    /// policy once if it is newer than what this node holds or has proven,
    /// and a neighbour that lags is answered with a rotation of this
    /// node's section. Only a sender that may stream to it offers: any
    /// relay, but to a sleepy node its anchor alone.
    pub(super) fn section_heard(
        &mut self,
        v: &View<'_>,
        section: Section,
        streams_here: bool,
        now: Instant,
        emit: &mut dyn FnMut(Action),
    ) {
        let told = section.image.map_or(0, |i| i.version).max(section.runs);
        let lags = self
            .streams()
            .is_some_and(|i| v.spreads && told < i.version);
        let rotated = self
            .rotation_at
            .is_some_and(|t| now < t + Self::rotation(v));
        if lags && !rotated {
            self.answer = true;
        }
        let Some(image) = section.offers().filter(|_| streams_here) else {
            return;
        };
        if self.offered == Some(image.version)
            || self.own.is_some_and(|o| image.version <= o.version)
            || usize::from(image.blocks) > MAX_BLOCKS
            || self.held_off(image.version, now)
        {
            return;
        }
        // A gather that is fed holds its place against any claim. One that
        // is not gives way: unproven, to an older image, since it may be
        // named by nobody who holds it; proven, to a newer one, since its
        // holders may have moved on to that.
        if let Some(g) = self.gather {
            let fed = now < g.fed + fed_within(v.config);
            let newer = image.version > g.image.version;
            let stays = match g.proven() {
                true => fed || !newer,
                false => fed && !newer,
            };
            if g.image.version == image.version || stays {
                return;
            }
        }
        self.offered = Some(image.version);
        emit(Action::Offered(image));
    }

    /// Its slot heartbeat went on its home channel: a relay whose sleepy
    /// nodes there gather what it holds sends them a run of its stream's
    /// pieces at once, while they listen: the pieces the pace gives its
    /// home channel in a rotation, less those it heard others send there
    /// since its last slot heartbeat, and one at least.
    pub(super) fn slot_heartbeat(&mut self, v: &View<'_>, now: Instant) {
        let home = self.needs(v, now)[usize::from(v.home.index())];
        if home.sleepy > 0 {
            let paced = Self::rotation(v).as_micros() / PACE.as_micros();
            let others = self.others.iter().filter(|&&o| o > self.slot_at).count() as u64;
            self.run_left = paced.saturating_sub(others).clamp(1, u64::from(u8::MAX)) as u8;
            self.slot_open = Some(now + SLOT_START);
            // A wait for a pace that had nothing to send ends: this run
            // is its own chance, and the bucket is asked again.
            self.ready_at = self.ready_at.min(now);
        }
        self.slot_at = now;
    }

    /// A piece's transmission ended: the next of its run may start at
    /// once, while the sleepy nodes still listen.
    pub(super) fn piece_done(&mut self, now: Instant) {
        if self.run_left > 0 {
            self.slot_open = Some(now + SLOT_START);
            self.ready_at = self.ready_at.min(now);
        }
    }

    /// A sleepy node heard its anchor's slot heartbeat: it decides whether
    /// it listens once the heartbeat's section is taken.
    pub(super) fn slot_heard(&mut self) {
        self.slot_heard = true;
    }

    /// Whether its anchor's slot heartbeat was just heard, once.
    pub(super) fn take_slot_heard(&mut self) -> bool {
        core::mem::take(&mut self.slot_heard)
    }

    /// A sleepy node heard its anchor's slot heartbeat, the anchor `n`:
    /// whether it listens for the piece after it, as it does while it
    /// gathers and its anchor holds a block it lacks.
    pub(super) fn helped_by(&self, neighbours: &Neighbours, anchor: Option<&Neighbour>) -> bool {
        let Some(g) = self.gather else {
            return false;
        };
        anchor
            .and_then(|n| neighbours.section(n))
            .and_then(|u| u.image.map(|i| (i, u.complete)))
            .is_some_and(|(i, complete)| i.version == g.image.version && complete > g.block)
    }

    /// A heartbeat was heard: a piece that ends within [`SLOT_LISTEN`] is
    /// its sender's piece after the slot.
    pub(super) fn beat_heard(&mut self, now: Instant) {
        self.beat_heard = now;
    }

    /// A piece was heard; whether it followed a heartbeat or a piece of
    /// its run within [`SLOT_LISTEN`], so that the next may follow it.
    pub(super) fn update_heard(
        &mut self,
        v: &View<'_>,
        u: Update<'_>,
        now: Instant,
        emit: &mut dyn FnMut(Action),
    ) -> bool {
        let Update::Piece {
            version,
            block,
            index,
            bytes,
        } = u;
        // Another node sends here: it shares the channel's pace.
        let o = &mut self.others;
        o.rotate_right(1);
        o[0] = now;
        let run = now.since(self.beat_heard) <= SLOT_LISTEN;
        if run {
            self.beat_heard = now;
        }
        if usize::from(index) >= PIECES {
            return run;
        }
        let Some(g) = self.gather.as_mut().filter(|g| g.image.version == version) else {
            return run;
        };
        if !g.take(block, index) {
            return run;
        }
        g.fed = now;
        let mut piece = Piece {
            block,
            index,
            bytes: [0; PIECE_BYTES],
        };
        piece.bytes.copy_from_slice(bytes);
        emit(Action::StorePiece(piece));
        self.helper = self.holder_of(v, block, now).map(|m| m.id);
        run
    }

    /// The blocks from the first of the image it streams that `m` holds,
    /// by its section.
    fn complete_of(v: &View<'_>, m: &Neighbour, image: Image) -> u8 {
        v.neighbours
            .section(m)
            .filter(|u| u.image.is_some_and(|i| i.version == image.version))
            .map_or(0, |u| u.complete)
    }

    /// Whether this node streams to the gatherer `n` on `n`'s channel. To
    /// a sleepy node only as its anchor, the one relay whose slot
    /// heartbeats it wakes for, known by its address. To another whose home
    /// is this node's, always. Elsewhere, unless a better holder serves it:
    /// a relay this node hears, at home on another channel than `n`, that
    /// holds more blocks from the first than `n`, lies no farther from `n`
    /// than this node, and holds more of them than this node, or as many
    /// with a lower ID (spec "Updates over the mesh").
    fn streams_to(&self, v: &View<'_>, n: &Neighbour, image: Image, now: Instant) -> bool {
        if n.sleepy {
            return n.position == v.position;
        }
        if n.home == v.home {
            return true;
        }
        let Some(u) = v.neighbours.section(n) else {
            return false;
        };
        let mine = self.section(v.config.version).map_or(0, |s| s.complete);
        let far = n.local.distance2(Point::default());
        !v.neighbours.live(now).any(|(_, m)| {
            let k = Self::complete_of(v, m, image);
            !m.sleepy
                && m.home != n.home
                && k > u.complete
                && (k > mine || k == mine && m.id.0 < v.id.0)
                && m.local.distance2(n.local) <= far
        })
    }

    /// The section of `n` if it gathers `image` and was heard within its
    /// lease, [`LEASE_ROTATIONS`] of `n`'s rotations and a heartbeat's lag,
    /// a sleepy node's rotation being its beat: a stream to a gatherer that
    /// fell silent, moved away or completed then stops within that time,
    /// and one whose restatement was lost once goes on
    /// (spec "Updates over the mesh").
    fn gatherer(v: &View<'_>, n: &Neighbour, image: Image, now: Instant) -> Option<Section> {
        let rotation = match n.sleepy {
            true => beat_period(v.config.delivery_ttl),
            false => heartbeat_interval(n.id, v.config).mul(u64::from(CHANNELS)),
        };
        let lease = rotation.mul(LEASE_ROTATIONS) + heartbeat_lag(v.config);
        let at = Neighbours::section_at(n)?;
        if now > at + lease {
            return None;
        }
        v.neighbours.section(n).filter(|u| u.gathers(image.version))
    }

    fn needs(&self, v: &View<'_>, now: Instant) -> [Need; CHANNELS as usize] {
        let mut need = [Need::default(); CHANNELS as usize];
        let Some(image) = self.streams() else {
            return need;
        };
        for (_, n) in v.neighbours.live(now) {
            let Some(u) = Self::gatherer(v, n, image, now) else {
                continue;
            };
            if !self.streams_to(v, n, image, now) {
                continue;
            }
            let lacks = |b: u8| b > u.complete || u.have < image.pieces(b);
            let Some(from) = (u.complete..image.blocks).find(|&b| self.holds_block(b) && lacks(b))
            else {
                continue;
            };
            let c = &mut need[usize::from(n.home.index())];
            if c.count == 0 {
                *c = Need {
                    from,
                    count: 0,
                    sleepy: 0,
                };
            }
            c.from = c.from.min(from);
            c.count = c.count.saturating_add(1);
            c.sleepy = c.sleepy.saturating_add(u8::from(n.sleepy));
        }
        need
    }

    /// The channels it streams on: its home, then those whose neighbours
    /// lack the most of what it holds, at most [`SERVED_CHANNELS`].
    fn stream_channels(
        &self,
        v: &View<'_>,
        now: Instant,
    ) -> [Option<(Channel, Need)>; SERVED_CHANNELS] {
        let need = self.needs(v, now);
        let mut out = [None; SERVED_CHANNELS];
        for c in Channel::all() {
            let n = need[usize::from(c.index())];
            if n.count == 0 {
                continue;
            }
            let better = |o: &Option<(Channel, Need)>| match o {
                None => true,
                Some((oc, on)) => (c == v.home, n.count) > (*oc == v.home, on.count),
            };
            if let Some(i) = out.iter().position(better) {
                out[i..].rotate_right(1);
                out[i] = Some((c, n));
            }
        }
        out
    }

    /// The pieces of block `b` the neighbour on channel `c` furthest
    /// behind on it lacks, by their sections: all of the block for one
    /// that lacks an earlier block, what it lacks of it for one at it.
    fn deficit(&self, v: &View<'_>, c: Channel, b: u8, now: Instant) -> u8 {
        let Some(image) = self.streams() else {
            return 0;
        };
        let k = image.pieces(b);
        v.neighbours
            .live(now)
            .filter(|(_, n)| n.home == c && self.streams_to(v, n, image, now))
            .filter_map(|(_, n)| Self::gatherer(v, n, image, now))
            .map(|u| match u.complete.cmp(&b) {
                core::cmp::Ordering::Greater => 0,
                core::cmp::Ordering::Equal => k.saturating_sub(u.have),
                core::cmp::Ordering::Less => k,
            })
            .max()
            .unwrap_or(0)
    }

    /// The block whose piece goes next on channel `c`: the block its sweep
    /// is at until it has sent what the neighbour furthest behind on it
    /// lacks and the loss margin, then the next block it holds that a
    /// neighbour there lacks, in order, and from the first again.
    fn stream_block(&self, v: &View<'_>, c: Channel, now: Instant) -> Option<u8> {
        let image = self.streams()?;
        let blocks = image.blocks;
        let i = usize::from(c.index());
        let cur = self.cur[i];
        let wanted = |b: u8| self.holds_block(b) && self.deficit(v, c, b, now) > 0;
        if self.sent_cur[i] < self.target[i] && wanted(cur) {
            return Some(cur);
        }
        if let Some(b) = self.tail(v, c, now) {
            return Some(b);
        }
        (cur.saturating_add(1)..blocks)
            .find(|&b| wanted(b))
            .or_else(|| (0..blocks.min(cur.saturating_add(1))).find(|&b| wanted(b)))
    }

    /// The block whose quota went on `c` while a neighbour there still
    /// lacks it, the lowest such: the stream goes on with fresh pieces of
    /// it, at the pace, until the neighbour's section shows it complete or
    /// its lease ends, so that a quota cut short by lost pieces never
    /// leaves the channel silent (spec "Updates over the mesh").
    fn tail(&self, v: &View<'_>, c: Channel, now: Instant) -> Option<u8> {
        let image = self.streams()?;
        let i = usize::from(c.index());
        let cur = self.cur[i];
        if cur == u8::MAX || self.sent_cur[i] < self.target[i] {
            return None;
        }
        let lowest =
            (0..image.blocks).find(|&b| self.holds_block(b) && self.deficit(v, c, b, now) > 0)?;
        (lowest == cur).then_some(cur)
    }

    /// Whether the channel's pace lets a piece go on `c` at `t`, and if not,
    /// when it will (spec "Updates over the mesh"): one a [`GAP`] or a
    /// [`PACE`] on each channel, on at most [`SERVED_CHANNELS`] in one pace.
    fn pace_allows(&self, v: &View<'_>, c: Channel, t: Instant) -> Instant {
        let mut at = t;
        for (d, x) in self.sent.iter().flatten() {
            if *d == c {
                // On its home channel it hears the others that send there,
                // and takes its share: one piece in k + 1 paces when k
                // others sent since its last.
                let k = if c == v.home {
                    self.others.iter().filter(|&&o| o > *x).count() as u64
                } else {
                    0
                };
                // The pace is a floor: with no other sender heard there
                // since its last piece and nothing else to send, the next
                // goes a gap after it.
                let alone = k == 0 && v.idle && self.tail(v, c, t).is_none();
                at = at.max(if alone {
                    *x + GAP
                } else {
                    *x + PACE.mul(k + 1)
                });
            }
        }
        let mut ends = [Instant::ZERO; SERVED_CHANNELS];
        let mut n = 0;
        for (d, x) in self.sent.iter().flatten() {
            if *d != c && *x + PACE > at {
                ends[n] = *x + PACE;
                n += 1;
            }
        }
        if n >= SERVED_CHANNELS {
            ends[..n].sort_unstable();
            at = at.max(ends[n - SERVED_CHANNELS]);
        }
        at
    }

    /// The channel of the next piece and when the pace lets it go. The
    /// run after the slot heartbeat is the home channel's pace for the
    /// rotation, sent together: while a sleepy node needs the stream
    /// there, it goes only then.
    fn next_out(&self, v: &View<'_>, now: Instant) -> Option<(Channel, Instant)> {
        let home = v.home;
        self.stream_channels(v, now)
            .into_iter()
            .flatten()
            .filter_map(|(c, need)| match self.slot_open {
                Some(_) if c == home => Some((c, now)),
                None if c == home && need.sleepy > 0 => None,
                _ => Some((c, self.pace_allows(v, c, now))),
            })
            .min_by_key(|x| x.1)
    }

    fn due_at(&self, v: &View<'_>, now: Instant) -> Option<Instant> {
        if !v.spreads {
            return None;
        }
        if let Some(c) = self.current {
            return Some(self.ready_at.max(self.pace_allows(v, c, now)));
        }
        if let Some(t) = self.slot_open {
            // The frame after the slot goes at once or not at all.
            return Some(t.max(self.ready_at));
        }
        self.next_out(v, now).map(|(_, t)| t.max(self.ready_at))
    }

    /// When the update job is next ready of its own accord: a time past
    /// is now. Asked with no time, it may name a time at which
    /// [`Spread::start`] then finds nothing; that one sets `ready_at` past
    /// the moment, so the node does not wake again for it.
    pub(super) fn ready_at(&self, v: &View<'_>) -> Option<Instant> {
        self.due_at(v, Instant::ZERO)
    }

    /// Whether a piece has waited behind [`PASS`] frames of the send
    /// queue, or is the frame after a slot heartbeat, and goes before the
    /// next.
    pub(super) fn first(&self) -> bool {
        self.passed >= PASS || self.slot_open.is_some()
    }

    /// A frame of the send queue went: it passed a piece that was due.
    pub(super) fn passed(&mut self, v: &View<'_>, now: Instant) {
        if self.due_at(v, now).is_some_and(|t| t <= now) {
            self.passed = self.passed.saturating_add(1);
        }
    }

    fn frame(&self, v: &View<'_>, c: Channel, now: Instant) -> Option<(Frame, u8)> {
        self.stream_channels(v, now)
            .into_iter()
            .flatten()
            .find(|(d, _)| *d == c)?;
        let block = self.stream_block(v, c, now)?;
        let image = self.streams()?;
        let packet = Update::Piece {
            version: image.version,
            block,
            index: self.index[usize::from(block)],
            bytes: &[0; PIECE_BYTES],
        };
        Some((
            mac::frame(&Packet::Update(packet), c, MIN_PREAMBLE_SYMBOLS),
            block,
        ))
    }

    /// Start the next piece that is due: its check of the channel, once
    /// the bucket holds it with the reply reserve left, as a forward must
    /// (spec "Duty cycle cap"); whether it took the radio.
    pub(super) fn start(
        &mut self,
        v: &View<'_>,
        radio: &mut Radio,
        rng: &mut Rng,
        now: Instant,
        emit: &mut dyn FnMut(Action),
    ) -> bool {
        if self.slot_open.is_some_and(|t| now > t) {
            // The frame after the slot heartbeat could not start at once.
            self.slot_end(SlotPiece::Late, emit);
        }
        if !v.spreads || now < self.ready_at {
            if now < self.ready_at {
                self.slot_end(SlotPiece::Late, emit);
            }
            return false;
        }
        if self.current.is_none() {
            let next = self.next_out(v, now);
            if self.slot_open.is_some() {
                match next {
                    Some((c, t)) if t <= now && c != v.home => {
                        self.slot_end(SlotPiece::OtherChannel, emit)
                    }
                    Some((_, t)) if t <= now => {}
                    _ => {
                        let why = self.slot_held(v, now);
                        self.slot_end(why, emit);
                    }
                }
            }
            let Some((c, _)) = next.filter(|o| o.1 <= now) else {
                self.slot_open = None;
                // Nothing goes now: not before the next that may, or a
                // pace on, when the neighbours' sections may need more.
                self.ready_at = next.map_or(now + PACE, |o| o.1);
                return false;
            };
            self.current = Some(c);
        }
        let Some(c) = self.current else {
            return false;
        };
        let Some((f, _)) = self.frame(v, c, now) else {
            self.current = None;
            self.ready_at = now + PACE;
            return false;
        };
        let airtime = mac::airtime(&f);
        if !affordable(radio, airtime, now) {
            self.slot_end(SlotPiece::Bucket, emit);
            // Two holders the bucket holds alike would wait out the same
            // refill and send together: each waits a random time more,
            // within two frames, as after a piece.
            let jitter = rng.duration(Duration::ZERO, FULL_FRAME.mul(2));
            self.ready_at = radio
                .budget()
                .allowed_at(airtime, REPLY_RESERVE, now)
                .map_or(now + PACE, |t| t + jitter);
            return false;
        }
        radio.check(Job::Update, c, emit);
        true
    }

    fn slot_end(&mut self, why: SlotPiece, emit: &mut dyn FnMut(Action)) {
        if why != SlotPiece::Sent {
            // The sleepy nodes stop listening when no piece follows.
            self.run_left = 0;
        }
        if self.slot_open.take().is_some() {
            emit(Action::Trace(Trace::SlotPiece(why)));
        }
    }

    /// A piece went on `c` at `now` in its pace: in place of this
    /// channel's entry of the last two, or of the oldest.
    fn paced(&mut self, c: Channel, now: Instant) {
        let slot = self
            .sent
            .iter()
            .position(|e| e.is_none_or(|(d, _)| d == c))
            .unwrap_or_else(|| {
                let oldest = self.sent.iter().flatten().map(|e| e.1).min();
                self.sent
                    .iter()
                    .position(|e| e.map(|e| e.1) == oldest)
                    .unwrap_or(0)
            });
        self.sent[slot] = Some((c, now));
    }

    fn slot_held(&self, v: &View<'_>, now: Instant) -> SlotPiece {
        let last = self.sent.iter().flatten().find(|(d, _)| *d == v.home);
        match last {
            Some((_, x)) if *x + PACE > now => SlotPiece::OwnPace,
            Some((_, x)) => {
                let k = self.others.iter().filter(|&&o| o > *x).count() as u64;
                if *x + PACE.mul(k + 1) > now {
                    SlotPiece::SharedPace
                } else {
                    SlotPiece::Channels
                }
            }
            None => SlotPiece::Channels,
        }
    }

    pub(super) fn check(
        &mut self,
        v: &View<'_>,
        radio: &mut Radio,
        rng: &mut Rng,
        busy: bool,
        now: Instant,
        emit: &mut dyn FnMut(Action),
    ) {
        let Some(c) = self.current else {
            return;
        };
        if busy {
            self.busy_checks += 1;
            // After the slot heartbeat the sleepy nodes listen only now.
            let slot = self.slot_open.is_some();
            self.slot_end(SlotPiece::ChannelBusy, emit);
            if slot {
                self.current = None;
                self.busy_checks = 0;
            } else if self.busy_checks >= v.config.lbt_attempts {
                // As a heartbeat is skipped: the piece waits its pace.
                self.sent_piece(rng, c, None, false, now);
            } else {
                self.ready_at = now + rng.duration(Duration::ZERO, v.config.lbt_backoff_max);
            }
            return;
        }
        let Some((f, block)) = self.frame(v, c, now) else {
            self.current = None;
            return;
        };
        let airtime = mac::airtime(&f);
        if !affordable(radio, airtime, now) {
            // An owed reply spent the bucket since the check began.
            self.ready_at = now;
            return;
        }
        let deficit = self.deficit(v, c, block, now);
        let slot = self.slot_open.is_some();
        self.slot_end(SlotPiece::Sent, emit);
        self.sent_piece(rng, c, Some((block, deficit)), slot, now);
        radio.serve(Job::Update, f, now, emit);
    }

    /// The piece went, or gave the channel up: the pace starts again,
    /// unless it was the piece after the slot heartbeat, which keeps none,
    /// and the stream moves on.
    fn sent_piece(
        &mut self,
        rng: &mut Rng,
        c: Channel,
        block: Option<(u8, u8)>,
        after_slot: bool,
        now: Instant,
    ) {
        // Two senders that start a pace together part by more than a frame,
        // and then hear each other, a quarter of the time each pace.
        let jitter = rng.duration(Duration::ZERO, FULL_FRAME.mul(2));
        self.current = None;
        self.busy_checks = 0;
        self.slot_open = None;
        if after_slot {
            self.run_left = self.run_left.saturating_sub(1);
        } else {
            self.passed = 0;
            self.paced(c, now + jitter);
        }
        if let Some((b, deficit)) = block {
            let at = &mut self.index[usize::from(b)];
            *at = ((usize::from(*at) + 1) % PIECES) as u8;
            let i = usize::from(c.index());
            if self.cur[i] == b {
                // Within the quota, or past it in the block's tail.
                self.sent_cur[i] = self.sent_cur[i].saturating_add(1);
            } else {
                // A new block of the sweep: what the neighbour furthest
                // behind on it lacks, and the margin for loss.
                self.cur[i] = b;
                self.sent_cur[i] = 1;
                self.target[i] = sweep_pieces(deficit);
            }
        }
    }
}

/// Whether a piece of `airtime` may go now: it must leave the reply
/// reserve, as a forward must (spec "Duty cycle cap").
fn affordable(radio: &Radio, airtime: Duration, now: Instant) -> bool {
    radio
        .budget()
        .would_allow_keeping(airtime, REPLY_RESERVE, now)
}

impl<const H: usize, const Q: usize> Node<H, Q> {
    /// The image this node holds whole, as its driver last said.
    pub fn image(&self) -> Option<Image> {
        self.spread.image()
    }

    /// The image it gathers and the first block it lacks.
    pub fn gathering(&self) -> Option<(Image, u8)> {
        self.spread.gathering()
    }

    /// The section a heartbeat sent on `channel` at `now` carries, if any
    /// ([`Spread::heartbeat_section`]).
    pub(super) fn heartbeat_section(&self, now: Instant, channel: Channel) -> Option<Section> {
        self.spread.heartbeat_section(&view!(self), now, channel)
    }

    pub(super) fn section_sent(&mut self, now: Instant, channel: Channel) {
        let sleepy = self.sleepy();
        self.spread
            .section_sent(self.config.version, sleepy, now, channel);
    }

    pub(super) fn holds_image(&mut self, image: Image, now: Instant) {
        self.spread.holds_image(image, &mut self.rng, now);
    }

    pub(super) fn gather(&mut self, image: Image, from: u8, now: Instant) {
        self.spread
            .gather(&self.config, image, from, &mut self.rng, now);
    }

    /// A heartbeat of `from` carried `section`: the neighbour table keeps
    /// it, and the spread hears it ([`Spread::section_heard`]).
    pub(super) fn section_heard(
        &mut self,
        from: NodeId,
        section: Section,
        now: Instant,
        emit: &mut dyn FnMut(Action),
    ) {
        self.neighbours.section_heard(from, section, now);
        let streams_here = !self.sleepy() || self.anchor().is_some_and(|a| a.id == from);
        let v = view!(self);
        self.spread
            .section_heard(&v, section, streams_here, now, emit);
    }

    /// An unproven gather nobody offers, or fed nothing for the bound, is
    /// given up ([`Spread::expire`]).
    pub(super) fn expire_update(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        self.spread.expire(&self.config, now, emit);
    }

    /// Its slot heartbeat went on its home channel
    /// ([`Spread::slot_heartbeat`]).
    pub(super) fn slot_heartbeat(&mut self, now: Instant) {
        let v = view!(self);
        self.spread.slot_heartbeat(&v, now);
    }

    /// A sleepy node heard its anchor's slot heartbeat
    /// ([`Spread::helped_by`]).
    pub(super) fn anchor_slot_heard(&mut self, now: Instant) {
        let anchor = self.anchor().and_then(|a| self.neighbours.get(a.id));
        if self.spread.helped_by(&self.neighbours, anchor) {
            self.listen.awake_until_at_least(now + SLOT_LISTEN);
        }
    }

    /// A piece's transmission ended ([`Spread::piece_done`]).
    pub(super) fn update_sent(&mut self, now: Instant) {
        self.spread.piece_done(now);
    }

    pub(super) fn update_heard(
        &mut self,
        u: Update<'_>,
        now: Instant,
        emit: &mut dyn FnMut(Action),
    ) {
        let v = view!(self);
        let run = self.spread.update_heard(&v, u, now, emit);
        // A sleepy node listens on while its anchor's run goes on.
        if run && self.sleepy() {
            self.listen.awake_until_at_least(now + SLOT_LISTEN);
        }
    }

    pub(super) fn update_ready_at(&self) -> Option<Instant> {
        self.spread.ready_at(&view!(self))
    }

    pub(super) fn update_passed(&mut self, now: Instant) {
        let v = view!(self);
        self.spread.passed(&v, now);
    }

    /// Start the next piece that is due ([`Spread::start`]).
    pub(super) fn start_update(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) -> bool {
        let v = view!(self);
        self.spread
            .start(&v, &mut self.radio, &mut self.rng, now, emit)
    }

    /// The check of the piece's channel completed ([`Spread::check`]).
    pub(super) fn update_check(&mut self, busy: bool, now: Instant, emit: &mut dyn FnMut(Action)) {
        let v = view!(self);
        self.spread
            .check(&v, &mut self.radio, &mut self.rng, busy, now, emit);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Config;
    use crate::tables::send_queue::{Message, Outgoing};

    /// The pace is a floor: alone and idle, the next piece goes after the
    /// gap; with a frame queued or another sender heard, it keeps the pace.
    #[test]
    fn the_pace_is_a_floor() {
        let mut r = crate::Relay::new(Config::default(), 1);
        let c = r.home;
        let x = Instant::ZERO + Duration::from_secs(100);
        r.spread.sent[0] = Some((c, x));
        assert_eq!(
            r.spread.pace_allows(&view!(r), c, x),
            x + GAP,
            "idle and alone: after the gap"
        );
        let message = Message::new(Default::default(), NodeId::NONE, &[]);
        r.queue
            .push(Outgoing::new(
                message,
                1,
                NodeId::NONE,
                c,
                MIN_PREAMBLE_SYMBOLS,
                crate::tables::send_queue::Kind::Forward,
            ))
            .unwrap();
        assert_eq!(
            r.spread.pace_allows(&view!(r), c, x),
            x + PACE,
            "busy: the pace"
        );
        r.queue.clear();
        r.spread.others[0] = x + Duration::from_secs(1);
        assert_eq!(
            r.spread.pace_allows(&view!(r), c, x),
            x + PACE.mul(2),
            "another heard: shared"
        );
    }

    /// What a gather lets through of the images offered: one that is fed
    /// nothing; unfed, an unproven one an older image, which holds it off,
    /// and a proven one a newer image; and only a holder of the proof
    /// offers (spec "Updates over the mesh").
    #[test]
    fn what_a_gather_lets_through() {
        let mut r = crate::Relay::new(Config::default(), 1);
        let start = Instant::ZERO + Duration::from_secs(10);
        let unfed = start + fed_within(&r.config);
        let image = |version| Image {
            version,
            blocks: 3,
            last: 7,
            key: 1,
        };
        let holder = |version| Section {
            runs: 1,
            image: Some(image(version)),
            complete: 2,
            have: 0,
        };
        let offered = |r: &mut crate::Relay, s: Section, now| {
            let mut got = None;
            let v = view!(r);
            r.spread.section_heard(&v, s, true, now, &mut |a| {
                if let Action::Offered(i) = a {
                    got = Some(i.version);
                }
            });
            got
        };
        let gather = |r: &mut crate::Relay, version, from| r.gather(image(version), from, start);
        // Only a holder of blocks 0 and 1 offers.
        let lacking = Section {
            complete: 1,
            ..holder(9)
        };
        assert_eq!(offered(&mut r, lacking, start), None);
        // Unproven: a newer image at once, an older one once unfed, and
        // the one left is held off.
        gather(&mut r, 9, 0);
        assert_eq!(offered(&mut r, holder(5), start), None);
        assert_eq!(offered(&mut r, holder(10), start), Some(10));
        gather(&mut r, 10, 0);
        assert_eq!(offered(&mut r, holder(5), unfed), Some(5));
        gather(&mut r, 5, 0);
        assert_eq!(offered(&mut r, holder(10), unfed), None, "held off");
        // Proven: nothing while fed, a newer one once unfed.
        gather(&mut r, 6, 2);
        assert_eq!(offered(&mut r, holder(7), start), None);
        assert_eq!(offered(&mut r, holder(4), unfed), None);
        assert_eq!(offered(&mut r, holder(7), unfed), Some(7));
        // Leaving a gather for a newer one holds nothing off.
        gather(&mut r, 7, 0);
        assert!(!r.spread.held_off(6, start) && !r.spread.held_off(9, start));
        assert!(r.spread.held_off(10, start));
        assert!(!r.spread.held_off(10, start + unproven_within(&r.config)));
    }

    /// A helper streams to a gatherer only within the lease since its last
    /// section; plain heartbeats do not renew it.
    #[test]
    fn a_helper_streams_to_a_gatherer_on_a_lease() {
        use crate::position::Position;
        use crate::wire::Heartbeat;
        let config = Config {
            boot_listen: Duration::from_secs(1),
            home_override: Channel::new(3),
            ..Config::default()
        };
        let mut r = crate::Relay::new(config, 1);
        let now = Instant::ZERO + Duration::from_secs(2);
        let image = Image {
            version: 2,
            blocks: 3,
            last: 7,
            key: 0,
        };
        r.holds_image(image, now);
        let me = r.config.position;
        let at = Position {
            lat: me.lat + 5_000,
            ..me
        };
        let hb = Heartbeat {
            sleepy: false,
            anchorless: false,
            carried: false,
            home: Channel::new(5).unwrap(),
            format: crate::wire::FORMAT,
            map_point: false,
            leaving: false,
            id: crate::ids::NodeId::from_u64(at.short().0 as u64),
            short: at.short(),
        };
        let section = Section {
            runs: 1,
            image: Some(image),
            complete: 1,
            have: 0,
        };
        let plane = r.plane;
        let beat = |r: &mut crate::Relay, t: Instant| {
            r.neighbours.heard(&hb, at, -80, &plane, t);
        };
        beat(&mut r, now);
        r.neighbours.section_heard(hb.id, section, now);
        let lacking = |r: &crate::Relay, t: Instant| r.spread.needs(&view!(r), t)[5].count;
        let lease = crate::node::heartbeat_interval(hb.id, &r.config)
            .mul(2 * u64::from(crate::radio::CHANNELS))
            + crate::node::heartbeat_lag(&r.config);
        let told = now;
        let mut t = now;
        while t < told + lease {
            beat(&mut r, t);
            t += Duration::from_secs(60);
        }
        let before = told + lease - Duration::from_secs(1);
        beat(&mut r, before);
        assert_eq!(lacking(&r, before), 1, "within the lease");
        let after = told + lease + Duration::from_secs(1);
        beat(&mut r, after);
        assert_eq!(lacking(&r, after), 0, "past the lease");
        r.neighbours.section_heard(hb.id, section, after);
        assert_eq!(lacking(&r, after), 1, "a section renews it");
    }
}
