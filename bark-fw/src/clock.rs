//! Microsecond stopwatch for radio measurements.
//!
//! The embassy clock runs on RTC1 at 32 768 Hz: one tick is 30.5 µs, a third
//! of the retune time the console measures, and its source is the internal
//! RC oscillator. TIMER1 at 1 MHz counts the 32 MHz crystal USB already needs,
//! so readings resolve 1 µs and keep running while the CPU sleeps in `wfe`
//! (a CPU cycle counter would stop).

use embassy_nrf::peripherals::TIMER1;
use embassy_nrf::timer::{Frequency, Timer};
use embassy_nrf::Peri;

/// A free-running 32-bit microsecond counter; it wraps after 71 minutes, so
/// intervals are taken with wrapping subtraction and must be shorter.
pub struct Clock {
    timer: Timer<'static>,
}

impl Clock {
    /// Start counting from zero.
    pub fn new(timer: Peri<'static, TIMER1>) -> Clock {
        let timer = Timer::new(timer);
        timer.set_frequency(Frequency::F1MHz);
        timer.start();
        Clock { timer }
    }

    /// The counter now, in µs.
    pub fn now(&self) -> u32 {
        self.timer.cc(0).capture()
    }

    /// µs since `start`, a value of [`Clock::now`].
    pub fn since(&self, start: u32) -> u32 {
        self.now().wrapping_sub(start)
    }
}
