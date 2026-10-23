//! A node's role by its power (spec "Nodes", "Sleepy nodes", roadmap
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

/// The grace of these tests, shorter than the design's to keep them quick.
const GRACE: u64 = 600;

/// A node at `east` with the design's first rotation, and whether it is on
/// external power at boot.
fn node(east: f64, external: bool, seed: u64) -> (Relay, bool) {
    let config = Config {
        position: pos(east),
        grace: secs(GRACE),
        ..Config::default()
    };
    (Relay::new(config, seed), external)
}

fn fix(h: &mut Harness<Relay>, i: usize, east: f64) {
    h.command(
        i,
        AppCommand::Fix {
            position: pos(east),
        },
    );
}

fn delivered_by(h: &Harness<Relay>, i: usize, id: MsgId) -> Option<Delivery> {
    h.nodes[i].traces.iter().find_map(|(_, t)| match t {
        Trace::Delivered { msg_id, by, .. } if *msg_id == id => Some(*by),
        _ => None,
    })
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

/// How many heartbeats node `i` sent for a change of its flags or map
/// point.
fn changes(h: &Harness<Relay>, i: usize) -> usize {
    traced(h, i, Trace::Changed)
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

/// A node on external power at boot is a relay, one on battery sleepy; a
/// report while it boots sets the role at once, with no trace, since
/// nobody depends on it yet.
#[test]
fn the_power_at_boot_sets_the_role() {
    let (r, s, late) = (0, 1, 2);
    let mut h = Harness::powered(vec![
        node(0.0, true, 1),
        node(50.0, false, 2),
        node(80.0, false, 3),
    ]);
    h.link(r, s);
    // Plugged in while it boots, before it has heard a relay, which would
    // end a sleepy node's first rotation: a relay.
    h.run_until(at(100), |_| false);
    assert!(h.nodes[late].core.booting());
    power(&mut h, late, true);
    h.link(r, late);
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
/// address is its own position, and it heartbeats as a relay, the first
/// time at once on its home channel, its anchor's. Its old anchor, which
/// held a message for it, hears it and hands the message over then
/// (spec "Heartbeat", "Delivery").
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
    assert!(h.run_until(h.now + secs(5), |h| delivered_at(h, s, id).is_some()));
    assert_eq!(changes(&h, s), 1);
    h.run_for(secs(1));
    assert!(h.nodes[r].traces.iter().any(|(_, t)| matches!(
        t,
        Trace::Delivered { msg_id, by: Delivery::Relay, .. } if *msg_id == id
    )));
    assert_eq!(beats_sleepy(&h, s), Some(false));
}

/// A relay holding a message for its sleepy node S, out of reach, and a
/// second relay O that S also hears, weaker.
fn holding(seed: u64) -> (Harness<Relay>, MsgId) {
    let (r, o, s) = (0, 1, 2);
    let mut h = Harness::powered(vec![
        node(0.0, true, seed),
        node(300.0, true, seed + 1),
        node(20.0, false, seed + 2),
    ]);
    h.link(r, o);
    h.link(r, s);
    h.link(o, s);
    h.set_rssi(r, s, -80);
    h.set_rssi(o, s, -100);
    h.start_at(s, at(10));
    h.run_until(at(1000), |_| false);
    let rid = h.nodes[r].core.id();
    assert_eq!(h.nodes[s].core.anchor().map(|a| a.id), Some(rid));
    h.unlink(r, s);
    let id = send(&mut h, o, s, b"held when the charger goes");
    h.run_for(secs(60));
    assert!(h.nodes[r].core.delivery_queue().any(|(m, _)| m == id));
    (h, id)
}

/// A relay unplugged changes nothing for its grace: it still owns and
/// queues, and power back within it, nothing has happened.
#[test]
fn a_relay_unplugged_relays_through_its_grace() {
    let (r, o, s) = (0, 1, 2);
    let (mut h, _) = holding(21);
    power(&mut h, r, false);
    h.run_for(secs(GRACE / 2));
    assert!(!h.nodes[r].core.sleepy() && !h.nodes[r].core.leaving());
    let id = send(&mut h, o, s, b"sent in the grace");
    h.run_for(secs(30));
    assert!(h.nodes[r].core.delivery_queue().any(|(m, _)| m == id));
    power(&mut h, r, true);
    h.run_for(secs(2 * GRACE));
    assert!(!h.nodes[r].core.sleepy());
    assert_eq!(traced(&h, r, Trace::GraceEnded), 0);
    assert_eq!(traced(&h, r, Trace::Role { sleepy: true }), 0);
    assert_eq!(beats_sleepy(&h, r), Some(false));
}

/// After its grace a relay takes no more mail and flushes what it holds,
/// each message once with the long preamble; then it turns sleepy, holding
/// nothing: what the flush did not deliver is dropped with a trace. It
/// anchors to a relay it knows, and its own sleepy node leaves it for
/// another (spec "Relay choice").
#[test]
fn after_its_grace_a_relay_flushes_and_turns_sleepy() {
    for lost in [false, true] {
        let (r, o, s) = (0, 1, 2);
        let (mut h, id) = holding(31);
        let oid = h.nodes[o].core.id();
        if !lost {
            // Back in reach, but R misses its beats: S sleeps between
            // them and hears only the long preamble.
            h.link(r, s);
            h.lose = Some(Box::new(move |_, from, to, bytes| {
                from == s && to == r && matches!(Packet::decode(bytes), Ok(Packet::Heartbeat(_)))
            }));
        }
        power(&mut h, r, false);
        let off = h.now;
        h.run_until(off + secs(GRACE) - secs(1), |_| false);
        assert!(!h.nodes[r].core.leaving());
        assert!(h.run_until(off + secs(GRACE + 60), |h| h.nodes[r].core.sleepy()));
        assert_eq!(traced(&h, r, Trace::GraceEnded), 1);
        assert_eq!(traced(&h, r, Trace::Role { sleepy: true }), 1);
        assert_eq!(h.nodes[r].core.delivery_queue().count(), 0);
        if lost {
            assert!(h.nodes[r].traces.iter().any(|(_, t)| matches!(
                t,
                Trace::Dropped { msg_id, reason: DropReason::Unplugged } if *msg_id == id
            )));
        } else {
            assert_eq!(
                delivered_by(&h, r, id),
                Some(Delivery::Flush),
                "lost: {lost}"
            );
            assert!(delivered_at(&h, s, id).is_some());
        }
        assert_eq!(h.nodes[r].core.anchor().map(|a| a.id), Some(oid));
        h.link(r, s);
        h.lose = None;
        let moved = |h: &Harness<Relay>| h.nodes[s].core.anchor().map(|a| a.id) == Some(oid);
        assert!(h.run_until(h.now + secs(1500), moved), "lost: {lost}");
    }
}

/// A relay carried in its grace flushed at its first far fix: at the
/// grace's end it turns sleepy at once, with no second flush, and stays
/// carried: its beats are flagged sleepy and carried (roadmap D72).
#[test]
fn a_relay_carried_in_its_grace_turns_sleepy_carried_without_a_second_flush() {
    let (r, s) = (0, 2);
    let (mut h, id) = holding(41);
    let sid = h.nodes[s].core.id();
    let long_to_s = |h: &Harness<Relay>| {
        h.nodes[r]
            .sent
            .iter()
            .filter(|(_, f)| {
                matches!(Packet::decode(f.payload()), Ok(Packet::Data(d)) if d.dst_hop == sid)
                    && f.preamble_symbols == bark_proto::sniff::LONG_PREAMBLE_SYMBOLS
            })
            .count()
    };
    power(&mut h, r, false);
    let off = h.now;
    let before = long_to_s(&h);
    for k in 0..=8 {
        fix(&mut h, r, 100.0 + 10.0 * k as f64);
        h.run_for(secs(10));
    }
    assert!(h.nodes[r].core.carried());
    assert_eq!(long_to_s(&h), before + 1, "the far fix's flush");
    assert!(h.run_until(off + secs(GRACE + 1), |h| h.nodes[r].core.sleepy()));
    assert_eq!(long_to_s(&h), before + 1, "no second flush");
    assert!(h.nodes[r].core.carried());
    h.run_for(secs(5));
    let last = h.nodes[r]
        .sent
        .iter()
        .rev()
        .find_map(|(_, f)| match Packet::decode(f.payload()) {
            Ok(Packet::Heartbeat(hb)) => Some(hb),
            _ => None,
        });
    assert!(last.is_some_and(|hb| hb.sleepy && hb.carried), "{last:?}");
    assert!(h.nodes[r].traces.iter().any(|(_, t)| matches!(
        t,
        Trace::Dropped { msg_id, reason: DropReason::Unplugged } if *msg_id == id
    )));
}

/// A relay unplugged with no other relay in reach is anchorless, and back
/// on the charger a relay again, at its own position.
#[test]
fn a_lone_relay_unplugged_is_anchorless_and_plugged_in_again_a_relay() {
    let r = 0;
    let mut h = Harness::powered(vec![node(0.0, true, 31)]);
    h.run_until(at(400), |_| false);
    power(&mut h, r, false);
    h.run_for(secs(GRACE + 1));
    assert!(h.nodes[r].core.anchorless());
    h.run_for(secs(300));
    power(&mut h, r, true);
    assert!(!h.nodes[r].core.sleepy());
    h.run_for(secs(60));
    assert_eq!(beats_sleepy(&h, r), Some(false));
    assert_eq!(h.nodes[r].core.address(), pos(0.0));
}

/// A node that turns carried, a relay or a sleepy node, heartbeats at
/// once on its home channel with the flag; carried, its position changes
/// with each fix, and it sends no heartbeat for that beyond its slots or
/// beats (spec "Heartbeat", roadmap D72).
#[test]
fn a_change_of_flag_heartbeats_at_once_and_a_carried_node_s_fixes_do_not() {
    for external in [true, false] {
        a_change_of_flag_heartbeats_at_once(external);
    }
}

fn a_change_of_flag_heartbeats_at_once(external: bool) {
    let r = 0;
    let mut h = Harness::powered(vec![node(0.0, external, 51)]);
    h.run_until(at(400), |_| false);
    let start = h.now;
    let home = h.nodes[r].core.home();
    let mut east = 100.0;
    while !h.nodes[r].core.carried() {
        fix(&mut h, r, east);
        east += 1.4;
        h.run_for(secs(1));
    }
    h.run_for(secs(1));
    assert_eq!(changes(&h, r), 1);
    let flagged = h.nodes[r].sent.iter().any(|(t, f)| {
        *t > start
            && f.channel == home
            && matches!(Packet::decode(f.payload()), Ok(Packet::Heartbeat(hb))
                if hb.carried && hb.sleepy == !external)
    });
    assert!(flagged, "on its home channel, external: {external}");
    for _ in 0..300 {
        fix(&mut h, r, east);
        east += 1.4;
        h.run_for(secs(1));
    }
    assert_eq!(changes(&h, r), 1);
    assert_eq!(h.nodes[r].core.config().position, pos(east - 1.4));
}

/// A relay that turns sleepy says so on the channel it was at home on:
/// its first heartbeat with the sleepy flag goes there, once, whatever
/// its new anchor's channel.
#[test]
fn a_relay_turning_sleepy_says_so_on_its_old_channel() {
    let (r, o) = (0, 1);
    let mut h = Harness::powered(vec![node(0.0, true, 61), node(300.0, true, 62)]);
    h.link(r, o);
    h.run_until(at(400), |_| false);
    let old = h.nodes[r].core.home();
    power(&mut h, r, false);
    assert!(h.run_until(h.now + secs(GRACE + 5), |h| h.nodes[r].core.sleepy()));
    h.run_for(secs(5));
    let sleepy_on_old = h.nodes[r]
        .sent
        .iter()
        .filter(|(_, f)| {
            f.channel == old
                && matches!(Packet::decode(f.payload()), Ok(Packet::Heartbeat(hb)) if hb.sleepy)
        })
        .count();
    assert!(sleepy_on_old >= 1);
    assert!(changes(&h, r) <= 1);
}
