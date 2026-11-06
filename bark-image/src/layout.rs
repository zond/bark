//! Where everything sits in the T-Echo's flash, its external flash and
//! its RAM, the one table the firmware, its loader, their linker scripts
//! and `bark-fw/scripts/uf2.py` (through the symbols the linker scripts
//! define) all take it from.
//!
//! The internal flash, 1 MiB in 4 KiB pages (nRF52840 Product
//! Specification, Memory and NVMC: a page is the erase unit). What the
//! board's bootloader fixes comes from its own source (roadmap D6, D129);
//! what lies between is ours, laid out in `docs/flash-plan-2026-11-06.md`,
//! section 1 (roadmap D139):
//!
//! | Region | Holds |
//! |---|---|
//! | `0x00000`–`0x01000` | MBR |
//! | [`LOADER`] | our loader, where the bootloader jumps |
//! | [`STATE`] | the loader's state: swap requested, progress, confirmed |
//! | [`ACTIVE`] | the running image |
//! | [`RECORD`] | the staged image's header, which the loader checks |
//! | [`SPARE`] | nothing yet: room for secrets to come |
//! | [`MAILBOX_STATE`] | the mailbox's state, its key among it (`docs/app-layer-2026-11-05.md`, section 5) |
//! | [`STORE`] | the node's stored state (roadmap D73) |
//! | `0xF4000`–`0x100000` | the bootloader, its settings, the MBR's parameters |
//!
//! What is large and not secret lies on the external flash
//! ([`external`]): the staged image, the coded pieces of an update and
//! the mailbox's ring. What runs, boots or is secret stays inside, the
//! secrets above [`DRIVE_END`].

/// The erase unit (nRF52840 Product Specification, NVMC).
pub const PAGE: u32 = 4096;

/// A range of flash or RAM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    /// First byte.
    pub start: u32,
    /// Bytes.
    pub len: u32,
}

impl Region {
    /// One past the last byte.
    pub const fn end(self) -> u32 {
        self.start + self.len
    }

    const fn pages_from(start: u32, pages: u32) -> Region {
        Region {
            start,
            len: pages * PAGE,
        }
    }
}

/// Where the board's bootloader jumps when no SoftDevice is present: the
/// MBR's end (`docs/softdevice-2026-11-05.md`). A UF2 that writes the
/// loader here also erases what is left of the SoftDevice up to
/// [`SOFTDEVICE_END`], and the loader's word at [`SOFTDEVICE_MAGIC_AT`]
/// must not be [`SOFTDEVICE_MAGIC`], or the bootloader takes it for one.
pub const APPLICATION_START: u32 = 0x1000;

/// The end of the S140 6.1.1 SoftDevice the boards came with.
pub const SOFTDEVICE_END: u32 = 0x26000;

/// Where the SoftDevice's information structure keeps its magic number.
pub const SOFTDEVICE_MAGIC_AT: u32 = 0x3004;

/// The magic number the bootloader looks for there.
pub const SOFTDEVICE_MAGIC: u32 = 0x51B1_E5DB;

/// The end of the flash the board's bootloader serves on its drive, as
/// `CURRENT.UF2`, and writes from a UF2 (`docs/softdevice-2026-11-05.md`,
/// "How the drive places a UF2"): anyone with the board and a double tap
/// reads every byte below it, so secrets lie above it.
pub const DRIVE_END: u32 = 0xEA000;

/// The first page of the board's bootloader, above everything of ours.
pub const BOOTLOADER_START: u32 = 0xF4000;

/// The end of flash, 1 MiB.
pub const FLASH_END: u32 = 0x10_0000;

/// Pages for the loader, the note's 16 KiB. It builds to 8.5 KiB, three
/// pages (`bark-fw/README.md`, "Updates"); the fourth leaves it room to
/// grow by nearly as much again, since the layout cannot move once
/// boards are in the field.
const LOADER_PAGES: u32 = 4;

/// The image's pages: everything between the loader's state and the
/// record, which takes the last page the bootloader's drive writes, so
/// that every UF2 writes it erased. 227 pages, 908 KiB.
const ACTIVE_PAGES: u32 = (DRIVE_END - PAGE - STATE.end()) / PAGE;

const STORE_START: u32 = BOOTLOADER_START - PAGE;

/// The loader.
pub const LOADER: Region = Region::pages_from(APPLICATION_START, LOADER_PAGES);
/// The loader's state page (embassy-boot's STATE partition).
pub const STATE: Region = Region::pages_from(LOADER.end(), 1);
/// The running image (embassy-boot's ACTIVE partition).
pub const ACTIVE: Region = Region::pages_from(STATE.end(), ACTIVE_PAGES);
/// The staged image's record: its [`crate::Header`], then the word the
/// loader writes once it has checked the image against it. Inside, where
/// no clip on the external flash reaches it: it holds the hash the
/// running firmware checked before it staged.
pub const RECORD: Region = Region::pages_from(ACTIVE.end(), 1);
/// The mailbox's state page: it holds the mailbox key.
pub const MAILBOX_STATE: Region = Region::pages_from(STORE_START - PAGE, 1);
/// Between the record and the mailbox's state, above [`DRIVE_END`]:
/// unused, for secrets to come, such as an update's given-up versions.
pub const SPARE: Region = Region {
    start: RECORD.end(),
    len: MAILBOX_STATE.start - RECORD.end(),
};
/// The node's stored state, one page (roadmap D73).
pub const STORE: Region = Region::pages_from(STORE_START, 1);

/// The external flash, a ZD25WQ16B of 2 MiB in sectors of 4 KiB
/// (`docs/external-flash-2026-11-06.md`; `crate::ext_flash`). Addresses
/// are the chip's, from 0; nothing here is in the processor's address
/// space.
///
/// | Region | Holds |
/// |---|---|
/// | [`external::DFU`] | the staged image, one sector larger than [`ACTIVE`] |
/// | [`external::PIECES`] | coded pieces of an update arriving over the mesh |
/// | [`external::MAILBOX`] | the mailbox's ring |
/// | the rest | free |
pub mod external {
    use super::{Region, ACTIVE_PAGES};

    /// The erase unit, a sector (ZD25WQ16B datasheet, "Sector Erase"),
    /// the size of an internal page, so that the swap copies one into
    /// the other.
    pub const SECTOR: u32 = super::PAGE;

    /// The chip's end, 16 Mbit (datasheet, "Features").
    pub const END: u32 = 2 * 1024 * 1024;

    /// Sectors for coded pieces: 352 pieces of 256 bytes, five and a half
    /// blocks of 64, more than a node gathers at once
    /// (`docs/mesh-update-2026-10-25.md`, "The code").
    const PIECES_SECTORS: u32 = 22;

    /// Sectors for the mailbox's ring: 64 of 16 items of 256 bytes, 1024
    /// items, 44 days at 23 a day (`docs/app-layer-2026-11-05.md`,
    /// section 5).
    const MAILBOX_SECTORS: u32 = 64;

    /// The staged image (embassy-boot's DFU partition), one sector more
    /// than the running one (embassy-boot, `BootLoader::new`: "The dfu
    /// partition must be at least PAGE_SIZE bigger than the active
    /// partition"). After a swap it holds the previous image, for the
    /// swap back.
    pub const DFU: Region = Region::pages_from(0, ACTIVE_PAGES + 1);
    /// Coded pieces of an update arriving over the mesh; unused until the
    /// transport exists.
    pub const PIECES: Region = Region::pages_from(DFU.end(), PIECES_SECTORS);
    /// The mailbox's ring. Its items are sealed to the phone, so whoever
    /// reads the chip learns nothing from them.
    pub const MAILBOX: Region = Region::pages_from(PIECES.end(), MAILBOX_SECTORS);
    /// Free.
    pub const FREE: Region = Region {
        start: MAILBOX.end(),
        len: END - MAILBOX.end(),
    };
}

/// What the loader write-protects before it jumps (nRF52840 Product
/// Specification, ACL): the MBR and itself, from the bottom of flash, so
/// that a faulty image cannot erase what boots it.
pub const PROTECT_LOW: Region = Region {
    start: 0,
    len: LOADER.end(),
};
/// The second region the loader write-protects: the bootloader, its
/// settings and the MBR's parameter page.
pub const PROTECT_HIGH: Region = Region {
    start: BOOTLOADER_START,
    len: FLASH_END - BOOTLOADER_START,
};

/// RAM, 256 KiB (nRF52840 Product Specification, Memory).
const RAM_START: u32 = 0x2000_0000;
const RAM_END: u32 = 0x2004_0000;

/// The MBR's RAM words, left untouched: Nordic SDK builds against the MBR
/// start RAM at `0x20000008`, and the bootloader calls into the MBR on
/// every reset.
const MBR_RAM: u32 = 8;

/// Where a panic message waits for the next boot (`panic-persist` in
/// `bark-fw`): below every other use of RAM, so neither the loader nor
/// the bootloader overwrites it. The bootloader's own RAM starts at
/// `0x20008000`, plus `0x20007F7C`..`0x20008000` for its double-reset
/// word and noinit block, so a soft reset through it should leave this
/// range intact; unverified on hardware.
pub const PANIC_DUMP: Region = Region {
    start: RAM_START + MBR_RAM,
    len: 0x400 - MBR_RAM,
};

/// RAM for `.data`, `.bss` and the stack, in the firmware and the loader
/// alike. The board's bootloader does not enable the SoftDevice on a
/// boot to the application, so all of it is ours; the BLE controller
/// (roadmap D5) will take its memory as a static buffer inside it.
pub const RAM: Region = Region {
    start: PANIC_DUMP.end(),
    len: RAM_END - PANIC_DUMP.end(),
};

// The internal regions fill the flash between the loader and the
// bootloader, the stored state's page is where D73 put it, and each
// protected region is within what the ACL takes: page-aligned, and at most
// half the flash (Product Specification, ACL).
const _: () = {
    // Every UF2 writes the running slot and the record, and the
    // bootloader's drive writes only below its end.
    assert!(RECORD.end() == DRIVE_END);
    // The secrets lie where the drive neither reads nor writes.
    assert!(MAILBOX_STATE.start >= DRIVE_END && STORE.start >= DRIVE_END);
    assert!(MAILBOX_STATE.end() == STORE.start);
    assert!(STORE.start == 0xF3000);
    assert!(STORE.end() == BOOTLOADER_START);
    assert!(external::DFU.len == ACTIVE.len + external::SECTOR);
    assert!(external::MAILBOX.end() <= external::END);
    assert!(PROTECT_LOW.len.is_multiple_of(PAGE) && PROTECT_LOW.len <= FLASH_END / 2);
    assert!(PROTECT_HIGH.start.is_multiple_of(PAGE) && PROTECT_HIGH.len <= FLASH_END / 2);
    // embassy-boot keeps two progress words a page for the swap and two for
    // the swap back, after two words of its own, in the state page:
    // `assert_partitions` in its `boot_loader.rs`. The NVMC writes words of
    // 4 bytes.
    assert!(2 + 4 * ACTIVE_PAGES <= PAGE / 4);
};

/// Which linker script to write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Image {
    /// The firmware, in [`ACTIVE`].
    Firmware,
    /// The loader, in [`LOADER`].
    Loader,
}

/// Write `memory.x` for `image`: cortex-m-rt's `FLASH` and `RAM`, and
/// symbols for every internal region, which `scripts/uf2.py` reads.
pub fn write_memory_x(out: &mut impl core::fmt::Write, image: Image) -> core::fmt::Result {
    let flash = match image {
        Image::Firmware => ACTIVE,
        Image::Loader => LOADER,
    };
    writeln!(
        out,
        "/* Written by build.rs from bark-image/src/layout.rs; edit that. */"
    )?;
    writeln!(out, "MEMORY\n{{")?;
    let region = |out: &mut dyn core::fmt::Write, name: &str, r: Region| {
        writeln!(
            out,
            "  {name:<7} : ORIGIN = {:#010x}, LENGTH = {:#x}",
            r.start, r.len
        )
    };
    region(out, "FLASH", flash)?;
    if image == Image::Firmware {
        region(out, "STORE", STORE)?;
        region(out, "PANDUMP", PANIC_DUMP)?;
    }
    region(out, "RAM", RAM)?;
    writeln!(out, "}}\n")?;
    let symbols = [
        ("__bark_loader", LOADER),
        ("__bootloader_state", STATE),
        ("__bootloader_active", ACTIVE),
        ("__bark_record", RECORD),
        ("__bark_spare", SPARE),
        ("__bark_mailbox_state", MAILBOX_STATE),
        ("__bark_store", STORE),
    ];
    for (name, r) in symbols {
        writeln!(out, "{name}_start = {:#010x};", r.start)?;
        writeln!(out, "{name}_end = {:#010x};", r.end())?;
    }
    if image == Image::Firmware {
        writeln!(out, "_panic_dump_start = ORIGIN(PANDUMP);")?;
        writeln!(out, "_panic_dump_end = ORIGIN(PANDUMP) + LENGTH(PANDUMP);")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::string::String;

    #[test]
    fn the_regions_are_the_plans() {
        assert_eq!(
            LOADER,
            Region {
                start: 0x1000,
                len: 0x4000
            }
        );
        assert_eq!(STATE.start, 0x5000);
        assert_eq!(
            ACTIVE,
            Region {
                start: 0x6000,
                len: 227 * PAGE
            }
        );
        assert_eq!(RECORD.start, 0xE9000);
        assert_eq!(
            SPARE,
            Region {
                start: 0xEA000,
                len: 8 * PAGE
            }
        );
        assert_eq!(MAILBOX_STATE.start, 0xF2000);
        assert_eq!(STORE.start, 0xF3000);
        assert_eq!(
            external::DFU,
            Region {
                start: 0,
                len: 228 * PAGE
            }
        );
        assert_eq!(
            external::PIECES,
            Region {
                start: 0xE4000,
                len: 22 * PAGE
            }
        );
        assert_eq!(
            external::MAILBOX,
            Region {
                start: 0xFA000,
                len: 64 * PAGE
            }
        );
        assert_eq!(
            external::FREE,
            Region {
                start: 0x13A000,
                len: 198 * PAGE
            }
        );
    }

    #[test]
    fn the_regions_follow_each_other() {
        let inside = [LOADER, STATE, ACTIVE, RECORD, SPARE, MAILBOX_STATE, STORE];
        let outside = [
            external::DFU,
            external::PIECES,
            external::MAILBOX,
            external::FREE,
        ];
        for order in [&inside[..], &outside[..]] {
            for pair in order.windows(2) {
                assert_eq!(pair[0].end(), pair[1].start);
            }
            for r in order {
                assert_eq!(r.start % PAGE, 0);
                assert_eq!(r.len % PAGE, 0);
            }
        }
        assert_eq!(inside[0].start, APPLICATION_START);
        assert_eq!(inside[6].end(), BOOTLOADER_START);
        assert_eq!(outside[0].start, 0);
        assert_eq!(outside[3].end(), external::END);
    }

    #[test]
    fn the_firmware_script_names_its_slot() {
        let mut s = String::new();
        write_memory_x(&mut s, Image::Firmware).unwrap();
        assert!(s.contains("FLASH   : ORIGIN = 0x00006000, LENGTH = 0xe3000"));
        assert!(s.contains("__bootloader_state_start = 0x00005000;"));
        assert!(s.contains("_panic_dump_start"));
        let mut l = String::new();
        write_memory_x(&mut l, Image::Loader).unwrap();
        assert!(l.contains("FLASH   : ORIGIN = 0x00001000, LENGTH = 0x4000"));
        assert!(!l.contains("PANDUMP"));
    }
}
