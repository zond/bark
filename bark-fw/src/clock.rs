//! The clock the radio's sequencer and the driver loop read, and the
//! console's microsecond stopwatch.
//!
//! The embassy clock runs on RTC1 at 32 768 Hz: one tick is 30.5 µs, a
//! tenth of a lean retune, and its source is the internal RC oscillator.
//! The console's stopwatch is TIMER1 at 1 MHz, counting the 32 MHz crystal
//! USB already needs, so readings resolve 1 µs and keep running while the
//! CPU sleeps in `wfe` (a CPU cycle counter would stop); the console build
//! reads the sequencer's phases on it.
//!
//! A node needs no such resolution, and a running TIMER keeps the
//! high-frequency clock requested while the CPU sleeps (nRF52840 Product
//! Specification, TIMER and CLOCK), so a node's clock is the embassy
//! clock.

use bark_proto::Instant;
use embassy_time::{Timer, TICK_HZ};

/// The clock, in the core's microseconds.
pub struct Clock {
    #[cfg(feature = "console")]
    timer: embassy_nrf::timer::Timer<'static>,
    /// The 32-bit counter extended to 64 bits: the last reading and the
    /// wraps seen. It wraps after 71 minutes, so it is read at least that
    /// often, which the console's loop does.
    #[cfg(feature = "console")]
    extended: core::cell::Cell<(u32, u64)>,
}

impl Clock {
    /// Start counting from zero.
    #[cfg(feature = "console")]
    pub fn new(timer: embassy_nrf::Peri<'static, embassy_nrf::peripherals::TIMER1>) -> Clock {
        use embassy_nrf::timer::{Frequency, Timer};
        let timer = Timer::new(timer);
        timer.set_frequency(Frequency::F1MHz);
        timer.start();
        Clock {
            timer,
            extended: core::cell::Cell::new((0, 0)),
        }
    }

    /// The embassy clock, to 30.5 µs.
    #[cfg(not(feature = "console"))]
    pub fn new() -> Clock {
        Clock {}
    }

    /// The counter now, in µs.
    #[cfg(feature = "console")]
    pub fn now(&self) -> u32 {
        self.timer.cc(0).capture()
    }

    /// The counter now, in µs.
    #[cfg(not(feature = "console"))]
    pub fn now(&self) -> u32 {
        // Truncated to the counter's width, which wraps as TIMER1's does.
        embassy_time::Instant::now().as_micros() as u32
    }

    /// µs since `start`, a value of [`Clock::now`].
    pub fn since(&self, start: u32) -> u32 {
        self.now().wrapping_sub(start)
    }

    /// Now, in µs, without a wrap.
    #[cfg(feature = "console")]
    fn now_us(&self) -> u64 {
        let (last, high) = self.extended.get();
        let now = self.now();
        let high = if now < last { high + (1 << 32) } else { high };
        self.extended.set((now, high));
        high | u64::from(now)
    }

    /// Now, in µs, without a wrap.
    #[cfg(not(feature = "console"))]
    fn now_us(&self) -> u64 {
        embassy_time::Instant::now().as_micros()
    }
}

impl bark_driver::Clock for &Clock {
    fn now(&self) -> Instant {
        Instant::from_micros(self.now_us())
    }

    async fn at(&self, t: Instant) {
        // Round up to the next tick: a tick rounded down would fire before
        // `t`, and the core must never be woken early (core-interface §2).
        // The stopwatch, where there is one, runs ahead of or behind the
        // embassy clock by its own drift, so the wait is checked against it
        // and taken again if it ended early.
        loop {
            let now = self.now_us();
            if now >= t.as_micros() {
                return;
            }
            let wait = t.as_micros() - now;
            let ticks = (u128::from(wait) * u128::from(TICK_HZ)).div_ceil(1_000_000);
            Timer::after_ticks(ticks as u64).await;
        }
    }
}
