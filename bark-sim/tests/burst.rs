//! A sleepy node the button turns relay bursts (roadmap D246; spec
//! "Heartbeat", "Nodes"): it knows the relays in reach within the bound.
//! One turned by its power does not.

use bark_proto::node::{burst_gap, heartbeat_lag};
use bark_proto::radio::{time_on_air, MIN_PREAMBLE_SYMBOLS};
use bark_proto::wire::HEARTBEAT_LEN;
use bark_proto::{Config, Duration, Instant, CHANNELS};
use bark_sim::scenario::{RoleSpec, ScriptAction};
use bark_sim::{Scenario, Sim};

/// From the turn to the last answer to the burst: its first slot is a
/// burst gap after the turn and its 13th 12 more, it may go its lateness
/// late and is on the air its airtime, and an answer comes within a burst
/// gap of its end (spec "Heartbeat": burst gap, lateness): 16.11 s.
fn bound() -> Duration {
    let c = Config::default();
    let airtime = time_on_air(HEARTBEAT_LEN as u8, MIN_PREAMBLE_SYMBOLS);
    burst_gap(&c).mul(CHANNELS as u64 + 1) + heartbeat_lag(&c) + airtime
}

fn at(s: f64) -> Instant {
    Instant::from_micros((s * 1e6) as u64)
}

/// The node named `name`.
fn node(sim: &Sim, name: &str) -> u32 {
    let tail = format!("({name})");
    (0..sim.node_count() as u32)
        .find(|&i| sim.label(i).ends_with(&tail))
        .unwrap_or_else(|| panic!("no node {name}"))
}

fn knows(sim: &Sim, i: u32, j: u32) -> bool {
    let id = sim.node_id(j);
    sim.relay(i).neighbours().any(|n| n.id == id)
}

/// The answers each node sent after `from`, by the trace in `file`.
fn answers(sim: &Sim, file: &std::path::Path, i: u32, from: f64) -> usize {
    let line = format!("{}: Answer", sim.label(i));
    std::fs::read_to_string(file)
        .unwrap()
        .lines()
        .filter(|l| l.ends_with(&line))
        .filter(|l| l.split(' ').next().unwrap().parse::<f64>().unwrap() >= from)
        .count()
}

/// Six relays in a ring 300 m round a sleepy node, each in its reach and
/// none in another's; the node is anchored to the first. Its button is
/// held, or it is plugged in, at 1500 s.
fn ring(seed: u64, action: ScriptAction) -> Scenario {
    let mut sc = Scenario::new("ring-wake", seed, 1600.0).point("S", RoleSpec::Leaf, 0.0, 0.0);
    for k in 0..6 {
        let a = k as f64 * std::f64::consts::TAU / 6.0;
        let name = format!("R{k}");
        sc = sc
            .point(&name, RoleSpec::Relay, 300.0 * a.cos(), 300.0 * a.sin())
            .link("S", &name, if k == 0 { -85.0 } else { -100.0 });
    }
    sc.script(1500.0, "S", action, None)
}

/// The chance that one of two relays that cannot hear each other loses
/// its answer to the other's at the node: each answers at a random time
/// within the 200 ms backoff of hearing the same heartbeat, and the two
/// overlap when they start within an airtime, 14.4 ms, of each other:
/// 2a/T - (a/T)², 0.139.
fn overlap() -> f64 {
    let a = time_on_air(HEARTBEAT_LEN as u8, MIN_PREAMBLE_SYMBOLS).as_micros() as f64;
    let t = Config::default().lbt_backoff_max.as_micros() as f64;
    2.0 * a / t - (a / t).powi(2)
}

/// Over seeds 1 to 20, the node woken by the button knows within the
/// bound every relay whose home no other relay of the ring shares, and
/// each relay answers it once. Relays that share a home answer the same
/// heartbeat of the burst and may collide: of those, no more are missed
/// than twice the rate `overlap` gives. Without the burst it heard the
/// relays only as their rotations came to its channel: up to 195 s.
#[test]
fn the_button_wakes_a_node_that_knows_its_relays_within_the_bound() {
    let limit = bound().as_micros() as f64 / 1e6;
    let (mut missed, mut expected) = (0usize, 0.0);
    for seed in 1..=20 {
        let sc = ring(seed, ScriptAction::Hold);
        let file = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("ring-wake-{seed}.trace"));
        let mut sim = Sim::new(&sc).expect("builds");
        sim.trace_to(Box::new(std::fs::File::create(&file).unwrap()));
        let s = node(&sim, "S");
        let relays: Vec<u32> = (0..6).map(|k| node(&sim, &format!("R{k}"))).collect();
        sim.run_until(at(1499.0));
        assert!(sim.relay(s).sleepy());
        let anchor = sim.relay(s).anchor().map(|a| a.id);
        assert_eq!(anchor, Some(sim.node_id(relays[0])));
        let homes: Vec<_> = relays.iter().map(|&r| sim.relay(r).home()).collect();
        sim.run_until(at(1500.0 + limit));
        assert!(!sim.relay(s).sleepy());
        for (k, &r) in relays.iter().enumerate() {
            let sharing = homes.iter().filter(|&&h| h == homes[k]).count() - 1;
            if sharing == 0 {
                assert!(knows(&sim, s, r), "seed {seed}: {}", sim.label(r));
            } else if !knows(&sim, s, r) {
                missed += 1;
            }
            expected += 1.0 - (1.0 - overlap()).powi(sharing as i32);
        }
        sim.run();
        for &r in &relays {
            assert_eq!(
                answers(&sim, &file, r, 1500.0),
                1,
                "seed {seed}: {}",
                sim.label(r)
            );
        }
    }
    assert!(
        missed as f64 <= 2.0 * expected,
        "{missed} missed, {expected:.2} expected"
    );
}

/// Plugging in sends no burst: a node plugged in draws no answer.
#[test]
fn plugging_in_draws_no_answer() {
    for seed in 1..=3 {
        let sc = ring(seed, ScriptAction::Plug);
        let file = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("ring-plug-{seed}.trace"));
        let mut sim = Sim::new(&sc).expect("builds");
        sim.trace_to(Box::new(std::fs::File::create(&file).unwrap()));
        sim.run();
        let s = node(&sim, "S");
        assert!(!sim.relay(s).sleepy());
        for k in 0..6 {
            let r = node(&sim, &format!("R{k}"));
            assert_eq!(
                answers(&sim, &file, r, 1500.0),
                0,
                "seed {seed}: {}",
                sim.label(r)
            );
        }
    }
}
