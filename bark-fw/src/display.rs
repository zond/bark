//! The e-paper: a picture drawn once at boot, then the panel asleep.
//!
//! The 1.54" panel (GDEH0154D67, 200 × 200, `board::Eink`) is driven by
//! an SSD1681, whose commands below are from its datasheet's command
//! table ("DS"). The controller holds the picture in its own RAM, one bit
//! a pixel, so after one full refresh it can be put in deep sleep, where
//! it draws nothing and keeps the picture on the glass; only a hardware
//! reset wakes it. The panel has its own SPI (SPIM3) and never shares the
//! radio's bus, and its refresh runs as its own future beside the radio's
//! start: every wait is on an interrupt, bounded, and the picture is
//! rendered in microseconds.
//!
//! Text is drawn from a 5 × 7 bitmap font of the hex digits and the few
//! letters the boot picture uses, scaled up by whole pixels.

use core::fmt;

use embassy_nrf::gpio::{Input, Level, Output, OutputDrive, Pull};
use embassy_nrf::interrupt::typelevel::{Binding, SPIM3};
use embassy_nrf::peripherals;
use embassy_nrf::spim::{self, Spim};
use embassy_nrf::Peri;
use embassy_time::{with_timeout, Duration, Instant, Timer};

use crate::board;
use crate::radio::RAIL_SETTLE;

/// The panel's width and height, pixels; its RAM is `HEIGHT` rows of
/// `WIDTH / 8` bytes, the leftmost pixel in a byte's top bit, 1 white
/// (DS, Write RAM).
pub const WIDTH: usize = 200;
pub const HEIGHT: usize = 200;
const ROW_BYTES: usize = WIDTH / 8;

/// Quarter turns clockwise from the controller's own orientation to the
/// board's. The controller's row 0 is at the edge the flex cable enters,
/// which on the T-Echo is the board's right: drawn unturned, the picture
/// came out a quarter turn clockwise (bench, 2026-10-30).
const ROTATION: u8 = 1;

/// A full refresh as the board's label gives it, 2 s (`board::Eink`).
const REFRESH: Duration = Duration::from_secs(2);

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

/// A picture to show: the controller's RAM image, white until drawn on.
pub struct Picture {
    bytes: [u8; HEIGHT * ROW_BYTES],
}

impl Picture {
    pub const fn white() -> Picture {
        Picture {
            bytes: [0xFF; HEIGHT * ROW_BYTES],
        }
    }

    /// Black at the board's `(x, y)`, the origin top left; outside the
    /// panel is ignored.
    fn plot(&mut self, x: usize, y: usize) {
        if x >= WIDTH || y >= HEIGHT {
            return;
        }
        let (cx, cy) = match ROTATION % 4 {
            0 => (x, y),
            1 => (y, WIDTH - 1 - x),
            2 => (WIDTH - 1 - x, HEIGHT - 1 - y),
            _ => (HEIGHT - 1 - y, x),
        };
        self.bytes[cy * ROW_BYTES + cx / 8] &= !(0x80 >> (cx % 8));
    }

    /// `text` centred, its top edge at row `y`, each font pixel `scale`
    /// pixels. A character the font lacks is a blank.
    pub fn text(&mut self, text: &[u8], scale: usize, y: usize) {
        let width = (text.len() * CELL - 1) * scale;
        let x0 = WIDTH.saturating_sub(width) / 2;
        for (i, &c) in text.iter().enumerate() {
            let glyph = glyph(c);
            for (row, bits) in glyph.iter().enumerate() {
                for col in 0..GLYPH_WIDTH {
                    if bits & (0x10 >> col) == 0 {
                        continue;
                    }
                    let px = x0 + (i * CELL + col) * scale;
                    let py = y + row * scale;
                    for dy in 0..scale {
                        for dx in 0..scale {
                            self.plot(px + dx, py + dy);
                        }
                    }
                }
            }
        }
    }
}

/// The font's glyph size, and the cell it takes with one column of space.
const GLYPH_WIDTH: usize = 5;
const GLYPH_HEIGHT: usize = 7;
const CELL: usize = GLYPH_WIDTH + 1;

/// Row height of text at `scale`, including a row of space below.
pub const fn line_height(scale: usize) -> usize {
    (GLYPH_HEIGHT + 1) * scale
}

/// The glyph of `c`: seven rows, the leftmost pixel in bit 4.
fn glyph(c: u8) -> [u8; GLYPH_HEIGHT] {
    // The font is capitals only; the ID's hex and the image's name are
    // lowercase in the log and on the bus.
    match c.to_ascii_uppercase() {
        b'0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        b'1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        b'2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
        b'3' => [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E],
        b'4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        b'5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        b'6' => [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        b'7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        b'8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        b'9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
        b'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        b'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        b'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        b'D' => [0x1C, 0x12, 0x11, 0x11, 0x11, 0x12, 0x1C],
        b'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        b'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        b'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        b'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        b'N' => [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
        b'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        b'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        b'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        b'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        b'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        b'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        _ => [0; GLYPH_HEIGHT],
    }
}

/// A line of text formatted without allocation: what does not fit is cut.
pub struct Line<const N: usize> {
    buf: [u8; N],
    len: usize,
}

impl<const N: usize> Line<N> {
    pub const fn new() -> Line<N> {
        Line {
            buf: [0; N],
            len: 0,
        }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }
}

impl<const N: usize> fmt::Write for Line<N> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for &b in s.as_bytes() {
            if self.len == N {
                break;
            }
            self.buf[self.len] = b.to_ascii_uppercase();
            self.len += 1;
        }
        Ok(())
    }
}

/// Why the picture did not reach the glass.
#[derive(Debug)]
enum Error {
    /// BUSY stayed high past [`BUSY_TIMEOUT`] at this step.
    Busy(&'static str),
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
        self.wait_ready("refresh").await?;
        self.command(DEEP_SLEEP, &[0x01]).await
    }
}

/// Show `picture` and leave the panel asleep with it. Never returns: the
/// pins stay driven, RES# high above all, since a disconnected RES# could
/// wake the controller into drawing again.
pub async fn show(
    pins: board::Eink,
    spim: Peri<'static, peripherals::SPI3>,
    irq: impl Binding<SPIM3, spim::InterruptHandler<peripherals::SPI3>> + 'static,
    picture: &Picture,
) -> ! {
    let t0 = Instant::now();
    // RES# low from the start. Whether the panel hangs on the switched rail
    // is not known, so its other pins are driven only once the rail would
    // be up, as the radio's are (`radio::Radio::new`).
    let mut reset = Output::new(pins.reset, Level::Low, OutputDrive::Standard);
    Timer::after(RAIL_SETTLE).await;
    let mut config = spim::Config::default();
    // 4 MHz writes the 5000-byte picture in 10 ms, nothing beside the 2 s
    // refresh, and leaves the flex cable to the panel margin. Mode 0 is the
    // controller's (DS, serial interface).
    config.frequency = spim::Frequency::M4;
    config.mode = spim::MODE_0;
    let mut panel = Panel {
        spi: Spim::new_txonly(spim, irq, pins.sclk, pins.mosi, config),
        cs: Output::new(pins.cs, Level::High, OutputDrive::Standard),
        dc: Output::new(pins.dc, Level::Low, OutputDrive::Standard),
        busy: Input::new(pins.busy, Pull::None),
    };
    Timer::after(RESET_PULSE).await;
    reset.set_high();
    Timer::after(RESET_PULSE).await;
    match panel.show(picture).await {
        Ok(()) => log::info!("display: shown in {} ms, asleep", t0.elapsed().as_millis()),
        Err(Error::Busy(step)) => log::warn!(
            "display: busy past {} s at {}; no picture",
            BUSY_TIMEOUT.as_secs(),
            step
        ),
        Err(Error::Spi(e)) => log::warn!("display: spi {:?}; no picture", e),
    }
    core::future::pending().await
}
