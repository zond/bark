//! When a node runs its GPS (`crate::gps`, spec "Beats", roadmap D80,
//! D82).
//!
//! A sleepy node asks its driver for a search ([`Action::Locate`]) only
//! when the radio says it may have moved: when it turns sleepy or boots
//! sleepy, when it takes an anchor or changes it, when it turns
//! anchorless, and once a delivery TTL while it stays anchorless. A search
//! ends at the first fix or after [`SEARCH_LIMIT`]; one that ends at its
//! limit found nothing, and none follows for [`BACKOFF`]. A fix asked for
//! at a new anchor is where the node chose from: it is taken after the
//! choice, and measuring a move from the position before it would check
//! the anchor just chosen.
//!
//! A relay's GPS runs all the time on external power and for the grace
//! after it is lost ([`Config::grace`]); after that, and whenever its
//! position is a map point, it rests in standby. A resting relay searches
//! at boot, and when the heartbeats of its fixed relay neighbours change
//! over a worst-case rotation more than fading explains. A search that
//! finds a far fix keeps the GPS running until the node is carried or a
//! fix is near again. A relay's search has no back-off.
//!
//! [`Config::grace`]: crate::Config::grace

use super::{Node, Stage};
use crate::event::{Action, Trace};
use crate::gps::{BACKOFF, SEARCH_LIMIT};
use crate::radio::FADE_DEPTH_DB;
use crate::time::{Duration, Instant};

/// How far beyond its mean, in its own standard deviations, the sum of a
/// rotation's squared surprises must lie before a resting relay searches:
/// two, which a still node's passes in 2.3 % of rotations, the convention
/// of the switching margin (spec "Relay choice").
const SURPRISE_SIGMAS: f32 = 2.0;

/// The variance of a heartbeat's surprise, the difference between its RSSI
/// and the neighbour's averaged RSSI before it, under fading of
/// [`FADE_DEPTH_DB`]: the frame's σ² and the average's σ²/3, since each
/// frame counts half in it and a neighbour's heartbeats reach the node's
/// channel a rotation apart, longer than the fading lasts. Frames in a
/// deep fade are not heard, so a still relay's surprises vary less than
/// this, and the test errs towards silence (roadmap D82).
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
    /// Until when the search under way runs.
    until: u32,
    /// What the node keeps as a sleepy node or as a relay. A node is one
    /// at a time, so the two share the room; each starts afresh when the
    /// node takes its role.
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
        at_choice: bool,
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
            at_choice: s.at_choice,
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
    /// When its last search started.
    tried_at: u32,
    /// A search is to start.
    due: bool,
    /// The next fix is the one asked for at a new anchor.
    at_choice: bool,
}

/// A relay's watch on its radio, and what keeps its GPS running.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Watch {
    /// When the rotation of surprises under way began.
    since: u32,
    /// The sum of the rotation's squared surprises over
    /// [`SURPRISE_VAR_DB2`], in thousandths.
    sum_milli: u32,
    /// The rotation's surprises.
    surprises: u8,
    /// [`DUE`], [`FOLLOW`] and [`PAST_GRACE`].
    flags: u8,
}

/// A search is to start.
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
    /// No search under way, none due.
    pub(super) const DEFAULT: Gps = Gps {
        until: 0,
        role: Role::Relay {
            since: 0,
            sum_milli: 0,
            surprises: 0,
            flags: 0,
        },
    };

    /// Change a sleepy node's searches, taken afresh if the node was a
    /// relay.
    fn searches<R>(&mut self, f: impl FnOnce(&mut Searches) -> R) -> R {
        let mut s = self.searches_now();
        let r = f(&mut s);
        self.role = s.into();
        r
    }

    /// Change a relay's watch, taken afresh if the node was sleepy.
    fn watch<R>(&mut self, f: impl FnOnce(&mut Watch) -> R) -> R {
        let mut w = self.watch_now();
        let r = f(&mut w);
        self.role = w.into();
        r
    }

    /// The searches as they stand, none for a relay.
    fn searches_now(&self) -> Searches {
        match self.role {
            Role::Sleepy {
                failed_at,
                tried_at,
                due,
                at_choice,
            } => Searches {
                failed_at,
                tried_at,
                due,
                at_choice,
            },
            Role::Relay { .. } => Searches::default(),
        }
    }

    /// The watch as it stands, none for a sleepy node.
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

/// `t` in whole seconds, rounded up, and at least 1.
fn mark(t: Instant) -> u32 {
    (t.as_micros().div_ceil(1_000_000) as u32).max(1)
}

/// The instant of a mark, `None` for none.
fn at(m: u32) -> Option<Instant> {
    (m > 0).then(|| Instant::ZERO + Duration::from_secs(m as u64))
}

impl<const H: usize, const Q: usize> Node<H, Q> {
    /// Whether the driver is to run the GPS all the time now (module
    /// documentation): a relay's runs on external power, for its grace on
    /// battery, while a far fix is followed and while it is carried, but
    /// never with a map point, which no fix may move; a sleepy node's only
    /// in the searches it asks for.
    pub fn gps_tracks(&self) -> bool {
        if self.sleepy || self.motion.map_point {
            return false;
        }
        let w = self.gps.watch_now();
        let rests = self.unplugged.is_some() && w.has(PAST_GRACE);
        self.motion.carried || w.has(FOLLOW) || !rests
    }

    /// Whether this relay rests its GPS and watches the radio for a sign
    /// that it moves: then the heartbeats of its fixed relay neighbours go
    /// to [`Node::gps_surprise`].
    pub(super) fn gps_watches(&self) -> bool {
        !self.sleepy && self.stage == Stage::Running && !self.motion.map_point && !self.gps_tracks()
    }

    /// A heartbeat of a fixed relay neighbour came `surprise_db` from that
    /// neighbour's averaged RSSI before it. At the end of a worst-case
    /// rotation, a sum of their squares beyond what fading explains starts
    /// a search (module documentation).
    pub(super) fn gps_surprise(
        &mut self,
        surprise_db: f32,
        now: Instant,
        emit: &mut dyn FnMut(Action),
    ) {
        let watches = self.gps_watches();
        let rotation = self.config.boot_listen;
        self.gps.watch(|w| {
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

    /// The node turned sleepy, or boots sleepy, or turned anchorless, or
    /// boots as a relay whose GPS rests: it searches.
    pub(super) fn gps_due(&mut self) {
        if self.sleepy {
            self.gps.searches(|s| s.due = true);
        } else {
            self.gps.watch(|w| w.set(DUE, true));
        }
    }

    /// The node took an anchor, or another: it searches, and the fix is
    /// where it chose from.
    pub(super) fn gps_anchor(&mut self) {
        self.gps.searches(|s| {
            s.due = true;
            s.at_choice = true;
        });
    }

    /// Whether the fix that came is the one asked for at a new anchor;
    /// the next is not.
    pub(super) fn gps_at_choice(&mut self) -> bool {
        if !self.sleepy {
            return false;
        }
        self.gps.searches(|s| core::mem::take(&mut s.at_choice))
    }

    /// A fix came: the search is over, and a relay follows a far one.
    pub(super) fn gps_fixed(&mut self) {
        self.gps.until = 0;
        if self.sleepy {
            self.gps.searches(|s| s.failed_at = 0);
        }
    }

    /// A fix came to a relay and was handled: a far one keeps its GPS on.
    pub(super) fn gps_after_fix(&mut self) {
        if !self.sleepy {
            let far = self.far();
            self.gps.watch(|w| w.set(FOLLOW, far));
        }
    }

    /// The node is a relay now: its GPS runs.
    pub(super) fn gps_relay(&mut self) {
        self.gps = Gps::DEFAULT;
    }

    /// The node turned sleepy: it searches, with nothing of a relay's
    /// left.
    pub(super) fn gps_sleepy(&mut self) {
        self.gps.until = 0;
        self.gps.role = Searches {
            due: true,
            ..Searches::default()
        }
        .into();
    }

    /// When a relay on battery rests its GPS: a grace after it lost
    /// external power.
    fn gps_grace_ends(&self) -> Option<Instant> {
        Some(self.unplugged? + self.config.grace)
    }

    /// When a search may start again after one that found nothing.
    fn gps_allowed_at(&self) -> Instant {
        at(self.gps.searches_now().failed_at).map_or(Instant::ZERO, |t| t + BACKOFF)
    }

    /// When an anchorless node's next search is due: once a delivery TTL,
    /// so that its address, its own position, is never older than what
    /// the network keeps for it (spec "Sleepy nodes alone").
    fn gps_anchorless_at(&self) -> Option<Instant> {
        let anchorless = self.sleepy && self.anchor.is_none() && self.stage == Stage::Running;
        if !anchorless || self.gps.until > 0 {
            return None;
        }
        let tried = self.gps.searches_now().tried_at;
        let next = at(tried).map_or(Instant::ZERO, |t| t + self.config.delivery_ttl);
        Some(next.max(self.gps_allowed_at()))
    }

    /// Start a search that is due, and end one at its limit; note a
    /// relay's grace on battery passing.
    pub(super) fn gps_timers(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        if !self.sleepy {
            let past = self.gps_grace_ends().is_some_and(|t| now >= t);
            self.gps.watch(|w| w.set(PAST_GRACE, past));
        }
        if at(self.gps.until).is_some_and(|t| now >= t) {
            self.gps.until = 0;
            if self.sleepy {
                self.gps.searches(|s| s.failed_at = mark(now));
            }
        }
        if self.gps_anchorless_at().is_some_and(|t| now >= t) {
            self.gps.searches(|s| s.due = true);
        }
        // A search due while it cannot start is dropped.
        let due = if self.sleepy {
            let held = now < self.gps_allowed_at();
            self.gps.searches(|s| core::mem::take(&mut s.due)) && !held
        } else {
            let rests = !self.gps_tracks() && !self.motion.map_point;
            self.gps.watch(|w| w.take(DUE)) && rests
        };
        if !due || self.gps.until > 0 {
            return;
        }
        self.gps.until = mark(now + SEARCH_LIMIT);
        if self.sleepy {
            self.gps.searches(|s| s.tried_at = mark(now));
        }
        emit(Action::Locate(at(self.gps.until).expect("just set")));
    }

    /// When the core next needs a wakeup for its searches, and for a
    /// relay the end of its grace on battery.
    pub(super) fn gps_wakeup(&self) -> Option<Instant> {
        let other = if self.sleepy {
            self.gps_anchorless_at()
        } else {
            self.gps_grace_ends()
                .filter(|_| !self.gps.watch_now().has(PAST_GRACE))
        };
        match (at(self.gps.until), other) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
}
