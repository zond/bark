//! What the driver tells the core and what the core asks the driver to do;
//! the contract is `docs/core-interface.md`. In short: the radio listens on
//! the channel of the last [`Action::Listen`]; `Lbt`, `Rssi` and `Transmit`
//! are excursions, at most one outstanding, each reported by exactly one
//! completion event, after which the radio listens again.

use crate::ids::{MsgId, NodeId};
use crate::position::Position;
use crate::radio::Channel;
use crate::status::Status;
use crate::store::Stored;
use crate::time::{Duration, Instant};
use crate::update::{Image, Piece};

/// Largest LoRa payload the SX126x carries, and so the largest frame the
/// core emits or accepts.
pub const MAX_FRAME: usize = 255;

/// A frame to transmit. Fixed-size so that no action allocates.
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
    /// Completion of [`Action::Lbt`].
    LbtDone {
        /// Channel that was checked.
        channel: Channel,
        /// Whether the channel activity detection saw a LoRa preamble.
        busy: bool,
        /// The instantaneous RSSI in dBm read while it sampled, put on the
        /// footing of a reading taken while listening.
        dbm: i16,
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
    /// Originate a message to `(dst_pos, dst_id)`, whose ID is
    /// [`MsgId::of`] the three. Offer it only while
    /// [`crate::Core::accepts_send`] holds; nothing is reported back but
    /// [`AppEvent::GivenBack`].
    Send {
        /// The destination's position (spec "Addresses").
        dst_pos: Position,
        /// The destination node.
        dst_id: NodeId,
        /// Opaque payload; the core never looks inside.
        payload: &'a [u8],
    },
    /// A map point set in the app, which wins over GPS (spec "Position").
    SetPosition {
        /// New position.
        position: Position,
    },
    /// Clear the map point (spec "Position"); the driver hands fixes again.
    ClearMapPoint,
    /// A GPS fix that passed the driver's plausibility checks (spec
    /// "Position"); the core decides whether it takes it as its position.
    Fix {
        /// Where the fix puts the node.
        position: Position,
    },
    /// The GPS runs but gives no fix the driver hands: the receiver lost
    /// its fix, or the driver dropped one. Sent at the change from fixes
    /// to none, so the gap does not count toward a run of far fixes.
    NoFix,
    /// Whether external power, USB, is present: at boot and at every
    /// change (spec "Nodes").
    Power {
        /// External power is present.
        external: bool,
    },
    /// A person holds the board's button: the node flips its role until
    /// the next change of external power (spec "Nodes"). A flip, not a
    /// target, since the button is one input and the core knows the role.
    ForceRole,
    /// The battery's voltage, uncalibrated, read on battery once a minute
    /// while the radio does not transmit (spec "Sleepy nodes").
    Battery {
        /// Millivolts.
        millivolts: u16,
    },
    /// The image this node holds whole and has checked, the one it runs
    /// or one staged after its signature passed: at boot and after each
    /// change (spec "Updates over the mesh").
    Holds(Image),
    /// Gather `image` from block `from` on: the driver's policy takes it,
    /// in answer to [`Action::Offered`], or at boot for one gathered in
    /// part before.
    Gather {
        /// The image.
        image: Image,
        /// The first block missing.
        from: u8,
    },
    /// Ask a neighbour for its status (spec "Neighbour status"). An answer,
    /// if one comes, is [`AppEvent::Status`]; an unknown node is not asked.
    Ask {
        /// The neighbour.
        neighbour: NodeId,
    },
    /// Take no message from now, `true`, so that what the node holds
    /// drains before it reboots into a staged image; or take them again,
    /// `false` (spec "Updates over the mesh"). While draining,
    /// [`crate::Core::accepts_send`] is false.
    Drain(bool),
    /// The block the node gathers decoded from its stored pieces
    /// ([`Action::StorePiece`]); `ok` if it matched its hash, else the node
    /// gathers it again.
    Block {
        /// The block.
        index: u8,
        /// It matched its hash.
        ok: bool,
    },
}

/// Why a node dropped a message, or refused one (spec "Forwarding"), for
/// logs and metrics through [`Trace`]; the network sends no failure notices.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DropReason {
    /// The origin has no neighbour left to try (spec "Return to parent").
    NoRoute,
    /// Sending it on would need a count above 255 (spec "Hop count").
    HopCap,
    /// No place for its record (spec "Message records"). Refused; the table
    /// outnumbers the send queue's places, so this does not happen.
    NoRecord,
    /// The send queue had no place (spec "Send queue").
    SendQueueFull,
    /// The duty-cycle bucket could not fit it (spec "Duty cycle cap").
    Budget,
    /// Every neighbour that could take it is banned
    /// (spec "Receiving DATA"). Refused.
    Banned,
    /// The queue for its next hop would make it wait too long (spec
    /// "Receiving DATA", "Send queue"). Refused.
    SameHop,
    /// The hand-over back to the parent failed (spec "Return to parent").
    ReturnFailed,
    /// The parent answered the return seen (spec "Return to parent").
    ReturnSeen,
    /// The node it is for did not take it within the delivery TTL (spec
    /// "Delivery queue").
    QueueExpired,
    /// The delivery queue, or for this node's application the inbox, had
    /// no room for it (spec "Delivery queue").
    DeliveryFull,
    /// The payload is longer than a DATA frame holds (spec "Packet formats").
    PayloadTooLong,
    /// The application's message, refused with a wait and given back
    /// ([`AppEvent::GivenBack`]).
    Pushback,
    /// The node drains for a reboot into a new image (spec "Send queue").
    /// Refused.
    Draining,
    /// Received, and the ACK that takes it could not go out
    /// (spec "The ACK"). Refused.
    Unanswered,
}

/// Events for the application side. Nothing else is reported of a
/// message it sent (`docs/core-interface.md`, "Application").
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppEvent {
    /// A message arrived for this node's application, and waits in the
    /// inbox until the application confirms it ([`crate::Core::inbox`],
    /// [`crate::Core::confirm`]).
    Message {
        /// Its id.
        msg_id: MsgId,
    },
    /// The application's message was refused and is the application's
    /// again: it may offer it at `retry_at` or give it up
    /// (spec "A sleepy node's own message").
    GivenBack {
        /// Its id.
        msg_id: MsgId,
        /// The end of the wait the refusal asked for.
        retry_at: Instant,
    },
    /// The node's first rotation (spec "Channel at boot"): at boot, at
    /// each relay it learns in it, and at its end.
    Boot {
        /// The relays, nodes that are not sleepy, it knows now.
        relays: u8,
        /// When the first rotation ends; `None` once it has.
        ends: Option<Instant>,
    },
    /// The first frame of any kind from another node since boot that
    /// decoded; once a boot. An image gathered over the mesh keeps itself
    /// only once it has heard a node (spec "Updates over the mesh").
    Heard,
    /// A neighbour told its status, asked by [`AppCommand::Ask`]
    /// (spec "Neighbour status").
    Status {
        /// The neighbour.
        from: NodeId,
        /// What it says of itself.
        status: Status,
    },
}

/// How a message held for a node reached it ([`Trace::Delivered`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Delivery {
    /// In the window after its beat (spec "Delivery").
    Window,
    /// Outside its window, with the long preamble (spec "Delivery").
    Long,
    /// Once, with the long preamble, from a node that has had a far fix
    /// (spec "Delivery").
    Flush,
    /// To the node itself heard as a node that is not sleepy, with the
    /// ordinary preamble (spec "Delivery").
    Relay,
}

/// What the core asks the driver to do. `Transmit` carries a frame by
/// value, as the core allocates nothing; drivers hold one at a time.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Set the resting channel. Takes effect at once; the driver listens here
    /// whenever no excursion is in progress.
    Listen(Channel),
    /// Listen before talking on `channel`: one channel activity detection
    /// with the RSSI read while it samples; report `Event::LbtDone`.
    Lbt(Channel),
    /// Sample instantaneous RSSI on `channel`; report `Event::RssiSample`.
    Rssi(Channel),
    /// Transmit a frame; report `Event::TxDone`.
    Transmit(Frame),
    /// The radio may receive duty-cycled on the resting channel until this
    /// instant, with the periods of [`crate::sniff::periods`]. What it
    /// catches is `Event::Received` as ever. The core's next `Listen`,
    /// `Lbt`, `Rssi` or `Transmit` ends it sooner; then the radio listens
    /// on the resting channel again (`docs/core-interface.md`, "Radio",
    /// rule 10).
    Sniff(Instant),
    /// Tell the application something.
    App(AppEvent),
    /// Keep this across a reboot, in place of what was kept before, and
    /// hand it back in [`crate::Config::stored`] at the next boot. Emitted
    /// only when it changes.
    Store(Stored),
    /// Run the GPS until this instant and hand the first fix that passes
    /// the checks as [`AppCommand::Fix`], then stop it; a later `Locate`
    /// moves the end, and one at or before now ends the search at once
    /// (spec "Position", "Beats").
    Locate(Instant),
    /// A protocol event for logs and the simulator's metrics; a driver may
    /// ignore it.
    Trace(Trace),
    /// A neighbour's heartbeat named an image newer than this node runs or
    /// gathers, once an image: if the driver's policy takes it, the driver
    /// answers [`AppCommand::Gather`]; else nothing.
    Offered(Image),
    /// Keep this piece of the block the node gathers until the block
    /// decodes; then answer [`AppCommand::Block`]. Each piece comes once.
    StorePiece(Piece),
    /// Transmit this piece frame as [`Action::Transmit`] does, after
    /// writing into it, from [`crate::wire::PIECE_HEADER_LEN`] on, the
    /// bytes of the piece its header names, of the image the node holds;
    /// report `Event::TxDone`.
    Serve(Frame),
}

/// What became of the piece due after a slot heartbeat ([`Trace::SlotPiece`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotPiece {
    /// It went.
    Sent,
    /// Its channel's pace held it, shared with others heard there.
    SharedPace,
    /// Its channel's pace held it, the node's own last piece there.
    OwnPace,
    /// Its pace on its two channels held it.
    Channels,
    /// The next piece was for another channel.
    OtherChannel,
    /// The bucket could not pay it with the reply reserve left.
    Bucket,
    /// Listen-before-talk found the channel busy.
    ChannelBusy,
    /// The radio was not free in time, or a wait was under way.
    Late,
}

/// A protocol event worth recording, emitted where it happens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trace {
    /// What became of the piece due after this relay's slot heartbeat
    /// (spec "Updates over the mesh").
    SlotPiece(SlotPiece),
    /// The home channel changed (spec "Channel at boot", "Channel moves").
    HomeChannelMoved {
        /// Previous home.
        from: Channel,
        /// New home.
        to: Channel,
    },
    /// A heartbeat found its channel busy and was skipped (spec "Heartbeat").
    HeartbeatSkipped {
        /// The channel it was for.
        channel: Channel,
    },
    /// A status frame found its channel busy and was given up (spec
    /// "Neighbour status").
    StatusGivenUp {
        /// The neighbour it was for.
        to: NodeId,
        /// It was an answer; else an ask.
        answer: bool,
    },
    /// The duty-cycle budget held a heartbeat back (spec "Duty cycle cap").
    HeartbeatDeferred {
        /// When the budget may next allow it.
        until: Instant,
    },
    /// This node dropped the message (spec "Addresses").
    Dropped {
        /// The message.
        msg_id: MsgId,
        /// Why.
        reason: DropReason,
    },
    /// A DATA packet was refused for what `reason` names
    /// (spec "Receiving DATA"). Not a drop: the sender still holds it.
    Refused {
        /// The message.
        msg_id: MsgId,
        /// [`DropReason::SendQueueFull`], [`DropReason::Budget`] or
        /// [`DropReason::DeliveryFull`].
        reason: DropReason,
    },
    /// The receiver of a hand-over refused it (spec "Bans").
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
    /// A neighbour acknowledged the message (spec "Next hop").
    Forwarded {
        /// The message.
        msg_id: MsgId,
        /// The neighbour.
        to: NodeId,
        /// The count it carried (spec "Hop count").
        hops: u8,
        /// The transmissions the hand-over took, the last one answered
        /// (spec "Hand-over").
        attempts: u8,
    },
    /// The parent acknowledged a returned message (spec "Return to parent").
    ReturnedToParent {
        /// The message.
        msg_id: MsgId,
        /// The parent.
        to: NodeId,
        /// The transmissions the hand-over took, the last one answered
        /// (spec "Hand-over").
        attempts: u8,
    },
    /// The receiver of a hand-over answered seen (spec "The ACK").
    Seen {
        /// The message.
        msg_id: MsgId,
        /// The receiver.
        by: NodeId,
    },
    /// A DATA of a message this node has a record of was answered, and
    /// was not a retransmission (spec "Receiving DATA" cases 4 to 6).
    Copy {
        /// The message.
        msg_id: MsgId,
        /// From a neighbour this node handed the message to: a return.
        returned: bool,
        /// This node holds another copy, which goes on (case 4).
        merged: bool,
        /// Answered seen: this node holds none and the copy stays with
        /// its sender (case 6).
        seen: bool,
    },
    /// Listen-before-talk gave up on the receiver's channel (spec
    /// "Hand-over").
    ChannelBusy {
        /// The message.
        msg_id: MsgId,
        /// The receiver.
        to: NodeId,
        /// When the hand-over listens again.
        until: Instant,
    },
    /// Every attempt of a hand-over went unanswered (spec "Hand-over").
    HandOverFailed {
        /// The message.
        msg_id: MsgId,
        /// The neighbour.
        to: NodeId,
        /// The neighbour is demoted or gone, so the message moves on;
        /// else it waits for the ban and goes to the same neighbour again.
        moves_on: bool,
    },
    /// A neighbour was demoted (spec "Neighbour table").
    Demoted {
        /// The neighbour.
        neighbour: NodeId,
    },
    /// The gate found a message's wait here for its way on
    /// (spec "Send queue").
    WayWait {
        /// The wait.
        wait: Duration,
        /// It passed the budget and the message was refused.
        refused: bool,
    },
    /// A refusal for want of a way on gave its sender a place in that
    /// neighbour's line (spec "Bans").
    InLine {
        /// The neighbour, the way on.
        neighbour: NodeId,
        /// The wait told.
        wait: Duration,
        /// The service time the place was spaced by.
        service: Duration,
        /// This relay is at its duty-cycle cap (spec "Duty cycle cap").
        at_cap: bool,
    },
    /// A demoted neighbour answered this relay (spec "Neighbour table").
    Answered {
        /// The neighbour.
        neighbour: NodeId,
    },
    /// A DATA addressed to this node went unanswered: its reply could
    /// not be afforded, or another was owed (spec "The ACK").
    Silent {
        /// Its message.
        msg_id: MsgId,
        /// Another reply was owed.
        owed: bool,
    },
    /// The node a message was held for took it (spec "Delivery").
    Delivered {
        /// The message.
        msg_id: MsgId,
        /// The node.
        node: NodeId,
        /// How it went.
        by: Delivery,
    },
    /// A fixed node had the first of a run of far fixes (spec "Position").
    FarFix,
    /// The node turned carried, or fixed again (spec "Position").
    Carried {
        /// Carried now; else fixed.
        carried: bool,
    },
    /// A sleepy node's anchor turned carried, and the node leaves it
    /// (spec "Relay choice").
    AnchorCarried,
    /// The `Action::Transmit` that follows is a heartbeat for a change of
    /// the node's flags or map point (spec "Heartbeat").
    Changed,
    /// The `Action::Transmit` that follows is a relay's answer to a node
    /// that knows no relay (spec "Heartbeat").
    Answer,
    /// The `Action::Transmit` that follows is a heartbeat of the node's
    /// boot burst (spec "Heartbeat").
    Burst,
    /// A fixed relay's grace on battery ended; it turns sleepy (spec
    /// "Delivery").
    GraceEnded,
    /// A relay on battery is carried; it turns sleepy (spec "Sleepy nodes").
    CarriedOnBattery,
    /// A relay on battery reached its low battery level; it turns sleepy.
    BatteryLow,
    /// A sample of the battery's drain closed (`drain.rs`): what it
    /// measured and the estimate after it, in ten-thousandths of a
    /// percent of the cell an hour.
    Drain {
        /// It ran sleepy; else as a relay.
        sleepy: bool,
        /// The sample's drain.
        measured: i32,
        /// The estimate.
        rate: u32,
    },
    /// The node turned sleepy, or a relay, by its power
    /// (spec "Sleepy nodes"); not while it boots.
    Role {
        /// Sleepy now; else a relay.
        sleepy: bool,
    },
    /// A person forced the node's role (`AppCommand::ForceRole`).
    Forced {
        /// Sleepy; else a relay.
        sleepy: bool,
    },
    /// A sleepy node that sleeps listens a rotation to check its anchor
    /// (spec "Relay choice").
    AnchorCheck,
    /// A relay that rests its GPS and watches its radio closed a rotation
    /// of surprises (`node/relay/gps.rs`, spec "Beats").
    MoveSign {
        /// The heartbeats of fixed relay neighbours in it.
        surprises: u8,
        /// The sum of their squared surprises over the variance fading
        /// gives them, in thousandths.
        sum_milli: u32,
        /// Whether the sum passed its point, and a search is due.
        moved: bool,
    },
    /// A window in which a sleeping node woke for its anchor's heartbeat
    /// ended (spec "Relay choice").
    AnchorWatch {
        /// Whether the heartbeat was heard in the window.
        heard: bool,
        /// When it started against the time expected, µs, negative if
        /// before.
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
    /// A forward refused by a neighbour leaving the air waits for it in the
    /// delivery queue (spec "Delivery queue").
    Waits {
        /// The message.
        msg_id: MsgId,
        /// The neighbour.
        node: NodeId,
    },
    /// A message that waited for a neighbour leaving the air went into
    /// the send queue (spec "Delivery queue").
    Released {
        /// The message.
        msg_id: MsgId,
    },
    /// A message the node held for another went into its send queue as a
    /// forward when it turned sleepy (spec "Sleepy nodes").
    HandedOn {
        /// The message.
        msg_id: MsgId,
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
