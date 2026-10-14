//! The sparse link table: receive power for every (transmitter, receiver)
//! pair that can interact, per channel, varying in time.
//!
//! `rx_dbm(a → b, c, t) = base(a → b) + static(pair, c) + fade(pair, c, t)`.
//!
//! - `base` includes the transmitter's power, path loss, per-node indoor
//!   loss and shadowing. It is per direction because two nodes may transmit
//!   at different powers.
//! - `static` is a per-channel offset a loaded table may give (antenna match
//!   across the band); zero for generated links.
//! - `fade` is frequency-selective multipath fading: one Gauss-Markov
//!   process per pair and channel, shared by both directions (the channel
//!   is reciprocal), stepped by a heap event on a slow clock.
//!
//! A pair is stored only if it can matter: its strongest plausible power
//! (base plus three fading standard deviations) reaches the cut-off,
//! sensitivity minus the interference margin. Generators find candidate
//! pairs with a spatial grid whose cell is the cut-off distance, so building
//! the table visits each node's 3 × 3 cell neighbourhood and never all N².

use crate::geo::{Grid, Xy};
use crate::rng::{normal_at, stream_seed, Purpose};
use bark_proto::radio::{Channel, CHANNELS};
use bark_proto::Duration;
use serde::{Deserialize, Serialize};

const NCH: usize = CHANNELS as usize;

/// Speed of light, m/s.
const C_M_PER_S: f64 = 299_792_458.0;

/// Free-space loss at 1 m for the band centre (channel 6, 433.92 MHz):
/// 20·log10(4π f / c) ≈ 25.2 dB.
pub fn free_space_loss_1m_db() -> f64 {
    let f = Channel::new(CHANNELS / 2).expect("centre channel").hz() as f64;
    20.0 * (4.0 * core::f64::consts::PI * f / C_M_PER_S).log10()
}

/// Shadowing draws are truncated at this many standard deviations, so that
/// the generator's cut-off distance is a hard bound.
pub const SHADOW_TRUNCATE_SIGMA: f64 = 3.0;

/// Fading headroom, in standard deviations, when deciding whether a pair
/// can reach the cut-off.
pub const FADE_HEADROOM_SIGMA: f64 = 3.0;

/// Which generator fills the table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Model {
    /// Log-distance path loss.
    #[default]
    LogDistance,
    /// Explicit table of pairs (`table`); absent pairs are out of range.
    Table,
}

/// One explicit link.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TableLink {
    /// One end (node name).
    pub a: String,
    /// The other end (node name).
    pub b: String,
    /// Receive power at `b` when `a` transmits, in dBm.
    pub dbm: f64,
    /// Receive power at `a` when `b` transmits; defaults to `dbm`.
    #[serde(default)]
    pub dbm_back: Option<f64>,
    /// Per-channel static offsets in dB, 13 values, applied both ways.
    #[serde(default)]
    pub channel_offsets_db: Option<Vec<f64>>,
}

/// Fading process parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FadingParams {
    /// Standard deviation of the per-channel offset in dB; 0 disables fading.
    pub sigma_db: f64,
    /// Correlation time in seconds.
    pub correlation_time_s: f64,
    /// Update step in seconds (the slow clock).
    pub step_s: f64,
}

impl Default for FadingParams {
    fn default() -> Self {
        FadingParams { sigma_db: 0.0, correlation_time_s: 60.0, step_s: 10.0 }
    }
}

/// Link generator parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LinkParams {
    /// Generator.
    pub model: Model,
    /// Path-loss exponent (2 free space; 2.7–3.5 urban; 4+ dense indoor).
    pub exponent: f64,
    /// Loss at 1 m in dB; `None` uses free space at the band centre.
    pub reference_loss_db: Option<f64>,
    /// Extra loss in dB for each end of a link that is an indoor node.
    pub indoor_loss_db: f64,
    /// Log-normal shadowing standard deviation in dB; 0 disables it.
    pub shadowing_sigma_db: f64,
    /// Per-channel time-varying fading.
    pub fading: FadingParams,
    /// Explicit links for `model = "table"`.
    pub table: Vec<TableLink>,
}

impl Default for LinkParams {
    fn default() -> Self {
        LinkParams {
            model: Model::LogDistance,
            exponent: 3.0,
            reference_loss_db: None,
            indoor_loss_db: 0.0,
            shadowing_sigma_db: 0.0,
            fading: FadingParams::default(),
            table: Vec::new(),
        }
    }
}

impl LinkParams {
    /// Loss at 1 m.
    pub fn l0(&self) -> f64 {
        self.reference_loss_db.unwrap_or_else(free_space_loss_1m_db)
    }

    /// Mean path loss (no shadowing, no indoor loss) at `d` metres. Below
    /// 1 m the 1 m value holds.
    pub fn path_loss_db(&self, d: f64) -> f64 {
        self.l0() + 10.0 * self.exponent * d.max(1.0).log10()
    }

    /// Largest distance at which a transmitter of `tx_dbm` can deliver
    /// `floor_dbm` with every allowance in its favour (no indoor loss,
    /// shadowing at its truncation, fading headroom).
    pub fn cutoff_distance_m(&self, tx_dbm: f64, floor_dbm: f64) -> f64 {
        let budget = tx_dbm + SHADOW_TRUNCATE_SIGMA * self.shadowing_sigma_db
            + FADE_HEADROOM_SIGMA * self.fading.sigma_db
            - floor_dbm
            - self.l0();
        10f64.powf(budget / (10.0 * self.exponent)).max(1.0)
    }
}

/// One directed link: receiver, base power, and its pair (for offsets).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Link {
    /// Receiving node.
    pub rx: u32,
    /// Base receive power in dBm.
    pub base_dbm: f32,
    /// Index of the unordered pair, for per-channel offsets.
    pub pair: u32,
}

/// Time-varying per-channel offsets.
pub struct Fading {
    rho: f64,
    innovation: f64,
    step: Duration,
    seed: u64,
    tick: u64,
    offsets: Vec<[f32; NCH]>,
}

/// What a generator did, for tests and the summary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct BuildStats {
    /// Candidate pairs the generator evaluated (each unordered pair once).
    pub evaluated: u64,
    /// Unordered pairs stored.
    pub pairs: u64,
    /// Directed links stored.
    pub links: u64,
    /// Grid cell side, the cut-off distance, in metres (0 for a table).
    pub cutoff_m: f64,
}

/// The sparse table, laid out as one row of [`Link`]s per transmitter.
pub struct LinkTable {
    row_start: Vec<u32>,
    links: Vec<Link>,
    pair_nodes: Vec<(u32, u32)>,
    static_offsets: Vec<[f32; NCH]>,
    fading: Option<Fading>,
    /// Build statistics.
    pub stats: BuildStats,
}

/// Per-node inputs to a generator.
pub struct NodeRadio<'a> {
    /// Positions on the local grid.
    pub xy: &'a [Xy],
    /// Transmit power in dBm.
    pub tx_dbm: &'a [f64],
    /// Indoor flag.
    pub indoor: &'a [bool],
    /// Names, for table lookups.
    pub names: &'a [Option<String>],
}

impl LinkTable {
    /// Build with the configured generator. `cutoff_dbm` is sensitivity
    /// minus the interference margin.
    pub fn build(p: &LinkParams, nodes: &NodeRadio<'_>, cutoff_dbm: f64, run_seed: u64) -> Result<LinkTable, String> {
        let n = nodes.xy.len();
        let mut rows: Vec<Vec<Link>> = vec![Vec::new(); n];
        let mut pair_nodes = Vec::new();
        let mut static_offsets = Vec::new();
        let mut stats = BuildStats::default();
        let headroom = FADE_HEADROOM_SIGMA * p.fading.sigma_db;
        match p.model {
            Model::LogDistance => {
                let tx_max = nodes.tx_dbm.iter().cloned().fold(f64::MIN, f64::max);
                let cut = p.cutoff_distance_m(tx_max, cutoff_dbm);
                stats.cutoff_m = cut;
                let grid = Grid::new(nodes.xy, cut);
                let shadow_seed = stream_seed(run_seed, Purpose::Shadowing, 0);
                for a in 0..n {
                    grid.neighbourhood(nodes.xy[a], |b| {
                        let b = b as usize;
                        if b <= a {
                            return;
                        }
                        stats.evaluated += 1;
                        let d = nodes.xy[a].dist(nodes.xy[b]);
                        if d > cut {
                            return;
                        }
                        let mut loss = p.path_loss_db(d);
                        loss += p.indoor_loss_db * (nodes.indoor[a] as u8 + nodes.indoor[b] as u8) as f64;
                        if p.shadowing_sigma_db > 0.0 {
                            loss -= p.shadowing_sigma_db * truncated_normal(shadow_seed, a as u64, b as u64);
                        }
                        let ab = nodes.tx_dbm[a] - loss;
                        let ba = nodes.tx_dbm[b] - loss;
                        push_pair(&mut rows, &mut pair_nodes, a, b, ab, ba, cutoff_dbm - headroom);
                    });
                }
            }
            Model::Table => {
                let index = |name: &str| -> Result<usize, String> {
                    nodes
                        .names
                        .iter()
                        .position(|x| x.as_deref() == Some(name))
                        .ok_or_else(|| format!("link table names unknown node {name:?}"))
                };
                let mut any_offsets = false;
                for l in &p.table {
                    let (a, b) = (index(&l.a)?, index(&l.b)?);
                    if a == b {
                        return Err(format!("link table has a self-link on {:?}", l.a));
                    }
                    stats.evaluated += 1;
                    let before = pair_nodes.len();
                    push_pair(&mut rows, &mut pair_nodes, a, b, l.dbm, l.dbm_back.unwrap_or(l.dbm), cutoff_dbm - headroom);
                    let mut off = [0f32; NCH];
                    if let Some(o) = &l.channel_offsets_db {
                        if o.len() != NCH {
                            return Err(format!("link {}–{}: channel_offsets_db needs {NCH} values", l.a, l.b));
                        }
                        for (d, s) in off.iter_mut().zip(o) {
                            *d = *s as f32;
                        }
                        any_offsets = true;
                    }
                    if pair_nodes.len() > before {
                        static_offsets.push(off);
                    }
                }
                if !any_offsets {
                    static_offsets.clear();
                }
            }
        }
        let mut row_start = Vec::with_capacity(n + 1);
        let mut links = Vec::with_capacity(rows.iter().map(Vec::len).sum());
        row_start.push(0u32);
        for mut r in rows {
            r.sort_by_key(|l| l.rx);
            links.extend_from_slice(&r);
            row_start.push(links.len() as u32);
        }
        stats.pairs = pair_nodes.len() as u64;
        stats.links = links.len() as u64;
        let fading = (p.fading.sigma_db > 0.0).then(|| Fading::new(&p.fading, &pair_nodes, run_seed));
        Ok(LinkTable { row_start, links, pair_nodes, static_offsets, fading, stats })
    }

    /// Stored receivers of transmitter `a`.
    pub fn row(&self, a: u32) -> &[Link] {
        &self.links[self.row_start[a as usize] as usize..self.row_start[a as usize + 1] as usize]
    }

    /// The link `a → b`, if stored.
    pub fn get(&self, a: u32, b: u32) -> Option<&Link> {
        let r = self.row(a);
        r.binary_search_by_key(&b, |l| l.rx).ok().map(|i| &r[i])
    }

    /// Receive power of `link` on `channel` now.
    pub fn rx_dbm(&self, link: &Link, channel: Channel) -> f64 {
        let c = channel.index() as usize;
        let mut p = link.base_dbm as f64;
        if let Some(o) = self.static_offsets.get(link.pair as usize) {
            p += o[c] as f64;
        }
        if let Some(f) = &self.fading {
            p += f.offsets[link.pair as usize][c] as f64;
        }
        p
    }

    /// Fading step, if fading is on.
    pub fn fading_step(&self) -> Option<Duration> {
        self.fading.as_ref().map(|f| f.step)
    }

    /// Advance every fading process by one step.
    pub fn fading_tick(&mut self) {
        if let Some(f) = &mut self.fading {
            f.tick += 1;
            for (o, &(a, b)) in f.offsets.iter_mut().zip(&self.pair_nodes) {
                for (c, x) in o.iter_mut().enumerate() {
                    let n = normal_at(f.seed, pair_key(a, b), (f.tick << 4) | c as u64);
                    *x = (f.rho * *x as f64 + f.innovation * n) as f32;
                }
            }
        }
    }

    /// Number of nodes the table covers.
    pub fn nodes(&self) -> usize {
        self.row_start.len() - 1
    }
}

impl Fading {
    fn new(p: &FadingParams, pairs: &[(u32, u32)], run_seed: u64) -> Fading {
        let seed = stream_seed(run_seed, Purpose::Fading, 0);
        let rho = (-p.step_s / p.correlation_time_s).exp();
        let offsets = pairs
            .iter()
            .map(|&(a, b)| {
                let mut o = [0f32; NCH];
                for (c, x) in o.iter_mut().enumerate() {
                    // Start in the stationary distribution.
                    *x = (p.sigma_db * normal_at(seed, pair_key(a, b), c as u64)) as f32;
                }
                o
            })
            .collect();
        Fading {
            rho,
            innovation: p.sigma_db * (1.0 - rho * rho).sqrt(),
            step: Duration::from_micros((p.step_s * 1e6).round() as u64),
            seed,
            tick: 0,
            offsets,
        }
    }
}

/// Key of an unordered pair, independent of table layout, so that the same
/// two nodes see the same fading whatever else is in the scenario.
fn pair_key(a: u32, b: u32) -> u64 {
    let (lo, hi) = if a < b { (a, b) } else { (b, a) };
    ((lo as u64) << 32) | hi as u64
}

/// Standard normal truncated at ±[`SHADOW_TRUNCATE_SIGMA`] by resampling;
/// a pure function of `(seed, a, b)`.
fn truncated_normal(seed: u64, a: u64, b: u64) -> f64 {
    let key = pair_key(a as u32, b as u32);
    (0u64..)
        .map(|k| normal_at(seed, key, k))
        .find(|x| x.abs() <= SHADOW_TRUNCATE_SIGMA)
        .expect("resampling terminates")
}

fn push_pair(rows: &mut [Vec<Link>], pairs: &mut Vec<(u32, u32)>, a: usize, b: usize, ab: f64, ba: f64, floor: f64) {
    if ab < floor && ba < floor {
        return;
    }
    let pair = pairs.len() as u32;
    pairs.push((a as u32, b as u32));
    if ab >= floor {
        rows[a].push(Link { rx: b as u32, base_dbm: ab as f32, pair });
    }
    if ba >= floor {
        rows[b].push(Link { rx: a as u32, base_dbm: ba as f32, pair });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Stream;

    fn scatter(n: usize, side: f64, seed: u64) -> Vec<Xy> {
        let mut s = Stream::new(seed, Purpose::Placement, 0);
        (0..n).map(|_| Xy { x: s.uniform() * side, y: s.uniform() * side }).collect()
    }

    #[test]
    fn free_space_reference() {
        assert!((free_space_loss_1m_db() - 25.2).abs() < 0.05);
    }

    #[test]
    fn sparse_table_never_stores_out_of_range_and_visits_only_grid_neighbourhood() {
        let n = 1500;
        let xy = scatter(n, 20_000.0, 1);
        let tx = vec![10.0; n];
        let indoor: Vec<bool> = (0..n).map(|i| i % 3 == 0).collect();
        let names = vec![None; n];
        let p = LinkParams { exponent: 3.5, indoor_loss_db: 10.0, shadowing_sigma_db: 4.0, ..LinkParams::default() };
        let cutoff = -129.0;
        let nodes = NodeRadio { xy: &xy, tx_dbm: &tx, indoor: &indoor, names: &names };
        let t = LinkTable::build(&p, &nodes, cutoff, 9).unwrap();
        let cut = t.stats.cutoff_m;
        assert!(cut < 5000.0, "cut-off {cut} m should be a fraction of the area");

        // Every stored link is within the cut-off distance and above the cut-off power.
        let mut stored = 0;
        for a in 0..n as u32 {
            for l in t.row(a) {
                assert!(xy[a as usize].dist(xy[l.rx as usize]) <= cut);
                assert!(l.base_dbm as f64 >= cutoff - 1e-4, "{} < {cutoff}", l.base_dbm);
                stored += 1;
            }
        }
        assert_eq!(stored as u64, t.stats.links);

        // Brute force over all pairs, in the test only: the table holds
        // exactly the pairs whose power reaches the cut-off.
        let seed = stream_seed(9, Purpose::Shadowing, 0);
        for a in 0..n {
            for b in 0..n {
                if a == b {
                    continue;
                }
                let d = xy[a].dist(xy[b]);
                let mut loss = p.path_loss_db(d) + 10.0 * (indoor[a] as u8 + indoor[b] as u8) as f64;
                loss -= 4.0 * truncated_normal(seed, a.min(b) as u64, a.max(b) as u64);
                let expect = 10.0 - loss >= cutoff && d <= cut;
                assert_eq!(t.get(a as u32, b as u32).is_some(), expect, "pair {a}->{b} at {d} m");
            }
        }

        // The generator evaluated no more pairs than the 3 × 3 neighbourhoods hold.
        let grid = Grid::new(&xy, cut);
        let mut bound = 0u64;
        for &q in &xy {
            grid.neighbourhood(q, |_| bound += 1);
        }
        assert!(t.stats.evaluated * 2 <= bound, "{} evaluated, bound {}", t.stats.evaluated, bound);
        assert!(t.stats.evaluated < (n * n / 2) as u64 / 2, "well below N²/2");
    }

    #[test]
    fn fading_is_stationary_and_reciprocal() {
        let xy = vec![Xy { x: 0.0, y: 0.0 }, Xy { x: 100.0, y: 0.0 }];
        let p = LinkParams {
            fading: FadingParams { sigma_db: 4.0, correlation_time_s: 10.0, step_s: 1.0 },
            ..LinkParams::default()
        };
        let nodes = NodeRadio { xy: &xy, tx_dbm: &[10.0, 10.0], indoor: &[false, false], names: &[None, None] };
        let mut t = LinkTable::build(&p, &nodes, -129.0, 3).unwrap();
        let (ab, ba) = (*t.get(0, 1).unwrap(), *t.get(1, 0).unwrap());
        let base = ab.base_dbm as f64;
        let (mut sum, mut sq, mut lag, mut prev) = (0.0, 0.0, 0.0, 0.0);
        let steps = 50_000;
        let ch = Channel::new(3).unwrap();
        for i in 0..steps {
            t.fading_tick();
            assert_eq!(t.rx_dbm(&ab, ch), t.rx_dbm(&ba, ch), "reciprocal");
            let x = t.rx_dbm(&ab, ch) - base;
            sum += x;
            sq += x * x;
            if i > 0 {
                lag += x * prev;
            }
            prev = x;
        }
        let var = sq / steps as f64 - (sum / steps as f64).powi(2);
        assert!((var.sqrt() - 4.0).abs() < 0.3, "sigma {}", var.sqrt());
        let rho = lag / (steps - 1) as f64 / var;
        assert!((rho - (-0.1f64).exp()).abs() < 0.03, "lag-1 correlation {rho}");
    }
}
