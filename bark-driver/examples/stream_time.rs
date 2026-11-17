//! How long a relay's calls take on this machine while it streams an
//! image to the gatherers among its neighbours, with 3 neighbours and
//! with a full table of 64 (spec "Updates over the mesh").
//!
//! Each neighbour's heartbeat is heard once a rotation; a third of them
//! gather the image, each at another block. The relay is told it holds the
//! image five seconds in. Excursions complete at once, as on a quiet
//! channel. The run is deterministic, so it is made three times and each
//! call's shortest time kept, to leave out other load on the machine.
//!
//! The 3-neighbour case is about what the boards on the bench run:
//! `cargo run --release -p bark-driver --example stream_time`.

use std::time::Instant as Wall;

use bark_proto::radio::{time_on_air, Channel};
use bark_proto::update::{Image, Section, SECTION_LEN};
use bark_proto::wire::{encode_section, Heartbeat, Packet};
use bark_proto::{
    Action, AppCommand, Config, Core, Duration, Event, Instant, NodeId, Position, Relay, MAX_FRAME,
};

/// Simulated time.
const RUN: Duration = Duration::from_secs(900);
/// Runs whose shortest time per call is kept.
const REPEATS: usize = 3;
/// A listen-before-talk's CAD at SF5 with 8 symbols, plus half a symbol
/// (SX1261/2 DS §6.1.5).
const CAD_TIME: Duration = Duration::from_micros(2176);
/// About a rotation of a neighbour's heartbeats over the 13 channels, in
/// which this one hears each once (spec "Heartbeat").
const ROTATION_US: u64 = 128_000_000;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Received,
    Wakeup,
    Completion,
}

/// An excursion's completion, owned.
#[derive(Clone, Copy)]
enum Done {
    Lbt(Channel),
    Rssi(Channel),
    Tx,
}

fn ch(k: u8) -> Channel {
    Channel::new(k).expect("in the plan")
}

/// One call: its kind, and the nanoseconds of `next_wakeup` before it and
/// of `handle`.
struct Call {
    kind: Kind,
    wake_ns: u128,
    handle_ns: u128,
}

/// One run with `neighbours`: its calls and the pieces served.
fn run(neighbours: usize) -> (Vec<Call>, u64) {
    let home = ch(3);
    let config = Config {
        position: Position::from_degrees(50.45, 30.52),
        home_override: Some(home),
        boot_listen: Duration::from_secs(1),
        version: 6,
        ..Config::default()
    };
    let image = Image {
        version: 7,
        blocks: 48,
        last: 64,
        key: 0,
    };
    let mut relay = Relay::new(config, 1);
    let side = (neighbours as f64).sqrt().ceil() as usize;
    let nb: Vec<(Position, Channel, Option<u8>)> = (0..neighbours)
        .map(|i| {
            let (x, y) = (
                (i % side) as f64 - side as f64 / 2.0,
                (i / side) as f64 - side as f64 / 2.0,
            );
            let p = Position::from_degrees(50.45 + y * 0.0025, 30.52 + x * 0.004);
            let gathers = (i % 3 == 0).then_some(((i / 3) * 2 % 48) as u8);
            (p, ch((i % 13) as u8), gathers)
        })
        .collect();
    let end = Instant::ZERO + RUN;
    let mut now = Instant::ZERO;
    let mut excursion: Option<(Instant, Done)> = None;
    let mut next_rx = Instant::ZERO + Duration::from_secs(2);
    let mut seq: usize = 0;
    let mut calls = Vec::new();
    let mut frame = [0u8; MAX_FRAME];
    let mut held = false;
    let mut pieces = 0u64;
    while now < end {
        let t = Wall::now();
        let wake = relay.next_wakeup().unwrap_or(end);
        let wake_ns = t.elapsed().as_nanos();
        let mut len = 0;
        let (kind, done, at) = if calls.is_empty() {
            (Kind::Wakeup, None, now)
        } else if let Some((t, d)) = excursion.filter(|(t, _)| *t <= wake && *t <= next_rx) {
            excursion = None;
            (Kind::Completion, Some(d), t)
        } else if next_rx < wake {
            let (pos, nhome, gathers) = nb[seq % neighbours];
            seq += 1;
            let hb = Packet::Heartbeat(Heartbeat {
                sleepy: false,
                anchorless: false,
                carried: false,
                home: nhome,
                format: bark_proto::wire::FORMAT,
                map_point: false,
                leaving: false,
                id: NodeId::from_u64(pos.short().0 as u64),
                short: pos.short(),
            });
            len = hb.encode(&mut frame).expect("encodes");
            let section = Section {
                runs: 6,
                image: gathers.map(|_| image),
                complete: gathers.unwrap_or(0),
                have: 0,
            };
            let mut b = [0u8; SECTION_LEN];
            encode_section(&section, &mut b);
            frame[len..len + SECTION_LEN].copy_from_slice(&b);
            len += SECTION_LEN;
            let t = next_rx;
            next_rx = t + Duration::from_micros(ROTATION_US / neighbours as u64);
            (Kind::Received, None, t)
        } else {
            (Kind::Wakeup, None, wake)
        };
        now = now.max(at);
        let event = match (kind, done) {
            (Kind::Received, _) => Event::Received {
                channel: home,
                rssi_dbm: -90,
                snr_db: 8,
                bytes: &frame[..len],
            },
            (_, Some(Done::Lbt(channel))) => Event::LbtDone {
                channel,
                busy: false,
                dbm: -120,
            },
            (_, Some(Done::Rssi(channel))) => Event::RssiSample { channel, dbm: -120 },
            (_, Some(Done::Tx)) => Event::TxDone,
            (_, None) => Event::Wakeup,
        };
        let mut asked = None;
        let start = Wall::now();
        relay.handle(event, now, &mut |a| match a {
            Action::Transmit(f) => {
                asked = Some((now + time_on_air(f.len, f.preamble_symbols), Done::Tx))
            }
            Action::Serve(f) => {
                pieces += 1;
                asked = Some((now + time_on_air(f.len, f.preamble_symbols), Done::Tx))
            }
            Action::Lbt(c) => asked = Some((now + CAD_TIME, Done::Lbt(c))),
            Action::Rssi(c) => asked = Some((now, Done::Rssi(c))),
            _ => {}
        });
        let handle_ns = start.elapsed().as_nanos();
        calls.push(Call {
            kind,
            wake_ns,
            handle_ns,
        });
        if asked.is_some() {
            assert!(excursion.is_none(), "second excursion");
            excursion = asked;
        }
        if !held && now > Instant::ZERO + Duration::from_secs(5) {
            held = true;
            relay.handle(Event::App(AppCommand::Holds(image)), now, &mut |_| {});
        }
    }
    (calls, pieces)
}

/// The median and the 99th percentile of `v`.
fn spread(mut v: Vec<u128>) -> (u128, u128) {
    v.sort_unstable();
    if v.is_empty() {
        return (0, 0);
    }
    (v[v.len() / 2], v[(v.len() - 1) * 99 / 100])
}

fn main() {
    for neighbours in [3usize, 64] {
        let (mut calls, pieces) = run(neighbours);
        for _ in 1..REPEATS {
            for (c, a) in calls.iter_mut().zip(run(neighbours).0) {
                c.handle_ns = c.handle_ns.min(a.handle_ns);
                c.wake_ns = c.wake_ns.min(a.wake_ns);
            }
        }
        // Past the first 150 s: the table full, the stream under way.
        let settled: Vec<&Call> = calls.iter().skip(calls.len() / 6).collect();
        println!(
            "neighbours {neighbours}: {} calls, {pieces} pieces served",
            calls.len()
        );
        let (m, p) = spread(settled.iter().map(|c| c.wake_ns).collect());
        println!("  next_wakeup: median {m} ns, p99 {p} ns");
        let (m, p) = spread(settled.iter().map(|c| c.wake_ns + c.handle_ns).collect());
        println!("  next_wakeup and handle: median {m} ns, p99 {p} ns");
        for kind in [Kind::Received, Kind::Wakeup, Kind::Completion] {
            let v: Vec<u128> = settled
                .iter()
                .filter(|c| c.kind == kind)
                .map(|c| c.handle_ns)
                .collect();
            let n = v.len();
            let (m, p) = spread(v);
            println!("  handle {kind:?} x{n}: median {m} ns, p99 {p} ns");
        }
    }
}
