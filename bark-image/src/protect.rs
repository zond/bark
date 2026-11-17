//! Read-out protection: whether the chip's debug port reads the chip, from
//! what the boot finds, for the log. The firmware never writes it (roadmap
//! D224); the reads are in `bark-fw/src/protect.rs`, the reading of them
//! here, to be tested on the host.
//!
//! nRF52840 Product Specification, "Access port protection": on build
//! codes D and earlier the port is open unless `UICR.APPROTECT` is
//! `Enabled`; on F and later it is closed after every reset unless
//! `UICR.APPROTECT` is `HwDisabled` and firmware has written `SwDisable` to
//! `APPROTECT.DISABLE` since, and not `Force` to `APPROTECT.FORCEPROTECT`.

/// `UICR.APPROTECT`'s values (Product Specification, UICR, `PALL`).
pub mod uicr {
    /// Protected, on every build code.
    pub const ENABLED: u32 = 0x0000_0000;
    /// Open as far as the hardware goes, on build codes F and later.
    pub const HW_DISABLED: u32 = 0x0000_005A;
    /// Erased: open on build codes D and earlier.
    pub const ERASED: u32 = 0xFFFF_FFFF;
}

/// `APPROTECT.DISABLE`'s `SwDisable` (Product Specification, APPROTECT).
pub const SW_DISABLE: u32 = 0x5A;
/// `APPROTECT.FORCEPROTECT`'s `Force` (Product Specification, APPROTECT).
pub const FORCE: u32 = 0x00;

/// How the chip protects its debug port, by its build code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chip {
    /// Build codes D and earlier: by UICR alone.
    Hardware,
    /// Build codes F and later (revision 3): by UICR and by firmware at
    /// every boot.
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

/// The `APPROTECT` peripheral's registers as read, on build codes F and
/// later; D and earlier have none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Soft {
    /// `APPROTECT.DISABLE`.
    pub disable: u32,
    /// `APPROTECT.FORCEPROTECT`.
    pub forceprotect: u32,
}

impl Soft {
    /// The registers' values after a reset (Product Specification,
    /// APPROTECT), which nothing has written.
    pub const RESET: Soft = Soft {
        disable: 0,
        forceprotect: 0xFFFF_FFFF,
    };
}

/// Whether the port is closed, from `UICR.APPROTECT` as of the last reset
/// and, on F, the `APPROTECT` registers. On D any value but erased is
/// taken as closed, as only `Enabled` and erased are defined.
pub fn closed(chip: Chip, uicr: u32, soft: Soft) -> bool {
    let pall = uicr & 0xFF;
    match chip {
        Chip::Hardware => pall != uicr::ERASED & 0xFF,
        Chip::HardwareAndSoftware => {
            let forced = soft.forceprotect & 0xFF == FORCE;
            let sw_disabled = soft.disable & 0xFF == SW_DISABLE;
            !(pall == uicr::HW_DISABLED && sw_disabled && !forced)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `AAF0` and `AAD0`, as the chip holds them.
    const REV3: u32 = u32::from_be_bytes(*b"AAF0");
    const REV2: u32 = u32::from_be_bytes(*b"AAD0");
    const REV1: u32 = u32::from_be_bytes(*b"AAC0");

    const SW_DISABLED: Soft = Soft {
        disable: SW_DISABLE,
        forceprotect: Soft::RESET.forceprotect,
    };

    #[test]
    fn chip_from_variant() {
        assert_eq!(Chip::build_code(REV3), b'F');
        assert_eq!(Chip::of(REV3), Chip::HardwareAndSoftware);
        assert_eq!(Chip::of(REV2), Chip::Hardware);
        assert_eq!(Chip::of(REV1), Chip::Hardware);
    }

    #[test]
    fn d_is_open_while_uicr_is_erased() {
        assert!(!closed(Chip::Hardware, uicr::ERASED, Soft::RESET));
        assert!(closed(Chip::Hardware, uicr::ENABLED, Soft::RESET));
    }

    /// A board as the factory makes it is closed on F, and stays so under
    /// firmware that writes nothing.
    #[test]
    fn f_is_open_only_when_uicr_and_firmware_both_open_it() {
        let f = Chip::HardwareAndSoftware;
        assert!(closed(f, uicr::ERASED, Soft::RESET));
        assert!(closed(f, uicr::ERASED, SW_DISABLED));
        assert!(closed(f, uicr::HW_DISABLED, Soft::RESET));
        assert!(!closed(f, uicr::HW_DISABLED, SW_DISABLED));
        assert!(closed(f, uicr::ENABLED, SW_DISABLED));
        let forced = Soft {
            forceprotect: FORCE,
            ..SW_DISABLED
        };
        assert!(closed(f, uicr::HW_DISABLED, forced));
    }
}
