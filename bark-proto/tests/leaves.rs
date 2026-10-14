//! Leaves and relays together (design §9, §10), driven through real bytes
//! on the test medium, with the design's timings throughout.

mod common;

use bark_proto::wire::{Packet, ACK_LEN};
use bark_proto::{
    AppCommand, AppEvent, Config, DropReason, Duration, Instant, Leaf, LeafId, MsgId, Node, Position, Relay, Trace,
};
use common::{secs, Harness};

fn pos(east: f64, north: f64) -> Position {
    let (lat, lon) = (50.45, 30.52);
    let m_per_deg = 6_371_000.0 * core::f64::consts::PI / 180.0;
    Position::from_degrees(lat + north / m_per_deg, lon + east / (m_per_deg * lat.to_radians().cos()))
}

fn relay(east: f64, seed: u64) -> Node {
    Node::Relay(Relay::new(Config { position: pos(east, 0.0), ..Config::default() }, seed))
}

fn leaf(east: f64, id: u8, seed: u64) -> Node {
    Node::Leaf(Leaf::new(Config { position: pos(east, 0.0), leaf_id: LeafId([id; 4]), ..Config::default() }, seed))
}

fn the_relay(h: &Harness<Node>, i: usize) -> &Relay {
    match &h.nodes[i].core {
        Node::Relay(r) => r,
        Node::Leaf(_) => panic!("node {i} is a leaf"),
    }
}

fn the_leaf(h: &Harness<Node>, i: usize) -> &Leaf {
    match &h.nodes[i].core {
        Node::Leaf(l) => l,
        Node::Relay(_) => panic!("node {i} is a relay"),
    }
}

fn position(h: &Harness<Node>, i: usize) -> Position {
    the_relay(h, i).config().position
}

fn attached(h: &Harness<Node>, l: usize, r: usize) -> bool {
    the_leaf(h, l).relay() == Some(position(h, r))
}

/// Leaf `from` sends `text` to leaf `to` at relay `at`.
fn send(h: &mut Harness<Node>, from: usize, to: usize, at: usize, text: &[u8]) -> MsgId {
    let (dst_pos, dst_leaf) = (position(h, at), the_leaf(h, to).id());
    h.command(from, AppCommand::Send { dst_pos, dst_leaf, payload: text });
    MsgId::of(dst_pos, dst_leaf, text)
}

fn reports(h: &Harness<Node>, i: usize, id: MsgId) -> Vec<AppEvent> {
    h.nodes[i]
        .app
        .iter()
        .map(|(_, e)| *e)
        .filter(|e| match e {
            AppEvent::Accepted(m) | AppEvent::Sent(m) => *m == id,
            AppEvent::Failed { msg_id, .. } | AppEvent::Message { msg_id, .. } => *msg_id == id,
        })
        .collect()
}

fn received(h: &Harness<Node>, i: usize, id: MsgId) -> usize {
    h.nodes[i].app.iter().filter(|(_, e)| matches!(e, AppEvent::Message { msg_id, .. } if *msg_id == id)).count()
}

fn dropped(h: &Harness<Node>, i: usize, id: MsgId) -> Option<(Instant, DropReason)> {
    h.nodes[i].traces.iter().find_map(|(t, tr)| match tr {
        Trace::Dropped { msg_id, reason } if *msg_id == id => Some((*t, *reason)),
        _ => None,
    })
}

/// Run until every leaf is attached to its relay: the relays' boot listen
/// (§6.2), then up to a rotation for each leaf to hear its relay (§10.1).
fn settle(h: &mut Harness<Node>, pairs: &[(usize, usize)]) {
    let pairs = pairs.to_vec();
    let deadline = Instant::ZERO + secs(150 + 2 * 195);
    assert!(h.run_until(deadline, |h| pairs.iter().all(|&(l, r)| attached(h, l, r))), "leaves did not attach");
}

/// Two relays and a leaf at each: a message from one leaf reaches the
/// other. The origin leaf hears `Accepted` and `Sent`, the destination leaf
/// `Message`, once.
#[test]
fn leaf_to_leaf_across_two_relays() {
    let (r1, r2, l1, l2) = (0, 1, 2, 3);
    let mut h = Harness::new(vec![relay(0.0, 1), relay(400.0, 2), leaf(-30.0, 1, 3), leaf(430.0, 2, 4)]);
    for (a, b) in [(r1, r2), (l1, r1), (l2, r2)] {
        h.link(a, b);
    }
    settle(&mut h, &[(l1, r1), (l2, r2)]);
    // Wait until the relays know each other (§7).
    let knows = |h: &Harness<Node>| the_relay(h, r1).neighbours().any(|n| n.position == position(h, r2));
    assert!(h.run_until(h.now + secs(400), knows));
    let start = h.now;
    let id = send(&mut h, l1, l2, r2, b"hello across");
    assert!(h.run_until(h.now + secs(5), |h| received(h, l2, id) > 0), "not delivered");
    println!("delivered in {} ms", h.now.since(start).as_millis());
    assert_eq!(reports(&h, l1, id), vec![AppEvent::Accepted(id), AppEvent::Sent(id)]);
    h.run_for(secs(30));
    assert_eq!(received(&h, l2, id), 1);
    let AppEvent::Message { payload, len, .. } = reports(&h, l2, id)[0] else { panic!() };
    assert_eq!(&payload[..len as usize], b"hello across");
    assert!(the_relay(&h, r2).config().position == position(&h, r2));
    assert!(h.nodes[r2].traces.iter().any(|(_, t)| *t == Trace::Delivered { msg_id: id, leaf: LeafId([2; 4]) }));
}

/// The destination leaf's first ACK of the DELIVER is lost: the relay
/// sends it again, the leaf ACKs again and surfaces it once (§10.3).
#[test]
fn a_repeated_deliver_is_acked_and_surfaced_once() {
    let (r, l1, l2) = (0, 1, 2);
    let mut h = Harness::new(vec![relay(0.0, 5), leaf(-30.0, 1, 6), leaf(30.0, 2, 7)]);
    h.link(l1, r);
    h.link(l2, r);
    settle(&mut h, &[(l1, r), (l2, r)]);
    let mut lost = false;
    h.lose = Some(Box::new(move |_, from, to, bytes| {
        let ack = from == l2 && to == r && bytes.len() == ACK_LEN && !lost;
        lost |= ack;
        ack
    }));
    let id = send(&mut h, l1, l2, r, b"twice?");
    let acks = |h: &Harness<Node>| {
        h.nodes[l2].sent.iter().filter(|(_, f)| matches!(Packet::decode(f.payload()), Ok(Packet::Ack(a)) if a.msg_id == id)).count()
    };
    assert!(h.run_until(h.now + secs(5), |h| acks(h) == 2), "no second ACK");
    h.run_for(secs(70));
    assert_eq!(acks(&h), 2);
    assert_eq!(received(&h, l2, id), 1);
    assert_eq!(reports(&h, l1, id), vec![AppEvent::Accepted(id), AppEvent::Sent(id)]);
}

/// Three relays in a line with a leaf at each end (design §15 step 4).
/// Messages are delivered; then the middle relay is unplugged, and every
/// later message is accepted by A and dropped there for want of a route
/// (§9.2.5), within the per-hop timeouts of §9.3.
#[test]
fn line_of_three_loses_its_middle() {
    let c = Config::default();
    let (a, b, cc, la, lc) = (0, 1, 2, 3, 4);
    let mut h = Harness::new(vec![relay(0.0, 11), relay(400.0, 12), relay(800.0, 13), leaf(-50.0, 1, 14), leaf(850.0, 2, 15)]);
    for (x, y) in [(a, b), (b, cc), (la, a), (lc, cc)] {
        h.link(x, y);
    }
    settle(&mut h, &[(la, a), (lc, cc)]);
    let line = |h: &Harness<Node>| {
        the_relay(h, a).neighbours().any(|n| n.position == position(h, b))
            && the_relay(h, b).neighbours().any(|n| n.position == position(h, cc))
    };
    assert!(h.run_until(h.now + secs(400), line));
    let interval = secs(30);
    for k in 0..3u8 {
        let id = send(&mut h, la, lc, cc, &[b'm', k]);
        let start = h.now;
        assert!(h.run_until(start + secs(5), |h| received(h, lc, id) == 1), "message {k} not delivered");
        println!("before: delivered in {} ms", h.now.since(start).as_millis());
        h.run_until(start + interval, |_| false);
    }
    h.kill(b);
    println!("B removed at {:.1} s", h.now.as_micros() as f64 / 1e6);
    // A hand-over attempt is at most the 500 ms listen-before-talk give-up,
    // a frame and the ACK timeout; three of them, with their backoffs, end
    // in a failure (§9.3).
    let per_hop = c.hop_backoff_max + bark_proto::tables::attempt_span(c.ack_timeout);
    let bound = per_hop.mul(c.hop_attempts as u64);
    let mut times = Vec::new();
    for k in 0..20u8 {
        let id = send(&mut h, la, lc, cc, &[b'n', k]);
        let start = h.now;
        let done = h.run_until(start + bound + secs(1), |h| dropped(h, a, id).is_some());
        assert!(done, "message {k} after the removal was not dropped at A");
        let (t, reason) = dropped(&h, a, id).unwrap();
        assert_eq!(reason, DropReason::NoRoute);
        times.push(t.since(start));
        h.run_until(start + interval, |_| false);
        assert_eq!(reports(&h, la, id), vec![AppEvent::Accepted(id), AppEvent::Sent(id)], "A took it");
    }
    let ms: Vec<u64> = times.iter().map(|d| d.as_millis()).collect();
    println!("after: dropped at A after {ms:?} ms (bound {} ms)", bound.as_millis());
    assert!(times[0] < bound + Duration::from_millis(500), "the first waits out three attempts");
    // B is demoted after two failed hand-overs (§8.2): from then on A has
    // no candidate and drops as soon as the leaf's message arrives.
    assert!(h.nodes[a].traces.iter().any(|(_, t)| matches!(t, Trace::Demoted { .. })));
    assert!(times[2..].iter().all(|t| *t < Duration::from_millis(200)), "{ms:?}");
    for k in 0..20u8 {
        assert_eq!(received(&h, lc, MsgId::of(position(&h, cc), LeafId([2; 4]), &[b'n', k])), 0);
    }
}
