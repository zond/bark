//! When a sleepy node runs its GPS (spec "Beats", roadmap D80): only
//! when the radio says it may have moved, a minute at most, and not for
//! eight hours after a search that found nothing.

mod common;

use bark_proto::gps::{BACKOFF, SEARCH_LIMIT};
use bark_proto::wire::Packet;
use bark_proto::{AppCommand, Config, Instant, Position, Relay, Trace};
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

/// A relay at 0 m and a sleepy node at 50 m.
fn pair() -> Harness<Relay> {
    let node = |east, seed| {
        let config = Config {
            position: pos(east),
            ..Config::default()
        };
        Relay::new(config, seed)
    };
    let mut h = Harness::powered(vec![(node(0.0, 1), true), (node(50.0, 2), false)]);
    h.link(0, 1);
    h
}

/// The beats node `i` sent from `from` on.
fn beats(h: &Harness<Relay>, i: usize, from: Instant) -> usize {
    h.nodes[i]
        .sent
        .iter()
        .filter(|(t, f)| {
            *t >= from && matches!(Packet::decode(f.payload()), Ok(Packet::Heartbeat(_)))
        })
        .count()
}

fn searches(h: &Harness<Relay>, i: usize, from: Instant) -> Vec<(Instant, Instant)> {
    h.nodes[i]
        .located
        .iter()
        .copied()
        .filter(|(t, _)| *t >= from)
        .collect()
}

/// An anchored node searches when it boots sleepy, once, and not at its
/// beats; a search that finds nothing runs a minute. A relay never asks.
#[test]
fn a_search_at_boot_and_none_at_beats() {
    let mut h = pair();
    h.run_until(at(4000), |_| false);
    assert!(h.nodes[1].core.anchor().is_some());
    let s = searches(&h, 1, Instant::ZERO);
    assert_eq!(s.len(), 1, "{s:?}");
    assert!(s[0].1.since(s[0].0) <= SEARCH_LIMIT + secs(1));
    assert!(beats(&h, 1, Instant::ZERO) > 10);
    assert!(searches(&h, 0, Instant::ZERO).is_empty(), "a relay asked");
}

/// An anchorless node searches once a delivery TTL while its searches
/// find a fix, and after one that finds nothing, not for the back-off.
#[test]
fn a_failed_search_holds_the_next_back() {
    let mut h = pair();
    h.unlink(0, 1);
    h.run_until(at(3 * 3600), |_| false);
    assert!(h.nodes[1].core.anchorless());
    // The search at boot, held back by its failure past the end of its
    // first rotation.
    assert_eq!(searches(&h, 1, Instant::ZERO).len(), 1);
    h.run_until(at(10 * 3600), |_| false);
    let s = searches(&h, 1, Instant::ZERO);
    assert_eq!(s.len(), 2, "{s:?}");
    assert!(s[1].0.since(s[0].1) >= BACKOFF - secs(1), "{s:?}");
    // Fixes found: once a delivery TTL.
    let mut h = pair();
    h.unlink(0, 1);
    let mut handed = 0;
    while h.now < at(3 * 3600) {
        h.run_for(secs(5));
        let n = h.nodes[1].located.len();
        if n > handed {
            handed = n;
            h.command(
                1,
                AppCommand::Fix {
                    position: pos(50.0),
                },
            );
        }
    }
    let s = searches(&h, 1, Instant::ZERO);
    assert!(s.len() >= 15, "{s:?}");
}

/// The fix asked for at a new anchor is where the node chose from: one
/// far from its last position does not check the anchor just chosen.
#[test]
fn the_fix_at_a_choice_is_where_the_node_chose() {
    let mut h = pair();
    h.run_until(at(30), |h| h.nodes[1].core.anchor().is_some());
    assert!(h.nodes[1].core.anchor().is_some());
    // 400 m from where it was, farther than the move bound from the
    // anchor 50 m away.
    h.command(
        1,
        AppCommand::Fix {
            position: pos(450.0),
        },
    );
    h.run_until(at(2000), |_| false);
    let checks = h.nodes[1]
        .traces
        .iter()
        .filter(|(_, t)| *t == Trace::AnchorCheck)
        .count();
    assert_eq!(checks, 0);
}
