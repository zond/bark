//! Leaves and relays together (design §9, §10), driven through real bytes
//! on the test medium, with the design's timings throughout.

mod common;

use bark_proto::wire::{Packet, ACK_LEN};
use bark_proto::{
    AppCommand, AppEvent, Config, DropReason, Duration, Instant, Leaf, LeafId, MsgId, Node,
    Position, Relay, Trace,
};
use common::{secs, Harness};

fn pos(east: f64, north: f64) -> Position {
    let (lat, lon) = (50.45, 30.52);
    let m_per_deg = 6_371_000.0 * core::f64::consts::PI / 180.0;
    Position::from_degrees(
        lat + north / m_per_deg,
        lon + east / (m_per_deg * lat.to_radians().cos()),
    )
}

fn relay(east: f64, seed: u64) -> Node {
    Node::Relay(Relay::new(
        Config {
            position: pos(east, 0.0),
            ..Config::default()
        },
        seed,
    ))
}

fn leaf(east: f64, id: u8, seed: u64) -> Node {
    Node::Leaf(Leaf::new(
        Config {
            position: pos(east, 0.0),
            leaf_id: LeafId([id; 4]),
            ..Config::default()
        },
        seed,
    ))
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
    h.command(
        from,
        AppCommand::Send {
            dst_pos,
            dst_leaf,
            payload: text,
        },
    );
    MsgId::of(dst_pos, dst_leaf, text)
}

/// What node `i`'s application heard about message `id`.
fn reports(h: &Harness<Node>, i: usize, id: MsgId) -> Vec<AppEvent> {
    h.nodes[i]
        .app
        .iter()
        .map(|(_, e)| *e)
        .filter(|AppEvent::Message { msg_id, .. }| *msg_id == id)
        .collect()
}

/// Leaf `l` handed message `id` to its relay: it holds it no longer, and
/// its application was told nothing (`docs/core-interface.md` §4).
fn handed_over(h: &Harness<Node>, l: usize, id: MsgId) -> bool {
    the_leaf(h, l).holding() != Some(id) && reports(h, l, id).is_empty()
}

fn received(h: &Harness<Node>, i: usize, id: MsgId) -> usize {
    h.nodes[i]
        .app
        .iter()
        .filter(|(_, e)| matches!(e, AppEvent::Message { msg_id, .. } if *msg_id == id))
        .count()
}

fn dropped(h: &Harness<Node>, i: usize, id: MsgId) -> Option<(Instant, DropReason)> {
    h.nodes[i].traces.iter().find_map(|(t, tr)| match tr {
        Trace::Dropped { msg_id, reason } if *msg_id == id => Some((*t, *reason)),
        _ => None,
    })
}

/// Run until every leaf is attached to its relay: the relays' boot listen
/// (§6.2), up to a rotation for each leaf to hear its relay, and a
/// rotation of listening before it chooses (§10.1).
fn settle(h: &mut Harness<Node>, pairs: &[(usize, usize)]) {
    let pairs = pairs.to_vec();
    let deadline = Instant::ZERO + secs(150 + 2 * 195);
    assert!(
        h.run_until(deadline, |h| pairs.iter().all(|&(l, r)| attached(h, l, r))),
        "leaves did not attach"
    );
}

/// Two relays and a leaf at each: a message from one leaf reaches the
/// other. The origin leaf's application hears nothing of it, the
/// destination leaf's `Message`, once.
#[test]
fn leaf_to_leaf_across_two_relays() {
    let (r1, r2, l1, l2) = (0, 1, 2, 3);
    let mut h = Harness::new(vec![
        relay(0.0, 1),
        relay(400.0, 2),
        leaf(-30.0, 1, 3),
        leaf(430.0, 2, 4),
    ]);
    for (a, b) in [(r1, r2), (l1, r1), (l2, r2)] {
        h.link(a, b);
    }
    settle(&mut h, &[(l1, r1), (l2, r2)]);
    // Wait until the relays know each other (§7).
    let knows = |h: &Harness<Node>| {
        the_relay(h, r1)
            .neighbours()
            .any(|n| n.position == position(h, r2))
    };
    assert!(h.run_until(h.now + secs(400), knows));
    let start = h.now;
    let id = send(&mut h, l1, l2, r2, b"hello across");
    assert!(
        h.run_until(h.now + secs(5), |h| received(h, l2, id) > 0),
        "not delivered"
    );
    println!("delivered in {} ms", h.now.since(start).as_millis());
    assert!(handed_over(&h, l1, id));
    h.run_for(secs(30));
    assert_eq!(received(&h, l2, id), 1);
    let AppEvent::Message { payload, len, .. } = reports(&h, l2, id)[0];
    assert_eq!(&payload[..len as usize], b"hello across");
    assert!(the_relay(&h, r2).config().position == position(&h, r2));
    assert!(h.nodes[r2].traces.iter().any(|(_, t)| *t
        == Trace::Delivered {
            msg_id: id,
            leaf: LeafId([2; 4])
        }));
}

/// R1's delivery queue is full: its application has filled it with
/// messages for L3, which is dark. R1 refuses L1's message for L3
/// (§9.2.4, §10.4); L1 keeps its relay, holds the message and offers
/// it again one ban after the first refusal, twice that after the second,
/// and so on (§9.3, §10.2, roadmap D27, D29), until L3 is back, R1 has
/// room, and it arrives.
#[test]
fn a_leaf_told_busy_holds_its_message_for_the_ban() {
    use bark_proto::tables::budget::hold;
    use bark_proto::tables::delivery::DELIVERY_QUEUE;
    let (r1, l1, l3) = (0, 1, 2);
    let mut h = Harness::new(vec![relay(0.0, 11), leaf(-30.0, 1, 13), leaf(30.0, 3, 14)]);
    h.link(l1, r1);
    h.link(l3, r1);
    settle(&mut h, &[(l1, r1), (l3, r1)]);
    let dark = std::rc::Rc::new(std::cell::Cell::new(true));
    let darkness = dark.clone();
    h.lose = Some(Box::new(move |_, from, to, _| {
        darkness.get() && (from == l3 || to == l3)
    }));
    let (p1, leaf3) = (position(&h, r1), the_leaf(&h, l3).id());
    for k in 0..DELIVERY_QUEUE as u8 {
        h.command(
            r1,
            AppCommand::Send {
                dst_pos: p1,
                dst_leaf: leaf3,
                payload: &[b'q', k],
            },
        );
    }
    let start = h.now;
    let id = send(&mut h, l1, l3, r1, b"after the ban");
    h.run_until(start + secs(10), |_| false);
    dark.set(false);
    assert!(
        h.run_until(h.now + secs(180), |h| received(h, l3, id) > 0),
        "not delivered"
    );
    let offers: Vec<Instant> = h.nodes[l1]
        .sent
        .iter()
        .filter(|(t, fr)| {
            *t >= start && matches!(Packet::decode(fr.payload()), Ok(Packet::Data(_)))
        })
        .map(|(t, _)| *t)
        .collect();
    let refusals: Vec<Instant> = h.nodes[l1]
        .received
        .iter()
        .filter(|(t, b)| {
            *t >= start && matches!(Packet::decode(b), Ok(Packet::Ack(a)) if a.refused)
        })
        .map(|(t, _)| *t)
        .collect();
    println!("offers at {offers:?}, refusals at {refusals:?}");
    assert!(!refusals.is_empty(), "R1 never refused");
    assert!(refusals.len() >= 3, "{refusals:?}");
    for (j, b) in refusals.iter().enumerate() {
        let next = offers.iter().find(|&&t| t > *b).expect("offered again");
        let due = hold(j as u8 + 1);
        assert!(
            next.since(*b) >= due && next.since(*b) < due + secs(1),
            "refusal {}: offered again after {:?}, not {due:?}",
            j + 1,
            next.since(*b)
        );
    }
    assert!(attached(&h, l1, r1), "a busy relay is not a failing one");
    assert!(handed_over(&h, l1, id));
}

/// L2 goes dark while R1's application sends it more messages than R2's
/// delivery queue holds. R2 queues them until the queue is full and then
/// refuses the next (§9.2.4, §10.4), keeping what it has. R1
/// holds that one, its application's, and offers it again after each ban
/// (§9.3), so its application waits. R2's DELIVERs to L2 go unanswered,
/// and R2 does not hand L2 another each time its send queue frees up:
/// until L2 is heard again only the 60 s retry tries it. Back, L2 gets
/// every message, the held one too.
#[test]
fn a_full_delivery_queue_refuses_and_a_dark_leaf_waits() {
    use bark_proto::tables::delivery::DELIVERY_QUEUE;
    let c = Config::default();
    let (r1, r2, l2) = (0, 1, 2);
    let mut h = Harness::new(vec![relay(0.0, 21), relay(400.0, 22), leaf(430.0, 2, 23)]);
    h.link(r1, r2);
    h.link(l2, r2);
    settle(&mut h, &[(l2, r2)]);
    let knows = |h: &Harness<Node>| {
        the_relay(h, r1)
            .neighbours()
            .any(|n| n.position == position(h, r2))
    };
    assert!(h.run_until(h.now + secs(400), knows));
    let dark = std::rc::Rc::new(std::cell::Cell::new(true));
    let darkness = dark.clone();
    h.lose = Some(Box::new(move |_, from, to, _| {
        darkness.get() && (from == l2 || to == l2)
    }));
    let (p2, leaf2) = (position(&h, r2), the_leaf(&h, l2).id());
    let mut ids = Vec::new();
    for k in 0..=DELIVERY_QUEUE as u8 {
        let text = [b'd', k];
        assert!(h.run_until(h.now + secs(5), |h| h.accepts(r1)));
        h.command(
            r1,
            AppCommand::Send {
                dst_pos: p2,
                dst_leaf: leaf2,
                payload: &text,
            },
        );
        ids.push(MsgId::of(p2, leaf2, &text));
        h.run_for(Duration::from_millis(500));
    }
    let last = *ids.last().unwrap();
    let refused = h.nodes[r2].traces.iter().any(|(_, t)| {
        *t == Trace::Refused {
            msg_id: last,
            reason: DropReason::DeliveryFull,
        }
    });
    assert!(
        refused,
        "R2 took a message its delivery queue had no room for"
    );
    assert_eq!(dropped(&h, r1, last), None);
    assert!(the_relay(&h, r1).holding().any(|id| id == last));
    assert!(!h.accepts(r1), "the application waits");
    assert!(ids[..DELIVERY_QUEUE]
        .iter()
        .all(|&id| dropped(&h, r2, id).is_none()));
    // A minute in the dark: R2's DELIVERs to L2 are the retry's three
    // attempts at most, not one hand-over after another.
    let from = h.now + secs(5);
    h.run_until(from + c.delivery_retry, |_| false);
    let delivers = h.nodes[r2]
        .sent
        .iter()
        .filter(|(t, f)| {
            *t >= from && matches!(Packet::decode(f.payload()), Ok(Packet::Deliver(_)))
        })
        .count();
    assert!(
        delivers <= c.hop_attempts as usize,
        "{delivers} DELIVERs to a dark leaf in a minute"
    );
    dark.set(false);
    let all = |h: &Harness<Node>| ids.iter().all(|&id| received(h, l2, id) == 1);
    assert!(
        h.run_until(h.now + c.delivery_retry + secs(30), all),
        "not all delivered"
    );
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
    assert!(
        h.run_until(h.now + secs(5), |h| acks(h) == 2),
        "no second ACK"
    );
    h.run_for(secs(70));
    assert_eq!(acks(&h), 2);
    assert_eq!(received(&h, l2, id), 1);
    assert!(handed_over(&h, l1, id));
}

/// Three relays in a line with a leaf at each end (design §15 step 4).
/// Messages are delivered; then the middle relay is unplugged, and every
/// later message is accepted by A and dropped there for want of a route
/// (§9.2.5), within the per-hop timeouts of §9.3.
#[test]
fn line_of_three_loses_its_middle() {
    let c = Config::default();
    let (a, b, cc, la, lc) = (0, 1, 2, 3, 4);
    let mut h = Harness::new(vec![
        relay(0.0, 11),
        relay(400.0, 12),
        relay(800.0, 13),
        leaf(-50.0, 1, 14),
        leaf(850.0, 2, 15),
    ]);
    for (x, y) in [(a, b), (b, cc), (la, a), (lc, cc)] {
        h.link(x, y);
    }
    settle(&mut h, &[(la, a), (lc, cc)]);
    let line = |h: &Harness<Node>| {
        the_relay(h, a)
            .neighbours()
            .any(|n| n.position == position(h, b))
            && the_relay(h, b)
                .neighbours()
                .any(|n| n.position == position(h, cc))
    };
    assert!(h.run_until(h.now + secs(400), line));
    let interval = secs(30);
    for k in 0..3u8 {
        let id = send(&mut h, la, lc, cc, &[b'm', k]);
        let start = h.now;
        assert!(
            h.run_until(start + secs(5), |h| received(h, lc, id) == 1),
            "message {k} not delivered"
        );
        println!("before: delivered in {} ms", h.now.since(start).as_millis());
        h.run_until(start + interval, |_| false);
    }
    h.kill(b);
    println!("B removed at {:.1} s", h.now.as_micros() as f64 / 1e6);
    // A hand-over attempt is at most the 500 ms listen-before-talk give-up,
    // a frame and the ACK timeout; three of them, with their backoffs, end
    // in a failure (§9.3).
    let attempts = bark_proto::tables::attempt_span(c.ack_timeout).mul(c.hop_attempts as u64);
    let waits = bark_proto::tables::retry_waits(c.retry_backoff, c.hop_attempts);
    let bound = Duration::from_micros(attempts.as_micros() + waits.as_micros());
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
        assert!(handed_over(&h, la, id), "A took it");
    }
    let ms: Vec<u64> = times.iter().map(|d| d.as_millis()).collect();
    println!(
        "after: dropped at A after {ms:?} ms (bound {} ms)",
        bound.as_millis()
    );
    assert!(
        times
            .iter()
            .all(|t| *t < bound + Duration::from_millis(500)),
        "each waits out at most three attempts: {ms:?}"
    );
    // B is demoted after two failed hand-overs (§8.2), but with no other
    // neighbour A still tries it (§9.2.3, roadmap D25) until its entry
    // expires, three rotations after its last heartbeat; from then on A
    // has no neighbour and drops as soon as the leaf's message arrives.
    assert!(h.nodes[a]
        .traces
        .iter()
        .any(|(_, t)| matches!(t, Trace::Demoted { .. })));
    let expiry = bark_proto::tables::neighbours::NeighbourParams::from_config(&c).expire_after;
    let tried = times
        .iter()
        .take_while(|t| **t > Duration::from_millis(200))
        .count();
    assert!(
        tried as u64 * interval.as_micros() <= expiry.as_micros() + interval.as_micros(),
        "{ms:?}"
    );
    assert!(
        times[tried..]
            .iter()
            .all(|t| *t < Duration::from_millis(200)),
        "{ms:?}"
    );
    assert!(tried < times.len(), "B never expired: {ms:?}");
    for k in 0..20u8 {
        assert_eq!(
            received(
                &h,
                lc,
                MsgId::of(position(&h, cc), LeafId([2; 4]), &[b'n', k])
            ),
            0
        );
    }
}

/// The address decides who delivers, not the leaf table (roadmap D21). A
/// leaf that has just moved to another relay stays in the old one's leaf
/// table until it times out (§10.2); a message for it at its new address
/// that passes the old relay goes on to the address.
#[test]
fn a_relay_holding_the_leaf_routes_by_the_address() {
    let (r1, r2, l1, l2) = (0, 1, 2, 3);
    let mut h = Harness::new(vec![
        relay(0.0, 21),
        relay(400.0, 22),
        leaf(-30.0, 1, 23),
        leaf(30.0, 2, 24),
    ]);
    for (a, b) in [(r1, r2), (l1, r1), (l2, r1)] {
        h.link(a, b);
    }
    settle(&mut h, &[(l1, r1), (l2, r1)]);
    let knows = |h: &Harness<Node>| {
        the_relay(h, r1)
            .neighbours()
            .any(|n| n.position == position(h, r2))
    };
    assert!(h.run_until(h.now + secs(400), knows));
    // l2 is attached at r1, but the sender has it at r2.
    let id = send(&mut h, l1, l2, r2, b"to the address");
    let queued_at_r2 = |h: &Harness<Node>| {
        h.nodes[r2]
            .traces
            .iter()
            .any(|(_, t)| matches!(t, Trace::Queued { msg_id, .. } if *msg_id == id))
    };
    assert!(h.run_until(h.now + secs(5), queued_at_r2), "not at r2");
    h.run_for(secs(30));
    assert_eq!(received(&h, l2, id), 0, "r1 delivered it from its table");
    assert!(!h.nodes[r1]
        .traces
        .iter()
        .any(|(_, t)| matches!(t, Trace::Queued { msg_id, .. } if *msg_id == id)));
}

/// When the leaf received its first heartbeat and sent its first ATTACH.
fn first_heartbeat_and_attach(h: &Harness<Node>, l: usize) -> (Instant, Instant) {
    let first = |frames: &[(Instant, Vec<u8>)], heartbeat: bool| {
        frames
            .iter()
            .find(|(_, b)| match Packet::decode(b) {
                Ok(Packet::Heartbeat(_)) => heartbeat,
                Ok(Packet::Attach(_)) => !heartbeat,
                _ => false,
            })
            .map(|(t, _)| *t)
            .expect("frame")
    };
    let sent: Vec<_> = h.nodes[l]
        .sent
        .iter()
        .map(|(t, f)| (*t, f.payload().to_vec()))
        .collect();
    (first(&h.nodes[l].received, true), first(&sent, false))
}

/// A leaf attaches to the strongest relay it hears over a rotation of
/// listening, not to the first (§10.1).
#[test]
fn a_leaf_chooses_the_strongest_relay_after_a_rotation() {
    let rotation = Config::default().heartbeat_max.mul(13);
    for seed in 0..4 {
        let (r1, r2, l) = (0, 1, 2);
        let mut h = Harness::new(vec![
            relay(0.0, 50 + seed),
            relay(400.0, 60 + seed),
            leaf(200.0, 1, 70 + seed),
        ]);
        h.link(l, r1);
        h.link(l, r2);
        h.set_rssi(l, r1, -100);
        h.set_rssi(l, r2, -88);
        settle(&mut h, &[(l, r2)]);
        let (heard, attach) = first_heartbeat_and_attach(&h, l);
        assert!(
            attach.since(heard) >= rotation,
            "seed {seed}: attached {:?} after the first heartbeat",
            attach.since(heard)
        );
    }
}

/// A leaf with a message to send does not wait out the rotation, and holds
/// the message until it has a relay (§10.1, §10.2).
#[test]
fn a_message_waits_for_a_relay() {
    let (r, l) = (0, 1);
    let mut h = Harness::new(vec![relay(0.0, 41), leaf(30.0, 1, 42)]);
    h.link(l, r);
    // Within the relay's boot listen: the leaf has heard nothing yet.
    h.run_until(Instant::ZERO + secs(10), |_| false);
    let dst = position(&h, r);
    let text = b"wait for me";
    h.command(
        l,
        AppCommand::Send {
            dst_pos: dst,
            dst_leaf: LeafId::RELAY,
            payload: text,
        },
    );
    let id = MsgId::of(dst, LeafId::RELAY, text);
    assert!(h.run_until(Instant::ZERO + secs(600), |h| received(h, r, id) == 1));
    h.run_for(secs(1));
    assert!(handed_over(&h, l, id));
    let (heard, attach) = first_heartbeat_and_attach(&h, l);
    assert!(
        attach.since(heard) < Duration::from_millis(100),
        "attached {:?} after the first heartbeat",
        attach.since(heard)
    );
}

/// A message no relay takes is dropped after the delivery TTL (§10.4),
/// not at once, and the application is not told.
#[test]
fn a_message_without_a_relay_is_dropped_after_the_delivery_ttl() {
    let mut h = Harness::new(vec![leaf(0.0, 1, 43)]);
    h.run_until(Instant::ZERO + secs(1), |_| false);
    let (dst, text) = (pos(500.0, 0.0), b"nobody there");
    h.command(
        0,
        AppCommand::Send {
            dst_pos: dst,
            dst_leaf: LeafId([9; 4]),
            payload: text,
        },
    );
    let id = MsgId::of(dst, LeafId([9; 4]), text);
    h.run_for(secs(599));
    assert_eq!(the_leaf(&h, 0).holding(), Some(id));
    assert!(!h.accepts(0), "a leaf holds one message");
    h.run_for(secs(2));
    assert_eq!(
        dropped(&h, 0, id).map(|(_, r)| r),
        Some(DropReason::NoRelay)
    );
    assert!(h.accepts(0));
    assert!(
        reports(&h, 0, id).is_empty(),
        "the application is told nothing"
    );
}

/// A leaf moves to a relay only when it is stronger by the margin over two
/// heartbeats (§10.1).
#[test]
fn a_leaf_switches_only_to_a_clearly_stronger_relay() {
    let (r1, r2, r3, l) = (0, 1, 2, 3);
    let mut h = Harness::new(vec![
        relay(0.0, 34),
        relay(400.0, 35),
        relay(-400.0, 36),
        leaf(30.0, 1, 37),
    ]);
    h.link(l, r1);
    h.set_rssi(l, r1, -90);
    settle(&mut h, &[(l, r1)]);
    // 5 dB stronger: not enough.
    h.link(l, r2);
    h.set_rssi(l, r2, -85);
    let start = h.now;
    assert!(!h.run_until(start + secs(1200), |h| attached(h, l, r2)));
    // 10 dB stronger: the leaf moves, once it has heard two heartbeats.
    h.link(l, r3);
    h.set_rssi(l, r3, -80);
    let start = h.now;
    assert!(h.run_until(start + secs(600), |h| attached(h, l, r3)));
    let r3_position = position(&h, r3);
    let heard = h.nodes[l]
        .received
        .iter()
        .filter(|(t, b)| {
            *t >= start
                && matches!(Packet::decode(b), Ok(Packet::Heartbeat(hb)) if hb.position == r3_position)
        })
        .count();
    assert!(heard >= 2, "switched after {heard} heartbeat(s)");
}

// --- Polls (§10.2, roadmap D39, D41) ---

use bark_proto::tables::leaves::{Polls, PERIOD, POLL_AIRTIME, POLL_LATE};
use bark_proto::wire::{Attach, Attached};

/// The ATTACHEDs node `r` sent to leaf `id` from `from` on: when each went
/// on air, and what it named.
fn attacheds(h: &Harness<Node>, r: usize, id: LeafId, from: Instant) -> Vec<(Instant, Attached)> {
    h.nodes[r]
        .sent
        .iter()
        .filter(|(t, _)| *t >= from)
        .filter_map(|(t, f)| match Packet::decode(f.payload()) {
            Ok(Packet::Attached(a)) if a.leaf == id => Some((*t, a)),
            _ => None,
        })
        .collect()
}

/// The ATTACHes leaf `l` sent from `from` on.
fn attaches(h: &Harness<Node>, l: usize, from: Instant) -> Vec<(Instant, Attach)> {
    h.nodes[l]
        .sent
        .iter()
        .filter(|(t, _)| *t >= from)
        .filter_map(|(t, f)| match Packet::decode(f.payload()) {
            Ok(Packet::Attach(a)) => Some((*t, a)),
            _ => None,
        })
        .collect()
}

/// The ATTACHes that attach, not answers to polls.
fn attaching(h: &Harness<Node>, l: usize, from: Instant) -> Vec<Instant> {
    attaches(h, l, from)
        .iter()
        .filter(|(_, a)| !a.reply)
        .map(|(t, _)| *t)
        .collect()
}

fn polls() -> Polls {
    Polls::from_config(&Config::default())
}

/// Where the ATTACHED sent at `t` said the next poll goes.
fn named(t: Instant, a: &Attached) -> Instant {
    t + POLL_AIRTIME + a.next_poll()
}

/// The first heartbeat leaf `l` received at or after `from`.
fn heartbeat_after(h: &Harness<Node>, l: usize, from: Instant) -> Option<Instant> {
    h.nodes[l]
        .received
        .iter()
        .find(|(t, b)| *t >= from && matches!(Packet::decode(b), Ok(Packet::Heartbeat(_))))
        .map(|(t, _)| *t)
}

fn dropped_at(h: &Harness<Node>, r: usize, id: LeafId) -> Option<Instant> {
    h.nodes[r].traces.iter().find_map(|(t, tr)| match tr {
        Trace::LeafDropped { leaf } if *leaf == id => Some(*t),
        _ => None,
    })
}

/// A leaf's first ATTACH is answered with ATTACHED at once, which attaches
/// it. Then the relay polls it at the slot each ATTACHED named, and the
/// leaf answers each poll with one ATTACH marked as a reply and sends no
/// other ATTACH. The first slot is a period after the attach, and each a
/// period after the last.
#[test]
fn a_leaf_is_attached_by_attached_and_answers_only_polls() {
    let (r, l) = (0, 1);
    let mut h = Harness::new(vec![relay(0.0, 41), leaf(30.0, 1, 42)]);
    h.link(l, r);
    settle(&mut h, &[(l, r)]);
    let id = the_leaf(&h, l).id();
    let first = attaches(&h, l, Instant::ZERO);
    assert_eq!(first.len(), 1, "{first:?}");
    assert!(!first[0].1.reply);
    let answer = attacheds(&h, r, id, Instant::ZERO);
    assert_eq!(answer.len(), 1);
    assert!(
        answer[0].0.since(first[0].0) < Config::default().ack_timeout,
        "answered at once"
    );
    let start = answer[0].0 + Duration::from_millis(1);
    h.run_until(start + PERIOD.mul(4) + secs(1), |_| false);
    let sent = attacheds(&h, r, id, Instant::ZERO);
    for w in sent.windows(2) {
        let ((t0, a0), (t1, _)) = (w[0], w[1]);
        let slot = named(t0, &a0);
        assert!(
            t1 + Duration::from_millis(5) >= slot && t1 <= slot + POLL_LATE,
            "poll at {t1:?}, named {slot:?}"
        );
    }
    let recent: Vec<_> = sent.iter().filter(|(t, _)| *t >= start).collect();
    assert_eq!(recent.len(), 4, "{recent:?}");
    let first_poll = recent[0].0.since(answer[0].0);
    assert!(first_poll.as_micros().abs_diff(PERIOD.as_micros()) <= POLL_LATE.as_micros());
    for w in recent.windows(2) {
        let gap = w[1].0.since(w[0].0);
        assert!(
            gap.as_micros().abs_diff(PERIOD.as_micros()) <= POLL_LATE.as_micros(),
            "{gap:?}"
        );
    }
    // Each poll was answered once, at once.
    let replies = attaches(&h, l, start);
    assert_eq!(replies.len(), recent.len(), "{replies:?}");
    for ((tp, _), (ta, a)) in recent.iter().zip(&replies) {
        assert!(a.reply);
        assert!(ta > tp && ta.since(*tp) < Duration::from_millis(150));
    }
    assert!(attaching(&h, l, first[0].0 + Duration::from_millis(1)).is_empty());
    assert!(attached(&h, l, r));
}

/// With several leaves the polls keep the spacing: three leaves that
/// attached within seconds of each other are polled every 300 s, 100 s
/// apart (§10.2).
#[test]
fn a_relay_spreads_its_polls() {
    let r = 0;
    let mut h = Harness::new(vec![
        relay(0.0, 51),
        leaf(30.0, 1, 52),
        leaf(40.0, 2, 53),
        leaf(50.0, 3, 54),
    ]);
    for l in 1..4 {
        h.link(l, r);
    }
    settle(&mut h, &[(1, r), (2, r), (3, r)]);
    let p = PERIOD;
    let spacing = polls().spacing(3);
    assert_eq!(spacing, Duration::from_secs(100));
    // While leaves attach the spacing shrinks; the round settles within
    // a few periods.
    let start = h.now + p.mul(3);
    h.run_until(start + p.mul(3) + secs(2), |_| false);
    let mut all = Vec::new();
    for l in 1..4 {
        let id = the_leaf(&h, l).id();
        let mine = attacheds(&h, r, id, start);
        assert!(mine.len() >= 2, "leaf {l}: {mine:?}");
        for w in mine.windows(2) {
            let gap = w[1].0.since(w[0].0);
            assert!(
                gap.as_micros().abs_diff(p.as_micros()) <= POLL_LATE.as_micros(),
                "leaf {l}: {gap:?}"
            );
        }
        all.extend(mine.iter().map(|(t, _)| *t));
    }
    all.sort_unstable();
    for w in all.windows(2) {
        assert!(
            w[1].since(w[0]) + POLL_LATE >= spacing,
            "two polls within a spacing: {w:?}"
        );
    }
}

/// A leaf that stops answering is polled until its polls have gone
/// unanswered for a TTL, and dropped once the answer to the last is
/// overdue; until then it is kept (§10.2). At 300 s that is three
/// polls.
#[test]
fn a_leaf_that_misses_its_polls_for_a_ttl_is_dropped() {
    let (r, l) = (0, 1);
    let mut h = Harness::new(vec![relay(0.0, 61), leaf(30.0, 1, 62)]);
    h.link(l, r);
    settle(&mut h, &[(l, r)]);
    let id = the_leaf(&h, l).id();
    h.run_for(PERIOD + secs(30));
    let gone = h.now;
    h.kill(l);
    let p = polls();
    let ttl = Config::default().delivery_ttl;
    assert!(
        h.run_until(gone + ttl + PERIOD + secs(5), |h| dropped_at(h, r, id)
            .is_some())
    );
    let at = dropped_at(&h, r, id).unwrap();
    let unanswered = attacheds(&h, r, id, gone);
    assert_eq!(unanswered.len(), 3, "{unanswered:?}");
    let (first, last) = (unanswered[0].0, unanswered.last().unwrap().0);
    assert!(last.since(first) + POLL_LATE >= ttl, "kept for a TTL");
    assert!(
        at >= last && at.since(last) <= p.reply_wait,
        "{at:?} after {last:?}"
    );
    assert_eq!(the_relay(&h, r).load(h.now).leaves, 0);
}

/// One poll lost to a fade costs one ATTACH: the leaf, which no longer
/// knows its slot, attaches again a heartbeat and up to a minute later,
/// and the relay, which still has it, answers with its slot unchanged; the
/// polls go on (§10.2).
#[test]
fn a_lost_poll_costs_one_attach() {
    use std::cell::Cell;
    use std::rc::Rc;
    let (r, l) = (0, 1);
    let mut h = Harness::new(vec![relay(0.0, 71), leaf(30.0, 1, 72)]);
    h.link(l, r);
    settle(&mut h, &[(l, r)]);
    let id = the_leaf(&h, l).id();
    let (t0, a0) = attacheds(&h, r, id, Instant::ZERO)[0];
    let slot = named(t0, &a0);
    let lost = Rc::new(Cell::new(0));
    let count = lost.clone();
    h.lose = Some(Box::new(move |_, from, to, bytes| {
        let poll = from == r && to == l && matches!(Packet::decode(bytes), Ok(Packet::Attached(_)));
        if poll && count.get() == 0 {
            count.set(1);
            return true;
        }
        false
    }));
    let start = h.now;
    h.run_until(slot + PERIOD.mul(2) + secs(30), |_| false);
    assert_eq!(lost.get(), 1);
    assert!(attached(&h, l, r));
    let again = attaching(&h, l, start);
    assert_eq!(again.len(), 1, "{again:?}");
    let c = Config::default();
    let heard = heartbeat_after(&h, l, slot + POLL_LATE).unwrap();
    assert!(again[0] >= heard && again[0] <= heard + c.leaf_retry + secs(1));
    // Its slot was kept: the polls after it are a period apart.
    let polls: Vec<_> = attacheds(&h, r, id, slot + secs(1))
        .iter()
        .filter(|(t, _)| *t > again[0] + secs(1))
        .map(|(t, _)| *t)
        .collect();
    assert_eq!(polls.len(), 2, "{polls:?}");
    let first = polls[0].since(slot);
    assert!(first.as_micros().abs_diff(PERIOD.as_micros()) <= POLL_LATE.as_micros());
    let answers = attaches(&h, l, start)
        .iter()
        .filter(|(_, a)| a.reply)
        .count();
    assert_eq!(answers, 2, "both later polls answered");
    assert!(dropped_at(&h, r, id).is_none());
    assert_eq!(the_relay(&h, r).load(h.now).leaves, 1);
}

/// A phone out of reach of every relay for longer than a TTL keeps its
/// relay. It sends nothing it cannot be heard for but one ATTACH a
/// rotation; its relay drops it; back in reach, it attaches to the same
/// relay a random 0 to 60 s after the first heartbeat it hears (§10.2).
#[test]
fn a_dark_leaf_keeps_its_relay_and_attaches_after_a_heartbeat() {
    use std::cell::Cell;
    use std::rc::Rc;
    let (r1, r2, l) = (0, 1, 2);
    let mut h = Harness::new(vec![relay(0.0, 151), relay(400.0, 152), leaf(30.0, 1, 153)]);
    h.link(l, r1);
    h.link(l, r2);
    h.set_rssi(l, r1, -90);
    h.set_rssi(l, r2, -100);
    settle(&mut h, &[(l, r1)]);
    // Only its own relay in reach once it is back, so that it has no
    // other to choose.
    h.unlink(l, r2);
    h.run_for(PERIOD + secs(30));
    let id = the_leaf(&h, l).id();
    let dark = Rc::new(Cell::new(true));
    let darkness = dark.clone();
    h.lose = Some(Box::new(move |_, from, to, _| {
        darkness.get() && (from == l || to == l)
    }));
    let c = Config::default();
    let rotation = c.heartbeat_max.mul(13);
    let gone = h.now;
    // The first poll it misses is up to a period away; the drop a TTL
    // after that.
    h.run_for(c.delivery_ttl + PERIOD + secs(5));
    assert!(dropped_at(&h, r1, id).is_some(), "the relay dropped it");
    assert!(attached(&h, l, r1), "the leaf kept its relay");
    let tries = attaching(&h, l, gone);
    let attempts = c.hop_attempts as usize;
    let rotations = (h.now.since(gone).as_micros() / rotation.as_micros()) as usize;
    assert!(tries.len() <= attempts * (rotations + 1), "{tries:?}");
    assert!(tries.len() >= attempts, "{tries:?}");
    dark.set(false);
    let back = h.now;
    assert!(h.run_until(back + rotation + c.leaf_retry + secs(5), |h| {
        the_relay(h, r1).load(h.now).leaves == 1
    }));
    let heard = heartbeat_after(&h, l, back).unwrap();
    let again = attaching(&h, l, back);
    assert!(!again.is_empty());
    assert!(again[0] >= heard && again[0] <= heard + c.leaf_retry + secs(1));
    assert!(attached(&h, l, r1));
    assert_eq!(the_relay(&h, r2).load(h.now).leaves, 0);
}

/// A leaf whose relay dies takes the next poll that does not come as the
/// sign: after the next heartbeat it hears, from the other relay, and a
/// random wait, it has not heard its own since the polls stopped, and
/// attaches to the other one (§10.1, §10.2).
#[test]
fn a_leaf_whose_relay_dies_attaches_to_another() {
    let (r1, r2, l) = (0, 1, 2);
    let mut h = Harness::new(vec![relay(0.0, 81), relay(400.0, 82), leaf(30.0, 1, 83)]);
    h.link(l, r1);
    h.link(l, r2);
    h.set_rssi(l, r1, -90);
    h.set_rssi(l, r2, -100);
    settle(&mut h, &[(l, r1)]);
    h.run_for(PERIOD + secs(30));
    let id = the_leaf(&h, l).id();
    let (t, a) = *attacheds(&h, r1, id, Instant::ZERO).last().unwrap();
    let missed = named(t, &a) + POLL_LATE;
    h.kill(r1);
    let c = Config::default();
    let rotation = c.heartbeat_max.mul(13);
    assert!(
        h.run_until(missed + rotation + c.leaf_retry + secs(5), |h| {
            attached(h, l, r2)
        })
    );
    let moved = h.now;
    let heard = heartbeat_after(&h, l, missed).unwrap();
    assert!(moved >= heard && moved <= heard + c.leaf_retry + secs(1));
    let to_r1 = attaches(&h, l, missed)
        .iter()
        .filter(|(_, a)| a.relay == position(&h, r1).short())
        .count();
    assert_eq!(to_r1, 0, "nothing sent to the dead relay");
}

/// A message for a leaf waits while its DELIVERs go unanswered; the next
/// poll counts it, and the DELIVER follows the answer at once (§10.2,
/// §10.4).
#[test]
fn a_poll_counts_the_messages_waiting() {
    use std::cell::Cell;
    use std::rc::Rc;
    let (r, l1, l2) = (0, 1, 2);
    let mut h = Harness::new(vec![relay(0.0, 91), leaf(-30.0, 1, 92), leaf(30.0, 2, 93)]);
    h.link(l1, r);
    h.link(l2, r);
    settle(&mut h, &[(l1, r), (l2, r)]);
    let leaf2 = the_leaf(&h, l2).id();
    let (t0, a0) = *attacheds(&h, r, leaf2, Instant::ZERO).last().unwrap();
    let slot = named(t0, &a0);
    // L2 is out of reach from now until just before its slot.
    let dark = Rc::new(Cell::new(true));
    let darkness = dark.clone();
    h.lose = Some(Box::new(move |_, from, to, _| {
        darkness.get() && (from == l2 || to == l2)
    }));
    let id = send(&mut h, l1, l2, r, b"while you were out");
    assert!(h.run_until(h.now + secs(5), |h| handed_over(h, l1, id)));
    h.run_until(Instant::from_micros(slot.as_micros() - 1_000_000), |_| {
        false
    });
    assert_eq!(received(&h, l2, id), 0);
    dark.set(false);
    let back = h.now;
    assert!(h.run_until(slot + secs(5), |h| received(h, l2, id) == 1));
    let got = h.now;
    let (t, a) = attacheds(&h, r, leaf2, back)[0];
    assert_eq!(a.waiting, 1);
    assert!(got > t && got.since(t) < secs(1), "{:?}", got.since(t));
}
