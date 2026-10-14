//! The relay's medium access: boot listen and home channel (design §6),
//! the busy sweep (§6.1), the heartbeat (§7), the neighbour table (§8)
//! and the duty-cycle budget (§3, §9.4).
//!
//! # Radio discipline
//!
//! Each use of the radio is a job with its own small state machine: the
//! sweep and the heartbeat, so far. The relay keeps at most one excursion
//! outstanding (`docs/core-interface.md` §3 rule 1). When the radio is free
//! it starts the first job that is ready, heartbeat before sweep; a job
//! that has begun a sequence of excursions (CAD, RSSI, transmit; or the
//! thirteen sweep readings) continues it from each completion event, so
//! sequences never interleave. Jobs that are not ready say when they will
//! be, and the earliest of those is `next_wakeup`.

use crate::event::{Action, AppCommand, Event, Frame, Trace};
use crate::geo::LocalPlane;
use crate::node::mac::{self, Radio};
use crate::node::{Config, Core, Rng};
use crate::position::Position;
use crate::radio::{time_on_air, Channel, CHANNELS, MIN_PREAMBLE_SYMBOLS};
use crate::tables::budget::RelayBudget;
use crate::tables::busy::{percent, BusyEstimator, ONE};
use crate::tables::neighbours::{Neighbour, NeighbourParams, Neighbours};
use crate::time::{Duration, Instant};
use crate::wire::{Heartbeat, Packet};

/// Who started the outstanding excursion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Job {
    Sweep,
    Heartbeat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    /// Not started: the driver's first `Wakeup` starts it.
    Off,
    /// Listening before choosing a home channel (design §6.2), until then.
    Booting(Instant),
    Running,
}

/// Listen-before-talk progress of one heartbeat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Check {
    Idle,
    Cad,
    Rssi,
    Tx,
}

/// The once-a-second thirteen-channel RSSI sweep (design §6.1).
struct Sweep {
    next_at: Instant,
    /// The channel being read, while a sweep is in progress.
    reading: Option<Channel>,
    /// When the home channel was last read.
    last_home: Instant,
    /// Time spent receiving on the home channel since `last_home`.
    receiving: Duration,
}

/// The rotating heartbeat (design §7) and the one-off announcement on the
/// old channel after a move (§6.3).
struct Beacon {
    next_at: Instant,
    /// Not before this: a listen-before-talk backoff or a budget deferral.
    ready_at: Instant,
    rotation: Channel,
    /// The old home, to announce the new one on, after a move.
    announce: Option<Channel>,
    /// The heartbeat in progress: its channel, and whether it is the announcement.
    current: Option<(Channel, bool)>,
    busy_checks: u8,
    check: Check,
}

/// The move rule's timers (design §6.3).
#[derive(Default)]
struct Mover {
    excess_since: Option<Instant>,
    move_at: Option<Instant>,
}

/// Relay role.
pub struct Relay {
    config: Config,
    plane: LocalPlane,
    rng: Rng,
    home: Channel,
    stage: Stage,
    radio: Radio<Job>,
    busy: BusyEstimator,
    budget: RelayBudget,
    neighbours: Neighbours,
    sweep: Sweep,
    beacon: Beacon,
    mover: Mover,
}

impl Relay {
    /// Create a relay. `seed` drives every random choice the relay makes;
    /// two relays with the same config and seed behave identically.
    pub fn new(config: Config, seed: u64) -> Relay {
        let mut rng = Rng::new(seed);
        let home = config.home_override.unwrap_or_else(|| random_channel(&mut rng));
        let rotation = random_channel(&mut rng);
        Relay {
            plane: LocalPlane::new(config.position),
            home,
            stage: Stage::Off,
            radio: Radio::new(),
            busy: BusyEstimator::from_config(&config),
            budget: RelayBudget::new(),
            neighbours: Neighbours::new(NeighbourParams::from_config(&config)),
            sweep: Sweep { next_at: Instant::ZERO, reading: None, last_home: Instant::ZERO, receiving: Duration::ZERO },
            beacon: Beacon {
                next_at: Instant::ZERO,
                ready_at: Instant::ZERO,
                rotation,
                announce: None,
                current: None,
                busy_checks: 0,
                check: Check::Idle,
            },
            mover: Mover::default(),
            config,
            rng,
        }
    }

    /// Current home channel.
    pub fn home(&self) -> Channel {
        self.home
    }

    /// Configuration. `position` is the relay's identity, which the boot
    /// listen may have moved by a metre (design §8.1).
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Whether the boot listen (design §6.2) is still running.
    pub fn booting(&self) -> bool {
        !matches!(self.stage, Stage::Running)
    }

    /// The neighbour table.
    pub fn neighbours(&self) -> impl Iterator<Item = &Neighbour> {
        self.neighbours.iter()
    }

    /// The busy estimates and noise floor.
    pub fn busy(&self) -> &BusyEstimator {
        &self.busy
    }

    fn start(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        self.stage = Stage::Booting(now + self.config.boot_listen);
        self.sweep.next_at = now;
        self.sweep.last_home = now;
        emit(Action::Listen(self.home));
    }

    /// End of the boot listen: choose home (design §6.2), start heartbeats.
    fn finish_boot(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        self.stage = Stage::Running;
        if self.config.home_override.is_none() {
            let home = self.quietest();
            self.retune(home, emit);
        }
        self.beacon.next_at = now + self.heartbeat_interval();
    }

    fn retune(&mut self, to: Channel, emit: &mut dyn FnMut(Action)) {
        if to != self.home {
            emit(Action::Trace(Trace::HomeChannelMoved { from: self.home, to }));
            self.home = to;
            emit(Action::Listen(to));
        }
    }

    /// `argmin busy[c]`, ties within 0.01 broken at random (design §6.2),
    /// so relays that hear nothing do not all pick channel 0.
    fn quietest(&mut self) -> Channel {
        let min = Channel::all().map(|c| self.busy.fraction(c)).min().unwrap_or(ONE);
        let busy = &self.busy;
        let near = |c: &Channel| busy.fraction(*c) <= min.saturating_add(percent(1));
        let count = Channel::all().filter(near).count() as u64;
        let pick = self.rng.range(0, count.saturating_sub(1)) as usize;
        Channel::all().filter(near).nth(pick).unwrap_or(self.home)
    }

    fn heartbeat_interval(&mut self) -> Duration {
        self.rng.duration(self.config.heartbeat_min, self.config.heartbeat_max)
    }

    fn timers(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        if let Stage::Booting(until) = self.stage {
            if now >= until {
                self.finish_boot(now, emit);
            }
        }
        if self.mover.move_at.is_some_and(|t| now >= t) {
            self.mover = Mover::default();
            // Re-check after the random wait (design §6.3).
            if self.excess() {
                let old = self.home;
                let to = self.quietest();
                self.retune(to, emit);
                if self.home != old {
                    self.beacon.announce = Some(old);
                }
            }
        }
    }

    /// Start whatever is ready until the radio is busy or nothing is.
    fn pump(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        while self.radio.idle() {
            let started = (self.beacon_due(now) && self.start_heartbeat(now, emit))
                || (self.sweep_due(now) && self.start_sweep(emit));
            if !started {
                break;
            }
        }
    }

    // --- Sweep (design §6.1) ---

    fn sweep_due(&self, now: Instant) -> bool {
        self.sweep.reading.is_none() && now >= self.sweep.next_at
    }

    fn start_sweep(&mut self, emit: &mut dyn FnMut(Action)) -> bool {
        self.sweep.next_at += self.config.sweep_interval;
        let first = Channel::all().next().unwrap_or(self.home);
        self.sweep.reading = Some(first);
        self.radio.start(Job::Sweep, Action::Rssi(first), emit);
        true
    }

    fn sweep_sample(&mut self, channel: Channel, dbm: i16, now: Instant, emit: &mut dyn FnMut(Action)) {
        if channel == self.home {
            let interval = now.since(self.sweep.last_home);
            self.busy.sample_home(channel, dbm, self.sweep.receiving, interval);
            self.sweep.receiving = Duration::ZERO;
            self.sweep.last_home = now;
        } else {
            self.busy.sample(channel, dbm);
        }
        if channel.index() + 1 < CHANNELS {
            let next = channel.next();
            self.sweep.reading = Some(next);
            self.radio.start(Job::Sweep, Action::Rssi(next), emit);
            return;
        }
        self.sweep.reading = None;
        // A sweep held up by receptions (core-interface rule 8) must not
        // leave a backlog of sweeps to run back to back.
        if self.sweep.next_at <= now {
            self.sweep.next_at = now + self.config.sweep_interval;
        }
        self.neighbours.expire(now);
        if self.stage == Stage::Running && self.config.home_override.is_none() {
            self.move_rule(now);
        }
    }

    /// `busy[home] − min busy` above the move threshold (design §6.3).
    fn excess(&self) -> bool {
        let min = Channel::all().map(|c| self.busy.fraction(c)).min().unwrap_or(0);
        self.busy.fraction(self.home).saturating_sub(min) > percent(self.config.move_threshold_percent)
    }

    fn move_rule(&mut self, now: Instant) {
        if !self.excess() {
            self.mover = Mover::default();
            return;
        }
        let since = *self.mover.excess_since.get_or_insert(now);
        if self.mover.move_at.is_none() && now.since(since) >= self.config.move_dwell {
            self.mover.move_at = Some(now + self.rng.duration(Duration::ZERO, self.config.move_wait_max));
        }
    }

    // --- Heartbeat (design §7) ---

    fn beacon_due(&self, now: Instant) -> bool {
        self.stage == Stage::Running
            && self.beacon.check == Check::Idle
            && now >= self.beacon.ready_at
            && (self.beacon.current.is_some() || self.beacon.announce.is_some() || now >= self.beacon.next_at)
    }

    /// The heartbeat in progress. It carries the home at the moment it is
    /// sent, so a move during its backoff still announces the new channel.
    fn heartbeat_frame(&self) -> Frame {
        let (channel, _) = self.beacon.current.unwrap_or((self.beacon.rotation, false));
        let hb = Heartbeat { surveyed: self.config.surveyed, home: self.home, position: self.config.position };
        mac::frame(&Packet::Heartbeat(hb), channel, MIN_PREAMBLE_SYMBOLS)
    }

    /// Check the budget before the channel: there is no point listening for
    /// a slot the budget will not let us use. Nothing else transmits
    /// between this check and the transmission, and the window only slides
    /// forward, so the budget cannot have shrunk by then.
    fn start_heartbeat(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) -> bool {
        if self.beacon.current.is_none() {
            self.beacon.current = Some(match self.beacon.announce {
                Some(old) => (old, true),
                None => (self.beacon.rotation, false),
            });
        }
        let f = self.heartbeat_frame();
        let airtime = mac::airtime(&f);
        if !self.budget.would_allow(airtime, now) {
            let until = self.budget.next_change(airtime, now).unwrap_or(now + self.config.heartbeat_max);
            self.beacon.ready_at = until;
            emit(Action::Trace(Trace::HeartbeatDeferred { until }));
            return false;
        }
        self.beacon.check = Check::Cad;
        self.radio.start(Job::Heartbeat, Action::Cad(f.channel), emit);
        true
    }

    fn heartbeat_check(&mut self, busy: bool, now: Instant, emit: &mut dyn FnMut(Action)) {
        let f = self.heartbeat_frame();
        if busy {
            self.beacon.check = Check::Idle;
            self.beacon.busy_checks += 1;
            if self.beacon.busy_checks >= self.config.lbt_attempts {
                emit(Action::Trace(Trace::HeartbeatSkipped { channel: f.channel }));
                self.heartbeat_done(now, false);
            } else {
                self.beacon.ready_at = now + self.rng.duration(Duration::ZERO, self.config.lbt_backoff_max);
            }
            return;
        }
        if self.beacon.check == Check::Cad {
            self.beacon.check = Check::Rssi;
            self.radio.start(Job::Heartbeat, Action::Rssi(f.channel), emit);
        } else {
            self.beacon.check = Check::Tx;
            self.budget.record(mac::airtime(&f), now);
            self.radio.start(Job::Heartbeat, Action::Transmit(f), emit);
        }
    }

    /// The heartbeat went out, or was skipped (design §7). A skipped one
    /// keeps its channel, so the neighbours parked there are not left out
    /// for a whole rotation.
    fn heartbeat_done(&mut self, now: Instant, sent: bool) {
        self.beacon.check = Check::Idle;
        self.beacon.busy_checks = 0;
        if let Some((channel, true)) = self.beacon.current.take() {
            // A second move meanwhile has a newer old channel to announce on.
            if self.beacon.announce == Some(channel) {
                self.beacon.announce = None;
            }
            return;
        }
        if sent {
            self.beacon.rotation = self.beacon.rotation.next();
        }
        self.beacon.next_at = now + self.heartbeat_interval();
    }

    // --- Reception ---

    fn received(&mut self, bytes: &[u8], now: Instant, emit: &mut dyn FnMut(Action)) {
        // The driver does not report the preamble a sender used. Assuming
        // the minimum undercounts only frames for sleeping leaves, which
        // are rare (design §10.5).
        let len = bytes.len().min(u8::MAX as usize) as u8;
        self.sweep.receiving = self.sweep.receiving + time_on_air(len, MIN_PREAMBLE_SYMBOLS);
        // ACK, ATTACH, DATA and DELIVER wait for the routing design to settle.
        if let Ok(Packet::Heartbeat(hb)) = Packet::decode(bytes) {
            self.heard(hb, now, emit);
        }
    }

    fn heard(&mut self, hb: Heartbeat, now: Instant, emit: &mut dyn FnMut(Action)) {
        if hb.position != self.config.position {
            self.neighbours.heard(&hb, &self.plane, now);
            return;
        }
        if let Stage::Booting(_) = self.stage {
            // Another relay holds this identity: step two counts (≈1.2 m)
            // north and listen again (design §8.1).
            let to = Position { lat: hb.position.lat + 2, ..hb.position };
            self.set_position(to);
            self.stage = Stage::Booting(now + self.config.boot_listen);
            emit(Action::Trace(Trace::PositionShifted { to }));
        }
    }

    fn set_position(&mut self, position: Position) {
        self.config.position = position;
        self.plane = LocalPlane::new(position);
        self.neighbours.replane(&self.plane);
    }

    fn app(&mut self, cmd: AppCommand<'_>) {
        // Send waits for the routing design to settle.
        if let AppCommand::SetPosition { position, surveyed } = cmd {
            self.config.surveyed = surveyed;
            self.set_position(position);
        }
    }
}

fn random_channel(rng: &mut Rng) -> Channel {
    let fallback = Channel::all().next().expect("the plan has channels");
    Channel::new(rng.range(0, CHANNELS as u64 - 1) as u8).unwrap_or(fallback)
}

impl Core for Relay {
    fn handle(&mut self, event: Event<'_>, now: Instant, emit: &mut dyn FnMut(Action)) {
        if self.stage == Stage::Off {
            self.start(now, emit);
        }
        let threshold = self.busy.threshold_dbm();
        match event {
            Event::Wakeup => {}
            Event::Received { bytes, .. } => self.received(bytes, now, emit),
            Event::CadDone { busy, .. } => {
                if let Some(Job::Heartbeat) = self.radio.complete() {
                    self.heartbeat_check(busy, now, emit);
                }
            }
            Event::RssiSample { channel, dbm } => match self.radio.complete() {
                Some(Job::Sweep) => self.sweep_sample(channel, dbm, now, emit),
                Some(Job::Heartbeat) => self.heartbeat_check(dbm > threshold, now, emit),
                None => {}
            },
            Event::TxDone => {
                if let Some(Job::Heartbeat) = self.radio.complete() {
                    self.heartbeat_done(now, true);
                }
            }
            Event::App(cmd) => self.app(cmd),
        }
        self.timers(now, emit);
        self.pump(now, emit);
    }

    fn next_wakeup(&self) -> Option<Instant> {
        let mut w: Option<Instant> = None;
        let mut at = |t: Instant| w = Some(w.map_or(t, |w| w.min(t)));
        match self.stage {
            Stage::Off => return Some(Instant::ZERO),
            Stage::Booting(until) => at(until),
            Stage::Running => {}
        }
        if let Some(t) = self.mover.move_at {
            at(t);
        }
        // Radio jobs only once the radio is free: their completion event
        // wakes the core otherwise.
        if self.radio.idle() {
            if self.stage == Stage::Running && self.beacon.check == Check::Idle {
                let pending = self.beacon.announce.is_some() || self.beacon.current.is_some();
                let due = if pending { self.beacon.ready_at } else { self.beacon.next_at };
                at(due.max(self.beacon.ready_at));
            }
            at(self.sweep.next_at);
        }
        w
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        extern crate std;
        // docs/core-interface.md §6: under 48 KiB with every table; until
        // the routing tables join it, the medium access alone is the
        // neighbour table, budget and busy estimate.
        let relay = core::mem::size_of::<Relay>();
        let leaf = core::mem::size_of::<crate::node::Leaf>();
        std::println!("Relay {relay} B, Leaf {leaf} B");
        assert!(relay < 4 * 1024, "{relay}");
        assert!(leaf < 1024, "{leaf}");
    }
}
