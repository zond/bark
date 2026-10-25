//! When a sleepy node runs its GPS (`crate::gps`, spec "Beats",
//! `docs/gps-schedule-2026-10-25.md`).
//!
//! A relay's GPS runs all the time, on external power. A sleepy node asks
//! its driver for a search ([`Action::Locate`]) at the times the schedule
//! gives, and the search ends at the first fix or at its limit; one that
//! ends at its limit found nothing, and with any schedule but
//! [`GpsSchedule::EveryBeat`] none follows for [`Config::gps_backoff`].
//!
//! A fix asked for at a new anchor is where the node chose from: it is
//! taken after the choice, and measuring a move from the position before
//! it would check the anchor just chosen.
//!
//! [`Config::gps_backoff`]: crate::Config::gps_backoff

use super::{Node, Stage};
use crate::event::Action;
use crate::gps::{GpsSchedule, BEAT_SEARCH_LIMIT, SEARCH_LIMIT};
use crate::node::OWNER_RADIUS_M;
use crate::time::{Duration, Instant};

/// A sleepy node's searches. Its times are whole seconds from the
/// clock's zero, 0 for none, so that the node stays within the size
/// `docs/core-interface.md` §6 gives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Gps {
    /// Until when the search under way runs.
    until: u32,
    /// When the last search found nothing, until a fix comes.
    failed_at: u32,
    /// When the last search started.
    tried_at: u32,
    /// A search is to start.
    due: bool,
    /// The next fix is the one asked for at a new anchor.
    pub(super) at_choice: bool,
    /// Its last fix was far from the one before (`GpsSchedule::Follow`).
    moving: bool,
}

impl Gps {
    /// No search under way, none due.
    pub(super) const DEFAULT: Gps = Gps {
        until: 0,
        failed_at: 0,
        tried_at: 0,
        due: false,
        at_choice: false,
        moving: false,
    };
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
    fn schedule(&self) -> GpsSchedule {
        self.config.gps
    }

    /// The node beats: with [`GpsSchedule::EveryBeat`], it searches.
    pub(super) fn gps_beat(&mut self) {
        match self.schedule() {
            GpsSchedule::EveryBeat => self.gps.due = true,
            GpsSchedule::Follow => self.gps.due |= self.gps.moving,
            GpsSchedule::Events | GpsSchedule::Anchorless => {}
        }
    }

    /// The node turned sleepy, or boots sleepy.
    pub(super) fn gps_sleepy(&mut self) {
        if matches!(self.schedule(), GpsSchedule::Events | GpsSchedule::Follow) {
            self.gps.due = true;
        }
    }

    /// The node took an anchor, or another.
    pub(super) fn gps_anchor(&mut self) {
        if matches!(self.schedule(), GpsSchedule::Events | GpsSchedule::Follow) {
            self.gps.due = true;
            self.gps.at_choice = true;
        }
    }

    /// The node turned anchorless.
    pub(super) fn gps_anchorless(&mut self) {
        if self.schedule() != GpsSchedule::EveryBeat {
            self.gps.due = true;
        }
    }

    /// A fix came, `moved_m` from the position before: the search is
    /// over.
    pub(super) fn gps_fixed(&mut self, moved_m: f32) {
        self.gps.until = 0;
        self.gps.failed_at = 0;
        self.gps.moving = moved_m > OWNER_RADIUS_M;
    }

    /// The node is a relay now: its GPS runs all the time.
    pub(super) fn gps_relay(&mut self) {
        self.gps = Gps::DEFAULT;
    }

    /// When a search may start again after one that found nothing.
    fn gps_allowed_at(&self) -> Instant {
        match (self.schedule(), at(self.gps.failed_at)) {
            (GpsSchedule::EveryBeat, _) | (_, None) => Instant::ZERO,
            (_, Some(t)) => t + self.config.gps_backoff,
        }
    }

    /// When an anchorless node's next search is due: once a delivery TTL,
    /// so that its address, its own position, is never older than what
    /// the network keeps for it (spec "Sleepy nodes alone").
    fn gps_anchorless_at(&self) -> Option<Instant> {
        let anchorless = self.sleepy && self.anchor.is_none() && self.stage == Stage::Running;
        let searching = self.gps.until > 0;
        if !anchorless || searching || self.schedule() == GpsSchedule::EveryBeat {
            return None;
        }
        let next = at(self.gps.tried_at).map_or(Instant::ZERO, |t| t + self.config.delivery_ttl);
        Some(next.max(self.gps_allowed_at()))
    }

    /// Start a search that is due, and end one at its limit.
    pub(super) fn gps_timers(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        if !self.sleepy {
            return;
        }
        if at(self.gps.until).is_some_and(|t| now >= t) {
            self.gps.until = 0;
            self.gps.failed_at = mark(now);
            self.gps.moving = false;
        }
        if self.gps_anchorless_at().is_some_and(|t| now >= t) {
            self.gps.due = true;
        }
        if !core::mem::take(&mut self.gps.due) {
            return;
        }
        let limit = match self.schedule() {
            GpsSchedule::EveryBeat => BEAT_SEARCH_LIMIT,
            GpsSchedule::Events | GpsSchedule::Follow | GpsSchedule::Anchorless => {
                if self.gps.until > 0 || now < self.gps_allowed_at() {
                    return;
                }
                SEARCH_LIMIT
            }
        };
        self.gps.until = mark(now + limit);
        self.gps.tried_at = mark(now);
        emit(Action::Locate(at(self.gps.until).expect("just set")));
    }

    /// When the core next needs a wakeup for its searches.
    pub(super) fn gps_wakeup(&self) -> Option<Instant> {
        if !self.sleepy {
            return None;
        }
        match (at(self.gps.until), self.gps_anchorless_at()) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
}
