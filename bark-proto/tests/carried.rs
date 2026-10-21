//! A node that moves (spec "Position and time", roadmap D70): a fixed
//! node keeps its position until a run of far fixes makes it carried; a
//! carried node takes each fix, flags its heartbeats, owns nothing by
//! position, and nobody anchors to it; the first far fix flushes the
//! delivery queue; mail for a node heard as a relay goes to it directly.

mod common;

use bark_proto::sniff::LONG_PREAMBLE_SYMBOLS;
use bark_proto::wire::Packet;
use bark_proto::{
    AppCommand, AppEvent, Config, Delivery, Instant, MsgId, NodeId, Position, Relay, Trace,
};
use common::{secs, Harness};

/// A position `east` metres from a point in Kyiv.
fn pos(east: f64) -> Position {
    let (lat, lon) = (50.45, 30.52);
    let m_per_deg = 6_371_000.0 * core::f64::consts::PI / 180.0;
    Position::from_degrees(lat, lon + east / (m_per_deg * lat.to_radians().cos()))
}

/// A node at `east`, with the short boot listen of `tests/sleepy.rs` for
/// one that is not sleepy.
fn node(east: f64, sleepy: bool, seed: u64) -> Relay {
    let config = Config {
        fixed_after: secs(600),
        ..Config::default()
    };
    let boot_listen = if sleepy { config.boot_listen } else { secs(5) };
    Relay::new(
        Config {
            position: pos(east),
            boot_listen,
            sleepy,
            ..config
        },
        seed,
    )
}

fn at(s: u64) -> Instant {
    Instant::ZERO + secs(s)
}

fn fix(h: &mut Harness<Relay>, i: usize, east: f64) {
    h.command(
        i,
        AppCommand::Fix {
            position: pos(east),
        },
    );
}

fn traced(h: &Harness<Relay>, i: usize, want: Trace) -> usize {
    h.nodes[i].traces.iter().filter(|(_, t)| *t == want).count()
}

fn delivered_at(h: &Harness<Relay>, i: usize, id: MsgId) -> Option<Instant> {
    h.nodes[i].app.iter().find_map(|(t, e)| match e {
        AppEvent::Message { msg_id, .. } if *msg_id == id => Some(*t),
        _ => None,
    })
}

/// Node `from`'s application sends `text` to `(dst_pos, dst_id)`.
fn send(
    h: &mut Harness<Relay>,
    from: usize,
    dst_pos: Position,
    dst_id: NodeId,
    text: &[u8],
) -> MsgId {
    h.command(
        from,
        AppCommand::Send {
            dst_pos,
            dst_id,
            payload: text,
        },
    );
    MsgId::of(dst_pos, dst_id, text)
}

/// Whether the last heartbeat node `i` sent had the carried flag.
fn flagged(h: &Harness<Relay>, i: usize) -> bool {
    h.nodes[i]
        .sent
        .iter()
        .rev()
        .find_map(|(_, f)| match Packet::decode(f.payload()) {
            Ok(Packet::Heartbeat(hb)) => Some(hb.carried),
            _ => None,
        })
        .expect("a heartbeat")
}

/// A fixed node keeps its position through fixes within 50 m, and
/// through far fixes until they have run for 72 s with none near between;
/// then it takes each fix and flags its heartbeats. Its fixes staying
/// within 50 m of one place for `fixed_after` make it fixed again, where
/// it is then.
#[test]
fn a_relay_turns_carried_after_a_run_of_far_fixes_and_fixed_after_staying() {
    let r = 0;
    let mut h = Harness::new(vec![node(0.0, false, 1)]);
    h.run_until(at(100), |_| false);
    let home = h.nodes[r].core.config().position;
    fix(&mut h, r, 40.0);
    assert_eq!(h.nodes[r].core.config().position, home, "within 50 m");
    fix(&mut h, r, 100.0);
    assert_eq!(traced(&h, r, Trace::FarFix), 1);
    h.run_for(secs(30));
    fix(&mut h, r, 10.0);
    // The run starts again at the next far fix.
    h.run_for(secs(10));
    fix(&mut h, r, 120.0);
    assert_eq!(traced(&h, r, Trace::FarFix), 2);
    for k in 1..=7 {
        h.run_for(secs(10));
        fix(&mut h, r, 120.0 + 14.0 * k as f64);
    }
    assert!(!h.nodes[r].core.carried(), "70 s of far fixes");
    assert_eq!(h.nodes[r].core.config().position, home);
    h.run_for(secs(2));
    fix(&mut h, r, 220.0);
    assert!(h.nodes[r].core.carried());
    assert_eq!(h.nodes[r].core.config().position, pos(220.0));
    h.run_for(secs(30));
    assert!(flagged(&h, r));
    fix(&mut h, r, 260.0);
    assert_eq!(h.nodes[r].core.config().position, pos(260.0));
    // Standing at 300 m, within 50 m of where it stopped.
    for k in 0..=10 {
        fix(&mut h, r, 300.0 + (k % 3) as f64 * 20.0);
        if k < 10 {
            assert!(h.nodes[r].core.carried(), "{k} minutes still");
            h.run_for(secs(60));
        }
    }
    assert!(!h.nodes[r].core.carried());
    assert_eq!(
        traced(&h, r, Trace::Carried { carried: false }),
        1,
        "fixed again"
    );
    let stop = h.nodes[r].core.config().position;
    fix(&mut h, r, 380.0);
    assert_eq!(h.nodes[r].core.config().position, stop);
    h.run_for(secs(30));
    assert!(!flagged(&h, r));
}

/// A sleepy node takes each fix as before.
#[test]
fn a_sleepy_node_takes_each_fix() {
    let mut h = Harness::new(vec![node(0.0, true, 2)]);
    h.run_until(at(10), |_| false);
    fix(&mut h, 0, 500.0);
    assert_eq!(h.nodes[0].core.config().position, pos(500.0));
    assert!(!h.nodes[0].core.carried());
}

/// R holds a message for its sleepy node S, which waits for S's beat
/// after S did not answer. R's first far fix sends it at once, with the
/// long preamble, one attempt; lost, it is not flushed again, and waits
/// for the beat (spec "Delivery queue").
#[test]
fn the_first_far_fix_flushes_the_delivery_queue_once() {
    for lost in [false, true] {
        let (r, o, s) = (0, 1, 2);
        let mut h = Harness::new(vec![
            node(0.0, false, 11),
            node(300.0, false, 12),
            node(20.0, true, 13),
        ]);
        h.link(r, o);
        h.link(r, s);
        h.start_at(s, at(10));
        h.run_until(at(1000), |_| false);
        assert!(h.nodes[s].core.anchor().is_some());
        let sid = h.nodes[s].core.id();
        // S out of reach: the hand-overs at once go unanswered.
        h.unlink(r, s);
        let dst = h.nodes[s].core.address();
        let id = send(&mut h, o, dst, sid, b"for the walker's node");
        let waits = |h: &Harness<Relay>| {
            h.nodes[r]
                .traces
                .iter()
                .any(|(_, t)| matches!(t, Trace::DeliveryWaits { msg_id, tried: true, .. } if *msg_id == id))
        };
        assert!(h.run_until(h.now + secs(60), waits));
        let to_s = |h: &Harness<Relay>| {
            h.nodes[r]
                .sent
                .iter()
                .filter(|(_, f)| {
                    matches!(Packet::decode(f.payload()), Ok(Packet::Data(d)) if d.dst_hop == sid)
                        && f.preamble_symbols == LONG_PREAMBLE_SYMBOLS
                })
                .count()
        };
        let before = to_s(&h);
        if !lost {
            h.link(r, s);
        }
        let far = h.now;
        fix(&mut h, r, 120.0);
        h.run_until(far + secs(5), |_| false);
        assert_eq!(to_s(&h), before + 1, "one long attempt, lost: {lost}");
        if lost {
            assert_eq!(delivered_at(&h, s, id), None);
            fix(&mut h, r, 0.0);
            fix(&mut h, r, 130.0);
            h.run_for(secs(5));
            assert_eq!(to_s(&h), before + 1, "flushed once");
        } else {
            assert!(delivered_at(&h, s, id).is_some());
            assert!(h.nodes[r].traces.iter().any(|(_, t)| matches!(
                t,
                Trace::Delivered { msg_id, by: Delivery::Flush, .. } if *msg_id == id
            )));
        }
    }
}

/// A carried node owns nothing by position: a message to its position for
/// another node is not queued there but searched on, and dropped at the
/// origin; fixed, the same node would queue it (spec "Owner").
#[test]
fn a_carried_node_queues_mail_for_nobody() {
    for carried in [false, true] {
        let (r, o) = (0, 1);
        let mut h = Harness::new(vec![node(0.0, false, 21), node(300.0, false, 22)]);
        h.link(r, o);
        h.run_until(at(400), |_| false);
        if carried {
            for k in 0..=8 {
                fix(&mut h, r, 100.0 + k as f64);
                h.run_for(secs(10));
            }
            assert!(h.nodes[r].core.carried());
            h.run_until(h.now + secs(400), |_| false);
        }
        let dst = h.nodes[r].core.config().position;
        let id = send(&mut h, o, dst, NodeId([9; 4]), b"for a sleepy node");
        h.run_for(secs(30));
        let queued = h.nodes[r]
            .traces
            .iter()
            .any(|(_, t)| matches!(t, Trace::Queued { msg_id, .. } if *msg_id == id));
        assert_eq!(queued, !carried);
    }
}

/// A sleepy node leaves an anchor whose heartbeat turns carried: for
/// another relay where one is, else into anchorless mode
/// (spec "Relay choice", "Sleepy nodes alone").
#[test]
fn a_sleepy_node_leaves_an_anchor_that_turns_carried() {
    for other in [true, false] {
        let (first, second, s) = (0, 1, 2);
        let mut h = Harness::new(vec![
            node(0.0, false, 31),
            node(300.0, false, 32),
            node(50.0, true, 33),
        ]);
        h.link(first, s);
        h.set_rssi(first, s, -80);
        if other {
            h.link(second, s);
            h.set_rssi(second, s, -100);
        }
        h.start_at(s, at(10));
        h.run_until(at(1000), |_| false);
        let id_of = |h: &Harness<Relay>, i: usize| h.nodes[i].core.id();
        assert_eq!(
            h.nodes[s].core.anchor().map(|a| a.id),
            Some(id_of(&h, first))
        );
        // The anchor walks off, slowly enough to stay in reach.
        for k in 0..=8 {
            fix(&mut h, first, 60.0 + k as f64);
            h.run_for(secs(10));
        }
        assert!(h.nodes[first].core.carried());
        let left =
            |h: &Harness<Relay>| h.nodes[s].core.anchor().map(|a| a.id) != Some(id_of(h, first));
        assert!(
            h.run_until(h.now + secs(1500), left),
            "other relay: {other}"
        );
        let want = other.then(|| id_of(&h, second));
        assert_eq!(h.nodes[s].core.anchor().map(|a| a.id), want);
        assert_eq!(h.nodes[s].core.anchorless(), !other);
        assert_eq!(traced(&h, s, Trace::AnchorCarried), 1);
    }
}

/// R owns X's address while X, a relay, is off: the message waits in R's
/// delivery queue. Once R hears X's heartbeat, a node that is not sleepy,
/// it hands the message to X with the ordinary preamble, and it is done
/// (spec "Delivery queue").
#[test]
fn mail_for_a_node_heard_as_a_relay_goes_to_it() {
    let (r, o, x) = (0, 1, 2);
    let mut h = Harness::new(vec![
        node(0.0, false, 41),
        node(300.0, false, 42),
        node(30.0, false, 43),
    ]);
    h.link(r, o);
    h.link(r, x);
    h.start_at(x, at(600));
    h.run_until(at(400), |_| false);
    let (dst, xid) = (h.nodes[x].core.config().position, h.nodes[x].core.id());
    let id = send(&mut h, o, dst, xid, b"for a relay");
    h.run_for(secs(30));
    assert!(h.nodes[r]
        .traces
        .iter()
        .any(|(_, t)| matches!(t, Trace::Queued { msg_id, .. } if *msg_id == id)));
    assert!(h.run_until(at(1000), |h| delivered_at(h, x, id).is_some()));
    h.run_for(secs(1));
    assert!(h.nodes[r].traces.iter().any(|(_, t)| matches!(
        t,
        Trace::Delivered { msg_id, by: Delivery::Relay, .. } if *msg_id == id
    )));
    assert_eq!(h.nodes[r].core.delivery_queue().count(), 0);
}

/// An anchor that turns carried while its sleepy node still listens to
/// choose, with no other relay, leaves the node anchorless; the choice
/// that was due is not left pending.
#[test]
fn an_anchor_carried_while_its_node_chooses() {
    let (r, s) = (0, 1);
    let mut h = Harness::new(vec![node(0.0, false, 51), node(50.0, true, 52)]);
    h.link(r, s);
    h.start_at(s, at(10));
    assert!(h.run_until(at(1000), |h| h.nodes[s].core.anchor().is_some()));
    for k in 0..=8 {
        fix(&mut h, r, 60.0 + k as f64);
        h.run_for(secs(10));
    }
    assert!(h.nodes[r].core.carried());
    h.run_for(secs(1000));
    assert!(h.nodes[s].core.anchorless());
}
