//! Radio-model tests, driven by scripted driver-level actions so they hold
//! whatever the protocol cores do (the cores here are the Phase 0 stubs;
//! leaves are used throughout because a stub leaf never touches its radio).

use bark_proto::radio::{time_on_air, MIN_PREAMBLE_SYMBOLS, SYMBOL_TIME};
use bark_proto::Instant;
use bark_sim::metrics::RxCounts;
use bark_sim::phy::RadioParams;
use bark_sim::scenario::{RoleSpec::Leaf, ScriptAction};
use bark_sim::sim::ScriptResult;
use bark_sim::{Scenario, Sim};

/// Two transmitters T1 (node 0) and T2 (node 1) heard by receiver R (node
/// 2) at the given powers; R listens on channel 3 from t = 0. T1 and T2 do
/// not hear each other.
fn trio(t1_dbm: f64, t2_dbm: f64) -> Scenario {
    Scenario::new("trio", 1, 2.0)
        .point("T1", Leaf, 0.0, 0.0)
        .point("T2", Leaf, 10.0, 0.0)
        .point("R", Leaf, 5.0, 0.0)
        .link("T1", "R", t1_dbm)
        .link("T2", "R", t2_dbm)
        .script(0.0, "R", ScriptAction::Listen, Some(3))
}

fn run(sc: &Scenario) -> Sim {
    let mut sim = Sim::new(sc).expect("scenario builds");
    sim.run();
    sim
}

fn rx(sim: &Sim, node: usize) -> RxCounts {
    let r = sim.metrics.nodes[node].rx;
    assert_eq!(
        r.candidates,
        r.decided(),
        "every candidate reception has exactly one outcome: {r:?}"
    );
    r
}

#[test]
fn single_packet_airtime_matches_datasheet_formula() {
    for (len, preamble) in [(8u8, 12u16), (40, 12), (255, 12), (100, 2000), (0, 12)] {
        let sc = trio(-100.0, -100.0).script_tx(1.0, "T1", 3, len, preamble);
        let sim = run(&sc);
        // SX1261/2 datasheet §6.1.4, SF5, explicit header, CRC on, CR 4/5:
        // preamble + 6.25 + 8 + ceil(max(8·len + 16 − 20 + 8, 0) / 20) · 5 symbols.
        let bits = (8 * len as i64 + 4).max(0);
        let quarter_symbols = 4 * preamble as u64 + 25 + 32 + 4 * ((bits + 19) / 20 * 5) as u64;
        let expect_us = quarter_symbols * 256 / 4;
        assert_eq!(time_on_air(len, preamble).as_micros(), expect_us);
        let m = &sim.metrics.nodes[0];
        assert_eq!(
            (m.tx_frames, m.airtime_us),
            (1, expect_us),
            "len {len}, preamble {preamble}"
        );
        // The excursion: RX→TX plus a synthesiser hop (T1 was not listening
        // on channel 3), airtime, STBY_RC→RX.
        let p = RadioParams::default();
        let done = 1_000_000 + p.rx_to_tx_us + p.synth_hop_us + expect_us + p.stby_to_rx_us;
        assert_eq!(
            sim.script_log,
            vec![ScriptResult::TxDone {
                node: 0,
                at: Instant::from_micros(done)
            }]
        );
        // R heard it.
        assert_eq!(rx(&sim, 2).delivered, 1);
    }
}

#[test]
fn capture_holds_at_exactly_the_threshold_and_fails_just_below() {
    // T1 locks first; T2 arrives 0.5 ms later, inside T1's preamble but
    // weaker. Capture threshold 5 dB.
    for (t2, t1_survives) in [(-105.0, true), (-104.9, false), (-110.0, true)] {
        let sc = trio(-100.0, t2)
            .script_tx(1.0, "T1", 3, 40, 12)
            .script_tx(1.0005, "T2", 3, 40, 12);
        let sim = run(&sc);
        let r = rx(&sim, 2);
        assert_eq!(r.candidates, 2);
        assert_eq!(r.lost_capture, 1, "T2 never gets the lock (T2 at {t2})");
        if t1_survives {
            assert_eq!((r.delivered, r.sinr_fail), (1, 0), "T2 at {t2}");
        } else {
            assert_eq!((r.delivered, r.sinr_fail), (0, 1), "T2 at {t2}");
            assert_eq!(r.sinr_fail_adjacent, 0);
        }
    }
}

#[test]
fn stronger_preamble_steals_the_lock_at_exactly_the_threshold() {
    // T1 is weaker and first; T2 arrives during T1's preamble.
    for (t2, steals) in [(-100.0, true), (-100.1, false)] {
        let sc = trio(-105.0, t2)
            .script_tx(1.0, "T1", 3, 40, 12)
            .script_tx(1.0005, "T2", 3, 40, 12);
        let sim = run(&sc);
        let r = rx(&sim, 2);
        if steals {
            // T1 lost to capture, T2 delivered: exactly 5 dB both ways.
            assert_eq!(
                (r.delivered, r.lost_capture, r.sinr_fail),
                (1, 1, 0),
                "T2 at {t2}"
            );
        } else {
            // T2 cannot steal; it is lost to capture and kills T1 (4.9 dB).
            assert_eq!(
                (r.delivered, r.lost_capture, r.sinr_fail),
                (0, 1, 1),
                "T2 at {t2}"
            );
        }
    }
    // After T1's preamble no steal is possible: T2 at +10 dB arriving 4 ms
    // later (preamble is 12 × 256 µs = 3.07 ms) is lost, and T1 with it.
    assert!(SYMBOL_TIME.mul(MIN_PREAMBLE_SYMBOLS as u64).as_micros() < 4000);
    let sc = trio(-105.0, -95.0)
        .script_tx(1.0, "T1", 3, 40, 12)
        .script_tx(1.004, "T2", 3, 40, 12);
    let r = rx(&run(&sc), 2);
    assert_eq!((r.delivered, r.lost_capture, r.sinr_fail), (0, 1, 1));
}

#[test]
fn receiver_mid_excursion_misses_packet_and_says_why() {
    let p = RadioParams::default();
    // R's excursion starts at 1.0 s. T1 is scripted at `tx_at`; its signal
    // reaches the air rx_to_tx + hop = 122 µs later.
    let lead = (p.rx_to_tx_us + p.synth_hop_us) as f64 / 1e6;
    let cases: [(ScriptAction, f64, &str); 5] = [
        // 40 µs into a CAD on another channel: still retuning.
        (ScriptAction::Cad, 1.0 + 40e-6 - lead, "retune"),
        // 1 ms into the CAD: sampling.
        (ScriptAction::Cad, 1.001 - lead, "cad"),
        // 200 µs into an off-channel RSSI: dwelling.
        (
            ScriptAction::Rssi,
            1.0 + (86.0 + 200.0) * 1e-6 - lead,
            "rssi",
        ),
        // 50 µs into R's own transmission: turnaround.
        (ScriptAction::Transmit, 1.0 + 50e-6 - lead, "tx"),
        // 5 ms into R's own transmission: on the air.
        (ScriptAction::Transmit, 1.005 - lead, "half_duplex"),
    ];
    for (action, tx_at, why) in cases {
        let mut sc = trio(-90.0, -200.0).script_tx(tx_at, "T1", 3, 40, 12);
        if action == ScriptAction::Transmit {
            sc = sc.script_tx(1.0, "R", 5, 40, 12);
        } else {
            sc = sc.script(1.0, "R", action, Some(5));
        }
        let r = rx(&run(&sc), 2);
        assert_eq!(r.candidates, 1, "{why}");
        assert_eq!(r.delivered, 0, "{why}");
        let got = [
            ("retune", r.deaf.retune),
            ("cad", r.deaf.cad),
            ("rssi", r.deaf.rssi),
            ("tx", r.deaf.tx),
            ("half_duplex", r.half_duplex),
        ];
        for (name, n) in got {
            assert_eq!(
                n,
                (name == why) as u64,
                "packet during {why}: counter {name} = {n}"
            );
        }
    }
    // Control: no excursion, delivered.
    let r = rx(
        &run(&trio(-90.0, -200.0).script_tx(1.0, "T1", 3, 40, 12)),
        2,
    );
    assert_eq!(r.delivered, 1);
}

#[test]
fn reception_aborted_by_an_excursion_is_counted_deaf() {
    // R locks to T1's packet, then starts a CAD 5 ms into it.
    let sc = trio(-90.0, -200.0).script_tx(1.0, "T1", 3, 40, 12).script(
        1.005,
        "R",
        ScriptAction::Cad,
        Some(7),
    );
    let r = rx(&run(&sc), 2);
    assert_eq!((r.candidates, r.delivered, r.deaf.retune), (1, 0, 1));
}

#[test]
fn adjacent_channel_leakage_depends_on_rejection() {
    // Wanted on channel 3 at -100 dBm; T2 on channel 4, 10 dB stronger,
    // overlapping. R rests on 3, so T2 is not a candidate there.
    for (acr1, blocked) in [(0.0, true), (60.0, false)] {
        let mut sc = trio(-100.0, -90.0)
            .script_tx(1.0, "T1", 3, 40, 12)
            .script_tx(1.002, "T2", 4, 40, 12);
        sc.radio.acr1_db = acr1;
        let r = rx(&run(&sc), 2);
        assert_eq!(r.candidates, 1, "ACR {acr1}");
        if blocked {
            assert_eq!(
                (r.delivered, r.sinr_fail, r.sinr_fail_adjacent),
                (0, 1, 1),
                "ACR {acr1}"
            );
        } else {
            assert_eq!((r.delivered, r.sinr_fail), (1, 0), "ACR {acr1}");
        }
    }
    // Three channels away nothing leaks, whatever the rejection setting.
    let mut sc = trio(-100.0, -60.0)
        .script_tx(1.0, "T1", 3, 40, 12)
        .script_tx(1.002, "T2", 6, 40, 12);
    sc.radio.acr1_db = 0.0;
    sc.radio.acr2_db = 0.0;
    assert_eq!(rx(&run(&sc), 2).delivered, 1);
}

#[test]
fn below_sensitivity_is_counted_and_still_interferes() {
    let p = RadioParams::default();
    let s = p.sensitivity_dbm();
    let r = rx(
        &run(&trio(s - 0.5, -200.0).script_tx(1.0, "T1", 3, 40, 12)),
        2,
    );
    assert_eq!((r.below_sensitivity, r.delivered), (1, 0));
    let r = rx(
        &run(&trio(s + 0.5, -200.0).script_tx(1.0, "T1", 3, 40, 12)),
        2,
    );
    assert_eq!(r.delivered, 1);
    // A packet 2 dB above sensitivity is killed by one 1 dB below it.
    let sc = trio(s + 2.0, s - 1.0)
        .script_tx(1.0, "T1", 3, 40, 12)
        .script_tx(1.002, "T2", 3, 40, 12);
    let r = rx(&run(&sc), 2);
    assert_eq!((r.delivered, r.sinr_fail, r.below_sensitivity), (0, 1, 1));
}

#[test]
fn cad_sees_lora_above_sensitivity_and_rssi_sums_power() {
    let p = RadioParams::default();
    // T1 transmits on 5 at -100 dBm; R does CAD and later RSSI on 5 while it is on air.
    let sc = trio(-100.0, -200.0)
        .script_tx(1.0, "T1", 5, 100, 12)
        .script(1.002, "R", ScriptAction::Cad, Some(5))
        .script(1.010, "R", ScriptAction::Rssi, Some(5))
        .script(1.030, "R", ScriptAction::Cad, Some(5))
        .script(1.040, "R", ScriptAction::Rssi, Some(5));
    let sim = run(&sc);
    let got: Vec<_> = sim
        .script_log
        .iter()
        .filter_map(|r| match *r {
            ScriptResult::Cad { busy, .. } => Some(format!("cad {busy}")),
            ScriptResult::Rssi { dbm, .. } => Some(format!("rssi {dbm}")),
            _ => None,
        })
        .collect();
    let noise = p.noise_floor_dbm().round() as i16;
    let with_signal = (10.0 * (10f64.powf(-10.0) + 10f64.powf(p.noise_floor_dbm() / 10.0)).log10())
        .round() as i16;
    assert_eq!(with_signal, -100);
    // 100 bytes at SF5 is 59 ms, so the second pair is still on air.
    assert!(time_on_air(100, 12).as_micros() > 41_000);
    assert_eq!(
        got,
        vec![
            "cad true".to_string(),
            format!("rssi {with_signal}"),
            "cad true".into(),
            format!("rssi {with_signal}")
        ]
    );
    // Quiet channel: CAD false, RSSI at the noise floor.
    let sim = run(&trio(-100.0, -200.0)
        .script(1.0, "R", ScriptAction::Cad, Some(5))
        .script(1.1, "R", ScriptAction::Rssi, Some(5)));
    assert_eq!(
        sim.script_log
            .iter()
            .filter(|r| matches!(
                r,
                ScriptResult::Cad { busy: false, .. } | ScriptResult::Rssi { .. }
            ))
            .count(),
        2
    );
    assert!(sim
        .script_log
        .iter()
        .any(|r| matches!(r, ScriptResult::Rssi { dbm, .. } if *dbm == noise)));
    // CAD misses with probability 1 never report busy.
    let mut sc = trio(-100.0, -200.0)
        .script_tx(1.0, "T1", 5, 100, 12)
        .script(1.002, "R", ScriptAction::Cad, Some(5));
    sc.radio.cad_miss_probability = 1.0;
    let sim = run(&sc);
    assert!(sim
        .script_log
        .iter()
        .any(|r| matches!(r, ScriptResult::Cad { busy: false, .. })));
}

#[test]
fn interferers_are_heard_by_rssi_and_by_cad_only_if_lora_like() {
    for lora in [false, true] {
        let mut sc = trio(-100.0, -200.0)
            .script(1.0, "R", ScriptAction::Cad, Some(6))
            .script(1.1, "R", ScriptAction::Rssi, Some(6));
        sc.links.reference_loss_db = Some(40.0);
        sc.links.exponent = 2.0;
        sc.interferers.push(bark_sim::scenario::InterfererSpec {
            x: 105.0,
            y: 0.0,
            channel: 6,
            power_dbm: 0.0,
            lora,
            mean_on_s: 10.0,
            mean_off_s: 0.0,
        });
        let sim = run(&sc);
        // 100 m at exponent 2 from 40 dB at 1 m: 80 dB loss, -80 dBm.
        assert!(sim.script_log.contains(&ScriptResult::Cad {
            node: 2,
            at: sim.script_log[0].at(),
            channel: 6,
            busy: lora
        }));
        assert!(sim
            .script_log
            .iter()
            .any(|r| matches!(r, ScriptResult::Rssi { dbm: -80, .. })));
    }
}

#[test]
#[should_panic(expected = "leaf 0 (T1): scripted event started Transmit")]
fn two_outstanding_excursions_panic_naming_the_node() {
    run(&trio(-100.0, -100.0)
        .script_tx(1.0, "T1", 3, 40, 12)
        .script_tx(1.001, "T1", 3, 40, 12));
}

#[test]
#[should_panic(expected = "leaf 1 (T2): transmit with a 8-symbol preamble")]
fn short_preamble_panics_naming_the_node() {
    run(&trio(-100.0, -100.0).script_tx(1.0, "T2", 3, 40, 8));
}
