//! The leaf table (design §10.2): leaves attached to this relay, keyed by
//! [`LeafId`].
//!
//! An ATTACH adds or refreshes an entry; the leaf repeats it every
//! `keepalive` (§10.2: 60 s) and the entry expires after three missed
//! keepalives, `3 × keepalive` (180 s) after the last one.
//!
//! When the table is full, an ATTACH from an unknown leaf takes the slot of
//! an expired entry if there is one, and is otherwise refused (decision
//! 2026-10-14; §10.2 does not say). Evicting a live leaf would only make it
//! re-attach within a keepalive and evict another, and a refusal lets the
//! relay withhold the ACK so the leaf attaches elsewhere. At 256 entries
//! against ≈80 expected leaves per relay (§10.2) this is an overload case.
//!
//! §10.2 also lists an `rssi` field; nothing in the design reads it, so it
//! is not kept.

use crate::ids::LeafId;
use crate::node::Config;
use crate::tables::fixed::FixedVec;
use crate::time::{Duration, Instant};
use crate::wire::RxMode;

/// Table capacity (design §10.2).
pub const LEAVES: usize = 256;

/// Keepalives a leaf may miss before its entry expires (design §10.2).
pub const MISSED_KEEPALIVES: u64 = 3;

/// One attached leaf.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeafEntry {
    /// The leaf.
    pub leaf: LeafId,
    /// When its last ATTACH was heard.
    pub last_seen: Instant,
    /// Its declared receive mode.
    pub mode: RxMode,
    /// Its last DELIVER went unanswered: it is not listening, and its
    /// messages wait for its next keepalive or the delivery retry (§10.4).
    pub missed: bool,
}

/// The table is full of live leaves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Full;

/// The leaf table.
#[derive(Clone)]
pub struct Leaves {
    entries: FixedVec<LeafEntry, LEAVES>,
    expire_after: Duration,
}

impl Leaves {
    /// An empty table whose entries expire `expire_after` after their last ATTACH.
    pub const fn new(expire_after: Duration) -> Leaves {
        let fill = LeafEntry {
            leaf: LeafId::RELAY,
            last_seen: Instant::ZERO,
            mode: RxMode::Listening,
            missed: false,
        };
        Leaves {
            entries: FixedVec::new(fill),
            expire_after,
        }
    }

    /// An empty table with the design's expiry for `config.keepalive`.
    pub const fn from_config(config: &Config) -> Leaves {
        Leaves::new(config.keepalive.mul(MISSED_KEEPALIVES))
    }

    /// Number of entries, expired ones not yet removed included.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there are no entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// All entries, in no particular order.
    pub fn iter(&self) -> impl Iterator<Item = &LeafEntry> {
        self.entries.as_slice().iter()
    }

    /// The live entry for `leaf` at `now`.
    pub fn get(&self, leaf: LeafId, now: Instant) -> Option<&LeafEntry> {
        self.iter()
            .find(|e| e.leaf == leaf && now.since(e.last_seen) < self.expire_after)
    }

    /// An ATTACH from `leaf` was heard at `now`.
    pub fn attached(&mut self, leaf: LeafId, mode: RxMode, now: Instant) -> Result<(), Full> {
        let entry = LeafEntry {
            leaf,
            last_seen: now,
            mode,
            missed: false,
        };
        let expire_after = self.expire_after;
        let slots = self.entries.as_mut_slice();
        if let Some(e) = slots.iter_mut().find(|e| e.leaf == leaf) {
            *e = entry;
            return Ok(());
        }
        if self.entries.push(entry).is_ok() {
            return Ok(());
        }
        let slots = self.entries.as_mut_slice();
        match slots
            .iter_mut()
            .find(|e| now.since(e.last_seen) >= expire_after)
        {
            Some(e) => {
                *e = entry;
                Ok(())
            }
            None => Err(Full),
        }
    }

    /// Mark whether `leaf`, if present, left its last DELIVER unanswered.
    pub fn set_missed(&mut self, leaf: LeafId, missed: bool) {
        if let Some(e) = self
            .entries
            .as_mut_slice()
            .iter_mut()
            .find(|e| e.leaf == leaf)
        {
            e.missed = missed;
        }
    }

    /// Forget `leaf`, if present.
    pub fn remove(&mut self, leaf: LeafId) {
        self.entries.retain(|e| e.leaf != leaf);
    }

    /// Remove every entry expired at `now`.
    pub fn expire(&mut self, now: Instant) {
        let expire_after = self.expire_after;
        self.entries
            .retain(|e| now.since(e.last_seen) < expire_after);
    }

    /// When the next entry expires, if any.
    pub fn next_expiry(&self) -> Option<Instant> {
        self.iter().map(|e| e.last_seen + self.expire_after).min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: u64) -> Instant {
        Instant::from_micros(s * 1_000_000)
    }

    fn id(i: u32) -> LeafId {
        LeafId(i.to_be_bytes())
    }

    #[test]
    fn attach_refresh_expire() {
        let mut t = Leaves::from_config(&Config::default());
        assert_eq!(t.attached(id(1), RxMode::Listening, at(0)), Ok(()));
        assert_eq!(t.attached(id(2), RxMode::Hybrid, at(10)), Ok(()));
        assert_eq!(t.get(id(2), at(10)).unwrap().mode, RxMode::Hybrid);
        assert_eq!(t.attached(id(1), RxMode::DutyCycled, at(60)), Ok(()));
        assert_eq!(t.len(), 2);
        assert_eq!(t.get(id(1), at(60)).unwrap().mode, RxMode::DutyCycled);
        assert_eq!(t.next_expiry(), Some(at(190)));
        assert!(t.get(id(2), at(189)).is_some());
        assert!(t.get(id(2), at(190)).is_none(), "three missed keepalives");
        t.set_missed(id(1), true);
        assert!(t.get(id(1), at(61)).unwrap().missed);
        assert_eq!(t.attached(id(1), RxMode::DutyCycled, at(62)), Ok(()));
        assert!(
            !t.get(id(1), at(62)).unwrap().missed,
            "a keepalive clears it"
        );
        t.expire(at(190));
        assert_eq!(t.len(), 1);
        t.remove(id(1));
        assert!(t.is_empty());
    }

    #[test]
    fn full_table_reuses_expired_slots_only() {
        let mut t = Leaves::from_config(&Config::default());
        for i in 0..LEAVES as u32 {
            assert_eq!(t.attached(id(i), RxMode::Listening, at(i as u64)), Ok(()));
        }
        assert_eq!(t.attached(id(1000), RxMode::Listening, at(179)), Err(Full));
        // Leaf 0, attached at 0 s, has expired at 180 s.
        assert_eq!(t.attached(id(1000), RxMode::Listening, at(180)), Ok(()));
        assert!(t.get(id(0), at(180)).is_none());
        assert!(t.get(id(1000), at(180)).is_some());
        assert_eq!(t.len(), LEAVES);
    }
}
