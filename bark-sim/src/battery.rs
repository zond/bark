//! Each node's battery, and the grid's outages that take USB power away
//! (`scenario::Battery`, spec "Nodes").
//!
//! A cell holds its share of the nominal capacity, drawn per node, and
//! starts at a charge drawn from its placement's power: on USB or on
//! battery. On USB it charges at the charger's current less the load; on
//! battery it discharges at the radio's currents for the time the node's
//! radio listened, received duty-cycled and transmitted (the currents of
//! `bark_proto::sniff`, as the sleepy nodes' current in the summary), the
//! GPS for the searches a sleepy node asks for (`gps.rs`), and a current
//! for the rest of the board by its role, a relay's GPS among it. The time is the radio's after the
//! settle time, which the metrics count, so outages begin after it.
//!
//! An outage takes USB power from every node on it when it begins and
//! gives it back when it ends. On battery the driver reads the cell's
//! open-circuit voltage at its charge, off by the node's converter's gain
//! error, once a minute, and reports it to the core (`AppCommand::Battery`),
//! which calibrates it.

use crate::metrics::NodeMetrics;
use crate::rng::{Purpose, Stream};
use crate::scenario::Battery;
use crate::sim::{secs, Ev, Sim};
use bark_proto::gps::GPS_CURRENT_UA;
use bark_proto::sniff::{RX_CURRENT_UA, SNIFF_CURRENT_NA, TX_CURRENT_UA};
use bark_proto::{AppCommand, Event, Instant};

/// A typical Li-ion cell's open-circuit voltage, V, at 0, 5, ... 100 % of
/// its charge, each read after an hour's rest following a discharge of
/// 5 % at about 0.2 C (Maxim Integrated, Application Note 4189,
/// "Characterizing a Lithium-Ion (Li+) Cell for Use with an
/// Open-Circuit-Voltage (OCV) Based Fuel Gauge", Table 1).
pub const OCV_V: [f64; 21] = [
    3.305545, 3.674776, 3.686654, 3.7098, 3.741018, 3.762185, 3.775129, 3.783504, 3.79325,
    3.805737, 3.820965, 3.841219, 3.884009, 3.917968, 3.945074, 3.974769, 4.008118, 4.045427,
    4.085934, 4.129486, 4.177454,
];

/// The open-circuit voltage, V, at charge `soc`, 0 to 1, between the
/// points of [`OCV_V`].
pub fn ocv(soc: f64) -> f64 {
    let x = soc.clamp(0.0, 1.0) * (OCV_V.len() - 1) as f64;
    let i = (x.floor() as usize).min(OCV_V.len() - 2);
    OCV_V[i] + (OCV_V[i + 1] - OCV_V[i]) * (x - i as f64)
}

/// One node's cell.
struct Cell {
    /// Its capacity, mAh.
    capacity: f64,
    /// Its charge at the last reading, mAh.
    charge: f64,
    /// The charge its radio and its GPS's searches had drawn by the last
    /// reading, µA·µs.
    drawn: f64,
    /// Its reading's gain: 1 for a true reading.
    gain: f64,
    /// Whether it ever ran flat.
    flat: bool,
}

/// The cells and the outages.
pub(crate) struct BatteryModel {
    p: Battery,
    cells: Vec<Cell>,
    /// The nodes the outage under way took USB from.
    unplugged: Vec<u32>,
    /// The series written by `bark-sim --battery FILE`.
    rows: String,
    /// Turns to sleepy and to relay so far.
    turns: [u64; 2],
}

/// The series' columns (`Sim::battery_row`): the nodes on USB; the
/// relays, those leaving and those on battery; the sleepy nodes and those
/// without an anchor; the charge, as a share of the cell, of the nodes
/// placed as relays and of the people's nodes, those placed as leaves;
/// the cells that ever ran flat; the turns to sleepy and to relay; and
/// the airtime so far by kind (`metrics::TypeAirtime`), s.
const HEADER: &str = "t_s,usb,relays,leaving,relays_on_battery,sleepy,anchorless,\
relay_charge_mean,relay_charge_min,people_charge_mean,people_charge_p10,people_charge_min,\
flat,to_sleepy,to_relay,heartbeat_s,beat_s,reach_s,anchorless_s,answer_s,ack_s,data_s,long_data_s\n";

impl Sim {
    /// Draw every node's cell and schedule the readings and outages.
    pub(crate) fn battery_setup(&mut self, p: &Battery, seed: u64) -> Result<(), String> {
        if !p.on {
            return Ok(());
        }
        if p.capacity_mah <= 0.0 || p.read_s <= 0.0 {
            return Err("battery: capacity and reading interval must be positive".into());
        }
        let settle = self.settle.as_micros() as f64 / 1e6;
        for w in &p.outages {
            if w[0] < settle || w[1] <= w[0] {
                return Err(format!(
                    "battery: outage {w:?} must begin after the settle time, {settle} s, and end after it begins"
                ));
            }
        }
        let n = self.node_count();
        let draw = |r: &mut Stream, [lo, hi]: [f64; 2]| lo + (hi - lo) * r.uniform();
        let cells = (0..n as u32)
            .map(|i| {
                let mut r = Stream::new(seed, Purpose::Battery, i as u64);
                let capacity = p.capacity_mah * draw(&mut r, p.cell);
                let start = if self.external[i as usize] {
                    p.charge_on_usb
                } else {
                    p.charge_on_battery
                };
                let charge = capacity * draw(&mut r, start);
                let gain = 1.0 + p.gain_error * (2.0 * r.uniform() - 1.0);
                Cell {
                    capacity,
                    charge,
                    drawn: 0.0,
                    gain,
                    flat: false,
                }
            })
            .collect();
        self.battery = Some(BatteryModel {
            p: p.clone(),
            cells,
            unplugged: Vec::new(),
            rows: HEADER.to_string(),
            turns: [0; 2],
        });
        for w in &p.outages {
            self.push(Instant::ZERO + secs(w[0]), Ev::Grid(false));
            self.push(Instant::ZERO + secs(w[1]), Ev::Grid(true));
        }
        self.push(self.settle, Ev::Battery);
        Ok(())
    }

    /// The grid goes down, taking USB from every node on it, or comes back.
    pub(crate) fn grid(&mut self, on: bool) {
        let on_usb: Vec<u32> = (0..self.node_count() as u32)
            .filter(|&i| self.external[i as usize])
            .collect();
        let Some(m) = self.battery.as_mut() else {
            return;
        };
        let nodes = if on {
            std::mem::take(&mut m.unplugged)
        } else {
            m.unplugged.clone_from(&on_usb);
            on_usb
        };
        for i in nodes {
            self.set_power(i, on);
        }
    }

    /// Every cell to now, each node on battery read, and a row of the
    /// series.
    pub(crate) fn battery_tick(&mut self) {
        let Some(mut m) = self.battery.take() else {
            return;
        };
        let since = |t: Instant| t.since(self.settle).as_micros() as f64;
        let window = since(self.now);
        let dt_h = m.p.read_s / 3600.0;
        for (i, c) in m.cells.iter_mut().enumerate() {
            let gps = self.gps_on_us(i as u32) as f64 * GPS_CURRENT_UA as f64;
            let drawn = radio_ua_us(&self.metrics.nodes[i], window) + gps;
            let board = if self.relay(i as u32).sleepy() {
                m.p.sleepy_ua
            } else {
                m.p.relay_ua
            };
            // µA·µs to mAh.
            let used = (drawn - c.drawn) / 3.6e12 + board * dt_h / 1e3;
            c.drawn = drawn;
            if !self.nodes[i].alive {
                continue;
            }
            if self.external[i] {
                c.charge = (c.charge + m.p.charge_ma * dt_h - used).min(c.capacity);
            } else {
                c.charge = (c.charge - used).max(0.0);
                c.flat |= c.charge == 0.0;
            }
        }
        let readings: Vec<(u32, u16)> = m
            .cells
            .iter()
            .enumerate()
            .filter(|&(i, _)| !self.external[i] && self.nodes[i].alive)
            .map(|(i, c)| {
                let v = ocv(c.charge / c.capacity) * c.gain;
                (i as u32, (v * 1000.0).round() as u16)
            })
            .collect();
        let next = self.now + secs(m.p.read_s);
        // Put back first: a reading may turn a node, which is counted.
        self.battery = Some(m);
        for (i, millivolts) in readings {
            self.deliver(i, Event::App(AppCommand::Battery { millivolts }));
        }
        let mut m = self.battery.take().expect("put back");
        self.battery_row(&mut m);
        self.battery = Some(m);
        self.push(next, Ev::Battery);
    }

    /// A trace of a core about its role (`Trace::Role`).
    pub(crate) fn battery_turn(&mut self, sleepy: bool) {
        if let Some(m) = self.battery.as_mut() {
            m.turns[usize::from(!sleepy)] += 1;
        }
    }

    fn battery_row(&self, m: &mut BatteryModel) {
        let mut relays = (0u32, 0u32, 0u32);
        let (mut sleepy, mut anchorless) = (0u32, 0u32);
        let (mut rc, mut sc) = (Vec::new(), Vec::new());
        let mut air = [0u64; 8];
        for (i, c) in m.cells.iter().enumerate() {
            let a = &self.metrics.nodes[i].airtime_by_type;
            for (k, us) in [
                a.heartbeat_us,
                a.beat_us,
                a.reach_us,
                a.anchorless_us,
                a.answer_us,
                a.ack_us,
                a.data_us,
                a.long_data_us,
            ]
            .into_iter()
            .enumerate()
            {
                air[k] += us;
            }
            if !self.nodes[i].alive {
                continue;
            }
            let r = self.relay(i as u32);
            let share = c.charge / c.capacity;
            if (i as u32) < self.relays() {
                rc.push(share);
            } else {
                sc.push(share);
            }
            if r.sleepy() {
                sleepy += 1;
                anchorless += u32::from(r.anchorless());
            } else {
                relays.0 += 1;
                relays.1 += u32::from(r.leaving());
                relays.2 += u32::from(!self.external[i]);
            }
        }
        let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len().max(1) as f64;
        sc.sort_by(f64::total_cmp);
        let min = |v: &[f64]| v.iter().copied().fold(f64::NAN, f64::min);
        let p10 = sc.get(sc.len() / 10).copied().unwrap_or(f64::NAN);
        let flat = m.cells.iter().filter(|c| c.flat).count();
        let usb = self.external.iter().filter(|&&e| e).count();
        let s = |us: u64| us as f64 / 1e6;
        m.rows.push_str(&format!(
            "{:.0},{usb},{},{},{},{sleepy},{anchorless},{:.4},{:.4},{:.4},{:.4},{:.4},{flat},{},{},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1}\n",
            self.now.as_micros() as f64 / 1e6,
            relays.0,
            relays.1,
            relays.2,
            mean(&rc),
            min(&rc),
            mean(&sc),
            p10,
            min(&sc),
            m.turns[0],
            m.turns[1],
            s(air[0]),
            s(air[1]),
            s(air[2]),
            s(air[3]),
            s(air[4]),
            s(air[5]),
            s(air[6]),
            s(air[7]),
        ));
    }

    /// The series of the cells and roles, if the scenario has batteries.
    pub fn battery_csv(&self) -> Option<&str> {
        self.battery.as_ref().map(|m| m.rows.as_str())
    }
}

/// The charge `node`'s radio has drawn since the settle time, `window`
/// µs long, at the currents of `bark_proto::sniff`, µA·µs.
fn radio_ua_us(node: &NodeMetrics, window: f64) -> f64 {
    let tx = node.settled_airtime_us as f64;
    let sniff = node.sniff_us.saturating_sub(node.sniff_rx_us) as f64;
    let listen = (window - sniff - tx).max(0.0);
    listen * RX_CURRENT_UA as f64
        + sniff * SNIFF_CURRENT_NA as f64 / 1e3
        + tx * TX_CURRENT_UA as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The curve rises with the charge, passes through its points, and
    /// puts the design's levels at 20 and 30 % (`BatteryLevels::DESIGN`).
    #[test]
    fn the_curve_and_the_levels() {
        assert!(OCV_V.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(ocv(0.0), OCV_V[0]);
        assert_eq!(ocv(1.0), OCV_V[20]);
        assert!((ocv(0.125) - (OCV_V[2] + OCV_V[3]) / 2.0).abs() < 1e-9);
        let levels = bark_proto::BatteryLevels::DESIGN;
        assert_eq!((ocv(0.2) * 1000.0).round() as u16, levels.low_mv);
        assert_eq!((ocv(0.3) * 1000.0).round() as u16, levels.high_mv);
    }
}
