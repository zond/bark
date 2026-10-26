//! How a relay runs its GPS, behind the research switch
//! `Config::relay_gps` (`docs/relay-gps-2026-10-26.md`): all the time by
//! default; resting, it searches at boot and, by the radio, when the
//! heartbeats of its fixed relay neighbours change more than fading
//! explains, and a far fix keeps it on until it is carried.

mod common;

use bark_proto::gps::RelayGps;
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

/// Five relays on USB 200 m apart, all hearing each other, node 0 with
/// `mode`.
fn row(mode: RelayGps) -> Harness<Relay> {
    let nodes = (0..5)
        .map(|i| {
            let config = Config {
                position: pos(200.0 * i as f64),
                relay_gps: if i == 0 { mode } else { RelayGps::Always },
                ..Config::default()
            };
            (Relay::new(config, i as u64 + 1), true)
        })
        .collect();
    let mut h = Harness::powered(nodes);
    for a in 0..5 {
        for b in a + 1..5 {
            h.link(a, b);
        }
    }
    h
}

fn searches(h: &Harness<Relay>, from: Instant) -> usize {
    h.nodes[0].located.iter().filter(|(t, _)| *t >= from).count()
}

/// By default a relay's GPS runs all the time and it never asks for a
/// search, on USB or on battery.
#[test]
fn by_default_a_relay_tracks() {
    let mut h = row(RelayGps::Always);
    h.run_until(at(600), |_| false);
    assert!(h.nodes[0].core.gps_tracks());
    h.command(0, AppCommand::Power { external: false });
    h.run_until(at(1200), |_| false);
    assert!(h.nodes[0].core.gps_tracks());
    assert_eq!(searches(&h, Instant::ZERO), 0);
}

/// With `Battery`, a relay tracks on USB and rests on battery, in its
/// grace too; with a map point it rests on USB as well.
#[test]
fn on_battery_it_rests() {
    let mut h = row(RelayGps::Battery);
    h.run_until(at(600), |_| false);
    assert!(h.nodes[0].core.gps_tracks());
    h.command(0, AppCommand::Power { external: false });
    h.run_until(at(1200), |_| false);
    assert!(!h.nodes[0].core.sleepy());
    assert!(!h.nodes[0].core.gps_tracks());
    h.command(0, AppCommand::Power { external: true });
    assert!(h.nodes[0].core.gps_tracks());
    h.command(0, AppCommand::SetPosition { position: pos(0.0) });
    assert!(!h.nodes[0].core.gps_tracks());
}

/// Resting, a still relay searches at boot and not again while its
/// neighbours' signals stay as they were; when every one of them drops by
/// 15 dB, as stepping into a building does, it searches within two
/// worst-case rotations.
#[test]
fn the_radio_starts_a_search() {
    let mut h = row(RelayGps::RadioEverywhere);
    h.run_until(at(3000), |_| false);
    assert!(!h.nodes[0].core.gps_tracks());
    assert_eq!(searches(&h, Instant::ZERO), 1, "{:?}", h.nodes[0].located);
    let quiet = common::SIGNAL_DBM;
    for b in 1..5 {
        h.set_rssi(0, b, quiet - 15);
    }
    h.run_until(at(3000 + 2 * 195 + 15), |_| false);
    assert!(searches(&h, at(3000)) >= 1, "{:?}", h.nodes[0].located);
}

/// A far fix keeps a resting relay's GPS on: 72 s of them make it carried,
/// and a near one sends the GPS back to rest.
#[test]
fn a_far_fix_keeps_it_on() {
    let mut h = row(RelayGps::RadioEverywhere);
    h.run_until(at(600), |_| false);
    assert!(!h.nodes[0].core.gps_tracks());
    h.command(0, AppCommand::Fix { position: pos(100.0) });
    assert!(h.nodes[0].core.gps_tracks());
    h.command(0, AppCommand::Fix { position: pos(10.0) });
    assert!(!h.nodes[0].core.gps_tracks());
    for s in 0..=72 {
        h.run_until(at(700 + s), |_| false);
        h.command(
            0,
            AppCommand::Fix {
                position: pos(100.0 + s as f64),
            },
        );
    }
    assert!(h.nodes[0].core.carried());
    assert!(h.nodes[0].core.gps_tracks());
}
