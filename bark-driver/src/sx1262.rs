//! The SX1262 driven with only the commands that change.
//!
//! [`Chip`] is the chip one command at a time, with its pin waits; the
//! firmware implements it over SPI. [`Sx1262`] keeps what the chip holds
//! ([`Held`]) and sends an operation only what it changes. After a sleep
//! or an error the whole configuration goes again; after a reset the boot's
//! measurements are taken again too (`docs/radio-recovery-2026-11-10.md`).
//! The oscillator's delays are kept by [`crate::tcxo`], and the error word
//! is read after every start. The tests at the end run the sequences
//! against a chip that records them.
//!
//! "DS" is the SX1261/2 datasheet, Rev 1.2 (June 2019).

use core::fmt::Display;

use bark_proto::radio::{time_on_air, Channel, RSSI_SETTLE, SYMBOL_TIME};
use bark_proto::sniff::{periods, LONG_PREAMBLE_SYMBOLS};
use bark_proto::{Frame, MAX_FRAME};

use crate::tcxo::{self, Search};
use crate::{Clock, Duration, Failure, Heard, Instant};

/// Interrupt flags (DS Table 13-29), as `GetIrqStatus` reports them.
pub mod irq {
    /// Packet transmission completed.
    pub const TX_DONE: u16 = 1 << 0;
    /// Packet received.
    pub const RX_DONE: u16 = 1 << 1;
    /// Preamble detected.
    pub const PREAMBLE_DETECTED: u16 = 1 << 2;
    /// Valid LoRa header received.
    pub const HEADER_VALID: u16 = 1 << 4;
    /// LoRa header CRC error.
    pub const HEADER_ERR: u16 = 1 << 5;
    /// Wrong CRC received.
    pub const CRC_ERR: u16 = 1 << 6;
    /// Channel activity detection finished.
    pub const CAD_DONE: u16 = 1 << 7;
    /// Channel activity detected.
    pub const CAD_DETECTED: u16 = 1 << 8;
    /// Rx or Tx timeout.
    pub const TIMEOUT: u16 = 1 << 9;
    /// Every flag an operation ends with or reports a packet by, set once
    /// for every mode to save a command at each change (DS §13.3.1
    /// SetDioIrqParams).
    pub const ALL: u16 = TX_DONE
        | RX_DONE
        | PREAMBLE_DETECTED
        | HEADER_VALID
        | HEADER_ERR
        | CRC_ERR
        | CAD_DONE
        | CAD_DETECTED
        | TIMEOUT;
}

/// `GetDeviceErrors` bit 5 (DS §13.3.6, Table 13-84): the oscillator was
/// not running at the end of its programmed delay.
pub const XOSC_START_ERR: u16 = 1 << 5;

/// `GetDeviceErrors` bit 6 (DS Table 13-84): the PLL did not lock, so the
/// chip could not enter the mode it was sent to.
pub const PLL_LOCK_ERR: u16 = 1 << 6;

/// The error bits that say a start failed: the oscillator was not up, or
/// the PLL could not lock on a clock still settling. Either leaves the chip
/// in STDBY_RC (`docs/radio-recovery-2026-11-10.md`).
pub const START_ERRORS: u16 = XOSC_START_ERR | PLL_LOCK_ERR;

/// The chip's mode in the status byte every read command returns (DS
/// §13.5.1, Table 13-76), bits 6:4.
pub mod status {
    /// STDBY_RC, the oscillator stopped: where a start that failed leaves
    /// the chip (`docs/radio-recovery-2026-11-10.md`, "The error decoded").
    pub const STBY_RC: u8 = 0x2;
    /// RX.
    pub const RX: u8 = 0x5;

    /// The mode in `status`.
    pub const fn mode(status: u8) -> u8 {
        (status >> 4) & 0x7
    }
}

/// Symbols a CAD samples (DS Table 13-72, `CADSymbols::_8`).
pub const CAD_SYMBOLS: u16 = 8;

/// The CAD's sampling. The RSSI read inside it must start before this
/// ends (`docs/bench-2026-10-30.md`: at 2.0 ms the CAD had ended).
pub const CAD_SAMPLING: Duration = SYMBOL_TIME.mul(CAD_SYMBOLS as u64);

/// A CAD from its start to its interrupt: its symbols and about half a
/// symbol of processing (DS §6.1.5), rounded up to a whole one.
pub const CAD_TIME: Duration = SYMBOL_TIME.mul(CAD_SYMBOLS as u64 + 1);

/// Interrupt latency allowed beyond an operation's duration: twice the
/// longest oscillator start plus the 126 µs switch to TX (DS Table 8-2),
/// so a chip that lost its oscillator is slow rather than an error.
pub const IRQ_SLACK: Duration = Duration::from_micros(2 * (tcxo::MAX.as_micros() + 126));

/// A reading at or below this is no reading: the receiver was not up
/// (`docs/bench-2026-10-30.md`, "−128 until a dwell of about 145 µs"). The
/// scale's bottom, −raw/2 with raw 255, is −127.5.
pub const NO_READING_DBM: i16 = -127;

/// Pairs the boot's self-check takes, a reading inside a CAD against one
/// while listening: enough for a median to shrug off a pair a frame hit.
pub const SELF_CHECK_PAIRS: usize = 5;

/// The largest offset the self-check accepts, dB: twice the 7 dB the bench
/// saw under a signal (within 2 dB on a quiet channel). The datasheet
/// documents `GetRssiInst` for receive only, so a chip that fails the
/// check is read after the CAD instead.
pub const SELF_CHECK_LIMIT_DB: i16 = 12;

/// The receiver's packet parameters: the longest preamble, as DS §6.1.4
/// asks where the length varies, and the longest payload
/// (`docs/core-interface.md` §3 rule 4).
pub const RX_PACKET: (u16, u8) = (LONG_PREAMBLE_SYMBOLS, MAX_FRAME as u8);

/// The chip, one command at a time. Every method waits for BUSY low, sends
/// one command and waits for BUSY to fall again, unless its comment says
/// more: the chip takes no command while BUSY is high, which it also is
/// while it handles an interrupt of its own (DS §8.3.1).
pub trait Chip {
    /// A failure of the bus, a pin wait or an interrupt wait; `Display`
    /// for the log, as [`crate::Radio::Error`].
    type Error: Display;

    /// Pulse NRESET and wait until the chip takes commands: STDBY_RC,
    /// everything it held lost (DS §9.1).
    async fn reset(&mut self) -> Result<(), Self::Error>;

    /// Wake a chip that may be asleep in a duty-cycled receive with a
    /// harmless command on a falling NSS edge (DS §8.2.2, §13.1.7), sent
    /// without waiting for BUSY first, which a sleeping chip holds high;
    /// then wait for BUSY low. Harmless to a chip awake.
    async fn wake(&mut self) -> Result<(), Self::Error>;

    /// `SetStandby`: STDBY_XOSC with the oscillator running, or STDBY_RC
    /// with it stopped (DS §13.1.2).
    async fn standby(&mut self, xosc: bool) -> Result<(), Self::Error>;

    /// `Calibrate` every block, from STDBY_RC, and wait for it; the chip
    /// starts its oscillator for it with the programmed delay (DS
    /// §13.1.12).
    async fn calibrate(&mut self) -> Result<(), Self::Error>;

    /// `CalibrateImage` for the channel plan's band, from STDBY_RC (DS
    /// §13.1.13).
    async fn calibrate_image(&mut self) -> Result<(), Self::Error>;

    /// Everything that never changes while the chip holds it, in one go:
    /// the regulator, the RF switch on DIO2, the packet type, the sync
    /// word, the buffer base, the retention list, the fallback to
    /// STDBY_XOSC after transmit and receive, the modulation with its
    /// errata write, the IQ errata write, the interrupt mask [`irq::ALL`]
    /// on DIO1, the symbol timeout and the receiver gain. From a standby.
    async fn configure(&mut self) -> Result<(), Self::Error>;

    /// `SetCadParams` for [`CAD_SYMBOLS`] and the detection thresholds
    /// (DS §13.4.8), sent before every `SetCAD`: the chip does not hold the
    /// thresholds across a receive, and sent once the CAD found a 10 %-duty
    /// beacon 0.5 % of the time (`docs/lean-driver-2026-10-30.md`).
    async fn cad_params(&mut self) -> Result<(), Self::Error>;

    /// `SetDIO3AsTCXOCtrl` with the board's voltage and `delay`, rounded
    /// up to the chip's steps (DS §13.3.6). From STDBY_RC.
    async fn set_tcxo(&mut self, delay: Duration) -> Result<(), Self::Error>;

    /// `GetDeviceErrors` (DS §13.5.6): the error word, and the status byte
    /// that comes with it, which holds the chip's mode ([`status`]).
    async fn errors(&mut self) -> Result<(u16, u8), Self::Error>;

    /// `ClearDeviceErrors` (DS §13.5.7).
    async fn clear_errors(&mut self) -> Result<(), Self::Error>;

    /// `SetRfFrequency` to the channel's centre (DS §13.4.1).
    async fn set_frequency(&mut self, channel: Channel) -> Result<(), Self::Error>;

    /// `SetPacketParams` with this preamble and payload length, explicit
    /// header, CRC on, IQ not inverted (DS §13.4.6).
    async fn set_packet_params(&mut self, preamble: u16, len: u8) -> Result<(), Self::Error>;

    /// The PA configuration and `SetTxParams` for `dbm` conducted, with
    /// the clamp errata write (DS §13.1.14, §13.4.4).
    async fn set_tx_power(&mut self, dbm: i8) -> Result<(), Self::Error>;

    /// `WriteBuffer` from offset 0 (DS §13.2.1).
    async fn write_payload(&mut self, bytes: &[u8]) -> Result<(), Self::Error>;

    /// `SetRx` continuous (DS §13.1.5). Returns once the chip is
    /// receiving, its oscillator started if it was stopped.
    async fn rx(&mut self) -> Result<(), Self::Error>;

    /// `SetRxDutyCycle` with these periods, from STDBY_RC (DS §13.1.7).
    async fn rx_duty_cycle(&mut self, rx: Duration, sleep: Duration) -> Result<(), Self::Error>;

    /// `SetCAD` (DS §13.1.8). Returns once the chip samples.
    async fn cad(&mut self) -> Result<(), Self::Error>;

    /// `SetTx` without a timeout (DS §13.1.4). Returns once the chip
    /// transmits.
    async fn tx(&mut self) -> Result<(), Self::Error>;

    /// `GetRssiInst`, whole dBm (DS §13.5.4).
    async fn rssi(&mut self) -> Result<i16, Self::Error>;

    /// `GetIrqStatus` (DS §13.3.3).
    async fn irq_status(&mut self) -> Result<u16, Self::Error>;

    /// `ClearIrqStatus` of every flag (DS §13.3.4).
    async fn clear_irq(&mut self) -> Result<(), Self::Error>;

    /// Wait for DIO1 to be high, for at most `timeout` if given, which
    /// then fails. No command. Cancel-safe.
    async fn await_irq(&mut self, timeout: Option<Duration>) -> Result<(), Self::Error>;

    /// The received frame: its bytes into `buf`, its length, packet RSSI
    /// in dBm and SNR in dB (`GetRxBufferStatus`, `ReadBuffer`,
    /// `GetPacketStatus`, DS §13.5.2, §13.2.2, §13.5.3).
    async fn read_frame(&mut self, buf: &mut [u8; MAX_FRAME])
        -> Result<(u8, i16, i8), Self::Error>;

    /// The processor's die temperature in quarter degrees, if the board
    /// reads it: uncalibrated, only its change is used (`crate::tcxo`).
    async fn die_temperature(&mut self) -> Option<i16>;
}

/// What the sequencer can fail with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error<E> {
    /// The chip's.
    Chip(E),
    /// An operation's interrupt came without its done flag: these flags.
    NotDone(u16),
    /// A receive that did not start, cold either: the status byte after
    /// the cold start.
    NotStarted(u8),
}

/// The text `Debug` writes.
impl<E: Display> Display for Error<E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::Chip(e) => write!(f, "Chip({e})"),
            Error::NotDone(flags) => write!(f, "NotDone({flags})"),
            Error::NotStarted(status) => write!(f, "NotStarted({status})"),
        }
    }
}

impl<E> From<E> for Error<E> {
    fn from(e: E) -> Self {
        Error::Chip(e)
    }
}

/// The chip's mode as the sequencer knows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// After an error: the chip is reset before its next operation.
    Unknown,
    /// STDBY_RC, the oscillator stopped.
    StandbyRc,
    /// STDBY_XOSC, the oscillator running.
    StandbyXosc,
    /// Receiving on [`Held::freq`].
    Rx,
    /// Receiving duty-cycled on [`Held::freq`], asleep between windows.
    Sniff,
    /// Sampling a CAD; ends in STDBY_RC.
    Cad,
    /// Transmitting; ends in STDBY_XOSC.
    Tx,
}

/// What the chip holds, as far as the sequencer has sent it. `None` is
/// not known, and is sent before it is needed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Held {
    /// Its mode.
    pub mode: Mode,
    /// Whether it holds the configuration that never changes.
    pub configured: bool,
    /// Whether it has been asleep since the configuration was sent: a
    /// duty-cycled receive sleeps between its windows.
    pub slept: bool,
    /// The frequency, as a channel.
    pub freq: Option<Channel>,
    /// The packet parameters: preamble symbols and payload length.
    pub packet: Option<(u16, u8)>,
    /// The oscillator delay programmed.
    pub tcxo: Option<Duration>,
    /// The transmit power, dBm.
    pub power: Option<i8>,
    /// When the oscillator last stopped, or the earliest it can have: at
    /// a CAD's end or at a standby with it off. `None` while it runs or
    /// when not known.
    pub osc_stopped: Option<Instant>,
}

impl Held {
    const UNKNOWN: Held = Held {
        mode: Mode::Unknown,
        configured: false,
        slept: false,
        freq: None,
        packet: None,
        tcxo: None,
        power: None,
        osc_stopped: None,
    };
}

/// Where a listen-before-talk reads the channel's RSSI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LbtRead {
    /// Inside the CAD, four symbol periods in, less this offset in dB
    /// from a reading while listening, measured at boot.
    Inside {
        /// The offset.
        offset_db: i16,
    },
    /// After the CAD, with the receiver started on the channel and
    /// settled: the self-check at boot failed.
    After,
}

/// The phases of a listen-before-talk, for the console.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LbtTiming {
    /// The CAD saw a preamble.
    pub busy: bool,
    /// The reading, on the footing of one while listening.
    pub dbm: i16,
    /// The reading as read inside the CAD, if one was.
    pub inside_dbm: Option<i16>,
    /// From the call to the CAD's start.
    pub prepare: Duration,
    /// The RSSI read inside the CAD.
    pub read: Duration,
    /// From the CAD's start to its result read and cleared.
    pub cad: Duration,
    /// After the CAD, to the reading after it, if the reading is taken
    /// after.
    pub after: Duration,
}

/// The phases of a transmission, for the console.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TxTiming {
    /// From the call to the transmission's start.
    pub prepare: Duration,
    /// From the transmission's start to its done flag read and cleared.
    pub tx: Duration,
}

/// Counters for the log.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Resets after an error or asked for, the one at boot not counted.
    pub resets: u32,
    /// Times the whole configuration was sent.
    pub configured: u32,
    /// Starts from a stopped oscillator with the cold delay.
    pub cold_starts: u32,
    /// Restarts within [`tcxo::STILL_RUNNING`] of stopping, with the
    /// restart's delay.
    pub restarts: u32,
}

/// A start of the oscillator, as the driver programmed it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Start {
    /// No start: it was running.
    Running,
    /// A restart with the restart's delay.
    Restart {
        /// How long the oscillator had been off.
        gap: Duration,
        /// The delay programmed.
        delay: Duration,
    },
    /// A cold start with the cold delay.
    Cold {
        /// How long the oscillator had been off, if known.
        gap: Option<Duration>,
        /// The delay programmed.
        delay: Duration,
    },
}

/// `, gap N us` for a restart's log line, nothing for a cold start's.
struct Gap(Option<Duration>);

impl core::fmt::Display for Gap {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.0 {
            Some(g) => write!(f, ", gap {} us", g.as_micros()),
            None => Ok(()),
        }
    }
}

/// Which delay a search probes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Which {
    Cold,
    /// The restart, after a CAD and back to this channel.
    Restart(Channel),
}

/// The sequencer: a chip, a clock, and what the chip holds.
pub struct Sx1262<C, K> {
    chip: C,
    clock: K,
    held: Held,
    cold: tcxo::Delay,
    restart: tcxo::Delay,
    lbt: LbtRead,
    stats: Stats,
    /// The chip was reset since the boot's measurements: they are taken
    /// again before it next listens.
    remeasure: bool,
    /// The last start of the oscillator.
    last_start: Start,
    /// The start whose error word is not read yet, blamed for a start
    /// error found before a reset.
    unread: Start,
}

impl<C: Chip, K: Clock> Sx1262<C, K> {
    /// Bring the chip up: reset and configure it, probe its oscillator's
    /// start, check the reading inside a CAD against one while listening
    /// on `channel`, and leave it listening there.
    pub async fn boot(chip: C, clock: K, channel: Channel) -> Result<Self, Error<C::Error>> {
        let now = clock.now();
        let mut radio = Sx1262 {
            chip,
            clock,
            held: Held::UNKNOWN,
            cold: tcxo::Delay::new(tcxo::STEP, tcxo::MAX, now),
            restart: tcxo::Delay::new(tcxo::SHORT_STEP, tcxo::MAX, now),
            lbt: LbtRead::After,
            stats: Stats::default(),
            remeasure: false,
            last_start: Start::Running,
            unread: Start::Running,
        };
        radio.ready().await?;
        radio.stats.resets = 0;
        radio.listen(channel).await?;
        Ok(radio)
    }

    /// Reset the chip before its next operation and measure again what
    /// the boot measured, as after an error: for a chip that answers
    /// without errors but no longer works as it did after boot.
    pub fn recover(&mut self) {
        self.held = Held::UNKNOWN;
    }

    /// The boot's measurements, on `channel`: the oscillator's delays,
    /// the cold one and then the restart's under it, searched from
    /// scratch, and the reading inside a CAD against one while listening.
    async fn measure(&mut self, channel: Channel) -> Result<(), Error<C::Error>> {
        self.remeasure = false;
        self.set_frequency(channel).await?;
        self.search(Which::Cold, self.cold.search(tcxo::MAX))
            .await?;
        let restart = self.restart.search(self.cold.delay);
        self.search(Which::Restart(channel), restart).await?;
        self.self_check(channel).await
    }

    /// The chip.
    pub fn chip(&mut self) -> &mut C {
        &mut self.chip
    }

    /// What the chip holds.
    pub fn held(&self) -> &Held {
        &self.held
    }

    /// The oscillator's cold delay and its history.
    pub fn cold(&self) -> &tcxo::Delay {
        &self.cold
    }

    /// The oscillator's restart delay and its history. A restart takes
    /// [`Sx1262::restart_delay`].
    pub fn restart(&self) -> &tcxo::Delay {
        &self.restart
    }

    /// What a restart is programmed with: the restart's delay, and never
    /// more than a cold start's.
    pub fn restart_delay(&self) -> Duration {
        self.restart.delay.min(self.cold.delay)
    }

    /// The last start of the oscillator.
    pub fn last_start(&self) -> Start {
        self.last_start
    }

    /// Where a listen-before-talk reads the RSSI.
    pub fn lbt_read(&self) -> LbtRead {
        self.lbt
    }

    /// Counters.
    pub fn stats(&self) -> Stats {
        self.stats
    }

    /// The bytes of the last packet, whatever its CRC said, for a log of
    /// what arrived.
    pub async fn read_last_frame(
        &mut self,
        buf: &mut [u8; MAX_FRAME],
    ) -> Result<(u8, i16, i8), Error<C::Error>> {
        let r = self.chip.read_frame(buf).await;
        self.note(r)
    }

    /// Note a chip failure: what it holds is unknown, so it is reset
    /// before its next operation.
    fn note<T>(&mut self, r: Result<T, C::Error>) -> Result<T, Error<C::Error>> {
        if r.is_err() {
            self.held = Held::UNKNOWN;
        }
        r.map_err(Error::Chip)
    }

    /// Bring the chip to a standby that holds the configuration: a reset
    /// after an error, a wake after a sleep, the configuration again
    /// after either, and the error flag read after a cycle of cold starts.
    async fn ready(&mut self) -> Result<(), Error<C::Error>> {
        let mut reset = false;
        match self.held.mode {
            Mode::Unknown => {
                reset = true;
                if self.stats.configured > 0 {
                    // What failed, for the log, before the reset clears it.
                    if let Ok((errors, _)) = self.chip.errors().await {
                        log::warn!("radio: device errors {:#x} before the reset", errors);
                        // Every start's word is read after it, so a start
                        // error here belongs to a start cut off before its
                        // read, if any.
                        if errors & START_ERRORS != 0 {
                            self.start_failed(self.unread).await;
                        }
                    }
                }
                let r = self.chip.reset().await;
                self.note(r)?;
                self.stats.resets += 1;
                // Cold: the reset cut the oscillator's supply.
                self.held = Held {
                    mode: Mode::StandbyRc,
                    osc_stopped: None,
                    ..Held::UNKNOWN
                };
                self.remeasure = true;
                // With a TCXO the flag is set at power-up, before the
                // chip knows its clock (DS §13.3.6).
                let r = self.chip.clear_errors().await;
                self.note(r)?;
                // The calibrations get the longest delay, not the probed
                // one: a TCXO's frequency settles after it starts.
                self.set_tcxo(tcxo::MAX).await?;
                let r = self.chip.calibrate().await;
                self.note(r)?;
                let r = self.chip.errors().await;
                let (errors, _) = self.note(r)?;
                if errors != 0 {
                    log::warn!("radio: calibration errors {:#x}", errors);
                    let r = self.chip.clear_errors().await;
                    self.note(r)?;
                }
                self.last_start = Start::Running;
                self.unread = Start::Running;
            }
            Mode::Sniff => {
                // A wake edge that comes as the chip goes to sleep may be
                // lost, BUSY then high for the sleep: try once more.
                if self.chip.wake().await.is_err() {
                    let r = self.chip.wake().await;
                    self.note(r)?;
                }
                let r = self.chip.standby(false).await;
                self.note(r)?;
                self.held.mode = Mode::StandbyRc;
                self.held.osc_stopped = None;
            }
            _ => {}
        }
        if self.held.slept {
            // The cycle's wakes were cold starts with the probed delay.
            self.held.slept = false;
            self.held.configured = false;
            let delay = self.held.tcxo.unwrap_or(self.cold.delay);
            self.start_errors(Start::Cold { gap: None, delay }).await?;
        }
        if !self.held.configured {
            let r = self.chip.configure().await;
            self.note(r)?;
            self.stats.configured += 1;
            self.held = Held {
                configured: true,
                freq: None,
                packet: None,
                tcxo: None,
                power: None,
                ..self.held
            };
            // The image calibration survives a sleep; a reset loses it.
            if reset {
                let r = self.chip.calibrate_image().await;
                self.note(r)?;
            }
        }
        Ok(())
    }

    /// From receiving into a standby with the oscillator running.
    async fn standby_xosc(&mut self) -> Result<(), Error<C::Error>> {
        if self.held.mode == Mode::Rx {
            let r = self.chip.standby(true).await;
            self.note(r)?;
            self.held.mode = Mode::StandbyXosc;
        }
        debug_assert!(matches!(
            self.held.mode,
            Mode::StandbyRc | Mode::StandbyXosc
        ));
        Ok(())
    }

    /// Clear what the receiver flagged before it stopped: a preamble seen
    /// as the receive was left holds DIO1 high, and the CAD or transmission
    /// would read no done flag (the bench's `NotDone(4)` and `NotDone(20)`).
    /// From a standby, where no new packet can be flagged.
    async fn clear_stale_irq(&mut self) -> Result<(), Error<C::Error>> {
        let r = self.chip.clear_irq().await;
        self.note(r)
    }

    /// Into STDBY_RC, the oscillator stopped.
    async fn standby_rc(&mut self) -> Result<(), Error<C::Error>> {
        if matches!(self.held.mode, Mode::Rx | Mode::StandbyXosc) {
            // It stops no sooner than the command is sent.
            let sent = self.clock.now();
            let r = self.chip.standby(false).await;
            self.note(r)?;
            self.held.mode = Mode::StandbyRc;
            self.held.osc_stopped = Some(sent);
        }
        debug_assert_eq!(self.held.mode, Mode::StandbyRc);
        Ok(())
    }

    async fn set_frequency(&mut self, channel: Channel) -> Result<(), Error<C::Error>> {
        if self.held.freq != Some(channel) {
            let r = self.chip.set_frequency(channel).await;
            self.note(r)?;
            self.held.freq = Some(channel);
        }
        Ok(())
    }

    async fn set_packet_params(&mut self, params: (u16, u8)) -> Result<(), Error<C::Error>> {
        if self.held.packet != Some(params) {
            let r = self.chip.set_packet_params(params.0, params.1).await;
            self.note(r)?;
            self.held.packet = Some(params);
        }
        Ok(())
    }

    async fn set_tx_power(&mut self, dbm: i8) -> Result<(), Error<C::Error>> {
        if self.held.power != Some(dbm) {
            let r = self.chip.set_tx_power(dbm).await;
            self.note(r)?;
            self.held.power = Some(dbm);
        }
        Ok(())
    }

    /// Program the oscillator delay, from STDBY_RC.
    async fn set_tcxo(&mut self, delay: Duration) -> Result<(), Error<C::Error>> {
        if self.held.tcxo != Some(delay) {
            debug_assert_eq!(self.held.mode, Mode::StandbyRc);
            let r = self.chip.set_tcxo(delay).await;
            self.note(r)?;
            self.held.tcxo = Some(delay);
        }
        Ok(())
    }

    /// Before a command that starts the oscillator from STDBY_RC: program
    /// the restart's delay if it stopped less than [`tcxo::STILL_RUNNING`]
    /// ago, else the cold one. Returns the start.
    async fn before_start(&mut self) -> Result<Start, Error<C::Error>> {
        if self.held.mode != Mode::StandbyRc {
            return Ok(Start::Running);
        }
        let stopped = self.held.osc_stopped;
        let running = |now: Instant| stopped.is_some_and(|t| now.since(t) < tcxo::STILL_RUNNING);
        let mut restart = running(self.clock.now());
        let mut delay = if restart {
            self.restart_delay()
        } else {
            self.cold.delay
        };
        self.set_tcxo(delay).await?;
        // Other tasks may run while the command waits for the chip, so the
        // window may have passed by the start: a cold start then.
        if restart && !running(self.clock.now()) {
            restart = false;
            delay = self.cold.delay;
            self.set_tcxo(delay).await?;
        }
        let start = match stopped.map(|t| self.clock.now().since(t)) {
            Some(gap) if restart => Start::Restart { gap, delay },
            gap => Start::Cold { gap, delay },
        };
        self.held.osc_stopped = None;
        self.last_start = start;
        self.unread = start;
        Ok(start)
    }

    /// After a start: whether it started cleanly, and the status byte read
    /// with the error word, `None` if the oscillator was running.
    async fn after_start(&mut self, start: Start) -> Result<(bool, Option<u8>), Error<C::Error>> {
        match start {
            Start::Running => return Ok((true, None)),
            Start::Cold { .. } => self.stats.cold_starts += 1,
            Start::Restart { .. } => self.stats.restarts += 1,
        }
        let (clean, status) = self.start_errors(start).await?;
        Ok((clean, Some(status)))
    }

    /// The error word after `start`, cleared, and the start's delay
    /// stepped up if it failed. Returns whether it started cleanly, and
    /// the status byte read with the word.
    async fn start_errors(&mut self, start: Start) -> Result<(bool, u8), Error<C::Error>> {
        let r = self.chip.errors().await;
        let (errors, status) = self.note(r)?;
        self.unread = Start::Running;
        if errors != 0 {
            let r = self.chip.clear_errors().await;
            self.note(r)?;
        }
        let failed = errors & START_ERRORS != 0;
        if failed {
            self.start_failed(start).await;
        }
        Ok((!failed, status))
    }

    /// `start` failed: its delay one step up, and a log line with how long
    /// the oscillator had been off, to tell a short delay from a long stop.
    async fn start_failed(&mut self, start: Start) {
        let temp = self.chip.die_temperature().await;
        let now = self.clock.now();
        match start {
            Start::Running => {}
            Start::Cold { gap, delay } => {
                self.cold.step_up(tcxo::MAX, now, temp);
                log::warn!(
                    "tcxo: a cold start after {:?} us off failed at {} us, cold starts now take {} us",
                    gap.map(|g| g.as_micros()),
                    delay.as_micros(),
                    self.cold.delay.as_micros()
                );
            }
            Start::Restart { gap, delay } => {
                self.restart.step_up(self.cold.delay, now, temp);
                log::warn!(
                    "tcxo: a restart {} us after stopping failed at {} us, restarts now take {} us",
                    gap.as_micros(),
                    delay.as_micros(),
                    self.restart_delay().as_micros()
                );
            }
        }
    }

    /// Listen on `channel` from any state, sending only what differs
    /// from what the chip holds; after a reset, the boot's measurements
    /// first.
    pub async fn listen(&mut self, channel: Channel) -> Result<(), Error<C::Error>> {
        self.ready().await?;
        if self.remeasure {
            self.measure(channel).await?;
        }
        self.receive(channel).await
    }

    /// Listen on `channel` from any state, with what was measured.
    async fn receive(&mut self, channel: Channel) -> Result<(), Error<C::Error>> {
        self.ready().await?;
        if self.held.mode == Mode::Rx && self.held.freq == Some(channel) {
            return Ok(());
        }
        self.reprobe_if_due(channel).await?;
        if self.held.mode == Mode::Rx && self.held.freq == Some(channel) {
            return Ok(());
        }
        self.start_rx(channel).await
    }

    /// From a standby, receiving on `channel`. A failed start can leave the
    /// chip in STDBY_RC, where its next status read fails (`OpError(170)`):
    /// it is started again cold with the stepped-up delay, and reset only
    /// if that fails too.
    async fn start_rx(&mut self, channel: Channel) -> Result<(), Error<C::Error>> {
        self.standby_xosc().await?;
        self.set_frequency(channel).await?;
        self.set_packet_params(RX_PACKET).await?;
        let mut last = None;
        for _ in 0..2 {
            if last.is_some() {
                let r = self.chip.standby(false).await;
                self.note(r)?;
                self.held.mode = Mode::StandbyRc;
                self.held.osc_stopped = None;
            }
            let start = self.before_start().await?;
            let r = self.chip.rx().await;
            self.note(r)?;
            self.held.mode = Mode::Rx;
            let (clean, status) = self.after_start(start).await?;
            if clean && status.is_none_or(|s| status::mode(s) == status::RX) {
                return Ok(());
            }
            last = status;
        }
        self.held = Held::UNKNOWN;
        Err(Error::NotStarted(last.unwrap_or(0)))
    }

    /// Receive duty-cycled on `channel` from any state (DS §13.1.7). The
    /// wakes are cold starts, so a longer delay comes off the sleep and the
    /// long preamble is still caught (`bark_proto::sniff`).
    pub async fn sniff(&mut self, channel: Channel) -> Result<(), Error<C::Error>> {
        self.ready().await?;
        if self.remeasure {
            self.measure(channel).await?;
        }
        self.standby_rc().await?;
        self.set_frequency(channel).await?;
        self.set_packet_params(RX_PACKET).await?;
        // The cycle's wakes are cold starts, read together after it.
        self.set_tcxo(self.cold.delay).await?;
        let (rx, sleep) = periods(self.cold.delay);
        let r = self.chip.rx_duty_cycle(rx, sleep).await;
        self.note(r)?;
        self.held.mode = Mode::Sniff;
        self.held.slept = true;
        self.held.osc_stopped = None;
        self.last_start = Start::Cold {
            gap: None,
            delay: self.cold.delay,
        };
        self.unread = self.last_start;
        Ok(())
    }

    /// Wait for the chip to have something to report. Cancel-safe.
    pub async fn wait(&mut self) -> Result<(), Error<C::Error>> {
        let r = self.chip.await_irq(None).await;
        self.note(r)
    }

    /// What the chip reported: a frame into `buf`, a packet begun, a
    /// packet failed, or nothing. Clears the flags. A duty-cycled receive
    /// ends with its packet, the chip then in STDBY_RC.
    pub async fn service(&mut self, buf: &mut [u8; MAX_FRAME]) -> Result<Heard, Error<C::Error>> {
        let r = self.chip.irq_status().await;
        let flags = self.note(r)?;
        let r = self.chip.clear_irq().await;
        self.note(r)?;
        let failed = irq::HEADER_ERR | irq::CRC_ERR | irq::TIMEOUT;
        if self.held.mode == Mode::Sniff && flags & (irq::RX_DONE | failed) != 0 {
            // Asleep between its windows, so its next start is cold.
            self.held.mode = Mode::StandbyRc;
            self.held.osc_stopped = None;
        }
        // The payload is checked only after the header passed, so a
        // payload error says the header was good.
        if flags & irq::CRC_ERR != 0 {
            return Ok(Heard::Failed(Failure::Crc));
        }
        if flags & irq::HEADER_ERR != 0 {
            return Ok(Heard::Failed(Failure::Header));
        }
        if flags & irq::TIMEOUT != 0 {
            return Ok(Heard::Failed(Failure::Timeout));
        }
        if flags & irq::RX_DONE != 0 {
            let r = self.chip.read_frame(buf).await;
            let (len, rssi_dbm, snr_db) = self.note(r)?;
            return Ok(Heard::Frame {
                len,
                rssi_dbm,
                snr_db,
            });
        }
        if flags & (irq::PREAMBLE_DETECTED | irq::HEADER_VALID) != 0 {
            return Ok(Heard::Started);
        }
        Ok(Heard::Nothing)
    }

    /// The instantaneous RSSI of the channel being listened to.
    pub async fn rssi(&mut self) -> Result<i16, Error<C::Error>> {
        let r = self.chip.rssi().await;
        self.note(r)
    }

    /// One CAD on `channel`, with the RSSI read inside it if `read`:
    /// whether a preamble was seen, and the reading as read, `None` if
    /// there was none or the read came too late to be one. Leaves the
    /// chip in STDBY_RC.
    async fn cad(
        &mut self,
        channel: Channel,
        read: bool,
    ) -> Result<(bool, Option<i16>, [Duration; 3]), Error<C::Error>> {
        let t0 = self.clock.now();
        let receiving = matches!(self.held.mode, Mode::Rx | Mode::Sniff);
        self.ready().await?;
        self.standby_xosc().await?;
        if receiving {
            self.clear_stale_irq().await?;
        }
        self.set_frequency(channel).await?;
        let r = self.chip.cad_params().await;
        self.note(r)?;
        let start = self.before_start().await?;
        let sent = self.clock.now();
        let r = self.chip.cad().await;
        self.note(r)?;
        self.held.mode = Mode::Cad;
        let started = self.clock.now();
        let mut inside = None;
        let mut read_took = Duration::ZERO;
        if read {
            self.clock.at(started + RSSI_SETTLE).await;
            let t_read = self.clock.now();
            if t_read <= started + CAD_SAMPLING {
                let r = self.chip.rssi().await;
                inside = Some(self.note(r)?);
            }
            read_took = self.clock.now().since(t_read);
        }
        let read = read_took;
        let r = self.chip.await_irq(Some(CAD_TIME + IRQ_SLACK)).await;
        self.note(r)?;
        let r = self.chip.irq_status().await;
        let flags = self.note(r)?;
        let r = self.chip.clear_irq().await;
        self.note(r)?;
        let ended = self.clock.now();
        self.held.mode = Mode::StandbyRc;
        // The oscillator stopped at the CAD's end, no sooner than its
        // sampling; `ended` may be late by the interrupt's wait, and a
        // restart timed from it could find the oscillator cold
        // (`docs/radio-recovery-2026-11-10.md`).
        self.held.osc_stopped = Some((sent + CAD_SAMPLING).min(ended));
        if flags & irq::CAD_DONE == 0 {
            self.held = Held::UNKNOWN;
            return Err(Error::NotDone(flags));
        }
        self.after_start(start).await?;
        let busy = flags & irq::CAD_DETECTED != 0;
        let phases = [started.since(t0), read, ended.since(started)];
        Ok((busy, inside.filter(|&d| d > NO_READING_DBM), phases))
    }

    /// Listen before talking on `channel`: the CAD and the reading, on
    /// the footing of a reading while listening. Leaves the chip in
    /// standby, or listening on `channel` if the reading is taken after
    /// the CAD.
    pub async fn lbt(&mut self, channel: Channel) -> Result<LbtTiming, Error<C::Error>> {
        let (busy, inside, [prepare, read, cad]) = self.cad(channel, true).await?;
        let mut after = Duration::ZERO;
        let dbm = match (self.lbt, inside) {
            (LbtRead::Inside { offset_db }, Some(dbm)) => dbm - offset_db,
            // No reading: the top of the scale, so the core does not
            // transmit on a check it never got.
            (LbtRead::Inside { .. }, None) => crate::FAILED_RSSI_DBM,
            (LbtRead::After, _) => {
                let t = self.clock.now();
                self.listen(channel).await?;
                self.clock.at(self.clock.now() + RSSI_SETTLE).await;
                let dbm = self.rssi().await?;
                after = self.clock.now().since(t);
                dbm
            }
        };
        Ok(LbtTiming {
            busy,
            dbm,
            inside_dbm: inside,
            prepare,
            read,
            cad,
            after,
        })
    }

    /// The CAD alone on `channel`, for the console: whether a preamble was
    /// seen, the preparation and the CAD's time. Leaves the chip in STDBY_RC.
    pub async fn cad_only(
        &mut self,
        channel: Channel,
    ) -> Result<(bool, Duration, Duration), Error<C::Error>> {
        let (busy, _, [prepare, _, cad]) = self.cad(channel, false).await?;
        Ok((busy, prepare, cad))
    }

    /// Send `frame` at `dbm` conducted. Leaves the chip in STDBY_XOSC.
    pub async fn transmit(&mut self, frame: &Frame, dbm: i8) -> Result<TxTiming, Error<C::Error>> {
        let t0 = self.clock.now();
        let receiving = matches!(self.held.mode, Mode::Rx | Mode::Sniff);
        self.ready().await?;
        self.standby_xosc().await?;
        if receiving {
            self.clear_stale_irq().await?;
        }
        self.set_frequency(frame.channel).await?;
        self.set_packet_params((frame.preamble_symbols, frame.len))
            .await?;
        self.set_tx_power(dbm).await?;
        let r = self.chip.write_payload(frame.payload()).await;
        self.note(r)?;
        let start = self.before_start().await?;
        let r = self.chip.tx().await;
        self.note(r)?;
        self.held.mode = Mode::Tx;
        let started = self.clock.now();
        let airtime = time_on_air(frame.len, frame.preamble_symbols);
        let r = self.chip.await_irq(Some(airtime + IRQ_SLACK)).await;
        self.note(r)?;
        let r = self.chip.irq_status().await;
        let flags = self.note(r)?;
        let r = self.chip.clear_irq().await;
        self.note(r)?;
        // The fallback mode set in the configuration.
        self.held.mode = Mode::StandbyXosc;
        if flags & irq::TX_DONE == 0 {
            self.held = Held::UNKNOWN;
            return Err(Error::NotDone(flags));
        }
        self.after_start(start).await?;
        Ok(TxTiming {
            prepare: started.since(t0),
            tx: self.clock.now().since(started),
        })
    }

    /// One probe of a cold start at `delay`: a standby with the
    /// oscillator off for [`tcxo::COLD_WAIT`], the delay programmed, the
    /// errors cleared, a receive, and the error word read.
    async fn probe_cold(&mut self, delay: Duration) -> Result<bool, Error<C::Error>> {
        self.ready().await?;
        self.standby_rc().await?;
        let off_since = self.held.osc_stopped.unwrap_or(self.clock.now());
        self.clock.at(off_since + tcxo::COLD_WAIT).await;
        self.set_tcxo(delay).await?;
        let r = self.chip.clear_errors().await;
        self.note(r)?;
        let r = self.chip.rx().await;
        self.note(r)?;
        self.held.mode = Mode::Rx;
        self.held.osc_stopped = None;
        let r = self.chip.errors().await;
        let (errors, _) = self.note(r)?;
        let ok = errors & START_ERRORS == 0;
        let r = self.chip.clear_errors().await;
        self.note(r)?;
        self.standby_rc().await?;
        let temp = self.chip.die_temperature().await;
        self.cold.probed(tcxo::Probe {
            delay,
            ok,
            at: self.clock.now(),
            temp,
            gap: None,
        });
        Ok(ok)
    }

    /// One probe of a restart at `delay`, as in use: a CAD on the channel
    /// after `home`, then back home. `None` if the gap reached
    /// [`tcxo::STILL_RUNNING`], so the start was not a restart.
    async fn probe_restart_once(
        &mut self,
        delay: Duration,
        home: Channel,
    ) -> Result<Option<bool>, Error<C::Error>> {
        self.ready().await?;
        if self.held.mode != Mode::Rx {
            // Listening first, started cold.
            self.held.osc_stopped = None;
            self.start_rx(home).await?;
        }
        self.cad(home.next(), false).await?;
        self.set_frequency(home).await?;
        self.set_packet_params(RX_PACKET).await?;
        self.set_tcxo(delay).await?;
        let stopped = self.held.osc_stopped.unwrap_or(Instant::ZERO);
        let gap = self.clock.now().since(stopped);
        let r = self.chip.rx().await;
        self.note(r)?;
        self.held.mode = Mode::Rx;
        self.held.osc_stopped = None;
        let r = self.chip.errors().await;
        let (errors, _) = self.note(r)?;
        if errors != 0 {
            let r = self.chip.clear_errors().await;
            self.note(r)?;
        }
        let ok = errors & START_ERRORS == 0;
        if !ok {
            // Perhaps left in STDBY_RC: the next probe starts it cold.
            let r = self.chip.standby(false).await;
            self.note(r)?;
            self.held.mode = Mode::StandbyRc;
        }
        if gap >= tcxo::STILL_RUNNING {
            return Ok(None);
        }
        let temp = self.chip.die_temperature().await;
        self.restart.probed(tcxo::Probe {
            delay,
            ok,
            at: self.clock.now(),
            temp,
            gap: Some(gap),
        });
        Ok(Some(ok))
    }

    /// A probe of a restart at `delay`. One whose gap reached the window is
    /// made again; [`tcxo::CONFIRM`] such in a row count as a failure.
    async fn probe_restart(
        &mut self,
        delay: Duration,
        home: Channel,
    ) -> Result<bool, Error<C::Error>> {
        for _ in 0..tcxo::CONFIRM {
            if let Some(ok) = self.probe_restart_once(delay, home).await? {
                return Ok(ok);
            }
        }
        Ok(false)
    }

    /// Run `search` on the delay `which` names to its outcome and settle
    /// the delay on it.
    async fn search(&mut self, which: Which, mut search: Search) -> Result<(), Error<C::Error>> {
        while let Some(delay) = search.next() {
            let ok = match which {
                Which::Cold => self.probe_cold(delay).await?,
                Which::Restart(home) => self.probe_restart(delay, home).await?,
            };
            search.result(ok);
        }
        let lowest = search.outcome().expect("the search is over");
        let now = self.clock.now();
        let (name, ceiling) = match which {
            Which::Cold => ("cold", tcxo::MAX),
            Which::Restart(_) => ("restart", self.cold.delay),
        };
        let delay = match which {
            Which::Cold => &mut self.cold,
            Which::Restart(_) => &mut self.restart,
        };
        let temp = delay.last.and_then(|p| p.temp);
        delay.settle(lowest, ceiling, now, temp);
        log::info!(
            "tcxo: {} delay {} us after {} probes, lowest confirmed {:?} us{}",
            name,
            delay.delay.as_micros(),
            delay.probes,
            lowest.map(|d| d.as_micros()),
            Gap(delay.last.and_then(|p| p.gap))
        );
        Ok(())
    }

    /// A probe below each delay in use whose schedule says it is due
    /// (`crate::tcxo`); the restart's comes back to `home`.
    async fn reprobe_if_due(&mut self, home: Channel) -> Result<(), Error<C::Error>> {
        let now = self.clock.now();
        if !self.cold.check_due(now) && !self.restart.check_due(now) {
            return Ok(());
        }
        let temp = self.chip.die_temperature().await;
        if self.cold.check_due(now) && self.cold.probe_due(now, temp) {
            self.reprobe_one(Which::Cold).await?;
        }
        if self.restart.check_due(now) && self.restart.probe_due(now, temp) {
            self.reprobe_one(Which::Restart(home)).await?;
        }
        Ok(())
    }

    /// The probes below both delays in use, now, the restart's back to
    /// `home`.
    pub async fn reprobe(&mut self, home: Channel) -> Result<(), Error<C::Error>> {
        self.reprobe_one(Which::Cold).await?;
        self.reprobe_one(Which::Restart(home)).await
    }

    /// The probe below one delay in use: at two steps down, so that a pass
    /// lowers the delay by one and keeps the margin.
    async fn reprobe_one(&mut self, which: Which) -> Result<(), Error<C::Error>> {
        let search = match which {
            Which::Cold => {
                let below = self.cold.probe_below().unwrap_or(tcxo::MIN);
                self.cold.confirm(below, tcxo::MAX)
            }
            Which::Restart(_) => {
                let below = self.restart.probe_below().unwrap_or(tcxo::SHORT_STEP);
                self.restart.confirm(below, self.cold.delay)
            }
        };
        self.search(which, search).await
    }

    /// Check the reading inside a CAD against one while listening on
    /// `channel`, in pairs, and decide where a listen-before-talk reads
    /// from (`SELF_CHECK_PAIRS`, `SELF_CHECK_LIMIT_DB`).
    async fn self_check(&mut self, channel: Channel) -> Result<(), Error<C::Error>> {
        let mut diffs = [0i16; SELF_CHECK_PAIRS];
        let mut pairs = 0;
        for diff in diffs.iter_mut() {
            self.receive(channel).await?;
            self.clock.at(self.clock.now() + RSSI_SETTLE).await;
            let listening = self.rssi().await?;
            let (_, inside, _) = self.cad(channel, true).await?;
            let Some(inside) = inside else {
                continue;
            };
            *diff = inside - listening;
            pairs += 1;
        }
        let diffs = &mut diffs[..pairs];
        diffs.sort_unstable();
        self.lbt = match diffs.get(pairs / 2) {
            Some(&median) if pairs == SELF_CHECK_PAIRS && median.abs() <= SELF_CHECK_LIMIT_DB => {
                LbtRead::Inside { offset_db: median }
            }
            _ => LbtRead::After,
        };
        log::info!(
            "radio: reading inside a CAD less listening {:?} dB over {} pairs: {:?}",
            diffs,
            pairs,
            self.lbt
        );
        Ok(())
    }
}

impl<C: Chip, K: Clock> crate::Radio for Sx1262<C, K> {
    type Error = Error<C::Error>;
    const DEAF_TO: Option<bark_proto::NodeId> = crate::BENCH_DEAF_TO;

    async fn listen(&mut self, channel: Channel) -> Result<(), Self::Error> {
        Sx1262::listen(self, channel).await
    }

    async fn sniff(&mut self, channel: Channel) -> Result<(), Self::Error> {
        Sx1262::sniff(self, channel).await
    }

    async fn wait(&mut self) -> Result<(), Self::Error> {
        Sx1262::wait(self).await
    }

    async fn service(&mut self, buf: &mut [u8; MAX_FRAME]) -> Result<Heard, Self::Error> {
        Sx1262::service(self, buf).await
    }

    async fn lbt(&mut self, channel: Channel) -> Result<(bool, i16), Self::Error> {
        let t = Sx1262::lbt(self, channel).await?;
        Ok((t.busy, t.dbm))
    }

    async fn rssi(&mut self) -> Result<i16, Self::Error> {
        Sx1262::rssi(self).await
    }

    async fn transmit(&mut self, frame: &Frame, dbm: i8) -> Result<(), Self::Error> {
        Sx1262::transmit(self, frame, dbm).await.map(|_| ())
    }

    fn recover(&mut self) {
        Sx1262::recover(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tcxo::{COLD_WAIT, CONFIRM, MARGIN, SHORT_STEP, STEP};
    use bark_proto::radio::MIN_PREAMBLE_SYMBOLS;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::vec::Vec;

    /// What the fake chip was told, one entry per command.
    #[derive(Clone, Debug, PartialEq, Eq)]
    enum Cmd {
        Reset,
        Wake,
        Standby(bool),
        Calibrate,
        CalibrateImage,
        Configure,
        CadParams,
        Tcxo(u64),
        Errors,
        ClearErrors,
        Freq(u8),
        Packet(u16, u8),
        Power(i8),
        Payload(usize),
        Rx,
        /// The receive and sleep periods, µs.
        RxDutyCycle(u64, u64),
        Cad,
        Tx,
        Rssi,
        IrqStatus,
        ClearIrq,
        AwaitIrq,
        ReadFrame,
    }

    /// The board as the fake chip sees it.
    #[derive(Clone, Debug)]
    struct Board {
        /// How long its oscillator takes to start.
        osc_start: Duration,
        /// What `GetRssiInst` reads inside a CAD and while listening.
        inside_dbm: i16,
        listening_dbm: i16,
        /// What a CAD finds.
        detected: bool,
        /// The flags the next `GetIrqStatus` reports, if not its own.
        flags: Option<u16>,
        /// Fail the next command.
        fail_next: bool,
        temp: Option<i16>,
        /// How long its oscillator takes to restart.
        warm_start: Duration,
        /// How long after a CAD's end its interrupt reaches the driver:
        /// the time another task held the processor.
        irq_late: Duration,
        /// Fail the next command that is this one.
        fail_on: Option<Cmd>,
        /// How long `SetDIO3AsTCXOCtrl` holds the driver, another task
        /// running meanwhile.
        tcxo_takes: Duration,
    }

    #[derive(Clone)]
    struct Fake {
        cmds: Rc<RefCell<Vec<Cmd>>>,
        board: Rc<RefCell<Board>>,
        clock: FakeClock,
        /// What the chip itself holds.
        delay: Duration,
        mode: Mode,
        errors: u16,
        osc_stopped_at: Option<Instant>,
        /// When the CAD under way began sampling.
        cad_at: Instant,
        /// Flags raised and not cleared, which hold DIO1 high.
        latched: u16,
        done: bool,
    }

    #[derive(Clone)]
    struct FakeClock(Rc<RefCell<Instant>>);

    impl Clock for FakeClock {
        fn now(&self) -> Instant {
            *self.0.borrow()
        }

        async fn at(&self, t: Instant) {
            let mut now = self.0.borrow_mut();
            if t > *now {
                *now = t;
            }
        }
    }

    impl Fake {
        fn new(board: Board) -> (Fake, Rc<RefCell<Vec<Cmd>>>, FakeClock) {
            let cmds = Rc::new(RefCell::new(Vec::new()));
            let clock = FakeClock(Rc::new(RefCell::new(Instant::ZERO)));
            let fake = Fake {
                cmds: cmds.clone(),
                board: Rc::new(RefCell::new(board)),
                clock: clock.clone(),
                delay: Duration::ZERO,
                mode: Mode::Unknown,
                errors: 0,
                osc_stopped_at: None,
                cad_at: Instant::ZERO,
                latched: 0,
                done: false,
            };
            (fake, cmds, clock)
        }

        fn cmd(&mut self, c: Cmd) -> Result<(), &'static str> {
            let mut b = self.board.borrow_mut();
            if std::mem::take(&mut b.fail_next) || b.fail_on.take_if(|f| *f == c).is_some() {
                self.cmds.borrow_mut().push(c);
                return Err("bus");
            }
            drop(b);
            self.cmds.borrow_mut().push(c);
            // A command takes about 50 µs on the bus.
            let now = self.clock.now();
            *self.clock.0.borrow_mut() = now + Duration::from_micros(50);
            Ok(())
        }

        /// The oscillator starts from STDBY_RC: still running within a
        /// millisecond, else cold, and an error if the delay is short.
        /// Returns whether it started.
        fn start_osc(&mut self) -> bool {
            if self.mode != Mode::StandbyRc {
                return true;
            }
            let now = self.clock.now();
            let running = self
                .osc_stopped_at
                .is_some_and(|t| now.since(t) < tcxo::STILL_RUNNING);
            let b = self.board.borrow();
            let needs = if running { b.warm_start } else { b.osc_start };
            let started = self.delay >= needs;
            if !started {
                self.errors |= XOSC_START_ERR;
            }
            drop(b);
            *self.clock.0.borrow_mut() = now + self.delay;
            started
        }

        /// The status byte's mode bits for the chip's mode.
        fn status(&self) -> u8 {
            let mode = match self.mode {
                Mode::StandbyRc | Mode::Unknown => status::STBY_RC,
                Mode::StandbyXosc => 0x3,
                Mode::Rx | Mode::Sniff | Mode::Cad => status::RX,
                Mode::Tx => 0x6,
            };
            mode << 4
        }
    }

    impl Chip for Fake {
        type Error = &'static str;

        async fn reset(&mut self) -> Result<(), &'static str> {
            self.cmd(Cmd::Reset)?;
            self.mode = Mode::StandbyRc;
            self.errors = XOSC_START_ERR;
            self.delay = Duration::ZERO;
            Ok(())
        }
        async fn wake(&mut self) -> Result<(), &'static str> {
            self.cmd(Cmd::Wake)
        }
        async fn standby(&mut self, xosc: bool) -> Result<(), &'static str> {
            self.cmd(Cmd::Standby(xosc))?;
            // From STDBY_RC, as after a start that failed, it stops no
            // oscillator.
            if !xosc && self.mode != Mode::StandbyRc {
                self.osc_stopped_at = Some(self.clock.now());
            }
            self.mode = if xosc {
                Mode::StandbyXosc
            } else {
                Mode::StandbyRc
            };
            Ok(())
        }
        async fn calibrate(&mut self) -> Result<(), &'static str> {
            self.cmd(Cmd::Calibrate)?;
            self.start_osc();
            self.osc_stopped_at = Some(self.clock.now());
            Ok(())
        }
        async fn calibrate_image(&mut self) -> Result<(), &'static str> {
            self.cmd(Cmd::CalibrateImage)
        }
        async fn configure(&mut self) -> Result<(), &'static str> {
            self.cmd(Cmd::Configure)
        }
        async fn cad_params(&mut self) -> Result<(), &'static str> {
            self.cmd(Cmd::CadParams)
        }
        async fn set_tcxo(&mut self, delay: Duration) -> Result<(), &'static str> {
            assert_eq!(self.mode, Mode::StandbyRc, "SetDIO3AsTCXOCtrl in STDBY_RC");
            self.cmd(Cmd::Tcxo(delay.as_micros()))?;
            assert!(delay > Duration::ZERO, "a delay of zero hangs the chip");
            self.delay = delay;
            let takes = self.board.borrow().tcxo_takes;
            let now = self.clock.now();
            *self.clock.0.borrow_mut() = now + takes;
            Ok(())
        }
        async fn errors(&mut self) -> Result<(u16, u8), &'static str> {
            self.cmd(Cmd::Errors)?;
            Ok((self.errors, self.status()))
        }
        async fn clear_errors(&mut self) -> Result<(), &'static str> {
            self.cmd(Cmd::ClearErrors)?;
            self.errors = 0;
            Ok(())
        }
        async fn set_frequency(&mut self, channel: Channel) -> Result<(), &'static str> {
            self.cmd(Cmd::Freq(channel.index()))
        }
        async fn set_packet_params(&mut self, preamble: u16, len: u8) -> Result<(), &'static str> {
            self.cmd(Cmd::Packet(preamble, len))
        }
        async fn set_tx_power(&mut self, dbm: i8) -> Result<(), &'static str> {
            self.cmd(Cmd::Power(dbm))
        }
        async fn write_payload(&mut self, bytes: &[u8]) -> Result<(), &'static str> {
            self.cmd(Cmd::Payload(bytes.len()))
        }
        async fn rx(&mut self) -> Result<(), &'static str> {
            self.cmd(Cmd::Rx)?;
            // A start that failed leaves the chip in STDBY_RC, as on the
            // bench (`OpError(170)`).
            if self.start_osc() {
                self.mode = Mode::Rx;
            }
            Ok(())
        }
        async fn rx_duty_cycle(
            &mut self,
            rx: Duration,
            sleep: Duration,
        ) -> Result<(), &'static str> {
            assert_eq!(self.mode, Mode::StandbyRc, "SetRxDutyCycle from STDBY_RC");
            self.cmd(Cmd::RxDutyCycle(rx.as_micros(), sleep.as_micros()))?;
            self.start_osc();
            self.mode = Mode::Sniff;
            Ok(())
        }
        async fn cad(&mut self) -> Result<(), &'static str> {
            self.cmd(Cmd::Cad)?;
            self.done = false;
            self.start_osc();
            self.cad_at = self.clock.now();
            self.mode = Mode::Cad;
            Ok(())
        }
        async fn tx(&mut self) -> Result<(), &'static str> {
            self.cmd(Cmd::Tx)?;
            self.done = false;
            self.start_osc();
            self.mode = Mode::Tx;
            Ok(())
        }
        async fn rssi(&mut self) -> Result<i16, &'static str> {
            self.cmd(Cmd::Rssi)?;
            let b = self.board.borrow();
            Ok(match self.mode {
                Mode::Cad => b.inside_dbm,
                Mode::Rx => b.listening_dbm,
                _ => NO_READING_DBM - 1,
            })
        }
        async fn irq_status(&mut self) -> Result<u16, &'static str> {
            self.cmd(Cmd::IrqStatus)?;
            if let Some(f) = self.board.borrow_mut().flags.take() {
                // A packet ends a duty-cycled receive in STDBY_RC.
                if self.mode == Mode::Sniff && f & irq::RX_DONE != 0 {
                    self.mode = Mode::StandbyRc;
                    self.osc_stopped_at = None;
                }
                return Ok(f);
            }
            let own = match self.mode {
                Mode::Cad if self.done => {
                    irq::CAD_DONE
                        | if self.board.borrow().detected {
                            irq::CAD_DETECTED
                        } else {
                            0
                        }
                }
                Mode::Tx if self.done => irq::TX_DONE,
                _ => 0,
            };
            Ok(own | self.latched)
        }
        async fn clear_irq(&mut self) -> Result<(), &'static str> {
            self.cmd(Cmd::ClearIrq)?;
            self.latched = 0;
            match self.mode {
                Mode::Cad => self.mode = Mode::StandbyRc,
                Mode::Tx => self.mode = Mode::StandbyXosc,
                _ => {}
            }
            Ok(())
        }
        async fn await_irq(&mut self, timeout: Option<Duration>) -> Result<(), &'static str> {
            self.cmd(Cmd::AwaitIrq)?;
            // A flag left raised holds DIO1 high: the wait ends at once.
            if self.latched != 0 {
                return Ok(());
            }
            self.done = true;
            let now = self.clock.now();
            let ends = match self.mode {
                // The CAD ends in STDBY_RC on its own time.
                Mode::Cad => {
                    let end = self.cad_at + CAD_TIME;
                    self.osc_stopped_at = Some(end);
                    end.max(now) + self.board.borrow().irq_late
                }
                Mode::Tx => now + Duration::from_millis(15),
                _ => now,
            };
            if timeout.is_some_and(|t| ends.since(now) > t) {
                return Err("dio1 timeout");
            }
            *self.clock.0.borrow_mut() = ends;
            Ok(())
        }
        async fn read_frame(
            &mut self,
            buf: &mut [u8; MAX_FRAME],
        ) -> Result<(u8, i16, i8), &'static str> {
            self.cmd(Cmd::ReadFrame)?;
            buf[0] = 1;
            Ok((1, -80, 5))
        }
        async fn die_temperature(&mut self) -> Option<i16> {
            self.board.borrow().temp
        }
    }

    /// The bench's board: its oscillator starts in 200 to 500 µs, so the
    /// shortest step probed passes and the delay settles one step over it.
    fn board() -> Board {
        Board {
            osc_start: Duration::from_micros(200),
            inside_dbm: -75,
            listening_dbm: -79,
            detected: false,
            flags: None,
            fail_next: false,
            temp: Some(100),
            warm_start: Duration::ZERO,
            irq_late: Duration::ZERO,
            fail_on: None,
            tcxo_takes: Duration::ZERO,
        }
    }

    fn ch(k: u8) -> Channel {
        Channel::new(k).unwrap()
    }

    fn run<T>(f: impl core::future::Future<Output = T>) -> T {
        let mut f = std::pin::pin!(f);
        let waker = std::task::Waker::noop();
        let mut cx = std::task::Context::from_waker(waker);
        loop {
            if let std::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
                return v;
            }
        }
    }

    type Radio = Sx1262<Fake, FakeClock>;

    fn booted(board: Board) -> (Radio, Rc<RefCell<Vec<Cmd>>>, FakeClock) {
        let (fake, cmds, clock) = Fake::new(board);
        let radio = run(Sx1262::boot(fake, clock.clone(), ch(3))).expect("boots");
        (radio, cmds, clock)
    }

    fn take(cmds: &Rc<RefCell<Vec<Cmd>>>) -> Vec<Cmd> {
        std::mem::take(&mut *cmds.borrow_mut())
    }

    std::thread_local! {
        static LOGGED: RefCell<Vec<std::string::String>> = const { RefCell::new(Vec::new()) };
    }

    /// The log lines of this test's thread.
    struct Logger;

    impl log::Log for Logger {
        fn enabled(&self, _: &log::Metadata) -> bool {
            true
        }
        fn log(&self, record: &log::Record) {
            let line = std::format!("{}", record.args());
            LOGGED.with(|l| l.borrow_mut().push(line));
        }
        fn flush(&self) {}
    }

    /// The lines logged on this thread since the last call.
    fn logged() -> Vec<std::string::String> {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            log::set_logger(&Logger).expect("one logger");
            log::set_max_level(log::LevelFilter::Info);
        });
        LOGGED.with(|l| std::mem::take(&mut *l.borrow_mut()))
    }

    #[test]
    fn boot_resets_configures_calibrates_probes_and_checks() {
        let (radio, cmds, _) = booted(board());
        let cmds = take(&cmds);
        assert_eq!(
            &cmds[..7],
            &[
                Cmd::Reset,
                Cmd::ClearErrors,
                Cmd::Tcxo(tcxo::MAX.as_micros()),
                Cmd::Calibrate,
                Cmd::Errors,
                Cmd::Configure,
                Cmd::CalibrateImage,
            ]
        );
        assert_eq!(cmds.iter().filter(|c| **c == Cmd::Configure).count(), 1);
        let t = radio.cold();
        assert_eq!(t.delay, Duration::from_micros(500));
        assert_eq!(t.probes as usize, 4 + CONFIRM as usize);
        let probe = [Cmd::ClearErrors, Cmd::Rx, Cmd::Errors, Cmd::ClearErrors];
        let probes = cmds.windows(4).filter(|w| w == &probe).count();
        assert_eq!(probes, t.probes as usize);
        // Its oscillator restarts at once: the floor and a step.
        let r = radio.restart();
        assert_eq!(r.delay, SHORT_STEP.mul(2));
        assert_eq!(r.probes as usize, 4 + CONFIRM as usize);
        assert_eq!(radio.lbt_read(), LbtRead::Inside { offset_db: 4 });
        assert_eq!(radio.held().mode, Mode::Rx);
        assert_eq!(radio.held().freq, Some(ch(3)));
        assert_eq!(radio.stats().resets, 0);
    }

    #[test]
    fn listening_on_the_same_channel_sends_nothing_and_a_retune_three_commands() {
        let (mut radio, cmds, _) = booted(board());
        take(&cmds);
        run(radio.listen(ch(3))).unwrap();
        assert_eq!(take(&cmds), []);
        run(radio.listen(ch(7))).unwrap();
        assert_eq!(take(&cmds), [Cmd::Standby(true), Cmd::Freq(7), Cmd::Rx]);
    }

    #[test]
    fn a_listen_before_talk_is_the_cad_with_the_reading_inside_and_a_short_restart() {
        let (mut radio, cmds, _) = booted(board());
        take(&cmds);
        let t = run(radio.lbt(ch(5))).unwrap();
        assert_eq!((t.busy, t.inside_dbm, t.dbm), (false, Some(-75), -79));
        assert_eq!(
            take(&cmds),
            [
                Cmd::Standby(true),
                Cmd::ClearIrq,
                Cmd::Freq(5),
                Cmd::CadParams,
                Cmd::Cad,
                Cmd::Rssi,
                Cmd::AwaitIrq,
                Cmd::IrqStatus,
                Cmd::ClearIrq,
            ]
        );
        assert_eq!(radio.held().mode, Mode::StandbyRc);
        // The restart's delay was programmed at the self-check's last
        // restart and holds.
        let (restarts, cold_starts) = (radio.stats().restarts, radio.stats().cold_starts);
        run(radio.listen(ch(3))).unwrap();
        assert_eq!(take(&cmds), [Cmd::Freq(3), Cmd::Rx, Cmd::Errors]);
        assert_eq!(radio.held().tcxo, Some(radio.restart_delay()));
        run(radio.lbt(ch(5))).unwrap();
        run(radio.listen(ch(3))).unwrap();
        let cmds = take(&cmds);
        assert!(!cmds.iter().any(|c| matches!(c, Cmd::Tcxo(_))), "{cmds:?}");
        assert_eq!(radio.stats().cold_starts, cold_starts);
        assert_eq!(radio.stats().restarts, restarts + 2);
        assert!(
            matches!(radio.last_start(), Start::Restart { gap, .. } if gap < tcxo::STILL_RUNNING)
        );
        assert_eq!(radio.cold().delay, Duration::from_micros(500));
    }

    /// The CAD's parameters go before every CAD, and busy is the
    /// CadDetected flag, with or without the reading inside.
    #[test]
    fn every_cad_sends_its_parameters_and_reports_the_detected_flag() {
        let (mut radio, cmds, _) = booted(board());
        let configured = radio.stats().configured;
        take(&cmds);
        for detected in [false, true, false] {
            radio.chip().board.borrow_mut().detected = detected;
            let t = run(radio.lbt(ch(5))).unwrap();
            assert_eq!(t.busy, detected);
            let sent = take(&cmds);
            let at = sent
                .iter()
                .position(|c| *c == Cmd::CadParams)
                .expect("parameters");
            assert_eq!(sent[at + 1], Cmd::Cad);
            run(radio.listen(ch(3))).unwrap();
            take(&cmds);
            let (busy, prepare, cad) = run(radio.cad_only(ch(5))).unwrap();
            assert_eq!(busy, detected);
            assert!(prepare < cad);
            let sent = take(&cmds);
            assert!(sent.windows(2).any(|w| w == [Cmd::CadParams, Cmd::Cad]));
            assert!(!sent.contains(&Cmd::Rssi), "{sent:?}");
            run(radio.listen(ch(3))).unwrap();
        }
        assert_eq!(radio.stats().configured, configured);
    }

    #[test]
    fn a_long_gap_after_a_cad_is_a_cold_start_with_the_probed_delay() {
        let (mut radio, cmds, clock) = booted(board());
        let cold_starts = radio.stats().cold_starts;
        take(&cmds);
        run(radio.lbt(ch(5))).unwrap();
        take(&cmds);
        let now = clock.now();
        run(clock.at(now + Duration::from_millis(3)));
        run(radio.listen(ch(3))).unwrap();
        assert_eq!(
            take(&cmds),
            [Cmd::Freq(3), Cmd::Tcxo(500), Cmd::Rx, Cmd::Errors]
        );
        assert_eq!(radio.stats().cold_starts, cold_starts + 1);
    }

    /// A failed cold start leaves the chip in STDBY_RC: the delay steps up
    /// and the receive starts again cold.
    #[test]
    fn a_cold_start_error_in_use_steps_the_delay_up_and_starts_again() {
        let (mut radio, cmds, clock) = booted(board());
        take(&cmds);
        // The board got cold: its oscillator now takes longer.
        radio.chip().board.borrow_mut().osc_start = Duration::from_micros(700);
        run(radio.lbt(ch(5))).unwrap();
        let now = clock.now();
        run(clock.at(now + Duration::from_millis(3)));
        take(&cmds);
        run(radio.listen(ch(3))).unwrap();
        assert_eq!(
            radio.cold().delay,
            Duration::from_micros(500 + STEP.as_micros())
        );
        assert_eq!(radio.cold().errors, 1);
        assert_eq!(
            take(&cmds),
            [
                Cmd::Freq(3),
                Cmd::Tcxo(500),
                Cmd::Rx,
                Cmd::Errors,
                Cmd::ClearErrors,
                Cmd::Standby(false),
                Cmd::Tcxo(750),
                Cmd::Rx,
                Cmd::Errors,
            ]
        );
        assert_eq!(radio.held().mode, Mode::Rx);
        assert_eq!(radio.chip().mode, Mode::Rx);
    }

    /// A cold start that fails twice is left to the reset, whose search
    /// finds what the board now needs.
    #[test]
    fn a_receive_that_will_not_start_cold_is_reset() {
        let (mut radio, cmds, clock) = booted(board());
        radio.chip().board.borrow_mut().osc_start = Duration::from_micros(1_100);
        run(radio.lbt(ch(5))).unwrap();
        let now = clock.now();
        run(clock.at(now + Duration::from_millis(3)));
        assert_eq!(
            run(radio.listen(ch(3))),
            Err(Error::NotStarted(status::STBY_RC << 4))
        );
        assert_eq!(radio.held().mode, Mode::Unknown);
        assert_eq!(radio.cold().errors, 2);
        take(&cmds);
        run(radio.listen(ch(3))).unwrap();
        assert_eq!(take(&cmds)[..2], [Cmd::Errors, Cmd::Reset]);
        assert_eq!(radio.cold().delay, Duration::from_micros(1_500));
        assert_eq!(radio.held().mode, Mode::Rx);
        assert_eq!(radio.chip().mode, Mode::Rx);
    }

    /// The restart's window may pass while another task runs during the
    /// delay's command: the start is then made cold.
    #[test]
    fn a_restart_whose_window_passed_while_the_delay_was_set_is_made_cold() {
        let (mut radio, cmds, clock) = booted(board());
        // The cold delay programmed last, so that the restart's is sent.
        run(radio.lbt(ch(5))).unwrap();
        run(clock.at(clock.now() + Duration::from_millis(3)));
        run(radio.listen(ch(3))).unwrap();
        run(radio.lbt(ch(5))).unwrap();
        radio.chip().board.borrow_mut().tcxo_takes = tcxo::STILL_RUNNING;
        take(&cmds);
        run(radio.listen(ch(3))).unwrap();
        assert_eq!(
            take(&cmds),
            [
                Cmd::Freq(3),
                Cmd::Tcxo(100),
                Cmd::Tcxo(500),
                Cmd::Rx,
                Cmd::Errors
            ]
        );
        assert!(
            matches!(radio.last_start(), Start::Cold { gap: Some(g), .. } if g >= tcxo::STILL_RUNNING)
        );
        assert_eq!((radio.cold().errors, radio.restart().errors), (0, 0));
        assert_eq!(radio.chip().mode, Mode::Rx);
    }

    #[test]
    fn a_transmission_sends_its_packet_parameters_and_the_receive_ones_after() {
        let (mut radio, cmds, _) = booted(board());
        take(&cmds);
        let mut frame = Frame {
            channel: ch(3),
            preamble_symbols: MIN_PREAMBLE_SYMBOLS,
            len: 13,
            bytes: [0; MAX_FRAME],
        };
        let t = run(radio.transmit(&frame, 10)).unwrap();
        assert!(t.tx >= Duration::from_millis(15));
        assert_eq!(
            take(&cmds),
            [
                Cmd::Standby(true),
                Cmd::ClearIrq,
                Cmd::Packet(MIN_PREAMBLE_SYMBOLS, 13),
                Cmd::Power(10),
                Cmd::Payload(13),
                Cmd::Tx,
                Cmd::AwaitIrq,
                Cmd::IrqStatus,
                Cmd::ClearIrq,
            ]
        );
        assert_eq!(radio.held().mode, Mode::StandbyXosc);
        run(radio.listen(ch(3))).unwrap();
        assert_eq!(
            take(&cmds),
            [Cmd::Packet(RX_PACKET.0, RX_PACKET.1), Cmd::Rx]
        );
        // The same frame again: the power and the parameters are held.
        frame.channel = ch(4);
        run(radio.transmit(&frame, 10)).unwrap();
        assert_eq!(
            take(&cmds)[..5],
            [
                Cmd::Standby(true),
                Cmd::ClearIrq,
                Cmd::Freq(4),
                Cmd::Packet(MIN_PREAMBLE_SYMBOLS, 13),
                Cmd::Payload(13),
            ]
        );
    }

    #[test]
    fn a_failure_says_which_check_failed() {
        let (mut radio, _, _) = booted(board());
        run(radio.listen(ch(3))).unwrap();
        let mut buf = [0; MAX_FRAME];
        for (flags, why) in [
            (irq::PREAMBLE_DETECTED | irq::HEADER_ERR, Failure::Header),
            (
                irq::HEADER_VALID | irq::CRC_ERR | irq::RX_DONE,
                Failure::Crc,
            ),
            (irq::TIMEOUT, Failure::Timeout),
        ] {
            radio.chip().board.borrow_mut().flags = Some(flags);
            let heard = run(radio.service(&mut buf)).unwrap();
            assert_eq!(heard, Heard::Failed(why));
        }
    }

    #[test]
    fn a_sniff_takes_the_probed_delay_and_everything_is_sent_again_after_it() {
        let (mut radio, cmds, _) = booted(board());
        take(&cmds);
        run(radio.sniff(ch(3))).unwrap();
        assert_eq!(
            take(&cmds),
            [
                Cmd::Standby(false),
                Cmd::Tcxo(500),
                Cmd::RxDutyCycle(3_750, 233_625)
            ]
        );
        // A packet ends the cycle in STDBY_RC.
        radio.chip().board.borrow_mut().flags = Some(irq::RX_DONE);
        let mut buf = [0; MAX_FRAME];
        let heard = run(radio.service(&mut buf)).unwrap();
        assert!(matches!(heard, Heard::Frame { len: 1, .. }));
        assert_eq!(radio.held().mode, Mode::StandbyRc);
        take(&cmds);
        // The cycle's cold starts checked, then the whole configuration.
        run(radio.listen(ch(3))).unwrap();
        assert_eq!(
            take(&cmds),
            [
                Cmd::Errors,
                Cmd::Configure,
                Cmd::Freq(3),
                Cmd::Packet(RX_PACKET.0, RX_PACKET.1),
                Cmd::Tcxo(500),
                Cmd::Rx,
                Cmd::Errors,
            ]
        );
        assert_eq!(radio.stats().configured, 2);
        // A sniff ended by the core's next action: the chip is woken first.
        run(radio.sniff(ch(3))).unwrap();
        take(&cmds);
        run(radio.listen(ch(3))).unwrap();
        assert_eq!(
            &take(&cmds)[..4],
            &[Cmd::Wake, Cmd::Standby(false), Cmd::Errors, Cmd::Configure]
        );
    }

    #[test]
    fn an_error_is_displayed_as_debug_writes_it() {
        for e in [
            Error::Chip("SPI"),
            Error::NotDone(0x40),
            Error::NotStarted(0x2a),
        ] {
            assert_eq!(std::format!("{e}"), std::format!("{e:?}").replace('"', ""));
        }
    }

    #[test]
    fn an_error_resets_the_chip_and_measures_again_as_at_boot() {
        let (mut radio, cmds, _) = booted(board());
        let probes = radio.cold().probes;
        take(&cmds);
        radio.chip().board.borrow_mut().fail_next = true;
        assert!(run(radio.lbt(ch(5))).is_err());
        assert_eq!(radio.held().mode, Mode::Unknown);
        take(&cmds);
        // The reading inside a CAD now sits 2 dB over one while listening.
        radio.chip().board.borrow_mut().inside_dbm = -77;
        run(radio.listen(ch(3))).unwrap();
        let cmds = take(&cmds);
        // What failed read first, then the boot's sequence.
        assert_eq!(
            &cmds[..8],
            &[
                Cmd::Errors,
                Cmd::Reset,
                Cmd::ClearErrors,
                Cmd::Tcxo(tcxo::MAX.as_micros()),
                Cmd::Calibrate,
                Cmd::Errors,
                Cmd::Configure,
                Cmd::CalibrateImage,
            ]
        );
        assert_eq!(radio.cold().probes, 2 * probes);
        assert_eq!(radio.cold().delay, Duration::from_micros(500));
        assert_eq!(radio.lbt_read(), LbtRead::Inside { offset_db: 2 });
        assert!(cmds.ends_with(&[Cmd::Rx, Cmd::Errors]));
        assert_eq!(radio.held().mode, Mode::Rx);
        assert_eq!(radio.stats().resets, 1);
        // Once: the next listen sends nothing.
        run(radio.listen(ch(3))).unwrap();
        assert_eq!(take_cmds(&radio), []);
        // An interrupt without its done flag is an error too.
        radio.chip().board.borrow_mut().flags = Some(0);
        assert_eq!(run(radio.lbt(ch(5))), Err(Error::NotDone(0)));
        assert_eq!(radio.held().mode, Mode::Unknown);
    }

    fn take_cmds(radio: &Radio) -> Vec<Cmd> {
        std::mem::take(&mut *radio.chip.cmds.borrow_mut())
    }

    /// Asked to, the chip is reset and measured again as after an error.
    #[test]
    fn a_recovery_asked_for_is_the_boot_again() {
        let (mut radio, cmds, _) = booted(board());
        let probes = radio.cold().probes;
        let restart_probes = radio.restart().probes;
        take(&cmds);
        radio.recover();
        run(radio.lbt(ch(5))).unwrap();
        run(radio.listen(ch(3))).unwrap();
        let cmds = take(&cmds);
        assert_eq!(cmds.iter().filter(|c| **c == Cmd::Reset).count(), 1);
        assert_eq!(radio.cold().probes, 2 * probes);
        assert_eq!(radio.restart().probes, 2 * restart_probes);
        assert_eq!(radio.stats().resets, 1);
        assert_eq!(radio.held().mode, Mode::Rx);
    }

    /// A preamble seen as the receive was left holds DIO1 high; cleared
    /// first, the CAD and the transmission end on their own flags.
    #[test]
    fn a_flag_left_by_the_receiver_does_not_end_a_cad_or_a_transmission() {
        let (mut radio, cmds, _) = booted(board());
        for flags in [
            irq::PREAMBLE_DETECTED,
            irq::PREAMBLE_DETECTED | irq::HEADER_VALID,
        ] {
            radio.chip().latched = flags;
            let t = run(radio.lbt(ch(5))).expect("the CAD ends on its own flag");
            assert!(!t.busy);
            run(radio.listen(ch(3))).unwrap();
            radio.chip().latched = flags;
            let frame = Frame {
                channel: ch(3),
                preamble_symbols: MIN_PREAMBLE_SYMBOLS,
                len: 13,
                bytes: [0; MAX_FRAME],
            };
            run(radio.transmit(&frame, 10)).expect("the transmission ends on its own flag");
            run(radio.listen(ch(3))).unwrap();
        }
        assert_eq!(radio.stats().resets, 0);
        // From the standby a CAD ends in, nothing is left to clear.
        run(radio.lbt(ch(5))).unwrap();
        take(&cmds);
        run(radio.lbt(ch(6))).unwrap();
        assert_eq!(
            take(&cmds).iter().filter(|c| **c == Cmd::ClearIrq).count(),
            1
        );
    }

    /// A restart slower than the boot found steps the restart's delay up,
    /// logged, and starts again cold; the cold delay is not blamed.
    #[test]
    fn a_failed_restart_steps_the_restart_up_and_starts_again_cold() {
        let (mut radio, cmds, _) = booted(board());
        assert_eq!(radio.restart_delay(), Duration::from_micros(100));
        radio.chip().board.borrow_mut().warm_start = Duration::from_micros(220);
        logged();
        run(radio.lbt(ch(5))).unwrap();
        take(&cmds);
        run(radio.listen(ch(3))).unwrap();
        assert_eq!(
            take(&cmds),
            [
                Cmd::Freq(3),
                Cmd::Rx,
                Cmd::Errors,
                Cmd::ClearErrors,
                Cmd::Standby(false),
                Cmd::Tcxo(500),
                Cmd::Rx,
                Cmd::Errors,
            ]
        );
        assert_eq!(radio.restart().delay, Duration::from_micros(150));
        assert_eq!(radio.restart().errors, 1);
        assert_eq!(
            (radio.cold().delay, radio.cold().errors),
            (Duration::from_micros(500), 0)
        );
        let lines = logged();
        let line = lines
            .iter()
            .find(|l| l.starts_with("tcxo: a restart"))
            .expect("logged");
        assert!(
            line.contains(" us after stopping failed at 100 us, restarts now take 150 us"),
            "{line}"
        );
        // Until it passes, at the first step over what the board needs.
        for _ in 0..4 {
            run(radio.lbt(ch(5))).unwrap();
            run(radio.listen(ch(3))).unwrap();
        }
        assert_eq!(radio.restart().delay, Duration::from_micros(250));
        assert_eq!(radio.restart().errors, 3);
        let restarts = radio.stats().restarts;
        run(radio.lbt(ch(5))).unwrap();
        take(&cmds);
        run(radio.listen(ch(3))).unwrap();
        assert_eq!(take(&cmds), [Cmd::Freq(3), Cmd::Rx, Cmd::Errors]);
        assert_eq!(radio.stats().restarts, restarts + 1);
        assert_eq!(radio.restart().errors, 3);
        assert_eq!(radio.cold().errors, 0);
    }

    /// A restart whose operation fails before its error word is read: the
    /// word read before the reset blames the restart.
    #[test]
    fn a_restart_cut_off_by_an_error_is_blamed_before_the_reset() {
        let (mut radio, cmds, _) = booted(board());
        radio.chip().board.borrow_mut().warm_start = Duration::from_micros(220);
        run(radio.lbt(ch(5))).unwrap();
        // The next check restarts from the first one's standby, and its
        // reading fails.
        radio.chip().board.borrow_mut().fail_on = Some(Cmd::Rssi);
        assert!(run(radio.lbt(ch(6))).is_err());
        take(&cmds);
        logged();
        run(radio.listen(ch(3))).unwrap();
        assert_eq!(take(&cmds)[..2], [Cmd::Errors, Cmd::Reset]);
        let lines = logged();
        assert!(
            lines
                .iter()
                .any(|l| l.contains("failed at 100 us, restarts now take 150 us")),
            "{lines:?}"
        );
        // The reset measures again, and the search finds what it needs.
        assert_eq!(radio.restart().delay, Duration::from_micros(300));
        assert_eq!(radio.restart().errors, 1);
    }

    /// A start error found before a reset after the start's word read
    /// clean, as a PLL lock failure from a running oscillator, blames no
    /// start.
    #[test]
    fn an_error_after_a_clean_start_is_blamed_on_no_start() {
        let (mut radio, cmds, _) = booted(board());
        run(radio.lbt(ch(5))).unwrap();
        run(radio.listen(ch(3))).unwrap();
        let restarts = radio.restart().errors;
        let colds = radio.cold().errors;
        // The next check runs from the oscillator left running, and fails.
        radio.chip().errors = PLL_LOCK_ERR;
        radio.chip().board.borrow_mut().fail_on = Some(Cmd::Rssi);
        assert!(run(radio.lbt(ch(5))).is_err());
        take(&cmds);
        logged();
        run(radio.listen(ch(3))).unwrap();
        let lines = logged();
        assert!(!lines.iter().any(|l| l.contains("failed at")), "{lines:?}");
        assert_eq!(radio.restart().errors, restarts);
        assert_eq!(radio.cold().errors, colds);
    }

    /// The boot's restart search, a CAD and back, on a board that needs
    /// 170 µs, lands a step over the lowest pass, under the cold delay.
    #[test]
    fn the_restart_search_at_boot_lands_above_the_board_with_a_margin() {
        let mut b = board();
        b.warm_start = Duration::from_micros(170);
        let (radio, cmds, _) = booted(b);
        let r = radio.restart();
        assert_eq!(r.delay, Duration::from_micros(250));
        assert!(r.delay < radio.cold().delay);
        let last = r.last.expect("probed");
        assert!(last.ok);
        assert!(
            last.gap.is_some_and(|g| g < tcxo::STILL_RUNNING),
            "{last:?}"
        );
        // Each probe a CAD on the next channel and the restart home.
        let cmds = take(&cmds);
        let probe = [Cmd::Freq(3), Cmd::Tcxo(200), Cmd::Rx, Cmd::Errors];
        assert!(cmds.windows(4).any(|w| w == probe), "{cmds:?}");
        assert!(cmds.contains(&Cmd::Freq(4)));
    }

    /// A CAD's interrupt that reaches the driver late: timed from the CAD's
    /// end, the return is cold, where timed from the interrupt it would
    /// have restarted with the short delay and failed.
    #[test]
    fn a_restart_after_a_late_interrupt_is_taken_as_cold() {
        let mut b = board();
        b.osc_start = Duration::from_micros(400);
        let (mut radio, cmds, _) = booted(b);
        radio.chip().board.borrow_mut().irq_late = Duration::from_micros(900);
        run(radio.lbt(ch(5))).unwrap();
        take(&cmds);
        run(radio.listen(ch(3))).unwrap();
        assert_eq!(
            take(&cmds),
            [Cmd::Freq(3), Cmd::Tcxo(750), Cmd::Rx, Cmd::Errors]
        );
        assert!(
            matches!(radio.last_start(), Start::Cold { gap: Some(g), .. } if g >= tcxo::STILL_RUNNING)
        );
        assert_eq!((radio.cold().errors, radio.restart().errors), (0, 0));
        // On time, the same return is a restart.
        radio.chip().board.borrow_mut().irq_late = Duration::ZERO;
        run(radio.lbt(ch(5))).unwrap();
        run(radio.listen(ch(3))).unwrap();
        assert!(matches!(radio.last_start(), Start::Restart { .. }));
    }

    #[test]
    fn a_failed_self_check_reads_after_the_cad() {
        let mut b = board();
        // The chip answers nothing inside a CAD.
        b.inside_dbm = NO_READING_DBM;
        let (mut radio, cmds, _) = booted(b);
        assert_eq!(radio.lbt_read(), LbtRead::After);
        take(&cmds);
        let t = run(radio.lbt(ch(5))).unwrap();
        assert_eq!((t.inside_dbm, t.dbm), (None, -79));
        assert!(t.after > RSSI_SETTLE);
        assert_eq!(radio.held().mode, Mode::Rx);
        assert_eq!(radio.held().freq, Some(ch(5)));
        // An offset beyond the limit fails it too.
        let mut b = board();
        b.inside_dbm = -50;
        let (radio, _, _) = booted(b);
        assert_eq!(radio.lbt_read(), LbtRead::After);
    }

    /// The cycle is derived for the least delay a probe can land on; a
    /// board whose delay settled higher sleeps that much less.
    #[test]
    fn a_sniff_on_a_cold_board_takes_the_delay_off_its_sleep() {
        assert_eq!(tcxo::MIN + MARGIN, bark_proto::sniff::SNIFF_TCXO_START);
        assert_eq!(
            STEP.as_micros() % bark_proto::sniff::SNIFF_STEP.as_micros(),
            0
        );
        let mut b = board();
        b.osc_start = Duration::from_micros(400);
        let (mut radio, cmds, _) = booted(b);
        assert_eq!(radio.cold().delay, Duration::from_micros(750));
        take(&cmds);
        run(radio.sniff(ch(3))).unwrap();
        assert_eq!(
            take(&cmds),
            [
                Cmd::Standby(false),
                Cmd::Tcxo(750),
                Cmd::RxDutyCycle(3_875, 233_375)
            ]
        );
    }

    #[test]
    fn a_reprobe_comes_after_the_floor_when_the_temperature_has_moved() {
        let mut b = board();
        b.osc_start = Duration::from_micros(400);
        let (mut radio, cmds, clock) = booted(b);
        assert_eq!(radio.cold().delay, Duration::from_micros(750));
        take(&cmds);
        let probes = radio.cold().probes;
        // The hour passes; the temperature has not moved.
        let later = clock.now() + tcxo::REPROBE_FLOOR;
        run(clock.at(later));
        run(radio.listen(ch(4))).unwrap();
        assert_eq!(radio.cold().probes, probes);
        // Warmed by the threshold and faster: the probe two steps below
        // passes, and the delay comes down one.
        radio.chip().board.borrow_mut().temp = Some(100 + tcxo::REPROBE_TEMP_QUARTERS);
        radio.chip().board.borrow_mut().osc_start = Duration::from_micros(200);
        let later = clock.now() + tcxo::TEMP_EVERY;
        run(clock.at(later));
        take(&cmds);
        run(radio.listen(ch(5))).unwrap();
        assert_eq!(radio.cold().probes, probes + u32::from(CONFIRM));
        assert_eq!(radio.cold().delay, tcxo::MIN + MARGIN);
        let cmds = take(&cmds);
        assert!(cmds.contains(&Cmd::Tcxo(tcxo::MIN.as_micros())));
        // Each probe waited the oscillator off for the cold wait.
        assert!(radio.cold().last.unwrap().at.since(later) >= COLD_WAIT);
    }

    /// Pushed up by unlucky starts at a steady temperature, both delays
    /// come back down a step once a day has passed without an error, and
    /// so does a board that reads no temperature.
    #[test]
    fn a_day_without_an_error_brings_both_delays_down_a_step() {
        for temp in [Some(100), None] {
            let mut b = board();
            b.temp = temp;
            let (mut radio, _, clock) = booted(b);
            // One unlucky start of each kind.
            radio.chip().board.borrow_mut().warm_start = Duration::from_micros(120);
            run(radio.lbt(ch(5))).unwrap();
            run(radio.listen(ch(3))).unwrap();
            radio.chip().board.borrow_mut().warm_start = Duration::ZERO;
            radio.chip().board.borrow_mut().osc_start = Duration::from_micros(600);
            run(radio.lbt(ch(5))).unwrap();
            run(clock.at(clock.now() + Duration::from_millis(3)));
            run(radio.listen(ch(3))).unwrap();
            radio.chip().board.borrow_mut().osc_start = Duration::from_micros(200);
            assert_eq!(radio.cold().delay, Duration::from_micros(750));
            assert_eq!(radio.restart().delay, Duration::from_micros(150));
            // An hour and more at the same temperature: nothing.
            let (cold, restart) = (radio.cold().probes, radio.restart().probes);
            run(clock.at(clock.now() + tcxo::REPROBE_FLOOR + tcxo::TEMP_EVERY));
            run(radio.listen(ch(4))).unwrap();
            assert_eq!(
                (radio.cold().probes, radio.restart().probes),
                (cold, restart)
            );
            // A day: each probed two steps down, which passes, and each
            // one step lower.
            run(clock.at(clock.now() + tcxo::REPROBE_AFTER));
            run(radio.listen(ch(5))).unwrap();
            assert_eq!(radio.cold().probes, cold + u32::from(CONFIRM), "{temp:?}");
            assert_eq!(
                radio.restart().probes,
                restart + u32::from(CONFIRM),
                "{temp:?}"
            );
            assert_eq!(radio.cold().delay, Duration::from_micros(500));
            assert_eq!(radio.restart().delay, Duration::from_micros(100));
            assert_eq!(radio.held().freq, Some(ch(5)));
            assert_eq!(radio.held().mode, Mode::Rx);
        }
    }
}
