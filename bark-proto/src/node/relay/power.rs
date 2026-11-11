//! A node's role by its power: relay on external power, sleepy on battery,
//! with the grace, the battery levels, the flush of a relay that leaves and
//! a role a person forced (spec "Nodes", "Sleepy nodes"). The battery's
//! readings are `relay/battery.rs`'s. A [`SleepyNode`](super::SleepyNode)
//! has no room for the mail a relay holds, and stays sleepy whatever its
//! power.

use super::{Mode, Node, Power, Sleep, Stage};

/// The role a person forced on a node (spec "Nodes").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Forced {
    /// A relay, whatever its power, until the power changes or its
    /// battery is low.
    Relay,
    /// Sleepy, whatever its power, until the power changes.
    Sleepy,
}
use crate::drain::Running;
use crate::event::{Action, AppEvent, Trace};
use crate::node::heartbeat_interval;
use crate::tables::send_queue::{Outgoing, State};
use crate::time::{Duration, Instant};

impl<const H: usize, const Q: usize> Node<H, Q> {
    /// Whether this node is sleepy now.
    pub fn sleepy(&self) -> bool {
        matches!(self.mode, Mode::Sleepy(_))
    }

    /// The role a person forced on the node, while it lasts.
    pub fn forced(&self) -> Option<Forced> {
        self.battery.forced()
    }

    /// Whether this relay on battery is to turn sleepy: it holds no mail,
    /// and flushes what it holds first.
    pub fn leaving(&self) -> bool {
        matches!(self.mode, Mode::Relay(Power::Leaving(_)))
    }

    /// Since when a relay has been on battery, while its grace lasts;
    /// once it is leaving, since when it has been leaving.
    pub(super) fn unplugged(&self) -> Option<Instant> {
        match self.mode {
            Mode::Relay(Power::Battery(t) | Power::Leaving(t)) => Some(t),
            Mode::Relay(Power::External) | Mode::Sleepy(_) => None,
        }
    }

    /// Whether this node has room for the mail a relay holds.
    pub(super) const fn can_relay() -> bool {
        H > 0 && Q > 0
    }

    /// When the first rotation that began at `from` ends; two for a sleepy
    /// node, as a listening receiver misses one heartbeat in ten
    /// (spec "Channel at boot").
    pub(super) fn boot_end(&self, from: Instant) -> Instant {
        let rotations = if self.sleepy() { 2 } else { 1 };
        from + self.config.boot_listen.mul(rotations)
    }

    pub(super) fn power(&mut self, external: bool, now: Instant, emit: &mut dyn FnMut(Action)) {
        let sleepy = !external || !Self::can_relay();
        self.battery.power(external, now);
        if self.battery.forced().is_some() {
            return;
        }
        if self.stage == Stage::Booting {
            // A fixed relay rebooted on battery relays on, its grace
            // counted from the boot.
            let kept = self.config.battery.is_some()
                && Self::can_relay()
                && self.config.stored.is_some_and(|s| s.relay && !s.carried);
            let turns = sleepy && !kept;
            let was = self.sleepy();
            // Nobody depends on a node that boots, so its mode follows
            // each report.
            self.mode = match (turns, was) {
                (true, true) => self.mode,
                (true, false) => Mode::Sleepy(Sleep::new(false)),
                (false, _) if sleepy => Mode::Relay(Power::Battery(now)),
                (false, _) => Mode::Relay(Power::External),
            };
            let ends = self.boot_end(self.boot.from());
            self.boot.ends_at(ends);
            let anchor = self.config.stored.and_then(|s| s.anchor);
            if let Some(a) = anchor.filter(|_| sleepy && self.anchor().is_none()) {
                self.resume_anchor(a, now, emit);
            } else {
                emit(Action::App(AppEvent::Boot {
                    relays: self.relays_known(now),
                    ends: Some(ends),
                }));
            }
            return;
        }
        match (sleepy, self.mode) {
            (false, Mode::Sleepy(_)) => self.turn_relay(Power::External, now, emit),
            // Back within the grace, or during the flush: it holds mail
            // again, and what was flushed stays queued.
            (false, Mode::Relay(_)) => self.mode = Mode::Relay(Power::External),
            (true, Mode::Relay(Power::External)) => self.mode = Mode::Relay(Power::Battery(now)),
            (true, _) => {}
        }
    }

    /// A person holds the button: the node flips its role, and is forced
    /// until the power changes (spec "Nodes").
    pub(super) fn force(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        if self.stage == Stage::Off {
            return;
        }
        let external = self.battery.on_usb();
        let booting = self.stage == Stage::Booting;
        let sleepy = !self.sleepy();
        if sleepy {
            match self.mode {
                Mode::Relay(Power::Leaving(_)) | Mode::Sleepy(_) => {}
                // Nobody depends on a node that boots (`power`).
                Mode::Relay(_) if booting => self.mode = Mode::Sleepy(Sleep::new(false)),
                Mode::Relay(_) => {
                    self.forwarding.feed_due = true;
                    self.mode = Mode::Relay(Power::Leaving(now));
                }
            }
        } else {
            if !Self::can_relay() || (!external && self.battery.low()) {
                return;
            }
            let power = if external {
                Power::External
            } else {
                Power::Battery(now)
            };
            match self.mode {
                Mode::Sleepy(_) if !booting => self.turn_relay(power, now, emit),
                _ => self.mode = Mode::Relay(power),
            }
        }
        self.battery.force(Some(if sleepy {
            Forced::Sleepy
        } else {
            Forced::Relay
        }));
        emit(Action::Trace(Trace::Forced { sleepy }));
    }

    pub(super) fn battery_read(&mut self, raw_mv: u16, now: Instant, emit: &mut dyn FnMut(Action)) {
        let running = self.running();
        self.battery
            .read(raw_mv, self.config.battery, running, now, emit);
    }

    /// How the node runs now, for learning its drain; `None` while it
    /// leaves, which lasts a TTL at most, and while a relay's GPS tracks,
    /// whose 29 mA would be taken for the relay's own drain.
    fn running(&self) -> Option<Running> {
        match self.mode {
            Mode::Sleepy(_) => Some(Running::Sleepy),
            Mode::Relay(Power::Battery(_)) if !self.gps_tracks() => Some(Running::Relay),
            Mode::Relay(_) => None,
        }
    }

    /// How long the node can go on as it runs now (spec "Sleepy nodes").
    /// `None` on external power, before a reading, past the time, and
    /// without battery levels.
    pub fn battery_left(&self, now: Instant) -> Option<Duration> {
        // Without levels it follows their rules, not its drain.
        self.config.battery?;
        let relay = matches!(self.mode, Mode::Relay(Power::Battery(_)));
        self.battery.left(relay, now)
    }

    /// The drains learned, a relay's and a sleepy node's, in percent of
    /// the cell an hour.
    pub fn battery_drains(&self) -> [f32; 2] {
        self.battery.drains()
    }

    /// A driver's battery reading, calibrated once there is a calibration.
    pub fn battery_millivolts(&self, raw_mv: u16) -> u16 {
        self.battery.millivolts(raw_mv)
    }

    /// When the grace of a fixed relay on battery ends, if its end would
    /// turn the node.
    pub(super) fn grace_ends(&self) -> Option<Instant> {
        let Mode::Relay(Power::Battery(t)) = self.mode else {
            return None;
        };
        let ends = self.config.battery.is_none()
            && !self.motion.carried()
            && self.battery.forced().is_none();
        ends.then_some(t + self.config.grace)
    }

    pub(super) fn flush_ends(&self) -> Option<Instant> {
        let Mode::Relay(Power::Leaving(since)) = self.mode else {
            return None;
        };
        Some(since + self.config.delivery_ttl)
    }

    pub(super) fn power_wakeup(&self) -> Option<Instant> {
        crate::time::earlier(self.grace_ends(), self.flush_ends())
    }

    /// The role's timers and levels (spec "Sleepy nodes").
    pub(super) fn power_timers(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        if self.stage != Stage::Running {
            return;
        }
        match self.mode {
            Mode::Sleepy(s) => {
                let free = self.battery.forced().is_none();
                if free && s.by_battery && !self.battery.low() && !self.motion.carried() {
                    self.turn_relay(Power::Battery(now), now, emit);
                } else if !self.delivery.is_empty() {
                    // Room came in the send queue, as a hand-over ended.
                    self.hand_on(now, emit);
                }
                return;
            }
            Mode::Relay(Power::Leaving(_)) => {}
            Mode::Relay(power) => {
                let on_battery = power != Power::External;
                let leaves = if on_battery && self.battery.low() {
                    emit(Action::Trace(Trace::BatteryLow));
                    self.battery.force(None);
                    true
                } else if self.forced() == Some(Forced::Relay) {
                    false
                } else if on_battery && self.motion.carried() {
                    emit(Action::Trace(Trace::CarriedOnBattery));
                    true
                } else if self.grace_ends().is_some_and(|t| now >= t) {
                    emit(Action::Trace(Trace::GraceEnded));
                    true
                } else {
                    false
                };
                if leaves {
                    self.forwarding.feed_due = true;
                    // The flush lasts a delivery TTL at most: held up longer
                    // by the duty cap or a busy channel, a node with a low
                    // battery would listen until it is flat.
                    self.mode = Mode::Relay(Power::Leaving(now));
                }
            }
        }
        if self.leaving() && (self.flushed() || self.flush_ends().is_some_and(|t| now >= t)) {
            self.turn_sleepy(now, emit);
        }
    }

    /// Whether each message of the delivery queue has had its flush, but
    /// those that wait for a neighbour, which the turn hands on, and no
    /// hand-over to a sleepy node or of the flush is queued.
    fn flushed(&self) -> bool {
        !self.delivery.iter().any(|q| !q.flushed && !q.waits())
            && !self.queue.iter().any(|o| o.kind.delivers())
    }

    /// Plugged in, or its battery high again: a relay at once.
    fn turn_relay(&mut self, power: Power, now: Instant, emit: &mut dyn FnMut(Action)) {
        self.mode = Mode::Relay(power);
        // Its anchor went with the mode.
        self.replane();
        self.gps.relay();
        self.fixed_here(now);
        self.unwatch();
        self.neighbours.pin(None);
        // Its slots at a random phase, as at boot, without the first
        // rotation's burst: its neighbours know it.
        let interval = heartbeat_interval(self.id, &self.config);
        let at = now + self.rng.duration(Duration::ZERO, interval);
        self.radio.beacon.beat(at, 0);
        self.radio.beacon.burst(0);
        self.radio.restart_sweep(now);
        emit(Action::Listen(self.home));
        emit(Action::Trace(Trace::Role { sleepy: false }));
        self.changed();
    }

    /// Sleepy, holding nothing, with an anchor chosen as at the end of a
    /// first rotation.
    fn turn_sleepy(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        self.mode = Mode::Sleepy(Sleep::new(
            self.battery.low() && self.battery.forced().is_none(),
        ));
        self.gps.sleepy();
        self.heard.clear();
        let held = self.queue.iter().position(Outgoing::held);
        if let Some(h) = held.and_then(|i| self.queue.remove(i)) {
            // Its own message for a node it held for: routed on, as a
            // sleepy node owns no other node's message.
            let _ = self.queue.push(Outgoing {
                state: State::Queued,
                ..h
            });
        }
        self.radio.clear_move();
        self.radio.beacon.sleepy();
        self.unwatch();
        emit(Action::Trace(Trace::Role { sleepy: true }));
        // Said on the channel its neighbours and sleepy nodes know it by.
        let old = self.home;
        self.radio.beacon.no_change();
        self.changed();
        if !self.reconsider_anchor(now, emit) {
            self.leave_anchor(now);
        }
        if self.home == old && self.anchor().is_some() {
            // Its first beat, at once on the same channel, says it.
            self.radio.beacon.no_change();
        }
        // A turn is no end of a message (spec "Overload waits").
        self.hand_on(now, emit);
    }

    fn unwatch(&mut self) {
        self.listen.forget();
        if let Some(s) = self.sleep_mut() {
            s.misses = 0;
            s.dark = false;
        }
    }
}
