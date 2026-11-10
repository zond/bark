//! What the driver tells the core and what the core asks the driver to do.
//!
//! The contract between the two is in `docs/core-interface.md`. In short: the
//! driver keeps the radio listening on the channel of the last
//! [`Action::Listen`]; a listen-before-talk, an RSSI reading and a transmit
//! are excursions that the driver performs and then reports with exactly
//! one completion event, after which the radio is listening again; the core
//! has at most one radio excursion outstanding at a time.

use crate::ids::{MsgId, NodeId};
use crate::position::Position;
use crate::radio::Channel;
use crate::status::Status;
use crate::store::Stored;
use crate::time::{Duration, Instant};
use crate::update::{Image, Piece};

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
    /// Completion of [`Action::Lbt`].
    LbtDone {
        /// Channel that was checked.
        channel: Channel,
        /// Whether the channel activity detection saw a LoRa preamble.
        busy: bool,
        /// The instantaneous RSSI in dBm read while it sampled, on the
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
    /// Set own position from a map point set in the app, which wins over
    /// GPS (spec "Position"): the node takes it and is fixed.
    SetPosition {
        /// New position.
        position: Position,
    },
    /// Clear the map point (spec "Position"): the node stays where it is
    /// until its next accepted fix, which is its place, and the driver
    /// hands fixes again.
    ClearMapPoint,
    /// A GPS fix that passed the driver's plausibility checks (spec
    /// "Position"). A node counts it toward turning carried or fixed
    /// again; a carried node, or a sleepy one without an anchor, takes it
    /// as its position, a fixed relay keeps its place, and a sleepy node
    /// with an anchor takes none: its position is its anchor's, and it
    /// asks for no fix (roadmap D91).
    Fix {
        /// Where the fix puts the node.
        position: Position,
    },
    /// The GPS runs but gave no fix the driver hands: the receiver lost
    /// its fix, or the driver dropped one. Sent at the change from fixes
    /// to none. Time until the next fix does not count towards a run of
    /// far fixes (spec "Position").
    NoFix,
    /// Whether external power, USB, is present: at boot and at every
    /// change (spec "Nodes", roadmap D71). It sets the node's role: on
    /// external power a relay, on battery sleepy.
    Power {
        /// External power is present.
        external: bool,
    },
    /// A person at the node holds the board's button: the node flips
    /// its role, a relay turning sleepy, leaving as at its low battery,
    /// and a sleepy node turning relay, and is forced until the next
    /// change of external power, which returns it to the power's rules.
    /// A forced relay on battery still turns sleepy at its low battery
    /// level, which ends the force, and a flip to relay is not taken on
    /// battery at that level (spec "Nodes", roadmap D160). A flip, not a
    /// target, since the button is one input and the core knows the role.
    ForceRole,
    /// The battery's voltage as the driver reads it, uncalibrated, while
    /// the radio does not transmit, on battery, once a minute (spec
    /// "Sleepy nodes", core-interface §4). The core calibrates it and
    /// compares it with [`crate::BatteryLevels`].
    Battery {
        /// Millivolts.
        millivolts: u16,
    },
    /// The image this node holds whole and has checked, the one it runs
    /// or one staged after its signature passed: at boot and after each
    /// change (spec "Updates over the mesh"). The node tells it in its
    /// heartbeats' section and serves its pieces, and wants only images
    /// above its version.
    Holds(Image),
    /// Gather `image`, from block `from` on, the blocks before it already
    /// held: the driver's policy takes it, as an answer to
    /// [`Action::Offered`] or at boot for one gathered in part before
    /// (spec "Updates over the mesh").
    Gather {
        /// The image.
        image: Image,
        /// The first block missing.
        from: u8,
    },
    /// A person looks at the node's neighbours: ask this one for its
    /// status (spec "Neighbour status"). The node asks a live neighbour once, and
    /// hands its answer, if one comes, as [`AppEvent::Status`]; a node it
    /// does not know it does not ask.
    Ask {
        /// The neighbour.
        neighbour: NodeId,
    },
    /// Take no message from now, `true`, so that what the node holds
    /// drains before it reboots into an image it has staged; or take them
    /// again, `false` (spec "Send queue", "Updates over the mesh", the
    /// install). A message offered to it is refused as for want of room,
    /// its sender told a place in the room's line, and the application's
    /// is not taken ([`crate::Core::accepts_send`]).
    Drain(bool),
    /// The block the node gathers decoded from the pieces it was given
    /// ([`Action::StorePiece`]): its hash checked, `ok`, and the node goes
    /// on to the next; or not, and the node gathers it again.
    Block {
        /// The block.
        index: u8,
        /// It matched its hash.
        ok: bool,
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
    /// The messages queued here for its next hop would make it wait
    /// longer than a hop's budget, and one of them has come as far or
    /// farther (spec "Receiving DATA", "Send queue"). Refused.
    SameHop,
    /// The hand-over back to the parent failed (spec "Return to parent", "Hand-over").
    ReturnFailed,
    /// The parent answered the return seen: it has gone on without this
    /// copy, and a return has nowhere else to go (spec "Return to parent").
    ReturnSeen,
    /// The node it is for did not take it within the delivery TTL (spec
    /// "Delivery queue").
    QueueExpired,
    /// The delivery queue, or for this node's application the inbox, had
    /// no room for it (spec "Delivery queue").
    DeliveryFull,
    /// The payload is longer than a DATA frame holds (spec "Packet formats").
    PayloadTooLong,
    /// The application's message, refused with a wait: given back to its
    /// application, which may offer it again after that wait
    /// (spec "A sleepy node's own message", "Receiving DATA").
    Pushback,
    /// The node drains for a reboot into a new image and takes no message
    /// (spec "Send queue"). Refused.
    Draining,
    /// Received, and the ACK that takes it could not go out: not taken,
    /// since a take is final only when its ACK has gone, and the sender's
    /// retry brings it again (spec "The ACK"). Refused.
    Unanswered,
}

/// Events for the application side: a message for it, a sleepy node's
/// own message given back, the node's first rotation and a neighbour's
/// status. Nothing else is
/// reported of a message it sent (`docs/core-interface.md` §4). `Message` carries its payload by
/// value for the same reason `Action::Transmit` does.
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
    /// The application's message, refused by the node's own gate or, a
    /// sleepy node's, by its first hop, is the application's again: the
    /// node takes none before `retry_at`, and the application may offer
    /// it again then or give it up (spec "Receiving DATA",
    /// "A sleepy node's own message").
    GivenBack {
        /// Its id.
        msg_id: MsgId,
        /// The end of the wait the refusal asked for.
        retry_at: Instant,
    },
    /// The node's first rotation (spec "Channel at boot"): at boot, at
    /// each relay it learns in it, and at its end, so that the app can
    /// show how it comes up.
    Boot {
        /// The relays, nodes that are not sleepy, it knows now.
        relays: u8,
        /// When the first rotation ends; `None` once it has.
        ends: Option<Instant>,
    },
    /// The first frame from another node since boot that decoded: a
    /// heartbeat, a DATA, an ACK, a piece or a status, whatever it is for.
    /// Once a boot. An image gathered over the mesh keeps itself only
    /// once it has heard a node (spec "Updates over the mesh", the
    /// install).
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
    /// Listen before talking on `channel`: one channel activity detection
    /// with the RSSI read while it samples; report `Event::LbtDone`. The
    /// two together say whether the channel is clear (spec "Heartbeat").
    Lbt(Channel),
    /// Sample instantaneous RSSI on `channel`; report `Event::RssiSample`.
    /// The busy sweep's reading (spec "Busy estimate").
    Rssi(Channel),
    /// Transmit a frame; report `Event::TxDone`.
    Transmit(Frame),
    /// The radio may receive duty-cycled on the resting channel until this
    /// instant: in windows of [`crate::sniff::SNIFF_RX`] every
    /// [`crate::sniff::SNIFF_CYCLE`], so that it catches every frame with
    /// the long preamble ([`crate::sniff::LONG_PREAMBLE_SYMBOLS`]) and may
    /// miss any other. What it catches is `Event::Received` as ever. The
    /// core's next `Listen`, `Lbt`, `Rssi` or `Transmit` ends it sooner;
    /// once it ends the radio listens on the resting channel again
    /// (`docs/core-interface.md` §3 rule 10). Only a sleepy node asks for
    /// it, between its windows.
    Sniff(Instant),
    /// Tell the application something.
    App(AppEvent),
    /// Keep this across a reboot, in place of what was kept before, and
    /// hand it back in [`crate::Config::stored`] at the next boot
    /// (`docs/core-interface.md` §4). Emitted only when it changes.
    Store(Stored),
    /// Run the GPS until this instant and hand the first fix that passes
    /// the checks as [`AppCommand::Fix`], then stop it; a later `Locate`
    /// moves the end, and one at or before now ends the search at once.
    /// A sleepy node asks while it has no anchor, a resting relay at the
    /// radio's sign of a move (`crate::gps`, spec "Position", "Beats").
    Locate(Instant),
    /// A protocol event for logs and the simulator's metrics. Carries no
    /// obligation; a driver may ignore it (`docs/core-interface.md` §3).
    Trace(Trace),
    /// A neighbour's heartbeat named an image above the version this node
    /// runs and the one it gathers, once an image: if the driver's policy
    /// takes it, the driver answers
    /// [`AppCommand::Gather`]; else nothing
    /// (spec "Updates over the mesh").
    Offered(Image),
    /// Keep this piece of the block the node gathers, until the block
    /// decodes; then answer [`AppCommand::Block`]. Pieces of every block
    /// the node lacks, each once (spec "Updates over the mesh").
    StorePiece(Piece),
    /// Transmit this piece frame as [`Action::Transmit`] does, after
    /// writing into it, from [`crate::wire::PIECE_HEADER_LEN`] on, the
    /// bytes of the piece its header names, of the image the node holds;
    /// report `Event::TxDone` (spec "Updates over the mesh").
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
    /// What became of the piece due after this relay's slot heartbeat on
    /// its home channel, for its sleepy nodes
    /// (spec "Updates over the mesh").
    SlotPiece(SlotPiece),
    /// The home channel changed: chosen at the end of the first rotation
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
    /// A status frame found its channel busy at every listen-before-talk
    /// check and was given up (spec "Neighbour status").
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
    /// A neighbour acknowledged the message, sent on toward the destination
    /// (spec "Next hop").
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
    /// The parent acknowledged the message, returned to it because no
    /// neighbour was left to try (spec "Return to parent").
    ReturnedToParent {
        /// The message.
        msg_id: MsgId,
        /// The parent.
        to: NodeId,
        /// The transmissions the hand-over took, the last one answered
        /// (spec "Hand-over").
        attempts: u8,
    },
    /// The receiver of a hand-over answered seen: it has the message's
    /// record and no copy of it. This node counts it tried and goes on to
    /// its next candidate, or drops a return (spec "The ACK").
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
    /// "Hand-over" step 2): the hand-over waits until `until` and listens
    /// again.
    ChannelBusy {
        /// The message.
        msg_id: MsgId,
        /// The receiver.
        to: NodeId,
        /// When the hand-over listens again.
        until: Instant,
    },
    /// Every attempt of a hand-over to a neighbour went unanswered, and a
    /// failure was counted against it (spec "Hand-over" step 6).
    HandOverFailed {
        /// The message.
        msg_id: MsgId,
        /// The neighbour.
        to: NodeId,
        /// The neighbour is demoted or gone, so the message moves on: a
        /// forward chooses its next hop at once, a return is dropped.
        /// Otherwise it waits for the ban and is offered to the same
        /// neighbour again.
        moves_on: bool,
    },
    /// A neighbour reached the demotion threshold and ranks behind the
    /// others (spec "Neighbour table", "Next hop").
    Demoted {
        /// The neighbour.
        neighbour: NodeId,
    },
    /// The gate found a message's wait here for its way on, by the queue
    /// ahead of it (spec "Send queue"); for counting only.
    WayWait {
        /// The wait.
        wait: Duration,
        /// It passed the budget and the message was refused.
        refused: bool,
    },
    /// A refusal for want of a way on gave its sender a place in that
    /// neighbour's line (spec "Bans"); for counting only.
    InLine {
        /// The neighbour, the way on.
        neighbour: NodeId,
        /// The wait told.
        wait: Duration,
        /// The service time the place was spaced by.
        service: Duration,
        /// This relay could not send a full frame with the reply reserve
        /// left: it is at its cap (spec "Duty cycle cap").
        at_cap: bool,
    },
    /// A neighbour demoted since it last answered this relay answered it
    /// (spec "Neighbour table"); for counting only.
    Answered {
        /// The neighbour.
        neighbour: NodeId,
    },
    /// A DATA addressed to this node went unanswered: its reply could
    /// not be afforded, or, with `owed`, another reply was owed (spec
    /// "The ACK"); for counting only.
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
    /// A fixed node had a fix farther than the GPS allowance from its
    /// position, the first of a run: it flushes its delivery queue (spec
    /// "Position").
    FarFix,
    /// The node turned carried, or fixed again (spec "Position").
    Carried {
        /// Carried now; else fixed.
        carried: bool,
    },
    /// A sleepy node's anchor turned carried, and the node leaves it
    /// (spec "Relay choice").
    AnchorCarried,
    /// A heartbeat for a change of the node's flags or map point goes out
    /// on its home channel (spec "Heartbeat"): the frame of the
    /// `Action::Transmit` that follows.
    Changed,
    /// A relay's answer to a node that knows no relay goes out (spec
    /// "Heartbeat"): the frame of the `Action::Transmit` that follows.
    Answer,
    /// A heartbeat of the node's burst, its first rotation's heartbeats a
    /// burst gap apart, goes out (spec "Heartbeat"): the frame of the
    /// `Action::Transmit` that follows.
    Burst,
    /// A fixed relay's grace on battery ended: it holds no more mail, and
    /// flushes what it holds before it turns sleepy (spec "Delivery").
    GraceEnded,
    /// A relay on battery is carried: it holds no more mail, and flushes
    /// what it holds before it turns sleepy (spec "Sleepy nodes").
    CarriedOnBattery,
    /// A relay on battery reached its low battery level: it holds no more
    /// mail, and flushes what it holds before it turns sleepy.
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
    /// A person forced the node's role (`AppCommand::ForceRole`): sleepy,
    /// or a relay, until its power changes.
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
    /// A forward refused by a neighbour leaving the air, where it may end,
    /// waits for that neighbour in the delivery queue
    /// (spec "Delivery queue").
    Waits {
        /// The message.
        msg_id: MsgId,
        /// The neighbour.
        node: NodeId,
    },
    /// A message that waited for a neighbour leaving the air went into
    /// the send queue as a forward, that neighbour back or gone
    /// (spec "Delivery queue").
    Released {
        /// The message.
        msg_id: MsgId,
    },
    /// A message the node held for another, left when it turned sleepy,
    /// went into its send queue as a forward toward its address (spec
    /// "Sleepy nodes").
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
