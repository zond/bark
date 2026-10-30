//! A node with no place (spec "Position", roadmap D83): a board built
//! without one and with nothing stored takes its first accepted fix as
//! its place, fixed, at once, and sends no heartbeat until then.

mod common;

use bark_proto::wire::Packet;
use bark_proto::{AppCommand, Config, Instant, Position, Relay};
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

/// A sleepy node with no place may take an anchor but sends no beat; at
/// its first fix it rebuilds its anchor's position near its place, and
/// beats its own.
#[test]
fn a_sleepy_node_with_no_place_waits_for_its_first_fix() {
    let mut h = pair(false);
    h.run_until(at(1000), |_| false);
    assert!(heartbeats(&h, 1, Instant::ZERO, pos(0.0)).is_empty());
    h.command(
        1,
        AppCommand::Fix {
            position: pos(30.0),
        },
    );
    assert!(!h.nodes[1].core.carried());
    let anchor = h.nodes[1].core.anchor().expect("an anchor");
    assert_eq!(anchor.position, pos(0.0));
    let since = h.now;
    h.run_until(at(2000), |_| false);
    let sent = heartbeats(&h, 1, since, pos(0.0));
    assert!(!sent.is_empty() && sent.iter().all(|&p| p == pos(30.0)));
}
