//! The leaf (spec "Leaves"): a carried node that attaches to one relay,
//! hands it the messages its application sends, and takes delivery of
//! the messages for it. It sends no heartbeats and forwards nothing.
//!
//! # Receive mode
//!
//! Only the always-listening mode of spec "Leaf receive mode" is implemented: the leaf
//! listens on its relay's home channel all the time and declares
//! [`RxMode::Listening`] in its ATTACH. The duty-cycled and hybrid modes,
//! which sleep between CADs and need the relay's long preamble, are
//! declared on the wire (`wire.rs`) and served by the relay, but no leaf
//! runs them yet. The leaf keeps the window in which its next poll comes
//! ([`Leaf::poll_window`]), which such a leaf would listen in.
//!
//! # Relay choice (spec "Relay choice")
//!
//! The leaf keeps the relays it hears in a small table ([`CANDIDATES`]),
//! each with the mean RSSI of its last two heartbeats. It attaches to the
//! strongest once it has listened for a worst-case rotation
//! (13 × `heartbeat_max`, 195 s) from the first heartbeat it heard, by
//! which time every relay in range has visited its channel; a leaf with a
//! message to send does not wait, and takes the strongest heard so far.
//!
//! Attached, it moves to another relay only when that relay has been heard
//! over at least [`SWITCH_HEARTBEATS`] heartbeats and is stronger by
//! [`SWITCH_MARGIN_DB`]. It attaches to the new relay at once and leaves
//! the old one to drop it.
//!
//! # Keeping the relay (spec "Attach and polls")
//!
//! The leaf attaches with an ATTACH, which the relay answers with ATTACHED;
//! that names the time of the leaf's first poll. After that the relay
//! drives: at each poll, an ATTACHED that names the next, the leaf answers
//! with one ATTACH marked as a reply, and sends no ATTACH otherwise. A
//! poll is taken only in its window, so that another relay's poll of the
//! same leaf ID on the same channel is ignored.
//!
//! A leaf whose window passes empty no longer knows when its next poll
//! comes, and takes the next one whenever it comes. It does not give its
//! relay up on a clock: a phone in a pocket or a metro is out of reach of
//! every relay, and should find its relay when it is back. It attaches
//! again, a random 0 to `leaf_retry` (60 s) after the next heartbeat it
//! hears, from any relay, unless a poll comes first; if it hears none, a
//! rotation (195 s) after the window, so that a dark phone sends little
//! and phones that come back together, a train surfacing, do not attach
//! at once. The ATTACH goes to its relay if the leaf has heard it since
//! the polls stopped; to the strongest other relay heard since then if it
//! has not, which is how a leaf leaves a dead relay; and to its relay if
//! it has heard none. A relay that still has the leaf answers and keeps
//! its slot.
//!
//! A message the relay did not take is tried again one `leaf_retry` later,
//! as long as the fading of a link lasts (correlation time 60 s in the
//! scenarios), so one fade costs a wait, not the relay. An ATTACH
//! unanswered at every attempt waits for a heartbeat as above. Failures in
//! two retries in a row give the relay up only if the leaf has heard
//! another relay since the first. A relay's heartbeat that names a new
//! home channel (spec "Channel moves") moves the leaf with it; the schedule goes on there.
//! A leaf that has lost its relay takes the strongest other one it has
//! heard, as above.
//!
//! # Sending
//!
//! A message from the application goes to the relay as DATA with `hops`
//! and `src_hop` zero (spec "Packet formats"), with the per-hop transfer of spec "Hand-over". The leaf
//! holds one message at a time, and takes one from the application only
//! when it holds none ([`Core::accepts_send`]); nothing about it is
//! reported back. A message the leaf cannot hand over, because it has no
//! relay or its relay does not answer, is held and tried again as above
//! for up to the delivery TTL (spec "Leaves": 10 minutes), the time the network
//! itself would keep it; then it is dropped with [`DropReason::NoRelay`].
//! A relay that refuses the message (spec "Receiving DATA") is busy, not failing: the
//! leaf holds the message and offers it again after one [`BAN`], the
//! time the relay's bucket takes to refill what it could not fit, doubled
//! after each further refusal in a row ([`hold`]; roadmap D27, D29).
//!
//! [`BAN`]: crate::tables::budget::BAN
//!
//! # Receiving
//!
//! A DELIVER for this leaf is ACKed with the leaf's ID as `from` and its
//! relay's short name as `to` (spec "Packet formats"), and surfaced once. One that arrives
//! while the leaf's one owed reply is another's is ignored, as a relay
//! ignores DATA then (spec "The ACK"). A relay re-sends the oldest message it holds
//! for a leaf before any other (`relay/service.rs`), so a repeat follows
//! its first copy with nothing from that relay in between; remembering
//! the last [`SEEN`] messages covers the relay and the one the leaf has
//! just left.

use crate::event::{Action, AppCommand, AppEvent, DropReason, Event, Frame, Trace, MAX_FRAME};
use crate::ids::{LeafId, MsgId};
use crate::node::mac::{self, Radio};
use crate::node::transfer::{Check, Missed, Replies, Reply, Step, Transfer};
use crate::node::{Config, Core, Rng};
use crate::position::{Position, ShortName};
use crate::radio::{Channel, CHANNELS, MIN_PREAMBLE_SYMBOLS};
use crate::tables::budget::{hold, Budget};
use crate::tables::busy::NOMINAL_NOISE_FLOOR_DBM;
use crate::tables::leaves::{POLL_AIRTIME, POLL_LATE};
use crate::tables::neighbours::NeighbourParams;
use crate::time::{Duration, Instant};
use crate::wire::{
    Ack, AckName, Attach, Attached, Data, Deliver, Heartbeat, Packet, RxMode, MAX_PAYLOAD,
    POLL_UNIT,
};

/// How much stronger, in its mean over two heartbeats, another relay must
/// be to take a leaf from its relay (spec "Relay choice").
///
/// A heartbeat's RSSI is the link's mean plus fading with σ = 4 dB in the
/// scenarios (`scenarios/s2-block.toml`, after typical indoor multipath).
/// A relay's heartbeats reach the leaf's channel once per rotation, ≈130 s
/// apart against a fading correlation time of 60 s, so they are close to
/// independent: a mean of two has σ/√2, and the difference of two such
/// means σ√(1/2 + 1/2) = σ = 4 dB. Two standard deviations, 8 dB, keeps
/// the leaf where it is on all but 2.3 % of comparisons with a relay that
/// is no better.
pub const SWITCH_MARGIN_DB: i16 = 8;

/// Heartbeats a relay must have been heard over before a leaf switches to
/// it (spec "Relay choice": 2); see [`SWITCH_MARGIN_DB`].
pub const SWITCH_HEARTBEATS: u8 = 2;

/// Relays a leaf keeps track of besides its own. An indoor leaf reaches
/// relays within ≈300 m (spec "Physical layer"); at the design's indoor density of ≈9.6
/// relays/km² (spec "Capacity and sizing") that is π × 0.3² × 9.6 ≈ 2.7 relays. Where there are
/// more, the weakest is forgotten first, so the table always holds the
/// strongest.
pub const CANDIDATES: usize = 4;

/// Consecutive retry periods with a failed hand-over after which a leaf
/// gives its relay up (spec "Attach and polls").
pub const FAILED_PERIODS: u8 = 2;

/// Frequency tolerance of each end's clock, parts per million (spec "Attach and polls"). A
/// 32.768 kHz tuning-fork crystal is typically ±20 ppm at 25 °C and drifts
/// by −0.034 ppm/°C² away from it, ≈ −30 ppm more at −5 °C; ±50 ppm covers
/// a crystal from −5 to +55 °C. The nRF52840's internal RC oscillator, at
/// ±500 ppm (Product Specification, CLOCK), would make the window 1.1 s
/// instead of 0.58 s, so a sleeping leaf times its slot from a crystal.
pub const CLOCK_PPM: u64 = 50;

/// DELIVERs a leaf remembers; see the module documentation.
pub const SEEN: usize = 2;

/// Who started the outstanding excursion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Job {
    Send,
    Ack,
}

/// What the hand-over carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sending {
    Attach,
    Data,
}

/// A relay the leaf has heard.
#[derive(Clone, Copy, Debug)]
struct Heard {
    position: Position,
    home: Channel,
    /// Mean RSSI of the last two heartbeats, dBm; the one heartbeat until
    /// there are two.
    rssi: i16,
    /// RSSI of the last heartbeat, dBm.
    last: i16,
    /// Heartbeats heard, up to [`SWITCH_HEARTBEATS`].
    heartbeats: u8,
    last_heard: Instant,
}

impl Heard {
    fn new(hb: &Heartbeat, rssi: i16, now: Instant) -> Heard {
        Heard {
            position: hb.position,
            home: hb.home,
            rssi,
            last: rssi,
            heartbeats: 1,
            last_heard: now,
        }
    }

    /// Another heartbeat from this relay.
    fn heard(&mut self, hb: &Heartbeat, rssi: i16, now: Instant) {
        self.rssi = (self.last as i32 + rssi as i32).div_euclid(2) as i16;
        self.last = rssi;
        self.heartbeats = (self.heartbeats + 1).min(SWITCH_HEARTBEATS);
        self.home = hb.home;
        self.last_heard = now;
    }
}

/// Leaf role. Small: the simulator holds twenty thousand of these.
pub struct Leaf {
    config: Config,
    rng: Rng,
    id: LeafId,
    started: bool,
    /// The channel the leaf listens on: its relay's home.
    home: Channel,
    radio: Radio<Job>,
    relay: Option<Heard>,
    /// Other relays heard, strongest kept.
    candidates: [Option<Heard>; CANDIDATES],
    /// When the leaf first heard a heartbeat since it last knew no relay at
    /// all: a rotation later it has heard every relay in range.
    listening_since: Option<Instant>,
    /// The relay has answered an ATTACH with ATTACHED since the leaf chose
    /// it.
    attached: bool,
    /// When the ATTACH that attaches is due, while not attached or adrift.
    attach_at: Instant,
    /// Retry periods in a row in which a hand-over to the relay failed.
    failed_periods: u8,
    /// When the first of them failed.
    failing_since: Instant,
    /// The next ATTACH waits for a heartbeat: then it is due a random
    /// wait later, unless `attach_at` comes first.
    awaiting_heartbeat: bool,
    /// When the next poll's slot begins, as the last ATTACHED said.
    poll_at: Instant,
    /// When the last ATTACHED arrived: the clocks drift from there, and
    /// the polls stopped then if they did.
    told_at: Instant,
    /// A window passed without a poll: the leaf does not know when the
    /// next comes, takes one whenever it does, and attaches again.
    adrift: bool,
    transfer: Transfer,
    sending: Sending,
    /// The message being sent, as DATA; readdressed for each hand-over.
    outgoing: Option<Frame>,
    /// When the message is given up if it has not been handed over.
    outgoing_until: Instant,
    /// Not before this is the message offered: the relay said it was busy.
    outgoing_at: Instant,
    /// Refusals of the message in a row; each doubles the hold
    /// (`budget::hold`).
    refusals: u8,
    acks: Replies,
    budget: Budget,
    seen: [MsgId; SEEN],
    seen_next: u8,
}

impl Leaf {
    /// Create a leaf; see [`crate::Relay::new`] for `seed`. Its ID is
    /// `config.leaf_id`, or one drawn from `seed` if that is zero.
    pub fn new(config: Config, seed: u64) -> Leaf {
        let mut rng = Rng::new(seed);
        let mut id = config.leaf_id;
        while id == LeafId::RELAY {
            id = LeafId((rng.next_u64() as u32).to_be_bytes());
        }
        let first = Channel::all().next().expect("the plan has channels");
        let random = Channel::new(rng.range(0, CHANNELS as u64 - 1) as u8).unwrap_or(first);
        Leaf {
            home: config.home_override.unwrap_or(random),
            config,
            rng,
            id,
            started: false,
            radio: Radio::new(),
            relay: None,
            candidates: [None; CANDIDATES],
            listening_since: None,
            attached: false,
            attach_at: Instant::ZERO,
            failed_periods: 0,
            poll_at: Instant::ZERO,
            told_at: Instant::ZERO,
            failing_since: Instant::ZERO,
            awaiting_heartbeat: false,
            adrift: false,
            transfer: Transfer::new(),
            sending: Sending::Attach,
            outgoing: None,
            outgoing_until: Instant::ZERO,
            outgoing_at: Instant::ZERO,
            refusals: 0,
            acks: Replies::new(),
            budget: Budget::new(),
            seen: [MsgId(0); SEEN],
            seen_next: 0,
        }
    }

    /// Configuration.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// This leaf's ID.
    pub fn id(&self) -> LeafId {
        self.id
    }

    /// The relay this leaf is attached to: chosen, and its ATTACH
    /// answered. Its position plus [`Leaf::id`] is the leaf's address
    /// (spec "Addresses").
    pub fn relay(&self) -> Option<Position> {
        self.relay.filter(|_| self.attached).map(|r| r.position)
    }

    /// The message this leaf holds to hand to its relay, if any.
    pub fn holding(&self) -> Option<MsgId> {
        self.outgoing_id()
    }

    /// The channel the leaf listens on.
    pub fn home(&self) -> Channel {
        self.home
    }

    /// When the next poll can come, while attached and not adrift: from its
    /// slot less the
    /// drift of two clocks at [`CLOCK_PPM`] since the last one and half a
    /// [`POLL_UNIT`] of rounding, to the slot plus the poll's lateness and
    /// airtime and the same margin. A sleeping leaf listens in it (spec "Attach and polls").
    pub fn poll_window(&self) -> Option<(Instant, Instant)> {
        if !self.attached || self.adrift {
            return None;
        }
        let since = self.poll_at.since(self.told_at).as_micros();
        let margin = Duration::from_micros(
            (since * 2 * CLOCK_PPM).div_ceil(1_000_000) + POLL_UNIT.as_micros() / 2,
        );
        let start =
            Instant::from_micros(self.poll_at.as_micros().saturating_sub(margin.as_micros()));
        Some((start, self.poll_at + POLL_LATE + POLL_AIRTIME + margin))
    }

    /// How long a candidate relay may go unheard before the leaf forgets
    /// it: the life of a neighbour entry (spec "Neighbour table").
    fn silence(&self) -> Duration {
        NeighbourParams::from_config(&self.config).expire_after
    }

    /// How long a leaf listens before it has heard every relay in range: a
    /// worst-case rotation, 13 × `heartbeat_max` (spec "Heartbeat").
    fn rotation(&self) -> Duration {
        NeighbourParams::from_config(&self.config).stale_after
    }

    /// RSSI above which listen-before-talk finds the channel busy: the
    /// nominal noise floor plus the busy margin (spec "Busy estimate"). A leaf runs no
    /// sweep to calibrate its floor.
    fn busy_dbm(&self) -> i16 {
        NOMINAL_NOISE_FLOOR_DBM + self.config.busy_threshold_db as i16
    }

    // --- Relay choice (spec "Relay choice") ---

    fn heartbeat(&mut self, hb: Heartbeat, rssi: i16, now: Instant, emit: &mut dyn FnMut(Action)) {
        self.listening_since.get_or_insert(now);
        if self.awaiting_heartbeat {
            // A relay is in reach again. The wait spreads the leaves that
            // come back together over a minute (spec "Attach and polls").
            self.awaiting_heartbeat = false;
            let wait = self.rng.duration(Duration::ZERO, self.config.leaf_retry);
            self.attach_at = self.attach_at.min(now + wait);
        }
        if let Some(mut relay) = self.relay.filter(|r| r.position == hb.position) {
            let moved = relay.home != hb.home;
            relay.heard(&hb, rssi, now);
            self.relay = Some(relay);
            if moved {
                // The announcement of a move (spec "Channel moves"): follow; the polls go on
                // on the new channel.
                self.home = hb.home;
                emit(Action::Listen(hb.home));
            }
        } else {
            self.remember(&hb, rssi, now);
        }
        self.choose(now, emit);
    }

    /// Note a heartbeat from a relay other than the leaf's own.
    fn remember(&mut self, hb: &Heartbeat, rssi: i16, now: Instant) {
        if let Some(c) = self
            .candidates
            .iter_mut()
            .flatten()
            .find(|c| c.position == hb.position)
        {
            c.heard(hb, rssi, now);
            return;
        }
        let silence = self.silence();
        let free = self
            .candidates
            .iter()
            .position(|c| c.is_none_or(|c| now.since(c.last_heard) >= silence));
        let slot = free.or_else(|| {
            let (i, weakest) = self
                .candidates
                .iter()
                .enumerate()
                .filter_map(|(i, c)| c.map(|c| (i, c)))
                .min_by_key(|(_, c)| c.rssi)?;
            (weakest.rssi < rssi).then_some(i)
        });
        if let Some(i) = slot {
            self.candidates[i] = Some(Heard::new(hb, rssi, now));
        }
    }

    /// The strongest candidate heard within the relay silence, and its slot.
    fn strongest(&self, now: Instant) -> Option<(usize, Heard)> {
        let silence = self.silence();
        self.candidates
            .iter()
            .enumerate()
            .filter_map(|(i, c)| c.map(|c| (i, c)))
            .filter(|(_, c)| now.since(c.last_heard) < silence)
            .max_by_key(|(_, c)| c.rssi)
    }

    /// Attach to a relay, or move to a better one, if the rules of spec "Relay choice"
    /// say so now.
    fn choose(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        let Some((slot, best)) = self.strongest(now) else {
            if self.relay.is_none() {
                // Nothing heard lately: the next heartbeat starts a new
                // rotation of listening.
                self.listening_since = None;
            }
            return;
        };
        let take = match self.relay {
            None => {
                self.outgoing.is_some()
                    || self
                        .listening_since
                        .is_some_and(|t| now.since(t) >= self.rotation())
            }
            Some(r) => {
                best.heartbeats >= SWITCH_HEARTBEATS && best.rssi >= r.rssi + SWITCH_MARGIN_DB
            }
        };
        if take {
            self.candidates[slot] = self.relay.take();
            self.switch(best, now, emit);
        }
    }

    /// Take `relay` and attach to it now.
    fn switch(&mut self, relay: Heard, now: Instant, emit: &mut dyn FnMut(Action)) {
        self.relay = Some(relay);
        self.attached = false;
        self.adrift = false;
        self.awaiting_heartbeat = false;
        self.attach_at = now;
        self.failed_periods = 0;
        // A hand-over in progress was for the old relay or channel.
        self.transfer.cancel();
        if relay.home != self.home {
            self.home = relay.home;
            emit(Action::Listen(relay.home));
        }
    }

    /// The relay stopped answering hand-overs, and another relay has been
    /// heard since.
    fn lose_relay(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        self.relay = None;
        self.attached = false;
        self.adrift = false;
        self.failed_periods = 0;
        self.transfer.cancel();
        self.choose(now, emit);
    }

    /// Whether a relay other than the leaf's own has been heard since `t`.
    fn heard_other_since(&self, t: Instant) -> bool {
        self.candidates.iter().flatten().any(|c| c.last_heard > t)
    }

    /// Before an ATTACH of a leaf adrift: if it has not heard its relay
    /// since the polls stopped but has heard another, its relay is dead or
    /// out of reach, and it takes the strongest of those (spec "Relay choice").
    fn reconsider(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        let since = self.told_at;
        let Some(relay) = self.relay else { return };
        if relay.last_heard > since {
            return;
        }
        let best = self
            .candidates
            .iter()
            .enumerate()
            .filter_map(|(i, c)| c.map(|c| (i, c)))
            .filter(|(_, c)| c.last_heard > since)
            .max_by_key(|(_, c)| c.rssi);
        if let Some((slot, best)) = best {
            self.candidates[slot] = self.relay.take();
            self.switch(best, now, emit);
        }
    }

    // --- Sending (spec "Hand-over", "Attach and polls") ---

    fn outgoing_id(&self) -> Option<MsgId> {
        match Packet::decode(self.outgoing.as_ref()?.payload()) {
            Ok(Packet::Data(d)) => Some(d.msg_id()),
            _ => None,
        }
    }

    fn app(&mut self, cmd: AppCommand<'_>, now: Instant, emit: &mut dyn FnMut(Action)) {
        let AppCommand::Send {
            dst_pos,
            dst_leaf,
            payload,
        } = cmd
        else {
            return;
        };
        if payload.len() > MAX_PAYLOAD || !self.accepts_send(now) {
            // The driver broke its contract (`docs/core-interface.md` §4).
            return;
        }
        // Addressed when its hand-over starts.
        let d = Data {
            sender_home: self.home,
            dst_hop: Default::default(),
            src_hop: Default::default(),
            dst_pos,
            dst_leaf,
            hops: 0,
            payload,
        };
        self.outgoing = Some(mac::frame(
            &Packet::Data(d),
            self.home,
            MIN_PREAMBLE_SYMBOLS,
        ));
        self.outgoing_until = now + self.config.delivery_ttl;
        if self.relay.is_none() {
            self.choose(now, emit);
        }
    }

    /// The message could not be handed over within the delivery TTL.
    fn give_up_message(&mut self, emit: &mut dyn FnMut(Action)) {
        let Some(msg_id) = self.outgoing_id() else {
            return;
        };
        if self.sending == Sending::Data {
            self.transfer.cancel();
        }
        self.outgoing = None;
        self.refusals = 0;
        emit(Action::Trace(Trace::Dropped {
            msg_id,
            reason: DropReason::NoRelay,
        }));
    }

    /// Start a hand-over if none is running: the ATTACH that attaches when
    /// it is due, until the relay has answered one and again while adrift;
    /// else the message.
    fn prepare(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        if !self.transfer.idle() || self.relay.is_none() {
            return;
        }
        let attach = (!self.attached || self.adrift) && now >= self.attach_at;
        if attach && self.adrift {
            self.reconsider(now, emit);
        }
        let Some(relay) = self.relay else { return };
        let from = AckName::from(relay.position.short());
        if attach {
            self.sending = Sending::Attach;
            let relay = relay.position.short();
            emit(Action::Trace(Trace::Attaching { relay }));
            // Answered by ATTACHED, never by an ACK (`attached_rx`).
            let expect = Ack {
                msg_id: MsgId(0),
                from,
                to: self.id.into(),
                refused: false,
            };
            self.transfer.start(expect, 0, now);
        } else if let Some(msg_id) = self
            .outgoing_id()
            .filter(|_| self.attached && now >= self.outgoing_at)
        {
            self.sending = Sending::Data;
            // A leaf's DATA names no sender (`src_hop` 0, spec "Packet formats").
            let expect = Ack {
                msg_id,
                from,
                to: ShortName(0).into(),
                refused: false,
            };
            self.transfer.start(expect, 0, now);
        }
    }

    /// The frame of the hand-over, addressed to the relay as it is now.
    fn frame(&self) -> Option<Frame> {
        let relay = self.relay?.position.short();
        let packet = match self.sending {
            Sending::Attach => Packet::Attach(Attach {
                leaf: self.id,
                relay,
                mode: self.mode(),
                reply: false,
            }),
            Sending::Data => {
                let Ok(Packet::Data(d)) = Packet::decode(self.outgoing.as_ref()?.payload()) else {
                    return None;
                };
                let addressed = Data {
                    sender_home: self.home,
                    dst_hop: relay,
                    ..d
                };
                return Some(mac::frame(
                    &Packet::Data(addressed),
                    self.home,
                    MIN_PREAMBLE_SYMBOLS,
                ));
            }
        };
        Some(mac::frame(&packet, self.home, MIN_PREAMBLE_SYMBOLS))
    }

    fn start_send(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) -> bool {
        if !self.transfer.due(now) {
            return false;
        }
        let Some(frame) = self.frame() else {
            self.transfer.cancel();
            return false;
        };
        let airtime = mac::airtime(&frame);
        if !self.budget.would_allow(airtime, now) {
            let until = self
                .budget
                .allowed_at(airtime, Duration::ZERO, now)
                .unwrap_or(now + self.config.leaf_retry);
            self.transfer.defer(until);
            return false;
        }
        self.transfer.begin_lbt(now);
        self.radio.start(Job::Send, Action::Cad(self.home), emit);
        true
    }

    fn send_check(&mut self, busy: bool, now: Instant, emit: &mut dyn FnMut(Action)) {
        let step = self.transfer.step();
        if !matches!(step, Step::Cad | Step::Rssi) {
            return;
        }
        let backoff = self.config.hop_backoff_max;
        match self.transfer.checked(busy, now, &mut self.rng, backoff) {
            Check::Clear if step == Step::Cad => {
                self.radio.start(Job::Send, Action::Rssi(self.home), emit)
            }
            Check::Clear => {
                if self.acks.wake_at().is_some_and(|t| t <= now) {
                    self.transfer.hold(now);
                    return;
                }
                let Some(frame) = self.frame() else {
                    self.transfer.cancel();
                    return;
                };
                let airtime = mac::airtime(&frame);
                if !self.budget.would_allow(airtime, now) {
                    let until = self
                        .budget
                        .allowed_at(airtime, Duration::ZERO, now)
                        .unwrap_or(now + self.config.leaf_retry);
                    self.transfer.defer(until);
                    return;
                }
                self.budget.record(airtime, now);
                self.transfer.transmitting();
                self.radio.start(Job::Send, Action::Transmit(frame), emit);
            }
            Check::Busy => {}
            Check::GaveUp => {
                let (max, retry) = (self.config.hop_attempts, self.config.retry_backoff);
                if self.transfer.lost_attempt(now, max, &mut self.rng, retry) == Missed::Failed {
                    self.send_failed(now, emit);
                }
            }
        }
    }

    /// Every attempt of a hand-over to the relay went unanswered: try again
    /// a retry period later, or give the relay up after failures in
    /// [`FAILED_PERIODS`] periods in a row. A message stays held.
    fn send_failed(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        if let (Sending::Attach, Some(r)) = (self.sending, self.relay) {
            let relay = r.position.short();
            emit(Action::Trace(Trace::AttachFailed { relay }));
        }
        self.failed_periods = self.failed_periods.saturating_add(1);
        if self.failed_periods == 1 {
            self.failing_since = now;
        }
        // A leaf that has heard no other relay since may be out of reach of
        // all, not left by its relay: it keeps it and tries again.
        if self.failed_periods >= FAILED_PERIODS && self.heard_other_since(self.failing_since) {
            return self.lose_relay(now, emit);
        }
        match self.sending {
            Sending::Attach => self.await_heartbeat(now),
            Sending::Data => {
                let later = now + self.config.leaf_retry;
                self.outgoing_at = self.outgoing_at.max(later);
            }
        }
    }

    /// The next ATTACH waits for a heartbeat, which says a relay is in
    /// reach; at the latest it goes a rotation on, when every relay in
    /// reach would have been heard (spec "Attach and polls").
    fn await_heartbeat(&mut self, now: Instant) {
        self.awaiting_heartbeat = true;
        self.attach_at = now + self.rotation();
    }

    fn acked(&mut self, ack: Ack, now: Instant) {
        if self.sending != Sending::Data {
            return;
        }
        if ack.refused {
            return self.refused(ack, now);
        }
        if !self.transfer.acked(ack) {
            return;
        }
        if let Some(r) = &mut self.relay {
            r.last_heard = now;
        }
        self.failed_periods = 0;
        self.outgoing = None;
        self.refusals = 0;
    }

    /// The receive mode this leaf declares (spec "Leaf receive mode").
    fn mode(&self) -> RxMode {
        RxMode::Listening
    }

    /// An ATTACHED for this leaf: the answer to its ATTACH, or in its window
    /// a poll, which it answers once (spec "Attach and polls"). Either names the next poll.
    fn attached_rx(&mut self, a: Attached, now: Instant) {
        let Some(relay) = self.relay else { return };
        let attaching = self.sending == Sending::Attach && !self.transfer.idle();
        let answer = attaching && self.transfer.transmitted();
        let in_window = self
            .poll_window()
            .is_some_and(|(start, end)| now >= start && now < end);
        if answer {
            self.transfer.cancel();
            self.attached = true;
            self.failed_periods = 0;
        } else if in_window || (self.attached && self.adrift) {
            if attaching {
                // The poll came first: no ATTACH is needed.
                self.transfer.cancel();
            }
            let reply = Reply::Attach(Attach {
                leaf: self.id,
                relay: relay.position.short(),
                mode: self.mode(),
                reply: true,
            });
            if self.acks.can_owe(reply, self.home, now) {
                self.acks
                    .owe(reply, self.home, now + self.config.ack_timeout, now);
            }
        } else {
            return;
        }
        self.poll_at = now + a.next_poll();
        self.told_at = now;
        self.adrift = false;
        self.awaiting_heartbeat = false;
        if let Some(r) = &mut self.relay {
            r.last_heard = now;
        }
    }

    /// The window of the expected poll passed empty: the leaf no longer
    /// knows when its next poll comes, and attaches again after the next
    /// heartbeat it hears.
    fn poll_missed(&mut self, now: Instant) {
        self.adrift = true;
        self.await_heartbeat(now);
    }

    /// The frame of the owed reply.
    fn reply_frame(o: crate::node::transfer::Owed) -> Frame {
        let packet = match o.reply {
            Reply::Ack(a) => Packet::Ack(a),
            Reply::Attach(a) => Packet::Attach(a),
            // A relay's reply.
            Reply::Attached(leaf) => Packet::Attached(Attached {
                leaf,
                next_poll: 0,
                waiting: 0,
            }),
        };
        mac::frame(&packet, o.channel, MIN_PREAMBLE_SYMBOLS)
    }

    /// The relay refused the message (spec "Receiving DATA"). It hears the leaf, so this
    /// counts as hearing it and is no failure; the message waits out the
    /// hold.
    fn refused(&mut self, ack: Ack, now: Instant) {
        if self.sending != Sending::Data || !self.transfer.answered_by(ack) {
            return;
        }
        self.transfer.cancel();
        if let Some(r) = &mut self.relay {
            r.last_heard = now;
        }
        self.failed_periods = 0;
        self.refusals = self.refusals.saturating_add(1);
        self.outgoing_at = now + hold(self.refusals);
    }

    // --- Receiving (spec "Delivery") ---

    fn deliver(&mut self, d: Deliver<'_>, now: Instant, emit: &mut dyn FnMut(Action)) {
        // The relay is the leaf's own: DELIVER does not name its sender.
        let relay = self.relay.map_or(ShortName(0), |r| r.position.short());
        let ack = Ack {
            msg_id: d.msg_id,
            from: self.id.into(),
            to: relay.into(),
            refused: false,
        };
        // Taken only if it can be answered, as a relay takes DATA: the
        // relay's retry brings it again (spec "The ACK", roadmap D29).
        if !self.acks.can_owe(ack, self.home, now) {
            return;
        }
        self.acks
            .owe(ack, self.home, now + self.config.ack_timeout, now);
        if self.seen.contains(&d.msg_id) {
            return;
        }
        self.seen[self.seen_next as usize] = d.msg_id;
        self.seen_next = ((self.seen_next as usize + 1) % SEEN) as u8;
        let mut payload = [0; MAX_FRAME];
        payload[..d.payload.len()].copy_from_slice(d.payload);
        emit(Action::App(AppEvent::Message {
            msg_id: d.msg_id,
            len: d.payload.len() as u8,
            payload,
        }));
    }

    fn start_ack(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) -> bool {
        let Some(o) = self.acks.due(now) else {
            return false;
        };
        let frame = Self::reply_frame(o);
        if !self.budget.would_allow(mac::airtime(&frame), now) {
            self.acks.abandon();
            return true;
        }
        match self.acks.begin() {
            Some(cad) => {
                self.radio.start(Job::Ack, cad, emit);
                true
            }
            None => false,
        }
    }

    fn ack_check(&mut self, busy: bool, now: Instant, emit: &mut dyn FnMut(Action)) {
        let backoff = self.config.hop_backoff_max;
        match self.acks.checked(busy, now, &mut self.rng, backoff) {
            Some((o, false)) => self.radio.start(Job::Ack, Action::Rssi(o.channel), emit),
            Some((o, true)) => {
                let frame = Self::reply_frame(o);
                let airtime = mac::airtime(&frame);
                if self.budget.would_allow(airtime, now) {
                    self.budget.record(airtime, now);
                    self.radio.start(Job::Ack, Action::Transmit(frame), emit);
                } else {
                    self.acks.abandon();
                }
            }
            None => {}
        }
    }

    fn received(&mut self, bytes: &[u8], rssi: i16, now: Instant, emit: &mut dyn FnMut(Action)) {
        match Packet::decode(bytes) {
            Ok(Packet::Heartbeat(hb)) => self.heartbeat(hb, rssi, now, emit),
            Ok(Packet::Ack(a)) => self.acked(a, now),
            Ok(Packet::Deliver(d)) if d.dst_leaf == self.id => self.deliver(d, now, emit),
            Ok(Packet::Attached(a)) if a.leaf == self.id => self.attached_rx(a, now),
            _ => {}
        }
    }

    fn timers(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        let (max, retry) = (self.config.hop_attempts, self.config.retry_backoff);
        if self.transfer.timeout(now, max, &mut self.rng, retry) == Some(Missed::Failed) {
            self.send_failed(now, emit);
        }
        if self.poll_window().is_some_and(|(_, end)| now >= end) {
            self.poll_missed(now);
        }
        if self.relay.is_none() {
            // The end of the first rotation of listening.
            self.choose(now, emit);
        }
        if self.outgoing.is_some() && now >= self.outgoing_until {
            self.give_up_message(emit);
        }
    }

    fn pump(&mut self, now: Instant, emit: &mut dyn FnMut(Action)) {
        while self.radio.idle() {
            self.prepare(now, emit);
            if !(self.start_ack(now, emit) || self.start_send(now, emit)) {
                break;
            }
        }
    }
}

impl Core for Leaf {
    fn handle(&mut self, event: Event<'_>, now: Instant, emit: &mut dyn FnMut(Action)) {
        if !self.started {
            self.started = true;
            emit(Action::Listen(self.home));
        }
        let busy_dbm = self.busy_dbm();
        match event {
            Event::Wakeup => {}
            Event::Received {
                rssi_dbm, bytes, ..
            } => self.received(bytes, rssi_dbm, now, emit),
            Event::CadDone { busy, .. } => match self.radio.complete() {
                Some(Job::Send) => self.send_check(busy, now, emit),
                Some(Job::Ack) => self.ack_check(busy, now, emit),
                None => {}
            },
            Event::RssiSample { dbm, .. } => match self.radio.complete() {
                Some(Job::Send) => self.send_check(dbm > busy_dbm, now, emit),
                Some(Job::Ack) => self.ack_check(dbm > busy_dbm, now, emit),
                None => {}
            },
            Event::TxDone => match self.radio.complete() {
                Some(Job::Send) if self.transfer.step() == Step::Tx => {
                    self.transfer.sent(now, self.config.ack_timeout)
                }
                Some(Job::Ack) => self.acks.sent(),
                _ => {}
            },
            Event::App(cmd) => self.app(cmd, now, emit),
        }
        self.timers(now, emit);
        self.pump(now, emit);
    }

    fn next_wakeup(&self) -> Option<Instant> {
        if !self.started {
            return Some(Instant::ZERO);
        }
        let mut w: Option<Instant> = None;
        let mut at = |t: Instant| w = Some(w.map_or(t, |w| w.min(t)));
        match self.poll_window() {
            Some((_, end)) => at(end),
            None if self.relay.is_none() => {
                if let Some(t) = self.listening_since {
                    at(t + self.rotation());
                }
            }
            None => {}
        }
        if self.outgoing.is_some() {
            at(self.outgoing_until);
        }
        if self.transfer.awaiting_ack() || self.radio.idle() {
            if let Some(t) = self.transfer.wake_at() {
                at(t);
            }
        }
        if self.radio.idle() {
            if let Some(t) = self.acks.wake_at() {
                at(t);
            }
            if self.transfer.idle() && self.relay.is_some() {
                if !self.attached || self.adrift {
                    at(self.attach_at);
                }
                if self.attached && self.outgoing.is_some() {
                    at(self.outgoing_at);
                }
            }
        }
        w
    }

    /// A leaf holds one message at a time (spec "Attach and polls").
    fn accepts_send(&self, _now: Instant) -> bool {
        self.outgoing.is_none()
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    #[test]
    fn size() {
        // docs/core-interface.md §6: under 1 KiB.
        let leaf = core::mem::size_of::<Leaf>();
        assert!(leaf < 1024, "{leaf} B");
    }

    /// A second DELIVER arrives while the ACK of the first waits out a
    /// busy channel. It is neither ACKed nor surfaced; the relay's retry,
    /// after the first ACK is gone, is (roadmap D29 rule 4).
    #[test]
    fn a_deliver_that_cannot_be_answered_is_not_taken() {
        let home = Channel::new(3).unwrap();
        let config = Config {
            home_override: Some(home),
            leaf_id: LeafId([0, 0, 0, 7]),
            ..Config::default()
        };
        let mut l = Leaf::new(config, 1);
        let mut now = Instant::ZERO;
        l.handle(Event::Wakeup, now, &mut |_| {});
        let deliver = |text: &'static [u8]| {
            let d = Deliver {
                dst_leaf: l.id,
                msg_id: MsgId(text.len() as u32),
                payload: text,
            };
            let f = mac::frame(&Packet::Deliver(d), home, MIN_PREAMBLE_SYMBOLS);
            (d.msg_id, f.bytes[..f.len as usize].to_vec())
        };
        let ((a, a_bytes), (b, b_bytes)) = (deliver(b"first"), deliver(b"second"));
        let received = |bytes| Event::Received {
            channel: home,
            rssi_dbm: -80,
            snr_db: 10,
            bytes,
        };
        let surfaced = |actions: &[Action], id: MsgId| {
            actions.iter().any(
                |x| matches!(x, Action::App(AppEvent::Message { msg_id, .. }) if *msg_id == id),
            )
        };

        now += Duration::from_secs(1);
        let mut actions = Vec::new();
        l.handle(received(&a_bytes), now, &mut |x| actions.push(x));
        assert!(surfaced(&actions, a));
        assert!(actions.contains(&Action::Cad(home)), "{actions:?}");
        // The channel is busy: A's ACK waits a backoff.
        now += Duration::from_millis(1);
        l.handle(
            Event::CadDone {
                channel: home,
                busy: true,
            },
            now,
            &mut |_| {},
        );
        assert!(l.acks.pending());

        now += Duration::from_millis(1);
        actions.clear();
        l.handle(received(&b_bytes), now, &mut |x| actions.push(x));
        assert!(actions.is_empty(), "{actions:?}");

        // A's ACK goes out; B was never owed one.
        let mut acked = Vec::new();
        let until = now + Duration::from_millis(100);
        while let Some(t) = l.next_wakeup().filter(|t| *t <= until) {
            now = now.max(t);
            let mut pending = Vec::new();
            l.handle(Event::Wakeup, now, &mut |x| pending.push(x));
            while let Some(x) = pending.pop() {
                let event = match x {
                    Action::Cad(channel) => Event::CadDone {
                        channel,
                        busy: false,
                    },
                    Action::Rssi(channel) => Event::RssiSample { channel, dbm: -130 },
                    Action::Transmit(f) => {
                        if let Ok(Packet::Ack(k)) = Packet::decode(f.payload()) {
                            acked.push(k.msg_id);
                        }
                        Event::TxDone
                    }
                    _ => continue,
                };
                l.handle(event, now, &mut |x| pending.push(x));
            }
        }
        assert_eq!(acked, [a]);

        // The relay's retry of B.
        now += Duration::from_millis(200);
        actions.clear();
        l.handle(received(&b_bytes), now, &mut |x| actions.push(x));
        assert!(surfaced(&actions, b));
        assert!(l.acks.pending());
    }
}
