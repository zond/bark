//! Reception model (docs/simulator-design.md §3.3), as power over intervals.
//!
//! Every node keeps the list of signals currently on the air at it: packets
//! from its stored transmitters and interferer bursts, each with channel,
//! power and end time. A transmission touches only its transmitter's stored
//! link row, when it reaches the air and when it leaves it.
//!
//! A node listening on channel `c`, with no excursion in progress:
//!
//! 1. **Detectability.** A packet on `c` below sensitivity is not received
//!    (it still counts as interference).
//! 2. **Lock.** With no lock, the receiver locks to the packet if the
//!    interference already on the air (co-channel, plus ±1 and ±2 channels
//!    after rejection) is below it by the capture threshold. A second
//!    packet on `c` that arrives during the locked packet's preamble steals
//!    the lock if it is stronger by the capture threshold; otherwise it is
//!    lost to capture.
//! 3. **Survival.** Interference only rises when a signal starts, so the
//!    lock is re-checked at every start that overlaps it: this is the worst
//!    instantaneous interference over the packet, the quantity that decides
//!    whether symbols are lost. If the packet falls below the capture
//!    threshold it is lost, and the receiver is deaf, demodulating garbage,
//!    until both it and the signal that broke it have ended.
//! 4. **Delivery.** A locked packet that reaches its end is delivered as
//!    `Event::Received`, CRC assumed perfect.
//!
//! Interference is evaluated online, so no interval needs to outlive the
//! signal: the worst value seen is carried in the lock.

use crate::metrics::Outcome;
use crate::phy::{self, Deaf};
use crate::sim::{Arrival, ExKind, Lock, Rx, Sim};
use bark_proto::radio::{CHANNELS, SYMBOL_TIME};
use bark_proto::{Event, Instant, MAX_FRAME};

/// Tolerance for threshold comparisons in dB, so that an exact 5.0 dB
/// margin computed through milliwatts counts as 5.0 dB.
const DB_EPS: f64 = 1e-9;

/// Whether a signal at `s_dbm` survives interference `i_mw` with `cap_db`.
fn captures(s_dbm: f64, i_mw: f64, cap_db: f64) -> bool {
    i_mw <= 0.0 || s_dbm - phy::dbm(i_mw) >= cap_db - DB_EPS
}

/// Interference at a receiver resting on `rest`, excluding `exclude`, as
/// (co-channel, adjacent) milliwatts after rejection.
fn interference(arrivals: &[Arrival], rest: u8, exclude: u64, rej: &[f64; 3]) -> (f64, f64) {
    let (mut co, mut adj) = (0.0, 0.0);
    for a in arrivals {
        if a.key == exclude {
            continue;
        }
        match a.channel.abs_diff(rest) {
            0 => co += a.mw,
            d @ (1 | 2) => adj += a.mw * rej[d as usize],
            _ => {}
        }
    }
    (co, adj)
}

/// A packet's receiver-independent facts.
#[derive(Clone, Copy)]
pub(crate) struct Packet {
    pub preamble_end: Instant,
}

impl Sim {
    pub(crate) fn air_start(&mut self, slot: u32, g: u32) {
        if !self.slot_valid(slot, g) {
            return;
        }
        let tx = self.txs[slot as usize].as_mut().expect("valid slot");
        tx.on_air = true;
        let (src, key, end, start) = (tx.src, tx.key, tx.end, tx.start);
        let ch = tx.frame.channel;
        let preamble_end = start + SYMBOL_TIME.mul(tx.frame.preamble_symbols as u64);
        let air = end.since(start).as_micros();
        let m = &mut self.metrics;
        m.nodes[src as usize].tx_frames += 1;
        m.nodes[src as usize].airtime_us += air;
        m.channels[ch.index() as usize].tx_frames += 1;
        m.channels[ch.index() as usize].airtime_us += air;
        self.push(end, crate::sim::Ev::AirEnd(slot, g));
        if !self.nodes[src as usize].present {
            return; // A dark leaf transmits into a tunnel.
        }
        let packet = Packet { preamble_end };
        for i in 0..self.links.row(src).len() {
            let l = self.links.row(src)[i];
            let dbm = self.links.rx_dbm(&l, ch);
            let a = Arrival {
                key,
                channel: ch.index(),
                dbm,
                mw: phy::mw(dbm),
                end,
                lora: true,
            };
            self.arrive(l.rx, a, Some(packet));
        }
    }

    /// A transmission leaves the air, at its end or because its transmitter
    /// was removed (`truncated`).
    pub(crate) fn air_end(&mut self, slot: u32, g: u32, truncated: bool) {
        if !self.slot_valid(slot, g) {
            return;
        }
        let tx = self.txs[slot as usize].take().expect("valid slot");
        self.free_slot(slot);
        if let Some(ex) = &mut self.nodes[tx.src as usize].ex {
            if ex.slot == Some(slot) {
                ex.slot = None;
            }
        }
        let ch = tx.frame.channel;
        let mut got: Vec<(u32, f64, f64)> = Vec::new();
        for i in 0..self.links.row(tx.src).len() {
            let b = self.links.row(tx.src)[i].rx;
            self.remove_arrival(b, tx.key);
            let st = &mut self.nodes[b as usize];
            if let Lock::Rx(r) = st.lock {
                if r.key == tx.key {
                    st.lock = Lock::Idle;
                    if truncated {
                        self.metrics.outcome(b, r.channel, Outcome::Truncated);
                    } else {
                        self.metrics.outcome(b, r.channel, Outcome::Delivered);
                        got.push((b, r.mw, r.max_i_mw));
                    }
                }
            }
        }
        let mut bytes = [0u8; MAX_FRAME];
        let len = tx.frame.len as usize;
        bytes[..len].copy_from_slice(tx.frame.payload());
        for (b, s_mw, i_mw) in got {
            // Packet RSSI is the in-band power over the packet, signal plus
            // noise, as the radio's RssiPkt reports it; SNR is against noise
            // plus the worst interference seen.
            let rssi = phy::dbm(s_mw + self.noise_mw).round() as i16;
            let snr = (phy::dbm(s_mw) - phy::dbm(self.noise_mw + i_mw))
                .round()
                .clamp(-128.0, 127.0) as i8;
            self.deliver(
                b,
                Event::Received {
                    channel: ch,
                    rssi_dbm: rssi,
                    snr_db: snr,
                    bytes: &bytes[..len],
                },
            );
        }
    }

    /// A signal reaches node `b`. `packet` is set for transmissions.
    pub(crate) fn arrive(&mut self, b: u32, a: Arrival, packet: Option<Packet>) {
        let now = self.now;
        let sens = self.sens_dbm;
        let cap = self.radio.capture_db;
        let rej = self.rej;
        let st = &mut self.nodes[b as usize];
        if !st.alive || !st.present {
            return;
        }
        if let Lock::Corrupt(until) = st.lock {
            if now >= until {
                st.lock = Lock::Idle;
            }
        }
        st.arrivals.push(a);
        if let Some(ex) = &mut st.ex {
            if ex.kind == ExKind::Cad
                && a.lora
                && a.channel == ex.channel.index()
                && a.dbm >= sens
                && now < ex.bounds[1]
                && a.end > ex.bounds[0]
            {
                ex.cad_seen = true;
            }
        }
        let observed = st.observer.is_some();
        let Some(rest) = st.resting.map(|c| c.index()) else {
            if observed {
                self.observe(b, a.channel);
            }
            return;
        };
        let offset = a.channel.abs_diff(rest);
        if offset > 2 {
            if observed {
                self.observe(b, a.channel);
            }
            return;
        }
        if let (Some(p), 0) = (packet, offset) {
            self.metrics.candidate(b, a.channel);
            let outcome = if a.dbm < sens {
                Some(Outcome::BelowSensitivity)
            } else if let Some(ex) = &st.ex {
                Some(Outcome::Deaf(ex.reason_at(now)))
            } else if now < st.listen_from {
                Some(Outcome::Deaf(Deaf::Retune))
            } else {
                let new_lock = |max_i_mw| Rx {
                    key: a.key,
                    channel: a.channel,
                    dbm: a.dbm,
                    mw: a.mw,
                    preamble_end: p.preamble_end,
                    end: a.end,
                    max_i_mw,
                };
                match st.lock {
                    Lock::Corrupt(_) => Some(Outcome::Deaf(Deaf::Corrupt)),
                    Lock::Idle => {
                        let (co, adj) = interference(&st.arrivals, rest, a.key, &rej);
                        if captures(a.dbm, co + adj, cap) {
                            st.lock = Lock::Rx(new_lock(co + adj));
                            None
                        } else {
                            Some(Outcome::SinrFail { adjacent: adj > co })
                        }
                    }
                    Lock::Rx(r) if now < r.preamble_end && a.dbm >= r.dbm + cap - DB_EPS => {
                        // Stronger preamble during the locked one's: steal.
                        self.metrics.outcome(b, r.channel, Outcome::LostCapture);
                        let (co, adj) = interference(&st.arrivals, rest, a.key, &rej);
                        if captures(a.dbm, co + adj, cap) {
                            st.lock = Lock::Rx(new_lock(co + adj));
                            None
                        } else {
                            st.lock = Lock::Corrupt(a.end);
                            Some(Outcome::SinrFail { adjacent: adj > co })
                        }
                    }
                    Lock::Rx(_) => Some(Outcome::LostCapture),
                }
            };
            if let Some(o) = outcome {
                self.metrics.outcome(b, a.channel, o);
            }
        }
        // The new signal raises interference on any packet being received.
        let st = &mut self.nodes[b as usize];
        if st.ex.is_none() {
            if let Lock::Rx(mut r) = st.lock {
                if r.key != a.key {
                    let (co, adj) = interference(&st.arrivals, rest, r.key, &rej);
                    r.max_i_mw = r.max_i_mw.max(co + adj);
                    if captures(r.dbm, co + adj, cap) {
                        st.lock = Lock::Rx(r);
                    } else {
                        st.lock = Lock::Corrupt(r.end.max(a.end));
                        self.metrics.outcome(
                            b,
                            r.channel,
                            Outcome::SinrFail { adjacent: adj > co },
                        );
                    }
                }
            }
        }
        if observed {
            self.observe(b, a.channel);
        }
    }

    /// A signal leaves node `b`.
    pub(crate) fn remove_arrival(&mut self, b: u32, key: u64) {
        let st = &mut self.nodes[b as usize];
        if let Some(i) = st.arrivals.iter().position(|a| a.key == key) {
            let ch = st.arrivals.swap_remove(i).channel;
            if st.observer.is_some() {
                self.observe(b, ch);
            }
        }
    }

    /// Drop every signal at `b` (it left radio reach or was removed).
    pub(crate) fn clear_arrivals(&mut self, b: u32) {
        let st = &mut self.nodes[b as usize];
        let gone: Vec<u8> = st.arrivals.drain(..).map(|a| a.channel).collect();
        if st.observer.is_some() {
            for ch in gone {
                self.observe(b, ch);
            }
        }
    }

    /// In-channel signal power at `b` on channel `c`, adjacent channels
    /// after rejection, mW, excluding `exclude`.
    pub(crate) fn in_channel_mw(&self, b: u32, c: u8, exclude: Option<u64>) -> f64 {
        let (co, adj) = interference(
            &self.nodes[b as usize].arrivals,
            c,
            exclude.unwrap_or(u64::MAX),
            &self.rej,
        );
        co + adj
    }

    /// Update an observer's busy state on the channels a signal on `ch` affects.
    fn observe(&mut self, b: u32, ch: u8) {
        let o = self.nodes[b as usize].observer.expect("observer") as usize;
        let now = self.now.as_micros();
        for c in ch.saturating_sub(2)..=(ch + 2).min(CHANNELS - 1) {
            let busy = self.in_channel_mw(b, c, None) > self.busy_signal_mw;
            self.metrics.observers[o].set(c as usize, busy, now);
        }
    }
}
