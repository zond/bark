//! What a node shows on its 1.54" e-paper (200 × 200, `bark-fw/src/display.rs`
//! drives it): its ID, its image, and its state as a person reads it,
//! drawn as the panel controller's RAM image. Building the picture needs
//! no hardware, so it is tested here; the firmware sends it.
//!
//! Text is drawn from a 5 × 7 bitmap font of the digits, the capital
//! letters and a few signs, scaled up by whole pixels.

#![no_std]

use core::fmt::{self, Write};

/// The panel's width and height, pixels; its RAM is `HEIGHT` rows of
/// `WIDTH / 8` bytes, the leftmost pixel in a byte's top bit, 1 white
/// (SSD1681 datasheet, Write RAM).
pub const WIDTH: usize = 200;
pub const HEIGHT: usize = 200;
/// Bytes in one row of the panel's RAM.
pub const ROW_BYTES: usize = WIDTH / 8;

/// Quarter turns clockwise from the controller's own orientation to the
/// board's. The controller's row 0 is at the edge the flex cable enters,
/// which on the T-Echo is the board's right: drawn unturned, the picture
/// came out a quarter turn clockwise (bench, 2026-10-30).
const ROTATION: u8 = 1;

/// A picture to show: the controller's RAM image, white until drawn on.
pub struct Picture {
    pub bytes: [u8; HEIGHT * ROW_BYTES],
}

impl Picture {
    pub const fn white() -> Picture {
        Picture {
            bytes: [0xFF; HEIGHT * ROW_BYTES],
        }
    }

    /// Black all over, all bits zero: what a static that should cost no
    /// flash holds until it is cleared, since zeros go to `.bss`.
    pub const fn zeroed() -> Picture {
        Picture {
            bytes: [0; HEIGHT * ROW_BYTES],
        }
    }

    /// White all over again.
    pub fn clear(&mut self) {
        self.bytes.fill(0xFF);
    }

    /// Whether the board's `(x, y)` is black.
    pub fn black(&self, x: usize, y: usize) -> bool {
        let (cx, cy) = turned(x, y);
        self.bytes[cy * ROW_BYTES + cx / 8] & (0x80 >> (cx % 8)) == 0
    }

    /// Black at the board's `(x, y)`, the origin top left; outside the
    /// panel is ignored.
    fn plot(&mut self, x: usize, y: usize) {
        if x >= WIDTH || y >= HEIGHT {
            return;
        }
        let (cx, cy) = turned(x, y);
        self.bytes[cy * ROW_BYTES + cx / 8] &= !(0x80 >> (cx % 8));
    }

    /// `text` centred, its top edge at row `y`, each font pixel `scale`
    /// pixels. A character the font lacks is a blank.
    pub fn text(&mut self, text: &[u8], scale: usize, y: usize) {
        let width = text_width(text.len(), scale);
        let x0 = WIDTH.saturating_sub(width) / 2;
        for (i, &c) in text.iter().enumerate() {
            let glyph = glyph(c).unwrap_or([0; GLYPH_HEIGHT]);
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

/// The controller's `(x, y)` of the board's.
fn turned(x: usize, y: usize) -> (usize, usize) {
    match ROTATION % 4 {
        0 => (x, y),
        1 => (y, WIDTH - 1 - x),
        2 => (WIDTH - 1 - x, HEIGHT - 1 - y),
        _ => (HEIGHT - 1 - y, x),
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

/// The width of `chars` characters at `scale`, pixels.
pub const fn text_width(chars: usize, scale: usize) -> usize {
    (chars * CELL).saturating_sub(1) * scale
}

/// The most characters a line at `scale` holds across the panel.
pub const fn line_chars(scale: usize) -> usize {
    (WIDTH / scale + 1) / CELL
}

/// The glyph of `c`, seven rows, the leftmost pixel in bit 4; `None` for
/// a character the font lacks. Lowercase is drawn as capitals: the ID's
/// hex is lowercase in the log and on the bus.
pub fn glyph(c: u8) -> Option<[u8; GLYPH_HEIGHT]> {
    Some(match c.to_ascii_uppercase() {
        b' ' => [0; GLYPH_HEIGHT],
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
        b'G' => [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F],
        b'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        b'I' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        b'J' => [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0C],
        b'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        b'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        b'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        b'N' => [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
        b'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        b'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        b'Q' => [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
        b'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        b'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        b'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        b'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        b'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        b'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0A],
        b'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        b'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        b'Z' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
        b'%' => [0x18, 0x19, 0x02, 0x04, 0x08, 0x13, 0x03],
        b':' => [0x00, 0x0C, 0x0C, 0x00, 0x0C, 0x0C, 0x00],
        b'+' => [0x00, 0x04, 0x04, 0x1F, 0x04, 0x04, 0x00],
        b'-' => [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
        _ => return None,
    })
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

impl<const N: usize> Default for Line<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Write for Line<N> {
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

/// The scale of every line: two pixels a font pixel, 16 characters
/// across, 16 pixels a line.
pub const SCALE: usize = 2;

/// The most characters on a line.
pub const LINE: usize = line_chars(SCALE);

const _: () = assert!(LINE == 16);

/// One line of the screen.
pub type Text = Line<LINE>;

/// The battery as the screen says it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Battery {
    /// On USB: the reading is the charger's.
    Usb,
    /// On battery, not read yet.
    #[default]
    NotRead,
    /// At or below the level at which a fixed relay turns sleepy, about
    /// 20 % (`bark_proto::BatteryLevels::DESIGN`).
    Low,
    /// About this much of its charge left.
    Percent(u8),
}

/// What the screen says of the node's state; the firmware takes it from
/// the core after each step.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct State {
    /// The first rotation after boot runs.
    pub starting: bool,
    /// It knows where it is.
    pub placed: bool,
    pub sleepy: bool,
    /// Sleepy, with a relay that holds its mail.
    pub anchored: bool,
    pub carried: bool,
    pub battery: Battery,
    /// Neighbours whose heartbeats it hears now.
    pub heard: u8,
    /// Of them, those heard loudly enough that a full frame reaches it
    /// through a fade: likely to hear it too, not proven.
    pub strong: u8,
    /// Of them, those that have taken a message from it: proven to hear it.
    pub both_ways: u8,
}

/// The time of day when the picture is drawn, if the node knows it
/// (`bark-fw/src/utc.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clock {
    /// UTC seconds since 1970.
    pub utc_s: u64,
    /// The phone's UTC offset, minutes, if a phone gave one: the time is
    /// then shown local, with the offset, else as UTC.
    pub offset_min: Option<i16>,
}

/// What never changes: the node's ID as hex, its image's name and version.
#[derive(Clone, Copy, Debug)]
pub struct Fixed<'a> {
    pub id: &'a [u8],
    pub image: &'a str,
    pub version: u32,
}

/// The top of the first line, and the gap between the fixed lines and the
/// state.
const TOP: usize = 6;
const GAP: usize = 8;

/// The most lines the screen takes: the ID and the image, the state's
/// eight, and the time.
pub const LINES: usize = 11;

const _: () = assert!(TOP + (LINES * line_height(SCALE)) + GAP <= HEIGHT);

/// The screen's lines, each with the row its top edge is at, for `state`
/// with the node up `up_min` minutes and the time `clock`; `None` past
/// the last.
pub fn lines(
    fixed: &Fixed<'_>,
    state: &State,
    up_min: u32,
    clock: Option<Clock>,
) -> [Option<(Text, usize)>; LINES] {
    let mut out: [Option<(Text, usize)>; LINES] = Default::default();
    let mut y = TOP;
    let mut n = 0;
    let mut put = |t: Text, gap: usize| {
        out[n] = Some((t, y));
        n += 1;
        y += line_height(SCALE) + gap;
    };
    let line = |args: fmt::Arguments<'_>| {
        let mut t = Text::new();
        let _ = t.write_fmt(args);
        t
    };
    put(line(format_args!("{}", Hex(fixed.id))), 0);
    put(
        line(format_args!("{} V{}", fixed.image, fixed.version)),
        GAP,
    );
    put(
        line(match (state.sleepy, state.anchored) {
            (false, _) => format_args!("RELAY"),
            (true, true) => format_args!("SLEEPY ANCHORED"),
            (true, false) => format_args!("SLEEPY NO ANCHOR"),
        }),
        0,
    );
    put(
        line(match (state.placed, state.carried) {
            (false, _) => format_args!("NO PLACE YET"),
            (true, true) => format_args!("CARRIED"),
            (true, false) => format_args!("FIXED"),
        }),
        0,
    );
    if state.starting {
        put(line(format_args!("STARTING")), 0);
    }
    put(line(format_args!("HEARD {}", state.heard)), 0);
    put(line(format_args!("STRONG {}", state.strong)), 0);
    put(line(format_args!("BOTH WAYS {}", state.both_ways)), 0);
    put(
        match state.battery {
            Battery::Usb => line(format_args!("ON USB")),
            Battery::NotRead => line(format_args!("BATTERY NOT READ")),
            Battery::Low => line(format_args!("BATTERY LOW")),
            Battery::Percent(p) => line(format_args!("BATTERY {p}%")),
        },
        0,
    );
    put(
        match up_min {
            m if m < 120 => line(format_args!("UP {m} MIN")),
            m if m < 48 * 60 => line(format_args!("UP {} HOURS", m / 60)),
            m => line(format_args!("UP {} DAYS", m / (24 * 60))),
        },
        0,
    );
    if let Some(c) = clock {
        put(line(format_args!("{}", c)), 0);
    }
    out
}

/// `UTC 12:34`, or with an offset the local time and the offset,
/// `12:34 +02` or `12:34 +05:30`.
impl fmt::Display for Clock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const DAY_MIN: i64 = 24 * 60;
        let utc_min = (self.utc_s / 60) as i64;
        let offset = i64::from(self.offset_min.unwrap_or(0));
        let m = (utc_min + offset).rem_euclid(DAY_MIN);
        let hhmm = (m / 60, m % 60);
        let Some(o) = self.offset_min else {
            return write!(f, "UTC {:02}:{:02}", hhmm.0, hhmm.1);
        };
        let sign = if o < 0 { '-' } else { '+' };
        let (oh, om) = (o.unsigned_abs() / 60, o.unsigned_abs() % 60);
        write!(f, "{:02}:{:02} {sign}{oh:02}", hhmm.0, hhmm.1)?;
        if om != 0 {
            write!(f, ":{om:02}")?;
        }
        Ok(())
    }
}

/// Bytes as they are, for the ID's hex.
struct Hex<'a>(&'a [u8]);

impl fmt::Display for Hex<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for &b in self.0 {
            f.write_char(b as char)?;
        }
        Ok(())
    }
}

/// Draw the screen into `picture`, which is cleared first.
pub fn draw(
    picture: &mut Picture,
    fixed: &Fixed<'_>,
    state: &State,
    up_min: u32,
    clock: Option<Clock>,
) {
    picture.clear();
    for (text, y) in lines(fixed, state, up_min, clock).iter().flatten() {
        picture.text(text.bytes(), SCALE, *y);
    }
}

#[cfg(test)]
mod tests;
