//! A node's role by its power (spec "Hardware", "Sleepy nodes", roadmap
//! D71).
//!
//! On external power a node is a relay; on battery it is sleepy. The
//! driver reports the power at boot and at every change
//! (`AppCommand::Power`). While the node boots nobody depends on it yet,
//! and a report sets its role at once, its boot listen as long as that
//! role's. Running, a node plugged in is a relay at once: it leaves its
//! anchor, its address is its own position, and it heartbeats as a relay.
//! A node unplugged turns sleepy: a sleepy node holds nothing, so what it
//! holds for its sleepy nodes is dropped, and it chooses an anchor among
//! the relays it knows as a booting sleepy node does.
//!
//! A [`SleepyNode`](super::SleepyNode) has no room for the mail a relay
//! holds, and stays sleepy whatever its power.

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
        if sleepy == self.sleepy {
            return;
        }
        if let Stage::Booting(_) = self.stage {
            self.sleepy = sleepy;
            self.stage = Stage::Booting(self.boot_end(self.boot_from));
            return;
        }
        if sleepy {
            self.turn_sleepy(now, emit);
        } else {
            self.turn_relay(now, emit);
        }
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
    }

    /// Unplugged: sleepy, holding nothing, with an anchor chosen as at
    /// the end of a boot listen.
    fn turn_sleepy(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        self.sleepy = true;
        for q in self.delivery.iter() {
            emit(Action::Trace(Trace::Dropped {
                msg_id: q.msg_id,
                reason: DropReason::Unplugged,
            }));
        }
        self.delivery.clear();
        self.heard.clear();
        if self.queue.current().is_some_and(|o| o.kind.delivers()) {
            self.handover.cancel();
            self.skipped = 0;
        }
        self.queue.retain(|_, o| !o.kind.delivers());
        if let Some(h) = self.held.take() {
            // Its own message for a node it held for: routed on, as a
            // sleepy node owns no other node's message.
            let _ = self.queue.push(h.out);
        }
        self.motion = Motion::FIXED;
        self.mover = Mover::default();
        self.beacon.announce = None;
        self.beacon.reach_all = None;
        self.beacon.reach_left = 0;
        self.unwatch();
        emit(Action::Trace(Trace::Role { sleepy: true }));
        if !self.reconsider_anchor(now, emit) {
            self.leave_anchor(now);
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
