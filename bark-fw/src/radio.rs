//! The SX1262: power-up, the pins the driver drives, and bark's modulation
//! (spec "Physical layer") on top of `lora-phy`.
//!
//! "DS" is the SX1261/2 datasheet, Rev 1.2 (June 2019).

use core::cell::Cell;

use bark_driver::Heard;
use bark_proto::radio::{
    time_on_air, Channel, BANDWIDTH_HZ, CHANNEL_0_HZ, CODING_RATE_DENOMINATOR, MAX_ERP_DBM,
    RSSI_SETTLE, SPREADING_FACTOR, SYMBOL_TIME,
};
use bark_proto::sniff::{LONG_PREAMBLE_SYMBOLS, SNIFF_RX, SNIFF_SLEEP};
use bark_proto::{Frame, MAX_FRAME};
use embassy_nrf::gpio::{Input, Level, Output, OutputDrive, Pull};
use embassy_nrf::interrupt::typelevel::{Binding, SPI2};
use embassy_nrf::peripherals;
use embassy_nrf::spim::{self, Spim};
use embassy_nrf::Peri;
use embassy_time::{with_deadline, with_timeout, Delay, Duration, Instant, Timer};
use embedded_hal_bus::spi::ExclusiveDevice;
use lora_phy::mod_params::{
    Bandwidth, CodingRate, DutyCycleParams, ModulationParams, PacketParams, RadioError,
    SpreadingFactor,
};
use lora_phy::mod_traits::{InterfaceVariant, IrqState};
use lora_phy::sx126x::{self, Sx126x, TcxoCtrlVoltage};
use lora_phy::{DelayNs, LoRa, RxMode};

use crate::board;
use crate::clock::Clock;

// bark's modulation is a compile-time choice of the core; the driver wants
// enum values, so tie the two together here.
const _: () = assert!(
    SPREADING_FACTOR == 5 && BANDWIDTH_HZ == 125_000 && CODING_RATE_DENOMINATOR == 5,
    "radio.rs maps bark_proto::radio's SF/BW/CR to lora-phy enums by hand"
);
const SF: SpreadingFactor = SpreadingFactor::_5;
const BW: Bandwidth = Bandwidth::_125KHz;
const CR: CodingRate = CodingRate::_4_5;

/// Largest LoRa payload, in bytes.
pub const MAX_LEN: usize = MAX_FRAME;

/// Antenna gain over a half-wave dipole, dBd. ERP = conducted power + gain
/// in dBd. Assumption: the stock 433 MHz antenna is no better than a dipole.
pub const ANTENNA_GAIN_DBD: i8 = 0;

/// Highest conducted power the console allows: the regulatory ERP limit
/// (ERC/REC 70-03 band g1, 10 mW; spec "Regulatory limits") less antenna gain.
pub const MAX_TX_DBM: i8 = MAX_ERP_DBM - ANTENNA_GAIN_DBD;

/// Lowest power the SX1262 high-power PA can be set to (DS §13.4.4 SetTxParams).
pub const MIN_TX_DBM: i8 = -9;

/// Time on air of the longest frame, with the long preamble.
#[cfg(feature = "console")]
pub const MAX_AIRTIME: Duration =
    Duration::from_micros(time_on_air(MAX_LEN as u8, LONG_PREAMBLE_SYMBOLS).as_micros());

/// TCXO start-up the chip waits out, in full, each time it powers the TCXO
/// (SetDIO3AsTCXOCtrl, DS §13.3.6). The radio idles in STDBY_XOSC, so this
/// is paid at power-up, after each CAD, which ends in STDBY_RC (DS Table
/// 13-73), and at each wake of duty-cycled receive. No source names the
/// module's TCXO or its start-up time. With 1 ms, CADs and transmissions
/// worked on two boards at room temperature (docs/bench-2026-10-30.md);
/// twice that leaves a factor two for a cold board or a slower part. A
/// delay too short raises XOSC_START_ERR (DS §13.3.6).
pub const TCXO_START: Duration = Duration::from_millis(2);

/// A duty-cycled receive period in the chip's steps of 15.625 µs, 64 to
/// the millisecond (DS §13.1.7).
const fn steps(d: bark_proto::Duration) -> u32 {
    assert!(
        (d.as_micros() * 64).is_multiple_of(1000),
        "not a whole number of steps"
    );
    (d.as_micros() * 64 / 1000) as u32
}

/// SetRxDutyCycle's receive and sleep periods (`bark_proto::sniff`): 384
/// and 22 344 steps.
const SNIFF: DutyCycleParams = DutyCycleParams {
    rx_time: steps(SNIFF_RX),
    sleep_time: steps(SNIFF_SLEEP),
};

/// Cold start to STDBY_RC, the longest switching time in DS Table 8-2; a
/// full calibration takes as long (DS §13.1.12).
const COLD_START: Duration = Duration::from_micros(3500);

/// Bound on any one BUSY-high stretch: twice the longest the driver can
/// cause, a calibration after a TCXO start. Past it the chip is absent,
/// unpowered or wedged, and the console says so instead of hanging.
const BUSY_TIMEOUT: Duration =
    Duration::from_micros(2 * (COLD_START.as_micros() + TCXO_START.as_micros()));

/// Interrupt latency allowed beyond an operation's own duration. Operations
/// start from STDBY_XOSC; this still allows twice a start from STDBY_RC, a
/// TCXO start plus the 126 µs switch to TX (DS Table 8-2), so a chip that
/// lost its oscillator is slow rather than an error.
pub const IRQ_SLACK: Duration = Duration::from_micros(2 * (TCXO_START.as_micros() + 126));

/// A CAD: its 8 symbols (fixed in the driver) and about half a symbol of
/// processing (DS §6.1.5), rounded up to a whole one.
const CAD_TIME: Duration = Duration::from_micros(9 * SYMBOL_TIME.as_micros());

/// NRESET low time: twice the typical 100 µs of DS §8.1.
const RESET_PULSE_US: u32 = 200;

/// The switched rail's ramp after PWR_EN rises. No source gives it; 10 ms
/// is far beyond a load switch's typical sub-millisecond ramp and costs
/// nothing once per boot. NRESET is held low throughout, and the chip's own
/// start-up is then gated on BUSY, so only the ramp itself is assumed.
pub const RAIL_SETTLE: Duration = Duration::from_millis(10);

/// nRF52840 CPU clock, Hz (nRF52840 Product Specification, CLOCK).
const CPU_HZ: u64 = 64_000_000;

/// BUSY rises at most 600 ns after NSS rises (DS §8.3.1, T_SW). The driver
/// polls BUSY right after each command; polled sooner it can still read the
/// previous low, and the next command would reach a chip that is not ready.
const BUSY_RISE_CYCLES: u32 = (600 * CPU_HZ).div_ceil(1_000_000_000) as u32;

/// The SPI device the driver talks to.
type Spi = ExclusiveDevice<Spim<'static>, Output<'static>, Delay>;

/// The driver at bark's chip and board configuration.
pub type Lora<'a> = LoRa<Sx126x<Spi, Pins<'a>, sx126x::Sx1262>, Delay>;

/// The control pins, as the driver's board interface.
///
/// The generic SX126x one in `lora-phy` waits for BUSY low only after each
/// command, at once, and without a bound; this one waits out BUSY's rise
/// time first and bounds every wait.
pub struct Pins<'a> {
    reset: Output<'static>,
    dio1: Input<'static>,
    busy: Input<'static>,
    /// When set, waiting for DIO1 fails past this instant. An operation
    /// whose interrupt never comes (wrong wiring, TCXO not starting) then
    /// returns an error instead of hanging the console.
    irq_deadline: &'a Cell<Option<Instant>>,
}

impl Pins<'_> {
    async fn wait_ready(&mut self, timeout: Duration) -> Result<(), RadioError> {
        cortex_m::asm::delay(BUSY_RISE_CYCLES);
        with_timeout(timeout, self.busy.wait_for_low())
            .await
            .map_err(|_| RadioError::Busy)
    }
}

impl InterfaceVariant for Pins<'_> {
    async fn reset(&mut self, delay: &mut impl DelayNs) -> Result<(), RadioError> {
        self.reset.set_low();
        delay.delay_us(RESET_PULSE_US).await;
        self.reset.set_high();
        // BUSY is high from reset until the chip reaches STDBY_RC (DS §9.1).
        self.wait_ready(BUSY_TIMEOUT).await
    }

    async fn wait_on_busy(&mut self) -> Result<(), RadioError> {
        self.wait_ready(BUSY_TIMEOUT).await
    }

    async fn await_irq(&mut self) -> Result<(), RadioError> {
        match self.irq_deadline.get() {
            None => {
                self.dio1.wait_for_high().await;
                Ok(())
            }
            Some(deadline) => with_deadline(deadline, self.dio1.wait_for_high())
                .await
                .map_err(|_| RadioError::DIO1),
        }
    }

    // DIO2 drives the module's RF switch (SetDIO2AsRfSwitchCtrl, which the
    // driver sends for the SX1262); no MCU pin takes part.
    async fn enable_rf_switch_rx(&mut self) -> Result<(), RadioError> {
        Ok(())
    }
    async fn enable_rf_switch_tx(&mut self) -> Result<(), RadioError> {
        Ok(())
    }
    async fn disable_rf_switch(&mut self) -> Result<(), RadioError> {
        Ok(())
    }
}

/// Durations of one transmission, µs.
// The console prints them; the node only needs the transmission done.
#[cfg_attr(not(feature = "console"), allow(dead_code))]
pub struct TxTiming {
    /// Retune, modulation, packet parameters and payload upload.
    pub prepare_us: u32,
    /// SetTx to TxDone handled: the switch to TX, the PA ramp and the time on air.
    pub tx_us: u32,
}

/// What a receive produced.
#[cfg(feature = "console")]
pub enum Received {
    /// A frame whose CRC passed.
    Frame {
        len: usize,
        status: lora_phy::mod_params::PacketStatus,
    },
    /// A frame whose payload CRC failed; the bytes are what arrived.
    CrcError {
        len: usize,
        status: lora_phy::mod_params::PacketStatus,
    },
    /// A header that failed its CRC: nothing was received.
    HeaderError,
}

/// The powered radio.
pub struct Radio<'a> {
    lora: Lora<'a>,
    rx_params: PacketParams,
    /// In continuous receive, where a retune is a frequency change; after
    /// a transmit or CAD the chip is in standby and needs the full setup.
    receiving: bool,
    /// In duty-cycled receive, where the chip may be asleep and must be
    /// woken before any command.
    sniffing: bool,
    irq_deadline: &'a Cell<Option<Instant>>,
    clock: &'a Clock,
    // Dropping an Output disconnects the pin, which would cut the rail.
    _radio_en: Output<'static>,
    _rail_on: Output<'static>,
}

impl<'a> Radio<'a> {
    /// Power the radio up and initialise it.
    ///
    /// Sequence: hold NRESET low; raise PWR_EN (radio and rail) and PWR_ON
    /// (the rail when not on USB); wait [`RAIL_SETTLE`]; only then drive NSS
    /// and the SPI lines, since a pin driven high into an unpowered chip
    /// feeds it through its protection diodes; then the driver's init
    /// pulses NRESET and waits for BUSY low before its first command.
    pub async fn new(
        pins: board::Radio,
        power: board::Power,
        spim: Peri<'static, peripherals::SPI2>,
        irq: impl Binding<SPI2, spim::InterruptHandler<peripherals::SPI2>> + 'static,
        irq_deadline: &'a Cell<Option<Instant>>,
        clock: &'a Clock,
    ) -> Result<Radio<'a>, RadioError> {
        let reset = Output::new(pins.reset, Level::Low, OutputDrive::Standard);
        let radio_en = Output::new(power.radio_en, Level::High, OutputDrive::Standard);
        let rail_on = Output::new(power.rail_on, Level::High, OutputDrive::Standard);
        Timer::after(RAIL_SETTLE).await;

        let nss = Output::new(pins.nss, Level::High, OutputDrive::Standard);
        let mut config = spim::Config::default();
        // 8 MHz: the most SPIM0..2 run at (nRF52840 PS, SPIM), within the
        // SX1262's 16 MHz (DS §8.2). Mode 0 is the chip's (DS §8.2).
        config.frequency = spim::Frequency::M8;
        config.mode = spim::MODE_0;
        let bus = Spim::new(spim, irq, pins.sck, pins.miso, pins.mosi, config);
        let spi = match ExclusiveDevice::new(bus, nss, Delay) {
            Ok(spi) => spi,
            Err(never) => match never {},
        };
        let iv = Pins {
            reset,
            dio1: Input::new(pins.dio1, Pull::None),
            busy: Input::new(pins.busy, Pull::None),
            irq_deadline,
        };
        let chip = sx126x::Config {
            chip: sx126x::Sx1262,
            tcxo_ctrl: Some(TcxoCtrlVoltage::Ctrl1V8),
            tcxo_delay_us: TCXO_START.as_micros() as u32,
            // So a retune, CAD or transmit starts without waiting for the TCXO.
            standby_xosc: true,
            use_dcdc: true,
            rx_boost: false,
        };
        // `false`: the private sync word 0x1424, not LoRaWAN's public 0x3444
        // (spec "Physical layer").
        let mut lora = LoRa::new(Sx126x::new(spi, iv, chip), false, Delay).await?;
        // Packet parameters do not depend on the channel. The receiver is
        // set for the longest preamble a frame has, as DS §6.1.4 asks where
        // it varies (core-interface §3 rule 4); it still takes the shortest.
        let mdl = lora.create_modulation_params(SF, BW, CR, CHANNEL_0_HZ)?;
        let rx_params = lora.create_rx_packet_params(
            LONG_PREAMBLE_SYMBOLS,
            false,
            MAX_LEN as u8,
            true,
            false,
            &mdl,
        )?;
        Ok(Radio {
            lora,
            rx_params,
            receiving: false,
            sniffing: false,
            irq_deadline,
            clock,
            _radio_en: radio_en,
            _rail_on: rail_on,
        })
    }

    fn modulation(&mut self, ch: Channel) -> Result<ModulationParams, RadioError> {
        self.lora.create_modulation_params(SF, BW, CR, ch.hz())
    }

    /// Out of duty-cycled receive, if it is in it, into standby. Asleep in
    /// the cycle the chip takes no command until a falling NSS edge wakes it
    /// (DS §13.1.7); one that comes as it goes to sleep may be lost, and
    /// then BUSY stays high for the sleep, so a timeout is tried once more.
    async fn wake(&mut self) -> Result<(), RadioError> {
        if !self.sniffing {
            return Ok(());
        }
        match self.lora.enter_standby().await {
            Err(RadioError::Busy) => self.lora.enter_standby().await?,
            result => result?,
        }
        self.sniffing = false;
        Ok(())
    }

    /// Listen on `ch` until told otherwise, from any state.
    pub async fn rest(&mut self, ch: Channel) -> Result<(), RadioError> {
        self.receiving = false;
        self.wake().await?;
        let mdl = self.modulation(ch)?;
        self.lora
            .prepare_for_rx(RxMode::Continuous, &mdl, &self.rx_params)
            .await?;
        // Flags left by an earlier operation would raise DIO1 at once and be
        // read as a reception.
        self.lora.clear_irq_status().await?;
        self.lora.start_rx().await?;
        self.receiving = true;
        Ok(())
    }

    /// Receive duty-cycled on `ch` (SetRxDutyCycle, DS §13.1.7), from any
    /// state, until a packet ends or the radio is told otherwise. In each
    /// sleep the chip is in SLEEP with its configuration kept and the TCXO
    /// unpowered, since DIO3 supplies it only in STDBY_XOSC, FS, TX and RX
    /// (DS §13.3.6).
    pub async fn sniff(&mut self, ch: Channel) -> Result<(), RadioError> {
        self.receiving = false;
        self.wake().await?;
        let mdl = self.modulation(ch)?;
        self.lora
            .prepare_for_rx(RxMode::DutyCycle(SNIFF), &mdl, &self.rx_params)
            .await?;
        self.lora.clear_irq_status().await?;
        self.lora.start_rx().await?;
        self.sniffing = true;
        Ok(())
    }

    /// Move a listening radio to `ch`, keeping everything else.
    pub async fn retune(&mut self, ch: Channel) -> Result<(), RadioError> {
        self.lora.rx_switch_channel(ch.hz()).await
    }

    /// Instantaneous RSSI on the channel being listened to, whole dBm.
    pub async fn rssi(&mut self) -> Result<i16, RadioError> {
        self.lora.get_rssi().await
    }

    /// Send `frame` on `ch` at `dbm` conducted after a preamble of
    /// `preamble` symbols. Leaves the radio in standby.
    pub async fn transmit(
        &mut self,
        ch: Channel,
        dbm: i8,
        preamble: u16,
        frame: &[u8],
    ) -> Result<TxTiming, RadioError> {
        let len = u8::try_from(frame.len())
            .map_err(|_| RadioError::PayloadSizeUnexpected(frame.len()))?;
        self.receiving = false;
        self.wake().await?;
        let t0 = self.clock.now();
        let mdl = self.modulation(ch)?;
        let mut params = self
            .lora
            .create_tx_packet_params(preamble, false, true, false, &mdl)?;
        self.lora
            .prepare_for_tx(&mdl, &mut params, dbm.into(), frame)
            .await?;
        let prepare_us = self.clock.since(t0);
        let airtime = Duration::from_micros(time_on_air(len, preamble).as_micros());
        let t1 = self.clock.now();
        self.irq_deadline
            .set(Some(Instant::now() + airtime + IRQ_SLACK));
        let result = self.lora.tx().await;
        self.irq_deadline.set(None);
        let tx_us = self.clock.since(t1);
        result.map(|()| TxTiming { prepare_us, tx_us })
    }

    /// One CAD on `ch`: whether a preamble was seen, the preparation time
    /// and the CAD time, µs. The CAD time includes the TCXO start after it,
    /// since CAD ends in STDBY_RC (DS Table 13-73). Leaves the radio in
    /// standby.
    pub async fn cad(&mut self, ch: Channel) -> Result<(bool, u32, u32), RadioError> {
        self.receiving = false;
        self.wake().await?;
        let t0 = self.clock.now();
        let mdl = self.modulation(ch)?;
        self.lora.prepare_for_cad(&mdl).await?;
        let prepare_us = self.clock.since(t0);
        let t1 = self.clock.now();
        self.irq_deadline
            .set(Some(Instant::now() + CAD_TIME + IRQ_SLACK));
        let result = self.lora.cad(&mdl).await;
        self.irq_deadline.set(None);
        let cad_us = self.clock.since(t1);
        result.map(|busy| (busy, prepare_us, cad_us))
    }

    /// Wait for DIO1 while listening. Cancel-safe: nothing has been read.
    pub async fn wait_irq(&mut self) -> Result<(), RadioError> {
        self.lora.wait_for_irq().await
    }

    /// Handle the interrupt [`Radio::wait_irq`] reported and, if it began a
    /// frame, the rest of the frame. The radio keeps listening afterwards.
    /// Must not be cancelled (the driver's rule for `complete_rx`).
    #[cfg(feature = "console")]
    pub async fn receive(&mut self, buf: &mut [u8; MAX_LEN]) -> Result<Received, RadioError> {
        let params = &self.rx_params;
        let deadline = Instant::now() + MAX_AIRTIME + IRQ_SLACK;
        self.irq_deadline.set(Some(deadline));
        let result = self.lora.complete_rx(params, buf).await;
        self.irq_deadline.set(None);
        match result {
            Ok((len, status)) => Ok(Received::Frame {
                len: len.into(),
                status,
            }),
            Err(RadioError::CrcError) => {
                let (len, status) = self.lora.get_rx_result(params, buf).await?;
                Ok(Received::CrcError {
                    len: len.into(),
                    status,
                })
            }
            Err(RadioError::HeaderError) => Ok(Received::HeaderError),
            Err(e) => Err(e),
        }
    }
}

impl Radio<'_> {
    /// What the interrupt [`Radio::wait_irq`] reported was: a frame read
    /// into `buf`, a packet begun, a packet failed, or nothing. Clears the
    /// interrupt; the radio keeps listening. Must not be cancelled.
    pub async fn service(&mut self, buf: &mut [u8; MAX_LEN]) -> Result<Heard, RadioError> {
        let state = self.lora.get_irq_state().await;
        self.lora.clear_irq_status().await?;
        match state {
            Ok(Some(IrqState::Done)) => {
                let (len, status) = self.lora.get_rx_result(&self.rx_params, buf).await?;
                Ok(Heard::Frame {
                    len,
                    rssi_dbm: status.rssi.dbm(),
                    snr_db: status.snr.db().clamp(i8::MIN.into(), i8::MAX.into()) as i8,
                })
            }
            Ok(Some(IrqState::PreambleReceived)) => Ok(Heard::Started),
            Ok(None) => Ok(Heard::Nothing),
            // A timeout ends a duty-cycled window that found a preamble and
            // no header in time (DS §13.1.7): a packet without a frame.
            Err(RadioError::CrcError | RadioError::HeaderError | RadioError::ReceiveTimeout) => {
                Ok(Heard::Failed)
            }
            Err(e) => Err(e),
        }
    }
}

impl bark_driver::Radio for Radio<'_> {
    type Error = RadioError;

    async fn listen(&mut self, channel: Channel) -> Result<(), RadioError> {
        match self.receiving {
            true => self.retune(channel).await,
            false => self.rest(channel).await,
        }
    }

    async fn sniff(&mut self, channel: Channel) -> Result<(), RadioError> {
        Radio::sniff(self, channel).await
    }

    async fn wait(&mut self) -> Result<(), RadioError> {
        self.wait_irq().await
    }

    async fn service(&mut self, buf: &mut [u8; MAX_LEN]) -> Result<Heard, RadioError> {
        Radio::service(self, buf).await
    }

    async fn lbt(&mut self, channel: Channel) -> Result<(bool, i16), RadioError> {
        let (busy, _, _) = Radio::cad(self, channel).await?;
        // The reading after the CAD, with the receiver started on the
        // channel and settled.
        self.rest(channel).await?;
        Timer::after(Duration::from_micros(RSSI_SETTLE.as_micros())).await;
        let dbm = Radio::rssi(self).await?;
        Ok((busy, dbm))
    }

    async fn rssi(&mut self) -> Result<i16, RadioError> {
        Radio::rssi(self).await
    }

    async fn transmit(&mut self, frame: &Frame, dbm: i8) -> Result<(), RadioError> {
        Radio::transmit(
            self,
            frame.channel,
            dbm,
            frame.preamble_symbols,
            frame.payload(),
        )
        .await
        .map(|_| ())
    }
}
