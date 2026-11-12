//! The firmware's main loop around a protocol core.
//!
//! [`Driver`] feeds a [`Core`] what its [`Radio`] hears and its [`Clock`]
//! says, and carries out the actions the core emits, as
//! `docs/core-interface.md` requires of a driver. The radio, the clock and
//! the application are traits, so the loop that runs on the board also runs
//! on the host against a fake radio in a test.
//!
//! One step at a time: wait for the radio, the core's wakeup or the
//! application; hand the core the event; perform the one excursion it asked
//! for and hand it the completion. The radio keeps receiving meanwhile.

#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]
// One single-threaded executor polls every future, so none needs `Send`.
#![allow(async_fn_in_trait)]

#[cfg(test)]
extern crate std;

pub mod gnss;
pub mod record;
pub mod sx1262;
pub mod tcxo;
pub mod utc;

use core::fmt::Display;
use core::future::pending;

use bark_proto::radio::{time_on_air, Channel, CHANNELS, MIN_PREAMBLE_SYMBOLS, RSSI_SETTLE};
use bark_proto::sniff::LONG_PREAMBLE_SYMBOLS;
use bark_proto::time::earlier;
use bark_proto::update::{Image, Piece};
use bark_proto::wire::{Packet, MAX_PAYLOAD};
use bark_proto::{
    Action, AppCommand, AppEvent, Core, Duration, Event, Frame, Instant, MsgId, NodeId, Position,
    Stored, Trace, MAX_FRAME,
};
use embassy_futures::select::{select3, Either3};

/// Airtime of the longest frame with the long preamble: the longest a
/// check may be held for a reception in progress (core-interface §3 rule 8).
pub const MAX_AIRTIME: Duration = time_on_air(MAX_FRAME as u8, LONG_PREAMBLE_SYMBOLS);

/// Wait between retuning and reading RSSI (spec "Busy estimate"); the
/// datasheet gives no settle time (SX1261/2 DS §13.5.4), the bench found one.
pub const RSSI_DWELL: Duration = RSSI_SETTLE;

/// RSSI reported for a failed reading: the top of the scale (GetRssiInst
/// gives −raw/2 dBm, raw ≥ 0; DS §13.5.4), so the core takes the channel as
/// busy rather than transmit on a reading it never got.
pub const FAILED_RSSI_DBM: i16 = 0;

/// Every channel, as a mask of bits by index.
const ALL_CHANNELS: u16 = (1 << CHANNELS) - 1;

/// How often the battery is read on battery (core-interface §4). A minute
/// late at the most a board draws (36 mA, a relay with GPS tracking) is
/// 0.6 mAh, under 0.1 % of an 850 mAh cell and below a reading's noise.
pub const BATTERY_EVERY: Duration = Duration::from_secs(60);

/// How often an empty readiness is logged: it can recur at every event, so
/// once a minute shows the fault without filling the log.
pub const EMPTY_READY_LOG_EVERY: Duration = Duration::from_secs(60);

/// What the radio found after [`Radio::wait`] returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Heard {
    /// A frame whose CRC passed; its bytes are in the buffer given to
    /// [`Radio::service`].
    Frame {
        /// Number of bytes.
        len: u8,
        /// Packet RSSI, dBm.
        rssi_dbm: i16,
        /// Packet SNR, dB.
        snr_db: i8,
    },
    /// A packet has begun (preamble or header seen) and is still arriving.
    Started,
    /// A packet ended without a frame.
    Failed(Failure),
    /// Nothing that concerns the core.
    Nothing,
}

/// Why a packet the radio began gave no frame. Counted apart: noise fails
/// at the header, a neighbouring channel's packet often fails the payload
/// (`docs/crc-failures-2026-11-10.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The header's CRC failed.
    Header,
    /// The header passed and the payload's CRC failed.
    Crc,
    /// The receive timed out.
    Timeout,
}

/// The node a bench build's radio is deaf to, from `BARK_IGNORE` (twelve
/// hex digits) at build time: boards on one desk all hear each other, so
/// two deaf to each other make a line through a third
/// (`docs/bringup-2026-11-06.md`). `None` in every other build.
pub const BENCH_DEAF_TO: Option<NodeId> = match option_env!("BARK_IGNORE") {
    None => None,
    Some(hex) => Some(node_id_of_hex(hex)),
};

/// A node ID from its twelve hex digits; any other text fails the build
/// when it is a constant's.
pub const fn node_id_of_hex(hex: &str) -> NodeId {
    let b = hex.as_bytes();
    assert!(b.len() == 2 * NodeId::LEN, "a node ID is twelve hex digits");
    let mut id = [0u8; NodeId::LEN];
    let mut i = 0;
    while i < b.len() {
        let digit = match b[i] {
            b'0'..=b'9' => b[i] - b'0',
            b'a'..=b'f' => b[i] - b'a' + 10,
            b'A'..=b'F' => b[i] - b'A' + 10,
            _ => panic!("a node ID is twelve hex digits"),
        };
        id[i / 2] = id[i / 2] << 4 | digit;
        i += 1;
    }
    NodeId(id)
}

/// The half-duplex LoRa radio at bark's modulation (spec "Physical layer").
pub trait Radio {
    /// A failure of the radio or its bus. `Display`, not `Debug`, for the
    /// log: the derived form of an error enum costs flash.
    type Error: Display;

    /// A node whose frames the driver drops; only a bench build has one
    /// ([`BENCH_DEAF_TO`]). They still keep the radio busy and show in its
    /// listen-before-talk.
    const DEAF_TO: Option<NodeId> = None;

    /// Listen on `channel` from whatever state the radio is in.
    async fn listen(&mut self, channel: Channel) -> Result<(), Self::Error>;

    /// Receive duty-cycled on `channel` from whatever state the radio is
    /// in (core-interface §3 rule 10). It ends with the first packet that
    /// ends; the driver then starts it again.
    async fn sniff(&mut self, channel: Channel) -> Result<(), Self::Error>;

    /// While listening, wait until the radio has something to report.
    /// Cancel-safe: it only waits.
    async fn wait(&mut self) -> Result<(), Self::Error>;

    /// Find out what [`Radio::wait`] announced, reading a frame's bytes into
    /// `buf`. The radio keeps listening. Short, and not cancel-safe.
    async fn service(&mut self, buf: &mut [u8; MAX_FRAME]) -> Result<Heard, Self::Error>;

    /// Listen before talking on `channel`: one CAD, with the RSSI read
    /// `radio::RSSI_SETTLE` into it. Returns whether a preamble was seen
    /// and the reading in dBm. Leaves the radio not listening.
    async fn lbt(&mut self, channel: Channel) -> Result<(bool, i16), Self::Error>;

    /// Instantaneous RSSI on the channel being listened to, dBm.
    async fn rssi(&mut self) -> Result<i16, Self::Error>;

    /// Send `frame` on its channel at `dbm` conducted. Returns once it has
    /// left the antenna. Leaves the radio not listening.
    async fn transmit(&mut self, frame: &Frame, dbm: i8) -> Result<(), Self::Error>;

    /// Bring the radio back as at boot before its next operation, as what
    /// it reads is no longer trusted ([`BusyRun`]). Leaves it not listening.
    fn recover(&mut self) {}
}

/// A monotonic clock in the core's units.
pub trait Clock {
    /// Now. Never decreases.
    fn now(&self) -> Instant;

    /// Resolves once `now() >= t`, possibly later. Cancel-safe.
    async fn at(&self, t: Instant);
}

/// A message the application wants to send.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outgoing {
    /// The destination's position (spec "Addresses").
    pub dst_pos: Position,
    /// The destination node.
    pub dst_id: NodeId,
    /// Payload bytes copied into the buffer given to [`App::take`].
    pub len: usize,
}

/// What the application asks of the node besides a message to send
/// (core-interface §4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    /// It has the inbox's oldest message, this one: the node may forget
    /// it ([`Core::confirm`]).
    Confirm(MsgId),
    /// Set the node's position to this map point
    /// (`AppCommand::SetPosition`).
    SetPosition(Position),
    /// Clear the map point (`AppCommand::ClearMapPoint`).
    ClearMapPoint,
    /// A GPS fix that passed the receiver's checks; handed to the core as
    /// `AppCommand::Fix` if it passes [`Core::fix_in_reach`] too.
    Fix(Position),
    /// The GPS runs and its fixes stopped passing (`AppCommand::NoFix`);
    /// sent at the change.
    NoFix,
    /// The image the node holds whole and checked (`AppCommand::Holds`).
    Holds(Image),
    /// Gather this image from block `from` on (`AppCommand::Gather`): the
    /// application's answer to [`App::offered`], or at boot for one
    /// gathered in part.
    Gather {
        /// The image.
        image: Image,
        /// The first block it lacks.
        from: u8,
    },
    /// A block of the image gathered decoded, and matched its hash or not
    /// (`AppCommand::Block`).
    Block {
        /// The block.
        index: u8,
        /// It matched.
        ok: bool,
    },
    /// Ask this neighbour for its status (`AppCommand::Ask`); its answer
    /// comes back to [`App::message`] as `AppEvent::Status`.
    Ask(NodeId),
    /// A person at the node forces the other role
    /// (`AppCommand::ForceRole`).
    ForceRole,
    /// Stop taking messages so the node drains before a reboot into a
    /// staged image, or take them again (`AppCommand::Drain`).
    Drain(bool),
}

/// What the GPS is to do (core-interface §4): rest in standby, search
/// until an instant for one fix, or run all the time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gps {
    /// In standby.
    Rest,
    /// Running until this instant, or until the driver says otherwise.
    Search(Instant),
    /// Running.
    Track,
}

/// What [`App::ready`] resolved for (core-interface §4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ready {
    /// Something for the driver: a request, a change of power or a
    /// message.
    Driver,
    /// Nothing for the driver: the step ends so the outer loop can serve
    /// the application from the core, such as a neighbour table page.
    Own,
}

/// The application side of the node (core-interface §4).
pub trait App {
    /// Resolves when something new arrived since it last resolved: a
    /// [`Request`], a change of power or, if `offer`, a message
    /// ([`Ready::Driver`], only when the driver will get something); or work
    /// for the outer loop ([`Ready::Own`]). `offer` is whether the core
    /// accepts a message now. Cancel-safe (core-interface §4).
    async fn ready(&mut self, offer: bool) -> Ready;

    /// The next request waiting, if any. The driver takes every one each
    /// time [`App::ready`] resolves, before it offers a message.
    fn request(&mut self) -> Option<Request>;

    /// Hand over the waiting message: its payload into `payload`, the rest
    /// returned. Called only while the core accepts a message.
    fn take(&mut self, payload: &mut [u8; MAX_PAYLOAD]) -> Option<Outgoing>;

    /// What the core tells the application (core-interface §4).
    fn message(&mut self, event: &AppEvent);

    /// Whether USB power is present now, if the node can tell
    /// (core-interface §4).
    fn power(&mut self) -> Option<bool>;

    /// The battery's uncalibrated voltage in millivolts, or `None` if the
    /// board cannot read it. Asked only between radio operations, never
    /// while the radio transmits.
    async fn battery(&mut self) -> Option<u16>;

    /// Keep `stored` across a reboot, for `Config::stored` at the next
    /// boot (core-interface §4).
    fn store(&mut self, stored: &Stored);

    /// Run the GPS as `gps` says until told otherwise; fixes come back as
    /// [`Request::Fix`].
    fn gps(&mut self, gps: Gps);

    /// A neighbour holds `image`, newer than what the node runs or
    /// gathers (`Action::Offered`): if the application's policy takes it,
    /// it answers [`Request::Gather`].
    fn offered(&mut self, image: &Image);

    /// Keep this piece of the image gathered (`Action::StorePiece`); a
    /// block it completes comes back as [`Request::Block`].
    fn store_piece(&mut self, piece: &Piece);

    /// Fill in the bytes of the piece `frame`'s header names, from
    /// `wire::PIECE_HEADER_LEN` on (`Action::Serve`); whether it did.
    async fn serve(&mut self, frame: &mut Frame) -> bool;
}

/// Counters for the log, and the timings that say whether the loop keeps
/// up with the radio (`docs/firmware-adapter-2026-10-15.md`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Events handed to the core.
    pub events: u32,
    /// Frames received with a good CRC.
    pub frames: u32,
    /// Packets that began and gave no frame: the sum of the three below.
    pub failed: u32,
    /// Of `failed`, those whose header CRC failed.
    pub failed_header: u32,
    /// Of `failed`, those whose header passed and payload CRC failed.
    pub failed_crc: u32,
    /// Of `failed`, those that timed out.
    pub failed_timeout: u32,
    /// Checks held for a reception in progress.
    pub held: u32,
    /// Radio operations that failed.
    pub radio_errors: u32,
    /// Times duty-cycled receive was started.
    pub sniffs: u32,
    /// Longest single `handle` call.
    pub longest_handle: Duration,
    /// Longest stretch away from the radio while it kept listening: a
    /// frame that ends in it waits in the radio.
    pub longest_away: Duration,
    /// Times [`App::ready`] resolved for the driver and the application
    /// gave nothing, against its contract.
    pub empty_readies: u32,
    /// Times [`App::ready`] resolved for the application's own turn
    /// ([`Ready::Own`]).
    pub own_readies: u32,
    /// Pieces the application could not fill, not sent ([`App::serve`]).
    pub unserved: u32,
    /// Times the radio was brought back as at boot because every channel
    /// read busy ([`BusyRun`]).
    pub recoveries: u32,
}

/// The listen-before-talk checks since the last one the core found clear.
///
/// A heartbeat skipped on all 13 channels with no clear check between is
/// 39 busy checks in a row (three per channel, spec "Heartbeat"). At the
/// 35 % airtime a channel carries (spec "Capacity and sizing") that is
/// 0.35^39, about 10^-18, by chance: the radio reads wrong, so it is reset
/// (`docs/radio-recovery-2026-11-10.md`). A sleepy node beats on its home
/// channel alone and never fills the mask.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BusyRun {
    /// The channels a heartbeat was skipped on, as a mask by index.
    pub skipped: u16,
    /// The checks made.
    pub checks: u32,
    /// Of `checks`, those whose CAD saw a preamble.
    pub cad: u32,
    /// The lowest and highest reading, dBm.
    pub dbm: Option<(i16, i16)>,
}

impl BusyRun {
    fn check(&mut self, cad: bool, dbm: i16) {
        self.checks += 1;
        self.cad += u32::from(cad);
        self.dbm = Some(match self.dbm {
            Some((lo, hi)) => (lo.min(dbm), hi.max(dbm)),
            None => (dbm, dbm),
        });
    }
}

/// The loop: one core, one radio, one clock, one application.
pub struct Driver<C, R, K, A> {
    core: C,
    radio: R,
    clock: K,
    app: A,
    tx_dbm: i8,
    booted: bool,
    /// The power last reported to the core.
    power: Option<bool>,
    /// When the battery is read next: on battery only.
    battery_at: Option<Instant>,
    /// Until when the search the core asked for runs (`Action::Locate`).
    search_until: Option<Instant>,
    /// What the application was last told of the GPS.
    gps: Option<Gps>,
    /// The last fix was dropped as out of reach: the next is logged only
    /// once one is taken again.
    fix_dropped: bool,
    /// [`App::ready`] last gave nothing: not waited on until the clock or
    /// the radio wakes the loop.
    app_empty: bool,
    /// When an empty readiness was last logged.
    empty_logged: Option<Instant>,
    /// Where the core wants the radio to listen.
    resting: Option<Channel>,
    /// Until when the core lets the radio receive duty-cycled
    /// (`Action::Sniff`).
    sniff_until: Option<Instant>,
    /// Where the radio listens; `None` during an excursion and after a
    /// failure, until the next step puts it back.
    listening: Option<Channel>,
    /// The radio receives duty-cycled on `listening`.
    sniffing: bool,
    /// When a packet began on `listening` that has not yet ended.
    receiving: Option<Instant>,
    /// The excursion the core asked for and is waiting on.
    excursion: Option<Action>,
    /// When the outstanding check began waiting for a reception to end.
    held_since: Option<Instant>,
    /// The busy checks since the last clear one.
    busy: BusyRun,
    rx: [u8; MAX_FRAME],
    payload: [u8; MAX_PAYLOAD],
    stats: Stats,
}

impl<C: Core, R: Radio, K: Clock, A: App> Driver<C, R, K, A> {
    /// A driver for `core`, transmitting at `tx_dbm` conducted
    /// (`Config::tx_power_dbm`).
    pub fn new(core: C, radio: R, clock: K, app: A, tx_dbm: i8) -> Self {
        Driver {
            core,
            radio,
            clock,
            app,
            tx_dbm,
            booted: false,
            power: None,
            battery_at: None,
            search_until: None,
            gps: None,
            fix_dropped: false,
            app_empty: false,
            empty_logged: None,
            resting: None,
            sniff_until: None,
            listening: None,
            sniffing: false,
            receiving: None,
            excursion: None,
            held_since: None,
            busy: BusyRun::default(),
            rx: [0; MAX_FRAME],
            payload: [0; MAX_PAYLOAD],
            stats: Stats::default(),
        }
    }

    /// The core.
    pub fn core(&self) -> &C {
        &self.core
    }

    /// The radio.
    pub fn radio(&self) -> &R {
        &self.radio
    }

    /// The application.
    pub fn app(&self) -> &A {
        &self.app
    }

    /// The application, to change.
    pub fn app_mut(&mut self) -> &mut A {
        &mut self.app
    }

    /// Counters and timings so far.
    pub fn stats(&self) -> Stats {
        self.stats
    }

    /// Wait for the next thing that happens and hand it to the core. An
    /// error is the radio's; the radio is put back to listening on the
    /// next step.
    pub async fn step(&mut self) -> Result<(), R::Error> {
        // The first event a core receives is a wakeup (core-interface §2).
        if !self.booted {
            self.booted = true;
            self.deliver(Event::Wakeup).await?;
            return self.report_power().await.map(drop);
        }
        self.listen_resting().await?;
        let now = self.clock.now();
        if let Some(action) = self.excursion {
            if !self.holds(&action, now) {
                self.excursion = None;
                self.held_since = None;
                let done = self.perform(action).await;
                return self.deliver(done).await;
            }
        }
        // No excursion is under way, so nothing transmits while the
        // converter samples.
        if self.battery_at.is_some_and(|t| now >= t) {
            return self.read_battery(now).await;
        }
        // A message that waits goes now, whatever woke the loop, as
        // `ready` resolves only for what is new.
        let offer = self.core.accepts_send(now);
        if offer && self.hand_over().await? {
            return Ok(());
        }

        let wake = self.core.next_wakeup();
        let hold_end = self.held_since.map(|t| t + MAX_AIRTIME);
        let deadline = earlier(
            earlier(earlier(wake, hold_end), self.radio_due()),
            self.battery_at,
        );
        let clock = &self.clock;
        let timer = async {
            match deadline {
                Some(t) => clock.at(t).await,
                None => pending().await,
            }
        };
        // An application whose readiness gave nothing may resolve at once
        // again and starve the executor, so it waits for another wake.
        let app_empty = self.app_empty;
        let app = &mut self.app;
        let ready = async {
            match app_empty {
                true => pending().await,
                false => app.ready(offer).await,
            }
        };
        let woke = select3(timer, self.radio.wait(), ready).await;
        self.app_empty = false;

        let start = self.clock.now();
        let listening = self.listening;
        let result = match woke {
            // Late is allowed, early is not (core-interface §2).
            Either3::First(()) => match wake {
                Some(w) if start >= w => self.deliver(Event::Wakeup).await,
                _ => self.listen_resting().await,
            },
            Either3::Second(Ok(())) => self.service().await,
            Either3::Second(Err(e)) => Err(self.failed(e)),
            Either3::Third(Ready::Driver) => self.serve_app(start).await,
            // Not a fault: the application gives the driver nothing by
            // design, and what woke it is served once the step returns.
            Either3::Third(Ready::Own) => {
                self.stats.own_readies += 1;
                Ok(())
            }
        };
        if listening.is_some() && self.listening == listening {
            let away = self.clock.now().since(start);
            self.stats.longest_away = self.stats.longest_away.max(away);
        }
        result
    }

    /// Whether `action` must wait for the packet being received: a check
    /// that takes the radio off its channel would cut it short (spec "Busy estimate"),
    /// for at most one longest frame (core-interface §3 rule 8).
    fn holds(&mut self, action: &Action, now: Instant) -> bool {
        let leaves = match action {
            Action::Lbt(_) => true,
            // A duty-cycled radio reads no RSSI: it must listen first.
            Action::Rssi(c) => self.listening != Some(*c) || self.sniffing,
            _ => false,
        };
        if !leaves || self.receiving.is_none() {
            return false;
        }
        let stats = &mut self.stats;
        let since = *self.held_since.get_or_insert_with(|| {
            stats.held += 1;
            now
        });
        now < since + MAX_AIRTIME
    }

    async fn service(&mut self) -> Result<(), R::Error> {
        let heard = match self.radio.service(&mut self.rx).await {
            Ok(h) => h,
            Err(e) => return Err(self.failed(e)),
        };
        if heard == Heard::Started {
            let now = self.clock.now();
            self.receiving.get_or_insert(now);
            return Ok(());
        }
        self.receiving = None;
        let listening = self.listening;
        if self.sniffing {
            // A packet, or a failed one, ends duty-cycled receive, and
            // after anything else the radio may have been woken out of
            // it; the next `listen_resting` starts it again.
            self.listening = None;
            self.sniffing = false;
        }
        match heard {
            Heard::Started | Heard::Nothing => {}
            Heard::Failed(why) => {
                self.stats.failed += 1;
                *match why {
                    Failure::Header => &mut self.stats.failed_header,
                    Failure::Crc => &mut self.stats.failed_crc,
                    Failure::Timeout => &mut self.stats.failed_timeout,
                } += 1;
            }
            Heard::Frame {
                len,
                rssi_dbm,
                snr_db,
            } => {
                if let Some(id) = R::DEAF_TO {
                    let bytes = &self.rx[..len as usize];
                    if Packet::decode(bytes).is_ok_and(|p| p.sender() == Some(id)) {
                        return Ok(());
                    }
                }
                self.stats.frames += 1;
                if let Some(channel) = listening {
                    // A copy, so the core can borrow it while the driver
                    // is borrowed mutably.
                    let bytes = self.rx;
                    let event = Event::Received {
                        channel,
                        rssi_dbm,
                        snr_db,
                        bytes: &bytes[..len as usize],
                    };
                    return self.deliver(event).await;
                }
            }
        }
        Ok(())
    }

    /// Take what the application is ready with: the power, its requests,
    /// then its message. If it gave nothing, note it (`app_empty`).
    async fn serve_app(&mut self, now: Instant) -> Result<(), R::Error> {
        // `|`, not `||`: each is taken whatever the ones before gave.
        let gave = self.report_power().await? | self.requests().await? | self.offer().await?;
        if !gave {
            self.app_empty = true;
            self.stats.empty_readies += 1;
            if self
                .empty_logged
                .is_none_or(|t| now >= t + EMPTY_READY_LOG_EVERY)
            {
                self.empty_logged = Some(now);
                log::warn!("app said ready but offered nothing");
            }
        }
        Ok(())
    }

    /// Report the power to the core if the application can tell it and it
    /// is not what the core was last told; whether it was reported.
    async fn report_power(&mut self) -> Result<bool, R::Error> {
        let power = self.app.power();
        if power.is_none() || power == self.power {
            return Ok(false);
        }
        self.power = power;
        let external = power.unwrap_or_default();
        // On battery the first reading comes at the next step.
        self.battery_at = (!external).then(|| self.clock.now());
        self.deliver(Event::App(AppCommand::Power { external }))
            .await?;
        Ok(true)
    }

    /// Read the battery and report the reading to the core.
    async fn read_battery(&mut self, now: Instant) -> Result<(), R::Error> {
        self.battery_at = Some(now + BATTERY_EVERY);
        match self.app.battery().await {
            Some(millivolts) => {
                self.deliver(Event::App(AppCommand::Battery { millivolts }))
                    .await
            }
            None => Ok(()),
        }
    }

    /// Carry out the application's requests; whether there were any.
    async fn requests(&mut self) -> Result<bool, R::Error> {
        let mut any = false;
        while let Some(request) = self.app.request() {
            any = true;
            match request {
                Request::Confirm(msg_id) => {
                    self.core.confirm(msg_id);
                }
                Request::SetPosition(position) => {
                    self.deliver(Event::App(AppCommand::SetPosition { position }))
                        .await?
                }
                Request::ClearMapPoint => {
                    self.deliver(Event::App(AppCommand::ClearMapPoint)).await?
                }
                Request::Fix(position) => self.fix(position).await?,
                Request::NoFix => self.deliver(Event::App(AppCommand::NoFix)).await?,
                Request::Holds(image) => self.deliver(Event::App(AppCommand::Holds(image))).await?,
                Request::Gather { image, from } => {
                    self.deliver(Event::App(AppCommand::Gather { image, from }))
                        .await?
                }
                Request::Block { index, ok } => {
                    self.deliver(Event::App(AppCommand::Block { index, ok }))
                        .await?
                }
                Request::Ask(neighbour) => {
                    self.deliver(Event::App(AppCommand::Ask { neighbour }))
                        .await?
                }
                Request::ForceRole => self.deliver(Event::App(AppCommand::ForceRole)).await?,
                Request::Drain(on) => self.deliver(Event::App(AppCommand::Drain(on))).await?,
            }
        }
        Ok(any)
    }

    /// Hand the core a fix if it is in reach of the relays the core knows
    /// (spec "Position"); the first such fix ends a search
    /// (core-interface §4).
    async fn fix(&mut self, position: Position) -> Result<(), R::Error> {
        let in_reach = self.core.fix_in_reach(position);
        if !in_reach && !self.fix_dropped {
            log::warn!("gps: fix {} beyond every known relay; dropped", position);
        }
        let was = self.fix_dropped;
        self.fix_dropped = !in_reach;
        if !in_reach {
            // A dropped fix is no fix (spec "Position").
            if !was {
                self.deliver(Event::App(AppCommand::NoFix)).await?;
            }
            return Ok(());
        }
        self.search_until = None;
        self.deliver(Event::App(AppCommand::Fix { position })).await
    }

    /// Tell the application what the GPS is to do now, if that changed.
    fn follow_gps(&mut self, now: Instant) {
        let search = self.search_until.filter(|&t| t > now);
        let gps = match (self.core.gps_tracks(), search) {
            (true, _) => Gps::Track,
            (false, Some(t)) => Gps::Search(t),
            (false, None) => Gps::Rest,
        };
        if self.gps != Some(gps) {
            self.gps = Some(gps);
            self.app.gps(gps);
        }
    }

    /// Hand the core the application's message if it accepts one; whether
    /// one was handed.
    async fn offer(&mut self) -> Result<bool, R::Error> {
        if !self.core.accepts_send(self.clock.now()) {
            return Ok(false);
        }
        self.hand_over().await
    }

    /// Hand the core the application's message, if it has one; whether
    /// it had. Only while the core accepts one.
    async fn hand_over(&mut self) -> Result<bool, R::Error> {
        let Some(out) = self.app.take(&mut self.payload) else {
            return Ok(false);
        };
        let payload = self.payload;
        let send = AppCommand::Send {
            dst_pos: out.dst_pos,
            dst_id: out.dst_id,
            payload: &payload[..out.len.min(MAX_PAYLOAD)],
        };
        self.deliver(Event::App(send)).await?;
        Ok(true)
    }

    /// Hand `event` to the core, keep what it asks for, and follow a change
    /// of resting channel at once.
    async fn deliver(&mut self, event: Event<'_>) -> Result<(), R::Error> {
        let now = self.clock.now();
        let lbt = matches!(event, Event::LbtDone { .. });
        let mut resting = self.resting;
        let mut excursion = self.excursion;
        let mut sniff = self.sniff_until;
        let mut search = self.search_until;
        let mut skipped = 0u16;
        let app = &mut self.app;
        self.core.handle(event, now, &mut |action| match action {
            Action::Listen(c) => {
                resting = Some(c);
                sniff = None;
            }
            Action::Lbt(_) | Action::Rssi(_) | Action::Transmit(_) | Action::Serve(_) => {
                assert!(
                    excursion.is_none(),
                    "second excursion outstanding (docs/core-interface.md §3 rule 1)"
                );
                excursion = Some(action);
                sniff = None;
            }
            Action::Sniff(until) => sniff = Some(until).filter(|&t| t > now),
            Action::App(e) => app.message(&e),
            Action::Store(s) => app.store(&s),
            Action::Locate(until) => search = Some(until).filter(|&t| t > now),
            Action::Trace(t) => {
                if let Trace::HeartbeatSkipped { channel } = t {
                    skipped |= 1 << channel.index();
                }
                log::info!("{:?}", t)
            }
            Action::Offered(image) => app.offered(&image),
            Action::StorePiece(piece) => app.store_piece(&piece),
        });
        let took = self.clock.now().since(now);
        self.stats.events += 1;
        self.stats.longest_handle = self.stats.longest_handle.max(took);
        self.resting = resting;
        self.excursion = excursion;
        self.sniff_until = sniff;
        self.search_until = search;
        // A check the core found clear is followed by what it checked for.
        if lbt && matches!(excursion, Some(Action::Transmit(_) | Action::Serve(_))) {
            self.busy = BusyRun::default();
        }
        self.busy.skipped |= skipped;
        if self.busy.skipped == ALL_CHANNELS {
            self.recover();
        }
        self.follow_gps(now);
        self.listen_resting().await
    }

    /// Every channel read busy ([`BusyRun`]): bring the radio back as at
    /// boot.
    fn recover(&mut self) {
        let run = core::mem::take(&mut self.busy);
        let (lo, hi) = run.dbm.unwrap_or_default();
        log::warn!(
            "radio: a heartbeat skipped on every channel, {} checks busy in a row, {} with a preamble seen, readings {} to {} dBm; resetting the radio",
            run.checks,
            run.cad,
            lo,
            hi
        );
        self.stats.recoveries += 1;
        self.radio.recover();
        self.deaf();
    }

    /// Carry out an excursion and return its completion event. A radio
    /// failure still completes it (core-interface §3 rule 2), with an answer
    /// that keeps the core from transmitting into an unchecked channel.
    async fn perform(&mut self, action: Action) -> Event<'static> {
        let done = match action {
            Action::Lbt(channel) => {
                self.deaf();
                let (busy, dbm) = match self.radio.lbt(channel).await {
                    Ok(result) => result,
                    Err(e) => {
                        self.failed(e);
                        (true, FAILED_RSSI_DBM)
                    }
                };
                self.busy.check(busy, dbm);
                Event::LbtDone { channel, busy, dbm }
            }
            Action::Rssi(channel) => {
                if self.listening != Some(channel) || self.sniffing {
                    self.deaf();
                    match self.radio.listen(channel).await {
                        Ok(()) => {
                            self.listening = Some(channel);
                            self.clock.at(self.clock.now() + RSSI_DWELL).await;
                        }
                        Err(e) => {
                            self.failed(e);
                        }
                    }
                }
                let dbm = match self.listening == Some(channel) {
                    true => match self.radio.rssi().await {
                        Ok(dbm) => dbm,
                        Err(e) => {
                            self.failed(e);
                            FAILED_RSSI_DBM
                        }
                    },
                    false => FAILED_RSSI_DBM,
                };
                Event::RssiSample { channel, dbm }
            }
            Action::Transmit(frame) => {
                assert!(
                    frame.preamble_symbols >= MIN_PREAMBLE_SYMBOLS,
                    "{}-symbol preamble (docs/core-interface.md §3 rule 4)",
                    frame.preamble_symbols
                );
                self.deaf();
                if let Err(e) = self.radio.transmit(&frame, self.tx_dbm).await {
                    self.failed(e);
                }
                Event::TxDone
            }
            // A piece the application could not fill is not sent: the
            // core counts it gone, as a piece lost on the air.
            Action::Serve(mut frame) => {
                if self.app.serve(&mut frame).await {
                    self.deaf();
                    if let Err(e) = self.radio.transmit(&frame, self.tx_dbm).await {
                        self.failed(e);
                    }
                } else {
                    self.stats.unserved += 1;
                }
                Event::TxDone
            }
            Action::Listen(_)
            | Action::Sniff(_)
            | Action::App(_)
            | Action::Store(_)
            | Action::Locate(_)
            | Action::Trace(_)
            | Action::Offered(_)
            | Action::StorePiece(_) => {
                unreachable!("only excursions are kept")
            }
        };
        // Back to the resting channel before the core hears of it; a
        // failure here is retried by the next step.
        let _ = self.listen_resting().await;
        done
    }

    /// Put the radio where the core wants it: on the resting channel,
    /// duty-cycled until the sniff's end, else listening. Not while an
    /// excursion waits: it would only delay it, and the excursion ends here.
    async fn listen_resting(&mut self) -> Result<(), R::Error> {
        let now = self.clock.now();
        if self.sniff_until.is_some_and(|t| t <= now) {
            self.sniff_until = None;
        }
        let Some(c) = self.resting.filter(|_| self.excursion.is_none()) else {
            return Ok(());
        };
        let sniff = self.sniff_until.is_some();
        if self.listening == Some(c) {
            // A switch waits for a packet under way, as a check does
            // (core-interface §3 rule 8), for at most one longest frame. One
            // that outlasts that while sniffing may have stuck the radio, so
            // the cycle is started again.
            let arriving = self.receiving.is_some_and(|t| now < t + MAX_AIRTIME);
            let stuck = self.sniffing && self.receiving.is_some() && !arriving;
            if (self.sniffing == sniff || arriving) && !stuck {
                return Ok(());
            }
        }
        let result = match sniff {
            true => self.radio.sniff(c).await,
            false => self.radio.listen(c).await,
        };
        match result {
            Ok(()) => {
                self.listening = Some(c);
                self.sniffing = sniff;
                self.receiving = None;
                self.stats.sniffs += u32::from(sniff);
                Ok(())
            }
            Err(e) => Err(self.failed(e)),
        }
    }

    /// When the radio's setting next changes without the core: the end of
    /// the sniff, or of the longest packet that holds it back.
    fn radio_due(&self) -> Option<Instant> {
        let settles = self.resting.is_some() && self.excursion.is_none();
        if !settles || (!self.sniffing && self.sniff_until.is_none()) {
            return None;
        }
        match self.receiving {
            Some(t) => Some(t + MAX_AIRTIME),
            None => self.sniff_until,
        }
    }

    /// The radio has left its channel.
    fn deaf(&mut self) {
        self.listening = None;
        self.sniffing = false;
        self.receiving = None;
    }

    /// Note a radio failure; the radio's state is unknown, so it is set
    /// listening again from scratch.
    fn failed(&mut self, e: R::Error) -> R::Error {
        log::warn!("radio: {}", e);
        self.stats.radio_errors += 1;
        self.deaf();
        e
    }
}
