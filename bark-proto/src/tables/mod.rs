//! Bounded state the protocol core keeps. Every container has a capacity
//! fixed by the design and lives inline, so nothing here allocates.

pub(crate) mod fixed;

pub mod budget;
pub mod busy;
pub mod delivered;
pub mod delivery;
pub mod heard;
pub mod kept;
pub mod neighbours;
pub mod records;
pub mod send_queue;

use crate::event::MAX_FRAME;
use crate::radio::{time_on_air, MIN_PREAMBLE_SYMBOLS};
use crate::time::Duration;
use crate::wire::ACK_LEN;

/// How long a hand-over keeps trying listen-before-talk before it gives
/// up and waits a retry wait (spec "Hand-over" step 2).
pub const LBT_GIVE_UP: Duration = Duration::from_millis(500);

/// How long a sender listens for its ACK (spec "Hand-over" step 5).
pub const ACK_TIMEOUT: Duration = Duration::from_millis(100);

/// Transmissions of one hand-over before it has failed (spec "Hand-over"
/// step 6).
pub const HOP_ATTEMPTS: u8 = 3;

/// Failed hand-overs in a row, with no answer between, that demote a
/// neighbour (spec "Neighbour table", "Hand-over" step 6). Three, not two:
/// the third offer comes 4.74 s after the first failure, after the 2.41 s
/// in which a receiver silent for want of reply budget refills it
/// (`budget::RETRY_REPLIES`).
pub const DEMOTE_AFTER_FAILURES: u8 = 3;

/// The longest one hand-over attempt takes: listen-before-talk until it
/// gives up, a full frame on air, and the ACK timeout (spec "Hand-over"):
/// 738.6 ms with the design's 100 ms timeout.
pub const fn attempt_span(ack_timeout: Duration) -> Duration {
    Duration::from_micros(
        LBT_GIVE_UP.as_micros()
            + time_on_air(MAX_FRAME as u8, MIN_PREAMBLE_SYMBOLS).as_micros()
            + ack_timeout.as_micros(),
    )
}

/// The time one hand-over holds its receiver, the unit of the wait before
/// a retry: a full frame and its ACK, 138.56 + 16.96 = 155.52 ms
/// (spec "Retry wait").
pub const RETRY_BACKOFF: Duration = Duration::from_micros(
    time_on_air(MAX_FRAME as u8, MIN_PREAMBLE_SYMBOLS).as_micros()
        + time_on_air(ACK_LEN as u8, MIN_PREAMBLE_SYMBOLS).as_micros(),
);

/// The window of the random wait before the retry that follows `attempts`
/// transmissions: `2^attempts × unit`, 311 ms and then 622 ms with
/// [`RETRY_BACKOFF`] (spec "Retry wait").
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

/// The longest a sender spends retrying a hand-over after its first
/// attempt when none is answered: 2.41 s with [`ACK_TIMEOUT`] and
/// [`HOP_ATTEMPTS`] (spec "Hand-over").
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

/// Table sizes against the RAM budget of spec "Implementation notes".
#[cfg(test)]
pub(crate) mod sizes {
    extern crate std;
    use super::*;
    use core::mem::size_of;

    #[test]
    fn table_sizes() {
        assert_eq!(size_of::<Option<neighbours::Neighbour>>(), 128);
        assert_eq!(size_of::<heard::HeardNode>(), 32);
        assert_eq!(size_of::<delivery::Queued>(), 264);
        assert_eq!(size_of::<records::Record>(), 40);
        assert_eq!(size_of::<crate::event::Frame>(), 260);
        assert_eq!(size_of::<send_queue::Outgoing>(), OUTGOING);
        // (spec "Send queue", "Delivery queue")
        assert_eq!(send_queue::SEND_QUEUE, 39);
        assert_eq!(send_queue::HOP_PLACES, 254);
        assert_eq!(send_queue::HOP_SLOTS, 64);
        assert_eq!(delivery::DELIVERY_QUEUE, 41);
        assert_eq!(delivery::INBOX, 23);
        let sum = tables();
        std::println!("{:>10}: {sum:>6} B", "tables");
        assert!(size_of::<busy::BusyEstimator>() <= 512);
    }

    /// A send-queue entry: its message and bookkeeping, 288 B, and the
    /// neighbours it goes round, one bit per neighbour-table slot, 8 B.
    const OUTGOING: usize = 296;

    /// The bytes of every table, each checked to be its entries and a few
    /// words of bookkeeping.
    pub(crate) fn tables() -> usize {
        let tables = [
            (
                "neighbours",
                size_of::<neighbours::Neighbours>(),
                neighbours::NEIGHBOURS * 128 + neighbours::IMAGES * 12,
            ),
            ("heard", size_of::<heard::Heard>(), heard::HEARD * 32),
            (
                "delivery",
                size_of::<delivery::DeliveryQueue>(),
                delivery::DELIVERY_QUEUE * 264,
            ),
            (
                "inbox",
                size_of::<delivery::DeliveryQueue<{ delivery::INBOX }>>(),
                delivery::INBOX * 264,
            ),
            (
                "records",
                size_of::<records::Records>(),
                records::RECORDS * 40,
            ),
            (
                "send queue",
                size_of::<send_queue::SendQueue>(),
                (send_queue::SEND_QUEUE + send_queue::HOP_SLOTS) * OUTGOING,
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
