//! A node that moves: whether it is fixed or carried, measured from its
//! fixes, and which fixes it takes as its position (spec "Position",
//! "Nodes"). A relay that is not carried keeps its place as its position,
//! so no later fix, true or spoofed, moves it.

use super::gps::mark;
use super::Node;
use crate::event::{Action, Trace};
use crate::geo::LocalPlane;
use crate::gps::SEARCH_LIMIT;
use crate::node::{CARRY_AFTER, CARRY_RADIUS_M};
use crate::position::Position;
use crate::time::{Duration, Instant};
use crate::Config;

/// Whether a node is fixed or carried, and the fixes that decide it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Motion {
    carried: bool,
    /// Its position is a map point; until it is cleared no fix is taken.
    map_point: bool,
    /// It has a place (spec "Position"); a board built without one has
    /// none until its first accepted fix.
    placed: bool,
    /// Until when its next accepted fix is its place, in whole seconds as
    /// the GPS keeps its times; 0 for never, `u32::MAX` for good.
    first_fix: u32,
    /// Fixed: since when its fixes have been far, moved on by the time the
    /// GPS gave no fix.
    far_since: Option<Instant>,
    far_last: Instant,
    /// The GPS lost its fix since [`Motion::far_last`].
    far_lost: bool,
    /// The place its fixes are compared with, and, carried, since when
    /// they have stayed near it.
    still_at: Position,
    still_since: Instant,
}

/// What a fix does beyond the node's motion ([`Motion::fix`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Fixed {
    /// Not taken: a map point is set, or a sleepy node has an anchor.
    Not,
    /// The node's place, fixed there at once.
    Place,
    /// Measured; `turned`: the carried flag if it changed.
    Measured {
        takes: bool,
        far_began: bool,
        turned: Option<bool>,
    },
}

impl Motion {
    /// Fixed at `at`, with no far fix.
    pub(super) const fn fixed(at: Position) -> Motion {
        Motion {
            carried: false,
            map_point: false,
            placed: true,
            first_fix: 0,
            far_since: None,
            far_last: Instant::ZERO,
            far_lost: false,
            still_at: at,
            still_since: Instant::ZERO,
        }
    }

    /// Fixed where `config` puts it after a boot. A carried node's stored
    /// place is where it last stood, so it is no place (spec "Position").
    pub(super) const fn at_boot(config: &Config) -> Motion {
        let placed = config.placed
            || match config.stored {
                Some(s) => !s.carried,
                None => false,
            };
        Motion {
            map_point: match config.stored {
                Some(s) => s.map_point && !s.carried,
                None => false,
            },
            placed,
            first_fix: if placed { 0 } else { u32::MAX },
            ..Motion::fixed(config.position)
        }
    }

    pub(super) const fn carried_at(at: Position, now: Instant) -> Motion {
        Motion {
            carried: true,
            map_point: false,
            placed: true,
            first_fix: 0,
            far_since: None,
            far_last: Instant::ZERO,
            far_lost: false,
            still_at: at,
            still_since: now,
        }
    }

    /// The place a fixed node stands, and a carried one stood when it
    /// was last fixed (`store.rs`).
    pub(super) fn place(&self) -> Position {
        self.still_at
    }

    pub(super) fn carried(&self) -> bool {
        self.carried
    }

    pub(super) fn map_point(&self) -> bool {
        self.map_point
    }

    pub(super) fn placed(&self) -> bool {
        self.placed
    }

    /// Whether its last fix was far, or it is carried: it flushes what
    /// it holds for its sleepy nodes.
    pub(super) fn far(&self) -> bool {
        self.carried || self.far_since.is_some()
    }

    /// A map point was set: fixed there. Returns whether it was carried.
    pub(super) fn set_map_point(&mut self, at: Position) -> bool {
        let was = self.carried;
        *self = Motion {
            map_point: true,
            ..Motion::fixed(at)
        };
        was
    }

    /// The map point was cleared: the next accepted fix is the place.
    /// Returns whether there was one.
    pub(super) fn clear_map_point(&mut self) -> bool {
        if !self.map_point {
            return false;
        }
        self.map_point = false;
        self.first_fix = u32::MAX;
        true
    }

    /// A GPS fix (spec "Position").
    pub(super) fn fix(
        &mut self,
        at: Position,
        plane: &LocalPlane,
        sleepy: bool,
        anchored: bool,
        fixed_after: Duration,
        now: Instant,
    ) -> Fixed {
        // No fix moves a map point; the driver hands none meanwhile.
        if self.map_point {
            return Fixed::Not;
        }
        if mark(now) <= self.first_fix && !self.carried {
            return Fixed::Place;
        }
        // With an anchor a sleepy node asks for no fix; one that comes all
        // the same, the search it ended still running, is not taken.
        if anchored {
            return Fixed::Not;
        }
        let far = plane.distance2(at, self.still_at) > CARRY_RADIUS_M * CARRY_RADIUS_M;
        let takes = sleepy || self.carried;
        let measured = |far_began, turned| Fixed::Measured {
            takes,
            far_began,
            turned,
        };
        if self.carried {
            if far {
                self.still_at = at;
                self.still_since = now;
            } else if now.since(self.still_since) >= fixed_after {
                *self = Motion::fixed(at);
                return measured(false, Some(false));
            }
            return measured(false, None);
        }
        if !far {
            self.far_since = None;
            return measured(false, None);
        }
        let began = self.far_since.is_none();
        let since = match self.far_since {
            Some(t) if self.far_lost => t + now.since(self.far_last),
            Some(t) => t,
            None => now,
        };
        self.far_since = Some(since);
        self.far_last = now;
        self.far_lost = false;
        if now.since(since) < CARRY_AFTER {
            return measured(began, None);
        }
        *self = Motion::carried_at(at, now);
        measured(began, Some(true))
    }

    /// The running GPS gave no usable fix: the time until the next fix does
    /// not count towards a run of far fixes.
    pub(super) fn no_fix(&mut self) {
        self.far_lost = true;
    }

    /// Plugged in at `at` (spec "Position", power): a fixed node takes a
    /// fix within a search's limit as its place, since a sleepy node's last
    /// fix may be hours old.
    pub(super) fn plugged_in(&mut self, at: Position, now: Instant) {
        if !self.carried {
            let map_point = self.map_point;
            *self = Motion {
                map_point,
                placed: self.placed,
                first_fix: if map_point {
                    0
                } else {
                    mark(now + SEARCH_LIMIT)
                },
                ..Motion::fixed(at)
            };
        }
    }
}

impl<const H: usize, const Q: usize> Node<H, Q> {
    /// Whether this node has a place (spec "Position"): until it has, its
    /// driver does not check fixes against its neighbours, whose positions
    /// it cannot rebuild.
    pub fn placed(&self) -> bool {
        self.motion.placed()
    }

    /// Whether this node is carried (spec "Position").
    pub fn carried(&self) -> bool {
        self.motion.carried()
    }

    /// Whether its last fix was far, or it is carried: it flushes what
    /// it holds for its sleepy nodes.
    pub(super) fn far(&self) -> bool {
        self.motion.far()
    }

    /// A map point was set: the node is there, and fixed. Returns whether
    /// it was carried.
    pub(super) fn map_point(&mut self, position: Position, now: Instant) -> bool {
        if !self.motion.placed() {
            self.first_place(position, now);
        }
        self.set_position(position);
        self.motion.set_map_point(position)
    }

    /// The map point was cleared: the node stays where it is until its
    /// next accepted fix, which is its place.
    pub(super) fn clear_map_point(&mut self) {
        if self.motion.clear_map_point() {
            self.changed();
        }
    }

    /// A GPS fix that passed the driver's checks (spec "Position").
    /// Returns whether the node took it as its position.
    pub(super) fn fix(&mut self, at: Position, now: Instant, emit: &mut dyn FnMut(Action)) -> bool {
        let sleepy = self.sleepy();
        let fixed = self.motion.fix(
            at,
            &self.plane,
            sleepy,
            self.anchored_sleepy(),
            self.config.fixed_after,
            now,
        );
        let Fixed::Measured {
            takes,
            far_began,
            turned,
        } = fixed
        else {
            if fixed == Fixed::Place {
                self.place_at(at, now);
            }
            return fixed == Fixed::Place;
        };
        if takes {
            self.set_position(at);
        }
        if far_began {
            emit(Action::Trace(Trace::FarFix));
            // A sleepy node holds nothing to flush.
            self.forwarding.feed_due |= !sleepy;
        }
        match turned {
            Some(carried) => {
                if carried {
                    self.set_position(at);
                }
                self.changed();
                emit(Action::Trace(Trace::Carried { carried }));
                true
            }
            None => takes,
        }
    }

    /// A fix taken as the node's place: fixed there at once; a relay whose
    /// place moved says so at once.
    fn place_at(&mut self, at: Position, now: Instant) {
        let had = self.motion.placed();
        let moved = at != self.config.position;
        if !had {
            self.first_place(at, now);
        }
        self.set_position(at);
        self.motion = Motion::fixed(at);
        if had && moved && !self.sleepy() {
            self.changed();
        }
    }

    /// A node with no place has one: it rebuilds the short positions it
    /// heard near it, and heartbeats from now.
    fn first_place(&mut self, at: Position, now: Instant) {
        self.neighbours.rebuild(at);
        if let Some(a) = self.sleep_mut().and_then(|s| s.anchor.as_mut()) {
            a.position = Position::from_short(a.position.short(), at);
        }
        self.radio.beacon.next_at(now);
    }

    /// Plugged in ([`Motion::plugged_in`]).
    pub(super) fn fixed_here(&mut self, now: Instant) {
        self.motion.plugged_in(self.config.position, now);
    }
}
