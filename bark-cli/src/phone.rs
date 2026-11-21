//! What the tool does with a node, as the app does it, above any
//! transport (spec "Phone link").

use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use bark_link::neighbours::{self, Header, Neighbour};
use bark_link::phones::{self, PhonesWrite, RECORDS};
use bark_link::{Answer, Ask, Clock, MapPoint, Node, NodeTime, Public, PublicWrite, Status};
use bark_proto::{NodeId, Position};

use crate::link::{id_text, Char, Error, Found, Link, Reach, Result, Transport};
use crate::state::{Paired, State};

/// A node's advertising interval, 1022.5 ms (spec "Phone link", Finding
/// the node).
const ADVERTISING_US: u64 = 1_022_500;

/// How long a scan looks for nodes: ten advertising intervals, so that a
/// node whose advertisements are missed now and then is still heard, as
/// the app's look for one node waits.
pub const SCAN_TIME: Duration = Duration::from_micros(10 * ADVERTISING_US);

/// How long a scan for a part of an ID listens on after it first hears a
/// node that ends so, for another that ends so too: two advertising
/// intervals, in which a node in reach that the radio hears well
/// advertises twice.
const SETTLE: Duration = Duration::from_micros(2 * ADVERTISING_US);

/// Attempts to connect before the tool gives up: the first and one more, a
/// choice. Each is long, as long as BlueZ's own (`bluez.rs`); the second
/// is for an attempt BlueZ ended early.
const CONNECT_TRIES: usize = 2;

/// How long the tool waits for an answer to an ask before it asks again,
/// and how many asks it makes: the app's (`bark-app/lib/asks.dart`), 2 s
/// for an ask and its answer over a loaded channel, three asks.
const ASK_WAIT: Duration = Duration::from_secs(2);
const ASK_ROUNDS: usize = 3;

/// How long the tool waits for the link to end after an unpairing write
/// that failed: the longest supervision timeout, 32 s (Vol 6 Part B
/// §4.5.2), by which a link the node ended is over for this side too, as
/// the app waits.
const UNPAIR_END: Duration = Duration::from_secs(32);

/// Reads of the phones list that may each find it changed before the tool
/// gives up on a still list.
const PHONES_TRIES: usize = 4;

/// A node as the person names it: its ID's twelve hex digits, or the last
/// few of them, as long as one node ends so.
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

impl fmt::Display for Select {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Select {
    pub fn matches(&self, id: NodeId) -> bool {
        id_text(id).ends_with(&self.0)
    }

    /// Names one node by all of its ID.
    fn whole(&self) -> Option<NodeId> {
        (self.0.len() == 2 * NodeId::LEN)
            .then(|| NodeId::from_u64(u64::from_str_radix(&self.0, 16).expect("hex digits")))
    }
}

/// The nodes in reach, strongest first.
pub fn scan<T: Transport>(t: &mut T, time: Duration) -> Result<Vec<Found>> {
    let mut found = t.scan(time, &mut |_, _| false)?;
    found.sort_by(|a, b| b.rssi.cmp(&a.rssi).then(a.id.0.cmp(&b.id.0)));
    Ok(found)
}

/// The one node `sel` names among those in reach. A whole ID ends the
/// scan as soon as its node is heard; a part of one [`SETTLE`] after a
/// node that ends so was heard, if no other has been.
pub fn find<T: Transport>(t: &mut T, sel: &Select, time: Duration) -> Result<Found> {
    // A node the transport holds, the only one `sel` names among them, is
    // connected to without a scan: it was heard lately.
    let mut held: Vec<Found> = t
        .known()?
        .into_iter()
        .filter(|f| sel.matches(f.id))
        .collect();
    if held.len() == 1 {
        return Ok(held.remove(0));
    }
    let mut first = None;
    let found = t.scan(time, &mut |f, ran| {
        let n = f.iter().filter(|f| sel.matches(f.id)).count();
        let since = *first.get_or_insert(ran);
        if n == 0 {
            first = None;
        }
        n == 1 && (sel.whole().is_some() || ran.saturating_sub(since) >= SETTLE)
    })?;
    let mut matching: Vec<Found> = found.into_iter().filter(|f| sel.matches(f.id)).collect();
    match matching.len() {
        0 => Err(Error::Other(format!(
            "no node {sel} heard in {:.1} s: come nearer, or listen longer (--scan-time 30)",
            time.as_secs_f64()
        ))),
        1 => Ok(matching.remove(0)),
        _ => {
            let ids: Vec<String> = matching.iter().map(|f| id_text(f.id)).collect();
            Err(Error::Other(format!(
                "{sel} names {}: give more of the ID",
                ids.join(", ")
            )))
        }
    }
}

/// What may go better at another try: the radio, not the node's answer.
fn retried(e: &Error) -> bool {
    matches!(
        e,
        Error::TimedOut(_) | Error::Transport(_) | Error::Ended | Error::NoDevice
    )
}

/// What the person is told when another program scans on the radio: its
/// scan takes the radio's time, and on the bench connects then failed,
/// and went through once it stopped (2026-11-21).
pub const OTHERS_SCANNING: &str =
    "another program is scanning for Bluetooth devices, so connecting may fail; closing it helps";

/// Whether another program scans, said to the person if so.
fn others_scanning<T: Transport>(t: &mut T) -> bool {
    let busy = t.others_scanning().unwrap_or(false);
    if busy {
        t.warn(OTHERS_SCANNING);
    }
    busy
}

fn gave_up(id: NodeId, e: &Error, busy: bool) -> Error {
    let advice = match busy {
        true => OTHERS_SCANNING,
        false => "come nearer, or try again",
    };
    Error::Other(format!(
        "could not connect to {} in {CONNECT_TRIES} tries ({e}): {advice}",
        id_text(id)
    ))
}

/// How many scans' time the look for a node may take when the transport
/// no longer holds its device before a new try. While another program's
/// discovery runs, the controller may report the node again only after a
/// while: on the bench, scans of 30 s heard nodes that scans of 10 s
/// missed (2026-11-21). A look ends as soon as the node is heard.
const RETRY_SCANS: u32 = 3;

/// Connects to `found`, heard in a scan for `sel`, trying again on the
/// same device, and looking for the node again only where the transport
/// no longer holds it.
fn connect_heard<T: Transport>(
    t: &mut T,
    sel: &Select,
    time: Duration,
    mut found: Found,
) -> Result<T::Link> {
    let busy = others_scanning(t);
    let mut tries = 1;
    loop {
        match t.connect(&found) {
            Ok(l) => return Ok(l),
            Err(Error::NoDevice) if tries < CONNECT_TRIES => {
                tries += 1;
                found = find(t, sel, time * RETRY_SCANS)?;
            }
            Err(e) if retried(&e) && tries < CONNECT_TRIES => tries += 1,
            Err(e) if retried(&e) => return Err(gave_up(found.id, &e, busy)),
            Err(e) => return Err(e),
        }
    }
}

/// A guest's link to the node `sel` names: a node takes one only while it
/// is public (spec "Phone link", Public). A private node refuses a guest's
/// read for want of a bond, at which BlueZ starts a pairing by itself, so
/// a guest does not connect to one it knows to be private.
pub fn guest<T: Transport>(t: &mut T, sel: &Select, time: Duration) -> Result<T::Link> {
    let found = find(t, sel, time)?;
    if found.public == Reach::Private {
        return Err(Error::Other(format!(
            "{} is private: only its paired phones use it",
            id_text(found.id)
        )));
    }
    connect_heard(t, sel, time, found)
}

/// Node, the link's first read: the node, and whether it and this tool
/// serve each other (spec "Phone link", Versions), whose answer the node
/// gives too as the phone writes Recipient.
pub fn node<L: Link>(l: &mut L) -> Result<Node> {
    Node::decode(&l.read(Char::Node)?)
        .map_err(|_| Error::Other("the node's Node value is too short".into()))
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

pub fn status<L: Link>(l: &mut L) -> Result<Status> {
    Status::decode(&l.read(Char::Status)?)
        .map_err(|_| Error::Other("the node's Status value is too short".into()))
}

/// Node, as a guest's link reads it first. BlueZ keeps the UUIDs a device
/// once advertised, so a node made private since may still look public;
/// it then refuses the read for want of a bond, or ends the link.
pub fn guest_node<L: Link>(l: &mut L) -> Result<Node> {
    match node(l) {
        Err(Error::Ended | Error::Att(0x05 | 0x0F)) => Err(Error::Other(
            "the node refused this guest: it is private now, and only its paired phones use it"
                .into(),
        )),
        n => n,
    }
}

/// Node then Status, as the app reads them as a guest's link comes up.
pub fn node_status<L: Link>(l: &mut L) -> Result<Status> {
    served(&guest_node(l)?)?;
    status(l)
}

fn no_longer(id: NodeId) -> Error {
    let id = id_text(id);
    Error::Other(format!(
        "{id} no longer knows this computer: pair again (bark pair {id})"
    ))
}

/// An owner's link to the paired node `sel` names, as the app makes one:
/// over the bond, without a scan, as BlueZ finds a bonded device itself;
/// Node read first, which shows whether the node still holds the bond
/// (spec "Phone link", The removed phone); then the computer's time, at
/// every connection (spec "Settings").
pub fn owner<T: Transport>(
    t: &mut T,
    state: &State,
    sel: &Select,
    clock: Clock,
) -> Result<(Node, T::Link)> {
    let p = state.pick(sel)?;
    if !t.bonded(&p.address)? {
        let id = id_text(p.id);
        return Err(Error::Other(format!(
            "this computer no longer holds its pairing with {id}: pair again (bark pair {id})"
        )));
    }
    let found = Found {
        id: p.id,
        address: p.address.clone(),
        rssi: None,
        public: Reach::Unknown,
        bonded: true,
    };
    let busy = others_scanning(t);
    let mut tries = 1;
    let mut l = loop {
        match t.connect(&found) {
            Ok(l) => break l,
            // A link the node ends before the first read is the node no
            // longer holding the bond.
            Err(Error::Ended) => return Err(no_longer(p.id)),
            Err(e) if retried(&e) && tries < CONNECT_TRIES => tries += 1,
            Err(e) if retried(&e) => return Err(gave_up(p.id, &e, busy)),
            Err(e) => return Err(e),
        }
    };
    let n = match node(&mut l) {
        Err(Error::Ended | Error::Att(0x05 | 0x0F)) => return Err(no_longer(p.id)),
        n => n?,
    };
    attach_owner(&mut l, &n, p.id, clock)?;
    Ok((n, l))
}

fn attach_owner<L: Link>(l: &mut L, n: &Node, id: NodeId, clock: Clock) -> Result<()> {
    served(n)?;
    if n.id != id {
        return Err(Error::Other(format!(
            "the node at this address is {}, not {}",
            id_text(n.id),
            id_text(id)
        )));
    }
    match l.write(Char::Clock, &clock.encode()) {
        Err(Error::Ended) => Err(Error::Ended),
        // As the app, the node only shows no time without it.
        _ => Ok(()),
    }
}

/// Pairs with the node `sel` names (spec "Phone link", Pairing). As the
/// app, only once this computer holds no bond with it: any goes first, as
/// pairing over an old bond fails whatever code is typed.
pub fn pair<T: Transport>(
    t: &mut T,
    state: &mut State,
    sel: &Select,
    time: Duration,
    code: Option<u32>,
    clock: Clock,
) -> Result<Node> {
    let mut found = find(t, sel, time)?;
    if found.bonded || state.paired.iter().any(|p| p.id == found.id) {
        t.forget(&found.address)?;
        state.remove(found.id);
        state.save()?;
        found = find(t, sel, time)?;
    }
    let mut l = connect_heard(t, sel, time, found.clone())?;
    if let Err(e) = l.pair(code) {
        let _ = l.disconnect();
        let _ = t.forget(&found.address);
        return Err(Error::Other(format!(
            "not paired ({e}): a wrong code, too slow, or the node is full (its screen says PHONES FULL)"
        )));
    }
    let n = node(&mut l)?;
    attach_owner(&mut l, &n, found.id, clock)?;
    state.add(Paired {
        id: found.id,
        address: found.address,
    });
    state.save()?;
    let _ = l.disconnect();
    Ok(n)
}

/// Has the node forget this computer's bond, then forgets its own copy
/// (spec "Public nodes"). The node answers the write and ends the link; a
/// write whose answer the end overtook counts once the link ends.
pub fn unpair<T: Transport>(
    t: &mut T,
    state: &mut State,
    sel: &Select,
    clock: Clock,
) -> Result<NodeId> {
    let p = state.pick(sel)?;
    let (_, mut l) = owner(t, state, sel, clock)?;
    match l.write(Char::Public, &PublicWrite::Unpair.encode()) {
        Ok(()) | Err(Error::Ended) => {}
        Err(e) => {
            if !ends(&mut l, UNPAIR_END) {
                return Err(e);
            }
        }
    }
    drop(l);
    t.forget(&p.address)?;
    state.remove(p.id);
    state.save()?;
    Ok(p.id)
}

/// Whether the link ends within `wait`.
fn ends<L: Link>(l: &mut L, wait: Duration) -> bool {
    let until = std::time::Instant::now() + wait;
    loop {
        let left = until.saturating_duration_since(std::time::Instant::now());
        match l.notified(left) {
            Err(Error::Ended) => return true,
            Ok(None) => return false,
            _ => {}
        }
    }
}

/// The neighbour table, page by page (spec "The neighbour map"): the
/// header of the last page and every neighbour.
pub fn neighbours<L: Link>(l: &mut L) -> Result<(Header, Vec<Neighbour>)> {
    let mut list = Vec::new();
    let mut slot = 0u8;
    loop {
        l.write(Char::Neighbours, &[slot])?;
        let page = l.read(Char::Neighbours)?;
        let (header, entries) = neighbours::decode(&page)
            .map_err(|_| Error::Other("the node's Neighbours page is malformed".into()))?;
        list.extend(entries);
        // 0 ends the table; slots only grow, so a node naming one already
        // read would otherwise loop.
        if header.next <= slot {
            return Ok((header, list));
        }
        slot = header.next;
    }
}

/// The neighbour `sel` names: a whole ID as it is, a part of one among the
/// node's neighbours.
pub fn neighbour<L: Link>(l: &mut L, sel: &Select) -> Result<NodeId> {
    if let Some(id) = sel.whole() {
        return Ok(id);
    }
    let (_, list) = neighbours(l)?;
    let mut matching: Vec<NodeId> = list
        .iter()
        .map(|n| n.id)
        .filter(|&id| sel.matches(id))
        .collect();
    match matching.len() {
        0 => Err(Error::Other(format!("the node has no neighbour {sel}"))),
        1 => Ok(matching.remove(0)),
        _ => Err(Error::Other(format!(
            "{sel} names more than one neighbour: give more of the ID"
        ))),
    }
}

/// Asks the neighbour `id` for its status, through the node
/// (spec "The neighbour map", Ask), as the app does: up to [`ASK_ROUNDS`]
/// asks, each waited for [`ASK_WAIT`].
pub fn ask<L: Link>(l: &mut L, id: NodeId) -> Result<Answer> {
    l.subscribe(Char::Ask)?;
    for _ in 0..ASK_ROUNDS {
        l.write(Char::Ask, &Ask(id).encode())?;
        let until = std::time::Instant::now() + ASK_WAIT;
        loop {
            let left = until.saturating_duration_since(std::time::Instant::now());
            match l.notified(left)? {
                None => break,
                Some((Char::Ask, v)) => {
                    if let Ok(Some(a)) = Answer::decode(&v) {
                        if a.from == id {
                            return Ok(a);
                        }
                    }
                }
                Some(_) => {}
            }
        }
    }
    Err(Error::Other(format!(
        "{} did not answer {ASK_ROUNDS} asks",
        id_text(id)
    )))
}

pub fn clock<L: Link>(l: &mut L) -> Result<NodeTime> {
    NodeTime::decode(&l.read(Char::Clock)?)
        .map_err(|_| Error::Other("the node's Clock value is malformed".into()))
}

/// Sets the node's map point, or clears it with none, and reads Status.
pub fn map_point<L: Link>(l: &mut L, at: Option<Position>) -> Result<Status> {
    let mut b = [0; MapPoint::MAX_LEN];
    let len = match at {
        Some(p) => MapPoint::Set(p),
        None => MapPoint::Clear,
    }
    .encode(&mut b);
    l.write(Char::MapPoint, &b[..len])?;
    status(l)
}

/// Makes the node public or private, if `set`, and reads it back.
pub fn public<L: Link>(l: &mut L, set: Option<bool>) -> Result<bool> {
    if let Some(p) = set {
        l.write(Char::Public, &PublicWrite::Set(Public(p)).encode())?;
    }
    Public::decode(&l.read(Char::Public)?)
        .map(|p| p.0)
        .map_err(|_| Error::Other("the node's Public value is malformed".into()))
}

/// A record as the tool keeps it from a Phones read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub number: u16,
    pub paired: bool,
    pub home: bool,
    pub linked: bool,
    pub mine: bool,
    pub tag: Option<u8>,
    pub day: u16,
    pub name: String,
}

/// The node's records, page by page, read again while Status's `phones`
/// moved since the first page (spec "Phones").
pub fn phones<L: Link>(l: &mut L) -> Result<Vec<Record>> {
    for _ in 0..PHONES_TRIES {
        let at = status(l)?.phones;
        let mut all = Vec::new();
        let mut from = 0u8;
        loop {
            let mut b = [0; PhonesWrite::MAX_LEN];
            let len = PhonesWrite::From(from).encode(&mut b);
            l.write(Char::Phones, &b[..len])?;
            let page = l.read(Char::Phones)?;
            let mut out = [None; RECORDS];
            let (next, n) = phones::decode_page(&page, &mut out)
                .map_err(|_| Error::Other("the node's Phones page is malformed".into()))?;
            all.extend(out[..n].iter().flatten().map(|r| Record {
                number: r.number,
                paired: r.paired,
                home: r.home,
                linked: r.linked,
                mine: r.mine,
                tag: r.tag,
                day: r.day,
                name: String::from_utf8_lossy(r.name).into_owned(),
            }));
            // A next place at or before the last guards against a loop.
            if next <= from {
                break;
            }
            from = next;
        }
        if status(l)?.phones == at {
            return Ok(all);
        }
    }
    Err(Error::Other(
        "the node's phones kept changing while they were read".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::{FakeNode, FakeRadio};

    fn id(n: u64) -> NodeId {
        NodeId::from_u64(n)
    }

    const CLOCK: Clock = Clock {
        utc_s: 1_795_000_000,
        offset_min: 60,
    };

    fn sel(s: &str) -> Select {
        s.parse().unwrap()
    }

    #[test]
    fn a_selection_is_hex_digits_of_an_id() {
        assert!("".parse::<Select>().is_err());
        assert!("12g".parse::<Select>().is_err());
        assert!("1234567890abc".parse::<Select>().is_err());
        let s = sel("bark-ABCDEF");
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
        assert_eq!(found[0].public, Reach::Private);
        assert_eq!(found[1].public, Reach::Public);
    }

    #[test]
    fn find_takes_a_unique_suffix_and_refuses_an_ambiguous_one() {
        let mut radio = FakeRadio::new(vec![
            FakeNode::new(id(0xfa4e_0000_0011)),
            FakeNode::new(id(0xfa4e_0000_0021)),
        ]);
        let f = find(&mut radio, &sel("11"), SCAN_TIME).unwrap();
        assert_eq!(f.id, id(0xfa4e_0000_0011));
        let e = find(&mut radio, &sel("1"), SCAN_TIME).unwrap_err();
        assert!(e.to_string().contains("give more"), "{e}");
        let e = find(&mut radio, &sel("31"), SCAN_TIME).unwrap_err();
        assert!(e.to_string().contains("--scan-time"), "{e}");
    }

    #[test]
    fn a_whole_id_ends_the_scan_at_its_node() {
        let mut radio = FakeRadio::new(vec![
            FakeNode::new(id(0xfa4e_0000_0001)),
            FakeNode::new(id(0xfa4e_0000_0002)),
            FakeNode::new(id(0xfa4e_0000_0003)),
        ]);
        find(&mut radio, &sel("fa4e00000001"), SCAN_TIME).unwrap();
        assert!(radio.scan_ended_early);
    }

    #[test]
    fn a_suffix_ends_the_scan_once_settled() {
        // Heard at 1/11, 2/11, ... of the scan: the settle time, 2 of its 10
        // intervals after the first, passes at the fourth node.
        let nodes = (1..=10)
            .map(|i| FakeNode::new(id(0xfa4e_0000_0000 + i)))
            .collect();
        let mut radio = FakeRadio::new(nodes);
        let f = find(&mut radio, &sel("1"), SCAN_TIME).unwrap();
        assert_eq!(f.id, id(0xfa4e_0000_0001));
        assert!(radio.scan_ended_early);
    }

    #[test]
    fn a_guest_reads_node_and_status() {
        let mut radio = FakeRadio::new(vec![FakeNode::new(id(0xfa4e_0000_0001)).image(46)]);
        let mut l = guest(&mut radio, &sel("01"), SCAN_TIME).unwrap();
        let read = node(&mut l).unwrap();
        assert_eq!(read.id, id(0xfa4e_0000_0001));
        assert_eq!(read.image, 46);
        let s = node_status(&mut l).unwrap();
        assert!(s.placed && s.external_power);
    }

    #[test]
    fn a_node_seen_public_and_linked_is_still_used_by_a_guest() {
        let mut radio = FakeRadio::new(vec![FakeNode::new(id(1))]);
        let l = guest(&mut radio, &sel("1"), SCAN_TIME).unwrap();
        l.disconnect().unwrap();
        // The link added the service's UUID to what the radio keeps.
        assert_eq!(
            scan(&mut radio, SCAN_TIME).unwrap()[0].public,
            Reach::Unknown
        );
        guest(&mut radio, &sel("1"), SCAN_TIME).unwrap();
    }

    #[test]
    fn a_node_made_private_since_refuses_the_guest() {
        let mut radio = FakeRadio::new(vec![FakeNode::new(id(1))]);
        guest(&mut radio, &sel("1"), SCAN_TIME).unwrap();
        radio.nodes[0].borrow_mut().public = false;
        let mut l = guest(&mut radio, &sel("1"), SCAN_TIME).unwrap();
        let e = node_status(&mut l).unwrap_err();
        assert!(e.to_string().contains("private now"), "{e}");
    }

    /// The bench, 2026-11-21: after `public off`, a scan still said public.
    #[test]
    fn a_scan_after_public_off_does_not_say_public() {
        let mut radio = FakeRadio::new(vec![FakeNode::new(id(0xbae6_e6cb_f4f5))]);
        assert_eq!(
            scan(&mut radio, SCAN_TIME).unwrap()[0].public,
            Reach::Public
        );
        let mut state = State::in_memory();
        pair(
            &mut radio,
            &mut state,
            &sel("f4f5"),
            SCAN_TIME,
            Some(123_456),
            CLOCK,
        )
        .unwrap();
        let (_, mut l) = owner(&mut radio, &state, &sel("f4f5"), CLOCK).unwrap();
        assert!(!public(&mut l, Some(false)).unwrap());
        assert_eq!(
            scan(&mut radio, SCAN_TIME).unwrap()[0].public,
            Reach::Unknown
        );
    }

    #[test]
    fn a_guest_does_not_connect_to_a_private_node() {
        let mut radio = FakeRadio::new(vec![FakeNode::new(id(1)).private()]);
        let e = guest(&mut radio, &sel("1"), SCAN_TIME).err().unwrap();
        assert!(e.to_string().contains("private"), "{e}");
        assert_eq!(radio.connects, 0);
    }

    #[test]
    fn a_connect_that_times_out_is_tried_again_on_the_same_device() {
        let mut radio = FakeRadio::new(vec![FakeNode::new(id(1))]);
        radio.failing = 1;
        guest(&mut radio, &sel("1"), SCAN_TIME).unwrap();
        assert_eq!((radio.connects, radio.scans), (2, 1));
        radio.failing = 2;
        let e = guest(&mut radio, &sel("1"), SCAN_TIME).err().unwrap();
        assert!(e.to_string().contains("in 2 tries"), "{e}");
    }

    #[test]
    fn another_program_scanning_is_said_before_the_connect_and_after_its_failure() {
        let mut radio = FakeRadio::new(vec![FakeNode::new(id(1))]);
        radio.others = true;
        guest(&mut radio, &sel("1"), SCAN_TIME).unwrap();
        assert_eq!(radio.warnings, [OTHERS_SCANNING]);
        radio.failing = 2;
        let e = guest(&mut radio, &sel("1"), SCAN_TIME).err().unwrap();
        assert!(e.to_string().ends_with(OTHERS_SCANNING), "{e}");
        radio.others = false;
        radio.warnings.clear();
        guest(&mut radio, &sel("1"), SCAN_TIME).unwrap();
        assert!(radio.warnings.is_empty());
    }

    #[test]
    fn a_node_heard_lately_is_connected_to_without_a_scan() {
        let mut radio = FakeRadio::new(vec![
            FakeNode::new(id(0xfa4e_0000_0001)),
            FakeNode::new(id(0xfa4e_0000_0002)),
        ]);
        guest(&mut radio, &sel("01"), SCAN_TIME).unwrap();
        assert_eq!(radio.scans, 1);
        guest(&mut radio, &sel("02"), SCAN_TIME).unwrap();
        guest(&mut radio, &sel("fa4e00000001"), SCAN_TIME).unwrap();
        assert_eq!((radio.scans, radio.connects), (1, 3));
        // A part of an ID that names two held still scans, and is refused.
        assert!(find(&mut radio, &sel("0"), SCAN_TIME).is_err());
        assert_eq!(radio.scans, 2);
    }

    #[test]
    fn a_device_forgotten_is_looked_for_again() {
        let mut radio = FakeRadio::new(vec![FakeNode::new(id(1))]);
        radio.lost = 1;
        guest(&mut radio, &sel("1"), SCAN_TIME).unwrap();
        assert_eq!((radio.connects, radio.scans), (2, 2));
    }

    #[test]
    fn a_node_of_another_link_version_is_not_served() {
        let mut radio = FakeRadio::new(vec![FakeNode::new(id(1)).versions(9, 9)]);
        let mut l = guest(&mut radio, &sel("1"), SCAN_TIME).unwrap();
        let e = node_status(&mut l).unwrap_err();
        assert!(e.to_string().contains("newer tool"), "{e}");
        let mut radio = FakeRadio::new(vec![FakeNode::new(id(1)).versions(0, 0)]);
        let mut l = guest(&mut radio, &sel("1"), SCAN_TIME).unwrap();
        let e = node_status(&mut l).unwrap_err();
        assert!(e.to_string().contains("newer firmware"), "{e}");
    }

    /// A radio with one private node, paired with.
    fn paired() -> (FakeRadio, State) {
        let mut radio = FakeRadio::new(vec![FakeNode::new(id(0xbae6_e6cb_f4f5)).private()]);
        let mut state = State::in_memory();
        let n = pair(
            &mut radio,
            &mut state,
            &sel("f4f5"),
            SCAN_TIME,
            Some(123_456),
            CLOCK,
        )
        .unwrap();
        assert_eq!(n.room, bark_link::phones::RECORDS as u8 - 1);
        (radio, state)
    }

    #[test]
    fn pairing_takes_the_code_and_keeps_the_node() {
        let (radio, state) = paired();
        assert_eq!(state.paired.len(), 1);
        assert!(radio.bonds.borrow().contains("fake-0"));
        assert_eq!(radio.nodes[0].borrow().clock, Some(CLOCK));
    }

    #[test]
    fn a_wrong_code_pairs_nothing() {
        let mut radio = FakeRadio::new(vec![FakeNode::new(id(1))]);
        let mut state = State::in_memory();
        let e = pair(&mut radio, &mut state, &sel("1"), SCAN_TIME, Some(1), CLOCK).unwrap_err();
        assert!(e.to_string().starts_with("not paired"), "{e}");
        assert!(state.paired.is_empty() && radio.bonds.borrow().is_empty());
    }

    #[test]
    fn pairing_again_forgets_the_old_bond_first() {
        let (mut radio, mut state) = paired();
        pair(
            &mut radio,
            &mut state,
            &sel("f4f5"),
            SCAN_TIME,
            Some(123_456),
            CLOCK,
        )
        .unwrap();
        assert_eq!(state.paired.len(), 1);
    }

    #[test]
    fn an_owner_connects_without_a_scan() {
        let (mut radio, state) = paired();
        let scans = radio.scans;
        let (n, mut l) = owner(&mut radio, &state, &sel("f4f5"), CLOCK).unwrap();
        assert_eq!(n.id, id(0xbae6_e6cb_f4f5));
        assert_eq!(radio.scans, scans);
        assert!(!public(&mut l, None).unwrap());
        assert!(public(&mut l, Some(true)).unwrap());
    }

    #[test]
    fn an_owner_command_needs_a_pairing() {
        let mut radio = FakeRadio::new(vec![FakeNode::new(id(1))]);
        let e = owner(&mut radio, &State::in_memory(), &sel("1"), CLOCK)
            .err()
            .unwrap();
        assert!(e.to_string().contains("bark pair 1"), "{e}");
    }

    #[test]
    fn a_node_that_lost_the_bond_says_so() {
        let (mut radio, state) = paired();
        radio.nodes[0].borrow_mut().phones.unpair(0);
        let e = owner(&mut radio, &state, &sel("f4f5"), CLOCK)
            .err()
            .unwrap();
        assert!(e.to_string().contains("no longer knows"), "{e}");
    }

    #[test]
    fn unpairing_forgets_both_sides() {
        let (mut radio, mut state) = paired();
        unpair(&mut radio, &mut state, &sel("f4f5"), CLOCK).unwrap();
        assert!(state.paired.is_empty());
        assert!(radio.bonds.borrow().is_empty());
        assert_eq!(radio.nodes[0].borrow().phones.paired(), 0);
    }

    #[test]
    fn the_neighbour_table_comes_in_pages() {
        let mut radio = FakeRadio::new(vec![FakeNode::new(id(1)).with_neighbours(30)]);
        let mut state = State::in_memory();
        pair(
            &mut radio,
            &mut state,
            &sel("1"),
            SCAN_TIME,
            Some(123_456),
            CLOCK,
        )
        .unwrap();
        let (_, mut l) = owner(&mut radio, &state, &sel("1"), CLOCK).unwrap();
        let (header, list) = neighbours(&mut l).unwrap();
        assert_eq!(list.len(), 30);
        assert_eq!(header.floor_dbm, -110);
        assert_eq!(list[29].id, id(0xbb00_0000_0000 + 30));
        assert_eq!(neighbour(&mut l, &sel("1e")).unwrap(), id(0xbb00_0000_001e));
        let a = ask(&mut l, id(0xbb00_0000_0002)).unwrap();
        assert_eq!(a.status.version, 46);
        let e = ask(&mut l, id(0xbb00_0000_0001)).unwrap_err();
        assert!(e.to_string().contains("did not answer"), "{e}");
    }

    #[test]
    fn the_phones_list_names_this_computer() {
        let mut radio = FakeRadio::new(vec![FakeNode::new(id(1)).with_other_phone()]);
        let mut state = State::in_memory();
        pair(
            &mut radio,
            &mut state,
            &sel("1"),
            SCAN_TIME,
            Some(123_456),
            CLOCK,
        )
        .unwrap();
        let (_, mut l) = owner(&mut radio, &state, &sel("1"), CLOCK).unwrap();
        let list = phones(&mut l).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "Olga");
        assert!(list[1].mine && list[1].paired);
        let s = map_point(&mut l, Some(Position::from_degrees(59.2, 18.2))).unwrap();
        assert!(s.map_point);
        let t = clock(&mut l).unwrap();
        assert_eq!(t.utc_s, CLOCK.utc_s);
    }
}
