//! A small deterministic medium for driving cores in tests.
//!
//! Time is a fake clock advanced from event to event. Each core has a
//! resting channel and at most one excursion; a frame reaches every other
//! live core that is linked to the sender, resting on the frame's channel
//! and not deaf for any part of the frame's airtime (`radio::time_on_air`),
//! unless another linked transmission on that channel overlaps it. The
//! harness enforces the driver side of `docs/core-interface.md` §3: it
//! panics if a core starts a second excursion, sends a short preamble or
//! an oversized frame, or exceeds the duty cycle; and it holds a channel
//! check while a reception is in progress (rule 8). It also panics if a
//! test offers a message to a core that does not accept one (spec "Physical layer").

#![allow(dead_code)]

use bark_proto::radio::{
    time_on_air, Channel, DUTY_CYCLE_WINDOW, MIN_PREAMBLE_SYMBOLS, SYMBOL_TIME,
};
use bark_proto::sniff::LONG_PREAMBLE_SYMBOLS;
use bark_proto::tables::budget::BUDGET;
use bark_proto::wire::MAX_PAYLOAD;
use bark_proto::{Action, AppCommand, AppEvent, Core, Duration, Event, Frame, Instant, Trace};
use std::collections::{HashMap, HashSet};

/// Synthesiser retune and settle, of the order the SX1262 datasheet gives
/// for a frequency change plus PLL lock.
pub const RETUNE: Duration = Duration::from_micros(200);
/// Channel activity detection over four symbols.
pub const CAD: Duration = Duration::from_micros(4 * SYMBOL_TIME.as_micros());
/// One RSSI reading after the receiver settles (spec "Busy estimate": ≈0.3 ms).
pub const RSSI: Duration = Duration::from_micros(300);
/// What an idle channel reads.
pub const NOISE_DBM: i16 = -121;
/// What a channel with a frame or a jammer on it reads.
pub const SIGNAL_DBM: i16 = -80;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Cad,
    Rssi,
    Tx,
}

#[derive(Clone, Copy, Debug)]
struct Excursion {
    kind: Kind,
    channel: Channel,
    start: Instant,
    end: Instant,
    /// Whether the radio is off the resting channel for it.
    deaf: bool,
}

#[derive(Clone, Debug)]
struct Tx {
    from: usize,
    channel: Channel,
    start: Instant,
    end: Instant,
    bytes: Vec<u8>,
    preamble: u16,
}

pub struct SimNode<C: Core> {
    pub core: C,
    pub alive: bool,
    resting: Option<Channel>,
    resting_since: Instant,
    ex: Option<Excursion>,
    /// A check held back while a frame was being received (rule 8).
    held: Option<(Action, Instant)>,
    deaf_until: Instant,
    wakeup: Option<Instant>,
    started: bool,
    /// The power its driver reports right after the boot wakeup, if any
    /// (`docs/core-interface.md` §4).
    power: Option<bool>,
    /// Every transmission: start and airtime.
    pub airtime: Vec<(Instant, Duration)>,
    /// Every CAD and RSSI check as it began: start, end, the action, and
    /// whether it took the node off its resting channel.
    pub checks: Vec<(Instant, Instant, Action, bool)>,
    pub app: Vec<(Instant, AppEvent)>,
    /// What it told its application of its first rotation.
    pub boot: Vec<(Instant, AppEvent)>,
    /// What the core last asked to keep across a reboot.
    pub stored: Option<bark_proto::Stored>,
    /// The searches for a fix the core asked for: when, and until when.
    pub located: Vec<(Instant, Instant)>,
    pub traces: Vec<(Instant, Trace)>,
    pub sent: Vec<(Instant, Frame)>,
    pub received: Vec<(Instant, Vec<u8>)>,
    /// When the radio received duty-cycled (`Action::Sniff`, rule 10):
    /// from, until. It hears then only frames with the long preamble.
    pub sniffed: Vec<(Instant, Instant)>,
}

/// Decides whether a frame is lost: `(now, from, to, bytes)`.
pub type Loss = Box<dyn FnMut(Instant, usize, usize, &[u8]) -> bool>;

pub struct Harness<C: Core> {
    pub now: Instant,
    pub nodes: Vec<SimNode<C>>,
    links: HashSet<(usize, usize)>,
    /// Channels a node hears a foreign carrier on: busy to RSSI, not to CAD.
    jammed: HashMap<(usize, Channel), (Duration, Duration)>,
    /// Packet RSSI per directed link, where it is not [`SIGNAL_DBM`].
    rssi: HashMap<(usize, usize), i16>,
    txs: Vec<Tx>,
    /// Excursions started, by kind, for accounting.
    pub excursions: usize,
    /// Frames to lose on the way, for tests of retransmission: called as
    /// `(now, from, to, bytes)` for each frame a node would receive.
    pub lose: Option<Loss>,
}

impl<C: Core> Harness<C> {
    pub fn new(cores: Vec<C>) -> Self {
        let nodes = cores
            .into_iter()
            .map(|core| SimNode {
                core,
                alive: true,
                resting: None,
                resting_since: Instant::ZERO,
                ex: None,
                held: None,
                deaf_until: Instant::ZERO,
                wakeup: Some(Instant::ZERO),
                started: false,
                power: None,
                airtime: Vec::new(),
                checks: Vec::new(),
                app: Vec::new(),
                boot: Vec::new(),
                stored: None,
                located: Vec::new(),
                traces: Vec::new(),
                sent: Vec::new(),
                received: Vec::new(),
                sniffed: Vec::new(),
            })
            .collect();
        Harness {
            now: Instant::ZERO,
            nodes,
            links: HashSet::new(),
            jammed: HashMap::new(),
            rssi: HashMap::new(),
            txs: Vec::new(),
            excursions: 0,
            lose: None,
        }
    }

    /// Cores each with whether external power is present at boot, which
    /// the harness reports right after the boot wakeup, as a driver does
    /// (`docs/core-interface.md` §4).
    pub fn powered(cores: Vec<(C, bool)>) -> Self {
        let power: Vec<bool> = cores.iter().map(|c| c.1).collect();
        let mut h = Harness::new(cores.into_iter().map(|c| c.0).collect());
        for (n, p) in h.nodes.iter_mut().zip(power) {
            n.power = Some(p);
        }
        h
    }

    pub fn link(&mut self, a: usize, b: usize) {
        self.links.insert((a, b));
        self.links.insert((b, a));
    }

    /// Frames between `a` and `b` arrive at `dbm`, both ways.
    pub fn set_rssi(&mut self, a: usize, b: usize, dbm: i16) {
        self.rssi.insert((a, b), dbm);
        self.rssi.insert((b, a), dbm);
    }

    /// Cut the link between `a` and `b`.
    pub fn unlink(&mut self, a: usize, b: usize) {
        self.links.remove(&(a, b));
        self.links.remove(&(b, a));
    }

    /// A carrier on `channel` at `node`, all the time.
    pub fn jam(&mut self, node: usize, channel: Channel) {
        self.jam_part(
            node,
            channel,
            Duration::from_micros(1),
            Duration::from_micros(1),
        );
    }

    /// A carrier on `channel` at `node` for the first `on` of every `period`.
    pub fn jam_part(&mut self, node: usize, channel: Channel, on: Duration, period: Duration) {
        self.jammed.insert((node, channel), (on, period));
    }

    fn jammed_at(&self, node: usize, channel: Channel, t: Instant) -> bool {
        self.jammed
            .get(&(node, channel))
            .is_some_and(|&(on, period)| t.as_micros() % period.as_micros() < on.as_micros())
    }

    pub fn unjam(&mut self, node: usize, channel: Channel) {
        self.jammed.remove(&(node, channel));
    }

    /// Stop a node, as if unplugged.
    pub fn kill(&mut self, node: usize) {
        self.nodes[node].alive = false;
    }

    /// Power node `node` off and on again now with `core`: it boots afresh,
    /// its power reported as at the first boot.
    pub fn reboot(&mut self, node: usize, core: C) {
        let n = &mut self.nodes[node];
        n.core = core;
        n.alive = true;
        n.started = false;
        n.wakeup = Some(self.now);
        n.ex = None;
        n.held = None;
        n.resting = None;
        n.deaf_until = self.now;
    }

    /// Delay a node's start until `at`.
    pub fn start_at(&mut self, node: usize, at: Instant) {
        self.nodes[node].wakeup = Some(at);
    }

    pub fn resting(&self, node: usize) -> Option<Channel> {
        self.nodes[node].resting
    }

    fn linked(&self, a: usize, b: usize) -> bool {
        self.links.contains(&(a, b))
    }

    /// Run until `until`, or until `stop` holds after an event.
    pub fn run_until(&mut self, until: Instant, mut stop: impl FnMut(&Self) -> bool) -> bool {
        loop {
            let Some(t) = self.next_time() else {
                return false;
            };
            if t > until {
                self.now = until;
                return false;
            }
            self.now = t;
            self.step();
            if stop(self) {
                return true;
            }
        }
    }

    /// Hand node `i` an application command now.
    pub fn command(&mut self, i: usize, cmd: AppCommand<'_>) {
        if let AppCommand::Send { payload, .. } = cmd {
            assert!(
                self.accepts(i),
                "node {i} offered a message it does not accept"
            );
            assert!(payload.len() <= MAX_PAYLOAD, "payload over the limit");
        }
        self.deliver(i, Event::App(cmd));
    }

    /// Put `f` on the air now as node `i`'s, past its core: for a test that
    /// needs more traffic than cores would send. No budget or excursion
    /// is checked or recorded for it.
    pub fn inject(&mut self, i: usize, f: Frame) {
        self.txs.push(Tx {
            from: i,
            channel: f.channel,
            start: self.now,
            end: self.now + time_on_air(f.len, f.preamble_symbols),
            bytes: f.payload().to_vec(),
            preamble: f.preamble_symbols,
        });
    }

    /// Whether node `i` would take an `AppCommand::Send` now.
    pub fn accepts(&self, i: usize) -> bool {
        self.nodes[i].core.accepts_send(self.now)
    }

    pub fn run_for(&mut self, d: Duration) {
        let until = self.now + d;
        self.run_until(until, |_| false);
    }

    fn next_time(&self) -> Option<Instant> {
        let mut t: Option<Instant> = None;
        let mut at = |x: Instant| t = Some(t.map_or(x, |t| t.min(x)));
        for tx in &self.txs {
            at(tx.end);
        }
        for n in self.nodes.iter().filter(|n| n.alive) {
            if let Some(ex) = &n.ex {
                at(ex.end);
            }
            if let Some((_, start)) = n.held {
                at(start);
            }
            if let Some(w) = n.wakeup {
                at(w);
            }
        }
        t
    }

    /// Process everything due at `self.now`: frames ending, then
    /// excursions completing, then held checks starting, then wakeups.
    fn step(&mut self) {
        let now = self.now;
        let ending: Vec<Tx> = self.txs.iter().filter(|t| t.end <= now).cloned().collect();
        for tx in &ending {
            for i in 0..self.nodes.len() {
                if self.hears(i, tx) {
                    if let Some(lose) = &mut self.lose {
                        if lose(now, tx.from, i, &tx.bytes) {
                            continue;
                        }
                    }
                    self.nodes[i].received.push((now, tx.bytes.clone()));
                    let ev = Event::Received {
                        channel: tx.channel,
                        rssi_dbm: self.rssi.get(&(tx.from, i)).copied().unwrap_or(SIGNAL_DBM),
                        snr_db: 10,
                        bytes: &tx.bytes,
                    };
                    self.deliver(i, ev);
                }
            }
        }
        self.txs.retain(|t| t.end > now);
        for i in 0..self.nodes.len() {
            let n = &self.nodes[i];
            let Some(ex) = n.ex else { continue };
            if !n.alive || ex.end > now {
                continue;
            }
            self.nodes[i].ex = None;
            let ev = match ex.kind {
                Kind::Cad => Event::CadDone {
                    channel: ex.channel,
                    busy: self.on_air(i, ex.channel, ex.start, ex.end),
                },
                Kind::Rssi => {
                    let dbm = if self.jammed_at(i, ex.channel, ex.end) {
                        SIGNAL_DBM
                    } else {
                        self.strongest(i, ex.channel, ex.end).unwrap_or(NOISE_DBM)
                    };
                    Event::RssiSample {
                        channel: ex.channel,
                        dbm,
                    }
                }
                Kind::Tx => Event::TxDone,
            };
            self.deliver(i, ev);
        }
        for i in 0..self.nodes.len() {
            if let Some((a, start)) = self.nodes[i].held {
                if start <= now && self.nodes[i].alive {
                    self.nodes[i].held = None;
                    self.begin(i, a);
                }
            }
        }
        for i in 0..self.nodes.len() {
            let n = &self.nodes[i];
            if n.alive && n.wakeup.is_some_and(|w| w <= now) {
                let boot = !std::mem::replace(&mut self.nodes[i].started, true);
                self.deliver(i, Event::Wakeup);
                if let Some(external) = self.nodes[i].power.filter(|_| boot) {
                    self.deliver(i, Event::App(AppCommand::Power { external }));
                }
            }
        }
    }

    /// Whether node `i` receives `tx` intact.
    fn hears(&self, i: usize, tx: &Tx) -> bool {
        let n = &self.nodes[i];
        n.alive
            && n.started
            && i != tx.from
            && self.linked(tx.from, i)
            && n.resting == Some(tx.channel)
            && n.resting_since <= tx.start
            && n.deaf_until <= tx.start
            && n.ex.is_none_or(|e| !e.deaf)
            && (tx.preamble >= LONG_PREAMBLE_SYMBOLS
                || !n
                    .sniffed
                    .iter()
                    .any(|&(a, b)| a <= tx.start && b > tx.start))
            && !self.txs.iter().any(|o| {
                o.from != tx.from
                    && o.channel == tx.channel
                    && self.linked(o.from, i)
                    && o.start < tx.end
                    && o.end > tx.start
            })
    }

    /// Whether a linked transmission is on `channel` at `i` during `[a, b]`.
    fn on_air(&self, i: usize, channel: Channel, a: Instant, b: Instant) -> bool {
        self.txs
            .iter()
            .any(|t| t.channel == channel && self.linked(t.from, i) && t.start <= b && t.end > a)
    }

    /// The strongest frame on `channel` at `i` at `t`, as an RSSI reading
    /// reads it.
    fn strongest(&self, i: usize, channel: Channel, t: Instant) -> Option<i16> {
        self.txs
            .iter()
            .filter(|x| x.channel == channel && self.linked(x.from, i) && x.start <= t && x.end > t)
            .map(|x| self.rssi.get(&(x.from, i)).copied().unwrap_or(SIGNAL_DBM))
            .max()
    }

    /// A frame on `i`'s resting channel that `i` is in the middle of receiving.
    fn receiving(&self, i: usize) -> Option<Instant> {
        let now = self.now;
        self.txs
            .iter()
            .filter(|t| t.start < now && self.hears(i, t))
            .map(|t| t.end)
            .max()
    }

    /// Hand `ev` to core `i`, then wake it again while it asks for a time
    /// already reached. A core that keeps asking for the present is stuck.
    fn deliver(&mut self, i: usize, ev: Event<'_>) {
        let now = self.now;
        self.handle(i, ev);
        for _ in 0..8 {
            match self.nodes[i].core.next_wakeup() {
                Some(w) if w <= now => self.handle(i, Event::Wakeup),
                w => {
                    self.nodes[i].wakeup = w;
                    return;
                }
            }
        }
        panic!("core {i} keeps asking for a wakeup at {now:?}");
    }

    fn handle(&mut self, i: usize, ev: Event<'_>) {
        let now = self.now;
        let mut actions = Vec::new();
        self.nodes[i].core.handle(ev, now, &mut |a| actions.push(a));
        for a in actions {
            self.apply(i, a);
        }
    }

    fn apply(&mut self, i: usize, a: Action) {
        let now = self.now;
        if matches!(
            a,
            Action::Listen(_)
                | Action::Cad(_)
                | Action::Rssi(_)
                | Action::Transmit(_)
                | Action::Sniff(_)
        ) {
            // Any use of the radio ends a sniff, and so does a new one.
            if let Some(last) = self.nodes[i].sniffed.last_mut() {
                last.1 = last.1.min(now);
            }
        }
        match a {
            Action::Sniff(until) => {
                assert!(until > now, "core {i} asked to sniff until the past");
                self.nodes[i].sniffed.push((now, until));
            }
            Action::Listen(c) => {
                let n = &mut self.nodes[i];
                if n.resting != Some(c) {
                    n.resting = Some(c);
                    n.resting_since = now + RETUNE;
                }
            }
            Action::App(e @ AppEvent::Boot { .. }) => self.nodes[i].boot.push((now, e)),
            Action::App(e) => self.nodes[i].app.push((now, e)),
            Action::Store(s) => self.nodes[i].stored = Some(s),
            Action::Locate(until) => self.nodes[i].located.push((now, until)),
            Action::Trace(t) => self.nodes[i].traces.push((now, t)),
            Action::Cad(_) | Action::Rssi(_) | Action::Transmit(_) => {
                let n = &self.nodes[i];
                assert!(
                    n.ex.is_none() && n.held.is_none(),
                    "core {i} started {a:?} with an excursion outstanding (rule 1)"
                );
                let off = matches!(a, Action::Cad(c) | Action::Rssi(c) if n.resting != Some(c));
                match self.receiving(i) {
                    Some(end) if off => self.nodes[i].held = Some((a, end)),
                    _ => self.begin(i, a),
                }
            }
        }
    }

    fn begin(&mut self, i: usize, a: Action) {
        let now = self.now;
        self.excursions += 1;
        let resting = self.nodes[i].resting;
        let (kind, channel, cost, deaf) = match a {
            Action::Cad(c) => (Kind::Cad, c, RETUNE + CAD + RETUNE, true),
            Action::Rssi(c) if resting == Some(c) => (Kind::Rssi, c, RSSI, false),
            Action::Rssi(c) => (Kind::Rssi, c, RETUNE + RSSI + RETUNE, true),
            Action::Transmit(f) => {
                assert!(
                    f.preamble_symbols >= MIN_PREAMBLE_SYMBOLS,
                    "core {i}: short preamble (rule 4)"
                );
                assert!(
                    f.len as usize <= f.bytes.len(),
                    "core {i}: oversized frame (rule 4)"
                );
                let airtime = time_on_air(f.len, f.preamble_symbols);
                self.check_budget(i, now, airtime);
                self.nodes[i].airtime.push((now, airtime));
                self.nodes[i].sent.push((now, f));
                self.txs.push(Tx {
                    from: i,
                    channel: f.channel,
                    start: now,
                    end: now + airtime,
                    bytes: f.payload().to_vec(),
                    preamble: f.preamble_symbols,
                });
                (Kind::Tx, f.channel, airtime + RETUNE, true)
            }
            _ => unreachable!(),
        };
        let end = now + cost;
        if kind != Kind::Tx {
            self.nodes[i].checks.push((now, end, a, deaf));
        }
        if deaf {
            self.nodes[i].deaf_until = end;
        }
        self.nodes[i].ex = Some(Excursion {
            kind,
            channel,
            start: now,
            end,
            deaf,
        });
    }

    /// The window ending when this transmission ends holds at most 10 %
    /// (spec "Regulatory limits").
    fn check_budget(&self, i: usize, start: Instant, airtime: Duration) {
        let end = start + airtime;
        let from = end
            .as_micros()
            .saturating_sub(DUTY_CYCLE_WINDOW.as_micros());
        let mut spent = airtime.as_micros();
        for &(s, a) in &self.nodes[i].airtime {
            let (s, e) = (s.as_micros(), s.as_micros() + a.as_micros());
            if e > from {
                spent += e - s.max(from);
            }
        }
        assert!(
            spent <= BUDGET.as_micros(),
            "core {i} exceeds the duty cycle: {spent} µs in an hour"
        );
    }
}

pub fn secs(s: u64) -> Duration {
    Duration::from_secs(s)
}

pub fn at(s: u64) -> Instant {
    Instant::ZERO + Duration::from_secs(s)
}

pub fn ch(i: u8) -> Channel {
    Channel::new(i).unwrap()
}
