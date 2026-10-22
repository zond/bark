//! People's nodes whose power switches between USB and battery, and with
//! it their role (`scenario::Power`, spec "Nodes", roadmap D71).
//!
//! A node that switches at random stays on USB and on battery for
//! exponential times of the scenario's means, from the power its
//! placement gives it. A walker's node, with `walkers`, is on battery
//! while it walks and on USB at its stops; walkers start at a stop. Each
//! change is reported to the node's core (`AppCommand::Power`), or, before
//! it boots, at its boot. A node on USB does not go dark: a pocket or a
//! basement is where a node on battery goes.

use crate::metrics::PowerMetrics;
use crate::rng::{Purpose, Stream};
use crate::sim::{key, Ev, Sim};
use bark_proto::wire::Packet;
use bark_proto::{AppCommand, Delivery, DropReason, Event, Frame, MsgId, NodeId};
use std::collections::HashMap;

/// The switching nodes and what is followed of them.
pub(crate) struct PowerModel {
    usb_s: f64,
    battery_s: f64,
    /// The nodes that switch at random, each with its stream.
    random: HashMap<u32, Stream>,
    /// Per node: whether it switches, at random or with its walks.
    switches: Vec<bool>,
    /// Switching node by its ID's key.
    index: HashMap<u32, u32>,
    /// The texts held at the end of a grace after the settle time, and
    /// their fate so far, an index of `PowerMetrics::held`.
    held: HashMap<u32, Option<usize>>,
    /// Per node, the anchor it had after its last event.
    anchors: Vec<Option<NodeId>>,
}

impl Sim {
    /// Choose the nodes whose power switches and start their processes.
    pub(crate) fn power_setup(
        &mut self,
        p: &crate::scenario::Power,
        seed: u64,
    ) -> Result<(), String> {
        if p.fraction <= 0.0 && !p.walkers {
            return Ok(());
        }
        if p.usb_s <= 0.0 || p.battery_s <= 0.0 {
            return Err("power: mean times must be positive".into());
        }
        let n = self.node_count();
        let mut m = PowerModel {
            usb_s: p.usb_s,
            battery_s: p.battery_s,
            random: HashMap::new(),
            switches: vec![false; n],
            index: HashMap::new(),
            held: HashMap::new(),
            anchors: vec![None; n],
        };
        let mut metrics = PowerMetrics::default();
        let people = self.people.clone();
        for i in people {
            let walker = self.walks.as_ref().and_then(|w| w.walker(i));
            let tied = p.walkers && walker.is_some_and(|k| k.route.is_none());
            let mut r = Stream::new(seed, Purpose::Power, i as u64);
            let random = walker.is_none() && r.chance(p.fraction);
            if !tied && !random {
                continue;
            }
            m.switches[i as usize] = true;
            m.index.insert(key(self.node_id(i)), i);
            metrics.switchers.push((i, i < self.relays(), tied));
            if tied {
                // At a stop: on USB.
                self.external[i as usize] = true;
            } else {
                let mean = if self.external[i as usize] {
                    p.usb_s
                } else {
                    p.battery_s
                };
                let first = crate::sim::secs(r.exponential(mean));
                m.random.insert(i, r);
                self.push(bark_proto::Instant::ZERO + first, Ev::Power(i));
            }
        }
        self.power = Some(m);
        self.metrics.research.power = Some(metrics);
        Ok(())
    }

    /// Whether `node`'s power switches.
    pub(crate) fn switches(&self, node: u32) -> bool {
        self.power
            .as_ref()
            .is_some_and(|m| m.switches[node as usize])
    }

    /// A switching node by its ID.
    pub(crate) fn switcher_by_id(&self, id: NodeId) -> Option<u32> {
        self.power.as_ref()?.index.get(&key(id)).copied()
    }

    /// A node that switches at random changes its power, and draws when
    /// it changes next.
    pub(crate) fn power_toggle(&mut self, node: u32) {
        let external = !self.external[node as usize];
        let Some(m) = self.power.as_mut() else { return };
        let mean = if external { m.usb_s } else { m.battery_s };
        let Some(r) = m.random.get_mut(&node) else {
            return;
        };
        let next = crate::sim::secs(r.exponential(mean));
        self.push(self.now + next, Ev::Power(node));
        self.set_power(node, external);
    }

    /// A walker started a walk or a stop: with `walkers`, its power
    /// follows.
    pub(crate) fn power_walk(&mut self, node: u32, walking: bool) {
        if self.switches(node) && !self.power_random(node) {
            self.set_power(node, !walking);
        }
    }

    fn power_random(&self, node: u32) -> bool {
        self.power
            .as_ref()
            .is_some_and(|m| m.random.contains_key(&node))
    }

    /// `node`'s power is `external` now: report it to its core if it runs.
    fn set_power(&mut self, node: u32, external: bool) {
        if self.external[node as usize] == external {
            return;
        }
        self.external[node as usize] = external;
        if !self.nodes[node as usize].alive {
            return;
        }
        let t = self.now.as_micros() as f64 / 1e6;
        let after = self.now >= self.settle;
        let r = &self.relay(node);
        let back = external && !r.sleepy() && !r.booting();
        let leaving = r.leaving();
        if let Some(m) = self.metrics.research.power.as_mut() {
            m.changes.push((t, node, external));
            if back && after {
                m.back[usize::from(leaving)] += 1;
            }
        }
        self.deliver(node, Event::App(AppCommand::Power { external }));
        self.set_presence(node);
    }

    /// After an event at a sleepy node: an anchor change whose old anchor
    /// is sleepy now was caused by a switch.
    pub(crate) fn power_after(&mut self, node: u32) {
        let Some(m) = self.power.as_ref() else { return };
        let r = self.relay(node);
        let now = r.anchor().map(|a| a.id);
        let was = m.anchors[node as usize];
        if was == now {
            return;
        }
        let gone = was
            .and_then(|old| m.index.get(&key(old)))
            .is_some_and(|&a| {
                let a = self.relay(a);
                a.sleepy() || a.leaving()
            });
        let counts = gone && r.sleepy() && self.now >= self.settle;
        if let Some(m) = self.power.as_mut() {
            m.anchors[node as usize] = now;
        }
        if counts {
            if let Some(p) = self.metrics.research.power.as_mut() {
                p.anchors_left += 1;
            }
        }
    }

    /// A trace of `node`'s core about its power.
    pub(crate) fn power_trace(&mut self, node: u32, t: &bark_proto::Trace) {
        use bark_proto::Trace as T;
        if self.power.is_none() {
            return;
        }
        let after = self.now >= self.settle;
        let s = self.now.as_micros() as f64 / 1e6;
        match *t {
            T::Role { sleepy } => {
                if let Some(p) = self.metrics.research.power.as_mut() {
                    p.turns.push((s, node, sleepy));
                }
            }
            T::GraceEnded if after => {
                let held: Vec<u32> = self
                    .relay(node)
                    .delivery_queue()
                    .map(|(id, _)| id.0)
                    .collect();
                let m = self.power.as_mut().expect("checked");
                for id in held {
                    m.held.entry(id).or_insert(None);
                }
                if let Some(p) = self.metrics.research.power.as_mut() {
                    p.graces += 1;
                }
            }
            T::Delivered { msg_id, by, .. } => {
                let fate = match by {
                    Delivery::Flush => 0,
                    Delivery::Window | Delivery::Long => 1,
                    Delivery::Relay => 2,
                };
                self.power_fate(msg_id, fate);
            }
            T::Dropped {
                msg_id,
                reason: DropReason::Unplugged,
            } => self.power_fate(msg_id, 3),
            _ => {}
        }
    }

    fn power_fate(&mut self, msg_id: MsgId, fate: usize) {
        if let Some(f) = self.power.as_mut().and_then(|m| m.held.get_mut(&msg_id.0)) {
            f.get_or_insert(fate);
        }
    }

    /// A heartbeat for a change, `f`, goes out: its airtime counted.
    pub(crate) fn power_changed(&mut self, f: &Frame) {
        debug_assert!(matches!(
            Packet::decode(f.payload()),
            Ok(Packet::Heartbeat(_))
        ));
        if self.now >= self.settle {
            let air = bark_proto::radio::time_on_air(f.len, f.preamble_symbols);
            if let Some(p) = self.metrics.research.power.as_mut() {
                p.changed.0 += 1;
                p.changed.1 += air.as_micros() as f64 / 1e6;
            }
        }
    }

    /// The fates of the texts held at the end of a grace
    /// (`PowerMetrics::held`).
    pub(crate) fn power_held(&self) -> [u64; 5] {
        let mut held = [0; 5];
        if let Some(m) = &self.power {
            for f in m.held.values() {
                held[f.unwrap_or(4)] += 1;
            }
        }
        held
    }
}
