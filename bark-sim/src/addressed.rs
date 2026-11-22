//! What becomes of frames addressed to a node that is not sleepy
//! (`metrics::Addressed`): by what the receiver was doing as each began,
//! what the lost ones met, the answers DATA got, the waits refusals asked
//! for, and how late relays' heartbeats fell against their slot grid.
//! Counting only: nothing here changes a draw or a decision.

use crate::metrics::{AddrFate, AddrKind, LostHow, Outcome, Reply};
use crate::sim::{key, AirTx, Sim};
use bark_proto::radio::Channel;
use bark_proto::sniff;
use bark_proto::wire::Packet;
use bark_proto::{Duration, Instant, NodeId};
use std::collections::HashMap;

/// What a frame on the air is, for [`LostHow`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AirKind {
    Data,
    Ack,
    /// A heartbeat of a node that is not sleepy.
    Heartbeat,
    /// A sleepy node's beat.
    Beat,
    /// An update piece or a status frame.
    Other,
}

/// What is followed of the frames on the air and their answers.
#[derive(Default)]
pub(crate) struct AddressedWatch {
    /// Frames on air addressed to a relay: key to the relay, the kind,
    /// and the frame's fate if lost, by the relay's state at the start.
    to: HashMap<u64, (u32, AddrKind, AddrFate)>,
    /// Frames on air: key to kind and the sender, for `lost_how`.
    kinds: HashMap<u64, (AirKind, u32)>,
    /// Frames on air addressed to a relay that lost them: key to how, and
    /// whether to a later frame within the harmless level.
    lost_how: HashMap<u64, (LostHow, bool)>,
    /// Per relay, its heartbeat grid's phase in its own clock and its
    /// interval, for `Addressed::heartbeat_late`.
    hb_grid: Vec<Option<(Instant, Duration)>>,
    /// DATA on air addressed to a relay: key to the ACK that would answer
    /// it, message ID and `to`.
    answer_of: HashMap<u64, (u32, u64)>,
    /// DATA a relay received and has not answered yet: (relay, message,
    /// `to`) to the answer's deadline and whether a leaf sent it.
    awaiting: HashMap<(u32, u32, u64), (Instant, bool)>,
    /// The count of the last DATA of each message to each relay, to read
    /// the count a refusing ACK answers.
    data_hops: HashMap<(u32, u64), u32>,
}

impl Sim {
    /// The frame `tx_key`, `decoded`, from `src` reaches the air on `ch`
    /// at `start` with `preamble` symbols.
    pub(crate) fn addressed_air_start(
        &mut self,
        tx_key: u64,
        src: u32,
        start: Instant,
        preamble: u16,
        ch: Channel,
        decoded: Option<Packet<'_>>,
    ) {
        let kind = match decoded {
            Some(Packet::Data(_)) => AirKind::Data,
            Some(Packet::Ack(_)) => AirKind::Ack,
            Some(Packet::Heartbeat(h)) if !h.sleepy => AirKind::Heartbeat,
            Some(Packet::Heartbeat(_)) => AirKind::Beat,
            Some(Packet::Update(_) | Packet::Status(_)) | None => AirKind::Other,
        };
        self.addressed.kinds.insert(tx_key, (kind, src));
        if let Some(Packet::Heartbeat(h)) = decoded {
            let in_slot = !h.sleepy
                && !h.anchorless
                && preamble < sniff::LONG_PREAMBLE_SYMBOLS
                && src < self.relays()
                && start >= self.settle;
            if in_slot {
                self.note_heartbeat_phase(src, start, h.id);
            }
        }
        let to = match decoded {
            Some(Packet::Data(d)) => Some((
                key(d.dst_hop),
                if src < self.relays() {
                    AddrKind::RelayData
                } else {
                    AddrKind::SleepyData
                },
            )),
            Some(Packet::Ack(a)) if src < self.relays() => Some((key(a.to), AddrKind::RelayAck)),
            Some(Packet::Heartbeat(h)) if h.sleepy => self
                .relay(src)
                .anchor()
                .map(|a| (key(a.id), AddrKind::Beat)),
            _ => None,
        };
        match decoded {
            Some(Packet::Data(d)) => {
                let to_relay = self
                    .relay_index
                    .get(&key(d.dst_hop))
                    .is_some_and(|&r| r < self.relays());
                let w = &mut self.addressed;
                w.answer_of.insert(tx_key, (d.msg_id().0, key(d.src_hop)));
                if to_relay {
                    w.data_hops.insert((d.msg_id().0, key(d.dst_hop)), d.hops);
                }
            }
            Some(Packet::Ack(a)) if src < self.relays() => {
                let hops = self.addressed.data_hops.remove(&(a.msg_id.0, key(a.from)));
                if hops == Some(0) {
                    let c = &mut self.metrics.nodes[src as usize].core;
                    if a.refused {
                        c.fresh_refused += 1;
                    } else {
                        c.fresh_taken += 1;
                    }
                }
                if a.refused {
                    use bark_proto::tables::budget::{told, BAN};
                    let bans = told(a.wait).as_micros() / BAN.as_micros();
                    let w = (u64::BITS - bans.leading_zeros()).min(7) as usize;
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
                if let Some((deadline, leaf)) = self.addressed.awaiting.remove(&k) {
                    let answer = if self.now > deadline {
                        Reply::Late
                    } else if a.refused {
                        Reply::Refused
                    } else {
                        Reply::Taken
                    };
                    self.metrics.addressed.replies[leaf as usize][answer as usize] += 1;
                }
            }
            _ => {}
        }
        if let Some(d) = to.and_then(|(name, kind)| self.relay_index.get(&name).map(|&d| (d, kind)))
        {
            let state = self.rx_state(d.0, ch);
            self.addressed.to.insert(tx_key, (d.0, d.1, state));
        }
    }

    /// The relay the frame `tx_key` on the air is addressed to, if any.
    pub(crate) fn addressed_to(&self, tx_key: u64) -> Option<u32> {
        self.addressed.to.get(&tx_key).map(|a| a.0)
    }

    /// `tx` left the air, received by `got`: its fate, and for a DATA
    /// received, the wait for its answer.
    pub(crate) fn addressed_air_end(&mut self, tx: &AirTx, got: &[(u32, f64, f64)]) {
        let w = &mut self.addressed;
        let answer = w.answer_of.remove(&tx.key);
        let how = w.lost_how.remove(&tx.key);
        w.kinds.remove(&tx.key);
        let Some((d, kind, state)) = w.to.remove(&tx.key) else {
            return;
        };
        let fate = if got.iter().any(|g| g.0 == d) {
            AddrFate::Received
        } else {
            state
        };
        if let (AddrFate::Received, Some((msg, to))) = (fate, answer) {
            if matches!(kind, AddrKind::RelayData | AddrKind::SleepyData) {
                let deadline = self.now + self.relay(d).config().ack_timeout;
                let leaf = kind == AddrKind::SleepyData;
                self.addressed
                    .awaiting
                    .insert((d, msg, to), (deadline, leaf));
            }
        }
        let a = &mut self.metrics.addressed;
        let (k, f, c) = (kind as usize, fate as usize, tx.frame.channel.index());
        a.counts[k][f] += 1;
        if fate != AddrFate::Received {
            let (how, later) = how.unwrap_or((LostHow::Other, false));
            a.lost_how[k][f][how as usize] += 1;
            a.lost_to_later[k][f] += u64::from(later);
        }
        let beats = &self.nodes[d as usize].beats;
        if beats.during(tx.start, tx.end, c, true) {
            a.with_beat[k][f] += 1;
        }
        if beats.during(tx.start, tx.end, c, false) {
            a.with_heartbeat[k][f] += 1;
        }
    }

    /// Once a minute: DATA a relay received whose answer is past its
    /// deadline, and a second more, had none.
    pub(crate) fn addressed_unanswered(&mut self) {
        let now = self.now;
        let replies = &mut self.metrics.addressed.replies;
        self.addressed.awaiting.retain(|_, (deadline, leaf)| {
            // An ACK may still come late; give it a second.
            let gone = now > *deadline + Duration::from_secs(1);
            if gone {
                replies[*leaf as usize][Reply::None as usize] += 1;
            }
            !gone
        });
    }

    /// A relay's heartbeat in a slot started at `start`: where it falls
    /// against the relay's grid, in the relay's own clock. The grid is the
    /// earliest phase seen, so a first heartbeat that was late shows the
    /// ones before the earliest as later than they were, by its lateness.
    fn note_heartbeat_phase(&mut self, src: u32, start: Instant, id: NodeId) {
        let local = self.clocks[src as usize].local(start);
        let interval = bark_proto::node::heartbeat_interval(id, self.relay(src).config());
        let grid = &mut self.addressed.hb_grid;
        if grid.len() <= src as usize {
            grid.resize(src as usize + 1, None);
        }
        let Some((t0, t)) = grid[src as usize] else {
            grid[src as usize] = Some((local, interval));
            return;
        };
        let off = local.since(t0).as_micros() % t.as_micros().max(1);
        let off = if off * 2 > t.as_micros() {
            // Earlier than the grid: the grid moves to it.
            grid[src as usize] = Some((t0 - Duration::from_micros(t.as_micros() - off), t));
            0
        } else {
            off
        };
        let bin = (off / 40_000).min(24) as usize;
        self.metrics.addressed.heartbeat_late[bin] += 1;
    }

    /// How the frame `key` on the air at `b` is classed for
    /// `Addressed::lost_how`, where it meets the frame `other`; `later`
    /// says whether `key` started after `other`.
    pub(crate) fn how_of(&self, b: u32, key: Option<u64>, other: u64, later: bool) -> LostHow {
        let Some(k) = key else {
            return LostHow::Other;
        };
        let kinds = &self.addressed.kinds;
        match kinds.get(&k) {
            Some(&(AirKind::Data, src)) => {
                if self.addressed_to(k) == Some(b) {
                    let Some(&(_, other_src)) = kinds.get(&other) else {
                        return LostHow::HiddenData;
                    };
                    // Whether the later sender could hear the earlier
                    // frame when it checked the channel.
                    let (from, to) = if later {
                        (other_src, src)
                    } else {
                        (src, other_src)
                    };
                    let audible = self
                        .links
                        .get(from, to)
                        .is_some_and(|l| l.base_dbm as f64 >= self.sens_dbm);
                    if audible {
                        LostHow::HeardData
                    } else {
                        LostHow::HiddenData
                    }
                } else {
                    LostHow::OtherData
                }
            }
            Some(&(AirKind::Ack, _)) => LostHow::Ack,
            Some(&(AirKind::Heartbeat, _)) => LostHow::Heartbeat,
            Some(&(AirKind::Beat, _)) => LostHow::Beat,
            Some(&(AirKind::Other, _)) | None => LostHow::Other,
        }
    }

    /// Note how `b` lost the frame `key`, where `met` classes the
    /// strongest frame it met, for the margin log and, if the frame was
    /// addressed to `b`, here.
    pub(crate) fn note_addressed_loss(&mut self, key: u64, b: u32, o: Outcome, met: LostHow) {
        let how = LostHow::of(o, met);
        if self.margin.is_some() {
            self.margin_lost(key, b, how);
        }
        if self.addressed_to(key) == Some(b) {
            let later = o == Outcome::LostToLater;
            self.addressed.lost_how.insert(key, (how, later));
        }
    }
}
