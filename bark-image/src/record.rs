//! The page [`crate::layout::RECORD`]: the header of the image staged in
//! [`crate::layout::external::DFU`], which the application writes before it asks
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

/// Where a bench loader's words sit (`crate::boot::cut`): for the swap,
/// then for the swap back, the moment it drew and where the copy was
/// stopped, a word each. A loader built without the bench leaves them
/// erased.
pub const CUTS_AT: u32 = CHECKED_AT + 4;
