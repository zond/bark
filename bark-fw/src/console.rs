//! Measurement console on the USB serial port.
//!
//! The host types one command per line; the firmware echoes it as a line
//! starting with `>` and answers in ordinary log lines of `key=value`
//! fields, so a transcript separates commands from everything else by the
//! first character. Between commands the radio listens on the home channel,
//! prints every frame it hears, answers pings and sends beacons. All
//! durations are µs on [`Clock`].
//!
//! Commands (`k` is a channel, 0..12):
//!
//! ```text
//! ch <k>                     set the home channel
//! tx <k> <hex|len>           send one frame
//! rx <k> [ms]                listen on k for ms (default 10 s), reporting every frame
//! cad <k>                    one channel activity detection, the listen-before-talk's
//! lbt <k>                    one listen-before-talk: its phases and its reading
//! tcxo                       the oscillator's probed delay and the last probe
//! probe                      probe the oscillator's start now, below the delay in use
//! rssi <k> [dwell_us]        one RSSI reading
//! sweep                      one RSSI reading on each channel
//! noise <k> <n> [gap_us]     n RSSI readings: min, p10, median, max
//! power <dbm>                transmit power, clamped to the ERP limit
//! beacon <k> <ms> [len]      send a frame every ms; stop with `stop`
//! ping <k>                   send a ping on k, wait home for the pong
//! status [reset]             uptime, battery, settings, counters
//! survey [n]                 the survey build's noise log, the last n minutes
//! stage <header hex>         start staging an image (`bark_image::Header`)
//! chunk <offset> <hex>       write up to 256 bytes of it, in order
//! commit                     check it and ask the loader to swap it in
//! reboot                     reset, to let the loader swap
//! ```
//!
//! `scripts/image.py stage` sends an image through the last four.
//!
//! Bench frames start with a heartbeat's first byte on channel 15, which no
//! node sends and nodes reject (spec "Packet formats"): byte 0 is `0x8F`,
//! byte 1 the format version, as in every packet, byte 2 the kind, in the
//! heartbeat's flags byte, byte 3 the kind's argument (the sender's home
//! channel, or a survey frame's transmit power as a signed byte), bytes
//! 4..8 a big-endian sequence number, and the rest `byte[i] = i`. A
//! heartbeat heard is printed with its sender's ID, the newest format it
//! speaks and its map point flag.
//!
//! Built with `BARK_SURVEY=<channel>`, the console is a survey for a board
//! left on battery away from the host: from boot, with no host needed, it
//! beacons on that channel through every frame length and power, each
//! frame saying its power, and every minute reads the noise on every
//! channel into a ring in RAM, which `survey` prints once the board is
//! plugged in again ([`SURVEY`], [`Survey`]).

use core::fmt;

use crate::battery::Battery;
use bark_driver::sx1262::{irq, TxTiming};
use bark_driver::{tcxo, Heard};
use bark_proto::radio::{
    time_on_air, Channel, CHANNELS, DUTY_CYCLE_PERCENT, MIN_PREAMBLE_SYMBOLS, SYMBOL_TIME,
};
use bark_proto::sniff::LONG_PREAMBLE_SYMBOLS;
use bark_proto::wire::{Packet, ACK_LEN, BENCH_FIRST, FORMAT, HEARTBEAT_LEN};
use bark_proto::Frame;
use embassy_futures::select::{select, select3, Either, Either3};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::pipe::Pipe;
use embassy_time::{Duration, Instant, Timer};
use embassy_usb_logger::ReceiverHandler;

use crate::clock::Clock;
use crate::radio::{Error, Radio, MAX_LEN, MAX_TX_DBM, MIN_TX_DBM};
use crate::update::{decimal, Flash, BARK_IMAGE_VERSION};
use bark_image::boot::{Stager, CHUNK};
use bark_image::HEADER_LEN;
use embassy_nrf::nvmc::Nvmc;

/// How long `reboot` waits for the log to reach the host: the 2 KiB log
/// buffer at one 64-byte packet per 1 ms frame (USB 2.0 full speed, bulk).
const LOG_DRAIN: Duration = Duration::from_millis(2048 / 64);

/// Log target of command echo lines; the log formatter prints them bare,
/// after a `>`.
pub const ECHO: &str = "echo";

/// Bytes from the host waiting to be read as commands.
static INPUT: Pipe<CriticalSectionRawMutex, 256> = Pipe::new();

/// Feeds bytes the host sends into [`INPUT`].
pub struct HostInput;

impl ReceiverHandler for HostInput {
    async fn handle_data(&self, data: &[u8]) {
        INPUT.write_all(data).await;
    }

    fn new() -> Self {
        HostInput
    }
}

/// Longest command line, bytes.
const LINE: usize = 600;

/// Command lines assembled from [`INPUT`].
pub struct Lines {
    buf: [u8; LINE],
    len: usize,
    overflow: bool,
    complete: bool,
}

/// A line that cannot be a command.
pub enum LineError {
    TooLong,
    NotUtf8,
}

impl Lines {
    pub const fn new() -> Lines {
        Lines {
            buf: [0; LINE],
            len: 0,
            overflow: false,
            complete: false,
        }
    }

    /// The next non-empty line, without its terminator. Cancel-safe: bytes
    /// read so far stay in `self`.
    pub async fn next(&mut self) -> Result<&str, LineError> {
        if self.complete {
            self.len = 0;
            self.overflow = false;
            self.complete = false;
        }
        loop {
            // One byte at a time: commands are typed by hand, and a byte
            // read is never lost when the caller drops this future.
            let mut byte = [0u8];
            INPUT.read(&mut byte).await;
            match byte[0] {
                b'\r' | b'\n' if self.len == 0 && !self.overflow => {}
                b'\r' | b'\n' => {
                    self.complete = true;
                    if self.overflow {
                        return Err(LineError::TooLong);
                    }
                    return core::str::from_utf8(&self.buf[..self.len])
                        .map_err(|_| LineError::NotUtf8);
                }
                b => {
                    if self.len < LINE {
                        self.buf[self.len] = b;
                        self.len += 1;
                    } else {
                        self.overflow = true;
                    }
                }
            }
        }
    }
}

/// Bench frame kinds, in byte 2.
const KIND_TEST: u8 = 0;
const KIND_PING: u8 = 1;
const KIND_PONG: u8 = 2;
/// A survey beacon: its argument is the transmit power, dBm, as a signed
/// byte.
const KIND_SURVEY: u8 = 3;
/// First byte, format, kind, argument, sequence number.
const BENCH_HEADER: usize = 8;

/// A bench frame's header fields.
#[derive(Clone, Copy)]
struct Bench {
    kind: u8,
    /// The sender's home channel, or a survey frame's power.
    arg: u8,
    seq: u32,
}

fn bench_frame(buf: &mut [u8; MAX_LEN], len: usize, kind: u8, arg: u8, seq: u32) -> &[u8] {
    let mut header = [0u8; BENCH_HEADER];
    header[0] = BENCH_FIRST;
    header[1] = FORMAT;
    header[2] = kind;
    header[3] = arg;
    header[4..8].copy_from_slice(&seq.to_be_bytes());
    for (i, b) in buf.iter_mut().enumerate().take(len) {
        *b = if i < BENCH_HEADER { header[i] } else { i as u8 };
    }
    &buf[..len]
}

fn parse_bench(frame: &[u8]) -> Option<Bench> {
    if frame.len() < BENCH_HEADER || frame[0] != BENCH_FIRST {
        return None;
    }
    Some(Bench {
        kind: frame[2],
        arg: frame[3],
        seq: u32::from_be_bytes([frame[4], frame[5], frame[6], frame[7]]),
    })
}

/// The survey build's channel, from `BARK_SURVEY` at build time, or none.
pub const SURVEY: Option<Channel> = match option_env!("BARK_SURVEY") {
    None => None,
    Some(v) => match Channel::new(decimal(v) as u8) {
        Some(ch) => Some(ch),
        None => panic!("BARK_SURVEY is a channel, 0 to 12"),
    },
};

/// The survey beacon's frame lengths (roadmap Phase 2, the loss-curve
/// row); for each, every power from [`MIN_TX_DBM`] to [`MAX_TX_DBM`] in
/// 1 dB steps.
const SURVEY_LENGTHS: [usize; 3] = [8, 64, 255];
const SURVEY_POWERS: usize = (MAX_TX_DBM - MIN_TX_DBM) as usize + 1;
const SURVEY_STEPS: usize = SURVEY_LENGTHS.len() * SURVEY_POWERS;

/// The length and power of survey beacon `step`.
fn survey_step(step: usize) -> (usize, i8) {
    let len = SURVEY_LENGTHS[step / SURVEY_POWERS];
    let power = MIN_TX_DBM + (step % SURVEY_POWERS) as i8;
    (len, power)
}

/// How often the survey reads the noise.
const SAMPLE_EVERY: Duration = Duration::from_secs(60);
/// Readings a channel, back to back, as `noise <k> 100`: the 10th
/// percentile then rests on ten readings.
const SAMPLE_READINGS: usize = 100;
/// Wait after the retune before the first reading: four symbol periods,
/// from when a reading is right (`docs/bench-2026-10-30.md`, RSSI against
/// dwell).
const SAMPLE_DWELL: Duration = Duration::from_micros(4 * SYMBOL_TIME.as_micros());
/// How long the ring reaches back: two days, in RAM the console build has
/// spare, with a night and a day of margin over the 24 h a placement needs.
const RING_HOURS: u64 = 48;
const RING_LEN: usize = (RING_HOURS * 3600 / SAMPLE_EVERY.as_secs()) as usize;

/// One minute's noise: the 10th percentile and the median of each
/// channel's readings, whole dBm.
#[derive(Clone, Copy)]
struct Sample {
    /// Uptime when it was taken.
    at_s: u32,
    p10: [i8; CHANNELS as usize],
    median: [i8; CHANNELS as usize],
}

impl Sample {
    const ZERO: Sample = Sample {
        at_s: 0,
        p10: [0; CHANNELS as usize],
        median: [0; CHANNELS as usize],
    };
}

/// The survey's noise log: the last [`RING_LEN`] samples.
struct Ring {
    samples: [Sample; RING_LEN],
    /// Where the next sample goes.
    next: usize,
    /// How many are held, at most `RING_LEN`.
    count: usize,
}

impl Ring {
    /// The one ring, in `.bss` (all zero) rather than on a stack; once.
    fn take() -> &'static mut Ring {
        static RING: static_cell::ConstStaticCell<Ring> = static_cell::ConstStaticCell::new(Ring {
            samples: [Sample::ZERO; RING_LEN],
            next: 0,
            count: 0,
        });
        RING.take()
    }

    fn push(&mut self, sample: Sample) {
        self.samples[self.next] = sample;
        self.next = (self.next + 1) % RING_LEN;
        self.count = (self.count + 1).min(RING_LEN);
    }

    /// The newest `n` samples, oldest first.
    fn newest(&self, n: usize) -> impl Iterator<Item = &Sample> {
        let n = n.min(self.count);
        let first = (self.next + RING_LEN - n) % RING_LEN;
        (0..n).map(move |i| &self.samples[(first + i) % RING_LEN])
    }
}

/// The survey build between commands: beacons on its channel through
/// every length and power, and a noise reading every minute.
struct Survey {
    ch: Channel,
    /// The next beacon's place in the cycle, `0..SURVEY_STEPS`.
    step: usize,
    next_beacon: Instant,
    next_sample: Instant,
    ring: &'static mut Ring,
}

/// A `survey` line's longest form, bytes, with the log's prefix: the pace
/// at which its lines are printed, one per 64-byte USB packet and 1 ms
/// frame, so that the log buffer never fills and drops them.
const SURVEY_LINE: usize = 200;
const SURVEY_LINE_DRAIN: Duration = Duration::from_millis(SURVEY_LINE.div_ceil(64) as u64);

/// What `tx` sends.
enum Payload<'l> {
    /// A bench frame of this length.
    Len(usize),
    /// Hex digits, checked: an even number, at most two per frame byte.
    Hex(&'l [u8]),
}

enum Command<'l> {
    Ch(Channel),
    Tx(Channel, Payload<'l>),
    Rx(Channel, u32),
    Cad(Channel),
    Lbt(Channel),
    Tcxo,
    Probe,
    Rssi(Channel, u32),
    Sweep,
    Noise(Channel, usize, u32),
    Power(i32),
    Preamble(u16),
    Beacon(Channel, u32, usize),
    Stop,
    Ping(Channel),
    Status(bool),
    /// The last n survey samples.
    Survey(usize),
    Help,
    /// A header, as checked hex digits.
    Stage(&'l [u8]),
    /// An offset and bytes, as checked hex digits.
    Chunk(u32, &'l [u8]),
    Commit,
    Reboot,
}

/// Default `rx` wait, ms.
const RX_DEFAULT_MS: u32 = 10_000;
/// Most RSSI readings `noise` keeps.
const NOISE_MAX: usize = 1000;
/// How long `ping` listens for its pong: ten ACK timeouts (spec "Hand-over"),
/// so a late pong is still seen and timed.
const PING_WAIT: Duration = Duration::from_millis(1000);
/// The per-hop ACK timeout the pong is measured against (spec "Hand-over").
const ACK_TIMEOUT_US: u32 = 100_000;

const HELP: &str =
    "ch <k> | tx <k> <hex|len> | rx <k> [ms] | cad <k> | lbt <k> | tcxo | probe | rssi <k> [dwell_us] | sweep | \
noise <k> <n> [gap_us] | power <dbm> | preamble <short|long|n> | beacon <k> <ms> [len] | stop | ping <k> | status [reset] | \
survey [n] | stage <header hex> | chunk <offset> <hex> | commit | reboot";

fn parse(line: &str) -> Result<Command<'_>, &'static str> {
    let line = line.trim_start_matches('>');
    let mut words = line.split_ascii_whitespace();
    let name = words.next().ok_or("empty line")?;
    let mut arg = || words.next();
    let command = match name {
        "ch" => Command::Ch(channel(arg())?),
        "tx" => {
            let ch = channel(arg())?;
            Command::Tx(
                ch,
                payload(arg().ok_or("tx needs a payload: hex bytes or a length")?)?,
            )
        }
        "rx" => Command::Rx(channel(arg())?, optional(arg(), RX_DEFAULT_MS)?),
        "cad" => Command::Cad(channel(arg())?),
        "lbt" => Command::Lbt(channel(arg())?),
        "tcxo" => Command::Tcxo,
        "probe" => Command::Probe,
        "rssi" => Command::Rssi(channel(arg())?, optional(arg(), 0)?),
        "sweep" => Command::Sweep,
        "noise" => {
            let ch = channel(arg())?;
            let n: usize = number(arg())?;
            if n == 0 || n > NOISE_MAX {
                return Err("noise: n must be 1..=1000");
            }
            Command::Noise(ch, n, optional(arg(), 0)?)
        }
        "power" => Command::Power(number(arg())?),
        "preamble" => Command::Preamble(match arg() {
            Some("short") => MIN_PREAMBLE_SYMBOLS,
            Some("long") => LONG_PREAMBLE_SYMBOLS,
            word => {
                let n = number(word)?;
                if !(MIN_PREAMBLE_SYMBOLS..=LONG_PREAMBLE_SYMBOLS).contains(&n) {
                    return Err("preamble: short, long or 12..=1396 symbols");
                }
                n
            }
        }),
        "beacon" => {
            let ch = channel(arg())?;
            let period = number(arg())?;
            let len = optional(arg(), ACK_LEN)?;
            if !(BENCH_HEADER..=MAX_LEN).contains(&len) {
                return Err("beacon: len must be 8..=255");
            }
            Command::Beacon(ch, period, len)
        }
        "stop" => Command::Stop,
        "ping" => Command::Ping(channel(arg())?),
        "status" => match arg() {
            None => Command::Status(false),
            Some("reset") => Command::Status(true),
            Some(_) => return Err("status takes nothing or `reset`"),
        },
        "survey" => Command::Survey(optional(arg(), usize::MAX)?),
        "help" | "?" => Command::Help,
        "stage" => Command::Stage(hex_bytes(
            arg().ok_or("stage needs the image's header in hex")?,
            HEADER_LEN,
            HEADER_LEN,
        )?),
        "chunk" => {
            let offset = number(arg())?;
            let bytes = hex_bytes(arg().ok_or("chunk needs bytes in hex")?, 1, CHUNK)?;
            Command::Chunk(offset, bytes)
        }
        "commit" => Command::Commit,
        "reboot" => Command::Reboot,
        _ => return Err("unknown command; try `help`"),
    };
    if words.next().is_some() {
        return Err("too many arguments");
    }
    Ok(command)
}

fn number<T: core::str::FromStr>(word: Option<&str>) -> Result<T, &'static str> {
    word.ok_or("missing number")?
        .parse()
        .map_err(|_| "not a number")
}

fn optional<T: core::str::FromStr>(word: Option<&str>, default: T) -> Result<T, &'static str> {
    word.map_or(Ok(default), |w| number(Some(w)))
}

fn channel(word: Option<&str>) -> Result<Channel, &'static str> {
    Channel::new(number(word)?).ok_or("channel must be 0..=12")
}

/// Up to three decimal digits is a length; anything else, or a `0x`
/// prefix, is hex bytes.
fn payload(word: &str) -> Result<Payload<'_>, &'static str> {
    if word.len() <= 3 && word.bytes().all(|b| b.is_ascii_digit()) {
        let len: usize = number(Some(word))?;
        return if (1..=MAX_LEN).contains(&len) {
            Ok(Payload::Len(len))
        } else {
            Err("length must be 1..=255")
        };
    }
    let hex = word.strip_prefix("0x").unwrap_or(word).as_bytes();
    if hex.is_empty() || !hex.len().is_multiple_of(2) || hex.len() / 2 > MAX_LEN {
        return Err("hex payload must be 1..=255 whole bytes");
    }
    if !hex.iter().all(u8::is_ascii_hexdigit) {
        return Err("not a hex digit");
    }
    Ok(Payload::Hex(hex))
}

/// `min..=max` whole bytes in hex, checked.
fn hex_bytes(word: &str, min: usize, max: usize) -> Result<&[u8], &'static str> {
    let hex = word.as_bytes();
    if !hex.len().is_multiple_of(2) || !(min..=max).contains(&(hex.len() / 2)) {
        return Err("wrong number of hex bytes");
    }
    if !hex.iter().all(u8::is_ascii_hexdigit) {
        return Err("not a hex digit");
    }
    Ok(hex)
}

/// Decode checked hex digits into `buf`.
fn decode_hex<'b>(hex: &[u8], buf: &'b mut [u8]) -> &'b [u8] {
    let nibble = |c: u8| match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        _ => c - b'A' + 10,
    };
    let len = hex.len() / 2;
    for (byte, pair) in buf.iter_mut().zip(hex.chunks(2)) {
        *byte = (nibble(pair[0]) << 4) | nibble(pair[1]);
    }
    &buf[..len]
}

/// Bytes as hex.
struct Hex<'b>(&'b [u8]);

impl fmt::Display for Hex<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|b| write!(f, "{:02x}", b))
    }
}

/// This board's node ID in hex.
struct Id;

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(core::str::from_utf8(&crate::node_id_hex()).expect("hex digits are ASCII"))
    }
}

/// Comma-separated values.
struct List<'l, T>(&'l [T]);

impl<T: fmt::Display> fmt::Display for List<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, v) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str(",")?;
            }
            write!(f, "{}", v)?;
        }
        Ok(())
    }
}

#[derive(Default)]
struct Counters {
    rx_ok: u32,
    rx_crc_error: u32,
    rx_header_error: u32,
    /// Packets that began, a preamble or a header seen.
    rx_started: u32,
    tx: u32,
    tx_airtime_us: u64,
    cad: u32,
    cad_busy: u32,
    pongs_sent: u32,
    pongs_received: u32,
    radio_errors: u32,
}

struct Beacon {
    ch: Channel,
    period: Duration,
    len: usize,
    next: Instant,
}

/// The console's state: settings, counters, and the radio.
pub struct Console<'a> {
    radio: Radio<'a>,
    clock: &'a Clock,
    battery: Battery<'a>,
    home: Channel,
    /// The radio is listening at home. Cleared by an error, so a radio that
    /// cannot get back to listening is not polled for interrupts (an
    /// unpowered chip's DIO1 floats).
    listening: bool,
    power: i8,
    /// Preamble symbols of `tx` and `beacon` frames.
    preamble: u16,
    beacon: Option<Beacon>,
    seq: u32,
    counters: Counters,
    rx: [u8; MAX_LEN],
    tx: [u8; MAX_LEN],
    flash: &'static Flash,
    /// The image being staged, between `stage` and `commit`.
    staging: Option<Stager<'static, Nvmc<'static>>>,
    /// The survey, in a survey build.
    survey: Option<Survey>,
}

impl<'a> Console<'a> {
    /// A console on channel 0, or the survey's channel, at the highest
    /// allowed power.
    pub fn new(
        radio: Radio<'a>,
        clock: &'a Clock,
        battery: Battery<'a>,
        flash: &'static Flash,
    ) -> Console<'a> {
        Console {
            flash,
            staging: None,
            radio,
            clock,
            battery,
            home: SURVEY.unwrap_or(Channel::new(0).expect("channel 0 is in the plan")),
            listening: false,
            power: MAX_TX_DBM,
            preamble: MIN_PREAMBLE_SYMBOLS,
            beacon: None,
            seq: 0,
            counters: Counters::default(),
            rx: [0; MAX_LEN],
            tx: [0; MAX_LEN],
            survey: SURVEY.map(|ch| Survey {
                ch,
                step: 0,
                next_beacon: Instant::now(),
                next_sample: Instant::now(),
                ring: Ring::take(),
            }),
        }
    }

    /// Serve commands forever.
    pub async fn run(&mut self, lines: &mut Lines) -> ! {
        self.recover().await;
        log::info!(
            "console ready: id={} ch={} power_dbm={}; `help` lists commands",
            Id,
            self.home.index(),
            self.power
        );
        if let Some(s) = &self.survey {
            log::info!(
                "survey ch={} lengths={} powers_dbm={}..={} sample_every_s={} ring_h={}",
                s.ch.index(),
                List(&SURVEY_LENGTHS),
                MIN_TX_DBM,
                MAX_TX_DBM,
                SAMPLE_EVERY.as_secs(),
                RING_HOURS
            );
        }
        loop {
            let beacon_at = self.beacon.as_ref().map(|b| b.next);
            let survey_at = self
                .survey
                .as_ref()
                .map(|s| s.next_beacon.min(s.next_sample));
            let due = match (beacon_at, survey_at) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
            let beacon_due = async {
                match due {
                    Some(t) => Timer::at(t).await,
                    None => core::future::pending().await,
                }
            };
            let listening = self.listening;
            let radio = &mut self.radio;
            let irq = async {
                match listening {
                    true => radio.wait().await,
                    false => core::future::pending().await,
                }
            };
            let result = match select3(lines.next(), irq, beacon_due).await {
                Either3::First(Ok(line)) => {
                    log::info!(target: ECHO, "{}", line);
                    match parse(line) {
                        Ok(command) => self.execute(command).await,
                        Err(why) => {
                            log::warn!("{}", why);
                            Ok(())
                        }
                    }
                }
                Either3::First(Err(LineError::TooLong)) => {
                    log::warn!("line longer than {} bytes ignored", LINE);
                    Ok(())
                }
                Either3::First(Err(LineError::NotUtf8)) => {
                    log::warn!("line is not UTF-8; ignored");
                    Ok(())
                }
                Either3::Second(irq) => self.serve_frame(irq).await,
                Either3::Third(()) => self.tick().await,
            };
            if let Err(e) = result {
                self.counters.radio_errors += 1;
                log::error!("radio error: {:?}", e);
                self.recover().await;
            }
        }
    }

    /// Back to listening at home, from whatever state.
    async fn recover(&mut self) {
        if let Err(e) = self.rest().await {
            self.counters.radio_errors += 1;
            log::error!("radio cannot return to listening: {:?}", e);
        }
    }

    /// Listen at home again, returning the time it took.
    async fn rest(&mut self) -> Result<u32, Error> {
        self.listening = false;
        let t0 = self.clock.now();
        self.radio.listen(self.home).await?;
        self.listening = true;
        Ok(self.clock.since(t0))
    }

    /// Send the first `len` bytes of the transmit buffer on `ch` at `dbm`
    /// after `preamble` symbols.
    async fn transmit(
        &mut self,
        ch: Channel,
        dbm: i8,
        preamble: u16,
        len: usize,
    ) -> Result<TxTiming, Error> {
        let frame = Frame {
            channel: ch,
            preamble_symbols: preamble,
            len: len as u8,
            bytes: self.tx,
        };
        self.listening = false;
        self.radio.transmit(&frame, dbm).await
    }

    /// Listen on `ch`, which may be home.
    async fn listen(&mut self, ch: Channel) -> Result<(), Error> {
        self.listening = false;
        self.radio.listen(ch).await?;
        self.listening = ch == self.home;
        Ok(())
    }

    async fn execute(&mut self, command: Command<'_>) -> Result<(), Error> {
        match command {
            Command::Ch(ch) => {
                let t0 = self.clock.now();
                self.home = ch;
                self.listen(ch).await?;
                let retune_us = self.clock.since(t0);
                log::info!(
                    "ch ch={} hz={} retune_us={}",
                    ch.index(),
                    ch.hz(),
                    retune_us
                );
            }
            Command::Tx(ch, payload) => {
                let frame = match payload {
                    Payload::Len(len) => {
                        self.seq = self.seq.wrapping_add(1);
                        bench_frame(&mut self.tx, len, KIND_TEST, self.home.index(), self.seq)
                    }
                    Payload::Hex(hex) => decode_hex(hex, &mut self.tx),
                };
                let len = frame.len();
                let (preamble, power) = (self.preamble, self.power);
                let timing = self.transmit(ch, power, preamble, len).await?;
                let airtime_us = airtime_us(len, preamble);
                self.count_tx(len, preamble);
                let rest_us = self.rest().await?;
                let tx_us = timing.tx.as_micros();
                log::info!(
                    "tx ch={} len={} preamble={} power_dbm={} prepare_us={} tx_us={} airtime_us={} excess_us={} rest_us={}",
                    ch.index(),
                    len,
                    preamble,
                    self.power,
                    timing.prepare.as_micros(),
                    tx_us,
                    airtime_us,
                    tx_us as i64 - airtime_us as i64,
                    rest_us
                );
            }
            Command::Rx(ch, ms) => {
                self.listen(ch).await?;
                let until = Instant::now() + Duration::from_millis(ms.into());
                let (ok, errors) = (self.counters.rx_ok, self.counters.rx_crc_error);
                self.listen_until(ch, until, |_| false).await?;
                log::info!(
                    "rx ch={} wait_ms={} frames={} crc_errors={}",
                    ch.index(),
                    ms,
                    self.counters.rx_ok - ok,
                    self.counters.rx_crc_error - errors
                );
                self.rest().await?;
            }
            Command::Cad(ch) => {
                self.listening = false;
                let t = self.radio.lbt(ch).await?;
                self.count_cad(t.busy);
                let rest_us = self.rest().await?;
                log::info!(
                    "cad ch={} busy={} prepare_us={} cad_us={} rest_us={}",
                    ch.index(),
                    t.busy,
                    t.prepare.as_micros(),
                    t.cad.as_micros(),
                    rest_us
                );
            }
            Command::Lbt(ch) => {
                self.listening = false;
                let t0 = self.clock.now();
                let t = self.radio.lbt(ch).await?;
                self.count_cad(t.busy);
                let rest_us = self.rest().await?;
                let total_us = self.clock.since(t0);
                log::info!(
                    "lbt ch={} busy={} dbm={} inside_dbm={:?} prepare_us={} read_us={} cad_us={} after_us={} rest_us={} total_us={}",
                    ch.index(),
                    t.busy,
                    t.dbm,
                    t.inside_dbm,
                    t.prepare.as_micros(),
                    t.read.as_micros(),
                    t.cad.as_micros(),
                    t.after.as_micros(),
                    rest_us,
                    total_us
                );
            }
            Command::Tcxo => self.report_tcxo(),
            Command::Probe => {
                self.listening = false;
                let t0 = self.clock.now();
                self.radio.reprobe().await?;
                let probe_us = self.clock.since(t0);
                self.rest().await?;
                log::info!("probe took_us={}", probe_us);
                self.report_tcxo();
            }
            Command::Rssi(ch, dwell_us) => {
                let t0 = self.clock.now();
                self.listen(ch).await?;
                let retune_us = self.clock.since(t0);
                let t1 = self.clock.now();
                if dwell_us > 0 {
                    Timer::after_micros(dwell_us.into()).await;
                }
                let dwelt_us = self.clock.since(t1);
                let t2 = self.clock.now();
                let dbm = self.radio.rssi().await?;
                let read_us = self.clock.since(t2);
                self.rest().await?;
                log::info!(
                    "rssi ch={} dbm={} retune_us={} dwell_us={} read_us={}",
                    ch.index(),
                    dbm,
                    retune_us,
                    dwelt_us,
                    read_us
                );
            }
            Command::Sweep => {
                let mut dbm = [0i16; CHANNELS as usize];
                let t0 = self.clock.now();
                for (ch, reading) in Channel::all().zip(dbm.iter_mut()) {
                    self.listen(ch).await?;
                    *reading = self.radio.rssi().await?;
                }
                self.rest().await?;
                let total_us = self.clock.since(t0);
                log::info!("sweep total_us={} dbm={}", total_us, List(&dbm));
            }
            Command::Noise(ch, n, gap_us) => {
                let mut samples = [0i16; NOISE_MAX];
                let t0 = self.clock.now();
                self.listen(ch).await?;
                for sample in samples.iter_mut().take(n) {
                    *sample = self.radio.rssi().await?;
                    if gap_us > 0 {
                        Timer::after_micros(gap_us.into()).await;
                    }
                }
                self.rest().await?;
                let total_us = self.clock.since(t0);
                let samples = &mut samples[..n];
                samples.sort_unstable();
                log::info!(
                    "noise ch={} n={} min={} p10={} median={} max={} total_us={}",
                    ch.index(),
                    n,
                    samples[0],
                    percentile(samples, 10),
                    percentile(samples, 50),
                    samples[n - 1],
                    total_us
                );
            }
            Command::Power(dbm) => {
                self.power = dbm.clamp(MIN_TX_DBM.into(), MAX_TX_DBM.into()) as i8;
                log::info!(
                    "power requested_dbm={} power_dbm={} max_dbm={}",
                    dbm,
                    self.power,
                    MAX_TX_DBM
                );
            }
            Command::Preamble(symbols) => {
                self.preamble = symbols;
                log::info!(
                    "preamble symbols={} airtime_us_{}={} airtime_us_255={}",
                    symbols,
                    HEARTBEAT_LEN,
                    airtime_us(HEARTBEAT_LEN, symbols),
                    airtime_us(MAX_LEN, symbols)
                );
            }
            Command::Beacon(ch, period_ms, len) => {
                // Regulatory duty cycle (spec "Regulatory limits"): airtime at most
                // DUTY_CYCLE_PERCENT of the period.
                let min_ms = (airtime_us(len, self.preamble) * 100 / u64::from(DUTY_CYCLE_PERCENT))
                    .div_ceil(1000);
                if u64::from(period_ms) < min_ms {
                    log::warn!(
                        "beacon: a {}-byte frame needs a period of at least {} ms for {}% duty cycle",
                        len,
                        min_ms,
                        DUTY_CYCLE_PERCENT
                    );
                } else {
                    let period = Duration::from_millis(period_ms.into());
                    self.beacon = Some(Beacon {
                        ch,
                        period,
                        len,
                        next: Instant::now(),
                    });
                    log::info!(
                        "beacon ch={} period_ms={} len={} preamble={}",
                        ch.index(),
                        period_ms,
                        len,
                        self.preamble
                    );
                }
            }
            Command::Stop => {
                self.beacon = None;
                log::info!("stop");
            }
            Command::Ping(ch) => self.ping(ch).await?,
            Command::Status(reset) => self.status(reset).await,
            Command::Survey(n) => self.print_survey(n).await,
            Command::Help => log::info!("{}", HELP),
            Command::Stage(hex) => {
                let mut header = [0u8; HEADER_LEN];
                decode_hex(hex, &mut header);
                self.staging = None;
                match Stager::begin(self.flash, &header, BARK_IMAGE_VERSION) {
                    Ok(s) => {
                        let h = s.header();
                        log::info!(
                            "stage version={} length={} sha256={} next=0",
                            h.version,
                            h.length,
                            Hex(&h.hash)
                        );
                        self.staging = Some(s);
                    }
                    Err(e) => log::warn!("stage refused={:?}", e),
                }
            }
            Command::Chunk(offset, hex) => {
                let Some(staging) = self.staging.as_mut() else {
                    log::warn!("chunk refused: no `stage` before it");
                    return Ok(());
                };
                let mut bytes = [0u8; CHUNK];
                let bytes = decode_hex(hex, &mut bytes);
                match staging.write(offset, bytes) {
                    Ok(next) => log::info!("chunk next={}", next),
                    Err(e) => log::warn!("chunk refused={:?} next={}", e, staging.next()),
                }
            }
            Command::Commit => match self.staging.take().map(Stager::finish) {
                None => log::warn!("commit refused: no `stage` before it"),
                Some(Ok(h)) => log::info!(
                    "commit version={} length={}: the loader swaps it in at the next boot; `reboot`",
                    h.version,
                    h.length
                ),
                Some(Err(e)) => log::warn!("commit refused={:?}", e),
            },
            Command::Reboot => {
                log::info!("reboot");
                Timer::after(LOG_DRAIN).await;
                cortex_m::peripheral::SCB::sys_reset();
            }
        }
        Ok(())
    }

    /// Wait for frames on the channel being listened to until `until`, and
    /// report each; stop at the first frame or CRC failure `accept` takes.
    /// Returns whether one was taken.
    async fn listen_until(
        &mut self,
        ch: Channel,
        until: Instant,
        mut accept: impl FnMut(Option<Bench>) -> bool,
    ) -> Result<bool, Error> {
        loop {
            match select(Timer::at(until), self.radio.wait()).await {
                Either::First(()) => return Ok(false),
                Either::Second(result) => {
                    result?;
                    match self.receive(ch).await? {
                        Some(bench) if accept(bench) => return Ok(true),
                        _ => {}
                    }
                }
            }
        }
    }

    /// Take what an interrupt announced and report it. `None` if there
    /// was no frame; `Some(bench header)` otherwise.
    async fn receive(&mut self, ch: Channel) -> Result<Option<Option<Bench>>, Error> {
        let heard = self.radio.service(&mut self.rx).await?;
        let at_us = self.clock.now();
        match heard {
            Heard::Frame {
                len,
                rssi_dbm,
                snr_db,
            } => {
                let len = usize::from(len);
                self.counters.rx_ok += 1;
                let bench = parse_bench(&self.rx[..len]);
                report_frame(ch, &self.rx[..len], rssi_dbm, snr_db, "ok", at_us, bench);
                report_heartbeat(&self.rx[..len]);
                Ok(Some(bench))
            }
            Heard::Failed if self.radio.last_flags() & irq::CRC_ERR != 0 => {
                // The bytes are in the chip's buffer still: what arrived.
                self.counters.rx_crc_error += 1;
                let (len, rssi_dbm, snr_db) = self.radio.read_last_frame(&mut self.rx).await?;
                let len = usize::from(len);
                let bench = parse_bench(&self.rx[..len]);
                report_frame(ch, &self.rx[..len], rssi_dbm, snr_db, "error", at_us, bench);
                Ok(Some(None))
            }
            Heard::Failed => {
                self.counters.rx_header_error += 1;
                log::info!("rx ch={} header_crc=error", ch.index());
                Ok(None)
            }
            Heard::Started => {
                self.counters.rx_started += 1;
                Ok(None)
            }
            Heard::Nothing => Ok(None),
        }
    }

    /// While resting: report the frame and answer a ping.
    async fn serve_frame(&mut self, irq: Result<(), Error>) -> Result<(), Error> {
        irq?;
        let Some(Some(bench)) = self.receive(self.home).await? else {
            return Ok(());
        };
        let t_rx = self.clock.now();
        if bench.kind != KIND_PING {
            return Ok(());
        }
        let Some(reply) = Channel::new(bench.arg) else {
            log::warn!(
                "ping seq={} names channel {}, not in the plan",
                bench.seq,
                bench.arg
            );
            return Ok(());
        };
        // Listen before talk as a receiver does before an ACK
        // (spec "The ACK").
        self.listening = false;
        let check = self.radio.lbt(reply).await?;
        self.count_cad(check.busy);
        if check.busy {
            let rest_us = self.rest().await?;
            log::info!(
                "pong ch={} seq={} busy=true sent=false rest_us={}",
                reply.index(),
                bench.seq,
                rest_us
            );
            return Ok(());
        }
        bench_frame(
            &mut self.tx,
            ACK_LEN,
            KIND_PONG,
            self.home.index(),
            bench.seq,
        );
        let power = self.power;
        let timing = self
            .transmit(reply, power, MIN_PREAMBLE_SYMBOLS, ACK_LEN)
            .await?;
        let turnaround_us = self.clock.since(t_rx);
        self.count_tx(ACK_LEN, MIN_PREAMBLE_SYMBOLS);
        self.counters.pongs_sent += 1;
        let rest_us = self.rest().await?;
        log::info!(
            "pong ch={} seq={} lbt_prepare_us={} cad_us={} lbt_dbm={} tx_prepare_us={} tx_us={} rx_to_tx_done_us={} rest_us={}",
            reply.index(),
            bench.seq,
            check.prepare.as_micros(),
            check.cad.as_micros(),
            check.dbm,
            timing.prepare.as_micros(),
            timing.tx.as_micros(),
            turnaround_us,
            rest_us
        );
        Ok(())
    }

    async fn ping(&mut self, ch: Channel) -> Result<(), Error> {
        self.seq = self.seq.wrapping_add(1);
        let seq = self.seq;
        bench_frame(&mut self.tx, ACK_LEN, KIND_PING, self.home.index(), seq);
        let power = self.power;
        let timing = self
            .transmit(ch, power, MIN_PREAMBLE_SYMBOLS, ACK_LEN)
            .await?;
        let t_sent = self.clock.now();
        let until = Instant::now() + PING_WAIT;
        self.count_tx(ACK_LEN, MIN_PREAMBLE_SYMBOLS);
        let rest_us = self.rest().await?;
        let home = self.home;
        let answered = self
            .listen_until(
                home,
                until,
                |bench| matches!(bench, Some(Bench { kind: KIND_PONG, seq: s, .. }) if s == seq),
            )
            .await?;
        let rtt_us = self.clock.since(t_sent);
        if answered {
            self.counters.pongs_received += 1;
            log::info!(
                "ping ch={} seq={} tx_prepare_us={} tx_us={} rest_us={} rtt_us={} ack_timeout_us={} within={}",
                ch.index(),
                seq,
                timing.prepare.as_micros(),
                timing.tx.as_micros(),
                rest_us,
                rtt_us,
                ACK_TIMEOUT_US,
                rtt_us <= ACK_TIMEOUT_US
            );
        } else {
            log::info!(
                "ping ch={} seq={} pong=none wait_ms={}",
                ch.index(),
                seq,
                PING_WAIT.as_millis()
            );
        }
        Ok(())
    }

    /// Whatever the timer came for: the beacon, the survey's beacon, the
    /// survey's noise reading.
    async fn tick(&mut self) -> Result<(), Error> {
        let now = Instant::now();
        if self.beacon.as_ref().is_some_and(|b| b.next <= now) {
            self.send_beacon().await?;
        }
        if self.survey.as_ref().is_some_and(|s| s.next_beacon <= now) {
            self.survey_beacon().await?;
        }
        if self.survey.as_ref().is_some_and(|s| s.next_sample <= now) {
            self.survey_sample().await?;
        }
        Ok(())
    }

    async fn send_beacon(&mut self) -> Result<(), Error> {
        let Some(beacon) = self.beacon.as_mut() else {
            return Ok(());
        };
        // A late beacon starts a new schedule rather than bursting to catch up.
        beacon.next = (beacon.next + beacon.period).max(Instant::now());
        let (ch, len) = (beacon.ch, beacon.len);
        self.seq = self.seq.wrapping_add(1);
        bench_frame(&mut self.tx, len, KIND_TEST, self.home.index(), self.seq);
        let (preamble, power) = (self.preamble, self.power);
        let timing = self.transmit(ch, power, preamble, len).await?;
        self.count_tx(len, preamble);
        self.rest().await?;
        log::info!(
            "beacon ch={} seq={} len={} tx_us={}",
            ch.index(),
            self.seq,
            len,
            timing.tx.as_micros()
        );
        Ok(())
    }

    /// The next survey beacon: one frame, then silence of the rest of the
    /// period the regulatory duty cycle allows it, so that no window
    /// exceeds the cycle (spec "Regulatory limits").
    async fn survey_beacon(&mut self) -> Result<(), Error> {
        let Some(survey) = self.survey.as_mut() else {
            return Ok(());
        };
        let (len, power) = survey_step(survey.step);
        survey.step = (survey.step + 1) % SURVEY_STEPS;
        let ch = survey.ch;
        let preamble = self.preamble;
        let period =
            Duration::from_micros(airtime_us(len, preamble) * 100 / u64::from(DUTY_CYCLE_PERCENT));
        self.seq = self.seq.wrapping_add(1);
        bench_frame(&mut self.tx, len, KIND_SURVEY, power as u8, self.seq);
        let sent = Instant::now();
        self.transmit(ch, power, preamble, len).await?;
        self.count_tx(len, preamble);
        if let Some(survey) = self.survey.as_mut() {
            survey.next_beacon = sent + period;
        }
        self.rest().await?;
        Ok(())
    }

    /// One minute's noise reading on every channel, into the ring and the
    /// log, as `noise <k> 100` would read each channel after the dwell.
    async fn survey_sample(&mut self) -> Result<(), Error> {
        let Some(survey) = self.survey.as_mut() else {
            return Ok(());
        };
        survey.next_sample = (survey.next_sample + SAMPLE_EVERY).max(Instant::now());
        let mut sample = Sample {
            at_s: Instant::now().as_secs() as u32,
            ..Sample::ZERO
        };
        let mut readings = [0i16; SAMPLE_READINGS];
        for (i, ch) in Channel::all().enumerate() {
            self.listen(ch).await?;
            Timer::after(SAMPLE_DWELL).await;
            for reading in readings.iter_mut() {
                *reading = self.radio.rssi().await?;
            }
            readings.sort_unstable();
            sample.p10[i] = dbm(percentile(&readings, 10));
            sample.median[i] = dbm(percentile(&readings, 50));
        }
        self.rest().await?;
        log::info!(
            "survey t_s={} p10={} median={}",
            sample.at_s,
            List(&sample.p10),
            List(&sample.median)
        );
        if let Some(survey) = self.survey.as_mut() {
            survey.ring.push(sample);
        }
        Ok(())
    }

    /// The newest `n` samples of the ring, oldest first, paced for the log.
    async fn print_survey(&mut self, n: usize) {
        let Some(survey) = self.survey.as_ref() else {
            log::warn!("survey: not a survey build (BARK_SURVEY at build time)");
            return;
        };
        log::info!(
            "survey ch={} step={} of {} samples={} of {} every_s={}",
            survey.ch.index(),
            survey.step,
            SURVEY_STEPS,
            survey.ring.count,
            RING_LEN,
            SAMPLE_EVERY.as_secs()
        );
        for sample in survey.ring.newest(n) {
            log::info!(
                "survey t_s={} p10={} median={}",
                sample.at_s,
                List(&sample.p10),
                List(&sample.median)
            );
            Timer::after(SURVEY_LINE_DRAIN).await;
        }
    }

    async fn status(&mut self, reset: bool) {
        let battery_mv = self.battery.read().await;
        let c = &self.counters;
        let r = self.radio.stats();
        log::info!(
            "status id={} uptime_s={} battery_mv={} ch={} hz={} power_dbm={} beacon={} rx_ok={} rx_crc_error={} \
rx_header_error={} rx_started={} tx={} tx_airtime_us={} cad={} cad_busy={} pongs_sent={} pongs_received={} \
radio_errors={} resets={} configured={} cold_starts={} tcxo_us={}",
            Id,
            Instant::now().as_secs(),
            battery_mv,
            self.home.index(),
            self.home.hz(),
            self.power,
            self.beacon.is_some(),
            c.rx_ok,
            c.rx_crc_error,
            c.rx_header_error,
            c.rx_started,
            c.tx,
            c.tx_airtime_us,
            c.cad,
            c.cad_busy,
            c.pongs_sent,
            c.pongs_received,
            c.radio_errors,
            r.resets,
            r.configured,
            r.cold_starts,
            self.radio.tcxo().delay.as_micros()
        );
        if reset {
            self.counters = Counters::default();
            log::info!("status counters reset");
        }
    }

    /// The oscillator's delay and the last probe (`bark_driver::tcxo`).
    fn report_tcxo(&self) {
        let t = self.radio.tcxo();
        let r = self.radio.stats();
        match t.last {
            Some(p) => log::info!(
                "tcxo delay_us={} short_us={} step_us={} probes={} errors={} cold_starts={} last_probe_us={} last_ok={} last_at_s={} last_temp_qc={:?} lbt_read={:?}",
                t.delay.as_micros(),
                tcxo::SHORT.as_micros(),
                tcxo::STEP.as_micros(),
                t.probes,
                t.errors,
                r.cold_starts,
                p.delay.as_micros(),
                p.ok,
                p.at.as_micros() / 1_000_000,
                p.temp,
                self.radio.lbt_read()
            ),
            None => log::info!(
                "tcxo delay_us={} short_us={} probes=0 errors={} cold_starts={} lbt_read={:?}",
                t.delay.as_micros(),
                tcxo::SHORT.as_micros(),
                t.errors,
                r.cold_starts,
                self.radio.lbt_read()
            ),
        }
    }

    fn count_tx(&mut self, len: usize, preamble: u16) {
        self.counters.tx += 1;
        self.counters.tx_airtime_us += airtime_us(len, preamble);
    }

    fn count_cad(&mut self, busy: bool) {
        self.counters.cad += 1;
        self.counters.cad_busy += u32::from(busy);
    }
}

/// A frame heard. A survey frame is one short line, since a host logs
/// thousands: its sender's power in place of the bytes.
fn report_frame(
    ch: Channel,
    frame: &[u8],
    rssi_dbm: i16,
    snr_db: i8,
    crc: &str,
    at_us: u32,
    bench: Option<Bench>,
) {
    match bench {
        Some(Bench {
            kind: KIND_SURVEY,
            arg,
            seq,
        }) => log::info!(
            "rx ch={} len={} rssi_dbm={} snr_db={} crc={} at_us={} survey_power_dbm={} seq={}",
            ch.index(),
            frame.len(),
            rssi_dbm,
            snr_db,
            crc,
            at_us,
            arg as i8,
            seq
        ),
        _ => log::info!(
            "rx ch={} len={} rssi_dbm={} snr_db={} crc={} at_us={} bytes={}",
            ch.index(),
            frame.len(),
            rssi_dbm,
            snr_db,
            crc,
            at_us,
            Hex(frame)
        ),
    }
}

/// A whole-dBm reading in a byte; the chip's range fits.
fn dbm(reading: i16) -> i8 {
    reading.clamp(i8::MIN.into(), i8::MAX.into()) as i8
}

/// A heartbeat's sender, the newest format it speaks and whether it stands
/// on a map point, so that a person at the console learns them of a
/// planted node.
fn report_heartbeat(frame: &[u8]) {
    if let Ok(Packet::Heartbeat(h)) = Packet::decode(frame) {
        log::info!(
            "heartbeat id={} format={} map_point={} home={}",
            Hex(&h.id.0),
            h.format,
            h.map_point,
            h.home.index()
        );
    }
}

/// Time on air of a `len`-byte frame after `preamble` symbols, µs.
fn airtime_us(len: usize, preamble: u16) -> u64 {
    time_on_air(len as u8, preamble).as_micros()
}

/// Nearest-rank percentile of sorted, non-empty `samples`.
fn percentile(samples: &[i16], p: usize) -> i16 {
    let rank = (p * samples.len()).div_ceil(100).max(1);
    samples[rank - 1]
}
