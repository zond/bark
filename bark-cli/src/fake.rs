//! A node that answers as the spec's "Phone link" says, built on
//! `bark-link`'s codecs as the firmware is, and a radio that hears a few
//! of them, for the tests of everything above the transport.

use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};
use std::rc::Rc;
use std::time::Duration;

use bark_link::neighbours::{self, Neighbour};
use bark_link::phones::{Keys, Phones, PhonesWrite};
use bark_link::{
    Answer, Ask, Clock, MapPoint, Node, NodeTime, Public, PublicWrite, Status, TimeSource,
};
use bark_proto::{NodeId, Position};

use crate::link::{id_text, Char, Error, Found, Link, Result, Transport};

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
    /// Scans call it public though it is not, as BlueZ keeps the UUIDs a
    /// device once advertised.
    pub looks_public: bool,
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
}

impl FakeNode {
    /// Public, on USB, placed, its link version this tool's, nobody
    /// paired.
    pub fn new(id: NodeId) -> FakeNode {
        let mut n = FakeNode {
            rssi: -60,
            public: true,
            looks_public: false,
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
                ..Status::EMPTY
            },
            code: 123_456,
            phones: Phones::EMPTY,
            floor_dbm: -110,
            neighbours: Vec::new(),
            answers: Vec::new(),
            clock: None,
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
        }
    }

    fn address(i: usize) -> String {
        format!("fake-{i}")
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
        for (i, n) in self.nodes.iter().enumerate() {
            let n = n.borrow();
            heard.push(Found {
                id: n.node.id,
                address: Self::address(i),
                rssi: Some(n.rssi),
                public: n.public || n.looks_public,
                bonded: self.bonds.borrow().contains(&Self::address(i)),
            });
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
            return Err(Error::NoDevice);
        }
        let node = self.nodes[i].clone();
        let address = Self::address(i);
        let bonded = self.bonds.borrow().contains(&address);
        let held = node.borrow().mine().is_some();
        Ok(FakeLink {
            node,
            bonds: self.bonds.clone(),
            address,
            owner: bonded && held,
            // A node that no longer holds this computer's bond ends the
            // link as soon as it is encrypted (spec "Phone link", The
            // removed phone).
            ended: bonded && !held,
            subscribed: HashSet::new(),
            queue: VecDeque::new(),
            neighbours_from: 0,
            phones_from: 0,
        })
    }

    fn bonded(&mut self, address: &str) -> Result<bool> {
        Ok(self.bonds.borrow().contains(address))
    }

    fn forget(&mut self, address: &str) -> Result<()> {
        self.bonds.borrow_mut().remove(address);
        Ok(())
    }
}

pub struct FakeLink {
    node: Rc<RefCell<FakeNode>>,
    bonds: Rc<RefCell<HashSet<String>>>,
    address: String,
    owner: bool,
    ended: bool,
    subscribed: HashSet<Char>,
    queue: VecDeque<(Char, Vec<u8>)>,
    neighbours_from: u8,
    phones_from: u8,
}

impl FakeLink {
    fn allowed(&self, c: Char) -> Result<()> {
        if self.ended {
            return Err(Error::Ended);
        }
        if self.owner || self.node.borrow().open(c) {
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
                    let i = n.mine().expect("an owner's link");
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
                _ => return Err(Error::Other("the fake removes no phone".into())),
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
        self.owner = true;
        Ok(())
    }

    fn disconnect(self) -> Result<()> {
        match self.ended {
            true => Err(Error::Ended),
            false => Ok(()),
        }
    }
}
