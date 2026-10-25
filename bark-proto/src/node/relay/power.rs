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
//! has had its attempt; then it turns sleepy. A sleepy node holds
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
//! A [`SleepyNode`](super::SleepyNode) has no room for the mail a relay
//! holds, and stays sleepy whatever its power.
//!
//! [`Config::grace`]: crate::Config::grace
//! [`Config::battery`]: crate::Config::battery

use super::{Mover, Node, Stage};
use crate::event::{Action, AppEvent, DropReason, Trace};
use crate::node::heartbeat_interval;
use crate::time::{Duration, Instant};

impl<const H: usize, const Q: usize> Node<H, Q> {
    /// Whether this node is sleepy now (module documentation).
    pub fn sleepy(&self) -> bool {
        self.sleepy
    }

    /// Whether this relay's grace on battery is over: it holds no mail,
    /// and flushes what it holds before it turns sleepy.
    pub fn leaving(&self) -> bool {
        self.leaving
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
        let rotations = if self.sleepy { 2 } else { 1 };
        from + self.config.boot_listen.mul(rotations)
    }

    /// The driver reports whether external power is present.
    pub(super) fn power(&mut self, external: bool, now: Instant, emit: &mut dyn FnMut(Action)) {
        let sleepy = !external || !Self::can_relay();
        if external {
            self.low = false;
        }
        if let Stage::Booting(_) = self.stage {
            // A fixed relay rebooted on battery relays on, its grace
            // counted from the boot.
            let kept = self.config.battery.is_some()
                && Self::can_relay()
                && self.config.stored.is_some_and(|s| s.relay && !s.carried);
            self.sleepy = sleepy && !kept;
            self.unplugged = (sleepy && kept).then_some(now);
            let ends = self.boot_end(self.boot_from);
            self.stage = Stage::Booting(ends);
            let anchor = self.config.stored.and_then(|s| s.anchor);
            if let Some(a) = anchor.filter(|_| sleepy && self.anchor.is_none()) {
                self.resume_anchor(a, now, emit);
            } else {
                emit(Action::App(AppEvent::Boot {
                    relays: self.relays_known(now),
                    ends: Some(ends),
                }));
            }
            return;
        }
        match (sleepy, self.sleepy) {
            (false, true) => self.turn_relay(now, emit),
            // Back within the grace, or during the flush: it holds mail
            // again, and what was flushed stays queued.
            (false, false) => {
                self.unplugged = None;
                self.leaving = false;
            }
            (true, false) => {
                self.unplugged.get_or_insert(now);
            }
            (true, true) => {}
        }
    }

    /// The driver reads the battery (module documentation).
    pub(super) fn battery(&mut self, millivolts: u16) {
        let Some(levels) = self.config.battery else {
            return;
        };
        if !self.sleepy && self.unplugged.is_none() {
            // On external power: the reading is the charger's.
            return;
        }
        if millivolts <= levels.low_mv {
            self.low = true;
        } else if millivolts >= levels.high_mv {
            self.low = false;
        }
    }

    /// When the grace of a relay on battery ends, if it is under way and
    /// its end would turn the node (module documentation).
    pub(super) fn grace_ends(&self) -> Option<Instant> {
        let t = self.unplugged.filter(|_| !self.leaving)?;
        let ends = self.config.battery.is_none() || self.motion.carried;
        ends.then_some(t + self.config.grace)
    }

    /// The grace's end, a low battery, and the turn to sleepy once the
    /// flush is done; a node turned sleepy by its battery relays again at
    /// the high level (module documentation).
    pub(super) fn power_timers(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        if self.stage != Stage::Running {
            return;
        }
        if self.sleepy {
            if self.battery_sleepy && !self.low && !self.motion.carried {
                self.turn_relay(now, emit);
                self.unplugged = Some(now);
            }
            return;
        }
        if !self.leaving {
            if self.unplugged.is_some() && self.low {
                self.leaving = true;
                emit(Action::Trace(Trace::BatteryLow));
            } else if self.grace_ends().is_some_and(|t| now >= t) {
                self.leaving = true;
                emit(Action::Trace(Trace::GraceEnded));
            }
            self.feed_due |= self.leaving;
        }
        if self.leaving && self.flushed() {
            self.turn_sleepy(now, emit);
        }
    }

    /// Whether each message of the delivery queue has had its flush,
    /// and no hand-over to a sleepy node or of the flush is queued.
    fn flushed(&self) -> bool {
        !self.delivery.iter().any(|q| !q.flushed) && !self.queue.iter().any(|o| o.kind.delivers())
    }

    /// Plugged in: a relay at once, with its own position as its address.
    fn turn_relay(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        self.sleepy = false;
        self.battery_sleepy = false;
        self.fixed_here();
        self.anchor = None;
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
        self.sleepy = true;
        self.battery_sleepy = self.low;
        self.unplugged = None;
        self.leaving = false;
        for q in self.delivery.iter() {
            emit(Action::Trace(Trace::Dropped {
                msg_id: q.msg_id,
                reason: DropReason::Unplugged,
            }));
        }
        self.delivery.clear();
        self.heard.clear();
        if let Some(h) = self.held.take() {
            // Its own message for a node it held for: routed on, as a
            // sleepy node owns no other node's message.
            if let Some(msg_id) = h.out.msg_id().filter(|_| self.queue.push(h.out).is_err()) {
                emit(Action::Trace(Trace::Dropped {
                    msg_id,
                    reason: DropReason::SendQueueFull,
                }));
            }
        }
        self.mover = Mover::default();
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
        if self.home == old && self.anchor.is_some() {
            // Its first beat, at once on the same channel, says it.
            self.beacon.change = None;
        }
    }

    /// Forget whatever a sleepy node listens for: its windows, the watch
    /// for its anchor's heartbeat, a choice or a check under way.
    fn unwatch(&mut self) {
        self.awake_until = Instant::ZERO;
        self.choosing = None;
        self.rescan = None;
        self.watch_at = None;
        self.misses = 0;
        self.dark = false;
        self.window_ends = None;
    }
}
