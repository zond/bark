//! Reception model (docs/simulator-design.md §3.3), as power over intervals.
//!
//! Every node keeps the list of signals currently on the air at it: packets
//! from its stored transmitters and interferer bursts, each with channel,
//! power and end time. A transmission touches only its transmitter's stored
//! link row, when it reaches the air and when it leaves it.
//!
//! A node listening on channel `c`, with no excursion in progress:
//!
//! 1. **Lock.** With no lock, the receiver locks to a packet on `c` that
//!    arrives. A second packet on `c` that arrives during the locked
//!    packet's preamble steals the lock if it is stronger by the capture
//!    threshold; otherwise it is lost to capture.
//! 2. **Survival.** While locked, every symbol may be lost, at a hazard
//!    the error curve gives from the signal against noise and the
//!    interference on the air (co-channel, plus ±1 and ±2 channels after
//!    rejection). Interference changes only when a signal starts or ends,
//!    so the hazard is constant between those moments and is summed over
//!    them. The packet is lost when the summed hazard reaches a budget
//!    drawn at lock from the exponential distribution, which loses it with
//!    the probability that not every symbol survives.
//! 3. **Loss.** A packet lost before its header has ended frees the
//!    receiver to search for a preamble again: a weak packet costs it a
//!    few symbols. One lost later leaves it deaf, demodulating garbage,
//!    until the packet's end, the length its header gave.
//! 4. **Delivery.** A locked packet that reaches its end is delivered as
//!    `Event::Received`, CRC assumed perfect.
//!
//! The hazard is summed lazily: whenever a node's lock is looked at, it is
//! first brought up to the present (`settle`), and a loss found then is
//! dated to when the budget ran out.

use crate::metrics::Outcome;
use crate::phy::{self, Deaf, Loss};
use crate::sim::{key, Arrival, ExKind, Lock, Rx, Sim};
use bark_proto::radio::{header_end, CHANNELS, MIN_PREAMBLE_SYMBOLS, SYMBOL_TIME};
use bark_proto::wire::Packet as WirePacket;
use bark_proto::{Duration, Event, Instant, MAX_FRAME};

/// Tolerance for threshold comparisons in dB, so that an exact 5.0 dB
/// margin computed through milliwatts counts as 5.0 dB.
const DB_EPS: f64 = 1e-9;

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
    pub header_end: Instant,
    /// A receiver needs only the minimum preamble to detect a packet
    /// (datasheet §6.1.1.1: 12 symbols at SF5), so a longer preamble
    /// gives it more chances, not more ways to fail: its symbols together
    /// weigh as much as the minimum.
    pub preamble_weight: f64,
}

impl Rx {
    /// Sum the hazard up to `now`. Returns when the packet was lost, if it
    /// was.
    fn accrue(&mut self, now: Instant) -> Option<Instant> {
        if self.rate == f64::INFINITY {
            return Some(self.since);
        }
        let symbol_us = SYMBOL_TIME.as_micros() as f64;
        let mut t = self.since;
        for (until, weight) in [(self.preamble_end, self.preamble_weight), (now, 1.0)] {
            let e = until.min(now);
            if e <= t {
                continue;
            }
            let per_us = self.rate * weight / symbol_us;
            let h = e.since(t).as_micros() as f64 * per_us;
            if self.spent + h >= self.budget {
                let us = ((self.budget - self.spent) / per_us).ceil() as u64;
                return Some((t + Duration::from_micros(us)).min(e));
            }
            self.spent += h;
            t = e;
        }
        self.since = now;
        None
    }
}

impl Sim {
    pub(crate) fn air_start(&mut self, slot: u32, g: u32) {
        if !self.slot_valid(slot, g) {
            return;
        }
        let tx = self.txs[slot as usize].as_mut().expect("valid slot");
        tx.on_air = true;
        let (src, tx_key, end, start) = (tx.src, tx.key, tx.end, tx.start);
        let ch = tx.frame.channel;
        let preamble = tx.frame.preamble_symbols;
        let packet = Packet {
            preamble_end: start + SYMBOL_TIME.mul(preamble as u64),
            header_end: start + header_end(preamble),
            preamble_weight: (MIN_PREAMBLE_SYMBOLS as f64 / preamble as f64).min(1.0),
        };
        let air = end.since(start).as_micros();
        let frame = tx.frame;
        let decoded = WirePacket::decode(frame.payload()).ok();
        let msg = match decoded {
            Some(WirePacket::Data(d)) => Some((d.msg_id(), d.hops)),
            Some(WirePacket::Ack(a)) => Some((a.msg_id, 0)),
            _ => None,
        };
        let to = match decoded {
            Some(WirePacket::Data(d)) => {
                Some((key(d.dst_hop), if src < self.relays() { 0 } else { 2 }))
            }
            Some(WirePacket::Ack(a)) if src < self.relays() => Some((key(a.to), 1)),
            _ => None,
        };
        let beat = match decoded {
            Some(WirePacket::Heartbeat(h)) => Some(h.sleepy),
            _ => None,
        };
        if let Some(WirePacket::Data(d)) = decoded {
            if let Some(&n) = self.sleepy_index.get(&key(d.dst_hop)) {
                let st = &self.nodes[n as usize];
                let asleep = st.asleep_at(self.now);
                self.to_sleepy.insert(tx_key, (n, asleep));
                if asleep && self.now >= self.settle {
                    let anchor = self.relay(n).anchor().map(|a| a.id);
                    let by = usize::from(anchor != Some(d.src_hop));
                    // The anchor's retry after the node slept on an ACK
                    // that was lost: three attempts and their waits.
                    let late =
                        usize::from(self.now.since(st.asleep_since) >= Duration::from_secs(5));
                    self.metrics.sleep.to_asleep[by][late] += 1;
                }
            }
        }
        let m = &mut self.metrics;
        m.nodes[src as usize].tx_frames += 1;
        m.nodes[src as usize].airtime_us += air;
        m.nodes[src as usize]
            .airtime_by_type
            .add(decoded.as_ref(), air);
        let chm = &mut m.channels[ch.index() as usize];
        chm.tx_frames += 1;
        chm.airtime_us += air;
        chm.airtime_by_type.add(decoded.as_ref(), air);
        if let Some((id, hops)) = msg {
            self.message_frame(id, hops, air);
        }
        match decoded {
            Some(WirePacket::Data(d)) => {
                self.answer_of
                    .insert(tx_key, (d.msg_id().0, key(d.src_hop)));
            }
            Some(WirePacket::Ack(a)) if src < self.relays() => {
                let k = (src, a.msg_id.0, key(a.to));
                if let Some((deadline, leaf)) = self.awaiting.remove(&k) {
                    let answer = if self.now > deadline {
                        2
                    } else if a.refused {
                        1
                    } else {
                        0
                    };
                    self.metrics.addressed.replies[leaf as usize][answer] += 1;
                }
            }
            _ => {}
        }
        if let Some(d) = to.and_then(|(name, kind)| self.relay_index.get(&name).map(|&d| (d, kind)))
        {
            let st = &self.nodes[d.0 as usize];
            let state = if st.resting != Some(ch) {
                4
            } else if st.ex.as_ref().is_some_and(|e| e.deaf) || self.now < st.listen_from {
                3
            } else if matches!(st.lock, Lock::Rx(_)) {
                2
            } else {
                1
            };
            self.addressed.insert(tx_key, (d.0, d.1, state));
        }
        self.push(end, crate::sim::Ev::AirEnd(slot, g));
        if !self.nodes[src as usize].present {
            return; // A dark leaf transmits into a tunnel.
        }
        for i in 0..self.links.row(src).len() {
            let l = self.links.row(src)[i];
            let dbm = self.links.rx_dbm(&l, ch);
            let a = Arrival {
                key: tx_key,
                channel: ch.index(),
                dbm,
                mw: phy::mw(dbm),
                end,
                lora: true,
            };
            if let Some(sleepy) = beat {
                self.nodes[l.rx as usize]
                    .beats
                    .push(start, end, ch.index(), sleepy);
            }
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
                        if !r.threshold_ok {
                            self.metrics.disagreement(b, r.channel, true);
                        }
                        got.push((b, r.mw, r.max_i_mw));
                    }
                }
            }
        }
        if let Some((n, asleep)) = self.to_sleepy.remove(&tx.key) {
            let fate = if got.iter().any(|g| g.0 == n) {
                0
            } else if asleep {
                1
            } else {
                2
            };
            if self.now >= self.settle {
                self.metrics.sleep.to_sleepy[fate] += 1;
            }
        }
        let heartbeat = matches!(
            WirePacket::decode(tx.frame.payload()),
            Ok(WirePacket::Heartbeat(h)) if !h.sleepy
        );
        if heartbeat && self.now >= self.settle {
            let (start, end, c) = (tx.start, tx.end, ch.index());
            for i in 0..self.links.row(tx.src).len() {
                let l = self.links.row(tx.src)[i];
                let b = l.rx;
                let st = &self.nodes[b as usize];
                // Only where it arrives above sensitivity on average.
                let audible = l.base_dbm as f64 >= self.sens_dbm;
                let off = !st.alive || !st.present || st.asleep_at(start);
                if !audible || off || st.resting != Some(ch) {
                    continue;
                }
                let h = &mut self.metrics.heartbeats;
                h.candidates += 1;
                if got.iter().any(|g| g.0 == b) {
                    h.received += 1;
                } else if st.beats.count(start, end, c) > 1 {
                    h.lost_with_heartbeat += 1;
                }
            }
        }
        let answer = self.answer_of.remove(&tx.key);
        if let Some((d, kind, state)) = self.addressed.remove(&tx.key) {
            let fate = if got.iter().any(|g| g.0 == d) {
                0
            } else {
                state
            };
            if let (0, Some((msg, to))) = (fate, answer) {
                if kind != 1 {
                    let deadline = self.now + self.relay(d).config().ack_timeout;
                    self.awaiting.insert((d, msg, to), (deadline, kind == 2));
                }
            }
            self.metrics.addressed.counts[kind as usize][fate as usize] += 1;
            let beats = &self.nodes[d as usize].beats;
            let (k, f, c) = (kind as usize, fate as usize, ch.index());
            if beats.during(tx.start, tx.end, c, true) {
                self.metrics.addressed.with_beat[k][f] += 1;
            }
            if beats.during(tx.start, tx.end, c, false) {
                self.metrics.addressed.with_heartbeat[k][f] += 1;
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
            let heartbeat = match WirePacket::decode(&bytes[..len]) {
                Ok(WirePacket::Heartbeat(h)) if b < self.relays() => Some(h.id),
                _ => None,
            };
            let before = heartbeat.map(|p| self.table_holds(b, p));
            self.deliver(
                b,
                Event::Received {
                    channel: ch,
                    rssi_dbm: rssi,
                    snr_db: snr,
                    bytes: &bytes[..len],
                },
            );
            if let (Some(p), Some((false, len))) = (heartbeat, before) {
                use bark_proto::tables::neighbours::NEIGHBOURS;
                if len >= NEIGHBOURS {
                    let added = self.table_holds(b, p).0;
                    let c = &mut self.metrics.nodes[b as usize].core;
                    if added {
                        c.evictions += 1;
                    } else {
                        c.rejections += 1;
                    }
                }
            }
        }
    }

    /// A signal reaches node `b`. `packet` is set for transmissions.
    pub(crate) fn arrive(&mut self, b: u32, a: Arrival, packet: Option<Packet>) {
        let now = self.now;
        let sens = self.sens_dbm;
        let cap = self.radio.capture_db;
        if !self.nodes[b as usize].alive || !self.nodes[b as usize].present {
            return;
        }
        self.settle(b);
        let st = &mut self.nodes[b as usize];
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
        let mut locked = false;
        if let (Some(p), 0) = (packet, offset) {
            self.metrics.candidate(b, a.channel);
            let outcome = if st.asleep_at(now) {
                Some(Outcome::Deaf(Deaf::Asleep))
            } else if let Some(ex) = st.ex.as_ref().filter(|e| e.deaf) {
                Some(Outcome::Deaf(ex.reason_at(now)))
            } else if now < st.listen_from {
                Some(Outcome::Deaf(Deaf::Retune))
            } else {
                match st.lock {
                    Lock::Corrupt(_) => Some(Outcome::Deaf(Deaf::Corrupt)),
                    Lock::Idle => {
                        locked = true;
                        None
                    }
                    Lock::Rx(r) if now < r.preamble_end && a.dbm >= r.dbm + cap - DB_EPS => {
                        // Stronger preamble during the locked one's: steal.
                        self.metrics.outcome(b, r.channel, Outcome::LostCapture);
                        locked = true;
                        None
                    }
                    Lock::Rx(_) => Some(Outcome::LostCapture),
                }
            };
            if let Some(o) = outcome {
                self.metrics.outcome(b, a.channel, o);
            }
            if locked {
                let st = &mut self.nodes[b as usize];
                let budget = st.rx_rng.exponential(1.0);
                st.lock = Lock::Rx(Rx {
                    key: a.key,
                    channel: a.channel,
                    dbm: a.dbm,
                    mw: a.mw,
                    preamble_end: p.preamble_end,
                    header_end: p.header_end,
                    end: a.end,
                    preamble_weight: p.preamble_weight,
                    max_i_mw: 0.0,
                    spent: 0.0,
                    budget,
                    since: now,
                    rate: 0.0,
                    cause: Loss::Noise,
                    threshold_ok: true,
                    rated_for: None,
                });
            }
        }
        // A new lock, or a new signal raising interference on the one held.
        self.rate_changed(b);
        if observed {
            self.observe(b, a.channel);
        }
    }

    /// A signal leaves node `b`.
    pub(crate) fn remove_arrival(&mut self, b: u32, key: u64) {
        self.settle(b);
        let st = &mut self.nodes[b as usize];
        if let Some(i) = st.arrivals.iter().position(|a| a.key == key) {
            let ch = st.arrivals.swap_remove(i).channel;
            self.rate_changed(b);
            if self.nodes[b as usize].observer.is_some() {
                self.observe(b, ch);
            }
        }
    }

    /// Bring `b`'s lock up to now, and if its packet has been lost, count
    /// it and free or deafen the receiver (module doc, step 3).
    pub(crate) fn settle(&mut self, b: u32) {
        let now = self.now;
        let st = &mut self.nodes[b as usize];
        let Lock::Rx(mut r) = st.lock else {
            return;
        };
        let Some(lost_at) = r.accrue(now) else {
            st.lock = Lock::Rx(r);
            return;
        };
        st.lock = if lost_at < r.header_end {
            Lock::Idle
        } else {
            Lock::Corrupt(r.end)
        };
        let o = match r.cause {
            Loss::Noise => Outcome::BelowSensitivity,
            Loss::CoChannel => Outcome::SinrFail { adjacent: false },
            Loss::Adjacent => Outcome::SinrFail { adjacent: true },
        };
        self.metrics.outcome(b, r.channel, o);
        if r.threshold_ok {
            self.metrics.disagreement(b, r.channel, false);
        }
    }

    /// The signals at `b` have changed: give its lock, already settled to
    /// now, the hazard of the present interference.
    fn rate_changed(&mut self, b: u32) {
        let st = &mut self.nodes[b as usize];
        let (Lock::Rx(mut r), Some(rest)) = (st.lock, st.resting) else {
            return;
        };
        if st.ex.as_ref().is_some_and(|e| e.deaf) {
            return;
        }
        let (co, adj) = interference(&st.arrivals, rest.index(), r.key, &self.rej);
        if r.rated_for == Some((co, adj)) {
            return;
        }
        r.rated_for = Some((co, adj));
        (r.rate, r.cause) = self.curve.hazard(r.dbm, co, adj);
        r.max_i_mw = r.max_i_mw.max(co + adj);
        r.threshold_ok &= self.curve.passes_threshold(r.dbm, co + adj);
        st.lock = Lock::Rx(r);
        self.settle(b);
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
