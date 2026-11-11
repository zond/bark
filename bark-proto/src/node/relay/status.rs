//! A neighbour's status asked and told (spec "Neighbour status"). The application
//! asks while a person looks at the node's neighbours; each neighbour
//! asked is marked in the neighbour table, and so is each that asked this
//! node, and the status job sends one frame at a time, an answer before a
//! question: on the neighbour's home channel, with the long preamble to
//! a sleepy one, which receives duty-cycled, after a listen-before-talk
//! and paid from the bucket with the reply reserve kept, as a piece is.
//! Neither is answered or sent again: the app asks again a neighbour
//! that has not answered.

use super::{Job, Node};
use crate::event::{Action, AppEvent, Frame, Trace};
use crate::ids::NodeId;
use crate::node::mac;
use crate::radio::MIN_PREAMBLE_SYMBOLS;
use crate::sniff::LONG_PREAMBLE_SYMBOLS;
use crate::status::Status;
use crate::tables::budget::REPLY_RESERVE;
use crate::time::{Duration, Instant};
use crate::wire::{Packet, StatusFrame};

/// The status job's state, in 16 bytes: the node's state beside its
/// tables is held under 1 KiB (`docs/core-interface.md` §6).
#[derive(Clone, Copy, Debug)]
pub(super) struct Asking {
    /// The frame under way: to whom, whether it tells, and the checks
    /// that found its channel busy.
    current: Option<(NodeId, bool, u8)>,
    /// Not before this: a listen-before-talk backoff or the bucket;
    /// [`Instant::MAX`] while no neighbour is marked.
    ready_at: Instant,
}

impl Asking {
    pub(super) const IDLE: Asking = Asking {
        current: None,
        ready_at: Instant::MAX,
    };

    /// A neighbour was marked at `now`: due now, unless a wait is under
    /// way.
    fn marked(&mut self, now: Instant) {
        if self.ready_at == Instant::MAX {
            self.ready_at = now;
        }
    }
}

impl<const H: usize, const Q: usize> Node<H, Q> {
    /// What this node's screen shows of it now, which its status tells
    /// (spec "Neighbour status").
    pub fn status(&self, now: Instant) -> Status {
        Status {
            placed: self.placed(),
            sleepy: self.sleepy(),
            anchored: self.anchor().is_some(),
            carried: self.carried(),
            battery: self.battery.status(),
            neighbours: self.neighbour_counts(now),
            version: self.config.version,
            gathering: self.gathering_status(),
        }
    }

    /// The application asks neighbour `id` for its status.
    pub(super) fn ask(&mut self, id: NodeId, now: Instant) {
        if self.neighbours.status_due(id, false, now) {
            self.radio.asking.marked(now);
        }
    }

    /// A status frame was received.
    pub(super) fn status_heard(
        &mut self,
        f: StatusFrame,
        now: Instant,
        emit: &mut dyn FnMut(Action),
    ) {
        match f {
            StatusFrame::Ask { to, from } if to == self.id => {
                // An asker this node does not hear could not hear it
                // either, and its home channel is not known.
                if self.neighbours.status_due(from, true, now) {
                    self.radio.asking.marked(now);
                }
            }
            StatusFrame::Tell { to, from, status } if to == self.id => {
                emit(Action::App(AppEvent::Status { from, status }));
            }
            _ => {}
        }
    }

    /// When the status job is next ready, if a frame may be due.
    pub(super) fn status_ready_at(&self) -> Option<Instant> {
        let at = self.radio.asking.ready_at;
        (at != Instant::MAX).then_some(at)
    }

    /// The frame to `to`, telling or asking, as it goes now; `None` if
    /// `to` is no longer in the table.
    fn status_frame(&self, to: NodeId, tell: bool, now: Instant) -> Option<Frame> {
        let n = self.neighbours.get(to)?;
        let from = self.id;
        let f = match tell {
            true => StatusFrame::Tell {
                to,
                from,
                status: self.status(now),
            },
            false => StatusFrame::Ask { to, from },
        };
        let preamble = match n.sleepy {
            true => LONG_PREAMBLE_SYMBOLS,
            false => MIN_PREAMBLE_SYMBOLS,
        };
        Some(mac::frame(&Packet::Status(f), n.home, preamble))
    }

    /// Start the next status frame due: its check of the channel, once the
    /// bucket holds it with the reply reserve left.
    pub(super) fn start_status(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) -> bool {
        if now < self.radio.asking.ready_at {
            return false;
        }
        let (to, tell) = match self.radio.asking.current {
            Some((to, tell, _)) => (to, tell),
            None => {
                let Some((to, tell)) = self.neighbours.next_status(now) else {
                    self.radio.asking.ready_at = Instant::MAX;
                    return false;
                };
                self.radio.asking.current = Some((to, tell, 0));
                (to, tell)
            }
        };
        let Some(f) = self.status_frame(to, tell, now) else {
            self.status_done(to, tell);
            return false;
        };
        let airtime = mac::airtime(&f);
        if !self.affordable(airtime, now) {
            self.radio.asking.ready_at = self
                .radio
                .budget()
                .allowed_at(airtime, REPLY_RESERVE, now)
                .unwrap_or(Instant::MAX);
            return false;
        }
        self.radio.check(Job::Status, f.channel, emit);
        true
    }

    /// The check of the status frame's channel completed: send it if the
    /// channel is clear, else wait a backoff, or give it up after as many
    /// checks as a heartbeat's (spec "Heartbeat").
    pub(super) fn status_check(&mut self, busy: bool, now: Instant, emit: &mut dyn FnMut(Action)) {
        let Some((to, tell, checks)) = self.radio.asking.current else {
            return;
        };
        if busy {
            let checks = checks + 1;
            self.radio.asking.current = Some((to, tell, checks));
            if checks >= self.config.lbt_attempts {
                self.status_done(to, tell);
                emit(Action::Trace(Trace::StatusGivenUp { to, answer: tell }));
            } else {
                self.radio.asking.ready_at = now
                    + self
                        .rng
                        .duration(Duration::ZERO, self.config.lbt_backoff_max);
            }
            return;
        }
        let Some(f) = self.status_frame(to, tell, now) else {
            self.status_done(to, tell);
            return;
        };
        let airtime = mac::airtime(&f);
        if !self.affordable(airtime, now) {
            // An owed reply spent the bucket since the check began.
            return;
        }
        self.status_done(to, tell);
        self.radio.send(Job::Status, f, now, emit);
    }

    /// The frame to `to` went, or was given up.
    fn status_done(&mut self, to: NodeId, tell: bool) {
        self.neighbours.status_gone(to, tell);
        self.radio.asking.current = None;
    }
}
