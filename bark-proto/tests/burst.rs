//! The burst of a sleepy node that turns relay (roadmap D246; spec
//! "Heartbeat", "Sleepy nodes"): after the heartbeat at the change, 13
//! heartbeats a burst gap apart, one a channel, with the anchorless flag,
//! which every relay in reach answers once on the node's home channel.

mod common;

use bark_proto::node::{burst_gap, heartbeat_interval, heartbeat_lag};
use bark_proto::radio::{time_on_air, Channel, MIN_PREAMBLE_SYMBOLS};
use bark_proto::wire::{Packet, HEARTBEAT_LEN};
use bark_proto::{
    AppCommand, Config, Duration, Frame, Instant, NodeId, Position, Relay, Trace, CHANNELS,
};
use common::{at, ch, secs, Harness};
use std::collections::HashSet;

/// A position `east` metres from a point in Kyiv.
fn pos(east: f64) -> Position {
    let (lat, lon) = (50.45, 30.52);
    let m_per_deg = 6_371_000.0 * core::f64::consts::PI / 180.0;
    Position::from_degrees(lat, lon + east / (m_per_deg * lat.to_radians().cos()))
}

fn config(east: f64, home: Option<u8>) -> Config {
    Config {
        position: pos(east),
        home_override: home.map(ch),
        ..Config::default()
    }
}

/// From the turn to the last answer to the burst: its first slot is a
/// burst gap after the turn and its 13th 12 more, it may go its lateness
/// late and is on the air its airtime, and an answer comes within a burst
/// gap of its end (spec "Heartbeat": burst gap, lateness): 16.11 s.
fn bound() -> Duration {
    let c = Config::default();
    let airtime = time_on_air(HEARTBEAT_LEN as u8, MIN_PREAMBLE_SYMBOLS);
    burst_gap(&c).mul(CHANNELS as u64 + 1) + heartbeat_lag(&c) + airtime
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum By {
    Power,
    Button,
}

fn turn(h: &mut Harness<Relay>, i: usize, by: By) {
    match by {
        By::Power => h.command(i, AppCommand::Power { external: true }),
        By::Button => h.command(i, AppCommand::ForceRole),
    }
    assert!(!h.nodes[i].core.sleepy());
}

/// A heartbeat a node sent: when, on which channel, its sleepy,
/// anchorless and carried flags, its preamble.
#[derive(Clone, Copy, Debug)]
struct Sent {
    at: Instant,
    channel: Channel,
    anchorless: bool,
    sleepy: bool,
    carried: bool,
    preamble: u16,
}

fn heartbeats(h: &Harness<Relay>, i: usize, from: Instant) -> Vec<Sent> {
    h.nodes[i]
        .sent
        .iter()
        .filter(|(t, _)| *t >= from)
        .filter_map(
            |(t, f): &(Instant, Frame)| match Packet::decode(f.payload()) {
                Ok(Packet::Heartbeat(hb)) => Some(Sent {
                    at: *t,
                    channel: f.channel,
                    anchorless: hb.anchorless,
                    sleepy: hb.sleepy,
                    carried: hb.carried,
                    preamble: f.preamble_symbols,
                }),
                _ => None,
            },
        )
        .collect()
}

fn traced(h: &Harness<Relay>, i: usize, from: Instant, want: Trace) -> usize {
    h.nodes[i]
        .traces
        .iter()
        .filter(|(t, x)| *t >= from && *x == want)
        .count()
}

fn knows(h: &Harness<Relay>, i: usize, id: NodeId) -> bool {
    h.nodes[i].core.neighbours().any(|n| n.id == id)
}

/// The 13 heartbeats of the burst in `hbs`, a burst gap apart, one a
/// channel, each with the anchorless flag, the first a burst gap after
/// `turn`; the slots after, T apart, without it.
fn check_burst(hbs: &[Sent], turn: Instant, id: NodeId) -> Vec<Sent> {
    let c = Config::default();
    let (gap, late) = (burst_gap(&c), heartbeat_lag(&c));
    let burst: Vec<Sent> = hbs.iter().filter(|b| b.anchorless).copied().collect();
    assert_eq!(burst.len(), CHANNELS as usize, "{hbs:?}");
    assert!(burst.iter().all(|b| !b.sleepy));
    let channels: HashSet<_> = burst.iter().map(|b| b.channel).collect();
    assert_eq!(channels.len(), CHANNELS as usize, "one a channel");
    let first = burst[0].at.since(turn);
    assert!(
        first >= gap && first <= gap + late,
        "{first:?} to the first"
    );
    for w in burst.windows(2) {
        let d = w[1].at.since(w[0].at);
        assert!(d + late >= gap && d <= gap + late, "{d:?} apart");
    }
    let last = burst[CHANNELS as usize - 1].at;
    let after: Vec<_> = hbs.iter().filter(|b| b.at > last).collect();
    let t = heartbeat_interval(id, &c);
    let d = after[0].at.since(last);
    assert!(d + late >= t && d <= t + late, "{d:?} to the first slot");
    burst
}

/// Relays A, B and C on channels 2, 5 and 9, a sleepy node S anchored
/// to A that heard B and C last long enough ago to have forgotten them,
/// and `extra` nodes in reach of all of them.
fn anchored_with(seed: u64, extra: Vec<(Relay, bool)>) -> Harness<Relay> {
    let (a, b, c, s) = (0, 1, 2, 3);
    let mut cores = vec![
        (Relay::new(config(0.0, Some(2)), seed), true),
        (Relay::new(config(400.0, Some(5)), seed + 1), true),
        (Relay::new(config(-400.0, Some(9)), seed + 2), true),
        (Relay::new(config(20.0, None), seed + 3), false),
    ];
    let n = cores.len() + extra.len();
    cores.extend(extra);
    let mut h = Harness::powered(cores);
    for (x, y) in [(a, b), (a, c), (b, c), (s, a)] {
        h.link(x, y);
    }
    for e in 4..n {
        for o in 0..e {
            h.link(e, o);
        }
    }
    h.set_rssi(a, s, -70);
    h.run_until(at(2000), |_| false);
    let aid = h.nodes[a].core.id();
    assert_eq!(h.nodes[s].core.anchor().map(|x| x.id), Some(aid));
    h.link(s, b);
    h.link(s, c);
    h
}

fn anchored(seed: u64) -> Harness<Relay> {
    anchored_with(seed, Vec::new())
}

/// Walk node `r` off until it is carried.
fn carry(h: &mut Harness<Relay>, r: usize) {
    let mut east = 100.0;
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

/// A sleepy node the button turns relay: the heartbeat at the change on
/// its home channel, then the burst, and every relay in reach answers it
/// once on that channel, so that it knows them all within the bound.
#[test]
fn the_button_turns_a_sleepy_node_relay_with_a_burst() {
    for seed in [1, 11, 21] {
        let (a, b, c, s) = (0, 1, 2, 3);
        let mut h = anchored(seed);
        let ids: Vec<NodeId> = [a, b, c].map(|i| h.nodes[i].core.id()).into();
        assert!(!knows(&h, s, ids[1]) && !knows(&h, s, ids[2]));
        let home = h.nodes[s].core.home();
        let t0 = h.now;
        turn(&mut h, s, By::Button);
        let all = |h: &Harness<Relay>| ids.iter().all(|&id| knows(h, s, id));
        assert!(h.run_until(t0 + bound(), all), "seed {seed}");
        assert_eq!(h.nodes[s].core.relays_known(h.now), 3);
        h.run_until(t0 + secs(60), |_| false);
        let hbs = heartbeats(&h, s, t0);
        assert_eq!(hbs[0].channel, home, "the change first, at home");
        assert!(!hbs[0].anchorless && !hbs[0].sleepy);
        let sid = h.nodes[s].core.id();
        check_burst(&hbs, t0, sid);
        for r in [a, b, c] {
            assert_eq!(traced(&h, r, t0, Trace::Answer), 1, "relay {r}");
            let on_home = heartbeats(&h, r, t0)
                .into_iter()
                .any(|x| x.at < t0 + bound() && x.channel == home);
            assert!(on_home, "relay {r} answers on its home");
        }
    }
}

/// Plugged in, a sleepy node is a relay at once with no burst: its
/// heartbeats carry no anchorless flag and nobody answers.
#[test]
fn plugging_in_sends_no_burst() {
    let s = 3;
    let mut h = anchored(5);
    let t0 = h.now;
    turn(&mut h, s, By::Power);
    h.run_until(t0 + secs(120), |_| false);
    let hbs = heartbeats(&h, s, t0);
    assert!(hbs.len() >= 2 && hbs.iter().all(|x| !x.anchorless && !x.sleepy));
    for r in 0..4 {
        assert_eq!(traced(&h, r, t0, Trace::Answer), 0, "node {r}");
        assert_eq!(traced(&h, r, t0, Trace::Burst), 0, "node {r}");
    }
}

/// Switched on on battery and turned relay by the button while it still
/// listens for a relay, a node is a relay in its first rotation and sends
/// one whole burst, whenever the button comes; its first rotation ends a
/// rotation after its boot, as a relay's.
#[test]
fn the_button_bursts_in_the_first_rotation() {
    let rotation = Config::default().boot_listen;
    for after in [3, 30, 200] {
        let (a, b, s) = (0, 1, 2);
        let mut h = Harness::powered(vec![
            (Relay::new(config(0.0, Some(2)), 71), true),
            (Relay::new(config(400.0, Some(5)), 72), true),
            (Relay::new(config(20.0, None), 73), false),
        ]);
        h.link(a, b);
        h.start_at(s, at(1000));
        h.run_until(at(1000 + after), |_| false);
        assert!(
            h.nodes[s].core.sleepy() && h.nodes[s].core.booting(),
            "{after} s"
        );
        h.link(s, a);
        h.link(s, b);
        let t0 = h.now;
        turn(&mut h, s, By::Button);
        let ids = [a, b].map(|i| h.nodes[i].core.id());
        let all = |h: &Harness<Relay>| ids.iter().all(|&id| knows(h, s, id));
        assert!(h.run_until(t0 + bound(), all), "{after} s");
        h.run_until(at(1000) + rotation + secs(60), |_| false);
        assert!(!h.nodes[s].core.booting(), "{after} s: a relay's rotation");
        let sid = h.nodes[s].core.id();
        check_burst(&heartbeats(&h, s, t0), t0, sid);
        assert_eq!(
            heartbeats(&h, s, at(0))
                .iter()
                .filter(|x| x.anchorless)
                .count(),
            13
        );
    }
}

/// A sleepy node's beat under way when the button turns it, held by a
/// busy channel, goes as the heartbeat at the change, not as a heartbeat
/// of the burst, and the burst is whole after it.
#[test]
fn a_beat_under_way_goes_as_the_change() {
    let s = 3;
    let mut h = anchored(81);
    let home = h.nodes[s].core.home();
    h.jam(s, home);
    let checks = h.nodes[s].checks.len();
    assert!(h.run_until(h.now + secs(1200), |h| h.nodes[s].checks.len() > checks));
    let t0 = h.now;
    turn(&mut h, s, By::Button);
    h.unjam(s, home);
    h.run_until(t0 + secs(60), |_| false);
    let hbs = heartbeats(&h, s, t0);
    assert!(!hbs[0].anchorless && hbs[0].channel == home, "{hbs:?}");
    let sid = h.nodes[s].core.id();
    check_burst(&hbs, t0, sid);
}

/// A node that turns sleepy again in its burst ends it: no heartbeat with
/// the anchorless flag after it begins to leave.
#[test]
fn turning_sleepy_ends_the_burst() {
    let s = 3;
    let mut h = anchored(41);
    let t0 = h.now;
    turn(&mut h, s, By::Button);
    h.run_until(t0 + burst_gap(&Config::default()).mul(4), |_| false);
    h.command(s, AppCommand::ForceRole);
    let left = h.now;
    assert!(h.run_until(h.now + secs(60), |h| h.nodes[s].core.sleepy()));
    h.run_until(t0 + secs(120), |_| false);
    let hbs = heartbeats(&h, s, t0);
    let burst = hbs.iter().filter(|x| x.anchorless && !x.sleepy).count();
    assert!((2..CHANNELS as usize).contains(&burst), "{hbs:?}");
    assert!(hbs
        .iter()
        .filter(|x| x.at >= left)
        .all(|x| x.sleepy || !x.anchorless));
}

/// A carried relay answers no burst; a carried sleepy node the button
/// turns relay bursts as any other, its heartbeats with the carried flag.
#[test]
fn a_carried_relay_bursts_and_answers_none() {
    let (s, d, x) = (3, 4, 5);
    let mut h = anchored_with(
        51,
        vec![
            (Relay::new(config(100.0, Some(11)), 55), true),
            (Relay::new(config(-100.0, None), 56), true),
        ],
    );
    carry(&mut h, d);
    carry(&mut h, x);
    h.command(x, AppCommand::Power { external: false });
    assert!(h.nodes[x].core.sleepy(), "carried on battery");
    h.run_for(secs(600));
    assert!(!h.nodes[d].core.sleepy() && h.nodes[d].core.carried());
    let t0 = h.now;
    turn(&mut h, s, By::Button);
    h.run_until(t0 + secs(60), |_| false);
    assert_eq!(traced(&h, d, t0, Trace::Answer), 0, "carried");
    let sid = h.nodes[s].core.id();
    check_burst(&heartbeats(&h, s, t0), t0, sid);
    let t1 = h.now;
    turn(&mut h, x, By::Button);
    assert!(h.nodes[x].core.carried());
    h.run_until(t1 + secs(60), |_| false);
    let xid = h.nodes[x].core.id();
    let burst = check_burst(&heartbeats(&h, x, t1), t1, xid);
    assert!(burst.iter().all(|b| b.carried));
    assert_eq!(traced(&h, d, t1, Trace::Answer), 0);
}

/// A node in anchorless mode the button turns relay bursts with the
/// ordinary preamble, as a relay at boot, though its beats had the long
/// one.
#[test]
fn an_anchorless_node_turned_relay_bursts_with_the_ordinary_preamble() {
    let (a, s) = (0, 1);
    let mut h = Harness::powered(vec![
        (Relay::new(config(0.0, Some(4)), 61), true),
        (Relay::new(config(20.0, None), 62), false),
    ]);
    h.run_until(at(1000), |_| false);
    assert!(h.nodes[s].core.anchorless());
    let beats = heartbeats(&h, s, Instant::ZERO);
    assert!(!beats.is_empty() && beats.iter().all(|b| b.preamble > MIN_PREAMBLE_SYMBOLS));
    h.link(a, s);
    let t0 = h.now;
    turn(&mut h, s, By::Button);
    let aid = h.nodes[a].core.id();
    assert!(h.run_until(t0 + bound(), |h| knows(h, s, aid)));
    h.run_until(t0 + secs(60), |_| false);
    let hbs = heartbeats(&h, s, t0);
    let sid = h.nodes[s].core.id();
    check_burst(&hbs, t0, sid);
    assert!(hbs.iter().all(|b| b.preamble == MIN_PREAMBLE_SYMBOLS));
    assert_eq!(traced(&h, a, t0, Trace::Answer), 1);
}

/// Relays that burst at boot with a first rotation shorter than the burst
/// answer each other's bursts once each: an answer carries no anchorless
/// flag, which would be answered in turn.
#[test]
fn an_answer_is_answered_by_no_one() {
    let short = Config {
        boot_listen: secs(5),
        ..config(0.0, Some(3))
    };
    let mut h = Harness::new(vec![
        Relay::new(short, 91),
        Relay::new(
            Config {
                position: pos(300.0),
                ..short
            },
            92,
        ),
    ]);
    h.link(0, 1);
    h.run_until(at(120), |_| false);
    for i in [0, 1] {
        assert!(traced(&h, i, Instant::ZERO, Trace::Answer) <= 1, "node {i}");
        let flagged = heartbeats(&h, i, Instant::ZERO)
            .into_iter()
            .filter(|x| x.anchorless);
        assert_eq!(flagged.count(), CHANNELS as usize, "node {i}");
    }
}

/// A node switched on on battery traces no burst: its beats are a sleepy
/// node's.
#[test]
fn a_node_booted_on_battery_traces_no_burst() {
    let mut h = anchored(101);
    h.run_for(secs(1200));
    assert!(h.nodes[3].core.sleepy());
    assert!(!heartbeats(&h, 3, Instant::ZERO).is_empty());
    assert_eq!(traced(&h, 3, Instant::ZERO, Trace::Burst), 0);
}
