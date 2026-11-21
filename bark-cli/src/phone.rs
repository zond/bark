//! What the tool does with a node, as the app does it, above any
//! transport (spec "Phone link").

use std::str::FromStr;
use std::time::Duration;

use bark_link::{Node, Status};
use bark_proto::NodeId;

use crate::link::{id_text, Error, Found, Link, Result, Transport};

/// How long a scan looks for nodes: ten of a node's advertising intervals
/// of 1022.5 ms (spec "Phone link", Finding the node), so that one whose
/// advertisements are missed now and then is still heard, as the app's
/// look for one node waits.
pub const SCAN_TIME: Duration = Duration::from_micros(10 * 1_022_500);

/// A node as the person names it: its ID's twelve hex digits, or the last
/// few of them, as long as one node heard ends so.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Select(String);

impl FromStr for Select {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Select, String> {
        let s = s.strip_prefix(bark_link::NAME_PREFIX).unwrap_or(s);
        let whole = 2 * NodeId::LEN;
        if s.is_empty() || s.len() > whole || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!(
                "{s:?} is not a node's ID: up to {whole} hex digits"
            ));
        }
        Ok(Select(s.to_ascii_lowercase()))
    }
}

impl Select {
    pub fn matches(&self, id: NodeId) -> bool {
        id_text(id).ends_with(&self.0)
    }

    /// Names one node by all of its ID, so the first heard is the one.
    fn whole(&self) -> bool {
        self.0.len() == 2 * NodeId::LEN
    }
}

/// The nodes in reach, strongest first.
pub fn scan<T: Transport>(t: &mut T, time: Duration) -> Result<Vec<Found>> {
    let mut found = t.scan(time, &mut |_| false)?;
    found.sort_by(|a, b| b.rssi.cmp(&a.rssi).then(a.id.0.cmp(&b.id.0)));
    Ok(found)
}

/// The one node `sel` names among those in reach. A whole ID ends the
/// scan as soon as its node is heard; a part of one waits the whole
/// scan, to know that no other node ends so too.
pub fn find<T: Transport>(t: &mut T, sel: &Select, time: Duration) -> Result<Found> {
    let found = t.scan(time, &mut |f| {
        sel.whole() && f.iter().any(|f| sel.matches(f.id))
    })?;
    let mut matching: Vec<Found> = found.into_iter().filter(|f| sel.matches(f.id)).collect();
    match matching.len() {
        0 => Err(Error::Other(format!(
            "no node {} in reach: come nearer and try again",
            sel.0
        ))),
        1 => Ok(matching.remove(0)),
        _ => {
            let ids: Vec<String> = matching.iter().map(|f| id_text(f.id)).collect();
            Err(Error::Other(format!(
                "{} names {}: give more of the ID",
                sel.0,
                ids.join(", ")
            )))
        }
    }
}

/// Connects to `node` as a guest, which a node takes only while it is
/// public (spec "Phone link", Public). A private node refuses a guest's
/// read for want of a bond, at which BlueZ starts a pairing by itself, so
/// a guest does not connect to one.
pub fn guest<T: Transport>(t: &mut T, node: &Found) -> Result<T::Link> {
    if !node.public {
        return Err(Error::Other(format!(
            "{} is private: only its paired phones use it",
            id_text(node.id)
        )));
    }
    t.connect(node)
}

/// Node, the link's first read: the node, and whether it and this tool
/// serve each other (spec "Phone link", Versions), whose answer the node
/// gives too as the phone writes Recipient.
pub fn node<L: Link>(l: &mut L) -> Result<Node> {
    let n = Node::decode(&l.read(crate::link::Char::Node)?)
        .map_err(|_| Error::Other("the node's Node value is too short".into()))?;
    Ok(n)
}

/// Whether the node `n` and this tool serve each other; if not, which of
/// them to update.
pub fn served(n: &Node) -> Result<()> {
    if bark_link::compatible(n.version, n.oldest) {
        return Ok(());
    }
    let side = if n.version < bark_link::OLDEST {
        "the node needs newer firmware"
    } else {
        "the node needs a newer tool"
    };
    Err(Error::Other(format!(
        "{side}: it speaks link version {}, oldest {}; this tool {}, oldest {}",
        n.version,
        n.oldest,
        bark_link::VERSION,
        bark_link::OLDEST
    )))
}

/// Status, after Node, as the app reads it at a connection.
pub fn status<L: Link>(l: &mut L) -> Result<Status> {
    served(&node(l)?)?;
    Status::decode(&l.read(crate::link::Char::Status)?)
        .map_err(|_| Error::Other("the node's Status value is too short".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::{FakeNode, FakeRadio};

    fn id(n: u64) -> NodeId {
        NodeId::from_u64(n)
    }

    #[test]
    fn a_selection_is_hex_digits_of_an_id() {
        assert!("".parse::<Select>().is_err());
        assert!("12g".parse::<Select>().is_err());
        assert!("1234567890abc".parse::<Select>().is_err());
        let s: Select = "bark-ABCDEF".parse().unwrap();
        assert!(s.matches(id(0x0000_00ab_cdef)));
        assert!(!s.matches(id(0x0000_00ab_cdee)));
    }

    #[test]
    fn a_scan_lists_strongest_first_and_says_public() {
        let mut radio = FakeRadio::new(vec![
            FakeNode::new(id(0xfa4e_0000_0001)).rssi(-80),
            FakeNode::new(id(0xfa4e_0000_0002)).rssi(-50).private(),
        ]);
        let found = scan(&mut radio, SCAN_TIME).unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].id, id(0xfa4e_0000_0002));
        assert!(!found[0].public);
        assert!(found[1].public);
    }

    #[test]
    fn find_takes_a_unique_suffix_and_refuses_an_ambiguous_one() {
        let mut radio = FakeRadio::new(vec![
            FakeNode::new(id(0xfa4e_0000_0011)),
            FakeNode::new(id(0xfa4e_0000_0021)),
        ]);
        let f = find(&mut radio, &"11".parse().unwrap(), SCAN_TIME).unwrap();
        assert_eq!(f.id, id(0xfa4e_0000_0011));
        let e = find(&mut radio, &"1".parse().unwrap(), SCAN_TIME).unwrap_err();
        assert!(e.to_string().contains("give more"), "{e}");
        let e = find(&mut radio, &"31".parse().unwrap(), SCAN_TIME).unwrap_err();
        assert!(e.to_string().contains("no node 31"), "{e}");
    }

    #[test]
    fn a_whole_id_ends_the_scan_at_its_node() {
        let mut radio = FakeRadio::new(vec![
            FakeNode::new(id(0xfa4e_0000_0001)),
            FakeNode::new(id(0xfa4e_0000_0002)),
        ]);
        find(&mut radio, &"fa4e00000001".parse().unwrap(), SCAN_TIME).unwrap();
        assert!(radio.scan_ended_early);
    }

    #[test]
    fn a_guest_reads_node_and_status() {
        let n = FakeNode::new(id(0xfa4e_0000_0001)).image(46);
        let mut radio = FakeRadio::new(vec![n]);
        let f = find(&mut radio, &"01".parse().unwrap(), SCAN_TIME).unwrap();
        let mut l = guest(&mut radio, &f).unwrap();
        let read = node(&mut l).unwrap();
        assert_eq!(read.id, id(0xfa4e_0000_0001));
        assert_eq!(read.image, 46);
        let s = status(&mut l).unwrap();
        assert!(s.placed && s.external_power);
    }

    #[test]
    fn a_guest_does_not_connect_to_a_private_node() {
        let mut radio = FakeRadio::new(vec![FakeNode::new(id(1)).private()]);
        let f = find(&mut radio, &"1".parse().unwrap(), SCAN_TIME).unwrap();
        let e = guest(&mut radio, &f).err().unwrap();
        assert!(e.to_string().contains("private"), "{e}");
        assert_eq!(radio.connects, 0);
    }

    #[test]
    fn a_node_of_another_link_version_is_not_served() {
        let mut radio = FakeRadio::new(vec![FakeNode::new(id(1)).versions(9, 9)]);
        let f = find(&mut radio, &"1".parse().unwrap(), SCAN_TIME).unwrap();
        let mut l = guest(&mut radio, &f).unwrap();
        let e = status(&mut l).unwrap_err();
        assert!(e.to_string().contains("newer tool"), "{e}");
        let mut radio = FakeRadio::new(vec![FakeNode::new(id(1)).versions(0, 0)]);
        let f = find(&mut radio, &"1".parse().unwrap(), SCAN_TIME).unwrap();
        let mut l = guest(&mut radio, &f).unwrap();
        let e = status(&mut l).unwrap_err();
        assert!(e.to_string().contains("newer firmware"), "{e}");
    }
}
