//! The debug port's read-out protection as the boot finds it, for the log
//! (README, "Read-out protection"). Nothing here writes it: a board stays
//! as its factory and its earlier images left it (roadmap D224).

use bark_image::protect::{self, Chip, Soft};
use embassy_nrf::pac;

/// What the boot found.
#[derive(Clone, Copy)]
pub struct State {
    /// The build code's letter, `F` on revision 3.
    pub build_code: u8,
    /// `UICR.APPROTECT` as it holds since the last reset.
    pub uicr: u32,
    /// Whether the debug port reads nothing.
    pub closed: bool,
}

/// Reads the chip's protection; only reads.
pub fn read() -> State {
    let variant = pac::FICR.info().variant().read();
    let chip = Chip::of(variant);
    let uicr = pac::UICR.approtect().read().0;
    // The registers exist only on build code F and later.
    let soft = match chip {
        Chip::Hardware => Soft::RESET,
        Chip::HardwareAndSoftware => Soft {
            disable: pac::APPROTECT.disable().read().0,
            forceprotect: pac::APPROTECT.forceprotect().read().0,
        },
    };
    State {
        build_code: Chip::build_code(variant),
        uicr,
        closed: protect::closed(chip, uicr, soft),
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
