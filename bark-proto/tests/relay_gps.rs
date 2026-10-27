//! How a relay runs its GPS (spec "Beats", roadmap D82): all the time on
//! external power and through its grace on battery; after that, and with a
//! map point, it rests, searching at boot and when the heartbeats of its
//! fixed relay neighbours change more than fading explains; a far fix
//! keeps it on until it is carried.

mod common;

use bark_proto::{AppCommand, BatteryLevels, Config, Duration, Instant, MsgId, Position, Relay};
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

/// The grace of the relays here, shorter than the design's hour.
const GRACE: Duration = Duration::from_secs(300);

/// Five relays on USB 200 m apart, all hearing each other, with battery
/// levels, so that a fixed one on battery relays on past its grace
/// (roadmap D81).
fn row() -> Harness<Relay> {
    let nodes = (0..5)
        .map(|i| {
            let config = Config {
                position: pos(200.0 * i as f64),
                battery: Some(BatteryLevels::DESIGN),
                grace: GRACE,
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
    h.nodes[0]
        .located
        .iter()
        .filter(|(t, _)| *t >= from)
        .count()
}

/// Node 0 on battery from 600 s, past its grace from 900 s.
fn resting() -> Harness<Relay> {
    let mut h = row();
    h.run_until(at(600), |_| false);
    h.command(0, AppCommand::Power { external: false });
    h
}

/// On USB a relay's GPS runs and it never asks for a search; on battery
/// it runs through the grace and then rests; back on USB it runs again.
#[test]
fn it_runs_on_usb_and_through_the_grace() {
    let mut h = resting();
    assert!(h.nodes[0].core.gps_tracks());
    h.run_until(at(600 + 290), |_| false);
    assert!(h.nodes[0].core.gps_tracks());
    h.run_until(at(600 + 310), |_| false);
    assert!(!h.nodes[0].core.sleepy());
    assert!(!h.nodes[0].core.gps_tracks());
    h.command(0, AppCommand::Power { external: true });
    assert!(h.nodes[0].core.gps_tracks());
    assert_eq!(searches(&h, Instant::ZERO), 0);
}

/// A relay with a map point rests its GPS on USB too: no fix may move it.
#[test]
fn a_map_point_rests_it() {
    let mut h = row();
    h.run_until(at(600), |_| false);
    h.command(0, AppCommand::SetPosition { position: pos(0.0) });
    assert!(!h.nodes[0].core.gps_tracks());
    h.run_until(at(3000), |_| false);
    assert_eq!(searches(&h, Instant::ZERO), 0);
}

/// Resting, a still relay does not search while its neighbours' signals
/// stay as they were; when every one of them drops by 15 dB, as stepping
/// into a building does, it searches within two worst-case rotations.
#[test]
fn the_radio_starts_a_search() {
    let mut h = resting();
    h.run_until(at(3000), |_| false);
    assert!(!h.nodes[0].core.gps_tracks());
    assert_eq!(searches(&h, Instant::ZERO), 0, "{:?}", h.nodes[0].located);
    for b in 1..5 {
        h.set_rssi(0, b, common::SIGNAL_DBM - 15);
    }
    h.run_until(at(3000 + 2 * 195 + 15), |_| false);
    assert!(searches(&h, at(3000)) >= 1, "{:?}", h.nodes[0].located);
}

/// A far fix keeps a resting relay's GPS on: 72 s of them make it carried,
/// and a near one sends the GPS back to rest.
#[test]
fn a_far_fix_keeps_it_on() {
    let mut h = resting();
    h.run_until(at(1000), |_| false);
    assert!(!h.nodes[0].core.gps_tracks());
    h.command(
        0,
        AppCommand::Fix {
            position: pos(100.0),
        },
    );
    assert!(h.nodes[0].core.gps_tracks());
    h.command(
        0,
        AppCommand::Fix {
            position: pos(10.0),
        },
    );
    assert!(!h.nodes[0].core.gps_tracks());
    for s in 0..=72 {
        h.run_until(at(1100 + s), |_| false);
        h.command(
            0,
            AppCommand::Fix {
                position: pos(100.0 + s as f64),
            },
        );
        assert!(h.nodes[0].core.gps_tracks() || h.nodes[0].core.carried());
    }
    // Carried past its grace on battery, it turns sleepy (roadmap D81).
    assert!(h.nodes[0].core.carried());
}

/// A fixed relay on battery past its grace, holding a message for its
/// sleepy node, reads low and flushes: its GPS stays at rest through the
/// flush, whose bound restarts the time it has been on battery.
#[test]
fn a_low_battery_s_flush_keeps_the_gps_at_rest() {
    let (r, o, s) = (0, 1, 2);
    let config = |east: f64, seed: u64, external: bool| {
        let c = Config {
            position: pos(east),
            battery: Some(BatteryLevels::DESIGN),
            grace: GRACE,
            ..Config::default()
        };
        (Relay::new(c, seed), external)
    };
    let mut h = Harness::powered(vec![
        config(0.0, 1, true),
        config(300.0, 2, true),
        config(20.0, 3, false),
    ]);
    h.link(r, o);
    h.link(r, s);
    h.link(o, s);
    h.set_rssi(r, s, -80);
    h.set_rssi(o, s, -100);
    h.start_at(s, at(10));
    h.run_until(at(1000), |_| false);
    assert_eq!(
        h.nodes[s].core.anchor().map(|a| a.id),
        Some(h.nodes[r].core.id())
    );
    h.command(r, AppCommand::Power { external: false });
    h.command(
        r,
        AppCommand::Battery {
            millivolts: BatteryLevels::DESIGN.high_mv + 100,
        },
    );
    h.run_for(GRACE + secs(10));
    assert!(!h.nodes[r].core.sleepy() && !h.nodes[r].core.gps_tracks());
    // Out of reach, its sleepy node's message waits at the relay.
    h.unlink(r, s);
    let (dst_pos, dst_id) = (h.nodes[s].core.address(), h.nodes[s].core.id());
    let text = b"held when the battery goes low";
    h.command(
        o,
        AppCommand::Send {
            dst_pos,
            dst_id,
            payload: text,
        },
    );
    let id = MsgId::of(dst_pos, dst_id, text);
    h.run_for(secs(60));
    assert!(h.nodes[r].core.delivery_queue().any(|(m, _)| m == id));
    h.command(
        r,
        AppCommand::Battery {
            millivolts: BatteryLevels::DESIGN.low_mv,
        },
    );
    assert!(h.nodes[r].core.leaving());
    while !h.nodes[r].core.sleepy() {
        assert!(!h.nodes[r].core.gps_tracks());
        h.run_for(secs(1));
    }
}
