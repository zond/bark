//! A sleepy node's anchor, beat and duty-cycled receive
//! (spec "Sleepy nodes").
//!
//! # Home and beat
//!
//! It takes as its anchor the first node it hears that is neither sleepy
//! nor carried, at once, and listens on the anchor's home channel; its
//! address is the anchor's position and its own ID (spec "Addresses").
//! The anchor's position is the position it works by while it has one
//! ([`Node::address`]): it beats it, centres its plane on it and rebuilds
//! the short positions it hears near it, and it takes no GPS fix and runs
//! no search (`relay/gps.rs`, `relay/motion.rs`). Hearing the anchor's
//! heartbeat each rotation says it is within reach of a fixed relay,
//! which is all its address needs; what its own fixes bought it, the
//! carried flag and a check of the anchor after a move, nothing reads
//! while it sleeps, and the anchor missed twice checks it a rotation or
//! two later (spec "Beats", roadmap D91).
//! It stays awake for a worst-case rotation after that and then takes the
//! strongest node it heard, without a margin; after that it moves only to
//! one heard over two heartbeats and [`SWITCH_MARGIN_DB`] stronger (spec
//! "Relay choice"). It leaves an anchor whose heartbeat turns carried or
//! sleepy, for another or, with none, into anchorless mode.
//! Back from the dark, it keeps its anchor. It
//! follows its anchor to a new home channel at the anchor's next heartbeat
//! it hears, and beats there within a heartbeat interval; it does not move
//! by the busy estimate.
//!
//! With an anchor it heartbeats on its home channel only, with the sleepy
//! bit, each beat a random half to one beat period after the last
//! (`tables::heard::beat_period`, 300 s): at most a period apart, so that
//! two beats missed are a delivery TTL; spread, so that two nodes that met
//! once do not meet again. After it chooses or
//! changes its anchor it beats at once and once more within a heartbeat
//! interval, so that one lost beat does not hide it from its new holder for
//! a period. It sweeps the channels only while it boots: receiving
//! duty-cycled it could not.
//!
//! Between its windows it receives duty-cycled, anchor or not, so any node
//! can reach it with the long preamble (`crate::sniff`). It holds mail for
//! nobody: it keeps no heard table and no delivery queue
//! ([`SleepyNode`]), owns only messages addressed to its ID, and forwards
//! what it is given at once (spec "Sleepy nodes"). Its neighbours keep it
//! as a last resort (spec "Next hop"); where its neighbour table holds
//! anchorless nodes it sends each beat on each of their channels too,
//! with the long preamble, so that they learn where it listens (spec
//! "Beats").
//!
//! Without an anchor it is anchorless ([`Node::anchorless`], spec
//! "Sleepy nodes alone"): it has no window, so every frame to it carries
//! the long preamble. It beats with the anchorless bit and the long
//! preamble, each beat a random half to one anchorless beat period, 46.2 s,
//! after the last, on the next channel each time, so that a relay on any
//! channel hears it within a rotation; nobody takes it as an anchor, since
//! it leaves the mode at the first relay it hears. A relay answers each
//! such beat it hears with a heartbeat on the node's channel with the long
//! preamble, at once and a fade later, which the node takes as the first
//! relay heard. Its home is the quietest channel at the end of its boot
//! listen, kept.
//!
//! # Duty-cycled receive (`docs/core-interface.md` §3 rule 10)
//!
//! A sleepy node tells its driver the radio may receive duty-cycled
//! ([`Action::Sniff`]) whenever it has nothing under way and is neither
//! in a window, after its beat or a message it took (spec "Delivery"),
//! nor listening for its anchor's heartbeat or to choose or check its
//! anchor (spec "Relay choice"), until it next needs the radio, for its
//! next beat or the next offer of a message it waits to send, unless the
//! core starts an excursion sooner. With [`Config::sleepy_listens`], a
//! research switch, it never does.

use super::{Anchor, Node, Stage};
use crate::event::{Action, Trace};
use crate::ids::NodeId;
use crate::node::{heartbeat_interval, heartbeat_lag};
use crate::radio::{time_on_air, CHANNELS, MIN_PREAMBLE_SYMBOLS};
use crate::sniff::anchorless_beat_period;
use crate::tables::heard::beat_period;
use crate::tables::neighbours::{Neighbour, Neighbours, BEATS};
use crate::time::{earlier, Duration, Instant};
use crate::wire::HEARTBEAT_LEN;

/// How much stronger, in the averaged RSSI of the neighbour table, another
/// node must be to take a sleepy node from its anchor
/// (spec "Relay choice"): two standard deviations of the difference of two means of
/// two heartbeats each under the scenarios' 4 dB fading.
/// The table's average lets each new frame count half, which varies less
/// than a mean of two (σ²/3 against σ²/2), so 8 dB is a little more than
/// two standard deviations here.
pub const SWITCH_MARGIN_DB: f32 = 8.0;

/// The most a node's clock may run fast or slow, parts per million: the
/// tolerance of a common 32.768 kHz watch crystal, which the board's
/// low-frequency clock runs from (nRF52840 product specification, LFXO).
/// To be measured on the boards.
pub const CLOCK_DRIFT_PPM: u64 = 50;

/// How far two clocks, each off by up to [`CLOCK_DRIFT_PPM`], drift apart
/// over `d`.
fn drift(d: Duration) -> Duration {
    Duration::from_micros(d.as_micros() * 2 * CLOCK_DRIFT_PPM / 1_000_000)
}

impl<const H: usize, const Q: usize> Node<H, Q> {
    /// How often a sleepy node without an anchor beats, at most (spec
    /// "Sleepy nodes alone").
    pub(super) fn anchorless_period(&self) -> Duration {
        anchorless_beat_period(self.config.delivery_ttl)
    }

    /// Whether the radio may receive duty-cycled at `now` (module
    /// documentation). Only a node with an anchor has windows: one
    /// without is sent to with the long preamble whenever.
    pub(super) fn may_sniff(&self, now: Instant) -> bool {
        let window = self.anchored_sleepy() && now < self.timers.awake_until;
        self.sleepy()
            && !self.config.sleepy_listens
            && self.stage == Stage::Running
            && !window
            && self.timers.choosing.is_none()
            && self.timers.rescan_ends.is_none()
            && self.radio_free()
            && !self
                .watch()
                .is_some_and(|(from, to)| now >= from && now < to)
    }

    /// When a sleepy node next needs its radio of its own accord: its
    /// next beat, or the next offer of a message it waits to send.
    pub(super) fn radio_due(&self) -> Instant {
        let beat = self.radio.beacon.next_at.max(self.radio.beacon.ready_at);
        let due = self.queue.earliest_due().map_or(beat, |t| t.min(beat));
        self.watch().map_or(due, |(from, _)| due.min(from))
    }

    /// When a node that sleeps listens for its anchor's next heartbeat on
    /// its channel, if it does (module documentation): from as early as it
    /// can come to as late as it can. `watch_at` is when it comes if it
    /// starts in its slot, from the time expected of the last one heard
    /// (`anchor_heard`). It comes earlier by as much as the two clocks
    /// drift apart, [`CLOCK_DRIFT_PPM`] each, since; and by twice as much
    /// again, since the last one may have been late by what keeps a
    /// heartbeat behind its slot ([`heartbeat_lag`]) and moved the time
    /// expected later by the drift when its slot was as early as the
    /// drift allowed. It may come later by the drift, and by that lag.
    pub(super) fn watch(&self) -> Option<(Instant, Instant)> {
        let at = self.timers.watch_at?;
        let drift = drift(at.since(self.timers.watch_base));
        let airtime = time_on_air(HEARTBEAT_LEN as u8, MIN_PREAMBLE_SYMBOLS);
        Some((
            at - drift.mul(3),
            at + drift + heartbeat_lag(&self.config) + airtime,
        ))
    }

    /// Follow the anchor's heartbeat: a node that sleeps expects it on its
    /// channel a rotation of the anchor's interval after the last
    /// (module documentation); once two rotations have passed without it,
    /// the anchor is gone, and the node listens a rotation to choose again
    /// (spec "Relay choice").
    pub(super) fn watch_anchor(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        let keeps = self.anchored_sleepy()
            && !self.config.sleepy_listens
            && self.timers.rescan_ends.is_none();
        let Some(a) = self.anchor().filter(|_| keeps) else {
            self.timers.watch_at = None;
            return;
        };
        let interval = heartbeat_interval(a.id, &self.config);
        if self.timers.watch_at.is_none() {
            self.set_misses(0);
            match self.neighbours.by_id(a.id) {
                // Its last heartbeat heard was of its burst, in no slot
                // (spec "Heartbeat"): it watches from the next it hears,
                // awake while it chooses, and else listening a rotation.
                Some((_, n)) if n.anchorless => {
                    if self.timers.choosing.is_none() {
                        self.check_anchor(now, emit);
                    }
                }
                Some((_, n)) => {
                    self.timers.watch_base = n.beat_at;
                    self.timers.watch_at = Some(Neighbours::next_beat(n, interval, self.home));
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
                self.timers.watch_at = None;
                self.check_anchor(now, emit);
                return;
            }
            self.timers.watch_at = self
                .timers
                .watch_at
                .map(|t| t + interval.mul(CHANNELS as u64));
        }
    }

    /// The anchor's heartbeat that started at `now` was heard: it is alive, and its next
    /// comes to this node's channel when the rotation brings it there. A
    /// heartbeat may come late behind its slot, never early, so the time
    /// expected is kept from the earliest heard, moved later only by as
    /// much as the clocks may drift apart.
    /// A heartbeat of the anchor outside its slots was heard: the anchor
    /// is alive, and the time its next is expected stays as it was.
    pub(super) fn anchor_alive(&mut self) {
        if self.timers.rescan_ends.is_some() && self.anchor_holds_mail() {
            self.anchored();
        }
        self.set_misses(0);
    }

    pub(super) fn set_misses(&mut self, misses: u8) {
        if let Some(s) = self.sleep_mut() {
            s.misses = misses;
        }
    }

    /// Whether the anchor's last heartbeat had neither the carried nor
    /// the sleepy flag: a check does not keep it otherwise
    /// (spec "Relay choice").
    pub(super) fn anchor_holds_mail(&self) -> bool {
        self.anchor()
            .and_then(|a| self.neighbours.get(a.id))
            .is_none_or(|n| n.holds_mail())
    }

    pub(super) fn anchor_heard(&mut self, now: Instant) {
        let Some(a) = self.anchor() else { return };
        if self.timers.rescan_ends.is_some() && self.anchor_holds_mail() {
            // Heard while checking: it is there (spec "Relay choice").
            self.anchored();
        }
        self.set_misses(0);
        let base = match (self.timers.watch_at, self.watch()) {
            (Some(at), Some((from, to))) if now >= from && now < to => {
                now.min(at + drift(at.since(self.timers.watch_base)))
            }
            _ => now,
        };
        let interval = heartbeat_interval(a.id, &self.config);
        if let Some((_, n)) = self.neighbours.by_id(a.id) {
            let n = Neighbour {
                beat_at: base,
                ..*n
            };
            self.timers.watch_base = base;
            self.timers.watch_at = Some(Neighbours::next_beat(&n, interval, self.home));
        }
    }

    /// Tell the driver the radio may receive duty-cycled, if it may and
    /// has not been told.
    pub(super) fn sniff(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        if self.timers.sniff_until.is_some_and(|t| t > now) {
            return;
        }
        // The ends of the windows it listens in, and the start of the next
        // for its anchor's heartbeat, need the core, to say so.
        self.timers.window_ends = None;
        if self.anchored_sleepy() {
            let ahead = |t: Option<Instant>| t.filter(|&t| t > now);
            let watch = self.watch();
            self.timers.window_ends = earlier(
                earlier(
                    ahead(Some(self.timers.awake_until)),
                    ahead(self.timers.choosing),
                ),
                earlier(ahead(watch.map(|w| w.0)), ahead(watch.map(|w| w.1))),
            );
        }
        if !self.may_sniff(now) {
            return;
        }
        let until = self.radio_due();
        if until > now {
            self.timers.sniff_until = Some(until);
            emit(Action::Sniff(until));
        }
    }

    /// This node has answered a DATA addressed to it: it listens one
    /// hand-over for the next (spec "Delivery").
    pub(super) fn took(&mut self, now: Instant) {
        self.timers.awake_until = now + self.heard.params().linger;
    }

    /// The time to a sleepy node's next beat: a random half to one beat
    /// period, the anchorless one without an anchor. A relay heartbeats on its fixed interval instead
    /// (`heartbeat_done`).
    pub(super) fn beat_interval(&mut self) -> Duration {
        if self.radio.beacon.quick > 0 {
            self.radio.beacon.quick -= 1;
            return self
                .rng
                .duration(self.config.heartbeat_min, self.config.heartbeat_max);
        }
        let period = if self.anchored_sleepy() {
            beat_period(self.config.delivery_ttl)
        } else {
            self.anchorless_period()
        };
        self.rng.duration(period.div(2), period)
    }

    /// Choose or keep a sleepy node's anchor among the live neighbours
    /// that are not sleepy, and follow its home channel. Returns whether
    /// the anchor changed.
    pub(super) fn reconsider_anchor(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) -> bool {
        if !self.sleepy() || self.stage != Stage::Running {
            return false;
        }
        let current = self
            .anchor()
            .and_then(|a| self.neighbours.live(now).find(|(_, n)| n.id == a.id))
            .map(|(_, n)| *n);
        // Any node that is neither sleepy nor carried can hold a sleepy
        // node's messages: without an anchor it takes the first it hears
        // (spec "Relay choice").
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
        let chosen = self.timers.choosing.is_some_and(|t| now >= t);
        if chosen {
            self.timers.choosing = None;
        }
        if current.is_some_and(|a| !a.holds_mail()) {
            // It holds mail no more, carried or sleepy: another, at once
            // if one is live, else the one a rotation's listen finds.
            let to = strongest(None, 0);
            if to.is_none() && self.timers.rescan_ends.is_some() {
                return false;
            }
            if self.timers.rescan_ends.is_none() && current.is_some_and(|a| a.carried) {
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
            // A rotation after its first anchor, by when every node in range
            // has visited its channel, and while few can have its address:
            // the strongest heard, without the margin.
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
                // heartbeat: each beats at a random time within a heartbeat
                // interval (spec "Beats").
                let soon = now + self.rng.duration(Duration::ZERO, self.config.heartbeat_max);
                self.radio.beacon.next_at = self.radio.beacon.next_at.min(soon);
            }
        }
        let Some(to) = to else {
            return false;
        };
        if first && !self.config.sleepy_listens {
            self.timers.choosing = Some(now + self.config.boot_listen);
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
        if self.timers.sniff_until.is_some() {
            // It receives duty-cycled no longer, and listens now to choose.
            emit(Action::Listen(self.home));
        }
        // Heard at once and again soon by the new holder.
        self.radio.beacon.next_at = now;
        self.radio.beacon.quick = 1;
        self.anchored();
        self.gps_anchor(now, emit);
    }

    /// The anchor was chosen or confirmed: a node that sleeps keeps it
    /// whether or not it hears it (module documentation).
    pub(super) fn anchored(&mut self) {
        self.timers.rescan_ends = None;
        self.timers.watch_at = None;
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
        self.gps_due();
        self.radio.beacon.next_at = now;
        self.radio.beacon.quick = 0;
    }

    /// Listen a worst-case rotation, from `now`, to check the anchor of a
    /// node that sleeps (module documentation).
    pub(super) fn check_anchor(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        if !self.anchored_sleepy()
            || self.config.sleepy_listens
            || self.timers.rescan_ends.is_some()
        {
            return;
        }
        self.timers.rescan_ends = Some(now + self.config.boot_listen);
        emit(Action::Trace(Trace::AnchorCheck));
    }

    /// The rotation that began at `since` is over, the anchor not heard in
    /// it: take the strongest node heard that is not sleepy, or keep the
    /// anchor if none was (spec "Relay choice").
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
            // Nothing at all heard: the node is out of reach, in a pocket or
            // a basement, not its anchor gone; it keeps it, and checks again
            // only once it hears anything again.
            None => {
                self.anchored();
                if let Some(s) = self.sleep_mut() {
                    s.dark = true;
                }
            }
        }
    }
}
