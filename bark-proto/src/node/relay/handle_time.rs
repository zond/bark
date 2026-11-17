//! How long one `handle` call of a loaded relay takes on this machine:
//! `cargo test --release -p bark-proto --lib handle_time -- --ignored --nocapture`.
//!
//! A relay hears DATA frames back to back, each for a destination 20 km
//! away in one of 64 directions, and its neighbours' heartbeats between
//! them; nobody answers its hand-overs. Excursions complete at once, as on
//! a quiet channel. Each run is deterministic, so it is made five times
//! and each call's shortest time kept, to leave out other load on the
//! machine.
//!
//! Left to itself the queue fills as far as the bucket and the bans let
//! it. The worst case is then made by hand: the queue topped up to its
//! whole regular room and some hop places, each entry with its record, as
//! arrivals would leave them, and one received DATA timed, and the choice of what to send next,
//! also with every neighbour banned, when nothing may go.

extern crate std;
use std::println;
use std::time::Instant as Wall;
use std::vec::Vec;

use super::*;
use crate::radio::time_on_air;
use crate::tables::records::Record;
use crate::tables::send_queue::{Message, Outgoing, SEND_QUEUE};
use crate::wire::{Data, Heartbeat, Hops, Packet, MAX_PAYLOAD};
use crate::MAX_FRAME;

/// Runs whose shortest time per call is kept.
const REPEATS: usize = 5;
/// A listen-before-talk's CAD at SF5 with 8 symbols, plus half a symbol
/// (SX1261/2 DS §6.1.5).
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
    Lbt(Channel),
    Rssi(Channel),
    Tx,
}

fn ch(k: u8) -> Channel {
    Channel::new(k).expect("in the plan")
}

/// A relay at the centre of `n` neighbours on a 2 km square, and what
/// feeds it.
struct Bench {
    relay: std::boxed::Box<Relay>,
    now: Instant,
    neighbours: Vec<(Position, Channel)>,
    far: Vec<Position>,
    payload: usize,
    seq: u32,
    next_rx: Instant,
    excursion: Option<(Instant, Done)>,
}

impl Bench {
    fn new(n: usize, payload: usize) -> Bench {
        let config = Config {
            position: Position::from_degrees(50.45, 30.52),
            home_override: Some(ch(3)),
            boot_listen: Duration::from_secs(1),
            ..Config::default()
        };
        let neighbours = (0..n)
            .map(|i| {
                let (x, y) = ((i % 8) as f64 - 3.5, (i / 8) as f64 - 3.5);
                let p = Position::from_degrees(50.45 + y * 0.0025, 30.52 + x * 0.004);
                (p, ch((i % 13) as u8))
            })
            .collect();
        // Many directions, so the queue's messages head to many neighbours
        // and no one way fills (spec "Send queue", the gate).
        let far = (0..64)
            .map(|k| {
                let a = k as f64 * core::f64::consts::TAU / 64.0;
                Position::from_degrees(50.45 + 0.18 * libm::cos(a), 30.52 + 0.28 * libm::sin(a))
            })
            .collect();
        Bench {
            relay: std::boxed::Box::new(Relay::new(config, 1)),
            now: Instant::ZERO,
            neighbours,
            far,
            payload,
            seq: 0,
            next_rx: Instant::ZERO + Duration::from_secs(2),
            excursion: None,
        }
    }

    /// The `seq`th frame heard: one in eight a heartbeat, about one every
    /// two seconds from 64 neighbours each heard once a rotation here.
    fn frame(&self, seq: u32, hops: Hops, out: &mut [u8; MAX_FRAME]) -> usize {
        let beat = seq.is_multiple_of(8);
        let k = if beat { seq / 8 } else { seq };
        let (pos, home) = self.neighbours[k as usize % self.neighbours.len()];
        let mut payload = [0u8; MAX_PAYLOAD];
        payload[..4].copy_from_slice(&seq.to_be_bytes());
        let id = NodeId::from_u64(pos.short().0 as u64);
        let packet = match beat {
            true => Packet::Heartbeat(Heartbeat {
                sleepy: false,
                anchorless: false,
                carried: false,
                home,
                format: crate::wire::FORMAT,
                map_point: false,
                leaving: false,
                id,
                short: pos.short(),
            }),
            false => Packet::Data(Data {
                wrong: 0,
                sender_home: home,
                dst_hop: self.relay.id(),
                src_hop: id,
                dst_pos: self.far[seq as usize % self.far.len()],
                dst_id: NodeId([9; 6]),
                hops,
                payload: &payload[..self.payload],
            }),
        };
        packet.encode(out).expect("encodes")
    }

    /// One event, the next of the run, timed.
    fn step(&mut self, hops: Option<Hops>) -> (Kind, u128) {
        let wake = self.relay.next_wakeup().unwrap_or(Instant::MAX);
        let mut frame = [0u8; MAX_FRAME];
        let mut len = 0;
        let (kind, done, at) = if let Some((t, d)) = self
            .excursion
            .filter(|(t, _)| *t <= wake && *t <= self.next_rx)
        {
            self.excursion = None;
            (Kind::Completion, Some(d), t)
        } else if self.next_rx < wake {
            self.seq += 1;
            len = self.frame(self.seq, hops.unwrap_or(1 + self.seq % 20), &mut frame);
            let t = self.next_rx;
            self.next_rx = t + time_on_air(len as u8, MIN_PREAMBLE_SYMBOLS);
            (Kind::Received, None, t)
        } else {
            (Kind::Wakeup, None, wake)
        };
        self.now = self.now.max(at);
        let event = match (kind, done) {
            (Kind::Received, _) => Event::Received {
                channel: ch(3),
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
        let ns = self.handle(event);
        (kind, ns)
    }

    /// Handle `event` now, timed, keeping the excursion it asks for.
    fn handle(&mut self, event: Event<'_>) -> u128 {
        let now = self.now;
        let mut asked = None;
        let start = Wall::now();
        self.relay.handle(event, now, &mut |a| match a {
            Action::Lbt(c) => asked = Some((now + CAD_TIME, Done::Lbt(c))),
            Action::Rssi(c) => asked = Some((now, Done::Rssi(c))),
            Action::Transmit(f) => {
                asked = Some((now + time_on_air(f.len, f.preamble_symbols), Done::Tx))
            }
            _ => {}
        });
        let ns = start.elapsed().as_nanos();
        if asked.is_some() {
            assert!(self.excursion.is_none(), "second excursion");
            self.excursion = asked;
        }
        ns
    }

    /// Top the queue up to its whole regular room and `places` hop places,
    /// each new entry with a record, as an arrival would leave it.
    fn top_up(&mut self, places: usize) {
        let template = *self.relay.queue.iter().next().expect("a forward queued");
        let mut k = 0u32;
        let mut add = |b: &mut Bench, place: u8| {
            k += 1;
            let mut payload = [0x5au8; MAX_PAYLOAD];
            payload[..4].copy_from_slice(&(0x8000_0000 | k).to_be_bytes());
            let far = b.far[k as usize % b.far.len()];
            let message = Message::new(far, NodeId([9; 6]), &payload[..b.payload]);
            let r = &mut b.relay;
            let at = r
                .records
                .place(|id| r.queue.iter().any(|o| o.msg_id() == id));
            let (pos, home) = b.neighbours[k as usize % b.neighbours.len()];
            let src = NodeId::from_u64(pos.short().0 as u64);
            let record = Record::new(message.id(), Some((src, home)), (3, 0), b.now);
            r.records.put(at.expect("a record's place"), record);
            let out = Outgoing {
                message,
                state: State::Queued,
                not_before: Instant::ZERO,
                toward: 0,
                ..template
            };
            assert!(r.push_forward(out, place, b.now), "room");
        };
        while self.relay.queue.regular() < SEND_QUEUE {
            add(self, 0);
        }
        while self.relay.queue_use().1 < places {
            let place = self.relay.queue.lowest_free_place().expect("a place");
            add(self, place);
        }
    }
}

/// One run of `secs` simulated seconds: each call's kind, time and the
/// queue's use before it.
fn run(n: usize, payload: usize, hops: Option<Hops>, secs: u64) -> Vec<(Kind, u128, usize)> {
    let mut b = Bench::new(n, payload);
    let end = Instant::ZERO + Duration::from_secs(secs);
    let mut calls = Vec::new();
    b.handle(Event::Wakeup);
    while b.now < end {
        let (r, p) = b.relay.queue_use();
        let (kind, ns) = b.step(hops);
        calls.push((kind, ns, r + p));
    }
    calls
}

/// The median and the highest of `v`.
fn spread(mut v: Vec<u128>) -> (u128, u128) {
    v.sort_unstable();
    (
        v.get(v.len() / 2).copied().unwrap_or(0),
        v.last().copied().unwrap_or(0),
    )
}

#[test]
#[ignore]
fn handle_time() {
    println!("left to itself, 600 s; calls with the queue at its fullest, ns");
    println!(
        "nb    B  count  queue | received n median max | wakeup median max | completion median max"
    );
    for (n, payload, hops) in [
        (3, MAX_PAYLOAD, None),
        (64, MAX_PAYLOAD, None),
        (64, MAX_PAYLOAD, Some(64)),
        (64, 40, None),
        (64, 40, Some(64)),
    ] {
        let mut calls = run(n, payload, hops, 600);
        for _ in 1..REPEATS {
            for (c, a) in calls.iter_mut().zip(run(n, payload, hops, 600)) {
                c.1 = c.1.min(a.1);
            }
        }
        let full = calls.iter().map(|c| c.2).max().unwrap_or(0);
        let of = |k: Kind| {
            spread(
                calls
                    .iter()
                    .filter(|c| c.0 == k && c.2 == full)
                    .map(|c| c.1)
                    .collect(),
            )
        };
        let count = calls
            .iter()
            .filter(|c| c.0 == Kind::Received && c.2 == full)
            .count();
        let (r, w, c) = (of(Kind::Received), of(Kind::Wakeup), of(Kind::Completion));
        let hops = hops.map_or("1-20".into(), |h: Hops| std::format!("{h}"));
        println!(
            "{n:>2} {payload:>4} {hops:>6} {full:>6} | {count:>6} {:>6} {:>6} | {:>6} {:>6} | {:>6} {:>6}",
            r.0, r.1, w.0, w.1, c.0, c.1
        );
    }
    println!("topped up after 120 s; one call, median and shortest of 8, ns");
    println!("nb    B  queue      | received median min | pick median min | all banned median min");
    for (n, payload) in [(64, MAX_PAYLOAD), (64, 40), (3, MAX_PAYLOAD), (3, 40)] {
        for places in [1, 9, 16, 64] {
            let (mut rx, mut pick, mut queue) = (Vec::new(), Vec::new(), (0, 0));
            let mut blocked = Vec::new();
            for i in 0..24 {
                let mut b = Bench::new(n, payload);
                b.handle(Event::Wakeup);
                // Counts of 1, so that the places are those topped up.
                while b.now < Instant::ZERO + Duration::from_secs(120) {
                    b.step(Some(1));
                }
                b.top_up(places);
                queue = b.relay.queue_use();
                if i % 3 == 0 {
                    // What each event costs when no hand-over is under way.
                    let start = Wall::now();
                    core::hint::black_box(b.relay.pick(b.now));
                    pick.push(start.elapsed().as_nanos());
                    continue;
                }
                if i % 3 == 1 {
                    // The same with every neighbour banned: nothing may go,
                    // and the wakeup needs the end of every wait.
                    let until = b.now + Duration::from_secs(60);
                    let ids: Vec<_> = b.relay.neighbours.slots().map(|(_, n)| n.id).collect();
                    for id in ids {
                        b.relay.neighbours.refused(id, until);
                    }
                    let start = Wall::now();
                    let p = core::hint::black_box(b.relay.pick(b.now));
                    blocked.push(start.elapsed().as_nanos());
                    assert_eq!(p, None, "nothing may go");
                    continue;
                }
                let mut frame = [0u8; MAX_FRAME];
                let len = b.frame(1 + 8 * 1000, 4, &mut frame);
                rx.push(b.handle(Event::Received {
                    channel: ch(3),
                    rssi_dbm: -90,
                    snr_db: 8,
                    bytes: &frame[..len],
                }));
            }
            let (r, w, a) = (
                spread(rx.clone()),
                spread(pick.clone()),
                spread(blocked.clone()),
            );
            let min = |v: &Vec<u128>| v.iter().copied().min().unwrap_or(0);
            println!(
                "{n:>2} {payload:>4} {:>3}+{:<3}    | {:>6} {:>6} | {:>6} {:>6} | {:>6} {:>6}",
                queue.0,
                queue.1,
                r.0,
                min(&rx),
                w.0,
                min(&pick),
                a.0,
                min(&blocked)
            );
        }
    }
}
