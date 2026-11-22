//! What DATA to sleepy nodes meets (`metrics::SleepMetrics`): whether
//! the node could hear it, what lost it, the long preamble's frames, and
//! what became of the first DATA of each delivery that then waits for the
//! node's next beat. Counting only: nothing here changes a draw or a
//! decision.

use crate::metrics::{FirstTry, Outcome, ToSleepyFate, AFTER_BEAT, AFTER_TAKE};
use crate::phy::Deaf;
use crate::sim::{key, Sim};
use bark_proto::sniff;
use bark_proto::wire::Data;
use bark_proto::{Duration, Instant, Trace};
use std::collections::HashMap;

/// A DATA on the air to a sleepy node.
#[derive(Clone, Copy, Debug)]
struct OnAir {
    node: u32,
    /// It could not hear the frame when it started: it received
    /// duty-cycled and the frame's preamble was short.
    deaf: bool,
    /// It was dark or removed then.
    absent: bool,
    /// The sender, its holder.
    holder: u32,
    msg: u32,
    /// The window it went in (`Sim::window_of`).
    window: usize,
    /// Why it was lost at the node, if it was.
    lost: Option<Outcome>,
}

/// What is followed of DATA to sleepy nodes between events.
pub(crate) struct SleepyDataWatch {
    /// DATA on air addressed to a sleepy node, by key.
    on_air: HashMap<u64, OnAir>,
    /// Per sleepy node by index less the relays: the start of its last
    /// beat and the end of the last DATA for it that it received.
    beat_took: Vec<(Instant, Instant)>,
    /// A delivery's hand-over under way, by holder and message: the window
    /// it went in and what became of its first DATA
    /// (`SleepMetrics::waited`).
    first_try: HashMap<(u32, u32), (usize, FirstTry)>,
}

impl SleepyDataWatch {
    pub(crate) fn new(sleepy: usize) -> SleepyDataWatch {
        SleepyDataWatch {
            on_air: HashMap::new(),
            beat_took: vec![(Instant::ZERO, Instant::ZERO); sleepy],
            first_try: HashMap::new(),
        }
    }
}

impl Sim {
    /// The DATA `d` from `src` reaches the air as `tx_key`, with
    /// `preamble` symbols, for `air_us`.
    pub(crate) fn sleepy_data_starts(
        &mut self,
        tx_key: u64,
        src: u32,
        preamble: u16,
        air_us: u64,
        d: &Data<'_>,
    ) {
        let Some(&n) = self.sleepy_index.get(&key(d.dst_hop)) else {
            return;
        };
        let st = &self.nodes[n as usize];
        // Receiving duty-cycled, it catches the long preamble only.
        let short = preamble < sniff::LONG_PREAMBLE_SYMBOLS;
        let sniffing = st.sniffing_at(self.now).is_some();
        let deaf = sniffing && short;
        let since = st.sniff_since;
        let to = OnAir {
            node: n,
            deaf,
            absent: !st.alive || !st.present,
            holder: src,
            msg: d.msg_id().0,
            window: self.window_of(n),
            lost: None,
        };
        self.sleepy_data.on_air.insert(tx_key, to);
        if !short {
            self.note_long_try(d.msg_id().0, src);
            let r = self.relay(n);
            let anchor = r.anchor().map(|a| a.id).filter(|_| r.sleepy());
            if anchor.is_some() && d.dst_hop == d.dst_id {
                let by = usize::from(anchor != Some(d.src_hop));
                let l = &mut self.metrics.sleep.long_to_node[by];
                l[0] += 1;
                l[1] += air_us;
            }
        }
        if deaf && self.now >= self.settle {
            let anchor = self.relay(n).anchor().map(|a| a.id);
            let by = usize::from(anchor != Some(d.src_hop));
            // The anchor's retry after the node slept on an ACK that was
            // lost: three attempts and their waits.
            let late = usize::from(self.now.since(since) >= Duration::from_secs(5));
            self.metrics.sleep.to_deaf[by][late] += 1;
        }
    }

    /// The sleepy node `src` beats, opening a window.
    pub(crate) fn sleepy_data_beat(&mut self, src: u32) {
        let i = (src - self.relays()) as usize;
        self.sleepy_data.beat_took[i].0 = self.now;
    }

    /// `b` lost the frame `tx_key` to `o`: if it is a DATA to `b`, why.
    pub(crate) fn sleepy_data_lost(&mut self, tx_key: u64, b: u32, o: Outcome) {
        if let Some(t) = self
            .sleepy_data
            .on_air
            .get_mut(&tx_key)
            .filter(|t| t.node == b)
        {
            t.lost = Some(o);
        }
    }

    /// The frame `tx_key` left the air, received by `got`: if it is a
    /// DATA to a sleepy node, what became of it.
    pub(crate) fn sleepy_data_ends(&mut self, tx_key: u64, got: &[(u32, f64, f64)]) {
        let Some(t) = self.sleepy_data.on_air.remove(&tx_key) else {
            return;
        };
        let received = got.iter().any(|g| g.0 == t.node);
        let (fate, how) = if received {
            let i = (t.node - self.relays()) as usize;
            self.sleepy_data.beat_took[i].1 = self.now;
            (ToSleepyFate::Received, FirstTry::Received)
        } else if t.deaf {
            (ToSleepyFate::Unheard, FirstTry::Deaf)
        } else if t.absent {
            (ToSleepyFate::Lost, FirstTry::Absent)
        } else {
            let how = match t.lost {
                Some(Outcome::BelowSensitivity) => FirstTry::Weak,
                Some(
                    Outcome::LostCapture | Outcome::SinrFail { .. } | Outcome::Deaf(Deaf::Corrupt),
                ) => FirstTry::Collided,
                Some(Outcome::Deaf(Deaf::Sniff)) => FirstTry::Deaf,
                Some(Outcome::Deaf(_)) => FirstTry::Busy,
                _ => FirstTry::Unheard,
            };
            (ToSleepyFate::Lost, how)
        };
        if self.now >= self.settle {
            self.metrics.sleep.to_sleepy[fate as usize] += 1;
        }
        self.sleepy_data
            .first_try
            .entry((t.holder, t.msg))
            .or_insert((t.window, how));
    }

    /// A trace of `node`'s core.
    pub(crate) fn sleepy_data_trace(&mut self, node: u32, t: &Trace) {
        match *t {
            Trace::Delivered { msg_id, .. } => {
                self.sleepy_data.first_try.remove(&(node, msg_id.0));
            }
            Trace::DeliveryWaits {
                msg_id,
                node: to,
                tried,
            } => self.delivery_waits(node, msg_id.0, key(to), tried),
            _ => {}
        }
    }

    /// A delivery by `holder` of `msg` to the node `to` waits for its next
    /// beat: count it by its window and its first DATA.
    fn delivery_waits(&mut self, holder: u32, msg: u32, to: u64, tried: bool) {
        let first = self.sleepy_data.first_try.remove(&(holder, msg));
        let Some(&n) = self.sleepy_index.get(&to) else {
            return;
        };
        if self.now < self.settle {
            return;
        }
        let (window, how) = match (first, tried) {
            (Some(f), _) => f,
            (None, t) => {
                let how = if t {
                    FirstTry::NotSent
                } else {
                    FirstTry::NotStarted
                };
                (self.window_of(n), how)
            }
        };
        self.metrics.sleep.waited[window][how as usize] += 1;
    }

    /// The window the sleepy node `n` listens in now: after its beat, or
    /// after a message it took since.
    fn window_of(&self, n: u32) -> usize {
        let (beat, took) = self.sleepy_data.beat_took[(n - self.relays()) as usize];
        if took > beat {
            AFTER_TAKE
        } else {
            AFTER_BEAT
        }
    }
}
