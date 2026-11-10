//! What a node shows on its 1.54" e-paper (200 × 200, `bark-fw/src/display.rs`
//! drives it): its ID, its image, and its state as a person reads it,
//! drawn as the panel controller's RAM image. Building the picture needs
//! no hardware, so it is tested here; the firmware sends it.
//!
//! Text is drawn from a 5 × 7 bitmap font of the digits, the capital
//! letters, one lowercase letter and a few signs, scaled up by whole
//! pixels.

#![no_std]

use core::fmt::{self, Write};

pub mod pad;
pub mod qr;
pub mod schedule;

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
    pub(crate) fn plot(&mut self, x: usize, y: usize) {
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
        self.text_at(text, scale, WIDTH.saturating_sub(width) / 2, y);
    }

    /// A button's label at the left edge, its top at row `y`: an arrow
    /// pointing left, a filled head the font's height on a one-pixel
    /// shaft, then `text` in the small font.
    pub fn label(&mut self, text: &[u8], y: usize) {
        let mid = GLYPH_HEIGHT / 2;
        for col in 0..=mid {
            for row in mid - col..=mid + col {
                self.plot(LABEL_LEFT + col, y + row);
            }
        }
        for col in mid + 1..ARROW {
            self.plot(LABEL_LEFT + col, y + mid);
        }
        self.text_at(text, LABEL_SCALE, LABEL_LEFT + ARROW + 2, y);
    }

    /// `text` with its top left corner at `(x0, y)`.
    pub fn text_at(&mut self, text: &[u8], scale: usize, x0: usize, y: usize) {
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
/// a character the font lacks. Lowercase is drawn as capitals, the ID's
/// hex being lowercase in the log and on the bus, but for `s`, the one
/// lowercase letter, five rows tall on the baseline, for the seconds
/// in the buttons' labels (`labels`).
pub fn glyph(c: u8) -> Option<[u8; GLYPH_HEIGHT]> {
    if c == b's' {
        return Some([0x00, 0x00, 0x0F, 0x10, 0x0E, 0x01, 0x1E]);
    }
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
        b'/' => [0x00, 0x01, 0x02, 0x04, 0x08, 0x10, 0x00],
        b'(' => [0x02, 0x04, 0x08, 0x08, 0x08, 0x04, 0x02],
        b')' => [0x08, 0x04, 0x02, 0x02, 0x02, 0x04, 0x08],
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

    /// Blanks after what is written, up to `len` characters.
    pub fn pad_to(&mut self, len: usize) {
        while self.len < len.min(N) {
            self.buf[self.len] = b' ';
            self.len += 1;
        }
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

/// How long the battery lasts, as the BATTERY line says it beside the
/// level: in the largest unit that keeps the figure to one or two digits,
/// but hours up to two days, as the uptime has them, since a day is a
/// coarse step there. Whole units, rounded down; days at most 99.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Left {
    Minutes(u8),
    Hours(u8),
    Days(u8),
}

impl Left {
    /// `minutes` as the line says it.
    pub fn from_minutes(minutes: u32) -> Left {
        match minutes {
            m if m < 60 => Left::Minutes(m as u8),
            m if m < 48 * 60 => Left::Hours((m / 60) as u8),
            m => Left::Days((m / (24 * 60)).min(99) as u8),
        }
    }
}

impl fmt::Display for Left {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Left::Minutes(m) => write!(f, "{m}M"),
            Left::Hours(h) => write!(f, "{h}H"),
            Left::Days(d) => write!(f, "{d}D"),
        }
    }
}

/// What the screen says of the node's state; the firmware takes it from
/// the core after each step.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct State {
    /// The first rotation after boot runs.
    pub starting: bool,
    /// It knows where it is. Shown for a relay only: a sleepy node is
    /// reached through its anchor.
    pub placed: bool,
    pub sleepy: bool,
    /// Sleepy, with a relay that holds its mail.
    pub anchored: bool,
    pub carried: bool,
    pub battery: Battery,
    /// How long the battery lasts at the drain the node has measured, if
    /// it has an estimate (`bark_proto::drain`).
    pub battery_left: Option<Left>,
    /// Neighbours whose heartbeats it hears now.
    pub heard: u8,
    /// Of them, those heard loudly enough that a full frame reaches it
    /// through a fade: likely to hear it too, not proven.
    pub strong: u8,
    /// Of them, those that have taken a message from it: proven to hear it.
    pub both_ways: u8,
    pub phone: Phone,
    /// The image it gathers over the mesh, if any.
    pub getting: Option<Getting>,
}

/// An image a node gathers over the mesh (`bark_proto::status::Gathering`):
/// its version, and of its blocks how many the node holds from the first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Getting {
    pub version: u32,
    pub complete: u8,
    pub blocks: u8,
}

/// The phone link as the screen says it (spec "Phone link", Pairing,
/// Phones; "Screen and buttons").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phone {
    /// No phone has paired; the node is public all the same.
    #[default]
    Unpaired,
    /// One phone is paired, and the node is its owners' alone.
    Private,
    /// One phone is paired, and the node serves guests too.
    Public,
    /// More than one phone is paired: how many, and whether the node is
    /// private.
    Phones { count: u8, private: bool },
    /// A phone is pairing: the six-digit code to type into it.
    Code(u32),
    /// A removal waits: the six-digit code to type into the phone that
    /// removes.
    Remove(u32),
    /// A pairing was refused, as every phone the node keeps is taken.
    Full,
}

impl Phone {
    /// A code a person at the node types into a phone, which the screen
    /// shows the lines for, and draws at once (`schedule`).
    pub fn code(&self) -> Option<u32> {
        match *self {
            Phone::Code(c) | Phone::Remove(c) => Some(c),
            _ => None,
        }
    }
}

/// The phone's line, on the lines and, while it says what a person at
/// the node acts on, under the code ([`code_line`]).
fn phone_line(phone: Phone) -> Text {
    match phone {
        Phone::Unpaired => row("PHONE", format_args!("UNPAIRED")),
        Phone::Private => row("PHONE", format_args!("PRIVATE")),
        Phone::Public => row("PHONE", format_args!("PUBLIC")),
        Phone::Phones { count, private } => {
            let mut label = Text::new();
            let _ = write!(label, "{count} PHONES");
            let label = core::str::from_utf8(label.bytes()).unwrap_or("PHONES");
            let set = if private { "PRIVATE" } else { "PUBLIC" };
            row(label, format_args!("{set}"))
        }
        Phone::Code(c) => row("PAIR CODE", format_args!("{c:06}")),
        Phone::Remove(c) => row("REMOVE", format_args!("{c:06}")),
        Phone::Full => row("PHONES", format_args!("FULL")),
    }
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

/// The top of the first line, and the gap below the header.
const TOP: usize = 10;
const GAP: usize = 8;

/// The buttons' labels: one font pixel a pixel, at the left edge, on the
/// lines picture's top and bottom margins, each the one nearer its
/// button: both buttons are on the case's left side as the screen reads,
/// the reset button nearer the top and the user button below the glass
/// (`docs/enclosure-2026-11-09.md`, the maker's case model). The long
/// press's length is `bark-fw`'s `button::LONG_PRESS`.
pub const LABEL_SCALE: usize = 1;
pub const LABEL_LEFT: usize = 2;
/// The label's arrow's width, pixels: its head, as wide as half the
/// font's height, and as much again of shaft.
const ARROW: usize = 2 * (GLYPH_HEIGHT / 2 + 1);
pub const REBOOT_TOP: usize = 1;
pub const ROLE_TOP: usize = HEIGHT - 1 - GLYPH_HEIGHT * LABEL_SCALE;

/// The most lines the screen takes: the role and `STARTING` as the
/// header, the ID, the image and the one it gathers, the phone, the
/// state's three, and the time.
pub const LINES: usize = 10;

const _: () = assert!(TOP + (LINES * line_height(SCALE)) + GAP <= ROLE_TOP);
const _: () = assert!(REBOOT_TOP + GLYPH_HEIGHT * LABEL_SCALE < TOP);

/// The labels the lines show beside the buttons that change something,
/// each with its top row, drawn after an arrow pointing at the button
/// ([`Picture::label`]): the reset button reboots; the user button held
/// flips the role, so it says the role it would turn the node to
/// (spec "Screen and buttons").
pub fn labels(state: &State) -> [(&'static str, usize); 2] {
    let role = match state.sleepy {
        false => "SLEEP (3s)",
        true => "WAKE (3s)",
    };
    [("REBOOT", REBOOT_TOP), (role, ROLE_TOP)]
}

/// The screen's lines, each with the row its top edge is at, for `state`
/// with the node up `up_min` minutes and the time `clock`; `None` past
/// the last.
///
/// The header says what the node is, a state as a word or two at the
/// left; every line below it is a label at the left and its value flush
/// right. Every line is a full line wide, so that it starts at the left
/// margin.
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
    put(
        plain(format_args!("{}", role(state))),
        if state.starting { 0 } else { GAP },
    );
    if state.starting {
        put(plain(format_args!("STARTING")), GAP);
    }
    put(row("ID", format_args!("{}", Hex(fixed.id))), 0);
    put(row(fixed.image, format_args!("V{}", fixed.version)), 0);
    if let Some(g) = state.getting {
        let mut label = Text::new();
        let _ = write!(label, "GET V{}", g.version);
        let label = core::str::from_utf8(label.bytes()).unwrap_or("GET");
        put(row(label, format_args!("{}/{}", g.complete, g.blocks)), 0);
    }
    put(phone_line(state.phone), 0);
    put(
        row(
            "KNOWN",
            format_args!("{}/{}/{}", state.heard, state.strong, state.both_ways),
        ),
        0,
    );
    put(
        match state.battery {
            Battery::Usb => row("POWER", format_args!("USB")),
            Battery::NotRead => row("BATTERY", format_args!("NOT READ")),
            Battery::Low => match state.battery_left {
                Some(left) => row("BATTERY", format_args!("LOW/{left}")),
                None => row("BATTERY", format_args!("LOW")),
            },
            Battery::Percent(p) => match state.battery_left {
                Some(left) => row("BATTERY", format_args!("{p}%/{left}")),
                None => row("BATTERY", format_args!("{p}%")),
            },
        },
        0,
    );
    put(
        match up_min {
            m if m < 120 => row("UP", format_args!("{m} MIN")),
            m if m < 48 * 60 => row("UP", format_args!("{} HOURS", m / 60)),
            m => row("UP", format_args!("{} DAYS", m / (24 * 60))),
        },
        0,
    );
    if let Some(c) = clock {
        put(row("TIME", format_args!("{}", c)), 0);
    }
    out
}

/// What the node is, the header's first line.
pub fn role(state: &State) -> &'static str {
    match (state.sleepy, state.anchored, state.placed, state.carried) {
        (true, true, _, _) => "SLEEPY ANCHORED",
        (true, false, _, _) => "SLEEPY NO ANCHOR",
        (false, _, false, _) => "RELAY NO PLACE",
        (false, _, true, true) => "CARRIED RELAY",
        (false, _, true, false) => "FIXED RELAY",
    }
}

/// Which picture the screen shows: the code, the default, or the lines a
/// touch asks for (`schedule`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Face {
    #[default]
    Code,
    Lines,
}

/// The line under the code: the pairing code while a phone pairs, the
/// removal code while a removal waits, and `PHONES FULL` after a pairing
/// refused for want of room, as on the lines; else the node's ID, which
/// its name in the app ends with.
pub fn code_line(fixed: &Fixed<'_>, state: &State) -> Text {
    match state.phone {
        p @ (Phone::Code(_) | Phone::Remove(_) | Phone::Full) => phone_line(p),
        _ => row("ID", format_args!("{}", Hex(fixed.id))),
    }
}

/// `args` at the left, the rest of the line blank.
fn plain(args: fmt::Arguments<'_>) -> Text {
    let mut t = Text::new();
    let _ = t.write_fmt(args);
    t.pad_to(LINE);
    t
}

/// `label` at the left and `value` flush right, blanks between.
fn row(label: &str, value: fmt::Arguments<'_>) -> Text {
    let mut v = Text::new();
    let _ = v.write_fmt(value);
    let mut t = Text::new();
    let _ = t.write_str(label);
    t.pad_to(LINE.saturating_sub(v.bytes().len()).max(label.len() + 1));
    let _ = t.write_str(core::str::from_utf8(v.bytes()).unwrap_or(""));
    t
}

/// The time as the `TIME` line's value: `12:34 UTC`, or with an offset
/// the local time and the offset, `12:34 +02` or `12:34 +0530`, which
/// fits the line beside its label.
impl fmt::Display for Clock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const DAY_MIN: i64 = 24 * 60;
        let utc_min = (self.utc_s / 60) as i64;
        let offset = i64::from(self.offset_min.unwrap_or(0));
        let m = (utc_min + offset).rem_euclid(DAY_MIN);
        write!(f, "{:02}:{:02}", m / 60, m % 60)?;
        let Some(o) = self.offset_min else {
            return f.write_str(" UTC");
        };
        let sign = if o < 0 { '-' } else { '+' };
        let (oh, om) = (o.unsigned_abs() / 60, o.unsigned_abs() % 60);
        write!(f, " {sign}{oh:02}")?;
        if om != 0 {
            write!(f, "{om:02}")?;
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

/// Draw `face` into `picture`, which is cleared first. The firmware draws
/// the same in steps, giving the executor back between them.
pub fn draw(
    picture: &mut Picture,
    face: Face,
    fixed: &Fixed<'_>,
    state: &State,
    up_min: u32,
    clock: Option<Clock>,
) {
    picture.clear();
    match face {
        Face::Lines => {
            for (text, y) in lines(fixed, state, up_min, clock).iter().flatten() {
                picture.text(text.bytes(), SCALE, *y);
            }
            for (label, y) in labels(state) {
                picture.label(label.as_bytes(), y);
            }
        }
        Face::Code => {
            for part in 0..qr::PARTS {
                qr::draw(picture, part);
            }
            picture.text(code_line(fixed, state).bytes(), SCALE, qr::LINE_TOP);
        }
    }
}

#[cfg(test)]
mod tests;
