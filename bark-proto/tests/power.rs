//! A node's role by its power (spec "Hardware", "Sleepy nodes", roadmap
//! D71): on external power a relay, on battery sleepy, set at boot and at
//! every change the driver reports.

mod common;

use bark_proto::wire::Packet;
use bark_proto::{
    AppCommand, AppEvent, Config, Delivery, DropReason, Instant, MsgId, Position, Relay, Trace,
};
use common::{secs, Harness};

/// A position `east` metres from a point in Kyiv.
fn pos(east: f64) -> Position {
    let (lat, lon) = (50.45, 30.52);
    let m_per_deg = 6_371_000.0 * core::f64::consts::PI / 180.0;
    Position::from_degrees(lat, lon + east / (m_per_deg * lat.to_radians().cos()))
}

/// A node at `east` with the design's boot listen, and whether it is on
/// external power at boot.
fn node(east: f64, external: bool, seed: u64) -> (Relay, bool) {
    let config = Config {
        position: pos(east),
        ..Config::default()
    };
    (Relay::new(config, seed), external)
}

fn at(s: u64) -> Instant {
    Instant::ZERO + secs(s)
}

fn power(h: &mut Harness<Relay>, i: usize, external: bool) {
    h.command(i, AppCommand::Power { external });
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

/// Whether node `i`'s last heartbeat had the sleepy flag.
fn beats_sleepy(h: &Harness<Relay>, i: usize) -> Option<bool> {
    h.nodes[i]
        .sent
        .iter()
        .rev()
        .find_map(|(_, f)| match Packet::decode(f.payload()) {
            Ok(Packet::Heartbeat(hb)) => Some(hb.sleepy),
            _ => None,
        })
}

/// Node `from`'s application sends `text` to node `to` at its address now.
fn send(h: &mut Harness<Relay>, from: usize, to: usize, text: &[u8]) -> MsgId {
    let (dst_pos, dst_id) = (h.nodes[to].core.address(), h.nodes[to].core.id());
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

/// A node on external power at boot is a relay, one on battery sleepy,
/// each with its role's boot listen; a report while it boots sets the role
/// at once, with no trace, since nobody depends on it yet.
#[test]
fn the_power_at_boot_sets_the_role() {
    let (r, s, late) = (0, 1, 2);
    let mut h = Harness::powered(vec![
        node(0.0, true, 1),
        node(50.0, false, 2),
        node(80.0, false, 3),
    ]);
    h.link(r, s);
    h.link(r, late);
    // Plugged in while it boots: a relay, its boot listen one rotation.
    h.run_until(at(100), |_| false);
    power(&mut h, late, true);
    let rotation = Config::default().boot_listen;
    h.run_until(at(rotation.as_micros() / 1_000_000 + 1), |_| false);
    assert!(!h.nodes[r].core.sleepy() && !h.nodes[r].core.booting());
    assert!(!h.nodes[late].core.sleepy() && !h.nodes[late].core.booting());
    h.run_until(at(1000), |_| false);
    assert!(h.nodes[s].core.sleepy());
    let rid = h.nodes[r].core.id();
    assert_eq!(h.nodes[s].core.anchor().map(|a| a.id), Some(rid));
    assert_eq!(beats_sleepy(&h, s), Some(true));
    assert_eq!(beats_sleepy(&h, late), Some(false));
    for i in [r, s, late] {
        assert_eq!(traced(&h, i, Trace::Role { sleepy: true }), 0);
        assert_eq!(traced(&h, i, Trace::Role { sleepy: false }), 0);
    }
}

/// A sleepy node plugged in is a relay at once: it leaves its anchor, its
/// address is its own position, and it heartbeats as a relay. Its old
/// anchor, which held a message for it, hands it over once it hears it as
/// a relay (spec "Delivery queue").
#[test]
fn a_sleepy_node_plugged_in_relays_and_gets_its_mail() {
    let (r, o, s) = (0, 1, 2);
    let mut h = Harness::powered(vec![
        node(0.0, true, 11),
        node(300.0, true, 12),
        node(20.0, false, 13),
    ]);
    h.link(r, o);
    h.link(r, s);
    h.start_at(s, at(10));
    h.run_until(at(1000), |_| false);
    let rid = h.nodes[r].core.id();
    assert_eq!(h.nodes[s].core.anchor().map(|a| a.id), Some(rid));
    // Out of reach for a while: the message waits at its anchor.
    h.unlink(r, s);
    let id = send(&mut h, o, s, b"for the node on the charger");
    h.run_for(secs(60));
    assert!(h.nodes[r].core.delivery_queue().any(|(m, _)| m == id));
    h.link(r, s);
    power(&mut h, s, true);
    assert!(!h.nodes[s].core.sleepy());
    assert_eq!(h.nodes[s].core.anchor(), None);
    assert_eq!(h.nodes[s].core.address(), pos(20.0));
    assert_eq!(traced(&h, s, Trace::Role { sleepy: false }), 1);
    assert!(h.run_until(h.now + secs(300), |h| delivered_at(h, s, id).is_some()));
    h.run_for(secs(1));
    assert!(h.nodes[r].traces.iter().any(|(_, t)| matches!(
        t,
        Trace::Delivered { msg_id, by: Delivery::Relay, .. } if *msg_id == id
    )));
    assert_eq!(beats_sleepy(&h, s), Some(false));
}

/// A relay unplugged turns sleepy: it holds nothing, so the message it
/// held for a sleepy node is dropped with a trace, and it anchors to a
/// relay it knows. Its own sleepy node leaves it for another relay
/// (spec "Relay choice").
#[test]
fn a_relay_unplugged_turns_sleepy_and_holds_nothing() {
    let (r, o, s) = (0, 1, 2);
    let mut h = Harness::powered(vec![
        node(0.0, true, 21),
        node(300.0, true, 22),
        node(20.0, false, 23),
    ]);
    h.link(r, o);
    h.link(r, s);
    h.link(o, s);
    h.set_rssi(r, s, -80);
    h.set_rssi(o, s, -100);
    h.start_at(s, at(10));
    h.run_until(at(1000), |_| false);
    let (rid, oid) = (h.nodes[r].core.id(), h.nodes[o].core.id());
    assert_eq!(h.nodes[s].core.anchor().map(|a| a.id), Some(rid));
    h.unlink(r, s);
    let id = send(&mut h, o, s, b"held when the charger goes");
    h.run_for(secs(60));
    assert!(h.nodes[r].core.delivery_queue().any(|(m, _)| m == id));
    h.link(r, s);
    power(&mut h, r, false);
    h.run_for(secs(1));
    assert!(h.nodes[r].core.sleepy());
    assert_eq!(h.nodes[r].core.delivery_queue().count(), 0);
    assert!(h.nodes[r].traces.iter().any(|(_, t)| matches!(
        t,
        Trace::Dropped { msg_id, reason: DropReason::Unplugged } if *msg_id == id
    )));
    assert_eq!(h.nodes[r].core.anchor().map(|a| a.id), Some(oid));
    assert_eq!(traced(&h, r, Trace::Role { sleepy: true }), 1);
    let moved = |h: &Harness<Relay>| h.nodes[s].core.anchor().map(|a| a.id) == Some(oid);
    assert!(h.run_until(h.now + secs(1500), moved));
}

/// A relay unplugged with no other relay in reach is anchorless, and back
/// on the charger a relay again, at its own position.
#[test]
fn a_lone_relay_unplugged_is_anchorless_and_plugged_in_again_a_relay() {
    let r = 0;
    let mut h = Harness::powered(vec![node(0.0, true, 31)]);
    h.run_until(at(400), |_| false);
    power(&mut h, r, false);
    h.run_for(secs(1));
    assert!(h.nodes[r].core.anchorless());
    h.run_for(secs(300));
    power(&mut h, r, true);
    assert!(!h.nodes[r].core.sleepy());
    h.run_for(secs(60));
    assert_eq!(beats_sleepy(&h, r), Some(false));
    assert_eq!(h.nodes[r].core.address(), pos(0.0));
}
