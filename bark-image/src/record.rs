//! The page [`crate::layout::RECORD`]: the header of the image staged in
//! [`crate::layout::DFU`], which the application writes before it asks
//! for the swap, and a word the loader writes once the image matches it.
//!
//! The word lets the loader tell a swap it has yet to start, whose image
//! it must check, from one a power cut stopped half way, whose staged slot
//! already holds half of each image and no longer matches anything.

use crate::HEADER_LEN;

/// Where the header starts in the page.
pub const HEADER_AT: u32 = 0;

/// Where the loader's word sits: right after the header.
pub const CHECKED_AT: u32 = HEADER_LEN as u32;

/// The word once the loader has checked the image: every bit cleared, so
/// that a write a power cut stopped part way, which clears only some,
/// reads as unchecked, and the image is checked again.
pub const CHECKED: u32 = 0;
