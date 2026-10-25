//! When a sleepy node runs its GPS (`bark_proto::gps`,
//! `docs/gps-schedule-2026-10-25.md`): before every beat as spec "Beats"
//! is written, or only at the events that may have moved it.

mod common;

use bark_proto::gps::{GpsSchedule, BEAT_SEARCH_LIMIT, SEARCH_LIMIT};
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

/// A relay at 0 m and a sleepy node at 50 m, the second with `gps`.
fn pair(gps: GpsSchedule) -> Harness<Relay> {
    let node = |east, seed| {
        let config = Config {
            position: pos(east),
            gps,
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

/// As spec "Beats" is written, a search of up to ten minutes goes with
/// every beat.
#[test]
fn every_beat_searches_at_each_beat() {
    let mut h = pair(GpsSchedule::EveryBeat);
    h.run_until(at(4000), |_| false);
    let from = at(1000);
    let s = searches(&h, 1, from);
    assert_eq!(s.len(), beats(&h, 1, from), "{s:?}");
    assert!(s
        .iter()
        .all(|(t, until)| until.since(*t) <= BEAT_SEARCH_LIMIT + secs(1)));
    assert!(searches(&h, 0, Instant::ZERO).is_empty(), "a relay asked");
}

/// With the events schedule an anchored node searches when it turns
/// sleepy, once, and not at its beats; a search that finds nothing runs
/// a minute.
#[test]
fn events_search_at_boot_and_not_at_beats() {
    let mut h = pair(GpsSchedule::Events);
    h.run_until(at(4000), |_| false);
    assert!(h.nodes[1].core.anchor().is_some());
    let s = searches(&h, 1, Instant::ZERO);
    assert_eq!(s.len(), 1, "{s:?}");
    assert!(s[0].1.since(s[0].0) <= SEARCH_LIMIT + secs(1));
    assert!(beats(&h, 1, Instant::ZERO) > 10);
}

/// A search that found nothing holds the next back for the back-off;
/// one that found a fix does not.
#[test]
fn a_failed_search_holds_the_next_back() {
    let mut h = pair(GpsSchedule::Anchorless);
    h.unlink(0, 1);
    h.run_until(at(3 * 3600), |_| false);
    assert!(h.nodes[1].core.anchorless());
    let s = searches(&h, 1, Instant::ZERO);
    // Anchorless from the end of its boot: a search then and one an hour
    // after each failure.
    assert!((3..=4).contains(&s.len()), "{s:?}");
    for w in s.windows(2) {
        assert!(w[1].0.since(w[0].1) >= secs(3600) - secs(1), "{s:?}");
    }
    // Fixes found: once a delivery TTL.
    let mut h = pair(GpsSchedule::Anchorless);
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

/// No search while anchored with the anchorless schedule.
#[test]
fn anchorless_schedule_leaves_the_gps_off_while_anchored() {
    let mut h = pair(GpsSchedule::Anchorless);
    h.run_until(at(4000), |_| false);
    assert!(h.nodes[1].core.anchor().is_some());
    assert!(searches(&h, 1, Instant::ZERO).is_empty());
}

/// The fix asked for at a new anchor is where the node chose from: one
/// far from its last position does not check the anchor just chosen,
/// where a fix at a beat does.
#[test]
fn the_fix_at_a_choice_is_where_the_node_chose() {
    let checks = |gps| {
        let mut h = pair(gps);
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
        h.nodes[1]
            .traces
            .iter()
            .filter(|(_, t)| *t == Trace::AnchorCheck)
            .count()
    };
    assert_eq!(checks(GpsSchedule::Events), 0);
    assert_eq!(checks(GpsSchedule::EveryBeat), 1);
}

/// With the follow schedule a node whose fixes move searches at each
/// beat, and stops at a search that finds nothing.
#[test]
fn follow_searches_while_the_fixes_move() {
    let mut h = pair(GpsSchedule::Follow);
    // The relay hears it wherever it goes, so that it keeps its anchor.
    let (mut east, mut handed) = (50.0, 0);
    while h.now < at(3000) {
        h.run_for(secs(1));
        let n = h.nodes[1].located.len();
        if n > handed {
            handed = n;
            east += 300.0;
            h.command(
                1,
                AppCommand::Fix {
                    position: pos(east),
                },
            );
        }
    }
    let walking = searches(&h, 1, at(1000)).len();
    assert!(walking + 2 >= beats(&h, 1, at(1000)), "{walking}");
    // Indoors: the next search finds nothing, and none follows at beats.
    h.run_until(at(6000), |_| false);
    let after = searches(&h, 1, at(3000)).len();
    assert_eq!(after, 1);
}
