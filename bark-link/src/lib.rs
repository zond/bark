//! The phone link's values: the BLE GATT service by which a phone app
//! talks to its node (spec "Phone link"). One codec, used by the firmware
//! and checked here on the host, so that the node and the app's tests
//! read the same bytes.
//!
//! Positions and node IDs keep the radio's encodings (`bark_proto`
//! `Position::encode`, `NodeId`), and multi-byte integers are big-endian
//! as on the radio (`docs/core-interface.md` §7). Values the node writes
//! may grow in a later link version, so their decoders ignore what
//! follows the fields they know; values the app writes are taken only
//! whole.

#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use bark_proto::wire::MAX_PAYLOAD;
use bark_proto::{Instant, MsgId, NodeId, Position};

/// The link version this crate speaks, in [`Node::version`]. Every
/// change to the node's attribute table comes with a new version, as the
/// node tells a bonded phone of a change by the version it last saw
/// (spec "Phone link", Changes to the table); 5 added Service Changed
/// and the database hash, 6 Update and Image, 7 Recipient and Public, 8
/// Guest, 9 Ask, 10 the image a neighbour gathers in Ask's answer, 11
/// the neighbours heard in Status, 12 Public's write that unpairs, 13
/// Phones, with the person's name in Recipient, `room` in Node and `mine`
/// in Home (roadmap D168).
pub const VERSION: u8 = 13;

/// The oldest link version whose apps this node still serves, in
/// [`Node::oldest`] (spec "Phone link", Versions; roadmap D177): 12, as
/// every read and write of a version 12 app still means what it meant.
/// Version 13 appended `room` and `oldest` to Node and `mine` to Home,
/// which such an app does not read; it writes Recipient's tag alone,
/// which the node still takes; it does not know Phones, which it never
/// touches; and its Inbox, Home and Public act on its own person's
/// record, which is what they acted on when the node kept one phone. An
/// app older than 12 cannot be told from one of 12, as it too writes the
/// tag alone, so 12 is the oldest the node can vouch for.
pub const OLDEST: u8 = 12;

/// Whether an app of link version `version`, which serves nodes of link
/// version `oldest` and later, and this node can serve each other: each
/// side's version at least the other's oldest (spec "Phone link",
/// Versions).
pub const fn compatible(version: u8, oldest: u8) -> bool {
    version >= OLDEST && VERSION >= oldest
}

/// The ATT MTU the node offers: the longest LL data PDU payload with the
/// data length extension, 251 bytes (Bluetooth Core Specification 5.0,
/// Vol 6 Part B §2.4), less the 4-byte L2CAP header (Vol 3 Part A §3.1).
/// Every value then fits one ATT PDU and one link-layer packet.
pub const ATT_MTU: u16 = 251 - 4;

/// Bytes of a value an ATT PDU carries at an MTU: a write request and a
/// notification each spend 3 on the opcode and the handle (Vol 3 Part F
/// §3.4.5.1, §3.4.7.1).
pub const fn value_room(mtu: u16) -> usize {
    mtu as usize - 3
}

/// The ATT MTU every connection starts with (Vol 3 Part F §3.2.8).
pub const DEFAULT_ATT_MTU: u16 = 23;

/// The 128-bit UUIDs, as numbers; [`le_bytes`] gives the order on the air.
/// One random version-4 UUID, its second 16 bits counting the service and
/// its characteristics.
pub mod uuid {
    /// The service.
    pub const SERVICE: u128 = 0x28bc0001_a1b8_4893_9537_2ee3559d7644;
    /// Node: read ([`crate::Node`]).
    pub const NODE: u128 = 0x28bc0002_a1b8_4893_9537_2ee3559d7644;
    /// Status: read, notify ([`crate::Status`]).
    pub const STATUS: u128 = 0x28bc0003_a1b8_4893_9537_2ee3559d7644;
    /// Send: write ([`crate::Send`]).
    pub const SEND: u128 = 0x28bc0004_a1b8_4893_9537_2ee3559d7644;
    /// Inbox: read the oldest message, write its ID to remove it
    /// ([`crate::Message`], [`crate::Confirm`]).
    pub const INBOX: u128 = 0x28bc0005_a1b8_4893_9537_2ee3559d7644;
    /// Given back: read, notify ([`crate::GivenBack`]).
    pub const GIVEN_BACK: u128 = 0x28bc0006_a1b8_4893_9537_2ee3559d7644;
    /// Map point: write ([`crate::MapPoint`]).
    pub const MAP_POINT: u128 = 0x28bc0007_a1b8_4893_9537_2ee3559d7644;
    /// Clock: write the phone's time, read the node's ([`crate::Clock`],
    /// [`crate::NodeTime`]); from link version 2.
    pub const CLOCK: u128 = 0x28bc0008_a1b8_4893_9537_2ee3559d7644;
    /// Home: write the mailbox key or clear it, read the mailbox's
    /// numbers ([`crate::home`]); from link version 3.
    pub const HOME: u128 = 0x28bc0009_a1b8_4893_9537_2ee3559d7644;
    /// Neighbours: write a slot, read the neighbour table from it
    /// ([`crate::neighbours`]); from link version 4.
    pub const NEIGHBOURS: u128 = 0x28bc000a_a1b8_4893_9537_2ee3559d7644;
    /// Update: begin, commit and reboot, and read where the update stands
    /// ([`crate::update`]); from link version 6.
    pub const UPDATE: u128 = 0x28bc000b_a1b8_4893_9537_2ee3559d7644;
    /// Image: write the image's bytes, with or without a response
    /// ([`crate::update::Piece`]); from link version 6.
    pub const IMAGE: u128 = 0x28bc000c_a1b8_4893_9537_2ee3559d7644;
    /// Recipient: write the phone's recipient tag at this node
    /// ([`crate::Recipient`]); from link version 7.
    pub const RECIPIENT: u128 = 0x28bc000d_a1b8_4893_9537_2ee3559d7644;
    /// Public: read and write whether the node serves phones without
    /// pairing ([`crate::Public`]); from link version 7.
    pub const PUBLIC: u128 = 0x28bc000e_a1b8_4893_9537_2ee3559d7644;
    /// Guest: write the number of the last item a guest took, after which
    /// its next read of Inbox looks ([`crate::After`],
    /// [`crate::GuestItem`]); from link version 8.
    pub const GUEST: u128 = 0x28bc000f_a1b8_4893_9537_2ee3559d7644;
    /// Ask: write a neighbour's ID to ask it for its status, read and
    /// notify the last answer ([`crate::Ask`], [`crate::Answer`]); from
    /// link version 9.
    pub const ASK: u128 = 0x28bc0010_a1b8_4893_9537_2ee3559d7644;
    /// Phones: list the node's records, and remove one with a code drawn
    /// on the screen ([`crate::phones`]); from link version 13.
    pub const PHONES: u128 = 0x28bc0011_a1b8_4893_9537_2ee3559d7644;
    /// What a public node advertises in place of [`SERVICE`]: the class
    /// of nodes that serve any phone. No attribute has it.
    pub const PUBLIC_NODE: u128 = 0x28bc1001_a1b8_4893_9537_2ee3559d7644;
}

/// A UUID's bytes in the order BLE sends them, least significant first
/// (Vol 3 Part B §2.5.1).
pub const fn le_bytes(uuid: u128) -> [u8; 16] {
    uuid.to_le_bytes()
}

/// The prefix of the node's advertised name; the twelve hex digits of its
/// ID follow, as on its display.
pub const NAME_PREFIX: &str = "bark-";

/// The advertised name: [`NAME_PREFIX`] and the ID in lowercase hex.
pub fn name(id: NodeId) -> [u8; NAME_LEN] {
    let digit = |n: u8| b"0123456789abcdef"[usize::from(n)];
    let mut name = [0u8; NAME_LEN];
    name[..NAME_PREFIX.len()].copy_from_slice(NAME_PREFIX.as_bytes());
    for (i, byte) in id.0.into_iter().enumerate() {
        name[NAME_PREFIX.len() + 2 * i] = digit(byte >> 4);
        name[NAME_PREFIX.len() + 2 * i + 1] = digit(byte & 0x0F);
    }
    name
}

/// Bytes in the advertised name.
pub const NAME_LEN: usize = NAME_PREFIX.len() + 2 * NodeId::LEN;

/// ATT error codes the node answers a write with (Vol 3 Part F §3.4.1.1).
pub mod att {
    /// The value's length is not one the characteristic takes.
    pub const INVALID_LENGTH: u8 = 0x0D;
    /// The value has the right length and a field it does not take.
    pub const VALUE_NOT_ALLOWED: u8 = 0x13;
    /// Not now: the node takes no message from its application at the
    /// moment; the app waits for [`crate::Status::ready`]. The first of
    /// the application error codes, 0x80 to 0x9F.
    pub const NOT_READY: u8 = 0x80;
    /// The node refused a begin, a commit or a reboot of an update; Update
    /// says why ([`crate::update::Refusal`]).
    pub const REFUSED: u8 = 0x81;
    /// The app's link version and the node's do not serve each other: one
    /// is below the other's oldest (spec "Phone link", Versions). The node
    /// answers Recipient's write so, then ends the link.
    pub const INCOMPATIBLE: u8 = 0x82;
}

/// Why a value was not taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Malformed {
    /// Too short, too long, or not a length the value has.
    Length,
    /// A field holds what it may not.
    Value,
}

impl Malformed {
    /// The ATT error the node answers a write with.
    pub const fn att_error(self) -> u8 {
        match self {
            Malformed::Length => att::INVALID_LENGTH,
            Malformed::Value => att::VALUE_NOT_ALLOWED,
        }
    }
}

const POS: usize = Position::WIRE_LEN;
const ID: usize = NodeId::LEN;
const MSG_ID: usize = 4;

fn position(b: &[u8]) -> Position {
    Position::decode(b[..POS].try_into().expect("seven bytes"))
}

fn node_id(b: &[u8]) -> NodeId {
    NodeId(b[..ID].try_into().expect("six bytes"))
}

fn msg_id(b: &[u8]) -> MsgId {
    MsgId(u32::from_be_bytes(
        b[..MSG_ID].try_into().expect("four bytes"),
    ))
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([b[at], b[at + 1]])
}

/// Whole seconds from `now` to `until`, rounded up so that an app that
/// waits them is never early, and 0 once `until` has passed; at most
/// `u16::MAX`, 18 h, beyond the longest wait a refusal tells, 89 min
/// (spec "Bans").
pub fn seconds_left(now: Instant, until: Instant) -> u16 {
    const SECOND_US: u64 = 1_000_000;
    let us = until.since(now).as_micros();
    us.div_ceil(SECOND_US).min(u16::MAX as u64) as u16
}

/// Node, read: what the node is, fixed while it runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    /// The link version the node speaks.
    pub version: u8,
    /// The node's ID, the second half of its address (spec "Addresses").
    pub id: NodeId,
    /// The firmware image's version (`bark-fw/README.md`, "Updates").
    pub image: u32,
    /// How many more phones may pair with it (spec "Phone link", Node);
    /// `None` from a node of link version 12 or before, which kept one.
    pub room: Option<u8>,
    /// The oldest link version whose apps it serves ([`OLDEST`]); `None`
    /// from a node of link version 12 or before.
    pub oldest: Option<u8>,
}

impl Node {
    /// Bytes in the value.
    pub const LEN: usize = Self::LEN_12 + 2;
    /// Bytes in the value of link version 12 and before.
    const LEN_12: usize = 1 + ID + 4;

    /// The value.
    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut b = [0u8; Self::LEN];
        b[0] = self.version;
        b[1..1 + ID].copy_from_slice(&self.id.0);
        b[1 + ID..Self::LEN_12].copy_from_slice(&self.image.to_be_bytes());
        b[Self::LEN_12] = self.room.unwrap_or(0);
        b[Self::LEN_12 + 1] = self.oldest.unwrap_or(0);
        b
    }

    /// Read a value, ignoring what a later version appends.
    pub fn decode(b: &[u8]) -> Result<Node, Malformed> {
        if b.len() < Self::LEN_12 {
            return Err(Malformed::Length);
        }
        Ok(Node {
            version: b[0],
            id: node_id(&b[1..]),
            image: u32::from_be_bytes(b[1 + ID..Self::LEN_12].try_into().expect("four bytes")),
            room: b.get(Self::LEN_12).copied(),
            oldest: b.get(Self::LEN_12 + 1).copied(),
        })
    }
}

/// Status, read and notify: how the node stands now. The node notifies
/// it whenever a field changes, but [`Status::boot_left_s`] counting
/// down. It fits the 20 bytes a notification carries at the default MTU.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Status {
    /// The node takes a message from the app now ([`Send`]).
    pub ready: bool,
    /// External power, USB, is present.
    pub external_power: bool,
    /// The node is sleepy (spec "Nodes"); measured, not set.
    pub sleepy: bool,
    /// The node is carried (spec "Nodes"); measured, not set.
    pub carried: bool,
    /// The node's position is a map point, not a GPS fix (spec "Position").
    pub map_point: bool,
    /// The node has a place, and [`Status::address`] is its address's
    /// position; without one it is zero (spec "Position").
    pub placed: bool,
    /// The first rotation is under way (spec "Channel at boot").
    pub booting: bool,
    /// A sleepy node without an anchor (spec "Sleepy nodes alone").
    pub anchorless: bool,
    /// The relays it knows, nodes that are not sleepy.
    pub relays: u8,
    /// Seconds left of the first rotation, rounded up; 0 once it is over.
    pub boot_left_s: u16,
    /// The position of the node's address: its own, or a sleepy node's
    /// anchor's (spec "Addresses").
    pub address: Position,
    /// The battery in millivolts as last read, uncalibrated; 0 on external
    /// power and before the first reading.
    pub battery_mv: u16,
    /// Messages in the inbox.
    pub inbox: u8,
    /// Its live neighbours, sleepy or not (spec "Neighbour status",
    /// heard).
    pub heard: u8,
    /// The low 16 bits of the image version the node drains to install
    /// (spec "Updates over the mesh", the install); 0 when none, and from
    /// a node of link version 12 or before.
    pub installing: u16,
}

impl Status {
    /// Bytes in the value.
    pub const LEN: usize = Self::LEN_12 + 2;
    /// Bytes in the value of link version 12 and before.
    const LEN_12: usize = 1 + 1 + 2 + POS + 2 + 1 + 1;

    /// Every flag clear and every count zero: a node that has said
    /// nothing yet.
    pub const EMPTY: Status = Status {
        ready: false,
        external_power: false,
        sleepy: false,
        carried: false,
        map_point: false,
        placed: false,
        booting: false,
        anchorless: false,
        relays: 0,
        boot_left_s: 0,
        address: Position { lat: 0, lon: 0 },
        battery_mv: 0,
        inbox: 0,
        heard: 0,
        installing: 0,
    };

    fn flags(&self) -> u8 {
        [
            self.ready,
            self.external_power,
            self.sleepy,
            self.carried,
            self.map_point,
            self.placed,
            self.booting,
            self.anchorless,
        ]
        .iter()
        .enumerate()
        .fold(0, |f, (bit, &set)| f | (u8::from(set) << bit))
    }

    /// The value.
    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut b = [0u8; Self::LEN];
        b[0] = self.flags();
        b[1] = self.relays;
        b[2..4].copy_from_slice(&self.boot_left_s.to_be_bytes());
        if self.placed {
            b[4..4 + POS].copy_from_slice(&self.address.encode());
        }
        b[4 + POS..6 + POS].copy_from_slice(&self.battery_mv.to_be_bytes());
        b[6 + POS] = self.inbox;
        b[7 + POS] = self.heard;
        b[Self::LEN_12..].copy_from_slice(&self.installing.to_be_bytes());
        b
    }

    /// Read a value, ignoring what a later version appends.
    pub fn decode(b: &[u8]) -> Result<Status, Malformed> {
        if b.len() < Self::LEN_12 {
            return Err(Malformed::Length);
        }
        let flag = |bit: u8| b[0] & (1 << bit) != 0;
        Ok(Status {
            ready: flag(0),
            external_power: flag(1),
            sleepy: flag(2),
            carried: flag(3),
            map_point: flag(4),
            placed: flag(5),
            booting: flag(6),
            anchorless: flag(7),
            relays: b[1],
            boot_left_s: u16_at(b, 2),
            address: position(&b[4..]),
            battery_mv: u16_at(b, 4 + POS),
            inbox: b[6 + POS],
            heard: b[7 + POS],
            installing: match b.len() < Self::LEN {
                true => 0,
                false => u16_at(b, Self::LEN_12),
            },
        })
    }
}

/// Send, write: a message for the node to originate, as
/// `AppCommand::Send` takes it (spec "Receiving DATA").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Send<'a> {
    /// The destination's position (spec "Addresses").
    pub dst_pos: Position,
    /// The destination node.
    pub dst_id: NodeId,
    /// The opaque payload, at most `MAX_PAYLOAD`, 227 bytes
    /// (spec "Packet formats").
    pub payload: &'a [u8],
}

impl<'a> Send<'a> {
    /// Bytes before the payload.
    pub const HEADER: usize = POS + ID;
    /// Bytes in the longest value.
    pub const MAX_LEN: usize = Self::HEADER + MAX_PAYLOAD;

    /// The message's ID, by which [`GivenBack`] names it
    /// (spec "Packet formats").
    pub fn msg_id(&self) -> MsgId {
        MsgId::of(self.dst_pos, self.dst_id, self.payload)
    }

    /// The value, into `b`; returns its length.
    pub fn encode(&self, b: &mut [u8; Send::<'static>::MAX_LEN]) -> usize {
        let len = Self::HEADER + self.payload.len();
        assert!(self.payload.len() <= MAX_PAYLOAD, "payload too long");
        b[..POS].copy_from_slice(&self.dst_pos.encode());
        b[POS..Self::HEADER].copy_from_slice(&self.dst_id.0);
        b[Self::HEADER..len].copy_from_slice(self.payload);
        len
    }

    /// Take a written value whole.
    pub fn decode(b: &'a [u8]) -> Result<Send<'a>, Malformed> {
        if !(Self::HEADER..=Self::MAX_LEN).contains(&b.len()) {
            return Err(Malformed::Length);
        }
        Ok(Send {
            dst_pos: position(b),
            dst_id: node_id(&b[POS..]),
            payload: &b[Self::HEADER..],
        })
    }
}

/// Inbox, read: the oldest message the node holds for the app, as the
/// core's inbox holds it (`Core::inbox`); the value is empty when it holds
/// none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Message<'a> {
    /// Its ID, which [`Confirm`] names.
    pub msg_id: MsgId,
    /// The opaque payload.
    pub payload: &'a [u8],
}

impl<'a> Message<'a> {
    /// Bytes before the payload.
    pub const HEADER: usize = MSG_ID;
    /// Bytes in the longest value.
    pub const MAX_LEN: usize = Self::HEADER + MAX_PAYLOAD;

    /// The value, into `b`; returns its length.
    pub fn encode(&self, b: &mut [u8; Message::<'static>::MAX_LEN]) -> usize {
        let len = Self::HEADER + self.payload.len();
        assert!(self.payload.len() <= MAX_PAYLOAD, "payload too long");
        b[..MSG_ID].copy_from_slice(&self.msg_id.0.to_be_bytes());
        b[MSG_ID..len].copy_from_slice(self.payload);
        len
    }

    /// Read a value: `None` when the inbox is empty. The whole value past
    /// the ID is the payload, so this value never grows.
    pub fn decode(b: &'a [u8]) -> Result<Option<Message<'a>>, Malformed> {
        match b.len() {
            0 => Ok(None),
            n if (Self::HEADER..=Self::MAX_LEN).contains(&n) => Ok(Some(Message {
                msg_id: msg_id(b),
                payload: &b[MSG_ID..],
            })),
            _ => Err(Malformed::Length),
        }
    }
}

/// Inbox, write: the app has the message with this ID and the node may
/// forget it. Only the oldest is removed; another ID is not taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Confirm(pub MsgId);

impl Confirm {
    /// Bytes in the value.
    pub const LEN: usize = MSG_ID;

    /// The value.
    pub fn encode(&self) -> [u8; Self::LEN] {
        self.0 .0.to_be_bytes()
    }

    /// Take a written value whole.
    pub fn decode(b: &[u8]) -> Result<Confirm, Malformed> {
        if b.len() != Self::LEN {
            return Err(Malformed::Length);
        }
        Ok(Confirm(msg_id(b)))
    }
}

/// Given back, read and notify: the app's message that the node gave
/// back, refused (spec "Addresses"), and the wait the refusal asked
/// for. The value is empty when the node has given none back since it
/// last took one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GivenBack {
    /// The message's ID ([`Send::msg_id`]).
    pub msg_id: MsgId,
    /// Seconds left of the wait, rounded up ([`seconds_left`]); 0 once it
    /// is over.
    pub wait_s: u16,
}

impl GivenBack {
    /// Bytes in the value.
    pub const LEN: usize = MSG_ID + 2;

    /// The value.
    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut b = [0u8; Self::LEN];
        b[..MSG_ID].copy_from_slice(&self.msg_id.0.to_be_bytes());
        b[MSG_ID..].copy_from_slice(&self.wait_s.to_be_bytes());
        b
    }

    /// Read a value, `None` when empty, ignoring what a later version
    /// appends.
    pub fn decode(b: &[u8]) -> Result<Option<GivenBack>, Malformed> {
        match b.len() {
            0 => Ok(None),
            n if n < Self::LEN => Err(Malformed::Length),
            _ => Ok(Some(GivenBack {
                msg_id: msg_id(b),
                wait_s: u16_at(b, MSG_ID),
            })),
        }
    }
}

/// Map point, write: set the node's position to a map point, or clear it
/// (spec "Position"; `AppCommand::SetPosition`, `ClearMapPoint`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapPoint {
    /// Clear the map point: one byte, 0.
    Clear,
    /// Set it: one byte, 1, then the position.
    Set(Position),
}

impl MapPoint {
    const CLEAR: u8 = 0;
    const SET: u8 = 1;
    /// Bytes in the longest value.
    pub const MAX_LEN: usize = 1 + POS;

    /// The value, into `b`; returns its length.
    pub fn encode(&self, b: &mut [u8; Self::MAX_LEN]) -> usize {
        match self {
            MapPoint::Clear => {
                b[0] = Self::CLEAR;
                1
            }
            MapPoint::Set(p) => {
                b[0] = Self::SET;
                b[1..].copy_from_slice(&p.encode());
                Self::MAX_LEN
            }
        }
    }

    /// Take a written value whole.
    pub fn decode(b: &[u8]) -> Result<MapPoint, Malformed> {
        match (b.first(), b.len()) {
            (Some(&Self::CLEAR), 1) => Ok(MapPoint::Clear),
            (Some(&Self::SET), Self::MAX_LEN) => Ok(MapPoint::Set(position(&b[1..]))),
            (Some(&Self::CLEAR | &Self::SET), _) | (None, _) => Err(Malformed::Length),
            (Some(_), _) => Err(Malformed::Value),
        }
    }
}

/// The UTC offsets in use, minutes: UTC−12:00 to UTC+14:00 (the time
/// zone database's range).
pub const OFFSETS_MIN: core::ops::RangeInclusive<i16> = -12 * 60..=14 * 60;

/// Clock, write: the phone's time, UTC seconds since 1970, and its UTC
/// offset in minutes. The node takes the offset always and the time while
/// it has none from its GPS (spec "Phone link").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clock {
    /// UTC seconds since 1970-01-01.
    pub utc_s: u32,
    /// Minutes to add to UTC for the phone's local time.
    pub offset_min: i16,
}

impl Clock {
    /// Bytes in the value.
    pub const LEN: usize = 4 + 2;

    /// The value.
    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut b = [0u8; Self::LEN];
        b[..4].copy_from_slice(&self.utc_s.to_be_bytes());
        b[4..].copy_from_slice(&self.offset_min.to_be_bytes());
        b
    }

    /// Take a written value whole; an offset outside [`OFFSETS_MIN`] is
    /// not taken.
    pub fn decode(b: &[u8]) -> Result<Clock, Malformed> {
        if b.len() != Self::LEN {
            return Err(Malformed::Length);
        }
        let offset_min = u16_at(b, 4) as i16;
        if !OFFSETS_MIN.contains(&offset_min) {
            return Err(Malformed::Value);
        }
        Ok(Clock {
            utc_s: u32::from_be_bytes(b[..4].try_into().expect("four bytes")),
            offset_min,
        })
    }
}

/// Where the node's time came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeSource {
    /// It has no time: none came, or the one that came has drifted past a
    /// minute.
    None = 0,
    /// Its GPS.
    Gps = 1,
    /// The phone.
    Phone = 2,
}

/// Clock, read: the node's time as it keeps it, and the offset it keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeTime {
    /// UTC seconds since 1970; 0 when the source is none.
    pub utc_s: u32,
    /// The UTC offset the phone last gave, minutes; 0 before one.
    pub offset_min: i16,
    /// Where the time came from.
    pub source: TimeSource,
}

impl NodeTime {
    /// Bytes in the value.
    pub const LEN: usize = Clock::LEN + 1;

    /// The value.
    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut b = [0u8; Self::LEN];
        let utc_s = match self.source {
            TimeSource::None => 0,
            _ => self.utc_s,
        };
        b[..Clock::LEN].copy_from_slice(
            &Clock {
                utc_s,
                offset_min: self.offset_min,
            }
            .encode(),
        );
        b[Clock::LEN] = self.source as u8;
        b
    }

    /// Read a value, ignoring what a later version appends.
    pub fn decode(b: &[u8]) -> Result<NodeTime, Malformed> {
        if b.len() < Self::LEN {
            return Err(Malformed::Length);
        }
        let source = match b[Clock::LEN] {
            0 => TimeSource::None,
            1 => TimeSource::Gps,
            2 => TimeSource::Phone,
            _ => return Err(Malformed::Value),
        };
        Ok(NodeTime {
            utc_s: u32::from_be_bytes(b[..4].try_into().expect("four bytes")),
            offset_min: u16_at(b, 4) as i16,
            source,
        })
    }
}

/// Recipient, write: the phone's recipient tag at this node, by which the
/// node hands it the payloads that are its own (spec "Phone link",
/// Recipient tag; `bark_app_layer::seal::recipient`), and from link
/// version 13 the app's link version, the oldest node version it serves
/// (spec "Phone link", Versions) and its person's name as their card has
/// it, which a paired phone's record keeps (spec "Phone link", Pairing).
/// An app of version 12 or older writes the tag alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Recipient {
    /// The tag.
    pub tag: u8,
    /// The app's link version.
    pub version: u8,
    /// The oldest node link version the app serves.
    pub oldest: u8,
    /// The name: UTF-8, at most [`phones::NAME_MAX`] bytes.
    pub name: phones::Name,
}

impl Recipient {
    /// Bytes in the longest value.
    pub const MAX_LEN: usize = 3 + phones::NAME_MAX;
    /// What a write of the tag alone says of its app: version 12 or older,
    /// which declared no oldest and so serves any node.
    const ALONE: (u8, u8) = (12, 0);

    /// The value, into `b`; its length.
    pub fn encode(&self, b: &mut [u8; Self::MAX_LEN]) -> usize {
        let name = self.name.as_bytes();
        b[..3].copy_from_slice(&[self.tag, self.version, self.oldest]);
        b[3..3 + name.len()].copy_from_slice(name);
        3 + name.len()
    }

    /// Take a written value whole: the tag alone as version 12's, and a
    /// name that is not UTF-8 is not taken.
    pub fn decode(b: &[u8]) -> Result<Recipient, Malformed> {
        match b {
            [tag] => Ok(Recipient {
                tag: *tag,
                version: Self::ALONE.0,
                oldest: Self::ALONE.1,
                name: phones::Name::EMPTY,
            }),
            [tag, version, oldest, name @ ..] => Ok(Recipient {
                tag: *tag,
                version: *version,
                oldest: *oldest,
                name: phones::Name::new(name)?,
            }),
            _ => Err(Malformed::Length),
        }
    }

    /// Whether the app and this node serve each other ([`compatible`]).
    pub fn compatible(&self) -> bool {
        compatible(self.version, self.oldest)
    }
}

/// Public, read and write: 1 when the node serves phones that have not
/// paired with it, as it does unless its owner says otherwise; 0 when
/// the owner has made it private, its alone (spec "Phone link", Public).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Public(pub bool);

impl Public {
    /// Bytes in the value.
    pub const LEN: usize = 1;

    /// The value.
    pub fn encode(&self) -> [u8; Self::LEN] {
        [u8::from(self.0)]
    }

    /// Take a written value whole.
    pub fn decode(b: &[u8]) -> Result<Public, Malformed> {
        match b {
            [0] => Ok(Public(false)),
            [1] => Ok(Public(true)),
            [_] => Err(Malformed::Value),
            _ => Err(Malformed::Length),
        }
    }
}

/// Public, as the owner writes it: whether the node is public, or that
/// the node is to forget its bond, unpaired, and be public again as a
/// node nobody has paired with is (spec "Phone link", Public); the
/// unpairing from link version 12.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublicWrite {
    /// Public or private.
    Set(Public),
    /// Forget the bond.
    Unpair,
}

impl PublicWrite {
    /// The byte of an unpairing.
    pub const UNPAIR: u8 = 2;

    /// The value.
    pub fn encode(&self) -> [u8; Public::LEN] {
        match self {
            PublicWrite::Set(p) => p.encode(),
            PublicWrite::Unpair => [Self::UNPAIR],
        }
    }

    /// Take a written value whole.
    pub fn decode(b: &[u8]) -> Result<PublicWrite, Malformed> {
        match b {
            [Self::UNPAIR] => Ok(PublicWrite::Unpair),
            _ => Public::decode(b).map(PublicWrite::Set),
        }
    }
}

/// Bytes of an item's number in the ring, which counts to 2^24 − 1
/// (`bark_app_layer::seal::NUMBER_LEN`).
const NUMBER: usize = 3;

/// Guest, write: the number of the last item the guest took; its next
/// read of Inbox gives the oldest item after it for its recipient tag
/// (spec "Phone link", Public). 0 at each connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct After(pub u32);

impl After {
    /// Bytes in the value.
    pub const LEN: usize = NUMBER;

    /// The value.
    pub fn encode(&self) -> [u8; Self::LEN] {
        let b = self.0.to_be_bytes();
        [b[1], b[2], b[3]]
    }

    /// Take a written value whole.
    pub fn decode(b: &[u8]) -> Result<After, Malformed> {
        match b {
            [x, y, z] => Ok(After(u32::from_be_bytes([0, *x, *y, *z]))),
            _ => Err(Malformed::Length),
        }
    }
}

/// Inbox, read on a guest's link: the item's number in the ring, the
/// message ID it arrived with, and its payload; empty when none waits
/// after the number written to Guest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuestItem<'a> {
    /// The item's number, which the guest writes back as [`After`] to
    /// read on.
    pub number: u32,
    /// Its message ID.
    pub msg_id: MsgId,
    /// The payload.
    pub payload: &'a [u8],
}

impl<'a> GuestItem<'a> {
    /// Bytes before the payload.
    pub const HEADER: usize = NUMBER + MSG_ID;
    /// Bytes in the longest value.
    pub const MAX_LEN: usize = Self::HEADER + MAX_PAYLOAD;

    /// The value, into `b`; its length.
    pub fn encode(&self, b: &mut [u8; GuestItem::MAX_LEN]) -> usize {
        b[..NUMBER].copy_from_slice(&After(self.number).encode());
        b[NUMBER..Self::HEADER].copy_from_slice(&self.msg_id.0.to_be_bytes());
        b[Self::HEADER..Self::HEADER + self.payload.len()].copy_from_slice(self.payload);
        Self::HEADER + self.payload.len()
    }

    /// Read a value, `None` when empty.
    pub fn decode(b: &'a [u8]) -> Result<Option<GuestItem<'a>>, Malformed> {
        match b.len() {
            0 => Ok(None),
            n if !(Self::HEADER..=Self::MAX_LEN).contains(&n) => Err(Malformed::Length),
            _ => Ok(Some(GuestItem {
                number: After::decode(&b[..NUMBER])?.0,
                msg_id: msg_id(&b[NUMBER..]),
                payload: &b[Self::HEADER..],
            })),
        }
    }
}

/// Ask, write: ask this neighbour for its status
/// (spec "Neighbour status"; `AppCommand::Ask`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ask(pub NodeId);

impl Ask {
    /// Bytes in the value.
    pub const LEN: usize = ID;

    /// The value.
    pub fn encode(&self) -> [u8; Self::LEN] {
        self.0 .0
    }

    /// Take a written value whole.
    pub fn decode(b: &[u8]) -> Result<Ask, Malformed> {
        match b.len() {
            Self::LEN => Ok(Ask(node_id(b))),
            _ => Err(Malformed::Length),
        }
    }
}

/// Ask, read and notify: the last answer the node was given, the
/// neighbour's ID and its status in the radio's encoding
/// (`bark_proto::wire::encode_status`); empty before the first since
/// the node booted. A notification at the default MTU carries its first
/// 20 bytes, all but the image the neighbour gathers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Answer {
    /// The neighbour that answered.
    pub from: NodeId,
    /// What it says of itself.
    pub status: bark_proto::status::Status,
}

impl Answer {
    /// Bytes in the value.
    pub const LEN: usize = ID + STATUS;

    /// The value.
    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut b = [0u8; Self::LEN];
        b[..ID].copy_from_slice(&self.from.0);
        let status: &mut [u8; STATUS] = (&mut b[ID..]).try_into().expect("STATUS bytes");
        bark_proto::wire::encode_status(&self.status, status);
        b
    }

    /// Read a value, `None` when empty, ignoring what a later version
    /// appends; one without the image gathered whole, link version 9's or
    /// a notification's at the default MTU, as gathering none.
    pub fn decode(b: &[u8]) -> Result<Option<Answer>, Malformed> {
        let mut status = [0u8; STATUS];
        match b.len() {
            0 => Ok(None),
            n if n < ID + STATUS_9 => Err(Malformed::Length),
            n => {
                let whole = if n >= Self::LEN { STATUS } else { STATUS_9 };
                status[..whole].copy_from_slice(&b[ID..ID + whole]);
                Ok(Some(Answer {
                    from: node_id(b),
                    status: bark_proto::wire::decode_status(&status),
                }))
            }
        }
    }
}

/// Bytes of a status, as on the radio.
const STATUS: usize = bark_proto::wire::STATUS_LEN;

/// Bytes of a status before the image gathered, as link version 9 had it.
const STATUS_9: usize = 9;

// Every value the node or the app writes fits one ATT PDU at the MTU the
// node offers; the status fits one at the default MTU.
const _: () = assert!(Send::MAX_LEN <= value_room(ATT_MTU));
const _: () = assert!(Message::MAX_LEN <= value_room(ATT_MTU));
const _: () = assert!(GuestItem::MAX_LEN <= value_room(ATT_MTU));
const _: () = assert!(Status::LEN <= value_room(DEFAULT_ATT_MTU));
const _: () = assert!(GivenBack::LEN <= value_room(DEFAULT_ATT_MTU));
// Ask's answer needs the MTU the app asks for; a notification at the
// default one still carries all but the image gathered.
const _: () = assert!(Answer::LEN <= value_room(ATT_MTU));
const _: () = assert!(ID + STATUS_9 <= value_room(DEFAULT_ATT_MTU));
const _: () = assert!(Send::MAX_LEN == 240);
const _: () = assert!(neighbours::MAX_LEN <= value_room(ATT_MTU));
const _: () = assert!(update::Piece::MAX_LEN == value_room(ATT_MTU));
const _: () = assert!(update::Progress::LEN <= value_room(ATT_MTU));
const _: () = assert!(Recipient::MAX_LEN <= value_room(ATT_MTU));
const _: () = assert!(phones::PhonesWrite::MAX_LEN <= value_room(DEFAULT_ATT_MTU));

pub mod bond;
pub mod home;
pub mod neighbours;
pub mod phones;
pub mod update;

#[cfg(test)]
mod tests;
