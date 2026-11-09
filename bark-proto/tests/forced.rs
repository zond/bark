//! A role a person flips with the board's button (spec "Nodes",
//! `relay/power.rs`, roadmap D160): it holds until the external power
//! changes, then the power's rules decide again; a forced relay on
//! battery still turns sleepy at its low battery level.

mod common;

use bark_proto::{AppCommand, BatteryLevels, Config, Forced, Position, Relay, SleepyNode, Trace};
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

fn config(east: f64, battery: Option<BatteryLevels>) -> Config {
    Config {
        position: pos(east),
        grace: secs(GRACE),
        battery,
        ..Config::default()
    }
}

fn power(h: &mut Harness<Relay>, i: usize, external: bool) {
    h.command(i, AppCommand::Power { external });
}

fn force(h: &mut Harness<Relay>, i: usize) {
    h.command(i, AppCommand::ForceRole);
}

fn battery(h: &mut Harness<Relay>, i: usize, millivolts: u16) {
    h.command(i, AppCommand::Battery { millivolts });
}

fn relaying(h: &Harness<Relay>, i: usize) -> bool {
    !h.nodes[i].core.sleepy() && !h.nodes[i].core.leaving()
}

/// Two relays on USB in reach of each other, settled; node 0 with
/// `battery` levels.
fn pair(seed: u64, battery: Option<BatteryLevels>) -> Harness<Relay> {
    let mut h = Harness::powered(vec![
        (Relay::new(config(0.0, battery), seed), true),
        (Relay::new(config(300.0, Some(LEVELS)), seed + 1), true),
    ]);
    h.link(0, 1);
    h.run_until(common::at(600), |_| false);
    h
}

/// Walk node `r` off until it is carried.
fn carry(h: &mut Harness<Relay>, r: usize) {
    let mut east = 300.0;
    while !h.nodes[r].core.carried() {
        h.command(
            r,
            AppCommand::Fix {
                position: pos(east),
            },
        );
        east += 10.0;
        h.run_for(secs(10));
    }
}

/// A relay on USB forced sleepy leaves, turns sleepy and stays sleepy on
/// USB for as long as the cable stays; unplugged, the force is over, and
/// plugged in again it is a relay at once.
#[test]
fn a_forced_sleepy_node_on_usb_stays_sleepy_until_the_power_changes() {
    let (r, o) = (0, 1);
    let mut h = pair(11, Some(LEVELS));
    force(&mut h, r);
    assert_eq!(h.nodes[r].core.forced(), Some(Forced::Sleepy));
    assert!(h.run_until(h.now + secs(60), |h| h.nodes[r].core.sleepy()));
    let oid = h.nodes[o].core.id();
    h.run_for(secs(3 * GRACE));
    assert!(h.nodes[r].core.sleepy(), "on USB, forced");
    assert_eq!(h.nodes[r].core.anchor().map(|a| a.id), Some(oid));
    power(&mut h, r, false);
    assert_eq!(h.nodes[r].core.forced(), None, "unplugged");
    h.run_for(secs(60));
    assert!(h.nodes[r].core.sleepy(), "on battery, by its power");
    power(&mut h, r, true);
    assert!(relaying(&h, r), "plugged in");
}

/// The same, the power changing the other way: plugged in, a node forced
/// sleepy on battery is a relay.
#[test]
fn a_forced_sleepy_node_on_battery_relays_when_plugged_in() {
    let r = 0;
    let mut h = pair(21, Some(LEVELS));
    power(&mut h, r, false);
    battery(&mut h, r, LEVELS.high_mv + 300);
    h.run_for(secs(60));
    assert!(relaying(&h, r), "a fixed relay on battery relays (D81)");
    force(&mut h, r);
    assert!(h.run_until(h.now + secs(60), |h| h.nodes[r].core.sleepy()));
    battery(&mut h, r, LEVELS.high_mv + 300);
    h.run_for(secs(3 * GRACE));
    assert!(h.nodes[r].core.sleepy(), "not by its battery");
    power(&mut h, r, true);
    assert_eq!(h.nodes[r].core.forced(), None);
    assert!(relaying(&h, r));
}

/// A sleepy node on battery forced relay relays on past its grace, and
/// carried; plugged in the force is over, and unplugged again its grace
/// decides as before.
#[test]
fn a_forced_relay_on_battery_stays_a_relay_until_the_power_changes() {
    let r = 0;
    let mut h = pair(31, None);
    power(&mut h, r, false);
    assert!(h.run_until(h.now + secs(GRACE + 60), |h| h.nodes[r].core.sleepy()));
    force(&mut h, r);
    assert_eq!(h.nodes[r].core.forced(), Some(Forced::Relay));
    assert!(relaying(&h, r), "at once");
    h.run_for(secs(3 * GRACE));
    assert!(relaying(&h, r), "past its grace");
    carry(&mut h, r);
    h.run_for(secs(60));
    assert!(relaying(&h, r), "carried");
    power(&mut h, r, true);
    assert_eq!(h.nodes[r].core.forced(), None, "plugged in");
    power(&mut h, r, false);
    assert!(
        h.run_until(h.now + secs(60), |h| h.nodes[r].core.sleepy()),
        "carried on battery, by the power's rules (D152)"
    );
}

/// A forced relay on battery leaves at its low battery level, and the
/// force ends there; on battery at the low level a force to relay is not
/// taken.
#[test]
fn a_low_battery_turns_a_forced_relay_sleepy() {
    let r = 0;
    let mut h = pair(41, Some(LEVELS));
    power(&mut h, r, false);
    battery(&mut h, r, LEVELS.high_mv + 300);
    carry(&mut h, r);
    assert!(h.run_until(h.now + secs(60), |h| h.nodes[r].core.sleepy()));
    force(&mut h, r);
    assert_eq!(h.nodes[r].core.forced(), Some(Forced::Relay));
    assert!(relaying(&h, r));
    battery(&mut h, r, LEVELS.low_mv);
    assert!(h.run_until(h.now + secs(60), |h| h.nodes[r].core.sleepy()));
    assert_eq!(h.nodes[r].core.forced(), None);
    let low = h.nodes[r]
        .traces
        .iter()
        .filter(|(_, t)| *t == Trace::BatteryLow)
        .count();
    assert_eq!(low, 1);
    force(&mut h, r);
    assert_eq!(h.nodes[r].core.forced(), None, "not taken at the low level");
    assert!(h.nodes[r].core.sleepy());
}

/// Each flip turns the role, and the node stays forced; a relay flipped
/// back within its flush relays on, with what it flushed still queued.
#[test]
fn each_flip_turns_the_role_and_keeps_it_forced() {
    let r = 0;
    let mut h = pair(51, Some(LEVELS));
    force(&mut h, r);
    assert_eq!(h.nodes[r].core.forced(), Some(Forced::Sleepy));
    force(&mut h, r);
    assert_eq!(h.nodes[r].core.forced(), Some(Forced::Relay));
    assert!(relaying(&h, r), "back from its flush");
    force(&mut h, r);
    assert!(h.run_until(h.now + secs(60), |h| h.nodes[r].core.sleepy()));
    force(&mut h, r);
    assert_eq!(h.nodes[r].core.forced(), Some(Forced::Relay));
    assert!(relaying(&h, r));
    let forced: Vec<bool> = h.nodes[r]
        .traces
        .iter()
        .filter_map(|(_, t)| match t {
            Trace::Forced { sleepy } => Some(*sleepy),
            _ => None,
        })
        .collect();
    assert_eq!(forced, [true, false, true, false]);
}

/// A relay that leaves is a relay still: a flip keeps it turning sleepy,
/// now forced.
#[test]
fn a_leaving_relay_flips_to_sleepy() {
    let r = 0;
    let mut h = pair(57, Some(LEVELS));
    power(&mut h, r, false);
    battery(&mut h, r, LEVELS.high_mv + 300);
    carry(&mut h, r);
    assert!(h.nodes[r].core.leaving() || h.nodes[r].core.sleepy());
    let leaving = h.nodes[r].core.leaving();
    force(&mut h, r);
    let want = if leaving {
        Forced::Sleepy
    } else {
        Forced::Relay
    };
    assert_eq!(h.nodes[r].core.forced(), Some(want));
}

/// A node with no room for a relay's mail is never forced to relay.
#[test]
fn a_sleepy_node_without_room_is_not_forced() {
    let mut h = Harness::powered(vec![(SleepyNode::new(config(0.0, Some(LEVELS)), 61), true)]);
    h.run_until(common::at(600), |_| false);
    h.command(0, AppCommand::ForceRole);
    assert_eq!(h.nodes[0].core.forced(), None);
    assert!(h.nodes[0].core.sleepy());
}
