//! The page [`crate::layout::RECORD`]: the header of the image staged in
//! [`crate::layout::external::DFU`], written before the swap is asked for,
//! the loader's word once the image matches it, and how the image came.
//!
//! The loader's word tells a swap yet to start, whose image it must check,
//! from one a power cut stopped half way, whose staged slot holds half of
//! each image and matches nothing.

use crate::HEADER_LEN;

/// Where the header starts in the page.
pub const HEADER_AT: u32 = 0;

/// Where the loader's word sits.
pub const CHECKED_AT: u32 = HEADER_LEN as u32;

/// The word once the loader has checked the image: every bit cleared, so
/// that a write a power cut stopped part way, which clears only some,
/// reads as unchecked, and the image is checked again.
pub const CHECKED: u32 = 0;

/// Where a bench loader's words sit (`crate::boot::cut`): for the swap,
/// then the swap back, the moment it drew and where the copy stopped. A
/// loader built without the bench leaves them erased.
pub const CUTS_AT: u32 = CHECKED_AT + 4;

/// Bytes of a bench loader's words: two for each of its two phases.
pub const CUTS_LEN: u32 = 16;

/// Where the word that says how the image came sits. Erased, a person
/// staged it and it confirms itself at its start; [`FROM_MESH`], it was
/// gathered over the mesh and confirms itself once it hears a node (spec
/// "Updates over the mesh").
pub const ORIGIN_AT: u32 = CUTS_AT + CUTS_LEN;

/// The word at [`ORIGIN_AT`] for an image gathered over the mesh, all bits
/// cleared as [`CHECKED`]. Written before the swap is asked for, so a write
/// a power cut stopped part way never reaches a trial.
pub const FROM_MESH: u32 = 0;
