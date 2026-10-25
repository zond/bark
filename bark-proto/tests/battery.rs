//! A fixed relay on battery relays until its battery is low (spec
//! "Nodes", `relay/power.rs`): the low level turns it sleepy, the high
//! level or external power makes it a relay again, a carried relay keeps
//! its grace, and a fixed relay that reboots on battery relays on.

mod common;

use bark_proto::{AppCommand, BatteryLevels, Config, Position, Relay, Trace};
use common::{secs, Harness};

/// A position `east` metres from a point in Kyiv.
fn pos(east: f64) -> Position {
    let (lat, lon) = (50.45, 30.52);
    let m_per_deg = 6_371_000.0 * core::f64::consts::PI / 180.0;
    Position::from_degrees(lat, lon + east / (m_per_deg * lat.to_radians().cos()))
}

/// The grace of these tests, shorter than the design's to keep them quick.
const GRACE: u64 = 600;

const LEVELS: BatteryLevels = BatteryLevels::DESIGN;

fn config(east: f64) -> Config {
    Config {
        position: pos(east),
        grace: secs(GRACE),
        battery: Some(LEVELS),
        ..Config::default()
    }
}

fn node(east: f64, external: bool, seed: u64) -> (Relay, bool) {
    (Relay::new(config(east), seed), external)
}

fn power(h: &mut Harness<Relay>, i: usize, external: bool) {
    h.command(i, AppCommand::Power { external });
}

fn battery(h: &mut Harness<Relay>, i: usize, millivolts: u16) {
    h.command(i, AppCommand::Battery { millivolts });
}

fn traced(h: &Harness<Relay>, i: usize, want: Trace) -> usize {
    h.nodes[i].traces.iter().filter(|(_, t)| *t == want).count()
}

/// Two relays in reach of each other, settled.
fn pair(seed: u64) -> Harness<Relay> {
    let mut h = Harness::powered(vec![node(0.0, true, seed), node(300.0, true, seed + 1)]);
    h.link(0, 1);
    h.run_until(common::at(600), |_| false);
    h
}

/// Unplugged, a fixed relay relays past its grace while its battery is
/// above the low level; at the low level it flushes and turns sleepy,
/// saying so at once; a reading between the levels keeps it sleepy, and
/// one at the high level makes it a relay again.
#[test]
fn a_fixed_relay_on_battery_relays_until_its_battery_is_low() {
    let (r, o) = (0, 1);
    let mut h = pair(11);
    power(&mut h, r, false);
    for _ in 0..3 {
        battery(&mut h, r, LEVELS.low_mv + 100);
        h.run_for(secs(GRACE));
    }
    assert!(!h.nodes[r].core.sleepy() && !h.nodes[r].core.leaving());
    assert_eq!(traced(&h, r, Trace::GraceEnded), 0);
    battery(&mut h, r, LEVELS.low_mv);
    assert!(h.run_until(h.now + secs(60), |h| h.nodes[r].core.sleepy()));
    assert_eq!(traced(&h, r, Trace::BatteryLow), 1);
    h.run_for(secs(2));
    assert_eq!(traced(&h, r, Trace::Changed), 1, "its change, at once");
    let oid = h.nodes[o].core.id();
    assert_eq!(h.nodes[r].core.anchor().map(|a| a.id), Some(oid));
    battery(&mut h, r, LEVELS.high_mv - 1);
    h.run_for(secs(60));
    assert!(h.nodes[r].core.sleepy(), "between the levels");
    battery(&mut h, r, LEVELS.high_mv);
    h.run_for(secs(1));
    assert!(!h.nodes[r].core.sleepy(), "at the high level");
    assert_eq!(h.nodes[r].core.address(), pos(0.0));
}

/// A relay its battery turned sleepy is a relay again on external power,
/// and its low reading is forgotten: unplugged again it relays on.
#[test]
fn external_power_clears_a_low_battery() {
    let r = 0;
    let mut h = pair(21);
    power(&mut h, r, false);
    battery(&mut h, r, LEVELS.low_mv - 50);
    assert!(h.run_until(h.now + secs(60), |h| h.nodes[r].core.sleepy()));
    power(&mut h, r, true);
    assert!(!h.nodes[r].core.sleepy());
    power(&mut h, r, false);
    h.run_for(secs(3 * GRACE));
    assert!(!h.nodes[r].core.sleepy() && !h.nodes[r].core.leaving());
}

/// On external power a reading changes nothing: it is the charger's.
#[test]
fn a_reading_on_external_power_is_not_taken() {
    let r = 0;
    let mut h = pair(31);
    battery(&mut h, r, LEVELS.low_mv - 50);
    power(&mut h, r, false);
    h.run_for(secs(2 * GRACE));
    assert!(!h.nodes[r].core.sleepy() && !h.nodes[r].core.leaving());
}

/// A carried relay on battery keeps its grace whatever its battery: its
/// owner's node is not to run flat relaying in a pocket. One carried
/// after its grace is over leaves at once.
#[test]
fn a_carried_relay_keeps_its_grace() {
    let r = 0;
    let mut h = pair(41);
    power(&mut h, r, false);
    battery(&mut h, r, LEVELS.high_mv + 300);
    h.run_for(secs(GRACE + 60));
    assert!(!h.nodes[r].core.sleepy());
    for k in 0..=8 {
        let east = 100.0 + 10.0 * k as f64;
        h.command(
            r,
            AppCommand::Fix {
                position: pos(east),
            },
        );
        h.run_for(secs(10));
    }
    assert!(h.nodes[r].core.carried());
    assert!(h.run_until(h.now + secs(60), |h| h.nodes[r].core.sleepy()));
    assert_eq!(traced(&h, r, Trace::GraceEnded), 1);
    assert_eq!(traced(&h, r, Trace::BatteryLow), 0);
}

/// Reboot node `r` on battery with `stored`, as its driver would.
#[inline(never)]
fn reboot_on_battery(h: &mut Harness<Relay>, r: usize, stored: bark_proto::Stored, seed: u64) {
    let core = Relay::new(
        Config {
            stored: Some(stored),
            ..config(0.0)
        },
        seed,
    );
    h.nodes[r].power = Some(false);
    h.reboot(r, core);
}

/// A fixed relay that reboots on battery relays on, from what it stored;
/// one whose record says it was sleepy boots sleepy.
#[test]
fn a_fixed_relay_rebooted_on_battery_relays_on() {
    let (r, o) = (0, 1);
    let mut h = pair(51);
    power(&mut h, r, false);
    h.run_for(secs(60));
    let stored = h.nodes[r].stored.expect("kept");
    assert!(stored.relay);
    reboot_on_battery(&mut h, r, stored, 52);
    h.run_for(secs(2 * GRACE));
    assert!(!h.nodes[r].core.sleepy() && !h.nodes[r].core.leaving());
    let sleepy = bark_proto::Stored {
        relay: false,
        ..stored
    };
    reboot_on_battery(&mut h, r, sleepy, 53);
    h.run_for(secs(400));
    assert!(h.nodes[r].core.sleepy());
    let oid = h.nodes[o].core.id();
    assert_eq!(h.nodes[r].core.anchor().map(|a| a.id), Some(oid));
}

/// Without battery levels a fixed relay turns sleepy at its grace's end,
/// whatever its battery reads.
#[test]
fn without_levels_the_grace_alone_decides() {
    let r = 0;
    let mut h = Harness::powered(vec![
        (
            Relay::new(
                Config {
                    battery: None,
                    ..config(0.0)
                },
                61,
            ),
            true,
        ),
        node(300.0, true, 62),
    ]);
    h.link(0, 1);
    h.run_until(common::at(600), |_| false);
    power(&mut h, r, false);
    battery(&mut h, r, LEVELS.high_mv + 300);
    assert!(h.run_until(h.now + secs(GRACE + 60), |h| h.nodes[r].core.sleepy()));
    assert_eq!(traced(&h, r, Trace::GraceEnded), 1);
}
