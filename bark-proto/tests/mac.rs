//! The relay's medium access, driven through real bytes on the test medium.

mod common;

use bark_proto::radio::{time_on_air, CHANNELS, DUTY_CYCLE_WINDOW, MIN_PREAMBLE_SYMBOLS};
use bark_proto::tables::budget::{BUDGET, DEPTH};
use bark_proto::wire::{Packet, HEARTBEAT_LEN};
use bark_proto::{Action, Channel, Config, Duration, Instant, Position, Relay, Trace};
use common::{at, ch, secs, Harness};

/// A position `east`, `north` metres from a point in Kyiv.
fn pos(east: f64, north: f64) -> Position {
    let (lat, lon) = (50.45, 30.52);
    let m_per_deg = 6_371_000.0 * core::f64::consts::PI / 180.0;
    Position::from_degrees(
        lat + north / m_per_deg,
        lon + east / (m_per_deg * lat.to_radians().cos()),
    )
}

fn relay(east: f64, seed: u64) -> Relay {
    Relay::new(
        Config {
            position: pos(east, 0.0),
            ..Config::default()
        },
        seed,
    )
}

fn knows(h: &Harness<Relay>, a: usize, b: usize) -> bool {
    let target = h.nodes[b].core.config().position;
    h.nodes[a].core.neighbours().any(|n| n.position == target)
}

/// Worst case for one rotation: thirteen heartbeats at the longest interval.
fn rotation(c: &Config) -> Duration {
    c.heartbeat_max.mul(CHANNELS as u64)
}

/// Each relay's heartbeat lands on the other's home once per rotation
/// (spec "Heartbeat"), so discovery takes one rotation after boot, unless that one
/// heartbeat arrives while the receiver is off its home channel: twelve
/// sweep readings of 0.7 ms a second here, ≈1 % of heartbeats, since this
/// medium loses a frame to any overlap and ignores the preamble the modem
/// can lock on after a short gap (spec "Busy estimate"). Then it takes the next rotation.
#[test]
fn two_relays_discover_each_other_within_one_rotation() {
    let c = Config::default();
    let mut worst = Duration::ZERO;
    let mut total = Duration::ZERO;
    let mut late = 0;
    let seeds = 20;
    for seed in 0..seeds {
        let mut h = Harness::new(vec![relay(0.0, 2 * seed), relay(500.0, 2 * seed + 1)]);
        h.link(0, 1);
        let boot_end = Instant::ZERO + c.boot_listen;
        let deadline = boot_end + rotation(&c).mul(2);
        assert!(
            h.run_until(deadline, |h| knows(h, 0, 1) && knows(h, 1, 0)),
            "seed {seed}: not discovered within two rotations after boot"
        );
        let took = h.now.since(boot_end);
        if took > rotation(&c) {
            late += 1;
        }
        worst = worst.max(took);
        total = total + took;
        // Both chose a home and listen there.
        for i in 0..2 {
            assert!(!h.nodes[i].core.booting());
            assert_eq!(h.resting(i), Some(h.nodes[i].core.home()));
        }
        // And each records the other's channel.
        let n = h.nodes[0].core.neighbours().next().unwrap();
        assert_eq!(n.home, h.nodes[1].core.home());
    }
    println!(
        "discovery after boot: mean {:.1} s, worst {:.1} s, {late} of {seeds} past one rotation ({} s)",
        total.as_millis() as f64 / 1000.0 / seeds as f64,
        worst.as_millis() as f64 / 1000.0,
        rotation(&c).as_millis() / 1000
    );
    assert!(
        late <= 2,
        "{late} of {seeds} pairs needed a second rotation"
    );
}

#[test]
fn sweep_reads_one_channel_at_a_time_spread_over_the_interval() {
    // Spec "Busy estimate", roadmap D13: one reading every interval / 13, channels
    // in turn, so the relay is off its home for one reading at a time.
    let home = ch(4);
    let c = Config {
        position: pos(0.0, 0.0),
        home_override: Some(home),
        ..Config::default()
    };
    let mut h = Harness::new(vec![Relay::new(c, 5)]);
    let (from, to) = (at(200), at(260));
    h.run_until(to, |_| false);
    let checks = &h.nodes[0].checks;
    // A heartbeat's listen-before-talk is a CAD and then an RSSI reading of
    // the same channel; every other reading is the sweep's.
    let lbt = |k: usize| {
        k > 0
            && matches!((checks[k - 1].2, checks[k].2), (Action::Cad(a), Action::Rssi(b)) if a == b)
    };
    let sweep: Vec<_> = (0..checks.len())
        .filter(|&k| checks[k].0 >= from && matches!(checks[k].2, Action::Rssi(_)) && !lbt(k))
        .map(|k| checks[k])
        .collect();
    let step = c.sweep_interval.div(CHANNELS as u64);
    let expected = to.since(from).as_micros() / step.as_micros();
    assert!(
        sweep.len() as u64 + 1 >= expected && sweep.len() as u64 <= expected + 1,
        "{} readings",
        sweep.len()
    );
    // A heartbeat (its checks and 12 ms frame) may delay one reading; the
    // next is back on schedule.
    let slack = Duration::from_millis(20).as_micros() as i64;
    for w in sweep.windows(2) {
        let (Action::Rssi(a), Action::Rssi(b)) = (w[0].2, w[1].2) else {
            unreachable!()
        };
        assert_eq!(b, a.next(), "channels in turn");
        let gap = w[1].0.since(w[0].0).as_micros() as i64;
        assert!(
            (gap - step.as_micros() as i64).abs() < slack,
            "{gap} µs between readings"
        );
        // Back on home well before the next reading leaves it.
        assert!(w[1].0.since(w[0].1) > Duration::from_millis(50));
    }
    // The reading of the home channel does not leave it (core-interface
    // spec "Regulatory limits" rule 7); every other one does.
    for (_, _, a, off) in &sweep {
        assert_eq!(*off, *a != Action::Rssi(home), "{a:?}");
    }
}

#[test]
fn heartbeat_rotates_through_every_channel_in_order() {
    let c = Config::default();
    let mut h = Harness::new(vec![relay(0.0, 7)]);
    h.run_until(at(150) + rotation(&c).mul(2), |_| false);
    let sent = &h.nodes[0].sent;
    assert!(
        sent.len() >= 2 * CHANNELS as usize,
        "{} heartbeats",
        sent.len()
    );
    for w in sent.windows(2) {
        let (t0, f0) = w[0];
        let (t1, f1) = w[1];
        assert_eq!(
            f1.channel,
            f0.channel.next(),
            r#"k ← k + 1 (spec "Heartbeat")"#
        );
        let gap = t1.since(t0);
        // The interval plus the listen-before-talk checks on an idle channel.
        assert!(
            gap >= c.heartbeat_min && gap <= c.heartbeat_max + Duration::from_millis(5),
            "{gap:?}"
        );
        let Ok(Packet::Heartbeat(hb)) = Packet::decode(f0.payload()) else {
            panic!("not a heartbeat")
        };
        let me = h.nodes[0].core.config().position;
        assert_eq!(
            (hb.home, hb.position, hb.surveyed),
            (h.nodes[0].core.home(), me, true)
        );
        assert_eq!(f0.len as usize, HEARTBEAT_LEN);
    }
}

#[test]
fn boot_avoids_busy_channels_and_spreads_ties() {
    // A relay that hears a carrier on ten channels homes on one of the
    // other three. (With twelve jammed, fewer than a tenth of the readings
    // are quiet and the noise floor, a 10th percentile, rises to the
    // carrier: every channel then looks idle. `tables/busy.rs`.)
    for first in [0u8, 1, 4] {
        let free = [first, first + 4, first + 8];
        let mut h = Harness::new(vec![relay(0.0, first as u64)]);
        for c in Channel::all().filter(|c| !free.contains(&c.index())) {
            h.jam(0, c);
        }
        h.run_until(at(151), |_| false);
        assert!(
            free.contains(&h.nodes[0].core.home().index()),
            "{:?}",
            h.nodes[0].core.home()
        );
    }
    // Relays that hear nothing do not all pick channel 0 (spec "Channel at boot").
    let mut h = Harness::new(
        (0..26)
            .map(|i| relay(i as f64 * 10_000.0, 100 + i))
            .collect(),
    );
    h.run_until(at(151), |_| false);
    let homes: std::collections::HashSet<_> = h.nodes.iter().map(|n| n.core.home()).collect();
    assert!(homes.len() >= 8, "{} distinct homes among 26", homes.len());
}

#[test]
fn home_override_skips_the_choice() {
    let c = Config {
        home_override: Some(ch(4)),
        ..Config::default()
    };
    let mut h = Harness::new(vec![Relay::new(c, 1)]);
    for c in Channel::all().filter(|c| c.index() != 9) {
        h.jam(0, c);
    }
    h.run_until(at(2000), |_| false);
    assert_eq!(h.nodes[0].core.home(), ch(4));
    assert_eq!(h.resting(0), Some(ch(4)));
}

#[test]
fn a_busy_home_moves_and_announces_on_the_old_channel() {
    // B is pinned to A's home, so it hears A's announcement there at once.
    let c = Config::default();
    let mut h = Harness::new(vec![relay(0.0, 3), relay(400.0, 4)]);
    h.link(0, 1);
    h.run_until(at(150), |_| false);
    let old = h.nodes[0].core.home();
    let mut h = Harness::new(vec![
        relay(0.0, 3),
        Relay::new(
            Config {
                position: pos(400.0, 0.0),
                home_override: Some(old),
                ..Config::default()
            },
            4,
        ),
    ]);
    h.link(0, 1);
    let boot_end = at(150);
    h.run_until(boot_end + rotation(&c), |h| knows(h, 1, 0));
    assert!(knows(&h, 1, 0));
    // A foreign carrier appears on A's home, on 30 % of the time. (A
    // channel busy all the time would also refuse the announcement below:
    // listen-before-talk on the channel being left.) Its period shares no
    // factor with the sweep's, so the home readings fall on every phase of
    // it rather than always in its on or its off part.
    let jam_at = h.now;
    h.jam_part(
        0,
        old,
        Duration::from_millis(32),
        Duration::from_millis(107),
    );
    let moved = |h: &Harness<Relay>| {
        h.nodes[0].traces.iter().any(|(_, t)| {
            matches!(t, Trace::HomeChannelMoved { .. }) && h.nodes[0].core.home() != old
        })
    };
    // The estimate must first cross 5 %, then hold for the dwell, then the
    // random wait: well within dwell + wait + a few minutes.
    let limit = jam_at + c.move_dwell + c.move_wait_max + secs(120);
    assert!(h.run_until(limit, moved), "no move");
    let (moved_at, new) = (h.now, h.nodes[0].core.home());
    assert_ne!(new, old);
    assert!(
        moved_at.since(jam_at) >= c.move_dwell,
        "moved before the dwell"
    );
    assert_eq!(h.resting(0), Some(new));
    // One heartbeat on the old channel carries the new home; B learns it
    // within a heartbeat's listen-before-talk, not a rotation.
    let learnt = |h: &Harness<Relay>| h.nodes[1].core.neighbours().any(|n| n.home == new);
    assert!(
        h.run_until(moved_at + secs(1), learnt),
        "B did not hear the announcement"
    );
    let (t, f) = h.nodes[0]
        .sent
        .iter()
        .rev()
        .find(|(_, f)| f.channel == old)
        .copied()
        .unwrap();
    assert!(t >= moved_at);
    let Ok(Packet::Heartbeat(hb)) = Packet::decode(f.payload()) else {
        panic!()
    };
    assert_eq!(hb.home, new);
    // No second move: the new home is quiet.
    h.run_for(c.move_dwell + c.move_wait_max);
    let moves = h.nodes[0]
        .traces
        .iter()
        .filter(|(_, t)| matches!(t, Trace::HomeChannelMoved { .. }))
        .count();
    assert_eq!(moves, 2, "the boot choice and one move");
    println!(
        "moved {:.0} s after the jam began",
        moved_at.since(jam_at).as_millis() as f64 / 1000.0
    );
}

#[test]
fn boot_steps_off_an_occupied_position() {
    // A is running when B boots at the same position; B hears A and moves.
    // B listens a worst-case rotation, so it hears A for certain.
    let c = Config::default();
    let p = pos(0.0, 0.0);
    let mut h = Harness::new(vec![
        Relay::new(
            Config {
                position: p,
                home_override: Some(ch(2)),
                boot_listen: secs(10),
                ..c
            },
            1,
        ),
        Relay::new(
            Config {
                position: p,
                home_override: Some(ch(2)),
                boot_listen: rotation(&c),
                ..c
            },
            2,
        ),
    ]);
    h.link(0, 1);
    h.start_at(1, at(60));
    h.run_until(at(60) + rotation(&c).mul(3), |_| false);
    let b = h.nodes[1].core.config().position;
    assert_ne!(b, p);
    assert_eq!(
        (b.lat - p.lat, b.lon),
        (2, p.lon),
        "two counts, ≈1.2 m north"
    );
    assert!(h.nodes[1]
        .traces
        .iter()
        .any(|(_, t)| *t == Trace::PositionShifted { to: b }));
    assert!(
        knows(&h, 1, 0) && knows(&h, 0, 1),
        "both run as distinct relays"
    );
}

#[test]
fn duty_budget_defers_heartbeats_and_never_drops_them() {
    // Heartbeats every 50 ms ask for 11.84 / 50 ≈ 24 % of airtime, over the
    // 10 % cap. The harness panics if any hour holds more than 360 s. B
    // watches A throughout.
    let fast = Config {
        heartbeat_min: Duration::from_millis(50),
        heartbeat_max: Duration::from_millis(50),
        boot_listen: secs(1),
        home_override: Some(ch(5)),
        ..Config::default()
    };
    let mut h = Harness::new(vec![
        Relay::new(
            Config {
                position: pos(0.0, 0.0),
                ..fast
            },
            1,
        ),
        Relay::new(
            Config {
                position: pos(300.0, 0.0),
                ..Config::default()
            },
            2,
        ),
    ]);
    h.link(0, 1);
    let end = Instant::ZERO + DUTY_CYCLE_WINDOW + secs(1200);
    h.run_until(end, |_| false);

    let a = &h.nodes[0];
    let hb = time_on_air(HEARTBEAT_LEN as u8, MIN_PREAMBLE_SYMBOLS);
    let deferrals: Vec<Instant> = a
        .traces
        .iter()
        .filter_map(|(_, t)| match t {
            Trace::HeartbeatDeferred { until } => Some(*until),
            _ => None,
        })
        .collect();
    assert!(!deferrals.is_empty(), "the budget never bound");
    assert!(!a
        .traces
        .iter()
        .any(|(_, t)| matches!(t, Trace::HeartbeatSkipped { .. })));
    // Deferred, not dropped: the held heartbeat goes out at the instant the
    // budget allows it again, or as soon after as listen-before-talk takes:
    // a few milliseconds, or its backoffs when B's heartbeat is on the
    // channel (spec "Heartbeat").
    let lbt = fast.lbt_backoff_max.mul(fast.lbt_attempts as u64);
    let mut checked = 0;
    for until in &deferrals {
        if *until >= end {
            continue;
        }
        let (t, _) = a
            .sent
            .iter()
            .find(|(t, _)| t >= until)
            .expect("a heartbeat after the deferral");
        assert!(
            t.since(*until) < lbt,
            "held {:?} past {until:?}",
            t.since(*until)
        );
        checked += 1;
    }
    assert!(checked > 0);
    // The heartbeat after the first deferral is the one that was held: no
    // heartbeat was produced and discarded meanwhile, so the rotation
    // continues one channel on.
    let first = deferrals[0];
    let before = a
        .sent
        .iter()
        .rev()
        .find(|(t, _)| *t < first)
        .unwrap()
        .1
        .channel;
    let after = a.sent.iter().find(|(t, _)| *t >= first).unwrap().1.channel;
    assert_eq!(after, before.next());
    // At the cap, the last hour carries what the bucket lets in over an
    // hour (`tables/budget.rs`), the budget less the bucket's depth, less
    // a frame.
    let from = end.as_micros() - DUTY_CYCLE_WINDOW.as_micros();
    let spent: u64 = a
        .airtime
        .iter()
        .filter(|(t, _)| t.as_micros() >= from)
        .map(|(_, d)| d.as_micros())
        .sum();
    let floor = (BUDGET - DEPTH).as_micros() - hb.as_micros();
    assert!(
        spent >= floor && spent <= BUDGET.as_micros(),
        "{spent} µs in the last hour"
    );
    // Rationed heartbeats still reach B.
    assert!(knows(&h, 1, 0));
    println!(
        "{} heartbeats, {} deferrals, {:.1} s on air in the last hour",
        a.sent.len(),
        deferrals.len(),
        spent as f64 / 1e6
    );
}
