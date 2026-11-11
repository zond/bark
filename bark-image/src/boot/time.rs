//! How long the loader's swap of a staged image keeps a node off the air:
//! the time a node back from it tells its core it was away, and the
//! simulator's swap (spec "Updates over the mesh").

use super::cut::{INTERNAL_ERASE_US, INTERNAL_WORD_US};
use crate::ext_flash::{ERASE_TYP_US, PROGRAM_PAGE, PROGRAM_TYP_US};
use crate::layout::{ACTIVE, PAGE};

/// The external flash's bus as measured on two boards: 256 KiB read in
/// 2 088 ms, about 1 Mbit/s from the software bus (roadmap, Phase 2, "The
/// external flash's bus").
const BUS_BYTES: u64 = 256 * 1024;
/// See [`BUS_BYTES`].
const BUS_US: u64 = 2_088_000;

/// µs to move `bytes` over the external flash's bus, rounded up.
pub const fn bus_us(bytes: u64) -> u64 {
    (bytes * BUS_US).div_ceil(BUS_BYTES)
}

/// One page of the swap, 223.0 ms: an internal erase and a page of word
/// writes (nRF52840 Product Specification, NVMC), an external erase and
/// the page programs of a sector (ZD25WQ16B datasheet, Table 4-3,
/// typical), and a page each way over the bus.
pub const PAGE_US: u64 = INTERNAL_ERASE_US as u64
    + (PAGE / 4 * INTERNAL_WORD_US) as u64
    + ERASE_TYP_US as u64
    + (PAGE / PROGRAM_PAGE * PROGRAM_TYP_US) as u64
    + bus_us(2 * PAGE as u64);

/// The longest the loader waits for the 32.768 kHz clock to start
/// (`lfclk::START_MS`).
pub const CLOCK_START_US: u64 = 1_000_000;

#[cfg(feature = "lfclk")]
const _: () = assert!(CLOCK_START_US == crate::lfclk::START_MS as u64 * 1000);

/// The swap of an image of `length` bytes, µs: every page of the slot,
/// which embassy-boot swaps whole whatever the length, the image's bytes
/// read over the bus for its hash, and the clock's start. 51.6 s to 59.0 s;
/// the bench measured 55 s (roadmap, Phase 2, "A swap").
pub const fn swap_us(length: u32) -> u64 {
    (ACTIVE.len / PAGE) as u64 * PAGE_US + bus_us(length as u64) + CLOCK_START_US
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_swap_from_the_loaders_figures() {
        assert_eq!(PAGE_US, 85_000 + 41_984 + 10_000 + 20_800 + 65_250);
        assert_eq!(swap_us(0) / 100_000, 516);
        assert_eq!(swap_us(ACTIVE.len) / 100_000, 590);
    }
}
