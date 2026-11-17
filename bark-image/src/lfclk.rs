//! The 32.768 kHz clock (LFCLK), started at boot by the loader and by the
//! firmware on the crystal if it starts, else on the internal RC
//! oscillator: a crystal may be missing or broken.
//!
//! A running watchdog holds the clock until a reset other than a soft one:
//! LFCLKSTOP then leaves it running, and a watchdog started with no clock
//! forces the RC oscillator on (nRF52840 Product Specification, WDT, CLOCK,
//! POWER). The board's bootloader, which runs before every boot, writes
//! LFCLKSRC with the RC oscillator and triggers LFCLKSTART; under a running
//! watchdog that moves the clock onto the RC oscillator, and it stays
//! there (roadmap D229). So a clock held on the RC oscillator is moved back
//! the same way, with the crystal: LFCLKSRC written while the clock runs,
//! which the Product Specification does not provide for (CLOCK, LFCLK),
//! and LFCLKSTART. A crystal that does not start never raises
//! LFCLKSTARTED, and nothing else tells a slow one from a missing one
//! (same section).

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
pub struct Found {
    /// The source the clock runs from.
    pub source: Source,
    /// Milliseconds from the crystal's start task to LFCLKSTARTED, when
    /// the probe started the crystal.
    pub started_ms: Option<u32>,
    /// The source a running watchdog held the clock on as the probe began.
    pub held: Option<Source>,
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
    // A clock the watchdog forced on, with no start task, is the RC
    // oscillator.
    Some(if runs_on(Lfclksrc::Xtal) {
        Source::Crystal
    } else {
        Source::Rc
    })
}

fn runs_on(src: Lfclksrc) -> bool {
    let stat = CLOCK.lfclkstat().read();
    stat.state() && stat.src() == src
}

/// Start the clock on the crystal, falling back to the RC oscillator. A
/// clock a running watchdog holds on the crystal is left as it is.
/// `delay` must not count on the clock being started.
pub fn probe(delay: &mut impl DelayNs) -> Found {
    let held = held();
    if held == Some(Source::Crystal) {
        return Found {
            source: Source::Crystal,
            started_ms: None,
            held,
        };
    }
    let started = || CLOCK.events_lfclkstarted().read() != 0;
    let start = |src: Lfclksrc| {
        // LFCLKSRC may be written only with the clock stopped, and
        // LFCLKSTOP triggered only with it running (Product Specification,
        // CLOCK, LFCLK); a held clock never stops, so it is not waited for.
        if held.is_none() && CLOCK.lfclkstat().read().state() {
            CLOCK.tasks_lfclkstop().write_value(1);
            while CLOCK.lfclkstat().read().state() {}
        }
        CLOCK.lfclksrc().write(|w| w.set_src(src));
        CLOCK.events_lfclkstarted().write_value(0);
        CLOCK.tasks_lfclkstart().write_value(1);
    };
    start(Lfclksrc::Xtal);
    for ms in 0..START_MS {
        // The status too: the event alone would not show a held clock
        // that kept to the RC oscillator.
        if started() && runs_on(Lfclksrc::Xtal) {
            return Found {
                source: Source::Crystal,
                started_ms: Some(ms),
                held,
            };
        }
        delay.delay_ms(1);
    }
    start(Lfclksrc::Rc);
    while !started() {}
    Found {
        source: Source::Rc,
        started_ms: None,
        held,
    }
}
