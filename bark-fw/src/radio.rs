//! The SX1262: power-up, the pins the driver drives, and bark's modulation
//! (protocol §4) on top of `lora-phy`.
//!
//! "DS" is the SX1261/2 datasheet, Rev 1.2 (June 2019).

use core::cell::Cell;

use bark_proto::radio::{
    Channel, BANDWIDTH_HZ, CHANNEL_0_HZ, CODING_RATE_DENOMINATOR, MIN_PREAMBLE_SYMBOLS,
    SPREADING_FACTOR,
};
use bark_proto::MAX_FRAME;
use embassy_nrf::gpio::{Input, Level, Output, OutputDrive, Pull};
use embassy_nrf::interrupt::typelevel::{Binding, SPI2};
use embassy_nrf::peripherals;
use embassy_nrf::spim::{self, Spim};
use embassy_nrf::Peri;
use embassy_time::{with_deadline, with_timeout, Delay, Duration, Instant, Timer};
use embedded_hal_bus::spi::ExclusiveDevice;
use lora_phy::mod_params::{
    Bandwidth, CodingRate, ModulationParams, PacketParams, RadioError, SpreadingFactor,
};
use lora_phy::mod_traits::InterfaceVariant;
use lora_phy::sx126x::{self, Sx126x, TcxoCtrlVoltage};
use lora_phy::{DelayNs, LoRa, RxMode};

use crate::board;

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

/// TCXO start allowance lora-phy programs with SetDIO3AsTCXOCtrl (its
/// `BRD_TCXO_WAKEUP_TIME`). The chip waits it out on every start of TX, RX,
/// FS or CAD from STDBY_RC (DS §13.3.6, note), and the driver always idles
/// in STDBY_RC.
pub const DRIVER_TCXO_START: Duration = Duration::from_millis(10);

/// Cold start to STDBY_RC, the longest switching time in DS Table 8-2.
const COLD_START: Duration = Duration::from_micros(3500);

/// Bound on any one BUSY-high stretch: twice the longest the driver can
/// cause, a cold start followed by a TCXO start. Past it the chip is absent,
/// unpowered or wedged, and the console says so instead of hanging.
const BUSY_TIMEOUT: Duration =
    Duration::from_micros(2 * (COLD_START.as_micros() + DRIVER_TCXO_START.as_micros()));

/// NRESET low time: twice the typical 100 µs of DS §8.1.
const RESET_PULSE_US: u32 = 200;

/// The switched rail's ramp after PWR_EN rises. No source gives it; 10 ms
/// is far beyond a load switch's typical sub-millisecond ramp and costs
/// nothing once per boot. NRESET is held low throughout, and the chip's own
/// start-up is then gated on BUSY, so only the ramp itself is assumed.
const RAIL_SETTLE: Duration = Duration::from_millis(10);

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

/// The powered radio.
pub struct Radio<'a> {
    lora: Lora<'a>,
    rx_params: PacketParams,
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
            use_dcdc: true,
            rx_boost: false,
        };
        // `false`: the private sync word 0x1424, not LoRaWAN's public 0x3444
        // (protocol §4).
        let mut lora = LoRa::new(Sx126x::new(spi, iv, chip), false, Delay).await?;
        // Packet parameters do not depend on the channel.
        let mdl = lora.create_modulation_params(SF, BW, CR, CHANNEL_0_HZ)?;
        let rx_params = lora.create_rx_packet_params(
            MIN_PREAMBLE_SYMBOLS,
            false,
            MAX_LEN as u8,
            true,
            false,
            &mdl,
        )?;
        Ok(Radio {
            lora,
            rx_params,
            _radio_en: radio_en,
            _rail_on: rail_on,
        })
    }

    fn modulation(&mut self, ch: Channel) -> Result<ModulationParams, RadioError> {
        self.lora.create_modulation_params(SF, BW, CR, ch.hz())
    }

    /// Listen on `ch` until told otherwise, from any state.
    pub async fn rest(&mut self, ch: Channel) -> Result<(), RadioError> {
        let mdl = self.modulation(ch)?;
        self.lora
            .prepare_for_rx(RxMode::Continuous, &mdl, &self.rx_params)
            .await?;
        // Flags left by an earlier operation would raise DIO1 at once and be
        // read as a reception.
        self.lora.clear_irq_status().await?;
        self.lora.start_rx().await
    }
}
