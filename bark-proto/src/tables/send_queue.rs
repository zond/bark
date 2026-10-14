//! The send queue (design §9.3): packets waiting for their hand-over, the
//! one being handed over included, first in first out.
//!
//! Its length is what a relay can send in the time an upstream sender
//! spends retrying a hand-over to it that goes unanswered: the retries
//! after the first attempt, each waiting the longest before it (301 ms,
//! then 602 ms; §9.3) and for the channel, carrying a full frame and
//! waiting out the ACK timeout,
//! `0.90 s + 2 × (500 ms + 138.6 ms + 100 ms) = 2.38 s`, over the time of
//! one full frame and its ACK, `138.6 + 11.8 = 150.4 ms`: ⌈15.8⌉ = 16. A
//! packet that would wait longer gets on sooner by the upstream relay
//! choosing another next hop, which it does at once when this relay, its
//! queue full, answers BUSY (§9.2.4). At the duty-cycle cap the queue
//! drains at the bucket's rate instead, a full frame every 1.4 s, so it
//! can hold a packet about 23 s (design open item O15).
//!
//! Each entry is a frame and what kind of hand-over it is, which decides
//! what happens when the hand-over fails (§9.3).

use crate::event::{Frame, MAX_FRAME};
use crate::ids::MsgId;
use crate::radio::{time_on_air, Channel, MIN_PREAMBLE_SYMBOLS};
use crate::tables::fixed::FixedVec;
use crate::tables::{attempt_span, retry_waits, RETRY_BACKOFF};
use crate::time::Duration;
use crate::wire::Packet;
use crate::wire::ACK_LEN;

/// How long an upstream sender spends retrying a hand-over after its
/// first attempt; see the module documentation.
pub const fn retrying_span(
    ack_timeout: Duration,
    hop_attempts: u8,
    retry_backoff: Duration,
) -> Duration {
    Duration::from_micros(
        attempt_span(ack_timeout).as_micros() * hop_attempts.saturating_sub(1) as u64
            + retry_waits(retry_backoff, hop_attempts).as_micros(),
    )
}

/// Queue length for a sender with these per-hop parameters; see the
/// module documentation.
pub const fn queue_for(ack_timeout: Duration, hop_attempts: u8, retry_backoff: Duration) -> usize {
    let retrying = retrying_span(ack_timeout, hop_attempts, retry_backoff).as_micros();
    let per_frame = time_on_air(MAX_FRAME as u8, MIN_PREAMBLE_SYMBOLS).as_micros()
        + time_on_air(ACK_LEN as u8, MIN_PREAMBLE_SYMBOLS).as_micros();
    retrying.div_ceil(per_frame) as usize
}

/// Queue length for the design's 100 ms ACK timeout, 3 attempts and retry
/// backoff (§9.3).
pub const SEND_QUEUE: usize = queue_for(Duration::from_millis(100), 3, RETRY_BACKOFF);

const _: () = assert!(SEND_QUEUE == 16);

/// What a queued frame is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A DATA to the next hop toward the destination (§9.2.3). Its next
    /// hop is chosen when its hand-over starts, and again after a failure.
    /// `report`: the relay's own application waits for `Sent` or `Failed`.
    Forward {
        /// Whether the application waits for the first hop's ACK.
        report: bool,
    },
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
    /// its hand-over starts.
    pub frame: Frame,
    /// What it is for.
    pub kind: Kind,
}

impl Outgoing {
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
            frames: FixedVec::new(Outgoing {
                frame,
                kind: Kind::Return,
            }),
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
    use crate::node::Config;

    fn frame(tag: u8) -> Outgoing {
        let mut bytes = [0; MAX_FRAME];
        bytes[0] = tag;
        let frame = Frame {
            channel: Channel::new(3).unwrap(),
            preamble_symbols: MIN_PREAMBLE_SYMBOLS,
            len: 1,
            bytes,
        };
        Outgoing {
            frame,
            kind: Kind::Bounce,
        }
    }

    #[test]
    fn size_follows_the_config() {
        let c = Config::default();
        assert_eq!(
            queue_for(c.ack_timeout, c.hop_attempts, c.retry_backoff),
            SEND_QUEUE
        );
        assert_eq!(
            queue_for(c.ack_timeout, 1, c.retry_backoff),
            0,
            "no retries, no time to fill a queue"
        );
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
}
