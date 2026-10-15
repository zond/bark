//! Bounded state the protocol core keeps. Every container has a capacity
//! fixed by the design and lives inline, so nothing here allocates
//! (roadmap D8).

pub(crate) mod fixed;

pub mod budget;
pub mod busy;
pub mod delivered;
pub mod delivery;
pub mod leaves;
pub mod neighbours;
pub mod records;
pub mod send_queue;

use crate::event::MAX_FRAME;
use crate::radio::{time_on_air, MIN_PREAMBLE_SYMBOLS};
use crate::time::Duration;
use crate::wire::ACK_LEN;

/// How long a hand-over keeps trying listen-before-talk before it gives
/// up and chooses the next hop again (design §9.3 step 2).
pub const LBT_GIVE_UP: Duration = Duration::from_millis(500);

/// The longest one hand-over attempt takes: listen-before-talk until it
/// gives up, a full frame on air, and the ACK timeout (design §9.2.2,
/// §9.3): 738.6 ms with the design's 100 ms timeout.
pub const fn attempt_span(ack_timeout: Duration) -> Duration {
    Duration::from_micros(
        LBT_GIVE_UP.as_micros()
            + time_on_air(MAX_FRAME as u8, MIN_PREAMBLE_SYMBOLS).as_micros()
            + ack_timeout.as_micros(),
    )
}

/// The time one hand-over holds its receiver: a full frame and its ACK,
/// 138.56 + 14.40 = 152.96 ms (design §9.3).
///
/// It is the unit of the wait before a retry. Two senders that cannot hear
/// each other and have collided at one receiver both miss the ACK and
/// retry at random times in `[0, W]`. Their frames miss each other only if
/// the starts are more than this apart, the first frame and the ACK that
/// answers it, so they come apart with probability `(1 − unit / W)²`:
/// never at `W` = one unit, 1/4 at two, 9/16 at four, for full frames.
/// Hence [`retry_window`] starts at two units and doubles (roadmap D22).
pub const RETRY_BACKOFF: Duration = Duration::from_micros(
    time_on_air(MAX_FRAME as u8, MIN_PREAMBLE_SYMBOLS).as_micros()
        + time_on_air(ACK_LEN as u8, MIN_PREAMBLE_SYMBOLS).as_micros(),
);

/// The window of the random wait before the retry that follows `attempts`
/// transmissions: `2^attempts × unit`, 306 ms and then 612 ms with the
/// design's three attempts and [`RETRY_BACKOFF`]. Doubling also spreads
/// out more than two senders, where a fixed window would not.
pub const fn retry_window(unit: Duration, attempts: u8) -> Duration {
    let factor = match 1u64.checked_shl(attempts as u32) {
        Some(f) => f,
        None => u64::MAX,
    };
    Duration::from_micros(unit.as_micros().saturating_mul(factor))
}

/// The longest a hand-over waits in all before its retries, with
/// `hop_attempts` transmissions: the sum of the retry windows.
pub const fn retry_waits(unit: Duration, hop_attempts: u8) -> Duration {
    let mut total = 0u64;
    let mut k = 1;
    while k < hop_attempts {
        total = total.saturating_add(retry_window(unit, k).as_micros());
        k += 1;
    }
    Duration::from_micros(total)
}

/// How long an upstream sender spends retrying a hand-over after its
/// first attempt, when every attempt goes unanswered: the retries, each
/// waiting the longest before it ([`retry_waits`]) and for the channel,
/// carrying a full frame and waiting out the ACK timeout. 2.39 s with the
/// design's 100 ms timeout and three attempts (§9.3).
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

/// Table sizes against the RAM budget of design §15.
#[cfg(test)]
pub(crate) mod sizes {
    extern crate std;
    use super::*;
    use core::mem::size_of;

    #[test]
    fn table_sizes() {
        // Per-entry sizes the capacities multiply.
        assert_eq!(size_of::<Option<neighbours::Neighbour>>(), 48);
        assert_eq!(size_of::<leaves::LeafEntry>(), 16);
        assert_eq!(size_of::<delivery::Queued>(), 256);
        assert_eq!(size_of::<records::Record>(), 32);
        assert_eq!(size_of::<crate::event::Frame>(), 260);
        assert_eq!(size_of::<send_queue::Outgoing>(), 272);
        // The send queue's regular room follows from the duty bucket, its
        // hop places from the hop count (§9.3), and the delivery queue's
        // places from a full leaf table over one TTL (§10.4).
        assert_eq!(send_queue::SEND_QUEUE, 40);
        assert_eq!(send_queue::HOP_PLACES, 254);
        assert_eq!(delivery::DELIVERY_QUEUE, 41);
        let sum = tables();
        std::println!("{:>10}: {sum:>6} B", "tables");
        assert!(size_of::<busy::BusyEstimator>() <= 512);
    }

    /// The bytes of every table, each checked to be what its entries take
    /// and a few words of bookkeeping.
    pub(crate) fn tables() -> usize {
        let tables = [
            (
                "neighbours",
                size_of::<neighbours::Neighbours>(),
                neighbours::NEIGHBOURS * 48,
            ),
            ("leaves", size_of::<leaves::Leaves>(), leaves::LEAVES * 16),
            (
                "delivery",
                size_of::<delivery::DeliveryQueue>(),
                delivery::DELIVERY_QUEUE * 256,
            ),
            (
                "records",
                size_of::<records::Records>(),
                records::RECORDS * 32,
            ),
            (
                "send queue",
                size_of::<send_queue::SendQueue>(),
                (send_queue::SEND_QUEUE + send_queue::HOP_PLACES) * 272,
            ),
            (
                "delivered",
                size_of::<delivered::Delivered>(),
                delivered::DELIVERED * 16,
            ),
            ("budget", size_of::<budget::Budget>(), 16),
            ("busy", size_of::<busy::BusyEstimator>(), 0),
        ];
        let mut sum = 0;
        for (name, size, entries) in tables {
            std::println!("{name:>10}: {size:>6} B");
            assert!(size <= entries + 64 || name == "busy", "{name}: {size} B");
            sum += size;
        }
        sum
    }
}
