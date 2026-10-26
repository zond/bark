//! A sleepy node's GPS searches (`Action::Locate`, `bark_proto::gps`).
//!
//! A search runs from the core's `Locate` until the first fix the driver
//! hands or the end the core gave. A node under the sky gets its fix after
//! a hot start if it had one within the time the satellites' orbits serve,
//! else after a cold start; a node that hears no sky gets none and runs to
//! the end. Under the sky are the nodes placed outdoors or on a roof, the
//! walkers while they walk, and of the others indoors the share
//! `[gps] no_sky` leaves, drawn once per node. The time each search runs
//! after the settle time is counted, at the receiver's current.

use crate::links::Site;
use crate::rng::{Purpose, Stream};
use crate::sim::{Ev, Sim};
use bark_proto::gps::{COLD_START, HOT_START, ORBIT_VALID};
use bark_proto::{Duration, Instant};

/// One node's receiver.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Receiver {
    /// Since when it searches, and until when.
    on: Option<(Instant, Instant)>,
    /// Counts its searches, so that a stale event finds a newer one.
    run: u32,
    /// When it last had a fix.
    fixed_at: Option<Instant>,
    /// The search under way hears the sky: its fix is on the way.
    pending: bool,
    /// Whether it hears the sky indoors: drawn at its first search.
    sky_indoors: Option<bool>,
}

impl Sim {
    /// `node`'s core asks for a search until `until`. A search under way
    /// that has heard no sky yet tries again from where the node is now.
    pub(crate) fn locate(&mut self, node: u32, until: Instant) {
        let now = self.now;
        let sky = self.hears_sky(node);
        let settled = now >= self.settle;
        let r = &mut self.receivers[node as usize];
        match r.on {
            Some((since, _)) => r.on = Some((since, until)),
            None => {
                r.on = Some((now, until));
                r.run += 1;
                r.pending = false;
                if settled {
                    self.metrics.nodes[node as usize].gps.searches += 1;
                }
            }
        }
        let r = &mut self.receivers[node as usize];
        let run = r.run;
        if sky && !r.pending {
            r.pending = true;
            let hot = r.fixed_at.is_some_and(|t| now.since(t) < ORBIT_VALID);
            let first = if hot { HOT_START } else { COLD_START };
            self.push(now + first, Ev::GpsFix(node, run));
        }
        self.push(until, Ev::GpsEnd(node, run));
    }

    /// The fix of search `run` comes, if it still runs and has not ended.
    pub(crate) fn gps_fix(&mut self, node: u32, run: u32) {
        let r = self.receivers[node as usize];
        let Some((_, until)) = r.on.filter(|_| r.run == run) else {
            return;
        };
        if self.now >= until {
            return;
        }
        if self.fix_now(node) {
            self.receivers[node as usize].fixed_at = Some(self.now);
            self.gps_off(node);
            if self.now >= self.settle {
                self.metrics.nodes[node as usize].gps.fixes += 1;
            }
        } else {
            // Refused by the checks: the receiver's next, a second later.
            self.push(self.now + Duration::from_secs(1), Ev::GpsFix(node, run));
        }
    }

    /// Search `run` reaches its end, unless it was moved.
    pub(crate) fn gps_end(&mut self, node: u32, run: u32) {
        let r = self.receivers[node as usize];
        if r.run == run && r.on.is_some_and(|(_, until)| self.now >= until) {
            self.gps_off(node);
        }
    }

    /// The search ends now: its time after the settle time is counted.
    pub(crate) fn gps_off(&mut self, node: u32) {
        let Some((since, _)) = self.receivers[node as usize].on.take() else {
            return;
        };
        let us = self.now.since(since.max(self.settle)).as_micros();
        if self.now > self.settle {
            self.metrics.nodes[node as usize].gps.on_us += us;
        }
    }

    /// The time `node`'s receiver ran after the settle time, a search
    /// still under way counted to now.
    pub(crate) fn gps_on_us(&self, node: u32) -> u64 {
        let done = self.metrics.nodes[node as usize].gps.on_us;
        let open = self.receivers[node as usize].on.map_or(0, |(since, _)| {
            self.now.since(since.max(self.settle)).as_micros()
        });
        done + open
    }

    /// Whether `node`'s receiver hears the sky where it is now.
    fn hears_sky(&mut self, node: u32) -> bool {
        let site = match self.walks.as_ref().and_then(|w| w.walker(node)) {
            Some(k) => k.site(),
            None => self.sites[node as usize],
        };
        if site != Site::Indoor {
            return true;
        }
        let (seed, no_sky) = (self.seed, self.gps.no_sky);
        *self.receivers[node as usize]
            .sky_indoors
            .get_or_insert_with(|| !Stream::new(seed, Purpose::GpsSky, node as u64).chance(no_sky))
    }
}
