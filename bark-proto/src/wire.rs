//! Encoding and decoding of every packet in spec "Packet formats". Type
//! codes 0x4, 0x6 and 0x7 are unused (roadmap D46).
//!
//! The first byte of every packet is `type:4 | aux:4`. Every node has a
//! 4-byte [`NodeId`], and fields that only name a node (DATA `dst_hop` and
//! `src_hop`, ACK `from` and `to`) carry it. DATA `dst_pos` locates a place
//! and is the 7-byte form of [`Position::encode`]; the heartbeat carries
//! the sender's position in the 4-byte short form of [`ShortName`], which
//! the receiver rebuilds from its own (spec "Position and time", roadmap D14). DATA carries no
//! message ID: every node computes [`Data::msg_id`] (D15). Search state
//! lives in the nodes, not the packet (D16). Lengths, in bytes:
//!
//! ```text
//! HEARTBEAT 0x1/0x2/0x8 first id pos                                = 9
//! ACK       0x3      first msg from to                              = 13
//! DATA      0x5      first dst_hop src_hop dst_pos dst_id hops
//!                    time_left payload                              = 22 + payload
//! ```
//!
//! A 9-byte heartbeat is as long on air as an 8-byte one, 20 payload
//! symbols at SF5 (`radio::time_on_air`), so naming the sender costs it no
//! airtime.
//!
//! An ACK answers every offer, taken or refused: its `aux` says which. A
//! refusal is for this hop only (spec "Receiving DATA"); the network still sends no
//! failure notices (D10). An ACK names the message, the node answering
//! and the node answered, so that a sender takes only the answer to its
//! own offer, however many others offer the same message to the same
//! node (D32).
//!
//! Choices the design leaves open (O2), fixed here and listed in
//! `docs/core-interface.md` §7:
//!
//! - Short positions and message IDs are big-endian; a node ID is four
//!   opaque bytes in order.
//! - The payload is at most [`MAX_PAYLOAD`] = 233 bytes, what a frame holds
//!   after the DATA header.
//! - ACK `to` names the sender of what is answered, the DATA's `src_hop`.
//! - ACK `aux` bit 0 is set when the offer is refused. The other `aux`
//!   bits of an ACK are sent as zero and ignored on receipt.
//! - A heartbeat's type says whether the sender is sleepy (0x8), and if it
//!   is not, whether its position is a map point set in the app (0x1) or
//!   from its GPS (0x2).
//! - DATA `time_left` is the time the message has left, in whole
//!   [`TTL_UNIT`]s, rounded down.
//! - DATA's "more follows" bit is the lowest of the five spare bits of
//!   `dst_pos`; `aux` has no spare bit. The message ID covers `dst_pos`
//!   with its spare bits zero, so the bit does not change it.
//! - A fixed-size packet with trailing bytes is malformed, not padded.

use crate::event::MAX_FRAME;
use crate::ids::{MsgId, NodeId};
use crate::position::{Position, ShortName};
use crate::radio::{time_on_air, Channel, MIN_PREAMBLE_SYMBOLS};
use crate::time::Duration;

// Field widths.
const FIRST_LEN: usize = 1;
const POS_LEN: usize = Position::WIRE_LEN;
const SHORT_LEN: usize = 4;
const MSG_ID_LEN: usize = 4;
const ID_LEN: usize = 4;
const HOPS_LEN: usize = 1;
const TIME_LEFT_LEN: usize = 1;

// First byte.
const FIRST: usize = 0;
const TYPE_SHIFT: u32 = 4;
const AUX_MASK: u8 = 0x0f;

// Type codes (spec "Packet formats").
const T_HEARTBEAT_SURVEYED: u8 = 0x1;
const T_HEARTBEAT_GPS: u8 = 0x2;
const T_ACK: u8 = 0x3;
const T_DATA: u8 = 0x5;
const T_HEARTBEAT_SLEEPY: u8 = 0x8;

// HEARTBEAT.
const HEARTBEAT_ID: usize = FIRST + FIRST_LEN;
const HEARTBEAT_POS: usize = HEARTBEAT_ID + ID_LEN;
/// HEARTBEAT length.
pub const HEARTBEAT_LEN: usize = HEARTBEAT_POS + SHORT_LEN;

// ACK.
const ACK_ID: usize = FIRST + FIRST_LEN;
const ACK_FROM: usize = ACK_ID + MSG_ID_LEN;
const ACK_TO: usize = ACK_FROM + ID_LEN;
/// ACK length (spec "Packet formats": 13).
pub const ACK_LEN: usize = ACK_TO + ID_LEN;
/// The `aux` bit of an ACK that refuses.
const ACK_REFUSED: u8 = 0x1;

// DATA.
const DATA_DST_HOP: usize = FIRST + FIRST_LEN;
const DATA_SRC_HOP: usize = DATA_DST_HOP + ID_LEN;
const DATA_DST_POS: usize = DATA_SRC_HOP + ID_LEN;
const DATA_DST_ID: usize = DATA_DST_POS + POS_LEN;
const DATA_HOPS: usize = DATA_DST_ID + ID_LEN;
const DATA_TIME_LEFT: usize = DATA_HOPS + HOPS_LEN;
/// DATA header length (spec "Packet formats": 22).
pub const DATA_HEADER_LEN: usize = DATA_TIME_LEFT + TIME_LEFT_LEN;
/// The byte of DATA that holds the "more follows" bit: the last of
/// `dst_pos`, whose low five bits are spare (`Position::encode`).
const DATA_MORE_AT: usize = DATA_DST_POS + POS_LEN - 1;
/// The "more follows" bit: the lowest spare bit of `dst_pos`. DATA `aux`
/// holds the sender's home channel, 0 to 12, in all four of its bits.
const DATA_MORE: u8 = 0x01;

/// Largest application payload: [`MAX_FRAME`] less the DATA header,
/// 255 − 22 = 233 bytes.
pub const MAX_PAYLOAD: usize = MAX_FRAME - DATA_HEADER_LEN;

const _: () = assert!(HEARTBEAT_LEN == 9 && ACK_LEN == 13);
const _: () = assert!(
    time_on_air(HEARTBEAT_LEN as u8, MIN_PREAMBLE_SYMBOLS).as_micros()
        == time_on_air(8, MIN_PREAMBLE_SYMBOLS).as_micros(),
    "the node ID costs the heartbeat no airtime"
);
const _: () = assert!(DATA_HEADER_LEN == 22);
const _: () = assert!(MAX_PAYLOAD == 233);

/// HEARTBEAT, types 0x1 and 0x2 from a node that is not sleepy, 0x8
/// from a sleepy one (spec "Heartbeat").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Heartbeat {
    /// Surveyed (0x1) or GPS-derived (0x2) position; a sleepy node's
    /// heartbeat does not say, and reads as not surveyed.
    pub surveyed: bool,
    /// From a sleepy node (0x8): one that beats only on its home channel,
    /// slowly, and is a next hop only where no other kind is in reach.
    pub sleepy: bool,
    /// The sender's home channel.
    pub home: Channel,
    /// The sender.
    pub id: NodeId,
    /// The sender's position in short form; [`Heartbeat::position`]
    /// rebuilds it.
    pub short: ShortName,
}

impl Heartbeat {
    /// The sender's position, rebuilt by a receiver at `near`:
    /// unambiguous among everything a node hears ([`Position::from_short`]).
    pub fn position(&self, near: Position) -> Position {
        Position::from_short(self.short, near)
    }
}

/// ACK, type 0x3 (spec "The ACK"): the answer to an offer, taken or refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ack {
    /// The answered message.
    pub msg_id: MsgId,
    /// The answering node.
    pub from: NodeId,
    /// The answered node.
    pub to: NodeId,
    /// The offer is refused (spec "Receiving DATA"): `from` does not take it.
    pub refused: bool,
}

impl Ack {
    /// Whether this answers the same offer as `other`: the same message
    /// between the same two nodes, taken or refused (roadmap D32).
    pub fn answers(&self, other: &Ack) -> bool {
        self.msg_id == other.msg_id && self.from == other.from && self.to == other.to
    }
}

/// DATA, type 0x5 (spec "Forwarding", "Packet formats").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Data<'a> {
    /// The sender's home channel, where the ACK and anything sent straight
    /// back go (`aux`).
    pub sender_home: Channel,
    /// The node this transmission is addressed to.
    pub dst_hop: NodeId,
    /// The node that sent this transmission.
    pub src_hop: NodeId,
    /// The destination position (spec "Addresses").
    pub dst_pos: Position,
    /// The destination node.
    pub dst_id: NodeId,
    /// Hand-overs so far (spec "Hop count").
    pub hops: u8,
    /// The time the message has left, in [`TTL_UNIT`]s (spec "Receiving DATA").
    pub time_left: u8,
    /// More messages for the node it is addressed to wait at the sender
    /// (spec "Delivery"); set only on a DATA to a sleepy node.
    pub more: bool,
    /// Opaque application payload, at most [`MAX_PAYLOAD`] bytes.
    pub payload: &'a [u8],
}

/// Unit of DATA's time left (spec "Packet formats"). Each node rounds the time
/// down when it sends, so each hand-over loses up to one unit; 2.5 s is
/// the finest unit at which one byte holds the delivery TTL, 600 s, as 240
/// (spec "Receiving DATA").
pub const TTL_UNIT: Duration = Duration::from_millis(2500);

impl Data<'_> {
    /// The message's ID (roadmap D15), the same on every hop.
    pub fn msg_id(&self) -> MsgId {
        MsgId::of(self.dst_pos, self.dst_id, self.payload)
    }

    /// The time the message has left.
    pub const fn time_left(&self) -> Duration {
        TTL_UNIT.mul(self.time_left as u64)
    }

    /// `left` in whole [`TTL_UNIT`]s, rounded down, so that no node keeps
    /// a message longer than it had, and capped at what the field holds.
    pub const fn units(left: Duration) -> u8 {
        let u = left.as_micros() / TTL_UNIT.as_micros();
        if u > u8::MAX as u64 {
            u8::MAX
        } else {
            u as u8
        }
    }
}

/// Any packet. Payloads borrow from the received frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Packet<'a> {
    /// HEARTBEAT.
    Heartbeat(Heartbeat),
    /// ACK.
    Ack(Ack),
    /// DATA.
    Data(Data<'a>),
}

/// Why a frame did not decode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// Unknown type nibble.
    BadType(u8),
    /// Fewer bytes than the packet's fields need.
    Short,
    /// Longer than [`MAX_FRAME`], or trailing bytes after a fixed-size packet.
    BadLength,
    /// A channel field outside the plan.
    ChannelOutOfRange(u8),
    /// A payload longer than [`MAX_PAYLOAD`].
    PayloadTooLong,
}

/// Why a packet did not encode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncodeError {
    /// The payload is longer than [`MAX_PAYLOAD`].
    PayloadTooLong,
}

fn first(kind: u8, aux: u8) -> u8 {
    (kind << TYPE_SHIFT) | (aux & AUX_MASK)
}

fn put(buf: &mut [u8], at: usize, bytes: &[u8]) {
    buf[at..at + bytes.len()].copy_from_slice(bytes);
}

fn get4(b: &[u8], at: usize) -> [u8; 4] {
    let mut a = [0; 4];
    a.copy_from_slice(&b[at..at + 4]);
    a
}

fn get_pos(b: &[u8], at: usize) -> Position {
    let mut a = [0; POS_LEN];
    a.copy_from_slice(&b[at..at + POS_LEN]);
    Position::decode(&a)
}

fn get_short(b: &[u8], at: usize) -> ShortName {
    ShortName(u32::from_be_bytes(get4(b, at)))
}

fn get_msg_id(b: &[u8], at: usize) -> MsgId {
    MsgId(u32::from_be_bytes(get4(b, at)))
}

fn channel(aux: u8) -> Result<Channel, DecodeError> {
    Channel::new(aux).ok_or(DecodeError::ChannelOutOfRange(aux))
}

/// Exactly `len` bytes, or the error that describes the mismatch.
fn exact(b: &[u8], len: usize) -> Result<(), DecodeError> {
    match b.len().cmp(&len) {
        core::cmp::Ordering::Less => Err(DecodeError::Short),
        core::cmp::Ordering::Equal => Ok(()),
        core::cmp::Ordering::Greater => Err(DecodeError::BadLength),
    }
}

/// The payload after a `header`-byte header: there must be a header, and
/// the payload must be within [`MAX_PAYLOAD`].
fn payload(b: &[u8], header: usize) -> Result<&[u8], DecodeError> {
    let p = b.get(header..).ok_or(DecodeError::Short)?;
    if p.len() > MAX_PAYLOAD {
        return Err(DecodeError::PayloadTooLong);
    }
    Ok(p)
}

impl Packet<'_> {
    /// Encode into `buf` and return the frame length.
    pub fn encode(&self, buf: &mut [u8; MAX_FRAME]) -> Result<usize, EncodeError> {
        match self {
            Packet::Heartbeat(h) => {
                let kind = match (h.sleepy, h.surveyed) {
                    (true, _) => T_HEARTBEAT_SLEEPY,
                    (false, true) => T_HEARTBEAT_SURVEYED,
                    (false, false) => T_HEARTBEAT_GPS,
                };
                buf[FIRST] = first(kind, h.home.index());
                put(buf, HEARTBEAT_ID, &h.id.0);
                put(buf, HEARTBEAT_POS, &h.short.0.to_be_bytes());
                Ok(HEARTBEAT_LEN)
            }
            Packet::Ack(a) => {
                let aux = if a.refused { ACK_REFUSED } else { 0 };
                buf[FIRST] = first(T_ACK, aux);
                put(buf, ACK_ID, &a.msg_id.0.to_be_bytes());
                put(buf, ACK_FROM, &a.from.0);
                put(buf, ACK_TO, &a.to.0);
                Ok(ACK_LEN)
            }
            Packet::Data(d) => {
                if d.payload.len() > MAX_PAYLOAD {
                    return Err(EncodeError::PayloadTooLong);
                }
                buf[FIRST] = first(T_DATA, d.sender_home.index());
                put(buf, DATA_DST_HOP, &d.dst_hop.0);
                put(buf, DATA_SRC_HOP, &d.src_hop.0);
                put(buf, DATA_DST_POS, &d.dst_pos.encode());
                if d.more {
                    buf[DATA_MORE_AT] |= DATA_MORE;
                }
                put(buf, DATA_DST_ID, &d.dst_id.0);
                buf[DATA_HOPS] = d.hops;
                buf[DATA_TIME_LEFT] = d.time_left;
                put(buf, DATA_HEADER_LEN, d.payload);
                Ok(DATA_HEADER_LEN + d.payload.len())
            }
        }
    }
}

impl<'a> Packet<'a> {
    /// Decode a received frame. Never panics, whatever the bytes.
    pub fn decode(b: &'a [u8]) -> Result<Packet<'a>, DecodeError> {
        if b.len() > MAX_FRAME {
            return Err(DecodeError::BadLength);
        }
        let first = *b.first().ok_or(DecodeError::Short)?;
        let kind = first >> TYPE_SHIFT;
        let aux = first & AUX_MASK;
        match kind {
            T_HEARTBEAT_SURVEYED | T_HEARTBEAT_GPS | T_HEARTBEAT_SLEEPY => {
                exact(b, HEARTBEAT_LEN)?;
                Ok(Packet::Heartbeat(Heartbeat {
                    surveyed: kind == T_HEARTBEAT_SURVEYED,
                    sleepy: kind == T_HEARTBEAT_SLEEPY,
                    home: channel(aux)?,
                    id: NodeId(get4(b, HEARTBEAT_ID)),
                    short: get_short(b, HEARTBEAT_POS),
                }))
            }
            T_ACK => {
                exact(b, ACK_LEN)?;
                Ok(Packet::Ack(Ack {
                    msg_id: get_msg_id(b, ACK_ID),
                    from: NodeId(get4(b, ACK_FROM)),
                    to: NodeId(get4(b, ACK_TO)),
                    refused: aux & ACK_REFUSED != 0,
                }))
            }
            T_DATA => {
                let payload = payload(b, DATA_HEADER_LEN)?;
                Ok(Packet::Data(Data {
                    sender_home: channel(aux)?,
                    dst_hop: NodeId(get4(b, DATA_DST_HOP)),
                    src_hop: NodeId(get4(b, DATA_SRC_HOP)),
                    dst_pos: get_pos(b, DATA_DST_POS),
                    dst_id: NodeId(get4(b, DATA_DST_ID)),
                    hops: b[DATA_HOPS],
                    time_left: b[DATA_TIME_LEFT],
                    more: b[DATA_MORE_AT] & DATA_MORE != 0,
                    payload,
                }))
            }
            other => Err(DecodeError::BadType(other)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::radio::CHANNELS;
    use crate::time::Duration;
    use proptest::prelude::*;

    fn position() -> impl Strategy<Value = Position> {
        (-(1i32 << 24)..(1 << 24), -(1i32 << 25)..(1 << 25))
            .prop_map(|(lat, lon)| Position { lat, lon })
    }

    fn chan() -> impl Strategy<Value = Channel> {
        (0..CHANNELS).prop_map(|c| Channel::new(c).unwrap())
    }

    fn id() -> impl Strategy<Value = NodeId> {
        any::<[u8; 4]>().prop_map(NodeId)
    }

    fn msg() -> impl Strategy<Value = MsgId> {
        any::<u32>().prop_map(MsgId)
    }

    fn payload() -> impl Strategy<Value = std::vec::Vec<u8>> {
        proptest::collection::vec(any::<u8>(), 0..=MAX_PAYLOAD)
    }

    extern crate std;

    /// Encode, check the length against the table above, decode, compare.
    fn round_trip(p: &Packet<'_>) {
        let mut buf = [0; MAX_FRAME];
        let len = p.encode(&mut buf).unwrap();
        let expected = match p {
            Packet::Heartbeat(_) => 9,
            Packet::Ack(_) => 13,
            Packet::Data(d) => 22 + d.payload.len(),
        };
        assert_eq!(len, expected);
        assert_eq!(Packet::decode(&buf[..len]).as_ref(), Ok(p));
    }

    proptest! {
        #[test]
        fn heartbeat_round_trips(
            surveyed: bool,
            sleepy: bool,
            home in chan(),
            id in id(),
            position in position(),
        ) {
            let surveyed = surveyed && !sleepy;
            let hb = Heartbeat { surveyed, sleepy, home, id, short: position.short() };
            round_trip(&Packet::Heartbeat(hb));
            prop_assert_eq!(hb.position(position), position);
        }

        #[test]
        fn ack_round_trips(msg_id in msg(), from: [u8; 4], to: [u8; 4], refused: bool) {
            let ack = Ack { msg_id, from: NodeId(from), to: NodeId(to), refused };
            round_trip(&Packet::Ack(ack));
            let flipped = Ack { refused: !refused, ..ack };
            prop_assert!(ack.answers(&flipped));
            let other = Ack { to: NodeId([!to[0], to[1], to[2], to[3]]), ..ack };
            prop_assert!(!ack.answers(&other), "another sender's answer");
        }

        #[test]
        fn data_round_trips(
            sender_home in chan(),
            dst_hop in id(),
            src_hop in id(),
            dst_pos in position(),
            dst_id in id(),
            hops: u8,
            time_left: u8,
            more: bool,
            payload in payload(),
        ) {
            let d = Data { sender_home, dst_hop, src_hop, dst_pos, dst_id, hops, more, time_left, payload: &payload };
            round_trip(&Packet::Data(d));
            // The ID survives the trip: it depends only on fields every hop
            // keeps, and the "more follows" bit leaves the position alone.
            let mut buf = [0; MAX_FRAME];
            let len = Packet::Data(d).encode(&mut buf).unwrap();
            let Ok(Packet::Data(back)) = Packet::decode(&buf[..len]) else { panic!() };
            prop_assert_eq!(back.msg_id(), MsgId::of(dst_pos, dst_id, &payload));
        }

        #[test]
        fn decode_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..=MAX_FRAME + 2)) {
            // Whatever decodes must re-encode to the same length and decode
            // the same, apart from bits the decoder ignores (unused aux bits
            // of an ACK, spare position bits but DATA's "more follows").
            if let Ok(p) = Packet::decode(&bytes) {
                let mut buf = [0; MAX_FRAME];
                let len = p.encode(&mut buf).unwrap();
                prop_assert_eq!(len, bytes.len());
                prop_assert_eq!(Packet::decode(&buf[..len]), Ok(p));
            }
        }

        #[test]
        fn every_type_never_panics(kind in 0u8..16, aux in 0u8..16, rest in proptest::collection::vec(any::<u8>(), 0..=MAX_FRAME)) {
            // Random first bytes rarely hit a known type with a valid aux;
            // force every type nibble so each decoder's length paths run.
            let mut bytes = std::vec![first(kind, aux)];
            bytes.extend_from_slice(&rest);
            let _ = Packet::decode(&bytes);
            for cut in [0, 1, 4, 5, 8, 9, 10, 20, 21, 22, 23] {
                let _ = Packet::decode(&bytes[..cut.min(bytes.len())]);
            }
        }
    }

    #[test]
    fn full_frame_fits() {
        let payload = [0xa5; MAX_PAYLOAD];
        let d = Data {
            sender_home: Channel::new(3).unwrap(),
            dst_hop: NodeId([1; 4]),
            src_hop: NodeId([2; 4]),
            dst_pos: Position::default(),
            dst_id: NodeId([3; 4]),
            hops: 0,
            time_left: 0,
            more: false,
            payload: &payload,
        };
        let mut buf = [0; MAX_FRAME];
        assert_eq!(Packet::Data(d).encode(&mut buf), Ok(MAX_FRAME));
        let long = [0; MAX_PAYLOAD + 1];
        assert_eq!(
            Packet::Data(Data {
                payload: &long,
                ..d
            })
            .encode(&mut buf),
            Err(EncodeError::PayloadTooLong)
        );
    }

    #[test]
    fn malformed_frames() {
        use DecodeError::*;
        assert_eq!(Packet::decode(&[]), Err(Short));
        assert_eq!(Packet::decode(&[0x00]), Err(BadType(0)));
        for gone in [0x40, 0x60, 0x70] {
            assert_eq!(
                Packet::decode(&[gone; 9]),
                Err(BadType(gone >> 4)),
                "ATTACH, DELIVER and ATTACHED are gone"
            );
        }
        assert_eq!(Packet::decode(&[0x90; 7]), Err(BadType(9)));
        assert_eq!(Packet::decode(&[0x80, 0, 0, 0, 0]), Err(Short));
        assert_eq!(
            Packet::decode(&[0x1d, 0, 0, 0, 0, 0, 0, 0, 0]),
            Err(ChannelOutOfRange(13))
        );
        assert_eq!(Packet::decode(&[0x10, 0, 0, 0, 0, 0, 0, 0]), Err(Short));
        assert_eq!(
            Packet::decode(&[0x10, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
            Err(BadLength)
        );
        assert_eq!(Packet::decode(&[0x30; ACK_LEN - 1]), Err(Short));
        assert_eq!(Packet::decode(&[0x30; ACK_LEN + 1]), Err(BadLength));
        assert_eq!(Packet::decode(&[0; MAX_FRAME + 1]), Err(BadLength));

        let mut data = [0u8; DATA_HEADER_LEN];
        data[FIRST] = first(T_DATA, 13);
        assert_eq!(Packet::decode(&data), Err(ChannelOutOfRange(13)));
        data[FIRST] = first(T_DATA, 0);
        assert!(
            Packet::decode(&data).is_ok(),
            "an empty payload is a payload"
        );
        assert_eq!(Packet::decode(&data[..DATA_HEADER_LEN - 1]), Err(Short));
    }

    #[test]
    fn exact_bytes() {
        // Pin the layouts: field order, big-endian positions and IDs.
        let mut buf = [0; MAX_FRAME];
        let position = Position::from_degrees(50.45, 30.52);
        let hb = Heartbeat {
            surveyed: false,
            sleepy: false,
            home: Channel::new(12).unwrap(),
            id: NodeId([1, 2, 3, 4]),
            short: ShortName(0x0506_0708),
        };
        let len = Packet::Heartbeat(hb).encode(&mut buf).unwrap();
        assert_eq!(&buf[..len], &[0x2c, 1, 2, 3, 4, 5, 6, 7, 8]);
        let sleepy = Heartbeat { sleepy: true, ..hb };
        Packet::Heartbeat(sleepy).encode(&mut buf).unwrap();
        assert_eq!(buf[0], 0x8c);
        assert_eq!(
            time_on_air(len as u8, MIN_PREAMBLE_SYMBOLS),
            Duration::from_micros(11_840),
            "46.25 symbols at SF5, as for 8 bytes"
        );

        let ack = Ack {
            msg_id: MsgId(0x0102_0304),
            from: NodeId([5, 6, 7, 8]),
            to: NodeId([9, 10, 11, 12]),
            refused: false,
        };
        let len = Packet::Ack(ack).encode(&mut buf).unwrap();
        assert_eq!(&buf[..len], &[0x30, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
        let refused = Ack {
            refused: true,
            ..ack
        };
        let len = Packet::Ack(refused).encode(&mut buf).unwrap();
        assert_eq!(&buf[..len], &[0x31, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
        assert_eq!(
            time_on_air(len as u8, MIN_PREAMBLE_SYMBOLS),
            Duration::from_micros(14_400),
            "51.25 symbols at SF5"
        );

        let d = Data {
            sender_home: Channel::new(9).unwrap(),
            dst_hop: NodeId([1, 2, 3, 4]),
            src_hop: NodeId([5, 6, 7, 8]),
            dst_pos: position,
            dst_id: NodeId([9, 10, 11, 12]),
            hops: 13,
            time_left: 14,
            more: false,
            payload: &[15, 16],
        };
        let len = Packet::Data(d).encode(&mut buf).unwrap();
        let mut want = std::vec![0x59, 1, 2, 3, 4, 5, 6, 7, 8];
        want.extend_from_slice(&position.encode());
        want.extend_from_slice(&[9, 10, 11, 12, 13, 14, 15, 16]);
        assert_eq!(&buf[..len], &want[..]);
        let header = Data { payload: &[], ..d };
        let len = Packet::Data(header).encode(&mut buf).unwrap();
        assert_eq!(
            crate::radio::time_on_air(len as u8, MIN_PREAMBLE_SYMBOLS),
            Duration::from_micros(18_240),
            "71.25 symbols at SF5, as for 21 bytes"
        );
        assert_eq!(d.time_left(), Duration::from_secs(35));
        assert_eq!(Data::units(Duration::from_millis(600_000)), 240);
        assert_eq!(Data::units(Duration::from_millis(2_499)), 0, "rounded down");
        assert_eq!(Data::units(Duration::from_secs(1000)), u8::MAX);
    }
}
