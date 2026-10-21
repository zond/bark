//! bark firmware for the LILYGO T-Echo.
//!
//! Blinks the green LED at 1 Hz while on USB, logs over USB CDC-ACM,
//! powers up the SX1262 at bark's modulation and runs the protocol core on
//! it (`node.rs`), a relay on USB and sleepy on battery (`power.rs`), or
//! with the `console` feature the bring-up measurement console
//! (`console.rs`) on the serial port. Touches no GPS, display or BLE.

#![no_std]
#![no_main]

mod board;
mod clock;
#[cfg(feature = "console")]
mod console;
#[cfg(not(feature = "console"))]
mod node;
mod power;
mod radio;

use core::cell::Cell;

use embassy_executor::Spawner;
use embassy_futures::join::join4;
use embassy_nrf::config::{Config, HfclkSource};
use embassy_nrf::usb::vbus_detect::{self, HardwareVbusDetect};
use embassy_nrf::{bind_interrupts, peripherals, spim, usb};
use embassy_time::Instant;
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
// The logger macro constructs the input handler through this trait.
use embassy_usb_logger::ReceiverHandler as _;
use panic_persist as _;

#[cfg(feature = "console")]
bind_interrupts!(struct Irqs {
    USBD => usb::InterruptHandler<peripherals::USBD>;
    CLOCK_POWER => vbus_detect::InterruptHandler, power::InterruptHandler;
    SPI2 => spim::InterruptHandler<peripherals::SPI2>;
    SAADC => embassy_nrf::saadc::InterruptHandler;
});
#[cfg(not(feature = "console"))]
bind_interrupts!(struct Irqs {
    USBD => usb::InterruptHandler<peripherals::USBD>;
    CLOCK_POWER => vbus_detect::InterruptHandler, power::InterruptHandler;
    SPI2 => spim::InterruptHandler<peripherals::SPI2>;
});

/// What the host types: commands for the console, or nothing to the node.
#[cfg(feature = "console")]
use console::HostInput as Input;
#[cfg(not(feature = "console"))]
use node::IgnoreInput as Input;

/// USB vendor ID of pid.codes, the open-source VID registry.
const USB_VID: u16 = 0x1209;
/// pid.codes test PID 0x0001, "reserved for use in private testing"; it must
/// not ship on a distributed device. Register a bark PID before that.
const USB_PID: u16 = 0x0001;

/// Full-speed bulk and control endpoint size (USB 2.0 §5.5.3, §5.8.3).
const USB_PACKET: u16 = 64;

/// Log ring size in bytes. Lines logged before the host opens the port wait
/// here; once it is full, new lines are dropped, so the banner survives.
const LOG_BUFFER: usize = 2048;

/// Heartbeat airtime as the core computes it (8-byte frame, minimum SF5
/// preamble). A compile-time call into `bark-proto` for the target.
const HEARTBEAT_AIRTIME_US: u64 =
    bark_proto::radio::time_on_air(8, bark_proto::radio::MIN_PREAMBLE_SYMBOLS).as_micros();

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    // Read before anything else can panic and overwrite it.
    let previous_panic = panic_persist::get_panic_message_utf8();

    let mut config = Config::default();
    // USBD needs the 32 MHz crystal oscillator running (nRF52840 Product
    // Specification, USBD chapter), and once started it runs while the CPU
    // sleeps. A node that boots without USB power keeps it off, so its USB
    // works only after a reset with the cable in; the console always
    // starts it, for its stopwatch (clock.rs). The 32.768 kHz clock stays
    // on the internal RC oscillator, which assumes nothing about a fitted
    // LF crystal.
    let on_usb = embassy_nrf::pac::POWER.usbregstatus().read().vbusdetect();
    config.hfclk_source = match cfg!(feature = "console") || on_usb {
        true => HfclkSource::ExternalXtal,
        false => HfclkSource::Internal,
    };
    let p = embassy_nrf::init(config);
    let board = board::take_board!(p);

    // USB device with one CDC-ACM interface carrying the log.
    let driver = usb::Driver::new(board.usbd, Irqs, HardwareVbusDetect::new(Irqs));
    let mut usb_config = embassy_usb::Config::new(USB_VID, USB_PID);
    usb_config.manufacturer = Some("bark");
    usb_config.product = Some("bark T-Echo");
    usb_config.serial_number = None;
    // Current we ask the host for, mA: one unit load, granted by every port
    // without negotiation (USB 2.0 §7.2.1).
    usb_config.max_power = 100;
    usb_config.max_packet_size_0 = USB_PACKET as u8;

    let mut config_descriptor = [0u8; 128];
    let mut bos_descriptor = [0u8; 16];
    let mut msos_descriptor = [0u8; 256];
    let mut control_buf = [0u8; 64];
    let mut cdc_state = State::new();
    let mut builder = embassy_usb::Builder::new(
        driver,
        usb_config,
        &mut config_descriptor,
        &mut bos_descriptor,
        &mut msos_descriptor,
        &mut control_buf,
    );
    let class = CdcAcmClass::new(&mut builder, &mut cdc_state, USB_PACKET);
    let mut device = builder.build();

    // Installs the global logger now; the returned future drains it to the
    // host once the host asserts DTR (opens the port), and feeds what the
    // host sends to the console.
    let log_sink = embassy_usb_logger::with_custom_style!(
        LOG_BUFFER,
        log::LevelFilter::Info,
        class,
        |record, writer| {
            use core::fmt::Write as _;
            #[cfg(feature = "console")]
            if record.target() == console::ECHO {
                let _ = write!(writer, "> {}\r\n", record.args());
                return;
            }
            let ms = Instant::now().as_millis();
            let _ = write!(
                writer,
                "[{:>6}.{:03} {:<5}] {}\r\n",
                ms / 1000,
                ms % 1000,
                record.level(),
                record.args()
            );
        },
        Input
    );

    log::info!("bark-fw {} on LILYGO T-Echo", env!("CARGO_PKG_VERSION"));
    log::info!(
        "bark-proto: {} channels from {} Hz, spacing {} Hz, SF{}, symbol {} us, heartbeat {} us",
        bark_proto::radio::CHANNELS,
        bark_proto::radio::CHANNEL_0_HZ,
        bark_proto::radio::CHANNEL_SPACING_HZ,
        bark_proto::radio::SPREADING_FACTOR,
        bark_proto::radio::SYMBOL_TIME.as_micros(),
        HEARTBEAT_AIRTIME_US,
    );
    if let Some(msg) = previous_panic {
        log::error!("previous boot panicked: {}", msg);
    }

    let blink = on_usb_only(board.leds.green);

    #[cfg(feature = "console")]
    let clock = clock::Clock::new(p.TIMER1);
    #[cfg(not(feature = "console"))]
    let clock = clock::Clock::new();
    let irq_deadline = Cell::new(None);
    #[cfg(feature = "console")]
    let battery = embassy_nrf::saadc::Saadc::new(
        p.SAADC,
        Irqs,
        embassy_nrf::saadc::Config::default(),
        [embassy_nrf::saadc::ChannelConfig::single_ended(
            board.battery.adc,
        )],
    );
    #[cfg(feature = "console")]
    let mut lines = console::Lines::new();
    #[cfg(not(feature = "console"))]
    let (config, core) = {
        let config = node::config();
        let seed = embassy_nrf::rng::Rng::new_blocking(p.RNG).blocking_next_u64();
        (config, node::core(config, seed))
    };
    let radio_task = async {
        let t0 = clock.now();
        match radio::Radio::new(
            board.radio,
            board.power,
            p.SPI2,
            Irqs,
            &irq_deadline,
            &clock,
        )
        .await
        {
            Ok(radio) => {
                log::info!("radio up in {} us", clock.since(t0));
                #[cfg(feature = "console")]
                console::Console::new(radio, &clock, battery)
                    .run(&mut lines)
                    .await;
                #[cfg(not(feature = "console"))]
                node::run(core, radio, &config).await;
            }
            // Without a radio there is nothing to measure; USB and the log
            // keep running so the error can be read.
            Err(e) => log::error!("radio init failed: {:?}", e),
        }
    };

    join4(device.run(), log_sink, blink, radio_task).await;
}

/// Blink half-period: 1 Hz means 500 ms on, 500 ms off.
const BLINK_HALF_PERIOD: embassy_time::Duration = embassy_time::Duration::from_millis(500);
/// Half-periods between uptime lines: 20 × 500 ms = 10 s.
const UPTIME_EVERY: u32 = 20;

/// While on USB, blink the green LED, log the uptime and keep the 32 MHz
/// crystal running, which USB needs (nRF52840 Product Specification,
/// USBD). On battery none of these: the LED would dwarf the radio's
/// current, a blink would wake the CPU twice a second, and the crystal,
/// once started, runs on while the CPU sleeps, where the internal
/// oscillator starts only while something needs it.
async fn on_usb_only(pin: embassy_nrf::Peri<'static, peripherals::P0_15>) {
    use embassy_futures::select::{select, Either};
    use embassy_nrf::gpio::{Level, Output, OutputDrive};
    // LED assumed active low (board.rs, Leds): start dark.
    let mut green = Output::new(pin, Level::High, OutputDrive::Standard);
    let mut half_periods: u32 = 0;
    loop {
        let usb = power::usb();
        let crystal = embassy_nrf::pac::CLOCK;
        if !usb {
            green.set_high();
            if !cfg!(feature = "console") {
                crystal.tasks_hfclkstop().write_value(1);
            }
            power::FOR_LED.wait().await;
            continue;
        }
        crystal.tasks_hfclkstart().write_value(1);
        let half = embassy_time::Timer::after(BLINK_HALF_PERIOD);
        if let Either::First(()) = select(half, power::FOR_LED.wait()).await {
            green.toggle();
            half_periods = half_periods.wrapping_add(1);
            if half_periods.is_multiple_of(UPTIME_EVERY) {
                log::info!("uptime {} s", Instant::now().as_secs());
            }
        }
    }
}
