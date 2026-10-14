//! Message IDs this relay has delivered as owner, kept for the delivery
//! TTL so that a later copy, from any neighbour, is ACKed and not
//! delivered again (design §9.2.7, §10.4).
//!
//! A ring of [`DELIVERED`] entries, oldest overwritten first; an entry
//! older than the TTL counts as absent. The size is what a relay delivers
//! in one TTL at the most load the design plans for: 400 leaves (§10.2, at
//! 50 % adoption) receiving ≈23 texts a day each (§13, the network at
//! capacity) is 9 200 a day, 64 per 10 minutes. Above that the oldest are
//! forgotten early and a late copy of one is delivered twice, which the
//! app, whose payloads are unique (§11), can recognise.

use crate::ids::MsgId;
use crate::node::Config;
use crate::time::{Duration, Instant};

const LEAVES_PLANNED: u64 = 400;
const TEXTS_PER_LEAF_PER_DAY: u64 = 23;
const DAY_S: u64 = 86_400;
const DESIGN_TTL_S: u64 = 600;

/// Ring capacity; see the module documentation.
pub const DELIVERED: usize = (LEAVES_PLANNED * TEXTS_PER_LEAF_PER_DAY * DESIGN_TTL_S).div_ceil(DAY_S) as usize;

const _: () = assert!(DELIVERED == 64);

/// The delivered window.
#[derive(Clone)]
pub struct Delivered {
    entries: [(MsgId, Instant); DELIVERED],
    next: usize,
    len: usize,
    ttl: Duration,
}

impl Delivered {
    /// Empty; entries live `ttl`.
    pub const fn new(ttl: Duration) -> Delivered {
        Delivered { entries: [(MsgId(0), Instant::ZERO); DELIVERED], next: 0, len: 0, ttl }
    }

    /// Empty, with `config.delivery_ttl`.
    pub fn from_config(config: &Config) -> Delivered {
        Delivered::new(config.delivery_ttl)
    }

    /// Whether `id` was delivered less than the TTL before `now`.
    pub fn contains(&self, id: MsgId, now: Instant) -> bool {
        self.entries[..self.len].iter().any(|&(i, at)| i == id && now.since(at) < self.ttl)
    }

    /// Record that `id` was delivered at `now`.
    pub fn insert(&mut self, id: MsgId, now: Instant) {
        self.entries[self.next] = (id, now);
        self.next = (self.next + 1) % DELIVERED;
        self.len = (self.len + 1).min(DELIVERED);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: u64) -> Instant {
        Instant::from_micros(s * 1_000_000)
    }

    #[test]
    fn ttl_and_ring() {
        let mut d = Delivered::from_config(&Config::default());
        assert!(!d.contains(MsgId(0), at(0)), "an empty slot is not message 0");
        d.insert(MsgId(7), at(0));
        assert!(d.contains(MsgId(7), at(599)));
        assert!(!d.contains(MsgId(7), at(600)), "kept for the delivery TTL");
        for i in 0..DELIVERED as u32 - 1 {
            d.insert(MsgId(100 + i), at(1));
        }
        assert!(d.contains(MsgId(7), at(1)), "the ring is full, nothing overwritten yet");
        d.insert(MsgId(999), at(1));
        assert!(!d.contains(MsgId(7), at(1)), "the oldest is overwritten first");
        assert!(d.contains(MsgId(100), at(1)) && d.contains(MsgId(999), at(1)));
    }
}
