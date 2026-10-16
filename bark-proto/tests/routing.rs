//! Routing between relays (design §9.2, §9.3), driven through real bytes
//! on the test medium. Every relay's application can send; a message to a
//! relay itself (leaf 0) is delivered to its application.

mod common;

use bark_proto::tables::budget::{hold, BAN};
use bark_proto::tables::neighbours::{Neighbour, NeighbourParams, Neighbours};
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

/// Relay `x`'s application sends `d` one short message after another,
/// each as soon as `x` takes it, until relay `f`, `x`'s only neighbour,
/// refuses one (§9.2.4): `f`'s send queue is then full, as long as `f`'s
/// own frames are lost.
fn fill(h: &mut Harness<Node>, x: usize, f: usize, d: usize) {
    let refused = |h: &Harness<Node>| {
        traces(h, f)
            .iter()
            .any(|t| matches!(t, Trace::Refused { .. }))
    };
    for k in 0u16.. {
        assert!(
            h.accepts(x) || h.run_until(h.now + secs(5), |h| h.accepts(x)),
            "{x} took no message"
        );
        send(h, x, d, &k.to_be_bytes());
        if h.run_until(h.now + Duration::from_millis(200), refused) {
            return;
        }
    }
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
    // The origin's application is told nothing (core-interface §4).
    assert!(h.nodes[o].app.is_empty());
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

/// A relay that has had the message, and holds none of it now, takes it
/// when it is offered and goes on with its own search (§9.2.4 case 5).
/// O, P and Q all hear each other; only R reaches D.
///
/// O → P → Q → O: O goes on, to Q, its next candidate. For Q, O is a
/// neighbour it handed the message to, so this is O's return: Q has
/// nothing left and returns it to P, and P to O. O moves on to R, and R
/// delivers to D. Nothing is sent straight back.
#[test]
fn a_relay_that_had_the_message_goes_on_with_its_own_search() {
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
    let returned = |at: usize, to: usize| {
        traces(&h, at).contains(&Trace::ReturnedToParent {
            msg_id: id,
            to: name(to),
        })
    };
    assert_eq!(forwards(&h, o, id), vec![name(p), name(q), name(r)]);
    assert_eq!(forwards(&h, p, id), vec![name(q)]);
    assert_eq!(forwards(&h, q, id), vec![name(o)]);
    assert!(returned(q, p) && returned(p, o));
    assert_eq!(messages(&h, d, id), 1);
    no_drops(&h);
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

/// A relay that took the message from this one and then offers it back
/// during its own search is taken for its return: this relay goes on with
/// its search. Here the child had the message before this relay handed
/// it over, and went on with its own search; as it has not tried this
/// relay, the search comes back to it.
///
/// W → Y → C → X. X offers Y, which has had the message and holds none of
/// it: it goes on with its own search, to X. For X, Y took the message from
/// it, so this is Y's return: X goes on to Z, a dead end, and returns to
/// C, which returns to Y. Y goes on to E, which reaches D. Before, Y sent
/// X's offer straight back, and ACKing it only would have ended the only
/// copy.
///
/// ```text
///   W - Y - C          D (far east)
///       |\  |          |
///       E  \|          E
///            X - Z
/// ```
#[test]
fn an_earlier_child_that_had_the_message_is_a_return() {
    let (w, y, c, x, z, e, d) = (0, 1, 2, 3, 4, 5, 6);
    // One at a time: seven relays at once would overflow a test's stack.
    let at = [
        (0.0, 0.0),
        (600.0, 0.0),
        (900.0, 200.0),
        (800.0, -200.0),
        (100.0, -400.0),
        (500.0, 300.0),
        (10_000.0, 0.0),
    ];
    let nodes = (0..).zip(at).map(|(i, (e, n))| relay(e, n, 111 + i));
    let mut h = build(
        nodes.collect(),
        &[(w, y), (y, c), (y, x), (c, x), (x, z), (y, e), (e, d)],
    );
    let id = send(&mut h, w, d, b"its own search");
    assert!(
        h.run_until(h.now + secs(20), |h| delivered(h, d, id)),
        "not delivered: X {:?}",
        traces(&h, x)
    );
    let name = |i| position(&h, i).short();
    assert_eq!(forwards(&h, x, id), vec![name(y), name(z)]);
    assert_eq!(forwards(&h, y, id), vec![name(c), name(x), name(e)]);
    assert_eq!(messages(&h, d, id), 1);
    no_drops(&h);
}

/// A neighbour whose hand-over failed, and which never had the message,
/// later offers it here (§9.2.4 case 5): X's three frames to Y are lost.
/// Y stays untried (roadmap D36), so X moves on to Z, and Z hands it to
/// Y, which offers it to X. X holds none of it and takes it, and hands it
/// to Y, its one candidate left. For Y, X is a neighbour it handed the
/// message to, so this is X's return: Y goes on to E, which reaches D.
/// Before, a failed hand-over marked Y tried, X dropped the message for
/// want of a route, and E was never tried.
///
/// ```text
///   E - Y - X          D (far east)
///   |    \  |
///   D     Z-+
/// ```
#[test]
fn a_neighbour_whose_hand_over_failed_is_tried_again() {
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
        "lost: X {:?} Y {:?}",
        traces(&h, x),
        traces(&h, y)
    );
    h.run_for(secs(10));
    let name = |i| position(&h, i).short();
    assert_eq!(forwards(&h, x, id), vec![name(z), name(y)]);
    assert_eq!(forwards(&h, z, id), vec![name(y)]);
    assert_eq!(forwards(&h, y, id), vec![name(x), name(e)]);
    assert_eq!(messages(&h, d, id), 1);
    no_drops(&h);
    assert!((0..5).all(|i| core(&h, i).holding().next().is_none()));
}

/// A split: X's hand-over to Y succeeds but every ACK is lost, so X
/// counts it failed and moves on to Z, which reaches D. Y searches too,
/// finds a dead end and returns the message to X. X holds none of it, so
/// it takes it as Y's return (§9.2.4 case 5). Y is still untried for X,
/// as its ACKs never came (roadmap D36), so X hands it to Y once more; Y
/// has nothing left and returns it again. X has then tried every
/// neighbour and, as the origin, drops that copy, as a search ends, and
/// the message is delivered once. The split costs one round trip more.
///
/// Y's return must come after X has moved on; during X's retries it
/// would end X's hand-over instead (case 4). So the frames are full, X
/// retries at once, and Y's first two frames to P are lost. X's three
/// attempts end within 3 × (138.6 + 100) = 716 ms of its first frame's
/// start. Y takes that frame at the earliest, ACKs it, and P takes Y's
/// third frame, two attempts later: from 138.6 + 14.4 + 2 × 238.6 = 630
/// ms. P's return and Y's return to X add two more frames and an ACK, so
/// Y's return ends after 1075 ms, when X has long moved on to Z. Each
/// relay has its own home channel, none samples other channels (§6.1),
/// and P has just sent a heartbeat, so P hears every frame Y sends it.
///
/// ```text
///   P - Y - X - Z - D
/// ```
#[test]
fn a_split_copy_returned_to_a_relay_holding_none_searches_on() {
    use bark_proto::wire::MAX_PAYLOAD;
    let (x, y, p, z, d) = (0, 1, 2, 3, 4);
    let relay_with = |east, north, seed, config: Config, home| {
        Node::Relay(Relay::new(
            Config {
                position: pos(east, north),
                boot_listen: secs(5),
                home_override: Some(ch(home)),
                ..config
            },
            seed,
        ))
    };
    let listening = Config {
        sweep_interval: secs(3600),
        ..Config::default()
    };
    let at_once = Config {
        retry_backoff: Duration::ZERO,
        ..listening
    };
    let mut h = build(
        vec![
            relay_with(500.0, 0.0, 131, at_once, 1),
            relay_with(800.0, 300.0, 132, listening, 2),
            relay_with(900.0, 700.0, 133, listening, 3),
            relay_with(700.0, -300.0, 134, listening, 4),
            relay_with(10_000.0, 0.0, 135, listening, 5),
        ],
        &[(x, y), (y, p), (x, z), (z, d)],
    );
    let [nx, ny, np, nz] = [x, y, p, z].map(|i| position(&h, i).short());
    let (mut moved_on, mut to_p) = (false, 0);
    h.lose = Some(Box::new(move |_, _, to, bytes| {
        match Packet::decode(bytes) {
            Ok(Packet::Data(d)) => {
                moved_on |= (d.src_hop, d.dst_hop) == (nx, nz);
                let lose = to == p && (d.src_hop, d.dst_hop) == (ny, np) && to_p < 2;
                to_p += usize::from(lose);
                lose
            }
            Ok(Packet::Ack(a)) => !moved_on && (a.from, a.to) == (ny.into(), nx.into()),
            _ => false,
        }
    }));
    // Start just as P sends a heartbeat: its next is at least 5 s off
    // (§7), so P is home for all of Y's frames.
    let beats = h.nodes[p].sent.len();
    let beat = |h: &Harness<Node>| {
        h.nodes[p].sent[beats..]
            .iter()
            .any(|(_, f)| matches!(Packet::decode(f.payload()), Ok(Packet::Heartbeat(_))))
    };
    assert!(h.run_until(h.now + secs(20), beat));
    let id = send(&mut h, x, d, &[b's'; MAX_PAYLOAD]);
    let name = |h: &Harness<Node>, i| position(h, i).short();
    let nx = name(&h, x);
    let back_at_y = Trace::ReturnedToParent { msg_id: id, to: nx };
    assert!(h.run_until(h.now + secs(20), |h| {
        delivered(h, d, id) && traces(h, y).contains(&back_at_y)
    }));
    h.run_for(secs(10));
    assert_eq!(forwards(&h, x, id), vec![name(&h, z), name(&h, y)]);
    assert_eq!(forwards(&h, y, id), vec![name(&h, p)]);
    let returns = traces(&h, y).iter().filter(|t| **t == back_at_y).count();
    assert_eq!(returns, 2);
    assert_eq!(messages(&h, d, id), 1);
    let dropped = Trace::Dropped {
        msg_id: id,
        reason: DropReason::NoRoute,
    };
    assert_eq!(traces(&h, x).iter().filter(|t| **t == dropped).count(), 1);
    for i in [y, p, z, d] {
        assert!(!traces(&h, i)
            .iter()
            .any(|t| matches!(t, Trace::Dropped { .. })));
    }
}

/// A relay at `east`, `north` with `config`, its position aside.
fn relay_with(east: f64, north: f64, seed: u64, config: Config) -> Node {
    Node::Relay(Relay::new(
        Config {
            position: pos(east, north),
            boot_listen: secs(5),
            ..config
        },
        seed,
    ))
}

/// The DATA in `bytes`, by sender and receiver, if it is one.
fn data_names(bytes: &[u8]) -> Option<(bark_proto::ShortName, bark_proto::ShortName)> {
    match Packet::decode(bytes) {
        Ok(Packet::Data(d)) => Some((d.src_hop, d.dst_hop)),
        _ => None,
    }
}

/// The ACK in `bytes`, by its sender and the one it answers, if it is one.
fn ack_names(bytes: &[u8]) -> Option<(bark_proto::wire::AckName, bark_proto::wire::AckName)> {
    match Packet::decode(bytes) {
        Ok(Packet::Ack(a)) => Some((a.from, a.to)),
        _ => None,
    }
}

/// How often relay `i` returned message `id` to its parent.
fn returns(h: &Harness<Node>, i: usize, id: MsgId) -> usize {
    traces(h, i)
        .iter()
        .filter(|t| matches!(t, Trace::ReturnedToParent { msg_id, .. } if *msg_id == id))
        .count()
}

/// A child returns the message to a parent that gave up on it (roadmap
/// D35, A). P's hand-over to C is taken, but C's ACKs are lost, so P
/// moves on to S. S hands it to C while C is still offering it to S, so
/// C takes it as S's return; C has nothing left and returns it to P,
/// which holds none of it. Before, P took this for an offer from an
/// earlier child and sent it straight back, and C, receiving it from its
/// parent, only ACKed it: no copy was left, and T, which reaches D, was
/// never tried. Now P takes it as C's return. C never ACKed it, so it is
/// untried (roadmap D36), but P's failed hand-over to it banned it
/// (roadmap D42), so P goes on to T at once.
///
/// ```text
///       C              D (far east)
///      / \             |
///     P - S            |
///      \               |
///       T -------------+
/// ```
#[test]
fn a_return_to_a_parent_that_gave_up_on_the_child_goes_on() {
    let (p, c, s, t, d) = (0, 1, 2, 3, 4);
    let hasty = Config {
        retry_backoff: Duration::ZERO,
        ..Config::default()
    };
    let patient = Config {
        hop_attempts: 20,
        ..hasty
    };
    // One at a time: built together, they would overflow a test's stack.
    let nodes = [
        (0.0, 0.0, 141, hasty),
        (600.0, 100.0, 142, patient),
        (500.0, -150.0, 143, Config::default()),
        (300.0, 400.0, 144, Config::default()),
        (10_000.0, 0.0, 145, Config::default()),
    ]
    .into_iter()
    .map(|(e, n, seed, config)| relay_with(e, n, seed, config));
    let mut h = build(nodes.collect(), &[(p, c), (p, s), (p, t), (c, s), (t, d)]);
    let [np, nc, ns] = [p, c, s].map(|i| position(&h, i).short());
    let mut p_moved = false;
    h.lose = Some(Box::new(move |_, _, _, bytes| {
        if let Some(names) = data_names(bytes) {
            p_moved |= names == (np, ns);
            return names == (nc, ns);
        }
        ack_names(bytes) == Some((nc.into(), np.into())) && !p_moved
    }));
    let id = send(&mut h, p, d, b"gave up");
    assert!(
        h.run_until(h.now + secs(30), |h| delivered(h, d, id)),
        "lost: P {:?} C {:?}",
        traces(&h, p),
        traces(&h, c)
    );
    let name = |i| position(&h, i).short();
    assert_eq!(forwards(&h, p, id), vec![name(s), name(t)]);
    assert_eq!(forwards(&h, s, id), vec![name(c)]);
    assert_eq!(returns(&h, c, id), 1);
    assert_eq!(messages(&h, d, id), 1);
    no_drops(&h);
}

/// A parent offers the message again to a child it forgot (roadmap D35,
/// B). P's message is for L, a leaf of Q's that is not there, and Q's
/// delivery queue is full of its own messages for L, so Q refuses it
/// (§10.4) and P waits for each ban to end (§9.3). Meanwhile P has tried
/// C, a dead end, which returned it. C is then out of P's hearing long
/// enough for its entry to expire, and comes back into a new slot, which
/// clears the mark that P handed it the message (§9.2.2). When Q next
/// refuses, P offers it to C again. Before, C, which had returned it and
/// holds none of it, only ACKed what came from its parent: no copy was
/// left. Now C takes it and returns it at once, and P waits for Q, which
/// takes it once its queue has room.
///
/// Heartbeats every 1 to 2 s make the rotation 19.5 s and the expiry
/// three rotations, 58.5 s (§8.2), so the test runs in minutes, not
/// hours.
#[test]
fn a_child_offered_the_message_again_by_its_parent_returns_it() {
    use bark_proto::tables::delivery::DELIVERY_QUEUE;
    let (p, c, q) = (0, 1, 2);
    let quick = Config {
        heartbeat_min: secs(1),
        heartbeat_max: secs(2),
        ..Config::default()
    };
    // One at a time: built together, they would overflow a test's stack.
    let nodes = [
        (0.0, 0.0, 151, quick),
        (-300.0, 100.0, 152, quick),
        (400.0, 0.0, 153, quick),
    ]
    .into_iter()
    .map(|(e, n, seed, config)| relay_with(e, n, seed, config));
    let mut h = build(nodes.collect(), &[(p, c), (p, q)]);
    let (pq, l) = (position(&h, q), LeafId([7; 4]));
    let to_l = |h: &mut Harness<Node>, from: usize, text: &[u8]| {
        h.command(
            from,
            AppCommand::Send {
                dst_pos: pq,
                dst_leaf: l,
                payload: text,
            },
        );
        MsgId::of(pq, l, text)
    };
    for k in 0..DELIVERY_QUEUE as u8 {
        to_l(&mut h, q, &[b'q', k]);
    }
    let id = to_l(&mut h, p, b"forgotten");
    let name = |h: &Harness<Node>, i| position(h, i).short();
    let back = |h: &Harness<Node>| returns(h, c, id) == 1;
    assert!(h.run_until(h.now + secs(10), back), "C did not return it");
    let returned_at = h.now;
    h.unlink(p, c);
    let expiry = secs(59);
    h.run_for(expiry + secs(1));
    let nc = name(&h, c);
    assert!(
        !core(&h, p).neighbours().any(|n| n.position.short() == nc),
        "C did not expire at P"
    );
    h.link(p, c);
    let offered_again = |h: &Harness<Node>| forwards(h, p, id).len() == 2;
    assert!(
        h.run_until(h.now + secs(300), offered_again),
        "P did not offer C again"
    );
    assert!(h.now > returned_at + expiry);
    let queued = |h: &Harness<Node>| {
        h.nodes[q]
            .traces
            .iter()
            .any(|(_, t)| matches!(t, Trace::Queued { msg_id, .. } if *msg_id == id))
    };
    assert!(
        h.run_until(h.now + secs(700), queued),
        "lost: P {:?} C {:?}",
        traces(&h, p),
        traces(&h, c)
    );
    // Q's ACK.
    h.run_for(secs(1));
    assert_eq!(forwards(&h, p, id), vec![nc, nc, name(&h, q)]);
    assert_eq!(returns(&h, c, id), 2);
    // Q's own messages expire in its queue; nothing else ends.
    for i in [p, c] {
        assert!(!traces(&h, i)
            .iter()
            .any(|t| matches!(t, Trace::Dropped { .. })));
    }
}

/// A copy taken for a retransmission (roadmap D35, C). X's hand-over to
/// Y is taken, but Y's ACKs are lost, so X moves on to Z. Y and Z are dead
/// ends and return it, Y after X has handed it to Z, and Z after Y, with
/// the count Y's return raised X's highest to. Before, X took Y's return
/// for an earlier child's offer and sent it straight back, where Y only
/// ACKed it, and took Z's return for a retransmission of a packet it had
/// handled, and only ACKed it: no copy was left, and W, which reaches D,
/// was never tried. A retransmission is now the last packet's sender and
/// count again. X, holding none, takes Y's return. Y never ACKed it, so
/// it is untried (roadmap D36), but X's failed hand-over banned it
/// (roadmap D42), so X goes on to W, which reaches D. Once the ban ends,
/// a later return brings X its copy back and X offers Y again; Y returns
/// it a second time and X, the origin with nothing left, drops that copy
/// as a search ends. The message arrives once.
///
/// ```text
///       Y              D (far east)
///      /               |
///     X - Z            |
///      \               |
///       W -------------+
/// ```
#[test]
fn a_copy_with_a_count_already_seen_is_not_a_retransmission() {
    let (x, y, z, w, d) = (0, 1, 2, 3, 4);
    let hasty = Config {
        retry_backoff: Duration::ZERO,
        ..Config::default()
    };
    let patient = Config {
        hop_attempts: 20,
        ..hasty
    };
    // One at a time: built together, they would overflow a test's stack.
    let nodes = [
        (0.0, 0.0, 161, hasty),
        (600.0, 100.0, 162, patient),
        (500.0, -150.0, 163, patient),
        (300.0, 400.0, 164, Config::default()),
        (10_000.0, 0.0, 165, Config::default()),
    ]
    .into_iter()
    .map(|(e, n, seed, config)| relay_with(e, n, seed, config));
    let mut h = build(nodes.collect(), &[(x, y), (x, z), (x, w), (w, d)]);
    let [nx, ny, nz] = [x, y, z].map(|i| position(&h, i).short());
    let (mut x_moved, mut y_back) = (false, false);
    h.lose = Some(Box::new(move |_, _, _, bytes| {
        if let Some(names) = data_names(bytes) {
            x_moved |= names == (nx, nz);
            if names == (ny, nx) {
                y_back |= x_moved;
                return !x_moved;
            }
            return names == (nz, nx) && !y_back;
        }
        ack_names(bytes) == Some((ny.into(), nx.into())) && !x_moved
    }));
    let id = send(&mut h, x, d, b"seen before");
    assert!(
        h.run_until(h.now + secs(30), |h| delivered(h, d, id)),
        "lost: X {:?}",
        traces(&h, x)
    );
    // Z's return.
    h.run_for(secs(10));
    let name = |i| position(&h, i).short();
    assert_eq!(forwards(&h, x, id), vec![name(z), name(w), name(y)]);
    assert_eq!((returns(&h, y, id), returns(&h, z, id)), (2, 1));
    assert_eq!(messages(&h, d, id), 1);
    // The copy Y returned the second time, at the origin with nothing left.
    let dropped = Trace::Dropped {
        msg_id: id,
        reason: DropReason::NoRoute,
    };
    let drops = traces(&h, x)
        .into_iter()
        .filter(|t| matches!(t, Trace::Dropped { .. }))
        .collect::<Vec<_>>();
    assert_eq!(drops, vec![dropped]);
    for i in 1..5 {
        assert!(!traces(&h, i)
            .iter()
            .any(|t| matches!(t, Trace::Dropped { .. })));
    }
    assert!((0..5).all(|i| core(&h, i).holding().next().is_none()));
}

/// A return cannot go back and forth between a parent and a child that
/// both hold none of the message (roadmap D35). P's only neighbour C
/// takes P's hand-over, but its ACKs are lost, so P counts it failed and
/// bans C (roadmap D42); with no other candidate the message waits in
/// P's queue for the ban. C, a dead end, returns it meanwhile, and the
/// copies merge at P (§9.2.4 case 4). C never ACKed it, so it is untried
/// (roadmap D36), and P hands it to C again when the ban ends. C,
/// receiving it from its parent with nothing left to try, returns it; P
/// has now tried C and drops it. Two returns, and nothing more on the
/// air. Before D35, P sent it straight back and C only ACKed it.
#[test]
fn a_return_does_not_go_back_and_forth() {
    let (p, c) = (0, 1);
    let patient = Config {
        hop_attempts: 20,
        retry_backoff: Duration::ZERO,
        ..Config::default()
    };
    // One at a time: built together, they would overflow a test's stack.
    let nodes = [
        (0.0, 0.0, 171, Config::default()),
        (400.0, 0.0, 172, patient),
    ]
    .into_iter()
    .map(|(e, n, seed, config)| relay_with(e, n, seed, config));
    let mut h = build(nodes.collect(), &[(p, c)]);
    let [np, nc] = [p, c].map(|i| position(&h, i).short());
    // C's return waits until P has given up: after P's third frame and
    // its ACK timeout, 100 ms.
    let (mut offers, mut open_at) = (0, None);
    h.lose = Some(Box::new(move |now, _, _, bytes| match data_names(bytes) {
        Some(n) if n == (np, nc) => {
            offers += 1;
            if offers == 3 {
                open_at = Some(now + Duration::from_millis(150));
            }
            false
        }
        Some(n) if n == (nc, np) => open_at.is_none_or(|t| now < t),
        _ => offers <= 3 && ack_names(bytes) == Some((nc.into(), np.into())),
    }));
    let (dst_pos, text) = (pos(5_000.0, 0.0), b"back and forth");
    h.command(
        p,
        AppCommand::Send {
            dst_pos,
            dst_leaf: LeafId::RELAY,
            payload: text,
        },
    );
    let id = MsgId::of(dst_pos, LeafId::RELAY, text);
    h.run_for(secs(60));
    let datas = |from: usize, to: bark_proto::ShortName| {
        h.nodes[from]
            .sent
            .iter()
            .filter(|(_, f)| matches!(data_names(f.payload()), Some((_, d)) if d == to))
            .count()
    };
    assert_eq!(forwards(&h, p, id), vec![nc]);
    assert_eq!(returns(&h, c, id), 2);
    assert_eq!(datas(p, nc), 4, "three unanswered, then one taken");
    let dropped = Trace::Dropped {
        msg_id: id,
        reason: DropReason::NoRoute,
    };
    assert_eq!(traces(&h, p).iter().filter(|t| **t == dropped).count(), 1);
    assert!(core(&h, p).holding().next().is_none());
    assert!(core(&h, c).holding().next().is_none());
}

/// A split met by the search coming back (roadmap D36). X's message is
/// for L, a leaf of P's that is not there, and P's delivery queue is full
/// of its own messages for L, so P refuses it (§10.4) and Y, P's only
/// neighbour but its parent, waits for each ban to end (§9.3). X's
/// hand-over to Y was taken, but Y's ACKs were lost, so X counts it
/// failed, leaves Y untried and moves on to Z, a dead end, which returns
/// it. X then hands it to Y again, with a higher count. Y holds its copy,
/// so it only ACKs (§9.2.4 case 4): the two copies merge, one round trip
/// more than had Y's first ACK come through, and nothing is lost. Once
/// P's messages expire, P takes Y's.
///
/// ```text
///   X - Y - P
///   |
///   Z
/// ```
#[test]
fn a_split_met_again_merges() {
    use bark_proto::tables::delivery::DELIVERY_QUEUE;
    let (x, y, z, p) = (0, 1, 2, 3);
    let hasty = Config {
        retry_backoff: Duration::ZERO,
        ..Config::default()
    };
    // One at a time: built together, they would overflow a test's stack.
    let nodes = [
        (0.0, 0.0, 181, hasty),
        (400.0, 0.0, 182, Config::default()),
        (300.0, -300.0, 183, Config::default()),
        (800.0, 0.0, 184, Config::default()),
    ]
    .into_iter()
    .map(|(e, n, seed, config)| relay_with(e, n, seed, config));
    let mut h = build(nodes.collect(), &[(x, y), (x, z), (y, p)]);
    let (pp, l) = (position(&h, p), LeafId([7; 4]));
    let to_l = |h: &mut Harness<Node>, from: usize, text: &[u8]| {
        h.command(
            from,
            AppCommand::Send {
                dst_pos: pp,
                dst_leaf: l,
                payload: text,
            },
        );
        MsgId::of(pp, l, text)
    };
    for k in 0..DELIVERY_QUEUE as u8 {
        to_l(&mut h, p, &[b'p', k]);
    }
    let [nx, ny, nz] = [x, y, z].map(|i| position(&h, i).short());
    let mut x_moved = false;
    h.lose = Some(Box::new(move |_, _, _, bytes| {
        x_moved |= data_names(bytes) == Some((nx, nz));
        ack_names(bytes) == Some((ny.into(), nx.into())) && !x_moved
    }));
    let id = to_l(&mut h, x, b"merged");
    let name = |h: &Harness<Node>, i| position(h, i).short();
    let again = |h: &Harness<Node>| forwards(h, x, id).len() == 2;
    assert!(
        h.run_until(h.now + secs(10), again),
        "X {:?} Y {:?}",
        traces(&h, x),
        traces(&h, y)
    );
    assert_eq!(forwards(&h, x, id), vec![name(&h, z), name(&h, y)]);
    assert_eq!(returns(&h, z, id), 1);
    let acks = h.nodes[y]
        .sent
        .iter()
        .filter(|(_, f)| ack_names(f.payload()) == Some((ny.into(), nx.into())))
        .count();
    assert!(acks >= 2, "Y took X's first offer, and answered the second");
    assert!(traces(&h, y)
        .iter()
        .any(|t| matches!(t, Trace::TurnedAway { msg_id, .. } if *msg_id == id)));
    assert!(core(&h, x).holding().next().is_none());
    assert_eq!(core(&h, y).holding().filter(|m| *m == id).count(), 1);
    let queued = |h: &Harness<Node>| {
        h.nodes[p]
            .traces
            .iter()
            .any(|(_, t)| matches!(t, Trace::Queued { msg_id, .. } if *msg_id == id))
    };
    assert!(h.run_until(h.now + secs(700), queued), "P never took it");
    h.run_for(secs(1));
    assert_eq!(forwards(&h, y, id), vec![name(&h, p)]);
    assert_eq!(returns(&h, y, id), 0);
    // P's own messages expire in its queue; nothing else ends.
    for i in [x, y, z] {
        assert!(!traces(&h, i)
            .iter()
            .any(|t| matches!(t, Trace::Dropped { .. })));
    }
}

/// With D unreachable, the same search ends at the origin with a drop
/// for want of a route (§9.2.5). The origin's application hears nothing
/// of it (core-interface §4).
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
    assert!(h.nodes[o].app.is_empty(), "the application was told");
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
    let sent = |h: &Harness<Node>| {
        traces(h, a)
            .iter()
            .any(|t| matches!(t, Trace::Forwarded { msg_id, .. } if *msg_id == id))
    };
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

/// O, M, D in a line. O's message to D fails at M twice, each failure
/// banning M (roadmap D42), and the second demotes it (§8.2); M is O's
/// only way on, so the message waits for each ban and is still handed to
/// M, as the last resort (§9.2.3, roadmap D25), and arrives.
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
    let id = send(&mut h, o, d, b"through anyway");
    let demoted = |h: &Harness<Node>| {
        traces(h, o)
            .iter()
            .any(|t| matches!(t, Trace::Demoted { .. }))
    };
    assert!(h.run_until(h.now + secs(20), demoted));
    assert!(!delivered(&h, d, id));
    lost.set(false);
    assert!(
        h.run_until(h.now + secs(10), |h| delivered(h, d, id)),
        "{:?}",
        traces(&h, o)
    );
    no_drops(&h);
}

/// Packet RSSI of a link heard 2 dB above the test medium's noise
/// (`common::NOISE_DBM`, −121 dBm): a heartbeat gets through, but a
/// full-length frame would not ride out a fade (§9.2.3, roadmap D38).
const FAINT_DBM: i16 = -119;

/// The DATA frames relay `from` sent to relay `to`.
fn data_sent(h: &Harness<Node>, from: usize, to: usize) -> usize {
    let (a, b) = (position(h, from).short(), position(h, to).short());
    h.nodes[from]
        .sent
        .iter()
        .filter(|(_, f)| data_names(f.payload()) == Some((a, b)))
        .count()
}

/// O hears F, the neighbour nearest D, only faintly: F is no candidate
/// while M, heard well, is left (§9.2.3, roadmap D38).
///
/// ```text
///          F
///        .   \
///       O     D
///        \   /
///          M
/// ```
#[test]
fn a_faint_neighbour_is_not_chosen() {
    let (o, f, m, d) = (0, 1, 2, 3);
    let mut h = Harness::new(vec![
        relay(0.0, 0.0, 71),
        relay(700.0, 100.0, 72),
        relay(400.0, -200.0, 73),
        relay(1000.0, 0.0, 74),
    ]);
    let links = [(o, f), (o, m), (f, d), (m, d)];
    for (a, b) in links {
        h.link(a, b);
    }
    h.set_rssi(o, f, FAINT_DBM);
    discover(&mut h, &links);
    let id = send(&mut h, o, d, b"the strong way");
    assert!(h.run_until(h.now + secs(10), |h| delivered(h, d, id)));
    let name = |i| position(&h, i).short();
    assert_eq!(forwards(&h, o, id), vec![name(m)]);
    assert_eq!(data_sent(&h, o, f), 0);
}

/// O's only way to D is F, heard faintly. F is the last resort: the
/// message goes through it (§9.2.3, roadmap D38). F's ACK, heard as well
/// as the link now is, gives F margin.
///
/// ```text
///       O . F - D
/// ```
#[test]
fn a_faint_neighbour_is_the_last_resort_and_its_ack_counts() {
    let (o, f, d) = (0, 1, 2);
    let mut h = Harness::new(vec![
        relay(0.0, 0.0, 81),
        relay(400.0, 0.0, 82),
        relay(800.0, 0.0, 83),
    ]);
    let links = [(o, f), (f, d)];
    for (a, b) in links {
        h.link(a, b);
    }
    h.set_rssi(o, f, FAINT_DBM);
    discover(&mut h, &links);
    let target = position(&h, f);
    let margin = |h: &Harness<Node>| {
        let r = core(h, o);
        let n = r.neighbours().find(|n| n.position == target).unwrap();
        Neighbours::has_margin(n, r.busy().noise_floor_dbm())
    };
    assert!(!margin(&h));
    let heartbeats = |h: &Harness<Node>| {
        h.nodes[o]
            .received
            .iter()
            .filter(|(_, b)| {
                matches!(Packet::decode(b), Ok(Packet::Heartbeat(hb)) if hb.position == target)
            })
            .count()
    };
    let id = send(&mut h, o, d, b"the only way");
    assert!(h.run_until(h.now + secs(10), |h| delivered(h, d, id)));
    assert!(!margin(&h), "an ACK as faint as the heartbeats");
    let before = heartbeats(&h);
    h.set_rssi(o, f, -100);
    let id = send(&mut h, o, d, b"the link is better now");
    assert!(h.run_until(h.now + secs(10), |h| delivered(h, d, id)));
    assert_eq!(heartbeats(&h), before, "no heartbeat of F heard meanwhile");
    assert!(margin(&h), "F's ACK was heard well");
    no_drops(&h);
}

/// F1, O's nearest neighbour toward D, has its send queue full: X, whose
/// only neighbour it is, has handed it short messages for D until it
/// refused one, and F1's own frames are lost. F1 answers O's DATA with a refusal
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
    let (o, f1, f2, d, x) = (0, 1, 2, 3, 4);
    let mut h = build(
        vec![
            relay(0.0, 0.0, 71),
            relay(400.0, 50.0, 72),
            relay(400.0, -100.0, 73),
            relay(800.0, 0.0, 74),
            relay(400.0, 400.0, 75),
        ],
        &[(o, f1), (o, f2), (f1, d), (f2, d), (x, f1)],
    );
    let lost = std::rc::Rc::new(std::cell::Cell::new(true));
    let losing = lost.clone();
    h.lose = Some(Box::new(move |_, from, _, bytes| {
        losing.get() && from == f1 && matches!(Packet::decode(bytes), Ok(Packet::Data(_)))
    }));
    fill(&mut h, x, f1, d);
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
        .expect("F1 refused");
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

/// P, O, F in a line. F's delivery queue is full of its own application's
/// messages for its leaf L, which is not there, so F refuses P's message
/// for L (§9.2.4, §10.4). F is the only neighbour O has not
/// tried, so the message waits in O's queue until F's ban ends, and O
/// offers it to F again then, once a ban; it is not returned to P or
/// dropped (§9.2.5, roadmap D31). Meanwhile O's queue moves on: its own
/// message for P goes out. Once F's messages expire, F takes the waiting
/// one.
#[test]
fn a_relay_with_only_banned_neighbours_waits_for_the_ban() {
    use bark_proto::tables::budget::BAN;
    use bark_proto::tables::delivery::DELIVERY_QUEUE;
    let (p, o, f) = (0, 1, 2);
    let mut h = build(
        vec![
            relay(-400.0, 0.0, 120),
            relay(0.0, 0.0, 121),
            relay(400.0, 0.0, 122),
        ],
        &[(p, o), (o, f)],
    );
    let (pf, l) = (position(&h, f), LeafId([7; 4]));
    let to_l = |h: &mut Harness<Node>, from: usize, text: &[u8]| {
        h.command(
            from,
            AppCommand::Send {
                dst_pos: pf,
                dst_leaf: l,
                payload: text,
            },
        );
        MsgId::of(pf, l, text)
    };
    for k in 0..DELIVERY_QUEUE as u8 {
        to_l(&mut h, f, &[b'f', k]);
    }
    let nf = pf.short();
    let offers_to_f = |h: &Harness<Node>, id: MsgId| {
        h.nodes[o]
            .sent
            .iter()
            .filter(|(_, fr)| {
                matches!(Packet::decode(fr.payload()), Ok(Packet::Data(q)) if q.dst_hop == nf && q.msg_id() == id)
            })
            .map(|(t, _)| *t)
            .collect::<Vec<_>>()
    };
    let ended = |h: &Harness<Node>, id: MsgId| {
        h.nodes.iter().any(|n| {
            n.traces.iter().any(|(_, tr)| {
                matches!(tr, Trace::Dropped { msg_id, .. } | Trace::ReturnedToParent { msg_id, .. } if *msg_id == id)
            })
        })
    };

    let first = to_l(&mut h, p, b"first");
    let refusals = |h: &Harness<Node>| {
        h.nodes[o]
            .traces
            .iter()
            .filter_map(|(t, tr)| match tr {
                Trace::TurnedAway { msg_id, by } if *msg_id == first && *by == nf => Some(*t),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert!(h.run_until(h.now + secs(5), |h| !refusals(h).is_empty()));
    let refused_at = refusals(&h)[0];
    h.run_for(Duration::from_millis(200));
    assert!(core(&h, o).holding().any(|m| m == first), "O let it go");
    assert!(!ended(&h, first), "returned or dropped for a ban");

    // Within the ban O's queue moves on.
    assert!(h.now < refused_at + BAN);
    let second = send(&mut h, o, p, b"second");
    assert!(h.run_until(h.now + secs(1), |h| delivered(h, p, second)));

    // Offered again once each ban has ended, and never within one.
    h.run_for(secs(30));
    let (offers, refused) = (offers_to_f(&h, first), refusals(&h));
    assert!(refused.len() >= 10, "{refused:?}");
    for r in refused.iter().filter(|r| **r + BAN + secs(1) < h.now) {
        let next = offers.iter().find(|t| *t > r).expect("offered again");
        assert!(*next >= *r + BAN, "offered again within the ban");
        assert!(*next < *r + BAN + secs(1), "not offered when the ban ended");
    }
    assert!(!ended(&h, first), "returned or dropped for a ban");

    // Once F's own messages expire, it takes the message.
    let queued = |h: &Harness<Node>| {
        h.nodes[f]
            .traces
            .iter()
            .any(|(_, t)| matches!(t, Trace::Queued { msg_id, .. } if *msg_id == first))
    };
    assert!(h.run_until(h.now + secs(700), queued), "F never took it");
    assert!(!ended(&h, first));
}

/// F, O's only neighbour, has its delivery queue full of its own
/// application's messages for its leaf L, which is not there, and so
/// refuses O's message for L (§9.2.4, §10.4). O holds the
/// message rather than drop it, says it takes no other, and offers it
/// again one ban after the first refusal, two after the second, four
/// after the third, and so on up to the bucket's refill (§9.3,
/// `budget::hold`). Once F's messages expire F takes it, and O's
/// application may send again; its next message starts from one ban
/// (§9.2.5, roadmap D29).
#[test]
fn a_relay_origin_holds_its_message_for_a_banned_neighbour() {
    use bark_proto::tables::budget::{hold, BAN};
    use bark_proto::tables::delivery::DELIVERY_QUEUE;
    let (o, f) = (0, 1);
    let mut h = build(
        vec![relay(0.0, 0.0, 141), relay(400.0, 0.0, 142)],
        &[(o, f)],
    );
    let (pf, l) = (position(&h, f), LeafId([7; 4]));
    let to_l = |h: &mut Harness<Node>, from: usize, text: &[u8]| {
        h.command(
            from,
            AppCommand::Send {
                dst_pos: pf,
                dst_leaf: l,
                payload: text,
            },
        );
        MsgId::of(pf, l, text)
    };
    // Payloads are unique (§11): F remembers the messages of each fill.
    let fill = |h: &mut Harness<Node>, round: u8, n: usize| {
        for k in 0..n as u8 {
            to_l(h, f, &[b'f', round, k]);
        }
    };
    let nf = pf.short();
    let refusals = |h: &Harness<Node>, id: MsgId| {
        h.nodes[o]
            .traces
            .iter()
            .filter_map(|(t, tr)| match tr {
                Trace::TurnedAway { msg_id, by } if *msg_id == id && *by == nf => Some(*t),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let offers = |h: &Harness<Node>, id: MsgId| {
        h.nodes[o]
            .sent
            .iter()
            .filter(|(_, fr)| matches!(Packet::decode(fr.payload()), Ok(Packet::Data(q)) if q.msg_id() == id))
            .map(|(t, _)| *t)
            .collect::<Vec<_>>()
    };
    // The wait from each refusal to the offer after it.
    let waits = |h: &Harness<Node>, id: MsgId| {
        let sent = offers(h, id);
        refusals(h, id)
            .into_iter()
            .filter_map(|r| sent.iter().find(|t| **t > r).map(|t| t.since(r)))
            .collect::<Vec<_>>()
    };

    fill(&mut h, 0, DELIVERY_QUEUE);
    let id = to_l(&mut h, o, b"held");
    assert!(h.run_until(h.now + secs(1), |h| !refusals(h, id).is_empty()));
    h.run_for(Duration::from_millis(100));
    assert!(core(&h, o).holding().any(|m| m == id), "O let it go");
    assert!(!h.accepts(o), "O takes a second message while it holds one");
    h.run_for(secs(300));
    let w = waits(&h, id);
    println!("waits after each refusal {w:?}");
    assert!(w.len() >= 8, "{w:?}");
    for (j, wait) in w.iter().enumerate() {
        let due = hold(j as u8 + 1);
        assert!(
            *wait >= due && *wait < due + secs(1),
            "refusal {}: offered again after {wait:?}, not {due:?}",
            j + 1
        );
    }
    let queued = |h: &Harness<Node>, id: MsgId| {
        h.nodes[f]
            .traces
            .iter()
            .any(|(_, t)| matches!(t, Trace::Queued { msg_id, .. } if *msg_id == id))
    };
    assert!(
        h.run_until(h.now + secs(400), |h| queued(h, id)),
        "F never took it"
    );
    h.run_for(secs(1));
    assert!(h.accepts(o));
    assert!(!traces(&h, o)
        .iter()
        .any(|t| matches!(t, Trace::Dropped { msg_id, .. } if *msg_id == id)));

    // Taken, the hold starts again from one ban.
    fill(&mut h, 1, DELIVERY_QUEUE - 1);
    let next = to_l(&mut h, o, b"next");
    assert!(h.run_until(h.now + secs(5), |h| !waits(h, next).is_empty()));
    let w = waits(&h, next)[0];
    assert!(w >= BAN && w < hold(2), "offered again after {w:?}");
}

/// O's only neighbour C is gone. O's own message fails its three
/// attempts there, which bans C (§9.3, roadmap D42); with no other
/// neighbour the message waits for each ban and is offered to C again,
/// and O takes no other message meanwhile (§9.2.5). Once C's entry
/// expires (§8.2), O has no neighbour, drops the message for want of a
/// route, and is ready again.
#[test]
fn a_relay_origin_whose_neighbour_is_gone_waits_then_drops() {
    let config = Config::default();
    let (o, c) = (0, 1);
    let mut h = build(
        vec![relay(0.0, 0.0, 151), relay(400.0, 0.0, 152)],
        &[(o, c)],
    );
    h.kill(c);
    let gone = h.now;
    let id = send(&mut h, o, c, b"nobody answers");
    let dropped = Trace::Dropped {
        msg_id: id,
        reason: DropReason::NoRoute,
    };
    let expiry = NeighbourParams::from_config(&config).expire_after;
    let offers = |h: &Harness<Node>| {
        h.nodes[o]
            .sent
            .iter()
            .filter(|(_, f)| {
                matches!(Packet::decode(f.payload()), Ok(Packet::Data(q)) if q.msg_id() == id)
            })
            .count()
    };
    h.run_for(secs(60));
    assert!(!h.accepts(o));
    assert!(!traces(&h, o).contains(&dropped));
    assert!(offers(&h) >= 3 * 4, "offered {} times", offers(&h));
    // Routed again when the ban running then ends, at most the bucket's
    // refill later (`budget::hold`).
    let last_ban = hold(u8::MAX);
    assert!(
        h.run_until(gone + expiry + last_ban + secs(1), |h| traces(h, o)
            .contains(&dropped))
    );
    assert!(
        h.now + config.heartbeat_max >= gone + expiry,
        "dropped before C expired"
    );
    h.run_for(secs(1));
    assert!(h.accepts(o));
    assert!(h.nodes[o].app.is_empty());
}

/// P, R, C, D in a line, and C is gone. R's only candidate fails and is
/// banned, longer after each failure (roadmap D42), and R waits for each
/// ban rather than give up on C. Once its entry for C expires (§8.2), R's
/// search ends as before: it returns the message to P, which has no
/// other neighbour and, as the origin, drops it (§9.2.5).
#[test]
fn a_relay_whose_candidates_are_gone_returns_to_the_parent() {
    let config = Config::default();
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
    h.kill(c);
    let killed = h.now;
    let id = send(&mut h, p, d, b"no way on");
    let gone = |h: &Harness<Node>| {
        traces(h, p)
            .iter()
            .any(|t| matches!(t, Trace::Dropped { msg_id, .. } if *msg_id == id))
    };
    let expiry = NeighbourParams::from_config(&config).expire_after;
    h.run_for(secs(60));
    assert!(!gone(&h), "P dropped it while C was banned");
    assert!(core(&h, r).holding().any(|m| m == id));
    assert!(
        h.run_until(killed + expiry + hold(u8::MAX) + secs(10), gone),
        "P did not drop it"
    );
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
}

/// P hands a message to C and moves its home channel (§6.3) while C is
/// still searching; C then returns the message to P's new home, as C's
/// neighbour table has it from P's announcement, not to the channel P
/// was on when it handed the message over (§9.2.5). P then tries Y, which
/// reaches D. C searches for a while because its four other neighbours,
/// X, are gone: each costs three unanswered attempts and is banned, and
/// the message waits for the bans (roadmap D42) until C's entries for them
/// expire. C listens on P's old home, so it hears the announcement.
///
/// ```text
///        X X X X
///         \ \/ /
///           C          D (1000, 0)
///          /          /
///         P -------- Y
/// ```
#[test]
fn a_return_goes_to_the_parent_s_home_as_it_is_now() {
    use bark_proto::tables::busy::percent;
    use bark_proto::{Action, Channel, CHANNELS};
    let (p, c, y, d) = (0, 1, 2, 3);
    let xs = [4, 5, 6, 7];
    // Move once the home has been busy this long, at once (§6.3: 10 min
    // and up to 5 min more, which only make the test longer).
    let dwell = secs(20);
    let topology = |c_home: u8| {
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
        let mut nodes = vec![
            Node::Relay(Relay::new(
                Config {
                    position: pos(0.0, 0.0),
                    boot_listen: secs(5),
                    move_dwell: dwell,
                    move_wait_max: Duration::ZERO,
                    ..Config::default()
                },
                141,
            )),
            fixed(400.0, 100.0, c_home, 142),
            fixed(300.0, -300.0, 5, 143),
            fixed(1000.0, 0.0, 7, 144),
        ];
        for (k, east) in [200.0, 400.0, 600.0, 800.0].into_iter().enumerate() {
            nodes.push(fixed(east, 500.0, 8 + k as u8, 145 + k as u64));
        }
        let mut links = vec![(p, c), (p, y), (y, d)];
        links.extend(xs.iter().map(|&x| (c, x)));
        build(nodes, &links)
    };
    // C listens where P chooses to: find out where that is.
    let old = core(&topology(0), p).home();
    let mut h = topology(old.index());
    assert_eq!(core(&h, p).home(), old);
    for x in xs {
        h.unlink(c, x);
    }

    // A carrier on P's home for 30 % of the time (as in `tests/mac.rs`).
    h.jam_part(
        p,
        old,
        Duration::from_millis(32),
        Duration::from_millis(107),
    );
    // The move rule runs at the end of each round of the sweep, after the
    // reading of the last channel (`relay.rs`); P moves `dwell` after the
    // first round that ends with its home too busy.
    let last = Action::Rssi(Channel::new(CHANNELS - 1).unwrap());
    let round_ends_busy = |h: &Harness<Node>| {
        let b = core(h, p).busy();
        let min = Channel::all().map(|c| b.fraction(c)).min().unwrap();
        let busy = b.fraction(old).saturating_sub(min) > percent(5);
        let round = h.nodes[p]
            .checks
            .last()
            .is_some_and(|&(_, end, a, _)| a == last && end == h.now);
        busy && round
    };
    assert!(h.run_until(h.now + secs(1800), round_ends_busy));
    h.run_for(dwell - Duration::from_millis(300));
    let id = send(&mut h, p, d, b"after the move");
    // C's entries for the X expire three rotations after their last
    // heartbeat (§8.2: 390 s); until then C waits for their bans.
    assert!(
        h.run_until(h.now + secs(450), |h| delivered(h, d, id)),
        "not delivered"
    );

    let moved = h.nodes[p].traces.iter().rev().find_map(|(t, tr)| match tr {
        Trace::HomeChannelMoved { to, .. } => Some((*t, *to)),
        _ => None,
    });
    let (moved_at, new) = moved.expect("P did not move");
    assert_ne!(new, old);
    let data_to = |i: usize, to: usize| {
        let to = position(&h, to).short();
        h.nodes[i]
            .sent
            .iter()
            .filter_map(|(t, fr)| match Packet::decode(fr.payload()) {
                Ok(Packet::Data(q)) if q.msg_id() == id && q.dst_hop == to => {
                    Some((*t, fr.channel))
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let (handed_at, _) = data_to(p, c)[0];
    assert!(handed_at < moved_at, "P moved before handing to C");
    let returns = data_to(c, p);
    assert_eq!(returns.len(), 1, "{returns:?}");
    let (returned_at, channel) = returns[0];
    assert!(returned_at > moved_at, "C returned before P moved");
    assert_eq!(channel, new, "returned to P's old home");
    assert!(traces(&h, c).contains(&Trace::ReturnedToParent {
        msg_id: id,
        to: position(&h, p).short()
    }));
    no_drops(&h);
}

/// Twenty-six relays, F their only neighbour, each forwarding for others,
/// offer F a new short message each time their ban of F ends, for an hour
/// and five minutes; F, its frames to D all lost, keeps its send queue
/// full and refuses nearly all of them. A forward is not held
/// (§9.2.5): each sender asks for one reply per ban, 1.53 s, together
/// seventeen a second, 18 % of F's time on air against the 9.83 % its
/// bucket refills. So the replies run down the reply reserve, and F stays
/// silent rather than spend the heartbeat reserve (§9.4, roadmap D27,
/// D29). No hour holds more than 360 s, replies included (the harness
/// checks every transmission), and no heartbeat of F is held back. The
/// senders' frames are put on the air past their cores, which hold their
/// own application's message for longer after each refusal (§9.3).
#[test]
fn replies_stay_within_the_reserve() {
    use bark_proto::radio::{DUTY_CYCLE_WINDOW, MAX_ERP_DBM, MIN_PREAMBLE_SYMBOLS};
    use bark_proto::tables::budget::{ACK_AIRTIME, BAN, BUDGET};
    use bark_proto::wire::Data;
    use bark_proto::{Frame, MAX_FRAME};
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
    let (pf, pd) = (position(&h, f), position(&h, d));
    let step = BAN.div(senders.len() as u64);
    let mut k = 0u32;
    while h.now < end {
        for (i, &s) in senders.iter().enumerate() {
            let text = k.to_be_bytes();
            let data = Data {
                sender_home: ch(2 + (i % 11) as u8),
                dst_hop: pf.short(),
                src_hop: position(&h, s).short(),
                dst_pos: pd,
                dst_leaf: LeafId::RELAY,
                hops: 1,
                payload: &text,
            };
            let mut frame = Frame {
                channel: ch(0),
                preamble_symbols: MIN_PREAMBLE_SYMBOLS,
                power_dbm: MAX_ERP_DBM,
                len: 0,
                bytes: [0; MAX_FRAME],
            };
            frame.len = Packet::Data(data).encode(&mut frame.bytes).unwrap() as u8;
            h.inject(s, frame);
            k += 1;
            let next = h.now + step;
            h.run_until(next, |_| false);
        }
    }
    let sent = &h.nodes[f].sent;
    let busy = sent
        .iter()
        .filter(|(_, fr)| matches!(Packet::decode(fr.payload()), Ok(Packet::Ack(a)) if a.refused))
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
        "{refused} refused, {busy} refusals sent ({:.1} s), {:.1} s on air in the last hour",
        (ACK_AIRTIME.as_micros() * busy as u64) as f64 / 1e6,
        spent.as_micros() as f64 / 1e6
    );
    assert!(spent <= BUDGET);
    assert!(
        spent > BUDGET.mul(9).div(10),
        "F was not held at its cap: {spent:?}"
    );
    assert!(
        refused > busy,
        "the reserve never ran out: {refused} refused, {busy} refusals"
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
/// refuses a message when the message's frame and everything it
/// has queued would not fit in the bucket (§9.2.4), and a hand-over whose
/// retries the bucket cannot fit yet waits for it, never longer than the
/// refill of a frame and the reply reserve, instead of being dropped. Its
/// heartbeats are never held back. The harness panics if any hour holds
/// more than 10 %, replies included.
///
/// O, refused, bans F without counting a failure (§9.3); F is its only
/// neighbour, so O holds its application's message and offers it again
/// after its hold, and its application waits (§9.2.5, §9.3). O and D
/// cannot hear each other: now and then D's ACKs to F are lost under O's
/// frames, all four of F's attempts go unanswered, and F returns the
/// message to O as the search requires (§9.2.5). O, its bucket spent on
/// offering its held message once a ban, may refuse the return, and F
/// then drops it: the only drops F makes.
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
    // O's application has a message every 2.5 s. It holds one at a time
    // and offers it the moment O takes one (core-interface §4); one due
    // while it still waits is skipped. Latency counts from when it was due.
    let end = h.now + DUTY_CYCLE_WINDOW.mul(3);
    let period = Duration::from_millis(2500);
    let (mut k, mut skipped, mut waited, mut longest_wait) = (0u32, 0u32, 0u32, Duration::ZERO);
    let mut sent_at = std::collections::HashMap::new();
    let mut taken_at = std::collections::HashMap::new();
    let mut due = h.now;
    let mut waiting: Option<Instant> = None;
    loop {
        if due < end && h.now >= due {
            if waiting.is_some() {
                skipped += 1;
            } else {
                waiting = Some(due);
            }
            due += period;
        }
        if let Some(first) = waiting.filter(|_| h.accepts(o)) {
            let mut text = [0u8; MAX_PAYLOAD];
            text[..4].copy_from_slice(&k.to_be_bytes());
            let id = send(&mut h, o, d, &text);
            sent_at.insert(id, first);
            taken_at.insert(id, h.now);
            k += 1;
            let wait = h.now.since(first);
            if wait > Duration::ZERO {
                waited += 1;
                longest_wait = longest_wait.max(wait);
            }
            waiting = None;
        }
        if due >= end && waiting.is_none() {
            break;
        }
        let holding = waiting.is_some();
        let until = if due < end { due } else { h.now + secs(1) };
        h.run_until(until, |h| holding && h.accepts(o));
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
        .filter(|(_, fr)| matches!(Packet::decode(fr.payload()), Ok(Packet::Ack(a)) if a.refused))
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
        "F forwarded {}, returned {}, deferred {} for budget, refused {full} with \
         its queue full and {broke} for its bucket, {busy_sent} refusals sent; \
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
        .map(|(t, AppEvent::Message { msg_id, .. })| t.since(sent_at[msg_id]))
        .collect();
    latency.sort();
    let mut since_taken: Vec<Duration> = h.nodes[d]
        .app
        .iter()
        .map(|(t, AppEvent::Message { msg_id, .. })| t.since(taken_at[msg_id]))
        .collect();
    since_taken.sort();
    let mut offered = to_f.clone();
    offered.sort_by_key(|m| m.0);
    offered.dedup();
    let mut o_drops = std::collections::BTreeMap::new();
    for t in traces(&h, o) {
        if let Trace::Dropped { reason, .. } = t {
            *o_drops.entry(format!("{reason:?}")).or_insert(0) += 1;
        }
    }
    println!(
        "O's application offered {k}, {waited} after waiting, longest {longest_wait:?}, \
         skipped {skipped}; longest gap between F's DATA frames {gap:?}; D's ACKs lost at F {acks_lost}; \
         O's frames to F {}, of which retries {}; O dropped {o_drops:?}; \
         F dropped {} returns O refused; \
         D received {} messages, latency from when due median {:?}, max {:?}, \
         from when O took it median {:?}, max {:?}",
        to_f.len(),
        to_f.len() - offered.len(),
        tf.iter()
            .filter(|t| matches!(t, Trace::Dropped { reason: DropReason::ReturnFailed, .. }))
            .count(),
        latency.len(),
        latency.get(latency.len() / 2),
        latency.last(),
        since_taken.get(since_taken.len() / 2),
        since_taken.last(),
    );
    assert!(full + broke > 0, "F never refused a message");
    assert!(busy_sent > 0 && turned_away > 0, "no refusal reached O");
    assert!(
        !deferred.is_empty(),
        "the budget never held a hand-over back"
    );
    let return_failed = |id: MsgId| {
        tf.contains(&Trace::Dropped {
            msg_id: id,
            reason: DropReason::ReturnFailed,
        })
    };
    assert!(
        deferred.iter().all(|&id| handed(id) || return_failed(id)),
        "a deferred message was neither forwarded nor returned"
    );
    let drops: Vec<_> = tf
        .iter()
        .filter(
            |t| matches!(t, Trace::Dropped { reason, .. } if *reason != DropReason::ReturnFailed),
        )
        .collect();
    assert!(drops.is_empty(), "F dropped: {drops:?}");
    assert!(
        !tf.iter()
            .any(|t| matches!(t, Trace::HeartbeatDeferred { .. })),
        "a heartbeat was held back"
    );
}

// --- Silence is pushback (§9.3 step 6, roadmap D42) ---

/// When relay `from` sent a DATA of `id` to relay `to`.
fn data_times(h: &Harness<Node>, from: usize, to: usize, id: MsgId) -> Vec<Instant> {
    let to = position(h, to).short();
    h.nodes[from]
        .sent
        .iter()
        .filter_map(|(t, f)| match Packet::decode(f.payload()) {
            Ok(Packet::Data(q)) if q.dst_hop == to && q.msg_id() == id => Some(*t),
            _ => None,
        })
        .collect()
}

/// `times` split where two frames are a ban or more apart: the
/// attempts of one hand-over each, as a retry waits less (§9.3).
fn handovers(times: &[Instant]) -> Vec<Vec<Instant>> {
    let mut out: Vec<Vec<Instant>> = Vec::new();
    for &t in times {
        match out.last_mut() {
            Some(run) if t.since(*run.last().unwrap()) < BAN => run.push(t),
            _ => out.push(vec![t]),
        }
    }
    out
}

fn entry(h: &Harness<Node>, at: usize, of: usize) -> Neighbour {
    let p = position(h, of);
    *core(h, at).neighbours().find(|n| n.position == p).unwrap()
}

/// O, with F1 and F2 both reaching D, F1 the nearer. Every DATA from O
/// to F1 is lost. The first message's three attempts go unanswered: F1
/// is banned for one ban (§9.3, roadmap D42) and the message goes to F2
/// at once. A second message within the ban is not offered to F1 at all.
/// After the ban, with the link back, F1 takes the third, and its ACK
/// clears its failure count.
///
/// ```text
///          F1
///        /    \
///       O      D
///        \    /
///          F2
/// ```
#[test]
fn an_unanswered_hand_over_bans_the_neighbour_for_every_message() {
    let (o, f1, f2, d) = (0, 1, 2, 3);
    let mut h = build(
        vec![
            relay(0.0, 0.0, 201),
            relay(400.0, 50.0, 202),
            relay(400.0, -100.0, 203),
            relay(800.0, 0.0, 204),
        ],
        &[(o, f1), (o, f2), (f1, d), (f2, d)],
    );
    let lost = std::rc::Rc::new(std::cell::Cell::new(true));
    let losing = lost.clone();
    h.lose = Some(Box::new(move |_, from, to, bytes| {
        losing.get()
            && (from, to) == (o, f1)
            && matches!(Packet::decode(bytes), Ok(Packet::Data(_)))
    }));
    let name = |h: &Harness<Node>, i| position(h, i).short();
    let first = send(&mut h, o, d, b"first");
    assert!(h.run_until(h.now + secs(5), |h| delivered(h, d, first)));
    let tried = data_times(&h, o, f1, first);
    assert_eq!(tried.len(), 3);
    let failed_at = *tried.last().unwrap();
    let to_f2 = data_times(&h, o, f2, first)[0];
    assert!(to_f2.since(failed_at) < secs(1), "not at once");
    let e = entry(&h, o, f1);
    assert_eq!(e.fail_count, 1);
    // The ban starts at the third attempt's ACK timeout.
    let ban_from = e.banned_until.since(failed_at);
    assert!(
        ban_from > BAN && ban_from < BAN + Duration::from_millis(300),
        "{ban_from:?}"
    );

    let second = send(&mut h, o, d, b"second");
    assert!(h.now < e.banned_until);
    assert!(h.run_until(h.now + secs(5), |h| delivered(h, d, second)));
    assert!(
        data_times(&h, o, f1, second).is_empty(),
        "offered within the ban"
    );
    assert_eq!(forwards(&h, o, second), vec![name(&h, f2)]);

    lost.set(false);
    h.run_for(secs(5));
    let third = send(&mut h, o, d, b"third");
    assert!(h.run_until(h.now + secs(5), |h| delivered(h, d, third)));
    assert_eq!(forwards(&h, o, third), vec![name(&h, f1)]);
    assert_eq!(entry(&h, o, f1).fail_count, 0);
    no_drops(&h);
}

/// P, O, M, D in a line; every DATA from O to M is lost. P's message
/// waits at O, M being its only candidate, and is offered to M again as
/// each ban ends: one ban after the first failure, two after the second,
/// four after the third, and so on up to the bucket's refill, 61 s (§9.3,
/// `budget::hold`). Once M's link is back M takes it, which clears its
/// failure count, and the next failure bans it for one ban again.
#[test]
fn the_ban_after_failures_in_a_row_doubles_up_to_the_bucket_s_refill() {
    let (p, o, m, d) = (0, 1, 2, 3);
    let mut h = build(
        vec![
            relay(0.0, 0.0, 211),
            relay(400.0, 0.0, 212),
            relay(800.0, 0.0, 213),
            relay(1200.0, 0.0, 214),
        ],
        &[(p, o), (o, m), (m, d)],
    );
    let lost = std::rc::Rc::new(std::cell::Cell::new(true));
    let losing = lost.clone();
    h.lose = Some(Box::new(move |_, from, to, bytes| {
        losing.get() && (from, to) == (o, m) && matches!(Packet::decode(bytes), Ok(Packet::Data(_)))
    }));
    let id = send(&mut h, p, d, b"backing off");
    h.run_for(secs(240));
    let runs = handovers(&data_times(&h, o, m, id));
    assert!(runs.len() >= 9, "{} hand-overs", runs.len());
    for (k, pair) in runs.windows(2).enumerate() {
        assert_eq!(pair[0].len(), 3, "hand-over {}: {:?}", k + 1, pair[0]);
        let gap = pair[1][0].since(*pair[0].last().unwrap());
        let due = hold(k as u8 + 1);
        assert!(
            gap >= due && gap < due + secs(1),
            "after failure {}: {gap:?}, not {due:?}",
            k + 1
        );
    }
    assert_eq!(hold(8), hold(u8::MAX), "capped by the eighth");
    assert!(!delivered(&h, d, id));

    lost.set(false);
    assert!(h.run_until(h.now + secs(70), |h| delivered(h, d, id)));
    assert_eq!(entry(&h, o, m).fail_count, 0);
    lost.set(true);
    let next = send(&mut h, p, d, b"from one ban again");
    h.run_for(secs(10));
    let runs = handovers(&data_times(&h, o, m, next));
    assert!(runs.len() >= 2, "{runs:?}");
    let gap = runs[1][0].since(*runs[0].last().unwrap());
    assert!(gap >= BAN && gap < hold(2), "{gap:?}");
}

/// R, P, O, M, D in a line. P's message reaches O first and R's, which
/// has come one hand-over farther, after it. O's first three frames to M
/// are lost, so M is banned, and both messages wait for it, M being O's
/// only candidate for either (§9.3). When the ban ends the most travelled
/// goes first, R's, though it arrived second (§9.3, roadmap D31), and
/// then P's.
#[test]
fn messages_waiting_for_a_failure_ban_go_most_travelled_first() {
    let (r, p, o, m, d) = (0, 1, 2, 3, 4);
    let mut h = build(
        vec![
            relay(0.0, 0.0, 221),
            relay(400.0, 0.0, 222),
            relay(800.0, 0.0, 223),
            relay(1200.0, 0.0, 224),
            relay(1600.0, 0.0, 225),
        ],
        &[(r, p), (p, o), (o, m), (m, d)],
    );
    let mut lost = 0;
    h.lose = Some(Box::new(move |_, from, to, bytes| {
        let data = (from, to) == (o, m) && matches!(Packet::decode(bytes), Ok(Packet::Data(_)));
        let lose = data && lost < 3;
        lost += usize::from(lose);
        lose
    }));
    let near = send(&mut h, p, d, b"from P");
    let far = send(&mut h, r, d, b"from R");
    assert!(h.run_until(h.now + secs(10), |h| delivered(h, d, near)
        && delivered(h, d, far)));
    let first = data_times(&h, o, m, near);
    assert!(first.len() >= 4, "three lost, then taken");
    let failed_at = first[2];
    let after = |id| {
        data_times(&h, o, m, id)
            .into_iter()
            .find(|t| *t > failed_at)
    };
    let (f, n) = (after(far).unwrap(), after(near).unwrap());
    assert!(f < n, "the less travelled went first");
    assert!(f.since(failed_at) >= BAN && f.since(failed_at) < BAN + secs(1));
    let hops = |id| {
        h.nodes[o]
            .sent
            .iter()
            .rev()
            .find_map(|(_, fr)| match Packet::decode(fr.payload()) {
                Ok(Packet::Data(q)) if q.msg_id() == id => Some(q.hops),
                _ => None,
            })
    };
    assert!(hops(far) > hops(near));
    no_drops(&h);
}

/// F1 is gone. O's first message tries it three times and goes to F2
/// at once; F1 is banned for one ban (roadmap D42). The second message,
/// after that ban, tries F1 again, and that second failure demotes it
/// (§8.2) and goes to F2 at once too. The third goes to F2 without a
/// frame to F1: a dead neighbour is routed around (roadmap D25).
#[test]
fn a_dead_neighbour_is_routed_around() {
    let (o, f1, f2, d) = (0, 1, 2, 3);
    let mut h = build(
        vec![
            relay(0.0, 0.0, 231),
            relay(400.0, 50.0, 232),
            relay(400.0, -100.0, 233),
            relay(800.0, 0.0, 234),
        ],
        &[(o, f1), (o, f2), (f1, d), (f2, d)],
    );
    h.kill(f1);
    let mut tried = Vec::new();
    for text in [b"one", b"two", b"six"] {
        let id = send(&mut h, o, d, text);
        assert!(h.run_until(h.now + secs(5), |h| delivered(h, d, id)));
        let to_f1 = data_times(&h, o, f1, id);
        if let (Some(last), Some(&on)) = (to_f1.last(), data_times(&h, o, f2, id).first()) {
            assert!(on.since(*last) < secs(1), "not at once");
        }
        tried.push(to_f1.len());
        h.run_for(secs(10));
    }
    assert_eq!(tried, vec![3, 3, 0]);
    assert!(traces(&h, o).contains(&Trace::Demoted {
        neighbour: position(&h, f1).short()
    }));
    no_drops(&h);
}
