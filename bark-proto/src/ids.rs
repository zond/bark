//! Identifiers carried on the wire (design §9.1, §14).

use crate::position::Position;

/// A leaf's 4-byte identifier. Zero means "the relay itself".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct LeafId(pub [u8; 4]);

impl LeafId {
    /// The relay-itself address.
    pub const RELAY: LeafId = LeafId([0; 4]);
}

/// A message's identity (design §14, roadmap D15). No packet carries it
/// except the ACK and DELIVER that answer for a message: every node that
/// handles the message computes it with [`MsgId::of`], and gets the same
/// value on every hop because none of the hashed fields changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct MsgId(pub u32);

impl MsgId {
    /// The ID of a message to `(dst_pos, dst_leaf)` carrying `payload`:
    /// 32-bit FNV-1a over `dst_pos` in its 7 wire bytes, `dst_leaf` and
    /// `payload`, concatenated. FNV-1a because the phone app must compute
    /// it too, and it is a few lines in any language. The app keeps
    /// payloads unique (design §11), so two IDs collide only by chance:
    /// among the 1788 messages a relay remembers at most (§9.2.2), with
    /// probability about 4 × 10⁻⁴.
    pub fn of(dst_pos: Position, dst_leaf: LeafId, payload: &[u8]) -> MsgId {
        let mut h: u32 = 0x811c_9dc5;
        for part in [&dst_pos.encode()[..], &dst_leaf.0, payload] {
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
        let leaf = LeafId([1, 2, 3, 4]);
        let mut all = [0u8; 7 + 4 + 5];
        all[..7].copy_from_slice(&p.encode());
        all[7..11].copy_from_slice(&leaf.0);
        all[11..].copy_from_slice(b"hello");
        assert_eq!(MsgId::of(p, leaf, b"hello"), MsgId(fnv1a(&all)));
    }

    #[test]
    fn every_field_counts() {
        let p = Position::from_degrees(50.45, 30.52);
        let a = MsgId::of(p, LeafId([1, 2, 3, 4]), b"a");
        assert_ne!(
            a,
            MsgId::of(
                Position {
                    lat: p.lat + 1,
                    ..p
                },
                LeafId([1, 2, 3, 4]),
                b"a"
            )
        );
        assert_ne!(a, MsgId::of(p, LeafId([1, 2, 3, 5]), b"a"));
        assert_ne!(a, MsgId::of(p, LeafId([1, 2, 3, 4]), b"b"));
        assert_ne!(a, MsgId::of(p, LeafId([1, 2, 3, 4]), b"a\0"));
    }
}
