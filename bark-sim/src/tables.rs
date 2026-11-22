//! What the neighbour tables hold (`metrics::Tables`, with `--tables`):
//! sampled once a minute after the settle time, the entries a heartbeat
//! made way for, and DATA between relays by the entry it went to; and,
//! with relays among the people, the relays no relay keeps
//! (`metrics::Dense`). Counting only: nothing here changes a draw or a
//! decision.

use crate::metrics::{Tables, REACH_BINS, REACH_BIN_M, SECTORS};
use crate::sim::{key, Sim, SERIES_EVERY_US};
use bark_proto::tables::neighbours::Neighbours;
use bark_proto::NodeId;
use std::collections::HashMap;

/// What is followed of the tables between events.
#[derive(Default)]
pub(crate) struct TablesWatch {
    /// DATA on air from a relay to a relay: key to the margin and
    /// distance bin of `Tables::data_by_reach`.
    reach_of: HashMap<u64, (usize, usize)>,
}

/// An entry of a neighbour table: ID, whether a relay, banned, and with
/// margin.
pub(crate) type Entry = (NodeId, bool, bool, bool);

/// The 45° sector of the bearing to `p`, north first and clockwise.
fn sector(p: bark_proto::geo::Point) -> usize {
    let deg = p.east.atan2(p.north).to_degrees().rem_euclid(360.0);
    ((deg / 45.0) as usize).min(SECTORS - 1)
}

/// The bin of [`Tables`] of a neighbour at `p`.
fn reach_bin(p: bark_proto::geo::Point) -> usize {
    ((p.distance_m(Default::default()) / REACH_BIN_M) as usize).min(REACH_BINS - 1)
}

impl Sim {
    /// Per relay, how many running relays keep it in their neighbour
    /// tables, and how many with margin.
    pub(crate) fn known_by(&self) -> Vec<(u32, u32)> {
        let mut known = vec![(0u32, 0u32); self.relays() as usize];
        for (i, r) in self.relays[..self.relays() as usize].iter().enumerate() {
            if !self.nodes[i].alive || r.booting() {
                continue;
            }
            let floor = r.busy().noise_floor_dbm(r.home());
            for e in r.neighbours().filter(|e| e.relay()) {
                if let Some(&j) = self.relay_index.get(&key(e.id)) {
                    let k = &mut known[j as usize];
                    k.0 += 1;
                    k.1 += u32::from(Neighbours::has_margin(e, floor));
                }
            }
        }
        known
    }

    /// Whether relays are placed among the people, so that the dense
    /// instruments are kept.
    fn dense(&self) -> bool {
        self.people.first().is_some_and(|&i| i < self.relays())
    }

    /// Once a minute after the settle time, with relays among the people:
    /// the running relays no running relay keeps (`metrics::Dense`).
    pub(crate) fn sample_dense(&mut self) {
        if !self.dense() || self.now < self.settle {
            return;
        }
        let known = self.known_by();
        let (mut none, mut faint) = ([0u32; 2], [0u32; 2]);
        for (i, k) in known.iter().enumerate() {
            if !self.nodes[i].alive || self.relays[i].booting() {
                continue;
            }
            let person = usize::from(!self.placed_relay(i as u32));
            none[person] += u32::from(k.0 == 0);
            faint[person] += u32::from(k.1 == 0);
        }
        let t = self.now.as_micros() / 1_000_000;
        let d = self
            .metrics
            .research
            .dense
            .get_or_insert_with(Default::default);
        d.unknown.push((t, none, faint));
    }

    /// Sample what the neighbour tables hold (`metrics::Tables`), for the
    /// JSON summary.
    pub fn tables_on(&mut self) {
        self.metrics.research.tables = Some(Tables {
            relays: vec![(0, 0, 0); self.relays() as usize],
            ..Default::default()
        });
    }

    /// Once a minute after the settle time, with `tables_on`: what the
    /// running relays' tables hold; every five minutes, whether each
    /// sleepy node's anchor is the strongest relay it hears.
    pub(crate) fn sample_tables(&mut self) {
        if self.metrics.research.tables.is_none() || self.now < self.settle {
            return;
        }
        let mut t = self.metrics.research.tables.take().expect("on");
        for i in 0..self.relays() as usize {
            let r = &self.relays[i];
            if !self.nodes[i].alive || r.booting() {
                continue;
            }
            let floor = r.busy().noise_floor_dbm(r.home());
            let e = &mut t.relays[i];
            e.0 += 1;
            for n in r.neighbours() {
                let margin = Neighbours::has_margin(n, floor);
                e.1 += 1;
                e.2 += u64::from(margin && n.relay());
                let s = sector(n.local);
                t.sectors[0][s] += 1;
                t.sectors[1][s] += u64::from(margin);
                if margin && n.relay() {
                    t.margin_by_distance[reach_bin(n.local)] += 1;
                }
            }
        }
        if (self.now.as_micros() / SERIES_EVERY_US).is_multiple_of(5) {
            self.sample_anchor_fit(&mut t);
        }
        self.metrics.research.tables = Some(t);
    }

    /// See `sample_tables`.
    fn sample_anchor_fit(&self, t: &mut Tables) {
        let n = self.relays() as usize;
        if self.relays.len() == n {
            return;
        }
        // Per sleepy node, the strongest running relay it hears and its
        // anchor's link, dBm.
        let mut best = vec![(f32::NEG_INFINITY, u32::MAX); self.relays.len() - n];
        let mut anchor = vec![f32::NEG_INFINITY; self.relays.len() - n];
        let anchors: Vec<Option<u32>> = (n..self.relays.len())
            .map(|s| self.attached_relay(s as u32))
            .collect();
        for a in 0..n {
            if !self.nodes[a].alive || self.relays[a].booting() {
                continue;
            }
            for l in self.links.row(a as u32) {
                let Some(k) = (l.rx as usize).checked_sub(n) else {
                    continue;
                };
                if (l.base_dbm as f64) < self.sens_dbm {
                    continue;
                }
                if l.base_dbm > best[k].0 {
                    best[k] = (l.base_dbm, a as u32);
                }
                if anchors[k] == Some(a as u32) {
                    anchor[k] = l.base_dbm;
                }
            }
        }
        for k in 0..best.len() {
            let s = n + k;
            let walks = self.walks.as_ref().and_then(|w| w.walker(s as u32));
            if anchors[k].is_none()
                || !self.nodes[s].alive
                || !self.nodes[s].present
                || walks.is_some()
            {
                continue;
            }
            let (dbm, b) = best[k];
            if b == u32::MAX {
                continue;
            }
            let id = self.relays[b as usize].id();
            let held = self.relays[s].neighbours().any(|e| e.id == id);
            t.anchors.0 += 1;
            t.anchors.1 += u64::from(anchors[k] == Some(b));
            t.anchors.2 += u64::from(held);
            let gap = (dbm - anchor[k]).max(0.0);
            t.anchor_gap_db[(gap as usize).min(30)] += 1;
        }
    }

    /// The entries of node `b`'s neighbour table: ID, whether a relay,
    /// banned, and with margin.
    fn entries(&self, b: u32) -> Vec<Entry> {
        let r = self.relay(b);
        let floor = r.busy().noise_floor_dbm(r.home());
        let now = self.now;
        r.neighbours()
            .map(|e| {
                let banned = now < e.banned_until;
                (e.id, e.relay(), banned, Neighbours::has_margin(e, floor))
            })
            .collect()
    }

    /// With `tables_on`, after the settle time: DATA from relay `src` to
    /// the relay `to` goes on the air as `tx_key`, by margin and distance
    /// (`metrics::Tables::data_by_reach`).
    pub(crate) fn tables_data(&mut self, src: u32, to: NodeId, tx_key: u64) {
        if self.metrics.research.tables.is_none() || self.now < self.settle || src >= self.relays()
        {
            return;
        }
        if !self
            .relay_index
            .get(&key(to))
            .is_some_and(|&r| r < self.relays())
        {
            return;
        }
        let r = self.relay(src);
        let floor = r.busy().noise_floor_dbm(r.home());
        let seen = r.neighbours().find(|e| e.id == to).map(|e| {
            (
                usize::from(Neighbours::has_margin(e, floor)),
                reach_bin(e.local),
            )
        });
        if let Some(s) = seen {
            self.tables.reach_of.insert(tx_key, s);
        }
    }

    /// With `tables_on`, after the settle time: the entries of `b`'s table
    /// before it is handed a heartbeat.
    pub(crate) fn tables_entries(&self, b: u32) -> Option<Vec<Entry>> {
        (self.metrics.research.tables.is_some() && self.now >= self.settle).then(|| self.entries(b))
    }

    /// The entries of `old` that `b`'s table no longer holds made way.
    pub(crate) fn tables_made_way(&mut self, b: u32, old: &[Entry]) {
        let now = self.entries(b);
        let t = self.metrics.research.tables.as_mut().expect("on");
        for e in old.iter().filter(|e| !now.iter().any(|n| n.0 == e.0)) {
            let k = &mut t.made_way[usize::from(e.1)];
            k[0] += 1;
            k[1] += u64::from(e.2);
            k[2] += u64::from(e.3);
        }
    }

    /// The frame `tx_key` left the air, received by `got`: a DATA between
    /// relays is counted by whether it reached the relay it was for.
    pub(crate) fn tables_air_end(&mut self, tx_key: u64, got: &[(u32, f64, f64)]) {
        if let Some((m, b)) = self.tables.reach_of.remove(&tx_key) {
            let d = self.addressed.get(&tx_key).map(|a| a.0);
            let got = d.is_some_and(|d| got.iter().any(|g| g.0 == d));
            if let Some(t) = &mut self.metrics.research.tables {
                t.data_by_reach[m][b][0] += 1;
                t.data_by_reach[m][b][1] += u64::from(got);
            }
        }
    }
}
