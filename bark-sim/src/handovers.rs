//! How hand-overs go (`metrics::Pressure`, `metrics::Holds`,
//! `metrics::HopPlaceRefusals`, `metrics::RecordsLost`): each message's
//! offers and their flaps, holds on a neighbour after a failure,
//! demotions until the neighbour answers, refusals for want of room, and
//! records that made way for another. Counting only: nothing here changes
//! a draw or a decision.

use crate::sim::{key, Sim};
use bark_proto::wire::Packet;
use bark_proto::{DropReason, Event, Frame, Instant, MsgId, NodeId, Trace};
use std::collections::{HashMap, HashSet};

/// What is followed of the hand-overs between events.
#[derive(Default)]
pub(crate) struct HandOverWatch {
    /// Messages held on a neighbour after a failed hand-over, by node and
    /// message ID (`metrics::Holds`).
    holds: HashMap<(u32, u32), Hold>,
    /// Every hand-over that ended, by message tag: the relay and the
    /// neighbour offered, in order (`metrics::Pressure`).
    offer_log: HashMap<u32, OfferLog>,
    /// The refuser of each relay origin's own message refused last, by
    /// origin and message tag, until its next hand-over ends
    /// (`metrics::Pressure::own_refused`).
    own_refused_by: HashMap<(u32, u32), u64>,
    /// Demotions not yet followed by an answer from the neighbour, by
    /// relay and the neighbour's key: when.
    demoted_at: HashMap<(u32, u64), Instant>,
    /// The relay DATA a relay is being handed now: (relay, message,
    /// sender's ID, count), for `metrics::HopPlaceRefusals`.
    rx_data: Option<(u32, u32, u64, u32)>,
    /// (relay, message) for every record a relay has kept, for
    /// `metrics::RecordsLost`.
    recorded: HashSet<(u32, u32)>,
    /// Refused messages not yet sent again: (sender, message) to the
    /// refusal's time and whether a hop place was free for another.
    refused_at: HashMap<(u32, u32), (Instant, bool)>,
}

/// A message held on a neighbour after a failed hand-over
/// (`metrics::Holds`).
#[derive(Clone, Copy)]
struct Hold {
    /// The neighbour's ID key.
    to: u64,
    /// Failed hand-overs to it in a row.
    failures: u8,
    /// When the first failed.
    since: Instant,
}

impl Hold {
    fn new(to: u64, since: Instant) -> Hold {
        Hold {
            to,
            failures: 0,
            since,
        }
    }
}

/// The hand-overs of one message that ended, at any relay
/// (`Sim::note_offer`).
#[derive(Default)]
pub(crate) struct OfferLog {
    /// Relay and neighbour of each, in order.
    pub(crate) to: Vec<(u32, u64)>,
    /// Offers to a neighbour, then another, then the first again, at one
    /// relay.
    pub(crate) flaps: u32,
}

impl OfferLog {
    /// Distinct relay and neighbour pairs offered.
    pub(crate) fn next_hops(&self) -> usize {
        let mut v = self.to.clone();
        v.sort_unstable();
        v.dedup();
        v.len()
    }
}

impl Sim {
    /// `node`'s core is about to be handed `ev`: a relay DATA to a relay
    /// is noted, and counted if the relay had kept a record of its message
    /// that made way for another.
    pub(crate) fn handover_before(&mut self, node: u32, ev: &Event<'_>) {
        if node < self.relays() {
            self.handovers.rx_data = match ev {
                Event::Received { bytes, .. } => match Packet::decode(bytes) {
                    Ok(Packet::Data(d)) if d.hops > 0 => {
                        Some((node, d.msg_id().0, key(d.src_hop), d.hops))
                    }
                    _ => None,
                },
                _ => None,
            };
        }
        if let Some((at, id, _, _)) = self.handovers.rx_data.filter(|r| r.0 == node) {
            let w = &self.handovers;
            if !self.relays[at as usize].knows(MsgId(id)) && w.recorded.contains(&(at, id)) {
                let tag = self.msg_tag(id);
                let l = &mut self.metrics.records_lost;
                l.packets += 1;
                l.tags.extend(tag);
            }
        }
    }

    /// `node`'s core has handled the event of [`Sim::handover_before`]:
    /// a record it keeps of the DATA's message is noted.
    pub(crate) fn handover_after(&mut self, node: u32) {
        if let Some((at, id, _, _)) = self.handovers.rx_data.filter(|r| r.0 == node) {
            if self.relays[node as usize].knows(MsgId(id)) {
                self.handovers.recorded.insert((at, id));
            }
        }
    }

    /// A trace of `node`'s core.
    pub(crate) fn handover_trace(&mut self, node: u32, t: &Trace) {
        match *t {
            Trace::Refused { msg_id, reason } => self.note_refusal(node, msg_id, reason),
            Trace::TurnedAway { msg_id, by } => {
                self.note_offer(node, msg_id, by);
                self.note_own_refusal(node, msg_id, by);
                if let Some(held) = self.hold_ended(node, msg_id) {
                    self.metrics.holds.ended_refused += 1;
                    self.metrics.holds.held_us.push(held);
                }
            }
            Trace::Demoted { neighbour } => {
                let now = self.now;
                if let std::collections::hash_map::Entry::Vacant(e) =
                    self.handovers.demoted_at.entry((node, key(neighbour)))
                {
                    e.insert(now);
                    self.metrics.pressure.demotions += 1;
                }
            }
            Trace::Answered { neighbour } => {
                if let Some(t) = self.handovers.demoted_at.remove(&(node, key(neighbour))) {
                    let s = self.now.since(t).as_micros() as f64 / 1e6;
                    self.metrics.pressure.answered_after(s);
                }
            }
            Trace::InLine {
                wait,
                service,
                at_cap,
                ..
            } => self.metrics.pressure.in_line(wait, service, at_cap),
            Trace::WayWait { wait, refused } if self.now >= self.settle => {
                self.metrics.pressure.way_wait(wait, refused)
            }
            Trace::Silent { owed, .. } => {
                let p = &mut self.metrics.pressure;
                if owed {
                    p.silent_owed += 1;
                } else {
                    p.silent_budget += 1;
                }
            }
            Trace::Forwarded {
                msg_id,
                to,
                attempts,
                ..
            }
            | Trace::ReturnedToParent {
                msg_id,
                to,
                attempts,
            } => {
                self.note_offer(node, msg_id, to);
                self.metrics.taken_at_attempt(attempts);
                self.hold_taken(node, msg_id, to);
            }
            Trace::HandOverFailed {
                msg_id,
                to,
                moves_on,
            } => {
                self.note_offer(node, msg_id, to);
                let now = self.now;
                let holds = &mut self.handovers.holds;
                let hold = holds
                    .entry((node, msg_id.0))
                    .and_modify(|h| {
                        if h.to != key(to) {
                            *h = Hold::new(key(to), now);
                        }
                    })
                    .or_insert_with(|| Hold::new(key(to), now));
                hold.failures = hold.failures.saturating_add(1);
                if moves_on {
                    let failures = hold.failures;
                    let held = now.since(hold.since);
                    holds.remove(&(node, msg_id.0));
                    self.metrics.holds.moved_on_after(failures);
                    self.metrics.holds.held_us.push(held.as_micros());
                }
            }
            Trace::Seen { msg_id, by } => {
                self.note_offer(node, msg_id, by);
                if let Some(held) = self.hold_ended(node, msg_id) {
                    self.metrics.holds.ended_seen += 1;
                    self.metrics.holds.held_us.push(held);
                }
            }
            _ => {}
        }
    }

    /// `node` dropped `msg_id`: a hold of it ends.
    pub(crate) fn handover_dropped(&mut self, node: u32, msg_id: MsgId) {
        if let Some(held) = self.hold_ended(node, msg_id) {
            self.metrics.holds.ended_dropped += 1;
            self.metrics.holds.held_us.push(held);
        }
    }

    /// `node` sends `f`: if it is a DATA of a message refused to it, the
    /// wait since ends.
    pub(crate) fn handover_transmit(&mut self, node: u32, f: &Frame) {
        if node >= self.relays() || self.handovers.refused_at.is_empty() {
            return;
        }
        let Ok(Packet::Data(d)) = Packet::decode(f.payload()) else {
            return;
        };
        if let Some((t, with)) = self.handovers.refused_at.remove(&(node, d.msg_id().0)) {
            let w = self.now.since(t).as_micros();
            let h = &mut self.metrics.hop_places;
            h.wait_us += w;
            if with {
                h.wait_with_place_us += w;
            }
        }
    }

    /// The hand-overs of the message `tag` that ended: how many, to how
    /// many distinct relay and neighbour pairs, and the flaps among them.
    pub(crate) fn offers_of(&self, tag: u32) -> (usize, usize, u32) {
        self.handovers
            .offer_log
            .get(&tag)
            .map_or((0, 0, 0), |l| (l.to.len(), l.next_hops(), l.flaps))
    }

    /// A hand-over of `msg_id` from `node` to `to` ended, taken, refused,
    /// failed or seen: counted per message, and a flap noted when `node`
    /// offers it to a neighbour, then another, then the first again.
    fn note_offer(&mut self, node: u32, msg_id: MsgId, to: NodeId) {
        let Some(tag) = self.msg_tag(msg_id.0) else {
            return;
        };
        let to = key(to);
        let w = &mut self.handovers;
        if let Some(by) = w.own_refused_by.remove(&(node, tag)) {
            self.metrics.pressure.own_round += u64::from(by != to);
        }
        let log = w.offer_log.entry(tag).or_default();
        let mut here = log.to.iter().rev().filter(|(n, _)| *n == node);
        if let Some(&(_, last)) = here.next() {
            if last != to && here.any(|&(_, k)| k == to) {
                log.flaps += 1;
                self.metrics.pressure.flaps += 1;
            }
        }
        log.to.push((node, to));
    }

    /// `by` refused `msg_id` at `node`: counted if it is the own message
    /// of a relay origin, not yet past its first hop, whose next
    /// hand-over [`Sim::note_offer`] then compares with `by`.
    fn note_own_refusal(&mut self, node: u32, msg_id: MsgId, by: NodeId) {
        let Some(tag) = self.msg_tag(msg_id.0) else {
            return;
        };
        if !self.own_unsent(node, tag) || self.relays[node as usize].sleepy() {
            return;
        }
        self.metrics.pressure.own_refused += 1;
        self.handovers.own_refused_by.insert((node, tag), key(by));
    }

    /// `node` handed `msg_id` to `to`: if it had held the message on `to`
    /// after failures, the hand-over was taken at an offer again, and the
    /// hold ends; held on another neighbour, that hold is over without an
    /// outcome to count.
    fn hold_taken(&mut self, node: u32, msg_id: MsgId, to: NodeId) {
        let hold = self.handovers.holds.remove(&(node, msg_id.0));
        let failures = hold.filter(|h| h.to == key(to)).map_or(0, |h| h.failures);
        self.metrics.holds.taken_after(failures);
        if let Some(h) = hold.filter(|h| h.to == key(to)) {
            self.metrics
                .holds
                .held_us
                .push(self.now.since(h.since).as_micros());
        }
    }

    /// `node`'s hold of `msg_id`, if any, ended by something other than a
    /// hand-over: how long it lasted, microseconds.
    fn hold_ended(&mut self, node: u32, msg_id: MsgId) -> Option<u64> {
        let h = self.handovers.holds.remove(&(node, msg_id.0))?;
        Some(self.now.since(h.since).as_micros())
    }

    /// A relay refused the relay DATA it is being handed, if it is that:
    /// see `metrics::HopPlaceRefusals`.
    fn note_refusal(&mut self, node: u32, msg_id: MsgId, reason: DropReason) {
        if !matches!(reason, DropReason::Budget | DropReason::SendQueueFull) {
            return;
        }
        let Some((at, id, from, _)) = self.handovers.rx_data else {
            return;
        };
        if at != node || id != msg_id.0 {
            return;
        }
        let Some(&sender) = self.relay_index.get(&from) else {
            return;
        };
        let me = self.relays[node as usize].id();
        let lowest = self.relays[node as usize].lowest_free_hop_place();
        let then = self.clocks[sender as usize].local(self.now);
        let held = self.relays[sender as usize].highest_count_for(me, then);
        let with = matches!((lowest, held), (Some(k), Some(h)) if h >= u32::from(k));
        let h = &mut self.metrics.hop_places;
        h.refusals += 1;
        h.with_place += u64::from(with);
        self.handovers
            .refused_at
            .insert((sender, id), (self.now, with));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinct_next_hops_count_each_relay_and_neighbour_once() {
        let log = OfferLog {
            to: vec![(1, 10), (1, 10), (2, 20), (1, 11)],
            flaps: 0,
        };
        assert_eq!(log.next_hops(), 3);
    }
}
