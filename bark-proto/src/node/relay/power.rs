//! A node's role by its power (spec "Hardware", "Sleepy nodes", roadmap
//! D71).
//!
//! On external power a node is a relay; on battery it is sleepy. The
//! driver reports the power at boot and at every change
//! (`AppCommand::Power`). While the node boots nobody depends on it yet,
//! and a report sets its role at once, its boot listen as long as that
//! role's.
//!
//! Running, a node plugged in is a relay at once: it leaves its anchor,
//! its address is its own position, and it heartbeats as a relay.
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
//! A [`SleepyNode`](super::SleepyNode) has no room for the mail a relay
//! holds, and stays sleepy whatever its power.
//!
//! [`Config::grace`]: crate::Config::grace

use super::{Mover, Node, Stage};
use crate::event::{Action, DropReason, Trace};
use crate::node::heartbeat_interval;
use crate::node::relay::motion::Motion;
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

    /// When the boot listen that began at `from` ends: a worst-case
    /// rotation, and for a sleepy node one more (spec "Channel at boot").
    pub(super) fn boot_end(&self, from: Instant) -> Instant {
        let rotations = if self.sleepy { 2 } else { 1 };
        from + self.config.boot_listen.mul(rotations)
    }

    /// The driver reports whether external power is present.
    pub(super) fn power(&mut self, external: bool, now: Instant, emit: &mut dyn FnMut(Action)) {
        let sleepy = !external || !Self::can_relay();
        if let Stage::Booting(_) = self.stage {
            self.sleepy = sleepy;
            self.stage = Stage::Booting(self.boot_end(self.boot_from));
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

    /// When the grace of a relay on battery ends, if it is under way.
    pub(super) fn grace_ends(&self) -> Option<Instant> {
        let t = self.unplugged.filter(|_| !self.leaving)?;
        Some(t + self.config.grace)
    }

    /// The grace's end and the turn to sleepy once the flush is done
    /// (module documentation).
    pub(super) fn power_timers(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        if self.stage != Stage::Running || self.sleepy {
            return;
        }
        if self.grace_ends().is_some_and(|t| now >= t) {
            self.leaving = true;
            self.feed_due = true;
            emit(Action::Trace(Trace::GraceEnded));
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
        self.anchor = None;
        self.unwatch();
        self.neighbours.pin(None);
        // Its slots as at the end of a boot listen, at a random phase.
        let interval = heartbeat_interval(self.id, &self.config);
        self.beacon.next_at = now + self.rng.duration(Duration::ZERO, interval);
        self.beacon.quick = 0;
        self.sweep.next_at = now;
        // It listens all the time from now on.
        emit(Action::Listen(self.home));
        emit(Action::Trace(Trace::Role { sleepy: false }));
        self.changed();
    }

    /// Sleepy, holding nothing, with an anchor chosen as at the end of a
    /// boot listen.
    fn turn_sleepy(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        self.sleepy = true;
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
        self.motion = Motion::FIXED;
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
