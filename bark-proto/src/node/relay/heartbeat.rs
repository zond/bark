//! The heartbeat (spec "Heartbeat"), sent in its kinds ([`Beat`]), and what
//! a heartbeat heard does to the tables and to a sleepy node's anchor.

use super::anchor::drift;
use super::{Job, Node, Stage};
use crate::event::{Action, AppEvent, Frame, Trace};
use crate::ids::NodeId;
use crate::node::{burst_gap, heartbeat_interval, heartbeat_lag, mac};
use crate::radio::{time_on_air, Channel, MIN_PREAMBLE_SYMBOLS};
use crate::sniff::LONG_PREAMBLE_SYMBOLS;
use crate::tables::heard::beat_period;
use crate::tables::neighbours::{Neighbour, Neighbours, NEIGHBOURS};
use crate::time::{Duration, Instant};
use crate::wire::{Heartbeat, Packet, HEARTBEAT_LEN};

/// The heartbeat job's state.
pub(super) struct Beacon {
    next_at: Instant,
    /// Not before this: a listen-before-talk backoff or a budget deferral.
    ready_at: Instant,
    rotation: Channel,
    /// The old home, to announce the new one on, after a move.
    announce: Option<Channel>,
    current: Option<(Channel, Beat)>,
    busy_checks: u8,
    /// A sleepy node's beats still to come at the heartbeat interval after
    /// a change of anchor.
    quick: u8,
    /// A sleepy node's anchorless neighbours' channels, as a mask taken
    /// when its beat came due, and those still to beat on before the beat
    /// itself goes (spec "Beats").
    reach_all: Option<u16>,
    reach_left: u16,
    /// A heartbeat owed for a change (`changed`): the home channel then.
    change: Option<Channel>,
    /// The first rotation's slots still to go a [`burst_gap`] apart.
    burst: u8,
}

impl Beacon {
    pub(super) const fn new(rotation: Channel) -> Beacon {
        Beacon {
            next_at: Instant::ZERO,
            ready_at: Instant::ZERO,
            rotation,
            announce: None,
            current: None,
            busy_checks: 0,
            quick: 0,
            reach_all: None,
            reach_left: 0,
            change: None,
            burst: 0,
        }
    }

    /// The next beat at `at`, and `quick` beats after it at the
    /// heartbeat interval (`Node::beat_interval`).
    pub(super) fn beat(&mut self, at: Instant, quick: u8) {
        self.next_at = at;
        self.quick = quick;
    }

    pub(super) fn next_at(&mut self, at: Instant) {
        self.next_at = at;
    }

    pub(super) fn beat_by(&mut self, at: Instant) {
        self.next_at = self.next_at.min(at);
    }

    pub(super) fn burst(&mut self, burst: u8) {
        self.burst = burst;
    }

    pub(super) fn moved_from(&mut self, old: Channel) {
        self.announce = Some(old);
    }

    pub(super) fn under_way(&self) -> bool {
        self.current.is_some()
    }

    pub(super) fn due_at(&self) -> Instant {
        self.next_at.max(self.ready_at)
    }

    /// The node turned sleepy: no announcement after a move, and its
    /// anchorless neighbours' channels are taken afresh.
    pub(super) fn sleepy(&mut self) {
        self.announce = None;
        self.reach_all = None;
        self.reach_left = 0;
    }

    pub(super) fn no_change(&mut self) {
        self.change = None;
    }
}

/// Which heartbeat the beacon sends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Beat {
    /// The one of its slot: on the rotation's channel, or a sleepy node's
    /// beat on its home.
    Slot,
    /// The one after a move, on the old channel (spec "Channel moves").
    Announce,
    /// A relay's answer to an anchorless node's beat, on that node's
    /// channel with the long preamble (spec "Sleepy nodes alone").
    Answer(NodeId),
    /// A sleepy node's beat, with the long preamble, on a channel its
    /// anchorless neighbours listen on (spec "Beats").
    Reach,
    /// The one after a change of flags or of the map point, on the home
    /// channel (spec "Heartbeat").
    Change,
}

/// How long a heartbeat heard was on the air. A relay's answer to an
/// anchorless node also has the long preamble, but cannot be told from its
/// other heartbeats.
fn heartbeat_airtime(hb: &Heartbeat) -> Duration {
    let preamble = if hb.anchorless && hb.sleepy {
        LONG_PREAMBLE_SYMBOLS
    } else {
        MIN_PREAMBLE_SYMBOLS
    };
    time_on_air(HEARTBEAT_LEN as u8, preamble)
}

fn channel_bit(c: Channel) -> u16 {
    1 << c.index()
}

impl<const H: usize, const Q: usize> Node<H, Q> {
    /// Whether the node heartbeats now. A sleepy node waits for an anchor
    /// or the end of its first rotation, since every answer to it would
    /// carry the long preamble (spec "Channel at boot").
    pub(super) fn heartbeats(&self) -> bool {
        // A heartbeat carries only the short position, which a node with
        // no place cannot give (spec "Position").
        if !self.motion.placed() {
            return false;
        }
        match self.stage {
            Stage::Off => false,
            Stage::Booting => !self.sleepy(),
            Stage::Running => true,
        }
    }

    pub(super) fn heartbeat_ready_at(&self) -> Instant {
        let b = &self.radio.beacon;
        let pending = b.announce.is_some() || b.current.is_some() || b.change.is_some();
        let due = if pending {
            b.ready_at
        } else {
            // A relay's answer may come before its next slot.
            let answer = self.answer_due().map(|(_, _, at)| at);
            answer.map_or(b.next_at, |t| t.min(b.next_at))
        };
        due.max(b.ready_at)
    }

    /// The time to a sleepy node's next beat (spec "Beats").
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

    /// Asked only while the radio is free.
    pub(super) fn beacon_due(&self, now: Instant) -> bool {
        self.heartbeats()
            && now >= self.radio.beacon.ready_at
            && (self.radio.beacon.current.is_some()
                || self.radio.beacon.announce.is_some()
                || now >= self.radio.beacon.next_at
                || self.radio.beacon.change.is_some()
                || self.answer_due().is_some_and(|(_, _, at)| now >= at))
    }

    /// The node's flags or its map point changed: one heartbeat on its home
    /// channel at once, for any number of changes before it goes
    /// (spec "Heartbeat").
    pub(super) fn changed(&mut self) {
        if self.stage == Stage::Running {
            self.radio.beacon.change.get_or_insert(self.home);
        }
    }

    /// How many times a relay answers a beat of an anchorless node: once
    /// a fade for two of its beat periods (spec "Sleepy nodes alone").
    pub(super) fn answers(&self) -> u8 {
        let span = self.anchorless_period().mul(2).as_micros();
        span.div_ceil(self.config.fade_retry.as_micros().max(1)) as u8
    }

    /// The next answer a relay owes a neighbour that knows no relay: its
    /// ID, the channel it listens on, and when.
    pub(super) fn answer_due(&self) -> Option<(NodeId, Channel, Instant)> {
        // Nobody anchors to a node that holds no mail. A node in its first
        // rotation is heard by its burst instead (spec "Heartbeat").
        if self.sleepy() || self.motion.carried() || self.leaving() || self.booting() {
            return None;
        }
        self.neighbours
            .answer_due(self.config.fade_retry, self.answers())
    }

    /// The heartbeat in progress. Built when sent, so a move during its
    /// backoff still announces the new channel.
    pub(super) fn heartbeat_frame(&self) -> Frame {
        let (channel, beat) = self
            .radio
            .beacon
            .current
            .unwrap_or((self.radio.beacon.rotation, Beat::Slot));
        let anchored = self.anchored_sleepy();
        let channel = match beat {
            Beat::Slot if anchored => self.home,
            _ => channel,
        };
        // A sleepy node without an anchor, and a relay in its burst, whose
        // neighbour table is new, ask to be answered (spec "Heartbeat").
        let anchorless = match self.sleep() {
            Some(s) => s.anchor.is_none(),
            None => self.radio.beacon.burst > 0,
        };
        let asker = match beat {
            Beat::Answer(id) => self.neighbours.get(id),
            _ => None,
        };
        let hb = Heartbeat {
            sleepy: self.sleepy(),
            anchorless,
            carried: self.motion.carried(),
            home: self.home,
            format: crate::wire::FORMAT,
            map_point: self.motion.map_point(),
            leaving: self.leaving_air(),
            id: self.id,
            short: self.address().short(),
        };
        let reaches_home = self.radio.beacon.reach_all.unwrap_or(0) & channel_bit(self.home) != 0;
        let preamble = match beat {
            // A sleepy node receives duty-cycled; a relay listens.
            Beat::Answer(_) if asker.is_some_and(|n| n.sleepy) => LONG_PREAMBLE_SYMBOLS,
            Beat::Answer(_) => MIN_PREAMBLE_SYMBOLS,
            Beat::Reach => LONG_PREAMBLE_SYMBOLS,
            _ if self.anchorless() => LONG_PREAMBLE_SYMBOLS,
            // An anchorless neighbour listens on its home too.
            Beat::Slot if anchored && reaches_home => LONG_PREAMBLE_SYMBOLS,
            _ => MIN_PREAMBLE_SYMBOLS,
        };
        mac::frame(&Packet::Heartbeat(hb), channel, preamble)
    }

    /// The heartbeat in progress, with the update's section when it carries
    /// one (spec "Updates over the mesh").
    pub(super) fn heartbeat_frame_at(&self, now: Instant) -> Frame {
        let mut f = self.heartbeat_frame();
        if let Some(section) = self.heartbeat_section(now, f.channel) {
            let at = usize::from(f.len);
            let mut b = [0u8; crate::update::SECTION_LEN];
            crate::wire::encode_section(&section, &mut b);
            f.bytes[at..at + b.len()].copy_from_slice(&b);
            f.len += b.len() as u8;
        }
        f
    }

    /// Check the budget before the channel: nothing else transmits between
    /// this check and the transmission, and the bucket only fills meanwhile.
    pub(super) fn start_heartbeat(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) -> bool {
        if self.radio.beacon.current.is_none() {
            if now >= self.radio.beacon.next_at && self.radio.beacon.reach_all.is_none() {
                let all = self.reach_channels(now);
                self.radio.beacon.reach_all = Some(all);
                self.radio.beacon.reach_left = all & !channel_bit(self.home);
            }
            let reach =
                Channel::all().find(|c| self.radio.beacon.reach_left & channel_bit(*c) != 0);
            self.radio.beacon.current = match (self.radio.beacon.announce, self.answer_due()) {
                (Some(old), _) => Some((old, Beat::Announce)),
                _ if now >= self.radio.beacon.next_at => match reach {
                    Some(c) => Some((c, Beat::Reach)),
                    None => Some((self.radio.beacon.rotation, Beat::Slot)),
                },
                _ if self.radio.beacon.change.is_some() => {
                    self.radio.beacon.change.take().map(|c| (c, Beat::Change))
                }
                (None, Some((id, home, at))) if now >= at => Some((home, Beat::Answer(id))),
                _ => return false,
            };
            if let Some((_, Beat::Answer(_))) = self.radio.beacon.current {
                // Every relay that heard the heartbeat answers it: spread
                // them so they do not all check the channel at once.
                let wait = self
                    .rng
                    .duration(Duration::ZERO, self.config.lbt_backoff_max);
                self.radio.beacon.ready_at = now + wait;
                return false;
            }
        }
        let f = self.heartbeat_frame_at(now);
        let airtime = mac::airtime(&f);
        if !self.radio.budget().would_allow(airtime, now) {
            let until = self
                .radio
                .budget()
                .allowed_at(airtime, Duration::ZERO, now)
                .unwrap_or(now + self.config.heartbeat_max);
            self.radio.beacon.ready_at = until;
            emit(Action::Trace(Trace::HeartbeatDeferred { until }));
            return false;
        }
        self.radio.check(Job::Heartbeat, f.channel, emit);
        true
    }

    /// The channels a sleepy node with an anchor also beats on, as a mask:
    /// its anchorless neighbours' (spec "Beats").
    pub(super) fn reach_channels(&self, now: Instant) -> u16 {
        if !self.anchored_sleepy() {
            return 0;
        }
        self.neighbours
            .live(now)
            .filter(|(_, n)| n.stays_anchorless())
            .fold(0, |m, (_, n)| m | channel_bit(n.home))
    }

    pub(super) fn heartbeat_check(
        &mut self,
        busy: bool,
        now: Instant,
        emit: &mut dyn FnMut(Action),
    ) {
        let f = self.heartbeat_frame_at(now);
        if busy {
            self.radio.beacon.busy_checks += 1;
            if self.radio.beacon.busy_checks >= self.config.lbt_attempts {
                emit(Action::Trace(Trace::HeartbeatSkipped {
                    channel: f.channel,
                }));
                self.heartbeat_done(now, false);
            } else {
                self.radio.beacon.ready_at = now
                    + self
                        .rng
                        .duration(Duration::ZERO, self.config.lbt_backoff_max);
            }
            return;
        }
        match self.radio.beacon.current {
            Some((_, Beat::Change)) => emit(Action::Trace(Trace::Changed)),
            Some((_, Beat::Answer(_))) => emit(Action::Trace(Trace::Answer)),
            Some((_, Beat::Slot)) if self.radio.beacon.burst > 0 => {
                emit(Action::Trace(Trace::Burst))
            }
            _ => {}
        }
        if usize::from(f.len) > HEARTBEAT_LEN {
            self.section_sent(now, f.channel);
        }
        self.radio.send(Job::Heartbeat, f, now, emit);
    }

    /// The heartbeat went out, or was skipped (spec "Heartbeat").
    pub(super) fn heartbeat_done(&mut self, now: Instant, sent: bool) {
        self.radio.beacon.busy_checks = 0;
        let home = self.home;
        let slot_home = matches!(self.radio.beacon.current, Some((c, Beat::Slot)) if c == home);
        if sent && slot_home && !self.sleepy() {
            // Its sleepy nodes listen now (spec "Updates over the mesh").
            self.slot_heartbeat(now);
        }
        match self.radio.beacon.current.take() {
            Some((channel, Beat::Announce)) => {
                // A second move meanwhile has a newer old channel to announce on.
                if self.radio.beacon.announce == Some(channel) {
                    self.radio.beacon.announce = None;
                }
                return;
            }
            Some((_, Beat::Answer(id))) => {
                self.neighbours.answer_sent(id, now, self.config.fade_retry);
                return;
            }
            Some((channel, Beat::Reach)) => {
                self.radio.beacon.reach_left &= !channel_bit(channel);
                return;
            }
            Some((channel, Beat::Change)) => {
                // A sleepy node's heartbeat on its anchor's channel is a
                // beat to whoever hears it: its window follows.
                if sent && self.anchored_sleepy() && channel == self.home {
                    self.listen.awake_until(now + self.heard.params().awake);
                }
                return;
            }
            _ => {}
        }
        // The next beat takes its anchorless neighbours' channels afresh.
        self.radio.beacon.reach_all = None;
        if sent && self.anchored_sleepy() {
            // The window after its beat (spec "Delivery").
            self.listen.awake_until(now + self.heard.params().awake);
        }
        if self.sleepy() && !self.booting() {
            // Without an anchor, the next channel each beat whether or not
            // this one went; with one, the rotation is not used.
            self.radio.beacon.rotation = self.radio.beacon.rotation.next();
            self.radio.beacon.next_at = now + self.beat_interval();
        } else {
            // A channel a slot whether or not the heartbeat went, so that a
            // neighbour that heard one knows when the next comes to its
            // channel.
            let mut next = self.radio.beacon.next_at + self.slot_gap();
            self.radio.beacon.rotation = self.radio.beacon.rotation.next();
            while next <= now {
                next += self.slot_gap();
                self.radio.beacon.rotation = self.radio.beacon.rotation.next();
            }
            self.radio.beacon.next_at = next;
        }
        if self.sleepy() {
            // Its own beat stands in for the sweep's readings.
            self.neighbours.expire(now);
        }
    }

    /// The time from the slot that has just gone to the next
    /// (spec "Heartbeat").
    pub(super) fn slot_gap(&mut self) -> Duration {
        self.radio.beacon.burst = self.radio.beacon.burst.saturating_sub(1);
        if self.radio.beacon.burst > 0 {
            burst_gap(&self.config)
        } else {
            heartbeat_interval(self.id, &self.config)
        }
    }

    /// A heartbeat heard (spec "Neighbour table", "Delivery",
    /// "Relay choice").
    pub(super) fn heard(
        &mut self,
        hb: Heartbeat,
        rssi: i16,
        now: Instant,
        emit: &mut dyn FnMut(Action),
    ) {
        if hb.id == self.id {
            return;
        }
        // Rebuilt near the position the node works by, which with an
        // anchor is within reach of every node it hears (spec "Beats").
        let position = hb.position(self.address());
        // A fixed relay's averaged RSSI before this heartbeat, for a relay
        // that watches its radio for a sign that it moves (`relay/gps.rs`).
        let average = self
            .gps
            .watches(&self.gps_situation())
            .then(|| self.neighbours.get(hb.id))
            .flatten()
            .filter(|n| n.holds_mail() && !hb.sleepy && !hb.carried)
            .map(|n| n.rssi);
        let heard = self.neighbours.heard(&hb, position, rssi, &self.plane, now);
        if let Some(a) = average {
            let s = self.gps_situation();
            let rotation = self.config.boot_listen;
            self.gps.surprise(&s, rssi as f32 - a, rotation, now, emit);
        }
        // When it started: a heartbeat's slot is its start, give or take
        // the listen-before-talk.
        let started = now - heartbeat_airtime(&hb);
        // A heartbeat of the anchor heard before the watch opens is in no
        // slot: it shows the anchor alive and moves nothing
        // (spec "Relay choice").
        let off_slot = self.anchor().is_some_and(|a| a.id == hb.id)
            && self.listen.watching()
            && self.watch().is_some_and(|(from, _)| started < from);
        // A relay's burst is in no slot, and its slots start anew
        // (spec "Heartbeat").
        let burst = hb.anchorless && !hb.sleepy;
        if burst {
            self.neighbours.burst_heard(hb.id);
        } else if !off_slot {
            self.neighbours.beat_heard(hb.id, self.home, started);
        }
        if let Some(slot) = heard.new_occupant() {
            self.records.slot_reoccupied(slot);
            self.queue.slot_reoccupied(slot);
            if let (Some(ends), false) = (self.boot.ends(), hb.sleepy) {
                // A relay learned in the first rotation: the application
                // shows it, and its own message waits no longer.
                emit(Action::App(AppEvent::Boot {
                    relays: self.relays_known(now),
                    ends: Some(ends),
                }));
                if let Some(id) = self.own.id {
                    self.queue.due_now(id, now);
                }
            }
        }
        if hb.sleepy {
            if !self.sleepy() && !hb.anchorless && self.heard.heard(hb.id, hb.home, rssi, now) {
                self.deliver_now(hb.id, now);
            }
            if self.anchor().is_some_and(|a| a.id == hb.id) {
                // Its anchor turned sleepy (`relay/power.rs`).
                self.reconsider_anchor(now, emit);
            }
            return;
        }
        if !self.sleepy() && !hb.leaving {
            self.heard_as_relay(hb.id, hb.home);
        }
        if self.sleepy() && self.booting() {
            // An anchor ends the first rotation, which chooses a home
            // for a node that hears none (spec "Channel at boot").
            self.finish_boot(now, emit);
        }
        self.reconsider_anchor(now, emit);
        if self.anchor().is_some_and(|a| a.id == hb.id) {
            if let (Some(at), Some((from, to))) = (self.listen.watch_at(), self.watch()) {
                if started >= from && started < to {
                    let offset = started.as_micros() as i64 - at.as_micros() as i64;
                    emit(Action::Trace(Trace::AnchorWatch {
                        heard: true,
                        offset_us: offset as i32,
                        listened: now.since(from),
                    }));
                }
            }
            if off_slot || burst {
                self.anchor_alive();
            } else {
                self.anchor_heard(started);
                if self.sleepy() {
                    self.anchor_slot_heard(now);
                }
            }
        }
    }

    /// Whether the node listens all the time: only such a node can tell a
    /// heartbeat that did not come from one it slept through.
    fn listens(&self) -> bool {
        !self.sleepy() && self.stage != Stage::Off
    }

    /// The window in which the next heartbeat of `n` is due on this node's
    /// channel, if its slots are known (spec "Heartbeat").
    pub(super) fn slot_window(&self, n: &Neighbour) -> Option<(Instant, Instant)> {
        if !n.slotted || !n.relay() || n.anchorless {
            return None;
        }
        let interval = heartbeat_interval(n.id, &self.config);
        let at = Neighbours::next_beat(n, interval, self.home);
        let lag = heartbeat_lag(&self.config) + drift(at.since(n.beat_at));
        let airtime = time_on_air(HEARTBEAT_LEN as u8, MIN_PREAMBLE_SYMBOLS);
        Some((at - lag, at + lag + airtime))
    }

    /// When the next such window opens or closes, if any: the core notes
    /// then how long the radio has been away, before anything else moves
    /// it.
    pub(super) fn next_slot_edge(&self) -> Option<Instant> {
        if !self.listens() {
            return None;
        }
        self.neighbours
            .iter()
            .filter_map(|n| {
                let (from, to) = self.slot_window(n)?;
                Some(if n.away_base.is_none() { from } else { to })
            })
            .min()
    }

    /// Count the heartbeats due on this channel that did not come
    /// (spec "Heartbeat"). Asked before an event is applied, so that a
    /// frame or an excursion ending now counts as away until now.
    pub(super) fn count_missed_heartbeats(&mut self, now: Instant) {
        if !self.listens() {
            return;
        }
        let floor = self.radio.busy().noise_floor_dbm();
        let home = self.home;
        for slot in 0..NEIGHBOURS {
            while let Some(n) = self.neighbours.slot(slot).copied() {
                let Some((from, to)) = self.slot_window(&n).filter(|&(from, _)| from <= now) else {
                    break;
                };
                let Some(base) = n.away_base else {
                    // Away since before the window opened counts from then.
                    self.neighbours
                        .window_opened(n.id, self.radio.away_us(now, from));
                    continue;
                };
                if to > now {
                    break;
                }
                let away = self.radio.away_us(now, to).wrapping_sub(base);
                let window = to.since(from).as_micros() as f32;
                self.neighbours
                    .missed(n.id, floor, 1.0 - away as f32 / window);
                let interval = heartbeat_interval(n.id, &self.config);
                let next = Neighbours::next_beat(&n, interval, home);
                self.neighbours.slot_passed(n.id, next, home);
            }
        }
    }
}
