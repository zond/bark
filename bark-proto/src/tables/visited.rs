//! The depth-first visited list carried inside DATA packets
//! (design §8.1, §9.2, §14).
//!
//! Each entry is the 16-bit position hash of a relay the packet has
//! already passed through during void recovery. A hash collision only makes
//! a candidate look visited, which skips it (design §8.1); it never loops a
//! packet. The capacity is the wire bound [`MAX_VISITED`].

use crate::wire::MAX_VISITED;

/// A bounded list of 16-bit position hashes, in the order they were pushed.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Visited {
    hashes: [u16; MAX_VISITED],
    len: u8,
}

impl Visited {
    /// The empty list.
    pub const EMPTY: Visited = Visited { hashes: [0; MAX_VISITED], len: 0 };

    /// The empty list.
    pub const fn new() -> Visited {
        Visited::EMPTY
    }

    /// Build from hashes, or `None` if there are more than [`MAX_VISITED`].
    pub fn from_hashes(hashes: &[u16]) -> Option<Visited> {
        if hashes.len() > MAX_VISITED {
            return None;
        }
        let mut v = Visited::EMPTY;
        v.hashes[..hashes.len()].copy_from_slice(hashes);
        v.len = hashes.len() as u8;
        Some(v)
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.len as usize
    }

    /// Whether the list is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Whether the list has no room left.
    pub fn is_full(&self) -> bool {
        self.len() == MAX_VISITED
    }

    /// The entries, oldest first.
    pub fn hashes(&self) -> &[u16] {
        &self.hashes[..self.len()]
    }

    /// Whether `hash` is in the list.
    pub fn contains(&self, hash: u16) -> bool {
        self.hashes().contains(&hash)
    }

    /// Append `hash`. Returns `false`, leaving the list unchanged, if it is
    /// full. Does not deduplicate: whether to push a hash that is already
    /// present is the router's decision.
    pub fn push(&mut self, hash: u16) -> bool {
        if self.is_full() {
            return false;
        }
        self.hashes[self.len()] = hash;
        self.len += 1;
        true
    }
}

impl Default for Visited {
    fn default() -> Self {
        Visited::EMPTY
    }
}

impl core::fmt::Debug for Visited {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list().entries(self.hashes()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_until_full() {
        let mut v = Visited::new();
        assert!(v.is_empty());
        for i in 0..MAX_VISITED as u16 {
            assert!(!v.contains(i));
            assert!(v.push(i));
            assert!(v.contains(i));
        }
        assert!(v.is_full());
        assert!(!v.push(999));
        assert!(!v.contains(999));
        assert_eq!(v.len(), MAX_VISITED);
    }

    #[test]
    fn slices() {
        let mut v = Visited::new();
        assert!(v.push(7));
        assert_eq!(Visited::from_hashes(v.hashes()), Some(v));
        assert_eq!(Visited::from_hashes(&[0; MAX_VISITED + 1]), None);
    }
}
