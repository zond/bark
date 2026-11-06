//! The firmware's main loop around a protocol core.
//!
//! [`Driver`] feeds a [`Core`] what its [`Radio`] hears and its [`Clock`]
//! says, and carries out the actions the core emits, as
//! `docs/core-interface.md` requires of a driver. The radio, the clock and
//! the application are traits, so the loop that runs on the board also runs
//! on the host against a fake radio in a test.
//!
//! The driver does one thing at a time: wait for the radio, the core's
//! wakeup, the application or a change of power; hand the core the event;
//! then perform the one excursion the core may have asked for and hand it
//! the completion. While the core runs, the radio keeps receiving on its
//! own.

#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]
// One single-threaded executor polls every future here, so none needs to
// be `Send`, which is what the lint is about.
#![allow(async_fn_in_trait)]

#[cfg(test)]
extern crate std;

pub mod gnss;
pub mod sx1262;
pub mod tcxo;
pub mod utc;

use core::fmt::Debug;
use core::future::pending;

use bark_proto::radio::{time_on_air, Channel, MIN_PREAMBLE_SYMBOLS, RSSI_SETTLE};
use bark_proto::sniff::LONG_PREAMBLE_SYMBOLS;
use bark_proto::time::earlier;
use bark_proto::wire::{Packet, MAX_PAYLOAD};
use bark_proto::{
    Action, AppCommand, AppEvent, Core, Duration, Event, Frame, Instant, MsgId, NodeId, Position,
    Stored, MAX_FRAME,
};
use embassy_futures::select::{select3, Either3};

/// Airtime of the longest frame with the long preamble: the longest a
/// check may be held for a reception in progress (core-interface §3 rule 8).
pub const MAX_AIRTIME: Duration = time_on_air(MAX_FRAME as u8, LONG_PREAMBLE_SYMBOLS);

/// Wait between retuning to another channel and reading its RSSI (spec
/// "Busy estimate"). The datasheet gives no settle time (SX1261/2 DS
/// §13.5.4); the bench found one.
pub const RSSI_DWELL: Duration = RSSI_SETTLE;

/// RSSI reported for a reading the radio failed to take, in a sweep or a
/// listen-before-talk: the top of its scale (GetRssiInst gives −raw/2 dBm,
/// raw ≥ 0; DS §13.5.4), so the core takes the channel as busy rather than
/// transmit on a reading it never got.
pub const FAILED_RSSI_DBM: i16 = 0;

/// How often the driver reads the battery while on battery and reports it
/// to the core (core-interface §4). The core turns a fixed relay sleepy at
/// a low reading, and a minute late is 0.6 mAh at the most a board draws,
/// a relay with its GPS tracking at 36 mA: under a tenth of a percent of
/// an 850 mAh cell, a fraction of a millivolt on its curve and below a
/// reading's noise. Each reading wakes the processor once and samples the
/// converter for microseconds, so it costs nothing measurable either.
pub const BATTERY_EVERY: Duration = Duration::from_secs(60);

/// How often the driver logs that the application said it was ready and
/// gave nothing. Once a minute shows a fault on the bench without filling
/// the log, as the fault can recur at every timer or radio event.
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
    /// A packet ended without a frame: its header or payload CRC failed.
    Failed,
    /// Nothing that concerns the core.
    Nothing,
}

/// The node a bench build's radio is deaf to, from `BARK_IGNORE` at build
/// time: twelve hex digits, as the banner and the USB serial number give a
/// node ID. Boards on one desk all hear each other, and two of them deaf to
/// each other make a line with a third between them
/// (`docs/bringup-2026-11-06.md`). `None`, hearing every node, in every
/// other build.
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
    /// A failure of the radio or its bus.
    type Error: Debug;

    /// A node whose frames the driver drops as if this radio never heard
    /// them; only a bench build has one ([`BENCH_DEAF_TO`]). The radio
    /// still receives them, so they still keep it busy and its
    /// listen-before-talk still sees them.
    const DEAF_TO: Option<NodeId> = None;

    /// Listen on `channel` from whatever state the radio is in.
    async fn listen(&mut self, channel: Channel) -> Result<(), Self::Error>;

    /// Receive duty-cycled on `channel` from whatever state the radio is
    /// in: windows of `sniff::SNIFF_RX` every `sniff::SNIFF_CYCLE`
    /// (core-interface §3 rule 10). It ends with the first packet that
    /// ends, with a frame or without; the driver then starts it again.
    async fn sniff(&mut self, channel: Channel) -> Result<(), Self::Error>;

    /// While listening, wait until the radio has something to report.
    /// Cancel-safe: it only waits.
    async fn wait(&mut self) -> Result<(), Self::Error>;

    /// Find out what [`Radio::wait`] announced, reading a frame's bytes into
    /// `buf`. The radio keeps listening. Short, and not cancel-safe.
    async fn service(&mut self, buf: &mut [u8; MAX_FRAME]) -> Result<Heard, Self::Error>;

    /// Listen before talking on `channel`: one channel activity detection
    /// with the RSSI read while it samples, four symbol periods in
    /// (`radio::RSSI_SETTLE`). Returns whether a LoRa preamble was seen and
    /// the reading in dBm, on the footing of a reading taken while
    /// listening. Leaves the radio not listening.
    async fn lbt(&mut self, channel: Channel) -> Result<(bool, i16), Self::Error>;

    /// Instantaneous RSSI on the channel being listened to, dBm.
    async fn rssi(&mut self) -> Result<i16, Self::Error>;

    /// Send `frame` on its channel at `dbm` conducted. Returns once it has
    /// left the antenna. Leaves the radio not listening.
    async fn transmit(&mut self, frame: &Frame, dbm: i8) -> Result<(), Self::Error>;
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
    /// A GPS fix that passed the receiver's own checks, the satellites'
    /// C/N0 among them; the driver hands it to the core as
    /// `AppCommand::Fix` if it passes the core's
    /// ([`Core::fix_in_reach`]) too.
    Fix(Position),
    /// The GPS runs and gave no fix that passed the receiver's checks,
    /// since the last fix: the driver tells the core
    /// (`AppCommand::NoFix`). Sent at the change from fixes to none.
    NoFix,
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

/// The application side of the node (core-interface §4).
pub trait App {
    /// Resolves when something new arrived for the driver since it last
    /// resolved: a [`Request`], a change of power since [`App::power`]
    /// last answered, or, if `offer`, a message. It resolves only when the
    /// driver will get something, never for what already waited when it
    /// began. `offer` is whether the core accepts a message now; one that
    /// arrives while it does not, the application holds or refuses.
    /// Cancel-safe (core-interface §4).
    async fn ready(&mut self, offer: bool);

    /// The next request waiting, if any. The driver takes every one each
    /// time [`App::ready`] resolves, before it offers a message.
    fn request(&mut self) -> Option<Request>;

    /// Hand over the waiting message: its payload into `payload`, the rest
    /// returned. The driver calls this before every wait and each time
    /// [`App::ready`] resolves, but only while the core accepts a message,
    /// and the core then takes it.
    fn take(&mut self, payload: &mut [u8; MAX_PAYLOAD]) -> Option<Outgoing>;

    /// What the core tells the application: a message for it, or its own
    /// message given back with when to offer it again (core-interface §4).
    fn message(&mut self, event: &AppEvent);

    /// Whether external power, USB, is present now, if the node can tell:
    /// the driver reports it to the core right after the boot wakeup and
    /// at every change (core-interface §4).
    fn power(&mut self) -> Option<bool>;

    /// The battery's voltage in millivolts as the board's converter reads
    /// it, uncalibrated, or `None` if the board cannot read it. The driver
    /// asks only on battery, once every [`BATTERY_EVERY`], and only between
    /// radio operations, never while the radio transmits.
    async fn battery(&mut self) -> Option<u16>;

    /// Keep `stored` across a reboot, in place of what was kept before,
    /// to hand back in `Config::stored` at the next boot (core-interface
    /// §4).
    fn store(&mut self, stored: &Stored);

    /// Run the GPS as `gps` says, from now until told otherwise. Called
    /// at each change; the receiver's fixes come back as
    /// [`Request::Fix`].
    fn gps(&mut self, gps: Gps);
}

/// Counters for the log, and the timings that say whether the loop keeps
/// up with the radio (`docs/firmware-adapter-2026-10-15.md`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Events handed to the core.
    pub events: u32,
    /// Frames received with a good CRC.
    pub frames: u32,
    /// Packets that began and failed their CRC.
    pub failed: u32,
    /// Checks held for a reception in progress.
    pub held: u32,
    /// Radio operations that failed.
    pub radio_errors: u32,
    /// Times duty-cycled receive was started: at each `Sniff` and again
    /// after each packet it caught.
    pub sniffs: u32,
    /// Longest single `handle` call.
    pub longest_handle: Duration,
    /// Longest stretch the loop spent away from the radio while the radio
    /// kept listening: a frame that ends in it waits in the radio.
    pub longest_away: Duration,
    /// Times [`App::ready`] resolved and the application gave nothing,
    /// against its contract.
    pub empty_readies: u32,
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
    /// [`App::ready`] last resolved and the application gave nothing,
    /// against its contract: it is not waited on until the clock or the
    /// radio wakes the loop.
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
        // Here no excursion is under way: the radio listens or receives
        // duty-cycled, and transmits nothing while the converter samples.
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
        // again and keep the loop, and the executor's other tasks, from
        // ever waiting. So it is not asked again until something else
        // wakes the loop, and until then it is as if it were idle.
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
            Either3::Third(()) => self.serve_app(start).await,
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
            // Duty-cycled receive ends with a packet (DS §13.1.7), and
            // after anything else the radio may have been woken out of
            // it; the next `listen_resting` starts it again.
            self.listening = None;
            self.sniffing = false;
        }
        match heard {
            Heard::Started | Heard::Nothing => {}
            Heard::Failed => self.stats.failed += 1,
            Heard::Frame {
                len,
                rssi_dbm,
                snr_db,
            } => {
                if let Some(id) = R::DEAF_TO {
                    let bytes = &self.rx[..len as usize];
                    if Packet::decode(bytes).is_ok_and(|p| p.sender() == id) {
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
        let mut resting = self.resting;
        let mut excursion = self.excursion;
        let mut sniff = self.sniff_until;
        let mut search = self.search_until;
        let app = &mut self.app;
        self.core.handle(event, now, &mut |action| match action {
            Action::Listen(c) => {
                resting = Some(c);
                sniff = None;
            }
            Action::Lbt(_) | Action::Rssi(_) | Action::Transmit(_) => {
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
            Action::Trace(t) => log::info!("{:?}", t),
        });
        let took = self.clock.now().since(now);
        self.stats.events += 1;
        self.stats.longest_handle = self.stats.longest_handle.max(took);
        self.resting = resting;
        self.excursion = excursion;
        self.sniff_until = sniff;
        self.search_until = search;
        self.follow_gps(now);
        self.listen_resting().await
    }

    /// Carry out an excursion and return its completion event. A radio
    /// failure still completes it, with the answer that keeps the core from
    /// transmitting into a channel it could not check (core-interface §3
    /// rule 2: every excursion completes with exactly one event).
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
            Action::Listen(_)
            | Action::Sniff(_)
            | Action::App(_)
            | Action::Store(_)
            | Action::Locate(_)
            | Action::Trace(_) => {
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
            // A packet under way is heard either way: the switch waits for
            // it to end, as a check is held for it (core-interface §3 rule
            // 8), and for no longer than one can last. One that outlasts
            // that in a window may have left the radio stuck there, and the
            // cycle is started again.
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

    /// Note a radio failure. The radio's state is unknown afterwards, so
    /// it is set listening again from scratch.
    fn failed(&mut self, e: R::Error) -> R::Error {
        log::warn!("radio: {:?}", e);
        self.stats.radio_errors += 1;
        self.deaf();
        e
    }
}
