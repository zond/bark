//! End-of-run summary: JSON for machines, a short table for people.
//!
//! The summary is a pure function of the run (no wall-clock time), so two
//! runs of one scenario produce byte-identical JSON.

use crate::links::BuildStats;
use crate::metrics::{percentile, DropCounts, NodeMetrics, RxCounts};
use crate::sim::{Fate, Sim};
use bark_proto::radio::CHANNELS;
use bark_proto::Instant;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt::Write;

/// Latency percentiles in milliseconds.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Latency {
    /// Median.
    pub p50_ms: f64,
    /// 90th percentile.
    pub p90_ms: f64,
    /// 99th percentile.
    pub p99_ms: f64,
    /// Maximum.
    pub max_ms: f64,
}

/// Application-level summary.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AppSummary {
    /// Messages originated.
    pub originated: u64,
    /// How long applications waited for their core to take a message.
    pub offers: Offers,
    /// Delivered to the destination's application (distinct).
    pub delivered: u64,
    /// `delivered / originated`, 0 if nothing was originated.
    pub delivery_ratio: f64,
    /// Duplicate deliveries.
    pub duplicates: u64,
    /// Deliveries to the wrong node.
    pub misdelivered: u64,
    /// Drop reports from cores, by reason.
    pub drops: DropCounts,
    /// Messages whose last reported fate is a drop, by reason.
    pub lost_dropped: DropCounts,
    /// Every message lost, by where and why: `"<node>: <reason>"`.
    pub lost_at: BTreeMap<String, u64>,
    /// Messages not sent because their destination had no address yet.
    pub unaddressed: u64,
    /// Messages a core still held at the end, neither delivered nor
    /// reported lost.
    pub in_flight: u64,
    /// Messages neither delivered, nor reported lost, nor held: lost
    /// without a trace.
    pub unaccounted: u64,
    /// Messages to a destination that could not be reached from the
    /// origin at the start of the run (`Sim::reachable`).
    pub unreachable: u64,
    /// Origination-to-delivery latency.
    pub latency: Option<Latency>,
    /// The same messages split by origination time: before the
    /// scenario's settle time, from then until the first scripted
    /// removal, and after it. A message originated as the run ends has
    /// had no time to arrive and is in no phase. Empty phases are left out.
    pub phases: Vec<PhaseSummary>,
}

/// The pushback applications felt (core-interface §4): each holds one
/// message and hands it over when its core takes one.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Offers {
    /// Messages the core did not take the moment they were due, those
    /// still waiting at the end included.
    pub waited: u64,
    /// Of those, still waiting at the end.
    pub still_waiting: u64,
    /// The longest of those waits, ms, until the end for one still
    /// waiting.
    pub longest_wait_ms: f64,
    /// Messages not sent because the application still waited with the
    /// previous one.
    pub skipped: u64,
}

/// Delivery of the messages originated in one phase of the run.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PhaseSummary {
    /// Which phase: `"before settle"`, `"settled"` or `"after removal"`.
    pub phase: String,
    /// Start, seconds.
    pub from_s: f64,
    /// End (exclusive), seconds.
    pub to_s: f64,
    /// Messages originated in the phase.
    pub originated: u64,
    /// Of those, delivered.
    pub delivered: u64,
    /// `delivered / originated`, 0 if nothing was originated.
    pub delivery_ratio: f64,
    /// Of those, lost, by where and why: `"<node>: <reason>"`.
    pub lost_at: BTreeMap<String, u64>,
    /// Of those, still held by a core at the end.
    pub in_flight: u64,
    /// Of those, neither delivered, nor reported lost, nor held.
    pub unaccounted: u64,
    /// Of those, to a destination that could not be reached from the
    /// origin at the start of the run; they count in `originated`.
    pub unreachable: u64,
    /// Of the unreachable ones, delivered anyway (the links changed).
    pub unreachable_delivered: u64,
    /// `delivered / originated` over the reachable ones only.
    pub reachable_delivery_ratio: f64,
}

/// Per-channel summary.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ChannelSummary {
    /// Channel index.
    pub channel: u8,
    /// Reception outcomes on this channel.
    pub rx: RxCounts,
    /// Frames sent on this channel.
    pub tx_frames: u64,
    /// Airtime on this channel, seconds.
    pub airtime_s: f64,
    /// Busy fraction over the run, mean over observers.
    pub busy_fraction: Option<f64>,
}

/// One observer's busy fractions.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ObserverSummary {
    /// Node label.
    pub node: String,
    /// Busy fraction per channel over the run.
    pub busy_fraction: Vec<f64>,
}

/// One node.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NodeSummary {
    /// Index.
    pub id: u32,
    /// Label.
    pub label: String,
    /// Counters.
    #[serde(flatten)]
    pub m: NodeMetrics,
    /// Airtime as a fraction of the run (the duty-cycle cap is 0.10).
    pub airtime_fraction: f64,
}

/// The run summary.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Summary {
    /// Scenario name.
    pub scenario: String,
    /// Run seed.
    pub seed: u64,
    /// Simulated seconds.
    pub duration_s: f64,
    /// Relays.
    pub relays: u32,
    /// Leaves.
    pub leaves: u32,
    /// Link table.
    pub links: BuildStats,
    /// Heap events processed.
    pub events: u64,
    /// Application level.
    pub app: AppSummary,
    /// Reception outcomes, all channels.
    pub rx: RxCounts,
    /// Frames transmitted.
    pub tx_frames: u64,
    /// Total airtime, seconds.
    pub airtime_s: f64,
    /// Largest per-node airtime fraction.
    pub max_airtime_fraction: f64,
    /// Resting-channel changes.
    pub home_moves: u64,
    /// Leaves attaching to a relay other than their last (design §10.1).
    pub relay_changes: u64,
    /// Per channel.
    pub channels: Vec<ChannelSummary>,
    /// Per observer.
    pub observers: Vec<ObserverSummary>,
    /// Per node, if requested.
    pub nodes: Option<Vec<NodeSummary>>,
}

impl Sim {
    /// Summarise the run so far.
    pub fn summary(&self) -> Summary {
        let m = &self.metrics;
        let dur_us = self.now().as_micros().max(1);
        let dur_s = dur_us as f64 / 1e6;
        let mut lat = m.app.latencies_us.clone();
        lat.sort_unstable();
        let ms = |p: f64| percentile(&lat, p).expect("non-empty") as f64 / 1e3;
        let latency = (!lat.is_empty()).then(|| Latency {
            p50_ms: ms(50.0),
            p90_ms: ms(90.0),
            p99_ms: ms(99.0),
            max_ms: ms(100.0),
        });
        let (mut lost_dropped, mut unaccounted) = (DropCounts::default(), 0);
        let (mut in_flight, mut unreachable) = (0, 0);
        let mut lost_at = BTreeMap::new();
        for (_, f, reachable) in self.messages() {
            unreachable += u64::from(!reachable);
            match f {
                Fate::Dropped { reason, .. } => lost_dropped.add(reason),
                Fate::Unaccounted => unaccounted += 1,
                Fate::InFlight => in_flight += 1,
                Fate::Delivered => {}
            }
            if let Some(at) = self.lost_label(f) {
                *lost_at.entry(at).or_insert(0) += 1;
            }
        }
        let mut offers = Offers {
            waited: m.app.waited,
            still_waiting: 0,
            longest_wait_ms: m.app.longest_wait_us as f64 / 1e3,
            skipped: m.app.skipped,
        };
        for w in self.waiting() {
            offers.waited += 1;
            offers.still_waiting += 1;
            offers.longest_wait_ms = offers.longest_wait_ms.max(w.as_micros() as f64 / 1e3);
        }
        let busy_run =
            |o: &crate::metrics::Observer, c: usize| o.busy_until(c, dur_us) as f64 / dur_us as f64;
        let channels = (0..CHANNELS as usize)
            .map(|c| {
                let ch = &m.channels[c];
                let busy = (!m.observers.is_empty()).then(|| {
                    m.observers.iter().map(|o| busy_run(o, c)).sum::<f64>()
                        / m.observers.len() as f64
                });
                ChannelSummary {
                    channel: c as u8,
                    rx: ch.rx,
                    tx_frames: ch.tx_frames,
                    airtime_s: ch.airtime_us as f64 / 1e6,
                    busy_fraction: busy,
                }
            })
            .collect();
        let observers = m
            .observers
            .iter()
            .map(|o| ObserverSummary {
                node: self.label(o.node),
                busy_fraction: (0..CHANNELS as usize).map(|c| busy_run(o, c)).collect(),
            })
            .collect();
        let airtime_us: u64 = m.nodes.iter().map(|n| n.airtime_us).sum();
        let max_air = m.nodes.iter().map(|n| n.airtime_us).max().unwrap_or(0);
        let nodes = self.per_node_summary.then(|| {
            m.nodes
                .iter()
                .enumerate()
                .map(|(i, n)| NodeSummary {
                    id: i as u32,
                    label: self.label(i as u32),
                    m: n.clone(),
                    airtime_fraction: n.airtime_us as f64 / dur_us as f64,
                })
                .collect()
        });
        Summary {
            scenario: self.scenario_name.clone(),
            seed: self.seed,
            duration_s: dur_s,
            relays: self.relays(),
            leaves: self.node_count() as u32 - self.relays(),
            links: self.links().stats,
            events: m.events,
            app: AppSummary {
                originated: m.app.originated,
                offers,
                delivered: m.app.delivered,
                delivery_ratio: if m.app.originated == 0 {
                    0.0
                } else {
                    m.app.delivered as f64 / m.app.originated as f64
                },
                duplicates: m.app.duplicates,
                misdelivered: m.app.misdelivered,
                drops: m.app.drops,
                lost_dropped,
                lost_at,
                unaddressed: m.app.unaddressed,
                in_flight,
                unaccounted,
                unreachable,
                latency,
                phases: self.phases(),
            },
            rx: m.rx_total(),
            tx_frames: m.channels.iter().map(|c| c.tx_frames).sum(),
            airtime_s: airtime_us as f64 / 1e6,
            max_airtime_fraction: max_air as f64 / dur_us as f64,
            home_moves: m.nodes.iter().map(|n| n.home_moves).sum(),
            relay_changes: m.nodes.iter().map(|n| n.relay_changes).sum(),
            channels,
            observers,
            nodes,
        }
    }
}

impl Sim {
    /// Where and why a lost message was lost; `None` unless it was.
    fn lost_label(&self, f: Fate) -> Option<String> {
        match f {
            Fate::Dropped { node, reason } => {
                Some(format!("{}: dropped {reason:?}", self.label(node)))
            }
            Fate::Delivered | Fate::Unaccounted | Fate::InFlight => None,
        }
    }

    fn phases(&self) -> Vec<PhaseSummary> {
        let end = self.end().min(self.now());
        let removal = self.removal.unwrap_or(end).min(end);
        let settle = self.settle.min(removal);
        let bounds = [
            ("before settle", Instant::ZERO, settle),
            ("settled", settle, removal),
            ("after removal", removal, end),
        ];
        bounds
            .into_iter()
            .filter(|(_, from, to)| from < to)
            .map(|(phase, from, to)| {
                let mut p = PhaseSummary {
                    phase: phase.into(),
                    from_s: from.as_micros() as f64 / 1e6,
                    to_s: to.as_micros() as f64 / 1e6,
                    originated: 0,
                    delivered: 0,
                    delivery_ratio: 0.0,
                    lost_at: BTreeMap::new(),
                    in_flight: 0,
                    unaccounted: 0,
                    unreachable: 0,
                    unreachable_delivered: 0,
                    reachable_delivery_ratio: 0.0,
                };
                for (_, f, reachable) in self.messages().filter(|(t, _, _)| from <= *t && *t < to) {
                    p.originated += 1;
                    match f {
                        Fate::Delivered => p.delivered += 1,
                        Fate::Unaccounted => p.unaccounted += 1,
                        Fate::InFlight => p.in_flight += 1,
                        _ => {}
                    }
                    if !reachable {
                        p.unreachable += 1;
                        p.unreachable_delivered += u64::from(f == Fate::Delivered);
                    }
                    if let Some(at) = self.lost_label(f) {
                        *p.lost_at.entry(at).or_insert(0) += 1;
                    }
                }
                if p.originated > 0 {
                    p.delivery_ratio = p.delivered as f64 / p.originated as f64;
                }
                let reachable = p.originated - p.unreachable;
                if reachable > 0 {
                    p.reachable_delivery_ratio =
                        (p.delivered - p.unreachable_delivered) as f64 / reachable as f64;
                }
                p
            })
            .collect()
    }
}

impl Summary {
    /// JSON.
    pub fn json(&self) -> String {
        serde_json::to_string_pretty(self).expect("summary serialises")
    }

    /// Short human-readable table.
    pub fn table(&self) -> String {
        let mut s = String::new();
        let a = &self.app;
        let _ = writeln!(
            s,
            "{} (seed {}): {:.0} s simulated, {} relays, {} leaves, {} links, {} events",
            self.scenario,
            self.seed,
            self.duration_s,
            self.relays,
            self.leaves,
            self.links.links,
            self.events
        );
        let lat = a
            .latency
            .as_ref()
            .map(|l| {
                format!(
                    "latency p50 {:.0} ms, p90 {:.0} ms, max {:.0} ms",
                    l.p50_ms, l.p90_ms, l.max_ms
                )
            })
            .unwrap_or_else(|| "no latency samples".into());
        let _ = writeln!(
            s,
            "messages: {} originated, {} delivered ({:.1}%), {} dropped, {} in flight, \
             {} unaccounted; {lat}",
            a.originated,
            a.delivered,
            100.0 * a.delivery_ratio,
            a.lost_dropped.total(),
            a.in_flight,
            a.unaccounted
        );
        let o = &a.offers;
        if o.waited + o.skipped > 0 {
            let _ = writeln!(
                s,
                "pushback: {} waited for their origin to take them ({} still at the end), \
                 longest {:.0} ms; {} skipped while one waited",
                o.waited, o.still_waiting, o.longest_wait_ms, o.skipped
            );
        }
        if a.unreachable > 0 {
            let _ = writeln!(
                s,
                "to a destination unreachable from the origin: {}",
                a.unreachable
            );
        }
        if a.unaddressed > 0 {
            let _ = writeln!(
                s,
                "not sent: {} to a leaf that had not attached yet",
                a.unaddressed
            );
        }
        for (at, n) in &a.lost_at {
            let _ = writeln!(s, "  lost {n:>5} at {at}");
        }
        for p in &a.phases {
            let _ = write!(
                s,
                "{} ({:.0}-{:.0} s): {} / {} ({:.3})",
                p.phase, p.from_s, p.to_s, p.delivered, p.originated, p.delivery_ratio
            );
            for (at, n) in &p.lost_at {
                let _ = write!(s, "; lost {n} at {at}");
            }
            if p.in_flight > 0 {
                let _ = write!(s, "; {} in flight", p.in_flight);
            }
            if p.unaccounted > 0 {
                let _ = write!(s, "; {} unaccounted", p.unaccounted);
            }
            if p.unreachable > 0 {
                let _ = write!(
                    s,
                    "; {} unreachable, reachable {:.3}",
                    p.unreachable, p.reachable_delivery_ratio
                );
            }
            let _ = writeln!(s);
        }
        let _ = writeln!(
            s,
            "radio: {} frames, {:.1} s airtime (max node {:.2}%), {} home moves, {} leaf relay changes",
            self.tx_frames,
            self.airtime_s,
            100.0 * self.max_airtime_fraction,
            self.home_moves,
            self.relay_changes
        );
        let _ = writeln!(
            s,
            "\n{:>3} {:>8} {:>9} {:>9} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>6}",
            "ch",
            "frames",
            "candid.",
            "delivrd",
            "<sens",
            "capture",
            "sinr",
            "deaf",
            "halfdpx",
            "trunc",
            "busy%"
        );
        let row = |s: &mut String, label: &str, frames: u64, r: &RxCounts, busy: Option<f64>| {
            let _ = writeln!(
                s,
                "{:>3} {:>8} {:>9} {:>9} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>6}",
                label,
                frames,
                r.candidates,
                r.delivered,
                r.below_sensitivity,
                r.lost_capture,
                r.sinr_fail,
                r.deaf.total(),
                r.half_duplex,
                r.truncated,
                busy.map(|b| format!("{:.1}", 100.0 * b))
                    .unwrap_or_else(|| "-".into())
            );
        };
        for c in &self.channels {
            row(
                &mut s,
                &c.channel.to_string(),
                c.tx_frames,
                &c.rx,
                c.busy_fraction,
            );
        }
        row(&mut s, "all", self.tx_frames, &self.rx, None);
        let d = &self.rx.deaf;
        let _ = writeln!(
            s,
            "deaf by reason: retune {}, cad {}, rssi {}, tx {}, corrupt {}, absent {}; sinr of which adjacent {}",
            d.retune, d.cad, d.rssi, d.tx, d.corrupt, d.absent, self.rx.sinr_fail_adjacent
        );
        s
    }
}
