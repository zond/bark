//! End-of-run summary: JSON for machines, a short table for people.
//!
//! The summary is a pure function of the run (no wall-clock time), so two
//! runs of one scenario produce byte-identical JSON.

use crate::links::{BuildStats, Site};
use crate::metrics::{
    percentile, DropCounts, HopPlaceRefusals, NodeMetrics, RxCounts, SleepMetrics, TypeAirtime,
};
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
    /// The same airtime by packet type.
    pub airtime_by_type: TypeAirtime,
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
    /// Metres east of the origin.
    pub x: f64,
    /// Metres north of the origin.
    pub y: f64,
    /// Where its antenna is.
    pub site: Site,
    /// The channel it rests on at the end: a relay's home channel.
    pub channel: Option<u8>,
    /// For a leaf, the relay it is attached to at the end.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relay: Option<u32>,
    /// Counters.
    #[serde(flatten)]
    pub m: NodeMetrics,
    /// Airtime as a fraction of the run (the duty-cycle cap is 0.10).
    pub airtime_fraction: f64,
}

/// How soon relays heard from leaves that came back into reach.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ReturnsSummary {
    /// Leaves that came back into reach.
    pub returned: u64,
    /// Of those, ones a relay heard an ATTACH from before they went dark
    /// again or the run ended.
    pub known: u64,
    /// Back in reach to known, seconds, of those known.
    pub p50_s: Option<f64>,
    /// See `p50_s`.
    pub p90_s: Option<f64>,
    /// See `p50_s`.
    pub max_s: Option<f64>,
}

/// What sleeping does, after the settle time (`metrics::SleepMetrics`).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SleepSummary {
    /// Listening time per sleepy node and hour, seconds: the time its
    /// radio was not asleep, mean, median and 90th percentile over the
    /// sleepy nodes.
    pub listen_s_per_h: [f64; 3],
    /// The counters.
    #[serde(flatten)]
    pub m: SleepMetrics,
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
    /// Leaves attaching to a relay other than their last (spec "Relay choice").
    pub relay_changes: u64,
    /// Dark leaves coming back, if any went dark.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub returns: Option<ReturnsSummary>,
    /// Sleepy nodes, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sleep: Option<SleepSummary>,
    /// Refusals for room, and the hop places free at them.
    #[serde(skip_serializing_if = "hop_places_none")]
    pub hop_places: HopPlaceRefusals,
    /// Frames addressed to a node that is not sleepy, by fate
    /// (`metrics::Addressed`).
    pub addressed: crate::metrics::Addressed,
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
                    airtime_by_type: ch.airtime_by_type,
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
                    x: self.xy(i as u32).x,
                    y: self.xy(i as u32).y,
                    site: self.site(i as u32),
                    channel: self.resting(i as u32).map(|c| c.index()),
                    relay: (i as u32 >= self.relays())
                        .then(|| self.attached_relay(i as u32))
                        .flatten(),
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
            returns: (m.returns.returned > 0).then(|| {
                let mut l = m.returns.latencies_us.clone();
                l.sort_unstable();
                let s = |p: f64| percentile(&l, p).map(|us| us as f64 / 1e6);
                ReturnsSummary {
                    returned: m.returns.returned,
                    known: m.returns.known,
                    p50_s: s(50.0),
                    p90_s: s(90.0),
                    max_s: s(100.0),
                }
            }),
            sleep: self.sleep_summary(),
            hop_places: m.hop_places,
            addressed: m.addressed,
            channels,
            observers,
            nodes,
        }
    }
}

impl Sim {
    /// Listening time of the sleepy nodes after the settle time, and what
    /// sleeping did.
    fn sleep_summary(&self) -> Option<SleepSummary> {
        let n = self.relays() as usize;
        let window = self
            .now()
            .as_micros()
            .saturating_sub(self.settle.as_micros());
        if self.node_count() <= n || window == 0 {
            return None;
        }
        let per_h = |asleep: u64| window.saturating_sub(asleep) as f64 / window as f64 * 3600.0;
        let mut listen: Vec<u64> = self.metrics.nodes[n..]
            .iter()
            .map(|m| (per_h(m.asleep_us) * 1e3) as u64)
            .collect();
        listen.sort_unstable();
        let mean = listen.iter().sum::<u64>() as f64 / listen.len() as f64 / 1e3;
        let at = |p: f64| percentile(&listen, p).unwrap_or(0) as f64 / 1e3;
        Some(SleepSummary {
            listen_s_per_h: [mean, at(50.0), at(90.0)],
            m: self.metrics.sleep,
        })
    }

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
        if let Some(z) = &self.sleep {
            let m = &z.m;
            let share = |x: u64| x as f64 / m.anchored.max(1) as f64;
            let _ = writeln!(
                s,
                "sleepy nodes listen {:.1} s an hour (median {:.1}, p90 {:.1}); DATA to them received {}, lost asleep {}, lost otherwise {}; delivery TTL ran out at the anchor {} (node in reach) {} (dark), elsewhere {}; anchor on another channel {:.4}, anchor not hearing {:.4}",
                z.listen_s_per_h[0],
                z.listen_s_per_h[1],
                z.listen_s_per_h[2],
                m.to_sleepy[0],
                m.to_sleepy[1],
                m.to_sleepy[2],
                m.queue_expired[0],
                m.queue_expired[1],
                m.queue_expired[2],
                share(m.off_channel),
                share(m.unheard)
            );
        }
        let h = &self.hop_places;
        if h.refusals > 0 {
            let _ = writeln!(
                s,
                "refused for room: {}, {} with a hop place free for another message the sender held for it; waiting after them {:.1} s, {:.1} s after those",
                h.refusals,
                h.with_place,
                h.wait_us as f64 / 1e6,
                h.wait_with_place_us as f64 / 1e6
            );
        }
        if let Some(r) = &self.returns {
            let f = |x: Option<f64>| x.map_or_else(|| "-".into(), |x| format!("{x:.1}"));
            let _ = writeln!(
                s,
                "leaves back in reach: {}, heard by a relay {}; after s p50 {} p90 {} max {}",
                r.returned,
                r.known,
                f(r.p50_s),
                f(r.p90_s),
                f(r.max_s)
            );
        }
        let _ = writeln!(
            s,
            "\n{:>3} {:>8} {:>9} {:>9} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>6}",
            "ch",
            "frames",
            "candid.",
            "delivrd",
            "noise",
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
        let _ = writeln!(
            s,
            "error curve against threshold: lost {} it passes, received {} it fails",
            self.rx.curve_lost_threshold_passed, self.rx.curve_received_threshold_failed
        );
        s
    }
}

fn hop_places_none(h: &HopPlaceRefusals) -> bool {
    h.refusals == 0
}
