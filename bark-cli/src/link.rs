//! What the tool needs of Bluetooth: find nodes, connect to one, and read
//! and write the service's characteristics
//! (spec "Phone link", The service). BlueZ gives it on the laptop
//! (`bluez.rs`), and a fake node in the tests.

use std::fmt;
use std::time::Duration;

use bark_link::uuid;
use bark_proto::NodeId;

/// The service's characteristics, in the order of the spec's table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Char {
    Node,
    Status,
    Send,
    Inbox,
    GivenBack,
    MapPoint,
    Clock,
    Home,
    Neighbours,
    Update,
    Image,
    Recipient,
    Public,
    Guest,
    Ask,
    Phones,
}

impl Char {
    /// Every characteristic.
    pub const ALL: [Char; 16] = [
        Char::Node,
        Char::Status,
        Char::Send,
        Char::Inbox,
        Char::GivenBack,
        Char::MapPoint,
        Char::Clock,
        Char::Home,
        Char::Neighbours,
        Char::Update,
        Char::Image,
        Char::Recipient,
        Char::Public,
        Char::Guest,
        Char::Ask,
        Char::Phones,
    ];

    pub fn uuid(self) -> u128 {
        match self {
            Char::Node => uuid::NODE,
            Char::Status => uuid::STATUS,
            Char::Send => uuid::SEND,
            Char::Inbox => uuid::INBOX,
            Char::GivenBack => uuid::GIVEN_BACK,
            Char::MapPoint => uuid::MAP_POINT,
            Char::Clock => uuid::CLOCK,
            Char::Home => uuid::HOME,
            Char::Neighbours => uuid::NEIGHBOURS,
            Char::Update => uuid::UPDATE,
            Char::Image => uuid::IMAGE,
            Char::Recipient => uuid::RECIPIENT,
            Char::Public => uuid::PUBLIC,
            Char::Guest => uuid::GUEST,
            Char::Ask => uuid::ASK,
            Char::Phones => uuid::PHONES,
        }
    }
}

/// A UUID as text: 8-4-4-4-12 lowercase hex digits.
pub fn uuid_text(u: u128) -> String {
    let h = format!("{u:032x}");
    format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    )
}

/// A node heard in a scan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    /// From its name, `bark-` and the ID's twelve hex digits.
    pub id: NodeId,
    /// Its Bluetooth address, as the transport names it.
    pub address: String,
    /// The strongest RSSI it was heard at, in dBm.
    pub rssi: Option<i16>,
    /// Whether it serves guests, as far as its advertising tells
    /// (spec "Phone link", Public).
    pub public: Reach,
    /// This computer holds a bond with it.
    pub bonded: bool,
}

/// Whether a node serves guests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    Public,
    Private,
    /// What the transport holds of its advertising cannot tell.
    Unknown,
}

impl Reach {
    /// What a device's UUIDs tell, as a transport that keeps every UUID a
    /// device was ever seen with, and those its services have, holds them:
    /// the public node's UUID, which no attribute has, alone is a public
    /// node, the service's alone a private one (spec "Phone link",
    /// Public); with both it may be either now.
    pub fn of_uuids(public_node: bool, service: bool) -> Reach {
        match (public_node, service) {
            (true, false) => Reach::Public,
            (false, true) => Reach::Private,
            _ => Reach::Unknown,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Reach::Public => "public",
            Reach::Private => "private",
            Reach::Unknown => "unknown",
        }
    }
}

/// The node's ID from its advertised name, or none for a name that is not
/// a node's.
pub fn id_of_name(name: &str) -> Option<NodeId> {
    let hex = name.strip_prefix(bark_link::NAME_PREFIX)?;
    if hex.len() != 2 * NodeId::LEN || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let n = u64::from_str_radix(hex, 16).ok()?;
    Some(NodeId::from_u64(n))
}

/// The ID as the node's display and name show it.
pub fn id_text(id: NodeId) -> String {
    format!("{:012x}", id.to_u64())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The node answered with this ATT error (Vol 3 Part F §3.4.1.1).
    Att(u8),
    /// The link ended.
    Ended,
    /// A step took longer than the tool waits for it.
    TimedOut(String),
    /// The transport no longer holds the device to connect to: it is to
    /// be heard in a scan again.
    NoDevice,
    /// What the transport said, verbatim.
    Transport(String),
    /// Anything else the tool cannot go on from.
    Other(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Att(code) => write!(f, "the node answered ATT error 0x{code:02x}"),
            Error::Ended => f.write_str("the link ended"),
            Error::NoDevice => f.write_str("Bluetooth no longer knows the node"),
            Error::TimedOut(s) | Error::Transport(s) | Error::Other(s) => f.write_str(s),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// The radio: finds nodes and connects to them.
pub trait Transport {
    type Link: Link;

    /// Listens for `time`, or until `enough`, asked as nodes are heard and
    /// as time passes, says the nodes heard so far suffice, given how long
    /// the scan has run; every node heard, each once.
    fn scan(
        &mut self,
        time: Duration,
        enough: &mut dyn FnMut(&[Found], Duration) -> bool,
    ) -> Result<Vec<Found>>;

    /// Connects to a node heard in a scan, or one this computer holds a
    /// bond with, with the service's characteristics found and the MTU
    /// exchanged. An attempt that fails leaves nothing pending.
    fn connect(&mut self, node: &Found) -> Result<Self::Link>;

    /// Whether this computer holds a bond with the device at `address`.
    fn bonded(&mut self, address: &str) -> Result<bool>;

    /// Forgets the device at `address`, its bond too.
    fn forget(&mut self, address: &str) -> Result<()>;

    /// The nodes whose devices the transport holds now, heard lately
    /// enough that it connects to them without a scan.
    fn known(&mut self) -> Result<Vec<Found>>;

    /// Whether another program is scanning on the same radio, which the
    /// radio's connects then share their time with.
    fn others_scanning(&mut self) -> Result<bool>;

    /// Tells the person something that does not stop the command.
    fn warn(&mut self, line: &str);
}

/// One connection to a node.
pub trait Link {
    /// The ATT MTU agreed.
    fn mtu(&self) -> u16;
    fn read(&mut self, c: Char) -> Result<Vec<u8>>;
    /// A write request, which the node answers.
    fn write(&mut self, c: Char, value: &[u8]) -> Result<()>;
    /// A write without response, which the node does not answer.
    fn write_command(&mut self, c: Char, value: &[u8]) -> Result<()>;
    /// Subscribes to the characteristic's notifications.
    fn subscribe(&mut self, c: Char) -> Result<()>;
    /// The next value notified within `wait`, if any.
    fn notified(&mut self, wait: Duration) -> Result<Option<(Char, Vec<u8>)>>;
    /// Pairs over this link by passkey entry: the node shows the code and
    /// this side types it (spec "Phone link", Pairing). `code` is the
    /// code, or none to ask the person for it.
    fn pair(&mut self, code: Option<u32>) -> Result<()>;
    fn disconnect(self) -> Result<()>;
}
