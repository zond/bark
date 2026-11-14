//! The page [`GIVEN_UP`]: the versions of the images this node swapped
//! back, written by [`crate::boot::start`] (spec "Updates over the mesh"),
//! and erased by the bench build's `forget` (`bark-fw/README.md`, "Bench
//! build").
//!
//! Each version is a little-endian word appended in the first erased slot.
//! A full page is erased and starts again with the newest: each version
//! costs a whole gathering and a swap back, so the page outlasts what a
//! node meets, and a version given up long ago is below the running one,
//! which refuses it anyway.

use embedded_storage::nor_flash::{NorFlash, ReadNorFlash};
use embedded_storage_async::nor_flash::NorFlash as AsyncNorFlash;

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

/// Give up the image of `version`. Appends it only if absent, as a boot
/// cut short before it confirmed its image writes it again.
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

/// Forget every version given up: the page erased. `page` is [`GIVEN_UP`]
/// as a flash that may wait, as a firmware writes its internal flash
/// between radio events.
pub async fn clear<F: AsyncNorFlash>(mut page: F) -> Result<(), FlashError> {
    page.erase(0, GIVEN_UP.len).await.map_err(flash_error)
}
