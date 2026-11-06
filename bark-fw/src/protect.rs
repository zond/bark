//! Read-out protection, set first thing at every boot (roadmap D138;
//! README, "Read-out protection"). The choice by build and chip is
//! `bark_image::protect`'s; this file makes the writes.

use bark_image::protect::{self, Build, Chip};
use embassy_nrf::pac;
use embassy_nrf::pac::approtect::vals::{Disable, Forceprotect};

/// Protected unless built with `BARK_OPEN=1`, so that no image is open
/// by mistake. Any other value fails the build rather than guess.
pub const BUILD: Build = match option_env!("BARK_OPEN") {
    None => Build::Protected,
    Some(v) => {
        assert!(matches!(v.as_bytes(), b"1"), "BARK_OPEN is 1 or unset");
        Build::Open
    }
};

/// What the boot found and left, for the log.
#[derive(Clone, Copy)]
pub struct State {
    /// The build code's letter, `F` on revision 3.
    pub build_code: u8,
    /// `UICR.APPROTECT` as it holds since the last reset.
    pub uicr: u32,
    /// Whether the debug port reads nothing.
    pub closed: bool,
}

/// Make the build's writes. A UICR write takes effect only at the next
/// reset (Product Specification, NVMC), so after one this resets the
/// chip, and the next boot finds the value in place and writes nothing.
/// Runs before anything else, so that the window in which a board that
/// is to be protected is still open, at its first boot after the flash,
/// lasts no longer than the write.
pub fn apply() -> State {
    let variant = pac::FICR.info().variant().read();
    let chip = Chip::of(variant);
    let steps = protect::steps(BUILD, chip);
    let uicr = pac::UICR.approtect().as_ptr() as *mut u32;
    if let Some(value) = steps.uicr {
        // SAFETY: `uicr` is UICR.APPROTECT, and nothing else uses the
        // NVMC this early.
        if unsafe { embassy_nrf::uicr_write(uicr, value) } == embassy_nrf::WriteResult::Written {
            cortex_m::peripheral::SCB::sys_reset();
        }
        // `Failed`: an open build on a board a protected image has
        // closed; only an erase of the whole chip opens it, and the
        // state below says so.
    }
    let ap = pac::APPROTECT;
    if steps.force {
        ap.forceprotect()
            .write(|w| w.set_forceprotect(Forceprotect::Force));
    }
    if steps.sw_disable {
        ap.disable().write(|w| w.set_disable(Disable::SwDisable));
    }
    // SAFETY: a read of UICR, which is memory-mapped flash.
    let held = unsafe { uicr.read_volatile() };
    State {
        build_code: Chip::build_code(variant),
        uicr: held,
        closed: steps.force || protect::closed(chip, held, steps.sw_disable),
    }
}

impl core::fmt::Display for State {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "protection={} build_code={} approtect={:#x}",
            if self.closed { "on" } else { "off" },
            self.build_code as char,
            self.uicr
        )
    }
}
