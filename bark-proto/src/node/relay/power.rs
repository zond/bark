//! Transmit power control: a research prototype, off unless
//! `Config::power` chooses a rule (`docs/power-research-2026-10-15.md`).
//!
//! A heartbeat states the power it went at (`wire::HeartbeatPower`), so a
//! relay that hears it knows the path loss to its sender: that power less
//! the RSSI. Paths are reciprocal, so the same loss says what power
//! reaches the sender back. A frame reaches a receiver when it arrives
//! the margin above sensitivity.
//!
//! - Per link: a frame to a known receiver goes at the power its loss
//!   needs; to an unknown one, at the limit.
//! - Operating power: the lowest stated step at which `degree` neighbours,
//!   and in every occupied sector the nearest neighbour, would be reached.
//!   It rises at once and falls only by two steps or more, so that the
//!   averaging noise of the losses does not move it.
//! - Discovery: under the rules that lower heartbeats, one rotation in
//!   `discovery_every` goes at the limit, so that every relay in reach
//!   is still heard and measured, and its entry is refreshed before it
//!   expires. Such a heartbeat adds a relay to the table only if a frame
//!   at the limit would reach it: a relay out of reach is no next hop.

use super::Relay;
use crate::event::Frame;
use crate::ids::LeafId;
use crate::node::PowerRule;
use crate::position::ShortName;
use crate::radio::CHANNELS;
use crate::tables::neighbours::{Neighbour, NEIGHBOURS};

/// No loss known.
const LOSS_UNKNOWN: i16 = i16::MAX;
use crate::wire::{Heartbeat, HeartbeatPower, Packet};

/// The power a frame needs over a loss of `loss_q` quarter dB, dBm, before
/// any limit.
fn needed(sensitivity_dbm: i8, margin_db: u8, loss_q: i16) -> i16 {
    sensitivity_dbm as i16 + margin_db as i16 + (loss_q + 3).div_euclid(4)
}

impl Relay {
    fn limit(&self) -> i8 {
        self.config.tx_power_dbm
    }

    fn need(&self, loss_q: i16) -> i16 {
        let p = &self.config.power;
        needed(p.sensitivity_dbm, p.margin_db, loss_q)
    }

    /// Whether a full frame at the limit reaches a neighbour of loss `loss_q`.
    fn in_reach(&self, loss_q: i16) -> bool {
        loss_q != LOSS_UNKNOWN && self.need(loss_q) <= self.limit() as i16
    }

    /// The power a frame needs over `loss_q`, within the floor and the limit.
    fn link_power(&self, loss_q: i16) -> i8 {
        if loss_q == LOSS_UNKNOWN {
            return self.limit();
        }
        self.need(loss_q)
            .clamp(HeartbeatPower::FLOOR_DBM as i16, self.limit() as i16) as i8
    }

    fn lowers_heartbeats(&self) -> bool {
        matches!(self.config.power.rule, PowerRule::Degree | PowerRule::Both)
    }

    /// What the next heartbeat states, and the power it goes at.
    pub(super) fn heartbeat_power(&self) -> (HeartbeatPower, i8) {
        if !self.lowers_heartbeats() {
            let stated = HeartbeatPower {
                operating_dbm: HeartbeatPower::step_down(self.limit()),
                full: false,
            };
            return (stated, self.limit());
        }
        let every = self.config.power.discovery_every as u32;
        let full = every > 0 && (self.beats / CHANNELS as u32).is_multiple_of(every);
        let stated = HeartbeatPower {
            operating_dbm: self.power_dbm,
            full,
        };
        (stated, if full { self.limit() } else { self.power_dbm })
    }

    /// The power for the hand-over frame `f`.
    pub(super) fn frame_power(&self, f: &Frame) -> i8 {
        match self.config.power.rule {
            PowerRule::Fixed => self.limit(),
            PowerRule::Degree => self.power_dbm,
            PowerRule::PerLink | PowerRule::Both => {
                let loss = match Packet::decode(f.payload()) {
                    Ok(Packet::Data(d)) => self
                        .neighbours
                        .by_short(d.dst_hop)
                        .map_or(LOSS_UNKNOWN, |(_, n)| self.loss_q(n)),
                    Ok(Packet::Deliver(d)) => self
                        .leaves
                        .get(d.dst_leaf)
                        .map_or(LOSS_UNKNOWN, |e| 4 * e.loss_db as i16),
                    _ => LOSS_UNKNOWN,
                };
                self.link_power(loss)
            }
        }
    }

    /// The power for a poll of `leaf`.
    pub(super) fn leaf_power(&self, leaf: LeafId) -> i8 {
        match self.config.power.rule {
            PowerRule::Fixed => self.limit(),
            PowerRule::Degree => self.power_dbm,
            PowerRule::PerLink | PowerRule::Both => self.link_power(
                self.leaves
                    .get(leaf)
                    .map_or(LOSS_UNKNOWN, |e| 4 * e.loss_db as i16),
            ),
        }
    }

    /// The power for the reply to the frame just received: from the relay
    /// `relay`, or from a leaf.
    pub(super) fn reply_power_for(&self, relay: Option<ShortName>) -> i8 {
        match self.config.power.rule {
            PowerRule::Fixed => self.limit(),
            PowerRule::Degree => self.power_dbm,
            PowerRule::PerLink | PowerRule::Both => match relay {
                Some(name) => self
                    .neighbours
                    .by_short(name)
                    .map_or(self.limit(), |(_, n)| self.link_power(self.loss_q(n))),
                None => self.link_power(self.leaf_loss()),
            },
        }
    }

    /// Loss to the leaf whose frame was just received: leaves send at the
    /// limit.
    pub(super) fn leaf_loss(&self) -> i16 {
        4 * (self.limit() as i16 - self.rx_rssi)
    }

    /// Whether a heartbeat may add its sender to the table: one at the
    /// limit only if a frame at the limit reaches the sender back.
    pub(super) fn admits(&self, hb: &Heartbeat) -> bool {
        !(self.lowers_heartbeats() && hb.power.full) || self.in_reach(self.heartbeat_loss(hb))
    }

    fn heartbeat_loss(&self, hb: &Heartbeat) -> i16 {
        4 * (self.heartbeat_tx(hb) as i16 - self.rx_rssi)
    }

    /// The power `hb` went at. The top step stands for the limit, which
    /// need not be a step.
    fn heartbeat_tx(&self, hb: &Heartbeat) -> i8 {
        if hb.power.full || hb.power.operating_dbm >= HeartbeatPower::step_down(self.limit()) {
            self.limit()
        } else {
            hb.power.operating_dbm.min(self.limit())
        }
    }

    /// Path loss to `n` in quarter dB, from its averaged RSSI at the limit.
    fn loss_q(&self, n: &Neighbour) -> i16 {
        libm::roundf(4.0 * (self.limit() as f32 - n.rssi)) as i16
    }

    /// The RSSI of heartbeat `hb`, heard at `rssi`, as if it had been
    /// sent at the limit.
    pub(super) fn at_limit(&self, hb: &Heartbeat, rssi: i16) -> i16 {
        rssi + (self.limit() - self.heartbeat_tx(hb)) as i16
    }

    /// Whether every frame goes at the limit, so that the RSSI of a frame
    /// that states no power is as at the limit.
    pub(super) fn frames_at_limit(&self) -> bool {
        self.config.power.rule == PowerRule::Fixed
    }

    /// Whether `n` may be a next hop under the rule.
    pub(super) fn usable(&self, n: &Neighbour) -> bool {
        let p = &self.config.power;
        let loss = self.loss_q(n);
        if p.reach_filter && !self.in_reach(loss) {
            return false;
        }
        match p.rule {
            PowerRule::Degree => self.need(loss) <= self.power_dbm.min(n.power_dbm) as i16,
            _ => true,
        }
    }

    /// Recompute the operating power from the table (after each heartbeat).
    pub(super) fn adjust_power(&mut self, now: crate::time::Instant) {
        if !self.lowers_heartbeats() {
            return;
        }
        let p = self.config.power;
        let sectors = p.sectors.clamp(0, 16) as usize;
        let mut losses = [0i16; NEIGHBOURS];
        let mut nearest = [LOSS_UNKNOWN; 16];
        let mut k = 0;
        for (_, n) in self.neighbours.live(now) {
            let loss = self.loss_q(n);
            if !self.in_reach(loss) {
                continue;
            }
            losses[k] = loss;
            k += 1;
            if sectors > 0 {
                let turn = libm::atan2f(n.local.north, n.local.east) / core::f32::consts::TAU;
                let s = ((turn + 1.0) * sectors as f32) as usize % sectors;
                nearest[s] = nearest[s].min(loss);
            }
        }
        let losses = &mut losses[..k];
        losses.sort_unstable();
        let mut need_q = match (p.degree as usize).checked_sub(1) {
            Some(i) if i < k => losses[i],
            _ => i16::MAX,
        };
        for &s in &nearest[..sectors] {
            if s != LOSS_UNKNOWN {
                need_q = need_q.max(s);
            }
        }
        let target = if need_q == i16::MAX {
            self.limit()
        } else {
            // The lowest stated step at or above what is needed.
            let need = self.need(need_q).min(self.limit() as i16) as i8;
            let step = HeartbeatPower::step_down(need);
            if step < need {
                (step + 2).min(self.limit())
            } else {
                step
            }
        };
        if target > self.power_dbm || target <= self.power_dbm - 4 {
            self.power_dbm = target;
        }
    }
}
