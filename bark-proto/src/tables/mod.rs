//! Bounded state the protocol core keeps. Every container has a capacity
//! fixed by the design and lives inline, so nothing here allocates
//! (roadmap D8).

mod fixed;

pub mod budget;
pub mod busy;
pub mod delivery;
pub mod leaves;
pub mod neighbours;
pub mod recent_rx;

/// Table sizes against the RAM targets of `docs/core-interface.md` §6.
#[cfg(test)]
mod sizes {
    extern crate std;
    use super::*;
    use core::mem::size_of;

    #[test]
    fn table_sizes() {
        let sizes = [
            ("neighbours", size_of::<neighbours::Neighbours>()),
            ("leaves", size_of::<leaves::Leaves>()),
            ("delivery", size_of::<delivery::DeliveryQueue>()),
            ("recent_rx", size_of::<recent_rx::RecentRx<{ recent_rx::RELAY_RECENT_RX }>>()),
            ("budget", size_of::<budget::RelayBudget>()),
            ("busy", size_of::<busy::BusyEstimator>()),
        ];
        for (name, size) in sizes {
            std::println!("{name:>10}: {size:>6} B");
        }
        // Per-entry sizes the capacities multiply.
        assert_eq!(size_of::<neighbours::Neighbour>(), 32);
        assert_eq!(size_of::<leaves::LeafEntry>(), 16);
        assert_eq!(size_of::<delivery::Queued>(), 256);
        // Whole tables, with headroom for the length and parameter fields.
        let bound = |entries: usize, entry: usize| entries * entry + 64;
        assert!(sizes[0].1 <= bound(neighbours::NEIGHBOURS, 32));
        assert!(sizes[1].1 <= bound(leaves::LEAVES, 16));
        assert!(sizes[2].1 <= bound(delivery::DELIVERY_QUEUE, 256));
        assert!(sizes[3].1 <= bound(recent_rx::RELAY_RECENT_RX, 4));
        assert!(sizes[4].1 <= 512);
        assert!(sizes[5].1 <= 512);
        // The relay's tables together. docs/core-interface.md §6 targets
        // 16 KiB for the whole Relay; the delivery queue alone is 16 KiB
        // (64 × 234-byte payloads), so that target cannot hold with the
        // design's queue. 24 KiB bounds what the tables need.
        let relay: usize = sizes[..6].iter().map(|&(_, s)| s).sum();
        std::println!("{:>10}: {relay:>6} B", "relay sum");
        assert!(relay <= 24 * 1024, "{relay}");
    }
}
