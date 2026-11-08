//! Images gathered from their coded pieces into a flash in RAM, with
//! power cuts at every flash operation; blocks that fail their checks;
//! and an image gathered through the external flash's driver on a fake
//! chip.

use std::vec;
use std::vec::Vec;

use super::*;
use crate::code::encode_piece;
use crate::ext_flash::fake;
use crate::layout::external::MAILBOX;

/// The power went: every later operation fails until the next boot.
#[derive(Debug, PartialEq)]
struct Cut;

/// The external flash in RAM, up to the mailbox: a program only clears
/// bits, an erase sets a sector to `0xFF`. The operation the power is cut
/// in is done half: the first half of the sector erased, or of the bytes
/// programmed, one of the states a real cut can leave.
struct Ram {
    mem: Vec<u8>,
    budget: Option<usize>,
    dead: bool,
    /// Polls of `done` before the operation under way ends.
    busy: u8,
    /// Programs and erases started.
    ops: usize,
}

impl Ram {
    fn new() -> Ram {
        Ram {
            mem: vec![0xff; MAILBOX.start as usize],
            budget: None,
            dead: false,
            busy: 0,
            ops: 0,
        }
    }

    fn cut_now(&mut self) -> Result<bool, Cut> {
        if self.dead {
            return Err(Cut);
        }
        assert_eq!(self.busy, 0, "an operation started over another");
        self.ops += 1;
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

    /// The next boot.
    fn revive(&mut self) {
        self.dead = false;
        self.budget = None;
        self.busy = 0;
    }
}

impl Flash for Ram {
    type Error = Cut;

    fn read(&mut self, address: u32, bytes: &mut [u8]) -> Result<(), Cut> {
        if self.dead {
            return Err(Cut);
        }
        assert_eq!(self.busy, 0, "a read while the chip works");
        let at = address as usize;
        bytes.copy_from_slice(&self.mem[at..at + bytes.len()]);
        Ok(())
    }

    fn start_program(&mut self, address: u32, data: &[u8]) -> Result<(), Cut> {
        assert!(address % PROGRAM_PAGE + data.len() as u32 <= PROGRAM_PAGE);
        let cut = self.cut_now()?;
        let n = if cut { data.len() / 2 } else { data.len() };
        let at = address as usize;
        for (m, &d) in self.mem[at..at + n].iter_mut().zip(data) {
            *m &= d;
        }
        if cut {
            return Err(Cut);
        }
        self.busy = 2;
        Ok(())
    }

    fn start_erase(&mut self, address: u32) -> Result<(), Cut> {
        assert!(address.is_multiple_of(SECTOR));
        let cut = self.cut_now()?;
        let at = address as usize;
        let n = if cut { SECTOR / 2 } else { SECTOR } as usize;
        self.mem[at..at + n].fill(0xff);
        if cut {
            return Err(Cut);
        }
        self.busy = 3;
        Ok(())
    }

    fn done(&mut self) -> Result<bool, Cut> {
        if self.dead {
            return Err(Cut);
        }
        match self.busy {
            0 => Ok(true),
            _ => {
                self.busy -= 1;
                Ok(false)
            }
        }
    }
}

/// A fixed sequence of pseudo-random numbers (xorshift64).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// An image as a holder packs it.
struct Built {
    image: Image,
    /// The header and the firmware, padded to whole pieces.
    stream: Vec<u8>,
    /// Each block's data pieces, the table's first.
    blocks: Vec<Vec<[u8; PIECE]>>,
}

fn sha(bytes: &[u8]) -> [u8; HASH_LEN] {
    Sha256::digest(bytes).into()
}

/// An image of a random header and `firmware` random bytes, named `name`
/// or by its header's hash.
fn build(firmware: usize, seed: u64, name: Option<[u8; HASH_LEN]>) -> Built {
    let mut rng = Rng(seed);
    let mut stream: Vec<u8> = (0..HEADER_LEN + firmware)
        .map(|_| rng.next() as u8)
        .collect();
    let name = name.unwrap_or_else(|| sha(&stream[..HEADER_LEN]));
    stream.resize(stream.len().next_multiple_of(PIECE), 0);
    let pieces: Vec<[u8; PIECE]> = stream
        .chunks(PIECE)
        .map(|c| c.try_into().unwrap())
        .collect();
    let mut blocks: Vec<Vec<[u8; PIECE]>> = pieces.chunks(DATA).map(|c| c.to_vec()).collect();
    let mut table = name.to_vec();
    for b in &blocks {
        table.extend_from_slice(&sha(b.as_flattened())[..ENTRY]);
    }
    table.resize(table.len().next_multiple_of(PIECE), 0);
    blocks.insert(
        0,
        table.chunks(PIECE).map(|c| c.try_into().unwrap()).collect(),
    );
    let image = Image {
        name,
        blocks: blocks.len() as u8,
        last: blocks.last().unwrap().len() as u8,
    };
    for (b, data) in blocks.iter().enumerate() {
        assert_eq!(image.pieces(b as u8), data.len());
    }
    Built {
        image,
        stream,
        blocks,
    }
}

/// Table and two blocks, of 64 pieces and of 5.
fn small() -> Built {
    build(69 * PIECE - HEADER_LEN - 100, 1, None)
}

/// Hands pieces to the store as holders would: each block's from a
/// random piece on, every seventh lost.
struct Feeder {
    next: Vec<u8>,
    /// A piece to send in place of the true one: block, index, bytes.
    forged: Option<(u8, u8, [u8; PIECE])>,
    events: Vec<Poll>,
}

impl Feeder {
    fn new(built: &Built, seed: u64) -> Feeder {
        let mut rng = Rng(seed);
        Feeder {
            next: built
                .blocks
                .iter()
                .map(|_| (rng.next() % CODED as u64) as u8)
                .collect(),
            forged: None,
            events: Vec::new(),
        }
    }

    /// Poll until idle, keeping the events.
    fn settle<F: Flash>(&mut self, store: &mut Store, flash: &mut F) -> Result<(), F::Error> {
        for _ in 0..1_000_000 {
            match store.poll(flash)? {
                Poll::Idle => return Ok(()),
                Poll::Busy | Poll::Again => {}
                e => self.events.push(e),
            }
        }
        panic!("the store never idles");
    }

    /// Gather until the image is whole.
    fn gather<F: Flash>(
        &mut self,
        built: &Built,
        store: &mut Store,
        flash: &mut F,
    ) -> Result<(), F::Error> {
        for _ in 0..100_000 {
            self.settle(store, flash)?;
            if store.whole() {
                return Ok(());
            }
            let first = store.lacks().unwrap();
            for b in first..(first + LANES as u8).min(built.image.blocks) {
                let k = self.next[usize::from(b)];
                self.next[usize::from(b)] = ((usize::from(k) + 1) % CODED) as u8;
                if k % 7 == 3 {
                    continue;
                }
                let piece = match self.forged {
                    Some((fb, fk, bytes)) if (fb, fk) == (b, k) => {
                        self.forged = None;
                        bytes
                    }
                    _ => encode_piece(&built.blocks[usize::from(b)], k).unwrap(),
                };
                store.offer(b, k, &piece);
                // A piece arrives while the store works.
                store.poll(flash)?;
            }
        }
        panic!("the image never came whole");
    }
}

/// The flash holds the image, the table and the header, and the store
/// says so, also after a reset.
fn check<F: Flash>(built: &Built, store: &Store, flash: &mut F)
where
    F::Error: core::fmt::Debug,
{
    assert!(store.whole());
    let table = built.blocks[0].as_flattened();
    assert_eq!(
        store.table().unwrap(),
        &table[..store.table().unwrap().len()]
    );
    let header = store.header(flash).unwrap().unwrap();
    assert_eq!(&header[..], &built.stream[..HEADER_LEN]);
    let firmware = &built.stream[HEADER_LEN..];
    let mut dfu = vec![0u8; firmware.len()];
    flash.read(DFU.start, &mut dfu).unwrap();
    assert!(dfu == firmware, "the staged slot differs");
    let mut again = Store::open(flash).unwrap();
    assert_eq!(again.begin(built.image), Begin::Resumed);
    while again.poll(flash).unwrap() != Poll::Idle {}
    assert!(again.whole());
    assert_eq!(again.table(), store.table());
}

fn gathered(built: &Built, seed: u64) -> (Ram, Feeder) {
    let mut flash = Ram::new();
    let mut store = Store::open(&mut flash).unwrap();
    assert_eq!(store.begin(built.image), Begin::Started);
    let mut feeder = Feeder::new(built, seed);
    feeder.gather(built, &mut store, &mut flash).unwrap();
    check(built, &store, &mut flash);
    (flash, feeder)
}

#[test]
fn the_check_is_crc_32() {
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
}

#[test]
fn an_image_is_gathered() {
    let built = small();
    let (flash, feeder) = gathered(&built, 2);
    // The short last block comes whole first.
    let decoded = [Poll::Decoded(0), Poll::Decoded(2), Poll::Decoded(1)];
    assert_eq!(feeder.events, decoded);
    // Clearing: the head, the lanes and the 5 sectors of the image, 26;
    // the record; the table's piece, page and byte; block 1's 64 slots, 70
    // writes of header and image, each group's bytes split where a page
    // or the header ends, and its byte; block 2's 5 slots, 5 writes and
    // byte; the lanes of the three blocks erased, 12.
    assert_eq!(flash.ops, 26 + 1 + 3 + (64 + 70 + 1) + (5 + 5 + 1) + 12);
}

#[test]
fn a_larger_image_is_gathered_through_its_lanes_again() {
    // Eight blocks of image: the lanes of the first are used again.
    let built = build(7 * DATA * PIECE + 3000, 3, None);
    assert_eq!(built.image.blocks, 9);
    let (_, feeder) = gathered(&built, 4);
    assert_eq!(feeder.events.len(), 9);
}

#[test]
fn the_largest_image_is_gathered() {
    let built = build(ACTIVE.len as usize, 5, None);
    assert_eq!(usize::from(built.image.blocks), MAX_BLOCKS);
    gathered(&built, 6);
    let mut too_large = built.image;
    too_large.last = DATA as u8;
    too_large.blocks += 1;
    let mut flash = Ram::new();
    let mut store = Store::open(&mut flash).unwrap();
    assert_eq!(store.begin(too_large), Begin::Unfit);
}

#[test]
fn a_power_cut_at_every_operation_loses_nothing() {
    let built = small();
    let (clean, _) = gathered(&built, 7);
    let mut tested = 0;
    for budget in 0..clean.ops {
        let mut flash = Ram::new();
        flash.budget = Some(budget);
        let mut store = Store::open(&mut flash).unwrap();
        store.begin(built.image);
        let mut feeder = Feeder::new(&built, 7);
        let mut cuts = 0;
        while let Err(Cut) = feeder.gather(&built, &mut store, &mut flash) {
            cuts += 1;
            flash.revive();
            store = Store::open(&mut flash).unwrap();
            assert_ne!(store.begin(built.image), Begin::Unfit);
        }
        assert_eq!(cuts, 1, "cut at operation {budget}");
        tested += 1;
        check(&built, &store, &mut flash);
    }
    assert_eq!(tested, 188);
}

#[test]
fn a_block_that_fails_its_hash_is_gathered_again() {
    let built = small();
    let mut flash = Ram::new();
    let mut store = Store::open(&mut flash).unwrap();
    store.begin(built.image);
    let mut feeder = Feeder::new(&built, 8);
    let k = feeder.next[1];
    let mut forged = encode_piece(&built.blocks[1], k).unwrap();
    forged[100] ^= 1;
    feeder.forged = Some((1, k, forged));
    feeder.gather(&built, &mut store, &mut flash).unwrap();
    let failed = feeder.events.iter().position(|&e| e == Poll::Failed(1));
    let decoded = feeder.events.iter().position(|&e| e == Poll::Decoded(1));
    assert!(failed.unwrap() < decoded.unwrap(), "{:?}", feeder.events);
    check(&built, &store, &mut flash);
}

#[test]
fn a_false_table_is_gathered_again() {
    let built = small();
    let mut flash = Ram::new();
    let mut store = Store::open(&mut flash).unwrap();
    store.begin(built.image);
    let mut feeder = Feeder::new(&built, 9);
    let k = feeder.next[0];
    let mut forged = encode_piece(&built.blocks[0], k).unwrap();
    forged[0] ^= 0x80;
    feeder.forged = Some((0, k, forged));
    feeder.gather(&built, &mut store, &mut flash).unwrap();
    assert_eq!(feeder.events[0], Poll::Failed(0));
    check(&built, &store, &mut flash);
}

#[test]
fn a_header_that_is_not_the_name_fails_its_block() {
    // The table names the image, the header hashes to something else.
    let built = build(3000, 10, Some([7; HASH_LEN]));
    let mut flash = Ram::new();
    let mut store = Store::open(&mut flash).unwrap();
    store.begin(built.image);
    let mut feeder = Feeder::new(&built, 11);
    for _ in 0..20 {
        for b in 0..2 {
            for _ in 0..3 {
                let k = feeder.next[b];
                feeder.next[b] += 1;
                store.offer(b as u8, k, &encode_piece(&built.blocks[b], k).unwrap());
            }
        }
        feeder.settle(&mut store, &mut flash).unwrap();
    }
    assert_eq!(feeder.events[0], Poll::Decoded(0));
    assert!(feeder.events[1..].iter().all(|&e| e == Poll::Failed(1)));
    assert!(feeder.events.len() > 2);
    assert_eq!(store.lacks(), Some(1));
}

#[test]
fn only_the_five_blocks_from_the_first_lacked_are_taken() {
    let built = build(9 * DATA * PIECE, 12, None);
    let mut flash = Ram::new();
    let mut store = Store::open(&mut flash).unwrap();
    let piece = [0u8; PIECE];
    assert_eq!(store.offer(0, 0, &piece), Offer::NotReady);
    store.begin(built.image);
    assert_eq!(store.offer(0, 0, &piece), Offer::NotReady);
    let mut feeder = Feeder::new(&built, 13);
    feeder.settle(&mut store, &mut flash).unwrap();
    assert_eq!(store.lacks(), Some(0));
    assert_eq!(store.offer(5, 0, &piece), Offer::Outside);
    assert_eq!(store.offer(4, 255, &piece), Offer::Outside);
    assert_eq!(store.offer(built.image.blocks, 0, &piece), Offer::Outside);
    let table = &built.blocks[0][0];
    assert_eq!(store.offer(0, 9, table), Offer::Kept);
    assert_eq!(store.offer(0, 9, table), Offer::Held);
    assert_eq!(store.offer(0, 10, table), Offer::Enough);
    feeder.settle(&mut store, &mut flash).unwrap();
    assert_eq!(feeder.events, [Poll::Decoded(0)]);
    assert_eq!(store.offer(0, 1, table), Offer::Decoded);
    assert_eq!(store.offer(5, 0, &piece), Offer::Kept);
    assert_eq!(store.offer(6, 0, &piece), Offer::Outside);
    assert_eq!(store.offer(4, 0, &piece), Offer::Kept);
    assert_eq!(store.offer(3, 0, &piece), Offer::Busy);
}

#[test]
fn a_new_image_clears_the_old_and_forget_clears_the_head() {
    let old = small();
    let (mut flash, _) = gathered(&old, 14);
    let new = build(5000, 15, None);
    let mut store = Store::open(&mut flash).unwrap();
    assert_eq!(store.begin(new.image), Begin::Started);
    let mut feeder = Feeder::new(&new, 16);
    feeder.gather(&new, &mut store, &mut flash).unwrap();
    check(&new, &store, &mut flash);
    store.forget();
    feeder.settle(&mut store, &mut flash).unwrap();
    let store = Store::open(&mut flash).unwrap();
    assert_eq!(store.image(), None);
}

#[test]
fn an_image_is_gathered_through_the_driver() {
    let built = build(2 * DATA * PIECE, 17, None);
    let mut chip = fake::chip();
    let mut store = Store::open(&mut chip).unwrap();
    store.begin(built.image);
    let mut feeder = Feeder::new(&built, 18);
    feeder.gather(&built, &mut store, &mut chip).unwrap();
    check(&built, &store, &mut chip);
    assert_eq!(chip.fake().ignored, 0);
    assert!(chip.fake().asleep());
}

#[test]
fn the_store_takes_about_4_kib() {
    // 4 176 B on this 64-bit host; the decode's factors take half as many
    // bytes on the chip.
    assert!(core::mem::size_of::<Store>() <= 4200);
}
