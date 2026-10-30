//! The SX1262 driven with only the commands that change (roadmap D88).
//!
//! [`Chip`] is the chip one command at a time, with the pin waits each
//! needs; the firmware implements it over the SPI bus. [`Sx1262`] keeps
//! what the chip holds ([`Held`]) and sends an operation only what it
//! changes: the settings that never change go once, a retune is standby,
//! frequency and receive, and a listen-before-talk is the CAD's
//! preparation and sampling with the RSSI read inside it. After a reset,
//! a sleep, an error or a cold start the chip may not hold what was sent,
//! so the whole configuration goes again. The oscillator's delay is
//! probed and kept by [`crate::tcxo`]. The tests at the end run the
//! sequences against a chip that records them.
//!
//! "DS" is the SX1261/2 datasheet, Rev 1.2 (June 2019).

use core::fmt::Debug;

use bark_proto::radio::{time_on_air, Channel, RSSI_SETTLE, SYMBOL_TIME};
use bark_proto::sniff::{LONG_PREAMBLE_SYMBOLS, SNIFF_RX, SNIFF_SLEEP};
use bark_proto::{Frame, MAX_FRAME};

use crate::tcxo::{self, Probe, Search};
use crate::{Clock, Duration, Heard, Instant};

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
    /// Every flag an operation ends with or reports a packet by: set once
    /// for receive, CAD and transmit alike, which saves a command at
    /// every change of mode (DS §13.3.1 SetDioIrqParams).
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

/// Symbols a CAD samples (DS Table 13-72, `CADSymbols::_8`).
pub const CAD_SYMBOLS: u16 = 8;

/// The CAD's sampling: its symbols. The RSSI read inside it must start
/// before this, while the receiver is up (`docs/bench-2026-10-30.md`:
/// read at 2.0 ms the CAD had ended).
pub const CAD_SAMPLING: Duration = SYMBOL_TIME.mul(CAD_SYMBOLS as u64);

/// A CAD from its start to its interrupt: its symbols and about half a
/// symbol of processing (DS §6.1.5), rounded up to a whole one.
pub const CAD_TIME: Duration = SYMBOL_TIME.mul(CAD_SYMBOLS as u64 + 1);

/// Interrupt latency allowed beyond an operation's own duration: twice
/// the longest oscillator start the search allows plus the 126 µs switch
/// to TX (DS Table 8-2), so a chip that lost its oscillator is slow
/// rather than an error.
pub const IRQ_SLACK: Duration = Duration::from_micros(2 * (tcxo::MAX.as_micros() + 126));

/// A reading at or below this is no reading: the receiver was not up
/// (`docs/bench-2026-10-30.md`, "−128 until a dwell of about 145 µs").
/// The scale's bottom, −raw/2 with raw 255, is −127.5.
pub const NO_READING_DBM: i16 = -127;

/// Pairs of readings the self-check takes at boot, a reading inside a
/// CAD against one while listening, on the home channel: enough for a
/// median to shrug off one pair a frame fell in.
pub const SELF_CHECK_PAIRS: usize = 5;

/// The largest offset the self-check accepts between a reading inside a
/// CAD and one while listening, dB: the bench saw the two agree within
/// 2 dB on a quiet channel and 3 to 7 dB apart under a signal; twice the
/// largest. A chip that answers `GetRssiInst` with something else during
/// a CAD, which the datasheet documents for receive only, fails the check
/// and the reading is taken after the CAD instead.
pub const SELF_CHECK_LIMIT_DB: i16 = 12;

/// The receiver's packet parameters: the longest preamble a frame has,
/// which DS §6.1.4 asks for where the length varies, and the longest
/// payload (`docs/core-interface.md` §3 rule 4).
pub const RX_PACKET: (u16, u8) = (LONG_PREAMBLE_SYMBOLS, MAX_FRAME as u8);

/// The chip, one command at a time. Every method sends one command and
/// waits for BUSY to fall, unless its comment says more.
pub trait Chip {
    /// A failure of the bus, a pin wait or an interrupt wait.
    type Error: Debug;

    /// Pulse NRESET and wait until the chip takes commands: STDBY_RC,
    /// everything it held lost (DS §9.1).
    async fn reset(&mut self) -> Result<(), Self::Error>;

    /// Wake a chip that may be asleep in a duty-cycled receive with a
    /// harmless command on a falling NSS edge (DS §13.1.7); the chip is
    /// then in STDBY_RC.
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
    /// errata write, the IQ errata write, the CAD parameters, the
    /// interrupt mask [`irq::ALL`] on DIO1, the symbol timeout and the
    /// receiver gain. From a standby.
    async fn configure(&mut self) -> Result<(), Self::Error>;

    /// `SetDIO3AsTCXOCtrl` with the board's voltage and `delay`, rounded
    /// up to the chip's steps (DS §13.3.6). From STDBY_RC.
    async fn set_tcxo(&mut self, delay: Duration) -> Result<(), Self::Error>;

    /// `GetDeviceErrors` (DS §13.5.6).
    async fn errors(&mut self) -> Result<u16, Self::Error>;

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
    /// When the oscillator last stopped: at a CAD's end or at a standby
    /// with it off. `None` while it runs.
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
    /// Resets after an error, the one at boot not counted.
    pub resets: u32,
    /// Times the whole configuration was sent.
    pub configured: u32,
    /// Starts from a stopped oscillator with the probed delay.
    pub cold_starts: u32,
}

/// The sequencer: a chip, a clock, and what the chip holds.
pub struct Sx1262<C, K> {
    chip: C,
    clock: K,
    held: Held,
    tcxo: tcxo::Delay,
    lbt: LbtRead,
    stats: Stats,
    /// The flags the last `service` read.
    flags: u16,
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
            tcxo: tcxo::Delay::new(now),
            lbt: LbtRead::After,
            stats: Stats::default(),
            flags: 0,
        };
        radio.ready().await?;
        radio.stats.resets = 0;
        radio.set_frequency(channel).await?;
        radio.search(Search::at_boot()).await?;
        radio.self_check(channel).await?;
        radio.listen(channel).await?;
        Ok(radio)
    }

    /// The chip.
    pub fn chip(&mut self) -> &mut C {
        &mut self.chip
    }

    /// What the chip holds.
    pub fn held(&self) -> &Held {
        &self.held
    }

    /// The oscillator delay and its history.
    pub fn tcxo(&self) -> &tcxo::Delay {
        &self.tcxo
    }

    /// Where a listen-before-talk reads the RSSI.
    pub fn lbt_read(&self) -> LbtRead {
        self.lbt
    }

    /// Counters.
    pub fn stats(&self) -> Stats {
        self.stats
    }

    /// The interrupt flags the last [`Sx1262::service`] read, for a log
    /// that tells a header failure from a payload one.
    pub fn last_flags(&self) -> u16 {
        self.flags
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
                let r = self.chip.reset().await;
                self.note(r)?;
                self.stats.resets += 1;
                // Cold: the reset cut the oscillator's supply.
                self.held = Held {
                    mode: Mode::StandbyRc,
                    osc_stopped: None,
                    ..Held::UNKNOWN
                };
                // With a TCXO the flag is set at power-up, before the
                // chip knows its clock (DS §13.3.6).
                let r = self.chip.clear_errors().await;
                self.note(r)?;
                self.set_tcxo(self.tcxo.delay).await?;
                let r = self.chip.calibrate().await;
                self.note(r)?;
                let r = self.chip.errors().await;
                let errors = self.note(r)?;
                if errors != 0 {
                    log::warn!("radio: calibration errors {:#x}", errors);
                    let r = self.chip.clear_errors().await;
                    self.note(r)?;
                }
            }
            Mode::Sniff => {
                // Asleep in the cycle the chip takes no command until a
                // falling NSS edge wakes it; one that comes as it goes to
                // sleep may be lost, and then BUSY stays high for the
                // sleep, so a failure is tried once more.
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
            self.start_errors().await?;
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

    /// Into STDBY_RC, the oscillator stopped.
    async fn standby_rc(&mut self) -> Result<(), Error<C::Error>> {
        if matches!(self.held.mode, Mode::Rx | Mode::StandbyXosc) {
            let r = self.chip.standby(false).await;
            self.note(r)?;
            self.held.mode = Mode::StandbyRc;
            self.held.osc_stopped = Some(self.clock.now());
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
    /// the delay for it, short if it stopped within [`tcxo::STILL_RUNNING`]
    /// and the probed one for a cold start. Returns whether the start is
    /// cold. From any other standby the oscillator runs on.
    async fn before_start(&mut self) -> Result<bool, Error<C::Error>> {
        if self.held.mode != Mode::StandbyRc {
            return Ok(false);
        }
        let now = self.clock.now();
        let cold = self
            .held
            .osc_stopped
            .is_none_or(|t| now.since(t) >= tcxo::STILL_RUNNING);
        let delay = if cold { self.tcxo.delay } else { tcxo::SHORT };
        self.set_tcxo(delay).await?;
        self.held.osc_stopped = None;
        Ok(cold)
    }

    /// After a cold start: the error flag, and a step up if it is set.
    async fn after_start(&mut self, cold: bool) -> Result<(), Error<C::Error>> {
        if cold {
            self.stats.cold_starts += 1;
            self.start_errors().await?;
        }
        Ok(())
    }

    async fn start_errors(&mut self) -> Result<(), Error<C::Error>> {
        let r = self.chip.errors().await;
        let errors = self.note(r)?;
        if errors & XOSC_START_ERR != 0 {
            let temp = self.chip.die_temperature().await;
            self.tcxo.step_up(self.clock.now(), temp);
            log::warn!(
                "tcxo: start error at {} us, now {} us",
                self.held.tcxo.map_or(0, |d| d.as_micros()),
                self.tcxo.delay.as_micros()
            );
        }
        if errors != 0 {
            let r = self.chip.clear_errors().await;
            self.note(r)?;
        }
        Ok(())
    }

    /// Listen on `channel` from any state, sending only what differs
    /// from what the chip holds.
    pub async fn listen(&mut self, channel: Channel) -> Result<(), Error<C::Error>> {
        self.ready().await?;
        if self.held.mode == Mode::Rx && self.held.freq == Some(channel) {
            return Ok(());
        }
        self.reprobe_if_due().await?;
        self.standby_xosc().await?;
        self.set_frequency(channel).await?;
        self.set_packet_params(RX_PACKET).await?;
        let cold = self.before_start().await?;
        let r = self.chip.rx().await;
        self.note(r)?;
        self.held.mode = Mode::Rx;
        self.after_start(cold).await
    }

    /// Receive duty-cycled on `channel` from any state (DS §13.1.7). The
    /// cycle's wakes are cold starts, so they take the probed delay.
    pub async fn sniff(&mut self, channel: Channel) -> Result<(), Error<C::Error>> {
        self.ready().await?;
        self.standby_rc().await?;
        self.set_frequency(channel).await?;
        self.set_packet_params(RX_PACKET).await?;
        self.set_tcxo(self.tcxo.delay).await?;
        let r = self.chip.rx_duty_cycle(SNIFF_RX, SNIFF_SLEEP).await;
        self.note(r)?;
        self.held.mode = Mode::Sniff;
        self.held.slept = true;
        self.held.osc_stopped = None;
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
        self.flags = flags;
        let r = self.chip.clear_irq().await;
        self.note(r)?;
        let failed = irq::HEADER_ERR | irq::CRC_ERR | irq::TIMEOUT;
        if self.held.mode == Mode::Sniff && flags & (irq::RX_DONE | failed) != 0 {
            // Asleep between its windows, so its next start is cold.
            self.held.mode = Mode::StandbyRc;
            self.held.osc_stopped = None;
        }
        if flags & failed != 0 {
            return Ok(Heard::Failed);
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

    /// One CAD on `channel` with the RSSI read inside it: whether a
    /// preamble was seen, and the reading as read, `None` if the read
    /// came too late to be one. Leaves the chip in STDBY_RC.
    async fn cad(
        &mut self,
        channel: Channel,
    ) -> Result<(bool, Option<i16>, [Duration; 3]), Error<C::Error>> {
        let t0 = self.clock.now();
        self.ready().await?;
        self.standby_xosc().await?;
        self.set_frequency(channel).await?;
        let cold = self.before_start().await?;
        let r = self.chip.cad().await;
        self.note(r)?;
        self.held.mode = Mode::Cad;
        let started = self.clock.now();
        self.clock.at(started + RSSI_SETTLE).await;
        let t_read = self.clock.now();
        let inside = if t_read <= started + CAD_SAMPLING {
            let r = self.chip.rssi().await;
            Some(self.note(r)?)
        } else {
            None
        };
        let read = self.clock.now().since(t_read);
        let r = self.chip.await_irq(Some(CAD_TIME + IRQ_SLACK)).await;
        self.note(r)?;
        let r = self.chip.irq_status().await;
        let flags = self.note(r)?;
        let r = self.chip.clear_irq().await;
        self.note(r)?;
        let ended = self.clock.now();
        self.held.mode = Mode::StandbyRc;
        self.held.osc_stopped = Some(ended);
        if flags & irq::CAD_DONE == 0 {
            self.held = Held::UNKNOWN;
            return Err(Error::NotDone(flags));
        }
        self.after_start(cold).await?;
        let busy = flags & irq::CAD_DETECTED != 0;
        let phases = [started.since(t0), read, ended.since(started)];
        Ok((busy, inside.filter(|&d| d > NO_READING_DBM), phases))
    }

    /// Listen before talking on `channel`: the CAD and the reading, on
    /// the footing of a reading while listening. Leaves the chip in
    /// standby, or listening on `channel` if the reading is taken after
    /// the CAD.
    pub async fn lbt(&mut self, channel: Channel) -> Result<LbtTiming, Error<C::Error>> {
        let (busy, inside, [prepare, read, cad]) = self.cad(channel).await?;
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

    /// Send `frame` at `dbm` conducted. Leaves the chip in STDBY_XOSC.
    pub async fn transmit(&mut self, frame: &Frame, dbm: i8) -> Result<TxTiming, Error<C::Error>> {
        let t0 = self.clock.now();
        self.ready().await?;
        self.standby_xosc().await?;
        self.set_frequency(frame.channel).await?;
        self.set_packet_params((frame.preamble_symbols, frame.len))
            .await?;
        self.set_tx_power(dbm).await?;
        let r = self.chip.write_payload(frame.payload()).await;
        self.note(r)?;
        let cold = self.before_start().await?;
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
        self.after_start(cold).await?;
        Ok(TxTiming {
            prepare: started.since(t0),
            tx: self.clock.now().since(started),
        })
    }

    /// One probe of the oscillator's start at `delay`: a standby with the
    /// oscillator off for [`tcxo::COLD_WAIT`], the delay programmed, the
    /// errors cleared, a receive, and the error word read.
    async fn probe(&mut self, delay: Duration) -> Result<bool, Error<C::Error>> {
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
        let errors = self.note(r)?;
        let ok = errors & XOSC_START_ERR == 0;
        let r = self.chip.clear_errors().await;
        self.note(r)?;
        self.standby_rc().await?;
        let temp = self.chip.die_temperature().await;
        self.tcxo.probed(Probe {
            delay,
            ok,
            at: self.clock.now(),
            temp,
        });
        Ok(ok)
    }

    /// Run `search` to its outcome and settle the delay on it.
    async fn search(&mut self, mut search: Search) -> Result<(), Error<C::Error>> {
        while let Some(delay) = search.next() {
            let ok = self.probe(delay).await?;
            search.result(ok);
        }
        let lowest = search.outcome().expect("the search is over");
        let temp = self.tcxo.last.and_then(|p| p.temp);
        self.tcxo.settle(lowest, self.clock.now(), temp);
        log::info!(
            "tcxo: delay {} us after {} probes, lowest confirmed {:?} us",
            self.tcxo.delay.as_micros(),
            self.tcxo.probes,
            lowest.map(|d| d.as_micros())
        );
        Ok(())
    }

    /// A probe below the delay in use, when the die's temperature has
    /// moved since it settled (`crate::tcxo`).
    async fn reprobe_if_due(&mut self) -> Result<(), Error<C::Error>> {
        let now = self.clock.now();
        if !self.tcxo.temp_due(now) {
            return Ok(());
        }
        let Some(temp) = self.chip.die_temperature().await else {
            return Ok(());
        };
        if self.tcxo.probe_due(now, temp) {
            self.reprobe().await?;
        }
        Ok(())
    }

    /// The probe below the delay in use, now: at two steps down, so that
    /// a pass lowers the delay by one and keeps the margin.
    pub async fn reprobe(&mut self) -> Result<(), Error<C::Error>> {
        let below = self.tcxo.probe_below().unwrap_or(tcxo::MIN);
        self.search(Search::confirm(below)).await
    }

    /// Check the reading inside a CAD against one while listening on
    /// `channel`, in pairs, and decide where a listen-before-talk reads
    /// from (`SELF_CHECK_PAIRS`, `SELF_CHECK_LIMIT_DB`).
    async fn self_check(&mut self, channel: Channel) -> Result<(), Error<C::Error>> {
        let mut diffs = [0i16; SELF_CHECK_PAIRS];
        let mut pairs = 0;
        for diff in diffs.iter_mut() {
            self.listen(channel).await?;
            self.clock.at(self.clock.now() + RSSI_SETTLE).await;
            let listening = self.rssi().await?;
            let (_, inside, _) = self.cad(channel).await?;
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tcxo::{COLD_WAIT, CONFIRM, MARGIN, SHORT, STEP};
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
        Tcxo(u64),
        Errors,
        ClearErrors,
        Freq(u8),
        Packet(u16, u8),
        Power(i8),
        Payload(usize),
        Rx,
        RxDutyCycle,
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
        /// The flags the next `GetIrqStatus` reports, if not the
        /// operation's own.
        flags: Option<u16>,
        /// Fail the next command.
        fail_next: bool,
        temp: Option<i16>,
    }

    #[derive(Clone)]
    struct Fake {
        cmds: Rc<RefCell<Vec<Cmd>>>,
        board: Rc<RefCell<Board>>,
        clock: FakeClock,
        /// What the chip itself holds: the programmed delay, its mode,
        /// its sticky error word.
        delay: Duration,
        mode: Mode,
        errors: u16,
        osc_stopped_at: Option<Instant>,
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
            };
            (fake, cmds, clock)
        }

        fn cmd(&mut self, c: Cmd) -> Result<(), &'static str> {
            self.cmds.borrow_mut().push(c);
            if std::mem::take(&mut self.board.borrow_mut().fail_next) {
                return Err("bus");
            }
            // A command takes about 50 µs on the bus.
            let now = self.clock.now();
            *self.clock.0.borrow_mut() = now + Duration::from_micros(50);
            Ok(())
        }

        /// The oscillator starts from STDBY_RC: still running within a
        /// millisecond, else cold, and an error if the delay is short.
        fn start_osc(&mut self) {
            if self.mode != Mode::StandbyRc {
                return;
            }
            let now = self.clock.now();
            let running = self
                .osc_stopped_at
                .is_some_and(|t| now.since(t) < tcxo::STILL_RUNNING);
            if !running && self.delay < self.board.borrow().osc_start {
                self.errors |= XOSC_START_ERR;
            }
            *self.clock.0.borrow_mut() = now + self.delay;
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
            if !xosc {
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
        async fn set_tcxo(&mut self, delay: Duration) -> Result<(), &'static str> {
            assert_eq!(self.mode, Mode::StandbyRc, "SetDIO3AsTCXOCtrl in STDBY_RC");
            self.cmd(Cmd::Tcxo(delay.as_micros()))?;
            assert!(delay > Duration::ZERO, "a delay of zero hangs the chip");
            self.delay = delay;
            Ok(())
        }
        async fn errors(&mut self) -> Result<u16, &'static str> {
            self.cmd(Cmd::Errors)?;
            Ok(self.errors)
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
            self.start_osc();
            self.mode = Mode::Rx;
            Ok(())
        }
        async fn rx_duty_cycle(&mut self, _: Duration, _: Duration) -> Result<(), &'static str> {
            assert_eq!(self.mode, Mode::StandbyRc, "SetRxDutyCycle from STDBY_RC");
            self.cmd(Cmd::RxDutyCycle)?;
            self.start_osc();
            self.mode = Mode::Sniff;
            Ok(())
        }
        async fn cad(&mut self) -> Result<(), &'static str> {
            self.cmd(Cmd::Cad)?;
            self.start_osc();
            self.mode = Mode::Cad;
            Ok(())
        }
        async fn tx(&mut self) -> Result<(), &'static str> {
            self.cmd(Cmd::Tx)?;
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
            Ok(match self.mode {
                Mode::Cad => {
                    irq::CAD_DONE
                        | if self.board.borrow().detected {
                            irq::CAD_DETECTED
                        } else {
                            0
                        }
                }
                Mode::Tx => irq::TX_DONE,
                _ => 0,
            })
        }
        async fn clear_irq(&mut self) -> Result<(), &'static str> {
            self.cmd(Cmd::ClearIrq)?;
            match self.mode {
                Mode::Cad => {
                    self.mode = Mode::StandbyRc;
                    self.osc_stopped_at = Some(self.clock.now());
                }
                Mode::Tx => self.mode = Mode::StandbyXosc,
                _ => {}
            }
            Ok(())
        }
        async fn await_irq(&mut self, timeout: Option<Duration>) -> Result<(), &'static str> {
            self.cmd(Cmd::AwaitIrq)?;
            let took = match self.mode {
                Mode::Cad => CAD_TIME,
                Mode::Tx => Duration::from_millis(15),
                _ => Duration::ZERO,
            };
            if timeout.is_some_and(|t| took > t) {
                return Err("dio1 timeout");
            }
            let now = self.clock.now();
            *self.clock.0.borrow_mut() = now + took;
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

    #[test]
    fn boot_resets_configures_calibrates_probes_and_checks() {
        let (radio, cmds, _) = booted(board());
        let cmds = take(&cmds);
        // The reset path: errors cleared, the longest delay for the
        // calibration, the configuration once, the image calibration.
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
        // The search landed one step over the lowest confirmed step.
        let t = radio.tcxo();
        assert_eq!(t.delay, Duration::from_micros(500));
        assert_eq!(t.probes as usize, 4 + CONFIRM as usize);
        // Each probe: the delay, clear, receive, the errors read.
        let probe = [Cmd::ClearErrors, Cmd::Rx, Cmd::Errors, Cmd::ClearErrors];
        let probes = cmds.windows(4).filter(|w| w == &probe).count();
        assert_eq!(probes, t.probes as usize);
        // The self-check took its pairs and chose the reading inside.
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
                Cmd::Freq(5),
                Cmd::Cad,
                Cmd::Rssi,
                Cmd::AwaitIrq,
                Cmd::IrqStatus,
                Cmd::ClearIrq,
            ]
        );
        assert_eq!(radio.held().mode, Mode::StandbyRc);
        // Back home from the standby a CAD ends in: the frequency and
        // receive; no error read, the oscillator still ran; the short
        // delay was programmed at the self-check's last CAD and holds.
        run(radio.listen(ch(3))).unwrap();
        assert_eq!(take(&cmds), [Cmd::Freq(3), Cmd::Rx]);
        assert_eq!(radio.held().tcxo, Some(SHORT));
        run(radio.lbt(ch(5))).unwrap();
        run(radio.listen(ch(3))).unwrap();
        let cmds = take(&cmds);
        assert!(!cmds.iter().any(|c| matches!(c, Cmd::Tcxo(_))), "{cmds:?}");
        assert_eq!(radio.stats().cold_starts, 0);
        assert_eq!(radio.tcxo().delay, Duration::from_micros(500));
    }

    #[test]
    fn a_long_gap_after_a_cad_is_a_cold_start_with_the_probed_delay() {
        let (mut radio, cmds, clock) = booted(board());
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
        assert_eq!(radio.stats().cold_starts, 1);
    }

    #[test]
    fn a_cold_start_error_in_use_steps_the_delay_up() {
        let (mut radio, cmds, clock) = booted(board());
        take(&cmds);
        // The board got cold: its oscillator now takes longer.
        radio.chip().board.borrow_mut().osc_start = Duration::from_micros(900);
        run(radio.lbt(ch(5))).unwrap();
        let now = clock.now();
        run(clock.at(now + Duration::from_millis(3)));
        run(radio.listen(ch(3))).unwrap();
        assert_eq!(
            radio.tcxo().delay,
            Duration::from_micros(500 + STEP.as_micros())
        );
        assert_eq!(radio.tcxo().errors, 1);
        assert!(take(&cmds).ends_with(&[Cmd::Rx, Cmd::Errors, Cmd::ClearErrors]));
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
            take(&cmds)[..4],
            [
                Cmd::Standby(true),
                Cmd::Freq(4),
                Cmd::Packet(MIN_PREAMBLE_SYMBOLS, 13),
                Cmd::Payload(13),
            ]
        );
    }

    #[test]
    fn a_sniff_takes_the_probed_delay_and_everything_is_sent_again_after_it() {
        let (mut radio, cmds, _) = booted(board());
        take(&cmds);
        run(radio.sniff(ch(3))).unwrap();
        assert_eq!(
            take(&cmds),
            [Cmd::Standby(false), Cmd::Tcxo(500), Cmd::RxDutyCycle]
        );
        // A packet ends the cycle in STDBY_RC.
        radio.chip().board.borrow_mut().flags = Some(irq::RX_DONE);
        let mut buf = [0; MAX_FRAME];
        let heard = run(radio.service(&mut buf)).unwrap();
        assert!(matches!(heard, Heard::Frame { len: 1, .. }));
        assert_eq!(radio.held().mode, Mode::StandbyRc);
        take(&cmds);
        // Listening again: the cycle's cold starts checked, the whole
        // configuration again, then everything tracked, then receive.
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
    fn an_error_resets_the_chip_before_its_next_operation() {
        let (mut radio, cmds, _) = booted(board());
        take(&cmds);
        radio.chip().board.borrow_mut().fail_next = true;
        assert!(run(radio.lbt(ch(5))).is_err());
        assert_eq!(radio.held().mode, Mode::Unknown);
        take(&cmds);
        run(radio.listen(ch(3))).unwrap();
        let cmds = take(&cmds);
        assert_eq!(
            &cmds[..7],
            &[
                Cmd::Reset,
                Cmd::ClearErrors,
                Cmd::Tcxo(500),
                Cmd::Calibrate,
                Cmd::Errors,
                Cmd::Configure,
                Cmd::CalibrateImage,
            ]
        );
        assert!(cmds.ends_with(&[Cmd::Rx, Cmd::Errors]));
        assert_eq!(radio.stats().resets, 1);
        // An interrupt without its done flag is an error too.
        radio.chip().board.borrow_mut().flags = Some(0);
        assert_eq!(run(radio.lbt(ch(5))), Err(Error::NotDone(0)));
        assert_eq!(radio.held().mode, Mode::Unknown);
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

    #[test]
    fn a_reprobe_comes_after_the_floor_when_the_temperature_has_moved() {
        // A board that booted cold: its oscillator took 400 µs, so the
        // delay settled at 750.
        let mut b = board();
        b.osc_start = Duration::from_micros(400);
        let (mut radio, cmds, clock) = booted(b);
        assert_eq!(radio.tcxo().delay, Duration::from_micros(750));
        take(&cmds);
        let probes = radio.tcxo().probes;
        // The hour passes; the temperature has not moved.
        let later = clock.now() + tcxo::REPROBE_FLOOR;
        run(clock.at(later));
        run(radio.listen(ch(4))).unwrap();
        assert_eq!(radio.tcxo().probes, probes);
        // It warms by the threshold and its oscillator starts faster: the
        // probe two steps below passes, and the delay comes down one.
        radio.chip().board.borrow_mut().temp = Some(100 + tcxo::REPROBE_TEMP_QUARTERS);
        radio.chip().board.borrow_mut().osc_start = Duration::from_micros(200);
        let later = clock.now() + tcxo::TEMP_EVERY;
        run(clock.at(later));
        take(&cmds);
        run(radio.listen(ch(5))).unwrap();
        assert_eq!(radio.tcxo().probes, probes + u32::from(CONFIRM));
        assert_eq!(radio.tcxo().delay, tcxo::MIN + MARGIN);
        let cmds = take(&cmds);
        assert!(cmds.contains(&Cmd::Tcxo(tcxo::MIN.as_micros())));
        // Each probe waited the oscillator off for the cold wait.
        assert!(radio.tcxo().last.unwrap().at.since(later) >= COLD_WAIT);
    }
}
