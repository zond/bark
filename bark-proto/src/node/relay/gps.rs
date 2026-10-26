//! When a sleepy node runs its GPS (`crate::gps`, spec "Beats", roadmap
//! D80).
//!
//! A relay's GPS runs all the time, on external power. A sleepy node asks
//! its driver for a search ([`Action::Locate`]) only when the radio says
//! it may have moved: when it turns sleepy or boots sleepy, when it takes
//! an anchor or changes it, when it turns anchorless, and once a delivery
//! TTL while it stays anchorless. A search ends at the first fix or after
//! [`SEARCH_LIMIT`]; one that ends at its limit found nothing, and none
//! follows for [`BACKOFF`].
//!
//! A fix asked for at a new anchor is where the node chose from: it is
//! taken after the choice, and measuring a move from the position before
//! it would check the anchor just chosen.
//!
//! A research switch, [`Config::relay_gps`], lets a relay rest its GPS in
//! standby (`docs/relay-gps-2026-10-26.md`). Resting, it searches at boot,
//! and with [`RelayGps::Radio`] when the radio says it may have moved: the
//! heartbeats of its fixed relay neighbours, over a worst-case rotation,
//! differ from their averaged RSSI more than fading explains. A search that
//! finds a far fix keeps the GPS on until the node is carried or a fix
//! comes near again; a relay's search has no back-off.
//!
//! [`Config::relay_gps`]: crate::Config::relay_gps

use super::{Node, Stage};
use crate::event::Action;
use crate::gps::{RelayGps, BACKOFF, SEARCH_LIMIT};
use crate::radio::FADE_DEPTH_DB;
use crate::time::{Duration, Instant};

/// How far beyond its mean, in its own standard deviations, the sum of a
/// rotation's squared surprises must lie before a resting relay searches:
/// two, which a still node's passes in 2.3 % of rotations, the convention
/// of the switching margin (spec "Relay choice").
const SURPRISE_SIGMAS: f32 = 2.0;

/// The variance of a heartbeat's surprise, the difference between its RSSI
/// and the averaged RSSI before it, under fading of [`FADE_DEPTH_DB`]: the
/// frame's σ² and the average's σ²/3, since each frame counts half in it
/// and a neighbour's heartbeats reach the node's channel a rotation apart,
/// longer than the fading lasts.
const SURPRISE_VAR_DB2: f32 = FADE_DEPTH_DB * FADE_DEPTH_DB * 4.0 / 3.0;

/// The point [`SURPRISE_SIGMAS`] above the mean of a chi-square of `k`
/// degrees of freedom, by the Wilson–Hilferty approximation.
fn chi_square_point(k: u8) -> f32 {
    let k = k as f32;
    let a = 2.0 / (9.0 * k);
    k * libm::powf(1.0 - a + SURPRISE_SIGMAS * libm::sqrtf(a), 3.0)
}

/// A sleepy node's searches. Its times are whole seconds from the
/// clock's zero, 0 for none, so that the node stays within the size
/// `docs/core-interface.md` §6 gives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Gps {
    /// Until when the search under way runs.
    until: u32,
    /// A sleepy node's: when its last search found nothing, until a fix
    /// comes. A relay's that watches its radio: the sum of the squared
    /// surprises of the rotation under way over [`SURPRISE_VAR_DB2`], in
    /// thousandths. Each role's is cleared when the node takes the other.
    failed_at: u32,
    /// A sleepy node's: when its last search started. A relay's that
    /// watches its radio: when the rotation of surprises under way began,
    /// 0 for none.
    tried_at: u32,
    /// A search is to start.
    due: bool,
    /// The next fix is the one asked for at a new anchor.
    pub(super) at_choice: bool,
    /// A relay's: [`FOLLOW`] and [`PAST_GRACE`].
    flags: u8,
    /// The surprises of the rotation under way.
    surprises: u8,
}

impl Gps {
    /// No search under way, none due.
    pub(super) const DEFAULT: Gps = Gps {
        until: 0,
        failed_at: 0,
        tried_at: 0,
        due: false,
        at_choice: false,
        flags: 0,
        surprises: 0,
    };
}

/// A resting relay's last fix was far: its GPS runs until it is carried
/// or a fix is near.
const FOLLOW: u8 = 1;

/// A relay with [`RelayGps::Grace`] has been on battery longer than its
/// grace: its GPS rests.
const PAST_GRACE: u8 = 2;

/// `t` in whole seconds, rounded up, and at least 1.
fn mark(t: Instant) -> u32 {
    (t.as_micros().div_ceil(1_000_000) as u32).max(1)
}

/// The instant of a mark, `None` for none.
fn at(m: u32) -> Option<Instant> {
    (m > 0).then(|| Instant::ZERO + Duration::from_secs(m as u64))
}

impl<const H: usize, const Q: usize> Node<H, Q> {
    /// Whether the driver is to run the GPS all the time now: a relay's
    /// runs all the time, a sleepy node's only in the searches it asks
    /// for (spec "Beats"). With [`Config::relay_gps`] a relay's may rest
    /// (module documentation), and one with a map point is handed no fix
    /// and rests.
    ///
    /// [`Config::relay_gps`]: crate::Config::relay_gps
    pub fn gps_tracks(&self) -> bool {
        if self.sleepy {
            return false;
        }
        let rests = match self.config.relay_gps {
            RelayGps::Always => return true,
            RelayGps::Battery | RelayGps::Radio => self.unplugged.is_some(),
            RelayGps::Grace => self.unplugged.is_some() && self.gps.flags & PAST_GRACE != 0,
            RelayGps::BatteryEverywhere | RelayGps::RadioEverywhere => true,
        };
        !self.motion.map_point && (self.motion.carried || (self.gps.flags & FOLLOW != 0) || !rests)
    }

    /// Whether this relay rests its GPS and watches the radio for a sign
    /// that it moves: then the heartbeats of its fixed relay neighbours go
    /// to [`Node::gps_surprise`].
    pub(super) fn gps_watches(&self) -> bool {
        matches!(
            self.config.relay_gps,
            RelayGps::Radio | RelayGps::RadioEverywhere | RelayGps::Grace
        ) && !self.sleepy
            && self.stage == Stage::Running
            && !self.motion.map_point
            && !self.gps_tracks()
    }

    /// A heartbeat of a fixed relay neighbour came `surprise_db` from its
    /// averaged RSSI before it. At the end of a worst-case rotation, a sum
    /// of their squares beyond what fading explains starts a search
    /// (module documentation).
    pub(super) fn gps_surprise(
        &mut self,
        surprise_db: f32,
        now: Instant,
        emit: &mut dyn FnMut(Action),
    ) {
        if !self.gps_watches() {
            self.gps.tried_at = 0;
            return;
        }
        let g = &mut self.gps;
        if at(g.tried_at).is_some_and(|t| now >= t + self.config.boot_listen) {
            let moved =
                g.surprises > 0 && g.failed_at as f32 / 1000.0 > chi_square_point(g.surprises);
            g.due |= moved;
            emit(Action::Trace(crate::event::Trace::MoveSign {
                surprises: g.surprises,
                sum_milli: g.failed_at,
                moved,
            }));
            g.tried_at = 0;
        }
        if g.tried_at == 0 {
            g.tried_at = mark(now);
            g.surprises = 0;
            g.failed_at = 0;
        }
        if g.surprises == u8::MAX {
            return;
        }
        g.surprises += 1;
        let z2 = surprise_db * surprise_db / SURPRISE_VAR_DB2;
        g.failed_at = g.failed_at.saturating_add((z2 * 1000.0) as u32);
    }

    /// A fix came to a relay: a far one keeps its resting GPS on.
    pub(super) fn gps_after_fix(&mut self) {
        let follow = !self.sleepy && self.far();
        self.gps.flags = (self.gps.flags & !FOLLOW) | if follow { FOLLOW } else { 0 };
    }

    /// The node turned sleepy, or boots sleepy, or turned anchorless: it
    /// searches.
    pub(super) fn gps_due(&mut self) {
        self.gps.due = true;
    }

    /// The node took an anchor, or another: it searches, and the fix is
    /// where it chose from.
    pub(super) fn gps_anchor(&mut self) {
        self.gps.due = true;
        self.gps.at_choice = true;
    }

    /// A fix came: the search is over.
    pub(super) fn gps_fixed(&mut self) {
        self.gps.until = 0;
        if self.sleepy {
            self.gps.failed_at = 0;
        }
    }

    /// The node is a relay now: its GPS runs all the time.
    pub(super) fn gps_relay(&mut self) {
        self.gps = Gps::DEFAULT;
    }

    /// With [`RelayGps::Grace`], when a relay on battery rests its GPS:
    /// a grace after it lost external power.
    fn gps_grace_ends(&self) -> Option<Instant> {
        let t = self.unplugged.filter(|_| self.config.relay_gps == RelayGps::Grace)?;
        Some(t + self.config.grace)
    }

    /// The node turned sleepy: it searches, with nothing of a relay's
    /// left.
    pub(super) fn gps_sleepy(&mut self) {
        self.gps = Gps::DEFAULT;
        self.gps.due = true;
    }

    /// When a search may start again after one that found nothing.
    fn gps_allowed_at(&self) -> Instant {
        at(self.gps.failed_at).map_or(Instant::ZERO, |t| t + BACKOFF)
    }

    /// When an anchorless node's next search is due: once a delivery TTL,
    /// so that its address, its own position, is never older than what
    /// the network keeps for it (spec "Sleepy nodes alone").
    fn gps_anchorless_at(&self) -> Option<Instant> {
        let anchorless = self.sleepy && self.anchor.is_none() && self.stage == Stage::Running;
        if !anchorless || self.gps.until > 0 {
            return None;
        }
        let next = at(self.gps.tried_at).map_or(Instant::ZERO, |t| t + self.config.delivery_ttl);
        Some(next.max(self.gps_allowed_at()))
    }

    /// Start a search that is due, and end one at its limit. A relay's
    /// search, with its GPS resting, has no back-off.
    pub(super) fn gps_timers(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        if !self.sleepy && self.config.relay_gps == RelayGps::Always {
            return;
        }
        if !self.sleepy {
            let past = self.gps_grace_ends().is_some_and(|t| now >= t);
            self.gps.flags = (self.gps.flags & !PAST_GRACE) | if past { PAST_GRACE } else { 0 };
        }
        if at(self.gps.until).is_some_and(|t| now >= t) {
            self.gps.until = 0;
            if self.sleepy {
                self.gps.failed_at = mark(now);
            }
        }
        if self.gps_anchorless_at().is_some_and(|t| now >= t) {
            self.gps.due = true;
        }
        let held = self.sleepy && now < self.gps_allowed_at();
        if !core::mem::take(&mut self.gps.due) || self.gps.until > 0 || held {
            return;
        }
        if !self.sleepy && (self.gps_tracks() || self.motion.map_point) {
            return;
        }
        self.gps.until = mark(now + SEARCH_LIMIT);
        if self.sleepy {
            self.gps.tried_at = mark(now);
        }
        emit(Action::Locate(at(self.gps.until).expect("just set")));
    }

    /// When the core next needs a wakeup for its searches.
    pub(super) fn gps_wakeup(&self) -> Option<Instant> {
        if !self.sleepy {
            let until = at(self.gps.until).filter(|_| self.config.relay_gps != RelayGps::Always);
            let grace = self.gps_grace_ends().filter(|_| self.gps.flags & PAST_GRACE == 0);
            return match (until, grace) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
        }
        match (at(self.gps.until), self.gps_anchorless_at()) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
}
