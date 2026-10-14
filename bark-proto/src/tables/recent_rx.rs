//! Duplicate suppression on `msg_id` (design §9.3, §14).
//!
//! A sender that misses an ACK retransmits; the receiver must recognise the
//! retransmission and ACK it without forwarding it again. The window is a
//! ring of the last `N` message IDs received, so its memory is fixed and it
//! needs no clock.
//!
//! # Size
//!
//! A ring of `N` remembers a message for as long as it takes `N` more
//! messages to arrive. It must outlast the retry span of an upstream
//! sender, from the first copy received to the last retry:
//!
//! ```text
//! span = (hop_attempts − 1) · (ack_timeout + LBT give-up + airtime of a 255-byte frame)
//!      = 2 · (100 ms + 500 ms + 138.6 ms) = 1.477 s                 (design §9.3)
//! ```
//!
//! and no channel delivers messages faster than one per airtime of the
//! shortest message-carrying frame, an empty DATA packet (21 bytes,
//! 18.2 ms at SF5 with the 12-symbol preamble). So
//! `N = ceil(1.477 s / 18.2 ms) = 81` holds every message for its whole
//! retry span even on a channel saturated with minimum-size frames, which
//! is far beyond the §13 rates (a channel carries ≈640 B/s under carrier
//! sense, a few messages per second). The bound uses airtime, not the §13
//! estimates, because those are uncertain by 2× and the airtime is not.
//! [`window_for`] computes it for other parameters.
//!
//! What it does not cover: a retry deferred by an exhausted duty-cycle
//! budget (§9.4) can arrive after any fixed window, and is then forwarded
//! again.

use crate::event::MAX_FRAME;
use crate::ids::MsgId;
use crate::radio::{time_on_air, MIN_PREAMBLE_SYMBOLS};
use crate::time::Duration;
use crate::wire::DATA_HEADER_LEN;

/// How long a hop keeps trying listen-before-talk before it gives up and
/// reselects (design §9.3 step 2: 500 ms).
pub const LBT_GIVE_UP: Duration = Duration::from_millis(500);

/// Window length that covers the retry span of a sender with these
/// parameters; see the module documentation.
pub const fn window_for(ack_timeout: Duration, hop_attempts: u8) -> usize {
    let attempt = ack_timeout.as_micros()
        + LBT_GIVE_UP.as_micros()
        + time_on_air(MAX_FRAME as u8, MIN_PREAMBLE_SYMBOLS).as_micros();
    let span = attempt * hop_attempts.saturating_sub(1) as u64;
    let shortest = time_on_air(DATA_HEADER_LEN as u8, MIN_PREAMBLE_SYMBOLS).as_micros();
    span.div_ceil(shortest) as usize
}

/// A relay's window, for the design's per-hop parameters (§9.3: 100 ms ACK
/// timeout, 3 attempts).
pub const RELAY_RECENT_RX: usize = window_for(Duration::from_millis(100), 3);

const _: () = assert!(RELAY_RECENT_RX == 81);

/// The last `N` message IDs received.
#[derive(Clone)]
pub struct RecentRx<const N: usize> {
    ids: [MsgId; N],
    next: usize,
    len: usize,
}

impl<const N: usize> RecentRx<N> {
    /// An empty window.
    pub const fn new() -> Self {
        const { assert!(N > 0, "an empty window suppresses nothing") };
        RecentRx { ids: [MsgId(0); N], next: 0, len: 0 }
    }

    /// Whether `id` is in the window.
    pub fn contains(&self, id: MsgId) -> bool {
        self.ids[..self.len].contains(&id)
    }

    /// Record `id`. Returns `true` if it is new, `false` if it is a duplicate
    /// (in which case the window is unchanged).
    pub fn insert(&mut self, id: MsgId) -> bool {
        if self.contains(id) {
            return false;
        }
        self.ids[self.next] = id;
        self.next = (self.next + 1) % N;
        self.len = (self.len + 1).min(N);
        true
    }
}

impl<const N: usize> Default for RecentRx<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Config;

    #[test]
    fn default_config_fits_the_relay_window() {
        let c = Config::default();
        assert!(window_for(c.ack_timeout, c.hop_attempts) <= RELAY_RECENT_RX);
        assert_eq!(window_for(c.ack_timeout, 1), 0, "no retries, nothing to suppress");
    }

    #[test]
    fn suppresses_within_the_window_and_forgets_after() {
        let mut w: RecentRx<4> = RecentRx::new();
        assert!(w.insert(MsgId(1)));
        assert!(!w.insert(MsgId(1)));
        for i in 2..=4 {
            assert!(w.insert(MsgId(i)));
        }
        assert!(w.contains(MsgId(1)));
        assert!(w.insert(MsgId(5)), "overwrites the oldest");
        assert!(!w.contains(MsgId(1)));
        assert!((2..=5).all(|i| w.contains(MsgId(i))));
        assert!(w.insert(MsgId(1)));
    }

    #[test]
    fn zero_is_not_special() {
        let mut w: RecentRx<RELAY_RECENT_RX> = RecentRx::new();
        assert!(!w.contains(MsgId(0)));
        assert!(w.insert(MsgId(0)));
        assert!(!w.insert(MsgId(0)));
    }
}
