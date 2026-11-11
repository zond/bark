//! The send queue (spec "Send queue"): packets waiting for their
//! hand-over, the one being handed over included, in the order they
//! arrived, in the regular room or a hop place.
//!
//! The hop places keep the network from locking up while nothing is
//! dropped for want of room: take the highest-numbered place held
//! anywhere, `k`. Its message sends `k + 1` or more, and place `k + 1` is
//! free everywhere, so its next hop can take it, unless the count is at
//! the cap, where the message ends. By induction downwards, every message
//! held moves. That holds while fewer than [`HOP_SLOTS`] places are held.
//!
//! Where a forward goes is not kept: the relay routes every entry each
//! time it may send, and tells [`SendQueue::next_due`] which may go now.
//! Choosing an entry does not move it: the hand-over refers to it where
//! it stands, so of equals the one that arrived first is offered first,
//! however often they wait.

use crate::event::{Frame, MAX_FRAME};
use crate::ids::{MsgId, NodeId};
use crate::position::Position;
use crate::radio::{time_on_air, Channel, MIN_PREAMBLE_SYMBOLS};
use crate::tables::budget::DEPTH;
use crate::tables::fixed::FixedVec;
use crate::tables::records::MAX_HOPS;
use crate::time::{Duration, Instant};
use crate::wire::{Data, Packet, ACK_LEN, DATA_HEADER_LEN, MAX_PAYLOAD};

/// A full frame and the ACK that takes it, µs.
const HAND_OVER_US: u64 = time_on_air(MAX_FRAME as u8, MIN_PREAMBLE_SYMBOLS).as_micros()
    + time_on_air(ACK_LEN as u8, MIN_PREAMBLE_SYMBOLS).as_micros();

/// Queue length for a bucket of `depth`: the depth over a full frame and
/// its ACK, ⌈6 s / 155.52 ms⌉ = 39 for the design's (spec "Send queue").
pub const fn queue_for(depth: Duration) -> usize {
    depth.as_micros().div_ceil(HAND_OVER_US) as usize
}

/// Queue length for the design's bucket (spec "Duty cycle cap").
pub const SEND_QUEUE: usize = queue_for(DEPTH);

const _: () = assert!(SEND_QUEUE == 39);

/// The gate's budget, the longest a relay lets a message wait here for
/// its next hop (spec "Send queue"): above the simulated city's longest
/// wait at the design rate, 20 to 30 s, so it refuses only a slow hop.
pub const HOP_WAIT: Duration = Duration::from_secs(30);

/// Hop places: one for each count a packet can be received with and still
/// be sent on, 1 to 254; a packet received at 255 ends at the hop cap.
pub const HOP_PLACES: usize = MAX_HOPS as usize - 1;

/// Hop places held at once (spec "Send queue"). The simulated city at the
/// design rate held at most 3 at once; holding 64 needs packets of count
/// 64 or more.
pub const HOP_SLOTS: usize = 64;

/// What a queued message is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A message to send on, routed when its hand-over starts
    /// (spec "Next hop").
    Forward,
    /// A forward whose hand-over goes back to the parent
    /// (spec "Return to parent").
    Return,
    /// A DATA to a sleepy node in its window (spec "Delivery"); left in
    /// the delivery queue if it fails.
    Deliver,
    /// A DATA from the delivery queue to its node whether or not it is in
    /// a window: with the long preamble, or to a node heard as not sleepy
    /// (spec "Delivery"). Left in the delivery queue if it fails.
    Direct,
}

impl Kind {
    /// Whether it carries a message from the delivery queue to its node.
    pub fn delivers(self) -> bool {
        matches!(self, Kind::Deliver | Kind::Direct)
    }
}

/// Where a queued message is in its hand-over (spec "Hand-over",
/// `node/transfer.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Waiting its turn, due at `not_before`.
    Queued,
    /// The application's message for a node this one holds for, set aside
    /// until the delivery queue has a place (spec "Return to parent"); it
    /// keeps its place and its airtime in admission.
    Held,
    /// Addressed, and waiting for this until the instant the hand-over in
    /// flight keeps.
    Waiting(Wait),
    /// On the radio: its channel checked, then its frame on air.
    Radio,
    /// Listening at home for its ACK until the instant the hand-over in
    /// flight keeps.
    AwaitAck,
}

/// What a hand-over waits for before its listen-before-talk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wait {
    /// Its turn: nothing, once the radio is free.
    Turn,
    /// A random backoff after a busy check.
    Backoff,
    /// The receiver's channel, after listen-before-talk gave up on it.
    Channel,
    /// An owed reply, which goes first.
    Reply,
    /// The duty-cycle budget.
    Budget,
    /// The retry wait after a missed ACK.
    Retry,
}

/// A message as a node holds it: what its ID is taken of
/// (spec "Packet formats").
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Message {
    /// Its destination's position.
    pub dst_pos: Position,
    /// Its destination node.
    pub dst_id: NodeId,
    len: u8,
    payload: [u8; MAX_PAYLOAD],
}

impl Message {
    /// The message to `(dst_pos, dst_id)` carrying `payload`, of at most
    /// [`MAX_PAYLOAD`] bytes, with its position as the wire carries it.
    pub fn new(dst_pos: Position, dst_id: NodeId, payload: &[u8]) -> Message {
        debug_assert!(payload.len() <= MAX_PAYLOAD, "{} bytes", payload.len());
        let len = payload.len().min(MAX_PAYLOAD);
        let mut m = Message {
            dst_pos: Position::decode(&dst_pos.encode()),
            dst_id,
            len: len as u8,
            payload: [0; MAX_PAYLOAD],
        };
        m.payload[..len].copy_from_slice(&payload[..len]);
        m
    }

    /// The message a DATA carries.
    pub fn of(d: &Data<'_>) -> Message {
        Message::new(d.dst_pos, d.dst_id, d.payload)
    }

    /// The payload.
    pub fn payload(&self) -> &[u8] {
        &self.payload[..self.len as usize]
    }

    /// Its ID.
    pub fn id(&self) -> MsgId {
        MsgId::of(self.dst_pos, self.dst_id, self.payload())
    }

    /// The airtime of a DATA carrying it with `preamble` symbols.
    pub fn airtime(&self, preamble: u16) -> Duration {
        time_on_air((DATA_HEADER_LEN + self.payload().len()) as u8, preamble)
    }
}

impl core::fmt::Debug for Message {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Message")
            .field("dst_pos", &self.dst_pos)
            .field("dst_id", &self.dst_id)
            .field("payload", &self.payload())
            .finish()
    }
}

/// A queued message and its hand-over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outgoing {
    /// The message.
    pub message: Message,
    /// The count its DATA carries: a forward's received count plus one
    /// until its hand-over stamps the count it goes with
    /// (spec "Hop count").
    pub hops: u8,
    /// The receiver; [`NodeId::NONE`] until a forward is addressed when
    /// its hand-over starts.
    pub to: NodeId,
    /// The channel it goes on, the receiver's home, read again before each
    /// attempt.
    pub channel: Channel,
    /// Its preamble, in symbols.
    pub preamble: u16,
    /// What it is for.
    pub kind: Kind,
    /// Where its hand-over is.
    pub state: State,
    /// Not handed over before this.
    pub not_before: Instant,
    /// Transmissions that went unanswered: of the hand-over under way,
    /// and of a return or a DATA to a sleepy node kept while it waits; a
    /// refusal does not count.
    pub tries: u8,
    /// The hop place it holds; 0 for the regular room.
    pub place: u8,
    /// Its next hop has been banned while it was queued, whichever next
    /// hop it is routed to now: it goes ahead of those that have not
    /// waited (spec "Overload waits").
    pub waited: bool,
    /// Where its message's record was last found in the record table: a
    /// hint, checked against the ID before it is used.
    pub record: u16,
    /// The neighbour-table slot, plus one, of where it was last routed;
    /// 0 before it was. With `since`, a sample of that neighbour's service
    /// time when it takes it (spec "Neighbour table").
    pub toward: u8,
    /// When it began to wait for that neighbour here: when it was queued,
    /// or routed to another than before.
    pub since: Instant,
    /// The neighbours it held for through their demotion, one bit per
    /// neighbour-table slot: tried for it while it is queued here, though
    /// none has it (spec "Next hop").
    pub passed: u64,
    /// The neighbours that refused it, routed past while their ban
    /// lasts, one bit per neighbour-table slot (spec "Next hop").
    pub avoid: u64,
}

impl Outgoing {
    /// `message` for `kind`, carrying count `hops`, to `to` on `channel`
    /// with `preamble` symbols, due at once.
    pub const fn new(
        message: Message,
        hops: u8,
        to: NodeId,
        channel: Channel,
        preamble: u16,
        kind: Kind,
    ) -> Outgoing {
        Outgoing {
            message,
            hops,
            to,
            channel,
            preamble,
            kind,
            state: State::Queued,
            not_before: Instant::ZERO,
            tries: 0,
            place: 0,
            waited: false,
            record: 0,
            toward: 0,
            since: Instant::ZERO,
            passed: 0,
            avoid: 0,
        }
    }

    /// The DATA it sends from `me`, at home on `home`.
    pub fn data(&self, me: NodeId, home: Channel) -> Data<'_> {
        Data {
            sender_home: home,
            dst_hop: self.to,
            src_hop: me,
            dst_pos: self.message.dst_pos,
            dst_id: self.message.dst_id,
            hops: self.hops,
            payload: self.message.payload(),
        }
    }

    /// The frame it goes in from `me`, at home on `home`.
    pub fn frame(&self, me: NodeId, home: Channel) -> Frame {
        let mut f = Frame {
            channel: self.channel,
            preamble_symbols: self.preamble,
            len: 0,
            bytes: [0; MAX_FRAME],
        };
        // The payload fits: a message holds at most `MAX_PAYLOAD` bytes.
        f.len = Packet::Data(self.data(me, home))
            .encode(&mut f.bytes)
            .unwrap_or(0) as u8;
        f
    }

    /// The airtime of its frame.
    pub fn airtime(&self) -> Duration {
        self.message.airtime(self.preamble)
    }

    /// Its message's ID.
    pub fn msg_id(&self) -> MsgId {
        self.message.id()
    }

    /// Go round neighbour-table `slot` while it is banned.
    pub fn go_round(&mut self, slot: usize) {
        self.avoid |= 1u64 << slot;
    }

    /// Keep going round only the slots of `banned`: a ban that ended
    /// ends the detour.
    pub fn keep_avoiding(&mut self, banned: u64) {
        self.avoid &= banned;
    }

    /// Whether it is set aside for the delivery queue.
    pub fn held(&self) -> bool {
        self.state == State::Held
    }

    /// Whether its hand-over is under way: addressed and waiting, on the
    /// radio or waiting for its ACK.
    pub fn in_flight(&self) -> bool {
        !matches!(self.state, State::Queued | State::Held)
    }

    /// Whether it has waited for a ban, or a wait of its own, since it was
    /// queued.
    fn waited(&self) -> bool {
        self.waited || self.not_before > Instant::ZERO
    }
}

#[cfg(test)]
impl Outgoing {
    /// The message `d` carries, to its `dst_hop` at its count, queued for
    /// `kind` on `channel` with `preamble` symbols.
    pub(crate) fn of(d: &Data<'_>, channel: Channel, preamble: u16, kind: Kind) -> Outgoing {
        Outgoing::new(Message::of(d), d.hops, d.dst_hop, channel, preamble, kind)
    }
}

/// The send queue is full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Full;

/// The send queue.
///
/// Its few fields first, then the items: an event reads the first and
/// seldom the items ([`crate::Node`]'s layout).
#[derive(Clone)]
#[repr(C)]
pub struct SendQueue {
    /// The entry being handed over, or last chosen to be.
    current: Option<usize>,
    entries: FixedVec<Outgoing, { SEND_QUEUE + HOP_SLOTS }>,
}

impl SendQueue {
    /// Empty.
    pub const fn new() -> SendQueue {
        let channel = match Channel::new(0) {
            Some(c) => c,
            None => unreachable!(),
        };
        let message = Message {
            dst_pos: Position { lat: 0, lon: 0 },
            dst_id: NodeId::NONE,
            len: 0,
            payload: [0; MAX_PAYLOAD],
        };
        let fill = Outgoing::new(message, 0, NodeId::NONE, channel, 0, Kind::Return);
        SendQueue {
            entries: FixedVec::new(fill),
            current: None,
        }
    }

    /// Number of queued messages.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nothing is queued.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Empty again, in place.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.current = None;
    }

    /// Entries in the regular room.
    pub fn regular(&self) -> usize {
        self.iter().filter(|o| o.place == 0).count()
    }

    /// The highest free hop place for a packet received with `hops`,
    /// counting `freed` as free: none while [`HOP_SLOTS`] are held.
    pub fn free_place(&self, hops: u8, freed: Option<u8>) -> Option<u8> {
        let mut taken = [false; HOP_PLACES + 1];
        let mut held = 0;
        for o in self.iter().filter(|o| o.place > 0) {
            taken[o.place as usize] = true;
            held += 1;
        }
        if let Some(k) = freed.filter(|&k| k > 0) {
            taken[k as usize] = false;
            held -= 1;
        }
        if held >= HOP_SLOTS {
            return None;
        }
        let top = (hops as usize).min(HOP_PLACES);
        (1..=top).rev().find(|&k| !taken[k]).map(|k| k as u8)
    }

    /// The lowest free hop place: a packet received with this count or
    /// higher finds one.
    pub fn lowest_free_place(&self) -> Option<u8> {
        let mut taken = [false; HOP_PLACES + 1];
        let mut held = 0;
        for o in self.iter().filter(|o| o.place > 0) {
            taken[o.place as usize] = true;
            held += 1;
        }
        if held >= HOP_SLOTS {
            return None;
        }
        (1..=HOP_PLACES).find(|&k| !taken[k]).map(|k| k as u8)
    }

    /// The airtime of every queued message, sent once each.
    pub fn airtime(&self) -> Duration {
        self.iter()
            .map(Outgoing::airtime)
            .fold(Duration::ZERO, |a, b| a + b)
    }

    /// Queue `out` last. Admission (spec "Receiving DATA") keeps the queue
    /// within bounds, so only a caller that skipped it finds it full.
    pub fn push(&mut self, out: Outgoing) -> Result<(), Full> {
        self.entries.push(out).map_err(|_| Full)
    }

    /// Make entry `i` the hand-over's. The queue keeps its order.
    pub fn select(&mut self, i: usize) {
        self.current = (i < self.len()).then_some(i);
    }

    /// Where the hand-over's entry is.
    pub fn current_index(&self) -> Option<usize> {
        self.current
    }

    /// The hand-over's entry.
    pub fn current(&self) -> Option<&Outgoing> {
        self.entries.as_slice().get(self.current?)
    }

    /// The hand-over's entry, to address or to let wait.
    pub fn current_mut(&mut self) -> Option<&mut Outgoing> {
        let i = self.current?;
        self.entries.as_mut_slice().get_mut(i)
    }

    /// Take the hand-over's entry out: its hand-over is over.
    pub fn take_current(&mut self) -> Option<Outgoing> {
        let i = self.current.take()?;
        Some(self.entries.remove(i))
    }

    /// No entry is the hand-over's.
    pub fn deselect(&mut self) {
        self.current = None;
    }

    /// Take entry `i` out, as it stands.
    pub fn remove(&mut self, i: usize) -> Option<Outgoing> {
        if i >= self.len() {
            return None;
        }
        self.current = match self.current {
            Some(c) if c == i => None,
            Some(c) if c > i => Some(c - 1),
            c => c,
        };
        Some(self.entries.remove(i))
    }

    /// The entry the next hand-over goes to, of those `sendable` says may
    /// go now (spec "Send queue", choosing what to send).
    pub fn next_due(
        &self,
        now: Instant,
        sendable: impl Fn(usize, &Outgoing) -> bool,
    ) -> Option<usize> {
        // Asked on every event; mostly there is nothing queued.
        if self.is_empty() {
            return None;
        }
        let due = |i: usize, o: &Outgoing| !o.held() && o.not_before <= now && sendable(i, o);
        if let Some(i) = self.delivery_due(now) {
            return Some(i);
        }
        self.iter()
            .enumerate()
            .filter(|&(i, o)| due(i, o) && o.waited())
            .max_by(|(i, a), (j, b)| a.hops.cmp(&b.hops).then(j.cmp(i)))
            .map(|(i, _)| i)
            .or_else(|| self.iter().enumerate().position(|(i, o)| due(i, o)))
    }

    /// Neighbour-table `slot` has a new occupant, which no entry has held
    /// for.
    pub fn slot_reoccupied(&mut self, slot: usize) {
        for o in self.entries.as_mut_slice() {
            o.passed &= !(1u64 << slot);
            o.keep_avoiding(!(1u64 << slot));
        }
    }

    /// Entry `i`, to mark.
    pub fn get_mut(&mut self, i: usize) -> Option<&mut Outgoing> {
        self.entries.as_mut_slice().get_mut(i)
    }

    /// Entry `i`.
    pub fn get(&self, i: usize) -> Option<&Outgoing> {
        self.entries.as_slice().get(i)
    }

    /// The first DATA from the delivery queue that is due.
    pub fn delivery_due(&self, now: Instant) -> Option<usize> {
        self.iter()
            .position(|o| o.kind.delivers() && !o.held() && o.not_before <= now)
    }

    /// The entry of message `id`, if it waits beyond `now`, is due at
    /// `now`.
    pub fn due_now(&mut self, id: MsgId, now: Instant) {
        for o in self.entries.as_mut_slice() {
            if !o.held() && o.not_before > now && o.msg_id() == id {
                o.not_before = now;
            }
        }
    }

    /// Queued entries, in the order they arrived.
    pub fn iter(&self) -> impl Iterator<Item = &Outgoing> {
        self.entries.as_slice().iter()
    }

    /// Keep only the entries for which `keep` holds, in order; `keep` is
    /// told whether the entry is the hand-over's, which follows its entry
    /// or ends with it.
    pub fn retain(&mut self, mut keep: impl FnMut(bool, &Outgoing) -> bool) {
        let mut i = 0;
        while i < self.len() {
            if keep(self.current == Some(i), &self.entries.as_slice()[i]) {
                i += 1;
                continue;
            }
            self.entries.remove(i);
            self.current = match self.current {
                Some(c) if c == i => None,
                Some(c) if c > i => Some(c - 1),
                c => c,
            };
        }
    }
}

impl Default for SendQueue {
    fn default() -> Self {
        SendQueue::new()
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::tables::budget::REPLY_RESERVE;

    fn message(tag: u8) -> Message {
        Message::new(Position::default(), NodeId([3; 6]), &[tag])
    }

    fn entry(tag: u8) -> Outgoing {
        let ch = Channel::new(3).unwrap();
        Outgoing::new(
            message(tag),
            1,
            NodeId([1; 6]),
            ch,
            MIN_PREAMBLE_SYMBOLS,
            Kind::Return,
        )
    }

    #[test]
    fn size_follows_the_bucket() {
        assert_eq!(queue_for(Duration::from_secs(6)), SEND_QUEUE);
        assert_eq!(queue_for(Duration::ZERO), 0);
        // What a full bucket takes in full frames, each with the ACK that
        // took it, leaving the reply reserve: fewer than the places.
        let per = time_on_air(MAX_FRAME as u8, MIN_PREAMBLE_SYMBOLS)
            + time_on_air(ACK_LEN as u8, MIN_PREAMBLE_SYMBOLS);
        let fit = (DEPTH - REPLY_RESERVE).as_micros() / per.as_micros();
        assert_eq!(fit, 33);
        assert!((fit as usize) < SEND_QUEUE);
    }

    #[test]
    fn airtime_counts_every_entry() {
        let mut q = SendQueue::new();
        assert_eq!(q.airtime(), Duration::ZERO);
        q.push(entry(1)).unwrap();
        q.push(entry(2)).unwrap();
        let one = time_on_air((DATA_HEADER_LEN + 1) as u8, MIN_PREAMBLE_SYMBOLS);
        assert_eq!(q.airtime(), one + one);
    }

    fn tags(q: &SendQueue) -> std::vec::Vec<u8> {
        q.iter().map(|o| o.message.payload()[0]).collect()
    }

    #[test]
    fn arrival_order_and_bounded() {
        let mut q = SendQueue::new();
        assert!(q.is_empty() && q.current().is_none() && q.take_current().is_none());
        for i in 0..SEND_QUEUE as u8 {
            assert_eq!(q.push(entry(i)), Ok(()));
        }
        assert_eq!(q.regular(), SEND_QUEUE);
        // The hand-over's entry is changed and taken where it stands.
        q.select(5);
        q.current_mut().unwrap().message = message(50);
        assert_eq!(q.take_current().unwrap().message.payload(), [50]);
        assert!(q.current().is_none());
        assert_eq!(&tags(&q)[4..6], &[4, 6]);
        assert_eq!(q.len(), SEND_QUEUE - 1);
        // Removing entries before the hand-over's leaves it on its entry.
        q.select(10);
        q.retain(|current, o| current || o.message.payload()[0] % 2 == 0);
        assert_eq!(q.current().unwrap().message.payload(), [11]);
        assert_eq!(q.len(), 21, "20 even tags and the current 11");
        q.retain(|current, _| !current);
        assert!(q.current().is_none());
    }

    #[test]
    fn every_place_fits() {
        let mut q = SendQueue::new();
        for i in 0..SEND_QUEUE {
            q.push(entry(i as u8)).unwrap();
        }
        for k in (HOP_PLACES - HOP_SLOTS + 1..=HOP_PLACES).rev() {
            assert_eq!(q.free_place(MAX_HOPS, None), Some(k as u8));
            let mut o = entry(0);
            o.place = k as u8;
            q.push(o).unwrap();
        }
        // Every slot held: no place, though lower numbers are unheld.
        assert_eq!(q.free_place(MAX_HOPS, None), None);
        assert_eq!(q.free_place(5, None), None);
        assert_eq!(q.lowest_free_place(), None);
        assert_eq!(q.free_place(5, Some(HOP_PLACES as u8)), Some(5));
        assert_eq!(q.push(entry(0)), Err(Full));
    }

    #[test]
    fn a_packet_takes_the_highest_free_place_not_above_its_count() {
        let mut q = SendQueue::new();
        assert_eq!(q.free_place(0, None), None, "nothing for count 0");
        assert_eq!(q.free_place(3, None), Some(3));
        for k in [3, 1] {
            let mut o = entry(k);
            o.place = k;
            q.push(o).unwrap();
        }
        assert_eq!(q.regular(), 0);
        assert_eq!(q.free_place(3, None), Some(2));
        assert_eq!(q.free_place(1, None), None);
        assert_eq!(q.free_place(1, Some(1)), Some(1));
        // The place frees when its entry leaves.
        q.select(0);
        q.take_current();
        assert_eq!(q.free_place(5, None), Some(5));
        assert_eq!(q.free_place(3, None), Some(3));
    }

    impl SendQueue {
        fn next_due_all(&self, now: Instant) -> Option<usize> {
            self.next_due(now, |_, _| true)
        }
    }

    /// A forward with `hops` that has waited until `due` ms, if given.
    fn data(tag: u8, hops: u8, due: Option<u64>) -> Outgoing {
        let ch = Channel::new(3).unwrap();
        let mut o = Outgoing::new(
            message(tag),
            hops,
            NodeId([1; 6]),
            ch,
            MIN_PREAMBLE_SYMBOLS,
            Kind::Forward,
        );
        if let Some(ms) = due {
            o.not_before = Instant::ZERO + Duration::from_millis(ms);
        }
        o
    }

    /// The most travelled of the due entries that have waited, here for
    /// waits of their own, goes first, ahead of the queue's order, also
    /// after its wait; one that never waited keeps its place.
    #[test]
    fn the_most_travelled_waiting_entry_goes_first() {
        let t = |ms| Instant::ZERO + Duration::from_millis(ms);
        let mut q = SendQueue::new();
        q.push(data(0, 1, None)).unwrap();
        q.push(data(1, 2, Some(10))).unwrap();
        q.push(data(2, 5, Some(10))).unwrap();
        q.push(data(3, 5, Some(10))).unwrap();
        q.push(data(4, 9, Some(20))).unwrap();
        assert_eq!(q.next_due_all(t(5)), Some(0), "none has waited its time");
        assert_eq!(q.next_due_all(t(10)), Some(2), "the earlier of equals");
        assert_eq!(q.next_due_all(t(20)), Some(4));
        q.select(4);
        q.take_current();
        assert_eq!(q.next_due_all(t(20)), Some(2));
        q.retain(|_, o| !o.waited());
        assert_eq!(q.next_due_all(t(20)), Some(0), "the one that never waited");
    }

    #[test]
    fn waiting_entries_keep_their_place() {
        let t = |ms| Instant::ZERO + Duration::from_millis(ms);
        let mut q = SendQueue::new();
        for (tag, due) in [(1, 30), (2, 10), (3, 0)] {
            let mut o = entry(tag);
            o.not_before = t(due);
            q.push(o).unwrap();
        }
        assert_eq!(q.next_due_all(t(5)), Some(2));
        assert_eq!(q.next_due_all(t(10)), Some(1));
        q.select(1);
        assert_eq!(tags(&q), [1, 2, 3], "chosen, not moved");
        q.retain(|_, _| false);
        assert_eq!(q.next_due_all(t(100)), None);
    }
}
