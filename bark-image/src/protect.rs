//! Read-out protection: which writes the firmware makes at boot so that
//! the debug port can or cannot read the chip. The register writes are in
//! `bark-fw/src/protect.rs`; the choice is here, to be tested on the host.
//!
//! nRF52840 Product Specification, "Access port protection": on build
//! codes D and earlier the port is open unless `UICR.APPROTECT` is
//! `Enabled`; on F and later it is closed after every reset unless
//! `UICR.APPROTECT` is `HwDisabled` and the firmware writes `SwDisable` to
//! `APPROTECT.DISABLE`. A UICR write takes effect at the next reset
//! (NVMC), and only `CTRL-AP ERASEALL`, erasing flash, UICR and RAM,
//! clears `Enabled`.

/// `UICR.APPROTECT`'s values (Product Specification, UICR, `PALL`).
pub mod uicr {
    /// Protected, on every build code.
    pub const ENABLED: u32 = 0x0000_0000;
    /// Open as far as the hardware goes, on build codes F and later.
    pub const HW_DISABLED: u32 = 0x0000_005A;
    /// Erased: open on build codes D and earlier.
    pub const ERASED: u32 = 0xFFFF_FFFF;
}

/// `APPROTECT.DISABLE`'s `SwDisable`, which opens the port until the next
/// reset on build codes F and later (Product Specification, APPROTECT).
pub const SW_DISABLE: u32 = 0x5A;

/// What the image was built to be: open for the bench, protected for a
/// board that leaves it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Build {
    /// Built with `BARK_OPEN=1`: the debug port reads the chip.
    Open,
    /// The default: the debug port reads nothing.
    Protected,
}

/// How the chip protects its debug port, by its build code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chip {
    /// Build codes D and earlier: by UICR alone.
    Hardware,
    /// Build codes F and later (revision 3): by UICR and by the firmware
    /// at every boot.
    HardwareAndSoftware,
}

impl Chip {
    /// The build code's letter from `FICR.INFO.VARIANT`: ASCII such as
    /// `AAF0` as one big-endian word, the letter its third byte (Product
    /// Specification, FICR).
    pub fn build_code(variant: u32) -> u8 {
        (variant >> 8) as u8
    }

    /// The chip whose `FICR.INFO.VARIANT` is `variant`. Revision 3 is
    /// build code F (nRF52840 IC revision overview); a later letter is
    /// taken to keep its mechanism.
    pub fn of(variant: u32) -> Self {
        match Self::build_code(variant) >= b'F' {
            true => Chip::HardwareAndSoftware,
            false => Chip::Hardware,
        }
    }
}

/// The writes at boot, in this order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Steps {
    /// The value to program into `UICR.APPROTECT`, if any. A write only
    /// clears bits, so it can close the port and never reopen it.
    pub uicr: Option<u32>,
    /// Write `SwDisable` to `APPROTECT.DISABLE`.
    pub sw_disable: bool,
    /// Write `Force` to `APPROTECT.FORCEPROTECT`, which keeps the port
    /// closed until the next reset whatever is written to `DISABLE`.
    pub force: bool,
}

/// What a build does on a chip.
///
/// A protected build programs `Enabled` on both chips, with
/// `FORCEPROTECT` on F as the Specification advises, so that no later
/// image or bootloader writing `DISABLE` reopens it; only an erase of the
/// whole chip does. On D an erased UICR is already open.
pub fn steps(build: Build, chip: Chip) -> Steps {
    match (build, chip) {
        (Build::Protected, Chip::Hardware) => Steps {
            uicr: Some(uicr::ENABLED),
            sw_disable: false,
            force: false,
        },
        (Build::Protected, Chip::HardwareAndSoftware) => Steps {
            uicr: Some(uicr::ENABLED),
            sw_disable: false,
            force: true,
        },
        (Build::Open, Chip::Hardware) => Steps {
            uicr: None,
            sw_disable: false,
            force: false,
        },
        (Build::Open, Chip::HardwareAndSoftware) => Steps {
            uicr: Some(uicr::HW_DISABLED),
            sw_disable: true,
            force: false,
        },
    }
}

/// Whether the port is closed, from `UICR.APPROTECT` as of the last reset
/// and whether `SwDisable` was written since. On D any value but erased is
/// taken as closed, as only `Enabled` and erased are defined.
pub fn closed(chip: Chip, uicr: u32, sw_disabled: bool) -> bool {
    let pall = uicr & 0xFF;
    match chip {
        Chip::Hardware => pall != uicr::ERASED & 0xFF,
        Chip::HardwareAndSoftware => !(pall == uicr::HW_DISABLED && sw_disabled),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `AAF0` and `AAD0`, as the chip holds them.
    const REV3: u32 = u32::from_be_bytes(*b"AAF0");
    const REV2: u32 = u32::from_be_bytes(*b"AAD0");
    const REV1: u32 = u32::from_be_bytes(*b"AAC0");

    #[test]
    fn chip_from_variant() {
        assert_eq!(Chip::build_code(REV3), b'F');
        assert_eq!(Chip::of(REV3), Chip::HardwareAndSoftware);
        assert_eq!(Chip::of(REV2), Chip::Hardware);
        assert_eq!(Chip::of(REV1), Chip::Hardware);
    }

    /// What each row of the table leaves after the reset that follows its
    /// UICR write, from every state a board can be in.
    #[test]
    fn every_build_on_every_chip() {
        let states = [
            (Chip::Hardware, &[uicr::ERASED, uicr::ENABLED][..]),
            (
                Chip::HardwareAndSoftware,
                &[uicr::ERASED, uicr::HW_DISABLED, uicr::ENABLED][..],
            ),
        ];
        for (chip, befores) in states {
            for &before in befores {
                for build in [Build::Open, Build::Protected] {
                    let s = steps(build, chip);
                    // A flash write only clears bits.
                    let after = match s.uicr {
                        Some(v) if before & v == v => v,
                        _ => before,
                    };
                    let shut = closed(chip, after, s.sw_disable) || s.force;
                    let expect = match build {
                        Build::Protected => true,
                        // Open unless a protected image has run before.
                        Build::Open => before == uicr::ENABLED,
                    };
                    assert_eq!(shut, expect, "{build:?} on {chip:?} from {before:#x}");
                }
            }
        }
    }

    #[test]
    fn writes_by_chip() {
        let open_f = steps(Build::Open, Chip::HardwareAndSoftware);
        assert_eq!(open_f.uicr, Some(uicr::HW_DISABLED));
        assert!(open_f.sw_disable && !open_f.force);
        assert_eq!(steps(Build::Open, Chip::Hardware).uicr, None);
        for chip in [Chip::Hardware, Chip::HardwareAndSoftware] {
            let s = steps(Build::Protected, chip);
            assert_eq!(s.uicr, Some(uicr::ENABLED));
            assert!(!s.sw_disable);
        }
    }
}
