//! What a sender knew of its next hop when it chose it, against what the
//! link was: one CSV row per DATA to a relay after the settle time, with
//! `--margin FILE` (`docs/margin-2026-11-02.md`). Counting only: nothing
//! here changes a draw or a decision, and off it costs nothing.
//!
//! The sender's side is its neighbour-table entry for the receiver at the
//! moment the frame reaches the air: its averaged RSSI, its noise floor,
//! the entry's age, demotion and failures. The link's side is the stored
//! base power, the power at the receiver on the frame's channel with its
//! fade, the fade the sender would see on its own home channel at the same
//! moment (the pair's process there), and the receiver's own floor. The
//! fade is a pure function of the pair, the channel and the step, so
//! reading it on another channel moves no process.
//!
//! The alternatives are the sender's other live, unbanned, undemoted relay
//! entries nearer the destination than the sender itself and no nearer
//! than the chosen one: a nearer one with margin would have been chosen
//! had it not been tried, set aside or the parent, which the simulator
//! cannot see. Of them the row keeps the one with the highest averaged
//! RSSI and, for each of several thresholds above the floor, the one
//! nearest the destination whose average clears it, each with its link's
//! power now on its home channel and whether it is listening there.

use std::collections::HashMap;
use std::io::Write;

use bark_proto::geo::LocalPlane;
use bark_proto::radio::{Channel, SYMBOL_TIME};
use bark_proto::tables::neighbours::Neighbours;
use bark_proto::tables::neighbours::{Neighbour, NeighbourParams};
use bark_proto::wire::Data;

use crate::sim::{key, Lock, Sim};

/// Thresholds above the noise floor, dB of averaged packet RSSI, for the
/// nearest alternative that clears each.
pub const THRESHOLDS_DB: [f32; 4] = [6.0, 8.0, 10.0, 12.0];

/// One alternative next hop: averaged RSSI, dBm; the power its link
/// carries now on its home channel, dBm, or NaN if no link is stored
/// (below the table's cut-off, or a walker); its distance to the
/// destination, m; whether it is listening and free on its home channel.
#[derive(Clone, Copy)]
struct Alt {
    est_dbm: f32,
    rx_dbm: f64,
    dist_m: f32,
    listening: bool,
}

impl Alt {
    fn csv(a: Option<Alt>) -> String {
        match a {
            Some(a) => format!(
                "{:.1},{:.1},{:.0},{}",
                a.est_dbm,
                a.rx_dbm,
                a.dist_m,
                u8::from(a.listening)
            ),
            None => ",,,".into(),
        }
    }
}

/// A row in the making: the sender's and the alternatives' side, gathered
/// when the frame reaches the air; the receiver's power when the arrivals
/// are placed; the fate at the frame's end.
pub(crate) struct Row {
    head: String,
    rx_dbm: Option<f64>,
    symbols: u64,
}

/// The log.
pub(crate) struct MarginLog {
    out: Box<dyn Write>,
    pending: HashMap<u64, Row>,
}

/// The CSV header.
const HEADER: &str = "t_s,kind,src,dst,msg,hops,ch,src_home,state,\
known,est_dbm,floor_src,margin,age_s,demoted,fails,\
base_dbm,base_back_dbm,fade_src_ch_db,floor_dst,d_src_m,d_dst_m,n_alt,n_alt_margin,\
best_est,best_rx,best_dist,best_listen,\
t6_est,t6_rx,t6_dist,t6_listen,t8_est,t8_rx,t8_dist,t8_listen,\
t10_est,t10_rx,t10_dist,t10_listen,t12_est,t12_rx,t12_dist,t12_listen,\
rx_dbm,p_noise_loss,fate,how\n";

impl Sim {
    /// Log every DATA to a relay after the settle time to `out`.
    pub fn margin_to(&mut self, mut out: Box<dyn Write>) {
        out.write_all(HEADER.as_bytes()).expect("margin log header");
        self.margin = Some(MarginLog {
            out,
            pending: HashMap::new(),
        });
    }

    /// The DATA `d` from `src` reaches the air on `ch` as frame `tx_key`;
    /// `state` is the receiver's state then (`metrics::Addressed`).
    pub(crate) fn margin_start(
        &mut self,
        tx_key: u64,
        src: u32,
        d: &Data<'_>,
        ch: Channel,
        state: u8,
        air_us: u64,
    ) {
        if self.margin.is_none() || self.now < self.settle {
            return;
        }
        let Some(&dst) = self.relay_index.get(&key(d.dst_hop)) else {
            return;
        };
        if dst >= self.relays() {
            return;
        }
        let kind = if src < self.relays() { 0 } else { 2 };
        let sender = self.relay(src);
        let floor_src = sender.busy().noise_floor_dbm();
        let src_home = sender.home();
        let plane = LocalPlane::new(self.origin.position(self.xy[src as usize]));
        let here = plane.point(self.origin.position(self.xy[src as usize]));
        let target = plane.point(d.dst_pos);
        let d_src = here.distance_m(target);
        let entry = sender.neighbours().find(|n| n.id == d.dst_hop).copied();
        let d_dst = plane
            .point(self.origin.position(self.xy[dst as usize]))
            .distance_m(target);
        let now = self.now;
        let params = NeighbourParams::from_config(sender.config());
        let demoted = |n: &Neighbour| {
            n.fail_count >= params.demote_after && now.since(n.demoted_at) < params.demotion
        };
        let sender_side = match &entry {
            Some(n) => format!(
                "1,{:.1},{},{},{:.1},{},{}",
                n.rssi,
                floor_src,
                u8::from(Neighbours::has_margin(n, floor_src)),
                now.since(n.last_heard).as_micros() as f64 / 1e6,
                u8::from(demoted(n)),
                n.fail_count
            ),
            None => format!("0,,{floor_src},,,,"),
        };
        // The link's base both ways; NaN where none is stored.
        let base = self
            .links
            .get(src, dst)
            .map_or(f64::NAN, |l| l.base_dbm as f64);
        let base_back = self
            .links
            .get(dst, src)
            .map_or(f64::NAN, |l| l.base_dbm as f64);
        let fade_src_ch = match self.links.get(src, dst).copied() {
            Some(l) => self.links.rx_dbm(&l, src_home) - base,
            None => f64::NAN,
        };
        let floor_dst = self.relay(dst).busy().noise_floor_dbm();

        // The alternatives.
        let sender = self.relay(src);
        let mut alts: Vec<(f32, f32, u32, Channel)> = Vec::new();
        for n in sender.neighbours() {
            if n.id == d.dst_hop || !n.relay() || now < n.banned_until || demoted(n) {
                continue;
            }
            let dist = plane.point(n.position).distance_m(target);
            if dist >= d_src || dist < d_dst {
                continue;
            }
            let Some(&node) = self.relay_index.get(&key(n.id)) else {
                continue;
            };
            alts.push((n.rssi, dist, node, n.home));
        }
        let n_alt = alts.len();
        let margin_dbm = Neighbours::margin_dbm(floor_src);
        let n_alt_margin = alts.iter().filter(|a| a.0 >= margin_dbm).count();
        let best = alts
            .iter()
            .copied()
            .max_by(|a, b| a.0.total_cmp(&b.0).then(b.1.total_cmp(&a.1)));
        let nearest_over = |x: f32| {
            alts.iter()
                .copied()
                .filter(|a| a.0 >= floor_src as f32 + x)
                .min_by(|a, b| a.1.total_cmp(&b.1).then(b.0.total_cmp(&a.0)))
        };
        let picks: Vec<Option<(f32, f32, u32, Channel)>> = std::iter::once(best)
            .chain(THRESHOLDS_DB.iter().map(|&x| nearest_over(x)))
            .collect();
        let mut cells = String::new();
        for p in picks {
            let alt = p.map(|(est, dist, node, home)| Alt {
                est_dbm: est,
                rx_dbm: match self.links.get(src, node).copied() {
                    Some(l) => self.links.rx_dbm(&l, home),
                    None => f64::NAN,
                },
                dist_m: dist,
                listening: self.listening_free(node, home),
            });
            cells.push(',');
            cells.push_str(&Alt::csv(alt));
        }

        let head = format!(
            "{:.3},{kind},{src},{dst},{},{},{},{},{state},{sender_side},{:.1},{:.1},{:.2},{floor_dst},{:.0},{:.0},{n_alt},{n_alt_margin}{cells}",
            now.as_micros() as f64 / 1e6,
            d.msg_id().0,
            d.hops,
            ch.index(),
            src_home.index(),
            base,
            base_back,
            fade_src_ch,
            d_src,
            d_dst,
        );
        let log = self.margin.as_mut().expect("on");
        log.pending.insert(
            tx_key,
            Row {
                head,
                rx_dbm: None,
                symbols: air_us / SYMBOL_TIME.as_micros(),
            },
        );
    }

    /// Whether `node` is listening on `home`, free, when asked.
    fn listening_free(&self, node: u32, home: Channel) -> bool {
        let st = &self.nodes[node as usize];
        st.alive
            && st.present
            && st.resting == Some(home)
            && !st.ex.as_ref().is_some_and(|e| e.deaf)
            && self.now >= st.listen_from
            && matches!(st.lock, Lock::Idle)
    }

    /// The frame `tx_key` is placed at `rx` at `dbm`, fade included.
    pub(crate) fn margin_rx(&mut self, tx_key: u64, rx: u32, dbm: f64) {
        let Some(log) = self.margin.as_mut() else {
            return;
        };
        if let Some(r) = log.pending.get_mut(&tx_key) {
            if self.addressed.get(&tx_key).is_some_and(|a| a.0 == rx) {
                r.rx_dbm = Some(dbm);
            }
        }
    }

    /// The frame `tx_key` has left the air with `fate` and, if lost, `how`
    /// (`metrics::Addressed`).
    pub(crate) fn margin_end(&mut self, tx_key: u64, fate: u8, how: Option<u8>) {
        let Some(log) = self.margin.as_mut() else {
            return;
        };
        let Some(r) = log.pending.remove(&tx_key) else {
            return;
        };
        let (rx, p_loss) = match r.rx_dbm {
            Some(dbm) => {
                let (rate, _) = self.curve.hazard(dbm, 0.0, 0.0);
                (dbm, 1.0 - (-rate * r.symbols as f64).exp())
            }
            None => (f64::NAN, 1.0),
        };
        let how = match how {
            Some(h) if fate != 0 => h as i8,
            _ => -1,
        };
        writeln!(log.out, "{},{rx:.1},{p_loss:.4},{fate},{how}", r.head).expect("margin log");
    }

    /// Whether a margin log is on and `now` is past the settle time.
    pub(crate) fn margin_on(&self) -> bool {
        self.margin.is_some() && self.now >= self.settle
    }
}
