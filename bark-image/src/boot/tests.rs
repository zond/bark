//! The swap and its check on two flashes in RAM, the board's internal
//! one and its external one, and power cuts at every flash operation of
//! a staging, a swap, a swap back and a confirmation; and a swap through
//! the external flash's driver on a fake chip.

use std::cell::RefCell as Cell;
use std::rc::Rc;
use std::vec;
use std::vec::Vec;

use embedded_storage::nor_flash::{ErrorType, NorFlashError, NorFlashErrorKind};

use super::*;
use crate::ext_flash;
use crate::ext_flash::fake;
use crate::layout::external::END;
use crate::layout::FLASH_END;

/// The power went: every later operation fails until the next boot.
#[derive(Debug)]
struct Cut;

impl NorFlashError for Cut {
    fn kind(&self) -> NorFlashErrorKind {
        NorFlashErrorKind::Other
    }
}

/// The board's supply, which both flashes share: with a budget, the
/// operation that exhausts it, on either flash, is the one cut.
#[derive(Default)]
struct Power {
    budget: Option<usize>,
    dead: bool,
}

/// NOR flash in RAM that writes `W` bytes at once: an erase sets a page
/// to `0xFF`, a write can only clear bits, as on the nRF52840 (Product
/// Specification, NVMC) and the external chip. The operation the power is
/// cut in is cut half way: the first half of the page erased, or the
/// first half of the writes done, which is one of the states a real cut
/// can leave.
struct RamFlash<const W: usize> {
    bytes: Vec<u8>,
    power: Rc<Cell<Power>>,
}

impl<const W: usize> RamFlash<W> {
    fn new(len: u32, power: &Rc<Cell<Power>>) -> Self {
        RamFlash {
            bytes: vec![0xff; len as usize],
            power: power.clone(),
        }
    }

    /// Whether this operation is the one the power is cut in.
    fn cut_now(&mut self) -> Result<bool, Cut> {
        let mut p = self.power.borrow_mut();
        if p.dead {
            return Err(Cut);
        }
        match &mut p.budget {
            Some(0) => {
                p.dead = true;
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

impl<const W: usize> ErrorType for RamFlash<W> {
    type Error = Cut;
}

impl<const W: usize> ReadNorFlash for RamFlash<W> {
    const READ_SIZE: usize = 1;

    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Cut> {
        if self.power.borrow().dead {
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

impl<const W: usize> NorFlash for RamFlash<W> {
    const WRITE_SIZE: usize = W;
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
        assert!((offset as usize).is_multiple_of(W) && data.len().is_multiple_of(W));
        let at = offset as usize;
        let n = match self.cut_now()? {
            true => data.len() / (2 * W) * W,
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

type Internal = Shared<RamFlash<4>>;
type External = Shared<RamFlash<1>>;

/// The board: its internal flash, its external one, and their supply.
struct Board {
    internal: Internal,
    external: External,
    power: Rc<Cell<Power>>,
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
fn board(a: &[u8]) -> Board {
    let power = Rc::new(Cell::new(Power::default()));
    let mut internal = RamFlash::new(FLASH_END, &power);
    let at = ACTIVE.start as usize;
    internal.bytes[at..at + a.len()].copy_from_slice(a);
    Board {
        internal: Mutex::new(RefCell::new(internal)),
        external: Mutex::new(RefCell::new(RamFlash::new(END, &power))),
        power,
    }
}

/// The key the test images are signed with, and the one the running
/// image holds.
fn signer() -> ed25519_compact::KeyPair {
    ed25519_compact::KeyPair::from_seed(ed25519_compact::Seed::new([7; 32]))
}

fn keys() -> [Key; SLOTS] {
    let mut k = [Key::NONE; SLOTS];
    k[0] = Key {
        id: 1,
        public: *signer().pk,
    };
    k
}

/// The header of `b`, signed in its first slot.
fn header_for(b: &[u8], version: u32) -> [u8; HEADER_LEN] {
    let mut h = unsigned_for(b, version);
    let at = sign::SIGNED_LEN;
    h[at] = 1;
    let signature = *signer().sk.sign(&h[..at], None);
    h[at + 1..at + sign::SLOT_LEN].copy_from_slice(&signature);
    h
}

/// The header of `b`, unsigned.
fn unsigned_for(b: &[u8], version: u32) -> [u8; HEADER_LEN] {
    Header {
        version,
        length: b.len() as u32,
        hash: Sha256::digest(b).into(),
    }
    .encode()
}

/// The value of a future that does not wait: staging on blocking flashes
/// ([`blocking`]) is done at its first poll.
fn now<T>(f: impl core::future::Future<Output = T>) -> T {
    let mut f = core::pin::pin!(f);
    let mut cx = core::task::Context::from_waker(core::task::Waker::noop());
    match f.as_mut().poll(&mut cx) {
        core::task::Poll::Ready(v) => v,
        core::task::Poll::Pending => panic!("a blocking flash waited"),
    }
}

type BlockingStager<'a, I, E> = Stager<Blocking<'a, I>, Blocking<'a, I>, Blocking<'a, E>>;

fn begin<'a, I: NorFlash, E: NorFlash>(
    internal: &'a Shared<I>,
    external: &'a Shared<E>,
    header: &[u8; HEADER_LEN],
    running: u32,
) -> Result<BlockingStager<'a, I, E>, StageError> {
    let (record, state, dfu) = blocking(internal, external);
    now(Stager::begin(record, state, dfu, header, running, &keys()))
}

fn stage_on<I: NorFlash, E: NorFlash>(
    internal: &Shared<I>,
    external: &Shared<E>,
    b: &[u8],
) -> Result<Header, StageError> {
    let mut s = begin(internal, external, &header_for(b, RUNNING + 1), RUNNING)?;
    for (i, chunk) in b.chunks(CHUNK).enumerate() {
        now(s.write((i * CHUNK) as u32, chunk))?;
    }
    now(s.finish())
}

fn stage(board: &Board, b: &[u8]) -> Result<Header, StageError> {
    stage_on(&board.internal, &board.external, b)
}

fn power(board: &Board, budget: Option<usize>) {
    *board.power.borrow_mut() = Power {
        budget,
        dead: false,
    };
}

fn spent(board: &Board) -> bool {
    board.power.borrow().dead
}

fn boot(board: &Board) -> Result<Boot, FlashError> {
    let mut page = [0u8; PAGE as usize];
    prepare(&board.internal, Some(&board.external), &mut page)
}

/// A boot whose external flash fails its presence check.
fn boot_without_chip(board: &Board) -> Result<Boot, FlashError> {
    let mut page = [0u8; PAGE as usize];
    prepare(&board.internal, None::<&External>, &mut page)
}

fn active(board: &Board, len: usize) -> Vec<u8> {
    board.internal.lock(|f| {
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
    let board = board(&a);
    assert_eq!(boot(&board), Ok(Boot::Normal));
    stage(&board, &b).unwrap();
    assert_eq!(boot(&board), Ok(Boot::Trial));
    assert_eq!(active(&board, b.len()), b);
    assert_eq!(confirm(&board.internal), Ok(Started::Confirmed));
    assert_eq!(boot(&board), Ok(Boot::Normal));
    assert_eq!(confirm(&board.internal), Ok(Started::Normal));
    assert_eq!(active(&board, b.len()), b);
}

#[test]
fn an_image_that_does_not_confirm_is_swapped_back() {
    let (a, b) = (a(), b());
    let board = board(&a);
    stage(&board, &b).unwrap();
    assert_eq!(boot(&board), Ok(Boot::Trial));
    // The previous image waits on the external flash, a sector up.
    board.external.lock(|f| {
        let at = (DFU.start + PAGE) as usize;
        assert_eq!(&f.borrow().bytes[at..at + a.len()], a.as_slice());
    });
    assert_eq!(boot(&board), Ok(Boot::Reverted));
    assert_eq!(active(&board, a.len()), a);
    assert_eq!(confirm(&board.internal), Ok(Started::Reverted));
    assert_eq!(boot(&board), Ok(Boot::Normal));
    assert_eq!(active(&board, a.len()), a);
}

#[test]
fn a_staged_image_that_went_bad_is_refused() {
    let (a, b) = (a(), b());
    let board = board(&a);
    stage(&board, &b).unwrap();
    // One bit of the staged image lost, as flash that went bad loses it.
    board.external.lock(|f| {
        let bytes = &mut f.borrow_mut().bytes;
        let at = (DFU.start as usize + 4096..)
            .find(|&i| bytes[i] != 0)
            .unwrap();
        bytes[at] &= bytes[at] - 1;
    });
    assert_eq!(boot(&board), Ok(Boot::Refused(Refusal::Hash)));
    assert_eq!(active(&board, a.len()), a);
    assert_eq!(boot(&board), Ok(Boot::Normal));
    assert_eq!(confirm(&board.internal), Ok(Started::Refused));
    assert_eq!(confirm(&board.internal), Ok(Started::Normal));
}

#[test]
fn a_swap_without_a_record_is_refused() {
    let (a, b) = (a(), b());
    let board = board(&a);
    stage(&board, &b).unwrap();
    board.internal.lock(|f| {
        let at = RECORD.start as usize;
        f.borrow_mut().bytes[at..at + PAGE as usize].fill(0xff)
    });
    assert_eq!(
        boot(&board),
        Ok(Boot::Refused(Refusal::Header(HeaderError::Magic)))
    );
    assert_eq!(active(&board, a.len()), a);
}

#[test]
fn a_swap_with_no_external_flash_is_cancelled() {
    let (a, b) = (a(), b());
    let board = board(&a);
    stage(&board, &b).unwrap();
    assert_eq!(boot_without_chip(&board), Ok(Boot::Refused(Refusal::Chip)));
    assert_eq!(active(&board, a.len()), a);
    assert_eq!(boot(&board), Ok(Boot::Normal));
    assert_eq!(confirm(&board.internal), Ok(Started::Refused));
    assert_eq!(boot_without_chip(&board), Ok(Boot::Normal));
}

#[test]
fn a_swap_under_way_waits_for_the_external_flash() {
    let (a, b) = (a(), b());
    let board = board(&a);
    stage(&board, &b).unwrap();
    // Cut half way through the swap.
    power(&board, Some(3 * (ACTIVE.len / PAGE) as usize));
    let _ = boot(&board);
    assert!(spent(&board));
    power(&board, None);
    // The chip gone: nothing is touched, and the next boot tries again.
    let before = active(&board, ACTIVE.len as usize);
    assert!(boot_without_chip(&board).is_err());
    assert_eq!(active(&board, ACTIVE.len as usize), before);
    assert_eq!(boot(&board), Ok(Boot::Trial));
    assert_eq!(active(&board, b.len()), b);
}

#[test]
fn a_swap_and_its_swap_back_through_the_driver() {
    let (a, b) = (a(), b());
    let board = board(&a);
    let chip = Mutex::new(RefCell::new(fake::chip()));
    stage_on(&board.internal, &chip, &b).unwrap();
    let mut page = [0u8; PAGE as usize];
    let started = chip.lock(|c| c.borrow_mut().fake().now);
    assert_eq!(
        prepare(&board.internal, Some(&chip), &mut page),
        Ok(Boot::Trial)
    );
    assert_eq!(active(&board, b.len()), b);
    let swap_us = chip.lock(|c| c.borrow_mut().fake().now) - started;
    assert_eq!(
        prepare(&board.internal, Some(&chip), &mut page),
        Ok(Boot::Reverted)
    );
    assert_eq!(active(&board, a.len()), a);
    chip.lock(|c| {
        let mut c = c.borrow_mut();
        assert_eq!(c.fake().ignored, 0);
        assert!(c.fake().asleep());
    });
    // The external side of a swap: per sector an erase and 16 programs,
    // typical, and the bytes over the bus.
    let typical = u64::from(ACTIVE.len / PAGE)
        * u64::from(ext_flash::ERASE_TYP_US + 16 * ext_flash::PROGRAM_TYP_US);
    assert!(swap_us >= typical, "{swap_us} us");
}

#[test]
fn staging_refuses_what_it_cannot_stage() {
    let (a, b) = (a(), b());
    let board = board(&a);
    let (i, e) = (&board.internal, &board.external);
    let old = header_for(&b, RUNNING);
    assert_eq!(
        begin(i, e, &old, RUNNING).err(),
        Some(StageError::Version {
            running: RUNNING,
            staged: RUNNING
        })
    );
    assert_eq!(
        begin(i, e, &unsigned_for(&b, RUNNING + 1), RUNNING).err(),
        Some(StageError::Signature(SignatureError::Unsigned))
    );
    let mut s = begin(i, e, &header_for(&b, RUNNING + 1), RUNNING).unwrap();
    assert_eq!(
        now(s.write(4, &b[4..8])),
        Err(StageError::Offset { expected: 0 })
    );
    assert_eq!(now(s.write(0, &b[..CHUNK + 1])), Err(StageError::Chunk));
    now(s.write(0, &b[..CHUNK])).unwrap();
    assert_eq!(
        now(s.finish()).err(),
        Some(StageError::Incomplete { got: CHUNK as u32 })
    );
    // A different image under the header.
    let mut s = begin(i, e, &header_for(&b, RUNNING + 1), RUNNING).unwrap();
    let other = image(3, b.len());
    for (n, chunk) in other.chunks(CHUNK).enumerate() {
        now(s.write((n * CHUNK) as u32, chunk)).unwrap();
    }
    assert_eq!(now(s.finish()).err(), Some(StageError::Hash));
    assert_eq!(boot(&board), Ok(Boot::Normal));
    // On trial, nothing is staged until the image confirms itself.
    stage(&board, &b).unwrap();
    assert_eq!(boot(&board), Ok(Boot::Trial));
    assert_eq!(
        begin(i, e, &header_for(&a[..100], RUNNING + 2), RUNNING + 1).err(),
        Some(StageError::Busy)
    );
}

/// Run `step` with the power cut at its first, second, ... flash
/// operation, on either flash, each time on a board `setup` made, until
/// it runs whole; `after` sees the board once the power is back. Returns
/// the cuts made.
fn cut_everywhere(
    setup: impl Fn() -> Board,
    step: impl Fn(&Board),
    after: impl Fn(&Board, usize),
) -> usize {
    for k in 0.. {
        let board = setup();
        power(&board, Some(k));
        step(&board);
        if !spent(&board) {
            return k;
        }
        power(&board, None);
        after(&board, k);
    }
    unreachable!()
}

#[test]
fn a_power_cut_anywhere_in_a_swap_leaves_the_new_image_whole() {
    let (a, b) = (a(), b());
    let setup = || {
        let board = board(&a);
        stage(&board, &b).unwrap();
        board
    };
    let cuts = cut_everywhere(
        setup,
        |f| {
            let _ = boot(f);
        },
        |f, k| {
            assert_eq!(boot(f), Ok(Boot::Trial), "cut at {k}");
            assert_eq!(active(f, b.len()), b, "cut at {k}");
            assert_eq!(confirm(&f.internal), Ok(Started::Confirmed), "cut at {k}");
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
        let board = board(&a);
        stage(&board, &b).unwrap();
        assert_eq!(boot(&board), Ok(Boot::Trial));
        board
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
            confirm(&f.internal).unwrap();
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
        let board = board(&a);
        stage(&board, &b).unwrap();
        assert_eq!(boot(&board), Ok(Boot::Trial));
        board
    };
    cut_everywhere(
        setup,
        |f| {
            let _ = confirm(&f.internal);
        },
        |f, k| {
            boot(f).unwrap();
            confirm(&f.internal).unwrap();
            assert_eq!(boot(f), Ok(Boot::Normal), "cut at {k}");
            let is_a = active(f, a.len()) == a;
            let is_b = active(f, b.len()) == b;
            assert!(is_a || is_b, "cut at {k}: neither image whole");
        },
    );
}

mod cut;
