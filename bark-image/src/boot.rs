//! Both sides of a swap, on any two flashes: what the loader does at each
//! boot ([`prepare`]), and what the application does to stage an image
//! ([`Stager`]) and to keep the one it runs ([`start`], [`confirm`]). The internal
//! flash holds the running image, the loader's state and the record; the
//! external one the staged image (`crate::layout`).
//!
//! The swap is embassy-boot's (`embassy_boot::BootLoader::prepare_boot`):
//! it swaps [`ACTIVE`] and [`DFU`] a page at a time, each copy into a page
//! whose content is held in another, and marks each copy done in the state
//! page only once it is whole, so a power cut at any point is taken up
//! again from the copy it stopped, and the swap ends with each image whole
//! in the other's slot. If the new image has not marked itself booted by
//! the next boot, the same steps run backwards (its documentation of
//! `prepare_boot`, "SWAPPING" and "REVERTING"). Its partitions may lie on
//! different flashes, each page of one copied into a sector of the other.
//! This module adds the check of the staged image against the header in
//! [`RECORD`] before the swap starts.

use core::cell::RefCell;

use embassy_boot::{
    AlignedBuffer, BlockingFirmwareState, BootError, BootLoader, BootLoaderConfig, FirmwareState,
    FirmwareUpdaterError, State,
};
use embassy_embedded_hal::adapter::BlockingAsync;
use embassy_embedded_hal::flash::partition::BlockingPartition;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embedded_storage::nor_flash::{NorFlash, NorFlashErrorKind, ReadNorFlash};
use embedded_storage_async::nor_flash::NorFlash as AsyncNorFlash;
use sha2::{Digest, Sha256};

use crate::layout::external::DFU;
use crate::layout::{Region, ACTIVE, PAGE, RECORD, STATE};
use crate::record::{CHECKED, CHECKED_AT, FROM_MESH, HEADER_AT, ORIGIN_AT};
use crate::sign::{self, Key, SignatureError, SLOTS};
use crate::{Header, HeaderError, HASH_LEN, HEADER_LEN, MAGIC};

/// The flash, shared by the partitions that each see one region of it.
pub type Shared<F> = Mutex<NoopRawMutex, RefCell<F>>;

/// One region of a shared blocking flash.
pub type Part<'a, F> = BlockingPartition<'a, NoopRawMutex, F>;

pub(crate) fn part<F: NorFlash>(flash: &Shared<F>, r: Region) -> Part<'_, F> {
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

pub(crate) fn flash_error<E: embedded_storage::nor_flash::NorFlashError>(e: E) -> FlashError {
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
    /// The external flash did not answer, so the staged image cannot be
    /// read.
    Chip,
    /// The record holds no header the loader can read.
    Header(HeaderError),
    /// The staged image does not hash to the record's hash.
    Hash,
}

/// The loader's work at a boot: check a staged image against its record,
/// then let embassy-boot swap, go on with a swap a power cut stopped, or
/// swap back. `external` is the external flash, `None` if it failed its
/// presence check: a swap not yet started is then cancelled, one under
/// way fails, to go on at a boot that finds the chip. `page` is the copy
/// buffer, one page.
pub fn prepare<I: NorFlash, E: NorFlash>(
    internal: &Shared<I>,
    external: Option<&Shared<E>>,
    page: &mut [u8; PAGE as usize],
) -> Result<Boot, FlashError> {
    prepare_with(internal, external, page, |_| Ok(()))
}

/// [`prepare`], with `before_copy` run once the staged image has passed
/// its check, just before embassy-boot copies (`cut`).
fn prepare_with<I: NorFlash, E: NorFlash>(
    internal: &Shared<I>,
    external: Option<&Shared<E>>,
    page: &mut [u8; PAGE as usize],
    before_copy: impl FnOnce(&Shared<I>) -> Result<(), FlashError>,
) -> Result<Boot, FlashError> {
    let before = state(internal)?;
    if before != State::Swap {
        return Ok(Boot::Normal);
    }
    let external = match external {
        Some(external) => external,
        None if was_checked(internal)? => return Err(FlashError(NorFlashErrorKind::Other)),
        None => return refuse(internal, Refusal::Chip),
    };
    if let Err(why) = check(internal, external, page)? {
        return refuse(internal, why);
    }
    before_copy(internal)?;
    let config = BootLoaderConfig {
        active: part(internal, ACTIVE),
        dfu: part(external, DFU),
        state: part(internal, STATE),
    };
    BootLoader::new(config).prepare_boot(page)?;
    Ok(match state(internal)? {
        State::Swap => Boot::Trial,
        _ => Boot::Reverted,
    })
}

/// Cancel the swap: the running image boots, and finds the record it
/// left unchecked (`confirm`).
fn refuse<I: NorFlash>(internal: &Shared<I>, why: Refusal) -> Result<Boot, FlashError> {
    let mut word = AlignedBuffer([0u8; 4]);
    BlockingFirmwareState::new(part(internal, STATE), &mut word.0).mark_booted()?;
    Ok(Boot::Refused(why))
}

/// Whether the record notes its image checked: a swap has started.
fn was_checked<I: NorFlash>(internal: &Shared<I>) -> Result<bool, FlashError> {
    let mut word = [0u8; 4];
    part(internal, RECORD)
        .read(CHECKED_AT, &mut word)
        .map_err(flash_error)?;
    Ok(u32::from_le_bytes(word) == CHECKED)
}

/// Whether the staged image may be swapped in: it was checked before, or
/// it hashes to its record's hash now, which is then noted.
fn check<I: NorFlash, E: NorFlash>(
    internal: &Shared<I>,
    external: &Shared<E>,
    page: &mut [u8; PAGE as usize],
) -> Result<Result<(), Refusal>, FlashError> {
    if was_checked(internal)? {
        return Ok(Ok(()));
    }
    let mut record = part(internal, RECORD);
    let mut bytes = [0u8; HEADER_LEN];
    record.read(HEADER_AT, &mut bytes).map_err(flash_error)?;
    let header = match Header::decode(&bytes) {
        Ok(h) => h,
        Err(e) => return Ok(Err(Refusal::Header(e))),
    };
    if hash(external, DFU, header.length, page)? != header.hash {
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
    /// The first boot of a new image a person staged, now confirmed: the
    /// loader will not swap it back.
    Confirmed,
    /// The first boot of a new image gathered over the mesh, not yet
    /// confirmed: it keeps itself only once it hears a node ([`confirm`]),
    /// and a reset before then swaps the previous image back (spec
    /// "Updates over the mesh", the install).
    OnTrial,
    /// The previous image is back: the new one did not confirm itself,
    /// and is given up ([`crate::given_up`]).
    Reverted {
        /// The version given up, if the record still names it.
        gave_up: Option<u32>,
    },
    /// The loader refused the staged image (`Boot::Refused`), whose record
    /// is now forgotten.
    Refused,
}

/// What the running image does at its start, once the node has started:
/// keep itself if a person staged it, or wait to hear a node if it came
/// over the mesh; after a swap back, give up the image that failed and
/// keep the one it runs. Only the internal flash takes part.
pub fn start<F: NorFlash>(flash: &Shared<F>) -> Result<Started, FlashError> {
    let found = state(flash)?;
    let mut record = part(flash, RECORD);
    let mut bytes = [0u8; HEADER_LEN];
    record.read(HEADER_AT, &mut bytes).map_err(flash_error)?;
    let word = |record: &mut Part<'_, F>, at| -> Result<u32, FlashError> {
        let mut w = [0u8; 4];
        record.read(at, &mut w).map_err(flash_error)?;
        Ok(u32::from_le_bytes(w))
    };
    if found == State::Boot {
        // A record never checked, with no swap asked for, is one the
        // loader refused: it cancels the swap before it checks anything
        // else.
        if bytes[..MAGIC.len()] == MAGIC && word(&mut record, CHECKED_AT)? != CHECKED {
            record.erase(0, RECORD.len).map_err(flash_error)?;
            return Ok(Started::Refused);
        }
        return Ok(Started::Normal);
    }
    let started = match found {
        State::Swap if word(&mut record, ORIGIN_AT)? == FROM_MESH => return Ok(Started::OnTrial),
        State::Swap => Started::Confirmed,
        State::Revert => {
            // Before the image it runs is kept, so that a reset between
            // the two gives it up at the next start all the same.
            let gave_up = match Header::decode(&bytes) {
                Ok(h) => {
                    crate::given_up::add(flash, h.version)?;
                    Some(h.version)
                }
                Err(_) => None,
            };
            Started::Reverted { gave_up }
        }
        _ => Started::Normal,
    };
    let mut word = AlignedBuffer([0u8; 4]);
    BlockingFirmwareState::new(part(flash, STATE), &mut word.0).mark_booted()?;
    Ok(started)
}

/// Keep the running image, on trial since [`start`] said
/// [`Started::OnTrial`]: it has heard a node. `state` is the loader's
/// [`STATE`] page, a flash that may wait, so that a firmware that writes
/// its internal flash between radio events confirms with the same steps.
/// Whether there was a trial to end.
pub async fn confirm<S: AsyncNorFlash>(mut state: S) -> Result<bool, FlashError> {
    let mut word = AlignedBuffer([0u8; 4]);
    let mut s = FirmwareState::new(&mut state, &mut word.0);
    if s.get_state().await.map_err(FlashError::from)? != State::Swap {
        return Ok(false);
    }
    s.mark_booted().await.map_err(FlashError::from)?;
    Ok(true)
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
    /// No signature in the header verifies under a key the running image
    /// holds.
    Signature(SignatureError),
    /// A swap is asked for or the running image is on trial: confirm or
    /// reboot first.
    Busy,
    /// A chunk not where the last one ended, or past the image's end.
    Offset {
        /// Where the next chunk must start.
        expected: u32,
    },
    /// A chunk longer than [`CHUNK`], or, before the end, not whole
    /// writes of the external flash.
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

/// A region of a blocking flash as staging takes it: its operations end
/// before they return, so each future is ready at its first poll.
pub type Blocking<'a, F> = BlockingAsync<Part<'a, F>>;

/// The record, the state page and the staged slot on two blocking
/// flashes, for [`Stager::begin`].
pub fn blocking<'a, I: NorFlash, E: NorFlash>(
    internal: &'a Shared<I>,
    external: &'a Shared<E>,
) -> (Blocking<'a, I>, Blocking<'a, I>, Blocking<'a, E>) {
    (
        BlockingAsync::new(part(internal, RECORD)),
        BlockingAsync::new(part(internal, STATE)),
        BlockingAsync::new(part(external, DFU)),
    )
}

/// How a staged image came, which decides how it keeps itself at its
/// first boot ([`start`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// A person staged it, from the phone or over USB, and is there to see
    /// whether it works: it keeps itself at once, and the app warns if the
    /// node then hears no one (spec "Phone link", Updates).
    Person,
    /// It was gathered over the mesh, so the node had a neighbour to take
    /// it from: it keeps itself only once it hears a node.
    Mesh,
}

/// An image being written into [`DFU`], in order, a chunk at a time.
///
/// It takes its three regions as flashes that may wait, so that a
/// firmware whose internal flash is written between radio events, and
/// whose external flash leaves the processor free while it erases, stages
/// with the same steps as one that blocks ([`blocking`]): `record` is
/// [`RECORD`], `state` the loader's [`STATE`] page, `dfu` the staged slot,
/// each addressed from its start.
pub struct Stager<R, S, D> {
    record: R,
    state: S,
    dfu: D,
    origin: Origin,
    header: Header,
    /// The header as it came, its signatures among it: what the record
    /// keeps, so that the image swapped in still names the key that
    /// signed it.
    signed: [u8; HEADER_LEN],
    next: u32,
}

impl<R: AsyncNorFlash, S: AsyncNorFlash, D: AsyncNorFlash> Stager<R, S, D> {
    /// Start staging the image the header `bytes` describes, above the
    /// running image's `running` version and signed by one of its
    /// `keys`. Forgets any image staged before.
    pub async fn begin(
        mut record: R,
        mut state: S,
        dfu: D,
        bytes: &[u8; HEADER_LEN],
        running: u32,
        keys: &[Key; SLOTS],
    ) -> Result<Self, StageError> {
        let header = Header::decode(bytes).map_err(StageError::Header)?;
        if header.version <= running {
            return Err(StageError::Version {
                running,
                staged: header.version,
            });
        }
        // The signature covers the hash, which `finish` checks against
        // the staged bytes, so a refusal here spares sending the image.
        sign::verify(bytes, keys).map_err(StageError::Signature)?;
        let mut word = AlignedBuffer([0u8; 4]);
        let now = FirmwareState::new(&mut state, &mut word.0)
            .get_state()
            .await
            .map_err(FlashError::from)?;
        if now == State::Swap {
            return Err(StageError::Busy);
        }
        record.erase(0, RECORD.len).await.map_err(flash_error)?;
        Ok(Stager {
            record,
            state,
            dfu,
            origin: Origin::Person,
            header,
            signed: *bytes,
            next: 0,
        })
    }

    /// [`Stager::begin`] for an image already written into the staged
    /// slot by other means, the mesh's store (`crate::pieces`), with the
    /// same checks; [`Stager::finish`] then hashes it, records it as
    /// gathered over the mesh ([`Origin::Mesh`]) and asks for the swap.
    pub async fn written(
        record: R,
        state: S,
        dfu: D,
        bytes: &[u8; HEADER_LEN],
        running: u32,
        keys: &[Key; SLOTS],
    ) -> Result<Self, StageError> {
        let mut s = Self::begin(record, state, dfu, bytes, running, keys).await?;
        s.next = s.header.length;
        s.origin = Origin::Mesh;
        Ok(s)
    }

    /// Where the next chunk starts.
    pub fn next(&self) -> u32 {
        self.next
    }

    /// The header being staged.
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// Write the chunk at `offset`, erasing each sector as the image
    /// enters it.
    pub async fn write(&mut self, offset: u32, data: &[u8]) -> Result<u32, StageError> {
        if offset != self.next || offset + data.len() as u32 > self.header.length {
            return Err(StageError::Offset {
                expected: self.next,
            });
        }
        let end = offset + data.len() as u32;
        let last = end == self.header.length;
        if data.len() > CHUNK || (!last && !data.len().is_multiple_of(D::WRITE_SIZE)) {
            return Err(StageError::Chunk);
        }
        let first_new_page = offset.div_ceil(PAGE) * PAGE;
        for page in (first_new_page..end).step_by(PAGE as usize) {
            self.dfu
                .erase(page, page + PAGE)
                .await
                .map_err(flash_error)?;
        }
        // A flash that writes more than a byte at once has the image's
        // last write padded with the erased value, as the UF2 pads the
        // internal flash.
        let mut padded = [0xffu8; CHUNK];
        padded[..data.len()].copy_from_slice(data);
        let len = data.len().next_multiple_of(D::WRITE_SIZE);
        self.dfu
            .write(offset, &padded[..len])
            .await
            .map_err(flash_error)?;
        self.next = end;
        Ok(end)
    }

    /// Check the staged bytes against the header, record it for the
    /// loader and ask for the swap at the next boot.
    pub async fn finish(mut self) -> Result<Header, StageError> {
        if self.next != self.header.length {
            return Err(StageError::Incomplete { got: self.next });
        }
        let mut digest = Sha256::new();
        let mut buf = [0u8; CHUNK];
        let mut at = 0;
        while at < self.header.length {
            let n = buf.len().min((self.header.length - at) as usize);
            self.dfu
                .read(at, &mut buf[..n])
                .await
                .map_err(flash_error)?;
            digest.update(&buf[..n]);
            at += n as u32;
        }
        if <[u8; HASH_LEN]>::from(digest.finalize()) != self.header.hash {
            return Err(StageError::Hash);
        }
        self.record
            .write(HEADER_AT, &self.signed)
            .await
            .map_err(flash_error)?;
        if self.origin == Origin::Mesh {
            self.record
                .write(ORIGIN_AT, &FROM_MESH.to_le_bytes())
                .await
                .map_err(flash_error)?;
        }
        let mut word = AlignedBuffer([0u8; 4]);
        FirmwareState::new(&mut self.state, &mut word.0)
            .mark_updated()
            .await
            .map_err(FlashError::from)?;
        Ok(self.header)
    }
}

pub mod cut;
pub mod time;

#[cfg(test)]
mod tests;
