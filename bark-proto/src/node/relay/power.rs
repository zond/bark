//! A node's role by its power (spec "Nodes", "Sleepy nodes", roadmap
//! D71).
//!
//! On external power a node is a relay; on battery it is sleepy. The
//! driver reports the power at boot and at every change
//! (`AppCommand::Power`). While the node boots nobody depends on it yet,
//! and a report sets its role at once; a sleepy node handed back an
//! anchor at boot takes it (`store.rs`).
//!
//! Running, a node plugged in is a relay at once: it leaves its anchor,
//! its address is its own position, and it heartbeats as a relay.
//!
//! The power changes only whether the node is sleepy. Whether it is
//! carried is measured from its fixes alike in either role
//! (`relay/motion.rs`): a carried node plugged in is a carried relay, and
//! a fixed one is fixed where it is then.
//!
//! A relay unplugged changes nothing for [`Config::grace`], so that a
//! charger moved across the room costs nobody anything; power back within
//! it, nothing happened. Then it takes no more mail, as a carried node
//! does, and flushes its delivery queue as a node does at its first far
//! fix (`relay/delivery.rs`), listening all the time until each message
//! has had its attempt, or for a delivery TTL at most; then it turns
//! sleepy. A sleepy node holds
//! nothing: what the flush did not deliver is dropped, and it chooses an
//! anchor among the relays it knows as a booting sleepy node does. A node
//! carried by then flushed at its first far fix already.
//!
//! With [`Config::battery`] only a carried relay leaves at its grace's
//! end. A fixed one relays on until its battery reads at or below the low
//! level (`AppCommand::Battery`), and leaves then, grace or not; carried
//! later, it leaves at once if its grace is over. The low reading holds
//! until one at or above the high level, or external power; a node that
//! turned sleepy by its battery relays again at the high level while it
//! is fixed, as a solar box's battery recharges. A fixed relay that
//! reboots on battery relays on, from what it stored (`store.rs`).
//!
//! The driver's readings are off by its converter's gain error until
//! calibrated: a node on external power long enough to be full that is
//! then on battery for a rest takes its next reading as a full cell's,
//! and scales every reading after it to match, keeping the scale across
//! reboots. Before the first calibration it compares the driver's
//! readings as they are.
//!
//! A [`SleepyNode`](super::SleepyNode) has no room for the mail a relay
//! holds, and stays sleepy whatever its power.
//!
//! [`Config::grace`]: crate::Config::grace
//! [`Config::battery`]: crate::Config::battery

use super::{Mode, Node, Power, Sleep, Stage};
use crate::event::{Action, AppEvent, DropReason, Trace};
use crate::node::{
    heartbeat_interval, BATTERY_CHARGED_AFTER, BATTERY_FULL_MV, BATTERY_GAIN_BOUND_PERMILLE,
    BATTERY_REST, BATTERY_SCALE_ONE,
};
use crate::tables::send_queue::{Outgoing, State};
use crate::time::{Duration, Instant};

/// What a node's power says of its cell (module documentation), with
/// times in whole seconds of the node's clock, which a node's 1 KiB
/// around its tables holds in half the room of an `Instant` (core-interface
/// §6); a u32 of seconds lasts 136 years.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Charge {
    /// On external power since then.
    Usb(u32),
    /// On battery.
    Battery,
    /// On battery after a full charge: its first reading from then is a
    /// full cell's, rested.
    Rested(u32),
}

impl Charge {
    fn secs(t: Instant) -> u32 {
        Self::secs_of(t.since(Instant::ZERO))
    }

    fn secs_of(d: Duration) -> u32 {
        (d.as_micros() / 1_000_000) as u32
    }
}

/// What the node knows of its battery (module documentation).
#[derive(Clone, Copy, Debug)]
pub(super) struct Battery {
    /// Read at or below the low level since it last read at or above the
    /// high one, or since its external power.
    pub(super) low: bool,
    /// The scale of the driver's readings ([`BATTERY_SCALE_ONE`] is one);
    /// `None` before the first calibration.
    pub(super) scale: Option<u16>,
    /// Since when it has been on external power as last reported, or
    /// when on battery its next reading is taken as a full cell's.
    pub(super) charge: Charge,
}

impl Battery {
    /// At boot: on battery, not low, with the scale kept from before the
    /// reboot if any.
    pub(super) const fn at_boot(scale: Option<u16>) -> Battery {
        Battery {
            low: false,
            scale,
            charge: Charge::Battery,
        }
    }
}

impl<const H: usize, const Q: usize> Node<H, Q> {
    /// Whether this node is sleepy now (module documentation).
    pub fn sleepy(&self) -> bool {
        matches!(self.mode, Mode::Sleepy(_))
    }

    /// Whether this relay's grace on battery is over: it holds no mail,
    /// and flushes what it holds before it turns sleepy.
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

    /// When the first rotation that began at `from` ends: a worst-case
    /// rotation, by when every relay in range has been heard, since every
    /// relay heartbeats from its own boot; for a sleepy node that hears
    /// none, two, since a listening receiver misses one heartbeat in ten
    /// (spec "Channel at boot").
    pub(super) fn boot_end(&self, from: Instant) -> Instant {
        let rotations = if self.sleepy() { 2 } else { 1 };
        from + self.config.boot_listen.mul(rotations)
    }

    /// The driver reports whether external power is present.
    pub(super) fn power(&mut self, external: bool, now: Instant, emit: &mut dyn FnMut(Action)) {
        let sleepy = !external || !Self::can_relay();
        let s = Charge::secs(now);
        self.battery.charge = match (external, self.battery.charge) {
            (true, Charge::Usb(since)) => Charge::Usb(since),
            (true, _) => {
                self.battery.low = false;
                Charge::Usb(s)
            }
            // Long enough on the charger to be full: its first reading
            // after a rest calibrates.
            (false, Charge::Usb(since)) if s >= since + Charge::secs_of(BATTERY_CHARGED_AFTER) => {
                Charge::Rested(s + Charge::secs_of(BATTERY_REST))
            }
            (false, Charge::Usb(_)) => Charge::Battery,
            (false, c) => c,
        };
        if self.stage == Stage::Booting {
            // A fixed relay rebooted on battery relays on, its grace
            // counted from the boot.
            let kept = self.config.battery.is_some()
                && Self::can_relay()
                && self.config.stored.is_some_and(|s| s.relay && !s.carried);
            let turns = sleepy && !kept;
            let was = self.sleepy();
            // Nobody depends on a node that boots, so its mode follows
            // each report; a sleepy node keeps what it has.
            self.mode = match (turns, was) {
                (true, true) => self.mode,
                (true, false) => Mode::Sleepy(Sleep::new(self.config.position, false)),
                (false, _) if sleepy => Mode::Relay(Power::Battery(now)),
                (false, _) => Mode::Relay(Power::External),
            };
            if turns && !was {
                self.gps_due();
            }
            let ends = self.boot_end(self.timers.boot_from);
            self.timers.boot_ends = Some(ends);
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

    /// The driver reads the battery: the first reading a rest after a full
    /// charge calibrates the readings, and the levels apply to the
    /// calibrated one (module documentation).
    pub(super) fn battery_read(&mut self, raw_mv: u16, now: Instant) {
        let Some(levels) = self.config.battery else {
            return;
        };
        if matches!(self.battery.charge, Charge::Usb(_)) || raw_mv == 0 {
            // On external power the reading is the charger's.
            return;
        }
        if matches!(self.battery.charge, Charge::Rested(t) if Charge::secs(now) >= t) {
            self.battery.charge = Charge::Battery;
            let scale = u32::from(BATTERY_FULL_MV) * BATTERY_SCALE_ONE / u32::from(raw_mv);
            let bound = BATTERY_SCALE_ONE * BATTERY_GAIN_BOUND_PERMILLE / 1000;
            if scale.abs_diff(BATTERY_SCALE_ONE) <= bound {
                self.battery.scale = Some(scale as u16);
            }
        }
        let millivolts = match self.battery.scale {
            Some(s) => (u32::from(raw_mv) * u32::from(s) / BATTERY_SCALE_ONE) as u16,
            None => raw_mv,
        };
        if millivolts <= levels.low_mv {
            self.battery.low = true;
        } else if millivolts >= levels.high_mv {
            self.battery.low = false;
        }
    }

    /// When the grace of a relay on battery ends, if it is under way and
    /// its end would turn the node (module documentation).
    pub(super) fn grace_ends(&self) -> Option<Instant> {
        let Mode::Relay(Power::Battery(t)) = self.mode else {
            return None;
        };
        let ends = self.config.battery.is_none() || self.motion.carried;
        ends.then_some(t + self.config.grace)
    }

    /// When the flush of a relay leaving ends at the latest (module
    /// documentation).
    pub(super) fn flush_ends(&self) -> Option<Instant> {
        let Mode::Relay(Power::Leaving(since)) = self.mode else {
            return None;
        };
        Some(since + self.config.delivery_ttl)
    }

    /// The grace's end, a low battery, and the turn to sleepy once the
    /// flush is done; a node turned sleepy by its battery relays again at
    /// the high level (module documentation).
    pub(super) fn power_timers(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        if self.stage != Stage::Running {
            return;
        }
        match self.mode {
            Mode::Sleepy(s) => {
                if s.by_battery && !self.battery.low && !self.motion.carried {
                    self.turn_relay(Power::Battery(now), now, emit);
                }
                return;
            }
            Mode::Relay(Power::Leaving(_)) => {}
            Mode::Relay(power) => {
                let leaves = if power != Power::External && self.battery.low {
                    emit(Action::Trace(Trace::BatteryLow));
                    true
                } else if self.grace_ends().is_some_and(|t| now >= t) {
                    emit(Action::Trace(Trace::GraceEnded));
                    true
                } else {
                    false
                };
                if leaves {
                    self.feed_due = true;
                    // What the network promises a node it cannot reach is a
                    // delivery TTL; a flush that the duty cap or a busy
                    // channel holds up longer would keep a node with a low
                    // battery listening until it is flat.
                    self.mode = Mode::Relay(Power::Leaving(now));
                }
            }
        }
        if self.leaving() && (self.flushed() || self.flush_ends().is_some_and(|t| now >= t)) {
            self.turn_sleepy(now, emit);
        }
    }

    /// Whether each message of the delivery queue has had its flush,
    /// and no hand-over to a sleepy node or of the flush is queued.
    fn flushed(&self) -> bool {
        !self.delivery.iter().any(|q| !q.flushed) && !self.queue.iter().any(|o| o.kind.delivers())
    }

    /// Plugged in, or its battery high again: a relay at once, on `power`,
    /// with its own position as its address.
    fn turn_relay(&mut self, power: Power, now: Instant, emit: &mut dyn FnMut(Action)) {
        self.mode = Mode::Relay(power);
        self.gps_relay();
        self.fixed_here(now);
        self.unwatch();
        self.neighbours.pin(None);
        // Its slots at a random phase, as at boot, without the first
        // rotation's burst: its neighbours know it.
        let interval = heartbeat_interval(self.id, &self.config);
        self.beacon.next_at = now + self.rng.duration(Duration::ZERO, interval);
        self.beacon.quick = 0;
        self.beacon.burst = 0;
        self.sweep.next_at = now;
        // It listens all the time from now on.
        emit(Action::Listen(self.home));
        emit(Action::Trace(Trace::Role { sleepy: false }));
        self.changed();
    }

    /// Sleepy, holding nothing, with an anchor chosen as at the end of a
    /// first rotation.
    fn turn_sleepy(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        self.mode = Mode::Sleepy(Sleep::new(self.config.position, self.battery.low));
        self.gps_sleepy();
        for q in self.delivery.iter() {
            emit(Action::Trace(Trace::Dropped {
                msg_id: q.msg_id,
                reason: DropReason::Unplugged,
            }));
        }
        self.delivery.clear();
        self.heard.clear();
        let held = self.queue.iter().position(Outgoing::held);
        if let Some(h) = held.and_then(|i| self.queue.remove(i)) {
            // Its own message for a node it held for: routed on, as a
            // sleepy node owns no other node's message, behind what is
            // queued.
            let _ = self.queue.push(Outgoing {
                state: State::Queued,
                ..h
            });
        }
        self.timers.clear_move();
        self.beacon.announce = None;
        self.beacon.reach_all = None;
        self.beacon.reach_left = 0;
        self.unwatch();
        emit(Action::Trace(Trace::Role { sleepy: true }));
        // Said on the channel its neighbours and sleepy nodes know it by.
        let old = self.home;
        self.beacon.change = None;
        self.changed();
        if !self.reconsider_anchor(now, emit) {
            self.leave_anchor(now);
        }
        if self.home == old && self.anchor().is_some() {
            // Its first beat, at once on the same channel, says it.
            self.beacon.change = None;
        }
    }

    /// Forget whatever a sleepy node listens for: its windows, the watch
    /// for its anchor's heartbeat, a choice or a check under way.
    fn unwatch(&mut self) {
        self.timers.awake_until = Instant::ZERO;
        self.timers.choosing = None;
        self.timers.rescan_ends = None;
        self.timers.watch_at = None;
        if let Some(s) = self.sleep_mut() {
            s.misses = 0;
            s.dark = false;
        }
        self.timers.window_ends = None;
    }
}
