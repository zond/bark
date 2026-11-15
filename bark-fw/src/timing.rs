//! The bench build's account of the work beside the radio (README, "Work
//! beside the radio", "Bench build"): each task's polls, timed less the
//! interrupts that came during them, with the place in the task that
//! took longest; the interrupts' own spans; and both over each
//! listen-before-talk, for the line of a check that got no reading.
//!
//! Times are the processor's cycles, the DWT's counter (Cortex-M4
//! Technical Reference Manual, DWT_CYCCNT), which wraps after 67 s: only
//! differences over less are taken. The interrupts bound in `main.rs`
//! are timed; GPIOTE's and the clock's RTC1, which embassy-nrf binds
//! itself, count as the time of the task they come in.

use core::cell::Cell;
use core::future::Future;
use core::sync::atomic::{AtomicU32, Ordering::Relaxed};

use cortex_m::interrupt::InterruptNumber;
use cortex_m::peripheral::DWT;
use embassy_nrf::interrupt::typelevel::{Handler, Interrupt};
use embassy_nrf::pac::Interrupt as Irq;
use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
use embassy_sync::blocking_mutex::Mutex;

use crate::radio::CPU_HZ;

const CYCLES_PER_US: u32 = (CPU_HZ / 1_000_000) as u32;

/// Count the processor's cycles; once at boot, before any task.
pub fn start() {
    // SAFETY: the DCB and the DWT are touched here alone.
    let mut p = unsafe { cortex_m::Peripherals::steal() };
    p.DCB.enable_trace();
    p.DWT.enable_cycle_counter();
}

fn now() -> u32 {
    DWT::cycle_count()
}

fn us(cycles: u32) -> u32 {
    cycles / CYCLES_PER_US
}

// The interrupts.

/// Interrupts entered and not yet left: one that comes inside another is
/// counted in the other's span.
static DEPTH: AtomicU32 = AtomicU32::new(0);
/// When the outermost interrupt began.
static ENTERED: AtomicU32 = AtomicU32::new(0);
/// Cycles in interrupts since boot, wrapping.
static IN_INTERRUPTS: AtomicU32 = AtomicU32::new(0);

/// The bits of a longest span that hold the interrupt's number: the
/// nRF52840's are below 48 (nRF52840 Product Specification,
/// Instantiation).
const NUMBER_BITS: u32 = 6;

/// The longest span since the minute's line, and since the check began:
/// its cycles above [`NUMBER_BITS`], its interrupt's number below, so
/// that one `fetch_max` keeps both.
static LONGEST_MINUTE: AtomicU32 = AtomicU32::new(0);
static LONGEST_CHECK: AtomicU32 = AtomicU32::new(0);

/// The first handler of a timed interrupt (`main.rs`, `Irqs`).
pub struct Enter;

/// The last handler of a timed interrupt.
pub struct Leave;

impl<I: Interrupt> Handler<I> for Enter {
    unsafe fn on_interrupt() {
        let t = now();
        if DEPTH.fetch_add(1, Relaxed) == 0 {
            ENTERED.store(t, Relaxed);
        }
    }
}

impl<I: Interrupt> Handler<I> for Leave {
    unsafe fn on_interrupt() {
        // Read before the depth falls: once it is 0 an interrupt that
        // comes in sets its own start, and a span taken from that one
        // would wrap to the largest (`USBD:1048575`, 2026-11-14).
        let entered = ENTERED.load(Relaxed);
        if DEPTH.fetch_sub(1, Relaxed) != 1 {
            return;
        }
        let took = now().wrapping_sub(entered);
        IN_INTERRUPTS.fetch_add(took, Relaxed);
        let packed =
            (took.min(u32::MAX >> NUMBER_BITS) << NUMBER_BITS) | u32::from(I::IRQ.number());
        LONGEST_MINUTE.fetch_max(packed, Relaxed);
        LONGEST_CHECK.fetch_max(packed, Relaxed);
    }
}

/// The interrupts `main.rs` times, for their names.
const TIMED_IRQS: [Irq; 10] = [
    Irq::CLOCK_POWER,
    Irq::RADIO,
    Irq::TWISPI1,
    Irq::SAADC,
    Irq::TIMER0,
    Irq::RTC0,
    Irq::EGU0_SWI0,
    Irq::USBD,
    Irq::UARTE1,
    Irq::SPI2,
];

/// A longest span as kept: `<interrupt>:<µs>`, `-` for none.
struct Span(u32);

impl core::fmt::Display for Span {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.0 == 0 {
            return f.write_str("-");
        }
        let number = (self.0 & ((1 << NUMBER_BITS) - 1)) as u16;
        match TIMED_IRQS.iter().find(|i| i.number() == number) {
            Some(i) => write!(f, "{i:?}")?,
            None => write!(f, "{number}")?,
        }
        write!(f, ":{}", us(self.0 >> NUMBER_BITS))
    }
}

// The tasks.

/// A task's longest poll since the minute's line, less the interrupts in
/// it, in cycles, and the place in it that took longest ([`at`]).
pub struct Longest {
    name: &'static str,
    longest: Mutex<ThreadModeRawMutex, Cell<(u32, &'static str)>>,
}

impl Longest {
    const fn new(name: &'static str) -> Longest {
        Longest {
            name,
            longest: Mutex::new(Cell::new((0, "-"))),
        }
    }
}

pub static UPDATE: Longest = Longest::new("update");
/// The phone link's host; each link's slot is [`LINK`].
pub static PHONE: Longest = Longest::new("phone");
pub static LINK: Longest = Longest::new("link");
pub static GPS: Longest = Longest::new("gps");
pub static SCREEN: Longest = Longest::new("screen");
pub static STORE: Longest = Longest::new("store");
pub static MAILBOX: Longest = Longest::new("mailbox");
pub static LOG: Longest = Longest::new("log");
pub static USB: Longest = Longest::new("usb");
pub static MPSL: Longest = Longest::new("mpsl");
pub static TRIAL: Longest = Longest::new("trial");
pub static LED: Longest = Longest::new("led");
pub static BUTTON: Longest = Longest::new("button");
pub static TOUCH: Longest = Longest::new("touch");
pub static WATCHDOG: Longest = Longest::new("watchdog");

static TASKS: [&Longest; 15] = [
    &UPDATE, &PHONE, &LINK, &GPS, &SCREEN, &STORE, &MAILBOX, &LOG, &USB, &MPSL, &TRIAL, &LED,
    &BUTTON, &TOUCH, &WATCHDOG,
];

/// The poll under way: when it began, the place it is at and since
/// when, and the longest place so far, less the interrupts.
#[derive(Clone, Copy)]
struct Segment {
    began: (u32, u32),
    place: &'static str,
    since: (u32, u32),
    longest: (u32, &'static str),
}

impl Segment {
    fn new() -> Segment {
        let t = (now(), IN_INTERRUPTS.load(Relaxed));
        Segment {
            began: t,
            place: "-",
            since: t,
            longest: (0, "-"),
        }
    }

    /// The place so far ends now.
    fn close(&mut self) {
        let own = own_since(self.since);
        if own > self.longest.0 {
            self.longest = (own, self.place);
        }
        self.since = (now(), IN_INTERRUPTS.load(Relaxed));
    }
}

static SEGMENT: Mutex<ThreadModeRawMutex, Cell<Option<Segment>>> = Mutex::new(Cell::new(None));

/// Cycles since a cycle count and an interrupts' count, less those in
/// interrupts since.
fn own_since((cycles, interrupts): (u32, u32)) -> u32 {
    let took = now().wrapping_sub(cycles);
    took.saturating_sub(IN_INTERRUPTS.load(Relaxed).wrapping_sub(interrupts))
}

/// Where the task being polled is, `-` outside a timed poll.
pub fn place() -> &'static str {
    SEGMENT.lock(|s| s.get().map_or("-", |g| g.place))
}

/// The task being polled is at `place` from now on.
pub fn at(place: &'static str) {
    SEGMENT.lock(|s| {
        if let Some(mut g) = s.get() {
            g.close();
            g.place = place;
            s.set(Some(g));
        }
    });
}

/// What ran beside the radio's task over a listen-before-talk.
#[derive(Clone, Copy)]
struct Window {
    /// The tasks' polls, less the interrupts.
    tasks: u32,
    /// The longest of them: its task, place and cycles.
    longest: (&'static str, &'static str, u32),
    /// The interrupts' count as it began.
    interrupts: u32,
}

static WINDOW: Mutex<ThreadModeRawMutex, Cell<Window>> = Mutex::new(Cell::new(Window {
    tasks: 0,
    longest: ("-", "-", 0),
    interrupts: 0,
}));

/// `run`, each poll timed as `task`'s.
pub fn timed<F: Future>(task: &'static Longest, run: F) -> Timed<F> {
    Timed { task, run }
}

/// A future whose polls are timed ([`timed`]). A struct, not an `async
/// fn`, so that the task's storage holds the future once.
pub struct Timed<F> {
    task: &'static Longest,
    run: F,
}

impl<F: Future> Future for Timed<F> {
    type Output = F::Output;

    fn poll(
        self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<F::Output> {
        // SAFETY: `run` is never moved out of the pinned `Timed`.
        let this = unsafe { self.get_unchecked_mut() };
        let run = unsafe { core::pin::Pin::new_unchecked(&mut this.run) };
        let task = this.task;
        SEGMENT.lock(|s| s.set(Some(Segment::new())));
        let polled = run.poll(cx);
        let (own, place) = SEGMENT.lock(|s| {
            let mut g = s.take().expect("set as the poll began");
            g.close();
            (own_since(g.began), g.longest.1)
        });
        task.longest.lock(|l| {
            if own > l.get().0 {
                l.set((own, place));
            }
        });
        WINDOW.lock(|w| {
            let mut v = w.get();
            v.tasks = v.tasks.wrapping_add(own);
            if own > v.longest.2 {
                v.longest = (task.name, place, own);
            }
            w.set(v);
        });
        polled
    }
}

/// A listen-before-talk begins: what runs beside it is counted from now.
pub fn check_begins() {
    let interrupts = IN_INTERRUPTS.load(Relaxed);
    WINDOW.lock(|w| {
        w.set(Window {
            tasks: 0,
            longest: ("-", "-", 0),
            interrupts,
        })
    });
    LONGEST_CHECK.store(0, Relaxed);
}

/// What ran beside a listen-before-talk since [`check_begins`], for its
/// line: `tasks_us=<n> task=<task>:<place>:<µs> irq_us=<n>
/// irq=<interrupt>:<µs>`.
#[derive(Clone, Copy)]
pub struct Beside {
    window: Window,
    interrupts: u32,
    longest_irq: u32,
}

pub fn check_ends() -> Beside {
    let window = WINDOW.lock(Cell::get);
    Beside {
        window,
        interrupts: IN_INTERRUPTS.load(Relaxed).wrapping_sub(window.interrupts),
        longest_irq: LONGEST_CHECK.load(Relaxed),
    }
}

impl core::fmt::Display for Beside {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let w = &self.window;
        write!(f, "tasks_us={} task=", us(w.tasks))?;
        match w.longest {
            (_, _, 0) => f.write_str("-")?,
            (task, place, c) => write!(f, "{task}:{place}:{}", us(c))?,
        }
        write!(
            f,
            " irq_us={} irq={}",
            us(self.interrupts),
            Span(self.longest_irq)
        )
    }
}

/// When the minute's line counted the interrupts from.
static MINUTE_FROM: AtomicU32 = AtomicU32::new(0);

/// Once a minute, with the busy estimates: each task's longest poll since
/// the last, less the interrupts in it, and where; the interrupts' time
/// and their longest span.
pub fn log_minute() {
    for task in TASKS {
        let (longest, at) = task.longest.lock(|l| l.replace((0, "-")));
        log::info!(
            "bench: longest poll task={} us={} at={}",
            task.name,
            us(longest),
            at
        );
    }
    let total = IN_INTERRUPTS.load(Relaxed);
    let from = MINUTE_FROM.swap(total, Relaxed);
    log::info!(
        "bench: interrupts us={} longest={}",
        us(total.wrapping_sub(from)),
        Span(LONGEST_MINUTE.swap(0, Relaxed))
    );
}
