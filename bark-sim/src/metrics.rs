//! Counters, per node and per channel, with drop reasons as first-class data.
//!
//! Reception accounting: every time a transmission on channel `c` reaches a
//! node whose resting channel is `c` (it would have heard the packet had
//! nothing gone wrong), that is a *candidate*, and each candidate ends in
//! exactly one outcome: delivered, lost to noise, lost capture, lost to
//! interference, truncated, deaf for a stated reason, or half duplex. Nodes
//! resting on other channels are not candidates: a packet sent elsewhere is
//! not a loss.

use crate::phy::Deaf;
use bark_proto::radio::CHANNELS;
use serde::Serialize;

const NCH: usize = CHANNELS as usize;

/// How one candidate reception ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Delivered to the core as `Event::Received`.
    Delivered,
    /// Lost with receiver noise the larger part of what it faced: below
    /// sensitivity, under the threshold rule.
    BelowSensitivity,
    /// Another packet held the receiver's lock (or a stronger one stole it
    /// during the preamble).
    LostCapture,
    /// Lost with interference the larger part of what it faced; `adjacent`
    /// if adjacent-channel leakage was the larger part of that.
    SinrFail {
        /// Adjacent channels dominated the interference.
        adjacent: bool,
    },
    /// The transmitter vanished mid-packet (scripted removal).
    Truncated,
    /// The receiver was not listening; [`Deaf::HalfDuplex`] is counted apart.
    Deaf(Deaf),
}

/// Deaf counters by reason.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct DeafCounts {
    /// Retune or mode switch.
    pub retune: u64,
    /// CAD in progress.
    pub cad: u64,
    /// RSSI sample in progress.
    pub rssi: u64,
    /// TX turnaround.
    pub tx: u64,
    /// Demodulating a corrupted packet.
    pub corrupt: u64,
    /// Out of radio reach (dark leaf).
    pub absent: u64,
    /// No window of the duty-cycled receive detected it (`Action::Sniff`).
    pub sniff: u64,
}

impl DeafCounts {
    fn add(&mut self, r: Deaf) {
        match r {
            Deaf::Retune => self.retune += 1,
            Deaf::Cad => self.cad += 1,
            Deaf::Rssi => self.rssi += 1,
            Deaf::Tx => self.tx += 1,
            Deaf::Corrupt => self.corrupt += 1,
            Deaf::Absent => self.absent += 1,
            Deaf::Sniff => self.sniff += 1,
            Deaf::HalfDuplex => unreachable!("counted as half_duplex"),
        }
    }

    /// Total.
    pub fn total(&self) -> u64 {
        self.retune + self.cad + self.rssi + self.tx + self.corrupt + self.absent + self.sniff
    }

    fn merge(&mut self, o: &DeafCounts) {
        self.retune += o.retune;
        self.cad += o.cad;
        self.rssi += o.rssi;
        self.tx += o.tx;
        self.corrupt += o.corrupt;
        self.absent += o.absent;
        self.sniff += o.sniff;
    }
}

/// Reception counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct RxCounts {
    /// Candidate receptions.
    pub candidates: u64,
    /// Delivered.
    pub delivered: u64,
    /// Below sensitivity.
    pub below_sensitivity: u64,
    /// Lost capture.
    pub lost_capture: u64,
    /// SINR failures, all.
    pub sinr_fail: u64,
    /// Of `sinr_fail`, those where adjacent-channel leakage dominated.
    pub sinr_fail_adjacent: u64,
    /// Transmitter removed mid-packet.
    pub truncated: u64,
    /// Receiver deaf, by reason.
    pub deaf: DeafCounts,
    /// Receiver transmitting.
    pub half_duplex: u64,
    /// Locked packets the error curve lost that the threshold rule would
    /// have passed, judged against the same signal and interference.
    pub curve_lost_threshold_passed: u64,
    /// Packets the error curve delivered that the threshold rule would
    /// have lost.
    pub curve_received_threshold_failed: u64,
}

impl RxCounts {
    fn add(&mut self, o: Outcome) {
        match o {
            Outcome::Delivered => self.delivered += 1,
            Outcome::BelowSensitivity => self.below_sensitivity += 1,
            Outcome::LostCapture => self.lost_capture += 1,
            Outcome::SinrFail { adjacent } => {
                self.sinr_fail += 1;
                self.sinr_fail_adjacent += adjacent as u64;
            }
            Outcome::Truncated => self.truncated += 1,
            Outcome::Deaf(Deaf::HalfDuplex) => self.half_duplex += 1,
            Outcome::Deaf(r) => self.deaf.add(r),
        }
    }

    /// Outcomes decided so far; equals `candidates` once nothing is in flight.
    pub fn decided(&self) -> u64 {
        self.delivered
            + self.below_sensitivity
            + self.lost_capture
            + self.sinr_fail
            + self.truncated
            + self.deaf.total()
            + self.half_duplex
    }

    /// Add another counter set.
    pub fn merge(&mut self, o: &RxCounts) {
        self.candidates += o.candidates;
        self.delivered += o.delivered;
        self.below_sensitivity += o.below_sensitivity;
        self.lost_capture += o.lost_capture;
        self.sinr_fail += o.sinr_fail;
        self.sinr_fail_adjacent += o.sinr_fail_adjacent;
        self.truncated += o.truncated;
        self.deaf.merge(&o.deaf);
        self.half_duplex += o.half_duplex;
        self.curve_lost_threshold_passed += o.curve_lost_threshold_passed;
        self.curve_received_threshold_failed += o.curve_received_threshold_failed;
    }
}

/// Per-node counters.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct NodeMetrics {
    /// Receptions at this node.
    pub rx: RxCounts,
    /// Frames transmitted.
    pub tx_frames: u64,
    /// Time on air, µs.
    pub airtime_us: u64,
    /// The same time on air by packet type.
    pub airtime_by_type: TypeAirtime,
    /// CAD excursions.
    pub cad: u64,
    /// RSSI excursions.
    pub rssi: u64,
    /// Resting-channel changes after the first `Listen`.
    pub home_moves: u64,
    /// For a sleepy node: times it took an anchor other than the one it
    /// last had (spec "Relay choice").
    pub relay_changes: u64,
    /// For a sleepy node: time its radio received duty-cycled after the
    /// settle time, µs, a frame it caught included.
    pub sniff_us: u64,
    /// Of `sniff_us`, the time from detecting the preamble of a frame it
    /// caught to the frame's end, when the radio receives in full.
    pub sniff_rx_us: u64,
    /// Time on air after the settle time, µs.
    pub settled_airtime_us: u64,
    /// For a sleepy node: when it took an anchor after a spell without
    /// one past its boot listen, and when the spell began, s.
    pub anchored_s: Vec<(f64, f64)>,
    /// Messages originated here by the traffic model.
    pub originated: u64,
    /// `AppEvent::Message` at this node.
    pub messages_in: u64,
    /// Messages this node dropped, by reason.
    pub drops: DropCounts,
    /// What the core reported in its traces.
    pub core: CoreCounts,
    /// A node's tables and bucket, sampled; `None` for a sleepy node or
    /// when not sampled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load: Option<LoadStats>,
}

/// Counts of what a core reported in its traces (`bark_proto::Trace`), and
/// of what heartbeats did to a relay's neighbour table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct CoreCounts {
    /// Home channel moves after the boot choice (spec "Channel moves").
    pub home_moves: u64,
    /// Heartbeats skipped because every listen-before-talk check was busy (spec "Heartbeat").
    pub heartbeats_skipped: u64,
    /// Heartbeats the bucket held back (spec "Duty cycle cap").
    pub heartbeats_deferred: u64,
    /// DATA packets this relay refused, by what it lacked (spec "Receiving DATA").
    pub refused: DropCounts,
    /// Refusals this node received, each a ban on the refuser (spec "Refusal").
    pub turned_away: u64,
    /// Hand-overs the bucket held back (spec "Duty cycle cap").
    pub handovers_deferred: u64,
    /// Neighbours demoted (spec "Neighbour table").
    pub demoted: u64,
    /// Rotations a sleepy node listened to check its anchor
    /// (spec "Relay choice").
    pub anchor_checks: u64,
    /// Messages a sleepy node took from this node as their holder.
    pub delivered: u64,
    /// Messages a neighbour took: sent on (spec "Next hop") or returned to the
    /// parent (spec "Return to parent").
    pub handed_over: u64,
    /// Of those, returned to the parent (spec "Return to parent").
    pub returned: u64,
    /// Heartbeats from a relay not in the table that took the place of
    /// one that was (D12).
    pub evictions: u64,
    /// Heartbeats from a relay not in the full table that found no place
    /// (D12).
    pub rejections: u64,
}

/// What became of frames addressed to one node that is not sleepy, by
/// what that node was doing when the frame started: `[kind][fate]`. Kinds:
/// DATA from a node that is not sleepy, an ACK from one, DATA from a
/// sleepy node. Fates: received; lost although the receiver was listening
/// and free; lost while it was receiving another frame; lost while it was
/// deaf on the channel (checking, transmitting, retuning); lost while it
/// was on another channel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Addressed {
    /// Counts.
    pub counts: [[u64; 5]; 3],
    /// Of `counts`, the frames during which a sleepy node's heartbeat was
    /// on the air at the receiver: the hidden-terminal question of a
    /// presence frame sent at a time the sender chose (roadmap D41).
    pub with_beat: [[u64; 5]; 3],
    /// The same for the heartbeat of a node that is not sleepy.
    pub with_heartbeat: [[u64; 5]; 3],
    /// Of the DATA frames received, `[relay's, leaf's][answer]`: an ACK that
    /// takes it, one that refuses it, either started later than the ACK
    /// timeout after the frame's end, or none.
    pub replies: [[u64; 4]; 2],
}

/// Refusals of a relay's DATA for want of room (spec "Receiving DATA"), and whether the
/// refusing relay then had a free hop place at a count the refused sender
/// would have sent another of its queued messages to it with: whether a
/// refusal that named the lowest count it would take could have let the
/// sender offer that message instead. Waiting is from each refusal to the
/// refused message's next DATA from that sender.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct HopPlaceRefusals {
    /// Refusals of a relay's DATA for its bucket or a full room.
    pub refusals: u64,
    /// Of those, with a hop place free for another message the sender held
    /// for the refuser.
    pub with_place: u64,
    /// Waiting after the refusals, µs.
    pub wait_us: u64,
    /// Of that, after those with a hop place free.
    pub wait_with_place_us: u64,
}

/// The sleepy nodes' counters summed, for the series (`Sim::series_on`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct LeafSums {
    /// Sleepy nodes in reach (not dark).
    pub present: u64,
    /// Sleepy nodes with an anchor.
    pub attached: u64,
    /// Sleepy nodes holding a message.
    pub holding: u64,
    /// Sleepy nodes whose application waits to hand one over.
    pub waiting: u64,
    /// Frames sent.
    pub tx_frames: u64,
    /// Airtime by type, µs.
    pub airtime_by_type: TypeAirtime,
    /// Anchor changes.
    pub relay_changes: u64,
    /// Messages received.
    pub messages_in: u64,
}

impl LeafSums {
    /// Add one leaf.
    pub fn add(
        &mut self,
        m: &NodeMetrics,
        attached: bool,
        holding: bool,
        waiting: bool,
        present: bool,
    ) {
        self.present += present as u64;
        self.attached += attached as u64;
        self.holding += holding as u64;
        self.waiting += waiting as u64;
        self.tx_frames += m.tx_frames;
        let (a, b) = (&mut self.airtime_by_type, &m.airtime_by_type);
        a.heartbeat_us += b.heartbeat_us;
        a.beat_us += b.beat_us;
        a.reach_us += b.reach_us;
        a.anchorless_us += b.anchorless_us;
        a.answer_us += b.answer_us;
        a.long_data_us += b.long_data_us;
        a.ack_us += b.ack_us;
        a.data_us += b.data_us;
        a.other_us += b.other_us;
        self.relay_changes += m.relay_changes;
        self.messages_in += m.messages_in;
    }
}

/// A relay's load, sampled at the metrics cadence: the bucket, the queues
/// and the tables (`bark_proto::Load`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct LoadStats {
    /// Samples taken while running (after the boot listen).
    pub samples: u64,
    /// Samples at the cap: the bucket held less than the reply reserve and
    /// one full frame, so the relay could take no full frame of new work.
    pub at_cap: u64,
    /// Samples below the reply reserve: only heartbeats and replies go.
    pub below_reserve: u64,
    /// Samples with a hand-over under way (`Relay::handing_over`).
    pub handing_over: u64,
    /// Runs of consecutive samples at the cap.
    pub cap_episodes: u64,
    /// The longest such run, samples.
    pub longest_cap: u64,
    /// The run in progress, samples.
    #[serde(skip)]
    pub cap_run: u64,
    /// Bucket content summed over samples, µs.
    pub bucket_sum_us: u64,
    /// Largest send queue.
    pub send_queue_max: u64,
    /// Send queue summed over samples.
    pub send_queue_sum: u64,
    /// Largest delivery queue.
    pub delivery_max: u64,
    /// Delivery queue summed over samples.
    pub delivery_sum: u64,
    /// Largest neighbour table.
    pub neighbours_max: u64,
    /// Neighbour table summed over samples.
    pub neighbours_sum: u64,
    /// Samples with the neighbour table full.
    pub neighbours_full: u64,
    /// Largest heard table.
    pub leaves_max: u64,
    /// Heard table summed over samples.
    pub leaves_sum: u64,
}

impl LoadStats {
    /// Add one sample. `cap` is the bucket content below which the
    /// relay is at the cap, `reserve` the reply reserve.
    pub fn add(
        &mut self,
        l: &bark_proto::Load,
        cap: bark_proto::Duration,
        reserve: bark_proto::Duration,
        full_table: usize,
    ) {
        self.samples += 1;
        if l.bucket < cap {
            self.at_cap += 1;
            if self.cap_run == 0 {
                self.cap_episodes += 1;
            }
            self.cap_run += 1;
            self.longest_cap = self.longest_cap.max(self.cap_run);
        } else {
            self.cap_run = 0;
        }
        self.below_reserve += u64::from(l.bucket < reserve);
        self.bucket_sum_us += l.bucket.as_micros();
        let (sq, dq, nb, lv) = (
            l.send_queue as u64,
            l.delivery_queue as u64,
            l.neighbours as u64,
            l.heard as u64,
        );
        self.send_queue_max = self.send_queue_max.max(sq);
        self.send_queue_sum += sq;
        self.delivery_max = self.delivery_max.max(dq);
        self.delivery_sum += dq;
        self.neighbours_max = self.neighbours_max.max(nb);
        self.neighbours_sum += nb;
        self.neighbours_full += u64::from(l.neighbours >= full_table);
        self.leaves_max = self.leaves_max.max(lv);
        self.leaves_sum += lv;
    }
}

/// Why a node dropped a message: the core's own reasons, reported in
/// `Trace::Dropped`. The protocol sends no failure notice (roadmap D10),
/// so a lost message is only visible where it died.
pub use bark_proto::DropReason;

/// Drop counters by reason.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct DropCounts {
    /// No neighbour left at the origin (spec "Return to parent").
    pub no_route: u64,
    /// The hop cap (spec "Hop count").
    pub hop_cap: u64,
    /// No place for its record (spec "Message records").
    pub no_record: u64,
    /// The send queue was full (spec "Send queue").
    pub send_queue_full: u64,
    /// The duty-cycle budget (spec "Duty cycle cap").
    pub budget: u64,
    /// Fresh work refused while every way on was banned (spec "Receiving DATA").
    pub banned: u64,
    /// The return to the parent failed (spec "Return to parent").
    pub return_failed: u64,
    /// The delivery TTL ran out (spec "Delivery queue").
    pub queue_expired: u64,
    /// Its time left ran out on the way (spec "Receiving DATA").
    pub expired: u64,
    /// The delivery queue was full (spec "Delivery queue").
    pub delivery_full: u64,
    /// The payload was too long (spec "Packet formats").
    pub payload_too_long: u64,
}

impl DropCounts {
    /// Count one drop.
    pub fn add(&mut self, r: DropReason) {
        let c = match r {
            DropReason::NoRoute => &mut self.no_route,
            DropReason::HopCap => &mut self.hop_cap,
            DropReason::NoRecord => &mut self.no_record,
            DropReason::SendQueueFull => &mut self.send_queue_full,
            DropReason::Budget => &mut self.budget,
            DropReason::Banned => &mut self.banned,
            DropReason::ReturnFailed => &mut self.return_failed,
            DropReason::QueueExpired => &mut self.queue_expired,
            DropReason::Expired => &mut self.expired,
            DropReason::DeliveryFull => &mut self.delivery_full,
            DropReason::PayloadTooLong => &mut self.payload_too_long,
        };
        *c += 1;
    }

    /// Total.
    pub fn total(&self) -> u64 {
        self.no_route
            + self.hop_cap
            + self.no_record
            + self.send_queue_full
            + self.budget
            + self.banned
            + self.return_failed
            + self.queue_expired
            + self.expired
            + self.delivery_full
            + self.payload_too_long
    }
}

/// Per-channel counters.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ChannelMetrics {
    /// Receptions on this channel.
    pub rx: RxCounts,
    /// Frames transmitted on this channel.
    pub tx_frames: u64,
    /// Time on air on this channel, µs.
    pub airtime_us: u64,
    /// The same time on air by packet type.
    pub airtime_by_type: TypeAirtime,
}

/// Time on air by packet type (spec "Packet formats"), µs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct TypeAirtime {
    /// HEARTBEAT of a node that is not sleepy, in its slot.
    pub heartbeat_us: u64,
    /// HEARTBEAT of a sleepy node with an anchor, with the short preamble.
    pub beat_us: u64,
    /// HEARTBEAT of a sleepy node with an anchor, with the long preamble,
    /// to its anchorless neighbours (spec "Beats").
    pub reach_us: u64,
    /// HEARTBEAT of a sleepy node without an anchor, with the long
    /// preamble.
    pub anchorless_us: u64,
    /// HEARTBEAT of a relay answering one, with the long preamble.
    pub answer_us: u64,
    /// ACK.
    pub ack_us: u64,
    /// DATA.
    pub data_us: u64,
    /// Of `data_us`, DATA with the long preamble, to a sleepy node
    /// receiving duty-cycled.
    pub long_data_us: u64,
    /// Anything else: a scripted test signal.
    pub other_us: u64,
}

impl TypeAirtime {
    /// Count `us` against the type of `p`, sent with `preamble` symbols.
    pub fn add(&mut self, p: Option<&bark_proto::wire::Packet<'_>>, preamble: u16, us: u64) {
        use bark_proto::wire::Packet;
        let long = preamble >= bark_proto::sniff::LONG_PREAMBLE_SYMBOLS;
        let c = match p {
            Some(Packet::Heartbeat(h)) if h.anchorless => &mut self.anchorless_us,
            Some(Packet::Heartbeat(h)) if h.sleepy && long => &mut self.reach_us,
            Some(Packet::Heartbeat(h)) if h.sleepy => &mut self.beat_us,
            Some(Packet::Heartbeat(_)) if long => &mut self.answer_us,
            Some(Packet::Heartbeat(_)) => &mut self.heartbeat_us,
            Some(Packet::Ack(_)) => &mut self.ack_us,
            Some(Packet::Data(_)) => {
                if long {
                    self.long_data_us += us;
                }
                &mut self.data_us
            }
            None => &mut self.other_us,
        };
        *c += us;
    }
}

/// Application-level counters, matched through a tag the traffic model
/// writes into each payload (the core never looks inside a payload).
/// End-to-end delivery is known only by observing `AppEvent::Message` at
/// the destination; nothing in the protocol reports it back.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct AppMetrics {
    /// Messages the traffic model had for a core to send.
    pub originated: u64,
    /// Of those, ones the core did not take the moment they were due, so
    /// the application waited (`Core::accepts_send`).
    pub waited: u64,
    /// The longest such wait, µs.
    pub longest_wait_us: u64,
    /// Messages the traffic model did not send because its application
    /// still waited with the previous one.
    pub skipped: u64,
    /// Distinct messages that reached their destination's application.
    pub delivered: u64,
    /// Further copies of already delivered messages.
    pub duplicates: u64,
    /// Messages that reached a node other than their destination.
    pub misdelivered: u64,
    /// Messages the traffic model did not send because their destination
    /// leaf had never attached, so no sender could know its address (spec "Addresses").
    pub unaddressed: u64,
    /// Drops reported by cores, by reason (events, not messages: two
    /// copies of one message may be dropped in two places).
    pub drops: DropCounts,
    /// Origination to destination application, µs, in arrival order.
    #[serde(skip)]
    pub latencies_us: Vec<u64>,
}

/// Heartbeats of nodes that are not sleepy, after the settle time, at the
/// nodes listening on their channel: how many arrived, and how many were
/// lost with another heartbeat on the air there.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct HeartbeatFates {
    /// Receivers alive, in reach, awake, resting on the heartbeat's channel, and
    /// reached above sensitivity on average.
    pub candidates: u64,
    /// Of those, ones that received it.
    pub received: u64,
    /// Of the others, ones with another heartbeat on the air there.
    pub lost_with_heartbeat: u64,
    /// Of the others, by why: to another frame, by capture or by symbol
    /// errors; to a receiver that was deaf, retuning, checking, sending or
    /// locked to another frame's garbage; and below sensitivity.
    pub lost_collided: u64,
    /// See `lost_collided`.
    pub lost_deaf: u64,
    /// See `lost_collided`.
    pub lost_weak: u64,
}

/// What sleepy nodes do and what is sent to them, counted after the
/// settle time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct SleepMetrics {
    /// DATA addressed to a sleepy node: received, sent while it could not
    /// hear it, lost otherwise.
    pub to_sleepy: [u64; 3],
    /// DATA sent to a sleepy node that could not hear it: receiving
    /// duty-cycled, and the frame's preamble short. By its
    /// anchor, and by any other node; and how long it had been so, under
    /// 5 s or more.
    pub to_deaf: [[u64; 2]; 2],
    /// Changes of a sleepy node's anchor.
    pub anchor_changes: u64,
    /// Messages a holder dropped at the end of the delivery TTL: held by
    /// the node's anchor while the node was in reach, by its anchor while
    /// it was dark, by a node that was no longer its anchor.
    pub queue_expired: [u64; 3],
    /// Minutes sampled.
    pub samples: u64,
    /// Summed over the samples, sleepy nodes with an anchor; of those,
    /// ones on another channel than their anchor's home, and ones their
    /// anchor does not have in its heard table.
    pub anchored: u64,
    /// See `anchored`.
    pub off_channel: u64,
    /// See `anchored`.
    pub unheard: u64,
    /// Deliveries that left their holder's send queue to wait for the
    /// node's next beat (`Trace::DeliveryWaits`), by the window they went
    /// in, [`AFTER_BEAT`] or [`AFTER_TAKE`], and by what became of the
    /// first DATA of their hand-over, a [`FirstTry`].
    pub waited: [[u64; FIRST_TRIES]; 2],
    /// Windows a sleeping node woke for its anchor's heartbeat in
    /// (`Trace::AnchorWatch`): with the heartbeat, and without it.
    pub watches: [u64; 2],
    /// Time the node listened in them, µs.
    pub watch_listen_us: u64,
    /// Heartbeats heard in them that started before the time expected,
    /// and how early the earliest was, µs, negative; 0 if none was.
    pub watch_early: u64,
    /// See `watch_early`.
    pub watch_earliest_us: i64,
    /// Heartbeats of its anchor that reached a sleepy node on its channel
    /// above sensitivity on average: with its radio on throughout, and
    /// with it receiving duty-cycled at the start or the end.
    pub anchor_beats: [u64; 2],
    /// Stops and walks walkers began (`walk.rs`).
    pub walks: [u64; 2],
    /// Checks of the anchor a GPS fix started: the node had moved too far
    /// from where it chose (spec "Relay choice").
    pub fix_checks: u64,
}

/// `SleepMetrics::waited`: the window after the node's beat.
pub const AFTER_BEAT: usize = 0;
/// `SleepMetrics::waited`: the window after a message the node took.
pub const AFTER_TAKE: usize = 1;

/// What became of the first DATA of a delivery's hand-over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FirstTry {
    /// The node stopped listening before the hand-over started.
    NotStarted,
    /// It started, but no DATA went on the air: the channel stayed busy.
    NotSent,
    /// It reached the node receiving duty-cycled, with a short preamble.
    Deaf,
    /// The node was out of reach (dark or gone).
    Absent,
    /// It reached the node below sensitivity.
    Weak,
    /// Another frame at the node took the receiver or broke it.
    Collided,
    /// The node's radio was busy: retuning, checking a channel or sending.
    Busy,
    /// It did not reach the node where it listened: another channel, or
    /// out of range.
    Unheard,
    /// The node received it; its answers were lost.
    Received,
}

/// The number of [`FirstTry`] cases.
pub const FIRST_TRIES: usize = 9;

/// How soon the network learns that a dark sleepy node is back (spec
/// "Delivery queue"): from the moment it is in reach again to the first
/// beat of it its anchor, or any node if it has none, receives.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct LeafReturns {
    /// Leaves that came back into reach.
    pub returned: u64,
    /// Of those, ones a relay heard from before they went dark again or
    /// the run ended.
    pub known: u64,
    /// Back in reach to known, µs, in order.
    #[serde(skip)]
    pub latencies_us: Vec<u64>,
}

/// Busy-time tracker for one observer node: the fraction of time the
/// in-channel power at the node (with adjacent leakage after rejection)
/// exceeds the noise floor by the protocol's busy threshold. This is the
/// ground truth the core's sampled busy estimate approximates.
#[derive(Clone, Debug, Default)]
pub struct Observer {
    /// Node index.
    pub node: u32,
    /// Busy now, per channel.
    pub busy: [bool; NCH],
    /// Since when, per channel, µs.
    pub since: [u64; NCH],
    /// Accumulated busy time, µs.
    pub total_us: [u64; NCH],
    /// Accumulated busy time at the last sample, µs.
    pub at_sample_us: [u64; NCH],
}

impl Observer {
    /// Set channel `c` busy or idle at `now_us`.
    pub fn set(&mut self, c: usize, busy: bool, now_us: u64) {
        if self.busy[c] == busy {
            return;
        }
        if self.busy[c] {
            self.total_us[c] += now_us - self.since[c];
        }
        self.busy[c] = busy;
        self.since[c] = now_us;
    }

    /// Busy time up to `now_us` on channel `c`.
    pub fn busy_until(&self, c: usize, now_us: u64) -> u64 {
        self.total_us[c]
            + if self.busy[c] {
                now_us - self.since[c]
            } else {
                0
            }
    }
}

/// One row of the time series.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SampleRow {
    /// End of the interval, seconds.
    pub t_s: f64,
    /// Channel.
    pub channel: u8,
    /// Mean busy fraction over observers during the interval.
    pub busy_fraction: f64,
    /// Frames started on this channel during the interval.
    pub tx_frames: u64,
    /// Receptions delivered on this channel during the interval.
    pub delivered: u64,
    /// Messages originated so far.
    pub app_originated: u64,
    /// Messages delivered end to end so far.
    pub app_delivered: u64,
}

/// Two nodes the scenario forced to share an ID (`scenario::SharedIds`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Pair {
    /// What they are.
    pub kind: crate::scenario::PairKind,
    /// Whether they hear each other.
    pub reach: crate::scenario::PairReach,
    /// The two nodes; `b` took `a`'s ID.
    pub a: u32,
    /// See `a`.
    pub b: u32,
    /// Whether they share it, or are only marked.
    pub shared: bool,
    /// Relays that hear both above sensitivity before fading.
    pub relays_hearing_both: u32,
}

/// Counts for the research models that are off by default
/// (`docs/simulator-design.md` §3.8).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Research {
    /// The forced pairs.
    pub pairs: Vec<Pair>,
    /// Messages that reached the application of a node with their
    /// destination's ID that is not their destination.
    pub to_twin: u64,
    /// The groups that go dark together.
    pub groups: Vec<Group>,
    /// Once a minute after the settle time, walkers (0) and riders (1)
    /// with an anchor: by what they do, moving or at a stop for less than
    /// 1, 2, 5 and 10 minutes or longer, how many were sampled and how
    /// many of them their anchor would hear (`Sim::sample_anchors`).
    pub movers: [[(u64, u64); MOVER_BINS]; 2],
    /// Once a minute after the settle time, per relay, the samples it
    /// was running on each home channel.
    pub homes: Vec<[u32; CHANNELS as usize]>,
    /// Every move of a relay's home after its boot: when, s, the relay,
    /// and the channels it left and took.
    pub moves: Vec<(f64, u32, u8, u8)>,
    /// The nodes with a GPS fault (`scenario::GpsFault`), relays and
    /// sleepy nodes.
    pub gps_nodes: [Vec<u32>; 2],
    /// Their fixes after the fault began: handed to the core, rejected for
    /// a jump, rejected as inconsistent with the neighbours heard; relays
    /// and sleepy nodes.
    pub fixes: [[u64; 3]; 2],
}

/// Bins of `Research::movers`: moving, then at a stop for less than each
/// of these many seconds, then longer.
pub const MOVER_STOPS_S: [u64; 4] = [60, 120, 300, 600];
/// See [`MOVER_STOPS_S`].
pub const MOVER_BINS: usize = MOVER_STOPS_S.len() + 2;

/// Sleepy nodes that went dark together (`scenario::DarkGroup`).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Group {
    /// The node whose area it is.
    pub near: u32,
    /// When it went dark and came back, seconds.
    pub from_s: f64,
    /// See `from_s`.
    pub to_s: f64,
    /// Its nodes.
    pub members: Vec<u32>,
}

/// All counters of a run.
#[derive(Clone, Debug, Default)]
pub struct Metrics {
    /// Per node, indexed like the simulator's nodes.
    pub nodes: Vec<NodeMetrics>,
    /// Per channel.
    pub channels: [ChannelMetrics; NCH],
    /// Application level.
    pub app: AppMetrics,
    /// Dark leaves coming back.
    pub returns: LeafReturns,
    /// What sleeping does to sleepy nodes and their holders.
    pub sleep: SleepMetrics,
    /// Heartbeats at their receivers.
    pub heartbeats: HeartbeatFates,
    /// Frames addressed to a relay, by fate.
    pub addressed: Addressed,
    /// Refusals for room, and the hop places free at them.
    pub hop_places: HopPlaceRefusals,
    /// Busy-time observers.
    pub observers: Vec<Observer>,
    /// Time series.
    pub samples: Vec<SampleRow>,
    /// Heap events processed.
    pub events: u64,
    /// The research models.
    pub research: Research,
    last_sample: SampleMark,
}

#[derive(Clone, Debug, Default)]
struct SampleMark {
    t_us: u64,
    tx: [u64; NCH],
    delivered: [u64; NCH],
}

impl Metrics {
    /// Counters for `n` nodes.
    pub fn new(n: usize) -> Metrics {
        Metrics {
            nodes: vec![NodeMetrics::default(); n],
            ..Metrics::default()
        }
    }

    /// A candidate reception at `node` on `channel`.
    pub fn candidate(&mut self, node: u32, channel: u8) {
        self.nodes[node as usize].rx.candidates += 1;
        self.channels[channel as usize].rx.candidates += 1;
    }

    /// The outcome of a candidate reception.
    pub fn outcome(&mut self, node: u32, channel: u8, o: Outcome) {
        self.nodes[node as usize].rx.add(o);
        self.channels[channel as usize].rx.add(o);
    }

    /// A locked packet on which the error curve and the threshold rule
    /// disagree: `delivered` is the curve's verdict.
    pub fn disagreement(&mut self, node: u32, channel: u8, delivered: bool) {
        for r in [
            &mut self.nodes[node as usize].rx,
            &mut self.channels[channel as usize].rx,
        ] {
            if delivered {
                r.curve_received_threshold_failed += 1;
            } else {
                r.curve_lost_threshold_passed += 1;
            }
        }
    }

    /// Reception totals over all channels.
    pub fn rx_total(&self) -> RxCounts {
        let mut t = RxCounts::default();
        for c in &self.channels {
            t.merge(&c.rx);
        }
        t
    }

    /// Close a time-series interval at `now_us`.
    pub fn sample(&mut self, now_us: u64) {
        let dt = now_us.saturating_sub(self.last_sample.t_us);
        for c in 0..NCH {
            let busy = if self.observers.is_empty() || dt == 0 {
                0.0
            } else {
                let mut sum = 0.0;
                for o in &mut self.observers {
                    let b = o.busy_until(c, now_us);
                    sum += (b - o.at_sample_us[c]) as f64 / dt as f64;
                    o.at_sample_us[c] = b;
                }
                sum / self.observers.len() as f64
            };
            let ch = &self.channels[c];
            self.samples.push(SampleRow {
                t_s: now_us as f64 / 1e6,
                channel: c as u8,
                busy_fraction: busy,
                tx_frames: ch.tx_frames - self.last_sample.tx[c],
                delivered: ch.rx.delivered - self.last_sample.delivered[c],
                app_originated: self.app.originated,
                app_delivered: self.app.delivered,
            });
            self.last_sample.tx[c] = ch.tx_frames;
            self.last_sample.delivered[c] = ch.rx.delivered;
        }
        self.last_sample.t_us = now_us;
    }

    /// The time series as CSV.
    pub fn csv(&self) -> String {
        let mut s = String::from(
            "t_s,channel,busy_fraction,tx_frames,delivered,app_originated,app_delivered\n",
        );
        for r in &self.samples {
            s.push_str(&format!(
                "{},{},{:.6},{},{},{},{}\n",
                r.t_s,
                r.channel,
                r.busy_fraction,
                r.tx_frames,
                r.delivered,
                r.app_originated,
                r.app_delivered
            ));
        }
        s
    }
}

/// Nearest-rank percentile of a sorted slice.
pub fn percentile(sorted: &[u64], p: f64) -> Option<u64> {
    if sorted.is_empty() {
        return None;
    }
    let rank = ((p / 100.0) * sorted.len() as f64).ceil().max(1.0) as usize;
    Some(sorted[rank.min(sorted.len()) - 1])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcomes_balance_candidates() {
        let mut m = Metrics::new(2);
        let outcomes = [
            Outcome::Delivered,
            Outcome::BelowSensitivity,
            Outcome::LostCapture,
            Outcome::SinrFail { adjacent: true },
            Outcome::Truncated,
            Outcome::Deaf(Deaf::Cad),
            Outcome::Deaf(Deaf::HalfDuplex),
        ];
        for o in outcomes {
            m.candidate(1, 4);
            m.outcome(1, 4, o);
        }
        let t = m.rx_total();
        assert_eq!(t.candidates, 7);
        assert_eq!(t.decided(), 7);
        assert_eq!(t.half_duplex, 1);
        assert_eq!(t.deaf.cad, 1);
        assert_eq!(t.sinr_fail_adjacent, 1);
        assert_eq!(m.nodes[1].rx, t);
    }

    #[test]
    fn load_counts_runs_at_the_cap() {
        let ms = bark_proto::Duration::from_millis;
        let mut l = LoadStats::default();
        for bucket in [500, 100, 100, 500, 100, 10, 100, 500] {
            let load = bark_proto::Load {
                bucket: ms(bucket),
                send_queue: 2,
                delivery_queue: 1,
                heard: 70,
                neighbours: 64,
            };
            l.add(&load, ms(380), ms(242), 64);
        }
        assert_eq!((l.samples, l.at_cap, l.below_reserve), (8, 5, 5));
        assert_eq!((l.cap_episodes, l.longest_cap), (2, 3));
        assert_eq!((l.neighbours_full, l.send_queue_sum), (8, 16));
    }

    #[test]
    fn percentiles() {
        let v = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        assert_eq!(percentile(&v, 50.0), Some(5));
        assert_eq!(percentile(&v, 90.0), Some(9));
        assert_eq!(percentile(&v, 100.0), Some(10));
        assert_eq!(percentile(&[], 50.0), None);
    }
}
