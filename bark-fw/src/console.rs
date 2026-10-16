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
//! rx <k> [ms]                listen on k for a frame (default 10 s)
//! cad <k>                    one channel activity detection
//! rssi <k> [dwell_us]        one RSSI reading
//! sweep                      one RSSI reading on each channel
//! noise <k> <n> [gap_us]     n RSSI readings: min, p10, median, max
//! power <dbm>                transmit power, clamped to the ERP limit
//! beacon <k> <ms> [len]      send a frame every ms; stop with `stop`
//! ping <k>                   send a ping on k, wait home for the pong
//! status [reset]             uptime, battery, settings, counters
//! ```
//!
//! Bench frames use packet type 0xF, which the protocol leaves unassigned
//! (spec "Packet formats"): byte 0 is `0xF0 | kind`, bytes 1..5 a big-endian
//! sequence number, byte 5 the sender's home channel, and the rest
//! `byte[i] = i`.

use core::fmt;

use bark_proto::radio::{time_on_air, Channel, CHANNELS, DUTY_CYCLE_PERCENT, MIN_PREAMBLE_SYMBOLS};
use bark_proto::wire::ACK_LEN;
use embassy_futures::select::{select, select3, Either, Either3};
use embassy_nrf::saadc::Saadc;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::pipe::Pipe;
use embassy_time::{Duration, Instant, Timer};
use embassy_usb_logger::ReceiverHandler;
use lora_phy::mod_params::{PacketStatus, RadioError};

use crate::clock::Clock;
use crate::radio::{Radio, Received, MAX_LEN, MAX_TX_DBM, MIN_TX_DBM};

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

/// Bench frame kinds (byte 0 low nibble).
const KIND_TEST: u8 = 0;
const KIND_PING: u8 = 1;
const KIND_PONG: u8 = 2;
const BENCH_TYPE: u8 = 0xF0;
/// Type byte, sequence number, home channel.
const BENCH_HEADER: usize = 6;

/// A bench frame's header fields.
#[derive(Clone, Copy)]
struct Bench {
    kind: u8,
    seq: u32,
    channel: u8,
}

fn bench_frame(buf: &mut [u8; MAX_LEN], len: usize, kind: u8, seq: u32, home: Channel) -> &[u8] {
    let mut header = [0u8; BENCH_HEADER];
    header[0] = BENCH_TYPE | kind;
    header[1..5].copy_from_slice(&seq.to_be_bytes());
    header[5] = home.index();
    for (i, b) in buf.iter_mut().enumerate().take(len) {
        *b = if i < BENCH_HEADER { header[i] } else { i as u8 };
    }
    &buf[..len]
}

fn parse_bench(frame: &[u8]) -> Option<Bench> {
    if frame.len() < BENCH_HEADER || frame[0] & 0xF0 != BENCH_TYPE {
        return None;
    }
    Some(Bench {
        kind: frame[0] & 0x0F,
        seq: u32::from_be_bytes([frame[1], frame[2], frame[3], frame[4]]),
        channel: frame[5],
    })
}

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
    Rssi(Channel, u32),
    Sweep,
    Noise(Channel, usize, u32),
    Power(i32),
    Beacon(Channel, u32, usize),
    Stop,
    Ping(Channel),
    Status(bool),
    Help,
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
    "ch <k> | tx <k> <hex|len> | rx <k> [ms] | cad <k> | rssi <k> [dwell_us] | sweep | \
noise <k> <n> [gap_us] | power <dbm> | beacon <k> <ms> [len] | stop | ping <k> | status [reset]";

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
        "beacon" => {
            let ch = channel(arg())?;
            let period = number(arg())?;
            let len = optional(arg(), ACK_LEN)?;
            if !(BENCH_HEADER..=MAX_LEN).contains(&len) {
                return Err("beacon: len must be 6..=255");
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
        "help" | "?" => Command::Help,
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

/// Decode checked hex digits into `buf`.
fn decode_hex<'b>(hex: &[u8], buf: &'b mut [u8; MAX_LEN]) -> &'b [u8] {
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

/// A value in quarter units (dB or dBm) as a decimal.
struct Quarter(i16);

impl fmt::Display for Quarter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let q = self.0.unsigned_abs();
        write!(f, "{}{}.{:02}", sign, q / 4, (q % 4) * 25)
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
    /// A preamble interrupt with no frame after it within the longest
    /// frame's airtime.
    rx_no_frame: u32,
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
    battery: Saadc<'a, 1>,
    home: Channel,
    /// The radio is listening at home. Cleared by an error, so a radio that
    /// cannot get back to listening is not polled for interrupts (an
    /// unpowered chip's DIO1 floats).
    listening: bool,
    power: i8,
    beacon: Option<Beacon>,
    seq: u32,
    counters: Counters,
    rx: [u8; MAX_LEN],
    tx: [u8; MAX_LEN],
}

impl<'a> Console<'a> {
    /// A console on channel 0 at the highest allowed power.
    pub fn new(radio: Radio<'a>, clock: &'a Clock, battery: Saadc<'a, 1>) -> Console<'a> {
        Console {
            radio,
            clock,
            battery,
            home: Channel::new(0).expect("channel 0 is in the plan"),
            listening: false,
            power: MAX_TX_DBM,
            beacon: None,
            seq: 0,
            counters: Counters::default(),
            rx: [0; MAX_LEN],
            tx: [0; MAX_LEN],
        }
    }

    /// Serve commands forever.
    pub async fn run(&mut self, lines: &mut Lines) -> ! {
        self.battery.calibrate().await;
        self.recover().await;
        log::info!(
            "console ready: ch={} power_dbm={}; `help` lists commands",
            self.home.index(),
            self.power
        );
        loop {
            let beacon_at = self.beacon.as_ref().map(|b| b.next);
            let beacon_due = async {
                match beacon_at {
                    Some(t) => Timer::at(t).await,
                    None => core::future::pending().await,
                }
            };
            let listening = self.listening;
            let radio = &mut self.radio;
            let irq = async {
                match listening {
                    true => radio.wait_irq().await,
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
                Either3::Third(()) => self.send_beacon().await,
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
    async fn rest(&mut self) -> Result<u32, RadioError> {
        self.listening = false;
        let t0 = self.clock.now();
        self.radio.rest(self.home).await?;
        self.listening = true;
        Ok(self.clock.since(t0))
    }

    async fn execute(&mut self, command: Command<'_>) -> Result<(), RadioError> {
        match command {
            Command::Ch(ch) => {
                let t0 = self.clock.now();
                self.radio.retune(ch).await?;
                let retune_us = self.clock.since(t0);
                self.home = ch;
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
                        bench_frame(&mut self.tx, len, KIND_TEST, self.seq, self.home)
                    }
                    Payload::Hex(hex) => decode_hex(hex, &mut self.tx),
                };
                let len = frame.len();
                let timing = self
                    .radio
                    .transmit(ch, self.power, MIN_PREAMBLE_SYMBOLS, frame)
                    .await?;
                let airtime_us = airtime_us(len);
                self.count_tx(len);
                let rest_us = self.rest().await?;
                log::info!(
                    "tx ch={} len={} power_dbm={} prepare_us={} tx_us={} airtime_us={} excess_us={} rest_us={}",
                    ch.index(),
                    len,
                    self.power,
                    timing.prepare_us,
                    timing.tx_us,
                    airtime_us,
                    timing.tx_us as i64 - airtime_us as i64,
                    rest_us
                );
            }
            Command::Rx(ch, ms) => {
                if ch != self.home {
                    self.radio.retune(ch).await?;
                }
                let until = Instant::now() + Duration::from_millis(ms.into());
                let heard = self.listen_until(ch, until, |_| true).await?;
                if !heard {
                    log::info!("rx ch={} timeout_ms={}", ch.index(), ms);
                }
                if ch != self.home {
                    self.radio.retune(self.home).await?;
                }
            }
            Command::Cad(ch) => {
                let (busy, prepare_us, cad_us) = self.radio.cad(ch).await?;
                self.count_cad(busy);
                let rest_us = self.rest().await?;
                log::info!(
                    "cad ch={} busy={} prepare_us={} cad_us={} rest_us={}",
                    ch.index(),
                    busy,
                    prepare_us,
                    cad_us,
                    rest_us
                );
            }
            Command::Rssi(ch, dwell_us) => {
                let t0 = self.clock.now();
                if ch != self.home {
                    self.radio.retune(ch).await?;
                }
                let retune_us = self.clock.since(t0);
                let t1 = self.clock.now();
                if dwell_us > 0 {
                    Timer::after_micros(dwell_us.into()).await;
                }
                let dwelt_us = self.clock.since(t1);
                let t2 = self.clock.now();
                let dbm = self.radio.rssi().await?;
                let read_us = self.clock.since(t2);
                if ch != self.home {
                    self.radio.retune(self.home).await?;
                }
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
                    self.radio.retune(ch).await?;
                    *reading = self.radio.rssi().await?;
                }
                self.radio.retune(self.home).await?;
                let total_us = self.clock.since(t0);
                log::info!("sweep total_us={} dbm={}", total_us, List(&dbm));
            }
            Command::Noise(ch, n, gap_us) => {
                let mut samples = [0i16; NOISE_MAX];
                let t0 = self.clock.now();
                if ch != self.home {
                    self.radio.retune(ch).await?;
                }
                for sample in samples.iter_mut().take(n) {
                    *sample = self.radio.rssi().await?;
                    if gap_us > 0 {
                        Timer::after_micros(gap_us.into()).await;
                    }
                }
                if ch != self.home {
                    self.radio.retune(self.home).await?;
                }
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
            Command::Beacon(ch, period_ms, len) => {
                // Regulatory duty cycle (spec "Regulatory limits"): airtime at most
                // DUTY_CYCLE_PERCENT of the period.
                let min_ms = (airtime_us(len) * 100 / u64::from(DUTY_CYCLE_PERCENT)).div_ceil(1000);
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
                        "beacon ch={} period_ms={} len={}",
                        ch.index(),
                        period_ms,
                        len
                    );
                }
            }
            Command::Stop => {
                self.beacon = None;
                log::info!("stop");
            }
            Command::Ping(ch) => self.ping(ch).await?,
            Command::Status(reset) => self.status(reset).await,
            Command::Help => log::info!("{}", HELP),
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
    ) -> Result<bool, RadioError> {
        loop {
            match select(Timer::at(until), self.radio.wait_irq()).await {
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

    /// Take the frame an interrupt announced and report it. `None` if
    /// there was no frame; `Some(bench header)` otherwise.
    async fn receive(&mut self, ch: Channel) -> Result<Option<Option<Bench>>, RadioError> {
        let received = match self.radio.receive(&mut self.rx).await {
            Err(RadioError::DIO1) => {
                self.counters.rx_no_frame += 1;
                return Ok(None);
            }
            other => other?,
        };
        let at_us = self.clock.now();
        match received {
            Received::Frame { len, status } => {
                self.counters.rx_ok += 1;
                report_frame(ch, &self.rx[..len], &status, "ok", at_us);
                Ok(Some(parse_bench(&self.rx[..len])))
            }
            Received::CrcError { len, status } => {
                self.counters.rx_crc_error += 1;
                report_frame(ch, &self.rx[..len], &status, "error", at_us);
                Ok(Some(None))
            }
            Received::HeaderError => {
                self.counters.rx_header_error += 1;
                log::info!("rx ch={} header_crc=error", ch.index());
                Ok(None)
            }
        }
    }

    /// While resting: report the frame and answer a ping.
    async fn serve_frame(&mut self, irq: Result<(), RadioError>) -> Result<(), RadioError> {
        irq?;
        let Some(Some(bench)) = self.receive(self.home).await? else {
            return Ok(());
        };
        let t_rx = self.clock.now();
        if bench.kind != KIND_PING {
            return Ok(());
        }
        let Some(reply) = Channel::new(bench.channel) else {
            log::warn!(
                "ping seq={} names channel {}, not in the plan",
                bench.seq,
                bench.channel
            );
            return Ok(());
        };
        // Listen before talk as a receiver does before an ACK
        // (spec "Hand-over"), CAD only.
        let (busy, cad_prepare_us, cad_us) = self.radio.cad(reply).await?;
        self.count_cad(busy);
        if busy {
            let rest_us = self.rest().await?;
            log::info!(
                "pong ch={} seq={} busy=true sent=false rest_us={}",
                reply.index(),
                bench.seq,
                rest_us
            );
            return Ok(());
        }
        let frame = bench_frame(&mut self.tx, ACK_LEN, KIND_PONG, bench.seq, self.home);
        let timing = self
            .radio
            .transmit(reply, self.power, MIN_PREAMBLE_SYMBOLS, frame)
            .await?;
        let turnaround_us = self.clock.since(t_rx);
        self.count_tx(ACK_LEN);
        self.counters.pongs_sent += 1;
        let rest_us = self.rest().await?;
        log::info!(
            "pong ch={} seq={} cad_prepare_us={} cad_us={} tx_prepare_us={} tx_us={} rx_to_tx_done_us={} rest_us={}",
            reply.index(),
            bench.seq,
            cad_prepare_us,
            cad_us,
            timing.prepare_us,
            timing.tx_us,
            turnaround_us,
            rest_us
        );
        Ok(())
    }

    async fn ping(&mut self, ch: Channel) -> Result<(), RadioError> {
        self.seq = self.seq.wrapping_add(1);
        let seq = self.seq;
        let frame = bench_frame(&mut self.tx, ACK_LEN, KIND_PING, seq, self.home);
        let timing = self
            .radio
            .transmit(ch, self.power, MIN_PREAMBLE_SYMBOLS, frame)
            .await?;
        let t_sent = self.clock.now();
        let until = Instant::now() + PING_WAIT;
        self.count_tx(ACK_LEN);
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
                timing.prepare_us,
                timing.tx_us,
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

    async fn send_beacon(&mut self) -> Result<(), RadioError> {
        let Some(beacon) = self.beacon.as_mut() else {
            return Ok(());
        };
        // A late beacon starts a new schedule rather than bursting to catch up.
        beacon.next = (beacon.next + beacon.period).max(Instant::now());
        let (ch, len) = (beacon.ch, beacon.len);
        self.seq = self.seq.wrapping_add(1);
        let frame = bench_frame(&mut self.tx, len, KIND_TEST, self.seq, self.home);
        let timing = self
            .radio
            .transmit(ch, self.power, MIN_PREAMBLE_SYMBOLS, frame)
            .await?;
        self.count_tx(len);
        self.rest().await?;
        log::info!(
            "beacon ch={} seq={} len={} tx_us={}",
            ch.index(),
            self.seq,
            len,
            timing.tx_us
        );
        Ok(())
    }

    async fn status(&mut self, reset: bool) {
        let mut raw = [0i16; 1];
        self.battery.sample(&mut raw).await;
        // board.rs, Battery: mV = raw × 7200 / 4096.
        let battery_mv = i32::from(raw[0]).max(0) * 7200 / 4096;
        let c = &self.counters;
        log::info!(
            "status uptime_s={} battery_mv={} ch={} hz={} power_dbm={} beacon={} rx_ok={} rx_crc_error={} \
rx_header_error={} rx_no_frame={} tx={} tx_airtime_us={} cad={} cad_busy={} pongs_sent={} pongs_received={} \
radio_errors={}",
            Instant::now().as_secs(),
            battery_mv,
            self.home.index(),
            self.home.hz(),
            self.power,
            self.beacon.is_some(),
            c.rx_ok,
            c.rx_crc_error,
            c.rx_header_error,
            c.rx_no_frame,
            c.tx,
            c.tx_airtime_us,
            c.cad,
            c.cad_busy,
            c.pongs_sent,
            c.pongs_received,
            c.radio_errors
        );
        if reset {
            self.counters = Counters::default();
            log::info!("status counters reset");
        }
    }

    fn count_tx(&mut self, len: usize) {
        self.counters.tx += 1;
        self.counters.tx_airtime_us += airtime_us(len);
    }

    fn count_cad(&mut self, busy: bool) {
        self.counters.cad += 1;
        self.counters.cad_busy += u32::from(busy);
    }
}

fn report_frame(ch: Channel, frame: &[u8], status: &PacketStatus, crc: &str, at_us: u32) {
    log::info!(
        "rx ch={} len={} rssi_dbm={} snr_db={} crc={} at_us={} bytes={}",
        ch.index(),
        frame.len(),
        Quarter(status.rssi.quarter_dbm()),
        Quarter(status.snr.quarter_db()),
        crc,
        at_us,
        Hex(frame)
    );
}

/// Time on air of a `len`-byte frame with bark's preamble, µs.
fn airtime_us(len: usize) -> u64 {
    time_on_air(len as u8, MIN_PREAMBLE_SYMBOLS).as_micros()
}

/// Nearest-rank percentile of sorted, non-empty `samples`.
fn percentile(samples: &[i16], p: usize) -> i16 {
    let rank = (p * samples.len()).div_ceil(100).max(1);
    samples[rank - 1]
}
