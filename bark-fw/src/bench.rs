//! The bench build, `--features bench` (README, "Bench build"): the node
//! build that also takes commands on the serial port, for the acceptance
//! and stress tests of `docs/bench-suite-2026-11-13.md`. No release sets
//! it.
//!
//! The commands are parsed in `bark_fw_logic::bench`. What they change
//! reaches the node through the two things it already has: the
//! application the driver runs, here wrapped in [`App`], which offers a
//! text as a phone's message is offered and tells the core the power it
//! is told, and the node's loop between two steps ([`serve`]), which sets
//! the transmit power and sends a carrier.

use core::cell::{Cell, RefCell};

use bark_driver::{Clock, Driver, Outgoing, Ready, Request};
use bark_fw_logic::bench::{parse, Command, Power, ResetCause, LINE};
use bark_fw_logic::line::Line;
use bark_proto::radio::{time_on_air, Channel, MIN_PREAMBLE_SYMBOLS, SYMBOL_TIME};
use bark_proto::wire::{BENCH_FIRST, FORMAT, MAX_PAYLOAD};
use bark_proto::{AppEvent, Core, Frame, MsgId, NodeId, Position, Stored, MAX_FRAME};
use embassy_futures::select::{select, Either};
use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::signal::Signal;

use crate::radio::{Radio, MAX_TX_DBM, MIN_TX_DBM};

const _: () = assert!(
    !cfg!(feature = "console"),
    "the bench build is a node build: not with the console"
);

/// What the commands asked of the node and it has not yet done. Every
/// part of it is touched from tasks alone, in thread mode.
struct Asked {
    power: Power,
    text: Option<Text>,
    tx_dbm: Option<i8>,
    carrier: Option<(Channel, u32)>,
}

#[derive(Clone, Copy)]
struct Text {
    dst_pos: Position,
    dst_id: NodeId,
    len: usize,
    payload: [u8; MAX_PAYLOAD],
}

static ASKED: Mutex<ThreadModeRawMutex, RefCell<Asked>> = Mutex::new(RefCell::new(Asked {
    power: Power::Real,
    text: None,
    tx_dbm: None,
    carrier: None,
}));

/// A command changed [`ASKED`].
static WAKE: Signal<ThreadModeRawMutex, ()> = Signal::new();

/// The line being typed.
static TYPED: Mutex<ThreadModeRawMutex, RefCell<Line<LINE>>> =
    Mutex::new(RefCell::new(Line::new()));

/// The driver's transmit power after the last step, for the status.
static TX_DBM: Mutex<ThreadModeRawMutex, Cell<Option<i8>>> = Mutex::new(Cell::new(None));

/// What the chip recorded of the resets before this boot, read once.
static RESET: Mutex<ThreadModeRawMutex, Cell<Option<u32>>> = Mutex::new(Cell::new(None));

/// Takes what the host types, and carries out each command as its line
/// ends.
pub async fn host_input(data: &[u8]) {
    for &byte in data {
        let reboot = TYPED.lock(|t| {
            let mut t = t.borrow_mut();
            if !t.push(byte) {
                return false;
            }
            match t.text() {
                Ok(line) => execute(line),
                Err(e) => {
                    log::warn!("bench: line refused: {:?}", e);
                    false
                }
            }
        });
        if reboot {
            crate::update::reboot().await;
        }
    }
}

/// Carries out `line`, all but a reboot, which it asks for.
fn execute(line: &str) -> bool {
    log::info!("bench> {}", line);
    let command = match parse(line) {
        Ok(c) => c,
        Err(why) => {
            log::warn!("bench: {}", why);
            return false;
        }
    };
    ASKED.lock(|a| {
        let mut a = a.borrow_mut();
        match command {
            Command::Text { to, at, words } => {
                if a.text.is_some() {
                    log::warn!("bench: text refused, the last one waits");
                    return;
                }
                let mut payload = [0; MAX_PAYLOAD];
                payload[..words.len()].copy_from_slice(words.as_bytes());
                a.text = Some(Text {
                    dst_pos: at,
                    dst_id: to,
                    len: words.len(),
                    payload,
                });
            }
            Command::Power(p) => {
                a.power = p;
                log::info!("bench: power {:?}", p);
            }
            Command::TxPower(dbm) => a.tx_dbm = Some(dbm.clamp(MIN_TX_DBM, MAX_TX_DBM)),
            Command::Carrier { channel, seconds } => a.carrier = Some((channel, seconds)),
            Command::Reboot | Command::Status => {}
        }
    });
    match command {
        Command::Reboot => return true,
        Command::Status => status(),
        _ => WAKE.signal(()),
    }
    false
}

/// What the 10 s line does not carry: the reset that started this boot,
/// and what the bench told the node.
fn status() {
    let reset = RESET.lock(|r| {
        let bits = r.get().unwrap_or_else(|| {
            let power = embassy_nrf::pac::POWER;
            let bits = power.resetreas().read().0;
            // Written ones clear, so that the next boot reads its own.
            power
                .resetreas()
                .write_value(embassy_nrf::pac::power::regs::Resetreas(bits));
            bits
        });
        r.set(Some(bits));
        bits
    });
    let power = ASKED.lock(|a| a.borrow().power);
    let tx_dbm = TX_DBM.lock(Cell::get);
    log::info!(
        "bench: status reset={} power={:?} tx_dbm={:?} stack_used={} of {}",
        ResetCause(reset),
        power,
        tx_dbm,
        crate::stack::used(),
        crate::stack::size()
    );
}

/// The node's application under the bench's commands: a text is offered
/// after whatever the application itself has, and the power the bench
/// tells it is the core's.
pub struct App<A> {
    inner: A,
    /// The power last given to the driver.
    reported: Option<bool>,
    /// The last text taken.
    sent: Option<MsgId>,
}

impl<A> From<A> for App<A> {
    fn from(inner: A) -> App<A> {
        log::warn!("bench build: commands on the serial port (--features bench)");
        App {
            inner,
            reported: None,
            sent: None,
        }
    }
}

// The node's loop reads the application it wraps.
impl<A> core::ops::Deref for App<A> {
    type Target = A;
    fn deref(&self) -> &A {
        &self.inner
    }
}

impl<A> core::ops::DerefMut for App<A> {
    fn deref_mut(&mut self) -> &mut A {
        &mut self.inner
    }
}

impl<A> App<A> {
    /// What a command gives the driver or the node's loop now.
    fn asked(&self, offer: bool) -> Option<Ready> {
        ASKED.lock(|a| {
            let a = a.borrow();
            let power = a.power.told().or(Some(crate::power::reported()));
            if power != self.reported || (offer && a.text.is_some()) {
                Some(Ready::Driver)
            } else if a.tx_dbm.is_some() || a.carrier.is_some() {
                Some(Ready::Own)
            } else {
                None
            }
        })
    }
}

impl<A: bark_driver::App> bark_driver::App for App<A> {
    async fn ready(&mut self, offer: bool) -> Ready {
        loop {
            match select(self.inner.ready(offer), WAKE.wait()).await {
                Either::First(ready) => return ready,
                Either::Second(()) => {
                    if let Some(ready) = self.asked(offer) {
                        return ready;
                    }
                }
            }
        }
    }

    fn request(&mut self) -> Option<Request> {
        self.inner.request()
    }

    fn take(&mut self, payload: &mut [u8; MAX_PAYLOAD]) -> Option<Outgoing> {
        self.inner.take(payload).or_else(|| {
            let t = ASKED.lock(|a| a.borrow_mut().text.take())?;
            payload[..t.len].copy_from_slice(&t.payload[..t.len]);
            let id = MsgId::of(t.dst_pos, t.dst_id, &t.payload[..t.len]);
            // As a phone's message is logged (`node.rs`), for
            // `scripts/bench.py texts`.
            log::info!("bench: send id={:08x} len={} taken", id.0, t.len);
            self.sent = Some(id);
            Some(Outgoing {
                dst_pos: t.dst_pos,
                dst_id: t.dst_id,
                len: t.len,
            })
        })
    }

    fn message(&mut self, event: &AppEvent) {
        match event {
            AppEvent::GivenBack { msg_id, retry_at } if self.sent == Some(*msg_id) => {
                let wait = retry_at.since(crate::clock::core_now());
                log::warn!(
                    "bench: text id={:08x} given back retry_in_ms={}",
                    msg_id.0,
                    wait.as_micros() / 1000
                );
            }
            _ => self.inner.message(event),
        }
    }

    fn power(&mut self) -> Option<bool> {
        let real = self.inner.power();
        let told = ASKED.lock(|a| a.borrow().power.told());
        self.reported = told.or(real);
        self.reported
    }

    async fn battery(&mut self) -> Option<u16> {
        self.inner.battery().await
    }

    fn store(&mut self, stored: &Stored) {
        self.inner.store(stored)
    }

    fn gps(&mut self, gps: bark_driver::Gps) {
        self.inner.gps(gps)
    }

    fn offered(&mut self, image: &bark_proto::update::Image) {
        self.inner.offered(image)
    }

    fn store_piece(&mut self, piece: &bark_proto::update::Piece) {
        self.inner.store_piece(piece)
    }

    async fn serve(&mut self, frame: &mut Frame) -> bool {
        self.inner.serve(frame).await
    }
}

/// Between two of the driver's steps: the transmit power asked for, and
/// the carrier.
pub async fn serve<C: Core, K: Clock, A: bark_driver::App>(
    driver: &mut Driver<C, Radio<'_>, K, A>,
) {
    let (tx_dbm, carrier) = ASKED.lock(|a| {
        let mut a = a.borrow_mut();
        (a.tx_dbm.take(), a.carrier.take())
    });
    if let Some(dbm) = tx_dbm {
        driver.set_tx_dbm(dbm);
        log::info!("bench: tx_dbm={}", dbm);
    }
    TX_DBM.lock(|t| t.set(Some(driver.tx_dbm())));
    if let Some((channel, seconds)) = carrier {
        let dbm = driver.tx_dbm();
        send_carrier(driver.radio_mut(), channel, seconds, dbm).await;
    }
}

/// The carrier's frames: two bytes, a bench frame's first (README,
/// Console), which every node rejects, behind the longest preamble that
/// ends in time, so that the channel is busy, to listen-before-talk's CAD
/// and to its reading alike, from the first frame to the last.
const CARRIER_LEN: u8 = 2;

/// Transmit on `channel` for `seconds`, frame after frame, at `dbm`. A
/// bench transmission: it keeps to no duty cycle.
async fn send_carrier(radio: &mut Radio<'_>, channel: Channel, seconds: u32, dbm: i8) {
    let mut frame = Frame {
        channel,
        preamble_symbols: MIN_PREAMBLE_SYMBOLS,
        len: CARRIER_LEN,
        bytes: [0; MAX_FRAME],
    };
    frame.bytes[..2].copy_from_slice(&[BENCH_FIRST, FORMAT]);
    let shortest = time_on_air(CARRIER_LEN, MIN_PREAMBLE_SYMBOLS).as_micros();
    // The part of a frame that is not its preamble.
    let rest = shortest - SYMBOL_TIME.as_micros() * u64::from(MIN_PREAMBLE_SYMBOLS);
    let start = embassy_time::Instant::now();
    let end = start + embassy_time::Duration::from_secs(seconds.into());
    log::info!(
        "bench: carrier ch={} for {} s at {} dBm",
        channel.index(),
        seconds,
        dbm
    );
    let mut frames = 0u32;
    loop {
        let now = embassy_time::Instant::now();
        if now >= end || (end - now).as_micros() < shortest {
            break;
        }
        let left = (end - now).as_micros();
        let symbols = (left - rest) / SYMBOL_TIME.as_micros();
        frame.preamble_symbols = symbols.clamp(MIN_PREAMBLE_SYMBOLS.into(), u16::MAX.into()) as u16;
        if let Err(e) = bark_driver::Radio::transmit(radio, &frame, dbm).await {
            log::warn!("bench: carrier: {}", e);
            break;
        }
        frames += 1;
    }
    log::info!(
        "bench: carrier ended frames={} ms={}",
        frames,
        start.elapsed().as_millis()
    );
}
