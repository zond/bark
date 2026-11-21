//! A node that answers as the spec's "Phone link" says, built on
//! `bark-link`'s codecs as the firmware is, and a radio that hears a few
//! of them, for the tests of everything above the transport.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use bark_link::{Node, Status};
use bark_proto::{NodeId, Position};

use crate::link::{id_text, Char, Error, Found, Link, Result, Transport};

/// A node: what it advertises and what it holds.
#[derive(Clone, Debug)]
pub struct FakeNode {
    pub rssi: i16,
    pub public: bool,
    pub node: Node,
    pub status: Status,
}

impl FakeNode {
    /// Public, on USB, placed, its link version this tool's.
    pub fn new(id: NodeId) -> FakeNode {
        FakeNode {
            rssi: -60,
            public: true,
            node: Node {
                version: bark_link::VERSION,
                oldest: bark_link::OLDEST,
                id,
                image: 1,
                room: 8,
            },
            status: Status {
                external_power: true,
                placed: true,
                address: Position::from_degrees(50.45, 30.52),
                relays: 3,
                heard: 4,
                ..Status::EMPTY
            },
        }
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
}

/// The radio: the nodes in reach, shared with their links.
pub struct FakeRadio {
    pub nodes: Vec<Rc<RefCell<FakeNode>>>,
    pub connects: usize,
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
            connects: 0,
            scan_ended_early: false,
        }
    }
}

impl Transport for FakeRadio {
    type Link = FakeLink;

    fn scan(
        &mut self,
        _: Duration,
        enough: &mut dyn FnMut(&[Found]) -> bool,
    ) -> Result<Vec<Found>> {
        self.scan_ended_early = false;
        let mut heard = Vec::new();
        for (i, n) in self.nodes.iter().enumerate() {
            let n = n.borrow();
            heard.push(Found {
                id: n.node.id,
                address: format!("fake-{i}"),
                rssi: Some(n.rssi),
                public: n.public,
            });
            if enough(&heard) {
                self.scan_ended_early = i + 1 < self.nodes.len();
                break;
            }
        }
        Ok(heard)
    }

    fn connect(&mut self, found: &Found) -> Result<FakeLink> {
        let node = self
            .nodes
            .iter()
            .find(|n| n.borrow().node.id == found.id)
            .ok_or_else(|| Error::Transport(format!("{} gone", id_text(found.id))))?;
        self.connects += 1;
        Ok(FakeLink { node: node.clone() })
    }
}

pub struct FakeLink {
    node: Rc<RefCell<FakeNode>>,
}

impl Link for FakeLink {
    fn mtu(&self) -> u16 {
        bark_link::ATT_MTU
    }

    fn read(&mut self, c: Char) -> Result<Vec<u8>> {
        let n = self.node.borrow();
        if !n.open(c) {
            return Err(Error::Att(0x05));
        }
        match c {
            Char::Node => Ok(n.node.encode().to_vec()),
            Char::Status => Ok(n.status.encode().to_vec()),
            _ => Err(Error::Other(format!("the fake does not read {c:?}"))),
        }
    }

    fn write(&mut self, c: Char, _: &[u8]) -> Result<()> {
        let n = self.node.borrow();
        if !n.open(c) {
            return Err(Error::Att(0x05));
        }
        Err(Error::Other(format!("the fake does not write {c:?}")))
    }

    fn disconnect(self) -> Result<()> {
        Ok(())
    }
}
