//! The swap and its check on a flash in RAM with the board's layout, and
//! power cuts at every flash operation of a staging, a swap, a swap back
//! and a confirmation.

use std::vec;
use std::vec::Vec;

use embedded_storage::nor_flash::{ErrorType, NorFlashError, NorFlashErrorKind};

use super::*;
use crate::layout::FLASH_END;

/// The power went: every later operation fails until the next boot.
#[derive(Debug)]
struct Cut;

impl NorFlashError for Cut {
    fn kind(&self) -> NorFlashErrorKind {
        NorFlashErrorKind::Other
    }
}

/// NOR flash in RAM: an erase sets a page to `0xFF`, a write can only
/// clear bits, as on the nRF52840 (Product Specification, NVMC). With a
/// budget, the operation that exhausts it is cut half way: the first half
/// of the page erased, or the first half of the words written, which is
/// one of the states a real cut can leave.
struct RamFlash {
    bytes: Vec<u8>,
    budget: Option<usize>,
    dead: bool,
}

impl RamFlash {
    fn new() -> RamFlash {
        RamFlash {
            bytes: vec![0xff; FLASH_END as usize],
            budget: None,
            dead: false,
        }
    }

    /// Whether this operation is the one the power is cut in.
    fn cut_now(&mut self) -> Result<bool, Cut> {
        if self.dead {
            return Err(Cut);
        }
        match &mut self.budget {
            Some(0) => {
                self.dead = true;
                Ok(true)
            }
            Some(n) => {
                *n -= 1;
                Ok(false)
            }
            None => Ok(false),
        }
    }
}

impl ErrorType for RamFlash {
    type Error = Cut;
}

impl ReadNorFlash for RamFlash {
    const READ_SIZE: usize = 1;

    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Cut> {
        if self.dead {
            return Err(Cut);
        }
        let at = offset as usize;
        bytes.copy_from_slice(&self.bytes[at..at + bytes.len()]);
        Ok(())
    }

    fn capacity(&self) -> usize {
        self.bytes.len()
    }
}

impl NorFlash for RamFlash {
    const WRITE_SIZE: usize = 4;
    const ERASE_SIZE: usize = PAGE as usize;

    fn erase(&mut self, from: u32, to: u32) -> Result<(), Cut> {
        assert!(from.is_multiple_of(PAGE) && to.is_multiple_of(PAGE));
        for page in (from..to).step_by(PAGE as usize) {
            let at = page as usize;
            if self.cut_now()? {
                self.bytes[at..at + PAGE as usize / 2].fill(0xff);
                return Err(Cut);
            }
            self.bytes[at..at + PAGE as usize].fill(0xff);
        }
        Ok(())
    }

    fn write(&mut self, offset: u32, data: &[u8]) -> Result<(), Cut> {
        assert!(offset.is_multiple_of(4) && data.len().is_multiple_of(4));
        let at = offset as usize;
        let n = match self.cut_now()? {
            true => data.len() / 8 * 4,
            false => data.len(),
        };
        for (cell, byte) in self.bytes[at..at + n].iter_mut().zip(data) {
            *cell &= byte;
        }
        match n == data.len() {
            true => Ok(()),
            false => Err(Cut),
        }
    }
}

/// Bytes that differ from image to image and from place to place.
fn image(seed: u32, len: usize) -> Vec<u8> {
    let mut x = seed.wrapping_mul(2_654_435_761) | 1;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect()
}

const RUNNING: u32 = 1;

/// A board running `a`, which fills its slot.
fn board(a: &[u8]) -> Shared<RamFlash> {
    let mut f = RamFlash::new();
    let at = ACTIVE.start as usize;
    f.bytes[at..at + a.len()].copy_from_slice(a);
    Mutex::new(RefCell::new(f))
}

fn header_for(b: &[u8], version: u32) -> [u8; HEADER_LEN] {
    Header {
        version,
        length: b.len() as u32,
        hash: Sha256::digest(b).into(),
    }
    .encode()
}

fn stage(flash: &Shared<RamFlash>, b: &[u8]) -> Result<Header, StageError> {
    let mut s = Stager::begin(flash, &header_for(b, RUNNING + 1), RUNNING)?;
    for (i, chunk) in b.chunks(CHUNK).enumerate() {
        s.write((i * CHUNK) as u32, chunk)?;
    }
    s.finish()
}

fn power(flash: &Shared<RamFlash>, budget: Option<usize>) {
    flash.lock(|f| {
        let mut f = f.borrow_mut();
        f.budget = budget;
        f.dead = false;
    });
}

fn spent(flash: &Shared<RamFlash>) -> bool {
    flash.lock(|f| f.borrow().dead)
}

fn boot(flash: &Shared<RamFlash>) -> Result<Boot, FlashError> {
    let mut page = [0u8; PAGE as usize];
    prepare(flash, &mut page)
}

fn active(flash: &Shared<RamFlash>, len: usize) -> Vec<u8> {
    flash.lock(|f| {
        let at = ACTIVE.start as usize;
        f.borrow().bytes[at..at + len].to_vec()
    })
}

fn a() -> Vec<u8> {
    image(1, ACTIVE.len as usize)
}

/// An odd length, so that the last chunk is not whole words.
fn b() -> Vec<u8> {
    image(2, 20_001)
}

#[test]
fn a_staged_image_is_swapped_in_and_kept_once_confirmed() {
    let (a, b) = (a(), b());
    let flash = board(&a);
    assert_eq!(boot(&flash), Ok(Boot::Normal));
    stage(&flash, &b).unwrap();
    assert_eq!(boot(&flash), Ok(Boot::Trial));
    assert_eq!(active(&flash, b.len()), b);
    assert_eq!(confirm(&flash), Ok(Started::Confirmed));
    assert_eq!(boot(&flash), Ok(Boot::Normal));
    assert_eq!(confirm(&flash), Ok(Started::Normal));
    assert_eq!(active(&flash, b.len()), b);
}

#[test]
fn an_image_that_does_not_confirm_is_swapped_back() {
    let (a, b) = (a(), b());
    let flash = board(&a);
    stage(&flash, &b).unwrap();
    assert_eq!(boot(&flash), Ok(Boot::Trial));
    assert_eq!(boot(&flash), Ok(Boot::Reverted));
    assert_eq!(active(&flash, a.len()), a);
    assert_eq!(confirm(&flash), Ok(Started::Reverted));
    assert_eq!(boot(&flash), Ok(Boot::Normal));
    assert_eq!(active(&flash, a.len()), a);
}

#[test]
fn a_staged_image_that_went_bad_is_refused() {
    let (a, b) = (a(), b());
    let flash = board(&a);
    stage(&flash, &b).unwrap();
    // One bit of the staged image lost, as flash that went bad loses it.
    flash.lock(|f| {
        let bytes = &mut f.borrow_mut().bytes;
        let at = (DFU.start as usize + 4096..)
            .find(|&i| bytes[i] != 0)
            .unwrap();
        bytes[at] &= bytes[at] - 1;
    });
    assert_eq!(boot(&flash), Ok(Boot::Refused(Refusal::Hash)));
    assert_eq!(active(&flash, a.len()), a);
    assert_eq!(boot(&flash), Ok(Boot::Normal));
    assert_eq!(confirm(&flash), Ok(Started::Normal));
}

#[test]
fn a_swap_without_a_record_is_refused() {
    let (a, b) = (a(), b());
    let flash = board(&a);
    stage(&flash, &b).unwrap();
    flash.lock(|f| {
        let at = RECORD.start as usize;
        f.borrow_mut().bytes[at..at + PAGE as usize].fill(0xff)
    });
    assert_eq!(
        boot(&flash),
        Ok(Boot::Refused(Refusal::Header(HeaderError::Magic)))
    );
    assert_eq!(active(&flash, a.len()), a);
}

#[test]
fn staging_refuses_what_it_cannot_stage() {
    let (a, b) = (a(), b());
    let flash = board(&a);
    let old = header_for(&b, RUNNING);
    assert_eq!(
        Stager::begin(&flash, &old, RUNNING).err(),
        Some(StageError::Version {
            running: RUNNING,
            staged: RUNNING
        })
    );
    let mut s = Stager::begin(&flash, &header_for(&b, RUNNING + 1), RUNNING).unwrap();
    assert_eq!(
        s.write(4, &b[4..8]),
        Err(StageError::Offset { expected: 0 })
    );
    assert_eq!(s.write(0, &b[..3]), Err(StageError::Chunk));
    s.write(0, &b[..CHUNK]).unwrap();
    assert_eq!(
        s.finish().err(),
        Some(StageError::Incomplete { got: CHUNK as u32 })
    );
    // A different image under the header.
    let mut s = Stager::begin(&flash, &header_for(&b, RUNNING + 1), RUNNING).unwrap();
    let other = image(3, b.len());
    for (i, chunk) in other.chunks(CHUNK).enumerate() {
        s.write((i * CHUNK) as u32, chunk).unwrap();
    }
    assert_eq!(s.finish().err(), Some(StageError::Hash));
    assert_eq!(boot(&flash), Ok(Boot::Normal));
    // On trial, nothing is staged until the image confirms itself.
    stage(&flash, &b).unwrap();
    assert_eq!(boot(&flash), Ok(Boot::Trial));
    assert_eq!(
        Stager::begin(&flash, &header_for(&a[..100], RUNNING + 2), RUNNING + 1).err(),
        Some(StageError::Busy)
    );
}

/// Run `step` with the power cut at its first, second, ... flash
/// operation, each time on a board `setup` made, until it runs whole;
/// `after` sees the board once the power is back. Returns the cuts made.
fn cut_everywhere(
    setup: impl Fn() -> Shared<RamFlash>,
    step: impl Fn(&Shared<RamFlash>),
    after: impl Fn(&Shared<RamFlash>, usize),
) -> usize {
    for k in 0.. {
        let flash = setup();
        power(&flash, Some(k));
        step(&flash);
        if !spent(&flash) {
            return k;
        }
        power(&flash, None);
        after(&flash, k);
    }
    unreachable!()
}

#[test]
fn a_power_cut_anywhere_in_a_swap_leaves_the_new_image_whole() {
    let (a, b) = (a(), b());
    let setup = || {
        let flash = board(&a);
        stage(&flash, &b).unwrap();
        flash
    };
    let cuts = cut_everywhere(
        setup,
        |f| {
            let _ = boot(f);
        },
        |f, k| {
            assert_eq!(boot(f), Ok(Boot::Trial), "cut at {k}");
            assert_eq!(active(f, b.len()), b, "cut at {k}");
            assert_eq!(confirm(f), Ok(Started::Confirmed), "cut at {k}");
            assert_eq!(boot(f), Ok(Boot::Normal), "cut at {k}");
        },
    );
    // Two copies a page, each an erase, a write and its progress word,
    // and the word that notes the check.
    assert_eq!(cuts, 6 * (ACTIVE.len / PAGE) as usize + 1);
}

#[test]
fn a_power_cut_anywhere_in_a_swap_back_leaves_the_old_image_whole() {
    let (a, b) = (a(), b());
    let setup = || {
        let flash = board(&a);
        stage(&flash, &b).unwrap();
        assert_eq!(boot(&flash), Ok(Boot::Trial));
        flash
    };
    let cuts = cut_everywhere(
        setup,
        |f| {
            let _ = boot(f);
        },
        |f, k| {
            let again = boot(f).unwrap();
            assert!(
                matches!(again, Boot::Reverted | Boot::Normal),
                "cut at {k}: {again:?}"
            );
            assert_eq!(active(f, a.len()), a, "cut at {k}");
            confirm(f).unwrap();
            assert_eq!(boot(f), Ok(Boot::Normal), "cut at {k}");
            assert_eq!(active(f, a.len()), a, "cut at {k}");
        },
    );
    assert!(cuts > 6 * (ACTIVE.len / PAGE) as usize);
}

#[test]
fn a_power_cut_anywhere_in_staging_leaves_the_running_image() {
    let (a, b) = (a(), b());
    cut_everywhere(
        || board(&a),
        |f| {
            let _ = stage(f, &b);
        },
        |f, k| {
            let at = boot(f).unwrap();
            assert!(
                !matches!(at, Boot::Trial | Boot::Reverted),
                "cut at {k}: {at:?}"
            );
            assert_eq!(active(f, a.len()), a, "cut at {k}");
            // And the board can stage again.
            stage(f, &b).unwrap();
            assert_eq!(boot(f), Ok(Boot::Trial), "cut at {k}");
            assert_eq!(active(f, b.len()), b, "cut at {k}");
        },
    );
}

#[test]
fn a_power_cut_in_a_confirmation_leaves_one_image_whole() {
    let (a, b) = (a(), b());
    let setup = || {
        let flash = board(&a);
        stage(&flash, &b).unwrap();
        assert_eq!(boot(&flash), Ok(Boot::Trial));
        flash
    };
    cut_everywhere(
        setup,
        |f| {
            let _ = confirm(f);
        },
        |f, k| {
            boot(f).unwrap();
            confirm(f).unwrap();
            assert_eq!(boot(f), Ok(Boot::Normal), "cut at {k}");
            let is_a = active(f, a.len()) == a;
            let is_b = active(f, b.len()) == b;
            assert!(is_a || is_b, "cut at {k}: neither image whole");
        },
    );
}
