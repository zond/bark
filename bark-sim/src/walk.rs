//! Walking nodes (docs/simulator-design.md §2, "Movement").
//!
//! A share of the sleepy nodes walk: each stops indoors at a point for a
//! while, then walks outdoors in a straight line to another, and so on.
//! Their links cannot be stored, since they change as the node moves, so
//! they are worked out when a frame needs them, from the same path loss as
//! the stored ones, a shadowing that varies over the walker's position and
//! a fading that varies over time, faster while it walks.
//!
//! Every link with a walker at either end is worked out here; the stored
//! table holds none.

use crate::geo::{Grid, Xy};
use crate::links::{LinkParams, Site, SHADOW_TRUNCATE_SIGMA};
use crate::rng::{mix, normal_fast_at, stream_seed, Purpose, Stream};
use crate::scenario::Movement;
use bark_proto::radio::{Channel, CHANNELS};
use bark_proto::{Duration, Instant};

const C_M_PER_S: f64 = 299_792_458.0;

/// A receiver's share of a frame from a walker or to one: its node, the
/// power before fading and with it, dBm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MovingLink {
    pub rx: u32,
    pub base_dbm: f32,
    pub dbm: f64,
}

/// One walking node.
#[derive(Clone, Debug)]
pub(crate) struct Walker {
    pub node: u32,
    from: Xy,
    to: Xy,
    since: Instant,
    pub until: Instant,
    /// Walking (outdoors) or stopped (indoors).
    pub walking: bool,
    /// Counts the walks and stops: a new stop is a new place with new
    /// multipath.
    pub leg: u32,
    rng: Stream,
}

impl Walker {
    /// Where it is at `t`.
    pub fn at(&self, t: Instant) -> Xy {
        if !self.walking || self.until <= self.since {
            return self.from;
        }
        let f = t.since(self.since).as_micros() as f64
            / self.until.since(self.since).as_micros() as f64;
        let f = f.clamp(0.0, 1.0);
        Xy {
            x: self.from.x + (self.to.x - self.from.x) * f,
            y: self.from.y + (self.to.y - self.from.y) * f,
        }
    }

    /// Where its antenna is: indoors at a stop, outdoors walking.
    pub fn site(&self) -> Site {
        if self.walking {
            Site::Outdoor
        } else {
            Site::Indoor
        }
    }
}

/// Static nodes of one site, gridded for walkers to find.
struct Class {
    grid: Option<Grid>,
    nodes: Vec<u32>,
}

/// The walkers, and what finding their links needs.
pub(crate) struct Walks {
    pub walkers: Vec<Walker>,
    /// Per node, its walker, or `u32::MAX`.
    of: Vec<u32>,
    p: Movement,
    links: LinkParams,
    tx_dbm: f64,
    floor_dbm: f64,
    /// Corners of the area walks stay in.
    lo: Xy,
    hi: Xy,
    /// Correlation time of the fading at a stop and while walking.
    tau_still: Duration,
    tau_walking: Duration,
    shadow_seed: u64,
    fade_seed: u64,
    /// Static nodes by site, gridded at the longest a walker reaches them.
    classes: [Class; 3],
    /// Walkers stopped and walking, gridded at the longest they reach
    /// anything, and when.
    moving: [(Option<Grid>, Vec<u32>); 2],
    built: Option<Instant>,
}

/// How often the walkers' grids are built again: their side allows for
/// as far as a walker goes in that time.
const REGRID: Duration = Duration::from_secs(10);

impl Walks {
    /// Choose the walkers among `sleepy` nodes and start each at a stop.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        p: &Movement,
        links: &LinkParams,
        tx_dbm: f64,
        floor_dbm: f64,
        xy: &[Xy],
        site: &[Site],
        sleepy: std::ops::Range<u32>,
        seed: u64,
    ) -> Walks {
        let n = xy.len();
        let mut of = vec![u32::MAX; n];
        let mut walkers = Vec::new();
        for i in sleepy {
            let mut rng = Stream::new(seed, Purpose::Walk, i as u64);
            if !rng.chance(p.fraction) {
                continue;
            }
            // In the stationary state: the time left at the first stop is
            // as long on average as a whole stop.
            let stop = Duration::from_micros((rng.exponential(p.stop_s) * 1e6) as u64);
            of[i as usize] = walkers.len() as u32;
            walkers.push(Walker {
                node: i,
                from: xy[i as usize],
                to: xy[i as usize],
                since: Instant::ZERO,
                until: Instant::ZERO + stop,
                walking: false,
                leg: 0,
                rng,
            });
        }
        let (mut lo, mut hi) = (xy[0], xy[0]);
        for q in xy {
            lo = Xy {
                x: lo.x.min(q.x),
                y: lo.y.min(q.y),
            };
            hi = Xy {
                x: hi.x.max(q.x),
                y: hi.y.max(q.y),
            };
        }
        let centre = Channel::new(CHANNELS / 2).expect("centre channel").hz() as f64;
        let wavelength = C_M_PER_S / centre;
        // Coherence time of fading for a receiver moving at v: 0.423 / f_D
        // with the Doppler spread f_D = v / λ.
        let tau_walking = 0.423 * wavelength / p.speed_m_s;
        let secs = |s: f64| Duration::from_micros((s * 1e6).round().max(1.0) as u64);
        let reach = |a: Site, b: Site| links.cutoff_distance_m(tx_dbm, floor_dbm, a, b);
        let classes = [Site::Outdoor, Site::Indoor, Site::Rooftop].map(|c| {
            let nodes: Vec<u32> = (0..n as u32)
                .filter(|&i| of[i as usize] == u32::MAX && site[i as usize] == c)
                .collect();
            let pts: Vec<Xy> = nodes.iter().map(|&i| xy[i as usize]).collect();
            let side = reach(Site::Outdoor, c).max(reach(Site::Indoor, c));
            Class {
                grid: (!pts.is_empty()).then(|| Grid::new(&pts, side)),
                nodes,
            }
        });
        Walks {
            walkers,
            of,
            p: p.clone(),
            links: links.clone(),
            tx_dbm,
            floor_dbm,
            lo,
            hi,
            tau_still: secs(links.fading.correlation_time_s),
            tau_walking: secs(tau_walking),
            shadow_seed: stream_seed(seed, Purpose::Shadowing, 1),
            fade_seed: stream_seed(seed, Purpose::Fading, 1),
            classes,
            moving: [(None, Vec::new()), (None, Vec::new())],
            built: None,
        }
    }

    /// Whether `node` walks.
    pub fn walks(&self, node: u32) -> bool {
        self.of.get(node as usize).is_some_and(|&w| w != u32::MAX)
    }

    /// The walker of `node`, if it walks.
    pub fn walker(&self, node: u32) -> Option<&Walker> {
        let w = *self.of.get(node as usize)?;
        self.walkers.get(w as usize)
    }

    /// Walker `w` ends its stop or its walk at `now`: it starts the other.
    /// Returns when that ends.
    pub fn next_leg(&mut self, w: u32, now: Instant) -> Instant {
        let (speed, trip, stop) = (self.p.speed_m_s, self.p.trip_m, self.p.stop_s);
        let (lo, hi) = (self.lo, self.hi);
        let k = &mut self.walkers[w as usize];
        let here = k.at(now);
        k.leg += 1;
        if k.walking {
            k.walking = false;
            k.from = here;
            k.to = here;
            k.since = now;
            k.until = now + Duration::from_micros((k.rng.exponential(stop) * 1e6) as u64);
        } else {
            // A straight walk in a random direction, as long as an
            // exponential draw, folded back into the area at its edges.
            let d = k.rng.exponential(trip);
            let a = k.rng.uniform() * core::f64::consts::TAU;
            let fold = |v: f64, lo: f64, hi: f64| {
                let span = hi - lo;
                if span <= 0.0 {
                    return lo;
                }
                let m = (v - lo).rem_euclid(2.0 * span);
                lo + if m > span { 2.0 * span - m } else { m }
            };
            let to = Xy {
                x: fold(here.x + d * a.cos(), lo.x, hi.x),
                y: fold(here.y + d * a.sin(), lo.y, hi.y),
            };
            let len = here.dist(to);
            k.walking = true;
            k.from = here;
            k.to = to;
            k.since = now;
            k.until = now + Duration::from_micros((len / speed * 1e6).round().max(1.0) as u64);
        }
        self.built = None;
        k.until
    }

    /// Grid the walkers again if they have moved on or changed what they do.
    fn regrid(&mut self, now: Instant) {
        if self.built.is_some_and(|b| now.since(b) < REGRID) {
            return;
        }
        let slack = self.p.speed_m_s * REGRID.as_micros() as f64 / 1e6;
        let reach = |a: Site| {
            [Site::Outdoor, Site::Indoor, Site::Rooftop]
                .map(|b| {
                    self.links
                        .cutoff_distance_m(self.tx_dbm, self.floor_dbm, a, b)
                })
                .into_iter()
                .fold(0.0, f64::max)
        };
        for (k, site) in [(0, Site::Indoor), (1, Site::Outdoor)] {
            let ws: Vec<u32> = (0..self.walkers.len() as u32)
                .filter(|&w| self.walkers[w as usize].site() == site)
                .collect();
            let pts: Vec<Xy> = ws
                .iter()
                .map(|&w| self.walkers[w as usize].at(now))
                .collect();
            let grid = (!pts.is_empty()).then(|| Grid::new(&pts, reach(site) + slack));
            self.moving[k] = (grid, ws);
        }
        self.built = Some(now);
    }

    /// Everyone that hears a frame `src` sends at `now` on `channel`, at
    /// or above the cut-off, over a link with a walker at either end. `xy`
    /// and `site` are where the nodes that do not walk are.
    pub fn links_from(
        &mut self,
        src: u32,
        now: Instant,
        channel: Channel,
        xy: &[Xy],
        site: &[Site],
    ) -> Vec<MovingLink> {
        self.regrid(now);
        let (from, from_site) = match self.walker(src) {
            Some(w) => (w.at(now), w.site()),
            None => (xy[src as usize], site[src as usize]),
        };
        let mut out = Vec::new();
        let mut consider = |s: &Self, rx: u32, at: Xy, rx_site: Site| {
            if rx == src {
                return;
            }
            let d = from.dist(at);
            if d > s
                .links
                .cutoff_distance_m(s.tx_dbm, s.floor_dbm, from_site, rx_site)
            {
                return;
            }
            let mover = match (s.walks(src), s.walks(rx)) {
                (true, true) => Xy {
                    x: (from.x + at.x) / 2.0,
                    y: (from.y + at.y) / 2.0,
                },
                (true, false) => from,
                _ => at,
            };
            let loss = s.links.path_loss_db(d) + s.links.site_loss_db(from_site, rx_site)
                - s.shadow_db(src, rx, mover);
            let base = s.tx_dbm - loss;
            if base < s.floor_dbm {
                return;
            }
            let dbm = base + s.fade_db(src, rx, channel, now);
            out.push(MovingLink {
                rx,
                base_dbm: base as f32,
                dbm,
            });
        };
        // Walkers in reach of anyone.
        for (grid, ws) in &self.moving {
            if let Some(g) = grid {
                g.neighbourhood(from, |k| {
                    let w = &self.walkers[ws[k as usize] as usize];
                    consider(self, w.node, w.at(now), w.site());
                });
            }
        }
        // Nodes that stay, in reach of a walker.
        if self.walks(src) {
            for class in &self.classes {
                if let Some(g) = &class.grid {
                    g.neighbourhood(from, |k| {
                        let n = class.nodes[k as usize];
                        consider(self, n, xy[n as usize], site[n as usize]);
                    });
                }
            }
        }
        out.sort_unstable_by_key(|l| l.rx);
        out
    }

    /// Shadowing of the link `a`–`b` with its moving end at `p`, dB: a
    /// normal of the scenario's deviation that varies smoothly over `p`,
    /// independent points the decorrelation distance apart, cut at
    /// [`SHADOW_TRUNCATE_SIGMA`] as the stored links' is.
    fn shadow_db(&self, a: u32, b: u32, p: Xy) -> f64 {
        let sigma = self.links.shadowing_sigma_db;
        if sigma <= 0.0 {
            return 0.0;
        }
        let side = self.p.shadow_decorrelation_m;
        let (gx, gy) = (p.x / side, p.y / side);
        let (ix, iy) = (gx.floor(), gy.floor());
        let (fx, fy) = (gx - ix, gy - iy);
        let pair = pair_key(a, b);
        let corner = |dx: f64, dy: f64| {
            let cx = (ix + dx) as i64 as u64;
            let cy = (iy + dy) as i64 as u64;
            normal_fast_at(self.shadow_seed, pair, mix(cx) ^ cy)
        };
        let w = [
            (1.0 - fx) * (1.0 - fy),
            fx * (1.0 - fy),
            (1.0 - fx) * fy,
            fx * fy,
        ];
        let n = [
            corner(0.0, 0.0),
            corner(1.0, 0.0),
            corner(0.0, 1.0),
            corner(1.0, 1.0),
        ];
        let norm = w.iter().map(|w| w * w).sum::<f64>().sqrt();
        let s: f64 = w.iter().zip(n).map(|(w, n)| w * n).sum::<f64>() / norm;
        sigma * s.clamp(-SHADOW_TRUNCATE_SIGMA, SHADOW_TRUNCATE_SIGMA)
    }

    /// Fading of the link `a`–`b` on `channel` at `t`, dB: a normal of the
    /// scenario's deviation that varies smoothly over time, independent
    /// points a correlation time apart: the scenario's while both ends
    /// stay put, the Doppler coherence time while either walks. A new stop
    /// or walk starts a new draw.
    fn fade_db(&self, a: u32, b: u32, channel: Channel, t: Instant) -> f64 {
        let sigma = self.links.fading.sigma_db;
        if sigma <= 0.0 {
            return 0.0;
        }
        let (mut walking, mut legs) = (false, 0u64);
        for n in [a, b] {
            if let Some(w) = self.walker(n) {
                walking |= w.walking;
                legs = mix(legs ^ (w.leg as u64) ^ ((n as u64) << 32));
            }
        }
        let tau = if walking {
            self.tau_walking
        } else {
            self.tau_still
        }
        .as_micros();
        let k = t.as_micros() / tau;
        let u = (t.as_micros() % tau) as f64 / tau as f64;
        let pair = pair_key(a, b) ^ legs;
        let at = |k: u64| normal_fast_at(self.fade_seed, pair, (k << 4) | channel.index() as u64);
        let x = (1.0 - u) * at(k) + u * at(k + 1);
        sigma * x / ((1.0 - u) * (1.0 - u) + u * u).sqrt()
    }
}

fn pair_key(a: u32, b: u32) -> u64 {
    let (lo, hi) = if a < b { (a, b) } else { (b, a) };
    ((lo as u64) << 32) | hi as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> LinkParams {
        LinkParams {
            exponent: 3.6,
            indoor_loss_db: 14.4,
            shadowing_sigma_db: 6.0,
            fading: crate::links::FadingParams {
                sigma_db: 4.0,
                correlation_time_s: 60.0,
                step_s: 10.0,
            },
            ..LinkParams::default()
        }
    }

    /// A relay at the origin and 400 sleepy nodes over a square kilometre,
    /// every one of them walking.
    fn town() -> (Walks, Vec<Xy>, Vec<Site>) {
        let mut xy = vec![Xy { x: 500.0, y: 500.0 }];
        let mut rng = Stream::new(1, Purpose::Placement, 0);
        for _ in 0..400 {
            xy.push(Xy {
                x: rng.uniform() * 1000.0,
                y: rng.uniform() * 1000.0,
            });
        }
        let mut site = vec![Site::Indoor; xy.len()];
        site[0] = Site::Outdoor;
        let m = Movement {
            fraction: 1.0,
            ..Movement::default()
        };
        let w = Walks::new(
            &m,
            &params(),
            10.0,
            -129.0,
            &xy,
            &site,
            1..xy.len() as u32,
            7,
        );
        (w, xy, site)
    }

    /// Walkers stop, then walk at their speed in a straight line inside
    /// the area, then stop again, indoors and outdoors in turn.
    #[test]
    fn walkers_walk_at_their_speed_inside_the_area() {
        let (mut w, _, _) = town();
        assert_eq!(w.walkers.len(), 400);
        let mut walked = 0;
        for k in 0..w.walkers.len() as u32 {
            let mut now = w.walkers[k as usize].until;
            for _ in 0..6 {
                let was = w.walkers[k as usize].walking;
                let until = w.next_leg(k, now);
                let walker = &w.walkers[k as usize];
                assert_ne!(walker.walking, was);
                assert_eq!(
                    walker.site(),
                    if walker.walking {
                        Site::Outdoor
                    } else {
                        Site::Indoor
                    }
                );
                if walker.walking {
                    let (a, b) = (walker.at(now), walker.at(until));
                    let secs = until.since(now).as_micros() as f64 / 1e6;
                    assert!((a.dist(b) / secs - 1.4).abs() < 1e-3);
                    let mid = walker.at(now + until.since(now).div(2));
                    assert!((mid.dist(a) - a.dist(b) / 2.0).abs() < 1e-3);
                    walked += 1;
                }
                for p in [walker.at(now), walker.at(until)] {
                    assert!((0.0..=1000.0).contains(&p.x) && (0.0..=1000.0).contains(&p.y));
                }
                now = until;
            }
        }
        assert_eq!(walked, 3 * 400);
    }

    /// The links a frame finds are those the stored table would hold:
    /// path loss and walls, shadowing of the scenario's deviation over
    /// places, fading of its deviation over time, both ways.
    #[test]
    fn links_follow_the_path_loss_with_shadowing_and_fading() {
        let (mut w, xy, site) = town();
        let ch = Channel::new(4).unwrap();
        let p = params();
        let at = Instant::from_micros(1_000_000);
        let from_relay = w.links_from(0, at, ch, &xy, &site);
        assert!(!from_relay.is_empty());
        let mut shadow = Vec::new();
        for l in &from_relay {
            let k = w.walker(l.rx).unwrap();
            let d = xy[0].dist(k.at(at));
            let mean = 10.0 - p.path_loss_db(d) - p.site_loss_db(Site::Outdoor, k.site());
            shadow.push(l.base_dbm as f64 - mean);
            assert!(l.base_dbm as f64 >= -129.0);
        }
        // Nodes near the cut-off are kept only when shadowing lifts them,
        // so look at the near ones.
        let near: Vec<f64> = from_relay
            .iter()
            .zip(&shadow)
            .filter(|(l, _)| xy[0].dist(w.walker(l.rx).unwrap().at(at)) < 250.0)
            .map(|(_, s)| *s)
            .collect();
        let sd = (near.iter().map(|s| s * s).sum::<f64>() / near.len() as f64).sqrt();
        assert!(
            (sd - 6.0).abs() < 1.0,
            "shadowing deviation {sd} dB over {}",
            near.len()
        );
        // Reciprocal before fading: what a walker sends the relay hears as
        // the relay's frame reaches the walker.
        let l = from_relay[0];
        let back = w.links_from(l.rx, at, ch, &xy, &site);
        let to_relay = back.iter().find(|b| b.rx == 0).expect("relay hears it");
        assert_eq!(to_relay.base_dbm, l.base_dbm);
        assert_eq!(to_relay.dbm, l.dbm);
        // Fading: the scenario's deviation, over a stop's slow time.
        let k = l.rx;
        let fades: Vec<f64> = (0..2000)
            .map(|i| w.fade_db(0, k, ch, Instant::from_micros(i * 600_000_000)))
            .collect();
        let sd = (fades.iter().map(|f| f * f).sum::<f64>() / fades.len() as f64).sqrt();
        assert!((sd - 4.0).abs() < 0.4, "fading deviation {sd} dB");
        let near_in_time = w.fade_db(0, k, ch, at + Duration::from_millis(100));
        assert!((near_in_time - w.fade_db(0, k, ch, at)).abs() < 1.0);
    }
}
