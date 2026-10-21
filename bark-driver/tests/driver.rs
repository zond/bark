//! The driver loop on the host, against a fake radio and a virtual clock.
//!
//! Time is virtual: every future the loop waits on is a check against the
//! world's clock, and when one poll of a step finds nothing ready the test
//! moves the clock to the earliest moment anything waits for. A step then
//! takes no time of its own, so wakeups and completions land exactly when
//! they are due and any lateness or earliness is the driver's.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::future::{poll_fn, Future};
use std::pin::pin;
use std::rc::Rc;
use std::task::Poll;

use bark_driver::{App, Clock, Driver, Heard, Outgoing, Radio, MAX_AIRTIME, RSSI_DWELL};
use bark_proto::radio::{time_on_air, Channel, MIN_PREAMBLE_SYMBOLS, SYMBOL_TIME};
use bark_proto::wire::{Heartbeat, Packet, MAX_PAYLOAD};
use bark_proto::{
    Action, AppEvent, Config, Core, Duration, Event, Frame, Instant, Position, Relay, MAX_FRAME,
};
use embassy_futures::poll_once;

/// CAD time at SF5 with the driver's 8 symbols, plus half a symbol of
/// processing (SX1261/2 DS §6.1.5).
const CAD_TIME: Duration = Duration::from_micros(17 * SYMBOL_TIME.as_micros() / 2);

/// What the fake radio was asked to do. A frame by value, as the core's
/// `Action::Transmit` carries it.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
enum Op {
    Listen(Channel),
    Sniff(Channel),
    Cad(Channel),
    Rssi(Channel),
    Transmit(Frame),
}

/// A frame on the air, heard if the radio listens on its channel from its
/// start to its end.
struct OnAir {
    start: Instant,
    end: Instant,
    channel: Channel,
    bytes: Vec<u8>,
    started: bool,
}

#[derive(Default)]
struct Inner {
    now: Instant,
    /// The earliest moment a future polled since the last move waits for.
    wanted: Option<Instant>,
    air: VecDeque<OnAir>,
    listening: Option<Channel>,
    listening_since: Instant,
    /// The radio receives duty-cycled, and stops at the end of a packet.
    sniffing: bool,
    /// The last frame completed and not yet read.
    ready: Option<(Channel, Vec<u8>)>,
    started: bool,
    lost: u32,
    ops: Vec<(Instant, Op)>,
}

#[derive(Clone, Default)]
struct World(Rc<RefCell<Inner>>);

impl World {
    fn now(&self) -> Instant {
        self.0.borrow().now
    }

    fn want(&self, t: Instant) {
        let mut w = self.0.borrow_mut();
        w.wanted = Some(w.wanted.map_or(t, |u| u.min(t)));
    }

    fn sleep(&self, t: Instant) -> impl Future<Output = ()> + '_ {
        poll_fn(move |_| {
            if self.now() >= t {
                Poll::Ready(())
            } else {
                self.want(t);
                Poll::Pending
            }
        })
    }

    /// Move the clock to the earliest moment anything waits for.
    fn advance(&self) {
        let mut w = self.0.borrow_mut();
        let t = w.wanted.take().expect("nothing left to happen");
        assert!(t > w.now, "a future waits for the past");
        w.now = t;
    }

    /// Put a frame on the air that ends at `end`.
    fn send(&self, end: Instant, channel: Channel, bytes: &[u8]) {
        let airtime = time_on_air(bytes.len() as u8, MIN_PREAMBLE_SYMBOLS);
        let mut w = self.0.borrow_mut();
        w.air.push_back(OnAir {
            start: Instant::from_micros(end.as_micros() - airtime.as_micros()),
            end,
            channel,
            bytes: bytes.to_vec(),
            started: false,
        });
        w.air.make_contiguous().sort_by_key(|a| a.end);
    }

    fn ops(&self) -> Vec<(Instant, Op)> {
        self.0.borrow().ops.clone()
    }

    fn op(&self, op: Op) {
        let mut w = self.0.borrow_mut();
        let now = w.now;
        w.ops.push((now, op));
    }
}

struct FakeClock {
    world: World,
    /// Fire this much before the instant asked for, once per instant, as a
    /// timer rounding down to its tick would.
    early: Duration,
    fired_early: std::cell::Cell<Option<Instant>>,
}

impl Clock for FakeClock {
    fn now(&self) -> Instant {
        self.world.now()
    }

    async fn at(&self, t: Instant) {
        if self.fired_early.get() != Some(t) {
            let early = Instant::from_micros(t.as_micros().saturating_sub(self.early.as_micros()));
            self.world.sleep(early).await;
            self.fired_early.set(Some(t));
            if self.world.now() < t {
                return;
            }
        }
        self.world.sleep(t).await
    }
}

struct FakeRadio {
    world: World,
    cad_busy: bool,
}

impl FakeRadio {
    /// Bring the air up to now: what started, what ended and was heard.
    /// True if there is something to report.
    fn poll_air(&self) -> bool {
        let mut w = self.world.0.borrow_mut();
        let now = w.now;
        let mut report = w.ready.is_some() || w.started;
        let mut next = None;
        let mut i = 0;
        while i < w.air.len() {
            let (start, end, channel, started) = (
                w.air[i].start,
                w.air[i].end,
                w.air[i].channel,
                w.air[i].started,
            );
            let heard = w.listening == Some(channel) && w.listening_since <= start;
            if end <= now {
                let a = w.air.remove(i).expect("in range");
                if heard {
                    // The radio keeps only the last frame (SX1261/2 DS
                    // §13.5.2, GetRxBufferStatus).
                    w.ready = Some((a.channel, a.bytes));
                    w.started = false;
                    report = true;
                    if w.sniffing {
                        // Duty-cycled receive ends in standby (DS §13.1.7).
                        w.listening = None;
                        w.sniffing = false;
                    }
                } else {
                    w.lost += 1;
                }
                continue;
            }
            if heard && start <= now && !started {
                w.air[i].started = true;
                w.started = true;
                report = true;
            }
            let at = if start > now { start } else { end };
            next = Some(next.map_or(at, |n: Instant| n.min(at)));
            i += 1;
        }
        drop(w);
        if let Some(t) = next {
            self.world.want(t);
        }
        report
    }

    fn deaf(&self) {
        let mut w = self.world.0.borrow_mut();
        w.listening = None;
        w.sniffing = false;
        w.started = false;
    }

    fn rx(&self, channel: Channel, sniffing: bool) {
        let mut w = self.world.0.borrow_mut();
        w.listening = Some(channel);
        w.listening_since = w.now;
        w.sniffing = sniffing;
        w.started = false;
    }
}

impl Radio for FakeRadio {
    type Error = ();

    async fn listen(&mut self, channel: Channel) -> Result<(), ()> {
        self.world.op(Op::Listen(channel));
        self.rx(channel, false);
        Ok(())
    }

    // Hears every frame: which ones a cycle catches is the chip's affair
    // (`bark_proto::sniff::detects`); the driver's is to keep it running.
    async fn sniff(&mut self, channel: Channel) -> Result<(), ()> {
        self.world.op(Op::Sniff(channel));
        self.rx(channel, true);
        Ok(())
    }

    async fn wait(&mut self) -> Result<(), ()> {
        poll_fn(|_| match self.poll_air() {
            true => Poll::Ready(Ok(())),
            false => Poll::Pending,
        })
        .await
    }

    async fn service(&mut self, buf: &mut [u8; MAX_FRAME]) -> Result<Heard, ()> {
        let mut w = self.world.0.borrow_mut();
        if let Some((_, bytes)) = w.ready.take() {
            buf[..bytes.len()].copy_from_slice(&bytes);
            return Ok(Heard::Frame {
                len: bytes.len() as u8,
                rssi_dbm: -90,
                snr_db: 8,
            });
        }
        if std::mem::take(&mut w.started) {
            return Ok(Heard::Started);
        }
        Ok(Heard::Nothing)
    }

    async fn cad(&mut self, channel: Channel) -> Result<bool, ()> {
        self.world.op(Op::Cad(channel));
        self.deaf();
        self.world.sleep(self.world.now() + CAD_TIME).await;
        Ok(self.cad_busy)
    }

    async fn rssi(&mut self) -> Result<i16, ()> {
        let ch = self.world.0.borrow().listening.expect("listening");
        self.world.op(Op::Rssi(ch));
        Ok(-120)
    }

    async fn transmit(&mut self, frame: &Frame, _dbm: i8) -> Result<(), ()> {
        self.world.op(Op::Transmit(*frame));
        self.deaf();
        let airtime = time_on_air(frame.len, frame.preamble_symbols);
        self.world.sleep(self.world.now() + airtime).await;
        Ok(())
    }
}

#[derive(Default)]
struct TestApp {
    outgoing: Option<(Outgoing, Vec<u8>)>,
    messages: Vec<AppEvent>,
}

impl App for TestApp {
    async fn ready(&mut self) {
        poll_fn(|_| match self.outgoing.is_some() {
            true => Poll::Ready(()),
            false => Poll::Pending,
        })
        .await
    }

    fn take(&mut self, payload: &mut [u8; MAX_PAYLOAD]) -> Option<Outgoing> {
        let (out, bytes) = self.outgoing.take()?;
        payload[..bytes.len()].copy_from_slice(&bytes);
        Some(out)
    }

    fn message(&mut self, event: &AppEvent) {
        self.messages.push(*event);
    }
}

type TestDriver<C> = Driver<C, FakeRadio, FakeClock, TestApp>;

fn driver<C: Core>(core: C, world: &World, early: Duration) -> TestDriver<C> {
    let radio = FakeRadio {
        world: world.clone(),
        cad_busy: false,
    };
    let clock = FakeClock {
        world: world.clone(),
        early,
        fired_early: Default::default(),
    };
    Driver::new(core, radio, clock, TestApp::default(), 10)
}

/// One step, moving the clock whenever the step waits.
fn step<C: Core>(d: &mut TestDriver<C>, world: &World) {
    let mut fut = pin!(d.step());
    loop {
        match poll_once(fut.as_mut()) {
            Poll::Ready(r) => return r.expect("the fake radio does not fail"),
            Poll::Pending => world.advance(),
        }
    }
}

fn run_until<C: Core>(d: &mut TestDriver<C>, world: &World, until: Instant) {
    while world.now() < until {
        step(d, world);
    }
}

fn ch(k: u8) -> Channel {
    Channel::new(k).expect("in the plan")
}

fn at_ms(ms: u64) -> Instant {
    Instant::ZERO + Duration::from_millis(ms)
}

/// A core that does what a test tells it: wakes at the instants in
/// `wakeups`, transmits on the wakeups listed in `send`, checks the
/// channel on those listed in `check`, emits the actions listed in `then`
/// on theirs, and records every event.
struct Scripted {
    home: Channel,
    wakeups: VecDeque<Instant>,
    send: Vec<Instant>,
    check: Vec<Instant>,
    then: Vec<(Instant, Action)>,
    seen: Vec<(Instant, String)>,
}

impl Scripted {
    fn new(home: Channel, wakeups: &[Instant]) -> Scripted {
        Scripted {
            home,
            wakeups: wakeups.iter().copied().collect(),
            send: Vec::new(),
            check: Vec::new(),
            then: Vec::new(),
            seen: Vec::new(),
        }
    }
}

fn frame(channel: Channel, bytes: &[u8]) -> Frame {
    let mut f = Frame {
        channel,
        preamble_symbols: MIN_PREAMBLE_SYMBOLS,
        len: bytes.len() as u8,
        bytes: [0; MAX_FRAME],
    };
    f.bytes[..bytes.len()].copy_from_slice(bytes);
    f
}

impl Core for Scripted {
    fn handle(&mut self, event: Event<'_>, now: Instant, emit: &mut dyn FnMut(Action)) {
        let text = match event {
            Event::Received { channel, bytes, .. } => format!("rx {} {bytes:?}", channel.index()),
            Event::Wakeup if self.seen.is_empty() => {
                emit(Action::Listen(self.home));
                "boot".into()
            }
            Event::Wakeup => {
                let due = self.wakeups.pop_front().expect("a wakeup was due");
                assert!(now >= due, "woken at {now:?}, before {due:?}");
                if self.send.contains(&due) {
                    emit(Action::Transmit(frame(ch(5), b"out")));
                }
                if self.check.contains(&due) {
                    emit(Action::Cad(self.home));
                }
                for (_, a) in self.then.iter().filter(|(t, _)| *t == due) {
                    emit(*a);
                }
                "wakeup".into()
            }
            Event::TxDone => "tx done".into(),
            Event::CadDone { busy, .. } => format!("cad busy={busy}"),
            other => format!("{other:?}"),
        };
        self.seen.push((now, text));
    }

    fn next_wakeup(&self) -> Option<Instant> {
        self.wakeups.front().copied()
    }

    fn accepts_send(&self, _: Instant) -> bool {
        false
    }
}

fn seen(d: &TestDriver<Scripted>) -> Vec<(u64, &str)> {
    d.core()
        .seen
        .iter()
        .map(|(t, s)| (t.as_micros(), s.as_str()))
        .collect()
}

/// Wakeups reach the core at the instant it asked for: not before, even on
/// a clock whose timer fires early, and not later on an exact one.
#[test]
fn wakeups_fire_when_due() {
    for early in [Duration::ZERO, Duration::from_micros(30)] {
        let world = World::default();
        let core = Scripted::new(ch(3), &[at_ms(1), at_ms(7), at_ms(250)]);
        let mut d = driver(core, &world, early);
        run_until(&mut d, &world, at_ms(250));
        assert_eq!(
            seen(&d),
            [
                (0, "boot"),
                (1_000, "wakeup"),
                (7_000, "wakeup"),
                (250_000, "wakeup")
            ],
            "timer early by {early:?}"
        );
        assert_eq!(world.ops(), [(Instant::ZERO, Op::Listen(ch(3)))]);
    }
}

/// A frame heard on the resting channel reaches the core when it ends, with
/// its channel and bytes; one on another channel does not.
#[test]
fn a_received_frame_reaches_the_core() {
    let world = World::default();
    let core = Scripted::new(ch(3), &[at_ms(100)]);
    let mut d = driver(core, &world, Duration::ZERO);
    world.send(at_ms(40), ch(3), &[1, 2, 3]);
    world.send(at_ms(60), ch(4), &[9]);
    run_until(&mut d, &world, at_ms(100));
    assert_eq!(
        seen(&d),
        [(0, "boot"), (40_000, "rx 3 [1, 2, 3]"), (100_000, "wakeup")]
    );
    assert_eq!(d.stats().frames, 1);
    assert_eq!(world.0.borrow().lost, 1, "the other channel's frame");
}

/// A transmit goes out on the frame's channel, the core hears `TxDone`
/// one airtime later, and the radio is back on the resting channel by then.
#[test]
fn a_transmit_goes_out_and_completes() {
    let world = World::default();
    let mut core = Scripted::new(ch(3), &[at_ms(10), at_ms(500)]);
    core.send.push(at_ms(10));
    let mut d = driver(core, &world, Duration::ZERO);
    run_until(&mut d, &world, at_ms(500));
    let airtime = time_on_air(3, MIN_PREAMBLE_SYMBOLS);
    let done = at_ms(10) + airtime;
    assert_eq!(
        seen(&d),
        [
            (0, "boot"),
            (10_000, "wakeup"),
            (done.as_micros(), "tx done"),
            (500_000, "wakeup")
        ]
    );
    assert_eq!(
        world.ops(),
        [
            (Instant::ZERO, Op::Listen(ch(3))),
            (at_ms(10), Op::Transmit(frame(ch(5), b"out"))),
            (done, Op::Listen(ch(3))),
        ]
    );
}

/// A check asked for while a packet is arriving waits for it to end, the
/// frame reaches the core first, and the check runs after (core-interface
/// spec "Regulatory limits" rule 8).
#[test]
fn a_check_waits_for_the_packet_in_progress() {
    let world = World::default();
    let mut core = Scripted::new(ch(3), &[at_ms(50), at_ms(400)]);
    core.check.push(at_ms(50));
    let mut d = driver(core, &world, Duration::ZERO);
    let bytes = [7u8; 40];
    let end = at_ms(60);
    world.send(end, ch(3), &bytes);
    assert!(end.since(at_ms(50)) < time_on_air(40, MIN_PREAMBLE_SYMBOLS));
    run_until(&mut d, &world, at_ms(400));
    let cad_done = end + CAD_TIME;
    assert_eq!(
        seen(&d),
        [
            (0, "boot"),
            (50_000, "wakeup"),
            (60_000, format!("rx 3 {:?}", bytes.to_vec()).as_str()),
            (cad_done.as_micros(), "cad busy=false"),
            (400_000, "wakeup"),
        ]
    );
    assert_eq!(world.ops()[1], (end, Op::Cad(ch(3))));
    assert_eq!(d.stats().held, 1);
}

/// A packet that never ends holds a check for at most one longest frame.
#[test]
fn a_hold_is_bounded() {
    let world = World::default();
    let mut core = Scripted::new(ch(3), &[at_ms(50), at_ms(1000)]);
    core.check.push(at_ms(50));
    let mut d = driver(core, &world, Duration::ZERO);
    // A long-preamble packet still arriving well after the hold's bound.
    let bytes = [7u8; 40];
    let end = at_ms(700);
    world.0.borrow_mut().air.push_back(OnAir {
        start: at_ms(20),
        end,
        channel: ch(3),
        bytes: bytes.to_vec(),
        started: false,
    });
    run_until(&mut d, &world, at_ms(1000));
    assert_eq!(world.ops()[1], (at_ms(50) + MAX_AIRTIME, Op::Cad(ch(3))));
    assert_eq!(world.0.borrow().lost, 1, "cut short by the check");
}

fn relay_config(position: Position) -> Config {
    Config {
        position,
        home_override: Some(ch(3)),
        boot_listen: Duration::from_secs(1),
        ..Config::default()
    }
}

/// A relay through the loop: it boots on its home channel, takes a
/// neighbour's heartbeat into its table, sweeps the channels, sends its own
/// heartbeat, and delivers its application's message to itself.
#[test]
fn a_relay_runs_through_the_loop() {
    let world = World::default();
    let me = Position::from_degrees(50.45, 30.52);
    let relay = Relay::new(relay_config(me), 7);
    let my_id = relay.id();
    let mut d = driver(relay, &world, Duration::ZERO);
    d.app_mut().outgoing = Some((
        Outgoing {
            dst_pos: me,
            dst_id: my_id,
            len: 5,
        },
        b"hello".to_vec(),
    ));
    // It sweeps at once, and rests on its home after the first reading.
    step(&mut d, &world);
    step(&mut d, &world);
    assert_eq!(
        world.ops().last().map(|o| o.1.clone()),
        Some(Op::Listen(ch(3)))
    );

    let other = Position::from_degrees(50.46, 30.52);
    let hb = Packet::Heartbeat(Heartbeat {
        sleepy: false,
        anchorless: false,
        carried: false,
        home: ch(6),
        id: bark_proto::NodeId([6; 4]),
        short: other.short(),
    });
    let mut buf = [0u8; MAX_FRAME];
    let len = hb.encode(&mut buf).expect("encodes");
    world.send(at_ms(500), ch(3), &buf[..len]);

    run_until(&mut d, &world, Instant::ZERO + Duration::from_secs(20));
    let relay = d.core();
    assert!(!relay.booting());
    assert!(
        relay.neighbours().any(|n| n.position == other),
        "the heartbeat reached the table"
    );
    let ops = world.ops();
    let sweeps = ops
        .iter()
        .filter(|(_, op)| matches!(op, Op::Rssi(_)))
        .count();
    assert!(sweeps > 13, "{sweeps} RSSI readings");
    let beats: Vec<_> = ops
        .iter()
        .filter_map(|(t, op)| match op {
            Op::Transmit(f) => match Packet::decode(f.payload()) {
                Ok(Packet::Heartbeat(h)) if h.position(me) == me => Some(*t),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert!(!beats.is_empty(), "no heartbeat sent: {ops:?}");
    // Every off-channel reading dwells before it is read.
    for w in ops.windows(2) {
        if let [(t0, Op::Listen(a)), (t1, Op::Rssi(b))] = w {
            if a == b && *a != ch(3) {
                assert_eq!(t1.since(*t0), RSSI_DWELL);
            }
        }
    }
    assert!(
        d.app()
            .messages
            .iter()
            .any(|e| matches!(e, AppEvent::Message { payload, len, .. } if &payload[..*len as usize] == b"hello")),
        "its own message came back: {:?}",
        d.app().messages
    );
    assert!(d.app().outgoing.is_none(), "taken");
}

/// A script that sniffs from 10 ms to `until` on channel 3 and wakes at
/// `wakeups` besides.
fn sniffing(until: Instant, wakeups: &[Instant]) -> Scripted {
    let mut all = vec![at_ms(10)];
    all.extend_from_slice(wakeups);
    let mut core = Scripted::new(ch(3), &all);
    core.then.push((at_ms(10), Action::Sniff(until)));
    core
}

/// `Sniff` starts duty-cycled receive on the resting channel, and at its
/// end the radio listens again, without the core.
#[test]
fn a_sniff_runs_until_its_end() {
    let world = World::default();
    let mut d = driver(sniffing(at_ms(200), &[at_ms(500)]), &world, Duration::ZERO);
    run_until(&mut d, &world, at_ms(500));
    assert_eq!(
        world.ops(),
        [
            (Instant::ZERO, Op::Listen(ch(3))),
            (at_ms(10), Op::Sniff(ch(3))),
            (at_ms(200), Op::Listen(ch(3))),
        ]
    );
    assert_eq!(
        seen(&d),
        [(0, "boot"), (10_000, "wakeup"), (500_000, "wakeup")]
    );
    assert_eq!(d.stats().sniffs, 1);
}

/// The radio stops duty-cycled receive with each frame it catches; the
/// driver hands the frame to the core and starts the cycle again, so the
/// next frame is caught too.
#[test]
fn a_sniff_starts_again_after_each_frame() {
    let world = World::default();
    let mut d = driver(sniffing(at_ms(300), &[]), &world, Duration::ZERO);
    world.send(at_ms(50), ch(3), &[1]);
    world.send(at_ms(120), ch(3), &[2]);
    run_until(&mut d, &world, at_ms(300));
    assert_eq!(
        seen(&d),
        [
            (0, "boot"),
            (10_000, "wakeup"),
            (50_000, "rx 3 [1]"),
            (120_000, "rx 3 [2]")
        ]
    );
    assert_eq!(
        world.ops(),
        [
            (Instant::ZERO, Op::Listen(ch(3))),
            (at_ms(10), Op::Sniff(ch(3))),
            (at_ms(50), Op::Sniff(ch(3))),
            (at_ms(120), Op::Sniff(ch(3))),
            (at_ms(300), Op::Listen(ch(3))),
        ]
    );
    assert_eq!(world.0.borrow().lost, 0);
}

/// The core's next `Listen`, `Cad`, `Rssi` or `Transmit` ends a sniff
/// before its end; the radio is carried out as ever and listens after.
#[test]
fn the_next_radio_action_ends_a_sniff() {
    let t = at_ms(100);
    let cad_done = t + CAD_TIME;
    let tx_done = t + time_on_air(3, MIN_PREAMBLE_SYMBOLS);
    let cases = [
        (
            Action::Cad(ch(3)),
            vec![(t, Op::Cad(ch(3))), (cad_done, Op::Listen(ch(3)))],
        ),
        (
            Action::Transmit(frame(ch(5), b"out")),
            vec![
                (t, Op::Transmit(frame(ch(5), b"out"))),
                (tx_done, Op::Listen(ch(3))),
            ],
        ),
        (Action::Listen(ch(4)), vec![(t, Op::Listen(ch(4)))]),
        // On the resting channel: listening, then the reading once the
        // receiver has settled.
        (
            Action::Rssi(ch(3)),
            vec![(t, Op::Listen(ch(3))), (t + RSSI_DWELL, Op::Rssi(ch(3)))],
        ),
    ];
    for (action, after) in cases {
        let world = World::default();
        let mut core = sniffing(at_ms(5000), &[t, at_ms(400)]);
        core.then.push((t, action));
        let mut d = driver(core, &world, Duration::ZERO);
        run_until(&mut d, &world, at_ms(400));
        let mut ops = vec![
            (Instant::ZERO, Op::Listen(ch(3))),
            (at_ms(10), Op::Sniff(ch(3))),
        ];
        ops.extend(after);
        assert_eq!(world.ops(), ops, "{action:?}");
    }
}

/// A sniff that ends while a packet arrives keeps the radio as it is
/// until the packet is over, so the packet is not cut short.
#[test]
fn a_sniff_ends_after_the_packet_under_way() {
    let world = World::default();
    let mut d = driver(sniffing(at_ms(200), &[at_ms(500)]), &world, Duration::ZERO);
    world.0.borrow_mut().air.push_back(OnAir {
        start: at_ms(180),
        end: at_ms(240),
        channel: ch(3),
        bytes: vec![7],
        started: false,
    });
    run_until(&mut d, &world, at_ms(500));
    assert!(seen(&d).contains(&(240_000, "rx 3 [7]")), "{:?}", seen(&d));
    assert_eq!(
        world.ops(),
        [
            (Instant::ZERO, Op::Listen(ch(3))),
            (at_ms(10), Op::Sniff(ch(3))),
            (at_ms(240), Op::Listen(ch(3))),
        ]
    );
}

/// A packet that begins in a window and never ends may leave the radio
/// stuck receiving: after the longest frame the cycle starts again.
#[test]
fn a_packet_that_never_ends_starts_the_cycle_again() {
    let world = World::default();
    let mut d = driver(sniffing(at_ms(5000), &[]), &world, Duration::ZERO);
    world.0.borrow_mut().air.push_back(OnAir {
        start: at_ms(20),
        end: at_ms(3000),
        channel: ch(3),
        bytes: vec![7],
        started: false,
    });
    run_until(&mut d, &world, at_ms(5000));
    assert_eq!(
        world.ops(),
        [
            (Instant::ZERO, Op::Listen(ch(3))),
            (at_ms(10), Op::Sniff(ch(3))),
            (at_ms(20) + MAX_AIRTIME, Op::Sniff(ch(3))),
            (at_ms(5000), Op::Listen(ch(3))),
        ]
    );
    assert_eq!(world.0.borrow().lost, 1);
}

/// A check that ends a sniff while a packet arrives is held for it as
/// ever, and the loop waits, without spinning, until the hold's bound.
#[test]
fn a_check_that_ends_a_sniff_waits_for_the_packet() {
    let world = World::default();
    let mut core = sniffing(at_ms(5000), &[at_ms(50), at_ms(1000)]);
    core.check.push(at_ms(50));
    let mut d = driver(core, &world, Duration::ZERO);
    world.0.borrow_mut().air.push_back(OnAir {
        start: at_ms(20),
        end: at_ms(700),
        channel: ch(3),
        bytes: vec![7],
        started: false,
    });
    let mut steps = 0;
    while world.now() < at_ms(1000) {
        step(&mut d, &world);
        steps += 1;
        assert!(steps < 100, "the loop spins");
    }
    assert_eq!(world.ops()[2], (at_ms(50) + MAX_AIRTIME, Op::Cad(ch(3))));
}
