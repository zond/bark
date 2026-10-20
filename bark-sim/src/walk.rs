//! Walking nodes (docs/simulator-design.md §3.7, "Movement").
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
use crate::scenario::{Movement, Vehicles};
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
    /// When its current stop or walk began, and when it ends.
    pub since: Instant,
    pub until: Instant,
    /// Walking (outdoors) or stopped (indoors).
    pub walking: bool,
    /// Counts the walks and stops: a new stop is a new place with new
    /// multipath.
    pub leg: u32,
    /// How fast it moves, metres a second.
    pub speed: f64,
    /// It rides in a vehicle along a route, between these ends.
    pub route: Option<(Xy, Xy)>,
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

    /// Whether it is driven now.
    pub fn driven(&self) -> bool {
        self.walking && self.route.is_some()
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
    v: Vehicles,
    /// The wavelength at the centre of the band, metres.
    wavelength: f64,
    links: LinkParams,
    tx_dbm: f64,
    floor_dbm: f64,
    /// Corners of the area walks stay in.
    lo: Xy,
    hi: Xy,
    /// Correlation time of the fading at a stop.
    tau_still: Duration,
    shadow_seed: u64,
    fade_seed: u64,
    /// The cut-off distance by the sites at the two ends, indexed by
    /// [`Site`]; kept since every candidate link is checked against it.
    cutoff: [[f64; 3]; 3],
    /// The side of the grids' cells: half the shortest cut-off of a link
    /// with a walker, so that a search for links visits few nodes out of
    /// reach.
    cell: f64,
    /// How far anyone moves between two builds of the walkers' grids.
    slack: f64,
    /// Static nodes by site, gridded.
    classes: [Class; 3],
    /// Walkers stopped and walking, gridded where they were when the grids
    /// were built, and when.
    moving: [(Option<Grid>, Vec<u32>); 2],
    built: Option<Instant>,
    /// Per walker at a stop, its links to the nodes that stay, by node, and
    /// their power before fading, worked out at its first frame of the
    /// stop: they are the same for the whole stop.
    stays: Vec<Option<Vec<(u32, f64)>>>,
    /// The nodes below this never sleep; most stay where they are.
    fixed: u32,
    /// Per node that never sleeps and walker, the power before fading of
    /// a frame from that node, `NAN` if not yet worked out in this stop,
    /// `-∞` if out of reach: it is the same for the whole stop, those
    /// nodes send most of the frames, and an array indexed by both is
    /// quicker to read than a search of the walker's links.
    heard: Vec<f64>,
}

/// How often the walkers' grids are built again: a search of them allows
/// for as far as a walker goes in that time.
pub(crate) const REGRID: Duration = Duration::from_secs(10);

impl Walks {
    /// Choose the walkers and riders among `sleepy` nodes and the relays
    /// placed as people, `relays`, and start each at a stop.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        p: &Movement,
        v: &Vehicles,
        links: &LinkParams,
        tx_dbm: f64,
        floor_dbm: f64,
        xy: &[Xy],
        site: &[Site],
        sleepy: std::ops::Range<u32>,
        relays: &[u32],
        seed: u64,
    ) -> Walks {
        let n = xy.len();
        let fixed = sleepy.start;
        let mut of = vec![u32::MAX; n];
        let mut walkers = Vec::new();
        let walking = sleepy
            .clone()
            .map(|i| (i, p.fraction))
            .chain(relays.iter().map(|&i| (i, p.relay_fraction)));
        for (i, share) in walking {
            let mut rng = Stream::new(seed, Purpose::Walk, i as u64);
            if !rng.chance(share) {
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
                speed: p.speed_m_s,
                route: None,
                rng,
            });
        }
        let ends = (
            Xy {
                x: v.route[0],
                y: v.route[1],
            },
            Xy {
                x: v.route[2],
                y: v.route[3],
            },
        );
        let riders = sleepy
            .map(|i| (i, v.fraction))
            .chain(relays.iter().map(|&i| (i, v.relay_fraction)));
        for (i, share) in riders {
            let mut rng = Stream::new(seed, Purpose::Vehicle, i as u64);
            if of[i as usize] != u32::MAX || !rng.chance(share) {
                continue;
            }
            // In the stationary state, at a point of the route.
            let stop = Duration::from_micros((rng.exponential(v.stop_s) * 1e6) as u64);
            let at = on_route(ends, rng.uniform());
            of[i as usize] = walkers.len() as u32;
            walkers.push(Walker {
                node: i,
                from: at,
                to: at,
                since: Instant::ZERO,
                until: Instant::ZERO + stop,
                walking: false,
                leg: 0,
                speed: v.speed_m_s,
                route: Some(ends),
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
        let fastest = walkers.iter().map(|w| w.speed).fold(0.0, f64::max);
        let secs = |s: f64| Duration::from_micros((s * 1e6).round().max(1.0) as u64);
        let reach = |a: Site, b: Site| links.cutoff_distance_m(tx_dbm, floor_dbm, a, b);
        let cell = [Site::Outdoor, Site::Indoor]
            .iter()
            .flat_map(|&a| Site::ALL.map(|b| reach(a, b)))
            .fold(f64::INFINITY, f64::min)
            / 2.0;
        let classes = [Site::Outdoor, Site::Indoor, Site::Rooftop].map(|c| {
            let nodes: Vec<u32> = (0..n as u32)
                .filter(|&i| of[i as usize] == u32::MAX && site[i as usize] == c)
                .collect();
            let pts: Vec<Xy> = nodes.iter().map(|&i| xy[i as usize]).collect();
            Class {
                grid: (!pts.is_empty()).then(|| Grid::new(&pts, cell)),
                nodes,
            }
        });
        Walks {
            of,
            p: p.clone(),
            v: v.clone(),
            wavelength,
            links: links.clone(),
            tx_dbm,
            floor_dbm,
            lo,
            hi,
            tau_still: secs(links.fading.correlation_time_s),
            cell,
            slack: fastest * REGRID.as_micros() as f64 / 1e6,
            cutoff: Site::ALL.map(|a| Site::ALL.map(|b| reach(a, b))),
            shadow_seed: stream_seed(seed, Purpose::Shadowing, 1),
            fade_seed: stream_seed(seed, Purpose::Fading, 1),
            classes,
            moving: [(None, Vec::new()), (None, Vec::new())],
            built: None,
            stays: vec![None; walkers.len()],
            heard: vec![f64::NAN; walkers.len() * fixed as usize],
            fixed,
            walkers,
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
        let (trip, (lo, hi)) = (self.p.trip_m, (self.lo, self.hi));
        let walkers = self.walkers.len();
        let k = &mut self.walkers[w as usize];
        let (speed, stop) = match k.route {
            Some(_) => (k.speed, self.v.stop_s),
            None => (k.speed, self.p.stop_s),
        };
        let here = k.at(now);
        k.leg += 1;
        self.stays[w as usize] = None;
        for b in self.heard.iter_mut().skip(w as usize).step_by(walkers) {
            *b = f64::NAN;
        }
        if k.walking {
            k.walking = false;
            k.from = here;
            k.to = here;
            k.since = now;
            k.until = now + Duration::from_micros((k.rng.exponential(stop) * 1e6) as u64);
        } else if let Some(ends) = k.route {
            // Driven along the route to a point drawn on it.
            let to = on_route(ends, k.rng.uniform());
            k.walking = true;
            k.from = here;
            k.to = to;
            k.since = now;
            let len = here.dist(to);
            k.until = now + Duration::from_micros((len / speed * 1e6).round().max(1.0) as u64);
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

    /// The walkers that a source of `power_dbm` at `at` reaches at or
    /// above `floor_dbm` at `now`, and the power at each: the path loss,
    /// the wall at a stop and the vehicle's body while driven. Like the
    /// sources placed among nodes that stay, it has no shadowing.
    pub fn reached_from(
        &self,
        at: Xy,
        power_dbm: f64,
        floor_dbm: f64,
        now: Instant,
    ) -> Vec<(u32, f32)> {
        self.walkers
            .iter()
            .filter_map(|w| {
                let mut dbm = power_dbm - self.links.path_loss_db(at.dist(w.at(now)));
                if !w.walking {
                    dbm -= self.links.indoor_loss_db;
                }
                if w.driven() {
                    dbm -= self.v.loss_db;
                }
                (dbm >= floor_dbm).then_some((w.node, dbm as f32))
            })
            .collect()
    }

    /// Grid the walkers again if they have moved on or changed what they do.
    fn regrid(&mut self, now: Instant) {
        if self.built.is_some_and(|b| now.since(b) < REGRID) {
            return;
        }
        for (k, site) in [(0, Site::Indoor), (1, Site::Outdoor)] {
            let ws: Vec<u32> = (0..self.walkers.len() as u32)
                .filter(|&w| self.walkers[w as usize].site() == site)
                .collect();
            let pts: Vec<Xy> = ws
                .iter()
                .map(|&w| self.walkers[w as usize].at(now))
                .collect();
            let grid = (!pts.is_empty()).then(|| Grid::new(&pts, self.cell));
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
        let mut stays = std::mem::take(&mut self.stays);
        let mut heard = std::mem::take(&mut self.heard);
        let mut push = |s: &Self, rx: u32, base: f64| {
            let dbm = base + s.fade_db(src, rx, channel, now);
            out.push(MovingLink {
                rx,
                base_dbm: base as f32,
                dbm,
            });
        };
        // Walkers in reach, wherever they went since the grids were built.
        for ((grid, ws), on) in self.moving.iter().zip([Site::Indoor, Site::Outdoor]) {
            if let Some(g) = grid {
                let r = wide(self.cutoff[from_site as usize][on as usize] + self.slack);
                g.within(from, r, |k| {
                    let wi = ws[k as usize];
                    let w = &self.walkers[wi as usize];
                    if w.node == src {
                        return;
                    }
                    let known = (src < self.fixed && !w.walking && !self.walks(src))
                        .then(|| src as usize * self.walkers.len() + wi as usize);
                    let base = match known.map(|i| heard[i]) {
                        Some(b) if !b.is_nan() => b,
                        _ => {
                            let b =
                                self.base_dbm(src, w.node, from, w.at(now), from_site, w.site());
                            if let Some(i) = known {
                                heard[i] = b;
                            }
                            b
                        }
                    };
                    if base != f64::NEG_INFINITY {
                        push(self, w.node, base);
                    }
                });
            }
        }
        // Nodes that stay, in reach of a walker.
        if let Some(&wi) = self.of.get(src as usize).filter(|&&w| w != u32::MAX) {
            if self.walkers[wi as usize].walking {
                for (class, c) in self.classes.iter().zip(Site::ALL) {
                    if let Some(g) = &class.grid {
                        let r = wide(self.cutoff[from_site as usize][c as usize]);
                        g.within(from, r, |k| {
                            let n = class.nodes[k as usize];
                            let base = self.base_dbm(
                                src,
                                n,
                                from,
                                xy[n as usize],
                                from_site,
                                site[n as usize],
                            );
                            if base != f64::NEG_INFINITY {
                                push(self, n, base);
                            }
                        });
                    }
                }
            } else {
                let row =
                    stays[wi as usize].get_or_insert_with(|| self.stay_links(src, from, xy, site));
                for &(n, base) in row.iter() {
                    push(self, n, base);
                }
            }
        }
        self.stays = stays;
        self.heard = heard;
        out.sort_unstable_by_key(|l| l.rx);
        out
    }

    /// The links of the walker `node`, stopped at `at`, to the nodes that
    /// stay, by node, and the power before fading on each.
    fn stay_links(&self, node: u32, at: Xy, xy: &[Xy], site: &[Site]) -> Vec<(u32, f64)> {
        let mut row = Vec::new();
        for (class, c) in self.classes.iter().zip(Site::ALL) {
            if let Some(g) = &class.grid {
                g.within(
                    at,
                    wide(self.cutoff[Site::Indoor as usize][c as usize]),
                    |k| {
                        let n = class.nodes[k as usize];
                        let base = self.base_dbm(
                            node,
                            n,
                            at,
                            xy[n as usize],
                            Site::Indoor,
                            site[n as usize],
                        );
                        if base != f64::NEG_INFINITY {
                            row.push((n, base));
                        }
                    },
                );
            }
        }
        row.sort_unstable_by_key(|l| l.0);
        row
    }

    /// The power before fading at `rx`, at `at` on `rx_site`, of a frame
    /// `src` sends from `from` on `from_site`, dBm; `-∞` if out of reach.
    fn base_dbm(&self, src: u32, rx: u32, from: Xy, at: Xy, from_site: Site, rx_site: Site) -> f64 {
        let d = from.dist(at);
        if d > self.cutoff[from_site as usize][rx_site as usize] {
            return f64::NEG_INFINITY;
        }
        let mover = match (self.walks(src), self.walks(rx)) {
            (true, true) => Xy {
                x: (from.x + at.x) / 2.0,
                y: (from.y + at.y) / 2.0,
            },
            (true, false) => from,
            _ => at,
        };
        let body = |n: u32| {
            if self.walker(n).is_some_and(Walker::driven) {
                self.v.loss_db
            } else {
                0.0
            }
        };
        let loss = self.links.path_loss_db(d)
            + self.links.site_loss_db(from_site, rx_site)
            + body(src)
            + body(rx)
            - self.shadow_db(src, rx, mover);
        let base = self.tx_dbm - loss;
        if base < self.floor_dbm {
            return f64::NEG_INFINITY;
        }
        base
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
    /// stay put, the Doppler coherence time of the faster end while either
    /// moves. A new stop or walk starts a new draw.
    fn fade_db(&self, a: u32, b: u32, channel: Channel, t: Instant) -> f64 {
        let sigma = self.links.fading.sigma_db;
        if sigma <= 0.0 {
            return 0.0;
        }
        let (mut speed, mut legs) = (0.0f64, 0u64);
        for n in [a, b] {
            if let Some(w) = self.walker(n) {
                if w.walking {
                    speed = speed.max(w.speed);
                }
                legs = mix(legs ^ (w.leg as u64) ^ ((n as u64) << 32));
            }
        }
        // Coherence time of fading for a receiver moving at v: 0.423 / f_D
        // with the Doppler spread f_D = v / λ.
        let tau = if speed > 0.0 {
            ((0.423 * self.wavelength / speed * 1e6).round() as u64).max(1)
        } else {
            self.tau_still.as_micros()
        };
        let k = t.as_micros() / tau;
        let u = (t.as_micros() % tau) as f64 / tau as f64;
        let pair = pair_key(a, b) ^ legs;
        let at = |k: u64| normal_fast_at(self.fade_seed, pair, (k << 4) | channel.index() as u64);
        let x = (1.0 - u) * at(k) + u * at(k + 1);
        sigma * x / ((1.0 - u) * (1.0 - u) + u * u).sqrt()
    }
}

/// A search wider than the cut-off `r` by more than the rounding of the
/// distance, so that it finds every node the cut-off keeps.
fn wide(r: f64) -> f64 {
    r * (1.0 + 1e-9) + 1.0
}

/// The point a fraction `f` of the way along a route.
fn on_route((a, b): (Xy, Xy), f: f64) -> Xy {
    Xy {
        x: a.x + (b.x - a.x) * f,
        y: a.y + (b.y - a.y) * f,
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
            &Vehicles::default(),
            &params(),
            10.0,
            -129.0,
            &xy,
            &site,
            1..xy.len() as u32,
            &[],
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

    /// Riders stay on their route, are driven along it at their speed,
    /// and lose the vehicle's body on every link while driven.
    #[test]
    fn riders_drive_along_their_route() {
        let (_, xy, site) = town();
        let v = Vehicles {
            fraction: 1.0,
            relay_fraction: 0.0,
            speed_m_s: 12.0,
            stop_s: 60.0,
            route: [0.0, 400.0, 1000.0, 400.0],
            loss_db: 9.0,
        };
        let m = Movement::default();
        let mut w = Walks::new(
            &m,
            &v,
            &params(),
            10.0,
            -129.0,
            &xy,
            &site,
            1..xy.len() as u32,
            &[],
            7,
        );
        assert_eq!(w.walkers.len(), 400);
        for k in 0..w.walkers.len() as u32 {
            let mut now = w.walkers[k as usize].until;
            // Driven, stopped, driven: each ends on the road.
            for _ in 0..3 {
                let until = w.next_leg(k, now);
                let r = &w.walkers[k as usize];
                for p in [r.at(now), r.at(until)] {
                    assert!((p.y - 400.0).abs() < 1e-6 && (0.0..=1000.0).contains(&p.x));
                }
                if r.driven() {
                    let secs = until.since(now).as_micros() as f64 / 1e6;
                    let (a, b) = (r.at(now), r.at(until));
                    assert!(secs < 1e-5 || (a.dist(b) / secs - 12.0).abs() < 1e-3);
                }
                now = until;
            }
        }
        // The body's loss: a driven rider and the relay, against the same
        // link with the rider stopped outdoors.
        let k = (0..w.walkers.len())
            .find(|&k| w.walkers[k].driven())
            .unwrap();
        let rider = w.walkers[k].node;
        let at = w.walkers[k].since;
        let link = |w: &mut Walks| {
            w.links_from(0, at, Channel::new(4).unwrap(), &xy, &site)
                .into_iter()
                .find(|l| l.rx == rider)
        };
        if let Some(driven) = link(&mut w) {
            w.v.loss_db = 0.0;
            let open = link(&mut w).expect("heard without the body");
            assert!((open.base_dbm - driven.base_dbm - 9.0).abs() < 1e-3);
        }
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
