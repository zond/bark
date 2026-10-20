//! Microsecond stopwatch for radio measurements.
//!
//! The embassy clock runs on RTC1 at 32 768 Hz: one tick is 30.5 µs, a third
//! of the retune time the console measures, and its source is the internal
//! RC oscillator. The console's stopwatch is TIMER1 at 1 MHz, counting the
//! 32 MHz crystal USB already needs, so readings resolve 1 µs and keep
//! running while the CPU sleeps in `wfe` (a CPU cycle counter would stop).
//!
//! A node needs no such resolution, and a running TIMER keeps the
//! high-frequency clock requested while the CPU sleeps (nRF52840 Product
//! Specification, TIMER and CLOCK), so a node's stopwatch is the embassy
//! clock.

/// A free-running 32-bit microsecond counter; it wraps after 71 minutes, so
/// intervals are taken with wrapping subtraction and must be shorter.
pub struct Clock {
    #[cfg(feature = "console")]
    timer: embassy_nrf::timer::Timer<'static>,
}

impl Clock {
    /// Start counting from zero.
    #[cfg(feature = "console")]
    pub fn new(timer: embassy_nrf::Peri<'static, embassy_nrf::peripherals::TIMER1>) -> Clock {
        use embassy_nrf::timer::{Frequency, Timer};
        let timer = Timer::new(timer);
        timer.set_frequency(Frequency::F1MHz);
        timer.start();
        Clock { timer }
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
}
