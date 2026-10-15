//! The send queue (design §9.3): packets waiting for their hand-over, the
//! one being handed over included, first in first out.
//!
//! A relay takes a packet only if the airtime of everything queued, that
//! packet included, fits in its duty bucket as it stands (§9.2.4, roadmap
//! D29), so the queue never holds more than the bucket can send at once.
//! Its length follows: the bucket's depth, 6 s, over the time of one full
//! frame and its ACK, `138.6 + 11.8 = 150.4 ms`: ⌈39.9⌉ = 40. Each packet
//! taken also costs the ACK that takes it, so from a full bucket with the
//! reply reserve left in at most 38 full frames fit, and for full frames
//! the bucket binds first. Short frames fill the 40 places first: forty
//! hand-overs of them still go out in a few seconds.
//!
//! Each entry is a frame and what kind of hand-over it is, which decides
//! what happens when the hand-over fails (§9.3). An entry whose receivers
//! are all banned waits in its place until a ban ends (§9.3, roadmap
//! D31); the hand-over goes to the first entry that is due.

use crate::event::{Frame, MAX_FRAME};
use crate::ids::MsgId;
use crate::radio::{time_on_air, Channel, MIN_PREAMBLE_SYMBOLS};
use crate::tables::budget::DEPTH;
use crate::tables::fixed::FixedVec;
use crate::time::{Duration, Instant};
use crate::wire::Packet;
use crate::wire::ACK_LEN;

/// Queue length for a bucket of `depth`; see the module documentation.
pub const fn queue_for(depth: Duration) -> usize {
    let per_frame = time_on_air(MAX_FRAME as u8, MIN_PREAMBLE_SYMBOLS).as_micros()
        + time_on_air(ACK_LEN as u8, MIN_PREAMBLE_SYMBOLS).as_micros();
    depth.as_micros().div_ceil(per_frame) as usize
}

/// Queue length for the design's bucket (§9.4).
pub const SEND_QUEUE: usize = queue_for(DEPTH);

const _: () = assert!(SEND_QUEUE == 40);

/// What a queued frame is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A DATA to the next hop toward the destination (§9.2.3). Its next
    /// hop is chosen when its hand-over starts, and again after a failure.
    Forward,
    /// A DATA back to the parent (§9.2.5); dropped if it fails.
    Return,
    /// A DATA straight back to a relay that offered a message this relay
    /// has (§9.2.4 case 6); dropped if it fails.
    Bounce,
    /// A DELIVER to an attached leaf (§10.3); left in the delivery queue
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
    /// Transmissions of a return or send-back that went unanswered, kept
    /// while it waits; a refusal does not count.
    pub tries: u8,
}

impl Outgoing {
    /// `frame` for `kind`, due at once.
    pub const fn new(frame: Frame, kind: Kind) -> Outgoing {
        Outgoing {
            frame,
            kind,
            not_before: Instant::ZERO,
            tries: 0,
        }
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
    frames: FixedVec<Outgoing, SEND_QUEUE>,
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

    /// Whether a packet that needs a place must be refused (§9.2.4).
    pub fn is_full(&self) -> bool {
        self.frames.is_full()
    }

    /// The airtime of every queued frame, sent once each.
    pub fn airtime(&self) -> Duration {
        self.iter()
            .map(|o| time_on_air(o.frame.len, o.frame.preamble_symbols))
            .fold(Duration::ZERO, |a, b| a + b)
    }

    /// Queue `out` last, unless the queue is full.
    pub fn push(&mut self, out: Outgoing) -> Result<(), Full> {
        self.frames.push(out).map_err(|_| Full)
    }

    /// The entry being handed over, or next to be.
    pub fn front(&self) -> Option<&Outgoing> {
        self.frames.as_slice().first()
    }

    /// The front entry, to readdress after a failed hand-over (§9.3 step 6).
    pub fn front_mut(&mut self) -> Option<&mut Outgoing> {
        self.frames.as_mut_slice().first_mut()
    }

    /// Take the front entry: its hand-over is over.
    pub fn pop(&mut self) -> Option<Outgoing> {
        (!self.is_empty()).then(|| self.frames.remove(0))
    }

    /// The entry the next hand-over goes to: the first that is due.
    pub fn next_due(&self, now: Instant) -> Option<usize> {
        self.iter().position(|o| o.not_before <= now)
    }

    /// Move entry `i` to the front; the others keep their order.
    pub fn to_front(&mut self, i: usize) {
        if i < self.len() {
            self.frames.as_mut_slice()[..=i].rotate_right(1);
        }
    }

    /// When the earliest waiting entry is due.
    pub fn earliest_due(&self) -> Option<Instant> {
        self.iter().map(|o| o.not_before).min()
    }

    /// Queued entries, front first.
    pub fn iter(&self) -> impl Iterator<Item = &Outgoing> {
        self.frames.as_slice().iter()
    }

    /// Keep only the entries for which `keep` holds, in order.
    pub fn retain(&mut self, keep: impl FnMut(&Outgoing) -> bool) {
        self.frames.retain(keep)
    }
}

impl Default for SendQueue {
    fn default() -> Self {
        SendQueue::new()
    }
}

#[cfg(test)]
mod tests {
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
        Outgoing::new(frame, Kind::Bounce)
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
        assert_eq!(fit, 38);
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

    #[test]
    fn fifo_and_bounded() {
        let mut q = SendQueue::new();
        assert!(q.is_empty() && q.front().is_none() && q.pop().is_none());
        for i in 0..SEND_QUEUE as u8 {
            assert_eq!(q.push(frame(i)), Ok(()));
        }
        assert!(q.is_full());
        assert_eq!(q.push(frame(99)), Err(Full));
        q.front_mut().unwrap().frame.bytes[0] = 50;
        assert_eq!(q.pop().unwrap().frame.bytes[0], 50);
        assert_eq!(q.front().unwrap().frame.bytes[0], 1);
        assert_eq!(
            q.iter().map(|f| f.frame.bytes[0]).last(),
            Some(SEND_QUEUE as u8 - 1)
        );
        assert_eq!(q.len(), SEND_QUEUE - 1);
        q.retain(|f| f.frame.bytes[0] % 2 == 0);
        assert_eq!(q.len(), (SEND_QUEUE - 1) / 2);
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
        q.to_front(1);
        let order: [u8; 3] = core::array::from_fn(|i| q.iter().nth(i).unwrap().frame.bytes[0]);
        assert_eq!(order, [2, 1, 3]);
        q.pop();
        q.pop();
        q.pop();
        assert_eq!(q.next_due(t(100)), None);
    }
}
