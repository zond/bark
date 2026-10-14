//! Counters, per node and per channel, with drop reasons as first-class data.
//!
//! Reception accounting: every time a transmission on channel `c` reaches a
//! node whose resting channel is `c` (it would have heard the packet had
//! nothing gone wrong), that is a *candidate*, and each candidate ends in
//! exactly one outcome: delivered, below sensitivity, lost capture, SINR
//! failure, truncated, deaf for a stated reason, or half duplex. Nodes
//! resting on other channels are not candidates: a packet sent elsewhere is
//! not a loss.

use crate::phy::Deaf;
use bark_proto::radio::CHANNELS;
use serde::Serialize;
use std::collections::BTreeMap;

const NCH: usize = CHANNELS as usize;

/// How one candidate reception ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Delivered to the core as `Event::Received`.
    Delivered,
    /// Below sensitivity at the receiver.
    BelowSensitivity,
    /// Another packet held the receiver's lock (or a stronger one stole it
    /// during the preamble).
    LostCapture,
    /// Overlapping interference exceeded the capture margin; `adjacent` if
    /// adjacent-channel leakage was the larger part of it.
    SinrFail {
        /// Adjacent channels dominated the interference.
        adjacent: bool,
    },
    /// The transmitter vanished mid-packet (scripted removal).
    Truncated,
    /// The receiver was not listening; [`Deaf::HalfDuplex`] is counted apart.
    Deaf(Deaf),
}

/// Deaf counters by reason.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct DeafCounts {
    /// Retune or mode switch.
    pub retune: u64,
    /// CAD in progress.
    pub cad: u64,
    /// RSSI sample in progress.
    pub rssi: u64,
    /// TX turnaround.
    pub tx: u64,
    /// Demodulating a corrupted packet.
    pub corrupt: u64,
    /// Out of radio reach (dark leaf).
    pub absent: u64,
}

impl DeafCounts {
    fn add(&mut self, r: Deaf) {
        match r {
            Deaf::Retune => self.retune += 1,
            Deaf::Cad => self.cad += 1,
            Deaf::Rssi => self.rssi += 1,
            Deaf::Tx => self.tx += 1,
            Deaf::Corrupt => self.corrupt += 1,
            Deaf::Absent => self.absent += 1,
            Deaf::HalfDuplex => unreachable!("counted as half_duplex"),
        }
    }

    /// Total.
    pub fn total(&self) -> u64 {
        self.retune + self.cad + self.rssi + self.tx + self.corrupt + self.absent
    }

    fn merge(&mut self, o: &DeafCounts) {
        self.retune += o.retune;
        self.cad += o.cad;
        self.rssi += o.rssi;
        self.tx += o.tx;
        self.corrupt += o.corrupt;
        self.absent += o.absent;
    }
}

/// Reception counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct RxCounts {
    /// Candidate receptions.
    pub candidates: u64,
    /// Delivered.
    pub delivered: u64,
    /// Below sensitivity.
    pub below_sensitivity: u64,
    /// Lost capture.
    pub lost_capture: u64,
    /// SINR failures, all.
    pub sinr_fail: u64,
    /// Of `sinr_fail`, those where adjacent-channel leakage dominated.
    pub sinr_fail_adjacent: u64,
    /// Transmitter removed mid-packet.
    pub truncated: u64,
    /// Receiver deaf, by reason.
    pub deaf: DeafCounts,
    /// Receiver transmitting.
    pub half_duplex: u64,
}

impl RxCounts {
    fn add(&mut self, o: Outcome) {
        match o {
            Outcome::Delivered => self.delivered += 1,
            Outcome::BelowSensitivity => self.below_sensitivity += 1,
            Outcome::LostCapture => self.lost_capture += 1,
            Outcome::SinrFail { adjacent } => {
                self.sinr_fail += 1;
                self.sinr_fail_adjacent += adjacent as u64;
            }
            Outcome::Truncated => self.truncated += 1,
            Outcome::Deaf(Deaf::HalfDuplex) => self.half_duplex += 1,
            Outcome::Deaf(r) => self.deaf.add(r),
        }
    }

    /// Outcomes decided so far; equals `candidates` once nothing is in flight.
    pub fn decided(&self) -> u64 {
        self.delivered
            + self.below_sensitivity
            + self.lost_capture
            + self.sinr_fail
            + self.truncated
            + self.deaf.total()
            + self.half_duplex
    }

    /// Add another counter set.
    pub fn merge(&mut self, o: &RxCounts) {
        self.candidates += o.candidates;
        self.delivered += o.delivered;
        self.below_sensitivity += o.below_sensitivity;
        self.lost_capture += o.lost_capture;
        self.sinr_fail += o.sinr_fail;
        self.sinr_fail_adjacent += o.sinr_fail_adjacent;
        self.truncated += o.truncated;
        self.deaf.merge(&o.deaf);
        self.half_duplex += o.half_duplex;
    }
}

/// Per-node counters.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct NodeMetrics {
    /// Receptions at this node.
    pub rx: RxCounts,
    /// Frames transmitted.
    pub tx_frames: u64,
    /// Time on air, µs.
    pub airtime_us: u64,
    /// CAD excursions.
    pub cad: u64,
    /// RSSI excursions.
    pub rssi: u64,
    /// Resting-channel changes after the first `Listen`.
    pub home_moves: u64,
    /// For a leaf: times it attached to a relay other than the one it was
    /// last attached to (design §10.1).
    pub relay_changes: u64,
    /// Messages originated here by the traffic model.
    pub originated: u64,
    /// Own messages whose first hop acknowledged them.
    pub sent: u64,
    /// Own messages that failed locally.
    pub failed: u64,
    /// `AppEvent::Message` at this node.
    pub messages_in: u64,
    /// Messages this node dropped, by reason.
    pub drops: DropCounts,
}

/// Why a relay dropped a message: the core's own reasons, reported in
/// `Trace::Dropped`. The protocol sends no failure notice (roadmap D10),
/// so a lost message is only visible where it died.
pub use bark_proto::DropReason;

/// Drop counters by reason.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct DropCounts {
    /// No neighbour left at the origin (§9.2.5).
    pub no_route: u64,
    /// The hop cap (§9.2.6).
    pub hop_cap: u64,
    /// The message's record was lost while its packet waited (§9.2.2).
    pub record_lost: u64,
    /// The send queue was full (§9.3).
    pub send_queue_full: u64,
    /// The duty-cycle budget (§9.4).
    pub budget: u64,
    /// The return to the parent failed (§9.2.5).
    pub return_failed: u64,
    /// A send straight back failed (§9.3).
    pub bounce_failed: u64,
    /// For a relay that is gone (§9.2.7).
    pub relay_gone: u64,
    /// The delivery TTL ran out (§10.4).
    pub queue_expired: u64,
    /// Displaced from a full delivery queue (§10.4).
    pub queue_displaced: u64,
    /// The payload was too long (§14).
    pub payload_too_long: u64,
}

impl DropCounts {
    /// Count one drop.
    pub fn add(&mut self, r: DropReason) {
        let c = match r {
            DropReason::NoRoute => &mut self.no_route,
            DropReason::HopCap => &mut self.hop_cap,
            DropReason::RecordLost => &mut self.record_lost,
            DropReason::SendQueueFull => &mut self.send_queue_full,
            DropReason::Budget => &mut self.budget,
            DropReason::ReturnFailed => &mut self.return_failed,
            DropReason::BounceFailed => &mut self.bounce_failed,
            DropReason::RelayGone => &mut self.relay_gone,
            DropReason::QueueExpired => &mut self.queue_expired,
            DropReason::QueueDisplaced => &mut self.queue_displaced,
            DropReason::PayloadTooLong => &mut self.payload_too_long,
        };
        *c += 1;
    }

    /// Total.
    pub fn total(&self) -> u64 {
        self.no_route
            + self.hop_cap
            + self.record_lost
            + self.send_queue_full
            + self.budget
            + self.return_failed
            + self.bounce_failed
            + self.relay_gone
            + self.queue_expired
            + self.queue_displaced
            + self.payload_too_long
    }
}

/// Per-channel counters.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ChannelMetrics {
    /// Receptions on this channel.
    pub rx: RxCounts,
    /// Frames transmitted on this channel.
    pub tx_frames: u64,
    /// Time on air on this channel, µs.
    pub airtime_us: u64,
}

/// Application-level counters, matched through a tag the traffic model
/// writes into each payload (the core never looks inside a payload).
/// End-to-end delivery is known only by observing `AppEvent::Message` at
/// the destination; nothing in the protocol reports it back.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct AppMetrics {
    /// Messages the traffic model asked a core to send.
    pub originated: u64,
    /// `AppEvent::Accepted`.
    pub accepted: u64,
    /// Distinct messages that reached their destination's application.
    pub delivered: u64,
    /// Further copies of already delivered messages.
    pub duplicates: u64,
    /// Messages that reached a node other than their destination.
    pub misdelivered: u64,
    /// Messages whose first hop acknowledged them, reported at origins.
    pub sent: u64,
    /// Local failures reported at origins, by reason.
    pub failed: BTreeMap<String, u64>,
    /// Messages the traffic model did not send because their destination
    /// leaf had never attached, so no sender could know its address (§9.1).
    pub unaddressed: u64,
    /// Drops reported by relays, by reason (events, not messages: two
    /// copies of one message may be dropped in two places).
    pub drops: DropCounts,
    /// Origination to destination application, µs, in arrival order.
    #[serde(skip)]
    pub latencies_us: Vec<u64>,
}

/// Busy-time tracker for one observer node: the fraction of time the
/// in-channel power at the node (with adjacent leakage after rejection)
/// exceeds the noise floor by the protocol's busy threshold. This is the
/// ground truth the core's sampled busy estimate approximates.
#[derive(Clone, Debug, Default)]
pub struct Observer {
    /// Node index.
    pub node: u32,
    /// Busy now, per channel.
    pub busy: [bool; NCH],
    /// Since when, per channel, µs.
    pub since: [u64; NCH],
    /// Accumulated busy time, µs.
    pub total_us: [u64; NCH],
    /// Accumulated busy time at the last sample, µs.
    pub at_sample_us: [u64; NCH],
}

impl Observer {
    /// Set channel `c` busy or idle at `now_us`.
    pub fn set(&mut self, c: usize, busy: bool, now_us: u64) {
        if self.busy[c] == busy {
            return;
        }
        if self.busy[c] {
            self.total_us[c] += now_us - self.since[c];
        }
        self.busy[c] = busy;
        self.since[c] = now_us;
    }

    /// Busy time up to `now_us` on channel `c`.
    pub fn busy_until(&self, c: usize, now_us: u64) -> u64 {
        self.total_us[c]
            + if self.busy[c] {
                now_us - self.since[c]
            } else {
                0
            }
    }
}

/// One row of the time series.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SampleRow {
    /// End of the interval, seconds.
    pub t_s: f64,
    /// Channel.
    pub channel: u8,
    /// Mean busy fraction over observers during the interval.
    pub busy_fraction: f64,
    /// Frames started on this channel during the interval.
    pub tx_frames: u64,
    /// Receptions delivered on this channel during the interval.
    pub delivered: u64,
    /// Messages originated so far.
    pub app_originated: u64,
    /// Messages delivered end to end so far.
    pub app_delivered: u64,
}

/// All counters of a run.
#[derive(Clone, Debug, Default)]
pub struct Metrics {
    /// Per node, indexed like the simulator's nodes.
    pub nodes: Vec<NodeMetrics>,
    /// Per channel.
    pub channels: [ChannelMetrics; NCH],
    /// Application level.
    pub app: AppMetrics,
    /// Busy-time observers.
    pub observers: Vec<Observer>,
    /// Time series.
    pub samples: Vec<SampleRow>,
    /// Heap events processed.
    pub events: u64,
    last_sample: SampleMark,
}

#[derive(Clone, Debug, Default)]
struct SampleMark {
    t_us: u64,
    tx: [u64; NCH],
    delivered: [u64; NCH],
}

impl Metrics {
    /// Counters for `n` nodes.
    pub fn new(n: usize) -> Metrics {
        Metrics {
            nodes: vec![NodeMetrics::default(); n],
            ..Metrics::default()
        }
    }

    /// A candidate reception at `node` on `channel`.
    pub fn candidate(&mut self, node: u32, channel: u8) {
        self.nodes[node as usize].rx.candidates += 1;
        self.channels[channel as usize].rx.candidates += 1;
    }

    /// The outcome of a candidate reception.
    pub fn outcome(&mut self, node: u32, channel: u8, o: Outcome) {
        self.nodes[node as usize].rx.add(o);
        self.channels[channel as usize].rx.add(o);
    }

    /// Reception totals over all channels.
    pub fn rx_total(&self) -> RxCounts {
        let mut t = RxCounts::default();
        for c in &self.channels {
            t.merge(&c.rx);
        }
        t
    }

    /// Close a time-series interval at `now_us`.
    pub fn sample(&mut self, now_us: u64) {
        let dt = now_us.saturating_sub(self.last_sample.t_us);
        for c in 0..NCH {
            let busy = if self.observers.is_empty() || dt == 0 {
                0.0
            } else {
                let mut sum = 0.0;
                for o in &mut self.observers {
                    let b = o.busy_until(c, now_us);
                    sum += (b - o.at_sample_us[c]) as f64 / dt as f64;
                    o.at_sample_us[c] = b;
                }
                sum / self.observers.len() as f64
            };
            let ch = &self.channels[c];
            self.samples.push(SampleRow {
                t_s: now_us as f64 / 1e6,
                channel: c as u8,
                busy_fraction: busy,
                tx_frames: ch.tx_frames - self.last_sample.tx[c],
                delivered: ch.rx.delivered - self.last_sample.delivered[c],
                app_originated: self.app.originated,
                app_delivered: self.app.delivered,
            });
            self.last_sample.tx[c] = ch.tx_frames;
            self.last_sample.delivered[c] = ch.rx.delivered;
        }
        self.last_sample.t_us = now_us;
    }

    /// The time series as CSV.
    pub fn csv(&self) -> String {
        let mut s = String::from(
            "t_s,channel,busy_fraction,tx_frames,delivered,app_originated,app_delivered\n",
        );
        for r in &self.samples {
            s.push_str(&format!(
                "{},{},{:.6},{},{},{},{}\n",
                r.t_s,
                r.channel,
                r.busy_fraction,
                r.tx_frames,
                r.delivered,
                r.app_originated,
                r.app_delivered
            ));
        }
        s
    }
}

/// Nearest-rank percentile of a sorted slice.
pub fn percentile(sorted: &[u64], p: f64) -> Option<u64> {
    if sorted.is_empty() {
        return None;
    }
    let rank = ((p / 100.0) * sorted.len() as f64).ceil().max(1.0) as usize;
    Some(sorted[rank.min(sorted.len()) - 1])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcomes_balance_candidates() {
        let mut m = Metrics::new(2);
        let outcomes = [
            Outcome::Delivered,
            Outcome::BelowSensitivity,
            Outcome::LostCapture,
            Outcome::SinrFail { adjacent: true },
            Outcome::Truncated,
            Outcome::Deaf(Deaf::Cad),
            Outcome::Deaf(Deaf::HalfDuplex),
        ];
        for o in outcomes {
            m.candidate(1, 4);
            m.outcome(1, 4, o);
        }
        let t = m.rx_total();
        assert_eq!(t.candidates, 7);
        assert_eq!(t.decided(), 7);
        assert_eq!(t.half_duplex, 1);
        assert_eq!(t.deaf.cad, 1);
        assert_eq!(t.sinr_fail_adjacent, 1);
        assert_eq!(m.nodes[1].rx, t);
    }

    #[test]
    fn percentiles() {
        let v = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        assert_eq!(percentile(&v, 50.0), Some(5));
        assert_eq!(percentile(&v, 90.0), Some(9));
        assert_eq!(percentile(&v, 100.0), Some(10));
        assert_eq!(percentile(&[], 50.0), None);
    }
}
