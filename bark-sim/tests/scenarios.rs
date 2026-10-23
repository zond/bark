//! Whole-scenario tests: determinism, the capacity-sized run, and S1.

use bark_sim::links::{FadingParams, Model, Site};
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
            rooftop: false,
        })
        .place(Placement::Density {
            role: RoleSpec::Leaf,
            x0: 0.0,
            y0: 0.0,
            width_m: 2000.0,
            height_m: 2000.0,
            per_km2: 50.0,
            indoor: true,
            rooftop: false,
            relay_share: 0.0,
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
        from_s: 0.0,
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
    // The sleepy nodes' boot sweep would meet the test frames below; one
    // reading a day keeps them on their channel. A relay that heartbeats
    // once a day is heard by none of them in two minutes, so none takes an
    // anchor and beats.
    sc.protocol.sweep_interval_ms = Some(13 * 86_400_000);
    sc.protocol.heartbeat_min_ms = Some(86_400_000);
    sc.protocol.heartbeat_max_ms = Some(86_400_000);
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

/// No message to a destination reachable from its origin is lost without
/// a trace: each is delivered, reported lost where it died, or still held
/// by a core at the end (`Sim::messages`). A message to an unreachable
/// destination can end untraced when it has split in two (spec "Why the search finds a path"):
/// both copies search the same relays, and a relay whose search for one
/// copy has ended absorbs the other. It would be lost anyway, and is
/// only counted.
fn accounted(sim: &Sim, what: &str) {
    let lost: Vec<_> = sim
        .messages()
        .enumerate()
        .filter(|(_, (_, f, reachable))| *f == Fate::Unaccounted && *reachable)
        .map(|(tag, (t, _, _))| (tag, t))
        .collect();
    assert!(lost.is_empty(), "{what}: lost without a trace {lost:?}");
}

/// Load a scenario from `scenarios/`.
fn scenario(file: &str) -> Scenario {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../scenarios")
        .join(file);
    Scenario::load(&path).unwrap_or_else(|e| panic!("{file}: {e}"))
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
    // No leaf has attached, so no message has an address yet.
    assert!(s.app.originated + s.app.unaddressed > 100);
    assert!(
        s.channels[6].busy_fraction.unwrap() > 0.0,
        "the interferer shows"
    );
    // Different seed, different run.
    assert_ne!(run(&busy_block(43)).0, j1);
}

#[test]
fn capacity_scenario_runs_a_minute_quickly() {
    // S4 (`scenarios/s4-capacity.toml`): spec "Capacity and sizing"'s collision domain,
    // 28 km² with 269 relays, a fifth of them on roofs, and 20 000 leaves.
    let mut sc = scenario("s4-capacity.toml");
    sc.duration_s = 60.0;
    // Every node switched on at once, so that the minute holds all of them.
    sc.boot = Default::default();
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
    assert_eq!((s.relays, s.leaves), (269, 20_000));
    let roofs = (0..s.relays)
        .filter(|&r| sim.site(r) == Site::Rooftop)
        .count();
    assert_eq!(roofs, 53);
    // Two rooftops 3 km apart hear each other at sensitivity on average
    // (spec "Physical layer", "Capacity and sizing"); a windowsill pair that far apart does not.
    let p = &sc.links;
    let at_3km = 10.0 - p.path_loss_db(3000.0);
    let sens = sc.radio.sensitivity_dbm();
    assert!((at_3km - p.site_loss_db(Site::Rooftop, Site::Rooftop) - sens).abs() < 0.1);
    assert!(at_3km - p.site_loss_db(Site::Outdoor, Site::Rooftop) < sens - 20.0);
    // In the first minute few leaves have attached, and texts to the
    // others have no address yet (spec "Channel at boot").
    let texts = s.app.originated + s.app.unaddressed;
    assert!(texts > 200, "{texts} texts in a minute");
    // "A few seconds" on an optimised build, and the minute is all boot
    // listen, in which each of the 20 000 sleepy nodes sweeps thirteen
    // channels a second; unoptimised test builds are several times
    // slower, so the bound is looser there.
    let limit = if cfg!(debug_assertions) { 90.0 } else { 20.0 };
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
        "the scenario's settle time is one rotation after the first"
    );
    for seed in S1_SEEDS {
        let mut sc = sc.clone();
        sc.seed = seed;
        s1_seed(&sc, settled);
    }
}

/// One run of S1. Once the relays know each other, a worst-case rotation
/// after their first (spec "Channel at boot", "Heartbeat"), every text with an address
/// is delivered until B is removed at 20 minutes; after that every one is
/// dropped at A for want of a route (spec "Return to parent"), or, while A
/// waits out the ban on B, refused at LA and given back to its application
/// with that wait (spec "A sleepy node's own message"). The
/// sleepy node LA hands its texts to A, then, as the origin of their
/// search (`docs/one-node-type-2026-10-16.md`); the application skips the
/// texts due while LA holds one (roadmap D34). Every message has a fate,
/// except one sent as the run ends.
fn s1_seed(sc: &Scenario, settled: bark_proto::Instant) {
    let seed = sc.seed;
    let mut sim = Sim::new(sc).expect("S1 builds");
    sim.run();
    let s = sim.summary();
    let skipped = s.app.offers.skipped;
    assert_eq!(
        s.app.originated + s.app.unaddressed + skipped,
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
    // Texts skipped while LA held one fall before the removal or after it.
    let after_n = after.len() as u64;
    assert_eq!(after_n, 20 - sim.skipped_since(removal), "seed {seed}");
    let given_back = |f: &Fate| {
        matches!(
            f,
            Fate::Dropped {
                reason: DropReason::Pushback,
                ..
            }
        )
    };
    assert!(
        after.iter().all(|(_, f)| *f == a_no_route || given_back(f)),
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
        (after_n, 0, 0),
        "seed {seed}"
    );
    let back = after.iter().filter(|(_, f)| given_back(f)).count() as u64;
    let no_route = off.lost_at.get("relay 0 (A): dropped NoRoute").copied();
    assert_eq!(no_route.unwrap_or(0) + back, after_n, "seed {seed}");
    assert_eq!(off.lost_at.len(), 1 + usize::from(back > 0), "seed {seed}");
    // B was removed at 20 minutes and its relay core stopped.
    assert!(sim.resting(1).is_none());
    assert!(sim.resting(0).is_some() && sim.resting(2).is_some());
}

/// S1b: A and C, which cannot hear each other, hand to B at the same
/// instant every 30 s, so every first hand-over collides at B. The retry
/// wait (spec "Retry wait", roadmap D22) pulls them apart: over 100 seeds the
/// settled phase delivers 0.961 on average, worst seed 0.900, against
/// 0.136 with the old 0–50 ms over 20. What is lost is a message whose
/// three attempts all collided. B demoted by A or C after two such
/// failures is still tried, as their last neighbour (roadmap D25); before
/// that, a seed where it happened lost a whole demotion's worth of
/// messages (worst seed 0.633).
#[test]
fn s1b_hidden_terminals_come_apart() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../scenarios/s1b-hidden.toml");
    let mut sc = Scenario::load(&path).expect("S1b parses");
    // Its sleepy nodes take what is held for them at once, so that what
    // is measured is the hand-over at B, not the wait for a beat at the end
    // of the run.
    sc.protocol.sleepy_listens = Some(true);
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
        assert!(settled.delivery_ratio >= 0.85, "seed {seed}: {}", s.table());
        accounted(&sim, &format!("S1b seed {seed}"));
        ratios.push(settled.delivery_ratio);
        let b = &s.nodes.as_ref().expect("per-node metrics")[1];
        assert!(
            b.m.rx.lost_capture + b.m.rx.sinr_fail > 0,
            "seed {seed}: no collisions at B; the scenario no longer tests hidden terminals"
        );
    }
    let mean = ratios.iter().sum::<f64>() / ratios.len() as f64;
    assert!(mean >= 0.93, "{ratios:?}");
}

/// S2, the block (docs/simulator-design.md §6): ten windowsill relays and
/// eighty indoor leaves over a square kilometre for an hour. A smoke test
/// on one seed: it runs, delivers most texts, and the relays neither
/// oscillate between channels nor approach their duty cycle.
///
/// Over seeds 1 to 10 the settled phase delivers 0.86, worst seed 0.70
/// (seed 1: 0.78), and the 80 leaves change relay 70 times an hour; before
/// a leaf kept its relay through a fade (roadmap D23), 0.79 and 174. The
/// bounds leave a margin below seed 1's 0.78 and 65. Indoors at the
/// design's relay density half the leaves hear their relay less than
/// 12 dB above sensitivity; a leaf still changes relay about once an
/// hour, after keepalives fail in two periods in a row, and texts already
/// queued at the old relay expire there.
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
    assert!(phase.delivery_ratio >= 0.7, "{}", s.table());
    assert!(s.relay_changes <= 100, "{}", s.table());
    assert_eq!((s.app.duplicates, s.app.misdelivered), (0, 0));
    accounted(&sim, "S2");
    let nodes = s.nodes.as_ref().expect("per-node metrics");
    for n in nodes.iter().filter(|n| n.label.starts_with("relay")) {
        // The choice at the end of the first rotation and no move after it
        // (spec "Channel at boot", "Channel moves").
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

/// The corridor (`scenarios/corridor.toml`): two flows meet head on at
/// every relay of a line of five. Seeds 1 to 3 deliver at least 0.93 of
/// the settled phase on `main` as of roadmap D28; the bound leaves room
/// for the spread between seeds.
///
/// Its sleepy nodes take what is held for them at once here
/// (`sleepy_listens`), so that what is measured is the corridor, not the
/// wait for a beat at the end of the run.
#[test]
fn corridor_carries_crossing_flows() {
    let mut sc = scenario("corridor.toml");
    sc.protocol.sleepy_listens = Some(true);
    for seed in 1..=3 {
        let mut sc = sc.clone();
        sc.seed = seed;
        let mut sim = Sim::new(&sc).expect("builds");
        sim.run();
        let s = sim.summary();
        let settled = &s.app.phases[1];
        assert_eq!(settled.phase, "settled");
        assert_eq!(settled.unreachable, 0, "the corridor is connected");
        assert!(settled.delivery_ratio >= 0.9, "seed {seed}: {}", s.table());
        assert_eq!((s.app.duplicates, s.app.misdelivered), (0, 0));
        accounted(&sim, &format!("corridor seed {seed}"));
    }
}

/// The ring (`scenarios/ring.toml`): eight relays, each leaf sending to
/// the leaf three relays on, every 2 s. The relays run at their duty cap
/// with messages waiting for each other round the ring, and none is
/// dropped for want of room (roadmap D31): seeds 1 to 40 deliver 0.966 to
/// 0.986 of the settled phase (roadmap D75), where returning or dropping
/// what found every neighbour busy delivered 0.49 to 0.66. The bound
/// leaves room for the spread between seeds (`docs/ring-jam-2026-10-18.md`).
///
/// Its sleepy nodes take what is held for them at once here
/// (`sleepy_listens`): on the beat a node can take only what its holder's
/// delivery queue held at the beat, 41 messages, against the 150 a beat
/// period of this flow brings (`docs/one-node-type-2026-10-16.md`).
#[test]
fn ring_carries_flows_that_wait_for_each_other() {
    let mut sc = scenario("ring.toml");
    sc.protocol.sleepy_listens = Some(true);
    for seed in 1..=3 {
        let mut sc = sc.clone();
        sc.seed = seed;
        let mut sim = Sim::new(&sc).expect("builds");
        sim.run();
        let s = sim.summary();
        let settled = &s.app.phases[1];
        assert_eq!(settled.phase, "settled");
        assert_eq!(settled.unreachable, 0, "the ring is connected");
        assert!(settled.delivery_ratio >= 0.96, "seed {seed}: {}", s.table());
        assert_eq!((s.app.duplicates, s.app.misdelivered), (0, 0));
        accounted(&sim, &format!("ring seed {seed}"));
    }
}

/// The random mesh (`scenarios/mesh-burst.toml`, `mesh-spread.toml`):
/// every leaf sends across the mesh, all at once or each at its own time,
/// and in the spread run the middle relay restarts. Seed 5 leaves a relay
/// that nothing reaches; its messages count as unreachable and the rest
/// are delivered.
#[test]
fn mesh_routes_bursts_and_a_restart() {
    for file in ["mesh-burst.toml", "mesh-spread.toml"] {
        let sc = scenario(file);
        for seed in [1, 5] {
            let mut sc = sc.clone();
            sc.seed = seed;
            let mut sim = Sim::new(&sc).expect("builds");
            sim.run();
            let s = sim.summary();
            let settled = &s.app.phases[1];
            assert_eq!(settled.phase, "settled");
            assert!(
                settled.originated >= 75,
                "{file} seed {seed}: {}",
                s.table()
            );
            assert_eq!(settled.unreachable > 0, seed == 5, "{file} seed {seed}");
            assert!(
                settled.reachable_delivery_ratio >= 0.9,
                "{file} seed {seed}: {}",
                s.table()
            );
            assert_eq!((s.app.duplicates, s.app.misdelivered), (0, 0));
            accounted(&sim, &format!("{file} seed {seed}"));
        }
    }
}

/// A restarted relay keeps its position, loses its neighbours and boots
/// again, on the home channel it stored (spec "Channel at boot",
/// "Implementation notes"), and its neighbours are heard again within its
/// first rotation (spec "Heartbeat").
#[test]
fn a_restarted_relay_boots_again_with_its_identity() {
    use bark_proto::{Duration, Instant};
    let sc = Scenario::new("restart", 1, 1200.0)
        .point("A", RoleSpec::Relay, 0.0, 0.0)
        .point("B", RoleSpec::Relay, 400.0, 0.0)
        .link("A", "B", -100.0);
    let mut sim = Sim::new(&sc).unwrap();
    let c = bark_proto::Config::default();
    let settled = Instant::ZERO + c.boot_listen + c.heartbeat_max.mul(13);
    sim.run_until(settled);
    let position = sim.relay(0).config().position;
    assert!(!sim.relay(0).booting());
    assert_eq!(sim.relay(0).neighbours().count(), 1);
    let home = sim.relay(0).home();
    sim.restart(0);
    assert!(sim.relay(0).booting());
    assert_eq!(sim.relay(0).home(), home);
    assert_eq!(sim.relay(0).neighbours().count(), 0);
    assert_eq!(sim.relay(0).config().position, position);
    sim.run_until(settled + c.boot_listen + c.heartbeat_max.mul(13) + Duration::from_secs(1));
    assert!(!sim.relay(0).booting());
    assert_eq!(sim.relay(0).neighbours().count(), 1);
    assert_eq!(sim.resting(0), Some(home));
}

/// Staggered switch-on: each node comes on within its kind's window and
/// is deaf and silent until then, and the run settles the settle time
/// after the last switch-on.
#[test]
fn nodes_switch_on_within_their_windows() {
    use bark_proto::Instant;
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../scenarios/s2-block.toml");
    let mut sc = Scenario::load(&path).expect("S2 parses");
    sc.boot.relays_s = 300.0;
    sc.boot.sleepy_s = 900.0;
    sc.duration_s = 1200.0;
    assert_eq!(sc.settled_s(), sc.settle_s + 900.0);
    let mut sim = Sim::new(&sc).unwrap();
    let at = |s: f64| Instant::from_micros((s * 1e6) as u64);
    sim.run_until(at(150.0));
    let relays = sim.relays();
    let on = |sim: &Sim, from: u32, to: u32| {
        (from..to).filter(|&n| sim.switched_on(n).is_some()).count()
    };
    let n = sim.node_count() as u32;
    let (r, l) = (on(&sim, 0, relays), on(&sim, relays, n));
    assert!(r > 0 && r < relays as usize, "{r} relays on at 150 s");
    assert!(
        l > 0 && l < (n - relays) as usize / 2,
        "{l} sleepy nodes on at 150 s"
    );
    for k in 0..n {
        if sim.switched_on(k).is_none() {
            assert!(sim.resting(k).is_none(), "node {k} listens before it is on");
        }
    }
    sim.run_until(at(1200.0));
    for k in 0..n {
        let t = sim.switched_on(k).expect("all on by the end");
        let window = if k < relays { 300.0 } else { 900.0 };
        assert!(t <= at(window), "node {k} on at {t:?}");
    }
}

/// Clocks off by up to 50 ppm (spec "Heartbeat"): a sleeping node still
/// wakes in time for every heartbeat of its anchor (spec "Relay choice"),
/// and never checks its anchor for want of one.
#[test]
fn drifting_clocks_keep_the_anchor_watch() {
    for seed in 1..=4 {
        let mut sc = Scenario::new("drift", seed, 3.0 * 3600.0)
            .point("R", RoleSpec::Relay, 0.0, 0.0)
            .point("L0", RoleSpec::Leaf, 50.0, 0.0)
            .point("L1", RoleSpec::Leaf, 100.0, 0.0)
            .point("L2", RoleSpec::Leaf, 150.0, 0.0)
            .link("R", "L0", -90.0)
            .link("R", "L1", -90.0)
            .link("R", "L2", -90.0);
        sc.clocks.drift_ppm = 50.0;
        sc.settle_s = 900.0;
        let mut sim = Sim::new(&sc).unwrap();
        sim.run();
        let s = &sim.metrics.sleep;
        assert!(
            s.watches[0] > 3 * 60,
            "seed {seed}: {} watches",
            s.watches[0]
        );
        assert_eq!(s.watches[1], 0, "seed {seed}: heartbeats missed");
        for n in 1..4 {
            let checks = sim.metrics.nodes[n].core.anchor_checks;
            assert_eq!(checks, 0, "seed {seed}: node {n} checked its anchor");
        }
    }
}

/// S2 with every sleepy node walking, stops of ten minutes: walkers move
/// far enough from where they chose to check their anchor
/// (spec "Relay choice"), change anchors, and their texts still arrive or
/// are accounted for.
#[test]
fn walkers_check_and_change_their_anchor() {
    let mut sc = scenario("s2-block.toml");
    sc.movement.fraction = 1.0;
    sc.movement.stop_s = 600.0;
    sc.duration_s = 2.0 * 3600.0;
    let mut sim = Sim::new(&sc).expect("S2 builds");
    sim.run();
    let s = sim.summary();
    let z = s.sleep.as_ref().expect("sleepy nodes");
    assert!(z.m.walks[1] > 80, "{} walks", z.m.walks[1]);
    assert!(
        z.m.fix_checks > 10,
        "{} checks after a move",
        z.m.fix_checks
    );
    assert!(
        z.m.anchor_changes > 20,
        "{} anchor changes",
        z.m.anchor_changes
    );
    let phase = &s.app.phases[1];
    assert!(phase.delivery_ratio >= 0.6, "{}", s.table());
    accounted(&sim, "S2 walking");
}

/// The strip (`scenarios/strip.toml`): three S4 domains side by side and a
/// centre with twice the people in the middle one. Only placed here; a
/// run takes three to four times an S4 run's memory.
#[test]
fn strip_places_three_domains_and_a_centre() {
    let sc = scenario("strip.toml");
    let n = sc.placed().expect("places");
    let relays = n.iter().filter(|p| p.role == RoleSpec::Relay).count();
    assert_eq!(relays, 3 * 269);
    let centre = |p: &&&bark_sim::scenario::PlacedNode| {
        (6637.25..9237.25).contains(&p.xy.x) && (1345.75..3945.75).contains(&p.xy.y)
    };
    let leaves: Vec<_> = n.iter().filter(|p| p.role == RoleSpec::Leaf).collect();
    assert_eq!(leaves.len(), 3 * 20_000 + 4829);
    let inside = leaves.iter().filter(centre).count() as f64;
    // Twice the density over 6.76 km²: about 9 657, give or take its root.
    assert!((inside - 9657.0).abs() < 300.0, "{inside} in the centre");
    assert_eq!(sc.settled_s(), 2700.0);
}

/// S2 with a third of the sleepy nodes walking, on battery while they
/// walk and on USB at their stops, and a third of the others switching
/// at random (`[power]`, roadmap D71): nodes turn sleepy after their grace
/// and relays at once, some power comes back within the grace, and texts
/// still arrive or are accounted for.
#[test]
fn nodes_switch_with_their_power() {
    let mut sc = scenario("s2-block.toml");
    sc.movement.fraction = 0.3;
    sc.movement.stop_s = 900.0;
    sc.power.walkers = true;
    sc.power.fraction = 0.3;
    sc.power.usb_s = 900.0;
    sc.power.battery_s = 600.0;
    sc.protocol.grace_ms = Some(300_000);
    sc.duration_s = 2.0 * 3600.0;
    let mut sim = Sim::new(&sc).expect("S2 builds");
    sim.run();
    let s = sim.summary();
    let p = s.research.power.as_ref().expect("nodes switch");
    let to_sleepy = p.turns.iter().filter(|t| t.2).count();
    let to_relay = p.turns.len() - to_sleepy;
    assert!(p.switchers.len() > 30, "{} switch", p.switchers.len());
    assert!(to_sleepy > 20 && to_relay > 20, "{to_sleepy} / {to_relay}");
    assert!(p.back[0] > 5, "{:?} back in the grace", p.back);
    assert!(p.graces > 10, "{} graces", p.graces);
    let phase = &s.app.phases[1];
    assert!(phase.delivery_ratio >= 0.6, "{}", s.table());
    accounted(&sim, "S2 switching");
}
