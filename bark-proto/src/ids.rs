//! Identifiers carried on the wire (spec "Addresses", "Packet formats").

use crate::position::Position;

/// A node's 4-byte identifier, four opaque bytes in order. Every node
/// has one; it names the node in the packet fields that only name a node
/// (DATA `dst_hop` and `src_hop`, ACK `from` and `to`, ATTACH `relay`)
/// and with a position makes a destination address (spec "Addresses").
/// Zero names no node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct NodeId(pub [u8; 4]);

impl NodeId {
    /// No node: a leaf's DATA names no sender, and a node configured with
    /// it draws its own.
    pub const NONE: NodeId = NodeId([0; 4]);
}

/// A message's identity (spec "Packet formats", roadmap D15). No packet carries it
/// except the ACK and DELIVER that answer for a message: every node that
/// handles the message computes it with [`MsgId::of`], and gets the same
/// value on every hop because none of the hashed fields changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct MsgId(pub u32);

impl MsgId {
    /// The ID of a message to `(dst_pos, dst_id)` carrying `payload`:
    /// 32-bit FNV-1a over `dst_pos` in its 7 wire bytes, `dst_id` and
    /// `payload`, concatenated. FNV-1a because the phone app must compute
    /// it too, and it is a few lines in any language. The app keeps
    /// payloads unique (spec "Phone link"), so two IDs collide only by chance:
    /// among the 1855 messages a relay remembers at most (spec "Message records"), with
    /// probability about 4 × 10⁻⁴.
    pub fn of(dst_pos: Position, dst_id: NodeId, payload: &[u8]) -> MsgId {
        let mut h: u32 = 0x811c_9dc5;
        for part in [&dst_pos.encode()[..], &dst_id.0, payload] {
            for &b in part {
                h ^= b as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
        }
        MsgId(h)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FNV-1a as published, for its test vectors.
    fn fnv1a(bytes: &[u8]) -> u32 {
        bytes.iter().fold(0x811c_9dc5, |h, &b| {
            (h ^ b as u32).wrapping_mul(0x0100_0193)
        })
    }

    #[test]
    fn msg_id_is_fnv1a_over_the_concatenation() {
        assert_eq!(fnv1a(b""), 0x811c_9dc5);
        assert_eq!(fnv1a(b"a"), 0xe40c_292c);
        assert_eq!(fnv1a(b"foobar"), 0xbf9c_f968);
        let p = Position::from_degrees(50.45, 30.52);
        let leaf = NodeId([1, 2, 3, 4]);
        let mut all = [0u8; 7 + 4 + 5];
        all[..7].copy_from_slice(&p.encode());
        all[7..11].copy_from_slice(&leaf.0);
        all[11..].copy_from_slice(b"hello");
        assert_eq!(MsgId::of(p, leaf, b"hello"), MsgId(fnv1a(&all)));
    }

    #[test]
    fn every_field_counts() {
        let p = Position::from_degrees(50.45, 30.52);
        let a = MsgId::of(p, NodeId([1, 2, 3, 4]), b"a");
        assert_ne!(
            a,
            MsgId::of(
                Position {
                    lat: p.lat + 1,
                    ..p
                },
                NodeId([1, 2, 3, 4]),
                b"a"
            )
        );
        assert_ne!(a, MsgId::of(p, NodeId([1, 2, 3, 5]), b"a"));
        assert_ne!(a, MsgId::of(p, NodeId([1, 2, 3, 4]), b"b"));
        assert_ne!(a, MsgId::of(p, NodeId([1, 2, 3, 4]), b"a\0"));
    }
}
