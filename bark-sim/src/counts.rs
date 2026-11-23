//! What each core reports in its traces, counted per node
//! (`metrics::CoreCounts`) and for the sleepy nodes (`metrics::SleepMetrics`).

use crate::sim::{key, Sim};
use bark_proto::Trace;

impl Sim {
    /// Count a trace of `node`'s core.
    pub(crate) fn count_trace(&mut self, node: u32, t: &Trace) {
        // The core's instant is on its own clock.
        let busy_wait = match *t {
            Trace::ChannelBusy { until, .. } => self.clocks[node as usize]
                .true_time(until)
                .since(self.now)
                .as_micros(),
            _ => 0,
        };
        match *t {
            Trace::ReturnHeld { msg_id, .. } => {
                self.returns_held.insert((node, msg_id.0));
                self.metrics.app.returns.held += 1;
            }
            Trace::ReturnedToParent {
                msg_id,
                sleepy: true,
                ..
            } => self.return_ended(node, msg_id, None),
            _ => {}
        }
        let c = &mut self.metrics.nodes[node as usize].core;
        match *t {
            Trace::HomeChannelMoved { from, to } if !self.was_booting => {
                c.home_moves += 1;
                if node < self.relays() {
                    let t = self.now.as_micros() as f64 / 1e6;
                    let m = (t, node, from.index(), to.index());
                    self.metrics.research.moves.push(m);
                }
            }
            Trace::HeartbeatSkipped { .. } => c.heartbeats_skipped += 1,
            Trace::HeartbeatDeferred { .. } => c.heartbeats_deferred += 1,
            Trace::Refused { reason, .. } => c.refused.add(reason),
            Trace::TurnedAway { .. } => c.turned_away += 1,
            Trace::HandOverDeferred { .. } => c.handovers_deferred += 1,
            Trace::ChannelBusy { .. } => {
                c.lbt_gave_up += 1;
                c.busy_wait_us += busy_wait;
            }
            Trace::Demoted { .. } => c.demoted += 1,
            Trace::AnchorCheck => {
                c.anchor_checks += 1;
                if self.fixing && self.now >= self.settle {
                    self.metrics.sleep.fix_checks += 1;
                }
            }
            Trace::AnchorWatch {
                heard,
                offset_us,
                listened,
            } if self.now >= self.settle => {
                let s = &mut self.metrics.sleep;
                s.watches[usize::from(!heard)] += 1;
                s.watch_listen_us += listened.as_micros();
                if heard && offset_us < 0 {
                    s.watch_early += 1;
                }
                if heard {
                    s.watch_earliest_us = s.watch_earliest_us.min(offset_us as i64);
                }
            }
            Trace::Delivered { .. } => c.delivered += 1,
            Trace::DeliveryWaits { tried, .. } => {
                if tried {
                    c.deliveries_failed += 1;
                } else {
                    c.deliveries_late += 1;
                }
            }
            Trace::Forwarded { hops, .. } => {
                c.handed_over += 1;
                c.originated_out += u64::from(hops == 1);
            }
            Trace::HandOverFailed {
                msg_id,
                to,
                moves_on,
            } => {
                c.failed += 1;
                let had = self
                    .relay_index
                    .get(&key(to))
                    .is_some_and(|&r| self.relays[r as usize].knows(msg_id));
                c.failed_taken += u64::from(had);
                c.moved_on += u64::from(moves_on);
                c.moved_on_taken += u64::from(moves_on && had);
            }
            Trace::Copy {
                returned,
                merged,
                seen,
                ..
            } => {
                if returned {
                    c.returns_in += 1;
                } else {
                    c.copies += 1;
                    c.merged += u64::from(merged);
                    c.seen += u64::from(seen);
                }
            }
            Trace::Seen { .. } => c.seen_acted += 1,
            Trace::ReturnedToParent { .. } => {
                c.handed_over += 1;
                c.returned += 1;
            }
            _ => {}
        }
    }
}
