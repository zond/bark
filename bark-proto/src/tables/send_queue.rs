//! The send queue (spec "Send queue"): packets waiting for their hand-over, the
//! one being handed over included, in the order they arrived.
//!
//! **Regular room.** A relay takes a packet into it only if the airtime of
//! everything queued, that packet included, fits in its duty bucket as it
//! stands (spec "Receiving DATA", roadmap D29). Its length follows: the bucket's depth,
//! 6 s, over the time of one full frame and its ACK, `138.56 + 14.40 =
//! 152.96 ms`: ⌈39.2⌉ = 40. Each packet taken also costs the ACK that
//! takes it, so from a full bucket with the reply reserve left in at most
//! 37 full frames fit, and for full frames the bucket binds first. Short
//! frames fill the 40 places first: forty hand-overs of them still go out
//! in a few seconds.
//!
//! **Hop places.** Beyond the regular room there is one place for each
//! hop count. A packet received with count `h` that the regular room
//! refuses takes the highest free place numbered `h` or lower, whatever
//! the bucket holds; the bucket still decides when it is sent. A packet
//! from a leaf or the relay's own application, count 0, has none, so
//! overload pushes back where messages enter (roadmap D31). The places
//! keep the network from locking up while nothing is dropped for want of
//! room: take the highest-numbered place held anywhere, `k`. Its message
//! has made at least `k` hops and sends `k + 1` or more, and place `k + 1`
//! is free everywhere, so its next hop can take it, unless the count is at
//! the cap, where the message ends (spec "Hop count"). By induction downwards, every
//! message held moves.
//!
//! Each entry is a frame and what kind of hand-over it is, which decides
//! what happens when the hand-over fails (spec "Hand-over"). An entry whose receivers
//! are all banned waits in its place until a ban ends (spec "Overload waits"). The next
//! hand-over goes to the first entry that is due, except that one that has
//! waited goes first, the most travelled first: the place one higher is
//! free only for the message that has come farthest, and a lower one
//! offered first would be refused and ban the neighbour again. Choosing an
//! entry does not move it: the hand-over refers to it where it stands, so
//! of equals the one that arrived first is offered first, however often
//! they wait (roadmap D31 rule 4).

use crate::event::{Frame, MAX_FRAME};
use crate::ids::MsgId;
use crate::radio::{time_on_air, Channel, MIN_PREAMBLE_SYMBOLS};
use crate::tables::budget::DEPTH;
use crate::tables::fixed::FixedVec;
use crate::tables::records::MAX_HOPS;
use crate::time::{Duration, Instant};
use crate::wire::Packet;
use crate::wire::ACK_LEN;

/// Queue length for a bucket of `depth`; see the module documentation.
pub const fn queue_for(depth: Duration) -> usize {
    let per_frame = time_on_air(MAX_FRAME as u8, MIN_PREAMBLE_SYMBOLS).as_micros()
        + time_on_air(ACK_LEN as u8, MIN_PREAMBLE_SYMBOLS).as_micros();
    depth.as_micros().div_ceil(per_frame) as usize
}

/// Queue length for the design's bucket (spec "Duty cycle cap").
pub const SEND_QUEUE: usize = queue_for(DEPTH);

const _: () = assert!(SEND_QUEUE == 40);

/// Hop places: one for each count a packet can be received with and still
/// be sent on, 1 to 254; a packet received at 255 ends at the hop cap.
pub const HOP_PLACES: usize = MAX_HOPS as usize - 1;

/// What a queued frame is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A DATA to the next hop toward the destination (spec "Next hop"). Its next
    /// hop is chosen when its hand-over starts, and again after a failure.
    Forward,
    /// A DATA back to the parent (spec "Return to parent"); dropped if it fails.
    Return,
    /// A DELIVER to an attached leaf (spec "Delivery"); left in the delivery queue
    /// if it fails.
    Deliver,
}

/// A queued frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outgoing {
    /// The frame as it will be sent; a forward's address is filled in when
    /// its hand-over starts, and every frame's channel before each attempt.
    pub frame: Frame,
    /// What it is for.
    pub kind: Kind,
    /// Not handed over before this: it waits for a ban to end.
    pub not_before: Instant,
    /// Transmissions of a return or DELIVER that went unanswered, kept
    /// while it waits; a refusal does not count.
    pub tries: u8,
    /// The hop place it holds; 0 for the regular room.
    pub place: u8,
    /// When a DATA's time left reaches zero (spec "Receiving DATA");
    /// [`Instant::MAX`] for a DELIVER, which the delivery queue's TTL ends.
    pub expires: Instant,
}

impl Outgoing {
    /// `frame` for `kind`, due at once.
    pub const fn new(frame: Frame, kind: Kind) -> Outgoing {
        Outgoing {
            frame,
            kind,
            not_before: Instant::ZERO,
            tries: 0,
            place: 0,
            expires: Instant::MAX,
        }
    }

    /// The hop count the frame carries; a DELIVER's is 0.
    pub fn hops(&self) -> u8 {
        match Packet::decode(self.frame.payload()) {
            Ok(Packet::Data(d)) => d.hops,
            _ => 0,
        }
    }

    /// Whether it has waited for a ban.
    fn waited(&self) -> bool {
        self.not_before > Instant::ZERO
    }

    /// The message the frame carries: a DATA's computed ID (roadmap D15)
    /// or a DELIVER's.
    pub fn msg_id(&self) -> Option<MsgId> {
        match Packet::decode(self.frame.payload()) {
            Ok(Packet::Data(d)) => Some(d.msg_id()),
            Ok(Packet::Deliver(d)) => Some(d.msg_id),
            _ => None,
        }
    }
}

/// The send queue is full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Full;

/// The send queue.
#[derive(Clone)]
pub struct SendQueue {
    frames: FixedVec<Outgoing, { SEND_QUEUE + HOP_PLACES }>,
    /// The entry being handed over, or last chosen to be.
    current: Option<usize>,
}

impl SendQueue {
    /// Empty.
    pub const fn new() -> SendQueue {
        let channel = match Channel::new(0) {
            Some(c) => c,
            None => unreachable!(),
        };
        let frame = Frame {
            channel,
            preamble_symbols: 0,
            len: 0,
            bytes: [0; MAX_FRAME],
        };
        SendQueue {
            frames: FixedVec::new(Outgoing::new(frame, Kind::Return)),
            current: None,
        }
    }

    /// Number of queued frames.
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// Whether nothing is queued.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Empty again, in place.
    pub fn clear(&mut self) {
        self.frames.clear();
        self.current = None;
    }

    /// Entries in the regular room.
    pub fn regular(&self) -> usize {
        self.iter().filter(|o| o.place == 0).count()
    }

    /// The highest free hop place for a packet received with `hops`,
    /// counting `freed` as free.
    pub fn free_place(&self, hops: u8, freed: Option<u8>) -> Option<u8> {
        let mut taken = [false; HOP_PLACES + 1];
        for o in self.iter() {
            taken[o.place as usize] = true;
        }
        if let Some(k) = freed {
            taken[k as usize] = false;
        }
        let top = (hops as usize).min(HOP_PLACES);
        (1..=top).rev().find(|&k| !taken[k]).map(|k| k as u8)
    }

    /// The lowest free hop place: a packet received with this count or
    /// higher finds one.
    pub fn lowest_free_place(&self) -> Option<u8> {
        let mut taken = [false; HOP_PLACES + 1];
        for o in self.iter() {
            taken[o.place as usize] = true;
        }
        (1..=HOP_PLACES).find(|&k| !taken[k]).map(|k| k as u8)
    }

    /// The airtime of every queued frame, sent once each.
    pub fn airtime(&self) -> Duration {
        self.iter()
            .map(|o| time_on_air(o.frame.len, o.frame.preamble_symbols))
            .fold(Duration::ZERO, |a, b| a + b)
    }

    /// Queue `out` last. Admission (spec "Receiving DATA") keeps the regular room and the
    /// hop places within bounds, so only a caller that skipped it finds the
    /// queue full.
    pub fn push(&mut self, out: Outgoing) -> Result<(), Full> {
        self.frames.push(out).map_err(|_| Full)
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
        self.frames.as_slice().get(self.current?)
    }

    /// The hand-over's entry, to address or to let wait.
    pub fn current_mut(&mut self) -> Option<&mut Outgoing> {
        let i = self.current?;
        self.frames.as_mut_slice().get_mut(i)
    }

    /// Take the hand-over's entry out: its hand-over is over.
    pub fn take_current(&mut self) -> Option<Outgoing> {
        let i = self.current.take()?;
        Some(self.frames.remove(i))
    }

    /// The entry the next hand-over goes to: of those that have waited and
    /// are due, the most travelled, the earliest of equals; else the first
    /// that is due.
    pub fn next_due(&self, now: Instant) -> Option<usize> {
        let due = |o: &Outgoing| o.not_before <= now;
        self.iter()
            .enumerate()
            .filter(|(_, o)| due(o) && o.waited())
            .max_by(|(i, a), (j, b)| a.hops().cmp(&b.hops()).then(j.cmp(i)))
            .map(|(i, _)| i)
            .or_else(|| self.iter().position(due))
    }

    /// When the earliest waiting entry is due.
    pub fn earliest_due(&self) -> Option<Instant> {
        self.iter().map(|o| o.not_before).min()
    }

    /// Queued entries, in the order they arrived.
    pub fn iter(&self) -> impl Iterator<Item = &Outgoing> {
        self.frames.as_slice().iter()
    }

    /// Keep only the entries for which `keep` holds, in order; `keep` is
    /// told whether the entry is the hand-over's, which follows its entry
    /// or ends with it.
    pub fn retain(&mut self, mut keep: impl FnMut(bool, &Outgoing) -> bool) {
        let mut i = 0;
        while i < self.len() {
            if keep(self.current == Some(i), &self.frames.as_slice()[i]) {
                i += 1;
                continue;
            }
            self.frames.remove(i);
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

    fn frame(tag: u8) -> Outgoing {
        let mut bytes = [0; MAX_FRAME];
        bytes[0] = tag;
        let frame = Frame {
            channel: Channel::new(3).unwrap(),
            preamble_symbols: MIN_PREAMBLE_SYMBOLS,
            len: 1,
            bytes,
        };
        Outgoing::new(frame, Kind::Return)
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
        assert_eq!(fit, 37);
        assert!((fit as usize) < SEND_QUEUE);
    }

    #[test]
    fn airtime_counts_every_frame() {
        let mut q = SendQueue::new();
        assert_eq!(q.airtime(), Duration::ZERO);
        q.push(frame(1)).unwrap();
        q.push(frame(2)).unwrap();
        let one = time_on_air(1, MIN_PREAMBLE_SYMBOLS);
        assert_eq!(q.airtime(), one + one);
    }

    fn tags(q: &SendQueue) -> std::vec::Vec<u8> {
        q.iter().map(|f| f.frame.bytes[0]).collect()
    }

    #[test]
    fn arrival_order_and_bounded() {
        let mut q = SendQueue::new();
        assert!(q.is_empty() && q.current().is_none() && q.take_current().is_none());
        for i in 0..SEND_QUEUE as u8 {
            assert_eq!(q.push(frame(i)), Ok(()));
        }
        assert_eq!(q.regular(), SEND_QUEUE);
        // The hand-over's entry is changed and taken where it stands.
        q.select(5);
        q.current_mut().unwrap().frame.bytes[0] = 50;
        assert_eq!(q.take_current().unwrap().frame.bytes[0], 50);
        assert!(q.current().is_none());
        assert_eq!(&tags(&q)[4..6], &[4, 6]);
        assert_eq!(q.len(), SEND_QUEUE - 1);
        // Removing entries before the hand-over's leaves it on its entry.
        q.select(10);
        q.retain(|current, f| current || f.frame.bytes[0] % 2 == 0);
        assert_eq!(q.current().unwrap().frame.bytes[0], 11);
        assert_eq!(q.len(), 21, "20 even tags and the current 11");
        q.retain(|current, _| !current);
        assert!(q.current().is_none());
    }

    #[test]
    fn every_place_fits() {
        let mut q = SendQueue::new();
        for i in 0..SEND_QUEUE {
            q.push(frame(i as u8)).unwrap();
        }
        for k in (1..=HOP_PLACES as u8).rev() {
            assert_eq!(q.free_place(MAX_HOPS, None), Some(k));
            let mut o = frame(0);
            o.place = k;
            q.push(o).unwrap();
        }
        assert_eq!(q.free_place(MAX_HOPS, None), None);
        assert_eq!(q.push(frame(0)), Err(Full));
    }

    #[test]
    fn a_packet_takes_the_highest_free_place_not_above_its_count() {
        let mut q = SendQueue::new();
        assert_eq!(q.free_place(0, None), None, "nothing for count 0");
        assert_eq!(q.free_place(3, None), Some(3));
        for k in [3, 1] {
            let mut o = frame(k);
            o.place = k;
            q.push(o).unwrap();
        }
        assert_eq!(q.regular(), 0);
        assert_eq!(q.free_place(3, None), Some(2));
        assert_eq!(q.free_place(1, None), None);
        assert_eq!(q.free_place(1, Some(1)), Some(1));
        // The place frees when its frame leaves.
        q.select(0);
        q.take_current();
        assert_eq!(q.free_place(5, None), Some(5));
        assert_eq!(q.free_place(3, None), Some(3));
    }

    /// A DATA frame with `hops` that has waited until `due` ms, if given.
    fn data(tag: u8, hops: u8, due: Option<u64>) -> Outgoing {
        use crate::ids::LeafId;
        use crate::position::ShortName;
        use crate::wire::Data;
        let d = Data {
            sender_home: Channel::new(3).unwrap(),
            dst_hop: ShortName(1),
            src_hop: ShortName(2),
            dst_pos: crate::position::Position { lat: 0, lon: 0 },
            dst_leaf: LeafId::RELAY,
            hops,
            time_left: 1,
            payload: &[tag],
        };
        let mut bytes = [0; MAX_FRAME];
        let len = Packet::Data(d).encode(&mut bytes).unwrap() as u8;
        let frame = Frame {
            channel: d.sender_home,
            preamble_symbols: MIN_PREAMBLE_SYMBOLS,
            len,
            bytes,
        };
        let mut o = Outgoing::new(frame, Kind::Forward);
        if let Some(ms) = due {
            o.not_before = Instant::ZERO + Duration::from_millis(ms);
        }
        o
    }

    /// When bans end, the most travelled of the frames that waited goes
    /// first, ahead of the queue's order; one that never waited keeps it.
    #[test]
    fn the_most_travelled_waiting_frame_goes_first() {
        let t = |ms| Instant::ZERO + Duration::from_millis(ms);
        let mut q = SendQueue::new();
        q.push(data(0, 1, None)).unwrap();
        q.push(data(1, 2, Some(10))).unwrap();
        q.push(data(2, 5, Some(10))).unwrap();
        q.push(data(3, 5, Some(10))).unwrap();
        q.push(data(4, 9, Some(20))).unwrap();
        assert_eq!(q.next_due(t(5)), Some(0), "none has waited its time");
        assert_eq!(q.next_due(t(10)), Some(2), "the earlier of equals");
        assert_eq!(q.next_due(t(20)), Some(4));
        q.select(4);
        q.take_current();
        assert_eq!(q.next_due(t(20)), Some(2));
        q.retain(|_, o| !o.waited());
        assert_eq!(q.next_due(t(20)), Some(0), "the one that never waited");
    }

    #[test]
    fn waiting_entries_keep_their_place() {
        let t = |ms| Instant::ZERO + Duration::from_millis(ms);
        let mut q = SendQueue::new();
        for (tag, due) in [(1, 30), (2, 10), (3, 0)] {
            let mut o = frame(tag);
            o.not_before = t(due);
            q.push(o).unwrap();
        }
        assert_eq!(q.earliest_due(), Some(t(0)));
        assert_eq!(q.next_due(t(5)), Some(2));
        assert_eq!(q.next_due(t(10)), Some(1));
        q.select(1);
        assert_eq!(tags(&q), [1, 2, 3], "chosen, not moved");
        q.retain(|_, _| false);
        assert_eq!(q.next_due(t(100)), None);
    }
}
