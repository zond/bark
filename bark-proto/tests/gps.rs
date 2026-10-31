//! When a sleepy node runs its GPS (spec "Beats", roadmap D80, D91):
//! only while it has no anchor, a minute at most, and not for eight
//! hours after a search that found nothing.

mod common;

use bark_proto::gps::{BACKOFF, SEARCH_LIMIT};
use bark_proto::sniff::LONG_PREAMBLE_SYMBOLS;
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

/// A node that boots sleepy and anchors in its first rotation never
/// searches: not at boot, not at the anchor, not at its beats. A relay
/// on external power never asks.
#[test]
fn no_search_with_an_anchor() {
    let mut h = pair();
    h.run_until(at(4000), |_| false);
    assert!(h.nodes[1].core.anchor().is_some());
    assert!(searches(&h, 1, Instant::ZERO).is_empty());
    assert!(beats(&h, 1, Instant::ZERO) > 10);
    assert!(searches(&h, 0, Instant::ZERO).is_empty(), "a relay asked");
}

/// An anchorless node searches once a delivery TTL while its searches
/// find a fix, and after one that finds nothing, not for the back-off;
/// a search that finds nothing runs a minute.
#[test]
fn a_failed_search_holds_the_next_back() {
    let mut h = pair();
    h.unlink(0, 1);
    h.run_until(at(3 * 3600), |_| false);
    assert!(h.nodes[1].core.anchorless());
    // The search when it turns anchorless at its first rotation's end,
    // held back by its failure.
    let s = searches(&h, 1, Instant::ZERO);
    assert_eq!(s.len(), 1, "{s:?}");
    assert!(s[0].1.since(s[0].0) <= SEARCH_LIMIT + secs(1));
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

/// A search under way ends when the node takes an anchor, with a
/// `Locate` at that instant: it works by the anchor's position from
/// then, and runs no search while it keeps one.
#[test]
fn a_search_under_way_ends_at_an_anchor() {
    let mut h = pair();
    // Nothing reaches the sleepy node: anchorless at its first rotation's
    // end, it searches.
    h.lose = Some(Box::new(|_, _, to, _| to == 1));
    assert!(h.run_until(at(1000), |h| !searches(h, 1, Instant::ZERO).is_empty()));
    let (started, until) = searches(&h, 1, Instant::ZERO)[0];
    assert_eq!(until, started + SEARCH_LIMIT);
    // The relay's answer to its beat reaches it, a heartbeat of the
    // relay's with the long preamble on its channel (spec "Sleepy nodes alone"),
    // put on the air here since the relay hears its beat only once a
    // rotation: it anchors, and the search ends there.
    h.lose = None;
    h.run_for(secs(5));
    let mut answer = h.nodes[0]
        .sent
        .iter()
        .rev()
        .find(|(_, f)| matches!(Packet::decode(f.payload()), Ok(Packet::Heartbeat(_))))
        .map(|(_, f)| *f)
        .expect("a heartbeat of the relay");
    answer.channel = h.resting(1).expect("listening");
    answer.preamble_symbols = LONG_PREAMBLE_SYMBOLS;
    h.inject(0, answer);
    assert!(h.run_until(until, |h| h.nodes[1].core.anchor().is_some()));
    let s = searches(&h, 1, Instant::ZERO);
    assert_eq!(s.len(), 2, "{s:?}");
    assert_eq!(s[1].0, s[1].1, "{s:?}");
    assert!(s[1].0 < until);
    // With an anchor, none for hours.
    h.run_until(at(4 * 3600), |_| false);
    assert_eq!(searches(&h, 1, Instant::ZERO).len(), 2);
    assert!(h.nodes[1].core.anchor().is_some());
}
