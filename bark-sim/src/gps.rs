//! A node's GPS: a sleepy node's searches (`Action::Locate`,
//! `bark_proto::gps`), and a relay's receiver, which runs while its core
//! says it tracks (`Node::gps_tracks`) and in its searches.
//!
//! A search runs from the core's `Locate` until the first fix the driver
//! hands or the end the core gave. A node under the sky gets its fix after
//! a hot start if it had one within the time the satellites' orbits serve,
//! else after a cold start; a node that hears no sky gets none and runs to
//! the end. Under the sky are the nodes placed outdoors or on a roof, the
//! walkers while they walk, and of the others indoors the share
//! `[gps] no_sky` leaves, drawn once per node. The time each search runs
//! after the settle time is counted, at the receiver's current, and a
//! relay's time on USB and on battery with the time its receiver ran.

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
    /// The search under way is a relay's.
    relay: bool,
    /// While the node is a relay: since when, whether on battery, and
    /// whether the receiver runs.
    acct: Option<(Instant, bool, bool)>,
}

impl Sim {
    /// `node`'s core asks for a search until `until`. A search under way
    /// that has heard no sky yet tries again from where the node is now.
    pub(crate) fn locate(&mut self, node: u32, until: Instant) {
        let now = self.now;
        let sky = self.hears_sky(node);
        let settled = now >= self.settle;
        let relay = !self.relay(node).sleepy();
        let r = &mut self.receivers[node as usize];
        match r.on {
            Some((since, _)) => r.on = Some((since, until)),
            None => {
                r.on = Some((now, until));
                r.run += 1;
                r.pending = false;
                r.relay = relay;
                let g = &mut self.metrics.nodes[node as usize].gps;
                match (settled, r.relay) {
                    (false, _) => {}
                    (true, false) => g.searches += 1,
                    (true, true) => g.relay_searches[0] += 1,
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
        self.gps_account(node);
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
            let relay = self.receivers[node as usize].relay;
            self.gps_off(node);
            if self.now >= self.settle {
                let g = &mut self.metrics.nodes[node as usize].gps;
                match relay {
                    false => g.fixes += 1,
                    true => g.relay_searches[1] += 1,
                }
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
        let r = &mut self.receivers[node as usize];
        let Some((since, _)) = r.on.take() else {
            return;
        };
        let us = self.now.since(since.max(self.settle)).as_micros();
        if self.now > self.settle && !r.relay {
            self.metrics.nodes[node as usize].gps.on_us += us;
        }
        self.gps_account(node);
    }

    /// Count the time `node` was a relay, and its receiver ran, up to
    /// now, if it was, and note what it is now: a relay or not, on
    /// battery or not, its receiver on or not.
    pub(crate) fn gps_account(&mut self, node: u32) {
        let i = node as usize;
        let core = self.relay(node);
        let state = (self.nodes[i].alive && !core.sleepy()).then(|| {
            let on = core.gps_tracks() || self.receivers[i].on.is_some();
            (!self.external[i], on)
        });
        let r = &mut self.receivers[i];
        if r.acct.map(|(_, b, on)| (b, on)) == state {
            return;
        }
        if let Some((since, battery, on)) = r.acct.take() {
            if self.now > self.settle {
                let us = self.now.since(since.max(self.settle)).as_micros();
                let g = &mut self.metrics.nodes[i].gps;
                g.relay_us[usize::from(battery)] += us;
                if on {
                    g.relay_on_us[usize::from(battery)] += us;
                }
            }
        }
        r.acct = state.map(|(battery, on)| (self.now, battery, on));
    }

    /// The time `node`'s receiver ran after the settle time, a search
    /// still under way counted to now.
    pub(crate) fn gps_on_us(&self, node: u32) -> u64 {
        let done = self.metrics.nodes[node as usize].gps.on_us;
        let r = &self.receivers[node as usize];
        let open = r.on.filter(|_| !r.relay).map_or(0, |(since, _)| {
            self.now.since(since.max(self.settle)).as_micros()
        });
        done + open
    }

    /// Count `node`'s time as a relay up to now (`gps_account`), as at
    /// the end of the run.
    pub(crate) fn gps_flush(&mut self, node: u32) {
        if let Some((since, battery, on)) = self.receivers[node as usize].acct.as_mut() {
            if self.now > self.settle {
                let us = self.now.since((*since).max(self.settle)).as_micros();
                let g = &mut self.metrics.nodes[node as usize].gps;
                g.relay_us[usize::from(*battery)] += us;
                if *on {
                    g.relay_on_us[usize::from(*battery)] += us;
                }
            }
            *since = self.now;
        }
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
