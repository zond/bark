//! Routing between relays (design §9.2, §9.3), driven through real bytes
//! on the test medium. Every relay's application can send; a message to a
//! relay itself (leaf 0) is delivered to its application.

mod common;

use bark_proto::wire::{Packet, ACK_LEN};
use bark_proto::{
    AppCommand, AppEvent, Config, DropReason, Duration, Instant, LeafId, MsgId, Node, Position,
    Relay, Trace,
};
use common::{ch, secs, Harness};

/// A position `east`, `north` metres from a point in Kyiv.
fn pos(east: f64, north: f64) -> Position {
    let (lat, lon) = (50.45, 30.52);
    let m_per_deg = 6_371_000.0 * core::f64::consts::PI / 180.0;
    Position::from_degrees(
        lat + north / m_per_deg,
        lon + east / (m_per_deg * lat.to_radians().cos()),
    )
}

/// The boot listen is a full rotation in the design (§6.2); a short one
/// keeps the tests quick and changes nothing after it.
fn relay(east: f64, north: f64, seed: u64) -> Node {
    Node::Relay(Relay::new(
        Config {
            position: pos(east, north),
            boot_listen: secs(5),
            ..Config::default()
        },
        seed,
    ))
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
    let found = h.run_until(Instant::ZERO + secs(600), |h| {
        pairs.iter().all(|&(a, b)| knows(h, a, b) && knows(h, b, a))
    });
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
    h.command(
        from,
        AppCommand::Send {
            dst_pos,
            dst_leaf: LeafId::RELAY,
            payload: text,
        },
    );
    MsgId::of(dst_pos, LeafId::RELAY, text)
}

fn messages(h: &Harness<Node>, i: usize, id: MsgId) -> usize {
    h.nodes[i]
        .app
        .iter()
        .filter(|(_, e)| matches!(e, AppEvent::Message { msg_id, .. } if *msg_id == id))
        .count()
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
        vec![
            relay(0.0, 0.0, 1),
            relay(400.0, 100.0, 2),
            relay(300.0, -300.0, 3),
            relay(1000.0, 0.0, 4),
        ],
        &[(o, x), (o, y), (y, d)],
    );
    let id = send(&mut h, o, d, b"around the gap");
    assert!(
        h.run_until(h.now + secs(10), |h| delivered(h, d, id)),
        "not delivered"
    );
    let name = |i| position(&h, i).short();
    assert!(traces(&h, x).contains(&Trace::ReturnedToParent {
        msg_id: id,
        to: name(o)
    }));
    let forwards: Vec<_> = traces(&h, o)
        .into_iter()
        .filter_map(|t| match t {
            Trace::Forwarded { msg_id, to, .. } if msg_id == id => Some(to),
            _ => None,
        })
        .collect();
    assert_eq!(
        forwards,
        vec![name(x), name(y)],
        "nearest first, then the next"
    );
    // Reported sent once, at the first hop's ACK (core-interface §4).
    let sent = h.nodes[o]
        .app
        .iter()
        .filter(|(_, e)| *e == AppEvent::Sent(id))
        .count();
    assert_eq!(sent, 1);
    assert_eq!(messages(&h, d, id), 1);
    for i in [o, x, y, d] {
        assert!(
            !traces(&h, i)
                .iter()
                .any(|t| matches!(t, Trace::Dropped { .. })),
            "relay {i} dropped"
        );
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
    assert!(
        h.run_until(h.now + secs(10), |h| delivered(h, d, id)),
        "not delivered"
    );
    let name = |i| position(&h, i).short();
    let bounced = |h: &Harness<Node>, at: usize, to: usize| {
        traces(h, at).contains(&Trace::Bounced {
            msg_id: id,
            to: name(to),
        })
    };
    assert!(bounced(&h, o, q), "O sends Q's offer straight back");
    assert!(
        bounced(&h, q, o),
        "Q, finished, sends O's offer straight back"
    );
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
        assert!(
            !traces(&h, i)
                .iter()
                .any(|t| matches!(t, Trace::Dropped { .. })),
            "relay {i} dropped"
        );
    }
}

/// The forwards of message `id` at relay `i`, in order.
fn forwards(h: &Harness<Node>, i: usize, id: MsgId) -> Vec<bark_proto::ShortName> {
    traces(h, i)
        .into_iter()
        .filter_map(|t| match t {
            Trace::Forwarded { msg_id, to, .. } if msg_id == id => Some(to),
            _ => None,
        })
        .collect()
}

fn no_drops(h: &Harness<Node>) {
    for i in 0..h.nodes.len() {
        assert!(
            !traces(h, i)
                .iter()
                .any(|t| matches!(t, Trace::Dropped { .. })),
            "relay {i} dropped: {:?}",
            traces(h, i)
        );
    }
}

/// An earlier child that offers the message during its own search is sent
/// it straight back (§9.2.4 case 5): its copy is the one searching. Here
/// it has the message before this relay hands it over, sends that
/// hand-over straight back, and offers the message itself later.
///
/// W → Y → C → X. X offers Y, which has the message and sends it straight
/// back (case 6); X moves on to Z, a dead end, and returns to C, which
/// returns to Y. Y now offers X, for which Y is an earlier child. X sends
/// it straight back, and Y goes on to E, which reaches D. ACK-only there
/// ended the only searching copy without a trace.
///
/// ```text
///   W - Y - C          D (far east)
///       |\  |          |
///       E  \|          E
///            X - Z
/// ```
#[test]
fn an_earlier_child_that_had_the_message_is_sent_it_back() {
    let (w, y, c, x, z, e, d) = (0, 1, 2, 3, 4, 5, 6);
    let mut h = build(
        vec![
            relay(0.0, 0.0, 111),
            relay(600.0, 0.0, 112),
            relay(900.0, 200.0, 113),
            relay(800.0, -200.0, 114),
            relay(100.0, -400.0, 115),
            relay(500.0, 300.0, 116),
            relay(10_000.0, 0.0, 117),
        ],
        &[(w, y), (y, c), (y, x), (c, x), (x, z), (y, e), (e, d)],
    );
    let id = send(&mut h, w, d, b"its own search");
    assert!(
        h.run_until(h.now + secs(20), |h| delivered(h, d, id)),
        "not delivered: X {:?}",
        traces(&h, x)
    );
    let name = |i| position(&h, i).short();
    let bounced = |at: usize, to: usize| {
        traces(&h, at).contains(&Trace::Bounced {
            msg_id: id,
            to: name(to),
        })
    };
    assert!(bounced(y, x), "Y sends X's offer straight back");
    assert!(bounced(x, y), "X sends Y's later offer straight back");
    assert_eq!(forwards(&h, x, id), vec![name(y), name(z)]);
    assert_eq!(forwards(&h, y, id), vec![name(c), name(x), name(e)]);
    assert_eq!(messages(&h, d, id), 1);
    no_drops(&h);
}

/// The same for a hand-over this relay counted as failed, though the
/// neighbour never took the message: X's three frames to Y are lost, as
/// three refusals whose BUSY replies were lost would be. X moves on to Z,
/// Z hands to Y, and Y offers X, an earlier child of X's. X sends it
/// straight back, and Y goes on to E, which reaches D.
///
/// ```text
///   E - Y - X          D (far east)
///   |    \  |
///   D     Z-+
/// ```
#[test]
fn an_earlier_child_whose_hand_over_failed_is_sent_it_back() {
    let (x, y, z, e, d) = (0, 1, 2, 3, 4);
    let mut h = build(
        vec![
            relay(500.0, 0.0, 121),
            relay(600.0, 300.0, 122),
            relay(400.0, -300.0, 123),
            relay(300.0, 400.0, 124),
            relay(10_000.0, 0.0, 125),
        ],
        &[(x, y), (x, z), (z, y), (y, e), (e, d)],
    );
    let mut lost = 0;
    h.lose = Some(Box::new(move |_, from, to, bytes| {
        let data = (from, to) == (x, y) && matches!(Packet::decode(bytes), Ok(Packet::Data(_)));
        let lose = data && lost < 3;
        lost += usize::from(lose);
        lose
    }));
    let id = send(&mut h, x, d, b"failed, not taken");
    assert!(
        h.run_until(h.now + secs(20), |h| delivered(h, d, id)),
        "not delivered: X {:?}",
        traces(&h, x)
    );
    let name = |i| position(&h, i).short();
    assert!(traces(&h, x).contains(&Trace::Bounced {
        msg_id: id,
        to: name(y),
    }));
    assert_eq!(forwards(&h, y, id), vec![name(x), name(e)]);
    assert_eq!(messages(&h, d, id), 1);
    no_drops(&h);
}

/// A split: X's hand-over to Y succeeds but all three ACKs are lost, so X
/// counts it failed and moves on to Z, which reaches D. Y searches too,
/// finds a dead end and returns the message to X. X sends it straight
/// back (§9.2.4 case 5), and Y, receiving it from its parent, only ACKs:
/// one copy dies there, and the message is delivered once.
///
/// ```text
///   P - Y - X - Z - D
/// ```
#[test]
fn a_split_copy_returned_by_an_earlier_child_ends_there() {
    let (x, y, p, z, d) = (0, 1, 2, 3, 4);
    let mut h = build(
        vec![
            relay(500.0, 0.0, 131),
            relay(800.0, 300.0, 132),
            relay(900.0, 700.0, 133),
            relay(700.0, -300.0, 134),
            relay(10_000.0, 0.0, 135),
        ],
        &[(x, y), (y, p), (x, z), (z, d)],
    );
    let mut lost = 0;
    h.lose = Some(Box::new(move |_, from, to, bytes| {
        let ack = (from, to) == (y, x) && matches!(Packet::decode(bytes), Ok(Packet::Ack(_)));
        let lose = ack && lost < 3;
        lost += usize::from(lose);
        lose
    }));
    let id = send(&mut h, x, d, b"split");
    let name = |h: &Harness<Node>, i| position(h, i).short();
    let (ny, nx) = (name(&h, y), name(&h, x));
    let back_at_y = Trace::ReturnedToParent { msg_id: id, to: nx };
    assert!(h.run_until(h.now + secs(20), |h| {
        delivered(h, d, id) && traces(h, y).contains(&back_at_y)
    }));
    h.run_for(secs(10));
    assert!(traces(&h, x).contains(&Trace::Bounced { msg_id: id, to: ny }));
    assert_eq!(forwards(&h, y, id), vec![name(&h, p)]);
    assert_eq!(messages(&h, d, id), 1);
    no_drops(&h);
}

/// With D unreachable, the same search ends at the origin with a drop
/// for want of a route (§9.2.5). The application heard `Sent` at the first
/// hop's ACK and hears nothing more (core-interface §4).
#[test]
fn search_without_owner_ends_at_the_origin() {
    let (o, p, q) = (0, 1, 2);
    let mut h = build(
        vec![
            relay(0.0, 0.0, 21),
            relay(600.0, 100.0, 22),
            relay(500.0, -150.0, 23),
            relay(2000.0, 0.0, 24),
        ],
        &[(o, p), (o, q), (p, q)],
    );
    let id = send(&mut h, o, 3, b"nowhere");
    let dropped = Trace::Dropped {
        msg_id: id,
        reason: DropReason::NoRoute,
    };
    assert!(
        h.run_until(h.now + secs(10), |h| traces(h, o).contains(&dropped)),
        "no drop at the origin"
    );
    let reports: Vec<AppEvent> = h.nodes[o]
        .app
        .iter()
        .map(|(_, e)| *e)
        .filter(|e| !matches!(e, AppEvent::Accepted(_)))
        .collect();
    assert_eq!(reports, vec![AppEvent::Sent(id)]);
    for i in [p, q] {
        assert!(
            !traces(&h, i)
                .iter()
                .any(|t| matches!(t, Trace::Dropped { .. })),
            "relay {i} dropped"
        );
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
    let datas = h.nodes[a]
        .sent
        .iter()
        .filter(|(_, f)| matches!(Packet::decode(f.payload()), Ok(Packet::Data(_))))
        .count();
    let acks = h.nodes[d]
        .sent
        .iter()
        .filter(|(_, f)| matches!(Packet::decode(f.payload()), Ok(Packet::Ack(_))))
        .count();
    assert_eq!(
        (datas, acks),
        (2, 2),
        "a retransmission, and an ACK for each"
    );
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

/// O, M, D in a line. Two messages whose frames to M are all lost demote
/// M at O (§8.2), but M is O's only way on, so the next message is still
/// handed to it, as the last resort (§9.2.3, roadmap D25), and arrives.
#[test]
fn a_demoted_neighbour_is_the_last_resort() {
    let (o, m, d) = (0, 1, 2);
    let mut h = build(
        vec![
            relay(0.0, 0.0, 61),
            relay(400.0, 0.0, 62),
            relay(800.0, 0.0, 63),
        ],
        &[(o, m), (m, d)],
    );
    let lost = std::rc::Rc::new(std::cell::Cell::new(true));
    let losing = lost.clone();
    h.lose = Some(Box::new(move |_, from, to, bytes| {
        losing.get() && (from, to) == (o, m) && matches!(Packet::decode(bytes), Ok(Packet::Data(_)))
    }));
    for k in 0..2u8 {
        let id = send(&mut h, o, d, &[b'x', k]);
        h.run_for(secs(10));
        assert!(!delivered(&h, d, id));
    }
    assert!(traces(&h, o)
        .iter()
        .any(|t| matches!(t, Trace::Demoted { .. })));
    lost.set(false);
    let id = send(&mut h, o, d, b"through anyway");
    h.run_for(secs(5));
    assert!(delivered(&h, d, id), "{:?}", traces(&h, o));
}

/// F1, O's nearest neighbour toward D, has its send queue full: its
/// application has just queued a short message for D in each of its
/// places, and its frames to D are lost. F1 answers O's DATA with BUSY
/// (§9.2.4), and O hands the message to F2 at once, counting no failure
/// against F1 (§9.3). O bans F1: no message is offered to it until the
/// ban ends; after it, F1 is O's first choice again (roadmap D27, D29).
///
/// ```text
///          F1
///        /    \
///       O      D
///        \    /
///          F2
/// ```
#[test]
fn a_ban_excludes_the_neighbour_and_expires() {
    use bark_proto::tables::budget::BAN;
    use bark_proto::tables::send_queue::SEND_QUEUE;
    let (o, f1, f2, d) = (0, 1, 2, 3);
    let mut h = build(
        vec![
            relay(0.0, 0.0, 71),
            relay(400.0, 50.0, 72),
            relay(400.0, -100.0, 73),
            relay(800.0, 0.0, 74),
        ],
        &[(o, f1), (o, f2), (f1, d), (f2, d)],
    );
    let lost = std::rc::Rc::new(std::cell::Cell::new(true));
    let losing = lost.clone();
    h.lose = Some(Box::new(move |_, from, to, bytes| {
        losing.get()
            && (from, to) == (f1, d)
            && matches!(Packet::decode(bytes), Ok(Packet::Data(_)))
    }));
    for k in 0..SEND_QUEUE as u8 {
        send(&mut h, f1, d, &[b'q', k]);
    }
    let name = |h: &Harness<Node>, i| position(h, i).short();
    let (n1, n2) = (name(&h, f1), name(&h, f2));
    let to_f1 = |h: &Harness<Node>, id: MsgId| {
        h.nodes[o].sent.iter().any(|(_, fr)| {
            matches!(Packet::decode(fr.payload()), Ok(Packet::Data(p)) if p.dst_hop == n1 && p.msg_id() == id)
        })
    };
    let forwarded = |h: &Harness<Node>, id: MsgId| {
        h.nodes[o].traces.iter().find_map(|(t, tr)| match tr {
            Trace::Forwarded { msg_id, to, .. } if *msg_id == id => Some((*t, *to)),
            _ => None,
        })
    };

    let first = send(&mut h, o, d, b"first");
    assert!(h.run_until(h.now + secs(5), |h| delivered(h, d, first)));
    let refused_at = h.nodes[o]
        .traces
        .iter()
        .find_map(|(t, tr)| match tr {
            Trace::TurnedAway { msg_id, by } if *msg_id == first && *by == n1 => Some(*t),
            _ => None,
        })
        .expect("F1 refused with BUSY");
    let (at, to) = forwarded(&h, first).unwrap();
    assert_eq!(to, n2);
    assert!(
        at.since(refused_at) < secs(1),
        "moved on at once: {:?}",
        at.since(refused_at)
    );
    let p1 = position(&h, f1);
    let entry = *core(&h, o).neighbours().find(|n| n.position == p1).unwrap();
    assert_eq!(entry.fail_count, 0, "a refusal is no failure");
    assert_eq!(entry.banned_until, refused_at + BAN);

    // Within the ban F1 is not offered the next message at all.
    let second = send(&mut h, o, d, b"second");
    assert!(h.now < refused_at + BAN);
    assert!(h.run_until(h.now + secs(5), |h| delivered(h, d, second)));
    assert!(!to_f1(&h, second), "offered to F1 within the ban");
    assert_eq!(forwarded(&h, second).map(|(_, to)| to), Some(n2));

    // After it, and with F1's queue gone, F1 is first again.
    lost.set(false);
    h.run_for(secs(30));
    let third = send(&mut h, o, d, b"third");
    assert!(h.run_until(h.now + secs(5), |h| delivered(h, d, third)));
    assert_eq!(forwarded(&h, third).map(|(_, to)| to), Some(n1));
}

/// P, O, F, D in a line. F's send queue is full: its application has
/// queued a short message for D in each of its places, and its frames to D
/// are lost for a while. F refuses P's message with BUSY (§9.2.4). F is
/// the only neighbour O has not tried, so O's search ends at once: it
/// returns the message to P, which, as the origin with no other
/// neighbour, drops it (§9.2.5). Within the ban O hands its own message
/// for D to P, though F is nearer D; after the ban, and with F's queue
/// gone, F takes the next message (roadmap D29).
#[test]
fn a_relay_with_only_banned_neighbours_ends_its_search_at_once() {
    use bark_proto::tables::budget::BAN;
    use bark_proto::tables::send_queue::SEND_QUEUE;
    let (p, o, f, d) = (0, 1, 2, 3);
    let mut h = build(
        vec![
            relay(-400.0, 0.0, 120),
            relay(0.0, 0.0, 121),
            relay(400.0, 0.0, 122),
            relay(800.0, 0.0, 123),
        ],
        &[(p, o), (o, f), (f, d)],
    );
    let lost = std::rc::Rc::new(std::cell::Cell::new(true));
    let losing = lost.clone();
    h.lose = Some(Box::new(move |_, from, to, bytes| {
        losing.get() && (from, to) == (f, d) && matches!(Packet::decode(bytes), Ok(Packet::Data(_)))
    }));
    for k in 0..SEND_QUEUE as u8 {
        send(&mut h, f, d, &[b'q', k]);
    }
    let name = |h: &Harness<Node>, i| position(h, i).short();
    let nf = name(&h, f);
    let to_f = |h: &Harness<Node>, id: MsgId| {
        h.nodes[o].sent.iter().any(|(_, fr)| {
            matches!(Packet::decode(fr.payload()), Ok(Packet::Data(q)) if q.dst_hop == nf && q.msg_id() == id)
        })
    };

    let first = send(&mut h, p, d, b"first");
    let gone = |h: &Harness<Node>, i: usize, id: MsgId| {
        h.nodes[i].traces.iter().find_map(|(t, tr)| match tr {
            Trace::Dropped { msg_id, reason } if *msg_id == id => Some((*t, *reason)),
            _ => None,
        })
    };
    assert!(h.run_until(h.now + secs(5), |h| gone(h, p, first).is_some()));
    h.run_for(Duration::from_millis(200));
    let refused_at = h.nodes[o]
        .traces
        .iter()
        .find_map(|(t, tr)| match tr {
            Trace::TurnedAway { msg_id, by } if *msg_id == first && *by == nf => Some(*t),
            _ => None,
        })
        .expect("F refused with BUSY");
    let returned_at = h.nodes[o]
        .traces
        .iter()
        .find_map(|(t, tr)| match tr {
            Trace::ReturnedToParent { msg_id, .. } if *msg_id == first => Some(*t),
            _ => None,
        })
        .expect("O did not return it to P");
    assert!(
        returned_at.since(refused_at) < Duration::from_millis(100),
        "returned {:?} after the BUSY",
        returned_at.since(refused_at)
    );
    assert_eq!(
        gone(&h, p, first).map(|(_, r)| r),
        Some(DropReason::NoRoute)
    );
    assert!(!traces(&h, o)
        .iter()
        .any(|t| matches!(t, Trace::Dropped { msg_id, .. } if *msg_id == first)));

    // Within the ban O's own message goes to P, farther from D, not to F.
    let np = name(&h, p);
    assert!(h.now < refused_at + BAN);
    let second = send(&mut h, o, d, b"second");
    let offered = |h: &Harness<Node>| {
        h.nodes[o]
            .sent
            .iter()
            .find_map(|(t, fr)| match Packet::decode(fr.payload()) {
                Ok(Packet::Data(q)) if q.msg_id() == second => Some((*t, q.dst_hop)),
                _ => None,
            })
    };
    assert!(h.run_until(h.now + secs(1), |h| offered(h).is_some()));
    let (at, to) = offered(&h).unwrap();
    assert!(at < refused_at + BAN);
    assert_eq!(to, np, "offered to F within the ban");

    // After it, and with F's queue gone, F takes a message again.
    lost.set(false);
    h.run_for(secs(30));
    let third = send(&mut h, o, d, b"third");
    assert!(h.run_until(h.now + secs(5), |h| delivered(h, d, third)));
    assert!(to_f(&h, third));
}

/// P, R, C, D in a line; every frame from R to C is lost. R's only
/// candidate fails its three attempts and none is banned, so R's search
/// ends as before: it returns the message to P, which has no other
/// neighbour and, as the origin, drops it (§9.2.5).
#[test]
fn a_relay_whose_candidates_failed_returns_to_the_parent() {
    let (p, r, c, d) = (0, 1, 2, 3);
    let mut h = build(
        vec![
            relay(0.0, 0.0, 131),
            relay(400.0, 0.0, 132),
            relay(800.0, 0.0, 133),
            relay(1200.0, 0.0, 134),
        ],
        &[(p, r), (r, c), (c, d)],
    );
    h.lose = Some(Box::new(move |_, from, to, _| (from, to) == (r, c)));
    let id = send(&mut h, p, d, b"no way on");
    let gone = |h: &Harness<Node>| {
        traces(h, p)
            .iter()
            .any(|t| matches!(t, Trace::Dropped { msg_id, .. } if *msg_id == id))
    };
    assert!(h.run_until(h.now + secs(10), gone), "P did not drop it");
    // P's ACK of the return, which R traces.
    h.run_for(secs(1));
    let name = |i| position(&h, i).short();
    assert!(traces(&h, r).contains(&Trace::ReturnedToParent {
        msg_id: id,
        to: name(p)
    }));
    assert!(traces(&h, p).contains(&Trace::Dropped {
        msg_id: id,
        reason: DropReason::NoRoute
    }));
    assert!(
        !traces(&h, r)
            .iter()
            .any(|t| matches!(t, Trace::TurnedAway { .. })),
        "C refused rather than failed"
    );
}

/// Twenty-six relays, F their only neighbour, offer F short messages for
/// an hour and five minutes; F, its frames to D all lost, keeps its send
/// queue full and refuses nearly all of them with BUSY. A sender drops
/// what it has for a banned F without offering it (§9.2.5), so each asks
/// for at most one reply per ban, 1.53 s: together seventeen a second, 18 % of F's
/// time on air against the 9.83 % its bucket refills. So the replies run
/// down the reply reserve, and F stays silent rather than spend the
/// heartbeat reserve (§9.4, roadmap D27, D29). No hour holds more than
/// 360 s, replies included (the harness checks every transmission), and
/// no heartbeat of F is held back.
#[test]
fn replies_stay_within_the_reserve() {
    use bark_proto::radio::DUTY_CYCLE_WINDOW;
    use bark_proto::tables::budget::{BUDGET, BUSY_AIRTIME};
    let (f, d) = (0, 1);
    let senders: Vec<usize> = (2..28).collect();
    let fixed = |east: f64, north: f64, home: u8, seed: u64| {
        Node::Relay(Relay::new(
            Config {
                position: pos(east, north),
                boot_listen: secs(5),
                home_override: Some(ch(home)),
                ..Config::default()
            },
            seed,
        ))
    };
    let mut nodes = vec![fixed(0.0, 0.0, 0, 81), fixed(400.0, 0.0, 1, 82)];
    // West of F on a half circle, their homes spread over the channels
    // F and D do not use.
    for (i, &s) in senders.iter().enumerate() {
        let angle = core::f64::consts::PI * (0.5 + (i as f64 + 0.5) / senders.len() as f64);
        nodes.push(fixed(
            400.0 * angle.cos(),
            400.0 * angle.sin(),
            2 + (i % 11) as u8,
            90 + s as u64,
        ));
    }
    let mut links = vec![(f, d)];
    links.extend(senders.iter().map(|&s| (f, s)));
    let mut h = build(nodes, &links);
    h.lose = Some(Box::new(move |_, from, to, bytes| {
        (from, to) == (f, d) && matches!(Packet::decode(bytes), Ok(Packet::Data(_)))
    }));
    let start = h.now;
    let end = start + DUTY_CYCLE_WINDOW + secs(300);
    let mut k = 0u32;
    while h.now < end {
        for &s in &senders {
            send(&mut h, s, d, &k.to_be_bytes());
            k += 1;
        }
        let next = h.now + Duration::from_millis(250);
        h.run_until(next, |_| false);
    }
    let sent = &h.nodes[f].sent;
    let busy = sent
        .iter()
        .filter(|(_, fr)| matches!(Packet::decode(fr.payload()), Ok(Packet::Busy(_))))
        .count();
    let refused = traces(&h, f)
        .iter()
        .filter(|t| matches!(t, Trace::Refused { .. }))
        .count();
    let hour = |from: Instant| {
        let to = from + DUTY_CYCLE_WINDOW;
        h.nodes[f]
            .airtime
            .iter()
            .filter(|(t, _)| *t >= from && *t < to)
            .fold(Duration::ZERO, |sum, (_, a)| sum + *a)
    };
    let spent = hour(start + secs(300));
    println!(
        "{refused} refused, {busy} BUSY sent ({:.1} s), {:.1} s on air in the last hour",
        (BUSY_AIRTIME.as_micros() * busy as u64) as f64 / 1e6,
        spent.as_micros() as f64 / 1e6
    );
    assert!(spent <= BUDGET);
    assert!(
        spent > BUDGET.mul(9).div(10),
        "F was not held at its cap: {spent:?}"
    );
    assert!(
        refused > busy,
        "the reserve never ran out: {refused} refused, {busy} BUSY"
    );
    assert!(
        !traces(&h, f)
            .iter()
            .any(|t| matches!(t, Trace::HeartbeatDeferred { .. })),
        "a reply spent the heartbeat reserve"
    );
}

/// O sends D a full frame every 2 s, 7 % of D's home channel and more than
/// the 5 % that moves a relay (§6.3). The traffic is D's own and would
/// follow it to any channel, so D does not count it as busy and stays, and
/// neither does O for D's ACKs (§6.1, roadmap D24).
#[test]
fn a_relay_does_not_flee_its_own_traffic() {
    use bark_proto::tables::busy::{percent, ONE};
    use bark_proto::wire::MAX_PAYLOAD;
    let (o, d) = (0, 1);
    let mut h = build(vec![relay(0.0, 0.0, 51), relay(400.0, 0.0, 52)], &[(o, d)]);
    let c = Config::default();
    let start = h.now;
    let end = start + c.move_dwell + c.move_wait_max + secs(600);
    let mut k = 0u32;
    while h.now < end {
        let mut text = [0u8; MAX_PAYLOAD];
        text[..4].copy_from_slice(&k.to_be_bytes());
        let id = send(&mut h, o, d, &text);
        k += 1;
        h.run_until(h.now + secs(2), |_| false);
        assert!(delivered(&h, d, id), "message {k} not delivered");
    }
    for i in [o, d] {
        let moves = h.nodes[i]
            .traces
            .iter()
            .filter(|(t, tr)| *t >= start && matches!(tr, Trace::HomeChannelMoved { .. }))
            .count();
        let r = core(&h, i);
        let busy = r.busy().fraction(r.home());
        println!(
            "relay {i}: home busy {:.2} %, {moves} moves",
            busy as f64 * 100.0 / ONE as f64
        );
        assert_eq!(moves, 0, "relay {i} moved");
        assert!(busy < percent(1), "relay {i} counts its own traffic");
    }
}

/// O sends to D through F every 2.5 s, over a link that loses F's first
/// two transmissions of every message, so each costs F three frames and an
/// ACK: about 0.43 s per message, 630 s an hour against the 354 s its
/// bucket lets through (§9.4). F works at the edge of its bucket: it
/// refuses a message with BUSY when the message's frame and everything it
/// has queued would not fit in the bucket (§9.2.4), and a hand-over whose
/// retries the bucket cannot fit yet waits for it, never longer than the
/// refill of a frame and the reply reserve, instead of being dropped. Its
/// heartbeats are never held back. The harness panics if any hour holds
/// more than 10 %, replies included.
///
/// O, told BUSY, bans F without counting a failure (§9.3); F is its only
/// neighbour, so it drops the message as the origin with no route, and
/// drops the messages that follow while the ban lasts (§9.2.5). O and D
/// cannot hear each other: now and then D's ACKs to F are lost under O's
/// frames, all four of F's attempts go unanswered, and F returns the
/// message to O as the search requires (§9.2.5). That is not a drop.
///
/// The home channels are fixed. The frames this test loses on the way to D
/// are on the air at D but never received, so D cannot tell them from
/// other nodes' traffic (§6.1): about two thirds of F's airtime, enough to
/// move D every hour or so (§6.3), and messages handed over in the minutes
/// before F hears D's new channel fail for that reason, which is not what
/// this test is about. F makes one attempt more than the
/// design's three: D also misses a frame now and then while it sweeps or
/// sends a heartbeat, which the loss rule does not count, and three
/// attempts would then run out for that reason alone. And the run goes on
/// for ten minutes after the last message, for those still queued.
#[test]
fn duty_budget_defers_forwarding_and_never_drops() {
    use bark_proto::radio::{time_on_air, DUTY_CYCLE_WINDOW, MIN_PREAMBLE_SYMBOLS};
    use bark_proto::tables::budget::{BUDGET, DEPTH, REPLY_RESERVE};
    use bark_proto::wire::MAX_PAYLOAD;
    use bark_proto::MAX_FRAME;
    const LOST: u32 = 2;
    let (o, f, d) = (0, 1, 2);
    let fixed = |east: f64, home: u8, hop_attempts: u8, seed: u64| {
        Node::Relay(Relay::new(
            Config {
                position: pos(east, 0.0),
                boot_listen: secs(5),
                home_override: Some(ch(home)),
                hop_attempts,
                ..Config::default()
            },
            seed,
        ))
    };
    let mut h = build(
        vec![
            fixed(0.0, 1, 3, 41),
            fixed(400.0, 2, 4, 42),
            fixed(800.0, 3, 3, 43),
        ],
        &[(o, f), (f, d)],
    );
    let mut tries = std::collections::HashMap::new();
    h.lose = Some(Box::new(move |_, from, to, bytes| {
        let Ok(Packet::Data(p)) = Packet::decode(bytes) else {
            return false;
        };
        if (from, to) != (f, d) {
            return false;
        }
        let n = tries.entry(p.msg_id()).or_insert(0);
        *n += 1;
        *n <= LOST
    }));
    let end = h.now + DUTY_CYCLE_WINDOW.mul(3);
    let mut k = 0u32;
    let mut sent_at = std::collections::HashMap::new();
    while h.now < end {
        let mut text = [0u8; MAX_PAYLOAD];
        text[..4].copy_from_slice(&k.to_be_bytes());
        sent_at.insert(send(&mut h, o, d, &text), h.now);
        k += 1;
        let next = h.now + Duration::from_millis(2500);
        h.run_until(next, |_| false);
    }
    h.run_for(secs(600));
    let tf = traces(&h, f);
    // No hand-over waits longer than the refill of a full frame and the
    // reply reserve it leaves (§9.4).
    let frame = time_on_air(MAX_FRAME as u8, MIN_PREAMBLE_SYMBOLS) + REPLY_RESERVE;
    let refill = Duration::from_micros(
        (frame.as_micros() * DUTY_CYCLE_WINDOW.as_micros()).div_ceil((BUDGET - DEPTH).as_micros()),
    );
    let longest = h.nodes[f]
        .traces
        .iter()
        .filter_map(|(t, tr)| match tr {
            Trace::HandOverDeferred { until, .. } => Some(until.since(*t)),
            _ => None,
        })
        .max()
        .expect("deferrals");
    assert!(longest <= refill, "deferred {longest:?}, refill {refill:?}");
    let mut deferred: Vec<MsgId> = tf
        .iter()
        .filter_map(|t| match t {
            Trace::HandOverDeferred { msg_id, .. } => Some(*msg_id),
            _ => None,
        })
        .collect();
    deferred.sort_by_key(|m| m.0);
    deferred.dedup();
    let refused = |why: DropReason| {
        tf.iter()
            .filter(|t| matches!(t, Trace::Refused { reason, .. } if *reason == why))
            .count()
    };
    let (full, broke) = (
        refused(DropReason::SendQueueFull),
        refused(DropReason::Budget),
    );
    let busy_sent = h.nodes[f]
        .sent
        .iter()
        .filter(|(_, fr)| matches!(Packet::decode(fr.payload()), Ok(Packet::Busy(_))))
        .count();
    let turned_away = traces(&h, o)
        .iter()
        .filter(|t| matches!(t, Trace::TurnedAway { .. }))
        .count();
    let handed = |id: MsgId| {
        tf.iter().any(|t| {
            matches!(t, Trace::Forwarded { msg_id, .. } | Trace::ReturnedToParent { msg_id, .. } if *msg_id == id)
        })
    };
    println!(
        "{k} sent; F forwarded {}, returned {}, deferred {} for budget, refused {full} with \
         its queue full and {broke} for its bucket, {busy_sent} BUSY sent; \
         O turned away {turned_away} times",
        tf.iter()
            .filter(|t| matches!(t, Trace::Forwarded { .. }))
            .count(),
        tf.iter()
            .filter(|t| matches!(t, Trace::ReturnedToParent { .. }))
            .count(),
        deferred.len(),
    );
    let (pf, pd) = (position(&h, f).short(), position(&h, d).short());
    let gap = h.nodes[f]
        .sent
        .iter()
        .filter(|(_, fr)| matches!(Packet::decode(fr.payload()), Ok(Packet::Data(_))))
        .map(|(t, _)| *t)
        .collect::<Vec<_>>()
        .windows(2)
        .map(|w| w[1].since(w[0]))
        .max()
        .unwrap_or_default();
    let acks_from_d = |frames: &[(Instant, Vec<u8>)]| {
        frames
            .iter()
            .filter(|(_, b)| matches!(Packet::decode(b), Ok(Packet::Ack(a)) if a.from == pd.into()))
            .count()
    };
    let d_sent: Vec<_> = h.nodes[d]
        .sent
        .iter()
        .map(|(t, fr)| (*t, fr.payload().to_vec()))
        .collect();
    let acks_lost = acks_from_d(&d_sent) - acks_from_d(&h.nodes[f].received);
    let to_f: Vec<MsgId> = h.nodes[o]
        .sent
        .iter()
        .filter_map(|(_, fr)| match Packet::decode(fr.payload()) {
            Ok(Packet::Data(p)) if p.dst_hop == pf => Some(p.msg_id()),
            _ => None,
        })
        .collect();
    let mut latency: Vec<Duration> = h.nodes[d]
        .app
        .iter()
        .filter_map(|(t, e)| match e {
            AppEvent::Message { msg_id, .. } => Some(t.since(sent_at[msg_id])),
            _ => None,
        })
        .collect();
    latency.sort();
    let mut offered = to_f.clone();
    offered.sort_by_key(|m| m.0);
    offered.dedup();
    let mut o_drops = std::collections::BTreeMap::new();
    for t in traces(&h, o) {
        if let Trace::Dropped { reason, .. } = t {
            *o_drops.entry(format!("{reason:?}")).or_insert(0) += 1;
        }
    }
    let o_failed = h.nodes[o]
        .app
        .iter()
        .filter(|(_, e)| matches!(e, AppEvent::Failed { .. }))
        .count();
    println!(
        "longest gap between F's DATA frames {gap:?}; D's ACKs lost at F {acks_lost}; \
         O's frames to F {}, of which retries {}; O dropped {o_drops:?}, told its application of {o_failed} failures; \
         D received {} messages, latency median {:?}, max {:?}",
        to_f.len(),
        to_f.len() - offered.len(),
        latency.len(),
        latency.get(latency.len() / 2),
        latency.last(),
    );
    assert!(full + broke > 0, "F never refused a message");
    assert!(busy_sent > 0 && turned_away > 0, "no BUSY reached O");
    assert!(
        !deferred.is_empty(),
        "the budget never held a hand-over back"
    );
    assert!(
        deferred.iter().all(|&id| handed(id)),
        "a deferred message was neither forwarded nor returned"
    );
    let drops: Vec<_> = tf
        .iter()
        .filter(|t| matches!(t, Trace::Dropped { .. }))
        .collect();
    assert!(drops.is_empty(), "F dropped: {drops:?}");
    assert!(
        !tf.iter()
            .any(|t| matches!(t, Trace::HeartbeatDeferred { .. })),
        "a heartbeat was held back"
    );
}
