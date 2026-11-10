//! The page [`GIVEN_UP`]: the versions of the images this node swapped
//! back (spec "Updates over the mesh", the install). The image that runs
//! again after a swap back writes the version of the one that failed
//! ([`crate::boot::start`]), and the node never gathers an image of that
//! version again, so it never serves it either; a release after it has a
//! higher version and is wanted as usual. A version names one image, so
//! the version is what the mesh names an image by.
//!
//! A version is a word, little-endian, appended in the first erased slot
//! of the page, 1 024 slots. A full page is erased and starts again with
//! the newest: each version costs a whole gathering and a swap back, so
//! 1 024 is more than a node meets, and an image given up long ago is
//! older than the one the node runs, which its version already refuses.

use embedded_storage::nor_flash::{NorFlash, ReadNorFlash};

use crate::boot::{flash_error, part, FlashError, Shared};
use crate::layout::GIVEN_UP;

/// Bytes of a slot: one version.
const SLOT: u32 = 4;

/// Slots in the page.
pub const SLOTS: u32 = GIVEN_UP.len / SLOT;

/// The erased value of a slot (nRF52840 Product Specification, NVMC): no
/// version, as no release is numbered 2³² − 1.
const ERASED: u32 = u32::MAX;

/// The version in slot `i`, `None` once the slots in use end.
fn slot<F: NorFlash>(flash: &Shared<F>, i: u32) -> Result<Option<u32>, FlashError> {
    let mut word = [0u8; SLOT as usize];
    part(flash, GIVEN_UP)
        .read(i * SLOT, &mut word)
        .map_err(flash_error)?;
    let version = u32::from_le_bytes(word);
    Ok((version != ERASED).then_some(version))
}

/// Whether the node gave up the image of `version`.
pub fn contains<F: NorFlash>(flash: &Shared<F>, version: u32) -> Result<bool, FlashError> {
    for i in 0..SLOTS {
        match slot(flash, i)? {
            Some(v) if v == version => return Ok(true),
            Some(_) => {}
            None => return Ok(false),
        }
    }
    Ok(false)
}

/// Give up the image of `version`: append it, unless it is there
/// already, so that a boot that writes it twice, cut short before it kept
/// the image it runs, writes it once.
pub fn add<F: NorFlash>(flash: &Shared<F>, version: u32) -> Result<(), FlashError> {
    let mut free = None;
    for i in 0..SLOTS {
        match slot(flash, i)? {
            Some(v) if v == version => return Ok(()),
            Some(_) => {}
            None => {
                free = Some(i);
                break;
            }
        }
    }
    let mut page = part(flash, GIVEN_UP);
    let at = match free {
        Some(i) => i * SLOT,
        None => {
            page.erase(0, GIVEN_UP.len).map_err(flash_error)?;
            0
        }
    };
    page.write(at, &version.to_le_bytes()).map_err(flash_error)
}
