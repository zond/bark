//! Firmware images spreading over the mesh
//! (spec "Updates over the mesh"): a relay that holds an image, whole or
//! its first blocks, announces it and sends the pieces of the block it is
//! asked for; one that gathers an image asks for its blocks in order and
//! hands the driver the pieces it takes. Every update frame keeps the pace
//! of [`PACE`] a channel, is paid from the bucket as a forward is, and
//! waits behind at most [`PASS`] frames of the send queue. A sleepy node
//! gathers from its anchor, which sends to it after its slot heartbeat.

use super::{Job, Node, Stage};
use crate::event::{Action, Frame};
use crate::ids::NodeId;
use crate::node::{heartbeat_interval, mac};
use crate::radio::CHANNELS;
use crate::radio::{Channel, MIN_PREAMBLE_SYMBOLS};
use crate::tables::budget::REPLY_RESERVE;
use crate::time::{Duration, Instant};
use crate::update::{
    credit, Image, ImageTag, Piece, ANNOUNCE_EVERY, HOLD_BACK, PACE, PASS, PIECES, REQUEST_WAIT,
    SERVED_CHANNELS, SLOT_LISTEN,
};
use crate::wire::{Packet, Update, PIECE_BYTES};

/// How long after its slot heartbeat a relay may still start the frame
/// for its sleepy nodes: what [`SLOT_LISTEN`] leaves beyond the frame,
/// its check and its start, 5 ms less about 3 ms and under 1 ms
/// (spec "Heartbeat").
const SLOT_START: Duration = Duration::from_millis(1);

/// Blocks a node takes pieces of at once: as many whole blocks as the
/// driver's room for pieces holds, 352 pieces (`bark-image/src/layout.rs`),
/// 5.5 blocks of [`crate::update::BLOCK_PIECES`].
const WINDOW: usize = 5;

/// A node that holds an image, as its announcement said: whom to ask.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Holder {
    id: NodeId,
    /// The channel it listens on.
    home: Channel,
    /// The blocks it holds, from the first.
    held: u8,
}

/// What a node that gathers an image keeps.
#[derive(Clone, Copy, Debug)]
struct Gather {
    image: Image,
    /// The holder it asks.
    holder: Option<Holder>,
    /// The first block it lacks: it takes pieces of this block and of the
    /// [`WINDOW`] − 1 after it.
    block: u8,
    /// The pieces of each block of the window handed to the driver, a bit
    /// each.
    seen: [[u8; PIECES.div_ceil(8)]; WINDOW],
    /// The blocks of the window that decoded, a bit each.
    done: u8,
    /// A request is owed from `ask_at`: the gathering began, or a block
    /// ended.
    ask: bool,
    ask_at: Instant,
    /// When it last asked, or took a piece; the later of the two starts
    /// [`REQUEST_WAIT`], and a random wait within a pace after it, since
    /// nodes that took the same pieces fall quiet together.
    quiet_since: Instant,
    quiet_wait: Duration,
}

impl Gather {
    /// Where block `b` is in the window, if it is and lacks pieces.
    fn slot(&self, b: u8) -> Option<usize> {
        let i = usize::from(b.checked_sub(self.block)?);
        (i < WINDOW && b < self.image.blocks && self.done & (1 << i) == 0).then_some(i)
    }

    /// Whether piece `index` of block `b` is one it takes: of a block of
    /// the window that lacks pieces, and not handed before. It marks it.
    fn take(&mut self, b: u8, index: u8) -> bool {
        let Some(i) = self.slot(b) else {
            return false;
        };
        let (byte, bit) = (usize::from(index / 8), 1 << (index % 8));
        let new = self.seen[i][byte] & bit == 0;
        self.seen[i][byte] |= bit;
        new
    }

    /// Block `b` decoded, `ok`, or failed its hash: the window moves past
    /// the blocks decoded, or the block's pieces are gathered again. A
    /// request is owed when the first block lacked changes, or fails.
    fn decoded(&mut self, b: u8, ok: bool) {
        let Some(i) = self.slot(b) else {
            return;
        };
        if !ok {
            self.seen[i] = [0; PIECES.div_ceil(8)];
            self.ask = i == 0;
            return;
        }
        self.done |= 1 << i;
        while self.done & 1 != 0 {
            self.block += 1;
            self.done >>= 1;
            self.seen.copy_within(1.., 0);
            self.seen[WINDOW - 1] = [0; PIECES.div_ceil(8)];
            self.ask = self.lacks();
        }
    }

    /// The pieces of the first block it lacks that it has yet to take.
    fn lacking(&self) -> u8 {
        let had: u32 = self.seen[0].iter().map(|b| b.count_ones()).sum();
        (u32::from(self.image.pieces(self.block)).saturating_sub(had)).max(1) as u8
    }

    /// Whether it still lacks a block.
    fn lacks(&self) -> bool {
        self.block < self.image.blocks
    }

    /// The holder it asks, if that one holds the block it lacks.
    fn source(&self) -> Option<Holder> {
        self.holder.filter(|h| h.held > self.block && self.lacks())
    }

    /// A holder announced the image: it asks the one that holds the block
    /// it lacks, one at home on `mine`, its own channel, before one that
    /// is not, and of those the one heard last. It owes a request when it
    /// had none to ask.
    fn heard_holder(&mut self, h: Holder, mine: Channel) -> bool {
        let had = self.source().is_some();
        let serves = |x: &Holder| x.held > self.block;
        let rank = |x: &Holder| (serves(x), x.home == mine);
        let take = match self.holder {
            None => true,
            Some(cur) if cur.id == h.id => true,
            Some(cur) => rank(&h) >= rank(&cur),
        };
        if take {
            self.holder = Some(h);
        }
        !had && self.source().is_some()
    }
}

/// Blocks asked for on one channel at once that a holder keeps: the
/// nodes there take the same pieces and move through the blocks together,
/// so few are asked for at a time; one asked for when all are taken is
/// asked for again after [`REQUEST_WAIT`].
const ASKED: usize = 4;

/// A block asked for on a channel.
#[derive(Clone, Copy, Debug)]
struct Asked {
    block: u8,
    /// The piece of it sent next.
    index: u8,
    /// Pieces still to send for the requests heard ([`Image::credit`]).
    left: u8,
}

/// A channel a holder sends pieces on, and the blocks asked for there.
#[derive(Clone, Copy, Debug)]
struct Serving {
    channel: Channel,
    /// The image they are of.
    tag: ImageTag,
    /// Only sleepy nodes asked: the pieces go after the slot heartbeat on
    /// the home channel, when they listen.
    sleepy: bool,
    asked: [Option<Asked>; ASKED],
    /// The block it has begun to send, which it sends until its pieces
    /// are spent.
    started: Option<u8>,
}

impl Serving {
    /// The block it sends now: the one begun, else the highest asked for,
    /// so that the nodes nearest to holding the image come to hold it
    /// first and serve it in turn.
    fn current(&self) -> Option<Asked> {
        let asked = || self.asked.iter().flatten();
        self.started
            .and_then(|b| asked().find(|a| a.block == b))
            .or_else(|| asked().max_by_key(|a| a.block))
            .copied()
    }

    /// A request for `block`: it buys `credit` pieces of it from now, or
    /// none if more are still to go,
    /// from piece `start` on for a block not kept. A block higher than one
    /// kept takes that one's place when all are taken.
    fn ask(&mut self, block: u8, credit: u8, start: u8) {
        if let Some(a) = self.asked.iter_mut().flatten().find(|a| a.block == block) {
            a.left = a.left.max(credit);
            return;
        }
        let new = Some(Asked {
            block,
            index: start,
            left: credit,
        });
        if let Some(free) = self.asked.iter_mut().find(|a| a.is_none()) {
            *free = new;
        } else if let Some(lowest) = self
            .asked
            .iter_mut()
            .filter(|a| a.is_some_and(|a| a.block < block))
            .min_by_key(|a| a.map(|a| a.block))
        {
            *lowest = new;
        }
    }

    /// The current block's piece went: the next piece, and the block is
    /// done once its credit is spent.
    fn sent(&mut self) {
        let Some(cur) = self.current() else { return };
        self.started = Some(cur.block);
        for a in self.asked.iter_mut() {
            if let Some(x) = a.as_mut().filter(|x| x.block == cur.block) {
                x.index = ((usize::from(x.index) + 1) % PIECES) as u8;
                x.left -= 1;
                if x.left == 0 {
                    *a = None;
                }
            }
        }
        if self.current().is_some_and(|a| a.block != cur.block) {
            self.started = None;
        }
    }
}

/// An update frame the node sends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Out {
    Request,
    Piece(usize),
    /// An announcement on this channel: owed, the hourly one on the home
    /// channel or on another, one for blocks newly held, or the one after
    /// the slot heartbeat.
    Announce(Channel, Turn),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Turn {
    Owed,
    Home,
    Other,
    Grown,
    Slot,
}

/// What the node keeps of the images spreading
/// (spec "Updates over the mesh", `docs/core-interface.md` §6).
pub(super) struct Spread {
    /// The image it holds whole, from its driver.
    own: Option<Image>,
    /// The last image offered to the driver, and its holder.
    offered: Option<(Image, Holder)>,
    gather: Option<Gather>,
    serving: [Option<Serving>; SERVED_CHANNELS],
    /// The last channels it sent an update frame on, and when: the only
    /// ones that can still be within their pace, since a pace holds at
    /// most this many.
    sent: [Option<(Channel, Instant)>; SERVED_CHANNELS],
    /// Frames of the send queue sent while an update frame was due.
    passed: u8,
    /// The next hourly announcement on the home channel, and on `other`.
    home_at: Option<Instant>,
    other_at: Option<Instant>,
    other: Channel,
    /// An announcement owed on a channel: to a node that asked for an
    /// older image.
    owed: Option<Channel>,
    /// When it announces on its home channel the blocks it holds of the
    /// image it gathers, which grew.
    grown_at: Option<Instant>,
    /// Until when the frame after its slot heartbeat on its home channel
    /// may start, where its sleepy nodes listen.
    slot_open: Option<Instant>,
    /// A sleepy node: when it next listens after its anchor's slot
    /// heartbeat for an announcement, not gathering.
    listen_at: Instant,
    /// Until when it sends no pieces on its home channel, where another
    /// holder sends them.
    held_back: Option<Instant>,
    /// The frame under way, and its listen-before-talk checks.
    current: Option<(Out, Channel)>,
    busy_checks: u8,
    /// Not before this: a listen-before-talk backoff or the bucket.
    ready_at: Instant,
}

impl Spread {
    pub(super) const fn new(other: Channel) -> Spread {
        Spread {
            own: None,
            offered: None,
            gather: None,
            serving: [None; SERVED_CHANNELS],
            sent: [None; SERVED_CHANNELS],
            passed: 0,
            home_at: None,
            other_at: None,
            other,
            owed: None,
            grown_at: None,
            slot_open: None,
            listen_at: Instant::ZERO,
            held_back: None,
            current: None,
            busy_checks: 0,
            ready_at: Instant::ZERO,
        }
    }
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

    /// Whether the node takes part now: a relay past its first rotation,
    /// or a sleepy node with an anchor, which only asks.
    fn spreads(&self) -> bool {
        self.stage == Stage::Running && (!self.sleepy() || self.anchored_sleepy())
    }

    /// The image whose pieces it can send for `tag`, and the blocks of it
    /// it holds: the one it holds whole, or the one it gathers, its blocks
    /// before the first it lacks, every one decoded and checked.
    fn served(&self, tag: ImageTag) -> Option<(Image, u8)> {
        let s = &self.spread;
        if let Some(o) = s.own.filter(|o| o.tag() == tag) {
            return Some((o, o.blocks));
        }
        s.gather
            .filter(|g| g.image.tag() == tag && g.block > 0)
            .map(|g| (g.image, g.block))
    }

    /// What it announces: the image it gathers while it holds blocks of
    /// it, the newest, else the one it holds whole.
    fn offer(&self) -> Option<(Image, u8)> {
        let s = &self.spread;
        s.gather
            .filter(|g| g.block > 0)
            .map(|g| (g.image, g.block))
            .or(s.own.map(|o| (o, o.blocks)))
    }

    /// `AppCommand::Holds`: it announces the image at once on its home
    /// channel and from an hour on hourly, and on another channel within
    /// the hour.
    pub(super) fn holds_image(&mut self, image: Image, now: Instant) {
        let s = &mut self.spread;
        if s.own == Some(image) {
            return;
        }
        s.own = Some(image);
        let wait = self.ask_wait();
        let s = &mut self.spread;
        if s.gather.is_some_and(|g| g.image.version <= image.version) {
            s.gather = None;
            s.grown_at = None;
        }
        // The blocks it served of it as it gathered go on.
        for v in s.serving.iter_mut() {
            if v.is_some_and(|v| v.tag != image.tag()) {
                *v = None;
            }
        }
        // Nodes that finish together would announce together.
        s.home_at = Some(now + wait);
        let first = self.rng.duration(Duration::ZERO, ANNOUNCE_EVERY);
        self.spread.other_at = Some(now + first);
    }

    /// `AppCommand::Gather`: the driver's policy took `image`; the node
    /// asks for it from block `from` on.
    pub(super) fn gather(&mut self, image: Image, from: u8, now: Instant) {
        let s = &mut self.spread;
        let holder = s.offered.filter(|(o, _)| o.id == image.id).map(|(_, h)| h);
        let mut g = Gather {
            image,
            holder,
            block: from,
            seen: [[0; PIECES.div_ceil(8)]; WINDOW],
            done: 0,
            ask: false,
            ask_at: now,
            quiet_since: now,
            quiet_wait: Duration::ZERO,
        };
        g.ask = g.source().is_some();
        g.ask_at = now + self.ask_wait();
        self.spread.gather = Some(g);
    }

    /// How long a request waits once it is owed: every node that heard
    /// the same announcement, or the same last piece of a block, owes one
    /// at the same moment, and a random wait within a pace keeps their
    /// requests apart.
    fn ask_wait(&mut self) -> Duration {
        self.rng.duration(Duration::ZERO, PACE)
    }

    /// `AppCommand::Block`: the block gathered decoded, or failed its hash.
    /// The blocks it holds of the image grew: a relay announces them on
    /// its home channel a random time within a pace on, as nodes that
    /// took the same pieces finish together; the pace keeps it to one a
    /// pace.
    pub(super) fn block_done(&mut self, index: u8, ok: bool, now: Instant) {
        let wait = self.ask_wait();
        let first_other = self.rng.duration(Duration::ZERO, ANNOUNCE_EVERY);
        let relay = !self.sleepy();
        let s = &mut self.spread;
        if let Some(g) = s.gather.as_mut() {
            let first = g.block;
            g.decoded(index, ok);
            if g.block == first && ok {
                return;
            }
            g.ask &= g.source().is_some();
            g.ask_at = now + wait;
            g.quiet_since = now;
            if relay && g.block > first {
                let at = now + wait;
                s.grown_at = Some(s.grown_at.map_or(at, |t| t.min(at)));
                // And hourly on the other channels in turn, as a whole
                // holder does, for the nodes there.
                s.other_at.get_or_insert(now + first_other);
            }
        }
    }

    /// Its slot heartbeat went on its home channel: a relay whose sleepy
    /// nodes listen now sends them one update frame at once, the piece
    /// they asked for or its announcement.
    pub(super) fn slot_heartbeat(&mut self, now: Instant) {
        let sleepy_asked = self
            .spread
            .serving
            .iter()
            .flatten()
            .any(|v| v.sleepy && v.channel == self.home);
        if sleepy_asked || (self.offer().is_some() && !self.heard.is_empty()) {
            self.spread.slot_open = Some(now + SLOT_START);
        }
    }

    /// A sleepy node heard its anchor's slot heartbeat: it listens for the
    /// frame after it while it gathers from its anchor, and once an hour
    /// for an announcement.
    pub(super) fn anchor_slot_heard(&mut self, now: Instant) {
        let anchor = self.anchor().map(|a| a.id);
        let s = &mut self.spread;
        let gathers = s
            .gather
            .and_then(|g| g.source())
            .is_some_and(|h| Some(h.id) == anchor);
        if !gathers && now < s.listen_at {
            return;
        }
        if !gathers {
            s.listen_at = now + ANNOUNCE_EVERY;
        }
        self.timers.awake_until = self.timers.awake_until.max(now + SLOT_LISTEN);
    }

    /// The next slot heartbeat of this relay on its home channel.
    fn next_home_slot(&self) -> Instant {
        let b = &self.radio.beacon;
        let steps = (u64::from(self.home.index()) + u64::from(CHANNELS)
            - u64::from(b.rotation.index()))
            % u64::from(CHANNELS);
        b.next_at + heartbeat_interval(self.id, &self.config).mul(steps)
    }

    /// An update frame heard on the home channel.
    pub(super) fn update_heard(
        &mut self,
        u: Update<'_>,
        now: Instant,
        emit: &mut dyn FnMut(Action),
    ) {
        let sleepy = self.sleepy();
        let anchor = self.anchor().map(|a| a.id);
        let mine = self.home;
        let me = self.me();
        let wait = self.ask_wait();
        // A block asked for again starts where the nodes that have some of
        // its pieces most likely lack them.
        let start = self.rng.range(0, PIECES as u64 - 1) as u8;
        let s = &mut self.spread;
        match u {
            Update::Announce {
                from,
                home,
                image,
                held,
            } => {
                if s.own.is_some_and(|o| o.id == image.id) {
                    // Heard whole on its own channel: its own hourly one
                    // waits, and comes after the next of the one heard.
                    if held >= image.blocks {
                        let next = now + ANNOUNCE_EVERY + wait;
                        s.home_at = s.home_at.map(|t| t.max(next));
                    }
                    return;
                }
                if image.version <= s.own.map_or(0, |o| o.version) || held == 0 {
                    return;
                }
                // A sleepy node asks only its anchor, which sends to it
                // when it listens.
                let h = Holder {
                    id: from,
                    home,
                    held,
                };
                let askable = !sleepy || anchor == Some(from);
                if let Some(g) = s.gather.as_mut().filter(|g| g.image.id == image.id) {
                    if askable && g.heard_holder(h, mine) {
                        g.ask = true;
                        g.ask_at = now + wait;
                    }
                    return;
                }
                if askable {
                    s.offered = Some((image, h));
                }
                emit(Action::Offered(image));
            }
            Update::Request {
                to,
                channel,
                version,
                sleeps,
                tag,
                block,
                lacks,
            } => {
                if to != me || sleepy {
                    return;
                }
                let served = self.served(tag);
                let s = &mut self.spread;
                let Some((image, held)) = served else {
                    // It asks for an older image than this one: tell it.
                    if s.own.is_some_and(|o| version < o.version) {
                        s.owed = Some(channel);
                    }
                    return;
                };
                // A block it does not hold yet: the asker asks again when
                // it hears that this one holds more, or asks another.
                if block >= held {
                    return;
                }
                let credit = credit(lacks.min(image.pieces(block)));
                match s
                    .serving
                    .iter_mut()
                    .flatten()
                    .find(|v| v.channel == channel)
                {
                    Some(v) => {
                        if v.tag != tag {
                            *v = Serving {
                                channel,
                                tag,
                                sleepy: sleeps,
                                asked: [None; ASKED],
                                started: None,
                            };
                        }
                        v.sleepy &= sleeps;
                        v.ask(block, credit, start);
                    }
                    None => {
                        if let Some(free) = s.serving.iter_mut().find(|v| v.is_none()) {
                            let mut v = Serving {
                                channel,
                                tag,
                                sleepy: sleeps,
                                asked: [None; ASKED],
                                started: None,
                            };
                            v.ask(block, credit, start);
                            *free = Some(v);
                        }
                    }
                }
            }
            Update::Piece {
                tag,
                block,
                index,
                bytes,
            } => {
                let serves = self.served(tag).is_some();
                let s = &mut self.spread;
                if serves {
                    // Another holder sends here: one is enough for the
                    // channel's nodes, and keeps it at the pace.
                    s.held_back = Some(now + HOLD_BACK);
                }
                if usize::from(index) >= PIECES {
                    return;
                }
                let Some(g) = s.gather.as_mut().filter(|g| g.image.tag() == tag) else {
                    return;
                };
                if !g.take(block, index) {
                    return;
                }
                g.quiet_since = now;
                let mut piece = Piece {
                    block,
                    index,
                    bytes: [0; PIECE_BYTES],
                };
                piece.bytes.copy_from_slice(bytes);
                emit(Action::StorePiece(piece));
            }
        }
    }

    /// Whether the channel's pace lets an update frame go on `c` at `t`,
    /// and if not, when it will (spec "Updates over the mesh"): one frame
    /// a [`PACE`] on each channel, on at most [`SERVED_CHANNELS`] in one.
    fn pace_allows(&self, c: Channel, t: Instant) -> Instant {
        let s = &self.spread;
        let mut at = t;
        for (d, x) in s.sent.iter().flatten() {
            if *d == c {
                at = at.max(*x + PACE);
            }
        }
        // The other channels still within their pace at `at`.
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

    /// The update frames the node would send, each with its channel and
    /// the time it wants to go, in the order they go: the announcements
    /// and the request, which are rare, before the pieces.
    fn wanted(&self, now: Instant) -> impl Iterator<Item = (Out, Channel, Instant)> + '_ {
        let s = &self.spread;
        let relay = !self.sleepy();
        let slot = s.slot_open.filter(|_| relay);
        let request = s.gather.and_then(|g| {
            let channel = g.source()?.home;
            let at = if g.ask {
                g.ask_at
            } else {
                g.quiet_since + REQUEST_WAIT + g.quiet_wait
            };
            Some((Out::Request, channel, at))
        });
        // Pieces for sleepy nodes go after the slot heartbeat; others on
        // the home channel wait for it while sleepy nodes there asked and
        // it comes within a pace, so that the pace leaves it room.
        let sleepy_home = s
            .serving
            .iter()
            .flatten()
            .any(|v| v.sleepy && v.channel == self.home);
        let home_slot = self.next_home_slot();
        let pieces = s.serving.iter().enumerate().filter_map(move |(i, v)| {
            let v = (*v)?;
            v.current()?;
            if !relay {
                return None;
            }
            let at = match s.held_back {
                Some(until) if v.channel == self.home => until.max(now),
                _ => now,
            };
            let at = match slot {
                Some(_) if v.channel == self.home && sleepy_home => now,
                None if v.sleepy => return None,
                None if v.channel == self.home && sleepy_home && home_slot < at + PACE => {
                    return None
                }
                _ => at,
            };
            Some((Out::Piece(i), v.channel, at))
        });
        let own = s.own.is_some() && relay;
        let owed = s
            .owed
            .filter(|_| own)
            .map(|c| (Out::Announce(c, Turn::Owed), c, now));
        let home = s
            .home_at
            .filter(|_| own)
            .map(|t| (Out::Announce(self.home, Turn::Home), self.home, t));
        let offers = relay && self.offer().is_some();
        let other = s.other_at.filter(|_| offers).map(|t| {
            let c = self.other_channel();
            (Out::Announce(c, Turn::Other), c, t)
        });
        let grown = s
            .grown_at
            .filter(|_| relay)
            .map(|t| (Out::Announce(self.home, Turn::Grown), self.home, t));
        let at_slot = slot
            .filter(|_| !sleepy_home && self.offer().is_some())
            .map(|_| (Out::Announce(self.home, Turn::Slot), self.home, now));
        at_slot
            .into_iter()
            .chain(owed)
            .chain(home)
            .chain(other)
            .chain(grown)
            .chain(request)
            .chain(pieces)
    }

    /// The channel of the next hourly announcement away from home.
    fn other_channel(&self) -> Channel {
        let c = self.spread.other;
        if c == self.home {
            c.next()
        } else {
            c
        }
    }

    /// When the next update frame may go, by the pace alone.
    fn update_due_at(&self, now: Instant) -> Option<Instant> {
        if !self.spreads() {
            return None;
        }
        if let Some((_, c)) = self.spread.current {
            return Some(self.spread.ready_at.max(self.pace_allows(c, now)));
        }
        if let Some(t) = self.spread.slot_open {
            // The frame after the slot goes at once or not at all.
            return Some(t);
        }
        self.next_out(now)
            .map(|(_, _, t)| t.max(self.spread.ready_at))
    }

    /// The update frame that goes next and when the pace lets it. An
    /// announcement or a request goes when its time has come and its
    /// channel's pace lets it; a piece goes when its channel's pace lets
    /// it, but not before an announcement or a request whose time has come
    /// by then, which it waits for. The answer does not depend on `now`
    /// but through the times the frames want, so that the time it gives
    /// is the time [`Node::start_update`] then finds.
    fn next_out(&self, now: Instant) -> Option<(Out, Channel, Instant)> {
        let at = |c: Channel, t: Instant| self.pace_allows(c, t.max(now));
        let first = self
            .wanted(now)
            .filter(|(o, _, _)| !matches!(o, Out::Piece(_)))
            .map(|(o, c, t)| (o, c, at(c, t)))
            .min_by_key(|x| x.2);
        let piece = self
            .wanted(now)
            .filter(|(o, _, _)| matches!(o, Out::Piece(_)))
            .map(|(o, c, t)| {
                let mut when = at(c, t);
                for (q, d, u) in self.wanted(now) {
                    if !matches!(q, Out::Piece(_)) && u <= when {
                        when = when.max(at(d, u));
                    }
                }
                (o, c, when)
            })
            .min_by_key(|x| x.2);
        match (first, piece) {
            (Some(f), Some(p)) if p.2 < f.2 => Some(p),
            (Some(f), _) => Some(f),
            (None, p) => p,
        }
    }

    /// When the update job is next ready of its own accord: a time past
    /// is now.
    pub(super) fn update_ready_at(&self) -> Option<Instant> {
        self.update_due_at(Instant::ZERO)
    }

    /// Whether an update frame has waited behind [`PASS`] frames of the
    /// send queue, and goes before the next.
    pub(super) fn update_first(&self) -> bool {
        self.spread.passed >= PASS || self.spread.slot_open.is_some()
    }

    /// A frame of the send queue went: it passed an update frame that was
    /// due.
    pub(super) fn update_passed(&mut self, now: Instant) {
        if self.update_due_at(now).is_some_and(|t| t <= now) {
            self.spread.passed = self.spread.passed.saturating_add(1);
        }
    }

    /// The frame of `out` on `channel`, as it goes now.
    fn update_frame(&self, out: Out, channel: Channel) -> Option<Frame> {
        let s = &self.spread;
        let packet = match out {
            Out::Request => {
                let g = s.gather?;
                Update::Request {
                    to: g.source()?.id,
                    channel: self.home,
                    sleeps: self.sleepy(),
                    version: g.image.version,
                    tag: g.image.tag(),
                    block: g.block,
                    lacks: g.lacking(),
                }
            }
            Out::Piece(i) => {
                let v = s.serving[i]?;
                let a = v.current()?;
                Update::Piece {
                    tag: v.tag,
                    block: a.block,
                    index: a.index,
                    bytes: &[0; PIECE_BYTES],
                }
            }
            Out::Announce(_, turn) => {
                let (image, held) = match turn {
                    Turn::Grown | Turn::Slot | Turn::Other => self.offer()?,
                    _ => s.own.map(|o| (o, o.blocks))?,
                };
                Update::Announce {
                    from: self.me(),
                    home: self.home,
                    image,
                    held,
                }
            }
        };
        Some(mac::frame(
            &Packet::Update(packet),
            channel,
            MIN_PREAMBLE_SYMBOLS,
        ))
    }

    /// Start the next update frame that is due: its check of the channel,
    /// once the bucket holds it with the reply reserve left, as a forward
    /// must (spec "Duty cycle cap").
    pub(super) fn start_update(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) -> bool {
        if self.spread.slot_open.is_some_and(|t| now > t) {
            // The frame after the slot heartbeat could not start at once.
            self.spread.slot_open = None;
        }
        if !self.spreads() || now < self.spread.ready_at {
            return false;
        }
        if self.spread.current.is_none() {
            let Some((out, c, _)) = self.next_out(now).filter(|o| o.2 <= now) else {
                self.spread.slot_open = None;
                return false;
            };
            self.spread.current = Some((out, c));
        }
        let Some((out, c)) = self.spread.current else {
            return false;
        };
        let Some(f) = self.update_frame(out, c) else {
            self.spread.current = None;
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

    /// The check of the update frame's channel completed.
    pub(super) fn update_check(&mut self, busy: bool, now: Instant, emit: &mut dyn FnMut(Action)) {
        let Some((out, c)) = self.spread.current else {
            return;
        };
        if busy {
            self.spread.busy_checks += 1;
            // After the slot heartbeat the sleepy nodes listen only now.
            let slot = self.spread.slot_open.take().is_some();
            if slot || self.spread.busy_checks >= self.config.lbt_attempts {
                // As a heartbeat is skipped: the frame waits its pace.
                self.update_sent(out, c, now);
            } else {
                self.spread.ready_at = now
                    + self
                        .rng
                        .duration(Duration::ZERO, self.config.lbt_backoff_max);
            }
            return;
        }
        let Some(f) = self.update_frame(out, c) else {
            self.spread.current = None;
            return;
        };
        let airtime = mac::airtime(&f);
        if !self.affordable(airtime, now) {
            // An owed reply spent the bucket since the check began.
            self.spread.ready_at = now;
            return;
        }
        self.radio.budget.record(airtime, now);
        self.update_sent(out, c, now);
        if matches!(out, Out::Piece(_)) {
            self.radio.excursion.serve(Job::Update, f, emit);
        } else {
            self.radio.excursion.transmit(Job::Update, f, emit);
        }
    }

    /// The frame went, or gave the channel up: the pace starts again, and
    /// what it was for moves on.
    fn update_sent(&mut self, out: Out, c: Channel, now: Instant) {
        let wait = self.ask_wait();
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
        match out {
            Out::Request => {
                if let Some(g) = s.gather.as_mut() {
                    g.ask = false;
                    g.quiet_since = now;
                    g.quiet_wait = wait;
                }
            }
            Out::Piece(i) => {
                if let Some(v) = s.serving[i].as_mut() {
                    v.sent();
                    if v.current().is_none() {
                        s.serving[i] = None;
                    }
                }
            }
            Out::Announce(_, Turn::Owed) => s.owed = None,
            Out::Announce(_, Turn::Home) => s.home_at = Some(now + ANNOUNCE_EVERY),
            Out::Announce(c, Turn::Other) => {
                s.other_at = Some(now + ANNOUNCE_EVERY);
                s.other = c.next();
            }
            Out::Announce(_, Turn::Grown) => s.grown_at = None,
            Out::Announce(_, Turn::Slot) => {}
        }
    }
}
