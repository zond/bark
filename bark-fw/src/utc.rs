//! The time of day, for the display and the phone: UTC from the GPS
//! (`gps.rs`), or from the phone while the node has none, carried on by
//! the embassy clock on RTC1 (`bark_driver::utc`); and the phone's UTC
//! offset, kept across a reboot (`store.rs`). The protocol reads none of
//! it (spec "Position").

use core::cell::Cell;

use bark_driver::utc::{Source, Utc};
use bark_proto::Instant;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;

/// The 32.768 kHz clock's tolerance, ppm: the crystal's, as the MPSL is
/// told it, ±250 ppm, since the fitted one's is not known; the RC
/// oscillator's, calibrated, ±500 ppm (`ble.rs`, `mpsl`).
pub fn ppm(source: crate::lfclk::Source) -> u32 {
    match source {
        crate::lfclk::Source::Crystal { .. } => 250,
        crate::lfclk::Source::Rc => 500,
    }
}

/// The time kept and the UTC offset in minutes.
static KEPT: Mutex<CriticalSectionRawMutex, Cell<(Utc, i16)>> =
    Mutex::new(Cell::new((Utc::new(500), 0)));

fn now() -> Instant {
    Instant::from_micros(embassy_time::Instant::now().as_micros())
}

fn with<R>(f: impl FnOnce(&mut Utc, &mut i16) -> R) -> R {
    KEPT.lock(|c| {
        let (mut utc, mut offset) = c.get();
        let r = f(&mut utc, &mut offset);
        c.set((utc, offset));
        r
    })
}

/// At boot: the clock's tolerance and the offset kept.
pub fn init(ppm: u32, offset_min: i16) {
    with(|u, o| {
        *u = Utc::new(ppm);
        *o = offset_min;
    });
}

/// A GPS fix said `utc_s`; the first time since none is logged.
pub fn gps(utc_s: u64) {
    let t = now();
    let had = with(|u, _| {
        let had = matches!(u.now(t), Some((_, Source::Gps)));
        u.gps(utc_s, t);
        had
    });
    if !had {
        log::info!("utc: {} s from the gps", utc_s);
    }
}

/// The phone said `utc_s` and its offset: the offset is taken, the time
/// only without a GPS time. Returns whether the offset changed.
pub fn phone(utc_s: u64, offset_min: i16) -> bool {
    let t = now();
    let (taken, changed) = with(|u, o| {
        let changed = *o != offset_min;
        *o = offset_min;
        (u.phone(utc_s, t), changed)
    });
    log::info!(
        "utc: phone {} s offset {} min, time {}",
        utc_s,
        offset_min,
        if taken { "taken" } else { "kept from the gps" }
    );
    changed
}

/// The time now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Time {
    /// UTC seconds since 1970, if known.
    pub utc_s: Option<u64>,
    /// Where it came from.
    pub source: Option<Source>,
    /// The phone's UTC offset, minutes; 0 until a phone gives one.
    pub offset_min: i16,
}

/// The time now, for the display and the log.
pub fn time() -> Time {
    let t = now();
    with(|u, o| {
        let n = u.now(t);
        Time {
            utc_s: n.map(|(s, _)| s),
            source: n.map(|(_, s)| s),
            offset_min: *o,
        }
    })
}
