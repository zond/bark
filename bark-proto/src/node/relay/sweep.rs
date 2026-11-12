//! The busy sweep and the move rule (spec "Busy estimate", "Channel moves").

use super::{Node, Stage};
use crate::event::Action;
use crate::radio::{time_on_air, Channel, CHANNELS, MIN_PREAMBLE_SYMBOLS};
use crate::tables::busy::percent;
use crate::time::{Duration, Instant};
use crate::wire::Packet;

/// The busy sweep (spec "Busy estimate").
pub(super) struct Sweep {
    next_at: Instant,
    channel: Channel,
    /// The last home reading, held until any reception it fell in has
    /// shown whether the energy was meant for this relay.
    pending: Option<HomeReading>,
    /// The move rule (spec "Channel moves").
    excess_since: Option<Instant>,
    move_at: Option<Instant>,
}

impl Sweep {
    /// Before the first reading: from `channel`, due at once.
    pub(super) const fn new(channel: Channel) -> Sweep {
        Sweep {
            next_at: Instant::ZERO,
            channel,
            pending: None,
            excess_since: None,
            move_at: None,
        }
    }

    pub(super) fn move_at(&self) -> Option<Instant> {
        self.move_at
    }

    pub(super) fn clear_move(&mut self) {
        self.excess_since = None;
        self.move_at = None;
    }

    /// A round of readings ended; `excess`: the home is busier than the
    /// quietest channel by more than the threshold.
    pub(super) fn move_rule(
        &mut self,
        excess: bool,
        dwell: Duration,
        wait: impl FnOnce() -> Duration,
        now: Instant,
    ) {
        if !excess {
            self.clear_move();
            return;
        }
        let since = *self.excess_since.get_or_insert(now);
        if self.move_at.is_none() && now.since(since) >= dwell {
            self.move_at = Some(now + wait());
        }
    }

    pub(super) fn next_at(&self) -> Instant {
        self.next_at
    }

    pub(super) fn restart(&mut self, now: Instant) {
        self.next_at = now;
    }

    pub(super) fn forget_home(&mut self) {
        self.pending = None;
    }

    /// A reading starts: the channel it reads.
    pub(super) fn start(&mut self, step: Duration) -> Channel {
        self.next_at += step;
        self.channel
    }

    /// A reading came back: the reading that now counts for the busy
    /// estimate, if any. A home reading is held one sweep interval, longer
    /// than any packet it can fall in (spec "Busy estimate").
    pub(super) fn sampled(
        &mut self,
        channel: Channel,
        dbm: i16,
        home: Channel,
        step: Duration,
        now: Instant,
    ) -> Option<(Channel, i16)> {
        let counts = if channel == home {
            // A reading that fell in a packet for this relay says nothing of
            // what would stay behind.
            let held = self
                .pending
                .take()
                .and_then(|p| p.dbm.map(|d| (p.channel, d)));
            self.pending = Some(HomeReading {
                channel,
                at: now,
                dbm: Some(dbm),
            });
            held
        } else {
            Some((channel, dbm))
        };
        self.channel = channel.next();
        // A reading held up by receptions (core-interface rule 8) or by
        // a hand-over must not leave a backlog to take back to back.
        if self.next_at <= now {
            self.next_at = now + step;
        }
        counts
    }

    /// A reception that would follow this relay to any channel ended: a
    /// home reading taken during it is set aside.
    pub(super) fn set_aside(&mut self, airtime: Duration, now: Instant) {
        if let Some(p) = &mut self.pending {
            if now.since(p.at) <= airtime {
                p.dbm = None;
            }
        }
    }
}

/// A reading of the home channel awaiting [`Sweep::pending`]'s verdict.
#[derive(Clone, Copy)]
struct HomeReading {
    channel: Channel,
    at: Instant,
    /// `None` once a packet for this relay turned out to be on the air
    /// when it was taken.
    dbm: Option<i16>,
}

impl<const H: usize, const Q: usize> Node<H, Q> {
    /// This node moved its home from `old`: sleepy nodes listening there
    /// now hear the heartbeat it sends on `old` and follow; the others tell
    /// their channel with their next beat (spec "Channel moves").
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
        self.sweeping() && now >= self.radio.sweep_at()
    }

    /// Whether the node sweeps: a relay, and a sleepy node while it boots;
    /// receiving duty-cycled it could not.
    pub(super) fn sweeping(&self) -> bool {
        !(self.sleepy() && self.stage == Stage::Running)
    }

    /// How many of its heartbeats' readings the floor of a node that does
    /// not sweep holds: two hours of its beats at one check each, 24, as a
    /// sweeping node's holds two of its windows, so that the floor follows
    /// a rise in about an hour (spec "Busy estimate").
    pub(super) fn check_window(&self) -> u32 {
        let beat = crate::tables::heard::beat_period(self.config.delivery_ttl);
        (2 * 3600 * 1_000_000 / beat.as_micros().max(1)) as u32
    }

    /// One reading at a time, each channel once per sweep interval: each
    /// reading of another channel loses the packets whose preamble starts
    /// in a 0.8 ms window around it (spec "Busy estimate").
    pub(super) fn sweep_step(&self) -> Duration {
        self.config.sweep_interval.div(CHANNELS as u64)
    }

    pub(super) fn start_sweep(&mut self, emit: &mut dyn FnMut(Action)) -> bool {
        let step = self.sweep_step();
        self.radio.start_sweep(step, emit);
        true
    }

    pub(super) fn sweep_sample(&mut self, channel: Channel, dbm: i16, now: Instant) {
        let step = self.sweep_step();
        self.radio.sweep_sample(channel, dbm, self.home, step, now);
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
            .map(|c| self.radio.busy().fraction(c))
            .min()
            .unwrap_or(0);
        self.radio.busy().fraction(self.home).saturating_sub(min)
            > percent(self.config.move_threshold_percent)
    }

    pub(super) fn move_rule(&mut self, now: Instant) {
        let excess = self.excess();
        let (dwell, wait_max) = (self.config.move_dwell, self.config.move_wait_max);
        self.radio.move_rule(
            excess,
            dwell,
            || self.rng.duration(Duration::ZERO, wait_max),
            now,
        );
    }

    /// Set aside a home reading taken while a packet addressed to this
    /// relay, or the reply to its hand-over, was on the air: that energy
    /// follows it to any channel (spec "Busy estimate").
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
        self.radio.set_aside_home_reading(airtime, now);
    }
}
