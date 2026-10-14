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
/// refuses a message without an ACK when its send queue could not go out
/// within the time O spends retrying (§9.2.4), and a hand-over the bucket
/// cannot fit yet waits for it, never longer than a frame's refill, instead
/// of being dropped. Its heartbeats are never held back. The harness
/// panics if any hour holds more than 10 %.
///
/// O retries the messages F refuses, on F's channel, and O and D cannot
/// hear each other: now and then D's ACKs to F are lost under O's frames,
/// all four of F's attempts go unanswered, and F returns the message to
/// O as the search requires (§9.2.5). That is not a drop.
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
    use bark_proto::tables::budget::{BUDGET, DEPTH, HEARTBEAT_RESERVE};
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
    while h.now < end {
        let mut text = [0u8; MAX_PAYLOAD];
        text[..4].copy_from_slice(&k.to_be_bytes());
        send(&mut h, o, d, &text);
        k += 1;
        let next = h.now + Duration::from_millis(2500);
        h.run_until(next, |_| false);
    }
    h.run_for(secs(600));
    let tf = traces(&h, f);
    // No hand-over waits longer than a full frame's refill (§9.4).
    let frame = time_on_air(MAX_FRAME as u8, MIN_PREAMBLE_SYMBOLS) + HEARTBEAT_RESERVE;
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
    let refused = tf
        .iter()
        .filter(|t| {
            matches!(
                t,
                Trace::Refused {
                    reason: DropReason::Budget,
                    ..
                }
            )
        })
        .count();
    let handed = |id: MsgId| {
        tf.iter().any(|t| {
            matches!(t, Trace::Forwarded { msg_id, .. } | Trace::ReturnedToParent { msg_id, .. } if *msg_id == id)
        })
    };
    println!(
        "{k} sent; F forwarded {}, returned {}, deferred {} for budget, refused {refused} for budget",
        tf.iter()
            .filter(|t| matches!(t, Trace::Forwarded { .. }))
            .count(),
        tf.iter()
            .filter(|t| matches!(t, Trace::ReturnedToParent { .. }))
            .count(),
        deferred.len(),
    );
    assert!(refused > 0, "the budget never refused a message");
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
