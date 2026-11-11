//! The 32.768 kHz clock (LFCLK), started at boot by the loader and by the
//! firmware on the crystal if it starts, else the internal RC oscillator:
//! whether a board has the crystal fitted is not known.
//!
//! A running watchdog holds the clock until a reset other than a soft one:
//! LFCLKSTOP then leaves it running, and a watchdog started with no clock
//! forces the RC oscillator on (nRF52840 Product Specification, WDT, CLOCK,
//! POWER). So the loader starts the clock before the watchdog, and nothing
//! here stops a held clock. A crystal that does not start never raises
//! LFCLKSTARTED, and nothing else tells a slow one from a missing one
//! (Product Specification, CLOCK, LFCLK).

use embedded_hal::delay::DelayNs;
use nrf_pac::clock::vals::Lfclksrc;
use nrf_pac::{CLOCK, WDT};

/// A source of the 32.768 kHz clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The crystal oscillator.
    Crystal,
    /// The internal RC oscillator.
    Rc,
}

/// What [`probe`] found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Found {
    /// The crystal.
    Crystal {
        /// Milliseconds from the start task to LFCLKSTARTED.
        started_ms: u32,
    },
    /// The RC oscillator: the crystal did not start.
    Rc,
    /// Already running on this source, held by the watchdog.
    Held(Source),
}

impl Found {
    /// The source the clock runs from.
    pub fn source(self) -> Source {
        match self {
            Found::Crystal { .. } => Source::Crystal,
            Found::Rc => Source::Rc,
            Found::Held(s) => s,
        }
    }
}

/// How long the crystal is given to start: four times its typical 0.25 s
/// (Product Specification, 32.768 kHz crystal oscillator, tSTART,LFXO).
pub const START_MS: u32 = 1000;

/// The source a running watchdog holds the clock on, or `None` when no
/// watchdog runs and the clock may be stopped.
pub fn held() -> Option<Source> {
    if !WDT.runstatus().read().runstatus() {
        return None;
    }
    let stat = CLOCK.lfclkstat().read();
    // A clock the watchdog forced on, with no start task, is the RC
    // oscillator.
    Some(match (stat.state(), stat.src()) {
        (true, Lfclksrc::Xtal) => Source::Crystal,
        _ => Source::Rc,
    })
}

/// Start the clock on the crystal, falling back to the RC oscillator, or
/// leave it on the source a running watchdog holds it on. `delay` must
/// not count on the clock being started.
pub fn probe(delay: &mut impl DelayNs) -> Found {
    if let Some(source) = held() {
        return Found::Held(source);
    }
    let started = || CLOCK.events_lfclkstarted().read() != 0;
    // LFCLKSRC may be written only with the clock stopped, and LFCLKSTOP
    // triggered only with it running (Product Specification, CLOCK, LFCLK).
    let start = |src: Lfclksrc| {
        if CLOCK.lfclkstat().read().state() {
            CLOCK.tasks_lfclkstop().write_value(1);
            while CLOCK.lfclkstat().read().state() {}
        }
        CLOCK.lfclksrc().write(|w| w.set_src(src));
        CLOCK.events_lfclkstarted().write_value(0);
        CLOCK.tasks_lfclkstart().write_value(1);
    };
    start(Lfclksrc::Xtal);
    for ms in 0..START_MS {
        if started() {
            return Found::Crystal { started_ms: ms };
        }
        delay.delay_ms(1);
    }
    start(Lfclksrc::Rc);
    while !started() {}
    Found::Rc
}
