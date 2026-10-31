//! The heartbeat (spec "Heartbeat"): a relay's on its fixed interval, a
//! channel a slot, with the first rotation's burst; a sleepy node's beat
//! on its home; the announcement on the old channel after a move; a
//! relay's answer to an anchorless node; and the one after a change of
//! flags. What a heartbeat heard does to the tables and to a sleepy
//! node's anchor is here too.

use super::{Job, Node, Stage};
use crate::event::{Action, AppEvent, Frame, Trace};
use crate::ids::NodeId;
use crate::node::{burst_gap, heartbeat_interval, mac};
use crate::radio::{time_on_air, Channel, MIN_PREAMBLE_SYMBOLS};
use crate::sniff::LONG_PREAMBLE_SYMBOLS;
use crate::time::{Duration, Instant};
use crate::wire::{Heartbeat, Packet, HEARTBEAT_LEN};

/// The rotating heartbeat (spec "Heartbeat") and the one-off announcement on the
/// old channel after a move (spec "Channel moves").
pub(super) struct Beacon {
    pub(super) next_at: Instant,
    /// Not before this: a listen-before-talk backoff or a budget deferral.
    pub(super) ready_at: Instant,
    pub(super) rotation: Channel,
    /// The old home, to announce the new one on, after a move.
    pub(super) announce: Option<Channel>,
    /// The heartbeat in progress: its channel, and which it is.
    pub(super) current: Option<(Channel, Beat)>,
    pub(super) busy_checks: u8,
    /// A sleepy node's beats still to come at the heartbeat interval after
    /// a change of anchor.
    pub(super) quick: u8,
    /// Of a sleepy node with an anchor, the channels its anchorless
    /// neighbours listen on, as a mask, taken when its beat came due (spec
    /// "Beats"), and those of them still to beat on with the long preamble
    /// before the beat itself goes.
    pub(super) reach_all: Option<u16>,
    pub(super) reach_left: u16,
    /// A heartbeat owed for a change of flags or of the map point
    /// (`changed`): the home channel when it changed.
    pub(super) change: Option<Channel>,
    /// Of the first rotation's slots, the ones still to go a
    /// [`burst_gap`] apart (spec "Heartbeat").
    pub(super) burst: u8,
}

impl Beacon {
    /// Before the first heartbeat: its rotation from `rotation`, due at
    /// once.
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
}

/// Which heartbeat the beacon sends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Beat {
    /// The one of its slot: on the rotation's channel, or a sleeping
    /// node's beat on its home.
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

/// How long a heartbeat heard was on the air: an anchorless sleepy node's
/// beat with the long preamble, any other as if with the shortest. A relay's
/// answer to an anchorless node also has the long one, but cannot be told
/// from its other heartbeats.
fn heartbeat_airtime(hb: &Heartbeat) -> Duration {
    let preamble = if hb.anchorless && hb.sleepy {
        LONG_PREAMBLE_SYMBOLS
    } else {
        MIN_PREAMBLE_SYMBOLS
    };
    time_on_air(HEARTBEAT_LEN as u8, preamble)
}

/// The bit of channel `c` in a mask of channels.
fn channel_bit(c: Channel) -> u16 {
    1 << c.index()
}

impl<const H: usize, const Q: usize> Node<H, Q> {
    /// Whether the node heartbeats now: from its boot, but a sleepy node
    /// only once it has an anchor or its first rotation has ended, since
    /// every answer to a sleepy node would carry the long preamble
    /// (spec "Channel at boot", roadmap D73).
    pub(super) fn heartbeats(&self) -> bool {
        // A heartbeat says where the node is, and a node with no place
        // knows its neighbours' positions only to the short form's period
        // (spec "Position", roadmap D83).
        if !self.motion.placed {
            return false;
        }
        match self.stage {
            Stage::Off => false,
            Stage::Booting => !self.sleepy(),
            Stage::Running => true,
        }
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

    /// The node's flags or its map point changed: it heartbeats on its
    /// home channel at once, outside its slots, so that the neighbours
    /// there address it anew now and not a rotation later, and an old
    /// anchor learns to hand over its mail (spec "Heartbeat"). One
    /// heartbeat for any number of changes before it goes, sleepy or not
    /// (roadmap D72). A position that changes with each fix, a carried
    /// node's or an anchorless one's, is not announced: its slots or
    /// beats carry it (roadmap D71).
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

    /// The next answer a relay owes a neighbour that knows no relay, if
    /// any: its ID, the channel it listens on, and when.
    pub(super) fn answer_due(&self) -> Option<(NodeId, Channel, Instant)> {
        // Nobody anchors to a node that holds no mail. A node in its first
        // rotation is heard by its burst instead (spec "Heartbeat").
        if self.sleepy() || self.motion.carried || self.leaving() || self.booting() {
            return None;
        }
        self.neighbours
            .answer_due(self.config.fade_retry, self.answers())
    }

    /// The heartbeat in progress. It carries the home at the moment it is
    /// sent, so a move during its backoff still announces the new channel,
    /// and the node's address, a sleepy node's anchor's position while it
    /// has one. A sleepy node with an anchor beats on its home; a sleepy node
    /// without an anchor, a relay answering one, and a sleepy node's beat
    /// on its anchorless neighbours' channels, its home among them, send
    /// with the long preamble.
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
            carried: self.motion.carried,
            home: self.home,
            format: crate::wire::FORMAT,
            map_point: self.motion.map_point,
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

    /// Check the budget before the channel: there is no point listening for
    /// a slot the budget will not let us use. Nothing else transmits
    /// between this check and the transmission, and the bucket only fills
    /// meanwhile, so the budget cannot have shrunk by then. The
    /// announcement after a move goes first, then the heartbeat of the
    /// slot, after a sleepy node's copies of it on its anchorless
    /// neighbours' channels, then an answer.
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
                // Every relay that heard the heartbeat answers it: each at
                // a random time within a backoff, so that they do not all
                // check the channel and send in the same instant.
                let wait = self
                    .rng
                    .duration(Duration::ZERO, self.config.lbt_backoff_max);
                self.radio.beacon.ready_at = now + wait;
                return false;
            }
        }
        let f = self.heartbeat_frame();
        let airtime = mac::airtime(&f);
        if !self.radio.budget.would_allow(airtime, now) {
            let until = self
                .radio
                .budget
                .allowed_at(airtime, Duration::ZERO, now)
                .unwrap_or(now + self.config.heartbeat_max);
            self.radio.beacon.ready_at = until;
            emit(Action::Trace(Trace::HeartbeatDeferred { until }));
            return false;
        }
        self.radio.excursion.check(Job::Heartbeat, f.channel, emit);
        true
    }

    /// The channels the anchorless neighbours of a sleepy node with an
    /// anchor listen on at `now`, as a mask: each of its beats goes on
    /// each of them too, with the long preamble, once it has heard the
    /// neighbour anchorless in two consecutive rotations of its beats
    /// (spec "Beats"). One heard so once may be switching on, and anchor
    /// within minutes.
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
        let f = self.heartbeat_frame();
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
        self.radio.budget.record(mac::airtime(&f), now);
        self.radio.excursion.transmit(Job::Heartbeat, f, emit);
    }

    /// The heartbeat went out, or was skipped (spec "Heartbeat"). On a fixed
    /// interval a skipped one gives up its channel for the rotation, so
    /// that the next comes where and when its neighbours expect it; a
    /// sleeping node's beat, always on its home, has no rotation to keep.
    pub(super) fn heartbeat_done(&mut self, now: Instant, sent: bool) {
        self.radio.beacon.busy_checks = 0;
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
                    self.timers.awake_until = now + self.heard.params().awake;
                }
                return;
            }
            _ => {}
        }
        // The next beat takes its anchorless neighbours' channels afresh.
        self.radio.beacon.reach_all = None;
        if sent && self.anchored_sleepy() {
            // The window after its beat (spec "Delivery").
            self.timers.awake_until = now + self.heard.params().awake;
        }
        if self.sleepy() && !self.booting() {
            // Without an anchor, the next channel each beat whether or not
            // this one went; with one, the rotation is not used.
            self.radio.beacon.rotation = self.radio.beacon.rotation.next();
            self.radio.beacon.next_at = now + self.beat_interval();
        } else {
            // On its fixed interval, a channel a slot whether or not the
            // heartbeat went, so that a neighbour that heard one knows when
            // the next comes to its channel (spec "Heartbeat"); in its first
            // rotation, a burst gap apart.
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

    /// The time from the slot that has just gone to the next: a burst gap
    /// while the first rotation's slots last, then the node's interval
    /// (spec "Heartbeat").
    pub(super) fn slot_gap(&mut self) -> Duration {
        self.radio.beacon.burst = self.radio.beacon.burst.saturating_sub(1);
        if self.radio.beacon.burst > 0 {
            burst_gap(&self.config)
        } else {
            heartbeat_interval(self.id, &self.config)
        }
    }

    /// A heartbeat. Every heartbeat fills the neighbour table, a sleepy
    /// node's marked sleepy (spec "Neighbour table"). At a node that is not
    /// sleepy, the beat of a sleepy node with an anchor fills the heard
    /// table too: the node listens now, and gets what is held for it; one
    /// without an anchor has no window to deliver in
    /// (spec "Sleepy nodes alone"). A sleepy node's anchor follows the
    /// heartbeats of nodes that are not sleepy (spec "Relay choice").
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
            .gps_watches()
            .then(|| self.neighbours.get(hb.id))
            .flatten()
            .filter(|n| n.holds_mail() && !hb.sleepy && !hb.carried)
            .map(|n| n.rssi);
        let heard = self.neighbours.heard(&hb, position, rssi, &self.plane, now);
        if let Some(a) = average {
            self.gps_surprise(rssi as f32 - a, now, emit);
        }
        // When it started: a heartbeat's slot is its start, give or take
        // the listen-before-talk.
        let started = now - heartbeat_airtime(&hb);
        // A heartbeat of the anchor that a sleeping node hears before its
        // watch opens is not in a slot: an answer to a node without an
        // anchor, or the one after a move (spec "Relay choice"). It shows the
        // anchor alive but says nothing of its slots.
        let off_slot = self.anchor().is_some_and(|a| a.id == hb.id)
            && self.timers.watch_at.is_some()
            && self.watch().is_some_and(|(from, _)| started < from);
        // A relay's burst goes a burst gap a heartbeat: none of it is in a
        // slot (spec "Heartbeat").
        let burst = hb.anchorless && !hb.sleepy;
        if !off_slot && !burst {
            self.neighbours.beat_heard(hb.id, self.home, started);
        }
        if let Some(slot) = heard.new_occupant() {
            self.records.slot_reoccupied(slot);
            self.forwarding.skipped &= !(1 << slot);
            if let (Some(ends), false) = (self.timers.boot_ends, hb.sleepy) {
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
        if !self.sleepy() {
            self.heard_as_relay(hb.id, hb.home);
        }
        if self.sleepy() && self.booting() {
            // An anchor ends the first rotation, which chooses a home
            // for a node that hears none (spec "Channel at boot").
            self.finish_boot(now, emit);
        }
        self.reconsider_anchor(now, emit);
        if self.anchor().is_some_and(|a| a.id == hb.id) {
            if let (Some(at), Some((from, to))) = (self.timers.watch_at, self.watch()) {
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
            }
        }
    }
}
