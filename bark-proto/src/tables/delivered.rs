//! Message IDs this relay has delivered as owner, so that a later copy,
//! from any neighbour, is ACKed and not delivered again (design §9.2.7,
//! §10.4).
//!
//! [`DELIVERED`] entries with no lifetime: when full, a new one takes the
//! place of the least recently active, the one delivered or last offered
//! again longest ago, as message records do (`records.rs`, roadmap D31).
//! The size is the delivery queue's, [`DELIVERY_QUEUE`]: what a relay
//! delivers in one delivery TTL for a full leaf table, 256 leaves (§10.2)
//! at ≈23 texts a day each (§13, the network at capacity), 41. Above that
//! a late copy of one forgotten is delivered twice, which the app, whose
//! payloads are unique (§11), can recognise.

use crate::ids::MsgId;
use crate::tables::delivery::DELIVERY_QUEUE;
use crate::time::Instant;

/// Capacity; see the module documentation.
pub const DELIVERED: usize = DELIVERY_QUEUE;

const _: () = assert!(DELIVERED == 41);

/// The delivered window.
#[derive(Clone)]
pub struct Delivered {
    /// Each ID with when it was last active.
    entries: [(MsgId, Instant); DELIVERED],
    len: usize,
}

impl Delivered {
    /// Empty.
    pub const fn new() -> Delivered {
        Delivered {
            entries: [(MsgId(0), Instant::ZERO); DELIVERED],
            len: 0,
        }
    }

    /// Whether `id` is remembered as delivered.
    pub fn contains(&self, id: MsgId) -> bool {
        self.entries[..self.len].iter().any(|&(i, _)| i == id)
    }

    /// A copy of `id` arrived at `now`: whether it was delivered. A
    /// remembered one is active again.
    pub fn offered(&mut self, id: MsgId, now: Instant) -> bool {
        match self.entries[..self.len].iter_mut().find(|(i, _)| *i == id) {
            Some(e) => {
                e.1 = now;
                true
            }
            None => false,
        }
    }

    /// Record that `id` was delivered at `now`.
    pub fn insert(&mut self, id: MsgId, now: Instant) {
        if self.offered(id, now) {
            return;
        }
        if self.len < DELIVERED {
            self.entries[self.len] = (id, now);
            self.len += 1;
            return;
        }
        if let Some(e) = self.entries.iter_mut().min_by_key(|(_, at)| *at) {
            *e = (id, now);
        }
    }
}

impl Default for Delivered {
    fn default() -> Self {
        Delivered::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: u64) -> Instant {
        Instant::from_micros(s * 1_000_000)
    }

    #[test]
    fn least_recently_active_goes_first() {
        let mut d = Delivered::new();
        assert!(!d.contains(MsgId(0)), "an empty slot is not message 0");
        d.insert(MsgId(7), at(0));
        assert!(d.contains(MsgId(7)), "no lifetime");
        for i in 0..DELIVERED as u32 - 1 {
            d.insert(MsgId(100 + i), at(1 + i as u64));
        }
        assert!(d.contains(MsgId(7)), "full, nothing replaced yet");
        // A copy makes 7 the most recently active; 100 goes instead.
        assert!(d.offered(MsgId(7), at(100)));
        assert!(!d.offered(MsgId(999), at(100)));
        d.insert(MsgId(999), at(101));
        assert!(d.contains(MsgId(7)) && d.contains(MsgId(999)));
        assert!(!d.contains(MsgId(100)) && d.contains(MsgId(101)));
    }
}
