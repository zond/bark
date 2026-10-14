//! Bounded state the protocol core keeps. Every container has a capacity
//! fixed by the design and lives inline, so nothing here allocates
//! (roadmap D8).

mod fixed;

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

/// How long a hand-over keeps trying listen-before-talk before it gives
/// up and chooses the next hop again (design §9.3 step 2).
pub const LBT_GIVE_UP: Duration = Duration::from_millis(500);

/// The longest one hand-over attempt takes: listen-before-talk until it
/// gives up, a full frame on air, and the ACK timeout (design §9.2.2,
/// §9.3): 738.6 ms with the design's 100 ms timeout.
pub const fn attempt_span(ack_timeout: Duration) -> Duration {
    Duration::from_micros(
        LBT_GIVE_UP.as_micros() + time_on_air(MAX_FRAME as u8, MIN_PREAMBLE_SYMBOLS).as_micros() + ack_timeout.as_micros(),
    )
}

/// Table sizes against the RAM budget of design §15.
#[cfg(test)]
mod sizes {
    extern crate std;
    use super::*;
    use core::mem::size_of;

    #[test]
    fn table_sizes() {
        // Per-entry sizes the capacities multiply.
        assert_eq!(size_of::<Option<neighbours::Neighbour>>(), 40);
        assert_eq!(size_of::<leaves::LeafEntry>(), 16);
        assert_eq!(size_of::<delivery::Queued>(), 256);
        assert_eq!(size_of::<records::Record>(), 32);
        assert_eq!(size_of::<crate::event::Frame>(), 260);
        // Whole tables: entries times capacity, and a few words of bookkeeping.
        let tables = [
            ("neighbours", size_of::<neighbours::Neighbours>(), neighbours::NEIGHBOURS * 40),
            ("leaves", size_of::<leaves::Leaves>(), leaves::LEAVES * 16),
            ("delivery", size_of::<delivery::DeliveryQueue>(), delivery::DELIVERY_QUEUE * 256),
            ("records", size_of::<records::Records>(), records::RECORDS * 32),
            ("send queue", size_of::<send_queue::SendQueue>(), send_queue::SEND_QUEUE * 260),
            ("delivered", size_of::<delivered::Delivered>(), delivered::DELIVERED * 16),
            ("budget", size_of::<budget::RelayBudget>(), 62 * 8),
            ("busy", size_of::<busy::BusyEstimator>(), 0),
        ];
        let mut sum = 0;
        for (name, size, entries) in tables {
            std::println!("{name:>10}: {size:>6} B");
            assert!(size <= entries + 64 || name == "busy", "{name}: {size} B");
            sum += size;
        }
        std::println!("{:>10}: {sum:>6} B", "tables");
        assert!(size_of::<busy::BusyEstimator>() <= 512);
    }
}
