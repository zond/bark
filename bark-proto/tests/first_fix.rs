//! A node with no place (spec "Position", roadmap D83): a board built
//! without one and with nothing stored takes its first accepted fix as
//! its place, fixed, at once, and sends no heartbeat until then.

mod common;

use bark_proto::wire::Packet;
use bark_proto::{AppCommand, Channel, Config, Instant, Position, Relay, Stored};
use common::{secs, Harness};

/// A position `east` metres from a point in Kyiv.
fn pos(east: f64) -> Position {
    let (lat, lon) = (50.45, 30.52);
    let m_per_deg = 6_371_000.0 * core::f64::consts::PI / 180.0;
    Position::from_degrees(lat, lon + east / (m_per_deg * lat.to_radians().cos()))
}

fn at(s: u64) -> Instant {
    Instant::ZERO + secs(s)
}

/// A relay at 0 m, placed, and node 1 with no place, on USB or not.
fn pair(external: bool) -> Harness<Relay> {
    let placed = Config {
        position: pos(0.0),
        ..Config::default()
    };
    let unplaced = Config {
        placed: false,
        ..Config::default()
    };
    let mut h = Harness::powered(vec![
        (Relay::new(placed, 1), true),
        (Relay::new(unplaced, 2), external),
    ]);
    h.link(0, 1);
    h
}

/// The heartbeats node `i` sent from `from` on, with their short positions
/// rebuilt near `near`.
fn heartbeats(h: &Harness<Relay>, i: usize, from: Instant, near: Position) -> Vec<Position> {
    h.nodes[i]
        .sent
        .iter()
        .filter(|(t, _)| *t >= from)
        .filter_map(|(_, f)| match Packet::decode(f.payload()) {
            Ok(Packet::Heartbeat(hb)) => Some(hb.position(near)),
            _ => None,
        })
        .collect()
}

/// A relay with no place sends no heartbeat and stores nothing; its first
/// fix is its place, fixed, and it heartbeats there at once, its first
/// rotation's burst first; later fixes within the allowance move nothing.
#[test]
fn a_relay_with_no_place_waits_for_its_first_fix() {
    let mut h = pair(true);
    h.run_until(at(600), |_| false);
    assert!(heartbeats(&h, 1, Instant::ZERO, pos(0.0)).is_empty());
    assert!(h.nodes[1].stored.is_none());
    h.command(
        1,
        AppCommand::Fix {
            position: pos(300.0),
        },
    );
    assert!(!h.nodes[1].core.carried());
    assert_eq!(h.nodes[1].core.config().position, pos(300.0));
    h.run_until(at(620), |_| false);
    let sent = heartbeats(&h, 1, at(600), pos(0.0));
    assert!(sent.len() >= 10, "{}", sent.len());
    assert!(sent.iter().all(|&p| p == pos(300.0)));
    assert_eq!(h.nodes[1].stored.map(|s| s.place), Some(pos(300.0)));
    for k in 0..100 {
        h.run_for(secs(1));
        h.command(
            1,
            AppCommand::Fix {
                position: pos(300.0 + (k % 3) as f64 * 10.0),
            },
        );
    }
    assert!(!h.nodes[1].core.carried());
    assert_eq!(h.nodes[1].core.config().position, pos(300.0));
}

/// A map point places a node with no place as a fix would.
#[test]
fn a_map_point_places_it() {
    let mut h = pair(true);
    h.run_until(at(600), |_| false);
    h.command(
        1,
        AppCommand::SetPosition {
            position: pos(200.0),
        },
    );
    h.run_until(at(620), |_| false);
    let sent = heartbeats(&h, 1, at(600), pos(0.0));
    assert!(!sent.is_empty() && sent.iter().all(|&p| p == pos(200.0)));
}

/// A sleepy node with no place may take an anchor but sends no beat, and
/// searches all the same, since without a fix of its own it cannot
/// rebuild its anchor's position; at its first fix it rebuilds that near
/// its place, beats it as its address, and searches no more.
#[test]
fn a_sleepy_node_with_no_place_waits_for_its_first_fix() {
    let mut h = pair(false);
    h.run_until(at(1000), |_| false);
    assert!(heartbeats(&h, 1, Instant::ZERO, pos(0.0)).is_empty());
    assert!(h.nodes[1].core.anchor().is_some());
    let searches = h.nodes[1].located.len();
    assert_eq!(searches, 1, "{:?}", h.nodes[1].located);
    h.command(
        1,
        AppCommand::Fix {
            position: pos(30.0),
        },
    );
    assert!(!h.nodes[1].core.carried());
    let anchor = h.nodes[1].core.anchor().expect("an anchor");
    assert_eq!(anchor.position, pos(0.0));
    assert_eq!(h.nodes[1].core.config().position, pos(30.0));
    let since = h.now;
    h.run_until(at(2000), |_| false);
    let sent = heartbeats(&h, 1, since, pos(0.0));
    assert!(!sent.is_empty() && sent.iter().all(|&p| p == pos(0.0)));
    assert_eq!(h.nodes[1].located.len(), searches);
}

/// The check a driver makes before it hands a fix (spec "Position",
/// roadmap D74): within the hearing radius of a relay the node knows, or
/// any fix while it knows none or has no place.
#[test]
fn a_fix_is_in_reach_near_a_known_relay() {
    let mut h = pair(true);
    assert!(h.nodes[0].core.fix_in_reach(pos(10_000.0)), "knows none");
    assert!(h.nodes[1].core.fix_in_reach(pos(10_000.0)), "no place");
    h.run_until(at(600), |_| false);
    assert!(h.nodes[0].core.neighbours().next().is_none());
    h.command(
        1,
        AppCommand::Fix {
            position: pos(300.0),
        },
    );
    h.run_until(at(700), |_| false);
    let relay = &h.nodes[0].core;
    assert!(relay.neighbours().next().is_some());
    assert!(relay.fix_in_reach(pos(3_200.0)));
    assert!(!relay.fix_in_reach(pos(3_400.0)));
    assert!(!relay.fix_in_reach(pos(-2_800.0)));
}

/// The bench case of roadmap D133: a board built without a place whose
/// stored state says it was carried has no place, since a carried node's
/// stored place is where it stood when last fixed. It hears its
/// neighbour before it has a place, sends no heartbeat, checks its first
/// fix against no neighbour, and takes it as its place, fixed; the
/// neighbour it heard is rebuilt near it, so the next fix is in reach.
#[test]
fn a_node_stored_as_carried_has_no_place() {
    let stored = Stored {
        home: Channel::all().next().expect("a channel"),
        place: Position::from_degrees(59.2, 18.2),
        carried: true,
        map_point: false,
        anchor: None,
        relay: true,
        battery_scale: None,
    };
    let unplaced = Config {
        placed: false,
        stored: Some(stored),
        ..Config::default()
    };
    let placed = Config {
        position: pos(0.0),
        ..Config::default()
    };
    let mut h = Harness::powered(vec![
        (Relay::new(placed, 1), true),
        (Relay::new(unplaced, 2), true),
    ]);
    h.link(0, 1);
    h.run_until(at(600), |_| false);
    assert!(!h.nodes[1].core.placed());
    assert!(h.nodes[1].core.neighbours().next().is_some());
    assert!(heartbeats(&h, 1, Instant::ZERO, pos(0.0)).is_empty());
    assert!(h.nodes[1].core.fix_in_reach(pos(300.0)));
    h.command(
        1,
        AppCommand::Fix {
            position: pos(300.0),
        },
    );
    let node = &h.nodes[1].core;
    assert!(node.placed() && !node.carried());
    assert_eq!(node.config().position, pos(300.0));
    assert_eq!(node.neighbours().next().map(|n| n.position), Some(pos(0.0)));
    assert!(node.fix_in_reach(pos(350.0)));
    h.run_until(at(620), |_| false);
    let sent = heartbeats(&h, 1, at(600), pos(0.0));
    assert!(!sent.is_empty() && sent.iter().all(|&p| p == pos(300.0)));
    let kept = h.nodes[1].stored.expect("a place to keep");
    assert_eq!((kept.place, kept.carried), (pos(300.0), false));
}
