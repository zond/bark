//! What a node still holds when it reboots into a new image, kept across
//! the reboot and offered again as its own (spec "Updates over the mesh",
//! the install): a slot for each message, its destination and payload,
//! which [`MsgId::of`] is taken of, so that it is the same message after.
//!
//! A slot is a page of the external flash, written once; the region is
//! erased once every message in it is offered again. Its ID is its check:
//! a slot cut short by a power cut does not hash to it.

use bark_proto::tables::delivery::{DELIVERY_QUEUE, INBOX};
use bark_proto::tables::send_queue::{HOP_SLOTS, SEND_QUEUE};
use bark_proto::wire::MAX_PAYLOAD;
use bark_proto::{Held, MsgId, NodeId, Position};

/// Bytes of a slot: a program page of the external flash (ZD25WQ16B
/// datasheet, "Page Program"), so that each is one write.
pub const SLOT_LEN: usize = 256;

/// The most a node holds: its send queue's regular room and the hop
/// places it may hold at once, its delivery queue and its inbox (spec
/// "Send queue", "Delivery queue").
pub const HELD_MAX: usize = SEND_QUEUE + HOP_SLOTS + DELIVERY_QUEUE + INBOX;

const LEN: usize = 0;
const ID: usize = 2;
const POS: usize = ID + 4;
const DST: usize = POS + Position::WIRE_LEN;
const PAYLOAD: usize = DST + NodeId::LEN;
const _: () = assert!(PAYLOAD + MAX_PAYLOAD <= SLOT_LEN);

/// The slot for `m`, or `None` for a payload longer than a frame holds.
pub fn encode(m: &Held<'_>) -> Option<[u8; SLOT_LEN]> {
    if m.payload.len() > MAX_PAYLOAD {
        return None;
    }
    let mut s = [0xFF; SLOT_LEN];
    s[LEN] = m.payload.len() as u8;
    s[ID..POS].copy_from_slice(&m.msg_id.0.to_be_bytes());
    s[POS..DST].copy_from_slice(&m.dst_pos.encode());
    s[DST..PAYLOAD].copy_from_slice(&m.dst_id.0);
    s[PAYLOAD..PAYLOAD + m.payload.len()].copy_from_slice(m.payload);
    Some(s)
}

/// The message a slot keeps: `None` for an erased slot and one that fails
/// its check.
pub fn decode(s: &[u8; SLOT_LEN]) -> Option<Held<'_>> {
    let len = usize::from(s[LEN]);
    if len > MAX_PAYLOAD {
        return None;
    }
    let pos: [u8; Position::WIRE_LEN] = s[POS..DST].try_into().ok()?;
    let id: [u8; NodeId::LEN] = s[DST..PAYLOAD].try_into().ok()?;
    let id_bytes: [u8; 4] = s[ID..POS].try_into().ok()?;
    let m = Held {
        msg_id: MsgId(u32::from_be_bytes(id_bytes)),
        dst_pos: Position::decode(&pos),
        dst_id: NodeId(id),
        payload: &s[PAYLOAD..PAYLOAD + len],
    };
    (MsgId::of(m.dst_pos, m.dst_id, m.payload) == m.msg_id).then_some(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held(payload: &[u8]) -> Held<'_> {
        let dst_pos = Position::from_degrees(50.45, 30.52);
        let dst_id = NodeId([1, 2, 3, 4, 5, 6]);
        Held {
            msg_id: MsgId::of(dst_pos, dst_id, payload),
            dst_pos,
            dst_id,
            payload,
        }
    }

    #[test]
    fn a_slot_keeps_the_same_message() {
        for payload in [&b""[..], b"a text", &[7; MAX_PAYLOAD]] {
            let m = held(payload);
            let s = encode(&m).expect("fits");
            assert_eq!(decode(&s), Some(m));
        }
        assert!(encode(&held(&[0; MAX_PAYLOAD + 1])).is_none());
    }

    #[test]
    fn an_erased_or_torn_slot_keeps_nothing() {
        assert_eq!(decode(&[0xFF; SLOT_LEN]), None);
        let m = held(b"a text");
        // A power cut in the write leaves the end erased.
        let mut torn = encode(&m).unwrap();
        torn[PAYLOAD + 3..].fill(0xFF);
        assert_eq!(decode(&torn), None);
    }

    #[test]
    fn the_most_a_node_holds() {
        assert_eq!(HELD_MAX, 39 + 64 + 41 + 23);
    }
}
