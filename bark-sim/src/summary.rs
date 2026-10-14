//! End-of-run summary: JSON for machines, a short table for people.
//!
//! The summary is a pure function of the run (no wall-clock time), so two
//! runs of one scenario produce byte-identical JSON.

use crate::links::BuildStats;
use crate::metrics::{percentile, NodeMetrics, RxCounts};
use crate::sim::Sim;
use bark_proto::radio::CHANNELS;
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
    /// Accepted by the origin's core.
    pub accepted: u64,
    /// Delivered to the destination's application (distinct).
    pub delivered: u64,
    /// `delivered / originated`, 0 if nothing was originated.
    pub delivery_ratio: f64,
    /// Duplicate deliveries.
    pub duplicates: u64,
    /// Deliveries to the wrong node.
    pub misdelivered: u64,
    /// End-to-end ACKs reported at origins.
    pub acks: u64,
    /// NACKs (failures) reported at origins, by reason code.
    pub nacks: BTreeMap<String, u64>,
    /// Origination-to-delivery latency.
    pub latency: Option<Latency>,
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
                accepted: m.app.accepted,
                delivered: m.app.delivered,
                delivery_ratio: if m.app.originated == 0 {
                    0.0
                } else {
                    m.app.delivered as f64 / m.app.originated as f64
                },
                duplicates: m.app.duplicates,
                misdelivered: m.app.misdelivered,
                acks: m.app.acks,
                nacks: m
                    .app
                    .nacks
                    .iter()
                    .map(|(k, v)| (format!("{k:#04x}"), *v))
                    .collect(),
                latency,
            },
            rx: m.rx_total(),
            tx_frames: m.channels.iter().map(|c| c.tx_frames).sum(),
            airtime_s: airtime_us as f64 / 1e6,
            max_airtime_fraction: max_air as f64 / dur_us as f64,
            home_moves: m.nodes.iter().map(|n| n.home_moves).sum(),
            channels,
            observers,
            nodes,
        }
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
            "messages: {} originated, {} delivered ({:.1}%), {} acks, {} nacks; {lat}",
            a.originated,
            a.delivered,
            100.0 * a.delivery_ratio,
            a.acks,
            a.nacks.values().sum::<u64>()
        );
        let _ = writeln!(
            s,
            "radio: {} frames, {:.1} s airtime (max node {:.2}%), {} home moves",
            self.tx_frames,
            self.airtime_s,
            100.0 * self.max_airtime_fraction,
            self.home_moves
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
