//! The firmware's main loop around a protocol core.
//!
//! [`Driver`] feeds a [`Core`] what its [`Radio`] hears and its [`Clock`]
//! says, and carries out the actions the core emits, as
//! `docs/core-interface.md` requires of a driver. The radio, the clock and
//! the application are traits, so the loop that runs on the board also runs
//! on the host against a fake radio in a test.
//!
//! The driver does one thing at a time: wait for the radio, the core's
//! wakeup or the application; hand the core the event; then perform the one
//! excursion the core may have asked for and hand it the completion. While
//! the core runs, the radio keeps receiving on its own.

#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]
// One single-threaded executor polls every future here, so none needs to
// be `Send`, which is what the lint is about.
#![allow(async_fn_in_trait)]

use core::fmt::Debug;
use core::future::pending;

use bark_proto::radio::{time_on_air, Channel, MIN_PREAMBLE_SYMBOLS};
use bark_proto::wire::MAX_PAYLOAD;
use bark_proto::{
    Action, AppCommand, AppEvent, Core, Duration, Event, Frame, Instant, NodeId, Position,
    MAX_FRAME,
};
use embassy_futures::select::{select3, Either3};

/// Airtime of the longest frame with the shortest preamble: the longest a
/// check may be held for a reception in progress (core-interface §3 rule 8).
pub const MAX_AIRTIME: Duration = time_on_air(MAX_FRAME as u8, MIN_PREAMBLE_SYMBOLS);

/// Wait between retuning to another channel and reading its RSSI
/// (spec "Busy estimate": read about 0.3 ms after the retune). The datasheet gives no
/// settle time (SX1261/2 DS §13.5.4).
pub const RSSI_DWELL: Duration = Duration::from_micros(300);

/// RSSI reported for a reading the radio failed to take: the top of its
/// scale (GetRssiInst gives −raw/2 dBm, raw ≥ 0; DS §13.5.4), so the core
/// takes the channel as busy rather than transmit on a reading it never got.
pub const FAILED_RSSI_DBM: i16 = 0;

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

/// The half-duplex LoRa radio at bark's modulation (spec "Physical layer").
pub trait Radio {
    /// A failure of the radio or its bus.
    type Error: Debug;

    /// Listen on `channel` from whatever state the radio is in.
    async fn listen(&mut self, channel: Channel) -> Result<(), Self::Error>;

    /// While listening, wait until the radio has something to report.
    /// Cancel-safe: it only waits.
    async fn wait(&mut self) -> Result<(), Self::Error>;

    /// Find out what [`Radio::wait`] announced, reading a frame's bytes into
    /// `buf`. The radio keeps listening. Short, and not cancel-safe.
    async fn service(&mut self, buf: &mut [u8; MAX_FRAME]) -> Result<Heard, Self::Error>;

    /// Channel activity detection on `channel`: whether a LoRa preamble was
    /// seen. Leaves the radio not listening.
    async fn cad(&mut self, channel: Channel) -> Result<bool, Self::Error>;

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

/// The application side of the node (core-interface §4).
pub trait App {
    /// Resolves when a message waits to be sent, at once if one already
    /// does. Cancel-safe.
    async fn ready(&mut self);

    /// Hand over the waiting message: its payload into `payload`, the rest
    /// returned. The driver calls this only while the core accepts a
    /// message, and the core then takes it.
    fn take(&mut self, payload: &mut [u8; MAX_PAYLOAD]) -> Option<Outgoing>;

    /// A message for this node's application.
    fn message(&mut self, event: &AppEvent);
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
    /// Longest single `handle` call.
    pub longest_handle: Duration,
    /// Longest stretch the loop spent away from the radio while the radio
    /// kept listening: a frame that ends in it waits in the radio.
    pub longest_away: Duration,
}

/// The loop: one core, one radio, one clock, one application.
pub struct Driver<C, R, K, A> {
    core: C,
    radio: R,
    clock: K,
    app: A,
    tx_dbm: i8,
    booted: bool,
    /// Where the core wants the radio to listen.
    resting: Option<Channel>,
    /// Where the radio listens; `None` during an excursion and after a
    /// failure, until the next step puts it back.
    listening: Option<Channel>,
    /// A packet has begun on `listening` and not yet ended.
    receiving: bool,
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
            resting: None,
            listening: None,
            receiving: false,
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
            return self.deliver(Event::Wakeup).await;
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

        let wake = self.core.next_wakeup();
        let hold_end = self.held_since.map(|t| t + MAX_AIRTIME);
        let deadline = match (wake, hold_end) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let offer = self.core.accepts_send(now);
        let clock = &self.clock;
        let timer = async {
            match deadline {
                Some(t) => clock.at(t).await,
                None => pending().await,
            }
        };
        let app = &mut self.app;
        let ready = async {
            match offer {
                true => app.ready().await,
                false => pending().await,
            }
        };
        let woke = select3(timer, self.radio.wait(), ready).await;

        let start = self.clock.now();
        let listening = self.listening;
        let result = match woke {
            // Late is allowed, early is not (core-interface §2).
            Either3::First(()) => match wake {
                Some(w) if start >= w => self.deliver(Event::Wakeup).await,
                _ => Ok(()),
            },
            Either3::Second(Ok(())) => self.service().await,
            Either3::Second(Err(e)) => Err(self.failed(e)),
            Either3::Third(()) => self.offer().await,
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
            Action::Cad(_) => true,
            Action::Rssi(c) => self.listening != Some(*c),
            _ => false,
        };
        if !leaves || !self.receiving {
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
        match heard {
            Heard::Started => self.receiving = true,
            Heard::Nothing => {}
            Heard::Failed => {
                self.receiving = false;
                self.stats.failed += 1;
            }
            Heard::Frame {
                len,
                rssi_dbm,
                snr_db,
            } => {
                self.receiving = false;
                self.stats.frames += 1;
                if let Some(channel) = self.listening {
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

    async fn offer(&mut self) -> Result<(), R::Error> {
        if !self.core.accepts_send(self.clock.now()) {
            return Ok(());
        }
        let Some(out) = self.app.take(&mut self.payload) else {
            return Ok(());
        };
        let payload = self.payload;
        let send = AppCommand::Send {
            dst_pos: out.dst_pos,
            dst_id: out.dst_id,
            payload: &payload[..out.len.min(MAX_PAYLOAD)],
        };
        self.deliver(Event::App(send)).await
    }

    /// Hand `event` to the core, keep what it asks for, and follow a change
    /// of resting channel at once.
    async fn deliver(&mut self, event: Event<'_>) -> Result<(), R::Error> {
        let now = self.clock.now();
        let mut resting = self.resting;
        let mut excursion = self.excursion;
        let app = &mut self.app;
        self.core.handle(event, now, &mut |action| match action {
            Action::Listen(c) => resting = Some(c),
            Action::Cad(_) | Action::Rssi(_) | Action::Transmit(_) => {
                assert!(
                    excursion.is_none(),
                    "second excursion outstanding (docs/core-interface.md §3 rule 1)"
                );
                excursion = Some(action);
            }
            Action::App(e) => app.message(&e),
            Action::Trace(t) => log::info!("{:?}", t),
            // The radio keeps listening: the contract allows it, and the
            // firmware does not sleep the radio yet
            // (spec "Implementation notes", bring-up step 5).
            Action::Sleep(_) => {}
        });
        let took = self.clock.now().since(now);
        self.stats.events += 1;
        self.stats.longest_handle = self.stats.longest_handle.max(took);
        self.resting = resting;
        self.excursion = excursion;
        self.listen_resting().await
    }

    /// Carry out an excursion and return its completion event. A radio
    /// failure still completes it, with the answer that keeps the core from
    /// transmitting into a channel it could not check (core-interface §3
    /// rule 2: every excursion completes with exactly one event).
    async fn perform(&mut self, action: Action) -> Event<'static> {
        let done = match action {
            Action::Cad(channel) => {
                self.listening = None;
                self.receiving = false;
                let busy = match self.radio.cad(channel).await {
                    Ok(busy) => busy,
                    Err(e) => {
                        self.failed(e);
                        true
                    }
                };
                Event::CadDone { channel, busy }
            }
            Action::Rssi(channel) => {
                if self.listening != Some(channel) {
                    self.listening = None;
                    self.receiving = false;
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
                self.listening = None;
                self.receiving = false;
                if let Err(e) = self.radio.transmit(&frame, self.tx_dbm).await {
                    self.failed(e);
                }
                Event::TxDone
            }
            Action::Listen(_) | Action::Sleep(_) | Action::App(_) | Action::Trace(_) => {
                unreachable!("only excursions are kept")
            }
        };
        // Back to the resting channel before the core hears of it; a
        // failure here is retried by the next step.
        let _ = self.listen_resting().await;
        done
    }

    async fn listen_resting(&mut self) -> Result<(), R::Error> {
        match self.resting {
            Some(c) if self.listening != Some(c) => match self.radio.listen(c).await {
                Ok(()) => {
                    self.listening = Some(c);
                    self.receiving = false;
                    Ok(())
                }
                Err(e) => Err(self.failed(e)),
            },
            _ => Ok(()),
        }
    }

    /// Note a radio failure. The radio's state is unknown afterwards, so
    /// it is set listening again from scratch.
    fn failed(&mut self, e: R::Error) -> R::Error {
        log::warn!("radio: {:?}", e);
        self.stats.radio_errors += 1;
        self.listening = None;
        self.receiving = false;
        e
    }
}
