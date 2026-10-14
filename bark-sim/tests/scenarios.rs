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

#[test]
fn s1_parses_and_runs() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../scenarios/s1-line.toml");
    let sc = Scenario::load(&path).expect("S1 parses");
    assert_eq!(sc.links.model, Model::Table);
    let mut sim = Sim::new(&sc).expect("S1 builds");
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
    sim.run();
    let s = sim.summary();
    print!("{}", s.table());
    assert_eq!(
        s.app.originated + s.app.unaddressed,
        60,
        "one text every 30 s for 30 minutes"
    );
    // Once the relays know each other, a worst-case rotation after their
    // boot listen (design §6.2, §7), every text with an address is
    // delivered until B is removed at 20 minutes; after that every one is
    // dropped at A for want of a route (design §9.2.5). Every message has
    // a fate, except one sent as the run ends.
    let c = bark_proto::Config::default();
    let settled = bark_proto::Instant::ZERO + c.boot_listen + c.heartbeat_max.mul(13);
    assert_eq!(
        settled.as_micros() as f64 / 1e6,
        sc.settle_s,
        "the scenario's settle time is one rotation after the boot listen"
    );
    let removal = bark_proto::Instant::ZERO + bark_proto::Duration::from_secs(1200);
    let fates: Vec<_> = sim.originated().filter(|(t, _)| *t < sim.end()).collect();
    assert!(
        fates.iter().all(|(_, f)| *f != Fate::Unaccounted),
        "{fates:?}"
    );
    let (before, after): (Vec<_>, Vec<_>) = fates.iter().partition(|(t, _)| *t < removal);
    let settled: Vec<_> = before.iter().filter(|(t, _)| *t >= settled).collect();
    assert!(
        settled.iter().all(|(_, f)| *f == Fate::Delivered),
        "{before:?}"
    );
    let a_no_route = Fate::Dropped {
        node: 0,
        reason: DropReason::NoRoute,
    };
    assert_eq!(after.len(), 20);
    assert!(after.iter().all(|(_, f)| *f == a_no_route), "{after:?}");
    // The summary splits the run the same way.
    let phases = &s.app.phases;
    let names: Vec<_> = phases.iter().map(|p| p.phase.as_str()).collect();
    assert_eq!(names, ["before settle", "settled", "after removal"]);
    let (on, off) = (&phases[1], &phases[2]);
    assert_eq!(on.originated as usize, settled.len());
    assert_eq!(
        (on.delivery_ratio, on.lost_at.len(), on.unaccounted),
        (1.0, 0, 0)
    );
    assert_eq!((off.originated, off.delivered, off.unaccounted), (20, 0, 0));
    assert_eq!(off.lost_at.get("relay 0 (A): dropped NoRoute"), Some(&20));
    assert_eq!(off.lost_at.len(), 1);
    // B was removed at 20 minutes and its relay core stopped.
    assert!(sim.resting(1).is_none());
    assert!(sim.resting(0).is_some() && sim.resting(2).is_some());
}
