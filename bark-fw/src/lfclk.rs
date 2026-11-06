//! The 32.768 kHz clock's source, found at boot. The BLE controller keeps
//! a connection's timing on it, and the core's timing assumes a watch
//! crystal (spec "Heartbeat"). Whether the board has the crystal fitted
//! is not known here, so the firmware tries it and falls back to the
//! internal RC oscillator.
//!
//! The crystal oscillator either starts within its start-up time or never
//! raises LFCLKSTARTED, and nothing else on the chip tells the two apart
//! (nRF52840 Product Specification, CLOCK, LFCLK). The probe runs before
//! the MPSL, which then keeps the clock on the source found.

use embassy_nrf::pac::clock::vals::Lfclksrc;
use embassy_nrf::pac::CLOCK;

/// Where the 32.768 kHz clock comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The crystal, started this many ms after it was asked.
    Crystal { started_ms: u32 },
    /// The internal RC oscillator: the crystal did not start.
    Rc,
}

/// How long the crystal is given to start: four times its typical start-up
/// time of 0.25 s (Product Specification, 32.768 kHz crystal oscillator,
/// tSTART,LFXO), so that a slow crystal is not taken for a missing one.
const START_MS: u32 = 1000;

/// CPU cycles in a millisecond at 64 MHz, the busy wait's step: the clock
/// that times everything else is the one being changed.
const CYCLES_PER_MS: u32 = 64_000;

/// Try the crystal, falling back to the RC oscillator; the LFCLK runs from
/// the source returned. Called once, after `embassy_nrf::init`, which
/// started it on the RC oscillator, and before the MPSL starts. The
/// embassy clock, on RTC1, stands still meanwhile, up to [`START_MS`].
pub fn probe() -> Source {
    let started = || CLOCK.events_lfclkstarted().read() != 0;
    let start = |src: Lfclksrc| {
        CLOCK.tasks_lfclkstop().write_value(1);
        while CLOCK.lfclkstat().read().state() {}
        CLOCK.lfclksrc().write(|w| w.set_src(src));
        CLOCK.events_lfclkstarted().write_value(0);
        CLOCK.tasks_lfclkstart().write_value(1);
    };
    start(Lfclksrc::Xtal);
    for ms in 0..START_MS {
        if started() {
            return Source::Crystal { started_ms: ms };
        }
        cortex_m::asm::delay(CYCLES_PER_MS);
    }
    start(Lfclksrc::Rc);
    while !started() {}
    Source::Rc
}
