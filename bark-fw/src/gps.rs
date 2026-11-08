//! The board's GNSS receiver, a Quectel L76K (`board.rs`, Gps), as its
//! core asks: in standby, searching for one fix, or running all the time
//! (`bark_driver::Gps`, spec "Position", "Beats"; roadmap D80, D82).
//!
//! The receiver sits on the switched rail with the radio and is powered
//! whenever the radio is (README, Radio power). Its WAKEUP line, P1.02,
//! is driven high to run it and low to let it rest in standby, 20 µA with
//! its RAM and clock kept for a hot start (L76K Hardware Design V1.0,
//! Table 2, §3.3.3). Its reset line is never driven. It talks NMEA 0183
//! at 9600 bps, 8N1, its default (`docs/gps-time-2026-11-05.md`), over
//! UARTE1, which is on only while it runs: at rest the UART is off and
//! drives nothing into it. At each start it is told which sentences to
//! send (`bark_driver::gnss::OUTPUT`).
//!
//! The task is its own and touches nothing of the radio's: it parses the
//! sentences (`bark_driver::gnss`), keeps the time (`utc.rs`), and hands
//! each fix whose satellites pass the C/N0 check to the node, which hands
//! it to the core if it is in reach of a known relay. Its UART's
//! interrupt runs at the priority of the node's others, below the MPSL's
//! (`main.rs`).

use core::cell::Cell;

use bark_driver::gnss::{Fix, Nmea, OUTPUT};
use bark_driver::Gps;
use bark_proto::Position;
use embassy_futures::select::{select3, Either3};
use embassy_nrf::gpio::{Level, Output, OutputDrive};
use embassy_nrf::{peripherals, uarte, Peri};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::signal::Signal;

/// Raw GSA and GSV sentences kept for the bench's line on a fix without
/// C/N0: none in the node image, whose stack wants the 736 bytes.
const SENTENCES_KEPT: usize = if cfg!(feature = "console") {
    bark_driver::gnss::RAW
} else {
    0
};

/// The receiver's rate (module documentation).
const BAUD: uarte::Baudrate = uarte::Baudrate::Baud9600;

/// Bytes read at once: a second's sentences come together and end in a
/// pause, at which a read returns. A whole second's, about 760 B of
/// [`OUTPUT`]'s sentences with 28 satellites in view and 960 B at most at
/// 9600 bps, so that a read ends at the pause and not within the burst,
/// where the UART's FIFO of a few bytes would overrun if the task were
/// late to start the next.
const READ: usize = 1024;

/// What the node last asked of the GPS.
static WANT: Signal<CriticalSectionRawMutex, Gps> = Signal::new();
/// The last fix that passed the receiver's checks, until the node takes it.
static FIX: Mutex<CriticalSectionRawMutex, Cell<Option<Position>>> = Mutex::new(Cell::new(None));
/// The receiver lost its fix since the node last took one, until the
/// node takes this.
static LOST: Mutex<CriticalSectionRawMutex, Cell<bool>> = Mutex::new(Cell::new(false));
/// Raised when a fix, or its loss, waits for the node.
pub static FOR_CORE: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// The node's side: run the GPS as `gps` says.
pub fn want(gps: Gps) {
    WANT.signal(gps);
}

/// The node's side: the fix waiting, if any.
pub fn take_fix() -> Option<Position> {
    FIX.lock(Cell::take)
}

/// The node's side: whether the receiver lost its fix since it last
/// asked. It hands this before the fix waiting, which came after.
pub fn take_lost() -> bool {
    LOST.lock(|l| l.replace(false))
}

/// The node's side: whether anything [`FOR_CORE`] was raised for waits,
/// so that a signal whose fix was taken wakes no one.
pub fn waiting() -> bool {
    FIX.lock(|x| x.get().is_some()) || LOST.lock(Cell::get)
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
    let mut nmea = Nmea::<SENTENCES_KEPT>::default();
    let mut buf = [0u8; READ];
    loop {
        while !runs(want) {
            want = WANT.wait().await;
        }
        wake.set_high();
        log::info!("gps: searching");
        note("searching");
        let mut state = State::Searching;
        let (mut tx, mut rx) = uarte::Uarte::new(
            p.uarte.reborrow(),
            p.pins.rx.reborrow(),
            p.pins.tx.reborrow(),
            crate::Irqs,
            config.clone(),
        )
        .split_with_idle(p.timer.reborrow(), p.ppi0.reborrow(), p.ppi1.reborrow());
        let mut told = false;
        while runs(want) {
            let until = async {
                match end(want) {
                    Some(t) => embassy_time::Timer::at(t).await,
                    None => core::future::pending().await,
                }
            };
            match select3(rx.read_until_idle(&mut buf), WANT.wait(), until).await {
                Either3::First(Ok(n)) => {
                    // Told once it is heard, so that it is up, and in the
                    // pause after a second's sentences, so that nothing
                    // arrives while the UART sends.
                    if !told && n < buf.len() {
                        let mut out = [0u8; OUTPUT.len()];
                        out.copy_from_slice(OUTPUT);
                        told = tx.write_from_ram(&out).await.is_ok();
                    }
                    let mut first = None;
                    nmea.feed(&buf[..n], &mut |f| {
                        let was = state;
                        state = fixed(f, state);
                        if was != State::Fixed && state == State::Fixed {
                            first = Some(f);
                        }
                    });
                    // A first fix with satellites but none of their C/N0
                    // found: the sentences, once a search, for the bench.
                    if let Some(f) = first.filter(|f| f.satellites > 0 && f.cn0.count == 0) {
                        log::warn!(
                            "gps: no c/n0 for {} sats, {} listed, {} in view, bad_sentences={}, told={}; sentences{}; last{}",
                            f.satellites,
                            f.cn0.listed,
                            f.cn0.in_view,
                            nmea.bad,
                            told,
                            Lines(nmea.addresses()),
                            Lines(nmea.last_gsa_gsv())
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
        drop((tx, rx));
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

/// A fix is lost: the core counts no time without one towards a run of
/// far fixes (spec "Position").
fn lose(state: State) {
    if state == State::Fixed {
        LOST.lock(|l| l.set(true));
        FOR_CORE.signal(());
    }
}

/// A value the receiver may leave out, `-` if it did, in tenths if the
/// flag says so.
struct Given(Option<u16>, bool);

impl core::fmt::Display for Given {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Given(None, _) => f.write_str("-"),
            Given(Some(v), false) => write!(f, "{v}"),
            Given(Some(v), true) => write!(f, "{}.{}", v / 10, v % 10),
        }
    }
}

/// Lines of text, each after a space.
struct Lines<I>(I);

impl<'a, I: Iterator<Item = &'a [u8]> + Clone> core::fmt::Display for Lines<I> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for line in self.0.clone() {
            write!(f, " {}", core::str::from_utf8(line).unwrap_or("?"))?;
        }
        Ok(())
    }
}

/// The receiver's state for the `boot:` line, whose lines a host that
/// opens the port late still reads (`boot.rs`).
fn note(state: &'static str) {
    crate::boot::note(|f| f.gps = state);
}

/// Take a second's [`Fix`]: keep its time, hand its position to the node
/// if its satellites look like a real sky's, and log it and a change of
/// state.
fn fixed(f: Fix, state: State) -> State {
    let Some(position) = f.position else {
        if state != State::Searching {
            log::info!("gps: no fix, searching");
            note("searching");
        }
        lose(state);
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
        lose(state);
        return State::Dropped;
    }
    // Every fix, for the bench: how a still board's fixes wander, with
    // what quality, and what speed its receiver measures meanwhile,
    // decides the carried rule (spec "Open items", O16, O18;
    // `docs/gps-bench-2026-11-06.md`).
    log::info!(
        "gps: fix at {} sats={} hdop={}.{} mode={} speed={}cm/s course={} c/n0 n={} min={} mean={} max={} dB-Hz",
        position,
        f.satellites,
        f.hdop_tenths / 10,
        f.hdop_tenths % 10,
        if f.mode.is_ascii_graphic() {
            f.mode as char
        } else {
            '-'
        },
        Given(f.speed_cm_s, false),
        Given(f.course_tenths, true),
        c.count,
        c.min,
        c.mean,
        c.max
    );
    if state != State::Fixed {
        note("fix");
    }
    FIX.lock(|x| x.set(Some(position)));
    FOR_CORE.signal(());
    State::Fixed
}
