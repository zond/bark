//! Firmware images spreading over the mesh (spec "Updates over the mesh").
//! A node says in a section of its heartbeats what image it runs, the
//! newest it holds or gathers, and how many of that one's blocks it holds
//! from the first. A node that hears a neighbour with a newer image its
//! driver's policy takes gathers it, keeping every piece of every block it
//! lacks. A node that holds blocks a neighbour it hears lacks streams them
//! on that neighbour's channel, sweeping the blocks in order: fresh pieces
//! of one block until the neighbour furthest behind on it has enough, then
//! the next block some neighbour there lacks, and the sweep again while
//! any lacks something. Senders on a channel share one piece a [`PACE`]
//! between them; every piece is paid from the bucket as a forward is, and
//! waits behind at most [`PASS`] frames of the send queue. A sleepy node only
//! gathers; its anchor's frame after a slot heartbeat is the next piece of
//! its stream on its home channel.

use super::{Job, Node, Stage};
use crate::event::{Action, Frame};
use crate::node::{heartbeat_interval, mac};
use crate::radio::{Channel, CHANNELS, MIN_PREAMBLE_SYMBOLS};
use crate::tables::budget::REPLY_RESERVE;
use crate::time::{Duration, Instant};
use crate::update::{
    sweep_pieces, Image, ImageTag, Piece, Section, MAX_BLOCKS, PACE, PASS, PIECES, SECTION_EVERY,
    SERVED_CHANNELS, SLOT_LISTEN,
};
use crate::wire::{Packet, Update, PIECE_BYTES};

/// How long after its slot heartbeat a relay may still start the frame
/// for its sleepy nodes: what [`SLOT_LISTEN`] leaves beyond the frame,
/// its check and its start, 5 ms less about 3 ms and under 1 ms
/// (spec "Heartbeat").
const SLOT_START: Duration = Duration::from_millis(1);

/// Other senders' pieces a node remembers, to count them against its own
/// pace on its home channel.
const OTHERS: usize = 8;

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
}

impl Gather {
    /// Whether block `b` decoded and passed.
    fn holds(&self, b: u8) -> bool {
        b < 64 && self.done >> b & 1 == 1
    }

    /// Whether piece `index` of block `b` is one it takes: of a block it
    /// lacks, and not handed before. It marks it.
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

    /// Block `b` decoded, `ok`, or failed its hash, and its pieces are
    /// taken again. The first block it lacks moves past the blocks held.
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
/// (spec "Updates over the mesh", `docs/core-interface.md` §6).
pub(super) struct Spread {
    /// The image it holds whole, from its driver.
    own: Option<Image>,
    /// The last image offered to the driver.
    offered: Option<(ImageTag, u32)>,
    gather: Option<Gather>,
    /// The last channels it sent a piece on, and when: the only ones that
    /// can still be within their pace, since a pace holds at most this
    /// many.
    sent: [Option<(Channel, Instant)>; SERVED_CHANNELS],
    /// Frames of the send queue sent while a piece was due.
    passed: u8,
    /// On each channel, the block its sweep is at (none before the first,
    /// `u8::MAX`), the pieces of it sent, and the pieces of it to send.
    cur: [u8; CHANNELS as usize],
    sent_cur: [u8; CHANNELS as usize],
    target: [u8; CHANNELS as usize],
    /// The piece of each block it streams next: from a random start,
    /// climbing, so a block's pieces repeat only after [`PIECES`].
    index: [u8; MAX_BLOCKS],
    /// Until when its heartbeats carry its section, from the hour's start.
    section_until: Instant,
    /// When its heartbeats next start to carry it for a rotation.
    section_at: Instant,
    /// Until when the frame after its slot heartbeat on its home channel
    /// may start, where its sleepy nodes listen.
    slot_open: Option<Instant>,
    /// When it last heard other nodes' pieces on its home channel, the
    /// last few: they share the channel's pace with it.
    others: [Instant; OTHERS],
    /// The piece under way, its channel, and its checks.
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
            sent: [None; SERVED_CHANNELS],
            passed: 0,
            cur: [u8::MAX; CHANNELS as usize],
            sent_cur: [0; CHANNELS as usize],
            target: [0; CHANNELS as usize],
            index: [0; MAX_BLOCKS],
            section_until: Instant::ZERO,
            section_at: Instant::ZERO,
            slot_open: None,
            others: [Instant::ZERO; OTHERS],
            current: None,
            busy_checks: 0,
            ready_at: Instant::ZERO,
        }
    }
}

/// What a channel needs of what this node holds: the lowest block a
/// neighbour heard there lacks, how many lack something, and whether all
/// of them are sleepy.
#[derive(Clone, Copy, Debug, Default)]
struct Need {
    from: u8,
    count: u8,
    sleepy: bool,
}

impl<const H: usize, const Q: usize> Node<H, Q> {
    /// The image this node holds whole, as its driver last said.
    pub fn image(&self) -> Option<Image> {
        self.spread.own
    }

    /// The image it gathers and the first block it lacks, if any.
    pub fn gathering(&self) -> Option<(Image, u8)> {
        self.spread.gather.map(|g| (g.image, g.block))
    }

    /// Whether the node streams now: a relay past its first rotation.
    fn spreads(&self) -> bool {
        self.stage == Stage::Running && !self.sleepy()
    }

    /// What its section says: the version it runs, and the newest image it
    /// holds or gathers with the blocks of it it holds from the first.
    fn section(&self) -> Option<Section> {
        let s = &self.spread;
        let runs = s.own.map_or(0, |o| o.version);
        let (image, complete, have) = match (s.gather, s.own) {
            (Some(g), _) => {
                let have: u32 = g
                    .seen
                    .get(usize::from(g.block))
                    .map_or(0, |b| b.iter().map(|x| x.count_ones()).sum());
                (g.image, g.block, have.min(255) as u8)
            }
            (None, Some(o)) => (o, o.blocks, 0),
            (None, None) => return None,
        };
        Some(Section {
            runs,
            image: Some(image),
            complete,
            have,
        })
    }

    /// The image it streams and whether it holds block `b` of it: the one
    /// it gathers once it holds a block of it, else the one it holds
    /// whole.
    fn streams(&self) -> Option<Image> {
        let s = &self.spread;
        s.gather.filter(|g| g.done != 0).map(|g| g.image).or(s.own)
    }

    fn holds_block(&self, b: u8) -> bool {
        let s = &self.spread;
        match s.gather.filter(|g| g.done != 0) {
            Some(g) => g.holds(b),
            None => s.own.is_some_and(|o| b < o.blocks),
        }
    }

    /// Whether a live neighbour it hears gathers an image and lacks a
    /// block, or it gathers itself: its heartbeats then carry its section
    /// every time.
    fn update_running(&self, now: Instant) -> bool {
        self.spread.gather.is_some()
            || self.neighbours.live(now).any(|(_, n)| {
                n.update
                    .is_some_and(|u| u.image.is_some_and(|i| u.complete < i.blocks))
            })
    }

    /// The section a heartbeat sent at `now` carries, if any: every
    /// heartbeat while an update runs here, else every heartbeat of one
    /// rotation an hour, so that each channel hears it.
    pub(super) fn heartbeat_section(&self, now: Instant) -> Option<Section> {
        let s = &self.spread;
        let due = self.update_running(now) || now < s.section_until || now >= s.section_at;
        self.section().filter(|_| due)
    }

    /// A heartbeat with the section went: the hour's rotation of them
    /// starts now if it was due.
    pub(super) fn section_sent(&mut self, now: Instant) {
        let s = &mut self.spread;
        if now >= s.section_at {
            s.section_at = now + SECTION_EVERY;
            s.section_until = now + self.config.heartbeat_max.mul(u64::from(CHANNELS));
        }
    }

    /// `AppCommand::Holds`: its section tells it within a rotation.
    pub(super) fn holds_image(&mut self, image: Image, now: Instant) {
        if self.spread.own == Some(image) {
            return;
        }
        self.random_starts();
        let s = &mut self.spread;
        s.own = Some(image);
        if s.gather.is_some_and(|g| g.image.version <= image.version) {
            s.gather = None;
        }
        s.section_at = now;
    }

    /// `AppCommand::Gather`: the driver's policy took `image`; the node
    /// gathers it, holding the blocks before `from` already, and a newer
    /// image it is told to gather replaces the one it gathered.
    pub(super) fn gather(&mut self, image: Image, from: u8, now: Instant) {
        if usize::from(image.blocks) > MAX_BLOCKS {
            return;
        }
        let mut g = Gather {
            image,
            block: 0,
            seen: [[0; PIECES.div_ceil(8)]; MAX_BLOCKS],
            done: 0,
        };
        for b in 0..from.min(image.blocks) {
            g.decoded(b, true);
        }
        self.random_starts();
        self.spread.gather = Some(g);
        self.spread.section_at = now;
    }

    /// Each block's pieces from a random start, so that two holders of it
    /// seldom send the same, and every sweep from the first block.
    fn random_starts(&mut self) {
        let s = &mut self.spread;
        s.cur = [u8::MAX; CHANNELS as usize];
        s.sent_cur = [0; CHANNELS as usize];
        s.target = [0; CHANNELS as usize];
        for i in 0..MAX_BLOCKS {
            self.spread.index[i] = self.rng.range(0, PIECES as u64 - 1) as u8;
        }
    }

    /// `AppCommand::Block`: a block decoded, or failed its hash.
    pub(super) fn block_done(&mut self, index: u8, ok: bool) {
        if let Some(g) = self.spread.gather.as_mut() {
            g.decoded(index, ok);
        }
    }

    /// A heartbeat of `from` carried `section`: the neighbour table keeps
    /// it, and a newer image than this node runs or gathers is offered to
    /// the driver's policy, once.
    pub(super) fn section_heard(
        &mut self,
        from: crate::ids::NodeId,
        section: Section,
        emit: &mut dyn FnMut(Action),
    ) {
        self.neighbours.section_heard(from, section);
        let Some(image) = section.image else {
            return;
        };
        let s = &mut self.spread;
        let newest = s
            .gather
            .map(|g| g.image.version)
            .max(s.own.map(|o| o.version))
            .unwrap_or(0);
        if image.version <= newest || s.offered == Some((image.tag(), image.version)) {
            return;
        }
        if usize::from(image.blocks) > MAX_BLOCKS {
            return;
        }
        s.offered = Some((image.tag(), image.version));
        emit(Action::Offered(image));
    }

    /// Its slot heartbeat went on its home channel: a relay whose sleepy
    /// nodes there gather what it holds sends them the next piece of its
    /// stream at once, while they listen.
    pub(super) fn slot_heartbeat(&mut self, now: Instant) {
        let home = self.needs(now)[usize::from(self.home.index())];
        if home.count > 0 {
            self.spread.slot_open = Some(now + SLOT_START);
            // A wait for a pace that had nothing to send ends: this frame
            // is its own chance, and the bucket is asked again.
            self.spread.ready_at = self.spread.ready_at.min(now);
        }
    }

    /// A sleepy node heard its anchor's slot heartbeat: it listens for the
    /// piece after it while it gathers and its anchor holds a block it
    /// lacks.
    pub(super) fn anchor_slot_heard(&mut self, now: Instant) {
        let Some(g) = self.spread.gather else {
            return;
        };
        let anchor = self.anchor().and_then(|a| self.neighbours.get(a.id));
        let helps = anchor
            .and_then(|n| n.update)
            .and_then(|u| u.image.map(|i| (i, u.complete)))
            .is_some_and(|(i, complete)| i.tag() == g.image.tag() && complete > g.block);
        if helps {
            self.timers.awake_until = self.timers.awake_until.max(now + SLOT_LISTEN);
        }
    }

    /// A piece heard on the home channel.
    pub(super) fn update_heard(
        &mut self,
        u: Update<'_>,
        now: Instant,
        emit: &mut dyn FnMut(Action),
    ) {
        let Update::Piece {
            tag,
            block,
            index,
            bytes,
        } = u;
        // Another node sends here: it shares the channel's pace.
        let o = &mut self.spread.others;
        o.rotate_right(1);
        o[0] = now;
        if usize::from(index) >= PIECES {
            return;
        }
        let Some(g) = self.spread.gather.as_mut().filter(|g| g.image.tag() == tag) else {
            return;
        };
        if !g.take(block, index) {
            return;
        }
        let mut piece = Piece {
            block,
            index,
            bytes: [0; PIECE_BYTES],
        };
        piece.bytes.copy_from_slice(bytes);
        emit(Action::StorePiece(piece));
    }

    /// What each channel needs of what this node streams, from the
    /// sections of the neighbours it hears there.
    fn needs(&self, now: Instant) -> [Need; CHANNELS as usize] {
        let mut need = [Need::default(); CHANNELS as usize];
        let Some(image) = self.streams() else {
            return need;
        };
        for (_, n) in self.neighbours.live(now) {
            let Some(u) = n.update.filter(|u| u.gathers(image.tag())) else {
                continue;
            };
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
                    sleepy: true,
                };
            }
            c.from = c.from.min(from);
            c.count = c.count.saturating_add(1);
            c.sleepy &= n.sleepy;
        }
        need
    }

    /// The channels it streams on: those whose neighbours lack the most of
    /// what it holds, its home first of equals, at most
    /// [`SERVED_CHANNELS`].
    fn stream_channels(&self, now: Instant) -> [Option<(Channel, Need)>; SERVED_CHANNELS] {
        let need = self.needs(now);
        let mut out = [None; SERVED_CHANNELS];
        for c in Channel::all() {
            let n = need[usize::from(c.index())];
            if n.count == 0 {
                continue;
            }
            let better = |o: &Option<(Channel, Need)>| match o {
                None => true,
                Some((oc, on)) => (n.count, c == self.home) > (on.count, *oc == self.home),
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
    fn deficit(&self, c: Channel, b: u8, now: Instant) -> u8 {
        let Some(image) = self.streams() else {
            return 0;
        };
        let k = image.pieces(b);
        self.neighbours
            .live(now)
            .filter(|(_, n)| n.home == c)
            .filter_map(|(_, n)| n.update.filter(|u| u.gathers(image.tag())))
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
    fn stream_block(&self, c: Channel, now: Instant) -> Option<u8> {
        let blocks = self.streams()?.blocks;
        let s = &self.spread;
        let i = usize::from(c.index());
        let cur = s.cur[i];
        let wanted = |b: u8| self.holds_block(b) && self.deficit(c, b, now) > 0;
        if s.sent_cur[i] < s.target[i] && wanted(cur) {
            return Some(cur);
        }
        (cur.saturating_add(1)..blocks)
            .find(|&b| wanted(b))
            .or_else(|| (0..blocks.min(cur.saturating_add(1))).find(|&b| wanted(b)))
    }

    /// Whether the channel's pace lets a piece go on `c` at `t`, and if not,
    /// when it will (spec "Updates over the mesh"): one a [`PACE`] on each
    /// channel, on at most [`SERVED_CHANNELS`] in one.
    fn pace_allows(&self, c: Channel, t: Instant) -> Instant {
        let s = &self.spread;
        let mut at = t;
        for (d, x) in s.sent.iter().flatten() {
            if *d == c {
                // On its home channel it hears the others that send there,
                // and takes its share: one piece in k + 1 paces when k
                // others sent since its last.
                let k = if c == self.home {
                    s.others.iter().filter(|&&o| o > *x).count() as u64
                } else {
                    0
                };
                at = at.max(*x + PACE.mul(k + 1));
            }
        }
        let mut ends = [Instant::ZERO; SERVED_CHANNELS];
        let mut n = 0;
        for (d, x) in s.sent.iter().flatten() {
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

    /// The next slot heartbeat of this relay on its home channel.
    fn next_home_slot(&self) -> Instant {
        let b = &self.radio.beacon;
        let steps = (u64::from(self.home.index()) + u64::from(CHANNELS)
            - u64::from(b.rotation.index()))
            % u64::from(CHANNELS);
        b.next_at + heartbeat_interval(self.id, &self.config).mul(steps)
    }

    /// The channel of the next piece and when the pace lets it go. A
    /// stream only sleepy nodes need goes right after the slot heartbeat;
    /// others on the home channel leave the pace room for that slot when
    /// sleepy nodes there need it and it comes within a pace.
    fn next_out(&self, now: Instant) -> Option<(Channel, Instant)> {
        let s = &self.spread;
        let home = self.home;
        let slot = s.slot_open;
        let home_slot = self.next_home_slot();
        self.stream_channels(now)
            .into_iter()
            .flatten()
            .filter_map(|(c, need)| {
                let at = now;
                let at = match slot {
                    Some(_) if c == home && at <= now => now,
                    Some(_) if c == home => return None,
                    None if c == home && need.sleepy => return None,
                    None if c == home && home_slot < at + PACE => return None,
                    _ => at,
                };
                Some((c, self.pace_allows(c, at)))
            })
            .min_by_key(|x| x.1)
    }

    /// When a piece next may go.
    fn update_due_at(&self, now: Instant) -> Option<Instant> {
        if !self.spreads() {
            return None;
        }
        if let Some(c) = self.spread.current {
            return Some(self.spread.ready_at.max(self.pace_allows(c, now)));
        }
        if let Some(t) = self.spread.slot_open {
            // The frame after the slot goes at once or not at all.
            return Some(t.max(self.spread.ready_at));
        }
        self.next_out(now).map(|(_, t)| t.max(self.spread.ready_at))
    }

    /// When the update job is next ready of its own accord: a time past
    /// is now. Asked with no time, it may name a time at which
    /// [`Node::start_update`] then finds nothing; that one sets
    /// `ready_at` past the moment, so the node does not wake again for it.
    pub(super) fn update_ready_at(&self) -> Option<Instant> {
        self.update_due_at(Instant::ZERO)
    }

    /// Whether a piece has waited behind [`PASS`] frames of the send
    /// queue, or is the frame after a slot heartbeat, and goes before the
    /// next.
    pub(super) fn update_first(&self) -> bool {
        self.spread.passed >= PASS || self.spread.slot_open.is_some()
    }

    /// A frame of the send queue went: it passed a piece that was due.
    pub(super) fn update_passed(&mut self, now: Instant) {
        if self.update_due_at(now).is_some_and(|t| t <= now) {
            self.spread.passed = self.spread.passed.saturating_add(1);
        }
    }

    /// The piece frame for channel `c`, as it goes now.
    fn update_frame(&self, c: Channel, now: Instant) -> Option<Frame> {
        self.stream_channels(now)
            .into_iter()
            .flatten()
            .find(|(d, _)| *d == c)?;
        let block = self.stream_block(c, now)?;
        let image = self.streams()?;
        let packet = Update::Piece {
            tag: image.tag(),
            block,
            index: self.spread.index[usize::from(block)],
            bytes: &[0; PIECE_BYTES],
        };
        Some(mac::frame(&Packet::Update(packet), c, MIN_PREAMBLE_SYMBOLS))
    }

    /// Start the next piece that is due: its check of the channel, once
    /// the bucket holds it with the reply reserve left, as a forward must
    /// (spec "Duty cycle cap").
    pub(super) fn start_update(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) -> bool {
        if self.spread.slot_open.is_some_and(|t| now > t) {
            // The frame after the slot heartbeat could not start at once.
            self.spread.slot_open = None;
        }
        if !self.spreads() || now < self.spread.ready_at {
            return false;
        }
        if self.spread.current.is_none() {
            let next = self.next_out(now);
            let Some((c, _)) = next.filter(|o| o.1 <= now) else {
                self.spread.slot_open = None;
                // Nothing goes now: not before the next that may, or a
                // pace on, when the neighbours' sections may need more.
                self.spread.ready_at = next.map_or(now + PACE, |o| o.1);
                return false;
            };
            self.spread.current = Some(c);
        }
        let Some(c) = self.spread.current else {
            return false;
        };
        let Some(f) = self.update_frame(c, now) else {
            self.spread.current = None;
            self.spread.ready_at = now + PACE;
            return false;
        };
        let airtime = mac::airtime(&f);
        if !self.affordable(airtime, now) {
            self.spread.ready_at = self
                .radio
                .budget
                .allowed_at(airtime, REPLY_RESERVE, now)
                .unwrap_or(now + PACE);
            return false;
        }
        self.radio.excursion.check(Job::Update, c, emit);
        true
    }

    /// The check of the piece's channel completed.
    pub(super) fn update_check(&mut self, busy: bool, now: Instant, emit: &mut dyn FnMut(Action)) {
        let Some(c) = self.spread.current else {
            return;
        };
        if busy {
            self.spread.busy_checks += 1;
            // After the slot heartbeat the sleepy nodes listen only now.
            let slot = self.spread.slot_open.take().is_some();
            if slot || self.spread.busy_checks >= self.config.lbt_attempts {
                // As a heartbeat is skipped: the piece waits its pace.
                self.update_sent(c, None, now);
            } else {
                self.spread.ready_at = now
                    + self
                        .rng
                        .duration(Duration::ZERO, self.config.lbt_backoff_max);
            }
            return;
        }
        let Some(f) = self.update_frame(c, now) else {
            self.spread.current = None;
            return;
        };
        let airtime = mac::airtime(&f);
        if !self.affordable(airtime, now) {
            // An owed reply spent the bucket since the check began.
            self.spread.ready_at = now;
            return;
        }
        let block = match Packet::decode(f.payload()) {
            Ok(Packet::Update(Update::Piece { block, .. })) => Some(block),
            _ => None,
        };
        let deficit = block.map_or(0, |b| self.deficit(c, b, now));
        self.radio.budget.record(airtime, now);
        self.update_sent(c, block.map(|b| (b, deficit)), now);
        self.radio.excursion.serve(Job::Update, f, emit);
    }

    /// The piece went, or gave the channel up: the pace starts again, and
    /// the stream moves on.
    fn update_sent(&mut self, c: Channel, block: Option<(u8, u8)>, now: Instant) {
        let s = &mut self.spread;
        s.current = None;
        s.busy_checks = 0;
        s.passed = 0;
        s.slot_open = None;
        // In place of this channel's entry, or of the oldest.
        let slot = s
            .sent
            .iter()
            .position(|e| e.is_none_or(|(d, _)| d == c))
            .unwrap_or_else(|| {
                let oldest = s.sent.iter().flatten().map(|e| e.1).min();
                s.sent
                    .iter()
                    .position(|e| e.map(|e| e.1) == oldest)
                    .unwrap_or(0)
            });
        s.sent[slot] = Some((c, now));
        if let Some((b, deficit)) = block {
            let at = &mut s.index[usize::from(b)];
            *at = ((usize::from(*at) + 1) % PIECES) as u8;
            let i = usize::from(c.index());
            if s.cur[i] == b && s.sent_cur[i] < s.target[i] {
                s.sent_cur[i] += 1;
            } else {
                // A new block of the sweep: what the neighbour furthest
                // behind on it lacks, and the margin for loss.
                s.cur[i] = b;
                s.sent_cur[i] = 1;
                s.target[i] = sweep_pieces(deficit);
            }
        }
    }
}
