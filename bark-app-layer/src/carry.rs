//! The messages a node holds, kept across a reboot into a new image and put
//! back where they were (spec "Updates over the mesh"): one slot per
//! [`Held`] message.
//!
//! A slot is a flash page written once. [`MsgId::of`] checks the
//! destination and payload, a CRC-8 checks the rest, so a slot a power cut
//! tore fails one of them.

use bark_proto::tables::delivery::{DELIVERY_QUEUE, INBOX};
use bark_proto::tables::send_queue::{HOP_SLOTS, SEND_QUEUE};
use bark_proto::wire::MAX_PAYLOAD;
use bark_proto::{Channel, Duration, Held, HeldAt, MsgId, NodeId, Position};

/// Bytes of a slot: a program page of the external flash (ZD25WQ16B
/// datasheet, "Page Program"), so that each is one write.
pub const SLOT_LEN: usize = 256;

/// The most a node holds: its send queue's regular room and the hop
/// places it may hold at once, its delivery queue and its inbox (spec
/// "Send queue", "Delivery queue").
pub const HELD_MAX: usize = SEND_QUEUE + HOP_SLOTS + DELIVERY_QUEUE + INBOX;

const LEN: usize = 0;
/// Where it was, in the low two bits, and the flushed mark.
const AT: usize = LEN + 1;
const ID: usize = AT + 1;
const POS: usize = ID + 4;
const DST: usize = POS + Position::WIRE_LEN;
const HOPS: usize = DST + NodeId::LEN;
/// A forward's parent or the neighbour a delivery waits for; never both.
const NODE: usize = HOPS + 1;
/// A forward's place and its parent's home, or a queued message's age.
const MORE: usize = NODE + NodeId::LEN;
const PAYLOAD: usize = MORE + 2;
const CHECK: usize = PAYLOAD + MAX_PAYLOAD;
const _: () = assert!(CHECK < SLOT_LEN);

const OWN: u8 = 0;
const FORWARD: u8 = 1;
const DELIVERY: u8 = 2;
const INBOX_AT: u8 = 3;
const FLUSHED: u8 = 4;

/// An age in whole seconds, rounded up so that nothing kept is taken as
/// fresher than it is. A queued message's age matters only against the
/// delivery TTL, 600 s, so one past 18 h is kept as 18 h.
fn age_bytes(age: Duration) -> [u8; 2] {
    let s = age.as_micros().div_ceil(1_000_000).min(u64::from(u16::MAX));
    (s as u16).to_be_bytes()
}

fn age_of(b: &[u8]) -> Duration {
    Duration::from_secs(u64::from(u16::from_be_bytes([b[0], b[1]])))
}

/// CRC-8 with the polynomial x⁸ + x² + x + 1 of what the ID does not
/// cover: where it was, its count, the node and the rest.
fn check(s: &[u8; SLOT_LEN]) -> u8 {
    let mut c = 0u8;
    for &b in core::iter::once(&s[AT]).chain(&s[HOPS..PAYLOAD]) {
        c ^= b;
        for _ in 0..8 {
            c = if c & 0x80 != 0 { c << 1 ^ 0x07 } else { c << 1 };
        }
    }
    c
}

/// The slot for `m`, or `None` for a payload longer than a frame holds.
pub fn encode(m: &Held<'_>) -> Option<[u8; SLOT_LEN]> {
    if m.payload.len() > MAX_PAYLOAD {
        return None;
    }
    let mut s = [0xFF; SLOT_LEN];
    s[LEN] = m.payload.len() as u8;
    s[ID..POS].copy_from_slice(&m.msg_id.0.to_be_bytes());
    s[POS..DST].copy_from_slice(&m.dst_pos.encode());
    s[DST..HOPS].copy_from_slice(&m.dst_id.0);
    s[HOPS] = m.hops;
    let (at, node, more) = match m.at {
        HeldAt::Own => (OWN, NodeId::NONE, [0; 2]),
        HeldAt::Forward { parent, place } => {
            let (p, home) = parent.map_or((NodeId::NONE, 0), |(p, h)| (p, h.index()));
            (FORWARD, p, [place, home])
        }
        HeldAt::Delivery {
            waits_for,
            age,
            flushed,
        } => (
            DELIVERY | if flushed { FLUSHED } else { 0 },
            waits_for,
            age_bytes(age),
        ),
        HeldAt::Inbox { age } => (INBOX_AT, NodeId::NONE, age_bytes(age)),
    };
    s[AT] = at;
    s[NODE..MORE].copy_from_slice(&node.0);
    s[MORE..PAYLOAD].copy_from_slice(&more);
    s[PAYLOAD..PAYLOAD + m.payload.len()].copy_from_slice(m.payload);
    s[CHECK] = check(&s);
    Some(s)
}

/// The message a slot keeps: `None` for an erased slot and one that fails
/// its check.
pub fn decode(s: &[u8; SLOT_LEN]) -> Option<Held<'_>> {
    let len = usize::from(s[LEN]);
    if len > MAX_PAYLOAD || s[CHECK] != check(s) {
        return None;
    }
    let pos: [u8; Position::WIRE_LEN] = s[POS..DST].try_into().ok()?;
    let dst: [u8; NodeId::LEN] = s[DST..HOPS].try_into().ok()?;
    let node = NodeId(s[NODE..MORE].try_into().ok()?);
    let id_bytes: [u8; 4] = s[ID..POS].try_into().ok()?;
    let more = &s[MORE..PAYLOAD];
    let at = match s[AT] & 3 {
        OWN => HeldAt::Own,
        FORWARD => HeldAt::Forward {
            parent: match node {
                NodeId::NONE => None,
                p => Some((p, Channel::new(more[1])?)),
            },
            place: more[0],
        },
        DELIVERY => HeldAt::Delivery {
            waits_for: node,
            age: age_of(more),
            flushed: s[AT] & FLUSHED != 0,
        },
        _ => HeldAt::Inbox { age: age_of(more) },
    };
    let m = Held {
        msg_id: MsgId(u32::from_be_bytes(id_bytes)),
        dst_pos: Position::decode(&pos),
        dst_id: NodeId(dst),
        payload: &s[PAYLOAD..PAYLOAD + len],
        hops: s[HOPS],
        at,
    };
    (MsgId::of(m.dst_pos, m.dst_id, m.payload) == m.msg_id).then_some(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held(payload: &[u8], at: HeldAt) -> Held<'_> {
        let dst_pos = Position::from_degrees(50.45, 30.52);
        let dst_id = NodeId([1, 2, 3, 4, 5, 6]);
        Held {
            msg_id: MsgId::of(dst_pos, dst_id, payload),
            dst_pos,
            dst_id,
            payload,
            hops: 17,
            at,
        }
    }

    #[test]
    fn a_slot_keeps_the_same_message_and_where_it_was() {
        let parent = (NodeId([7; 6]), Channel::new(12).unwrap());
        let ats = [
            HeldAt::Own,
            HeldAt::Forward {
                parent: Some(parent),
                place: 200,
            },
            HeldAt::Forward {
                parent: None,
                place: 0,
            },
            HeldAt::Delivery {
                waits_for: NodeId([8; 6]),
                age: Duration::from_secs(599),
                flushed: true,
            },
            HeldAt::Delivery {
                waits_for: NodeId::NONE,
                age: Duration::from_secs(1),
                flushed: false,
            },
            HeldAt::Inbox {
                age: Duration::from_secs(86_400),
            },
        ];
        for payload in [&b""[..], b"a text", &[7; MAX_PAYLOAD]] {
            for at in ats {
                let m = held(payload, at);
                let s = encode(&m).expect("fits");
                let back = match at {
                    // Kept to 18 h, which is past any delivery TTL.
                    HeldAt::Inbox { .. } => held(
                        payload,
                        HeldAt::Inbox {
                            age: Duration::from_secs(65_535),
                        },
                    ),
                    _ => m,
                };
                assert_eq!(decode(&s), Some(back));
            }
        }
        assert!(encode(&held(&[0; MAX_PAYLOAD + 1], HeldAt::Own)).is_none());
    }

    #[test]
    fn an_age_is_never_kept_younger() {
        let at = |age| HeldAt::Inbox { age };
        let m = held(b"a text", at(Duration::from_millis(1_001)));
        let s = encode(&m).unwrap();
        assert_eq!(decode(&s).unwrap().at, at(Duration::from_secs(2)));
    }

    #[test]
    fn an_erased_or_torn_slot_keeps_nothing() {
        assert_eq!(decode(&[0xFF; SLOT_LEN]), None);
        let m = held(b"a text", HeldAt::Own);
        // A power cut in the write leaves the end erased.
        let mut torn = encode(&m).unwrap();
        torn[PAYLOAD + 3..].fill(0xFF);
        assert_eq!(decode(&torn), None);
        let mut torn = encode(&m).unwrap();
        torn[HOPS] = 0xFF;
        assert_eq!(decode(&torn), None, "its count");
    }

    #[test]
    fn the_most_a_node_holds() {
        assert_eq!(HELD_MAX, 39 + 64 + 41 + 23);
        assert_eq!(CHECK, 255);
    }
}
