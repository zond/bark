//! The page [`GIVEN_UP`]: the names of the images this node swapped back
//! (spec "Updates over the mesh", the install). The image that runs again
//! after a swap back writes the name of the one that failed
//! ([`crate::boot::start`]), and the node never gathers an image of that
//! name again, so it never serves it either; a release after it is
//! wanted as usual.
//!
//! A name is the whole SHA-256 of the image's signed header
//! ([`crate::pieces::name`]), 32 bytes, appended in the first erased slot
//! of the page, 128 slots. The mesh names an image by its first
//! [`TAG_LEN`] bytes, so an offer is matched on those. A full page is
//! erased and starts again with the newest: each name costs a whole
//! gathering and a swap back, so 128 is more than a node meets, and an
//! image given up long ago is older than the one the node runs, which
//! its version already refuses.

use embedded_storage::nor_flash::{NorFlash, ReadNorFlash};

use crate::boot::{flash_error, part, FlashError, Shared};
use crate::layout::GIVEN_UP;
use crate::pieces::TAG_LEN;
use crate::HASH_LEN;

/// Bytes of a slot: one name.
const SLOT: u32 = HASH_LEN as u32;

/// Slots in the page.
pub const SLOTS: u32 = GIVEN_UP.len / SLOT;

/// The erased value of the flash (nRF52840 Product Specification, NVMC).
const ERASED: u8 = 0xff;

/// The name in slot `i`, `None` once the slots in use end.
fn slot<F: NorFlash>(flash: &Shared<F>, i: u32) -> Result<Option<[u8; HASH_LEN]>, FlashError> {
    let mut name = [0u8; HASH_LEN];
    part(flash, GIVEN_UP)
        .read(i * SLOT, &mut name)
        .map_err(flash_error)?;
    Ok((!name.iter().all(|&b| b == ERASED)).then_some(name))
}

/// Whether the node gave up an image whose name begins with `tag`: the
/// whole name, or the [`TAG_LEN`] bytes the mesh carries.
pub fn contains<F: NorFlash>(flash: &Shared<F>, tag: &[u8]) -> Result<bool, FlashError> {
    debug_assert!(tag.len() == TAG_LEN || tag.len() == HASH_LEN);
    for i in 0..SLOTS {
        match slot(flash, i)? {
            Some(name) if name[..tag.len()] == *tag => return Ok(true),
            Some(_) => {}
            None => return Ok(false),
        }
    }
    Ok(false)
}

/// Give up the image named `name`: append it, unless it is there already,
/// so that a boot that writes it twice, cut short before it kept the
/// image it runs, writes it once.
pub fn add<F: NorFlash>(flash: &Shared<F>, name: &[u8; HASH_LEN]) -> Result<(), FlashError> {
    let mut free = None;
    for i in 0..SLOTS {
        match slot(flash, i)? {
            Some(n) if n == *name => return Ok(()),
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
    page.write(at, name).map_err(flash_error)
}
