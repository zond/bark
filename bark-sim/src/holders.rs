//! What became of a sleepy node's beats at the holders of messages for it
//! (`sim::HolderWatch`, in the per-message table): which holder queued
//! which message, each beat's fate at each holder, and the DATA a holder
//! sent with the long preamble. Counting only: nothing here changes a
//! draw or a decision.

use crate::metrics::Outcome;
use crate::phy::Deaf;
use crate::sim::{key, BeatFate, Fate, Sim};
use bark_proto::{MsgId, NodeId, Trace};
use std::collections::HashMap;

/// What is followed of the holders between events.
#[derive(Default)]
pub(crate) struct HolderBeats {
    /// Messages queued, by holder and destination node's ID.
    queued_for: HashMap<(u32, u64), Vec<u32>>,
    /// The holders of messages queued for each node, by the node's ID.
    holders_of: HashMap<u64, Vec<u32>>,
    /// Beats on the air of a node with messages queued: key to each
    /// holder and the beat's fate there so far.
    on_air: HashMap<u64, Vec<(u32, Option<BeatFate>)>>,
}

impl Sim {
    /// A trace of `node`'s core.
    pub(crate) fn holders_trace(&mut self, node: u32, t: &Trace) {
        match *t {
            Trace::HeartbeatSkipped { channel } => {
                let r = &self.relays[node as usize];
                if r.sleepy() && r.anchor().is_some() && channel == r.home() {
                    let id = key(r.id());
                    for h in self.holders_waiting(id) {
                        self.watch_note(h, id, BeatFate::Skipped, false);
                    }
                }
            }
            Trace::Queued { msg_id, node: to } => self.holder_queued(node, msg_id, to),
            _ => {}
        }
    }

    /// `holder` queued `msg_id` for its node `to`.
    fn holder_queued(&mut self, holder: u32, msg_id: MsgId, to: NodeId) {
        let Some(tag) = self.msg_tag(msg_id.0) else {
            return;
        };
        let relays = self.relays();
        let r = &self.relays[holder as usize];
        let dst = self.origins[tag as usize].dst as usize;
        let w = &mut self.origins[tag as usize].watch;
        w.queued_at = Some(self.now);
        w.known = r.heard_nodes().get(to).is_some();
        w.anchored = self.relays[dst].anchor().is_some_and(|a| a.id == r.id());
        let row = self.links.row(dst as u32);
        let to_relay = row.iter().filter(|l| l.rx < relays);
        w.best_dbm = to_relay.map(|l| l.base_dbm).reduce(f32::max);
        w.holder_dbm = row.iter().find(|l| l.rx == holder).map(|l| l.base_dbm);
        let holders = self.holder_beats.holders_of.entry(key(to)).or_default();
        if !holders.contains(&holder) {
            holders.push(holder);
        }
        self.origins[tag as usize].holder = Some(holder);
        self.holder_beats
            .queued_for
            .entry((holder, key(to)))
            .or_default()
            .push(tag);
    }

    /// `receiver` heard a heartbeat of `id`: a beat its holder heard, for
    /// each message it holds for that node.
    pub(crate) fn holder_heard(&mut self, receiver: u32, id: NodeId) {
        if let Some(tags) = self.holder_beats.queued_for.get_mut(&(receiver, key(id))) {
            let origins = &mut self.origins;
            tags.retain(|&t| origins[t as usize].fate == Fate::Unaccounted);
            for &t in tags.iter() {
                origins[t as usize].holder_beats += 1;
            }
        }
    }

    /// A beat of the sleepy node `id` reaches the air in `slot`: note, for
    /// each holder of messages for it, what is already known of its fate
    /// there.
    pub(crate) fn holders_beat_starts(&mut self, slot: u32, id: u64, anchorless: bool) {
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
                Some(BeatFate::Anchorless)
            } else if !present {
                Some(BeatFate::Dark)
            } else if !linked {
                Some(BeatFate::Far)
            } else if self.nodes[h as usize].resting != Some(ch) {
                Some(BeatFate::OffChannel)
            } else {
                None
            };
            watch.push((h, fate));
        }
        self.holder_beats.on_air.insert(tx_key, watch);
    }

    /// `b` lost the frame `tx_key` to `o`: if it is a watched beat and `b`
    /// a holder, the beat's fate there.
    pub(crate) fn holders_beat_lost(&mut self, tx_key: u64, b: u32, o: Outcome) {
        let w = &mut self.holder_beats.on_air;
        let Some(w) = (!w.is_empty()).then(|| w.get_mut(&tx_key)).flatten() else {
            return;
        };
        for (_, fate) in w.iter_mut().filter(|(h, f)| *h == b && f.is_none()) {
            *fate = Some(match o {
                Outcome::BelowSensitivity => BeatFate::Weak,
                Outcome::LostCapture
                | Outcome::LostToLater
                | Outcome::SinrFail { .. }
                | Outcome::Deaf(Deaf::Corrupt) => BeatFate::Collided,
                Outcome::Deaf(Deaf::Tx | Deaf::HalfDuplex) => BeatFate::Transmitting,
                Outcome::Deaf(Deaf::Cad | Deaf::Rssi | Deaf::Retune) => BeatFate::Checking,
                _ => BeatFate::Other,
            });
        }
    }

    /// The frame `tx_key` from `src` left the air, received by `got`: if
    /// it is a watched beat, its fate at each holder is counted.
    pub(crate) fn holders_beat_ends(&mut self, tx_key: u64, src: u32, got: &[(u32, f64, f64)]) {
        let w = &mut self.holder_beats.on_air;
        let Some(w) = (!w.is_empty()).then(|| w.remove(&tx_key)).flatten() else {
            return;
        };
        let id = key(self.relay(src).id());
        for (h, fate) in w {
            let received = got.iter().any(|g| g.0 == h);
            let fate = if received {
                BeatFate::Received
            } else {
                fate.unwrap_or(BeatFate::Other)
            };
            let elsewhere = !received && got.iter().any(|g| g.0 != h && !self.relay(g.0).sleepy());
            self.watch_note(h, id, fate, elsewhere);
        }
    }

    /// A DATA of the message `msg` went from `src` with the long preamble.
    pub(crate) fn note_long_try(&mut self, msg: u32, src: u32) {
        if let Some(tag) = self.msg_tag(msg) {
            let o = &mut self.origins[tag as usize];
            if o.holder == Some(src) {
                o.watch.long_tries += 1;
            }
        }
    }

    /// The holders with messages for the node `id` not yet accounted for.
    fn holders_waiting(&mut self, id: u64) -> Vec<u32> {
        let w = &mut self.holder_beats;
        let Some(holders) = w.holders_of.get_mut(&id) else {
            return Vec::new();
        };
        let (queued, origins) = (&w.queued_for, &self.origins);
        holders.retain(|&h| {
            queued.get(&(h, id)).is_some_and(|tags| {
                tags.iter()
                    .any(|&t| origins[t as usize].fate == Fate::Unaccounted)
            })
        });
        holders.clone()
    }

    /// Count a beat of the node `id` with `fate` at `holder` for each
    /// message the holder holds for it.
    fn watch_note(&mut self, holder: u32, id: u64, fate: BeatFate, elsewhere: bool) {
        if let Some(tags) = self.holder_beats.queued_for.get(&(holder, id)) {
            for &t in tags {
                let o = &mut self.origins[t as usize];
                if o.fate == Fate::Unaccounted {
                    o.watch.beats[fate as usize] += 1;
                    o.watch.elsewhere += u32::from(elsewhere);
                }
            }
        }
    }
}
