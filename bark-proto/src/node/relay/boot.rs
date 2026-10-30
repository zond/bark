//! The first rotation and the home channel (spec "Channel at boot",
//! "Home channel selection", roadmap D73): a node heartbeats from its
//! boot on its stored home or a random channel, learns its neighbours for
//! a worst-case rotation, and then chooses its home among the quietest
//! channels, or, sleepy, the anchor it has heard.

use super::motion::Motion;
use super::{Anchor, Node, Stage};
use crate::event::{Action, AppEvent, Trace};
use crate::node::{heartbeat_interval, Rng};
use crate::radio::{Channel, CHANNELS};
use crate::tables::busy::{percent, ONE};
use crate::time::{Duration, Instant};

pub(super) fn random_channel(rng: &mut Rng) -> Channel {
    let fallback = Channel::all().next().expect("the plan has channels");
    Channel::new(rng.range(0, CHANNELS as u64 - 1) as u8).unwrap_or(fallback)
}

impl<const H: usize, const Q: usize> Node<H, Q> {
    /// Boot: the first rotation begins (spec "Channel at boot"). The node
    /// heartbeats from its first slot, on its stored home or a random
    /// channel, and takes traffic from the first neighbour it hears.
    pub(super) fn start(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        self.timers.boot_from = now;
        let ends = self.boot_end(now);
        self.stage = Stage::Booting;
        self.timers.boot_ends = Some(ends);
        self.sweep.next_at = now;
        if self.config.stored.is_some_and(|s| s.carried) {
            // Still since now: no clock ran while it was off.
            self.motion = Motion::carried(self.config.position, now);
        }
        // At a random phase of its interval: nodes that boot together, after
        // a power cut, would otherwise heartbeat in step whenever their
        // intervals have a small common multiple.
        let interval = heartbeat_interval(self.id, &self.config);
        self.beacon.next_at = now + self.rng.duration(Duration::ZERO, interval);
        // A sleepy node does not burst: it heartbeats once it has an anchor
        // or its first rotation has ended, and its beats keep no slots.
        self.beacon.burst = if self.sleepy() { 0 } else { CHANNELS };
        if !self.gps_tracks() {
            self.gps_due();
        }
        emit(Action::Listen(self.home));
        emit(Action::App(AppEvent::Boot {
            relays: 0,
            ends: Some(ends),
        }));
    }

    /// The relays this node knows: live neighbours that are not sleepy.
    pub(super) fn relays_known(&self, now: Instant) -> u8 {
        let n = self.neighbours.live(now).filter(|(_, n)| n.relay()).count();
        n.min(u8::MAX as usize) as u8
    }

    /// End of the first rotation (spec "Channel at boot"): a node without
    /// a stored home chooses one, which for a relay is a move like any
    /// other (spec "Channel moves"); a sleepy node takes the anchor it has
    /// heard, or is anchorless.
    pub(super) fn finish_boot(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        self.stage = Stage::Running;
        self.timers.boot_ends = None;
        emit(Action::App(AppEvent::Boot {
            relays: self.relays_known(now),
            ends: None,
        }));
        let chooses = self.config.home_override.is_none() && self.config.stored.is_none();
        if self.sleepy() {
            if chooses && !self.home_is_quietest() {
                let home = self.quietest();
                self.retune(home, emit);
            }
            if !self.reconsider_anchor(now, emit) {
                self.gps_due();
                // At a random time within a beat period, so that nodes that
                // boot together do not beat together.
                self.beacon.next_at =
                    now + self.rng.duration(Duration::ZERO, self.anchorless_period());
            }
            return;
        }
        if chooses && !self.home_is_quietest() {
            let old = self.home;
            let to = self.quietest();
            self.retune(to, emit);
            if self.home != old {
                self.beacon.announce = Some(old);
                self.take_listeners(old, now);
            }
        }
    }

    /// Whether the home channel is among the quietest, tied within 0.01
    /// (spec "Channel at boot"): a node keeps it then, since a move costs
    /// the neighbours that know it a rotation of a stale channel.
    pub(super) fn home_is_quietest(&self) -> bool {
        let min = Channel::all()
            .map(|c| self.busy.fraction(c))
            .min()
            .unwrap_or(ONE);
        self.busy.fraction(self.home) <= min.saturating_add(percent(1))
    }

    /// A sleepy node handed back its anchor at boot (`store.rs`) takes it
    /// at once and beats to it, and listens a rotation for its heartbeat,
    /// as when it checks its anchor (spec "Relay choice"): it keeps it at
    /// the first, and takes the strongest relay heard if it hears none.
    pub(super) fn resume_anchor(&mut self, a: Anchor, now: Instant, emit: &mut dyn FnMut(Action)) {
        self.stage = Stage::Running;
        self.timers.boot_ends = None;
        emit(Action::App(AppEvent::Boot {
            relays: 0,
            ends: None,
        }));
        self.set_anchor(Some(a));
        self.anchored();
        self.beacon.next_at = now;
        self.beacon.quick = 1;
        self.check_anchor(now, emit);
    }

    pub(super) fn retune(&mut self, to: Channel, emit: &mut dyn FnMut(Action)) {
        if to != self.home {
            // What was heard on the old channel says nothing of the new.
            self.sweep.pending = None;
            emit(Action::Trace(Trace::HomeChannelMoved {
                from: self.home,
                to,
            }));
            self.home = to;
            emit(Action::Listen(to));
        }
    }

    /// `argmin busy[c]`, ties within 0.01 broken at random (spec "Channel at boot"),
    /// so nodes that hear nothing do not all pick channel 0.
    pub(super) fn quietest(&mut self) -> Channel {
        let min = Channel::all()
            .map(|c| self.busy.fraction(c))
            .min()
            .unwrap_or(ONE);
        let busy = &self.busy;
        let near = |c: &Channel| busy.fraction(*c) <= min.saturating_add(percent(1));
        let count = Channel::all().filter(near).count() as u64;
        let pick = self.rng.range(0, count.saturating_sub(1)) as usize;
        Channel::all().filter(near).nth(pick).unwrap_or(self.home)
    }
}
