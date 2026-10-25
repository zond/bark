//! Where everything sits in the T-Echo's flash and RAM, the one table the
//! firmware, its loader, their linker scripts and `bark-fw/scripts/uf2.py`
//! (through the symbols the linker scripts define) all take it from.
//!
//! Flash, 1 MiB in 4 KiB pages (nRF52840 Product Specification, Memory and
//! NVMC: a page is the erase unit). What the board's bootloader fixes
//! comes from its own linker script and the S140 6.1.1 SoftDevice
//! specification (roadmap D6); what lies between is ours, laid out in
//! `docs/mesh-update-2026-10-25.md`, "Flash layout":
//!
//! | Region | Holds |
//! |---|---|
//! | `0x00000`–`0x01000` | MBR |
//! | `0x01000`–`0x26000` | SoftDevice S140 6.1.1, kept, never enabled |
//! | [`LOADER`] | our loader, where the bootloader jumps |
//! | [`STATE`] | the loader's state: swap requested, progress, confirmed |
//! | [`ACTIVE`] | the running image |
//! | [`DFU`] | the staged image, one page larger than [`ACTIVE`] |
//! | [`RECORD`] | the staged image's header, which the loader checks |
//! | [`PIECES`] | coded pieces of an update arriving over the mesh |
//! | [`STORE`] | the node's stored state (roadmap D73) |
//! | `0xF4000`–`0x100000` | the bootloader, its settings, the MBR's parameters |

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

/// Where the board's bootloader jumps: the first page above the
/// SoftDevice (its linker script, `FLASH` origin).
pub const APPLICATION_START: u32 = 0x26000;

/// The first page of the board's bootloader, above everything of ours.
pub const BOOTLOADER_START: u32 = 0xF4000;

/// The end of flash, 1 MiB.
pub const FLASH_END: u32 = 0x10_0000;

/// Pages for the loader, the note's 16 KiB. It builds to 8.5 KiB, three
/// pages (`bark-fw/README.md`, "Updates"); the fourth leaves it room to
/// grow by nearly as much again, since the layout cannot move once
/// boards are in the field.
const LOADER_PAGES: u32 = 4;

/// Pages for update staging: one for [`RECORD`] and 22 for coded pieces,
/// 352 pieces of 256 bytes, five and a half blocks of 64, more than a
/// node gathers at once (the note, "The code").
const STAGING_PAGES: u32 = 23;

/// The pages between the loader and the stored state's page.
const SHARED_PAGES: u32 = (STORE_START - APPLICATION_START) / PAGE;

/// The image's pages: what is left once the loader, its state and the
/// staging have theirs, split between the running image and the staged
/// one, which needs a page more (embassy-boot, `BootLoader::new`: "The
/// dfu partition must be at least PAGE_SIZE bigger than the active
/// partition"). 88 pages, 352 KiB: 2.1 times the image of 2026-10-25,
/// 1.44 times the 250 kB the note plans for once BLE and the application
/// are in.
const ACTIVE_PAGES: u32 = (SHARED_PAGES - LOADER_PAGES - 1 - STAGING_PAGES - 1) / 2;

const STORE_START: u32 = BOOTLOADER_START - PAGE;

/// The loader.
pub const LOADER: Region = Region::pages_from(APPLICATION_START, LOADER_PAGES);
/// The loader's state page (embassy-boot's STATE partition).
pub const STATE: Region = Region::pages_from(LOADER.end(), 1);
/// The running image (embassy-boot's ACTIVE partition).
pub const ACTIVE: Region = Region::pages_from(STATE.end(), ACTIVE_PAGES);
/// The staged image (embassy-boot's DFU partition).
pub const DFU: Region = Region::pages_from(ACTIVE.end(), ACTIVE_PAGES + 1);
/// The staged image's record: its [`crate::Header`], then the word the
/// loader writes once it has checked the image against it.
pub const RECORD: Region = Region::pages_from(DFU.end(), 1);
/// Coded pieces of an update arriving over the mesh; unused until the
/// transport exists.
pub const PIECES: Region = Region::pages_from(RECORD.end(), STAGING_PAGES - 1);
/// The node's stored state, one page (roadmap D73).
pub const STORE: Region = Region::pages_from(STORE_START, 1);

/// What the loader write-protects before it jumps (nRF52840 Product
/// Specification, ACL): the MBR, the SoftDevice and itself, from the
/// bottom of flash, so that a faulty image cannot erase what boots it.
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
/// alike.
pub const RAM: Region = Region {
    start: PANIC_DUMP.end(),
    len: RAM_END - PANIC_DUMP.end(),
};

// The layout fills the shared pages exactly, the stored state's page is
// where D73 put it, and each protected region is within what the ACL takes:
// page-aligned, and at most half the flash (Product Specification, ACL).
const _: () = {
    assert!(PIECES.end() == STORE.start);
    assert!(STORE.start == 0xF3000);
    assert!(STORE.end() == BOOTLOADER_START);
    assert!(DFU.len == ACTIVE.len + PAGE);
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
/// symbols for every region, which embassy-boot and `scripts/uf2.py` read.
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
        ("__bootloader_dfu", DFU),
        ("__bark_record", RECORD),
        ("__bark_pieces", PIECES),
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
    fn the_regions_are_the_notes() {
        assert_eq!(
            LOADER,
            Region {
                start: 0x26000,
                len: 0x4000
            }
        );
        assert_eq!(STATE.start, 0x2A000);
        assert_eq!(
            ACTIVE,
            Region {
                start: 0x2B000,
                len: 88 * PAGE
            }
        );
        assert_eq!(
            DFU,
            Region {
                start: 0x83000,
                len: 89 * PAGE
            }
        );
        assert_eq!(RECORD.start, 0xDC000);
        assert_eq!(
            PIECES,
            Region {
                start: 0xDD000,
                len: 22 * PAGE
            }
        );
        assert_eq!(STORE.start, 0xF3000);
    }

    #[test]
    fn the_regions_follow_each_other() {
        let order = [LOADER, STATE, ACTIVE, DFU, RECORD, PIECES, STORE];
        for pair in order.windows(2) {
            assert_eq!(pair[0].end(), pair[1].start);
        }
        assert_eq!(order[0].start, APPLICATION_START);
        assert_eq!(order[6].end(), BOOTLOADER_START);
        for r in order {
            assert_eq!(r.start % PAGE, 0);
            assert_eq!(r.len % PAGE, 0);
        }
    }

    #[test]
    fn the_firmware_script_names_its_slot() {
        let mut s = String::new();
        write_memory_x(&mut s, Image::Firmware).unwrap();
        assert!(s.contains("FLASH   : ORIGIN = 0x0002b000, LENGTH = 0x58000"));
        assert!(s.contains("__bootloader_state_start = 0x0002a000;"));
        assert!(s.contains("_panic_dump_start"));
        let mut l = String::new();
        write_memory_x(&mut l, Image::Loader).unwrap();
        assert!(l.contains("FLASH   : ORIGIN = 0x00026000, LENGTH = 0x4000"));
        assert!(!l.contains("PANDUMP"));
    }
}
