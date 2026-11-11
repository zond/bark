//! A sleepy node's anchor, beat and duty-cycled receive (spec
//! "Sleepy nodes", "Relay choice", "Beats", "Sleepy nodes alone",
//! "Delivery").
//!
//! The radio may receive duty-cycled (`docs/core-interface.md` §3 rule 10)
//! whenever the node has nothing under way and is neither in a window nor
//! listening to watch, choose or check its anchor, until it next needs the
//! radio. With [`Config::sleepy_listens`], a research switch, it never does.

use super::{Anchor, Node, Stage};
use crate::event::{Action, Trace};
use crate::ids::NodeId;
use crate::node::{heartbeat_interval, heartbeat_lag};
use crate::radio::{time_on_air, CHANNELS, MIN_PREAMBLE_SYMBOLS};
use crate::sniff::anchorless_beat_period;
use crate::tables::neighbours::{Neighbour, Neighbours, BEATS};
use crate::time::{earlier, Duration, Instant};
use crate::wire::HEARTBEAT_LEN;

/// How much stronger, in averaged RSSI, another node must be to take a
/// sleepy node from its anchor: two standard deviations of the difference
/// of two-heartbeat means under 4 dB fading (spec "Relay choice").
pub const SWITCH_MARGIN_DB: f32 = 8.0;

/// The most a node's clock may run fast or slow, ppm: a common 32.768 kHz
/// watch crystal's tolerance, which the low-frequency clock runs from
/// (nRF52840 product specification, LFXO). To be measured on the boards.
pub const CLOCK_DRIFT_PPM: u64 = 50;

/// How far two clocks, each off by up to [`CLOCK_DRIFT_PPM`], drift apart
/// over `d`.
pub(super) fn drift(d: Duration) -> Duration {
    Duration::from_micros(d.as_micros() * 2 * CLOCK_DRIFT_PPM / 1_000_000)
}

/// The deadlines of a sleepy node's anchor and its windows.
#[derive(Clone, Copy, Debug)]
pub(super) struct Listen {
    /// When the rotation that checks the anchor ends (spec "Relay choice").
    rescan_ends: Option<Instant>,
    /// When the first choice of anchor is made (spec "Relay choice").
    choosing: Option<Instant>,
    /// When the anchor's next heartbeat is expected on this channel, and
    /// from when it expected the last.
    watch_at: Option<Instant>,
    watch_base: Instant,
    /// Until when it listens after its beat or a message it took.
    awake_until: Instant,
    window_ends: Option<Instant>,
    /// Until when the driver was told the radio may receive duty-cycled.
    sniff_until: Option<Instant>,
}

impl Listen {
    pub(super) const UNSET: Listen = Listen {
        rescan_ends: None,
        choosing: None,
        watch_at: None,
        watch_base: Instant::ZERO,
        awake_until: Instant::ZERO,
        window_ends: None,
        sniff_until: None,
    };

    /// The anchor's heartbeat is watched for in a window, so the window's
    /// end covers it.
    pub(super) fn next_wakeup(&self) -> Option<Instant> {
        [self.rescan_ends, self.choosing, self.window_ends]
            .into_iter()
            .flatten()
            .min()
    }

    /// The end of a check of the anchor that is due at `now`.
    pub(super) fn check_due(&self, now: Instant) -> Option<Instant> {
        self.rescan_ends.filter(|&t| now >= t)
    }

    pub(super) fn choice_due(&self, now: Instant) -> bool {
        self.choosing.is_some_and(|t| now >= t)
    }

    pub(super) fn awake_until(&mut self, t: Instant) {
        self.awake_until = t;
    }

    pub(super) fn awake_until_at_least(&mut self, t: Instant) {
        self.awake_until = self.awake_until.max(t);
    }

    pub(super) fn watching(&self) -> bool {
        self.watch_at.is_some()
    }

    pub(super) fn watch_at(&self) -> Option<Instant> {
        self.watch_at
    }

    /// The core used the radio, which ends a duty-cycled receive
    /// (`docs/core-interface.md` §3 rule 10).
    pub(super) fn radio_used(&mut self) {
        self.sniff_until = None;
    }

    pub(super) fn forget(&mut self) {
        self.awake_until = Instant::ZERO;
        self.choosing = None;
        self.rescan_ends = None;
        self.watch_at = None;
        self.window_ends = None;
    }
}

impl<const H: usize, const Q: usize> Node<H, Q> {
    /// How often a sleepy node without an anchor beats, at most (spec
    /// "Sleepy nodes alone").
    pub(super) fn anchorless_period(&self) -> Duration {
        anchorless_beat_period(self.config.delivery_ttl)
    }

    /// Whether the radio may receive duty-cycled at `now`. Only a node with
    /// an anchor has windows: one without is always sent the long preamble.
    pub(super) fn may_sniff(&self, now: Instant) -> bool {
        let window = self.anchored_sleepy() && now < self.listen.awake_until;
        self.sleepy()
            && !self.config.sleepy_listens
            && self.stage == Stage::Running
            && !window
            && self.listen.choosing.is_none()
            && self.listen.rescan_ends.is_none()
            && self.radio_free()
            && !self
                .watch()
                .is_some_and(|(from, to)| now >= from && now < to)
    }

    /// When a sleepy node next needs its radio of its own accord: its
    /// next beat, or the next offer of a message it waits to send.
    pub(super) fn radio_due(&self) -> Instant {
        let beat = self.radio.beacon.due_at();
        let due = self.forwarding.wake.map_or(beat, |t| t.min(beat));
        self.watch().map_or(due, |(from, _)| due.min(from))
    }

    /// When a sleepy node listens for its anchor's next heartbeat: from as
    /// early as it can come to as late (spec "Relay choice", asleep).
    pub(super) fn watch(&self) -> Option<(Instant, Instant)> {
        let at = self.listen.watch_at?;
        let drift = drift(at.since(self.listen.watch_base));
        let airtime = time_on_air(HEARTBEAT_LEN as u8, MIN_PREAMBLE_SYMBOLS);
        Some((
            at - drift.mul(3),
            at + drift + heartbeat_lag(&self.config) + airtime,
        ))
    }

    /// Follow the anchor's heartbeat, expected a rotation after the last;
    /// two missed start a check of the anchor (spec "Relay choice").
    pub(super) fn watch_anchor(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        let keeps = self.anchored_sleepy()
            && !self.config.sleepy_listens
            && self.listen.rescan_ends.is_none();
        let Some(a) = self.anchor().filter(|_| keeps) else {
            self.listen.watch_at = None;
            return;
        };
        let interval = heartbeat_interval(a.id, &self.config);
        if self.listen.watch_at.is_none() {
            self.set_misses(0);
            match self.neighbours.by_id(a.id) {
                // Its last heartbeat heard was of its burst, in no slot
                // (spec "Heartbeat"): it watches from the next it hears,
                // awake while it chooses, and else listening a rotation.
                Some((_, n)) if n.anchorless => {
                    if self.listen.choosing.is_none() {
                        self.check_anchor(now, emit);
                    }
                }
                Some((_, n)) => {
                    self.listen.watch_base = n.beat_at;
                    self.listen.watch_at = Some(Neighbours::next_beat(n, interval, self.home));
                }
                None => {}
            }
        }
        while let Some((from, to)) = self.watch().filter(|&(_, to)| now >= to) {
            emit(Action::Trace(Trace::AnchorWatch {
                heard: false,
                offset_us: 0,
                listened: to.since(from),
            }));
            let (misses, dark) = self.sleep().map_or((0, false), |s| (s.misses, s.dark));
            self.set_misses(misses.saturating_add(1));
            if misses.saturating_add(1) >= 2 && !dark {
                self.listen.watch_at = None;
                self.check_anchor(now, emit);
                return;
            }
            self.listen.watch_at = self
                .listen
                .watch_at
                .map(|t| t + interval.mul(CHANNELS as u64));
        }
    }

    /// A heartbeat of the anchor outside its slots was heard: the anchor
    /// is alive, and the time its next is expected stays as it was.
    pub(super) fn anchor_alive(&mut self) {
        if self.listen.rescan_ends.is_some() && self.anchor_holds_mail() {
            self.anchored();
        }
        self.set_misses(0);
    }

    pub(super) fn set_misses(&mut self, misses: u8) {
        if let Some(s) = self.sleep_mut() {
            s.misses = misses;
        }
    }

    /// Whether the anchor's last heartbeat had neither the carried nor the
    /// sleepy flag; a check keeps it only then (spec "Relay choice").
    pub(super) fn anchor_holds_mail(&self) -> bool {
        self.anchor()
            .and_then(|a| self.neighbours.get(a.id))
            .is_none_or(|n| n.holds_mail())
    }

    /// The anchor's heartbeat that started at `now` was heard. A heartbeat
    /// may come late behind its slot, never early, so the time expected is
    /// kept from the earliest heard, moved later only by the clocks' drift.
    pub(super) fn anchor_heard(&mut self, now: Instant) {
        let Some(a) = self.anchor() else { return };
        if self.listen.rescan_ends.is_some() && self.anchor_holds_mail() {
            // Heard while checking: it is there (spec "Relay choice").
            self.anchored();
        }
        self.set_misses(0);
        let base = match (self.listen.watch_at, self.watch()) {
            (Some(at), Some((from, to))) if now >= from && now < to => {
                now.min(at + drift(at.since(self.listen.watch_base)))
            }
            _ => now,
        };
        let interval = heartbeat_interval(a.id, &self.config);
        if let Some((_, n)) = self.neighbours.by_id(a.id) {
            let n = Neighbour {
                beat_at: base,
                ..*n
            };
            self.listen.watch_base = base;
            self.listen.watch_at = Some(Neighbours::next_beat(&n, interval, self.home));
        }
    }

    /// Tell the driver the radio may receive duty-cycled, if it may and
    /// has not been told.
    pub(super) fn sniff(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        if self.listen.sniff_until.is_some_and(|t| t > now) {
            return;
        }
        // The ends of the windows, and the start of the next watch for the
        // anchor's heartbeat, need the core.
        self.listen.window_ends = None;
        if self.anchored_sleepy() {
            let ahead = |t: Option<Instant>| t.filter(|&t| t > now);
            let watch = self.watch();
            self.listen.window_ends = earlier(
                earlier(
                    ahead(Some(self.listen.awake_until)),
                    ahead(self.listen.choosing),
                ),
                earlier(ahead(watch.map(|w| w.0)), ahead(watch.map(|w| w.1))),
            );
        }
        if !self.may_sniff(now) {
            return;
        }
        let until = self.radio_due();
        if until > now {
            self.listen.sniff_until = Some(until);
            emit(Action::Sniff(until));
        }
    }

    /// This node has answered a DATA addressed to it: it listens one
    /// hand-over for the next (spec "Delivery").
    pub(super) fn took(&mut self, now: Instant) {
        self.listen.awake_until = now + self.heard.params().linger;
    }

    /// Choose or keep a sleepy node's anchor, and follow its home channel
    /// (spec "Relay choice"). Returns whether the anchor changed.
    pub(super) fn reconsider_anchor(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) -> bool {
        if !self.sleepy() || self.stage != Stage::Running {
            return false;
        }
        let current = self
            .anchor()
            .and_then(|a| self.neighbours.live(now).find(|(_, n)| n.id == a.id))
            .map(|(_, n)| *n);
        // Any node that is neither sleepy nor carried can hold a sleepy
        // node's messages.
        let strongest = |except: Option<NodeId>, beats: u8| {
            self.neighbours
                .live(now)
                .map(|(_, n)| n)
                .filter(|n| n.holds_mail() && Some(n.id) != except && n.beats >= beats)
                .max_by(|a, b| a.rssi.total_cmp(&b.rssi))
                .copied()
        };
        let first = self.anchor().is_none();
        // The choice after the first anchor is made when its time comes,
        // whether or not the anchor is still in the table: left for later,
        // its time would wake the node at once, again and again.
        let chosen = self.listen.choosing.is_some_and(|t| now >= t);
        if chosen {
            self.listen.choosing = None;
        }
        if current.is_some_and(|a| !a.holds_mail()) {
            // It holds mail no more: another at once if one is live, else
            // the one a rotation's listen finds.
            let to = strongest(None, 0);
            if to.is_none() && self.listen.rescan_ends.is_some() {
                return false;
            }
            if self.listen.rescan_ends.is_none() && current.is_some_and(|a| a.carried) {
                emit(Action::Trace(Trace::AnchorCarried));
            }
            match to {
                Some(to) => self.take_anchor(&to, now, emit),
                None if self.config.sleepy_listens => self.leave_anchor(now),
                None => self.check_anchor(now, emit),
            }
            return to.is_some();
        }
        let to = match current {
            None => strongest(None, 0),
            // The first choice: without the margin, as few can have its
            // address yet.
            Some(a) if chosen => strongest(Some(a.id), 0).filter(|b| b.rssi > a.rssi),
            Some(a) => strongest(Some(a.id), BEATS).filter(|b| b.rssi >= a.rssi + SWITCH_MARGIN_DB),
        };
        if let (Some(a), None) = (current, to) {
            // Kept: follow it to a new home, and to a new position.
            self.set_anchor(Some(Anchor {
                id: a.id,
                position: a.position,
            }));
            if a.home != self.home && self.config.home_override.is_none() {
                self.retune(a.home, emit);
                // Its holder learns the new channel only from a beat there,
                // and every node of the anchor follows on the same
                // heartbeat: spread the beats (spec "Beats").
                let soon = now + self.rng.duration(Duration::ZERO, self.config.heartbeat_max);
                self.radio.beacon.beat_by(soon);
            }
        }
        let Some(to) = to else {
            return false;
        };
        if first && !self.config.sleepy_listens {
            self.listen.choosing = Some(now + self.config.boot_listen);
        }
        self.take_anchor(&to, now, emit);
        true
    }

    /// Make `to` the anchor: listen on its home, and beat there at once.
    pub(super) fn take_anchor(
        &mut self,
        to: &Neighbour,
        now: Instant,
        emit: &mut dyn FnMut(Action),
    ) {
        self.set_anchor(Some(Anchor {
            id: to.id,
            position: to.position,
        }));
        if self.config.home_override.is_none() {
            self.retune(to.home, emit);
        }
        if self.listen.sniff_until.is_some() {
            // It receives duty-cycled no longer, and listens now to choose.
            emit(Action::Listen(self.home));
        }
        // Heard at once and again soon by the new holder.
        self.radio.beacon.beat(now, 1);
        self.anchored();
        self.gps.anchored(self.motion.placed(), now, emit);
    }

    /// The anchor was chosen or confirmed: a sleepy node keeps its entry
    /// whatever its age (spec "Relay choice").
    pub(super) fn anchored(&mut self) {
        self.listen.rescan_ends = None;
        self.listen.watch_at = None;
        let keep = self.anchored_sleepy() && !self.config.sleepy_listens;
        self.neighbours
            .pin(self.anchor().filter(|_| keep).map(|a| a.id));
    }

    /// A sleepy node's anchor is `anchor`; a relay has none. The node
    /// works by its address, so the plane follows it.
    pub(super) fn set_anchor(&mut self, anchor: Option<Anchor>) {
        let was = self.address();
        if let Some(s) = self.sleep_mut() {
            s.anchor = anchor;
        }
        if self.address() != was {
            self.replane();
        }
    }

    /// Give up an anchor that turned carried, with no other to take: the
    /// node is anchorless (spec "Sleepy nodes alone") and beats at once.
    pub(super) fn leave_anchor(&mut self, now: Instant) {
        self.set_anchor(None);
        self.anchored();
        self.gps.due(self.sleepy());
        self.radio.beacon.beat(now, 0);
    }

    /// Listen a worst-case rotation to check the anchor
    /// (spec "Relay choice").
    pub(super) fn check_anchor(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        if !self.anchored_sleepy()
            || self.config.sleepy_listens
            || self.listen.rescan_ends.is_some()
        {
            return;
        }
        self.listen.rescan_ends = Some(now + self.config.boot_listen);
        emit(Action::Trace(Trace::AnchorCheck));
    }

    /// The check that began at `since` is over (spec "Relay choice").
    pub(super) fn anchor_checked(
        &mut self,
        since: Instant,
        now: Instant,
        emit: &mut dyn FnMut(Action),
    ) {
        let heard = |n: &&Neighbour| n.holds_mail() && n.last_heard >= since;
        let anchor = self.anchor().map(|a| a.id);
        if self
            .neighbours
            .iter()
            .filter(heard)
            .any(|n| Some(n.id) == anchor)
        {
            self.anchored();
            return;
        }
        let best = self
            .neighbours
            .iter()
            .filter(heard)
            .max_by(|a, b| a.rssi.total_cmp(&b.rssi))
            .copied();
        let gone = |id: NodeId| self.neighbours.get(id).is_some_and(|n| !n.holds_mail());
        match best {
            Some(b) => self.take_anchor(&b, now, emit),
            // Its anchor moves or sleeps, and no other holds mail here.
            None if anchor.is_some_and(gone) => self.leave_anchor(now),
            // Nothing at all heard: the node is in a pocket or a basement,
            // not its anchor gone; it checks again once it hears anything.
            None => {
                self.anchored();
                if let Some(s) = self.sleep_mut() {
                    s.dark = true;
                }
            }
        }
    }
}
