//! How soon a node that switches on takes part (roadmap D73).
//!
//! Each boot, the first at the node's switch-on and every scripted start
//! or restart, is watched until it has its answers: when the node first
//! knows a neighbour; for a sleepy node, when it first has an anchor; for
//! a relay, when each relay in range, one that receives its heartbeat at
//! or above sensitivity on average, first holds it in its neighbour
//! table with the home channel it listens on then, so that what a
//! restarted node's neighbours kept of it counts only while it is right;
//! and for a scripted boot, when a text the node's application
//! offers at once reaches its destination.

use crate::metrics::percentile;
use bark_proto::{Channel, Instant, NodeId, Relay};
use serde::Serialize;

/// What switched the node on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BootKind {
    /// Its switch-on at the start of the run.
    Initial,
    /// A scripted `start`: a node planted in the running network.
    Start,
    /// A scripted `restart`: off and on again, its stored state kept.
    Restart,
    /// A scripted `wipe`: off and on again with its stored state erased.
    Wipe,
}

/// One boot under watch.
pub(crate) struct Watch {
    node: u32,
    id: NodeId,
    kind: BootKind,
    sleepy: bool,
    at: Instant,
    neighbour: Option<Instant>,
    anchor: Option<Instant>,
    in_range: u32,
    /// When each relay in range first held the node.
    held: Vec<Instant>,
    /// The tag of the text offered at boot.
    pub(crate) text: Option<u32>,
    /// A later boot of the node took its place.
    closed: bool,
}

/// Every boot of the run.
pub(crate) struct Boots {
    watches: Vec<Watch>,
    /// Per node, its boot still waiting for its first neighbour or anchor.
    open: Vec<Option<u32>>,
    /// Per relay, the boots of nodes in its range it does not hold yet.
    pending: Vec<Vec<u32>>,
}

impl Boots {
    /// For `nodes` nodes.
    pub(crate) fn new(nodes: usize) -> Boots {
        Boots {
            watches: Vec::new(),
            open: vec![None; nodes],
            pending: vec![Vec::new(); nodes],
        }
    }

    /// Node `node`, `sleepy` or not, with ID `id`, booted at `at`; the
    /// relays in its range are `in_range`. Returns the watch's index.
    pub(crate) fn begin(
        &mut self,
        node: u32,
        id: NodeId,
        kind: BootKind,
        sleepy: bool,
        at: Instant,
        in_range: &[u32],
    ) -> usize {
        for w in self.watches.iter_mut().filter(|w| w.node == node) {
            w.closed = true;
        }
        let k = self.watches.len();
        self.watches.push(Watch {
            node,
            id,
            kind,
            sleepy,
            at,
            neighbour: None,
            anchor: None,
            in_range: in_range.len() as u32,
            held: Vec::new(),
            text: None,
            closed: false,
        });
        self.open[node as usize] = Some(k as u32);
        for &r in in_range {
            self.pending[r as usize].push(k as u32);
        }
        k
    }

    /// The text offered at boot `k` has tag `tag`.
    pub(crate) fn text(&mut self, k: usize, tag: u32) {
        self.watches[k].text = Some(tag);
    }

    /// Node `node`, whose core is `core`, has handled an event at `now`,
    /// a frame it `received` or another; `home` gives the home channel a
    /// node listens on now. Only a frame fills a neighbour table.
    pub(crate) fn after_event(
        &mut self,
        node: u32,
        core: &Relay,
        now: Instant,
        received: bool,
        home: impl Fn(u32) -> Channel,
    ) {
        let n = node as usize;
        if let Some(k) = self.open[n] {
            let w = &mut self.watches[k as usize];
            if received && w.neighbour.is_none() && core.neighbours().next().is_some() {
                w.neighbour = Some(now);
            }
            if w.sleepy && w.anchor.is_none() && core.anchor().is_some() {
                w.anchor = Some(now);
            }
            if w.neighbour.is_some() && (!w.sleepy || w.anchor.is_some()) {
                self.open[n] = None;
            }
        }
        if !received || self.pending[n].is_empty() {
            return;
        }
        let watches = &mut self.watches;
        self.pending[n].retain(|&k| {
            let w = &mut watches[k as usize];
            if w.closed {
                return false;
            }
            let h = home(w.node);
            if core.neighbours().any(|x| x.id == w.id && x.home == h) {
                w.held.push(now);
                return false;
            }
            true
        });
    }

    /// The boots by kind and role, with the delivery of each boot's text
    /// as `delivered` gives it, from the text's tag.
    pub(crate) fn summary(&self, delivered: impl Fn(u32) -> Option<Instant>) -> Vec<BootRow> {
        let mut keys: Vec<(BootKind, bool)> =
            self.watches.iter().map(|w| (w.kind, w.sleepy)).collect();
        keys.sort_unstable();
        keys.dedup();
        keys.into_iter()
            .map(|(kind, sleepy)| {
                let ws: Vec<&Watch> = self
                    .watches
                    .iter()
                    .filter(|w| w.kind == kind && w.sleepy == sleepy)
                    .collect();
                let after = |w: &Watch, t: Option<Instant>| t.map(|t| t.since(w.at).as_micros());
                // The time by which a share of the relays in range held it.
                let share = |w: &Watch, p: f64| {
                    let mut h: Vec<u64> =
                        w.held.iter().map(|t| t.since(w.at).as_micros()).collect();
                    h.sort_unstable();
                    let need = (p * w.in_range as f64).ceil() as usize;
                    (w.in_range > 0 && h.len() >= need).then(|| h[need.max(1) - 1])
                };
                let ranged: Vec<&&Watch> = ws.iter().filter(|w| w.in_range > 0).collect();
                let texts: Vec<&&Watch> = ws.iter().filter(|w| w.text.is_some()).collect();
                BootRow {
                    kind,
                    sleepy,
                    boots: ws.len() as u64,
                    neighbour_s: Spread::of(ws.iter().map(|w| after(w, w.neighbour))),
                    anchor_s: sleepy.then(|| Spread::of(ws.iter().map(|w| after(w, w.anchor)))),
                    in_range_mean: ranged.iter().map(|w| w.in_range as f64).sum::<f64>()
                        / ranged.len().max(1) as f64,
                    held_half_s: Spread::of(ranged.iter().map(|w| share(w, 0.5))),
                    held_most_s: Spread::of(ranged.iter().map(|w| share(w, 0.9))),
                    held_all_s: Spread::of(ranged.iter().map(|w| share(w, 1.0))),
                    held_share: ranged
                        .iter()
                        .map(|w| w.held.len() as f64 / w.in_range as f64)
                        .sum::<f64>()
                        / ranged.len().max(1) as f64,
                    text_s: Spread::of(texts.iter().map(|w| after(w, w.text.and_then(&delivered)))),
                }
            })
            .collect()
    }
}

/// Times from boot, seconds: how many of the boots reached the event, and
/// the median, 90th percentile and largest of those.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Spread {
    /// Boots counted.
    pub of: u64,
    /// Of those, ones that reached it.
    pub reached: u64,
    /// Median, s.
    pub p50: Option<f64>,
    /// 90th percentile, s.
    pub p90: Option<f64>,
    /// Largest, s.
    pub max: Option<f64>,
}

impl Spread {
    fn of(times: impl Iterator<Item = Option<u64>>) -> Spread {
        let mut of = 0;
        let mut t: Vec<u64> = times.inspect(|_| of += 1).flatten().collect();
        t.sort_unstable();
        let s = |p| percentile(&t, p).map(|us| us as f64 / 1e6);
        Spread {
            of,
            reached: t.len() as u64,
            p50: s(50.0),
            p90: s(90.0),
            max: s(100.0),
        }
    }

    fn line(&self) -> String {
        let f = |v: Option<f64>| v.map_or("-".into(), |v| format!("{v:.1}"));
        format!(
            "{}/{} p50 {} p90 {} max {}",
            self.reached,
            self.of,
            f(self.p50),
            f(self.p90),
            f(self.max)
        )
    }
}

/// The boots of one kind and role.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BootRow {
    /// What switched the nodes on.
    pub kind: BootKind,
    /// Whether they were sleepy at boot.
    pub sleepy: bool,
    /// How many.
    pub boots: u64,
    /// To the first neighbour known.
    pub neighbour_s: Spread,
    /// Of sleepy nodes, to the first anchor.
    pub anchor_s: Option<Spread>,
    /// Relays in range of a boot, of those with any.
    pub in_range_mean: f64,
    /// To half, nine in ten, and all of the relays in range holding the
    /// node in their neighbour tables.
    pub held_half_s: Spread,
    /// See `held_half_s`.
    pub held_most_s: Spread,
    /// See `held_half_s`.
    pub held_all_s: Spread,
    /// The share of the relays in range that held it by the end, mean.
    pub held_share: f64,
    /// To the delivery of the text offered at boot.
    pub text_s: Spread,
}

impl BootRow {
    /// Lines for the printed table.
    pub fn lines(&self) -> String {
        let mut s = format!(
            "boots {:?} {}: {}; first neighbour {}",
            self.kind,
            if self.sleepy { "sleepy" } else { "relay" },
            self.boots,
            self.neighbour_s.line()
        );
        if let Some(a) = &self.anchor_s {
            s.push_str(&format!("; anchor {}", a.line()));
        }
        if self.held_all_s.of > 0 {
            s.push_str(&format!(
                "\n  held by relays in range ({:.1} on average, {:.1} % by the end): half {}; 90 % {}; all {}",
                self.in_range_mean,
                100.0 * self.held_share,
                self.held_half_s.line(),
                self.held_most_s.line(),
                self.held_all_s.line()
            ));
        }
        if self.text_s.of > 0 {
            s.push_str(&format!(
                "\n  text at boot delivered {}",
                self.text_s.line()
            ));
        }
        s.push('\n');
        s
    }
}
