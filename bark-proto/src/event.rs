//! What the driver tells the core and what the core asks the driver to do.
//!
//! The contract between the two is in `docs/core-interface.md`. In short: the
//! driver keeps the radio listening on the channel of the last
//! [`Action::Listen`]; CAD, RSSI and transmit are excursions that the driver
//! performs and then reports with exactly one completion event, after which
//! the radio is listening again; the core has at most one radio excursion
//! outstanding at a time.

use crate::ids::{LeafId, MsgId};
use crate::position::Position;
use crate::radio::Channel;
use crate::time::Instant;

/// Largest LoRa payload the SX126x carries, and so the largest frame the
/// core will ever emit or accept.
pub const MAX_FRAME: usize = 255;

/// A frame to transmit: the LoRa payload bytes, their count, the channel and
/// the preamble length. Fixed-size so that no action allocates.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Frame {
    /// Channel to transmit on.
    pub channel: Channel,
    /// Preamble symbols; never below `radio::MIN_PREAMBLE_SYMBOLS`. Long
    /// preambles target duty-cycled leaves (design §10.5).
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
    /// Originate a message to `(dst_pos, dst_leaf)` with an opaque payload.
    /// Its ID is [`MsgId::of`] the three, reported in `AppEvent::Accepted`.
    Send {
        /// Position of the destination's relay (design §9.1).
        dst_pos: Position,
        /// Destination leaf, or `LeafId::RELAY` for the relay itself.
        dst_leaf: LeafId,
        /// Opaque payload; the core never looks inside.
        payload: &'a [u8],
    },
    /// Set or update own position (surveyed from the app, or an accepted GPS fix).
    SetPosition {
        /// New position.
        position: Position,
        /// `true` if surveyed or user-entered, `false` if GPS-derived (heartbeat type 0x1 vs 0x2).
        surveyed: bool,
    },
}

/// Why a message failed at this node. Every reason is local: the network
/// sends no failure notices (roadmap D10).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FailReason {
    /// A leaf with no relay to hand the message to.
    NoRelay,
    /// The first hop never acknowledged it, after every attempt.
    NoAck,
    /// The duty-cycle budget could not fit it (design §9.4).
    Budget,
    /// This relay originated it and has no neighbour left to try (design §9.2.5).
    NoRoute,
}

/// Events for the application side. `Message` carries its payload by value
/// for the same reason `Action::Transmit` does.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppEvent {
    /// A `Send` was queued; here is its id.
    Accepted(MsgId),
    /// The first hop acknowledged the message. Nothing more is reported
    /// about it: the protocol has no end-to-end acknowledgement (design §9.1).
    Sent(MsgId),
    /// The message failed here, before or at the first hop.
    Failed {
        /// Message that failed.
        msg_id: MsgId,
        /// Why.
        reason: FailReason,
    },
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
    /// (design §6.2) or moved by the move rule (§6.3).
    HomeChannelMoved {
        /// Previous home.
        from: Channel,
        /// New home.
        to: Channel,
    },
    /// The boot listen heard this relay's own position from another relay
    /// and stepped about a metre away from it (design §8.1).
    PositionShifted {
        /// The new position.
        to: Position,
    },
    /// A heartbeat found its channel busy at every listen-before-talk check
    /// and was skipped (design §7).
    HeartbeatSkipped {
        /// The channel it was for.
        channel: Channel,
    },
    /// The duty-cycle budget held a heartbeat back (design §9.4).
    HeartbeatDeferred {
        /// When the budget may next allow it.
        until: Instant,
    },
}
