//! The SX1262 on the board: power-up, the pins the chip is driven by, the
//! SPI bus, and the chip one command at a time, as the sequencer in
//! `bark_driver::sx1262` drives it (roadmap D88). The sequencer keeps what
//! the chip holds and sends only what changes; this module sends the
//! bytes, with the vendored `lora-phy` for their encoding.
//!
//! "DS" is the SX1261/2 datasheet, Rev 1.2 (June 2019).

use bark_driver::sx1262::{self, Chip, Sx1262};
use bark_driver::tcxo;
use bark_proto::radio::{
    Channel, BANDWIDTH_HZ, CHANNEL_0_HZ, CODING_RATE_DENOMINATOR, MAX_ERP_DBM, SPREADING_FACTOR,
};
use bark_proto::{Duration, MAX_FRAME};
use embassy_nrf::gpio::{Input, Level, Output, OutputDrive, Pull};
use embassy_nrf::interrupt::typelevel::{Binding, SPI2, TEMP};
use embassy_nrf::spim::{self, Spim};
use embassy_nrf::temp::{self, Temp};
use embassy_nrf::{peripherals, Peri};
use embassy_time::{with_timeout, Delay, Timer};
use lora_phy::mod_params::{
    Bandwidth, CodingRate, ModulationParams, PacketParams, RadioError, RadioMode, SpreadingFactor,
};
use lora_phy::mod_traits::{InterfaceVariant, RadioKind};
use lora_phy::sx126x::{self, Sx126x, TcxoCtrlVoltage};
use lora_phy::DelayNs;

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
#[cfg(feature = "console")]
pub const MAX_LEN: usize = MAX_FRAME;

/// Antenna gain over a half-wave dipole, dBd. ERP = conducted power + gain
/// in dBd. Assumption: the stock 433 MHz antenna is no better than a dipole.
pub const ANTENNA_GAIN_DBD: i8 = 0;

/// Highest conducted power the console allows: the regulatory ERP limit
/// (ERC/REC 70-03 band g1, 10 mW; spec "Regulatory limits") less antenna gain.
pub const MAX_TX_DBM: i8 = MAX_ERP_DBM - ANTENNA_GAIN_DBD;

/// Lowest power the SX1262 high-power PA can be set to (DS §13.4.4 SetTxParams).
pub const MIN_TX_DBM: i8 = -9;

/// The private sync word (spec "Physical layer") in the two-byte form
/// the SX126x writes, 0x1424 for the legacy 0x12; LoRaWAN's public one is
/// 0x3444.
const SYNC_WORD: u16 = 0x1424;

/// The TCXO's supply on DIO3 (`board.rs`).
const TCXO_VOLTAGE: TcxoCtrlVoltage = TcxoCtrlVoltage::Ctrl1V8;

/// Cold start to STDBY_RC, the longest switching time in DS Table 8-2; a
/// full calibration takes as long (DS §13.1.12).
const COLD_START: Duration = Duration::from_micros(3500);

/// Bound on any one BUSY-high stretch: twice the longest the driver can
/// cause, a calibration after the longest oscillator start the search
/// allows. Past it the chip is absent, unpowered or wedged, and the
/// operation fails instead of hanging.
const BUSY_TIMEOUT: embassy_time::Duration =
    embassy_time::Duration::from_micros(2 * (COLD_START.as_micros() + tcxo::MAX.as_micros()));

/// NRESET low time: twice the typical 100 µs of DS §8.1.
const RESET_PULSE_US: u32 = 200;

/// The switched rail's ramp after PWR_EN rises. No source gives it; 10 ms
/// is far beyond a load switch's typical sub-millisecond ramp and costs
/// nothing once per boot. NRESET is held low throughout, and the chip's own
/// start-up is then gated on BUSY, so only the ramp itself is assumed.
pub const RAIL_SETTLE: embassy_time::Duration = embassy_time::Duration::from_millis(10);

/// nRF52840 CPU clock, Hz (nRF52840 Product Specification, CLOCK).
const CPU_HZ: u64 = 64_000_000;

/// BUSY rises at most 600 ns after NSS rises (DS §8.3.1, T_SW). The driver
/// polls BUSY right after each command; polled sooner it can still read the
/// previous low, and the next command would reach a chip that is not ready.
const BUSY_RISE_CYCLES: u32 = (600 * CPU_HZ).div_ceil(1_000_000_000) as u32;

/// A duty-cycled receive period in the chip's steps of 15.625 µs, 64 to
/// the millisecond (DS §13.1.7).
fn steps(d: Duration) -> u32 {
    (d.as_micros() * 64 / 1000) as u32
}

/// The SPI bus driven blocking inside the asynchronous device: a command
/// is a few bytes at 8 MHz, microseconds, and the interrupt and executor
/// wake of a DMA transfer cost 35 µs more on each, a third of every
/// command (`docs/bench-2026-10-30.md`, "What the retune is made of, an
/// experiment").
pub struct BlockingSpi {
    bus: Spim<'static>,
    nss: Output<'static>,
}

impl embedded_hal_async::spi::ErrorType for BlockingSpi {
    type Error = spim::Error;
}

impl embedded_hal_async::spi::SpiDevice<u8> for BlockingSpi {
    async fn transaction(
        &mut self,
        operations: &mut [embedded_hal_async::spi::Operation<'_, u8>],
    ) -> Result<(), Self::Error> {
        use embedded_hal_async::spi::Operation;
        self.nss.set_low();
        let mut result = Ok(());
        for op in operations.iter_mut() {
            result = match op {
                Operation::Read(buf) => self.bus.blocking_read(buf),
                Operation::Write(buf) => self.bus.blocking_write(buf),
                Operation::Transfer(read, write) => self.bus.blocking_transfer(read, write),
                Operation::TransferInPlace(buf) => self.bus.blocking_transfer_in_place(buf),
                Operation::DelayNs(ns) => {
                    embassy_time::block_for(embassy_time::Duration::from_nanos(u64::from(*ns)));
                    Ok(())
                }
            };
            if result.is_err() {
                break;
            }
        }
        self.nss.set_high();
        result
    }
}

/// The control pins, as the driver's board interface.
///
/// The generic SX126x one in `lora-phy` waits for BUSY low only after each
/// command, at once, and without a bound; this one waits out BUSY's rise
/// time first and bounds every wait.
pub struct Pins {
    reset: Output<'static>,
    dio1: Input<'static>,
    busy: Input<'static>,
}

impl Pins {
    async fn wait_ready(&mut self) -> Result<(), RadioError> {
        cortex_m::asm::delay(BUSY_RISE_CYCLES);
        with_timeout(BUSY_TIMEOUT, self.busy.wait_for_low())
            .await
            .map_err(|_| RadioError::Busy)
    }

    /// Wait for DIO1 high, for at most `timeout` if given. An operation
    /// whose interrupt never comes (wrong wiring, the oscillator not
    /// starting) then fails instead of hanging.
    async fn await_dio1(&mut self, timeout: Option<Duration>) -> Result<(), RadioError> {
        match timeout {
            None => {
                self.dio1.wait_for_high().await;
                Ok(())
            }
            Some(t) => with_timeout(
                embassy_time::Duration::from_micros(t.as_micros()),
                self.dio1.wait_for_high(),
            )
            .await
            .map_err(|_| RadioError::DIO1),
        }
    }
}

impl InterfaceVariant for Pins {
    async fn reset(&mut self, delay: &mut impl DelayNs) -> Result<(), RadioError> {
        self.reset.set_low();
        delay.delay_us(RESET_PULSE_US).await;
        self.reset.set_high();
        // BUSY is high from reset until the chip reaches STDBY_RC (DS §9.1).
        self.wait_ready().await
    }

    async fn wait_on_busy(&mut self) -> Result<(), RadioError> {
        self.wait_ready().await
    }

    async fn await_irq(&mut self) -> Result<(), RadioError> {
        self.await_dio1(None).await
    }

    // DIO2 drives the module's RF switch (SetDIO2AsRfSwitchCtrl, sent in
    // the configuration); no MCU pin takes part.
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

/// The chip over its bus and pins, one command at a time, and the
/// processor's die temperature sensor for the oscillator's schedule.
pub struct Hardware<'a> {
    sx: Sx126x<BlockingSpi, Pins, sx126x::Sx1262>,
    temp: Temp<'a>,
    modulation: ModulationParams,
    // Dropping an Output disconnects the pin, which would cut the rail.
    _radio_en: Output<'static>,
    _rail_on: Output<'static>,
}

/// The radio: the sequencer on this board's chip.
pub type Radio<'a> = Sx1262<Hardware<'a>, &'a Clock>;

/// What the radio fails with.
pub type Error = sx1262::Error<RadioError>;

/// Power the radio up and bring it up listening on `channel`
/// (`Sx1262::boot`).
///
/// Sequence: hold NRESET low; raise PWR_EN (radio and rail) and PWR_ON
/// (the rail when not on USB); wait [`RAIL_SETTLE`]; only then drive NSS
/// and the SPI lines, since a pin driven high into an unpowered chip
/// feeds it through its protection diodes; then the sequencer pulses
/// NRESET and waits for BUSY low before its first command.
#[allow(clippy::too_many_arguments)]
pub async fn boot<'a>(
    pins: board::Radio,
    power: board::Power,
    spim: Peri<'static, peripherals::SPI2>,
    irq: impl Binding<SPI2, spim::InterruptHandler<peripherals::SPI2>> + 'static,
    temp: Peri<'a, peripherals::TEMP>,
    temp_irq: impl Binding<TEMP, temp::InterruptHandler> + 'a,
    clock: &'a Clock,
    channel: Channel,
) -> Result<Radio<'a>, Error> {
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
    let spi = BlockingSpi { bus, nss };
    let iv = Pins {
        reset,
        dio1: Input::new(pins.dio1, Pull::None),
        busy: Input::new(pins.busy, Pull::None),
    };
    // The sequencer programs the oscillator's delay and chooses each
    // standby itself; the vendored driver's own sequences, which these
    // fields shape, are not used.
    let chip = sx126x::Config {
        chip: sx126x::Sx1262,
        tcxo_ctrl: Some(TCXO_VOLTAGE),
        tcxo_delay_us: tcxo::MAX.as_micros() as u32,
        standby_xosc: false,
        use_dcdc: true,
        rx_boost: false,
    };
    let sx = Sx126x::new(spi, iv, chip);
    let modulation = sx.create_modulation_params(SF, BW, CR, CHANNEL_0_HZ)?;
    let hardware = Hardware {
        sx,
        temp: Temp::new(temp, temp_irq),
        modulation,
        _radio_en: radio_en,
        _rail_on: rail_on,
    };
    Sx1262::boot(hardware, clock, channel).await
}

/// The receiver's packet parameters as `lora-phy` wants them, for
/// reading a frame out.
const RX_PARAMS: PacketParams = PacketParams {
    preamble_length: sx1262::RX_PACKET.0,
    implicit_header: false,
    payload_length: sx1262::RX_PACKET.1,
    crc_on: true,
    iq_inverted: false,
};

impl Chip for Hardware<'_> {
    type Error = RadioError;

    async fn reset(&mut self) -> Result<(), RadioError> {
        RadioKind::reset(&mut self.sx, &mut Delay).await
    }

    async fn wake(&mut self) -> Result<(), RadioError> {
        // GetStatus on a falling NSS edge, then BUSY low (DS §13.1.7).
        self.sx.ensure_ready(RadioMode::Sleep).await
    }

    async fn standby(&mut self, xosc: bool) -> Result<(), RadioError> {
        self.sx.standby(xosc).await
    }

    async fn calibrate(&mut self) -> Result<(), RadioError> {
        self.sx.calibrate_all().await
    }

    async fn calibrate_image(&mut self) -> Result<(), RadioError> {
        self.sx.calibrate_image(CHANNEL_0_HZ).await
    }

    async fn configure(&mut self) -> Result<(), RadioError> {
        self.sx.regulator_dcdc().await?;
        self.sx.dio2_rf_switch(true).await?;
        self.sx.packet_type_lora().await?;
        self.sx.set_lora_sync_word(SYNC_WORD).await?;
        self.sx.buffer_base_zero().await?;
        self.sx.retention_list().await?;
        self.sx.fallback_stdby_xosc().await?;
        // With the errata write for the transmit modulation.
        self.sx.set_modulation_params(&self.modulation).await?;
        self.sx.iq_errata(false).await?;
        self.sx
            .irq_params(sx1262::irq::ALL, sx1262::irq::ALL)
            .await?;
        self.sx.symbol_timeout(0).await?;
        self.sx.rx_gain(false).await
    }

    async fn cad_params(&mut self) -> Result<(), RadioError> {
        self.sx.cad_params(SF).await
    }

    async fn set_tcxo(&mut self, delay: Duration) -> Result<(), RadioError> {
        self.sx.tcxo(TCXO_VOLTAGE, delay.as_micros() as u32).await
    }

    async fn errors(&mut self) -> Result<u16, RadioError> {
        self.sx.device_errors().await
    }

    async fn clear_errors(&mut self) -> Result<(), RadioError> {
        self.sx.clear_device_errors().await
    }

    async fn set_frequency(&mut self, channel: Channel) -> Result<(), RadioError> {
        self.sx.set_channel(channel.hz()).await
    }

    async fn set_packet_params(&mut self, preamble: u16, len: u8) -> Result<(), RadioError> {
        let params = PacketParams {
            preamble_length: preamble,
            payload_length: len,
            ..RX_PARAMS
        };
        self.sx.packet_params(&params).await
    }

    async fn set_tx_power(&mut self, dbm: i8) -> Result<(), RadioError> {
        self.sx
            .set_tx_power_and_ramp_time(dbm.into(), None, true)
            .await
    }

    async fn write_payload(&mut self, bytes: &[u8]) -> Result<(), RadioError> {
        self.sx.set_payload(bytes).await
    }

    async fn rx(&mut self) -> Result<(), RadioError> {
        self.sx.rx_continuous().await
    }

    async fn rx_duty_cycle(&mut self, rx: Duration, sleep: Duration) -> Result<(), RadioError> {
        self.sx.rx_duty_cycle(steps(rx), steps(sleep)).await
    }

    async fn cad(&mut self) -> Result<(), RadioError> {
        self.sx.start_cad().await
    }

    async fn tx(&mut self) -> Result<(), RadioError> {
        self.sx.start_tx().await
    }

    async fn rssi(&mut self) -> Result<i16, RadioError> {
        self.sx.get_rssi().await
    }

    async fn irq_status(&mut self) -> Result<u16, RadioError> {
        self.sx.irq_status().await
    }

    async fn clear_irq(&mut self) -> Result<(), RadioError> {
        self.sx.clear_irq_status().await
    }

    async fn await_irq(&mut self, timeout: Option<Duration>) -> Result<(), RadioError> {
        self.sx.iv_mut().await_dio1(timeout).await
    }

    async fn read_frame(&mut self, buf: &mut [u8; MAX_FRAME]) -> Result<(u8, i16, i8), RadioError> {
        let len = self.sx.get_rx_payload(&RX_PARAMS, buf).await?;
        let status = self.sx.get_rx_packet_status().await?;
        let snr = status.snr.db().clamp(i8::MIN.into(), i8::MAX.into()) as i8;
        Ok((len, status.rssi.dbm(), snr))
    }

    async fn die_temperature(&mut self) -> Option<i16> {
        // The sensor's quarter degrees, as the fixed-point value's bits.
        let quarters = self.temp.read().await.to_bits();
        Some(quarters.clamp(i16::MIN.into(), i16::MAX.into()) as i16)
    }
}
