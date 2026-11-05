//! The board's GNSS receiver, a Quectel L76K (`board.rs`, Gps), as its
//! core asks: in standby, searching for one fix, or running all the time
//! (`bark_driver::Gps`, spec "Position", "Beats"; roadmap D80, D82).
//!
//! The receiver sits on the switched rail with the radio and is powered
//! whenever the radio is (README, Radio power). Its WAKEUP line, P1.02,
//! is driven high to run it and low to let it rest in standby, 20 µA with
//! its RAM and clock kept for a hot start (L76K Hardware Design V1.0,
//! Table 2, §3.3.3). Its reset line is never driven. It sends NMEA 0183
//! at 9600 bps, 8N1, its default (`docs/gps-time-2026-11-05.md`), which
//! UARTE1 receives only while it runs: at rest the UART is off.
//!
//! The task is its own and touches nothing of the radio's: it parses the
//! sentences (`bark_driver::gnss`), keeps the time (`utc.rs`), and hands
//! each fix whose satellites pass the C/N0 check to the node, which hands
//! it to the core if it is in reach of a known relay. Its UART's
//! interrupt runs at the priority of the node's others, below the MPSL's
//! (`main.rs`).

use core::cell::Cell;

use bark_driver::gnss::{Fix, Nmea};
use bark_driver::Gps;
use bark_proto::Position;
use embassy_futures::select::{select3, Either3};
use embassy_nrf::gpio::{Level, Output, OutputDrive};
use embassy_nrf::{peripherals, uarte, Peri};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::signal::Signal;

/// The receiver's rate (module documentation).
const BAUD: uarte::Baudrate = uarte::Baudrate::Baud9600;

/// Bytes read at once: a second's sentences come together and end in a
/// pause, at which a read returns. Two lines' worth keeps the reads few
/// without holding a whole second.
const READ: usize = 256;

/// What the node last asked of the GPS.
static WANT: Signal<CriticalSectionRawMutex, Gps> = Signal::new();
/// The last fix that passed the receiver's checks, until the node takes it.
static FIX: Mutex<CriticalSectionRawMutex, Cell<Option<Position>>> = Mutex::new(Cell::new(None));
/// Raised when a fix waits for the node.
pub static FOR_CORE: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// The node's side: run the GPS as `gps` says.
pub fn want(gps: Gps) {
    WANT.signal(gps);
}

/// The node's side: the fix waiting, if any.
pub fn take_fix() -> Option<Position> {
    FIX.lock(Cell::take)
}

/// What the task takes.
pub struct Parts {
    pub uarte: Peri<'static, peripherals::UARTE1>,
    pub timer: Peri<'static, peripherals::TIMER2>,
    pub ppi0: Peri<'static, peripherals::PPI_CH0>,
    pub ppi1: Peri<'static, peripherals::PPI_CH1>,
    pub pins: crate::board::Gps,
}

/// Where the receiver stands, for the log at each change.
#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Searching,
    Fixed,
    /// A fix whose satellites' C/N0 is not a real sky's.
    Dropped,
}

/// The end of a search, on the embassy clock, which the core's instants
/// count from (`clock.rs`).
fn end(gps: Gps) -> Option<embassy_time::Instant> {
    match gps {
        Gps::Search(t) => Some(embassy_time::Instant::from_micros(t.as_micros())),
        Gps::Rest | Gps::Track => None,
    }
}

fn runs(gps: Gps) -> bool {
    match gps {
        Gps::Rest => false,
        Gps::Track => true,
        Gps::Search(_) => end(gps).is_some_and(|t| t > embassy_time::Instant::now()),
    }
}

#[embassy_executor::task]
pub async fn run(mut p: Parts) -> ! {
    let mut wake = Output::new(p.pins.standby, Level::Low, OutputDrive::Standard);
    let mut config = uarte::Config::default();
    config.baudrate = BAUD;
    let mut want = Gps::Rest;
    let mut nmea = Nmea::default();
    let mut buf = [0u8; READ];
    loop {
        while !runs(want) {
            want = WANT.wait().await;
        }
        wake.set_high();
        log::info!("gps: searching");
        note("searching");
        let mut state = State::Searching;
        let mut rx = uarte::UarteRx::new(
            p.uarte.reborrow(),
            crate::Irqs,
            p.pins.rx.reborrow(),
            config.clone(),
        )
        .with_idle(p.timer.reborrow(), p.ppi0.reborrow(), p.ppi1.reborrow());
        while runs(want) {
            let until = async {
                match end(want) {
                    Some(t) => embassy_time::Timer::at(t).await,
                    None => core::future::pending().await,
                }
            };
            match select3(rx.read_until_idle(&mut buf), WANT.wait(), until).await {
                Either3::First(Ok(n)) => {
                    let mut first = None;
                    nmea.feed(&buf[..n], &mut |f| {
                        let was = state;
                        state = fixed(f, state);
                        if was != State::Fixed && state == State::Fixed {
                            first = Some(f.cn0);
                        }
                    });
                    // A first fix whose used satellites have no C/N0
                    // found: the sentences, once, for the bench.
                    if let Some(c) = first.filter(|c| c.count == 0 && c.listed > 0) {
                        let (gsa, gsv) = nmea.last_gsa_gsv();
                        log::warn!(
                            "gps: no c/n0 for {} used of {} in view; last {} and {}",
                            c.listed,
                            c.in_view,
                            core::str::from_utf8(gsa).unwrap_or("?"),
                            core::str::from_utf8(gsv).unwrap_or("?")
                        );
                    }
                }
                // A framing or overrun error loses a sentence, which the
                // checksum would drop anyway.
                Either3::First(Err(_)) => nmea.bad += 1,
                Either3::Second(w) => want = w,
                Either3::Third(()) => want = Gps::Rest,
            }
        }
        drop(rx);
        wake.set_low();
        let found = if state == State::Fixed {
            ""
        } else {
            " without a fix"
        };
        log::info!("gps: rest{} bad_sentences={}", found, nmea.bad);
        note("rest");
    }
}

/// The receiver's state for the `boot:` line, whose lines a host that
/// opens the port late still reads (`boot.rs`).
fn note(state: &'static str) {
    crate::boot::note(|f| f.gps = state);
}

/// Take a second's [`Fix`]: keep its time, hand its position to the node
/// if its satellites look like a real sky's, and log a change of state.
fn fixed(f: Fix, state: State) -> State {
    let Some(position) = f.position else {
        if state != State::Searching {
            log::info!("gps: no fix, searching");
            note("searching");
        }
        return State::Searching;
    };
    if let Some(s) = f.utc_s {
        crate::utc::gps(s);
    }
    let c = f.cn0;
    if !c.real() {
        if state != State::Dropped {
            log::warn!(
                "gps: fix dropped, c/n0 {}..{} dB-Hz outside the band",
                c.min,
                c.max
            );
        }
        return State::Dropped;
    }
    if state != State::Fixed {
        log::info!(
            "gps: fix at {} sats={} c/n0 n={} min={} mean={} max={} dB-Hz",
            position,
            f.satellites,
            c.count,
            c.min,
            c.mean,
            c.max
        );
    }
    if state != State::Fixed {
        note("fix");
    }
    FIX.lock(|x| x.set(Some(position)));
    FOR_CORE.signal(());
    State::Fixed
}
