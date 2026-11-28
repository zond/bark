//! How a relay runs its GPS (spec "Position", a relay's GPS): all the time
//! on external power and through its grace on battery until it has stood
//! still a day, then resting and searching every half hour, every hour
//! after two days; past the grace on battery, and with a map point, it
//! rests; resting, it searches at boot and when the heartbeats of its fixed
//! relay neighbours change more than fading explains; a far fix keeps it on
//! until it is carried.

mod common;

use bark_proto::status::ReadingAge;
use bark_proto::{
    AppCommand, BatteryLevels, Config, Duration, Instant, MsgId, Position, Relay, Stored,
};
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
/// levels, so that a fixed one on battery relays on past its grace.
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
            position: pos(300.0),
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
                position: pos(300.0 + s as f64),
            },
        );
        assert!(h.nodes[0].core.gps_tracks() || h.nodes[0].core.carried());
    }
    // Carried past its grace on battery, it turns sleepy.
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
    // Out of reach of both, its sleepy node's message waits at the relay;
    // in O's reach, O would send it the message at once (spec "Next hop").
    h.unlink(r, s);
    h.unlink(o, s);
    let (dst_pos, dst_id) = (h.nodes[s].core.address(), h.nodes[s].core.id());
    let text = b"held when the battery goes low";
    h.command(
        o,
        AppCommand::Send {
            dst_pos,
            dst_id,
            payload: text,
            tag: 0,
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

/// The status tells the GPS's latest reading and how long ago it came,
/// a reading under the floor as any other, the GPS running or at rest
/// since; a restart forgets it (spec "Neighbour status").
#[test]
fn the_status_tells_the_latest_signal_and_its_age() {
    let mut h = row();
    let told = |h: &Harness<Relay>| {
        let s = h.nodes[0].core.status(h.now);
        (s.gps_cn0, s.gps_age)
    };
    assert_eq!(told(&h), (0, ReadingAge::NONE), "none yet");
    h.run_for(secs(10));
    assert!(h.nodes[0].core.gps_tracks());
    // A reading under the floor of 22 dB-Hz, whose fix the firmware
    // drops, is told all the same.
    h.command(0, AppCommand::Signal { cn0: 17 });
    assert_eq!(told(&h), (17, ReadingAge(0)));
    h.command(0, AppCommand::Signal { cn0: 0 });
    assert_eq!(told(&h), (0, ReadingAge(0)), "no satellite used");
    h.command(0, AppCommand::Signal { cn0: 35 });
    h.run_for(secs(59));
    assert_eq!(told(&h), (35, ReadingAge(0)));
    h.run_for(secs(1));
    assert_eq!(told(&h), (35, ReadingAge(1)));
    // On battery past its grace the GPS rests, and the reading stands.
    h.command(0, AppCommand::Power { external: false });
    h.run_for(GRACE + secs(60));
    assert!(!h.nodes[0].core.gps_tracks());
    let minutes = (GRACE.as_micros() / 60_000_000 + 2) as u32;
    assert_eq!(told(&h), (35, ReadingAge::from_minutes(minutes)));
    h.run_for(secs(40 * 60));
    assert_eq!(told(&h), (35, ReadingAge::from_minutes(minutes + 40)));
    let config = Config {
        position: pos(0.0),
        ..Config::default()
    };
    h.reboot(0, Relay::new(config, 9));
    assert_eq!(told(&h), (0, ReadingAge::NONE), "forgotten");
}

const HOUR: u64 = 3600;

/// One relay on USB at `pos(0.0)`, alone, with `config`'s changes.
fn alone(config: Config, seed: u64) -> Harness<Relay> {
    let config = Config {
        position: pos(0.0),
        ..config
    };
    Harness::powered(vec![(Relay::new(config, seed), true)])
}

/// The searches node 0 started from `from` until now, in hours after it.
fn searched_at(h: &Harness<Relay>, from: Instant) -> Vec<f64> {
    h.nodes[0]
        .located
        .iter()
        .filter(|(t, _)| *t >= from)
        .map(|(t, _)| t.since(from).as_micros() as f64 / 3.6e9)
        .collect()
}

/// `n` searches `every_min` apart, the first at `first_h`.
fn every(first_h: f64, every_min: f64, n: usize) -> Vec<f64> {
    (0..n)
        .map(|k| first_h + k as f64 * every_min / 60.0)
        .collect()
}

/// Whether two lists of hours agree to the minute.
fn near(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1.0 / 60.0)
}

/// On USB a placed relay's GPS runs for its first still day, then rests
/// and searches every half hour, and after two days every hour.
#[test]
fn after_a_still_day_it_rests_and_searches() {
    let mut h = alone(Config::default(), 1);
    h.run_until(at(24 * HOUR - 60), |_| false);
    assert!(h.nodes[0].core.gps_tracks());
    assert!(searched_at(&h, Instant::ZERO).is_empty());
    h.run_until(at(24 * HOUR + 60), |_| false);
    assert!(!h.nodes[0].core.gps_tracks());
    h.run_until(at(51 * HOUR - 60), |_| false);
    assert!(!h.nodes[0].core.gps_tracks());
    let mut want = every(24.5, 30.0, 48);
    want.extend(every(49.0, 60.0, 2));
    let got = searched_at(&h, Instant::ZERO);
    assert!(near(&got, &want), "{got:?}");
}

/// A relay with no place keeps its GPS running; its still day counts from
/// the fix that is its place.
#[test]
fn the_day_counts_from_its_first_fix() {
    let mut h = alone(
        Config {
            placed: false,
            ..Config::default()
        },
        2,
    );
    h.run_until(at(30 * HOUR), |_| false);
    assert!(h.nodes[0].core.gps_tracks());
    h.command(0, AppCommand::Fix { position: pos(0.0) });
    assert!(h.nodes[0].core.placed());
    h.run_until(at(54 * HOUR - 60), |_| false);
    assert!(h.nodes[0].core.gps_tracks());
    h.run_until(at(54 * HOUR + 60), |_| false);
    assert!(!h.nodes[0].core.gps_tracks());
}

/// A map point starts its still time over, from the fix that is its
/// place once the point is cleared.
#[test]
fn a_map_point_starts_the_day_over() {
    let mut h = alone(Config::default(), 3);
    h.run_until(at(25 * HOUR), |_| false);
    assert!(!h.nodes[0].core.gps_tracks());
    h.command(0, AppCommand::SetPosition { position: pos(0.0) });
    h.command(0, AppCommand::ClearMapPoint);
    assert!(h.nodes[0].core.gps_tracks());
    h.run_until(at(30 * HOUR), |_| false);
    assert!(h.nodes[0].core.gps_tracks());
    h.command(0, AppCommand::Fix { position: pos(5.0) });
    h.run_until(at(54 * HOUR - 60), |_| false);
    assert!(h.nodes[0].core.gps_tracks());
    h.run_until(at(54 * HOUR + 60), |_| false);
    assert!(!h.nodes[0].core.gps_tracks());
}

/// A far fix keeps the resting GPS on; carried, the relay's still time
/// starts over once it is fixed again.
#[test]
fn carried_its_day_starts_over() {
    let fixed_after = secs(600);
    let mut h = alone(
        Config {
            fixed_after,
            ..Config::default()
        },
        4,
    );
    h.run_until(at(25 * HOUR), |_| false);
    assert!(!h.nodes[0].core.gps_tracks());
    h.command(
        0,
        AppCommand::Fix {
            position: pos(300.0),
        },
    );
    assert!(h.nodes[0].core.gps_tracks());
    for s in 1..=72 {
        h.run_until(at(25 * HOUR + s), |_| false);
        h.command(
            0,
            AppCommand::Fix {
                position: pos(300.0 + s as f64),
            },
        );
    }
    assert!(h.nodes[0].core.carried() && h.nodes[0].core.gps_tracks());
    let mut t = 25 * HOUR + 72;
    while h.nodes[0].core.carried() {
        t += 1;
        h.run_until(at(t), |_| false);
        h.command(
            0,
            AppCommand::Fix {
                position: pos(400.0),
            },
        );
    }
    assert!(h.nodes[0].core.gps_tracks());
    h.run_until(at(t + 24 * HOUR - 60), |_| false);
    assert!(h.nodes[0].core.gps_tracks());
    h.run_until(at(t + 24 * HOUR + 60), |_| false);
    assert!(!h.nodes[0].core.gps_tracks());
}

/// On battery within its grace a still relay searches as on USB; past the
/// grace it searches only as before, at boot and at a sign that it moved.
#[test]
fn past_its_grace_on_battery_it_keeps_the_battery_rule() {
    let mut h = alone(
        Config {
            battery: Some(BatteryLevels::DESIGN),
            grace: secs(2 * HOUR),
            ..Config::default()
        },
        5,
    );
    h.run_until(at(30 * HOUR), |_| false);
    h.command(0, AppCommand::Power { external: false });
    h.command(
        0,
        AppCommand::Battery {
            millivolts: BatteryLevels::DESIGN.high_mv + 100,
        },
    );
    h.run_until(at(32 * HOUR - 60), |_| false);
    assert!(!h.nodes[0].core.gps_tracks());
    let within = searched_at(&h, at(30 * HOUR));
    assert!(near(&within, &every(0.0, 30.0, 4)), "{within:?}");
    h.run_until(at(40 * HOUR), |_| false);
    assert!(!h.nodes[0].core.sleepy() && !h.nodes[0].core.gps_tracks());
    assert_eq!(searched_at(&h, at(32 * HOUR)), Vec::<f64>::new());
}

/// Rebooted with `stored`, on USB.
fn reboot(h: &mut Harness<Relay>, stored: Stored, seed: u64) {
    let core = Relay::new(
        Config {
            position: pos(0.0),
            stored: Some(stored),
            ..Config::default()
        },
        seed,
    );
    h.reboot(0, core);
}

/// Its still time survives a restart in whole hours: at boot it rests and
/// searches, and a near fix leaves the count running on.
#[test]
fn its_still_time_survives_a_restart() {
    let mut h = alone(Config::default(), 6);
    h.run_until(at(30 * HOUR + 1800), |_| false);
    let stored = h.nodes[0].stored.expect("kept");
    assert_eq!(stored.still_hours, 30);
    let boot = h.now;
    reboot(&mut h, stored, 7);
    h.run_for(secs(1));
    assert!(!h.nodes[0].core.gps_tracks());
    assert_eq!(searched_at(&h, boot).len(), 1, "a search at boot");
    h.command(
        0,
        AppCommand::Fix {
            position: pos(10.0),
        },
    );
    assert!(!h.nodes[0].core.gps_tracks());
    // From 30 h of still time it searches at 30.5 h, and hourly past 48 h.
    h.run_until(boot + secs(21 * HOUR - 60), |_| false);
    let got = searched_at(&h, boot);
    let mut want = vec![0.0];
    want.extend(every(0.5, 30.0, 36));
    want.extend(every(19.0, 60.0, 2));
    assert!(near(&got, &want), "{got:?}");
}

/// A relay stored early in its day runs its GPS for what is left of it.
#[test]
fn a_restart_early_in_the_day_runs_the_rest_of_it() {
    let mut h = alone(Config::default(), 8);
    h.run_until(at(10 * HOUR + 1800), |_| false);
    let stored = h.nodes[0].stored.expect("kept");
    assert_eq!(stored.still_hours, 10);
    let boot = h.now;
    reboot(&mut h, stored, 9);
    h.run_for(secs(1));
    assert!(h.nodes[0].core.gps_tracks());
    h.run_until(boot + secs(14 * HOUR - 60), |_| false);
    assert!(h.nodes[0].core.gps_tracks());
    h.run_until(boot + secs(14 * HOUR + 60), |_| false);
    assert!(!h.nodes[0].core.gps_tracks());
}

/// A far first fix after a restart keeps the GPS on until a fix is near
/// again.
#[test]
fn a_far_fix_after_a_restart_keeps_it_on() {
    let mut h = alone(Config::default(), 10);
    h.run_until(at(30 * HOUR), |_| false);
    let stored = h.nodes[0].stored.expect("kept");
    let boot = h.now;
    reboot(&mut h, stored, 11);
    h.run_for(secs(1));
    assert!(!h.nodes[0].core.gps_tracks());
    h.command(
        0,
        AppCommand::Fix {
            position: pos(300.0),
        },
    );
    assert!(h.nodes[0].core.gps_tracks());
    h.run_until(boot + secs(30), |_| false);
    h.command(
        0,
        AppCommand::Fix {
            position: pos(20.0),
        },
    );
    assert!(!h.nodes[0].core.gps_tracks());
}

/// Sleepy, a node's still time waits; turned relay again it rests its
/// GPS at once, and searches for where it is now.
#[test]
fn a_sleepy_spell_pauses_its_still_time() {
    let mut h = alone(
        Config {
            battery: Some(BatteryLevels::DESIGN),
            ..Config::default()
        },
        12,
    );
    h.run_until(at(25 * HOUR), |_| false);
    let before = h.nodes[0].stored.expect("kept").still_hours;
    assert!(before >= 24);
    h.command(0, AppCommand::ForceRole);
    h.run_until(at(26 * HOUR), |h| h.nodes[0].core.sleepy());
    assert!(h.nodes[0].core.sleepy());
    h.run_until(at(40 * HOUR), |_| false);
    let turned = h.now;
    h.command(0, AppCommand::ForceRole);
    assert!(!h.nodes[0].core.sleepy());
    assert!(!h.nodes[0].core.gps_tracks());
    assert_eq!(searched_at(&h, turned).len(), 1);
    assert_eq!(h.nodes[0].stored.expect("kept").still_hours, before);
}
