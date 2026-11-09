//! The e-paper: the panel driven through its controller. The console
//! draws one picture at boot ([`show`]); the node draws its state
//! (`bark-screen`) at boot and again when it changes, seldom ([`run`]),
//! the panel asleep between.
//!
//! The 1.54" panel (GDEH0154D67, 200 × 200, `board::Eink`) is driven by
//! an SSD1681, whose commands below are from its datasheet's command
//! table ("DS"). The controller holds the picture in its own RAM, one bit
//! a pixel, so after one full refresh it can be put in deep sleep, where
//! it draws nothing and keeps the picture on the glass; only a hardware
//! reset wakes it, so each refresh starts from one. The panel has its own
//! SPI (SPIM1) and never shares the radio's bus, and its refreshes run as
//! a task of their own beside the radio's loop (`main.rs`, `screen`): every wait is on an
//! interrupt, bounded, and the picture is drawn a line at a time with the
//! executor given back between lines, so that the radio's loop waits on
//! it for no more than one line's drawing.
//!
//! Not SPIM3, the nRF52840's fast instance: its transmitted data can be
//! corrupted while the CPU or another EasyDMA master uses the same RAM
//! block as its buffer (nRF52840 Errata, anomaly 198). The node, with the
//! BLE controller and the radio busy beside the panel, sent the panel
//! commands it did not act on, where the console drew. SPIM1 has no such
//! erratum, and the panel needs no more than its 8 MHz.

use embassy_nrf::gpio::{Input, Level, Output, OutputDrive, Pull};
use embassy_nrf::interrupt::typelevel::{Binding, TWISPI1};
use embassy_nrf::peripherals;
use embassy_nrf::spim::{self, Spim};
use embassy_nrf::Peri;
use embassy_time::{with_timeout, Duration, Instant, Timer};

#[cfg(not(feature = "console"))]
use bark_screen::State;
use bark_screen::{Picture, HEIGHT, ROW_BYTES};

use crate::board;
use crate::radio::RAIL_SETTLE;

/// A full refresh as the board's label gives it, 2 s (`board::Eink`).
const REFRESH: Duration = Duration::from_millis(bark_screen::schedule::REFRESH_MS);

/// How long after MASTER_ACTIVATION BUSY must have risen: far more than
/// the controller's reaction to a command, far less than the refresh.
const REFRESH_START: Duration = Duration::from_millis(100);

/// Bound on any one BUSY-high stretch: three refreshes. Past it the
/// controller is absent, unpowered or wedged, and the boot goes on without
/// a picture instead of hanging.
const BUSY_TIMEOUT: Duration = Duration::from_secs(3 * REFRESH.as_secs());

/// RES# held low, and the wait after it rises before the first command.
/// The datasheet asks for microseconds; 10 ms costs nothing once a boot.
const RESET_PULSE: Duration = Duration::from_millis(10);

// Commands (DS, command table). Each is followed by its parameter bytes.
/// Driver Output Control: the number of gate lines less one, and the scan
/// order.
const DRIVER_OUTPUT_CONTROL: u8 = 0x01;
/// Deep Sleep Mode: parameter 0x01 is mode 1, RAM kept, exit by RES# only.
const DEEP_SLEEP: u8 = 0x10;
/// Data Entry Mode: parameter 0x03 steps X then Y, both increasing, so RAM
/// is written row by row from the top-left corner.
const DATA_ENTRY_MODE: u8 = 0x11;
/// Software Reset: the command and parameter registers to their defaults;
/// BUSY is high while it runs.
const SW_RESET: u8 = 0x12;
/// Temperature Sensor Control: parameter 0x80 selects the internal sensor,
/// which the waveform's timing is read against.
const TEMPERATURE_SENSOR: u8 = 0x18;
/// Master Activation: run the update sequence set by Display Update
/// Control 2; BUSY is high until it ends.
const MASTER_ACTIVATION: u8 = 0x20;
/// Display Update Control 2: parameter 0xF7 is the whole sequence of a full
/// refresh (clock and analog on, temperature read, the waveform loaded from
/// the controller's own storage, the display driven, analog and clock off).
const DISPLAY_UPDATE_CONTROL_2: u8 = 0x22;
/// Write RAM, the black-and-white plane.
const WRITE_RAM_BW: u8 = 0x24;
/// Write RAM, the second plane, which the full-refresh waveform reads as
/// the previous picture; written with the same picture so nothing is left
/// undefined.
const WRITE_RAM_RED: u8 = 0x26;
/// Border Waveform Control: parameter 0x05 drives the border through the
/// picture's own waveform, so it refreshes to white with the rest.
const BORDER_WAVEFORM: u8 = 0x3C;
/// RAM X address start and end, in bytes of 8 pixels.
const RAM_X_RANGE: u8 = 0x44;
/// RAM Y address start and end, rows.
const RAM_Y_RANGE: u8 = 0x45;
/// RAM X address counter: where the next Write RAM starts.
const RAM_X_COUNTER: u8 = 0x4E;
/// RAM Y address counter.
const RAM_Y_COUNTER: u8 = 0x4F;

/// How the boot picture went, for the node's `boot:` line (`boot.rs`);
/// the console logs it only.
#[cfg_attr(feature = "console", allow(dead_code))]
#[derive(Clone, Copy, Debug)]
pub enum Outcome {
    /// On the glass, this long after boot.
    Shown { ms: u32 },
    /// BUSY stayed high past [`BUSY_TIMEOUT`] at this step.
    Busy(&'static str),
    /// The refresh never started: the panel did not act on the commands.
    NoRefresh,
    /// The bus failed.
    Spi,
}

/// The outcome of [`show`], once known.
pub static OUTCOME: embassy_sync::blocking_mutex::Mutex<
    embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
    core::cell::Cell<Option<Outcome>>,
> = embassy_sync::blocking_mutex::Mutex::new(core::cell::Cell::new(None));

/// Why the picture did not reach the glass.
#[derive(Debug)]
enum Error {
    /// BUSY stayed high past [`BUSY_TIMEOUT`] at this step.
    Busy(&'static str),
    /// BUSY never rose after the refresh was asked for.
    NoRefresh,
    Spi(spim::Error),
}

impl From<spim::Error> for Error {
    fn from(e: spim::Error) -> Error {
        Error::Spi(e)
    }
}

/// The panel's pins and bus while it is being written.
struct Panel {
    spi: Spim<'static>,
    cs: Output<'static>,
    dc: Output<'static>,
    busy: Input<'static>,
}

impl Panel {
    /// One command with its parameter bytes. CS frames the whole, and D/C
    /// tells the controller which bytes are which (DS, serial interface).
    async fn command(&mut self, command: u8, data: &[u8]) -> Result<(), Error> {
        self.cs.set_low();
        self.dc.set_low();
        let result = async {
            self.spi.write(&[command]).await?;
            self.dc.set_high();
            if !data.is_empty() {
                self.spi.write(data).await?;
            }
            Ok(())
        }
        .await;
        self.cs.set_high();
        result
    }

    /// Wait for the controller to finish `step`.
    async fn wait_ready(&mut self, step: &'static str) -> Result<(), Error> {
        with_timeout(BUSY_TIMEOUT, self.busy.wait_for_low())
            .await
            .map_err(|_| Error::Busy(step))
    }

    /// From reset to the picture on the glass and the controller asleep.
    async fn show(&mut self, picture: &Picture) -> Result<(), Error> {
        self.wait_ready("reset").await?;
        self.command(SW_RESET, &[]).await?;
        self.wait_ready("software reset").await?;
        let last_row = (HEIGHT - 1) as u8;
        let last_column = (ROW_BYTES - 1) as u8;
        self.command(DRIVER_OUTPUT_CONTROL, &[last_row, 0x00, 0x00])
            .await?;
        self.command(DATA_ENTRY_MODE, &[0x03]).await?;
        self.command(RAM_X_RANGE, &[0x00, last_column]).await?;
        self.command(RAM_Y_RANGE, &[0x00, 0x00, last_row, 0x00])
            .await?;
        self.command(BORDER_WAVEFORM, &[0x05]).await?;
        self.command(TEMPERATURE_SENSOR, &[0x80]).await?;
        for plane in [WRITE_RAM_BW, WRITE_RAM_RED] {
            self.command(RAM_X_COUNTER, &[0x00]).await?;
            self.command(RAM_Y_COUNTER, &[0x00, 0x00]).await?;
            self.command(plane, &picture.bytes).await?;
        }
        self.command(DISPLAY_UPDATE_CONTROL_2, &[0xF7]).await?;
        self.command(MASTER_ACTIVATION, &[]).await?;
        // A refresh holds BUSY high for its 2 s; a controller that never
        // raises it did not take the commands, and the glass is unchanged.
        with_timeout(REFRESH_START, self.busy.wait_for_high())
            .await
            .map_err(|_| Error::NoRefresh)?;
        self.wait_ready("refresh").await?;
        self.command(DEEP_SLEEP, &[0x01]).await
    }
}

/// The panel set up: its pins driven, RES# among them, for as long as the
/// firmware runs, since a disconnected RES# could wake the controller into
/// drawing again. The pins are driven only once the switched rail is up,
/// which the radio raises first thing and holds for good
/// (`radio::boot`), as a pin driven high into an unpowered chip feeds it
/// through its protection diodes.
struct Screen {
    panel: Panel,
    reset: Output<'static>,
}

impl Screen {
    async fn new(
        pins: board::Eink,
        spim: Peri<'static, peripherals::TWISPI1>,
        irq: impl Binding<TWISPI1, spim::InterruptHandler<peripherals::TWISPI1>> + 'static,
    ) -> Screen {
        let reset = Output::new(pins.reset, Level::Low, OutputDrive::Standard);
        Timer::after(RAIL_SETTLE).await;
        let mut config = spim::Config::default();
        // 4 MHz writes the 5000-byte picture in 10 ms, nothing beside the
        // 2 s refresh, and leaves the flex cable to the panel margin. Mode
        // 0 is the controller's (DS, serial interface).
        config.frequency = spim::Frequency::M4;
        config.mode = spim::MODE_0;
        let panel = Panel {
            spi: Spim::new_txonly(spim, irq, pins.sclk, pins.mosi, config),
            cs: Output::new(pins.cs, Level::High, OutputDrive::Standard),
            dc: Output::new(pins.dc, Level::Low, OutputDrive::Standard),
            busy: Input::new(pins.busy, Pull::None),
        };
        Screen { panel, reset }
    }

    /// Wake the controller by a hardware reset, draw `picture`, and put it
    /// back to sleep; how it went, logged.
    async fn draw(&mut self, picture: &Picture) -> Outcome {
        let t0 = Instant::now();
        self.reset.set_low();
        Timer::after(RESET_PULSE).await;
        self.reset.set_high();
        Timer::after(RESET_PULSE).await;
        match self.panel.show(picture).await {
            Ok(()) => {
                let ms = t0.elapsed().as_millis() as u32;
                log::info!("display: shown in {} ms, asleep", ms);
                Outcome::Shown { ms }
            }
            Err(Error::Busy(step)) => {
                log::warn!(
                    "display: busy past {} s at {}; no picture",
                    BUSY_TIMEOUT.as_secs(),
                    step
                );
                Outcome::Busy(step)
            }
            Err(Error::NoRefresh) => {
                log::warn!("display: the refresh never started; no picture");
                Outcome::NoRefresh
            }
            Err(Error::Spi(e)) => {
                log::warn!("display: spi {:?}; no picture", e);
                Outcome::Spi
            }
        }
    }
}

/// Show `picture` and leave the panel asleep with it, for good.
#[cfg(feature = "console")]
pub async fn show(
    pins: board::Eink,
    spim: Peri<'static, peripherals::TWISPI1>,
    irq: impl Binding<TWISPI1, spim::InterruptHandler<peripherals::TWISPI1>> + 'static,
    picture: &Picture,
) -> ! {
    let mut screen = Screen::new(pins, spim, irq).await;
    let outcome = screen.draw(picture).await;
    OUTCOME.lock(|o| o.set(Some(outcome)));
    core::future::pending().await
}

/// What the node shows of its state, as it last published it, and a
/// signal at each change.
#[cfg(not(feature = "console"))]
static STATE: embassy_sync::blocking_mutex::Mutex<
    embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
    core::cell::Cell<State>,
> = embassy_sync::blocking_mutex::Mutex::new(core::cell::Cell::new(State {
    starting: true,
    placed: false,
    sleepy: false,
    anchored: false,
    carried: false,
    battery: bark_screen::Battery::NotRead,
    heard: 0,
    strong: 0,
    both_ways: 0,
    phone: bark_screen::Phone::Unpaired,
    getting: None,
}));

/// The phone link's line (`ble.rs`), apart from the node's state, as it
/// changes outside the node's steps.
#[cfg(not(feature = "console"))]
static PHONE: embassy_sync::blocking_mutex::Mutex<
    embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
    core::cell::Cell<bark_screen::Phone>,
> = embassy_sync::blocking_mutex::Mutex::new(core::cell::Cell::new(bark_screen::Phone::Unpaired));
#[cfg(not(feature = "console"))]
static CHANGED: embassy_sync::signal::Signal<
    embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
    (),
> = embassy_sync::signal::Signal::new();

/// A person at the node: true asks for the lines, false is a touch of
/// the pad, which flips the picture.
#[cfg(not(feature = "console"))]
static PERSON: embassy_sync::signal::Signal<
    embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
    bool,
> = embassy_sync::signal::Signal::new();

/// The pad's side (`button.rs`): a touch.
#[cfg(not(feature = "console"))]
pub fn touched() {
    PERSON.signal(false);
}

/// The button's side (`button.rs`): the lines, for the role it forced.
#[cfg(not(feature = "console"))]
pub fn lines() {
    PERSON.signal(true);
}

/// The phone link's line at boot, drawn with the first picture.
#[cfg(not(feature = "console"))]
pub fn phone_at_boot(phone: bark_screen::Phone) {
    PHONE.lock(|p| p.set(phone));
}

/// The phone link's side: its line, a new pairing code drawn at once and
/// any other change about one refresh after it (`bark_screen::schedule`).
#[cfg(not(feature = "console"))]
pub fn phone(phone: bark_screen::Phone) {
    if PHONE.lock(|p| p.replace(phone)) != phone {
        CHANGED.signal(());
    }
}

/// The node's side, after a step: its state as the screen says it, but
/// for the phone's line, which the phone link gives ([`phone`]).
#[cfg(not(feature = "console"))]
pub fn publish(state: State) {
    if STATE.lock(|s| s.replace(state)) != state {
        CHANGED.signal(());
    }
}

/// How long the first picture waits for the node's first state.
#[cfg(not(feature = "console"))]
const FIRST_STATE: Duration = Duration::from_secs(5);

/// Draw the node's screen at boot, and again as `bark_screen::schedule`
/// says: the code, the lines for a while after a touch of the pad
/// ([`touched`]) or a long press of the button ([`lines`]), a new
/// pairing code at once, and a change a person acts on in the picture
/// shown about one refresh after it. Each picture is drawn to its end
/// before the next is considered, so none starts while a refresh runs.
#[cfg(not(feature = "console"))]
pub async fn run(
    pins: board::Eink,
    spim: Peri<'static, peripherals::TWISPI1>,
    irq: impl Binding<TWISPI1, spim::InterruptHandler<peripherals::TWISPI1>> + 'static,
    fixed: bark_screen::Fixed<'static>,
) -> ! {
    use bark_screen::Face;
    use embassy_futures::select::{select3, Either3};
    // In a static, so that no 5000-byte value is built or moved through a
    // stack frame: the stack beside the relay is tight (`main.rs`,
    // `screen`).
    static PICTURE: static_cell::ConstStaticCell<Picture> =
        static_cell::ConstStaticCell::new(Picture::zeroed());
    let picture = PICTURE.take();
    let mut screen = Screen::new(pins, spim, irq).await;
    let mut schedule = bark_screen::schedule::Schedule::new();
    // The first picture shows what the node knows once its loop has run,
    // which is within a second of the radio coming up.
    let _ = with_timeout(FIRST_STATE, CHANGED.wait()).await;
    loop {
        let state = State {
            phone: PHONE.lock(core::cell::Cell::get),
            ..STATE.lock(core::cell::Cell::get)
        };
        let now = Instant::now();
        let ms = now.as_millis();
        let due = schedule.due(&state, ms);
        if due.is_none_or(|d| d > ms) {
            let at = due.map_or(Instant::MAX, Instant::from_millis);
            if let Either3::Second(lines) =
                select3(CHANGED.wait(), PERSON.wait(), Timer::at(at)).await
            {
                match lines {
                    true => schedule.lines(Instant::now().as_millis()),
                    false => schedule.touched(Instant::now().as_millis()),
                }
            }
            continue;
        }
        let face = schedule.face(ms);
        picture.clear();
        match face {
            Face::Lines => {
                let up_min = (now.as_secs() / 60) as u32;
                let t = crate::utc::time();
                let clock = t.utc_s.map(|utc_s| bark_screen::Clock {
                    utc_s,
                    offset_min: t.offset_min,
                });
                for (text, y) in bark_screen::lines(&fixed, &state, up_min, clock)
                    .iter()
                    .flatten()
                {
                    picture.text(text.bytes(), bark_screen::SCALE, *y);
                    embassy_futures::yield_now().await;
                }
                for (label, y) in bark_screen::labels(&state) {
                    picture.label(label.as_bytes(), y);
                }
            }
            Face::Code => {
                for part in 0..bark_screen::qr::PARTS {
                    bark_screen::qr::draw(picture, part);
                    embassy_futures::yield_now().await;
                }
                let line = bark_screen::code_line(&fixed, &state);
                picture.text(line.bytes(), bark_screen::SCALE, bark_screen::qr::LINE_TOP);
            }
        }
        let outcome = screen.draw(picture).await;
        // The boot's outcome is the first picture's.
        OUTCOME.lock(|o| o.set(o.get().or(Some(outcome))));
        schedule.drawn(face, state);
    }
}
