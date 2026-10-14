//! bark firmware for the LILYGO T-Echo: bring-up skeleton.
//!
//! Blinks the green LED at 1 Hz and logs over USB CDC-ACM. Proves the
//! toolchain, the flash layout under the stock bootloader, and that
//! `bark-proto` builds for the target. Touches no radio, GPS, display or BLE.

#![no_std]
#![no_main]

mod board;

use embassy_executor::Spawner;
use embassy_futures::join::join3;
use embassy_nrf::config::{Config, HfclkSource};
use embassy_nrf::gpio::{Level, Output, OutputDrive};
use embassy_nrf::usb::vbus_detect::{self, HardwareVbusDetect};
use embassy_nrf::{bind_interrupts, peripherals, usb};
use embassy_time::{Duration, Instant, Ticker};
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use panic_persist as _;

bind_interrupts!(struct Irqs {
    USBD => usb::InterruptHandler<peripherals::USBD>;
    CLOCK_POWER => vbus_detect::InterruptHandler;
});

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

/// Blink half-period: 1 Hz means 500 ms on, 500 ms off.
const BLINK_HALF_PERIOD: Duration = Duration::from_millis(500);
/// Half-periods between uptime lines: 20 × 500 ms = 10 s.
const UPTIME_EVERY: u32 = 20;

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
    // Specification, USBD chapter). The 32.768 kHz clock stays on the
    // internal RC oscillator, which assumes nothing about a fitted LF crystal.
    config.hfclk_source = HfclkSource::ExternalXtal;
    let p = embassy_nrf::init(config);
    let board = board::take_board!(p);

    // LED assumed active low (board.rs, Leds): start dark.
    let mut green = Output::new(board.leds.green, Level::High, OutputDrive::Standard);

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
    // host once the host asserts DTR (opens the port).
    let log_sink = embassy_usb_logger::with_custom_style!(
        LOG_BUFFER,
        log::LevelFilter::Info,
        class,
        |record, writer| {
            use core::fmt::Write as _;
            let ms = Instant::now().as_millis();
            let _ = write!(
                writer,
                "[{:>6}.{:03} {:<5}] {}\r\n",
                ms / 1000,
                ms % 1000,
                record.level(),
                record.args()
            );
        }
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

    let blink = async {
        let mut ticker = Ticker::every(BLINK_HALF_PERIOD);
        let mut half_periods: u32 = 0;
        loop {
            ticker.next().await;
            green.toggle();
            half_periods = half_periods.wrapping_add(1);
            if half_periods.is_multiple_of(UPTIME_EVERY) {
                log::info!("uptime {} s", Instant::now().as_secs());
            }
        }
    };

    join3(device.run(), log_sink, blink).await;
}
