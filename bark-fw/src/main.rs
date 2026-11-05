//! bark firmware for the LILYGO T-Echo.
//!
//! Blinks the green LED at 1 Hz while on USB, logs over USB CDC-ACM,
//! shows the node ID and the image on the e-paper (`display.rs`), powers
//! up the SX1262 at bark's modulation and runs the protocol core on it
//! (`node.rs`), a relay on USB and sleepy on battery (`power.rs`), or
//! with the `console` feature the bring-up measurement console
//! (`console.rs`) on the serial port. Reads the battery (`battery.rs`);
//! touches no GPS. The node build runs the phone link (`ble.rs`).

#![no_std]
#![no_main]

mod battery;
#[cfg(not(feature = "console"))]
mod ble;
mod board;
mod clock;
#[cfg(feature = "console")]
mod console;
mod display;
#[cfg(not(feature = "console"))]
mod node;
mod power;
mod radio;
#[cfg(not(feature = "console"))]
mod store;
mod update;

use core::fmt::Write as _;

use bark_proto::NodeId;
use embassy_executor::Spawner;
use embassy_futures::join::{join3, join5};
use embassy_nrf::config::{Config, HfclkSource};
use embassy_nrf::usb::vbus_detect::{self, HardwareVbusDetect};
use embassy_nrf::{bind_interrupts, peripherals, spim, usb};
use embassy_time::Instant;
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
// The logger macro constructs the input handler through this trait.
use embassy_usb_logger::ReceiverHandler as _;
use panic_persist as _;
// The critical section, in both builds: it masks every interrupt but the
// MPSL's radio timing (`ble.rs`); the console links it for that alone.
use nrf_mpsl as _;

#[cfg(feature = "console")]
bind_interrupts!(struct Irqs {
    USBD => usb::InterruptHandler<peripherals::USBD>;
    CLOCK_POWER => vbus_detect::InterruptHandler, power::InterruptHandler;
    SPI2 => spim::InterruptHandler<peripherals::SPI2>;
    SPIM3 => spim::InterruptHandler<peripherals::SPI3>;
    SAADC => embassy_nrf::saadc::InterruptHandler;
    TEMP => embassy_nrf::temp::InterruptHandler;
});

// The MPSL's interrupts beside ours (`ble.rs`); TEMP is the MPSL's.
#[cfg(not(feature = "console"))]
bind_interrupts!(struct Irqs {
    USBD => usb::InterruptHandler<peripherals::USBD>;
    CLOCK_POWER => vbus_detect::InterruptHandler, power::InterruptHandler, nrf_mpsl::ClockInterruptHandler;
    SPI2 => spim::InterruptHandler<peripherals::SPI2>;
    SPIM3 => spim::InterruptHandler<peripherals::SPI3>;
    SAADC => embassy_nrf::saadc::InterruptHandler;
    EGU0_SWI0 => nrf_mpsl::LowPrioInterruptHandler;
    RADIO => nrf_mpsl::HighPrioInterruptHandler;
    TIMER0 => nrf_mpsl::HighPrioInterruptHandler;
    RTC0 => nrf_mpsl::HighPrioInterruptHandler;
});

/// The priority of every interrupt of ours in the node build: below the
/// MPSL's radio timing at 0, whose deadlines nothing may delay
/// (`ble.rs`), and above its low-priority work at 4.
#[cfg(not(feature = "console"))]
const APP_PRIORITY: embassy_nrf::interrupt::Priority = embassy_nrf::interrupt::Priority::P2;

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

/// Which image this is, for the display and the log.
#[cfg(feature = "console")]
const IMAGE: &str = match console::SURVEY {
    None => "console",
    Some(_) => "survey",
};
#[cfg(not(feature = "console"))]
const IMAGE: &str = "node";

// A survey is a console; set for a node build the variable would be
// ignored, and the board would not do what was asked of it.
#[cfg(not(feature = "console"))]
const _: () = assert!(
    option_env!("BARK_SURVEY").is_none(),
    "BARK_SURVEY needs --features console"
);

/// The node's ID: the low 48 bits of the 64-bit factory device ID, all 32
/// of DEVICEID[0] and the low 16 of DEVICEID[1] (nRF52840 Product
/// Specification, FICR DEVICEID; spec "Node identity"). The same at every
/// boot.
pub fn node_id() -> NodeId {
    let ficr = embassy_nrf::pac::FICR;
    let low = ficr.deviceid(0).read() as u64;
    let high = ficr.deviceid(1).read() as u64;
    NodeId::from_u64(high << 32 | low)
}

/// The ID as twelve lowercase hex digits, the form every log line and the
/// USB serial number use.
pub fn node_id_hex() -> [u8; 2 * NodeId::LEN] {
    let digit = |n: u8| b"0123456789abcdef"[usize::from(n)];
    let mut hex = [0u8; 2 * NodeId::LEN];
    for (i, byte) in node_id().0.into_iter().enumerate() {
        hex[2 * i] = digit(byte >> 4);
        hex[2 * i + 1] = digit(byte & 0x0F);
    }
    hex
}

/// The boot picture: the ID in two rows of six digits, large, then the
/// image and its version.
fn boot_picture(hex: &[u8; 2 * NodeId::LEN]) -> display::Picture {
    // Twelve 5-pixel glyphs with a column between, at 2 pixels each, span
    // 142 of the 200 columns: the largest the whole ID fits on one row,
    // which reads as one word.
    const ID_SCALE: usize = 2;
    const TEXT_SCALE: usize = 2;
    const TOP: usize = 60;
    let mut picture = display::Picture::white();
    let mut y = TOP;
    picture.text(hex, ID_SCALE, y);
    y += display::line_height(ID_SCALE);
    y += display::line_height(TEXT_SCALE) / 2;
    picture.text(IMAGE.as_bytes(), TEXT_SCALE, y);
    y += display::line_height(TEXT_SCALE);
    let mut line = display::Line::<32>::new();
    let _ = write!(line, "V{}", update::BARK_IMAGE_VERSION);
    #[cfg(feature = "console")]
    if let Some(ch) = console::SURVEY {
        let _ = write!(line, " CH{}", ch.index());
    }
    picture.text(line.bytes(), TEXT_SCALE, y);
    picture
}

/// Heartbeat airtime as the core computes it (the heartbeat frame, minimum SF5
/// preamble). A compile-time call into `bark-proto` for the target.
const HEARTBEAT_AIRTIME_US: u64 = bark_proto::radio::time_on_air(
    bark_proto::wire::HEARTBEAT_LEN as u8,
    bark_proto::radio::MIN_PREAMBLE_SYMBOLS,
)
.as_micros();

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    // Read before anything else can panic and overwrite it.
    let previous_panic = panic_persist::get_panic_message_utf8();

    let mut config = Config::default();
    // USBD needs the 32 MHz crystal oscillator running (nRF52840 Product
    // Specification, USBD chapter). The console starts it at boot, for its
    // stopwatch too (clock.rs); in the node build the MPSL owns the clock,
    // starts the crystal for its radio, and holds it while asked to, on
    // USB (`on_usb_only`). The 32.768 kHz clock stays on the internal RC
    // oscillator, which assumes nothing about a fitted LF crystal.
    config.hfclk_source = match cfg!(feature = "console") {
        true => HfclkSource::ExternalXtal,
        false => HfclkSource::Internal,
    };
    #[cfg(not(feature = "console"))]
    {
        config.gpiote_interrupt_priority = APP_PRIORITY;
        config.time_interrupt_priority = APP_PRIORITY;
        use embassy_nrf::interrupt::{Interrupt, InterruptExt as _};
        for irq in [
            Interrupt::USBD,
            Interrupt::SPI2,
            Interrupt::SPIM3,
            Interrupt::SAADC,
        ] {
            irq.set_priority(APP_PRIORITY);
        }
    }
    // Mutable for the seed's borrow of the generator in the node build.
    #[cfg_attr(feature = "console", allow(unused_mut))]
    let mut p = embassy_nrf::init(config);
    #[cfg(not(feature = "console"))]
    let mpsl = ble::mpsl(ble::MpslParts {
        rtc0: p.RTC0,
        timer0: p.TIMER0,
        temp: p.TEMP,
        ppi19: p.PPI_CH19,
        ppi30: p.PPI_CH30,
        ppi31: p.PPI_CH31,
    });
    // The core's seed, before the controller takes the generator.
    #[cfg(not(feature = "console"))]
    let seed = embassy_nrf::rng::Rng::new_blocking(p.RNG.reborrow()).blocking_next_u64();
    let board = board::take_board!(p);

    // The ID as the USB serial number, so that the host names each board's
    // port by it (`/dev/serial/by-id/`). The descriptor is read for as
    // long as the device runs.
    static SERIAL: static_cell::StaticCell<[u8; 2 * NodeId::LEN]> = static_cell::StaticCell::new();
    let hex: &'static [u8; 2 * NodeId::LEN] = SERIAL.init(node_id_hex());
    let serial = core::str::from_utf8(hex).expect("hex digits are ASCII");

    // USB device with one CDC-ACM interface carrying the log.
    let driver = usb::Driver::new(board.usbd, Irqs, HardwareVbusDetect::new(Irqs));
    let mut usb_config = embassy_usb::Config::new(USB_VID, USB_PID);
    usb_config.manufacturer = Some("bark");
    usb_config.product = Some("bark T-Echo");
    usb_config.serial_number = Some(serial);
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

    log::info!(
        "bark-fw {} on LILYGO T-Echo, {} image version {}, id={}",
        env!("CARGO_PKG_VERSION"),
        IMAGE,
        update::BARK_IMAGE_VERSION,
        serial
    );
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

    #[cfg(feature = "console")]
    let blink = on_usb_only(board.leds.green);
    #[cfg(not(feature = "console"))]
    let blink = on_usb_only(board.leds.green, mpsl);
    #[cfg(not(feature = "console"))]
    let phone = embassy_futures::join::join(mpsl.run(), async {
        let parts = nrf_sdc::Peripherals::new(
            p.PPI_CH17, p.PPI_CH18, p.PPI_CH20, p.PPI_CH21, p.PPI_CH22, p.PPI_CH23, p.PPI_CH24,
            p.PPI_CH25, p.PPI_CH26, p.PPI_CH27, p.PPI_CH28, p.PPI_CH29,
        );
        match ble::controller(parts, p.RNG, mpsl) {
            Ok(c) => ble::run(c, node_id(), update::BARK_IMAGE_VERSION).await,
            Err(e) => log::error!("ble: controller {:?}", e),
        }
    });
    #[cfg(feature = "console")]
    let phone = async {};
    let shared = update::flash(embassy_nrf::nvmc::Nvmc::new(p.NVMC));

    #[cfg(feature = "console")]
    let clock = clock::Clock::new(p.TIMER1);
    #[cfg(not(feature = "console"))]
    let clock = clock::Clock::new();
    let battery = battery::Battery::new(embassy_nrf::saadc::Saadc::new(
        p.SAADC,
        Irqs,
        battery::Battery::config(),
        [battery::Battery::channel(board.battery.adc)],
    ));
    #[cfg(feature = "console")]
    let mut lines = console::Lines::new();
    #[cfg(not(feature = "console"))]
    let (config, core, flash) = {
        let (flash, stored) = store::Flash::open(shared);
        let config = node::config(stored);
        (config, node::core(config, seed), flash)
    };
    // Where the radio's self-check reads the channel at boot: the home
    // channel if one is stored, else the console's or the first.
    #[cfg(feature = "console")]
    let channel = console::SURVEY.unwrap_or(bark_proto::Channel::new(0).expect("in the plan"));
    #[cfg(not(feature = "console"))]
    let channel = config
        .stored
        .map_or(bark_proto::Channel::new(0).expect("in the plan"), |s| {
            s.home
        });
    let radio_task = async {
        let t0 = clock.now();
        match radio::boot(
            board.radio,
            board.power,
            p.SPI2,
            Irqs,
            #[cfg(feature = "console")]
            radio::Thermometer::Sensor(embassy_nrf::temp::Temp::new(p.TEMP, Irqs)),
            #[cfg(not(feature = "console"))]
            radio::Thermometer::Mpsl(mpsl),
            &clock,
            channel,
        )
        .await
        {
            Ok(radio) => {
                log::info!(
                    "radio up in {} us, tcxo {} us after {} probes, lbt reading {:?}",
                    clock.since(t0),
                    radio.tcxo().delay.as_micros(),
                    radio.tcxo().probes,
                    radio.lbt_read()
                );
                update::started(shared);
                #[cfg(feature = "console")]
                console::Console::new(radio, &clock, battery, shared)
                    .run(&mut lines)
                    .await;
                #[cfg(not(feature = "console"))]
                node::run(core, radio, flash, battery, &config, &clock).await;
            }
            // Without a radio there is nothing to measure; USB and the log
            // keep running so the error can be read.
            Err(e) => log::error!("radio init failed: {:?}", e),
        }
    };

    let picture = boot_picture(hex);
    // After the radio's future, which raises the switched rail first thing.
    let shown = display::show(board.eink, p.SPI3, Irqs, &picture);

    join3(
        join5(
            device.run(),
            log_sink,
            blink,
            radio_task,
            update::watchdog(p.WDT),
        ),
        shown,
        phone,
    )
    .await;
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
async fn on_usb_only(
    pin: embassy_nrf::Peri<'static, peripherals::P1_01>,
    #[cfg(not(feature = "console"))] mpsl: &'static nrf_mpsl::MultiprotocolServiceLayer<'static>,
) {
    use embassy_futures::select::{select, Either};
    use embassy_nrf::gpio::{Level, Output, OutputDrive};
    // LED assumed active low (board.rs, Leds): start dark.
    let mut green = Output::new(pin, Level::High, OutputDrive::Standard);
    let mut half_periods: u32 = 0;
    // The crystal, held for USB while on it: asked of the MPSL, which owns
    // the clock in the node build (`ble.rs`); the console starts it for
    // good at boot.
    #[cfg(not(feature = "console"))]
    let mut crystal = None;
    loop {
        let usb = power::usb();
        if !usb {
            green.set_high();
            #[cfg(not(feature = "console"))]
            drop(crystal.take());
            power::FOR_LED.wait().await;
            continue;
        }
        #[cfg(not(feature = "console"))]
        if crystal.is_none() {
            crystal = mpsl.request_hfclk().await.ok();
        }
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
