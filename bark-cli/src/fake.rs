//! A node that answers as the spec's "Phone link" says, built on
//! `bark-link`'s codecs as the firmware is, and a radio that hears a few
//! of them, for the tests of everything above the transport.

use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};
use std::rc::Rc;
use std::time::Duration;

use bark_link::home::{Home, SetHome};
use bark_link::neighbours::{self, Neighbour};
use bark_link::phones::{Keys, Phones, PhonesWrite};
use bark_link::update::{Command, Piece, Progress, Refusal, State as UpdateState};
use bark_link::{
    After, Answer, Ask, Clock, Confirm, GivenBack, GuestItem, MapPoint, Message, Node, NodeTime,
    Public, PublicWrite, Recipient, Send, Status, TimeSource,
};
use bark_proto::{NodeId, Position};

use crate::link::{id_text, Char, Error, Found, Link, Reach, Result, Transport};

/// This computer's identity address, as the node's records keep a phone's.
const ME: [u8; 6] = [1, 2, 3, 4, 5, 0xc6];

fn keys(addr: [u8; 6]) -> Keys {
    Keys {
        kind: 1,
        addr,
        irk: None,
        ltk: [7; 16],
        seen: bark_link::VERSION,
        service_changed: false,
    }
}

/// A node: what it advertises and what it holds.
#[derive(Clone, Debug)]
pub struct FakeNode {
    pub rssi: i16,
    pub public: bool,
    pub node: Node,
    pub status: Status,
    /// The passkey it shows when a pairing asks.
    pub code: u32,
    pub phones: Phones,
    pub floor_dbm: i8,
    /// Its neighbour table, a neighbour a slot.
    pub neighbours: Vec<Neighbour>,
    /// The neighbours that answer an ask, and what they say.
    pub answers: Vec<(NodeId, bark_proto::status::Status)>,
    pub clock: Option<Clock>,
    /// The ring: each item's number, the message ID it arrived with and its
    /// payload (spec "Mailbox", The ring).
    pub ring: Vec<(u32, bark_proto::MsgId, Vec<u8>)>,
    /// The number of the last of this computer's items it confirmed.
    pub confirmed: u32,
    /// The values written to Send that it took.
    pub sent: Vec<Vec<u8>>,
    /// Sends it answers not ready before it takes one.
    pub busy: usize,
    /// The message it gave back, if any, until it takes another.
    pub given_back: Option<GivenBack>,
    pub home: Option<[u8; 32]>,
    /// Where an update from the phone stands, and the bytes held.
    pub update: Progress,
    pub staged: Vec<u8>,
    staging: u32,
    /// Pieces it drops, as a full host queue does.
    pub drops: usize,
}

impl FakeNode {
    /// Public, on USB, placed, its link version this tool's, nobody
    /// paired.
    pub fn new(id: NodeId) -> FakeNode {
        let mut n = FakeNode {
            rssi: -60,
            public: true,
            node: Node {
                version: bark_link::VERSION,
                oldest: bark_link::OLDEST,
                id,
                image: 1,
                room: 0,
            },
            status: Status {
                external_power: true,
                placed: true,
                address: Position::from_degrees(50.45, 30.52),
                relays: 3,
                heard: 4,
                gps_cn0: 38,
                ..Status::EMPTY
            },
            code: 123_456,
            phones: Phones::EMPTY,
            floor_dbm: -110,
            neighbours: Vec::new(),
            answers: Vec::new(),
            clock: None,
            ring: Vec::new(),
            confirmed: 0,
            sent: Vec::new(),
            busy: 0,
            given_back: None,
            home: None,
            update: Progress::IDLE,
            staged: Vec::new(),
            staging: 0,
            drops: 0,
        };
        n.node.room = n.phones.room();
        n
    }

    pub fn rssi(mut self, rssi: i16) -> FakeNode {
        self.rssi = rssi;
        self
    }

    pub fn private(mut self) -> FakeNode {
        self.public = false;
        self
    }

    pub fn image(mut self, image: u32) -> FakeNode {
        self.node.image = image;
        self
    }

    pub fn versions(mut self, version: u8, oldest: u8) -> FakeNode {
        self.node.version = version;
        self.node.oldest = oldest;
        self
    }

    /// Another phone paired with it before this computer.
    pub fn with_other_phone(mut self) -> FakeNode {
        self.phones
            .pair(keys([9, 9, 9, 9, 9, 0xc9]), 0)
            .expect("room");
        self.phones
            .set_recipient(0, 0x42, bark_link::phones::Name::new(b"Olga").unwrap());
        self.node.room = self.phones.room();
        self
    }

    /// `n` neighbours, the ones with an even ID answering asks.
    pub fn with_neighbours(mut self, n: u64) -> FakeNode {
        for i in 1..=n {
            let id = NodeId::from_u64(0xbb00_0000_0000 + i);
            self.neighbours.push(Neighbour {
                id,
                position: Position::from_degrees(50.0 + i as f64 / 1000.0, 30.0),
                rssi_ddbm: -1000 + i as i16,
                age_s: i as u16,
                sleepy: i % 3 == 0,
                ..Neighbour::default()
            });
            if i % 2 == 0 {
                self.answers.push((
                    id,
                    bark_proto::status::Status {
                        placed: true,
                        gps_cn0: 21,
                        version: 46,
                        ..Default::default()
                    },
                ));
            }
        }
        self
    }

    /// The record of this computer's bond, if the node holds it.
    fn mine(&self) -> Option<usize> {
        self.phones.with_phone(1, &ME)
    }

    /// What a link without a bond may use on a public node
    /// (spec "Phone link", Public): the rest needs a bond.
    fn open(&self, c: Char) -> bool {
        self.public
            && matches!(
                c,
                Char::Node
                    | Char::Status
                    | Char::Send
                    | Char::Inbox
                    | Char::GivenBack
                    | Char::Recipient
                    | Char::Guest
            )
    }

    fn relisted(&mut self) {
        self.node.room = self.phones.room();
        self.status.phones = self.phones.listed();
    }
}

/// The radio: the nodes in reach, shared with their links, and the bonds
/// this computer holds.
pub struct FakeRadio {
    pub nodes: Vec<Rc<RefCell<FakeNode>>>,
    pub bonds: Rc<RefCell<HashSet<String>>>,
    pub connects: usize,
    pub scans: usize,
    /// Connects that time out before one goes through.
    pub failing: usize,
    /// Connects that find the device forgotten, before one goes through.
    pub lost: usize,
    /// The last scan stopped before its time.
    pub scan_ended_early: bool,
    /// The addresses whose devices the radio holds, heard or linked.
    pub held: HashSet<String>,
    /// Another program scans on the radio.
    pub others: bool,
    /// What the tool told the person.
    pub warnings: Vec<String>,
    /// Of each address, whether the radio has seen the public node's UUID
    /// and the service's, kept for ever as BlueZ keeps them.
    pub uuids: std::collections::HashMap<String, (bool, bool)>,
}

impl FakeRadio {
    pub fn new(nodes: Vec<FakeNode>) -> FakeRadio {
        FakeRadio {
            nodes: nodes
                .into_iter()
                .map(|n| Rc::new(RefCell::new(n)))
                .collect(),
            bonds: Rc::default(),
            connects: 0,
            scans: 0,
            failing: 0,
            lost: 0,
            scan_ended_early: false,
            held: HashSet::new(),
            others: false,
            warnings: Vec::new(),
            uuids: Default::default(),
        }
    }

    fn address(i: usize) -> String {
        format!("fake-{i}")
    }

    fn found(&self, i: usize, n: &FakeNode) -> Found {
        Found {
            id: n.node.id,
            address: Self::address(i),
            rssi: Some(n.rssi),
            public: self
                .uuids
                .get(&Self::address(i))
                .map_or(Reach::Unknown, |&(p, s)| Reach::of_uuids(p, s)),
            bonded: self.bonds.borrow().contains(&Self::address(i)),
        }
    }
}

impl Transport for FakeRadio {
    type Link = FakeLink;

    fn scan(
        &mut self,
        time: Duration,
        enough: &mut dyn FnMut(&[Found], Duration) -> bool,
    ) -> Result<Vec<Found>> {
        self.scans += 1;
        self.scan_ended_early = false;
        let mut heard = Vec::new();
        // A node is heard an advertising interval after the one before.
        let step = time / (self.nodes.len() as u32 + 1);
        for (i, n) in self.nodes.clone().iter().enumerate() {
            self.held.insert(Self::address(i));
            // What it advertises now joins what was seen before.
            let public = n.borrow().public;
            let seen = self.uuids.entry(Self::address(i)).or_default();
            seen.0 |= public;
            seen.1 |= !public;
            heard.push(self.found(i, &n.borrow()));
            if enough(&heard, step * (i as u32 + 1)) {
                self.scan_ended_early = i + 1 < self.nodes.len();
                break;
            }
        }
        if !self.scan_ended_early {
            enough(&heard, time);
        }
        Ok(heard)
    }

    fn connect(&mut self, found: &Found) -> Result<FakeLink> {
        let i = self
            .nodes
            .iter()
            .position(|n| n.borrow().node.id == found.id)
            .ok_or_else(|| Error::Transport(format!("{} gone", id_text(found.id))))?;
        self.connects += 1;
        if self.failing > 0 {
            self.failing -= 1;
            return Err(Error::TimedOut("no connection within 70 s".into()));
        }
        if self.lost > 0 {
            self.lost -= 1;
            self.held.remove(&Self::address(i));
            return Err(Error::NoDevice);
        }
        let node = self.nodes[i].clone();
        // A link finds the service, which the radio keeps too.
        self.uuids.entry(Self::address(i)).or_default().1 = true;
        let address = Self::address(i);
        let bonded = self.bonds.borrow().contains(&address);
        let held = node.borrow().mine().is_some();
        Ok(FakeLink {
            node,
            bonds: self.bonds.clone(),
            address,
            paired: bonded && held,
            // A node that no longer holds this computer's bond ends the
            // link as soon as it is encrypted (spec "Phone link", The
            // removed phone).
            ended: bonded && !held,
            subscribed: HashSet::new(),
            queue: VecDeque::new(),
            neighbours_from: 0,
            phones_from: 0,
            code_drawn: false,
            tag: None,
            after: 0,
        })
    }

    fn bonded(&mut self, address: &str) -> Result<bool> {
        Ok(self.bonds.borrow().contains(address))
    }

    fn forget(&mut self, address: &str) -> Result<()> {
        self.bonds.borrow_mut().remove(address);
        self.held.remove(address);
        Ok(())
    }

    fn known(&mut self) -> Result<Vec<Found>> {
        let mut found = Vec::new();
        for (i, n) in self.nodes.iter().enumerate() {
            if self.held.contains(&Self::address(i)) {
                found.push(self.found(i, &n.borrow()));
            }
        }
        Ok(found)
    }

    fn others_scanning(&mut self) -> Result<bool> {
        Ok(self.others)
    }

    fn warn(&mut self, line: &str) {
        self.warnings.push(line.to_string());
    }
}

pub struct FakeLink {
    node: Rc<RefCell<FakeNode>>,
    bonds: Rc<RefCell<HashSet<String>>>,
    address: String,
    paired: bool,
    ended: bool,
    subscribed: HashSet<Char>,
    queue: VecDeque<(Char, Vec<u8>)>,
    neighbours_from: u8,
    phones_from: u8,
    /// A code for a change to the phones is drawn and not yet tried.
    code_drawn: bool,
    /// The recipient tag the link wrote.
    tag: Option<u8>,
    /// What a guest wrote to Guest.
    after: u32,
}

impl FakeLink {
    fn allowed(&self, c: Char) -> Result<()> {
        if self.ended {
            return Err(Error::Ended);
        }
        if self.paired || self.node.borrow().open(c) {
            Ok(())
        } else {
            Err(Error::Att(0x05))
        }
    }
}

fn taken<T>(r: std::result::Result<T, bark_link::Malformed>) -> Result<T> {
    r.map_err(|m| Error::Att(m.att_error()))
}

impl Link for FakeLink {
    fn mtu(&self) -> u16 {
        bark_link::ATT_MTU
    }

    fn read(&mut self, c: Char) -> Result<Vec<u8>> {
        self.allowed(c)?;
        let n = self.node.borrow();
        match c {
            Char::Node => Ok(n.node.encode().to_vec()),
            Char::Status => Ok(n.status.encode().to_vec()),
            Char::Public => Ok(Public(n.public).encode().to_vec()),
            Char::Clock => Ok(match n.clock {
                Some(c) => NodeTime {
                    utc_s: c.utc_s,
                    offset_min: c.offset_min,
                    source: TimeSource::Phone,
                },
                None => NodeTime {
                    utc_s: 0,
                    offset_min: 0,
                    source: TimeSource::None,
                },
            }
            .encode()
            .to_vec()),
            Char::Neighbours => {
                let from = usize::from(self.neighbours_from);
                let page: Vec<Neighbour> = n
                    .neighbours
                    .iter()
                    .skip(from)
                    .take(neighbours::PER_PAGE)
                    .copied()
                    .collect();
                let after = from + page.len();
                let header = neighbours::Header {
                    next: if after < n.neighbours.len() {
                        after as u8
                    } else {
                        0
                    },
                    floor_dbm: n.floor_dbm,
                    own: n.status.address,
                };
                let mut b = [0; neighbours::MAX_LEN];
                let len = neighbours::encode(&header, &page, &mut b);
                Ok(b[..len].to_vec())
            }
            Char::Update => Ok(n.update.encode().to_vec()),
            Char::GivenBack => Ok(n.given_back.map_or(Vec::new(), |g| g.encode().to_vec())),
            Char::Home => Ok(Home {
                set: n.home.is_some(),
                mine: n.home.is_some(),
                oldest: n.ring.first().map_or(0, |i| i.0),
                newest: n.ring.last().map_or(0, |i| i.0),
            }
            .encode()
            .to_vec()),
            Char::Inbox => {
                let Some(tag) = self.tag else {
                    return Ok(Vec::new());
                };
                let ours = |after: u32| {
                    n.ring.iter().find(|i| {
                        i.0 > after && bark_app_layer::seal::recipient_of(&i.2) == Some(tag)
                    })
                };
                if self.paired {
                    let Some((_, id, p)) = ours(n.confirmed) else {
                        return Ok(Vec::new());
                    };
                    let mut b = [0; Message::MAX_LEN];
                    let len = Message {
                        msg_id: *id,
                        payload: p,
                    }
                    .encode(&mut b);
                    Ok(b[..len].to_vec())
                } else {
                    let Some((number, id, p)) = ours(self.after) else {
                        return Ok(Vec::new());
                    };
                    let mut b = [0; GuestItem::MAX_LEN];
                    let len = GuestItem {
                        number: *number,
                        msg_id: *id,
                        payload: p,
                    }
                    .encode(&mut b);
                    Ok(b[..len].to_vec())
                }
            }
            Char::Phones => {
                let mut b = [0; bark_link::phones::MAX_LEN];
                let len = n.phones.page(
                    self.phones_from,
                    n.mine(),
                    1 << n.mine().unwrap_or(0),
                    &mut b,
                );
                Ok(b[..len].to_vec())
            }
            _ => Err(Error::Other(format!("the fake does not read {c:?}"))),
        }
    }

    fn write(&mut self, c: Char, v: &[u8]) -> Result<()> {
        self.allowed(c)?;
        let mut n = self.node.borrow_mut();
        match c {
            Char::Public => match taken(PublicWrite::decode(v))? {
                PublicWrite::Set(p) => n.public = p.0,
                PublicWrite::Unpair => {
                    let i = n.mine().expect("a paired phone's link");
                    n.phones.unpair(i);
                    n.relisted();
                    // The node answers, then ends the link.
                    self.ended = true;
                }
            },
            Char::Clock => n.clock = Some(taken(Clock::decode(v))?),
            Char::MapPoint => match taken(MapPoint::decode(v))? {
                MapPoint::Set(p) => {
                    n.status.map_point = true;
                    n.status.placed = true;
                    n.status.address = p;
                }
                MapPoint::Clear => n.status.map_point = false,
            },
            Char::Neighbours => self.neighbours_from = taken(neighbours::decode_from(v))?,
            Char::Phones => match taken(PhonesWrite::decode(v))? {
                PhonesWrite::From(s) => self.phones_from = s,
                PhonesWrite::Code => self.code_drawn = true,
                PhonesWrite::Remove { .. } | PhonesWrite::Owner { .. } if !self.code_drawn => {
                    return Err(Error::Att(bark_link::att::REFUSED));
                }
                w @ (PhonesWrite::Remove { code, .. } | PhonesWrite::Owner { code, .. }) => {
                    // One try for each code drawn.
                    self.code_drawn = false;
                    if code != n.code {
                        return Err(Error::Att(bark_link::att::REFUSED));
                    }
                    let done = match w {
                        PhonesWrite::Remove { number, parts, .. } => {
                            n.phones.remove(number, parts).is_some()
                        }
                        PhonesWrite::Owner { number, .. } => n.phones.make_owner(number).is_some(),
                        _ => unreachable!("a change"),
                    };
                    if !done {
                        return Err(Error::Att(bark_link::att::VALUE_NOT_ALLOWED));
                    }
                    n.relisted();
                }
            },
            Char::Update => {
                let refuse = |n: &mut FakeNode, why| {
                    n.update.refused = Some(why);
                    Err(Error::Att(bark_link::att::REFUSED))
                };
                match taken(Command::decode(v))? {
                    Command::Begin(h) => {
                        let word =
                            |at: usize| u32::from_le_bytes(h[at..at + 4].try_into().unwrap());
                        if &h[..4] != b"bark" {
                            return refuse(&mut n, Refusal::Header);
                        }
                        if word(8) <= n.node.image {
                            return refuse(&mut n, Refusal::Version);
                        }
                        n.staged.clear();
                        n.staging = word(12);
                        n.update = Progress {
                            state: UpdateState::Receiving,
                            refused: None,
                            held: 0,
                            version: word(8),
                            hash: h[16..48].try_into().unwrap(),
                        };
                    }
                    Command::Commit => {
                        if n.update.state != UpdateState::Staged {
                            return refuse(&mut n, Refusal::Incomplete);
                        }
                        use sha2::Digest;
                        if sha2::Sha256::digest(&n.staged)[..] != n.update.hash {
                            n.update = Progress::IDLE;
                            return refuse(&mut n, Refusal::Hash);
                        }
                        n.update.state = UpdateState::Committed;
                    }
                    Command::Reboot => {
                        if n.update.state == UpdateState::Committed {
                            n.node.image = n.update.version;
                            n.update = Progress::IDLE;
                        }
                        // It answers, then resets.
                        self.ended = true;
                    }
                }
            }
            Char::Recipient => {
                let rec = taken(Recipient::decode(v))?;
                if !rec.compatible() {
                    self.ended = true;
                    return Err(Error::Att(bark_link::att::INCOMPATIBLE));
                }
                self.tag = Some(rec.tag);
                if let (true, Some(i)) = (self.paired, n.mine()) {
                    n.phones.set_recipient(i, rec.tag, rec.name);
                    n.relisted();
                }
            }
            Char::Send => {
                taken(Send::decode(v))?;
                if n.busy > 0 {
                    n.busy -= 1;
                    if self.subscribed.contains(&Char::Status) {
                        let ready = Status {
                            ready: true,
                            ..n.status
                        };
                        self.queue
                            .push_back((Char::Status, ready.encode().to_vec()));
                    }
                    return Err(Error::Att(bark_link::att::NOT_READY));
                }
                n.sent.push(v.to_vec());
                // What it gave back before is over (spec "Sending").
                n.given_back = None;
            }
            Char::Inbox => {
                let Confirm(id) = taken(Confirm::decode(v))?;
                let tag = self.tag;
                let next = n
                    .ring
                    .iter()
                    .find(|i| i.0 > n.confirmed && bark_app_layer::seal::recipient_of(&i.2) == tag)
                    .map(|i| (i.0, i.1));
                match next {
                    Some((number, msg)) if self.paired && msg == id => n.confirmed = number,
                    _ => return Err(Error::Att(bark_link::att::VALUE_NOT_ALLOWED)),
                }
            }
            Char::Guest => self.after = taken(After::decode(v))?.0,
            Char::Home => match taken(SetHome::decode(v))? {
                SetHome::Set { key, sent } => {
                    n.home = Some(key);
                    if let Some(i) = n.mine() {
                        n.phones.set_home(i, Some(key), sent);
                        n.relisted();
                    }
                }
                SetHome::Clear => {
                    n.home = None;
                    if let Some(i) = n.mine() {
                        n.phones.set_home(i, None, 0);
                        n.relisted();
                    }
                }
            },
            Char::Ask => {
                let Ask(id) = taken(Ask::decode(v))?;
                let answer = n.answers.iter().find(|a| a.0 == id);
                if let (Some(&(from, status)), true) =
                    (answer, self.subscribed.contains(&Char::Ask))
                {
                    self.queue
                        .push_back((Char::Ask, Answer { from, status }.encode().to_vec()));
                }
            }
            _ => return Err(Error::Other(format!("the fake does not write {c:?}"))),
        }
        Ok(())
    }

    fn write_command(&mut self, c: Char, v: &[u8]) -> Result<()> {
        self.allowed(c)?;
        let mut n = self.node.borrow_mut();
        if c != Char::Image {
            return Err(Error::Other(format!("the fake takes no command on {c:?}")));
        }
        if n.drops > 0 {
            n.drops -= 1;
            return Ok(());
        }
        let piece = taken(Piece::decode(v))?;
        // A piece only where the bytes held end
        // (spec "Updates from the phone").
        if n.update.state == UpdateState::Receiving && piece.offset == n.update.held {
            n.staged.extend_from_slice(piece.bytes);
            n.update.held += piece.bytes.len() as u32;
            if n.update.held == n.staging {
                n.update.state = UpdateState::Staged;
            }
        }
        Ok(())
    }

    fn subscribe(&mut self, c: Char) -> Result<()> {
        self.allowed(c)?;
        self.subscribed.insert(c);
        Ok(())
    }

    fn notified(&mut self, _: Duration) -> Result<Option<(Char, Vec<u8>)>> {
        if self.ended {
            return Err(Error::Ended);
        }
        Ok(self.queue.pop_front())
    }

    fn pair(&mut self, code: Option<u32>) -> Result<()> {
        if self.ended {
            return Err(Error::Ended);
        }
        let mut n = self.node.borrow_mut();
        if code != Some(n.code) {
            return Err(Error::Transport(
                "org.bluez.Error.AuthenticationFailed Authentication Failed".into(),
            ));
        }
        if n.phones.pair(keys(ME), 0).is_err() {
            return Err(Error::Transport(
                "org.bluez.Error.AuthenticationFailed Authentication Failed".into(),
            ));
        }
        n.relisted();
        self.bonds.borrow_mut().insert(self.address.clone());
        self.paired = true;
        Ok(())
    }

    fn disconnect(self) -> Result<()> {
        match self.ended {
            true => Err(Error::Ended),
            false => Ok(()),
        }
    }
}
