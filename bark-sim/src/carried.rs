//! What the nodes that move do (`metrics::Carried`): their first far
//! fixes, their turns to carried and back, the walks of fixed relays, how
//! holders deliver to their nodes, and the airtime of it. Counting only:
//! nothing here changes a draw or a decision.

use crate::metrics::{Carried, FarFate};
use crate::sim::{Fate, Sim};
use bark_proto::radio::time_on_air;
use bark_proto::sniff::LONG_PREAMBLE_SYMBOLS;
use bark_proto::wire::Packet;
use bark_proto::{Delivery, Frame, Instant, MsgId, Position, Trace};
use std::collections::{HashMap, HashSet};

/// What is followed of the nodes that move between events.
#[derive(Default)]
pub(crate) struct CarriedWatch {
    /// The texts held at a first far fix after the settle time: whether
    /// their holder had turned fixed again before, and how it delivered
    /// them.
    held_far: HashMap<u32, (bool, Option<FarFate>)>,
    /// Per relay, its last first far fix, and whether it has turned fixed
    /// again since it was first carried.
    far_at: HashMap<u32, (Instant, bool)>,
    /// Fixed nodes plugged in that have had no fix since: when, their
    /// position then, and their entry in `Carried::plugged_fixed`.
    plugged: HashMap<u32, (Instant, Position, usize)>,
    /// The walks under way, begun after the settle time, of walkers that
    /// were fixed relays when they set off: when the walk began, whether
    /// the node was on battery, and when it turned carried.
    relay_walks: HashMap<u32, (Instant, bool, Option<Instant>)>,
    /// The sleepy nodes whose carried flag changed since their last
    /// heartbeat for a change.
    flipped: HashSet<u32>,
    /// The nodes about to send a heartbeat for a change that says a sleepy
    /// node's carried flag.
    flipping: HashSet<u32>,
    /// Deliveries by the way they went, after the settle time
    /// (`Carried::delivered_by`).
    delivered_by: [u64; 4],
}

impl Sim {
    /// What the nodes that move do, from the first event that shows one.
    fn carried_metrics(&mut self) -> &mut Carried {
        self.metrics
            .research
            .carried
            .get_or_insert_with(Default::default)
    }

    /// A trace of `node`'s core.
    pub(crate) fn carried_trace(&mut self, node: u32, t: &Trace) {
        let after = self.now >= self.settle;
        match *t {
            Trace::MoveSign {
                surprises,
                sum_milli,
                moved,
            } if after => {
                let walking = self
                    .walks
                    .as_ref()
                    .and_then(|w| w.walker(node))
                    .is_some_and(|k| k.walking);
                let m = &mut self.carried_metrics().move_sign[usize::from(walking)];
                m[0] += 1.0;
                m[1] += f64::from(u8::from(moved));
                m[2] += sum_milli as f64 / 1000.0 / f64::from(surprises.max(1));
            }
            Trace::Delivered { msg_id, by, .. } => self.note_delivered_by(msg_id, by),
            Trace::FarFix => self.note_far_fix(node),
            Trace::Changed => {
                let w = &mut self.carried;
                if w.flipped.remove(&node) {
                    w.flipping.insert(node);
                } else {
                    w.flipping.remove(&node);
                }
            }
            Trace::Role { sleepy: false } if self.relays[node as usize].carried() => {
                self.carried_metrics().plugged_in += u64::from(after);
            }
            Trace::Role { sleepy: false } if self.relays[node as usize].placed() => {
                let at = self.relays[node as usize].config().position;
                let c = self.carried_metrics();
                c.plugged_fixed.push(None);
                let k = c.plugged_fixed.len() - 1;
                self.carried.plugged.insert(node, (self.now, at, k));
            }
            Trace::Carried { carried } => self.note_carried(node, carried),
            Trace::AnchorCarried => {
                self.carried_metrics().anchors_left += u64::from(after);
            }
            Trace::Queued { .. } if self.relays[node as usize].carried() => {
                self.carried_metrics().queued += 1;
            }
            _ => {}
        }
    }

    /// `relay` had the first far fix of a run: what it holds for its
    /// sleepy nodes is flushed.
    fn note_far_fix(&mut self, relay: u32) {
        let walks = self.walks.as_ref().is_some_and(|w| w.walks(relay));
        let t = self.now;
        let again = self.carried.far_at.get(&relay).is_some_and(|&(_, a)| a);
        self.carried.far_at.insert(relay, (t, again));
        let s = t.as_micros() as f64 / 1e6;
        self.carried_metrics().far_fixes.push((s, relay, walks));
        if t >= self.settle {
            let held: Vec<u32> = self.relays[relay as usize]
                .delivery_queue()
                .map(|(id, _)| id.0)
                .collect();
            for id in held {
                self.carried.held_far.entry(id).or_insert((again, None));
            }
        }
    }

    /// A walker set off or stopped: a walk of a fixed relay, begun after
    /// the settle time, is followed until it ends.
    pub(crate) fn carried_walk(&mut self, node: u32, walking: bool) {
        if !walking {
            self.carried_walk_end(node);
            return;
        }
        let r = &self.relays[node as usize];
        if self.now >= self.settle && self.nodes[node as usize].alive && !r.sleepy() && !r.carried()
        {
            let battery = !self.external[node as usize];
            self.carried
                .relay_walks
                .insert(node, (self.now, battery, None));
        }
    }

    /// The walk of `node` under way, if followed, ends now.
    pub(crate) fn carried_walk_end(&mut self, node: u32) {
        if let Some((from, battery, carried)) = self.carried.relay_walks.remove(&node) {
            let s = |t: Instant| t.since(from).as_micros() as f64 / 1e6;
            let walk = (node, s(self.now), carried.map(s), battery);
            self.carried_metrics().relay_walks.push(walk);
        }
    }

    /// `relay` turned carried, or fixed again.
    fn note_carried(&mut self, relay: u32, carried: bool) {
        let t = self.now;
        if let Some((_, _, at)) = self.carried.relay_walks.get_mut(&relay) {
            if carried {
                at.get_or_insert(t);
            }
        }
        let walks = self.walks.as_ref().is_some_and(|w| w.walks(relay));
        let sleepy = self.relays[relay as usize].sleepy();
        let w = &mut self.carried;
        if sleepy && !w.flipped.remove(&relay) {
            w.flipped.insert(relay);
        }
        let s = t.as_micros() as f64 / 1e6;
        if carried {
            let since = w.far_at.get(&relay).map_or(t, |&(f, _)| f);
            let run = t.since(since).as_micros() as f64 / 1e6;
            self.carried_metrics()
                .carried
                .push((s, relay, walks, run, sleepy));
        } else {
            w.far_at.entry(relay).or_insert((t, true)).1 = true;
            self.carried_metrics().fixed.push((s, relay, walks, sleepy));
        }
    }

    /// `node` sends a heartbeat for a change, `f`: if it says a sleepy
    /// node's carried flag, counted.
    pub(crate) fn carried_changed(&mut self, node: u32, f: &Frame) {
        let flip = self.carried.flipping.remove(&node);
        if flip && self.now >= self.settle {
            let air = time_on_air(f.len, f.preamble_symbols).as_micros() as f64 / 1e6;
            self.carried_metrics().sleepy_changed.add(air);
        }
    }

    /// `node` sends `f`: the flush, a delivery to a node as a relay and a
    /// heartbeat flagged sleepy and carried are counted.
    pub(crate) fn carried_transmit(&mut self, node: u32, f: &Frame) {
        let after = self.now >= self.settle;
        let air = time_on_air(f.len, f.preamble_symbols).as_micros() as f64 / 1e6;
        if self.relays[node as usize].sending_direct() {
            let c = self.carried_metrics();
            let to = if f.preamble_symbols >= LONG_PREAMBLE_SYMBOLS {
                &mut c.flush
            } else {
                &mut c.to_relay
            };
            if after {
                to.add(air);
            }
        }
        if !after {
            return;
        }
        if let Ok(Packet::Heartbeat(hb)) = Packet::decode(f.payload()) {
            if hb.sleepy && hb.carried {
                self.carried_metrics().beats.add(air);
            }
        }
    }

    /// A holder delivered `msg_id` to its node `by` that way.
    fn note_delivered_by(&mut self, msg_id: MsgId, by: Delivery) {
        let (i, fate) = match by {
            Delivery::Window => (0, FarFate::Beat),
            Delivery::Long => (1, FarFate::Beat),
            Delivery::Flush => (2, FarFate::Flush),
            Delivery::Relay => (3, FarFate::AsRelay),
        };
        if self.now >= self.settle {
            self.carried.delivered_by[i] += 1;
        }
        if matches!(by, Delivery::Flush | Delivery::Relay) {
            self.carried_metrics();
        }
        if let Some((_, f)) = self.carried.held_far.get_mut(&msg_id.0) {
            f.get_or_insert(fate);
        }
    }

    /// `n`'s core was handed a fix: if `n` is a fixed node plugged in
    /// within a search's limit, whether the fix moved its place, and how
    /// far it truly was from it.
    pub(crate) fn carried_fix(&mut self, n: u32) {
        if let Some((t, before, k)) = self.carried.plugged.remove(&n) {
            if self.now.since(t) <= bark_proto::gps::SEARCH_LIMIT {
                let truly = self.origin.position(self.place(n));
                let m = bark_proto::geo::LocalPlane::new(before).distance_m(before, truly);
                let kept = self.relays[n as usize].config().position == before;
                self.carried_metrics().plugged_fixed[k] = Some((kept, f64::from(m)));
            }
        }
    }

    /// The fates of the texts held at a first far fix (`Carried::held`).
    pub(crate) fn held_fates(&self) -> [[u64; FarFate::N]; 2] {
        let mut held = [[0; FarFate::N]; 2];
        for (id, &(again, fate)) in &self.carried.held_far {
            let got = self.message_fate(*id) == Some(Fate::Delivered);
            let fate = fate.unwrap_or(if got {
                FarFate::Otherwise
            } else {
                FarFate::None
            });
            held[usize::from(again)][fate as usize] += 1;
        }
        held
    }

    /// Deliveries by the way they went (`Carried::delivered_by`).
    pub(crate) fn delivered_by(&self) -> [u64; 4] {
        self.carried.delivered_by
    }
}
