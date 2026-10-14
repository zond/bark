//! Encoding and decoding of every packet in design §14.
//!
//! The first byte of every packet is `type:4 | aux:4`. Positions are the
//! 7-byte form of [`Position::encode`]; message and leaf identifiers are
//! 4 bytes, big-endian; visited-list entries are 2-byte big-endian
//! 16-bit position hashes. Lengths, in bytes:
//!
//! ```text
//! HEARTBEAT 0x1/0x2  first pos                                         = 8
//! ACK       0x3      first msg_id                                      = 5
//! ATTACH    0x4      first leaf_id relay_pos                           = 12
//! DATA      0x5      first dst_hop msg_id dst_pos dst_leaf hops
//!                    visited_len visited(2n) payload                   = 25 + 2n + payload
//! DELIVER   0x6      first dst_leaf msg_id payload                     = 9 + payload
//! ```
//!
//! Type 0x7 is unassigned: the network sends no failure notices (roadmap D10).
//!
//! Choices the design leaves open (O2), fixed here and listed in
//! `docs/core-interface.md` §7:
//!
//! - The visited list holds at most [`MAX_VISITED`] = 16 entries, the depth
//!   design §8.1 sizes it for, and the application payload is at most
//!   [`MAX_PAYLOAD`], what remains of [`MAX_FRAME`] after the DATA header
//!   and a full visited list. So every DATA packet, whatever its payload,
//!   can be extended to the full void-recovery depth: whether a message can
//!   route around a void never depends on its size. The decoder enforces
//!   both bounds, so the invariant holds for packets from other nodes too.
//! - `aux` is unused in ACK and DELIVER; it is sent as zero and ignored on
//!   receipt, as are the spare position bits.
//! - ATTACH `aux` is the [`RxMode`] code.
//! - A fixed-size packet with trailing bytes is malformed, not padded.

use crate::event::MAX_FRAME;
use crate::ids::{LeafId, MsgId};
use crate::position::Position;
use crate::radio::Channel;
use crate::tables::visited::Visited;

// Field widths.
const FIRST_LEN: usize = 1;
const POS_LEN: usize = Position::WIRE_LEN;
const MSG_ID_LEN: usize = 4;
const LEAF_ID_LEN: usize = 4;
const HOPS_LEN: usize = 1;
const VISITED_LEN_LEN: usize = 1;
const HASH_LEN: usize = 2;

// First byte.
const FIRST: usize = 0;
const TYPE_SHIFT: u32 = 4;
const AUX_MASK: u8 = 0x0f;

// Type codes (design §14).
const T_HEARTBEAT_SURVEYED: u8 = 0x1;
const T_HEARTBEAT_GPS: u8 = 0x2;
const T_ACK: u8 = 0x3;
const T_ATTACH: u8 = 0x4;
const T_DATA: u8 = 0x5;
const T_DELIVER: u8 = 0x6;

// HEARTBEAT.
const HEARTBEAT_POS: usize = FIRST + FIRST_LEN;
/// HEARTBEAT length (design §14: 8).
pub const HEARTBEAT_LEN: usize = HEARTBEAT_POS + POS_LEN;

// ACK.
const ACK_MSG_ID: usize = FIRST + FIRST_LEN;
/// ACK length (design §14: 5).
pub const ACK_LEN: usize = ACK_MSG_ID + MSG_ID_LEN;

// ATTACH.
const ATTACH_LEAF: usize = FIRST + FIRST_LEN;
const ATTACH_RELAY_POS: usize = ATTACH_LEAF + LEAF_ID_LEN;
/// ATTACH length (design §14: 12).
pub const ATTACH_LEN: usize = ATTACH_RELAY_POS + POS_LEN;

// DATA.
const DATA_DST_HOP: usize = FIRST + FIRST_LEN;
const DATA_MSG_ID: usize = DATA_DST_HOP + POS_LEN;
const DATA_DST_POS: usize = DATA_MSG_ID + MSG_ID_LEN;
const DATA_DST_LEAF: usize = DATA_DST_POS + POS_LEN;
const DATA_HOPS: usize = DATA_DST_LEAF + LEAF_ID_LEN;
const DATA_VISITED_LEN: usize = DATA_HOPS + HOPS_LEN;
const DATA_VISITED: usize = DATA_VISITED_LEN + VISITED_LEN_LEN;
/// DATA header length before the visited list (design §14: 25).
pub const DATA_HEADER_LEN: usize = DATA_VISITED;

// DELIVER.
const DELIVER_LEAF: usize = FIRST + FIRST_LEN;
const DELIVER_MSG_ID: usize = DELIVER_LEAF + LEAF_ID_LEN;
/// DELIVER header length before the payload (design §14: 9).
pub const DELIVER_HEADER_LEN: usize = DELIVER_MSG_ID + MSG_ID_LEN;

/// Most entries a visited list carries: the 16-deep list design §8.1 sizes
/// at 32 bytes.
pub const MAX_VISITED: usize = 16;

/// Largest application payload: [`MAX_FRAME`] less the DATA header and a
/// full visited list, 255 − 25 − 32 = 198 bytes. DATA and DELIVER carry at
/// most this.
pub const MAX_PAYLOAD: usize = MAX_FRAME - DATA_HEADER_LEN - MAX_VISITED * HASH_LEN;

const _: () = assert!(HEARTBEAT_LEN == 8 && ACK_LEN == 5 && ATTACH_LEN == 12);
const _: () = assert!(DATA_HEADER_LEN == 25 && DELIVER_HEADER_LEN == 9);
const _: () = assert!(MAX_PAYLOAD == 198);
const _: () = assert!(MAX_VISITED <= u8::MAX as usize, "visited_len is one byte");

/// A leaf's receive mode, carried in ATTACH `aux` (design §10.5). Sets the
/// preamble of packets the relay sends to the leaf.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum RxMode {
    /// Always listening; normal preamble.
    Listening = 0,
    /// Sleeps and wakes for CAD every 500 ms; needs a long preamble.
    DutyCycled = 1,
    /// Listens one rotation every ten minutes, CAD in between; long preamble.
    Hybrid = 2,
}

impl RxMode {
    /// From the wire code, or `None` if unknown.
    pub const fn from_code(code: u8) -> Option<RxMode> {
        match code {
            0 => Some(RxMode::Listening),
            1 => Some(RxMode::DutyCycled),
            2 => Some(RxMode::Hybrid),
            _ => None,
        }
    }
}

/// HEARTBEAT, types 0x1 and 0x2 (design §7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Heartbeat {
    /// Surveyed (0x1) or GPS-derived (0x2) position.
    pub surveyed: bool,
    /// The sender's home channel.
    pub home: Channel,
    /// The sender's position, which is its identity.
    pub position: Position,
}

/// ACK, type 0x3 (design §9.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ack {
    /// The acknowledged message.
    pub msg_id: MsgId,
}

/// ATTACH, type 0x4 (design §10.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attach {
    /// The attaching leaf.
    pub leaf: LeafId,
    /// The relay attached to.
    pub relay: Position,
    /// The leaf's receive mode.
    pub mode: RxMode,
}

/// The DATA routing header (design §14).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Route {
    /// The sender's home channel, where the ACK goes (`aux`).
    pub sender_home: Channel,
    /// The next hop this transmission is addressed to.
    pub dst_hop: Position,
    /// The message, constant across hops.
    pub msg_id: MsgId,
    /// The destination relay's position.
    pub dst_pos: Position,
    /// The destination leaf, or [`LeafId::RELAY`].
    pub dst_leaf: LeafId,
    /// Hops taken so far.
    pub hops: u8,
    /// Void-recovery visited list.
    pub visited: Visited,
}

/// DATA, type 0x5 (design §9, §14).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Data<'a> {
    /// Routing header.
    pub route: Route,
    /// Opaque application payload, at most [`MAX_PAYLOAD`] bytes.
    pub payload: &'a [u8],
}

/// DELIVER, type 0x6: relay to leaf on the relay's home channel (design §10.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Deliver<'a> {
    /// The receiving leaf.
    pub dst_leaf: LeafId,
    /// The message.
    pub msg_id: MsgId,
    /// Opaque application payload, at most [`MAX_PAYLOAD`] bytes.
    pub payload: &'a [u8],
}

/// Any packet. Payloads borrow from the received frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Packet<'a> {
    /// HEARTBEAT.
    Heartbeat(Heartbeat),
    /// ACK.
    Ack(Ack),
    /// ATTACH.
    Attach(Attach),
    /// DATA.
    Data(Data<'a>),
    /// DELIVER.
    Deliver(Deliver<'a>),
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
    /// A visited list longer than [`MAX_VISITED`].
    VisitedTooLong(u8),
    /// A payload longer than [`MAX_PAYLOAD`].
    PayloadTooLong,
    /// An unknown ATTACH receive mode.
    BadRxMode(u8),
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

fn put_pos(buf: &mut [u8], at: usize, p: Position) {
    buf[at..at + POS_LEN].copy_from_slice(&p.encode());
}

fn put_u32(buf: &mut [u8], at: usize, v: u32) {
    buf[at..at + 4].copy_from_slice(&v.to_be_bytes());
}

fn get_pos(b: &[u8], at: usize) -> Position {
    let mut a = [0; POS_LEN];
    a.copy_from_slice(&b[at..at + POS_LEN]);
    Position::decode(&a)
}

fn get_u32(b: &[u8], at: usize) -> u32 {
    let mut a = [0; 4];
    a.copy_from_slice(&b[at..at + 4]);
    u32::from_be_bytes(a)
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

impl Route {
    /// Write the header, visited list included, into `buf`; returns where
    /// the payload starts.
    fn encode(&self, buf: &mut [u8; MAX_FRAME]) -> usize {
        buf[FIRST] = first(T_DATA, self.sender_home.index());
        put_pos(buf, DATA_DST_HOP, self.dst_hop);
        put_u32(buf, DATA_MSG_ID, self.msg_id.0);
        put_pos(buf, DATA_DST_POS, self.dst_pos);
        buf[DATA_DST_LEAF..DATA_DST_LEAF + LEAF_ID_LEN].copy_from_slice(&self.dst_leaf.0);
        buf[DATA_HOPS] = self.hops;
        buf[DATA_VISITED_LEN] = self.visited.len() as u8;
        let mut at = DATA_VISITED;
        for h in self.visited.hashes() {
            buf[at..at + HASH_LEN].copy_from_slice(&h.to_be_bytes());
            at += HASH_LEN;
        }
        at
    }

    /// Read the header from `b`, which holds at least `DATA_HEADER_LEN`
    /// bytes; returns the header and the payload.
    fn decode(b: &[u8]) -> Result<(Route, &[u8]), DecodeError> {
        if b.len() < DATA_HEADER_LEN {
            return Err(DecodeError::Short);
        }
        let n = b[DATA_VISITED_LEN];
        if n as usize > MAX_VISITED {
            return Err(DecodeError::VisitedTooLong(n));
        }
        let payload_at = DATA_VISITED + n as usize * HASH_LEN;
        if b.len() < payload_at {
            return Err(DecodeError::Short);
        }
        let mut visited = Visited::new();
        for e in b[DATA_VISITED..payload_at].as_chunks::<HASH_LEN>().0 {
            visited.push(u16::from_be_bytes(*e));
        }
        let mut leaf = [0; LEAF_ID_LEN];
        leaf.copy_from_slice(&b[DATA_DST_LEAF..DATA_DST_LEAF + LEAF_ID_LEN]);
        let route = Route {
            sender_home: channel(b[FIRST] & AUX_MASK)?,
            dst_hop: get_pos(b, DATA_DST_HOP),
            msg_id: MsgId(get_u32(b, DATA_MSG_ID)),
            dst_pos: get_pos(b, DATA_DST_POS),
            dst_leaf: LeafId(leaf),
            hops: b[DATA_HOPS],
            visited,
        };
        Ok((route, &b[payload_at..]))
    }
}

impl Packet<'_> {
    /// Encode into `buf` and return the frame length.
    pub fn encode(&self, buf: &mut [u8; MAX_FRAME]) -> Result<usize, EncodeError> {
        match self {
            Packet::Heartbeat(h) => {
                let kind = if h.surveyed { T_HEARTBEAT_SURVEYED } else { T_HEARTBEAT_GPS };
                buf[FIRST] = first(kind, h.home.index());
                put_pos(buf, HEARTBEAT_POS, h.position);
                Ok(HEARTBEAT_LEN)
            }
            Packet::Ack(a) => {
                buf[FIRST] = first(T_ACK, 0);
                put_u32(buf, ACK_MSG_ID, a.msg_id.0);
                Ok(ACK_LEN)
            }
            Packet::Attach(a) => {
                buf[FIRST] = first(T_ATTACH, a.mode as u8);
                buf[ATTACH_LEAF..ATTACH_LEAF + LEAF_ID_LEN].copy_from_slice(&a.leaf.0);
                put_pos(buf, ATTACH_RELAY_POS, a.relay);
                Ok(ATTACH_LEN)
            }
            Packet::Data(d) => {
                if d.payload.len() > MAX_PAYLOAD {
                    return Err(EncodeError::PayloadTooLong);
                }
                let at = d.route.encode(buf);
                buf[at..at + d.payload.len()].copy_from_slice(d.payload);
                Ok(at + d.payload.len())
            }
            Packet::Deliver(d) => {
                if d.payload.len() > MAX_PAYLOAD {
                    return Err(EncodeError::PayloadTooLong);
                }
                buf[FIRST] = first(T_DELIVER, 0);
                buf[DELIVER_LEAF..DELIVER_LEAF + LEAF_ID_LEN].copy_from_slice(&d.dst_leaf.0);
                put_u32(buf, DELIVER_MSG_ID, d.msg_id.0);
                let end = DELIVER_HEADER_LEN + d.payload.len();
                buf[DELIVER_HEADER_LEN..end].copy_from_slice(d.payload);
                Ok(end)
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
            T_HEARTBEAT_SURVEYED | T_HEARTBEAT_GPS => {
                exact(b, HEARTBEAT_LEN)?;
                Ok(Packet::Heartbeat(Heartbeat {
                    surveyed: kind == T_HEARTBEAT_SURVEYED,
                    home: channel(aux)?,
                    position: get_pos(b, HEARTBEAT_POS),
                }))
            }
            T_ACK => {
                exact(b, ACK_LEN)?;
                Ok(Packet::Ack(Ack { msg_id: MsgId(get_u32(b, ACK_MSG_ID)) }))
            }
            T_ATTACH => {
                exact(b, ATTACH_LEN)?;
                let mut leaf = [0; LEAF_ID_LEN];
                leaf.copy_from_slice(&b[ATTACH_LEAF..ATTACH_LEAF + LEAF_ID_LEN]);
                Ok(Packet::Attach(Attach {
                    leaf: LeafId(leaf),
                    relay: get_pos(b, ATTACH_RELAY_POS),
                    mode: RxMode::from_code(aux).ok_or(DecodeError::BadRxMode(aux))?,
                }))
            }
            T_DATA => {
                let (route, payload) = Route::decode(b)?;
                if payload.len() > MAX_PAYLOAD {
                    return Err(DecodeError::PayloadTooLong);
                }
                Ok(Packet::Data(Data { route, payload }))
            }
            T_DELIVER => {
                if b.len() < DELIVER_HEADER_LEN {
                    return Err(DecodeError::Short);
                }
                let payload = &b[DELIVER_HEADER_LEN..];
                if payload.len() > MAX_PAYLOAD {
                    return Err(DecodeError::PayloadTooLong);
                }
                let mut leaf = [0; LEAF_ID_LEN];
                leaf.copy_from_slice(&b[DELIVER_LEAF..DELIVER_LEAF + LEAF_ID_LEN]);
                Ok(Packet::Deliver(Deliver {
                    dst_leaf: LeafId(leaf),
                    msg_id: MsgId(get_u32(b, DELIVER_MSG_ID)),
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
    use proptest::prelude::*;

    fn position() -> impl Strategy<Value = Position> {
        (-(1i32 << 24)..(1 << 24), -(1i32 << 25)..(1 << 25)).prop_map(|(lat, lon)| Position { lat, lon })
    }

    fn chan() -> impl Strategy<Value = Channel> {
        (0..CHANNELS).prop_map(|c| Channel::new(c).unwrap())
    }

    fn leaf() -> impl Strategy<Value = LeafId> {
        any::<[u8; 4]>().prop_map(LeafId)
    }

    fn msg() -> impl Strategy<Value = MsgId> {
        any::<u32>().prop_map(MsgId)
    }

    fn rx_mode() -> impl Strategy<Value = RxMode> {
        prop_oneof![Just(RxMode::Listening), Just(RxMode::DutyCycled), Just(RxMode::Hybrid)]
    }

    fn route() -> impl Strategy<Value = Route> {
        (
            chan(),
            position(),
            msg(),
            position(),
            leaf(),
            any::<u8>(),
            proptest::collection::vec(any::<u16>(), 0..=MAX_VISITED),
        )
            .prop_map(|(sender_home, dst_hop, msg_id, dst_pos, dst_leaf, hops, v)| Route {
                sender_home,
                dst_hop,
                msg_id,
                dst_pos,
                dst_leaf,
                hops,
                visited: Visited::from_hashes(&v).unwrap(),
            })
    }

    /// Encode, check the length against design §14, decode, compare.
    fn round_trip(p: &Packet<'_>) {
        let mut buf = [0; MAX_FRAME];
        let len = p.encode(&mut buf).unwrap();
        let expected = match p {
            Packet::Heartbeat(_) => 8,
            Packet::Ack(_) => 5,
            Packet::Attach(_) => 12,
            Packet::Data(d) => 25 + 2 * d.route.visited.len() + d.payload.len(),
            Packet::Deliver(d) => 9 + d.payload.len(),
        };
        assert_eq!(len, expected);
        assert_eq!(Packet::decode(&buf[..len]).as_ref(), Ok(p));
    }

    proptest! {
        #[test]
        fn heartbeat_round_trips(surveyed: bool, home in chan(), position in position()) {
            round_trip(&Packet::Heartbeat(Heartbeat { surveyed, home, position }));
        }

        #[test]
        fn ack_round_trips(msg_id in msg()) {
            round_trip(&Packet::Ack(Ack { msg_id }));
        }

        #[test]
        fn attach_round_trips(leaf in leaf(), relay in position(), mode in rx_mode()) {
            round_trip(&Packet::Attach(Attach { leaf, relay, mode }));
        }

        #[test]
        fn data_round_trips(route in route(), payload in proptest::collection::vec(any::<u8>(), 0..=MAX_PAYLOAD)) {
            round_trip(&Packet::Data(Data { route, payload: &payload }));
        }

        #[test]
        fn deliver_round_trips(dst_leaf in leaf(), msg_id in msg(), payload in proptest::collection::vec(any::<u8>(), 0..=MAX_PAYLOAD)) {
            round_trip(&Packet::Deliver(Deliver { dst_leaf, msg_id, payload: &payload }));
        }

        #[test]
        fn decode_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..=MAX_FRAME + 2)) {
            // Whatever decodes must re-encode to the same bytes, apart from
            // the bits the decoder ignores (aux of ACK and DELIVER, spare
            // position bits), so compare decode(encode(decode(b))) instead.
            if let Ok(p) = Packet::decode(&bytes) {
                let mut buf = [0; MAX_FRAME];
                let len = p.encode(&mut buf).unwrap();
                prop_assert_eq!(len, bytes.len());
                prop_assert_eq!(Packet::decode(&buf[..len]), Ok(p));
            }
        }

        #[test]
        fn corrupted_data_never_panics(route in route(), payload in proptest::collection::vec(any::<u8>(), 0..=MAX_PAYLOAD), at: usize, byte: u8, cut: usize) {
            // Random bytes rarely get past the type check; corrupt a valid
            // frame instead so the length and visited-list paths are hit.
            let mut buf = [0; MAX_FRAME];
            let len = Packet::Data(Data { route, payload: &payload }).encode(&mut buf).unwrap();
            buf[at % len] = byte;
            let _ = Packet::decode(&buf[..cut % (len + 1)]);
            let _ = Packet::decode(&buf[..len]);
        }
    }

    #[test]
    fn full_frame_fits() {
        let route = Route {
            sender_home: Channel::new(3).unwrap(),
            dst_hop: Position::default(),
            msg_id: MsgId(1),
            dst_pos: Position::default(),
            dst_leaf: LeafId::RELAY,
            hops: 0,
            visited: Visited::from_hashes(&[7; MAX_VISITED]).unwrap(),
        };
        let payload = [0xa5; MAX_PAYLOAD];
        let mut buf = [0; MAX_FRAME];
        assert_eq!(Packet::Data(Data { route, payload: &payload }).encode(&mut buf), Ok(MAX_FRAME));
        let long = [0; MAX_PAYLOAD + 1];
        assert_eq!(
            Packet::Data(Data { route, payload: &long }).encode(&mut buf),
            Err(EncodeError::PayloadTooLong)
        );
        let d = Deliver { dst_leaf: LeafId::RELAY, msg_id: MsgId(1), payload: &long };
        assert_eq!(Packet::Deliver(d).encode(&mut buf), Err(EncodeError::PayloadTooLong));
    }

    #[test]
    fn malformed_frames() {
        use DecodeError::*;
        assert_eq!(Packet::decode(&[]), Err(Short));
        assert_eq!(Packet::decode(&[0x00]), Err(BadType(0)));
        assert_eq!(Packet::decode(&[0x80, 0, 0, 0, 0]), Err(BadType(8)));
        assert_eq!(Packet::decode(&[0x1d, 0, 0, 0, 0, 0, 0, 0]), Err(ChannelOutOfRange(13)));
        assert_eq!(Packet::decode(&[0x10, 0, 0, 0, 0, 0, 0]), Err(Short));
        assert_eq!(Packet::decode(&[0x10, 0, 0, 0, 0, 0, 0, 0, 0]), Err(BadLength));
        assert_eq!(Packet::decode(&[0x43, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]), Err(BadRxMode(3)));
        assert_eq!(Packet::decode(&[0x30, 0, 0, 0]), Err(Short));
        assert_eq!(Packet::decode(&[0x60, 0, 0, 0, 0, 0, 0, 0]), Err(Short));
        assert_eq!(Packet::decode(&[0; MAX_FRAME + 1]), Err(BadLength));

        let mut data = [0u8; DATA_HEADER_LEN];
        data[FIRST] = first(T_DATA, 13);
        assert_eq!(Packet::decode(&data), Err(ChannelOutOfRange(13)));
        data[FIRST] = first(T_DATA, 0);
        data[DATA_VISITED_LEN] = MAX_VISITED as u8 + 1;
        assert_eq!(Packet::decode(&data), Err(VisitedTooLong(MAX_VISITED as u8 + 1)));
        data[DATA_VISITED_LEN] = 1;
        assert_eq!(Packet::decode(&data), Err(Short));
        assert_eq!(Packet::decode(&data[..DATA_HEADER_LEN - 1]), Err(Short));

        // Payload beyond MAX_PAYLOAD is rejected even though it fits the frame.
        let mut big = [0u8; DATA_HEADER_LEN + MAX_PAYLOAD + 1];
        big[FIRST] = first(T_DATA, 0);
        assert_eq!(Packet::decode(&big), Err(PayloadTooLong));
        let mut deliver = [0u8; DELIVER_HEADER_LEN + MAX_PAYLOAD + 1];
        deliver[FIRST] = first(T_DELIVER, 0);
        assert_eq!(Packet::decode(&deliver), Err(PayloadTooLong));

        // Type 0x7 is unassigned (D10).
        assert_eq!(Packet::decode(&[0x70; DATA_HEADER_LEN + 5]), Err(BadType(7)));
    }

    #[test]
    fn heartbeat_bytes() {
        // Pin the exact layout: type 0x2, channel 12, position bytes.
        let position = Position::from_degrees(50.45, 30.52);
        let hb = Heartbeat { surveyed: false, home: Channel::new(12).unwrap(), position };
        let mut buf = [0; MAX_FRAME];
        let len = Packet::Heartbeat(hb).encode(&mut buf).unwrap();
        assert_eq!(buf[0], 0x2c);
        assert_eq!(&buf[1..len], &position.encode());
    }
}
