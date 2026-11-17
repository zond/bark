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
/// or by the hash of its header's signed bytes; `resigned`, its signature
/// slots changed after it was named.
fn build(firmware: usize, seed: u64, name: Option<[u8; HASH_LEN]>, resigned: bool) -> Built {
    let mut rng = Rng(seed);
    let stream: Vec<u8> = (0..HEADER_LEN + firmware)
        .map(|_| rng.next() as u8)
        .collect();
    from_stream(stream, name, resigned)
}

/// An image of `stream`, its header and firmware, as [`build`] makes it.
fn from_stream(mut stream: Vec<u8>, name: Option<[u8; HASH_LEN]>, resigned: bool) -> Built {
    let name = name.unwrap_or_else(|| sha(&stream[..SIGNED_LEN]));
    if resigned {
        for b in &mut stream[SIGNED_LEN..HEADER_LEN] {
            *b ^= 0x5a;
        }
    }
    stream.resize(stream.len().next_multiple_of(PIECE), 0);
    let pieces: Vec<[u8; PIECE]> = stream
        .chunks(PIECE)
        .map(|c| c.try_into().unwrap())
        .collect();
    let mut blocks: Vec<Vec<[u8; PIECE]>> = pieces.chunks(DATA).map(|c| c.to_vec()).collect();
    let mut table = name.to_vec();
    for (b, data) in blocks.iter().enumerate() {
        // Block 1's entry leaves out the header.
        let from = if b == 0 { HEADER_LEN } else { 0 };
        table.extend_from_slice(&sha(&data.as_flattened()[from..])[..ENTRY]);
    }
    let own = sha(&table);
    table.extend_from_slice(&own[..ENTRY]);
    table.resize(table.len().next_multiple_of(PIECE), 0);
    blocks.insert(
        0,
        table.chunks(PIECE).map(|c| c.try_into().unwrap()).collect(),
    );
    let image = Image {
        blocks: blocks.len() as u8,
        last: blocks.last().unwrap().len() as u8,
        version: 7,
        key: 1,
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
    build(69 * PIECE - HEADER_LEN - 100, 1, None, false)
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
    // Clearing: the head, the three blocks' lanes and the 5 sectors of the
    // image, 18;
    // the record; every slot in 16 programs of up to `CHUNK` bytes; the
    // table's slot, its 16 parts and its byte; block 1's 64 slots and its
    // byte, block 2's 5 slots and its byte, and 1 096 writes of the two
    // blocks' header and image, each group's two pieces in parts of up to
    // `CHUNK` bytes, split also where a page or the header ends; the lanes
    // of the three blocks erased, 12.
    let slot = WRITTEN.div_ceil(CHUNK);
    assert_eq!(slot, 16);
    assert_eq!(
        flash.ops,
        18 + 1 + (slot + 16 + 1) + (64 * slot + 1) + (5 * slot + 1) + 1096 + 12
    );
}

#[test]
fn a_larger_image_is_gathered_a_lane_a_block() {
    // Eight blocks of image, each in a lane of its own.
    let built = build(7 * DATA * PIECE + 3000, 3, None, false);
    assert_eq!(built.image.blocks, 9);
    let (_, feeder) = gathered(&built, 4);
    assert_eq!(feeder.events.len(), 9);
}

#[test]
fn the_largest_image_is_gathered() {
    // The most the lanes hold: 47 blocks of image behind the header.
    let built = build((MAX_BLOCKS - 1) * DATA * PIECE - HEADER_LEN, 5, None, false);
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
    assert_eq!(tested, 2266);
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
fn a_header_whose_signed_bytes_are_not_the_name_fails_its_block() {
    // The table names the image, the header's signed bytes hash to
    // something else.
    let built = build(3000, 10, Some([7; HASH_LEN]), false);
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
fn an_image_signed_again_keeps_its_name() {
    // Other signature slots, the same signed bytes: the same name and the
    // same table, and the block passes; the signatures are checked once
    // blocks 0 and 1 are held.
    let first = build(3000, 19, None, false);
    let again = build(3000, 19, None, true);
    assert_eq!(first.image, again.image);
    assert_ne!(first.stream, again.stream);
    assert_eq!(first.blocks[0], again.blocks[0]);
    gathered(&again, 20);
}

/// The key the proof's tests sign with.
fn signer() -> ed25519_compact::KeyPair {
    ed25519_compact::KeyPair::from_seed(ed25519_compact::Seed::new([3; 32]))
}

fn proof_keys() -> [Key; KEYS] {
    let mut k = [Key::NONE; KEYS];
    k[1] = Key {
        id: 4,
        public: *signer().pk,
    };
    k
}

/// An image of random firmware under a header of `version`, `table` its
/// table hash, signed by [`signer`] in its first slot.
fn signed(firmware: usize, version: u32, table: Option<[u8; HASH_LEN]>) -> Built {
    let mut rng = Rng(u64::from(version));
    let body: Vec<u8> = (0..firmware).map(|_| rng.next() as u8).collect();
    let mut entries = Entries::new();
    entries.feed(&[0; HEADER_LEN], |_, _| ());
    entries.feed(&body, |_, _| ());
    let mut h = Header {
        version,
        length: firmware as u32,
        hash: sha(&body),
        table: table.unwrap_or_else(|| entries.finish(firmware as u32, |_, _| ())),
    }
    .encode();
    h[SIGNED_LEN] = 4;
    let signature = *signer().sk.sign(&h[..SIGNED_LEN], None);
    h[SIGNED_LEN + 1..SIGNED_LEN + sign::SLOT_LEN].copy_from_slice(&signature);
    let mut stream = h.to_vec();
    stream.extend_from_slice(&body);
    let mut built = from_stream(stream, None, false);
    built.image.version = version;
    built.image.key = 4;
    built
}

/// Blocks 0 and 1 of an image signed by a held key, whose header names
/// its table, prove it; a table, a version, a shape or a key not the
/// header's does not (spec "Updates over the mesh").
#[test]
fn the_first_two_blocks_prove_the_image() {
    let firmware = 2 * DATA * PIECE + 500;
    let built = signed(firmware, 9, None);
    let (mut flash, _) = gathered(&built, 31);
    let store = {
        let mut s = Store::open(&mut flash).unwrap();
        s.begin(built.image);
        while s.poll(&mut flash).unwrap() != Poll::Idle {}
        s
    };
    let header = store.header(&mut flash).unwrap().unwrap();
    let table = store.table().unwrap();
    let image = built.image;
    let keys = proof_keys();
    let h = check_proof(&header, table, &image, 8, &keys).unwrap();
    assert_eq!((h.version, h.length), (9, firmware as u32));
    // The table the holder makes of what it holds hashes the same.
    let mut made = Table::new(image, &name(&header));
    let mut tabling = Tabling::new();
    tabling.feed(&mut made, &built.stream[..HEADER_LEN + firmware]);
    let hashes = tabling.finish(&mut made);
    assert_eq!(hashes.table, h.table);
    assert_eq!(hashes.firmware, h.hash);
    assert_eq!(&made.bytes[..table.len()], table);
    // Running it already, or another version than the section's.
    assert_eq!(
        check_proof(&header, table, &image, 9, &keys),
        Err(ProofError::Version(9))
    );
    let other = Image {
        version: 10,
        ..image
    };
    assert_eq!(
        check_proof(&header, table, &other, 8, &keys),
        Err(ProofError::Version(9))
    );
    let longer = Image {
        last: image.last - 1,
        ..image
    };
    assert_eq!(
        check_proof(&header, table, &longer, 8, &keys),
        Err(ProofError::Shape)
    );
    assert_eq!(
        check_proof(&header, table, &image, 8, &[Key::NONE; KEYS]),
        Err(ProofError::Signature(SignatureError::UnknownKey))
    );
    let mut table2 = table.to_vec();
    table2[HASH_LEN] ^= 1;
    assert_eq!(
        check_proof(&header, &table2, &image, 8, &keys),
        Err(ProofError::Table)
    );
    // Signed, but naming another table.
    let wrong = signed(firmware, 9, Some([1; HASH_LEN]));
    let header = &wrong.stream[..HEADER_LEN].try_into().unwrap();
    let table = &wrong.blocks[0].as_flattened()[..table.len()];
    assert_eq!(
        check_proof(header, table, &wrong.image, 8, &keys),
        Err(ProofError::Table)
    );
}

#[test]
fn every_block_of_the_image_is_taken() {
    let built = build(9 * DATA * PIECE, 12, None, false);
    let mut flash = Ram::new();
    let mut store = Store::open(&mut flash).unwrap();
    let piece = [0u8; PIECE];
    assert_eq!(store.offer(0, 0, &piece), Offer::NotReady);
    store.begin(built.image);
    assert_eq!(store.offer(0, 0, &piece), Offer::NotReady);
    let mut feeder = Feeder::new(&built, 13);
    feeder.settle(&mut store, &mut flash).unwrap();
    assert_eq!(store.lacks(), Some(0));
    assert_eq!(store.offer(4, 255, &piece), Offer::Outside);
    assert_eq!(store.offer(built.image.blocks, 0, &piece), Offer::Outside);
    let table = &built.blocks[0][0];
    assert_eq!(store.offer(0, 9, table), Offer::Kept);
    assert_eq!(store.offer(0, 9, table), Offer::Held);
    assert_eq!(store.offer(0, 10, table), Offer::Enough);
    // The last block, far from the first lacked, is taken too.
    let last = built.image.blocks - 1;
    assert_eq!(store.offer(last, 0, &piece), Offer::Kept);
    assert_eq!(store.offer(5, 0, &piece), Offer::Busy);
    feeder.settle(&mut store, &mut flash).unwrap();
    assert_eq!(feeder.events, [Poll::Decoded(0)]);
    assert_eq!(store.offer(0, 1, table), Offer::Decoded);
    assert_eq!(store.offer(last, 0, &piece), Offer::Held);
    assert_eq!(store.offer(5, 0, &piece), Offer::Kept);
    assert_eq!(store.offer(6, 0, &piece), Offer::Kept);
    assert_eq!(store.offer(3, 0, &piece), Offer::Busy);
}

#[test]
fn a_new_image_clears_the_old_and_forget_clears_the_head() {
    let old = small();
    let (mut flash, _) = gathered(&old, 14);
    let new = build(5000, 15, None, false);
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
    let built = build(2 * DATA * PIECE, 17, None, false);
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
fn the_store_takes_under_4_kib() {
    // On this 64-bit host; the decode's factors take half as many bytes on
    // the chip. The lanes' sets of pieces held are 48 × 32 B of it.
    std::println!("Store {} B", core::mem::size_of::<Store>());
    assert!(core::mem::size_of::<Store>() <= 4000);
}

#[test]
fn a_holder_makes_the_blocks_a_gatherer_takes() {
    let firmware = 3 * DATA * PIECE + 1234;
    let built = build(firmware, 21, None, false);
    let header: &[u8; HEADER_LEN] = built.stream[..HEADER_LEN].try_into().unwrap();
    let image = held(firmware as u32, 7, 1).unwrap();
    assert_eq!(image, built.image);
    let mut table = Table::new(image, &name(header));
    let mut tabling = Tabling::new();
    for chunk in built.stream[..HEADER_LEN + firmware].chunks(1000) {
        tabling.feed(&mut table, chunk);
    }
    let hash = tabling.finish(&mut table);
    assert_eq!(
        hash.firmware,
        sha(&built.stream[HEADER_LEN..HEADER_LEN + firmware])
    );
    assert_eq!(hash.table, table_hash(&table.bytes, image.blocks));
    // Fed by halves, a part at a time, as the firmware feeds it.
    let mut halves = Table::new(image, &name(header));
    let mut tabling = Tabling::new();
    for chunk in built.stream[..HEADER_LEN + firmware].chunks(64) {
        tabling.firmware(chunk);
        tabling.blocks(&mut halves, chunk);
    }
    assert_eq!(tabling.finish(&mut halves), hash);
    assert_eq!(halves.bytes, table.bytes);
    assert_eq!(table.image(), image);
    for (i, piece) in built.blocks[0].iter().enumerate() {
        assert_eq!(table.piece(i), Some(piece));
    }
    assert_eq!(table.piece(built.blocks[0].len()), None);
    for b in 1..image.blocks {
        for i in 0..image.pieces(b) {
            let at = piece_offset(b, i) as usize;
            assert_eq!(
                &built.stream[at..at + PIECE],
                &built.blocks[usize::from(b)][i]
            );
        }
    }
    // One block more than the lanes is not held.
    let largest = ((MAX_BLOCKS - 1) * DATA * PIECE - HEADER_LEN) as u32;
    assert!(held(largest, 7, 1).is_some());
    assert_eq!(held(largest + 1, 7, 1), None);
}

#[test]
fn a_gatherer_sends_the_blocks_it_decoded() {
    let built = small();
    let (mut flash, _) = gathered(&built, 22);
    let mut store = Store::open(&mut flash).unwrap();
    while store.poll(&mut flash).unwrap() != Poll::Idle {}
    let mut out = [0u8; PIECE];
    for (b, block) in built.blocks.iter().enumerate() {
        for (i, piece) in block.iter().enumerate() {
            assert!(store.data_piece(&mut flash, b as u8, i, &mut out).unwrap());
            assert_eq!(&out, piece, "block {b} piece {i}");
        }
        let past = block.len();
        assert!(!store
            .data_piece(&mut flash, b as u8, past, &mut out)
            .unwrap());
    }
}

/// A flash that checks each step's work: every step sees first whether
/// the chip is done, then moves at most one transfer of at most `CHUNK`
/// bytes over the bus.
struct Metered {
    ram: Ram,
    /// Transfers since the last `done`.
    since_done: usize,
    reads: usize,
    programs: usize,
}

impl Metered {
    fn transfer(&mut self, len: usize) {
        assert!(len <= CHUNK, "a transfer of {len} bytes in a step");
        self.since_done += 1;
        assert_eq!(self.since_done, 1, "two transfers in a step");
    }
}

impl Flash for Metered {
    type Error = Cut;

    fn read(&mut self, address: u32, bytes: &mut [u8]) -> Result<(), Cut> {
        self.transfer(bytes.len());
        self.reads += 1;
        self.ram.read(address, bytes)
    }

    fn start_program(&mut self, address: u32, data: &[u8]) -> Result<(), Cut> {
        self.transfer(data.len());
        self.programs += 1;
        self.ram.start_program(address, data)
    }

    fn start_erase(&mut self, address: u32) -> Result<(), Cut> {
        self.transfer(0);
        self.ram.start_erase(address)
    }

    fn done(&mut self) -> Result<bool, Cut> {
        self.since_done = 0;
        self.ram.done()
    }
}

#[test]
fn every_step_moves_at_most_a_chunk() {
    // Three blocks, the pieces drawn at random, so that groups lack data
    // pieces and read every piece of their block; then reopened, so that
    // the head and every slot are read again.
    let built = build(2 * DATA * PIECE, 24, None, false);
    let mut flash = Metered {
        ram: Ram::new(),
        since_done: 0,
        reads: 0,
        programs: 0,
    };
    let mut store = Store::open(&mut flash).unwrap();
    store.begin(built.image);
    let mut feeder = Feeder::new(&built, 25);
    feeder.gather(&built, &mut store, &mut flash).unwrap();
    assert!(store.whole());
    let read = flash.reads;
    let mut again = Store::open(&mut flash).unwrap();
    assert_eq!(again.begin(built.image), Begin::Resumed);
    while again.poll(&mut flash).unwrap() != Poll::Idle {}
    assert!(again.whole());
    // Some 4 000 slots were read again, a 256-byte slot in 16 steps.
    assert!(flash.reads - read >= LANES * DATA * 16);
    assert!(flash.programs > 0);
}

#[test]
fn a_step_waits_for_another_users_program() {
    let built = small();
    let (mut flash, _) = gathered(&built, 26);
    let mut store = Store::EMPTY;
    store.reopen();
    // The mailbox programs a page meanwhile: the store reads nothing
    // until it is done (the fake asserts no read while the chip works).
    flash.start_program(0, &[0xff]).unwrap();
    assert_eq!(store.poll(&mut flash).unwrap(), Poll::Busy);
    while !store.opened() {
        store.poll(&mut flash).unwrap();
    }
    assert_eq!(store.image(), Some(built.image));
}

#[test]
fn the_record_keeps_the_version_and_key() {
    let built = small();
    let mut flash = Ram::new();
    let mut store = Store::open(&mut flash).unwrap();
    store.begin(built.image);
    while store.poll(&mut flash).unwrap() != Poll::Idle {}
    assert!(store.ready());
    let again = Store::open(&mut flash).unwrap();
    assert_eq!(again.image(), Some(built.image));
}

/// A holder makes every coded piece of a one-piece table, of full blocks
/// and of a short last block, as the gatherer's blocks are, and any of
/// them gives the block back.
#[test]
fn a_holder_serves_every_piece_of_every_block() {
    // Pieces 64 + 64 + 53 behind the table, the last padded.
    let firmware = 181 * PIECE - HEADER_LEN - 100;
    let built = build(firmware, 23, None, false);
    let header: &[u8; HEADER_LEN] = built.stream[..HEADER_LEN].try_into().unwrap();
    let image = held(firmware as u32, 7, 1).unwrap();
    assert_eq!(image, built.image);
    assert_eq!(
        (0..image.blocks)
            .map(|b| image.pieces(b))
            .collect::<Vec<_>>(),
        [1, 64, 64, 53]
    );
    let mut table = Table::new(image, &name(header));
    let mut tabling = Tabling::new();
    tabling.feed(&mut table, &built.stream[..HEADER_LEN + firmware]);
    tabling.finish(&mut table);
    let body = &built.stream[HEADER_LEN..HEADER_LEN + firmware];
    let data = |b: u8, i: usize| -> [u8; PIECE] {
        match b {
            0 => *table.piece(i).unwrap(),
            _ => {
                let mut out = [0u8; PIECE];
                held_piece(header, firmware as u32, b, i, &mut out, |at, bytes| {
                    let at = at as usize;
                    bytes.copy_from_slice(&body[at..at + bytes.len()]);
                    Ok::<(), ()>(())
                })
                .unwrap();
                out
            }
        }
    };
    for b in 0..image.blocks {
        let n = image.pieces(b);
        let block = &built.blocks[usize::from(b)];
        let mut coded = Vec::new();
        for k in 0..CODED as u8 {
            let mut e = crate::code::Encoder::new(n, k).unwrap();
            for i in 0..n {
                if e.wants(i) {
                    e.add(i, &data(b, i));
                }
            }
            let piece = e.finish();
            assert_eq!(
                piece,
                encode_piece(block, k).unwrap(),
                "block {b} piece {k}"
            );
            coded.push(piece);
        }
        // The last n pieces, none of them data, give the block back.
        let indices: Vec<u8> = (CODED - n..CODED).map(|k| k as u8).collect();
        let mut out = vec![[0u8; PIECE]; n];
        crate::code::decode(&indices, &coded[CODED - n..], &mut out).unwrap();
        assert_eq!(&out, block, "block {b}");
    }
}

/// The table hash as `bark-fw/scripts/uf2.py`'s `table_hash` makes it, of
/// firmware of bytes `i % 251`: a short last block, two whole blocks, a
/// byte; `bark-fw/scripts/test_image.py` holds the script to the same.
#[test]
fn the_table_hash_is_the_scripts() {
    let hex = |s: &str| -> [u8; HASH_LEN] {
        core::array::from_fn(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
    };
    for (n, want) in [
        (
            40_000,
            "60157232e6c3bf5dec13d14b5770636e0047f1d46b93521762a5db6a55817bb3",
        ),
        (
            2 * DATA * PIECE - HEADER_LEN,
            "838fcb1a622ac4ac1e1f104a28fbf8e7626d45465169447342cc09d85948b312",
        ),
        (
            1,
            "42c4462d671587364a6d739226b8c42ff1b1aa9f53399a8b25f723bb7b420efa",
        ),
    ] {
        let firmware: Vec<u8> = (0..n).map(|i| (i % 251) as u8).collect();
        let mut entries = Entries::new();
        entries.feed(&[0xee; HEADER_LEN], |_, _| ());
        entries.feed(&firmware, |_, _| ());
        assert_eq!(entries.finish(n as u32, |_, _| ()), hex(want), "{n} bytes");
    }
}
