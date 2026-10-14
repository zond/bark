//! Routing between relays (design §9.2, §9.3), driven through real bytes
//! on the test medium. Every relay's application can send; a message to a
//! relay itself (leaf 0) is delivered to its application.

mod common;

use bark_proto::wire::{Packet, ACK_LEN};
use bark_proto::{AppCommand, AppEvent, Config, DropReason, Instant, LeafId, MsgId, Node, Position, Relay, Trace};
use common::{secs, Harness};

/// A position `east`, `north` metres from a point in Kyiv.
fn pos(east: f64, north: f64) -> Position {
    let (lat, lon) = (50.45, 30.52);
    let m_per_deg = 6_371_000.0 * core::f64::consts::PI / 180.0;
    Position::from_degrees(lat + north / m_per_deg, lon + east / (m_per_deg * lat.to_radians().cos()))
}

/// The boot listen is a full rotation in the design (§6.2); a short one
/// keeps the tests quick and changes nothing after it.
fn relay(east: f64, north: f64, seed: u64) -> Node {
    Node::Relay(Relay::new(Config { position: pos(east, north), boot_listen: secs(5), ..Config::default() }, seed))
}

fn core(h: &Harness<Node>, i: usize) -> &Relay {
    match &h.nodes[i].core {
        Node::Relay(r) => r,
        Node::Leaf(_) => panic!("node {i} is a leaf"),
    }
}

fn position(h: &Harness<Node>, i: usize) -> Position {
    core(h, i).config().position
}

fn knows(h: &Harness<Node>, a: usize, b: usize) -> bool {
    let target = position(h, b);
    core(h, a).neighbours().any(|n| n.position == target)
}

/// Run until every linked pair knows each other (design §7: within about a
/// rotation after boot).
fn discover(h: &mut Harness<Node>, pairs: &[(usize, usize)]) {
    let pairs = pairs.to_vec();
    let found = h.run_until(Instant::ZERO + secs(600), |h| pairs.iter().all(|&(a, b)| knows(h, a, b) && knows(h, b, a)));
    assert!(found, "neighbours not discovered");
}

fn build(nodes: Vec<Node>, links: &[(usize, usize)]) -> Harness<Node> {
    let mut h = Harness::new(nodes);
    for &(a, b) in links {
        h.link(a, b);
    }
    discover(&mut h, links);
    h
}

/// Relay `from`'s application sends `text` to relay `to` itself.
fn send(h: &mut Harness<Node>, from: usize, to: usize, text: &[u8]) -> MsgId {
    let dst_pos = position(h, to);
    h.command(from, AppCommand::Send { dst_pos, dst_leaf: LeafId::RELAY, payload: text });
    MsgId::of(dst_pos, LeafId::RELAY, text)
}

fn messages(h: &Harness<Node>, i: usize, id: MsgId) -> usize {
    h.nodes[i].app.iter().filter(|(_, e)| matches!(e, AppEvent::Message { msg_id, .. } if *msg_id == id)).count()
}

fn traces(h: &Harness<Node>, i: usize) -> Vec<Trace> {
    h.nodes[i].traces.iter().map(|(_, t)| *t).collect()
}

fn delivered(h: &Harness<Node>, i: usize, id: MsgId) -> bool {
    messages(h, i, id) > 0
}

/// Four relays; the greedy choice at O, X, is a dead end. X has no
/// neighbour but its parent, so it returns the message (§9.2.5); O takes
/// its next candidate, Y, which reaches D.
///
/// ```text
///         X            D (1000, 0)
///        /            /
///       O ---------- Y
/// ```
#[test]
fn dead_end_returns_to_parent_and_parent_tries_next() {
    let (o, x, y, d) = (0, 1, 2, 3);
    let mut h = build(
        vec![relay(0.0, 0.0, 1), relay(400.0, 100.0, 2), relay(300.0, -300.0, 3), relay(1000.0, 0.0, 4)],
        &[(o, x), (o, y), (y, d)],
    );
    let id = send(&mut h, o, d, b"around the gap");
    assert!(h.run_until(h.now + secs(10), |h| delivered(h, d, id)), "not delivered");
    let name = |i| position(&h, i).short();
    assert!(traces(&h, x).contains(&Trace::ReturnedToParent { msg_id: id, to: name(o) }));
    let forwards: Vec<_> = traces(&h, o)
        .into_iter()
        .filter_map(|t| match t {
            Trace::Forwarded { msg_id, to, .. } if msg_id == id => Some(to),
            _ => None,
        })
        .collect();
    assert_eq!(forwards, vec![name(x), name(y)], "nearest first, then the next");
    // Reported sent once, at the first hop's ACK (core-interface §4).
    let sent = h.nodes[o].app.iter().filter(|(_, e)| *e == AppEvent::Sent(id)).count();
    assert_eq!(sent, 1);
    assert_eq!(messages(&h, d, id), 1);
    for i in [o, x, y, d] {
        assert!(!traces(&h, i).iter().any(|t| matches!(t, Trace::Dropped { .. })), "relay {i} dropped");
    }
}

/// A neighbour that already has the message sends it straight back
/// (§9.2.4 case 6), and the sender moves on. O, P and Q all hear each
/// other; only R reaches D.
///
/// O → P → Q → O: O has it, sends it back to Q. Q and then P have nothing
/// left and return it. O tries its next, Q, which has finished its part
/// of the search and sends it straight back. O moves on to R, and R
/// delivers to D.
#[test]
fn a_neighbour_that_has_the_message_sends_it_straight_back() {
    let (o, p, q, r, d) = (0, 1, 2, 3, 4);
    let mut h = build(
        vec![
            relay(0.0, 0.0, 11),
            relay(600.0, 100.0, 12),
            relay(500.0, -150.0, 13),
            relay(100.0, 300.0, 14),
            relay(2000.0, 0.0, 15),
        ],
        &[(o, p), (o, q), (p, q), (o, r), (r, d)],
    );
    let id = send(&mut h, o, d, b"bounce");
    assert!(h.run_until(h.now + secs(10), |h| delivered(h, d, id)), "not delivered");
    let name = |i| position(&h, i).short();
    let bounced = |h: &Harness<Node>, at: usize, to: usize| traces(h, at).contains(&Trace::Bounced { msg_id: id, to: name(to) });
    assert!(bounced(&h, o, q), "O sends Q's offer straight back");
    assert!(bounced(&h, q, o), "Q, finished, sends O's offer straight back");
    let forwards: Vec<_> = traces(&h, o)
        .into_iter()
        .filter_map(|t| match t {
            Trace::Forwarded { msg_id, to, .. } if msg_id == id => Some(to),
            _ => None,
        })
        .collect();
    assert_eq!(forwards, vec![name(p), name(q), name(r)]);
    assert_eq!(messages(&h, d, id), 1);
    for i in 0..5 {
        assert!(!traces(&h, i).iter().any(|t| matches!(t, Trace::Dropped { .. })), "relay {i} dropped");
    }
}

/// With D unreachable, the same search ends at the origin with a drop
/// for want of a route (§9.2.5). The application heard `Sent` at the first
/// hop's ACK and hears nothing more (core-interface §4).
#[test]
fn search_without_owner_ends_at_the_origin() {
    let (o, p, q) = (0, 1, 2);
    let mut h = build(
        vec![relay(0.0, 0.0, 21), relay(600.0, 100.0, 22), relay(500.0, -150.0, 23), relay(2000.0, 0.0, 24)],
        &[(o, p), (o, q), (p, q)],
    );
    let id = send(&mut h, o, 3, b"nowhere");
    let dropped = Trace::Dropped { msg_id: id, reason: DropReason::NoRoute };
    assert!(h.run_until(h.now + secs(10), |h| traces(h, o).contains(&dropped)), "no drop at the origin");
    let reports: Vec<AppEvent> = h.nodes[o].app.iter().map(|(_, e)| *e).filter(|e| !matches!(e, AppEvent::Accepted(_))).collect();
    assert_eq!(reports, vec![AppEvent::Sent(id)]);
    for i in [p, q] {
        assert!(!traces(&h, i).iter().any(|t| matches!(t, Trace::Dropped { .. })), "relay {i} dropped");
    }
}

/// D's first ACK is lost; A retransmits with the same count (§9.2.6). D
/// takes it for a copy (§9.2.4 case 3), ACKs it and does not deliver it
/// again.
#[test]
fn a_retransmission_is_acked_and_not_delivered_twice() {
    let (a, d) = (0, 1);
    let mut h = build(vec![relay(0.0, 0.0, 31), relay(400.0, 0.0, 32)], &[(a, d)]);
    let mut lost = 0;
    h.lose = Some(Box::new(move |_, from, to, bytes| {
        let ack = from == d && to == a && bytes.len() == ACK_LEN && lost == 0;
        lost += usize::from(ack);
        ack
    }));
    let id = send(&mut h, a, d, b"once");
    let sent = |h: &Harness<Node>| h.nodes[a].app.iter().any(|(_, e)| *e == AppEvent::Sent(id));
    assert!(h.run_until(h.now + secs(5), sent), "never acknowledged");
    h.run_for(secs(5));
    assert_eq!(messages(&h, d, id), 1, "delivered once");
    let datas = h.nodes[a].sent.iter().filter(|(_, f)| matches!(Packet::decode(f.payload()), Ok(Packet::Data(_)))).count();
    let acks = h.nodes[d].sent.iter().filter(|(_, f)| matches!(Packet::decode(f.payload()), Ok(Packet::Ack(_)))).count();
    assert_eq!((datas, acks), (2, 2), "a retransmission, and an ACK for each");
    // The retransmission repeats the count.
    let counts: Vec<u8> = h.nodes[a]
        .sent
        .iter()
        .filter_map(|(_, f)| match Packet::decode(f.payload()) {
            Ok(Packet::Data(d)) => Some(d.hops),
            _ => None,
        })
        .collect();
    assert_eq!(counts, vec![1, 1]);
}
