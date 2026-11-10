//! The drain before a reboot into a new image (spec "Send queue",
//! "Updates over the mesh", the install): a draining relay takes no
//! message under load and holds nothing well within the bound; what it
//! still holds at the bound is offered again after the reboot, the same
//! message.

mod common;

use bark_proto::{
    AppCommand, AppEvent, Config, DropReason, Instant, MsgId, NodeId, Position, Relay, Trace,
};
use common::{secs, Harness};
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
    assert!(h.nodes[m].core.held().next().is_none(), "held at the end");
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
    for i in 0..h.nodes.len() {
        assert!(
            !h.nodes[i]
                .traces
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
/// queue past the bound. The owner may reboot then, and what it held,
/// offered again as its own after the reboot, is the same message, back
/// in its delivery queue.
#[test]
fn a_message_held_past_the_bound_is_offered_again_after_the_reboot() {
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
        .held()
        .map(|x| (x.msg_id, x.dst_pos, x.dst_id, x.payload.to_vec()))
        .collect();
    assert_eq!(held, vec![(id, dst_pos, dark, payload.to_vec())]);
    // What the driver kept, offered again once the new image takes it.
    h.reboot(m, relay(300.0, 0.0, 20));
    assert!(h.run_until(h.now + secs(30), |h| h.accepts(m)));
    for (msg_id, dst_pos, dst_id, payload) in &held {
        h.command(
            m,
            AppCommand::Send {
                dst_pos: *dst_pos,
                dst_id: *dst_id,
                payload,
            },
        );
        assert_eq!(MsgId::of(*dst_pos, *dst_id, payload), *msg_id);
    }
    assert!(queued(&h), "not back in the delivery queue");
}
