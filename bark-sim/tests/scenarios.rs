//! Whole-scenario tests: determinism, the capacity-sized run, and S1.

use bark_sim::links::{FadingParams, Model};
use bark_sim::metrics::DropReason;
use bark_sim::scenario::{
    Availability, Destination, InterfererSpec, Placement, RoleSpec, ScriptAction, TrafficSpec,
};
use bark_sim::sim::Fate;
use bark_sim::{Scenario, Sim};
use std::time::Instant as Wall;

/// A city block with every moving part switched on: generated nodes,
/// shadowing, indoor loss, fading, interferers, Poisson traffic, dark
/// leaves, scripted test signals, observers and a time series.
fn busy_block(seed: u64) -> Scenario {
    let mut sc = Scenario::new("busy-block", seed, 120.0)
        .place(Placement::Lattice {
            role: RoleSpec::Relay,
            x0: 0.0,
            y0: 0.0,
            width_m: 2000.0,
            height_m: 2000.0,
            spacing_m: 500.0,
            jitter_m: 50.0,
            indoor: false,
        })
        .place(Placement::Density {
            role: RoleSpec::Leaf,
            x0: 0.0,
            y0: 0.0,
            width_m: 2000.0,
            height_m: 2000.0,
            per_km2: 50.0,
            indoor: true,
        });
    sc.links.exponent = 3.5;
    sc.links.indoor_loss_db = 10.0;
    sc.links.shadowing_sigma_db = 6.0;
    sc.links.fading = FadingParams {
        sigma_db: 4.0,
        correlation_time_s: 30.0,
        step_s: 5.0,
    };
    sc.interferers.push(InterfererSpec {
        x: 1000.0,
        y: 1000.0,
        channel: 6,
        power_dbm: 10.0,
        lora: false,
        mean_on_s: 2.0,
        mean_off_s: 5.0,
    });
    sc.traffic.push(TrafficSpec::Poisson {
        per_leaf_per_hour: 60.0,
        payload_min: 20,
        payload_max: 120,
        destination: Destination::Within { radius_m: 800.0 },
    });
    sc.availability = Some(Availability {
        mean_available_s: 60.0,
        mean_dark_s: 20.0,
    });
    sc.metrics.observers = vec!["relay:0".into(), "relay:12".into()];
    sc.metrics.cadence_s = Some(10.0);
    // Leaves stand in for traffic the stub cores do not generate: each
    // listens on a channel and transmits test frames.
    for k in 0..300u32 {
        let leaf = format!("leaf:{}", k % 200);
        let at = 0.5 + k as f64 * 0.37;
        let ch = (k % 13) as u8;
        if k < 200 {
            sc = sc.script(0.0, &leaf, ScriptAction::Listen, Some(ch));
        }
        sc = sc.script_tx(at, &leaf, ch, 20 + (k % 100) as u8, 12);
        if k % 7 == 0 {
            sc = sc.script(
                at + 0.2,
                &format!("leaf:{}", (k + 3) % 200),
                ScriptAction::Rssi,
                Some(ch),
            );
        }
    }
    sc.events.push(bark_sim::scenario::ScriptedEvent {
        at_s: 60.0,
        node: "relay:5".into(),
        action: ScriptAction::Remove,
        channel: None,
        len: None,
        preamble_symbols: None,
    });
    sc
}

fn run(sc: &Scenario) -> (String, String) {
    let mut sim = Sim::new(sc).expect("builds");
    sim.run();
    (sim.summary().json(), sim.metrics.csv())
}

#[test]
fn same_seed_gives_bit_identical_output() {
    let sc = busy_block(42);
    let (j1, c1) = run(&sc);
    let (j2, c2) = run(&sc);
    assert_eq!(j1, j2);
    assert_eq!(c1, c2);
    // And the run did something worth comparing.
    let mut sim = Sim::new(&sc).unwrap();
    sim.run();
    let s = sim.summary();
    assert!(s.rx.candidates > 100, "{}", s.table());
    assert!(
        s.rx.delivered > 0 && s.rx.deaf.total() + s.rx.lost_capture + s.rx.sinr_fail > 0,
        "{}",
        s.table()
    );
    // Two minutes is inside the relays' boot listen (design §6.2): no leaf
    // has attached, so no message has an address yet.
    assert!(s.app.originated + s.app.unaddressed > 100);
    assert!(
        s.channels[6].busy_fraction.unwrap() > 0.0,
        "the interferer shows"
    );
    // Different seed, different run.
    assert_ne!(run(&busy_block(43)).0, j1);
}

#[test]
fn capacity_sized_scenario_runs_a_minute_quickly() {
    // Design §13 collision domain: 28 km², 270 relays, 20 000 leaves, texts
    // at ten per leaf per day.
    let side = 28e6f64.sqrt();
    let area_km2 = side * side / 1e6;
    let mut sc = Scenario::new("capacity", 7, 60.0)
        .place(Placement::Density {
            role: RoleSpec::Relay,
            x0: 0.0,
            y0: 0.0,
            width_m: side,
            height_m: side,
            per_km2: 270.0 / area_km2,
            indoor: false,
        })
        .place(Placement::Density {
            role: RoleSpec::Leaf,
            x0: 0.0,
            y0: 0.0,
            width_m: side,
            height_m: side,
            per_km2: 20_000.0 / area_km2,
            indoor: true,
        });
    sc.links.exponent = 3.5;
    sc.links.indoor_loss_db = 15.0;
    sc.links.shadowing_sigma_db = 6.0;
    sc.links.fading = FadingParams {
        sigma_db: 4.0,
        correlation_time_s: 60.0,
        step_s: 10.0,
    };
    sc.traffic.push(TrafficSpec::Poisson {
        per_leaf_per_hour: 10.0 / 24.0,
        payload_min: 20,
        payload_max: 140,
        destination: Destination::Uniform,
    });
    sc.interferers.push(InterfererSpec {
        x: side / 2.0,
        y: side / 2.0,
        channel: 6,
        power_dbm: 10.0,
        lora: false,
        mean_on_s: 1.0,
        mean_off_s: 4.0,
    });
    sc.metrics.observers = vec!["relay:0".into(), "relay:100".into(), "relay:200".into()];
    sc.metrics.per_node = false;
    let t0 = Wall::now();
    let mut sim = Sim::new(&sc).expect("builds");
    let built = t0.elapsed();
    sim.run();
    let total = t0.elapsed();
    let s = sim.summary();
    println!(
        "capacity scenario: {} relays, {} leaves, {} links ({} pairs, cut-off {:.0} m), {} events; \
         build {:.2} s, run {:.2} s, total {:.2} s",
        s.relays,
        s.leaves,
        s.links.links,
        s.links.pairs,
        s.links.cutoff_m,
        s.events,
        built.as_secs_f64(),
        (total - built).as_secs_f64(),
        total.as_secs_f64()
    );
    assert_eq!((s.relays, s.leaves), (270, 20_000));
    // Within the relays' boot listen (design §6.2) no leaf has attached,
    // so the texts have no address yet.
    let texts = s.app.originated + s.app.unaddressed;
    assert!(texts > 50, "{texts} texts in a minute");
    // "A few seconds" on an optimised build; unoptimised test builds are
    // several times slower, so the bound is looser there.
    let limit = if cfg!(debug_assertions) { 30.0 } else { 5.0 };
    assert!(
        total.as_secs_f64() < limit,
        "took {:.2} s",
        total.as_secs_f64()
    );
}

/// S1 seeds run by the test: the first ten, and the four that lost
/// messages at B before the owner rule ignored the leaf table (roadmap
/// D21).
const S1_SEEDS: [u64; 14] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 27, 32, 40];

#[test]
fn s1_parses_and_runs() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../scenarios/s1-line.toml");
    let sc = Scenario::load(&path).expect("S1 parses");
    assert_eq!(sc.links.model, Model::Table);
    let sim = Sim::new(&sc).expect("S1 builds");
    // A and C cannot hear each other.
    let p = sc.radio.sensitivity_dbm();
    let (a, c) = (0, 2);
    let ac = sim
        .links()
        .get(a, c)
        .map(|l| l.base_dbm as f64)
        .unwrap_or(f64::NEG_INFINITY);
    assert!(ac < p, "A hears C at {ac} dBm");
    assert!(
        sim.links()
            .get(a, 1)
            .is_some_and(|l| l.base_dbm as f64 > p + 10.0),
        "A hears B"
    );
    let c = bark_proto::Config::default();
    let settled = bark_proto::Instant::ZERO + c.boot_listen + c.heartbeat_max.mul(13);
    assert_eq!(
        settled.as_micros() as f64 / 1e6,
        sc.settle_s,
        "the scenario's settle time is one rotation after the boot listen"
    );
    for seed in S1_SEEDS {
        let mut sc = sc.clone();
        sc.seed = seed;
        s1_seed(&sc, settled);
    }
}

/// One run of S1. Once the relays know each other, a worst-case rotation
/// after their boot listen (design §6.2, §7), every text with an address
/// is delivered until B is removed at 20 minutes; after that every one is
/// dropped at A for want of a route (design §9.2.5). Every message has a
/// fate, except one sent as the run ends.
fn s1_seed(sc: &Scenario, settled: bark_proto::Instant) {
    let seed = sc.seed;
    let mut sim = Sim::new(sc).expect("S1 builds");
    sim.run();
    let s = sim.summary();
    assert_eq!(
        s.app.originated + s.app.unaddressed,
        60,
        "seed {seed}: one text every 30 s for 30 minutes"
    );
    let removal = bark_proto::Instant::ZERO + bark_proto::Duration::from_secs(1200);
    let fates: Vec<_> = sim.originated().filter(|(t, _)| *t < sim.end()).collect();
    assert!(
        fates.iter().all(|(_, f)| *f != Fate::Unaccounted),
        "seed {seed}: {fates:?}"
    );
    let (before, after): (Vec<_>, Vec<_>) = fates.iter().partition(|(t, _)| *t < removal);
    let on: Vec<_> = before.iter().filter(|(t, _)| *t >= settled).collect();
    assert!(
        on.iter().all(|(_, f)| *f == Fate::Delivered),
        "seed {seed}: {before:?}\n{}",
        s.table()
    );
    let a_no_route = Fate::Dropped {
        node: 0,
        reason: DropReason::NoRoute,
    };
    assert_eq!(after.len(), 20, "seed {seed}");
    assert!(
        after.iter().all(|(_, f)| *f == a_no_route),
        "seed {seed}: {after:?}"
    );
    // The summary splits the run the same way.
    let phases = &s.app.phases;
    let names: Vec<_> = phases.iter().map(|p| p.phase.as_str()).collect();
    assert_eq!(names, ["before settle", "settled", "after removal"]);
    let (mid, off) = (&phases[1], &phases[2]);
    assert_eq!(mid.originated as usize, on.len(), "seed {seed}");
    assert_eq!(
        (mid.delivery_ratio, mid.lost_at.len(), mid.unaccounted),
        (1.0, 0, 0),
        "seed {seed}"
    );
    assert_eq!(
        (off.originated, off.delivered, off.unaccounted),
        (20, 0, 0),
        "seed {seed}"
    );
    assert_eq!(off.lost_at.get("relay 0 (A): dropped NoRoute"), Some(&20));
    assert_eq!(off.lost_at.len(), 1, "seed {seed}");
    // B was removed at 20 minutes and its relay core stopped.
    assert!(sim.resting(1).is_none());
    assert!(sim.resting(0).is_some() && sim.resting(2).is_some());
}

/// S1b: A and C, which cannot hear each other, hand to B at the same
/// instant every 30 s, so every first hand-over collides at B. The retry
/// wait (protocol §9.3, roadmap D22) pulls them apart: over 100 seeds the
/// settled phase delivers 0.945 on average, against 0.136 with the old
/// 0–50 ms over 20. What is still lost is mostly B demoted by A or C after
/// two failed hand-overs, with no other route: a seed where that happens
/// loses a whole demotion's worth of messages, so the test bounds the
/// mean over ten seeds rather than each.
#[test]
fn s1b_hidden_terminals_come_apart() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../scenarios/s1b-hidden.toml");
    let sc = Scenario::load(&path).expect("S1b parses");
    let seeds = 1..=10;
    let mut ratios = Vec::new();
    for seed in seeds {
        let mut sc = sc.clone();
        sc.seed = seed;
        let mut sim = Sim::new(&sc).expect("S1b builds");
        sim.run();
        let s = sim.summary();
        let settled = &s.app.phases[1];
        assert_eq!(settled.phase, "settled");
        ratios.push(settled.delivery_ratio);
        let b = &s.nodes.as_ref().expect("per-node metrics")[1];
        assert!(
            b.m.rx.lost_capture + b.m.rx.sinr_fail > 0,
            "seed {seed}: no collisions at B; the scenario no longer tests hidden terminals"
        );
    }
    let mean = ratios.iter().sum::<f64>() / ratios.len() as f64;
    assert!(mean >= 0.9, "{ratios:?}");
}

/// S2, the block (docs/simulator-design.md §6): ten windowsill relays and
/// eighty indoor leaves over a square kilometre for an hour. A smoke test
/// on one seed: it runs, delivers most texts, and the relays neither
/// oscillate between channels nor approach their duty cycle.
///
/// The delivery bound is loose on purpose. Over seeds 1 to 10 the settled
/// phase delivers 0.86 (seed 1: 0.78), 0.81 before a leaf kept its relay
/// through a fade (roadmap D23). Indoors at the design's relay density
/// half the leaves hear their relay less than 12 dB above sensitivity;
/// a leaf still changes relay about once an hour, after keepalives fail
/// in two periods in a row, and texts already queued at the old relay
/// expire there.
#[test]
fn s2_block_smoke() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../scenarios/s2-block.toml");
    let sc = Scenario::load(&path).expect("S2 parses");
    let c = bark_proto::Config::default();
    let settled = (c.boot_listen + c.heartbeat_max.mul(13)).as_micros() as f64 / 1e6;
    assert_eq!(settled, sc.settle_s, "settle time derived as in S1");
    let mut sim = Sim::new(&sc).expect("S2 builds");
    sim.run();
    let s = sim.summary();
    assert_eq!((s.relays, s.leaves), (10, 80));
    let phase = &s.app.phases[1];
    assert_eq!(phase.phase, "settled");
    assert!(phase.originated >= 15, "{}", s.table());
    assert!(phase.delivery_ratio >= 0.6, "{}", s.table());
    assert_eq!((s.app.duplicates, s.app.misdelivered), (0, 0));
    let nodes = s.nodes.as_ref().expect("per-node metrics");
    for n in nodes.iter().filter(|n| n.label.starts_with("relay")) {
        // The choice at the end of the boot listen and no move after it
        // (protocol §6.2, §6.3).
        assert!(
            n.m.home_moves <= 1,
            "{}: {} home moves",
            n.label,
            n.m.home_moves
        );
        assert!(
            n.airtime_fraction < 0.02,
            "{}: airtime {:.3}",
            n.label,
            n.airtime_fraction
        );
    }
}

/// The event trace (`--trace FILE`) records a message from end to end and
/// does not change the run.
#[test]
fn trace_follows_a_message_and_changes_nothing() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../scenarios/s1-line.toml");
    let sc = Scenario::load(&path).expect("S1 parses");
    let (plain, _) = run(&sc);
    let file = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("s1.trace");
    let mut sim = Sim::new(&sc).expect("S1 builds");
    sim.trace_to(Box::new(std::fs::File::create(&file).unwrap()));
    sim.run();
    assert_eq!(sim.summary().json(), plain);
    let trace = std::fs::read_to_string(&file).unwrap();
    let lines: Vec<_> = trace.lines().filter(|l| l.contains("#20")).collect();
    for step in [
        "leaf 3 (LA): app send #20",
        "relay 0 (A): forwarded #20 to relay 1 (B)",
        "relay 2 (C): queued #20 for leaf 4 (LC)",
        "leaf 4 (LC): app message #20",
    ] {
        assert!(
            lines.iter().any(|l| l.contains(step)),
            "{step:?} not in\n{}",
            lines.join("\n")
        );
    }
}
