//! Encoding and decoding of every packet in spec "Packet formats".
//!
//! The first byte of every packet is `type:4 | aux:4`. Fields that locate
//! a place (heartbeat `pos`, DATA `dst_pos`) are the 7-byte form of
//! [`Position::encode`]; fields that only name a relay (ATTACH `relay`, DATA
//! `dst_hop` and `src_hop`) are its 4-byte [`ShortName`] (roadmap D14).
//! DATA carries no message ID: every node computes [`Data::msg_id`] (D15).
//! Search state lives in the relays, not the packet (D16). Lengths, in
//! bytes:
//!
//! ```text
//! HEARTBEAT 0x1/0x2  first pos                                      = 8
//! ACK       0x3      first id from to                               = 13
//! ATTACH    0x4      first leaf_id relay                            = 9
//! DATA      0x5      first dst_hop src_hop dst_pos dst_leaf hops
//!                    time_left payload                              = 22 + payload
//! DELIVER   0x6      first dst_leaf id payload                      = 9 + payload
//! ATTACHED  0x7      first leaf_id next_poll                        = 7
//! ```
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
//! - Short names and message IDs are big-endian; a leaf ID is four opaque
//!   bytes in order. ACK `from` and `to` are either, so they are four
//!   bytes compared whole.
//! - The payload is at most [`MAX_PAYLOAD`] = 233 bytes, what a frame holds
//!   after the DATA header. DELIVER only carries a DATA payload onward, so
//!   it has the same cap although its header is shorter.
//! - ACK `to` names the sender of what is answered: a DATA's `src_hop`,
//!   zero from a leaf, whose ID DATA does not carry; for a DELIVER, the
//!   relay the leaf is attached to. An ATTACH is answered by ATTACHED, not
//!   by an ACK (roadmap D39).
//! - ACK `aux` bit 0 is set when the offer is refused. The other `aux`
//!   bits of an ACK, and all of a DELIVER's, are sent as zero and ignored
//!   on receipt, as are the spare position bits.
//! - ATTACH `aux` is the [`RxMode`] code in its low three bits, and
//!   [`ATTACH_REPLY`] when the ATTACH answers a poll.
//! - ATTACHED `next_poll` is big-endian, in [`POLL_UNIT`]s: the delay from
//!   the end of the frame to the leaf's next slot. Its `aux` is the count
//!   of messages waiting for the leaf, saturating at [`WAITING_MAX`].
//! - DATA `time_left` is the time the message has left, in whole
//!   [`TTL_UNIT`]s, rounded down.
//! - A fixed-size packet with trailing bytes is malformed, not padded.

use crate::event::MAX_FRAME;
use crate::ids::{LeafId, MsgId};
use crate::position::{Position, ShortName};
use crate::radio::{Channel, MIN_PREAMBLE_SYMBOLS, SYMBOL_TIME};
use crate::time::Duration;

// Field widths.
const FIRST_LEN: usize = 1;
const POS_LEN: usize = Position::WIRE_LEN;
const NAME_LEN: usize = 4;
const MSG_ID_LEN: usize = 4;
const LEAF_ID_LEN: usize = 4;
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
const T_ATTACH: u8 = 0x4;
const T_DATA: u8 = 0x5;
const T_DELIVER: u8 = 0x6;
const T_ATTACHED: u8 = 0x7;

// HEARTBEAT.
const HEARTBEAT_POS: usize = FIRST + FIRST_LEN;
/// HEARTBEAT length (spec "Packet formats": 8).
pub const HEARTBEAT_LEN: usize = HEARTBEAT_POS + POS_LEN;

// ACK.
const ACK_ID: usize = FIRST + FIRST_LEN;
const ACK_FROM: usize = ACK_ID + MSG_ID_LEN;
const ACK_TO: usize = ACK_FROM + NAME_LEN;
/// ACK length (spec "Packet formats": 13).
pub const ACK_LEN: usize = ACK_TO + NAME_LEN;
/// The `aux` bit of an ACK that refuses.
const ACK_REFUSED: u8 = 0x1;

// ATTACH.
const ATTACH_LEAF: usize = FIRST + FIRST_LEN;
const ATTACH_RELAY: usize = ATTACH_LEAF + LEAF_ID_LEN;
/// ATTACH length (spec "Packet formats": 9).
pub const ATTACH_LEN: usize = ATTACH_RELAY + NAME_LEN;
/// The `aux` bit of an ATTACH that answers a poll (spec "Attach and polls").
const ATTACH_REPLY: u8 = 0x8;
/// The `aux` bits of an ATTACH that hold the receive mode.
const ATTACH_MODE: u8 = 0x7;

// ATTACHED.
const ATTACHED_LEAF: usize = FIRST + FIRST_LEN;
const ATTACHED_NEXT: usize = ATTACHED_LEAF + LEAF_ID_LEN;
const NEXT_POLL_LEN: usize = 2;
/// ATTACHED length (spec "Packet formats": 7).
pub const ATTACHED_LEN: usize = ATTACHED_NEXT + NEXT_POLL_LEN;

// DATA.
const DATA_DST_HOP: usize = FIRST + FIRST_LEN;
const DATA_SRC_HOP: usize = DATA_DST_HOP + NAME_LEN;
const DATA_DST_POS: usize = DATA_SRC_HOP + NAME_LEN;
const DATA_DST_LEAF: usize = DATA_DST_POS + POS_LEN;
const DATA_HOPS: usize = DATA_DST_LEAF + LEAF_ID_LEN;
const DATA_TIME_LEFT: usize = DATA_HOPS + HOPS_LEN;
/// DATA header length (spec "Packet formats": 22).
pub const DATA_HEADER_LEN: usize = DATA_TIME_LEFT + TIME_LEFT_LEN;

// DELIVER.
const DELIVER_LEAF: usize = FIRST + FIRST_LEN;
const DELIVER_ID: usize = DELIVER_LEAF + LEAF_ID_LEN;
/// DELIVER header length before the payload (spec "Packet formats": 9).
pub const DELIVER_HEADER_LEN: usize = DELIVER_ID + MSG_ID_LEN;

/// Largest application payload: [`MAX_FRAME`] less the DATA header,
/// 255 − 22 = 233 bytes. DATA and DELIVER carry at most this.
pub const MAX_PAYLOAD: usize = MAX_FRAME - DATA_HEADER_LEN;

const _: () = assert!(HEARTBEAT_LEN == 8 && ACK_LEN == 13 && ATTACH_LEN == 9);
const _: () = assert!(ATTACHED_LEN == 7);
const _: () = assert!(DATA_HEADER_LEN == 22 && DELIVER_HEADER_LEN == 9);
const _: () = assert!(MAX_PAYLOAD == 233);

/// A leaf's receive mode, carried in ATTACH `aux` (spec "Leaf receive mode"). Sets the
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

    /// Preamble of packets to a leaf in this mode (spec "Leaf receive mode"): the
    /// minimum for a leaf that listens, and at least 512 ms, 2000 symbols,
    /// for one that wakes for a CAD every 500 ms, so that every wake finds
    /// a preamble on the air.
    pub const fn preamble_symbols(self) -> u16 {
        match self {
            RxMode::Listening => MIN_PREAMBLE_SYMBOLS,
            RxMode::DutyCycled | RxMode::Hybrid => SLEEPING_PREAMBLE
                .as_micros()
                .div_ceil(SYMBOL_TIME.as_micros())
                as u16,
        }
    }
}

/// Shortest preamble for a leaf that sleeps between CADs (spec "Leaf receive mode").
pub const SLEEPING_PREAMBLE: Duration = Duration::from_millis(512);

const _: () = assert!(RxMode::Hybrid.preamble_symbols() == 2000);

/// HEARTBEAT, types 0x1 and 0x2 (spec "Heartbeat").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Heartbeat {
    /// Surveyed (0x1) or GPS-derived (0x2) position.
    pub surveyed: bool,
    /// The sender's home channel.
    pub home: Channel,
    /// The sender's position, which is its identity.
    pub position: Position,
}

/// A party to an ACK: a relay's short name or a leaf's ID, in the 4 bytes
/// of ACK `from` or `to` (roadmap D19, D32). The receiver knows which it
/// expects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AckName(pub [u8; 4]);

impl From<ShortName> for AckName {
    fn from(relay: ShortName) -> AckName {
        AckName(relay.0.to_be_bytes())
    }
}

impl From<LeafId> for AckName {
    fn from(leaf: LeafId) -> AckName {
        AckName(leaf.0)
    }
}

/// ACK, type 0x3 (spec "The ACK"): the answer to an offer, taken or refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ack {
    /// The answered message; for an ATTACH, the leaf's ID read as a
    /// big-endian number.
    pub msg_id: MsgId,
    /// The answering relay or leaf.
    pub from: AckName,
    /// The answered relay or leaf.
    pub to: AckName,
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

/// ATTACH, type 0x4 (spec "Attach and polls").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attach {
    /// The attaching leaf.
    pub leaf: LeafId,
    /// The relay attached to.
    pub relay: ShortName,
    /// The leaf's receive mode.
    pub mode: RxMode,
    /// It answers the relay's poll; otherwise it asks to attach.
    pub reply: bool,
}

impl Attach {
    /// The ATTACH that answers a poll, rather than one that attaches.
    pub const fn reply(self) -> Attach {
        Attach {
            reply: true,
            ..self
        }
    }
}

/// The most messages waiting an ATTACHED can count: all four bits of its
/// `aux` (spec "Leaf receive mode"). The byte a count up to 255 would take makes the
/// frame 46.25 symbols instead of 41.25, 11.84 ms instead of 10.56, and
/// that 12 % more poll airtime alone tipped the capacity domain at twice
/// the spec "Capacity and sizing" rate into collapse (roadmap D41).
pub const WAITING_MAX: u8 = AUX_MASK;

/// Unit of ATTACHED's time to the next poll (spec "Attach and polls"). Sixteen bits
/// of it reach 655 s, past the delivery TTL of 600 s, which bounds the
/// poll period (spec "Attach and polls"); it is a third of the clock
/// drift over the default period, so a poll time rounded to it costs a
/// sleeping leaf's window little.
pub const POLL_UNIT: Duration = Duration::from_millis(10);

/// ATTACHED, type 0x7 (spec "Attach and polls"): the relay to one of its leaves, on
/// the relay's home channel. It answers an ATTACH that attaches, and on
/// its own it is a poll, which the leaf answers with an ATTACH.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attached {
    /// The leaf.
    pub leaf: LeafId,
    /// From the end of this frame to the start of the leaf's next slot, in
    /// [`POLL_UNIT`]s. Not a period: each ATTACHED names the next slot
    /// afresh.
    pub next_poll: u16,
    /// Messages the relay holds for the leaf (spec "Delivery queue"), up to
    /// [`WAITING_MAX`].
    pub waiting: u8,
}

impl Attached {
    /// The time to the next poll.
    pub const fn next_poll(&self) -> Duration {
        POLL_UNIT.mul(self.next_poll as u64)
    }

    /// `d` in whole [`POLL_UNIT`]s, rounded to the nearest and capped at
    /// what the field holds.
    pub const fn units(d: Duration) -> u16 {
        let u = (d.as_micros() + POLL_UNIT.as_micros() / 2) / POLL_UNIT.as_micros();
        if u > u16::MAX as u64 {
            u16::MAX
        } else {
            u as u16
        }
    }
}

/// DATA, type 0x5 (spec "Forwarding", "Packet formats").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Data<'a> {
    /// The sender's home channel, where the ACK and anything sent straight
    /// back go (`aux`).
    pub sender_home: Channel,
    /// The relay this transmission is addressed to.
    pub dst_hop: ShortName,
    /// The relay that sent this transmission; zero from a leaf.
    pub src_hop: ShortName,
    /// The destination relay's position.
    pub dst_pos: Position,
    /// The destination leaf, or [`LeafId::RELAY`].
    pub dst_leaf: LeafId,
    /// Hand-overs so far (spec "Hop count").
    pub hops: u8,
    /// The time the message has left, in [`TTL_UNIT`]s (spec "Receiving DATA").
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
        MsgId::of(self.dst_pos, self.dst_leaf, self.payload)
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

/// DELIVER, type 0x6: relay to leaf on the relay's home channel (spec "Delivery").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Deliver<'a> {
    /// The receiving leaf.
    pub dst_leaf: LeafId,
    /// The message, so the leaf can ACK it without hashing.
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
    /// ATTACHED.
    Attached(Attached),
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

fn get_name(b: &[u8], at: usize) -> ShortName {
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
                let kind = if h.surveyed {
                    T_HEARTBEAT_SURVEYED
                } else {
                    T_HEARTBEAT_GPS
                };
                buf[FIRST] = first(kind, h.home.index());
                put(buf, HEARTBEAT_POS, &h.position.encode());
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
            Packet::Attach(a) => {
                let reply = if a.reply { ATTACH_REPLY } else { 0 };
                buf[FIRST] = first(T_ATTACH, a.mode as u8 | reply);
                put(buf, ATTACH_LEAF, &a.leaf.0);
                put(buf, ATTACH_RELAY, &a.relay.0.to_be_bytes());
                Ok(ATTACH_LEN)
            }
            Packet::Data(d) => {
                if d.payload.len() > MAX_PAYLOAD {
                    return Err(EncodeError::PayloadTooLong);
                }
                buf[FIRST] = first(T_DATA, d.sender_home.index());
                put(buf, DATA_DST_HOP, &d.dst_hop.0.to_be_bytes());
                put(buf, DATA_SRC_HOP, &d.src_hop.0.to_be_bytes());
                put(buf, DATA_DST_POS, &d.dst_pos.encode());
                put(buf, DATA_DST_LEAF, &d.dst_leaf.0);
                buf[DATA_HOPS] = d.hops;
                buf[DATA_TIME_LEFT] = d.time_left;
                put(buf, DATA_HEADER_LEN, d.payload);
                Ok(DATA_HEADER_LEN + d.payload.len())
            }
            Packet::Deliver(d) => {
                if d.payload.len() > MAX_PAYLOAD {
                    return Err(EncodeError::PayloadTooLong);
                }
                buf[FIRST] = first(T_DELIVER, 0);
                put(buf, DELIVER_LEAF, &d.dst_leaf.0);
                put(buf, DELIVER_ID, &d.msg_id.0.to_be_bytes());
                put(buf, DELIVER_HEADER_LEN, d.payload);
                Ok(DELIVER_HEADER_LEN + d.payload.len())
            }
            Packet::Attached(a) => {
                buf[FIRST] = first(T_ATTACHED, a.waiting.min(WAITING_MAX));
                put(buf, ATTACHED_LEAF, &a.leaf.0);
                put(buf, ATTACHED_NEXT, &a.next_poll.to_be_bytes());
                Ok(ATTACHED_LEN)
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
                Ok(Packet::Ack(Ack {
                    msg_id: get_msg_id(b, ACK_ID),
                    from: AckName(get4(b, ACK_FROM)),
                    to: AckName(get4(b, ACK_TO)),
                    refused: aux & ACK_REFUSED != 0,
                }))
            }
            T_ATTACH => {
                exact(b, ATTACH_LEN)?;
                Ok(Packet::Attach(Attach {
                    leaf: LeafId(get4(b, ATTACH_LEAF)),
                    relay: get_name(b, ATTACH_RELAY),
                    mode: RxMode::from_code(aux & ATTACH_MODE)
                        .ok_or(DecodeError::BadRxMode(aux & ATTACH_MODE))?,
                    reply: aux & ATTACH_REPLY != 0,
                }))
            }
            T_ATTACHED => {
                exact(b, ATTACHED_LEN)?;
                Ok(Packet::Attached(Attached {
                    leaf: LeafId(get4(b, ATTACHED_LEAF)),
                    next_poll: u16::from_be_bytes([b[ATTACHED_NEXT], b[ATTACHED_NEXT + 1]]),
                    waiting: aux,
                }))
            }
            T_DATA => {
                let payload = payload(b, DATA_HEADER_LEN)?;
                Ok(Packet::Data(Data {
                    sender_home: channel(aux)?,
                    dst_hop: get_name(b, DATA_DST_HOP),
                    src_hop: get_name(b, DATA_SRC_HOP),
                    dst_pos: get_pos(b, DATA_DST_POS),
                    dst_leaf: LeafId(get4(b, DATA_DST_LEAF)),
                    hops: b[DATA_HOPS],
                    time_left: b[DATA_TIME_LEFT],
                    payload,
                }))
            }
            T_DELIVER => {
                let payload = payload(b, DELIVER_HEADER_LEN)?;
                Ok(Packet::Deliver(Deliver {
                    dst_leaf: LeafId(get4(b, DELIVER_LEAF)),
                    msg_id: get_msg_id(b, DELIVER_ID),
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
        (-(1i32 << 24)..(1 << 24), -(1i32 << 25)..(1 << 25))
            .prop_map(|(lat, lon)| Position { lat, lon })
    }

    fn chan() -> impl Strategy<Value = Channel> {
        (0..CHANNELS).prop_map(|c| Channel::new(c).unwrap())
    }

    fn leaf() -> impl Strategy<Value = LeafId> {
        any::<[u8; 4]>().prop_map(LeafId)
    }

    fn name() -> impl Strategy<Value = ShortName> {
        any::<u32>().prop_map(ShortName)
    }

    fn msg() -> impl Strategy<Value = MsgId> {
        any::<u32>().prop_map(MsgId)
    }

    fn rx_mode() -> impl Strategy<Value = RxMode> {
        prop_oneof![
            Just(RxMode::Listening),
            Just(RxMode::DutyCycled),
            Just(RxMode::Hybrid)
        ]
    }

    fn payload() -> impl Strategy<Value = std::vec::Vec<u8>> {
        proptest::collection::vec(any::<u8>(), 0..=MAX_PAYLOAD)
    }

    extern crate std;

    /// Encode, check the length against the table of spec "Packet formats", decode,
    /// compare.
    fn round_trip(p: &Packet<'_>) {
        let mut buf = [0; MAX_FRAME];
        let len = p.encode(&mut buf).unwrap();
        let expected = match p {
            Packet::Heartbeat(_) => 8,
            Packet::Ack(_) => 13,
            Packet::Attach(_) => 9,
            Packet::Data(d) => 22 + d.payload.len(),
            Packet::Deliver(d) => 9 + d.payload.len(),
            Packet::Attached(_) => 7,
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
        fn ack_round_trips(msg_id in msg(), from: [u8; 4], to: [u8; 4], refused: bool) {
            let ack = Ack { msg_id, from: AckName(from), to: AckName(to), refused };
            round_trip(&Packet::Ack(ack));
            let flipped = Ack { refused: !refused, ..ack };
            prop_assert!(ack.answers(&flipped));
            let other = Ack { to: AckName([!to[0], to[1], to[2], to[3]]), ..ack };
            prop_assert!(!ack.answers(&other), "another sender's answer");
        }

        #[test]
        fn attach_round_trips(leaf in leaf(), relay in name(), mode in rx_mode(), reply: bool) {
            round_trip(&Packet::Attach(Attach { leaf, relay, mode, reply }));
        }

        #[test]
        fn attached_round_trips(leaf in leaf(), next_poll: u16, waiting in 0..=WAITING_MAX) {
            let a = Attached { leaf, next_poll, waiting };
            round_trip(&Packet::Attached(a));
            prop_assert_eq!(Attached::units(a.next_poll()), next_poll);
        }

        #[test]
        fn data_round_trips(
            sender_home in chan(),
            dst_hop in name(),
            src_hop in name(),
            dst_pos in position(),
            dst_leaf in leaf(),
            hops: u8,
            time_left: u8,
            payload in payload(),
        ) {
            let d = Data { sender_home, dst_hop, src_hop, dst_pos, dst_leaf, hops, time_left, payload: &payload };
            round_trip(&Packet::Data(d));
            // The ID survives the trip: it depends only on fields every hop keeps.
            let mut buf = [0; MAX_FRAME];
            let len = Packet::Data(d).encode(&mut buf).unwrap();
            let Ok(Packet::Data(back)) = Packet::decode(&buf[..len]) else { panic!() };
            prop_assert_eq!(back.msg_id(), MsgId::of(dst_pos, dst_leaf, &payload));
        }

        #[test]
        fn deliver_round_trips(dst_leaf in leaf(), msg_id in msg(), payload in payload()) {
            round_trip(&Packet::Deliver(Deliver { dst_leaf, msg_id, payload: &payload }));
        }

        #[test]
        fn decode_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..=MAX_FRAME + 2)) {
            // Whatever decodes must re-encode to the same length and decode
            // the same, apart from bits the decoder ignores (unused aux bits
            // of ACK and DELIVER, spare position bits).
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
            dst_hop: ShortName(1),
            src_hop: ShortName(2),
            dst_pos: Position::default(),
            dst_leaf: LeafId::RELAY,
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
        let deliver = Deliver {
            dst_leaf: LeafId::RELAY,
            msg_id: MsgId(1),
            payload: &long,
        };
        assert_eq!(
            Packet::Deliver(deliver).encode(&mut buf),
            Err(EncodeError::PayloadTooLong)
        );
    }

    #[test]
    fn malformed_frames() {
        use DecodeError::*;
        assert_eq!(Packet::decode(&[]), Err(Short));
        assert_eq!(Packet::decode(&[0x00]), Err(BadType(0)));
        assert_eq!(Packet::decode(&[0x70; 7 - 1]), Err(Short));
        assert_eq!(Packet::decode(&[0x70; 7 + 1]), Err(BadLength));
        assert_eq!(Packet::decode(&[0x80; 7]), Err(BadType(8)));
        assert_eq!(Packet::decode(&[0x80, 0, 0, 0, 0]), Err(BadType(8)));
        assert_eq!(
            Packet::decode(&[0x1d, 0, 0, 0, 0, 0, 0, 0]),
            Err(ChannelOutOfRange(13))
        );
        assert_eq!(Packet::decode(&[0x10, 0, 0, 0, 0, 0, 0]), Err(Short));
        assert_eq!(
            Packet::decode(&[0x10, 0, 0, 0, 0, 0, 0, 0, 0]),
            Err(BadLength)
        );
        assert_eq!(Packet::decode(&[0x30; ACK_LEN - 1]), Err(Short));
        assert_eq!(Packet::decode(&[0x30; ACK_LEN + 1]), Err(BadLength));
        assert_eq!(
            Packet::decode(&[0x43, 0, 0, 0, 0, 0, 0, 0, 0]),
            Err(BadRxMode(3))
        );
        assert_eq!(
            Packet::decode(&[0x4b, 0, 0, 0, 0, 0, 0, 0, 0]),
            Err(BadRxMode(3)),
            "the reply bit is not part of the mode"
        );
        assert_eq!(Packet::decode(&[0x40; ATTACH_LEN + 1]), Err(BadLength));
        assert_eq!(Packet::decode(&[0x60, 0, 0, 0, 0, 0, 0, 0]), Err(Short));
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

        // MAX_PAYLOAD + 1 fits a DELIVER frame but is still rejected.
        let mut deliver = [0u8; DELIVER_HEADER_LEN + MAX_PAYLOAD + 1];
        deliver[FIRST] = first(T_DELIVER, 0);
        assert_eq!(Packet::decode(&deliver), Err(PayloadTooLong));
        assert!(Packet::decode(&deliver[..DELIVER_HEADER_LEN + MAX_PAYLOAD]).is_ok());
    }

    #[test]
    fn exact_bytes() {
        // Pin the layouts: field order, big-endian names and IDs.
        let mut buf = [0; MAX_FRAME];
        let position = Position::from_degrees(50.45, 30.52);
        let hb = Heartbeat {
            surveyed: false,
            home: Channel::new(12).unwrap(),
            position,
        };
        let len = Packet::Heartbeat(hb).encode(&mut buf).unwrap();
        assert_eq!(buf[0], 0x2c);
        assert_eq!(&buf[1..len], &position.encode());

        let ack = Ack {
            msg_id: MsgId(0x0102_0304),
            from: ShortName(0x0506_0708).into(),
            to: LeafId([9, 10, 11, 12]).into(),
            refused: false,
        };
        let len = Packet::Ack(ack).encode(&mut buf).unwrap();
        assert_eq!(&buf[..len], &[0x30, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
        assert_eq!(
            AckName::from(LeafId([5, 6, 7, 8])),
            ack.from,
            "a leaf ID is its bytes in order"
        );
        let refused = Ack {
            refused: true,
            ..ack
        };
        let len = Packet::Ack(refused).encode(&mut buf).unwrap();
        assert_eq!(&buf[..len], &[0x31, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
        assert_eq!(
            crate::radio::time_on_air(len as u8, MIN_PREAMBLE_SYMBOLS),
            Duration::from_micros(14_400),
            "51.25 symbols at SF5"
        );

        let attach = Attach {
            leaf: LeafId([1, 2, 3, 4]),
            relay: ShortName(0x0506_0708),
            mode: RxMode::Hybrid,
            reply: false,
        };
        let len = Packet::Attach(attach).encode(&mut buf).unwrap();
        assert_eq!(&buf[..len], &[0x42, 1, 2, 3, 4, 5, 6, 7, 8]);
        let len = Packet::Attach(attach.reply()).encode(&mut buf).unwrap();
        assert_eq!(&buf[..len], &[0x4a, 1, 2, 3, 4, 5, 6, 7, 8]);

        let attached = Attached {
            leaf: LeafId([1, 2, 3, 4]),
            next_poll: 0x0506,
            waiting: 7,
        };
        let len = Packet::Attached(attached).encode(&mut buf).unwrap();
        assert_eq!(&buf[..len], &[0x77, 1, 2, 3, 4, 5, 6]);
        let many = Attached {
            waiting: 200,
            ..attached
        };
        let len = Packet::Attached(many).encode(&mut buf).unwrap();
        assert_eq!(buf[0], 0x7f, "saturates at 15");
        assert_eq!(
            Packet::decode(&buf[..len]),
            Ok(Packet::Attached(Attached {
                waiting: WAITING_MAX,
                ..attached
            }))
        );
        assert_eq!(
            crate::radio::time_on_air(len as u8, MIN_PREAMBLE_SYMBOLS),
            Duration::from_micros(10_560),
            "41.25 symbols at SF5, as for 5 or 6 bytes"
        );
        assert_eq!(Attached::units(Duration::from_millis(300_004)), 30_000);
        assert_eq!(Attached::units(Duration::from_secs(1000)), u16::MAX);

        let d = Data {
            sender_home: Channel::new(9).unwrap(),
            dst_hop: ShortName(0x0102_0304),
            src_hop: ShortName(0x0506_0708),
            dst_pos: position,
            dst_leaf: LeafId([9, 10, 11, 12]),
            hops: 13,
            time_left: 14,
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

        let deliver = Deliver {
            dst_leaf: LeafId([1, 2, 3, 4]),
            msg_id: MsgId(0x0506_0708),
            payload: &[9],
        };
        let len = Packet::Deliver(deliver).encode(&mut buf).unwrap();
        assert_eq!(&buf[..len], &[0x60, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }
}
