//! The send queue (design §9.3): packets waiting for their hand-over, the
//! one being handed over included, first in first out.
//!
//! Its length is what a relay can send in the time its upstream sender
//! spends retrying a hand-over to it: the retries after the first attempt,
//! each waiting the longest for the channel, carrying a full frame and
//! waiting out the ACK timeout,
//! `2 × (500 ms + 138.6 ms + 100 ms) = 1.48 s`, over the time of one full
//! frame and its ACK, `138.6 + 11.8 = 150.4 ms`: ⌈9.8⌉ = 10. A packet that
//! would wait longer gets on sooner by the upstream relay choosing another
//! next hop, which it does when this relay, its queue full, does not ACK.

use crate::event::{Frame, MAX_FRAME};
use crate::radio::{time_on_air, Channel, MIN_PREAMBLE_SYMBOLS};
use crate::tables::attempt_span;
use crate::tables::fixed::FixedVec;
use crate::time::Duration;
use crate::wire::ACK_LEN;

/// Queue length for a sender with these per-hop parameters; see the
/// module documentation.
pub const fn queue_for(ack_timeout: Duration, hop_attempts: u8) -> usize {
    let retrying = attempt_span(ack_timeout).as_micros() * hop_attempts.saturating_sub(1) as u64;
    let per_frame = time_on_air(MAX_FRAME as u8, MIN_PREAMBLE_SYMBOLS).as_micros()
        + time_on_air(ACK_LEN as u8, MIN_PREAMBLE_SYMBOLS).as_micros();
    retrying.div_ceil(per_frame) as usize
}

/// Queue length for the design's 100 ms ACK timeout and 3 attempts (§9.3).
pub const SEND_QUEUE: usize = queue_for(Duration::from_millis(100), 3);

const _: () = assert!(SEND_QUEUE == 10);

/// The send queue is full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Full;

/// The send queue.
#[derive(Clone)]
pub struct SendQueue {
    frames: FixedVec<Frame, SEND_QUEUE>,
}

impl SendQueue {
    /// Empty.
    pub const fn new() -> SendQueue {
        let channel = match Channel::new(0) {
            Some(c) => c,
            None => unreachable!(),
        };
        let fill = Frame { channel, preamble_symbols: 0, len: 0, bytes: [0; MAX_FRAME] };
        SendQueue { frames: FixedVec::new(fill) }
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

    /// Queue `frame` last, unless the queue is full.
    pub fn push(&mut self, frame: Frame) -> Result<(), Full> {
        self.frames.push(frame).map_err(|_| Full)
    }

    /// The frame being handed over, or next to be.
    pub fn front(&self) -> Option<&Frame> {
        self.frames.as_slice().first()
    }

    /// The front frame, to readdress after a failed hand-over (§9.3 step 6).
    pub fn front_mut(&mut self) -> Option<&mut Frame> {
        self.frames.as_mut_slice().first_mut()
    }

    /// Take the front frame: its hand-over is over.
    pub fn pop(&mut self) -> Option<Frame> {
        (!self.is_empty()).then(|| self.frames.remove(0))
    }

    /// Queued frames, front first.
    pub fn iter(&self) -> impl Iterator<Item = &Frame> {
        self.frames.as_slice().iter()
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

    fn frame(tag: u8) -> Frame {
        let mut bytes = [0; MAX_FRAME];
        bytes[0] = tag;
        Frame { channel: Channel::new(3).unwrap(), preamble_symbols: MIN_PREAMBLE_SYMBOLS, len: 1, bytes }
    }

    #[test]
    fn size_follows_the_config() {
        let c = Config::default();
        assert_eq!(queue_for(c.ack_timeout, c.hop_attempts), SEND_QUEUE);
        assert_eq!(queue_for(c.ack_timeout, 1), 0, "no retries, no time to fill a queue");
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
        q.front_mut().unwrap().bytes[0] = 50;
        assert_eq!(q.pop().unwrap().bytes[0], 50);
        assert_eq!(q.front().unwrap().bytes[0], 1);
        assert_eq!(q.iter().map(|f| f.bytes[0]).last(), Some(SEND_QUEUE as u8 - 1));
        assert_eq!(q.len(), SEND_QUEUE - 1);
    }
}
