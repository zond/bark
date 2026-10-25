//! Both sides of a swap, on any flash: what the loader does at each boot
//! ([`prepare`]), and what the application does to stage an image
//! ([`Stager`]) and to keep the one it runs ([`confirm`]).
//!
//! The swap is embassy-boot's (`embassy_boot::BootLoader::prepare_boot`):
//! it swaps [`ACTIVE`] and [`DFU`] a page at a time, each copy into a page
//! whose content is held in another, and marks each copy done in the state
//! page only once it is whole, so a power cut at any point is taken up
//! again from the copy it stopped, and the swap ends with each image whole
//! in the other's slot. If the new image has not marked itself booted by
//! the next boot, the same steps run backwards (its documentation of
//! `prepare_boot`, "SWAPPING" and "REVERTING"). This module adds the check
//! of the staged image against the header in [`RECORD`] before the swap
//! starts.

use core::cell::RefCell;

use embassy_boot::{
    AlignedBuffer, BlockingFirmwareState, BootError, BootLoader, BootLoaderConfig,
    FirmwareUpdaterError, State,
};
use embassy_embedded_hal::flash::partition::BlockingPartition;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embedded_storage::nor_flash::{NorFlash, NorFlashErrorKind, ReadNorFlash};
use sha2::{Digest, Sha256};

use crate::layout::{Region, ACTIVE, DFU, PAGE, RECORD, STATE};
use crate::record::{CHECKED, CHECKED_AT, HEADER_AT};
use crate::{Header, HeaderError, HASH_LEN, HEADER_LEN};

/// The flash, shared by the partitions that each see one region of it.
pub type Shared<F> = Mutex<NoopRawMutex, RefCell<F>>;

type Part<'a, F> = BlockingPartition<'a, NoopRawMutex, F>;

fn part<F: NorFlash>(flash: &Shared<F>, r: Region) -> Part<'_, F> {
    BlockingPartition::new(flash, r.start, r.len)
}

/// The flash failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlashError(pub NorFlashErrorKind);

impl From<BootError> for FlashError {
    fn from(e: BootError) -> Self {
        match e {
            BootError::Flash(kind) => FlashError(kind),
            BootError::BadMagic => FlashError(NorFlashErrorKind::Other),
        }
    }
}

impl From<FirmwareUpdaterError> for FlashError {
    fn from(e: FirmwareUpdaterError) -> Self {
        match e {
            FirmwareUpdaterError::Flash(kind) => FlashError(kind),
            _ => FlashError(NorFlashErrorKind::Other),
        }
    }
}

fn flash_error<E: embedded_storage::nor_flash::NorFlashError>(e: E) -> FlashError {
    FlashError(e.kind())
}

/// The state page's word, as embassy-boot reads it.
fn state<F: NorFlash>(flash: &Shared<F>) -> Result<State, FlashError> {
    let mut word = AlignedBuffer([0u8; 4]);
    Ok(BlockingFirmwareState::new(part(flash, STATE), &mut word.0).get_state()?)
}

/// Whether a swap is asked for, under way, or to be undone: the boots on
/// which the loader arms the watchdog.
pub fn swap_requested<F: NorFlash>(flash: &Shared<F>) -> Result<bool, FlashError> {
    Ok(state(flash)? == State::Swap)
}

/// What the loader did at this boot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Boot {
    /// Nothing: the running image boots.
    Normal,
    /// Swapped in the staged image, which must now confirm itself.
    Trial,
    /// Swapped the previous image back, the new one not having confirmed.
    Reverted,
    /// Refused the staged image and boots the running one.
    Refused(Refusal),
}

/// Why the loader refused a staged image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The record holds no header the loader can read.
    Header(HeaderError),
    /// The staged image does not hash to the record's hash.
    Hash,
}

/// The loader's work at a boot: check a staged image against its record,
/// then let embassy-boot swap, go on with a swap a power cut stopped, or
/// swap back. `page` is the copy buffer, one page.
pub fn prepare<F: NorFlash>(
    flash: &Shared<F>,
    page: &mut [u8; PAGE as usize],
) -> Result<Boot, FlashError> {
    let before = state(flash)?;
    if before == State::Swap {
        if let Err(why) = check(flash, page)? {
            let mut word = AlignedBuffer([0u8; 4]);
            BlockingFirmwareState::new(part(flash, STATE), &mut word.0).mark_booted()?;
            return Ok(Boot::Refused(why));
        }
    }
    let config = BootLoaderConfig {
        active: part(flash, ACTIVE),
        dfu: part(flash, DFU),
        state: part(flash, STATE),
    };
    BootLoader::new(config).prepare_boot(page)?;
    Ok(match (before, state(flash)?) {
        (State::Swap, State::Swap) => Boot::Trial,
        (State::Swap, _) => Boot::Reverted,
        _ => Boot::Normal,
    })
}

/// Whether the staged image may be swapped in: it was checked before, or
/// it hashes to its record's hash now, which is then noted.
fn check<F: NorFlash>(
    flash: &Shared<F>,
    page: &mut [u8; PAGE as usize],
) -> Result<Result<(), Refusal>, FlashError> {
    let mut record = part(flash, RECORD);
    let mut word = [0u8; 4];
    record.read(CHECKED_AT, &mut word).map_err(flash_error)?;
    if u32::from_le_bytes(word) == CHECKED {
        return Ok(Ok(()));
    }
    let mut bytes = [0u8; HEADER_LEN];
    record.read(HEADER_AT, &mut bytes).map_err(flash_error)?;
    let header = match Header::decode(&bytes) {
        Ok(h) => h,
        Err(e) => return Ok(Err(Refusal::Header(e))),
    };
    if hash(flash, DFU, header.length, page)? != header.hash {
        return Ok(Err(Refusal::Hash));
    }
    record
        .write(CHECKED_AT, &CHECKED.to_le_bytes())
        .map_err(flash_error)?;
    Ok(Ok(()))
}

/// SHA-256 of the first `length` bytes of `region`, read through `buf`.
fn hash<F: NorFlash>(
    flash: &Shared<F>,
    region: Region,
    length: u32,
    buf: &mut [u8],
) -> Result<[u8; HASH_LEN], FlashError> {
    let mut slot = part(flash, region);
    let mut digest = Sha256::new();
    let mut at = 0;
    while at < length {
        let n = buf.len().min((length - at) as usize);
        slot.read(at, &mut buf[..n]).map_err(flash_error)?;
        digest.update(&buf[..n]);
        at += n as u32;
    }
    Ok(digest.finalize().into())
}

/// What the running image found at its start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Started {
    /// An ordinary boot.
    Normal,
    /// The first boot of a new image, now confirmed: the loader will not
    /// swap it back.
    Confirmed,
    /// The previous image is back: the new one did not confirm itself.
    Reverted,
}

/// Mark the running image as the one to keep. Called once the node has
/// started; until then a reset swaps the previous image back.
pub fn confirm<F: NorFlash>(flash: &Shared<F>) -> Result<Started, FlashError> {
    let found = state(flash)?;
    if found == State::Boot {
        return Ok(Started::Normal);
    }
    let mut word = AlignedBuffer([0u8; 4]);
    BlockingFirmwareState::new(part(flash, STATE), &mut word.0).mark_booted()?;
    Ok(match found {
        State::Swap => Started::Confirmed,
        State::Revert => Started::Reverted,
        _ => Started::Normal,
    })
}

/// Bytes [`Stager::write`] takes at once.
pub const CHUNK: usize = 256;

/// Why staging stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageError {
    /// Not a header this firmware can stage.
    Header(HeaderError),
    /// Not above the running image's version.
    Version {
        /// The running image's.
        running: u32,
        /// The header's.
        staged: u32,
    },
    /// A swap is asked for or the running image is on trial: confirm or
    /// reboot first.
    Busy,
    /// A chunk not where the last one ended, or past the image's end.
    Offset {
        /// Where the next chunk must start.
        expected: u32,
    },
    /// A chunk longer than [`CHUNK`], or not whole words before the end.
    Chunk,
    /// Asked to finish before every byte came.
    Incomplete {
        /// Bytes so far.
        got: u32,
    },
    /// The staged bytes do not hash to the header's hash.
    Hash,
    /// The flash failed.
    Flash(FlashError),
}

impl From<FlashError> for StageError {
    fn from(e: FlashError) -> Self {
        StageError::Flash(e)
    }
}

/// An image being written into [`DFU`], in order, a chunk at a time.
pub struct Stager<'a, F: NorFlash> {
    flash: &'a Shared<F>,
    header: Header,
    next: u32,
}

impl<'a, F: NorFlash> Stager<'a, F> {
    /// Start staging the image `header` describes, above the running
    /// image's `running` version. Forgets any image staged before.
    pub fn begin(
        flash: &'a Shared<F>,
        header: &[u8; HEADER_LEN],
        running: u32,
    ) -> Result<Self, StageError> {
        let header = Header::decode(header).map_err(StageError::Header)?;
        if header.version <= running {
            return Err(StageError::Version {
                running,
                staged: header.version,
            });
        }
        if state(flash)? == State::Swap {
            return Err(StageError::Busy);
        }
        part(flash, RECORD)
            .erase(0, RECORD.len)
            .map_err(flash_error)?;
        Ok(Stager {
            flash,
            header,
            next: 0,
        })
    }

    /// Where the next chunk starts.
    pub fn next(&self) -> u32 {
        self.next
    }

    /// The header being staged.
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// Write the chunk at `offset`, erasing each page as the image enters
    /// it. Every chunk but the last is whole words.
    pub fn write(&mut self, offset: u32, data: &[u8]) -> Result<u32, StageError> {
        if offset != self.next || offset + data.len() as u32 > self.header.length {
            return Err(StageError::Offset {
                expected: self.next,
            });
        }
        let end = offset + data.len() as u32;
        let last = end == self.header.length;
        if data.len() > CHUNK || (!last && !data.len().is_multiple_of(4)) {
            return Err(StageError::Chunk);
        }
        let mut dfu = part(self.flash, DFU);
        let first_new_page = offset.div_ceil(PAGE) * PAGE;
        for page in (first_new_page..end).step_by(PAGE as usize) {
            dfu.erase(page, page + PAGE).map_err(flash_error)?;
        }
        // The NVMC writes whole words; the image's last word is padded
        // with the erased value, as the UF2 pads the flash.
        let mut words = [0xffu8; CHUNK];
        words[..data.len()].copy_from_slice(data);
        let padded = data.len().next_multiple_of(4);
        dfu.write(offset, &words[..padded]).map_err(flash_error)?;
        self.next = end;
        Ok(end)
    }

    /// Check the staged bytes against the header, record it for the
    /// loader and ask for the swap at the next boot.
    pub fn finish(self) -> Result<Header, StageError> {
        if self.next != self.header.length {
            return Err(StageError::Incomplete { got: self.next });
        }
        let mut buf = [0u8; CHUNK];
        if hash(self.flash, DFU, self.header.length, &mut buf)? != self.header.hash {
            return Err(StageError::Hash);
        }
        part(self.flash, RECORD)
            .write(HEADER_AT, &self.header.encode())
            .map_err(flash_error)?;
        let mut word = AlignedBuffer([0u8; 4]);
        BlockingFirmwareState::new(part(self.flash, STATE), &mut word.0)
            .mark_updated()
            .map_err(FlashError::from)?;
        Ok(self.header)
    }
}

#[cfg(test)]
mod tests;
