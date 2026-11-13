//! The drain before a reboot into a new image (spec "Send queue",
//! "Updates over the mesh", the install): a draining relay takes no
//! message under load and holds nothing well within the bound; what it
//! still holds at the bound is put back after the reboot, the same
//! message.

mod common;

use bark_proto::tables::delivery::DELIVERY_QUEUE;
use bark_proto::{
    AppCommand, AppEvent, Config, DropReason, HeldAt, Instant, MsgId, NodeId, Position, Relay,
    Trace,
};
use common::{secs, swap, Harness};
use std::collections::HashSet;

/// A position `east` metres from a point in Kyiv.
fn pos(east: f64, north: f64) -> Position {
    let (lat, lon) = (50.45, 30.52);
    let m_per_deg = 6_371_000.0 * core::f64::consts::PI / 180.0;
    Position::from_degrees(
        lat + north / m_per_deg,
        lon + east / (m_per_deg * lat.to_radians().cos()),
    )
}

fn relay(east: f64, north: f64, seed: u64) -> Relay {
    Relay::new(
        Config {
            position: pos(east, north),
            boot_listen: secs(5),
            ..Config::default()
        },
        seed,
    )
}

fn knows(h: &Harness<Relay>, a: usize, b: usize) -> bool {
    let target = h.nodes[b].core.config().position;
    h.nodes[a].core.neighbours().any(|n| n.position == target)
}

fn build(nodes: Vec<Relay>, links: &[(usize, usize)]) -> Harness<Relay> {
    let mut h = Harness::new(nodes);
    for &(a, b) in links {
        h.link(a, b);
    }
    let found = h.run_until(Instant::ZERO + secs(600), |h| {
        links.iter().all(|&(a, b)| knows(h, a, b) && knows(h, b, a))
    });
    assert!(found, "neighbours not discovered");
    h
}

fn delivered(h: &Harness<Relay>, i: usize) -> HashSet<MsgId> {
    h.nodes[i]
        .app
        .iter()
        .filter_map(|(_, e)| match e {
            AppEvent::Message { msg_id } => Some(*msg_id),
            _ => None,
        })
        .collect()
}

/// The sources send to D through M, each a message whenever its node
/// takes one, numbered from `k` on, until `until`. With `close`, M drains
/// from then, or from earlier once it holds ten messages, and the load
/// ends once it has drained. Returns the IDs the sources' nodes took and
/// when M began to drain.
fn load(
    h: &mut Harness<Relay>,
    (sources, m, d): (&[usize], usize, usize),
    k: &mut u32,
    close: Option<Instant>,
    until: Instant,
) -> (Vec<MsgId>, Option<Instant>) {
    let (dst_pos, dst_id) = (h.nodes[d].core.config().position, h.nodes[d].core.id());
    let mut sent = Vec::new();
    let mut closed = None;
    while h.now < until {
        let full = h.nodes[m].core.holding().count() >= 10;
        if closed.is_none() && close.is_some_and(|c| h.now >= c || full) {
            h.command(m, AppCommand::Drain(true));
            closed = Some(h.now);
        }
        for &s in sources {
            if h.accepts(s) {
                let payload = k.to_be_bytes();
                *k += 1;
                h.command(
                    s,
                    AppCommand::Send {
                        dst_pos,
                        dst_id,
                        payload: &payload,
                    },
                );
                sent.push(MsgId::of(dst_pos, dst_id, &payload));
            }
        }
        let step = h.now + bark_proto::Duration::from_millis(100);
        h.run_until(step, |_| false);
        if closed.is_some() && h.nodes[m].core.drained(h.now) {
            break;
        }
    }
    (sent, closed)
}

/// Under a load that fills its queue, a relay that drains refuses every
/// message from its close on, hands on what it held, and holds nothing
/// well within the bound, the delivery TTL; nothing the network took is
/// lost, and after its reboot it carries the load again.
#[test]
fn a_draining_relay_under_load_takes_nothing_and_empties_within_the_bound() {
    let sources = [0, 1, 2, 3];
    let (m, d) = (4, 5);
    let mut h = build(
        vec![
            relay(0.0, 0.0, 1),
            relay(0.0, 300.0, 2),
            relay(150.0, -250.0, 3),
            relay(150.0, 550.0, 4),
            relay(300.0, 150.0, 5),
            relay(600.0, 150.0, 6),
        ],
        &[(0, m), (1, m), (2, m), (3, m), (m, d)],
    );
    let ttl = Config::default().delivery_ttl;
    let mut k = 0;
    let latest = h.now + secs(300);
    let (sent, closed) = load(&mut h, (&sources, m, d), &mut k, Some(latest), latest + ttl);
    let close = closed.expect("closed");
    assert!(close < latest, "the load never filled M's queue");
    let at = h.now;
    assert!(h.nodes[m].core.drained(at), "not drained within the bound");
    assert!(h.nodes[m].core.held(at).next().is_none(), "held at the end");
    let refused = |h: &Harness<Relay>, why: Option<DropReason>| {
        h.nodes[m].traces.iter().filter(|(t, _)| *t >= close).any(
            |(_, t)| matches!(t, Trace::Refused { reason, .. } if why.is_none_or(|w| w == *reason)),
        )
    };
    std::println!(
        "drained {:?} after the close; {} messages sent",
        at.since(close),
        sent.len()
    );
    assert!(refused(&h, Some(DropReason::Draining)));
    let others = h.nodes[m].traces.iter().filter(|(t, _)| *t >= close).any(
        |(_, t)| matches!(t, Trace::Refused { reason, .. } if *reason != DropReason::Draining),
    );
    assert!(!others, "refused for another reason while draining");
    assert!(!h.accepts(m));
    // Nothing the network took is lost: each message is delivered or
    // still at its source, once what was on the air has landed.
    h.run_until(at + secs(10), |_| false);
    let got = delivered(&h, d);
    for id in &sent {
        let at_source = sources
            .iter()
            .any(|&s| h.nodes[s].core.holding().any(|x| x == *id));
        assert!(got.contains(id) || at_source, "{id:?} lost");
    }
    for (i, n) in h.nodes.iter().enumerate() {
        assert!(
            !n.traces
                .iter()
                .any(|(_, t)| matches!(t, Trace::Dropped { .. })),
            "relay {i} dropped"
        );
    }
    // The reboot: M comes back and carries the load again.
    h.reboot(m, relay(300.0, 150.0, 30));
    let before = got.len();
    load(&mut h, (&sources, m, d), &mut k, None, at + secs(310));
    assert!(
        delivered(&h, d).len() > before,
        "no message after the reboot"
    );
}

/// A message for a node that stays dark waits in its owner's delivery
/// queue past the bound. The owner may reboot then, and what it held is
/// put back after the reboot: the same message, back in its delivery
/// queue with the age it had and the time away.
#[test]
fn a_message_held_past_the_bound_is_put_back_after_the_reboot() {
    let (s, m) = (0, 1);
    let mut h = build(vec![relay(0.0, 0.0, 1), relay(300.0, 0.0, 2)], &[(s, m)]);
    let dark = NodeId([9; 6]);
    let dst_pos = h.nodes[m].core.config().position;
    let payload = b"for a node that sleeps";
    h.command(
        s,
        AppCommand::Send {
            dst_pos,
            dst_id: dark,
            payload,
        },
    );
    let id = MsgId::of(dst_pos, dark, payload);
    let queued = |h: &Harness<Relay>| h.nodes[m].core.delivery_queue().any(|(x, _)| x == id);
    assert!(
        h.run_until(h.now + secs(30), queued),
        "not queued at its owner"
    );
    let since = h.now;
    h.command(m, AppCommand::Drain(true));
    let close = h.now;
    let ttl = Config::default().delivery_ttl;
    // The bound counts from the whole second at or after the close.
    h.run_until(close + ttl - secs(1), |_| false);
    assert!(!h.nodes[m].core.drained(h.now), "drained before the bound");
    h.run_until(close + ttl + secs(1), |_| false);
    assert!(h.nodes[m].core.drained(h.now));
    let held: Vec<(MsgId, Position, NodeId, Vec<u8>)> = h.nodes[m]
        .core
        .held(h.now)
        .map(|x| (x.msg_id, x.dst_pos, x.dst_id, x.payload.to_vec()))
        .collect();
    assert_eq!(held, vec![(id, dst_pos, dark, payload.to_vec())]);
    let own = swap(&mut h, m, relay(300.0, 0.0, 20), secs(59));
    assert!(own.is_empty());
    assert!(queued(&h), "not back in the delivery queue");
    let age = h.nodes[m].core.held(h.now).next().map(|x| x.at);
    let HeldAt::Delivery { age, .. } = age.expect("held") else {
        panic!("not in the delivery queue");
    };
    let true_age = h.now.since(since);
    assert!(age >= true_age && age <= true_age + secs(1), "{age:?}");
}

/// While a relay drains, its neighbour hears from its heartbeats that it
/// is leaving the air; once the drain opens again, its next heartbeat
/// says it no longer is (spec "Packet formats", "Neighbour table").
#[test]
fn a_neighbour_hears_a_draining_relay_leaving() {
    let (s, m) = (0, 1);
    let mut h = build(vec![relay(0.0, 0.0, 1), relay(300.0, 0.0, 2)], &[(s, m)]);
    let id = h.nodes[m].core.id();
    let leaving = |h: &Harness<Relay>| {
        h.nodes[s]
            .core
            .neighbours()
            .any(|n| n.id == id && n.leaving)
    };
    assert!(!leaving(&h));
    h.command(m, AppCommand::Drain(true));
    // Within a worst-case rotation its heartbeat reaches every channel.
    let rotation = Config::default().boot_listen;
    assert!(h.run_until(h.now + rotation, leaving), "not heard leaving");
    h.command(m, AppCommand::Drain(false));
    assert!(
        h.run_until(h.now + rotation, |h| !leaving(h)),
        "still leaving"
    );
}

/// Whether `node`'s traces have message `id` wait for `by`.
fn waits(h: &Harness<Relay>, node: usize, id: MsgId, by: NodeId) -> bool {
    h.nodes[node].traces.iter().any(|(_, t)| {
        *t == Trace::Waits {
            msg_id: id,
            node: by,
        }
    })
}

/// Whether any node dropped message `id` at the hop cap.
fn at_cap(h: &Harness<Relay>, id: MsgId) -> bool {
    h.nodes.iter().any(|n| {
        n.traces.iter().any(|(_, t)| {
            *t == Trace::Dropped {
                msg_id: id,
                reason: DropReason::HopCap,
            }
        })
    })
}

/// S sends through N a message that can end only at L, which drains: to
/// L itself, or with `dark` to a sleepy node at L's position, which L
/// holds mail for. M is a neighbour of N and L that the message could
/// be passed round to. Returns the harness, the message, and the nodes
/// S, N, M and L.
fn for_a_draining_node(dark: bool) -> (Harness<Relay>, MsgId, [usize; 4]) {
    let (s, n, m, l) = (0, 1, 2, 3);
    let mut h = build(
        vec![
            relay(0.0, 0.0, 1),
            relay(300.0, 0.0, 2),
            relay(450.0, 250.0, 3),
            relay(600.0, 0.0, 4),
        ],
        &[(s, n), (n, m), (n, l), (m, l)],
    );
    let lid = h.nodes[l].core.id();
    h.command(l, AppCommand::Drain(true));
    let rotation = Config::default().boot_listen;
    let leaving = |h: &Harness<Relay>| {
        [n, m].iter().all(|&i| {
            h.nodes[i]
                .core
                .neighbours()
                .any(|x| x.id == lid && x.leaving)
        })
    };
    assert!(h.run_until(h.now + rotation, leaving), "not heard leaving");
    let dst_pos = h.nodes[l].core.config().position;
    let dst_id = if dark { NodeId([9; 6]) } else { lid };
    let payload = b"for a node that is leaving";
    h.command(
        s,
        AppCommand::Send {
            dst_pos,
            dst_id,
            payload,
        },
    );
    let id = MsgId::of(dst_pos, dst_id, payload);
    assert!(
        h.run_until(h.now + secs(60), |h| waits(h, n, id, lid)),
        "not waiting at N"
    );
    (h, id, [s, n, m, l])
}

/// A message that can end only at a node that drains waits at the
/// neighbour that holds it, and is not passed round among the node's
/// neighbours; once the node is back from its swap it is delivered
/// (spec "Delivery queue").
#[test]
fn a_message_for_a_draining_node_waits_at_its_neighbour_until_it_is_back() {
    for dark in [false, true] {
        let (mut h, id, [_, n, m, l]) = for_a_draining_node(dark);
        let lid = h.nodes[l].core.id();
        // The drain's whole bound, then the swap off the air.
        let ttl = Config::default().delivery_ttl;
        h.run_until(h.now + ttl, |_| false);
        assert!(!h.nodes[m].core.knows(id), "passed round to M");
        assert!(h.nodes[n].core.holding().any(|x| x == id));
        h.kill(l);
        h.run_until(h.now + secs(59), |_| false);
        // The same node, so the same ID.
        h.reboot(l, relay(600.0, 0.0, 4));
        let arrived = |h: &Harness<Relay>| {
            if dark {
                h.nodes[l].core.delivery_queue().any(|(x, _)| x == id)
            } else {
                delivered(h, l).contains(&id)
            }
        };
        let rotation = Config::default().boot_listen;
        assert!(
            h.run_until(h.now + rotation + secs(60), arrived),
            "not delivered after the swap, dark {dark}"
        );
        assert!(!h.nodes[m].core.knows(id), "passed round to M");
        assert!(!at_cap(&h, id));
        assert!(!waits(&h, m, id, lid));
    }
}

/// A message that waits for a node still leaving past the delivery TTL
/// is stale, as any in the delivery queue: it stays until a message
/// arriving at the full queue needs its place, and is then dropped for
/// its age, the queue's own end (spec "Delivery queue").
#[test]
fn a_message_that_waits_past_the_ttl_goes_as_a_stale_one() {
    let (mut h, id, [s, n, m, _]) = for_a_draining_node(false);
    let ttl = Config::default().delivery_ttl;
    h.run_until(h.now + ttl + secs(1), |_| false);
    assert!(h.nodes[n].core.holding().any(|x| x == id), "gone early");
    // Messages for nodes N holds mail for fill its delivery queue.
    let dst_pos = h.nodes[n].core.config().position;
    let expired = |h: &Harness<Relay>| {
        h.nodes[n].traces.iter().any(|(_, t)| {
            *t == Trace::Dropped {
                msg_id: id,
                reason: DropReason::QueueExpired,
            }
        })
    };
    let mut k = 0u32;
    while !expired(&h) && k < 2 * DELIVERY_QUEUE as u32 {
        if h.accepts(s) {
            let payload = k.to_be_bytes();
            let dst_id = NodeId([10, 0, 0, 0, 0, k as u8]);
            h.command(
                s,
                AppCommand::Send {
                    dst_pos,
                    dst_id,
                    payload: &payload,
                },
            );
            k += 1;
        }
        let step = h.now + bark_proto::Duration::from_millis(500);
        h.run_until(step, |_| false);
    }
    assert!(expired(&h), "not dropped as stale");
    assert!(!h.nodes[m].core.knows(id), "passed round to M");
    assert!(!at_cap(&h, id));
}

/// Two neighbours that drain in turn do not hold each other up. A
/// draining node takes nothing, so what one holds for the other it took
/// before its own drain, and the other, draining then, took nothing to
/// hold for it: here L holds a message for N, which drains, and then
/// drains itself. Once N is back from its swap, L hands the message on
/// and has drained well within its bound (spec "Delivery queue",
/// "Updates over the mesh").
#[test]
fn two_neighbours_draining_in_turn_do_not_hold_each_other_up() {
    let (n, l, b) = (0, 1, 2);
    let mut h = build(
        vec![
            relay(0.0, 0.0, 1),
            relay(300.0, 0.0, 2),
            relay(600.0, 0.0, 3),
        ],
        &[(n, l), (l, b)],
    );
    let rotation = Config::default().boot_listen;
    let ttl = Config::default().delivery_ttl;
    let nid = h.nodes[n].core.id();
    h.command(n, AppCommand::Drain(true));
    h.run_until(h.now + rotation, |_| false);
    let (dst_pos, payload) = (h.nodes[n].core.config().position, b"to N");
    h.command(
        b,
        AppCommand::Send {
            dst_pos,
            dst_id: nid,
            payload,
        },
    );
    let id = MsgId::of(dst_pos, nid, payload);
    assert!(h.run_until(h.now + secs(60), |h| waits(h, l, id, nid)));
    h.command(l, AppCommand::Drain(true));
    let close = h.now;
    assert!(!h.nodes[l].core.drained(h.now), "L holds the message");
    // N holds nothing: it reboots at once, and is off the air for its
    // swap.
    assert!(h.nodes[n].core.drained(h.now));
    h.kill(n);
    h.run_until(h.now + secs(59), |_| false);
    h.reboot(n, relay(0.0, 0.0, 1));
    assert!(
        h.run_until(close + ttl, |h| h.nodes[l].core.drained(h.now)),
        "L not drained within its bound"
    );
    std::println!("L drained {:?} after its close", h.now.since(close));
    let half = bark_proto::Duration::from_micros(ttl.as_micros() / 2);
    assert!(h.now.since(close) < half, "L waited long");
    assert!(
        h.run_until(h.now + secs(10), |h| delivered(h, n).contains(&id)),
        "not delivered"
    );
    assert!(!at_cap(&h, id));
}
