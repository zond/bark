//! Identifiers carried on the wire (spec "Addresses", "Packet formats").

use crate::position::Position;

/// A node's identifier, six opaque bytes (spec "Node identity"). Zero
/// names no node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct NodeId(pub [u8; NodeId::LEN]);

impl NodeId {
    /// Bytes in an ID (spec "Node identity").
    pub const LEN: usize = 6;

    /// No node: a record with no parent, and a node configured with it
    /// draws its own.
    pub const NONE: NodeId = NodeId([0; NodeId::LEN]);

    /// The ID whose bytes are the low 48 bits of `n`, big-endian.
    pub const fn from_u64(n: u64) -> NodeId {
        let b = n.to_be_bytes();
        NodeId([b[2], b[3], b[4], b[5], b[6], b[7]])
    }

    /// The ID as a number, the inverse of [`NodeId::from_u64`].
    pub const fn to_u64(self) -> u64 {
        let b = self.0;
        u64::from_be_bytes([0, 0, b[0], b[1], b[2], b[3], b[4], b[5]])
    }

    /// The last byte, which sets the node's heartbeat interval
    /// (spec "Heartbeat").
    pub const fn last(self) -> u8 {
        self.0[NodeId::LEN - 1]
    }
}

/// A message's identity (spec "Packet formats"). Only the ACK carries it;
/// every hop computes it with [`MsgId::of`] from fields no hop changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct MsgId(pub u32);

impl MsgId {
    /// The ID of a message to `(dst_pos, dst_id)` carrying `payload`:
    /// 32-bit FNV-1a over the three in wire bytes, concatenated. FNV-1a
    /// because the phone app computes it too. Payloads are unique (spec
    /// "Phone link"), so IDs collide only by chance: about 5 × 10⁻⁵ among
    /// the 649 a relay remembers (spec "Message records").
    pub fn of(dst_pos: Position, dst_id: NodeId, payload: &[u8]) -> MsgId {
        MsgId(fnv1a(&[&dst_pos.encode(), &dst_id.0, payload]))
    }
}

/// 32-bit FNV-1a of `parts`, concatenated. The message ID, the stored
/// records' checks and the phone's own copies of both use it, so one
/// definition keeps them the same.
pub fn fnv1a(parts: &[&[u8]]) -> u32 {
    // Plain loops: an unoptimised build, as the simulator's tests are,
    // hashes every message through here.
    let mut h: u32 = 0x811c_9dc5;
    for part in parts {
        for &b in *part {
            h = (h ^ u32::from(b)).wrapping_mul(0x0100_0193);
        }
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv1a_is_as_published() {
        assert_eq!(fnv1a(&[]), 0x811c_9dc5);
        assert_eq!(fnv1a(&[b""]), 0x811c_9dc5);
        assert_eq!(fnv1a(&[b"a"]), 0xe40c_292c);
        assert_eq!(fnv1a(&[b"foobar"]), 0xbf9c_f968);
        assert_eq!(fnv1a(&[b"foo", b"", b"bar"]), 0xbf9c_f968);
    }

    #[test]
    fn msg_id_is_fnv1a_over_the_concatenation() {
        let p = Position::from_degrees(50.45, 30.52);
        let leaf = NodeId([1, 2, 3, 4, 5, 6]);
        let mut all = [0u8; 7 + 6 + 5];
        all[..7].copy_from_slice(&p.encode());
        all[7..13].copy_from_slice(&leaf.0);
        all[13..].copy_from_slice(b"hello");
        assert_eq!(MsgId::of(p, leaf, b"hello"), MsgId(fnv1a(&[&all])));
    }

    #[test]
    fn numbers_are_the_low_48_bits() {
        let id = NodeId::from_u64(0xffff_0102_0304_0506);
        assert_eq!(id, NodeId([1, 2, 3, 4, 5, 6]));
        assert_eq!(id.to_u64(), 0x0102_0304_0506);
        assert_eq!(id.last(), 6);
    }

    #[test]
    fn every_field_counts() {
        let p = Position::from_degrees(50.45, 30.52);
        let a = MsgId::of(p, NodeId([1, 2, 3, 4, 5, 6]), b"a");
        assert_ne!(
            a,
            MsgId::of(
                Position {
                    lat: p.lat + 1,
                    ..p
                },
                NodeId([1, 2, 3, 4, 5, 6]),
                b"a"
            )
        );
        assert_ne!(a, MsgId::of(p, NodeId([1, 2, 3, 4, 5, 7]), b"a"));
        assert_ne!(a, MsgId::of(p, NodeId([1, 2, 3, 4, 5, 6]), b"b"));
        assert_ne!(a, MsgId::of(p, NodeId([1, 2, 3, 4, 5, 6]), b"a\0"));
    }
}
