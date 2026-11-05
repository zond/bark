//! bark firmware for the LILYGO T-Echo.
//!
//! Blinks the green LED at 1 Hz while on USB, logs over USB CDC-ACM,
//! shows the node ID and the image on the e-paper (`display.rs`), powers
//! up the SX1262 at bark's modulation and runs the protocol core on it
//! (`node.rs`), a relay on USB and sleepy on battery (`power.rs`), or
//! with the `console` feature the bring-up measurement console
//! (`console.rs`) on the serial port. Reads the battery (`battery.rs`).
//! The node build runs the phone link (`ble.rs`) and the GPS (`gps.rs`).

#![no_std]
#![no_main]

mod battery;
#[cfg(not(feature = "console"))]
mod ble;
mod board;
#[cfg(not(feature = "console"))]
mod boot;
mod clock;
#[cfg(feature = "console")]
mod console;
mod display;
#[cfg(not(feature = "console"))]
mod gps;
#[cfg(not(feature = "console"))]
mod lfclk;
#[cfg(not(feature = "console"))]
mod node;
mod power;
mod radio;
#[cfg(not(feature = "console"))]
mod stack;
#[cfg(not(feature = "console"))]
mod store;
mod touch;
mod update;
#[cfg(not(feature = "console"))]
mod utc;

#[cfg(feature = "console")]
use core::fmt::Write as _;

use bark_proto::NodeId;
use embassy_executor::Spawner;
#[cfg(feature = "console")]
use embassy_futures::join::join3;
use embassy_nrf::config::{Config, HfclkSource};
use embassy_nrf::usb::vbus_detect::{self, HardwareVbusDetect};
use embassy_nrf::{bind_interrupts, peripherals, spim, usb};
use embassy_time::Instant;
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
// The input handler is constructed through this trait.
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
    TWISPI1 => spim::InterruptHandler<peripherals::TWISPI1>;
    SAADC => embassy_nrf::saadc::InterruptHandler;
    TEMP => embassy_nrf::temp::InterruptHandler;
});

// The MPSL's interrupts beside ours (`ble.rs`); TEMP is the MPSL's.
#[cfg(not(feature = "console"))]
bind_interrupts!(struct Irqs {
    USBD => usb::InterruptHandler<peripherals::USBD>;
    CLOCK_POWER => vbus_detect::InterruptHandler, power::InterruptHandler, nrf_mpsl::ClockInterruptHandler;
    SPI2 => spim::InterruptHandler<peripherals::SPI2>;
    TWISPI1 => spim::InterruptHandler<peripherals::TWISPI1>;
    SAADC => embassy_nrf::saadc::InterruptHandler;
    EGU0_SWI0 => nrf_mpsl::LowPrioInterruptHandler;
    RADIO => nrf_mpsl::HighPrioInterruptHandler;
    TIMER0 => nrf_mpsl::HighPrioInterruptHandler;
    RTC0 => nrf_mpsl::HighPrioInterruptHandler;
    UARTE1 => embassy_nrf::uarte::InterruptHandler<peripherals::UARTE1>;
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
#[cfg(feature = "console")]
fn boot_picture(hex: &[u8; 2 * NodeId::LEN]) -> bark_screen::Picture {
    // Twelve 5-pixel glyphs with a column between, at 2 pixels each, span
    // 142 of the 200 columns: the largest the whole ID fits on one row,
    // which reads as one word.
    const ID_SCALE: usize = 2;
    const TEXT_SCALE: usize = 2;
    const TOP: usize = 60;
    let mut picture = bark_screen::Picture::white();
    let mut y = TOP;
    picture.text(hex, ID_SCALE, y);
    y += bark_screen::line_height(ID_SCALE);
    y += bark_screen::line_height(TEXT_SCALE) / 2;
    picture.text(IMAGE.as_bytes(), TEXT_SCALE, y);
    y += bark_screen::line_height(TEXT_SCALE);
    let mut line = bark_screen::Line::<32>::new();
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
async fn main(spawner: Spawner) {
    // Read before anything else can panic and overwrite it.
    let previous_panic = panic_persist::get_panic_message_utf8();
    #[cfg(not(feature = "console"))]
    stack::paint();

    let mut config = Config::default();
    // USBD needs the 32 MHz crystal oscillator running (nRF52840 Product
    // Specification, USBD chapter). The console starts it at boot, for its
    // stopwatch too (clock.rs); in the node build the MPSL owns the clock,
    // starts the crystal for its radio, and holds it while asked to, on
    // USB (`on_usb_only`). The 32.768 kHz clock starts on the internal RC
    // oscillator, which assumes nothing about a fitted crystal; the node
    // build then tries the crystal (`lfclk.rs`), and the console keeps the
    // RC oscillator.
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
            Interrupt::TWISPI1,
            Interrupt::SAADC,
            Interrupt::UARTE1,
        ] {
            irq.set_priority(APP_PRIORITY);
        }
    }
    // Mutable for the seed's borrow of the generator in the node build.
    #[cfg_attr(feature = "console", allow(unused_mut))]
    let mut p = embassy_nrf::init(config);
    // The 32.768 kHz clock's source, before the MPSL takes the clock.
    #[cfg(not(feature = "console"))]
    let lf_source = lfclk::probe();
    #[cfg(not(feature = "console"))]
    boot::note(|f| f.lfclk = Some(lf_source));
    #[cfg(not(feature = "console"))]
    let mpsl = ble::mpsl(
        ble::MpslParts {
            rtc0: p.RTC0,
            timer0: p.TIMER0,
            temp: p.TEMP,
            ppi19: p.PPI_CH19,
            ppi30: p.PPI_CH30,
            ppi31: p.PPI_CH31,
        },
        lf_source,
    );
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

    // USB device with one CDC-ACM interface carrying the log. Its buffers
    // live in a static, since the device runs in a task of its own for as
    // long as the firmware runs.
    let driver = usb::Driver::new(board.usbd, Irqs, HardwareVbusDetect::new(Irqs));
    let mut usb_config = embassy_usb::Config::new(USB_VID, USB_PID);
    usb_config.manufacturer = Some("bark");
    usb_config.product = Some("bark T-Echo");
    usb_config.serial_number = Some(serial);
    // Current we ask the host for, mA: one unit load, granted by every port
    // without negotiation (USB 2.0 §7.2.1).
    usb_config.max_power = 100;
    usb_config.max_packet_size_0 = USB_PACKET as u8;
    static USB: static_cell::StaticCell<UsbParts> = static_cell::StaticCell::new();
    let UsbParts {
        config_descriptor,
        bos_descriptor,
        msos_descriptor,
        control_buf,
        cdc_state,
        touch,
    } = USB.init(UsbParts {
        config_descriptor: [0; 128],
        bos_descriptor: [0; 16],
        msos_descriptor: [0; 256],
        control_buf: [0; 64],
        cdc_state: State::new(),
        // The serial port's way into the bootloader (`touch.rs`).
        touch: touch::Touch,
    });
    let mut builder = embassy_usb::Builder::new(
        driver,
        usb_config,
        config_descriptor,
        bos_descriptor,
        msos_descriptor,
        control_buf,
    );
    // Before the class, so that it sees the line coding first.
    builder.handler(touch);
    let class = CdcAcmClass::new(&mut builder, cdc_state, USB_PACKET);
    spawner.spawn(usb_device(builder.build()).expect("spawned once"));

    // The global logger, installed now; its task drains it to the host
    // once the host asserts DTR (opens the port), and feeds what the host
    // sends to the console.
    static LOGGER: static_cell::ConstStaticCell<Logger> =
        static_cell::ConstStaticCell::new(Logger::with_custom_style(log_line));
    let logger = LOGGER.take();
    logger.with_handler(Input::new());
    let logger: &'static Logger = logger;
    if log::set_logger(logger).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
    spawner.spawn(log_sink(logger, class).expect("spawned once"));

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
    #[cfg(not(feature = "console"))]
    match lf_source {
        lfclk::Source::Crystal { started_ms } => {
            log::info!("lfclk: crystal, started in {} ms", started_ms)
        }
        lfclk::Source::Rc => log::warn!("lfclk: no crystal started; RC oscillator, calibrated"),
    }
    if let Some(msg) = previous_panic {
        log::error!("previous boot panicked: {}", msg);
    }
    spawner.spawn(touch_task().expect("spawned once"));
    spawner.spawn(watchdog(p.WDT).expect("spawned once"));
    let battery = battery::Battery::new(embassy_nrf::saadc::Saadc::new(
        p.SAADC,
        Irqs,
        battery::Battery::config(),
        [battery::Battery::channel(board.battery.adc)],
    ));

    #[cfg(feature = "console")]
    {
        // The console keeps its one-shot structure in this task: it holds
        // no relay, and its frame is small.
        let shared = update::flash(embassy_nrf::nvmc::Nvmc::new(p.NVMC));
        let clock = clock::Clock::new(p.TIMER1);
        let mut lines = console::Lines::new();
        let channel = console::SURVEY.unwrap_or(bark_proto::Channel::new(0).expect("in the plan"));
        let radio_task = async {
            let t0 = clock.now();
            match radio::boot(
                board.radio,
                board.power,
                p.SPI2,
                Irqs,
                radio::Thermometer::Sensor(embassy_nrf::temp::Temp::new(p.TEMP, Irqs)),
                &clock,
                channel,
            )
            .await
            {
                Ok(radio) => {
                    radio_up(&radio, &clock, t0);
                    update::started(shared);
                    console::Console::new(radio, &clock, battery, shared)
                        .run(&mut lines)
                        .await;
                }
                // Without a radio there is nothing to measure; USB and the
                // log keep running so the error can be read.
                Err(e) => log::error!("radio init failed: {:?}", e),
            }
        };
        let picture = boot_picture(hex);
        // After the radio's future, which raises the switched rail first
        // thing.
        let shown = display::show(board.eink, p.TWISPI1, Irqs, &picture);
        join3(on_usb_only(board.leds.green), radio_task, shown).await;
    }

    // The node: every long-lived part a task of its own, so that none
    // passes through this task's stack frame, and this task ends here.
    #[cfg(not(feature = "console"))]
    {
        spawner.spawn(blink(board.leds.green, mpsl).expect("spawned once"));
        spawner.spawn(mpsl_task(mpsl).expect("spawned once"));
        let ble_parts = nrf_sdc::Peripherals::new(
            p.PPI_CH17, p.PPI_CH18, p.PPI_CH20, p.PPI_CH21, p.PPI_CH22, p.PPI_CH23, p.PPI_CH24,
            p.PPI_CH25, p.PPI_CH26, p.PPI_CH27, p.PPI_CH28, p.PPI_CH29,
        );
        spawner.spawn(phone(ble_parts, p.RNG, mpsl).expect("spawned once"));
        // The node's flash: the stored state through the MPSL's timeslots
        // (`store.rs`), and the update's confirmation through the NVMC
        // driver directly, which runs once at boot before the BLE
        // controller starts (`ble::START`).
        let mpsl_flash = nrf_mpsl::Flash::take(mpsl, p.NVMC);
        // SAFETY: a second handle on the NVMC, for `update::started` alone,
        // which writes before the controller starts and so before anything
        // writes through the MPSL: the core asks to store only once the
        // node runs, after `update::started`.
        let shared = update::flash(embassy_nrf::nvmc::Nvmc::new(unsafe {
            embassy_nrf::peripherals::NVMC::steal()
        }));
        let (flash, stored, offset_min) = store::Flash::open(mpsl_flash);
        utc::init(utc::ppm(lf_source), offset_min);
        let config = node::config(stored);
        let core = node::core(config, seed);
        // Its flash writes in a task of their own (`store.rs`).
        spawner.spawn(store::run(flash).expect("spawned once"));
        // Its own task, so that its UART's bytes wake nothing else.
        spawner.spawn(
            gps::run(gps::Parts {
                uarte: p.UARTE1,
                timer: p.TIMER2,
                ppi0: p.PPI_CH0,
                ppi1: p.PPI_CH1,
                pins: board.gps,
            })
            .expect("spawned once"),
        );
        spawner.spawn(
            node_radio(NodeRadio {
                radio: board.radio,
                power: board.power,
                spi: p.SPI2,
                mpsl,
                shared,
                battery,
                core,
                config,
            })
            .expect("spawned once"),
        );
        // After the radio's task, which raises the switched rail first
        // thing.
        let fixed = bark_screen::Fixed {
            id: hex,
            image: IMAGE,
            version: update::BARK_IMAGE_VERSION,
        };
        spawner.spawn(screen(board.eink, p.TWISPI1, fixed).expect("spawned once"));
    }
}

/// The USB device's buffers and its two handlers, kept for as long as it
/// runs.
struct UsbParts {
    config_descriptor: [u8; 128],
    bos_descriptor: [u8; 16],
    msos_descriptor: [u8; 256],
    control_buf: [u8; 64],
    cdc_state: State<'static>,
    touch: touch::Touch,
}

type UsbDriver = usb::Driver<'static, HardwareVbusDetect>;
type Logger = embassy_usb_logger::UsbLogger<LOG_BUFFER, Input>;

/// One log line as the host reads it: the time since boot, the level and
/// the message; in the console, the host's own command echoed.
fn log_line(record: &log::Record<'_>, writer: &mut embassy_usb_logger::Writer<'_, LOG_BUFFER>) {
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
}

/// The USB device.
#[embassy_executor::task]
async fn usb_device(mut device: embassy_usb::UsbDevice<'static, UsbDriver>) {
    device.run().await
}

/// The log, to the host.
#[embassy_executor::task]
async fn log_sink(logger: &'static Logger, class: CdcAcmClass<'static, UsbDriver>) {
    logger.create_future_from_class(class).await
}

/// The bootloader from the serial port (`touch.rs`).
#[embassy_executor::task]
async fn touch_task() {
    touch::run().await
}

/// The watchdog of an image on trial (`update.rs`).
#[embassy_executor::task]
async fn watchdog(dog: embassy_nrf::Peri<'static, peripherals::WDT>) {
    update::watchdog(dog).await
}

/// The radio's start, logged.
fn radio_up(radio: &radio::Radio<'_>, clock: &clock::Clock, t0: u32) {
    log::info!(
        "radio up in {} us, tcxo {} us after {} probes, lbt reading {:?}",
        clock.since(t0),
        radio.tcxo().delay.as_micros(),
        radio.tcxo().probes,
        radio.lbt_read()
    );
}

/// The LED and the crystal while on USB (`on_usb_only`).
#[cfg(not(feature = "console"))]
#[embassy_executor::task]
async fn blink(
    pin: embassy_nrf::Peri<'static, peripherals::P1_01>,
    mpsl: &'static nrf_mpsl::MultiprotocolServiceLayer<'static>,
) {
    on_usb_only(pin, mpsl).await
}

/// The MPSL's own work (`ble.rs`).
#[cfg(not(feature = "console"))]
#[embassy_executor::task]
async fn mpsl_task(mpsl: &'static nrf_mpsl::MultiprotocolServiceLayer<'static>) {
    mpsl.run().await
}

/// The phone link: the controller once the image has confirmed itself
/// (`ble::START`), and the host and the service over it (`ble.rs`).
#[cfg(not(feature = "console"))]
#[embassy_executor::task]
async fn phone(
    parts: nrf_sdc::Peripherals<'static>,
    rng: embassy_nrf::Peri<'static, peripherals::RNG>,
    mpsl: &'static nrf_mpsl::MultiprotocolServiceLayer<'static>,
) {
    ble::START.wait().await;
    match ble::controller(parts, rng, mpsl) {
        Ok(c) => ble::run(c, node_id(), update::BARK_IMAGE_VERSION).await,
        Err(e) => log::error!("ble: controller {:?}", e),
    }
}

/// What the radio's task takes.
#[cfg(not(feature = "console"))]
struct NodeRadio {
    radio: board::Radio,
    power: board::Power,
    spi: embassy_nrf::Peri<'static, peripherals::SPI2>,
    mpsl: &'static nrf_mpsl::MultiprotocolServiceLayer<'static>,
    shared: &'static update::Flash,
    battery: battery::Battery<'static>,
    core: &'static mut node::Role,
    config: bark_proto::Config,
}

/// The radio and the core on it: the radio up, the image confirmed, the
/// phone link let start, and the driver's loop (`node.rs`).
#[cfg(not(feature = "console"))]
#[embassy_executor::task]
async fn node_radio(n: NodeRadio) {
    let clock = clock::Clock::new();
    // Where the radio's self-check reads the channel at boot: the home
    // channel if one is stored, else the first.
    let channel = n
        .config
        .stored
        .map_or(bark_proto::Channel::new(0).expect("in the plan"), |s| {
            s.home
        });
    let t0 = clock.now();
    match radio::boot(
        n.radio,
        n.power,
        n.spi,
        Irqs,
        radio::Thermometer::Mpsl(n.mpsl),
        &clock,
        channel,
    )
    .await
    {
        Ok(radio) => {
            radio_up(&radio, &clock, t0);
            update::started(n.shared);
            ble::START.signal(());
            node::run(n.core, radio, n.battery, &n.config, &clock).await;
        }
        // Without a radio USB and the log keep running so the error can be
        // read, and the phone link still comes up, so the node can be found.
        Err(e) => {
            log::error!("radio init failed: {:?}", e);
            ble::START.signal(());
        }
    }
}

/// The node's screen (`display::run`).
#[cfg(not(feature = "console"))]
#[embassy_executor::task]
async fn screen(
    pins: board::Eink,
    spim: embassy_nrf::Peri<'static, peripherals::TWISPI1>,
    fixed: bark_screen::Fixed<'static>,
) {
    display::run(pins, spim, Irqs, fixed).await
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
