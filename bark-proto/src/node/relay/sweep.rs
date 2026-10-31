//! The busy sweep and the move rule (spec "Busy estimate",
//! "Channel moves", roadmap D86): one reading a second, the channels in
//! turn, the home's reading held until any reception it fell in has shown
//! whether the energy was this node's own; a home that stays busier than
//! the quietest channel by the threshold for the dwell is left, after a
//! random wait, for the quietest.

use super::{Job, Node, Stage};
use crate::event::Action;
use crate::radio::{time_on_air, Channel, CHANNELS, MIN_PREAMBLE_SYMBOLS};
use crate::tables::busy::percent;
use crate::time::{Duration, Instant};
use crate::wire::Packet;

/// The busy sweep (spec "Busy estimate"): one RSSI reading at a time, channels in
/// turn, spread over the sweep interval.
pub(super) struct Sweep {
    pub(super) next_at: Instant,
    /// The channel the next reading takes.
    pub(super) channel: Channel,
    /// The last home reading, held until any reception it fell in has
    /// ended and shown whether the energy it read was this relay's own.
    pub(super) pending: Option<HomeReading>,
}

impl Sweep {
    /// Before the first reading: from `channel`, due at once.
    pub(super) const fn new(channel: Channel) -> Sweep {
        Sweep {
            next_at: Instant::ZERO,
            channel,
            pending: None,
        }
    }
}

/// A reading of the home channel awaiting [`Sweep::pending`]'s verdict.
#[derive(Clone, Copy)]
pub(super) struct HomeReading {
    pub(super) channel: Channel,
    pub(super) at: Instant,
    /// `None` once a packet addressed to this relay turned out to be on
    /// the air when it was taken.
    pub(super) dbm: Option<i16>,
}

impl<const H: usize, const Q: usize> Node<H, Q> {
    /// This node has moved its home from `old`: the sleepy nodes there that
    /// listen now hear the heartbeat it sends on `old` and follow it, so it
    /// sends to them on its new home. The others still sleep there and
    /// say where they are with their next beat (spec "Channel moves").
    pub(super) fn take_listeners(&mut self, old: Channel, now: Instant) {
        let home = self.home;
        for i in 0..self.heard.len() {
            let Some(n) = self.heard.iter().nth(i).copied() else {
                break;
            };
            if n.home == old && self.listening(n.id, now) {
                self.heard.moved(n.id, home);
                self.neighbours.home_heard(n.id, home);
            }
        }
    }

    pub(super) fn sweep_due(&self, now: Instant) -> bool {
        self.sweeping() && now >= self.radio.sweep.next_at
    }

    /// Whether the node sweeps the channels: a relay, and a sleepy node
    /// while it boots; receiving duty-cycled it could not (module
    /// documentation).
    pub(super) fn sweeping(&self) -> bool {
        !(self.sleepy() && self.stage == Stage::Running)
    }

    /// Each channel once per sweep interval, one reading at a time (spec
    /// "Busy estimate", roadmap D86): each reading of another channel loses
    /// the packets whose preamble starts in a 1.5 ms window around it.
    pub(super) fn sweep_step(&self) -> Duration {
        self.config.sweep_interval.div(CHANNELS as u64)
    }

    pub(super) fn start_sweep(&mut self, emit: &mut dyn FnMut(Action)) -> bool {
        self.radio.sweep.next_at += self.sweep_step();
        self.radio
            .excursion
            .read(Job::Sweep, self.radio.sweep.channel, emit);
        true
    }

    /// A reading of the home channel is held for one sweep interval, longer
    /// than any packet it can fall in, and then counts as a reading of any
    /// other channel does, unless energy that would follow this relay
    /// was on the air when it was taken (spec "Busy estimate").
    pub(super) fn sweep_sample(&mut self, channel: Channel, dbm: i16, now: Instant) {
        if channel == self.home {
            // A reading that fell in this relay's own packet says nothing
            // of what would stay behind, and is not taken.
            if let Some((c, dbm)) = self
                .radio
                .sweep
                .pending
                .take()
                .and_then(|p| p.dbm.map(|d| (p.channel, d)))
            {
                self.radio.busy.sample(c, dbm);
            }
            self.radio.sweep.pending = Some(HomeReading {
                channel,
                at: now,
                dbm: Some(dbm),
            });
        } else {
            self.radio.busy.sample(channel, dbm);
        }
        self.radio.sweep.channel = channel.next();
        // A reading held up by receptions (core-interface rule 8) or by
        // a hand-over must not leave a backlog to take back to back.
        if self.radio.sweep.next_at <= now {
            self.radio.sweep.next_at = now + self.sweep_step();
        }
        self.neighbours.expire(now);
        if channel.index() + 1 < CHANNELS {
            return;
        }
        // Once per round of the channels.
        if self.stage == Stage::Running && self.config.home_override.is_none() && self.sweeping() {
            self.move_rule(now);
        }
    }

    /// `busy[home] − min busy` above the move threshold (spec "Channel moves").
    pub(super) fn excess(&self) -> bool {
        let min = Channel::all()
            .map(|c| self.radio.busy.fraction(c))
            .min()
            .unwrap_or(0);
        self.radio.busy.fraction(self.home).saturating_sub(min)
            > percent(self.config.move_threshold_percent)
    }

    pub(super) fn move_rule(&mut self, now: Instant) {
        if !self.excess() {
            self.timers.clear_move();
            return;
        }
        let since = *self.timers.excess_since.get_or_insert(now);
        if self.timers.move_at.is_none() && now.since(since) >= self.config.move_dwell {
            self.timers.move_at =
                Some(now + self.rng.duration(Duration::ZERO, self.config.move_wait_max));
        }
    }

    /// Set aside a home reading taken while a reception that follows this
    /// relay to any channel was on the air: a packet addressed to it, or
    /// the reply to its own hand-over (spec "Busy estimate"). Everything
    /// else stays behind, and the reading counts it as the sweep counts
    /// any channel's.
    pub(super) fn set_aside_own(&mut self, bytes: &[u8], now: Instant) {
        let me = self.me();
        let own = match Packet::decode(bytes) {
            Ok(Packet::Data(d)) => d.dst_hop == me,
            Ok(Packet::Ack(a)) => self.in_flight().is_some_and(|o| o.answered_by(a, me)),
            _ => false,
        };
        if !own {
            return;
        }
        // A frame's preamble is not reported: one addressed to a node
        // that sweeps has the shortest.
        let len = bytes.len().min(u8::MAX as usize) as u8;
        let airtime = time_on_air(len, MIN_PREAMBLE_SYMBOLS);
        if let Some(p) = &mut self.radio.sweep.pending {
            if now.since(p.at) <= airtime {
                p.dbm = None;
            }
        }
    }
}
