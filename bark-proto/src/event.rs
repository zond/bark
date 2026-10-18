//! What the driver tells the core and what the core asks the driver to do.
//!
//! The contract between the two is in `docs/core-interface.md`. In short: the
//! driver keeps the radio listening on the channel of the last
//! [`Action::Listen`]; CAD, RSSI and transmit are excursions that the driver
//! performs and then reports with exactly one completion event, after which
//! the radio is listening again; the core has at most one radio excursion
//! outstanding at a time.

use crate::ids::{MsgId, NodeId};
use crate::position::Position;
use crate::radio::Channel;
use crate::time::{Duration, Instant};

/// Largest LoRa payload the SX126x carries, and so the largest frame the
/// core will ever emit or accept.
pub const MAX_FRAME: usize = 255;

/// A frame to transmit: the LoRa payload bytes, their count, the channel and
/// the preamble length. Fixed-size so that no action allocates.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Frame {
    /// Channel to transmit on.
    pub channel: Channel,
    /// Preamble symbols; never below `radio::MIN_PREAMBLE_SYMBOLS`.
    pub preamble_symbols: u16,
    /// Number of valid bytes in `bytes`.
    pub len: u8,
    /// Payload bytes; only `..len` are meaningful.
    pub bytes: [u8; MAX_FRAME],
}

impl Frame {
    /// The valid bytes.
    pub fn payload(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }
}

impl core::fmt::Debug for Frame {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Frame")
            .field("channel", &self.channel)
            .field("preamble_symbols", &self.preamble_symbols)
            .field("bytes", &self.payload())
            .finish()
    }
}

/// Something that happened, delivered to the core with the time it is
/// delivered at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event<'a> {
    /// The driver's clock reached the core's `next_wakeup()`.
    Wakeup,
    /// A frame with a valid CRC was received while listening on `channel`.
    Received {
        /// Channel the radio was listening on.
        channel: Channel,
        /// Packet RSSI in dBm as the radio reports it.
        rssi_dbm: i16,
        /// Packet SNR in dB as the radio reports it.
        snr_db: i8,
        /// The LoRa payload.
        bytes: &'a [u8],
    },
    /// Completion of [`Action::Cad`].
    CadDone {
        /// Channel that was checked.
        channel: Channel,
        /// Whether a LoRa preamble was detected.
        busy: bool,
    },
    /// Completion of [`Action::Rssi`].
    RssiSample {
        /// Channel that was sampled.
        channel: Channel,
        /// Instantaneous RSSI in dBm.
        dbm: i16,
    },
    /// Completion of [`Action::Transmit`]: the frame has left the antenna.
    TxDone,
    /// A command from the application (phone over BLE, serial console, or
    /// the simulator's traffic generator).
    App(AppCommand<'a>),
}

/// Commands from the application side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppCommand<'a> {
    /// Originate a message to `(dst_pos, dst_id)` with an opaque payload,
    /// whose ID is [`MsgId::of`] the three. Offered only while
    /// [`crate::Core::accepts_send`] holds, and nothing is reported back
    /// (`docs/core-interface.md` §4).
    Send {
        /// The destination's position (spec "Addresses").
        dst_pos: Position,
        /// The destination node.
        dst_id: NodeId,
        /// Opaque payload; the core never looks inside.
        payload: &'a [u8],
    },
    /// Set or update own position: a map point set in the app, or a GPS fix
    /// that passed the plausibility checks (spec "Position and time").
    SetPosition {
        /// New position.
        position: Position,
    },
}

/// Why a node dropped a message, or refused one (spec "Forwarding"). The network
/// sends no failure notices (roadmap D10); these reach only the
/// simulator's metrics and the firmware's log, through [`Trace`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DropReason {
    /// The origin has no neighbour left to try (spec "Return to parent").
    NoRoute,
    /// Sending it on would need a count above 255 (spec "Hop count").
    HopCap,
    /// No place for its record: every record is of a message this relay
    /// holds (spec "Message records"). Refused; the table outnumbers the send
    /// queue's places, so this does not happen.
    NoRecord,
    /// The send queue had no place (spec "Send queue").
    SendQueueFull,
    /// The duty-cycle bucket could not fit it with everything queued
    /// (spec "Receiving DATA", "Duty cycle cap").
    Budget,
    /// Fresh work, and every neighbour that could take it is banned
    /// (spec "Receiving DATA"). Refused.
    Banned,
    /// The hand-over back to the parent failed (spec "Return to parent", "Hand-over").
    ReturnFailed,
    /// The node it is for did not take it within the delivery TTL (spec
    /// "Delivery queue").
    QueueExpired,
    /// Its time left ran out before it reached its owner (spec "Receiving DATA").
    Expired,
    /// The delivery queue had no room for it (spec "Delivery queue").
    DeliveryFull,
    /// The payload is longer than a DATA frame holds (spec "Packet formats").
    PayloadTooLong,
}

/// Events for the application side: only a message for it. What becomes
/// of a message it sent is not reported (`docs/core-interface.md` §4).
/// `Message` carries its payload by value for the same reason
/// `Action::Transmit` does.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppEvent {
    /// A message arrived for this node's application.
    Message {
        /// Its id.
        msg_id: MsgId,
        /// Number of valid bytes in `payload`.
        len: u8,
        /// Opaque payload; only `..len` is meaningful.
        payload: [u8; MAX_FRAME],
    },
}

/// What the core asks the driver to do.
///
/// `Transmit` carries a full frame by value: the core allocates nothing, so
/// the price of a frame is paid in the enum's size. Drivers hold one at a time.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Set the resting channel. Takes effect at once; the driver listens here
    /// whenever no excursion is in progress.
    Listen(Channel),
    /// Perform channel activity detection on `channel`; report `Event::CadDone`.
    Cad(Channel),
    /// Sample instantaneous RSSI on `channel`; report `Event::RssiSample`.
    Rssi(Channel),
    /// Transmit a frame; report `Event::TxDone`.
    Transmit(Frame),
    /// The radio may receive duty-cycled on the resting channel until this
    /// instant: in windows of [`crate::sniff::SNIFF_RX`] every
    /// [`crate::sniff::SNIFF_CYCLE`], so that it catches every frame with
    /// the long preamble ([`crate::sniff::LONG_PREAMBLE_SYMBOLS`]) and may
    /// miss any other. What it catches is `Event::Received` as ever. The
    /// core's next `Listen`, `Cad`, `Rssi` or `Transmit` ends it sooner;
    /// once it ends the radio listens on the resting channel again
    /// (`docs/core-interface.md` §3 rule 10). Only a sleepy node asks for
    /// it, between its windows.
    Sniff(Instant),
    /// Tell the application something.
    App(AppEvent),
    /// A protocol event for logs and the simulator's metrics. Carries no
    /// obligation; a driver may ignore it (`docs/core-interface.md` §3).
    Trace(Trace),
}

/// A protocol event worth recording, emitted where it happens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trace {
    /// The home channel changed: chosen at the end of the boot listen
    /// (spec "Channel at boot") or moved by the move rule (spec "Channel moves").
    HomeChannelMoved {
        /// Previous home.
        from: Channel,
        /// New home.
        to: Channel,
    },
    /// A heartbeat found its channel busy at every listen-before-talk check
    /// and was skipped (spec "Heartbeat").
    HeartbeatSkipped {
        /// The channel it was for.
        channel: Channel,
    },
    /// The duty-cycle budget held a heartbeat back (spec "Duty cycle cap").
    HeartbeatDeferred {
        /// When the budget may next allow it.
        until: Instant,
    },
    /// This node dropped the message; nothing else is reported about it,
    /// to the application either (spec "Addresses").
    Dropped {
        /// The message.
        msg_id: MsgId,
        /// Why.
        reason: DropReason,
    },
    /// A DATA packet was refused, with an ACK that says so if the budget
    /// allowed one, because acting on it needed what `reason` names
    /// (spec "Receiving DATA"). Not a drop: the sender still holds the message and
    /// will try elsewhere.
    Refused {
        /// The message.
        msg_id: MsgId,
        /// [`DropReason::SendQueueFull`], [`DropReason::Budget`] or
        /// [`DropReason::DeliveryFull`].
        reason: DropReason,
    },
    /// The receiver of a hand-over refused it (spec "Refusal").
    TurnedAway {
        /// The message.
        msg_id: MsgId,
        /// The refusing node.
        by: NodeId,
    },
    /// The duty-cycle budget held a hand-over back (spec "Duty cycle cap").
    HandOverDeferred {
        /// The message.
        msg_id: MsgId,
        /// When the budget may next allow it.
        until: Instant,
    },
    /// A neighbour acknowledged the message, sent on toward the destination
    /// (spec "Next hop").
    Forwarded {
        /// The message.
        msg_id: MsgId,
        /// The neighbour.
        to: NodeId,
        /// The count it carried (spec "Hop count").
        hops: u8,
    },
    /// The parent acknowledged the message, returned to it because no
    /// neighbour was left to try (spec "Return to parent").
    ReturnedToParent {
        /// The message.
        msg_id: MsgId,
        /// The parent.
        to: NodeId,
    },
    /// Every attempt of a hand-over to a neighbour went unanswered, and a
    /// failure was counted against it (spec "Hand-over" step 6).
    HandOverFailed {
        /// The message.
        msg_id: MsgId,
        /// The neighbour.
        to: NodeId,
    },
    /// A neighbour reached the demotion threshold and is no longer a next
    /// hop (spec "Neighbour table").
    Demoted {
        /// The neighbour.
        neighbour: NodeId,
    },
    /// The node a message was held for took it (spec "Delivery").
    Delivered {
        /// The message.
        msg_id: MsgId,
        /// The node.
        node: NodeId,
    },
    /// A sleepy node that sleeps listens a rotation to check its anchor
    /// (spec "Relay choice").
    AnchorCheck,
    /// A window in which a sleeping node woke for its anchor's heartbeat
    /// ended (spec "Relay choice"): with the heartbeat, which started
    /// `offset_us` after the time expected, negative if before it; or
    /// without it. `listened` is how long the window had been open.
    AnchorWatch {
        /// Whether the heartbeat was heard in the window.
        heard: bool,
        /// When it started against the time expected, µs.
        offset_us: i32,
        /// How long the node listened in the window.
        listened: Duration,
    },
    /// A message for another node entered the delivery queue (spec
    /// "Delivery queue").
    Queued {
        /// The message.
        msg_id: MsgId,
        /// The node.
        node: NodeId,
    },
    /// A delivery left the send queue to wait in the delivery queue for
    /// its node's next beat (spec "Delivery queue").
    DeliveryWaits {
        /// The message.
        msg_id: MsgId,
        /// The node.
        node: NodeId,
        /// Its hand-over started and every attempt went unanswered;
        /// otherwise the node had stopped listening before it started.
        tried: bool,
    },
}
