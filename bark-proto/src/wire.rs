//! Encoding and decoding of every packet in spec "Packet formats".
//!
//! The first byte of every packet is its kind and that kind's one small
//! field (roadmap D76, D77):
//!
//! ```text
//! 1 0 0 0 c c c c   HEARTBEAT: home channel cccc
//! 0 0 a a a a a a   DATA: aux, the sender's home channel
//! 0 1 a a a a a a   ACK: aux, the wait
//! ```
//!
//! The second is the format version (roadmap D77): in a heartbeat the
//! newest format the sender speaks, in DATA and the ACK the format of the
//! frame. This code speaks format [`FORMAT`] only, so it sends 1 and
//! rejects a DATA or ACK of any other; the heartbeat's layout never
//! changes, so it reads every heartbeat and keeps the sender's format.
//! A heartbeat's third byte holds its flags; a receiver ignores the bits
//! it does not know, which later formats may define.
//!
//! Every node has a 6-byte [`NodeId`], and fields that only name a node
//! (DATA `dst_hop` and `src_hop`, ACK `from` and `to`) carry it. DATA
//! `dst_pos` locates a place and is the 7-byte form of
//! [`Position::encode`]; the heartbeat carries the sender's position in the
//! 4-byte short form of [`ShortName`], which the receiver rebuilds from its
//! own (spec "Positions on the wire", roadmap D14). DATA carries no message
//! ID: every node computes [`Data::msg_id`] (D15). Search state lives in
//! the nodes, not the packet (D16). Lengths, in bytes:
//!
//! ```text
//! HEARTBEAT  first format flags id pos                            = 13
//! ACK        first format msg from to                             = 18
//! DATA       first format dst_hop src_hop dst_pos dst_id hops
//!            time_left payload                                    = 29 + payload
//! ```
//!
//! An ACK answers every offer, taken or refused: its `aux` says which. A
//! refusal is for this hop only (spec "Receiving DATA"); the network still
//! sends no failure notices (D10). An ACK names the message, the node
//! answering and the node answered, so that a sender takes only the answer
//! to its own offer, however many others offer the same message to the
//! same node (D32).
//!
//! Choices the design leaves open (O2), fixed here and listed in
//! `docs/core-interface.md` §7:
//!
//! - Short positions and message IDs are big-endian; a node ID is six
//!   opaque bytes in order.
//! - The payload is at most [`MAX_PAYLOAD`] = 226 bytes, what a frame holds
//!   after the DATA header.
//! - ACK `to` names the sender of what is answered, the DATA's `src_hop`.
//! - ACK `aux` is 0 when the offer is taken and, when it is refused, the
//!   wait the refuser asks for in whole bans, 1 to 63 (`tables/budget.rs`).
//!   The wait needs no byte of its own, so every ACK has one length and
//!   one airtime.
//! - DATA `aux` is the sender's home channel, 0 to 12; its two high bits
//!   are zero, and a DATA with either set is rejected.
//! - The heartbeat's flags are bit 0 sleepy, bit 1 anchorless, bit 2
//!   carried and bit 3 map point, each with or without the others.
//! - A heartbeat byte whose channel is 15, which no node sends, starts the
//!   firmware's bench frames ([`BENCH_FIRST`]); nodes reject it as a
//!   channel outside the plan.
//! - DATA `time_left` is the time the message has left, in whole
//!   [`TTL_UNIT`]s, rounded down.
//! - An ACK with trailing bytes is malformed, not padded. A heartbeat is
//!   read from its first [`HEARTBEAT_LEN`] bytes, and what follows is
//!   ignored, so that a later format may append fields.

use crate::event::MAX_FRAME;
use crate::ids::{MsgId, NodeId};
use crate::position::{Position, ShortName};
use crate::radio::{time_on_air, Channel, MIN_PREAMBLE_SYMBOLS};
use crate::time::Duration;

/// The wire format this code speaks, and so sends and reads (spec
/// "Packet formats", roadmap D77).
pub const FORMAT: u8 = 1;

// Field widths.
const FIRST_LEN: usize = 1;
const FORMAT_LEN: usize = 1;
const FLAGS_LEN: usize = 1;
const POS_LEN: usize = Position::WIRE_LEN;
const SHORT_LEN: usize = 4;
const MSG_ID_LEN: usize = 4;
const ID_LEN: usize = NodeId::LEN;
const HOPS_LEN: usize = 1;
const TIME_LEFT_LEN: usize = 1;

// First byte.
const FIRST: usize = 0;
const HEARTBEAT_BIT: u8 = 0x80;
const ANSWER_BIT: u8 = 0x40;
/// The bits of `aux`, below the kind bits (spec "Packet formats"): the
/// longest wait an ACK asks for, in bans.
pub const AUX_MASK: u8 = 0x3f;
// A heartbeat's channel, below three bits that are zero.
const CHANNEL_MASK: u8 = 0x0f;
const HEARTBEAT_ZERO: u8 = 0x70;
const _: () = assert!(
    HEARTBEAT_BIT | HEARTBEAT_ZERO | CHANNEL_MASK == 0xff
        && ANSWER_BIT | AUX_MASK == !HEARTBEAT_BIT,
    "spec \"Packet formats\": each bit of the first byte has one meaning"
);
// The channel field of a bench frame's first byte: no node's home.
const BENCH_CHANNEL: u8 = CHANNEL_MASK;
const _: () = assert!(Channel::new(BENCH_CHANNEL).is_none());

/// The first byte of a firmware bench frame: a heartbeat byte on channel
/// 15, which no node sends and every node rejects (spec "Packet formats").
pub const BENCH_FIRST: u8 = HEARTBEAT_BIT | BENCH_CHANNEL;

// The second byte, the format version.
const VERSION: usize = FIRST + FIRST_LEN;

// HEARTBEAT.
const HEARTBEAT_FLAGS: usize = VERSION + FORMAT_LEN;
const HEARTBEAT_ID: usize = HEARTBEAT_FLAGS + FLAGS_LEN;
// The bits of the flags byte that this format defines; the others are
// sent zero and ignored.
const SLEEPY: u8 = 0x01;
const ANCHORLESS: u8 = 0x02;
const CARRIED: u8 = 0x04;
const MAP_POINT: u8 = 0x08;
const HEARTBEAT_POS: usize = HEARTBEAT_ID + ID_LEN;
/// HEARTBEAT length.
pub const HEARTBEAT_LEN: usize = HEARTBEAT_POS + SHORT_LEN;

// ACK.
const ACK_ID: usize = VERSION + FORMAT_LEN;
const ACK_FROM: usize = ACK_ID + MSG_ID_LEN;
const ACK_TO: usize = ACK_FROM + ID_LEN;
/// ACK length (spec "Packet formats": 18).
pub const ACK_LEN: usize = ACK_TO + ID_LEN;

// DATA.
const DATA_DST_HOP: usize = VERSION + FORMAT_LEN;
const DATA_SRC_HOP: usize = DATA_DST_HOP + ID_LEN;
const DATA_DST_POS: usize = DATA_SRC_HOP + ID_LEN;
const DATA_DST_ID: usize = DATA_DST_POS + POS_LEN;
const DATA_HOPS: usize = DATA_DST_ID + ID_LEN;
const DATA_TIME_LEFT: usize = DATA_HOPS + HOPS_LEN;
/// DATA header length (spec "Packet formats": 29).
pub const DATA_HEADER_LEN: usize = DATA_TIME_LEFT + TIME_LEFT_LEN;

/// Largest application payload: [`MAX_FRAME`] less the DATA header,
/// 255 − 29 = 226 bytes.
pub const MAX_PAYLOAD: usize = MAX_FRAME - DATA_HEADER_LEN;

const _: () = assert!(HEARTBEAT_LEN == 13 && ACK_LEN == 18);
const _: () = assert!(DATA_HEADER_LEN == 29);
const _: () = assert!(MAX_PAYLOAD == 226);

/// Whether a frame of `len` bytes takes as long on air as one `less`
/// bytes shorter.
const fn free(len: usize, less: usize) -> bool {
    time_on_air(len as u8, MIN_PREAMBLE_SYMBOLS).as_micros()
        == time_on_air((len - less) as u8, MIN_PREAMBLE_SYMBOLS).as_micros()
}
const _: () = assert!(
    free(HEARTBEAT_LEN, FORMAT_LEN + FLAGS_LEN) && free(ACK_LEN, FORMAT_LEN),
    "spec \"Packet formats\": the version and flags bytes cost no airtime"
);
const _: () = assert!(
    !free(HEARTBEAT_LEN + 1, 1),
    "spec \"Packet formats\": a byte appended to the heartbeat costs a block"
);

/// HEARTBEAT (spec "Heartbeat").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Heartbeat {
    /// Sleepy (0x1): reach the sender cheaply in its window after its
    /// beat, or at any time with the long preamble; it holds mail for
    /// nobody. Clear, it listens all the time.
    pub sleepy: bool,
    /// Anchorless (0x2): the sender knows no node
    /// that holds mail, one neither sleepy nor carried; such a node that
    /// hears it answers.
    pub anchorless: bool,
    /// Carried (0x4): the sender moves. It takes
    /// each GPS fix and holds mail for nobody, so nobody anchors to it.
    pub carried: bool,
    /// The sender's home channel.
    pub home: Channel,
    /// The newest wire format the sender speaks: its capability.
    pub format: u8,
    /// The sender's position is a map point a person set in the app, not
    /// a GPS fix (spec "Position").
    pub map_point: bool,
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

/// ACK (spec "The ACK"): the answer to an offer, taken or refused.
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
    /// For a refusal, the wait the refuser asks for in whole bans, 1 to
    /// [`AUX_MASK`] (spec "Bans"); 0 in an ACK that takes.
    pub wait: u8,
}

impl Ack {
    /// Whether this answers the same offer as `other`: the same message
    /// between the same two nodes, taken or refused (roadmap D32).
    pub fn answers(&self, other: &Ack) -> bool {
        self.msg_id == other.msg_id && self.from == other.from && self.to == other.to
    }
}

/// DATA (spec "Forwarding", "Packet formats").
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
    /// The time the message has left, in [`TTL_UNIT`]s (spec "Time to live").
    pub time_left: u8,
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
    /// Fewer bytes than the packet's fields need.
    Short,
    /// Longer than [`MAX_FRAME`], or trailing bytes after an ACK.
    BadLength,
    /// A channel field outside the plan.
    ChannelOutOfRange(u8),
    /// A payload longer than [`MAX_PAYLOAD`].
    PayloadTooLong,
    /// A first byte of no kind: a heartbeat's with a bit between its kind
    /// and its channel set.
    Kind(u8),
    /// A DATA or ACK in a format this code does not speak.
    Format(u8),
}

/// Why a packet did not encode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncodeError {
    /// The payload is longer than [`MAX_PAYLOAD`].
    PayloadTooLong,
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

fn get_id(b: &[u8], at: usize) -> NodeId {
    let mut a = [0; ID_LEN];
    a.copy_from_slice(&b[at..at + ID_LEN]);
    NodeId(a)
}

fn get_msg_id(b: &[u8], at: usize) -> MsgId {
    MsgId(u32::from_be_bytes(get4(b, at)))
}

/// The format of a DATA or ACK, if this code speaks it.
fn format(b: &[u8]) -> Result<(), DecodeError> {
    match b[VERSION] {
        FORMAT => Ok(()),
        other => Err(DecodeError::Format(other)),
    }
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
                let flag = |set: bool, bit: u8| if set { bit } else { 0 };
                buf[FIRST] = HEARTBEAT_BIT | h.home.index();
                buf[VERSION] = h.format;
                buf[HEARTBEAT_FLAGS] = flag(h.sleepy, SLEEPY)
                    | flag(h.anchorless, ANCHORLESS)
                    | flag(h.carried, CARRIED)
                    | flag(h.map_point, MAP_POINT);
                put(buf, HEARTBEAT_ID, &h.id.0);
                put(buf, HEARTBEAT_POS, &h.short.0.to_be_bytes());
                Ok(HEARTBEAT_LEN)
            }
            Packet::Ack(a) => {
                // A refusal is never told as 0, which means taken.
                let wait = if a.refused {
                    a.wait.clamp(1, AUX_MASK)
                } else {
                    0
                };
                buf[FIRST] = ANSWER_BIT | wait;
                buf[VERSION] = FORMAT;
                put(buf, ACK_ID, &a.msg_id.0.to_be_bytes());
                put(buf, ACK_FROM, &a.from.0);
                put(buf, ACK_TO, &a.to.0);
                Ok(ACK_LEN)
            }
            Packet::Data(d) => {
                if d.payload.len() > MAX_PAYLOAD {
                    return Err(EncodeError::PayloadTooLong);
                }
                buf[FIRST] = d.sender_home.index();
                buf[VERSION] = FORMAT;
                put(buf, DATA_DST_HOP, &d.dst_hop.0);
                put(buf, DATA_SRC_HOP, &d.src_hop.0);
                put(buf, DATA_DST_POS, &d.dst_pos.encode());
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
        if first & HEARTBEAT_BIT != 0 {
            if first & HEARTBEAT_ZERO != 0 {
                return Err(DecodeError::Kind(first));
            }
            if b.len() < HEARTBEAT_LEN {
                return Err(DecodeError::Short);
            }
            let flags = b[HEARTBEAT_FLAGS];
            return Ok(Packet::Heartbeat(Heartbeat {
                sleepy: flags & SLEEPY != 0,
                anchorless: flags & ANCHORLESS != 0,
                carried: flags & CARRIED != 0,
                home: channel(first & CHANNEL_MASK)?,
                format: b[VERSION],
                map_point: flags & MAP_POINT != 0,
                id: get_id(b, HEARTBEAT_ID),
                short: get_short(b, HEARTBEAT_POS),
            }));
        }
        let aux = first & AUX_MASK;
        if first & ANSWER_BIT != 0 {
            exact(b, ACK_LEN)?;
            format(b)?;
            return Ok(Packet::Ack(Ack {
                msg_id: get_msg_id(b, ACK_ID),
                from: get_id(b, ACK_FROM),
                to: get_id(b, ACK_TO),
                refused: aux != 0,
                wait: aux,
            }));
        }
        let payload = payload(b, DATA_HEADER_LEN)?;
        format(b)?;
        Ok(Packet::Data(Data {
            sender_home: channel(aux)?,
            dst_hop: get_id(b, DATA_DST_HOP),
            src_hop: get_id(b, DATA_SRC_HOP),
            dst_pos: get_pos(b, DATA_DST_POS),
            dst_id: get_id(b, DATA_DST_ID),
            hops: b[DATA_HOPS],
            time_left: b[DATA_TIME_LEFT],
            payload,
        }))
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
        any::<[u8; ID_LEN]>().prop_map(NodeId)
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
            Packet::Heartbeat(_) => 13,
            Packet::Ack(_) => 18,
            Packet::Data(d) => 29 + d.payload.len(),
        };
        assert_eq!(len, expected);
        assert_eq!(Packet::decode(&buf[..len]).as_ref(), Ok(p));
    }

    proptest! {
        #[test]
        fn heartbeat_round_trips(
            sleepy: bool,
            anchorless: bool,
            carried: bool,
            home in chan(),
            format: u8,
            map_point: bool,
            id in id(),
            position in position(),
        ) {
            // Every heartbeat is read, whatever format its sender speaks.
            let hb = Heartbeat { sleepy, anchorless, carried, home, format, map_point, id, short: position.short() };
            round_trip(&Packet::Heartbeat(hb));
            prop_assert_eq!(hb.position(position), position);
        }

        #[test]
        fn ack_round_trips(
            msg_id in msg(),
            from in id(),
            to in id(),
            refused: bool,
            step in 1..=AUX_MASK,
        ) {
            let wait = if refused { step } else { 0 };
            let ack = Ack { msg_id, from, to, refused, wait };
            round_trip(&Packet::Ack(ack));
            let flipped = Ack { refused: !refused, ..ack };
            prop_assert!(ack.answers(&flipped));
            let mut flipped_to = to;
            flipped_to.0[0] = !flipped_to.0[0];
            let other = Ack { to: flipped_to, ..ack };
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
            payload in payload(),
        ) {
            let d = Data { sender_home, dst_hop, src_hop, dst_pos, dst_id, hops, time_left, payload: &payload };
            round_trip(&Packet::Data(d));
            // The ID survives the trip: it depends only on fields every hop keeps.
            let mut buf = [0; MAX_FRAME];
            let len = Packet::Data(d).encode(&mut buf).unwrap();
            let Ok(Packet::Data(back)) = Packet::decode(&buf[..len]) else { panic!() };
            prop_assert_eq!(back.msg_id(), MsgId::of(dst_pos, dst_id, &payload));
        }

        #[test]
        fn decode_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..=MAX_FRAME + 2)) {
            // Whatever decodes must re-encode to the same length and decode
            // the same, apart from bits the decoder ignores (spare position
            // bits).
            if let Ok(p) = Packet::decode(&bytes) {
                let mut buf = [0; MAX_FRAME];
                let len = p.encode(&mut buf).unwrap();
                if !matches!(p, Packet::Heartbeat(_)) {
                    prop_assert_eq!(len, bytes.len());
                }
                prop_assert_eq!(Packet::decode(&buf[..len]), Ok(p));
            }
        }

        #[test]
        fn every_first_byte_never_panics(first: u8, rest in proptest::collection::vec(any::<u8>(), 0..=MAX_FRAME)) {
            // Cut at every length a decoder checks, so that each length
            // path runs whatever the first byte.
            let mut bytes = std::vec![first];
            bytes.extend_from_slice(&rest);
            let _ = Packet::decode(&bytes);
            for cut in [0, 1, 2, 3, 7, 8, 12, 13, 14, 17, 18, 19, 28, 29, 30] {
                let _ = Packet::decode(&bytes[..cut.min(bytes.len())]);
            }
        }
    }

    #[test]
    fn full_frame_fits() {
        let payload = [0xa5; MAX_PAYLOAD];
        let d = Data {
            sender_home: Channel::new(3).unwrap(),
            dst_hop: NodeId([1; ID_LEN]),
            src_hop: NodeId([2; ID_LEN]),
            dst_pos: Position::default(),
            dst_id: NodeId([3; ID_LEN]),
            hops: 0,
            time_left: 0,
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
        assert_eq!(Packet::decode(&[0x80, 0, 0, 0, 0]), Err(Short));
        let mut hb = [0u8; HEARTBEAT_LEN + 1];
        hb[FIRST] = 0x8d;
        assert_eq!(
            Packet::decode(&hb[..HEARTBEAT_LEN]),
            Err(ChannelOutOfRange(13))
        );
        for first in [0x90, 0xa3, 0xcc, 0xff] {
            hb[FIRST] = first;
            assert_eq!(Packet::decode(&hb[..HEARTBEAT_LEN]), Err(Kind(first)));
        }
        hb[FIRST] = 0x80;
        assert_eq!(Packet::decode(&hb[..HEARTBEAT_LEN - 1]), Err(Short));
        let mut long = [0xa5u8; HEARTBEAT_LEN + 7];
        long[..HEARTBEAT_LEN].copy_from_slice(&hb[..HEARTBEAT_LEN]);
        for len in [HEARTBEAT_LEN + 1, HEARTBEAT_LEN + 7] {
            assert_eq!(
                Packet::decode(&long[..len]),
                Packet::decode(&hb[..HEARTBEAT_LEN]),
                "what follows a heartbeat's first 13 bytes is ignored"
            );
        }
        assert!(Packet::decode(&hb[..HEARTBEAT_LEN]).is_ok());
        for first in [0x40, 0x41, 0x7f] {
            assert_eq!(Packet::decode(&[first; ACK_LEN - 1]), Err(Short));
            assert_eq!(Packet::decode(&[first; ACK_LEN + 1]), Err(BadLength));
        }
        assert_eq!(Packet::decode(&[0; MAX_FRAME + 1]), Err(BadLength));

        let mut data = [0u8; DATA_HEADER_LEN];
        data[VERSION] = FORMAT;
        for (aux, why) in [
            (13, "outside the plan"),
            (0x10, "a high bit"),
            (0x23, "a high bit"),
        ] {
            data[FIRST] = aux;
            assert_eq!(Packet::decode(&data), Err(ChannelOutOfRange(aux)), "{why}");
        }
        data[FIRST] = 0;
        assert!(
            Packet::decode(&data).is_ok(),
            "an empty payload is a payload"
        );
        assert_eq!(Packet::decode(&data[..DATA_HEADER_LEN - 1]), Err(Short));
    }

    #[test]
    fn formats() {
        // A DATA or ACK in a format this code does not speak is rejected;
        // a heartbeat is read whatever its sender speaks.
        let mut data = [0u8; DATA_HEADER_LEN + 1];
        let mut ack = [ANSWER_BIT; ACK_LEN];
        let mut hb = [HEARTBEAT_BIT; HEARTBEAT_LEN];
        for f in [0, 2, 0xff] {
            data[VERSION] = f;
            ack[VERSION] = f;
            hb[VERSION] = f;
            assert_eq!(Packet::decode(&data), Err(DecodeError::Format(f)));
            assert_eq!(Packet::decode(&ack), Err(DecodeError::Format(f)));
            let Ok(Packet::Heartbeat(h)) = Packet::decode(&hb) else {
                panic!("a heartbeat of format {f}")
            };
            assert_eq!(h.format, f);
        }
    }

    #[test]
    fn unknown_heartbeat_flags_are_ignored() {
        let mut hb = [0u8; HEARTBEAT_LEN];
        hb[FIRST] = HEARTBEAT_BIT;
        hb[VERSION] = FORMAT;
        for flags in [0x00, 0x08, 0xf0, 0xf8] {
            hb[HEARTBEAT_FLAGS] = flags;
            let Ok(Packet::Heartbeat(h)) = Packet::decode(&hb) else {
                panic!("{flags:#x}")
            };
            assert_eq!(h.map_point, flags & MAP_POINT != 0, "{flags:#x}");
            assert!(!h.sleepy && !h.anchorless && !h.carried, "{flags:#x}");
        }
    }

    /// The version byte's cost in DATA (spec "Hand-over"): a block of
    /// symbols for 91 of the 227 payload lengths, those whose frame is 1
    /// or 4 bytes mod 5, 0.51 ms on average; and one block for the
    /// 100-byte text of spec "Capacity and sizing", 1.7 % of its frame.
    #[test]
    fn the_version_byte_in_data() {
        let air = |len: usize| time_on_air(len as u8, MIN_PREAMBLE_SYMBOLS).as_micros();
        let block = air(16) - air(15);
        assert_eq!(block, 1280, "5 symbols of 256 µs");
        let grows: std::vec::Vec<usize> = (0..=MAX_PAYLOAD)
            .filter(|p| air(DATA_HEADER_LEN + p) > air(DATA_HEADER_LEN - FORMAT_LEN + p))
            .collect();
        assert_eq!(grows.len(), 91);
        assert!(grows
            .iter()
            .all(|p| matches!((DATA_HEADER_LEN + p) % 5, 1 | 4)));
        let mean_us = grows.len() as u64 * block / (MAX_PAYLOAD as u64 + 1);
        assert_eq!(mean_us, 513);
        let text = DATA_HEADER_LEN + 100;
        assert_eq!(air(text) - air(text - FORMAT_LEN), block);
        assert_eq!(air(text), 74_560);
        assert_eq!(block * 1000 / (air(text) - block), 17, "1.7 % of the frame");
    }

    #[test]
    fn bench_frames_are_rejected() {
        let mut frame = [0u8; HEARTBEAT_LEN];
        frame[FIRST] = BENCH_FIRST;
        for kind in 0..=u8::MAX {
            frame[HEARTBEAT_FLAGS] = kind;
            assert_eq!(
                Packet::decode(&frame),
                Err(DecodeError::ChannelOutOfRange(BENCH_CHANNEL))
            );
        }
    }

    #[test]
    fn exact_bytes() {
        // Pin the layouts: field order, big-endian positions and IDs.
        let mut buf = [0; MAX_FRAME];
        let position = Position::from_degrees(50.45, 30.52);
        let hb = Heartbeat {
            sleepy: false,
            anchorless: false,
            carried: false,
            home: Channel::new(12).unwrap(),
            format: FORMAT,
            map_point: false,
            id: NodeId([1, 2, 3, 4, 5, 6]),
            short: ShortName(0x0708_090a),
        };
        let len = Packet::Heartbeat(hb).encode(&mut buf).unwrap();
        assert_eq!(&buf[..len], &[0x8c, 1, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        for (bit, set) in [
            (0x01, Heartbeat { sleepy: true, ..hb }),
            (
                0x02,
                Heartbeat {
                    anchorless: true,
                    ..hb
                },
            ),
            (
                0x04,
                Heartbeat {
                    carried: true,
                    ..hb
                },
            ),
            (
                0x08,
                Heartbeat {
                    map_point: true,
                    ..hb
                },
            ),
        ] {
            Packet::Heartbeat(set).encode(&mut buf).unwrap();
            assert_eq!(buf[..2], [0x8c, 1], "{bit:#x}: only the flags byte");
            assert_eq!(buf[2], bit);
        }
        assert_eq!(
            time_on_air(len as u8, MIN_PREAMBLE_SYMBOLS),
            Duration::from_micros(14_400),
            "56.25 symbols at SF5"
        );

        let ack = Ack {
            msg_id: MsgId(0x0102_0304),
            from: NodeId([5, 6, 7, 8, 9, 10]),
            to: NodeId([11, 12, 13, 14, 15, 16]),
            refused: false,
            wait: 0,
        };
        let len = Packet::Ack(ack).encode(&mut buf).unwrap();
        assert_eq!(
            &buf[..len],
            &[0x40, 1, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
        );
        assert_eq!(
            time_on_air(len as u8, MIN_PREAMBLE_SYMBOLS),
            Duration::from_micros(16_960),
            "66.25 symbols at SF5"
        );
        let refused = Ack {
            refused: true,
            wait: 24,
            ..ack
        };
        let len = Packet::Ack(refused).encode(&mut buf).unwrap();
        assert_eq!(
            &buf[..len],
            &[0x58, 1, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
            "the wait in aux"
        );
        let unsaid = Ack {
            refused: true,
            wait: 0,
            ..ack
        };
        Packet::Ack(unsaid).encode(&mut buf).unwrap();
        assert_eq!(buf[0], 0x41, "a refusal is never told as taken");

        let d = Data {
            sender_home: Channel::new(9).unwrap(),
            dst_hop: NodeId([1, 2, 3, 4, 5, 6]),
            src_hop: NodeId([7, 8, 9, 10, 11, 12]),
            dst_pos: position,
            dst_id: NodeId([13, 14, 15, 16, 17, 18]),
            hops: 19,
            time_left: 14,
            payload: &[21, 22],
        };
        let len = Packet::Data(d).encode(&mut buf).unwrap();
        let mut want = std::vec![0x09, 1, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
        want.extend_from_slice(&position.encode());
        want.extend_from_slice(&[13, 14, 15, 16, 17, 18, 19, 14, 21, 22]);
        assert_eq!(&buf[..len], &want[..]);
        let header = Data { payload: &[], ..d };
        let len = Packet::Data(header).encode(&mut buf).unwrap();
        assert_eq!(
            time_on_air(len as u8, MIN_PREAMBLE_SYMBOLS),
            Duration::from_micros(23_360),
            "91.25 symbols at SF5"
        );
        assert_eq!(d.time_left(), Duration::from_secs(35));
        assert_eq!(Data::units(Duration::from_millis(600_000)), 240);
        assert_eq!(Data::units(Duration::from_millis(2_499)), 0, "rounded down");
        assert_eq!(Data::units(Duration::from_secs(1000)), u8::MAX);
    }
}
