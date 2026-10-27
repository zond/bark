//! A node that moves (spec "Position", roadmap D70, D72).
//!
//! Whether a node is fixed or carried is measured the same way whether it
//! is sleepy or not. Fixed, its fixes are compared with the place it
//! stands: its first fix, its map point, or the fix it had when it was
//! last made fixed. A fix farther than the GPS allowance
//! ([`OWNER_RADIUS_M`]) from that place is a far fix. After far fixes for
//! [`CARRY_AFTER`] with none within the allowance between, it is carried.
//! Carried, it is fixed again once its fixes have stayed within the
//! allowance of one place for [`Config::fixed_after`], and keeps the fix
//! it has then.
//!
//! A node that is neither sleepy nor carried keeps its place as its
//! position, so that no later fix, true or spoofed, moves it (roadmap
//! D63). A sleepy or carried node takes each fix. A carried node owns
//! nothing by position, holds mail for nobody and is nobody's anchor; it
//! flags its heartbeats. At the first far fix of a run a node that holds
//! mail flushes its delivery queue: each message goes to its node once,
//! with the long preamble (`relay/delivery.rs`), since a node that is
//! leaving will not hear its sleepy nodes' beats for long.
//!
//! [`Config::fixed_after`]: crate::Config::fixed_after

use super::Node;
use crate::event::{Action, Trace};
use crate::node::{CARRY_AFTER, OWNER_RADIUS_M};
use crate::position::Position;
use crate::time::Instant;
use crate::Config;

/// Whether a node is fixed or carried, and the fixes that decide it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Motion {
    /// It moves: it takes each fix and holds mail for nobody.
    pub(super) carried: bool,
    /// Its position is a map point the application set (spec
    /// "Position"); until the application clears it no fix is taken.
    pub(super) map_point: bool,
    /// It has a place: the position it heartbeats is where it is. A
    /// board built without one has none until its first accepted fix.
    pub(super) placed: bool,
    /// Its next accepted fix is its place, fixed, at once: it has none,
    /// or its map point was cleared (roadmap D83, D84).
    pub(super) first_fix: bool,
    /// Fixed: since when its fixes have been far, with none near since.
    far_since: Option<Instant>,
    /// The place its fixes are compared with, and, carried, since when
    /// they have stayed within the allowance of it.
    still_at: Position,
    still_since: Instant,
}

impl Motion {
    /// Fixed at `at`, with no far fix.
    pub(super) const fn fixed(at: Position) -> Motion {
        Motion {
            carried: false,
            map_point: false,
            placed: true,
            first_fix: false,
            far_since: None,
            still_at: at,
            still_since: Instant::ZERO,
        }
    }
}

impl Motion {
    /// Fixed where `config` puts it after a boot, on the map point it
    /// stood on before, if it did.
    pub(super) const fn at_boot(config: &Config) -> Motion {
        let placed = config.placed || config.stored.is_some();
        Motion {
            map_point: match config.stored {
                Some(s) => s.map_point && !s.carried,
                None => false,
            },
            placed,
            first_fix: !placed,
            ..Motion::fixed(config.position)
        }
    }

    /// Carried at `at`, still there since `now`.
    pub(super) const fn carried(at: Position, now: Instant) -> Motion {
        Motion {
            carried: true,
            map_point: false,
            placed: true,
            first_fix: false,
            far_since: None,
            still_at: at,
            still_since: now,
        }
    }

    /// The place a fixed node stands, and a carried one stood when it
    /// was last fixed (`store.rs`).
    pub(super) fn place(&self) -> Position {
        self.still_at
    }
}

impl<const H: usize, const Q: usize> Node<H, Q> {
    /// Whether this node has a place (module documentation): until it
    /// has, its driver does not check fixes against its neighbours, whose
    /// positions it cannot rebuild.
    pub fn placed(&self) -> bool {
        self.motion.placed
    }

    /// Whether this node is carried (module documentation).
    pub fn carried(&self) -> bool {
        self.motion.carried
    }

    /// Whether its last fix was far, or it is carried: it flushes what
    /// it holds for its sleepy nodes.
    pub(super) fn far(&self) -> bool {
        self.motion.carried || self.motion.far_since.is_some()
    }

    /// The application set a map point: the node is there, and fixed.
    /// Returns whether it was carried.
    pub(super) fn map_point(&mut self, position: Position, now: Instant) -> bool {
        let was = self.motion.carried;
        if !self.motion.placed {
            self.first_place(position, now);
        }
        self.set_position(position);
        self.motion = Motion {
            map_point: true,
            ..Motion::fixed(position)
        };
        was
    }

    /// The application cleared its map point: the node stays where it is
    /// until its next accepted fix, which is its place.
    pub(super) fn clear_map_point(&mut self) {
        if self.motion.map_point {
            self.motion.map_point = false;
            self.motion.first_fix = true;
            self.changed();
        }
    }

    /// The driver hands a GPS fix that passed its checks (module
    /// documentation). Returns whether the node took it as its position.
    pub(super) fn fix(&mut self, at: Position, now: Instant, emit: &mut dyn FnMut(Action)) -> bool {
        // No fix moves a map point; the driver hands none meanwhile.
        if self.motion.map_point {
            return false;
        }
        if self.motion.first_fix && !self.motion.carried {
            self.place_at(at, now);
            return true;
        }
        let far = self.plane.distance2(at, self.motion.still_at) > OWNER_RADIUS_M * OWNER_RADIUS_M;
        let takes = self.sleepy || self.motion.carried;
        if takes {
            self.set_position(at);
        }
        if self.motion.carried {
            let still = &mut self.motion;
            if far {
                still.still_at = at;
                still.still_since = now;
            } else if now.since(still.still_since) >= self.config.fixed_after {
                self.motion = Motion::fixed(at);
                self.changed();
                emit(Action::Trace(Trace::Carried { carried: false }));
            }
            return true;
        }
        if !far {
            self.motion.far_since = None;
            return takes;
        }
        let since = match self.motion.far_since {
            Some(t) => t,
            None => {
                self.motion.far_since = Some(now);
                emit(Action::Trace(Trace::FarFix));
                // A sleepy node holds nothing to flush.
                self.feed_due |= !self.sleepy;
                now
            }
        };
        if now.since(since) < CARRY_AFTER {
            return takes;
        }
        self.set_position(at);
        self.motion = Motion::carried(at, now);
        self.changed();
        emit(Action::Trace(Trace::Carried { carried: true }));
        true
    }

    /// The first accepted fix of a node that takes it as its place: fixed
    /// there at once. A node that had no place rebuilds its neighbours'
    /// positions and heartbeats from now, a relay its first rotation's
    /// burst first, as at boot; a relay whose place moved says so at once.
    fn place_at(&mut self, at: Position, now: Instant) {
        let had = self.motion.placed;
        let moved = at != self.config.position;
        if !had {
            self.first_place(at, now);
        }
        self.set_position(at);
        self.motion = Motion::fixed(at);
        self.chosen_from = at;
        if had && moved && !self.sleepy {
            self.changed();
        }
    }

    /// A node with no place has one at `at`: the positions it heard,
    /// rebuilt from their short forms near it, and it heartbeats from now.
    fn first_place(&mut self, at: Position, now: Instant) {
        self.neighbours.rebuild(at);
        if let Some(a) = &mut self.anchor {
            a.position = Position::from_short(a.position.short(), at);
        }
        self.beacon.next_at = now;
    }

    /// Plugged in: a carried node stays carried; a fixed one is fixed
    /// where it is now, which it keeps as a relay until its first fix,
    /// which is its place: a sleepy node's last fix may be hours old
    /// (roadmap D80, D85).
    pub(super) fn fixed_here(&mut self) {
        if !self.motion.carried {
            let map_point = self.motion.map_point;
            self.motion = Motion {
                map_point,
                placed: self.motion.placed,
                ..Motion::fixed(self.config.position)
            };
        }
    }
}
