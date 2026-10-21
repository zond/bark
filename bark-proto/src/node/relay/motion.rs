//! A node that moves (spec "Position and time", roadmap D70).
//!
//! A sleepy node takes each GPS fix the driver hands it. A node that is not
//! sleepy is fixed or carried. Fixed, it keeps its first fix or its map
//! point, so that no later fix, true or spoofed, moves it (roadmap D63); a
//! fix farther than the GPS allowance ([`OWNER_RADIUS_M`]) from its
//! position is a far fix. At the first far fix of a run it flushes its
//! delivery queue: each message held goes to its node once, with the long
//! preamble (`relay/delivery.rs`), since a node that is leaving will not
//! hear its sleepy nodes' beats for long. After far fixes for
//! [`CARRY_AFTER`] with none within the allowance between, it is carried:
//! it takes each fix, flags its heartbeats, owns nothing by position and
//! so queues mail for nobody, and nobody anchors to it. It is fixed again
//! once its fixes have stayed within the allowance of one place for
//! [`Config::fixed_after`], and keeps the fix it has then.
//!
//! [`Config::fixed_after`]: crate::Config::fixed_after

use super::Node;
use crate::event::{Action, Trace};
use crate::node::{CARRY_AFTER, OWNER_RADIUS_M};
use crate::position::Position;
use crate::time::Instant;

/// Whether a node that is not sleepy is fixed or carried, and the fixes
/// that decide it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Motion {
    /// It moves: it takes each fix and holds mail for nobody.
    pub(super) carried: bool,
    /// Fixed: since when its fixes have been far, with none near since.
    far_since: Option<Instant>,
    /// Carried: the place its fixes have stayed within the allowance of,
    /// and since when.
    still_at: Position,
    still_since: Instant,
}

impl Motion {
    /// Fixed, with no far fix.
    pub(super) const FIXED: Motion = Motion {
        carried: false,
        far_since: None,
        still_at: Position { lat: 0, lon: 0 },
        still_since: Instant::ZERO,
    };
}

impl<const H: usize, const Q: usize> Node<H, Q> {
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
    pub(super) fn map_point(&mut self, position: Position) {
        self.set_position(position);
        self.motion = Motion::FIXED;
    }

    /// The driver hands a GPS fix that passed its checks (module
    /// documentation). Returns whether the node took it as its position.
    pub(super) fn fix(&mut self, at: Position, now: Instant, emit: &mut dyn FnMut(Action)) -> bool {
        if self.config.sleepy {
            self.set_position(at);
            return true;
        }
        let r2 = OWNER_RADIUS_M * OWNER_RADIUS_M;
        if self.motion.carried {
            self.set_position(at);
            let still = &mut self.motion;
            if self
                .plane
                .point(still.still_at)
                .distance2(Default::default())
                > r2
            {
                still.still_at = at;
                still.still_since = now;
            } else if now.since(still.still_since) >= self.config.fixed_after {
                self.motion = Motion::FIXED;
                emit(Action::Trace(Trace::Carried { carried: false }));
            }
            return true;
        }
        if self.plane.point(at).distance2(Default::default()) <= r2 {
            self.motion.far_since = None;
            return false;
        }
        let since = match self.motion.far_since {
            Some(t) => t,
            None => {
                self.motion.far_since = Some(now);
                emit(Action::Trace(Trace::FarFix));
                self.feed_due = true;
                now
            }
        };
        if now.since(since) >= CARRY_AFTER {
            self.set_position(at);
            self.motion = Motion {
                carried: true,
                far_since: None,
                still_at: at,
                still_since: now,
            };
            emit(Action::Trace(Trace::Carried { carried: true }));
            return true;
        }
        false
    }
}
