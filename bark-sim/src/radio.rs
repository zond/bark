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

use crate::metrics::{FirstTry, Outcome};
use crate::phy::{self, Deaf, Loss};
use crate::sim::{key, Arrival, ExKind, FrameKind, Lock, Rx, Sim, ToSleepy};
use crate::walk::MovingLink;
use bark_proto::radio::{header_end, CHANNELS, MIN_PREAMBLE_SYMBOLS, SYMBOL_TIME};
use bark_proto::sniff;
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
            Some(WirePacket::Data(d)) => Some((d.msg_id(), d.hops, FrameKind::Data)),
            Some(WirePacket::Ack(a)) if a.refused => Some((a.msg_id, 0, FrameKind::Refused)),
            Some(WirePacket::Ack(a)) => Some((a.msg_id, 0, FrameKind::Taken)),
            _ => None,
        };
        let to = match decoded {
            Some(WirePacket::Data(d)) => {
                Some((key(d.dst_hop), if src < self.relays() { 0 } else { 2 }))
            }
            Some(WirePacket::Ack(a)) if src < self.relays() => Some((key(a.to), 1)),
            Some(WirePacket::Heartbeat(h)) if h.sleepy => {
                self.relay(src).anchor().map(|a| (key(a.id), 3))
            }
            _ => None,
        };
        let beat = match decoded {
            // A beat that opens a window: the sleepy bit alone.
            Some(WirePacket::Heartbeat(h)) => Some(h.sleepy && !h.anchorless),
            _ => None,
        };
        if let Some(WirePacket::Heartbeat(h)) = decoded.filter(|_| src >= self.relays()) {
            // Not its copies with the long preamble on anchorless
            // neighbours' channels (spec "Beats").
            let short = preamble < sniff::LONG_PREAMBLE_SYMBOLS;
            if h.sleepy && (short || h.anchorless) {
                self.watch_beat(slot, key(h.id), h.anchorless);
            }
        }
        if matches!(decoded, Some(WirePacket::Heartbeat(h)) if !h.sleepy) && self.now >= self.settle
        {
            self.beat_losses.insert(tx_key, Vec::new());
        }
        if let Some(WirePacket::Data(d)) = decoded {
            if let Some(&n) = self.sleepy_index.get(&key(d.dst_hop)) {
                let st = &self.nodes[n as usize];
                // Receiving duty-cycled, it catches the long preamble only.
                let short = preamble < sniff::LONG_PREAMBLE_SYMBOLS;
                let sniffing = st.sniffing_at(self.now).is_some();
                let deaf = sniffing && short;
                let since = st.sniff_since;
                let to = ToSleepy {
                    node: n,
                    deaf,
                    absent: !st.alive || !st.present,
                    holder: src,
                    msg: d.msg_id().0,
                    window: self.window_of(n),
                    lost: None,
                };
                self.to_sleepy.insert(tx_key, to);
                if !short {
                    self.note_long_try(d.msg_id().0, src);
                }
                if deaf && self.now >= self.settle {
                    let anchor = self.relay(n).anchor().map(|a| a.id);
                    let by = usize::from(anchor != Some(d.src_hop));
                    // The anchor's retry after the node slept on an ACK
                    // that was lost: three attempts and their waits.
                    let late = usize::from(self.now.since(since) >= Duration::from_secs(5));
                    self.metrics.sleep.to_deaf[by][late] += 1;
                }
            }
        }
        if beat == Some(true) && src >= self.relays() {
            let i = (src - self.relays()) as usize;
            self.beat_took[i].0 = self.now;
        }
        let m = &mut self.metrics;
        m.nodes[src as usize].tx_frames += 1;
        m.nodes[src as usize].airtime_us += air;
        if self.now >= self.settle {
            m.nodes[src as usize].settled_airtime_us += air;
        }
        m.nodes[src as usize]
            .airtime_by_type
            .add(decoded.as_ref(), preamble, air);
        let chm = &mut m.channels[ch.index() as usize];
        chm.tx_frames += 1;
        chm.airtime_us += air;
        chm.airtime_by_type.add(decoded.as_ref(), preamble, air);
        if let Some((id, hops, kind)) = msg {
            let to_dst = match decoded {
                Some(WirePacket::Data(d)) if d.dst_hop == d.dst_id => Some(d.time_left()),
                _ => None,
            };
            self.message_frame(id, hops, air, src, kind, to_dst);
        }
        match decoded {
            Some(WirePacket::Data(d)) => {
                self.answer_of
                    .insert(tx_key, (d.msg_id().0, key(d.src_hop)));
                if self
                    .relay_index
                    .get(&key(d.dst_hop))
                    .is_some_and(|&r| r < self.relays())
                {
                    self.data_hops
                        .insert((d.msg_id().0, key(d.dst_hop)), d.hops);
                }
            }
            Some(WirePacket::Ack(a)) if src < self.relays() => {
                let hops = self.data_hops.remove(&(a.msg_id.0, key(a.from)));
                if a.refused {
                    let w = (u8::BITS - a.wait.leading_zeros()).min(7) as usize;
                    self.metrics.addressed.refusal_steps[w] += 1;
                    if let Some(h) = hops {
                        let k = match h {
                            0 => 0,
                            1 => 1,
                            2 | 3 => 2,
                            _ => 3,
                        };
                        self.metrics.addressed.refusal_by_hops[k][w] += 1;
                    }
                }
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
        // Links with a walker at either end, worked out now and kept for
        // the frame's end.
        let Some(walks) = &mut self.walks else {
            return;
        };
        let extra = walks.links_from(src, start, ch, &self.xy, &self.sites);
        for l in &extra {
            let a = Arrival {
                key: tx_key,
                channel: ch.index(),
                dbm: l.dbm,
                mw: phy::mw(l.dbm),
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
        if let Some(tx) = self.txs[slot as usize].as_mut() {
            tx.extra = extra;
        }
    }

    /// A beat of the sleepy node `id` reaches the air in `slot`: note, for
    /// each holder of messages for it, what is already known of its fate
    /// there (`crate::sim::HolderWatch`).
    fn watch_beat(&mut self, slot: u32, id: u32, anchorless: bool) {
        use crate::sim::beat_fate as f;
        let holders = self.holders_waiting(id);
        if holders.is_empty() {
            return;
        }
        let tx = self.txs[slot as usize].as_ref().expect("valid slot");
        let (src, ch, tx_key) = (tx.src, tx.frame.channel, tx.key);
        let receivers = self.links.row(src).len() + tx.extra.len();
        let present = self.nodes[src as usize].present;
        let mut watch = Vec::new();
        for h in holders {
            let linked = (0..receivers).any(|i| {
                let (b, dbm) = self.receiver(src, &tx.extra, i);
                b == h && dbm as f64 >= self.sens_dbm
            });
            let fate = if anchorless {
                Some(f::ANCHORLESS)
            } else if !present {
                Some(f::DARK)
            } else if !linked {
                Some(f::FAR)
            } else if self.nodes[h as usize].resting != Some(ch) {
                Some(f::OFF_CHANNEL)
            } else {
                None
            };
            watch.push((h, fate));
        }
        self.beat_watch.insert(tx_key, watch);
    }

    /// The `i`-th receiver of a frame from `src`, `extra` its links with
    /// walkers: its node and its power before fading.
    fn receiver(&self, src: u32, extra: &[MovingLink], i: usize) -> (u32, f32) {
        let row = self.links.row(src);
        match row.get(i) {
            Some(l) => (l.rx, l.base_dbm),
            None => {
                let l = extra[i - row.len()];
                (l.rx, l.base_dbm)
            }
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
        let receivers = self.links.row(tx.src).len() + tx.extra.len();
        for i in 0..receivers {
            let (b, _) = self.receiver(tx.src, &tx.extra, i);
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
                        if let (Some(at), Some((_, until))) = (r.caught, st.sniff) {
                            // The radio received in full from the detection,
                            // and its cycle starts again now.
                            let from = at.max(self.settle);
                            if self.now > from {
                                let m = &mut self.metrics.nodes[b as usize];
                                m.sniff_rx_us += self.now.since(from).as_micros();
                            }
                            st.sniff = Some((self.now, until));
                        }
                        got.push((b, r.mw, r.max_i_mw));
                    }
                }
            }
        }
        if let Some(w) = self.beat_watch.remove(&tx.key) {
            use crate::sim::beat_fate as f;
            let id = key(self.relay(tx.src).id());
            for (h, fate) in w {
                let received = got.iter().any(|g| g.0 == h);
                let fate = if received {
                    f::RECEIVED
                } else {
                    fate.unwrap_or(f::OTHER)
                };
                let elsewhere = !received
                    && got
                        .iter()
                        .any(|g| g.0 != h && !self.relay(g.0).config().sleepy);
                self.watch_note(h, id, fate, elsewhere);
            }
        }
        if let Some(t) = self.to_sleepy.remove(&tx.key) {
            let received = got.iter().any(|g| g.0 == t.node);
            let (fate, how) = if received {
                let i = (t.node - self.relays()) as usize;
                self.beat_took[i].1 = self.now;
                (0, FirstTry::Received)
            } else if t.deaf {
                (1, FirstTry::Deaf)
            } else if t.absent {
                (2, FirstTry::Absent)
            } else {
                let how = match t.lost {
                    Some(Outcome::BelowSensitivity) => FirstTry::Weak,
                    Some(
                        Outcome::LostCapture
                        | Outcome::SinrFail { .. }
                        | Outcome::Deaf(Deaf::Corrupt),
                    ) => FirstTry::Collided,
                    Some(Outcome::Deaf(Deaf::Sniff)) => FirstTry::Deaf,
                    Some(Outcome::Deaf(_)) => FirstTry::Busy,
                    _ => FirstTry::Unheard,
                };
                (2, how)
            };
            if self.now >= self.settle {
                self.metrics.sleep.to_sleepy[fate] += 1;
            }
            self.first_try
                .entry((t.holder, t.msg))
                .or_insert((t.window, how));
        }
        let heartbeat = matches!(
            WirePacket::decode(tx.frame.payload()),
            Ok(WirePacket::Heartbeat(h)) if !h.sleepy
        );
        let losses = self.beat_losses.remove(&tx.key).unwrap_or_default();
        if heartbeat && self.now >= self.settle {
            let (start, end, c) = (tx.start, tx.end, ch.index());
            let src_id = self.relay(tx.src).id();
            for i in 0..receivers {
                let (b, base_dbm) = self.receiver(tx.src, &tx.extra, i);
                let st = &self.nodes[b as usize];
                // Only where it arrives above sensitivity on average.
                let audible = base_dbm as f64 >= self.sens_dbm;
                let anchored_here =
                    b >= self.relays() && self.relay(b).anchor().is_some_and(|a| a.id == src_id);
                // Receiving duty-cycled, it catches the long preamble only.
                let short = tx.frame.preamble_symbols < sniff::LONG_PREAMBLE_SYMBOLS;
                let sniffing = st.sniffing_at(start).is_some() || st.sniffing_at(end).is_some();
                if audible && anchored_here && st.alive && st.present && st.resting == Some(ch) {
                    self.metrics.sleep.anchor_beats[usize::from(sniffing && short)] += 1;
                }
                let st = &self.nodes[b as usize];
                let off = !st.alive || !st.present || (short && st.sniffing_at(start).is_some());
                if !audible || off || st.resting != Some(ch) {
                    continue;
                }
                let h = &mut self.metrics.heartbeats;
                h.candidates += 1;
                if got.iter().any(|g| g.0 == b) {
                    h.received += 1;
                    continue;
                }
                if st.beats.count(start, end, c) > 1 {
                    h.lost_with_heartbeat += 1;
                }
                match losses.iter().find(|l| l.0 == b).map(|l| l.1) {
                    Some(Outcome::LostCapture | Outcome::SinrFail { .. }) => h.lost_collided += 1,
                    Some(Outcome::BelowSensitivity) => h.lost_weak += 1,
                    Some(Outcome::Deaf(_)) => h.lost_deaf += 1,
                    _ => {}
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
                if kind == 0 || kind == 2 {
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
            // A duty-cycled receiver takes the packet only once a window
            // has detected its preamble.
            let caught = st.sniffing_at(now).map(|t0| {
                sniff::detects(t0.as_micros(), now.as_micros(), p.preamble_end.since(now))
            });
            let outcome = if caught == Some(None) {
                Some(Outcome::Deaf(Deaf::Sniff))
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
                        self.note_to_sleepy(r.key, b, Outcome::LostCapture);
                        locked = true;
                        None
                    }
                    Lock::Rx(_) => Some(Outcome::LostCapture),
                }
            };
            if let Some(o) = outcome {
                self.metrics.outcome(b, a.channel, o);
                self.note_to_sleepy(a.key, b, o);
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
                    caught: caught.flatten().map(Instant::from_micros),
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
        self.note_to_sleepy(r.key, b, o);
        if r.threshold_ok {
            self.metrics.disagreement(b, r.channel, false);
        }
    }

    /// Note why the DATA `key` to a sleepy node was lost there, if `b` is
    /// that node.
    fn note_to_sleepy(&mut self, key: u64, b: u32, o: Outcome) {
        self.note_beat_loss(key, b, o);
        if let Some(t) = self.to_sleepy.get_mut(&key).filter(|t| t.node == b) {
            t.lost = Some(o);
        }
    }

    /// Note why `b` lost the frame `key`, if it is a heartbeat followed
    /// for [`crate::metrics::HeartbeatFates`].
    pub(crate) fn note_beat_loss(&mut self, key: u64, b: u32, o: Outcome) {
        if let Some(w) = self.beat_watch.get_mut(&key) {
            use crate::sim::beat_fate as f;
            for (_, fate) in w.iter_mut().filter(|(h, f)| *h == b && f.is_none()) {
                *fate = Some(match o {
                    Outcome::BelowSensitivity => f::WEAK,
                    Outcome::LostCapture
                    | Outcome::SinrFail { .. }
                    | Outcome::Deaf(Deaf::Corrupt) => f::COLLIDED,
                    Outcome::Deaf(Deaf::Tx | Deaf::HalfDuplex) => f::TRANSMITTING,
                    Outcome::Deaf(Deaf::Cad | Deaf::Rssi | Deaf::Retune) => f::CHECKING,
                    _ => f::OTHER,
                });
            }
        }
        if let Some(l) = self.beat_losses.get_mut(&key) {
            l.push((b, o));
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
