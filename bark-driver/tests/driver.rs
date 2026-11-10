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

use bark_driver::{
    App, Clock, Driver, Gps, Heard, Outgoing, Radio, Ready, Request, MAX_AIRTIME, RSSI_DWELL,
};
use bark_proto::radio::{time_on_air, Channel, MIN_PREAMBLE_SYMBOLS, SYMBOL_TIME};
use bark_proto::update::{Image, Piece};
use bark_proto::wire::{
    Ack, Data, Heartbeat, Packet, Update, MAX_PAYLOAD, PIECE_BYTES, PIECE_HEADER_LEN,
};
use bark_proto::{
    Action, AppEvent, Config, Core, Duration, Event, Frame, Instant, MsgId, NodeId, Position,
    Relay, Stored, MAX_FRAME,
};
use embassy_futures::poll_once;

/// A listen-before-talk's CAD time at SF5 with the driver's 8 symbols,
/// plus half a symbol of processing (SX1261/2 DS §6.1.5).
const CAD_TIME: Duration = Duration::from_micros(17 * SYMBOL_TIME.as_micros() / 2);

/// What the fake radio was asked to do. A frame by value, as the core's
/// `Action::Transmit` carries it.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
enum Op {
    Listen(Channel),
    Sniff(Channel),
    Lbt(Channel),
    Rssi(Channel),
    Transmit(Frame),
    Recover,
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

    async fn lbt(&mut self, channel: Channel) -> Result<(bool, i16), ()> {
        self.world.op(Op::Lbt(channel));
        self.deaf();
        self.world.sleep(self.world.now() + CAD_TIME).await;
        Ok((self.cad_busy, climb(CAD_TIME)))
    }

    async fn rssi(&mut self) -> Result<i16, ()> {
        let (ch, since) = {
            let w = self.world.0.borrow();
            (w.listening.expect("listening"), w.listening_since)
        };
        self.world.op(Op::Rssi(ch));
        Ok(climb(self.world.now().since(since)))
    }

    async fn transmit(&mut self, frame: &Frame, _dbm: i8) -> Result<(), ()> {
        self.world.op(Op::Transmit(*frame));
        self.deaf();
        let airtime = time_on_air(frame.len, frame.preamble_symbols);
        self.world.sleep(self.world.now() + airtime).await;
        Ok(())
    }

    fn recover(&mut self) {
        self.world.op(Op::Recover);
        self.deaf();
    }
}

/// What the SX1262 reads on a quiet channel against the time since its
/// receiver started (docs/bench-2026-10-30.md, "The night alone, and two
/// boards the next morning"): nothing, then a step a symbol period up to
/// the settled level.
fn climb(since: Duration) -> i16 {
    match since.as_micros() {
        0..150 => -128,
        150..400 => -90,
        400..700 => -83,
        _ => -80,
    }
}

#[derive(Default)]
struct TestApp {
    outgoing: Option<(Outgoing, Vec<u8>)>,
    messages: Vec<AppEvent>,
    /// The power the application tells, and the one the driver last read.
    power: Option<bool>,
    read: Option<bool>,
    /// What the core last asked to keep.
    stored: Option<Stored>,
    /// The battery reading the application gives, and how many times the
    /// driver asked for one.
    battery: Option<u16>,
    battery_reads: u32,
    /// Requests waiting, and what the driver said of the GPS, in order.
    requests: VecDeque<Request>,
    gps: Vec<Gps>,
    /// A signal raised and not yet seen by `ready`.
    signalled: bool,
    /// `ready` resolves at once whether anything waits or not, against
    /// its contract, as the node's did while its mailbox said a reply was
    /// due that it then did not give; and how many times it resolved.
    eager: bool,
    readies: u32,
    /// Work of its own the loop that runs the driver serves after the
    /// step, as the node does a phone's read of the neighbour table.
    own: bool,
    /// What it queues, without a signal, when the core hands it a
    /// message, as a home's mailbox does a reply to a fetch.
    reply: Option<(Outgoing, Vec<u8>)>,
    /// The images offered and the pieces handed to it, and the byte it
    /// fills a piece served with, `None` if it cannot.
    offered: Vec<Image>,
    pieces: Vec<Piece>,
    fill: Option<u8>,
}

impl TestApp {
    fn signal(&mut self) {
        self.signalled = true;
    }

    fn set_power(&mut self, power: bool) {
        self.power = Some(power);
        self.signal();
    }

    fn push_request(&mut self, request: Request) {
        self.requests.push_back(request);
        self.signal();
    }
}

impl App for TestApp {
    /// At a signal, resolves if the driver gets something, and otherwise
    /// takes the signal and waits on.
    async fn ready(&mut self, offer: bool) -> Ready {
        let ready = poll_fn(|_| {
            let signalled = core::mem::take(&mut self.signalled);
            let new = signalled
                && ((offer && self.outgoing.is_some())
                    || self.power != self.read
                    || !self.requests.is_empty());
            match (self.eager || new, signalled && self.own) {
                (true, _) => Poll::Ready(Ready::Driver),
                (false, true) => Poll::Ready(Ready::Own),
                (false, false) => Poll::Pending,
            }
        })
        .await;
        self.readies += 1;
        ready
    }

    fn power(&mut self) -> Option<bool> {
        self.read = self.power;
        self.power
    }

    async fn battery(&mut self) -> Option<u16> {
        self.battery_reads += 1;
        self.battery
    }

    fn request(&mut self) -> Option<Request> {
        self.requests.pop_front()
    }

    fn take(&mut self, payload: &mut [u8; MAX_PAYLOAD]) -> Option<Outgoing> {
        let (out, bytes) = self.outgoing.take()?;
        payload[..bytes.len()].copy_from_slice(&bytes);
        Some(out)
    }

    fn message(&mut self, event: &AppEvent) {
        self.messages.push(*event);
        if let AppEvent::Message { .. } = event {
            self.outgoing = self.outgoing.take().or(self.reply.take());
        }
    }

    fn store(&mut self, stored: &Stored) {
        self.stored = Some(*stored);
    }

    fn gps(&mut self, gps: Gps) {
        self.gps.push(gps);
    }

    fn offered(&mut self, image: &Image) {
        self.offered.push(*image);
    }

    fn store_piece(&mut self, piece: &Piece) {
        self.pieces.push(*piece);
    }

    async fn serve(&mut self, frame: &mut Frame) -> bool {
        let Some(b) = self.fill else {
            return false;
        };
        frame.bytes[PIECE_HEADER_LEN..usize::from(frame.len)].fill(b);
        true
    }
}

type TestDriver<C, R = FakeRadio> = Driver<C, R, FakeClock, TestApp>;

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
fn step<C: Core, R: Radio<Error = ()>>(d: &mut TestDriver<C, R>, world: &World) {
    let mut fut = pin!(d.step());
    loop {
        match poll_once(fut.as_mut()) {
            Poll::Ready(r) => return r.expect("the fake radio does not fail"),
            Poll::Pending => world.advance(),
        }
    }
}

fn run_until<C: Core, R: Radio<Error = ()>>(
    d: &mut TestDriver<C, R>,
    world: &World,
    until: Instant,
) {
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
    /// From when its GPS tracks, and fixes beyond reach of what it knows.
    tracks_from: Option<Instant>,
    beyond: Vec<Position>,
    /// Whether it accepts a message from the application, and from when
    /// it does in any case.
    accepts: bool,
    accepts_from: Option<Instant>,
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
            tracks_from: None,
            beyond: Vec::new(),
            accepts: false,
            accepts_from: None,
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
                    emit(Action::Lbt(self.home));
                }
                for (_, a) in self.then.iter().filter(|(t, _)| *t == due) {
                    emit(*a);
                }
                "wakeup".into()
            }
            Event::TxDone => "tx done".into(),
            Event::LbtDone { busy, dbm, .. } => format!("lbt busy={busy} dbm={dbm}"),
            other => format!("{other:?}"),
        };
        self.seen.push((now, text));
    }

    fn next_wakeup(&self) -> Option<Instant> {
        self.wakeups.front().copied()
    }

    fn accepts_send(&self, now: Instant) -> bool {
        self.accepts || self.accepts_from.is_some_and(|t| now >= t)
    }

    fn inbox(&self) -> Option<(MsgId, &[u8])> {
        None
    }

    fn inbox_len(&self) -> usize {
        0
    }

    fn confirm(&mut self, _: MsgId) -> bool {
        false
    }

    fn gps_tracks(&self) -> bool {
        let now = self.seen.last().map(|(t, _)| *t);
        self.tracks_from.is_some_and(|t| now >= Some(t))
    }

    fn fix_in_reach(&self, position: Position) -> bool {
        !self.beyond.contains(&position)
    }
}

fn seen<R: Radio>(d: &TestDriver<Scripted, R>) -> Vec<(u64, &str)> {
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

/// The node the deaf radio below drops the frames of.
const DEAF: NodeId = NodeId([0xba, 0xe6, 0xe6, 0xcb, 0xf4, 0xf5]);

/// A bench build's radio (`bark_driver::BENCH_DEAF_TO`): the fake radio,
/// deaf to [`DEAF`].
struct DeafRadio(FakeRadio);

impl Radio for DeafRadio {
    type Error = ();
    const DEAF_TO: Option<NodeId> = Some(DEAF);

    async fn listen(&mut self, channel: Channel) -> Result<(), ()> {
        self.0.listen(channel).await
    }

    async fn sniff(&mut self, channel: Channel) -> Result<(), ()> {
        self.0.sniff(channel).await
    }

    async fn wait(&mut self) -> Result<(), ()> {
        self.0.wait().await
    }

    async fn service(&mut self, buf: &mut [u8; MAX_FRAME]) -> Result<Heard, ()> {
        self.0.service(buf).await
    }

    async fn lbt(&mut self, channel: Channel) -> Result<(bool, i16), ()> {
        self.0.lbt(channel).await
    }

    async fn rssi(&mut self) -> Result<i16, ()> {
        self.0.rssi().await
    }

    async fn transmit(&mut self, frame: &Frame, dbm: i8) -> Result<(), ()> {
        self.0.transmit(frame, dbm).await
    }
}

/// A radio deaf to one node passes the core none of its frames, a
/// heartbeat, a DATA or an ACK, and counts none of them, while every other
/// node's frame and a frame that names no sender reach the core.
#[test]
fn a_deaf_radio_drops_every_frame_of_its_node() {
    let world = World::default();
    let core = Scripted::new(ch(3), &[at_ms(600)]);
    let radio = DeafRadio(FakeRadio {
        world: world.clone(),
        cad_busy: false,
    });
    let clock = FakeClock {
        world: world.clone(),
        early: Duration::ZERO,
        fired_early: Default::default(),
    };
    let mut d = Driver::new(core, radio, clock, TestApp::default(), 10);
    let other = NodeId([1, 2, 3, 4, 5, 6]);
    let encode = |p: Packet<'_>| {
        let mut buf = [0; MAX_FRAME];
        let len = p.encode(&mut buf).expect("encodes");
        buf[..len].to_vec()
    };
    let heartbeat = |id| {
        encode(Packet::Heartbeat(Heartbeat {
            sleepy: false,
            anchorless: false,
            carried: false,
            home: ch(3),
            format: bark_proto::wire::FORMAT,
            map_point: false,
            leaving: false,
            id,
            short: Position::default().short(),
        }))
    };
    let data = encode(Packet::Data(Data {
        sender_home: ch(3),
        dst_hop: other,
        src_hop: DEAF,
        dst_pos: Position::default(),
        dst_id: other,
        hops: 1,
        payload: b"text",
    }));
    let ack = encode(Packet::Ack(Ack {
        msg_id: MsgId(7),
        from: DEAF,
        to: other,
        refused: false,
        wait: 0,
        seen: false,
    }));
    let heard = heartbeat(other);
    let bench = [bark_proto::wire::BENCH_FIRST, 1, 0];
    for (end, bytes) in [
        (100, heartbeat(DEAF)),
        (200, data),
        (300, ack),
        (400, heard.clone()),
        (500, bench.to_vec()),
    ] {
        world.send(at_ms(end), ch(3), &bytes);
    }
    run_until(&mut d, &world, at_ms(600));
    let heard = format!("rx 3 {heard:?}");
    let bench = format!("rx 3 {:?}", &bench[..]);
    assert_eq!(
        seen(&d),
        [
            (0, "boot"),
            (400_000, heard.as_str()),
            (500_000, bench.as_str()),
            (600_000, "wakeup")
        ]
    );
    assert_eq!(world.0.borrow().lost, 0, "the radio heard all five");
    assert_eq!(d.stats().frames, 2);
}

/// `BARK_IGNORE` is read as the log prints a node ID.
#[test]
fn a_node_id_is_read_from_its_hex_digits() {
    assert_eq!(bark_driver::node_id_of_hex("bae6e6cbf4f5"), DEAF);
    assert_eq!(bark_driver::node_id_of_hex("BAE6E6CBF4F5"), DEAF);
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
            (cad_done.as_micros(), "lbt busy=false dbm=-80"),
            (400_000, "wakeup"),
        ]
    );
    assert_eq!(world.ops()[1], (end, Op::Lbt(ch(3))));
    assert_eq!(d.stats().held, 1);
}

/// A reading of another channel waits until the reading is right, where
/// the old 300 µs read 10 dB low; one of the resting channel is read as
/// the radio listens, without a restart.
#[test]
fn a_reading_waits_for_the_receiver_to_settle() {
    assert_eq!(climb(Duration::from_micros(300)), -90);
    let world = World::default();
    let mut core = Scripted::new(ch(3), &[at_ms(10), at_ms(20), at_ms(100)]);
    core.then.push((at_ms(10), Action::Rssi(ch(5))));
    core.then.push((at_ms(20), Action::Rssi(ch(3))));
    let mut d = driver(core, &world, Duration::ZERO);
    run_until(&mut d, &world, at_ms(100));
    let read = at_ms(10) + RSSI_DWELL;
    assert_eq!(
        world.ops(),
        [
            (Instant::ZERO, Op::Listen(ch(3))),
            (at_ms(10), Op::Listen(ch(5))),
            (read, Op::Rssi(ch(5))),
            (read, Op::Listen(ch(3))),
            (at_ms(20), Op::Rssi(ch(3))),
        ]
    );
    let samples: Vec<_> = seen(&d)
        .into_iter()
        .filter(|(_, s)| s.starts_with("RssiSample"))
        .collect();
    assert_eq!(samples.len(), 2);
    assert!(
        samples.iter().all(|(_, s)| s.ends_with("dbm: -80 }")),
        "{samples:?}"
    );
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
    assert_eq!(world.ops()[1], (at_ms(50) + MAX_AIRTIME, Op::Lbt(ch(3))));
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
        format: bark_proto::wire::FORMAT,
        map_point: false,
        leaving: false,
        id: bark_proto::NodeId([6; 6]),
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
    assert_eq!(d.stats().recoveries, 0);
    let (id, payload) = d.core().inbox().expect("its own message came back");
    assert_eq!(payload, b"hello");
    assert!(
        d.app()
            .messages
            .iter()
            .any(|e| matches!(e, AppEvent::Message { msg_id } if *msg_id == id)),
        "and was announced: {:?}",
        d.app().messages
    );
    assert!(d.app().outgoing.is_none(), "taken");
}

/// A radio whose every check reads busy: the relay skips a heartbeat on
/// every channel in turn, and once it has on all of them, the driver
/// brings the radio back as at boot (roadmap D192), and not before.
#[test]
fn a_heartbeat_skipped_on_every_channel_brings_the_radio_back() {
    let world = World::default();
    let me = Position::from_degrees(50.45, 30.52);
    let relay = Relay::new(relay_config(me), 7);
    let mut d = Driver::new(
        relay,
        FakeRadio {
            world: world.clone(),
            cad_busy: true,
        },
        FakeClock {
            world: world.clone(),
            early: Duration::ZERO,
            fired_early: Default::default(),
        },
        TestApp::default(),
        10,
    );
    run_until(&mut d, &world, Instant::ZERO + Duration::from_secs(600));
    let ops = world.ops();
    let first = ops
        .iter()
        .position(|(_, op)| *op == Op::Recover)
        .expect("the radio was brought back");
    // Before it, every check found its channel busy, three to a
    // heartbeat, on every channel, and nothing was sent.
    let checked: std::collections::BTreeSet<u8> = ops[..first]
        .iter()
        .filter_map(|(_, op)| match op {
            Op::Lbt(c) => Some(c.index()),
            _ => None,
        })
        .collect();
    assert_eq!(checked.len(), usize::from(bark_proto::CHANNELS));
    let checks = ops[..first]
        .iter()
        .filter(|(_, op)| matches!(op, Op::Lbt(_)))
        .count();
    assert!(
        checks >= 3 * usize::from(bark_proto::CHANNELS),
        "{checks} checks"
    );
    assert!(!ops[..first]
        .iter()
        .any(|(_, op)| matches!(op, Op::Transmit(_))));
    // Then it listens again, and counts afresh.
    assert!(
        matches!(ops[first + 1].1, Op::Listen(_)),
        "{:?}",
        ops[first + 1]
    );
    let recovered = ops.iter().filter(|(_, op)| *op == Op::Recover).count();
    assert_eq!(d.stats().recoveries as usize, recovered);
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

/// The core's next `Listen`, `Lbt`, `Rssi` or `Transmit` ends a sniff
/// before its end; the radio is carried out as ever and listens after.
#[test]
fn the_next_radio_action_ends_a_sniff() {
    let t = at_ms(100);
    let cad_done = t + CAD_TIME;
    let tx_done = t + time_on_air(3, MIN_PREAMBLE_SYMBOLS);
    let cases = [
        (
            Action::Lbt(ch(3)),
            vec![(t, Op::Lbt(ch(3))), (cad_done, Op::Listen(ch(3)))],
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
    assert_eq!(world.ops()[2], (at_ms(50) + MAX_AIRTIME, Op::Lbt(ch(3))));
}

/// The power the application tells reaches the core right after the boot
/// wakeup, and again when it changes, once each time (core-interface §4).
#[test]
fn the_power_reaches_the_core_at_boot_and_at_each_change() {
    let world = World::default();
    let core = Scripted::new(ch(3), &[at_ms(100), at_ms(300)]);
    let mut d = driver(core, &world, Duration::ZERO);
    d.app_mut().power = Some(true);
    run_until(&mut d, &world, at_ms(100));
    d.app_mut().set_power(false);
    step(&mut d, &world);
    run_until(&mut d, &world, at_ms(300));
    assert_eq!(
        seen(&d),
        [
            (0, "boot"),
            (0, "App(Power { external: true })"),
            (100_000, "wakeup"),
            (100_000, "App(Power { external: false })"),
            (300_000, "wakeup")
        ]
    );
}

/// On battery the driver reads the battery at once and then once every
/// `BATTERY_EVERY`, and reports each reading to the core; on USB it reads
/// none (core-interface §4).
#[test]
fn the_battery_reaches_the_core_once_a_minute_on_battery() {
    let world = World::default();
    let core = Scripted::new(ch(3), &[at_ms(1)]);
    let mut d = driver(core, &world, Duration::ZERO);
    d.app_mut().power = Some(true);
    d.app_mut().battery = Some(3900);
    run_until(&mut d, &world, at_ms(1));
    assert_eq!(d.app().battery_reads, 0, "none on USB");
    d.app_mut().set_power(false);
    run_until(&mut d, &world, at_ms(200_000));
    let readings: Vec<u64> = seen(&d)
        .iter()
        .filter(|(_, s)| *s == "App(Battery { millivolts: 3900 })")
        .map(|(t, _)| *t)
        .collect();
    let every = bark_driver::BATTERY_EVERY.as_micros();
    assert_eq!(readings.len(), 4, "{:?}", seen(&d));
    for w in readings.windows(2) {
        assert_eq!(w[1] - w[0], every);
    }
}

/// A relay behind a box, as the firmware keeps one behind a static.
struct Boxed(Box<Relay>);

impl Core for Boxed {
    fn handle(&mut self, event: Event<'_>, now: Instant, emit: &mut dyn FnMut(Action)) {
        self.0.handle(event, now, emit)
    }

    fn next_wakeup(&self) -> Option<Instant> {
        self.0.next_wakeup()
    }

    fn accepts_send(&self, now: Instant) -> bool {
        self.0.accepts_send(now)
    }

    fn inbox(&self) -> Option<(MsgId, &[u8])> {
        self.0.inbox()
    }

    fn inbox_len(&self) -> usize {
        self.0.inbox_len()
    }

    fn confirm(&mut self, msg_id: MsgId) -> bool {
        self.0.confirm(msg_id)
    }

    fn gps_tracks(&self) -> bool {
        self.0.gps_tracks()
    }

    fn fix_in_reach(&self, position: Position) -> bool {
        self.0.fix_in_reach(position)
    }
}

/// A relay set up as the firmware sets one up, in zeroed memory with
/// `Relay::reset` (`bark-fw/src/node.rs`), does what a new one does.
#[test]
fn a_relay_reset_in_zeroed_memory_is_a_new_one() {
    let config = relay_config(Position::from_degrees(50.45, 30.52));
    // SAFETY: all zero bytes are a valid `Relay`; `bark-fw/src/node.rs`
    // checks it when it is built.
    let mut zeroed = unsafe { Box::<Relay>::new_zeroed().assume_init() };
    zeroed.reset(config, 7);
    let until = Instant::ZERO + Duration::from_secs(60);
    let run = |core: Boxed| {
        let world = World::default();
        let mut d = driver(core, &world, Duration::ZERO);
        run_until(&mut d, &world, until);
        world.ops()
    };
    let from_zeros = run(Boxed(zeroed));
    let fresh = run(Boxed(Box::new(Relay::new(config, 7))));
    assert!(fresh.iter().any(|(_, op)| matches!(op, Op::Transmit(_))));
    assert_eq!(from_zeros, fresh);
}

/// The GPS follows the core (core-interface §4): a search from a
/// `Locate` until its end, ended by the first fix in reach and by a
/// `Locate` at or before now, and running all the time while the core
/// says it tracks; a fix beyond reach of every known relay is not handed,
/// and the core is told once that it has no fix.
#[test]
fn the_gps_follows_the_core() {
    let world = World::default();
    let wakeups = [at_ms(1), at_ms(10), at_ms(20), at_ms(30), at_ms(40)];
    let mut core = Scripted::new(ch(3), &wakeups);
    let here = Position::from_degrees(50.45, 30.52);
    let far = Position::from_degrees(51.45, 30.52);
    core.beyond.push(far);
    core.then.push((at_ms(1), Action::Locate(at_ms(500))));
    core.then.push((at_ms(20), Action::Locate(at_ms(500))));
    core.then.push((at_ms(30), Action::Locate(at_ms(30))));
    core.tracks_from = Some(at_ms(40));
    let mut d = driver(core, &world, Duration::ZERO);
    let fixes = |d: &TestDriver<Scripted>| {
        d.core()
            .seen
            .iter()
            .filter(|(_, s)| s.contains("Fix {"))
            .count()
    };
    let none = |d: &TestDriver<Scripted>| {
        d.core()
            .seen
            .iter()
            .filter(|(_, s)| s.contains("NoFix"))
            .count()
    };
    run_until(&mut d, &world, at_ms(1));
    assert_eq!(d.app().gps, [Gps::Rest, Gps::Search(at_ms(500))]);
    d.app_mut().push_request(Request::Fix(far));
    step(&mut d, &world);
    assert_eq!(fixes(&d), 0, "beyond reach");
    assert_eq!(none(&d), 1, "no fix");
    d.app_mut().requests.push_back(Request::Fix(far));
    step(&mut d, &world);
    assert_eq!(none(&d), 1, "once, at the change");
    assert_eq!(d.app().gps.last(), Some(&Gps::Search(at_ms(500))));
    d.app_mut().push_request(Request::Fix(here));
    step(&mut d, &world);
    assert_eq!(fixes(&d), 1);
    assert_eq!(d.app().gps.last(), Some(&Gps::Rest), "the fix ends it");
    run_until(&mut d, &world, at_ms(20));
    assert_eq!(d.app().gps.last(), Some(&Gps::Search(at_ms(500))));
    run_until(&mut d, &world, at_ms(30));
    assert_eq!(d.app().gps.last(), Some(&Gps::Rest), "ended at once");
    run_until(&mut d, &world, at_ms(40));
    assert_eq!(d.app().gps.last(), Some(&Gps::Track));
    d.app_mut().push_request(Request::Fix(here));
    step(&mut d, &world);
    assert_eq!(fixes(&d), 2);
    assert_eq!(d.app().gps.last(), Some(&Gps::Track), "tracking goes on");
}

/// A relay built with a place runs its GPS on external power: the place
/// is not a map point (roadmap D83).
#[test]
fn a_placed_relay_on_usb_tracks() {
    let world = World::default();
    let me = Position::from_degrees(50.45, 30.52);
    let mut d = driver(Relay::new(relay_config(me), 7), &world, Duration::ZERO);
    d.app_mut().power = Some(true);
    run_until(&mut d, &world, Instant::ZERO + Duration::from_secs(5));
    assert!(!d.core().on_map_point());
    assert_eq!(d.app().gps.last(), Some(&Gps::Track), "{:?}", d.app().gps);
}

/// Steps until the clock reaches `until`, at most `max`: a loop that never
/// waits takes steps without moving the clock and runs out of them.
fn run_bounded<C: Core>(d: &mut TestDriver<C>, world: &World, until: Instant, max: u32) {
    let mut steps = 0;
    while world.now() < until {
        assert!(
            steps < max,
            "{max} steps and the clock still at {:?}",
            world.now()
        );
        step(d, world);
        steps += 1;
    }
}

/// An application that says it is ready at once, always, and then gives
/// nothing, as the node's did on the bench 2026-11-06 while its mailbox
/// said a reply was due that it then did not give: the loop still waits for its clock
/// and its radio as with an idle application, and asks the application
/// again only after one of them woke it (core-interface §4).
#[test]
fn a_ready_that_gives_nothing_does_not_spin_the_loop() {
    let world = World::default();
    let mut core = Scripted::new(ch(3), &[at_ms(100), at_ms(200), at_ms(300)]);
    core.accepts = true;
    let mut d = driver(core, &world, Duration::ZERO);
    d.app_mut().eager = true;
    world.send(at_ms(150), ch(3), &[1, 2, 3]);
    // The boot, and per event of the clock or the radio, the frame's start
    // and its end among them, one step for it and one for the
    // application's empty readiness.
    run_bounded(&mut d, &world, at_ms(300), 1 + 2 * 5);
    assert_eq!(
        seen(&d),
        [
            (0, "boot"),
            (100_000, "wakeup"),
            (150_000, "rx 3 [1, 2, 3]"),
            (200_000, "wakeup"),
            (300_000, "wakeup")
        ]
    );
    assert_eq!(
        d.app().readies,
        5,
        "after the boot and each event but the last"
    );
    assert_eq!(d.stats().empty_readies, 5);
}

/// A readiness for the application's own turn, as the node's for a
/// phone's read of the neighbour table or the inbox, which only the loop
/// that runs the driver serves, ends the step at once and is no empty
/// readiness: the next signal ends the next step as quickly, rather than
/// waiting for the clock or the radio (bench 2026-11-10, core-interface §4).
#[test]
fn a_ready_for_the_application_itself_ends_the_step() {
    let world = World::default();
    let core = Scripted::new(ch(3), &[at_ms(100), at_ms(200)]);
    let mut d = driver(core, &world, Duration::ZERO);
    step(&mut d, &world);
    d.app_mut().own = true;
    for _ in 0..2 {
        d.app_mut().signal();
        step(&mut d, &world);
        assert_eq!(world.now(), Instant::ZERO, "not woken by the clock");
    }
    d.app_mut().own = false;
    run_bounded(&mut d, &world, at_ms(100), 1);
    assert_eq!(seen(&d), [(0, "boot"), (100_000, "wakeup")]);
    assert_eq!(d.stats().own_readies, 2);
    assert_eq!(d.stats().empty_readies, 0);
}

/// A message the application has goes to the core at once, and so does
/// one that comes after an empty readiness, at the next step: the driver
/// offers before it waits (core-interface §4).
#[test]
fn a_message_offered_goes_at_once() {
    let world = World::default();
    let mut core = Scripted::new(ch(3), &[at_ms(100), at_ms(200)]);
    core.accepts = true;
    let mut d = driver(core, &world, Duration::ZERO);
    step(&mut d, &world);
    d.app_mut().outgoing = Some((out(), vec![4, 5]));
    d.app_mut().signal();
    step(&mut d, &world);
    assert_eq!(d.app().outgoing, None, "taken");
    // Empty, then a message: it does not wait for the wakeup.
    d.app_mut().eager = true;
    step(&mut d, &world);
    d.app_mut().eager = false;
    d.app_mut().outgoing = Some((out(), vec![6]));
    step(&mut d, &world);
    assert_eq!(d.app().outgoing, None, "taken");
    assert_eq!(sends(&d), [0, 0]);
}

fn out() -> Outgoing {
    Outgoing {
        dst_pos: Position::default(),
        dst_id: NodeId([7; 6]),
        len: 2,
    }
}

/// When the core took each message from the application.
fn sends(d: &TestDriver<Scripted>) -> Vec<u64> {
    seen(d)
        .into_iter()
        .filter(|(_, s)| s.contains("Send"))
        .map(|(t, _)| t)
        .collect()
}

/// A message queued with no signal goes at the next step, and one the
/// application holds while the core takes none goes at the step after the
/// one that lets the core take it; neither is a readiness.
#[test]
fn a_waiting_message_goes_without_a_signal() {
    let world = World::default();
    let mut core = Scripted::new(ch(3), &[at_ms(100), at_ms(200)]);
    core.accepts_from = Some(at_ms(100));
    let mut d = driver(core, &world, Duration::ZERO);
    step(&mut d, &world);
    // The core takes none until its wakeup at 100 ms; the application
    // holds what arrives meanwhile.
    d.app_mut().outgoing = Some((out(), vec![6]));
    d.app_mut().signal();
    run_bounded(&mut d, &world, at_ms(100), 1);
    step(&mut d, &world);
    assert_eq!(d.app().outgoing, None, "taken");
    // With no signal at all.
    d.app_mut().outgoing = Some((out(), vec![7]));
    step(&mut d, &world);
    assert_eq!(d.app().outgoing, None, "taken");
    assert_eq!(sends(&d), [100_000, 100_000]);
    assert_eq!(d.app().readies, 0);
    assert_eq!(d.stats().empty_readies, 0);
}

/// A signal with nothing behind it, and a power event that changed
/// nothing, end no wait: the loop waits for its clock as with an idle
/// application.
#[test]
fn a_signal_with_nothing_behind_it_is_no_readiness() {
    let world = World::default();
    let core = Scripted::new(ch(3), &[at_ms(100), at_ms(200)]);
    let mut d = driver(core, &world, Duration::ZERO);
    d.app_mut().power = Some(true);
    step(&mut d, &world);
    d.app_mut().signal();
    run_bounded(&mut d, &world, at_ms(100), 1);
    d.app_mut().set_power(true);
    run_bounded(&mut d, &world, at_ms(200), 1);
    assert_eq!(
        seen(&d),
        [
            (0, "boot"),
            (0, "App(Power { external: true })"),
            (100_000, "wakeup"),
            (200_000, "wakeup")
        ]
    );
    assert_eq!(d.app().readies, 0);
    assert_eq!(d.stats().empty_readies, 0);
}

/// A reply that becomes due when the core hands the application a
/// message, as a home's to a fetch, goes at the next step, with no
/// signal and no write from the phone.
#[test]
fn a_reply_that_becomes_due_goes_without_a_signal() {
    let world = World::default();
    let mut core = Scripted::new(ch(3), &[at_ms(100), at_ms(200)]);
    core.accepts = true;
    let fetch = AppEvent::Message { msg_id: MsgId(9) };
    core.then.push((at_ms(100), Action::App(fetch)));
    let mut d = driver(core, &world, Duration::ZERO);
    d.app_mut().reply = Some((out(), vec![4, 5]));
    run_bounded(&mut d, &world, at_ms(100), 2);
    step(&mut d, &world);
    assert_eq!(d.app().messages, [fetch]);
    assert_eq!(sends(&d), [100_000]);
    assert_eq!(d.app().readies, 0);
}

fn image() -> Image {
    Image {
        version: 8,
        blocks: 3,
        last: 5,
        key: 1,
    }
}

/// The core's update actions reach the application, and its answers
/// reach the core as commands (core-interface §4).
#[test]
fn an_update_goes_between_the_core_and_the_application() {
    let world = World::default();
    let mut core = Scripted::new(ch(3), &[at_ms(10), at_ms(500)]);
    let piece = Piece {
        block: 1,
        index: 9,
        bytes: [7; PIECE_BYTES],
    };
    core.then.push((at_ms(10), Action::Offered(image())));
    core.then.push((at_ms(10), Action::StorePiece(piece)));
    let mut d = driver(core, &world, Duration::ZERO);
    run_until(&mut d, &world, at_ms(100));
    assert_eq!(d.app().offered, [image()]);
    assert_eq!(d.app().pieces, [piece]);
    d.app_mut().push_request(Request::Gather {
        image: image(),
        from: 0,
    });
    d.app_mut()
        .push_request(Request::Block { index: 1, ok: true });
    d.app_mut().push_request(Request::Holds(image()));
    step(&mut d, &world);
    let told: Vec<&str> = seen(&d)
        .into_iter()
        .map(|(_, s)| s)
        .filter(|s| s.starts_with("App"))
        .collect();
    assert_eq!(
        told,
        [
            format!("App(Gather {{ image: {:?}, from: 0 }})", image()),
            "App(Block { index: 1, ok: true })".into(),
            format!("App(Holds({:?}))", image()),
        ]
    );
}

/// The application's ask reaches the core as a command, and a
/// neighbour's status the core hands reaches the application
/// (core-interface §4).
#[test]
fn a_status_goes_between_the_core_and_the_application() {
    let world = World::default();
    let mut core = Scripted::new(ch(3), &[at_ms(10), at_ms(500)]);
    let neighbour = NodeId([7; 6]);
    let status = bark_proto::status::Status {
        sleepy: true,
        version: 9,
        ..Default::default()
    };
    let told = AppEvent::Status {
        from: neighbour,
        status,
    };
    core.then.push((at_ms(10), Action::App(told)));
    let mut d = driver(core, &world, Duration::ZERO);
    run_until(&mut d, &world, at_ms(100));
    assert_eq!(d.app().messages, [told]);
    d.app_mut().push_request(Request::Ask(neighbour));
    step(&mut d, &world);
    let asked: Vec<&str> = seen(&d)
        .into_iter()
        .map(|(_, s)| s)
        .filter(|s| s.starts_with("App"))
        .collect();
    assert_eq!(asked, [format!("App(Ask {{ neighbour: {neighbour:?} }})")]);
}

/// A piece frame goes with the bytes the application wrote into it; one
/// it could not fill is not sent, and still completes.
#[test]
fn a_piece_goes_filled_or_not_at_all() {
    let world = World::default();
    let mut core = Scripted::new(ch(3), &[at_ms(10), at_ms(500), at_ms(1000)]);
    let mut buf = [0u8; MAX_FRAME];
    let piece = Packet::Update(Update::Piece {
        version: 2,
        block: 1,
        index: 9,
        bytes: &[0; PIECE_BYTES],
    });
    let len = piece.encode(&mut buf).expect("it fits");
    let out = frame(ch(5), &buf[..len]);
    core.then.push((at_ms(10), Action::Serve(out)));
    core.then.push((at_ms(500), Action::Serve(out)));
    let mut d = driver(core, &world, Duration::ZERO);
    d.app_mut().fill = Some(0x5a);
    run_until(&mut d, &world, at_ms(400));
    let mut filled = out;
    filled.bytes[PIECE_HEADER_LEN..len].fill(0x5a);
    assert!(world
        .ops()
        .iter()
        .any(|(t, op)| *t == at_ms(10) && *op == Op::Transmit(filled)));
    d.app_mut().fill = None;
    run_until(&mut d, &world, at_ms(1000));
    assert!(!world.ops().iter().any(|(t, _)| *t == at_ms(500)));
    assert_eq!(d.stats().unserved, 1);
    assert_eq!(seen(&d).iter().filter(|(_, s)| *s == "tx done").count(), 2);
}
