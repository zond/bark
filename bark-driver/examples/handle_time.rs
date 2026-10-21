//! How long one `handle` call of a loaded relay takes on this machine.
//!
//! A relay with a full neighbour table hears full-length DATA frames back to
//! back, every one for a destination far away, and the neighbours' heartbeats
//! between them; nobody answers its hand-overs, so its records and send queue
//! fill and stay full. Excursions complete at once, as on a quiet channel.
//!
//! `cargo run --release -p bark-driver --example handle_time` runs it five
//! times and prints, by event, each call's shortest time over the five,
//! which leaves out what other work on the machine added, and the slowest
//! calls. With a call count `STOP` it runs once and stops before that call,
//! so that runs under `perf stat -e instructions:u` stopping before and
//! after one call give that call's instruction count: the run is
//! deterministic.

use std::time::Instant as Wall;

use bark_proto::radio::{time_on_air, Channel, MIN_PREAMBLE_SYMBOLS};
use bark_proto::wire::{Data, Heartbeat, Packet, MAX_PAYLOAD};
use bark_proto::{
    Action, Config, Core, Duration, Event, Instant, NodeId, Position, Relay, MAX_FRAME,
};

/// Neighbours: the table's capacity (spec "Neighbour table").
const NEIGHBOURS: usize = 64;
/// Simulated time.
const RUN: Duration = Duration::from_secs(600);
/// Runs whose shortest time per call is kept.
const REPEATS: usize = 5;
/// CAD at SF5 with 8 symbols, plus half a symbol (SX1261/2 DS §6.1.5).
const CAD_TIME: Duration = Duration::from_micros(2176);

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Received,
    Wakeup,
    Completion,
}

/// An excursion's completion, owned.
#[derive(Clone, Copy)]
enum Done {
    Cad(Channel),
    Rssi(Channel),
    Tx,
}

fn ch(k: u8) -> Channel {
    Channel::new(k).expect("in the plan")
}

/// One run: each call's kind and nanoseconds, and the relay at the end.
fn run(stop: u64) -> (Vec<(Kind, u128)>, Relay) {
    let me = Position::from_degrees(50.45, 30.52);
    let home = ch(3);
    let config = Config {
        position: me,
        home_override: Some(home),
        boot_listen: Duration::from_secs(1),
        ..Config::default()
    };
    let mut relay = Relay::new(config, 1);
    // Neighbours on a 2 km square around it; the destination 20 km north.
    let neighbours: Vec<(Position, Channel)> = (0..NEIGHBOURS)
        .map(|i| {
            let (x, y) = ((i % 8) as f64 - 3.5, (i / 8) as f64 - 3.5);
            let p = Position::from_degrees(50.45 + y * 0.0025, 30.52 + x * 0.004);
            (p, ch((i % 13) as u8))
        })
        .collect();
    let far = Position::from_degrees(50.63, 30.52);

    let end = Instant::ZERO + RUN;
    let mut now = Instant::ZERO;
    let mut excursion: Option<(Instant, Done)> = None;
    let mut next_rx = Instant::ZERO + Duration::from_secs(2);
    let mut seq: u32 = 0;
    let mut times = Vec::new();
    let mut frame = [0u8; MAX_FRAME];
    let mut payload = [0u8; MAX_PAYLOAD];

    while now < end && (times.len() as u64) < stop {
        let wake = relay.next_wakeup().unwrap_or(end);
        let mut len = 0;
        let (kind, done, at) = if times.is_empty() {
            (Kind::Wakeup, None, now)
        } else if let Some((t, d)) = excursion.filter(|(t, _)| *t <= wake && *t <= next_rx) {
            excursion = None;
            (Kind::Completion, Some(d), t)
        } else if next_rx < wake {
            seq += 1;
            // One frame in eight is a heartbeat, going round the neighbours:
            // 64 neighbours each heard once a rotation on this channel make
            // about one every two seconds. DATA comes from any of them.
            let beat = seq.is_multiple_of(8);
            let k = if beat { seq / 8 } else { seq };
            let (pos, nhome) = neighbours[k as usize % NEIGHBOURS];
            payload[..4].copy_from_slice(&seq.to_be_bytes());
            let packet = match beat {
                true => Packet::Heartbeat(Heartbeat {
                    sleepy: false,
                    anchorless: false,
                    carried: false,
                    home: nhome,
                    id: NodeId(pos.short().0.to_be_bytes()),
                    short: pos.short(),
                }),
                false => Packet::Data(Data {
                    sender_home: nhome,
                    dst_hop: relay.id(),
                    src_hop: NodeId(pos.short().0.to_be_bytes()),
                    dst_pos: far,
                    dst_id: NodeId([9; 4]),
                    hops: 1 + (seq % 20) as u8,
                    time_left: u8::MAX,
                    payload: &payload,
                }),
            };
            len = packet.encode(&mut frame).expect("encodes");
            let t = next_rx;
            next_rx = t + time_on_air(len as u8, MIN_PREAMBLE_SYMBOLS);
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
            (_, Some(Done::Cad(channel))) => Event::CadDone {
                channel,
                busy: false,
            },
            (_, Some(Done::Rssi(channel))) => Event::RssiSample { channel, dbm: -120 },
            (_, Some(Done::Tx)) => Event::TxDone,
            (_, None) => Event::Wakeup,
        };
        let mut asked = None;
        let start = Wall::now();
        relay.handle(event, now, &mut |a| match a {
            Action::Cad(c) => asked = Some((now + CAD_TIME, Done::Cad(c))),
            Action::Rssi(c) => asked = Some((now, Done::Rssi(c))),
            Action::Transmit(f) => {
                asked = Some((now + time_on_air(f.len, f.preamble_symbols), Done::Tx))
            }
            _ => {}
        });
        times.push((kind, start.elapsed().as_nanos()));
        if asked.is_some() {
            assert!(excursion.is_none(), "second excursion");
            excursion = asked;
        }
    }
    (times, relay)
}

fn main() {
    if let Some(stop) = std::env::args().nth(1) {
        let (times, _) = run(stop.parse().expect("a call count"));
        println!("calls={}", times.len());
        return;
    }
    let (mut times, relay) = run(u64::MAX);
    for _ in 1..REPEATS {
        let (again, _) = run(u64::MAX);
        for (t, a) in times.iter_mut().zip(again) {
            t.1 = t.1.min(a.1);
        }
    }
    println!(
        "calls={} holding={} neighbours={}",
        times.len(),
        relay.holding().count(),
        relay.neighbours().count()
    );
    let mut slowest: Vec<(usize, u128)> = times.iter().map(|(_, t)| *t).enumerate().collect();
    slowest.sort_unstable_by_key(|(_, t)| std::cmp::Reverse(*t));
    for (i, t) in slowest.iter().take(8) {
        println!("slow call={i} ns={t}");
    }
    for (kind, name) in [
        (Kind::Received, "received"),
        (Kind::Wakeup, "wakeup"),
        (Kind::Completion, "completion"),
    ] {
        let mut v: Vec<u128> = times
            .iter()
            .filter(|(k, _)| *k == kind)
            .map(|(_, t)| *t)
            .collect();
        v.sort_unstable();
        let at = |per_mille: usize| v[(v.len() - 1) * per_mille / 1000];
        println!(
            "{name:<10} n={:<6} mean_ns={:<5} p50_ns={:<5} p99_ns={:<5} p99.9_ns={:<5} max_ns={}",
            v.len(),
            v.iter().sum::<u128>() / v.len() as u128,
            at(500),
            at(990),
            at(999),
            v[v.len() - 1]
        );
    }
}
