//! When a node runs its GPS (spec "Position", "Beats"): a sleepy node's
//! searches ([`Action::Locate`]), and a relay's GPS that runs or rests and,
//! resting, watches its neighbours' heartbeats for a sign that it moved.

use super::{Node, Stage};
use crate::event::{Action, Trace};
use crate::geo::LocalPlane;
use crate::gps::{BACKOFF, HEARING_RADIUS_M, SEARCH_LIMIT};
use crate::node::Config;
use crate::position::Position;
use crate::radio::FADE_DEPTH_DB;
use crate::time::{Duration, Instant};

/// How far beyond its mean, in its standard deviations, a rotation's sum of
/// squared surprises must lie before a resting relay searches: the
/// switching margin's convention, 2.3 % of a still node's rotations
/// (spec "Position", "Relay choice").
const SURPRISE_SIGMAS: f32 = 2.0;

/// The variance of a heartbeat's surprise under fading of [`FADE_DEPTH_DB`]:
/// the frame's σ² and the average's σ²/3 (spec "Position"). Frames in a
/// deep fade are not heard, so a still relay's surprises vary less, and the
/// test errs towards silence.
const SURPRISE_VAR_DB2: f32 = FADE_DEPTH_DB * FADE_DEPTH_DB * 4.0 / 3.0;

/// The point [`SURPRISE_SIGMAS`] above the mean of a chi-square of `k`
/// degrees of freedom, by the Wilson–Hilferty approximation.
fn chi_square_point(k: u8) -> f32 {
    let k = k as f32;
    let a = 2.0 / (9.0 * k);
    k * libm::powf(1.0 - a + SURPRISE_SIGMAS * libm::sqrtf(a), 3.0)
}

/// A node's GPS: the search under way, and what its role keeps. Times are
/// whole seconds from the clock's zero, 0 for none, so that the node
/// stays within the size `docs/core-interface.md` §6 gives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Gps {
    until: u32,
    /// A node is sleepy or a relay at a time, so the two share the room;
    /// each starts afresh when the node takes its role.
    role: Role,
}

/// [`Searches`] or [`Watch`], their fields held in the variants so that
/// the tag fits beside them and the node stays within its size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Sleepy {
        failed_at: u32,
        tried_at: u32,
        due: bool,
    },
    Relay {
        since: u32,
        sum_milli: u32,
        surprises: u8,
        flags: u8,
    },
}

impl From<Searches> for Role {
    fn from(s: Searches) -> Role {
        Role::Sleepy {
            failed_at: s.failed_at,
            tried_at: s.tried_at,
            due: s.due,
        }
    }
}

impl From<Watch> for Role {
    fn from(w: Watch) -> Role {
        Role::Relay {
            since: w.since,
            sum_milli: w.sum_milli,
            surprises: w.surprises,
            flags: w.flags,
        }
    }
}

/// A sleepy node's searches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Searches {
    /// When its last search found nothing, until a fix comes.
    failed_at: u32,
    tried_at: u32,
    due: bool,
}

/// A relay's watch on its radio, and what keeps its GPS running.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Watch {
    since: u32,
    /// The sum of the rotation's squared surprises over
    /// [`SURPRISE_VAR_DB2`], in thousandths.
    sum_milli: u32,
    surprises: u8,
    flags: u8,
}

const DUE: u8 = 1;

/// Its last fix was far: its GPS runs until it is carried or a fix is
/// near.
const FOLLOW: u8 = 2;

/// It has been on battery longer than its grace: its GPS rests.
const PAST_GRACE: u8 = 4;

impl Watch {
    fn has(&self, flag: u8) -> bool {
        self.flags & flag != 0
    }

    fn set(&mut self, flag: u8, on: bool) {
        self.flags = if on {
            self.flags | flag
        } else {
            self.flags & !flag
        };
    }

    /// Whether `flag` was set; it is not now.
    fn take(&mut self, flag: u8) -> bool {
        let was = self.has(flag);
        self.set(flag, false);
        was
    }
}

impl Gps {
    pub(super) const DEFAULT: Gps = Gps {
        until: 0,
        role: Role::Relay {
            since: 0,
            sum_milli: 0,
            surprises: 0,
            flags: 0,
        },
    };

    fn searches<R>(&mut self, f: impl FnOnce(&mut Searches) -> R) -> R {
        let mut s = self.searches_now();
        let r = f(&mut s);
        self.role = s.into();
        r
    }

    fn watch<R>(&mut self, f: impl FnOnce(&mut Watch) -> R) -> R {
        let mut w = self.watch_now();
        let r = f(&mut w);
        self.role = w.into();
        r
    }

    fn searches_now(&self) -> Searches {
        match self.role {
            Role::Sleepy {
                failed_at,
                tried_at,
                due,
            } => Searches {
                failed_at,
                tried_at,
                due,
            },
            Role::Relay { .. } => Searches::default(),
        }
    }

    fn watch_now(&self) -> Watch {
        match self.role {
            Role::Relay {
                since,
                sum_milli,
                surprises,
                flags,
            } => Watch {
                since,
                sum_milli,
                surprises,
                flags,
            },
            Role::Sleepy { .. } => Watch::default(),
        }
    }
}

/// `t` in whole seconds, rounded up, and at least 1, as 0 means none.
pub(super) fn mark(t: Instant) -> u32 {
    (t.as_micros().div_ceil(1_000_000) as u32).max(1)
}

fn at(m: u32) -> Option<Instant> {
    (m > 0).then(|| Instant::ZERO + Duration::from_secs(m as u64))
}

/// What the GPS's rules read of the rest of the node.
#[derive(Clone, Copy, Debug)]
pub(super) struct Situation {
    pub(super) sleepy: bool,
    pub(super) running: bool,
    pub(super) map_point: bool,
    pub(super) carried: bool,
    pub(super) placed: bool,
    pub(super) anchored: bool,
    /// A sleepy node without an anchor, past its boot listen.
    pub(super) anchorless: bool,
    /// Since when a relay has been on battery, or leaving.
    pub(super) unplugged: Option<Instant>,
}

impl Gps {
    /// Whether the driver is to run the GPS all the time now
    /// (spec "Position", a relay's GPS); a sleepy node's runs only in the
    /// searches it asks for.
    pub(super) fn tracks(&self, s: &Situation) -> bool {
        if s.sleepy || s.map_point {
            return false;
        }
        let w = self.watch_now();
        let rests = s.unplugged.is_some() && w.has(PAST_GRACE);
        s.carried || w.has(FOLLOW) || !rests
    }

    /// Whether a relay rests its GPS and watches the radio for a sign that
    /// it moves: then the heartbeats of its fixed relay neighbours go to
    /// [`Gps::surprise`].
    pub(super) fn watches(&self, s: &Situation) -> bool {
        !s.sleepy && s.running && !s.map_point && !self.tracks(s)
    }

    /// A heartbeat of a fixed relay neighbour came `surprise_db` from that
    /// neighbour's averaged RSSI before it (spec "Position").
    pub(super) fn surprise(
        &mut self,
        s: &Situation,
        surprise_db: f32,
        rotation: Duration,
        now: Instant,
        emit: &mut dyn FnMut(Action),
    ) {
        let watches = self.watches(s);
        self.watch(|w| {
            if !watches {
                w.since = 0;
                return;
            }
            if at(w.since).is_some_and(|t| now >= t + rotation) {
                let moved =
                    w.surprises > 0 && w.sum_milli as f32 / 1000.0 > chi_square_point(w.surprises);
                if moved {
                    w.set(DUE, true);
                }
                emit(Action::Trace(Trace::MoveSign {
                    surprises: w.surprises,
                    sum_milli: w.sum_milli,
                    moved,
                }));
                w.since = 0;
            }
            if w.since == 0 {
                *w = Watch {
                    since: mark(now),
                    sum_milli: 0,
                    surprises: 0,
                    ..*w
                };
            }
            if w.surprises == u8::MAX {
                return;
            }
            w.surprises += 1;
            let z2 = surprise_db * surprise_db / SURPRISE_VAR_DB2;
            w.sum_milli = w.sum_milli.saturating_add((z2 * 1000.0) as u32);
        });
    }

    /// The node turned anchorless, or boots as a relay whose GPS rests:
    /// it searches.
    pub(super) fn due(&mut self, sleepy: bool) {
        if sleepy {
            self.searches(|s| s.due = true);
        } else {
            self.watch(|w| w.set(DUE, true));
        }
    }

    /// The node took an anchor and works by its position: a search under
    /// way ends, unless the node has no place. A `Locate` at or before now
    /// ends a search (`docs/core-interface.md` §4).
    pub(super) fn anchored(&mut self, placed: bool, now: Instant, emit: &mut dyn FnMut(Action)) {
        if Self::searches_anchored(placed) {
            return;
        }
        self.searches(|s| s.due = false);
        if self.until > 0 {
            self.until = 0;
            emit(Action::Locate(now));
        }
    }

    pub(super) fn fixed(&mut self, sleepy: bool) {
        self.until = 0;
        if sleepy {
            self.searches(|s| s.failed_at = 0);
        }
    }

    /// A fix came to a relay and was handled: a `far` one keeps its GPS on.
    pub(super) fn after_fix(&mut self, sleepy: bool, far: bool) {
        if !sleepy {
            self.watch(|w| w.set(FOLLOW, far));
        }
    }

    pub(super) fn relay(&mut self) {
        *self = Gps::DEFAULT;
    }

    /// The node turned sleepy: a search under way runs on as a sleepy
    /// node's.
    pub(super) fn sleepy(&mut self) {
        self.role = Searches::default().into();
    }

    fn grace_ends(s: &Situation, grace: Duration) -> Option<Instant> {
        Some(s.unplugged? + grace)
    }

    fn allowed_at(&self) -> Instant {
        at(self.searches_now().failed_at).map_or(Instant::ZERO, |t| t + BACKOFF)
    }

    /// Whether a sleepy node searches while it has an anchor: only one
    /// with no place, which cannot rebuild its anchor's position without
    /// a fix of its own.
    fn searches_anchored(placed: bool) -> bool {
        !placed
    }

    /// When a sleepy node's next search is due of its own accord: anchorless,
    /// once a delivery TTL (spec "Sleepy nodes alone"); without a place,
    /// when the back-off ends.
    fn sleepy_at(&self, s: &Situation, delivery_ttl: Duration) -> Option<Instant> {
        if self.until > 0 {
            return None;
        }
        if Self::searches_anchored(s.placed) {
            return Some(self.allowed_at());
        }
        if !s.anchorless {
            return None;
        }
        let tried = self.searches_now().tried_at;
        let next = at(tried).map_or(Instant::ZERO, |t| t + delivery_ttl);
        Some(next.max(self.allowed_at()))
    }

    /// Start a search that is due, and end one at its limit; note a
    /// relay's grace on battery passing.
    pub(super) fn fire(
        &mut self,
        s: &Situation,
        config: &Config,
        now: Instant,
        emit: &mut dyn FnMut(Action),
    ) {
        if !s.sleepy {
            // Once past, past until external power: the flush of a relay
            // leaving restarts `unplugged` for its own bound, and its GPS
            // is not to wake for it at a low battery.
            let on_battery = s.unplugged.is_some();
            let ended = Self::grace_ends(s, config.grace).is_some_and(|t| now >= t);
            self.watch(|w| {
                let past = on_battery && (ended || w.has(PAST_GRACE));
                w.set(PAST_GRACE, past);
            });
        }
        if at(self.until).is_some_and(|t| now >= t) {
            self.until = 0;
            if s.sleepy {
                self.searches(|s| s.failed_at = mark(now));
            }
        }
        if self
            .sleepy_at(s, config.delivery_ttl)
            .is_some_and(|t| now >= t)
        {
            self.searches(|s| s.due = true);
        }
        // A search due while it cannot start is dropped.
        let due = if s.sleepy {
            // No fix is taken while a map point is set, nor with an anchor.
            let anchored = s.anchored && !Self::searches_anchored(s.placed);
            let held = now < self.allowed_at() || s.map_point || anchored;
            self.searches(|s| core::mem::take(&mut s.due)) && !held
        } else {
            let rests = !self.tracks(s) && !s.map_point;
            self.watch(|w| w.take(DUE)) && rests
        };
        if !due || self.until > 0 {
            return;
        }
        self.until = mark(now + SEARCH_LIMIT);
        if s.sleepy {
            self.searches(|s| s.tried_at = mark(now));
        }
        emit(Action::Locate(at(self.until).expect("just set")));
    }

    pub(super) fn next_wakeup(&self, s: &Situation, config: &Config) -> Option<Instant> {
        let other = if s.sleepy {
            self.sleepy_at(s, config.delivery_ttl)
        } else {
            Self::grace_ends(s, config.grace).filter(|_| !self.watch_now().has(PAST_GRACE))
        };
        match (at(self.until), other) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
}

impl<const H: usize, const Q: usize> Node<H, Q> {
    pub(super) fn gps_situation(&self) -> Situation {
        Situation {
            sleepy: self.sleepy(),
            running: self.stage == Stage::Running,
            map_point: self.motion.map_point(),
            carried: self.motion.carried(),
            placed: self.motion.placed(),
            anchored: self.anchored_sleepy(),
            anchorless: self.anchorless(),
            unplugged: self.unplugged(),
        }
    }

    /// Whether the driver is to run the GPS all the time now
    /// ([`Gps::tracks`]).
    pub fn gps_tracks(&self) -> bool {
        self.gps.tracks(&self.gps_situation())
    }

    /// Whether a fix at `position` passes the check against the relays this
    /// node knows (spec "Position"); with no place it cannot rebuild where
    /// they are, so it passes.
    pub fn fix_in_reach(&self, position: Position) -> bool {
        if !self.placed() {
            return true;
        }
        let plane = LocalPlane::new(position);
        let mut known = self.neighbours().map(|n| n.position).peekable();
        known.peek().is_none()
            || known.any(|p| plane.point(p).distance_m(Default::default()) <= HEARING_RADIUS_M)
    }
}
