//! The pieces of an image a node gathers over the mesh (roadmap D149,
//! D151; `docs/mesh-update-2026-10-25.md`, "The code"), kept on the
//! external flash until their block decodes, and the decoded image,
//! written into the staged slot as the phone's staging writes it
//! (`crate::boot::Stager`).
//!
//! **What is gathered.** The image as packed: its 256-byte header, then
//! the bytes of the firmware's slot (`bark-fw/README.md`, "Updates"), in
//! blocks of [`DATA`] pieces of [`PIECE`] bytes, the last block shorter
//! and its last piece padded. Block 0 is the block table: the image's name,
//! then the first 8 bytes of the SHA-256 of each other block's pieces,
//! padded with zeros to whole pieces. The name is the SHA-256 of the
//! header's first [`SIGNED_LEN`] bytes, the part its signatures cover
//! (`crate::sign`): the same image signed again with another key keeps
//! its name, so the mesh sees one image, whichever of its signatures a
//! holder has. The mesh names it by the name's first [`TAG_LEN`] bytes,
//! in a heartbeat and in every piece; the store begins with those, and
//! takes the whole name from block 0.
//!
//! **Where it goes**, in [`PIECES`]' 193 sectors:
//!
//! | Sector | Holds |
//! |---|---|
//! | 0, the head | the image gathered and its check; a byte for each block, cleared once it is decoded and written; the block table; the header |
//! | 1 to 192 | [`LANES`], 48, lanes of four sectors, 64 slots: block `b`'s pieces in lane `b` |
//!
//! A node takes the pieces of every block it lacks, whichever comes, since
//! a holder streams all the blocks it holds in turn (roadmap D151), so an
//! image has at most [`MAX_BLOCKS`], 48, the table's among them; a lane is
//! freed by erasing its own four sectors. A slot is one page program of 256
//! bytes: the piece, its block, its index, and a CRC-32 (ISO-HDLC, as in
//! IEEE 802.3) of those 249 bytes; the last three bytes stay erased. A slot
//! cut short by a power cut is neither erased nor passes its check, and
//! holds nothing; slots are written in order, so the first after the last
//! that is not erased is the next to write. The decoded image goes into
//! [`DFU`] from its start, the header aside into the head, so the staged
//! slot holds what the loader swaps in.
//!
//! **How a block is decoded.** When a lane holds as many pieces as its
//! block has, its slots are read for the pieces to use, one a step, and
//! the block is decoded twice, two data pieces at a time, each two from
//! one read of each piece the lane holds ([`crate::code`]).
//! The first time it is checked: block 0 must begin with the first bytes
//! of the image's name, any other block hash to its entry in the table,
//! and block 1, which begins with the header, have a header whose signed
//! bytes hash to the name block 0 gave; its signatures are checked once the image is whole. A block
//! that fails is erased from its lane and gathered again
//! ([`Poll::Failed`]). The second time it is written, and its byte in the
//! head cleared ([`Poll::Decoded`]); then its lane is erased. Nothing
//! that failed is ever written, so every byte written is the one that
//! belongs there, and a write done again after a power cut writes the
//! same bytes. A block of 64 none of whose data pieces came reads its 64
//! slots, then 4 096 pieces, 32 groups of 64 twice over, and does about 2
//! million multiply-adds of a byte; at the bus's 2 088 ms for 256 KiB read
//! (`docs/roadmap.md`, "The external flash's bus"), 2 ms a piece, and the
//! code's 14 cycles a byte, about 8.3 s of reads and 0.4 s of sums, in
//! steps of one piece read and its sums, 0.11 ms; a block takes at least
//! the 80 pieces of a holder's quota a gap of 4.16 s apart, 5.5 minutes, to
//! gather (spec "Updates over the mesh"), so the decode is under 3 % of
//! that.
//!
//! **What a reset keeps.** The head: the image, which blocks are done, the
//! table and the header; the decoded blocks in [`DFU`]; and every slot
//! written whole. [`Store::open`] reads the head, and [`Store::poll`] then
//! reads every slot, one a step, and erases any lane that holds what is
//! not wanted: a block done, a block in another block's lane, or one
//! whose slots cut short leave too few for its pieces. Lost and gathered again: the
//! pieces queued and not yet written, a decode under way, done again from
//! the slots, and an erase under way, done again.
//!
//! **Pacing.** [`Store::poll`] does one step: it starts a program or an
//! erase and returns, finds at its next call whether the chip is done, or
//! reads one slot and adds it into the sums. The firmware calls it between
//! radio events until it says [`Poll::Idle`]. Pieces handed to
//! [`Store::offer`] wait in RAM until a step writes them, before any other
//! work. The firmware must not stage from the phone while a gathering's
//! image is in [`DFU`], nor begin one while a swap is asked for or the
//! running image is on trial ([`crate::boot::swap_requested`]): both
//! write the same slot. Once the image is whole, the header's signature
//! and the image's hash are checked and the record written for the loader
//! as for an image from the phone; that is the firmware's, not this
//! module's.
//!
//! **RAM.** 3 928 B in the node image (`llvm-nm`, 2026-11-08): the two
//! data pieces of the group decoded, 494 B; the pieces used and their
//! slots, 128 B, and their factors; two slots queued, 516 B; the table,
//! two pieces, 494 B; each lane's set of the pieces it holds, a bit an
//! index, 32 B, 1.5 KiB for the 48. Two data pieces at a time and not
//! eight keep 1.5 KB for the stack, for 6 s more of reads a block.

use core::cmp::min;

use sha2::{Digest, Sha256};

use crate::code::{mul_add, At, Points, CODED, DATA, PIECE};
use crate::ext_flash::{self, Bus, Chip, PROGRAM_PAGE};
use crate::layout::external::{DFU, PIECES, SECTOR};
use crate::layout::ACTIVE;
use crate::sign::SIGNED_LEN;
use crate::{HASH_LEN, HEADER_LEN};

/// A NOR flash that erases and programs while the processor goes on: the
/// external chip ([`Chip`]), addressed from its start.
pub trait Flash {
    /// Why an operation failed.
    type Error;
    /// Read `bytes` at `address`; never called while a program or an erase
    /// runs.
    fn read(&mut self, address: u32, bytes: &mut [u8]) -> Result<(), Self::Error>;
    /// Start programming `data` at `address`, within one page of
    /// [`PROGRAM_PAGE`] bytes, erased since it was last programmed.
    fn start_program(&mut self, address: u32, data: &[u8]) -> Result<(), Self::Error>;
    /// Start erasing the sector at `address`.
    fn start_erase(&mut self, address: u32) -> Result<(), Self::Error>;
    /// Whether the last program or erase has ended.
    fn done(&mut self) -> Result<bool, Self::Error>;
}

impl<B: Bus> Flash for Chip<B> {
    type Error = ext_flash::Error;

    fn read(&mut self, address: u32, bytes: &mut [u8]) -> Result<(), Self::Error> {
        embedded_storage::nor_flash::ReadNorFlash::read(self, address, bytes)
    }

    fn start_program(&mut self, address: u32, data: &[u8]) -> Result<(), Self::Error> {
        Chip::start_program(self, address, data)
    }

    fn start_erase(&mut self, address: u32) -> Result<(), Self::Error> {
        Chip::start_erase(self, address)
    }

    fn done(&mut self) -> Result<bool, Self::Error> {
        // A chip stuck at an operation keeps the store at `Busy`: the
        // caller gives up after the operation's maximum time, as
        // `Chip::finish` does.
        Ok(Chip::done(self))
    }
}

/// Bytes of a slot: one page program.
pub const SLOT: u32 = PROGRAM_PAGE;

/// Blocks whose pieces a node keeps at once: every block of the image,
/// one lane each, since a holder streams every block it holds and a node
/// keeps every piece of every block it lacks (roadmap D151).
pub const LANES: usize = crate::layout::external::PIECE_LANES as usize;

const SLOTS_PER_SECTOR: usize = (SECTOR / SLOT) as usize;

/// A lane's sectors: room for a whole block's pieces.
const LANE_SECTORS: u32 = DATA.div_ceil(SLOTS_PER_SECTOR) as u32;

const HEAD: u32 = PIECES.start;
const LANES_START: u32 = HEAD + SECTOR;

// The slot: the piece, its block and index, the check.
const BLOCK_AT: usize = PIECE;
const INDEX_AT: usize = PIECE + 1;
const CHECK_AT: usize = PIECE + 2;

/// Bytes of a whole block.
const BLOCK_BYTES: u32 = (DATA * PIECE) as u32;

/// Bytes of a block's entry in the table.
const ENTRY: usize = 8;

/// The most blocks an image can have, the table among them: a lane each,
/// and no more than fit the running slot behind its header.
pub const MAX_BLOCKS: usize = {
    let slot = 1 + (HEADER_LEN as u32 + ACTIVE.len).div_ceil(BLOCK_BYTES) as usize;
    if slot < LANES {
        slot
    } else {
        LANES
    }
};

/// The longest table's pieces.
const TABLE_PIECES: usize = (HASH_LEN + ENTRY * (MAX_BLOCKS - 1)).div_ceil(PIECE);

// The head: the image's record at its start, then the done bytes, the
// table and the header, each from a page of its own.
// The record: the tag, blocks, last, version, key, then the check.
const CHECKED: usize = TAG_LEN + 2 + 4 + 1;
const RECORD_LEN: usize = CHECKED + 4;
const DONE_AT: u32 = 64;
const TABLE_AT: u32 = SLOT;
const HEADER_AT: u32 = 4 * SLOT;

/// Pieces waiting in RAM to be written: one arrives at most every 4.16 s
/// from a holder, the update's gap (roadmap D156), and is written in
/// 1.3 ms ([`ext_flash::PROGRAM_TYP_US`]) before any other step, so two
/// are room for two holders' pieces at once.
const QUEUE: usize = 2;

/// Data pieces decoded at once: 494 bytes of RAM, and the lane read 32
/// times a pass for a block of 64 none of whose data pieces came.
const GROUP: usize = 2;

/// A slot's piece index that is no piece.
const NONE: u8 = CODED as u8;

const _: () = {
    assert!(LANES_START + LANES as u32 * LANE_SECTORS * SECTOR <= PIECES.end());
    assert!(CHECK_AT + 4 <= SLOT as usize);
    // A block's done bit.
    assert!(MAX_BLOCKS <= 64);
    assert!(RECORD_LEN as u32 <= DONE_AT);
    assert!(DONE_AT + MAX_BLOCKS as u32 <= TABLE_AT);
    assert!(TABLE_AT + (TABLE_PIECES * PIECE) as u32 <= HEADER_AT);
    assert!(HEADER_AT + HEADER_LEN as u32 <= SECTOR);
    assert!(HEADER_AT.is_multiple_of(PROGRAM_PAGE));
    assert!(TABLE_PIECES <= GROUP);
};

/// CRC-32, ISO-HDLC (ISO/IEC 13239; IEEE 802.3's): reflected polynomial
/// `0xEDB88320`, all ones in and out.
fn crc32(bytes: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in bytes {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = (c >> 1) ^ (0xEDB8_8320 & (c & 1).wrapping_neg());
        }
    }
    !c
}

/// Bytes of a name the mesh carries in a heartbeat and in a piece: its
/// first four (spec "Updates over the mesh").
pub const TAG_LEN: usize = 4;

/// An image to gather, as a neighbour's heartbeat describes it: the
/// first bytes of its name, the whole of which block 0 carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Image {
    /// The first [`TAG_LEN`] bytes of the SHA-256 of its header's signed
    /// bytes, its name.
    pub tag: [u8; TAG_LEN],
    /// Its blocks, the table among them.
    pub blocks: u8,
    /// The pieces of its last block.
    pub last: u8,
    /// Its version and the key the section named, kept so that after a
    /// reset the node says what it gathers before block 1 brings the
    /// header.
    pub version: u32,
    /// See `version`.
    pub key: u8,
}

impl Image {
    /// The data pieces of block `b`; 0 past the last.
    pub fn pieces(&self, b: u8) -> usize {
        match b {
            0 => (HASH_LEN + ENTRY * usize::from(self.blocks.saturating_sub(1))).div_ceil(PIECE),
            _ if b >= self.blocks => 0,
            _ if b + 1 == self.blocks => usize::from(self.last),
            _ => DATA,
        }
    }

    /// Bytes of the image as gathered, its last piece padded.
    fn len(&self) -> u32 {
        (u32::from(self.blocks - 2) * DATA as u32 + u32::from(self.last)) * PIECE as u32
    }

    /// Whether it has a header and fits the running slot behind it.
    fn fits(&self) -> bool {
        (2..=MAX_BLOCKS).contains(&usize::from(self.blocks))
            && (1..=DATA).contains(&usize::from(self.last))
            && self.len() > HEADER_LEN as u32
            && self.len() - (PIECE as u32) < HEADER_LEN as u32 + ACTIVE.len
    }

    /// The sectors of [`DFU`] it writes.
    fn dfu_sectors(&self) -> u32 {
        (self.len() - HEADER_LEN as u32).div_ceil(SECTOR)
    }

    fn record(&self) -> [u8; RECORD_LEN] {
        let mut r = [0u8; RECORD_LEN];
        r[..TAG_LEN].copy_from_slice(&self.tag);
        r[TAG_LEN] = self.blocks;
        r[TAG_LEN + 1] = self.last;
        r[TAG_LEN + 2..TAG_LEN + 6].copy_from_slice(&self.version.to_le_bytes());
        r[TAG_LEN + 6] = self.key;
        let check = crc32(&r[..CHECKED]);
        r[CHECKED..].copy_from_slice(&check.to_le_bytes());
        r
    }

    fn from_record(r: &[u8; RECORD_LEN]) -> Option<Image> {
        let c = CHECKED;
        let check = u32::from_le_bytes([r[c], r[c + 1], r[c + 2], r[c + 3]]);
        if crc32(&r[..c]) != check {
            return None;
        }
        let mut tag = [0u8; TAG_LEN];
        tag.copy_from_slice(&r[..TAG_LEN]);
        let v = TAG_LEN + 2;
        let image = Image {
            tag,
            blocks: r[TAG_LEN],
            last: r[TAG_LEN + 1],
            version: u32::from_le_bytes([r[v], r[v + 1], r[v + 2], r[v + 3]]),
            key: r[TAG_LEN + 6],
        };
        image.fits().then_some(image)
    }
}

/// The name of the image `header` heads: the SHA-256 of the bytes its
/// signatures cover.
pub fn name(header: &[u8; HEADER_LEN]) -> [u8; HASH_LEN] {
    Sha256::digest(&header[..SIGNED_LEN]).into()
}

/// The image of `header`, with `length` bytes of firmware behind it, as
/// a holder tells and sends it: `None` if it has more blocks than a lane
/// each.
pub fn held(header: &[u8; HEADER_LEN], length: u32, version: u32, key: u8) -> Option<Image> {
    let pieces = (HEADER_LEN as u32 + length).div_ceil(PIECE as u32);
    let blocks = 1 + pieces.div_ceil(DATA as u32);
    if blocks as usize > MAX_BLOCKS {
        return None;
    }
    let mut tag = [0u8; TAG_LEN];
    tag.copy_from_slice(&name(header)[..TAG_LEN]);
    Some(Image {
        tag,
        blocks: blocks as u8,
        last: (pieces - (blocks - 2) * DATA as u32) as u8,
        version,
        key,
    })
}

/// Where data piece `i` of block `block`, 1 or later, starts in the
/// packed image: blocks of [`DATA`] pieces from the header on.
pub fn piece_offset(block: u8, i: usize) -> u32 {
    ((u32::from(block) - 1) * DATA as u32 + i as u32) * PIECE as u32
}

/// Data piece `i` of block `block`, 1 or later, of an image a node
/// holds as its `header` and `length` bytes of firmware, which `read`
/// gives from the firmware's start: the header's bytes, then the
/// firmware's, then zeros to the piece's end.
pub fn held_piece<E>(
    header: &[u8; HEADER_LEN],
    length: u32,
    block: u8,
    i: usize,
    out: &mut [u8; PIECE],
    mut read: impl FnMut(u32, &mut [u8]) -> Result<(), E>,
) -> Result<(), E> {
    let at = piece_offset(block, i);
    let end = at + PIECE as u32;
    out.fill(0);
    // The header's part.
    let head = (HEADER_LEN as u32).clamp(at, end);
    if at < head {
        out[..(head - at) as usize].copy_from_slice(&header[at as usize..head as usize]);
    }
    // The firmware's part, from where the header ends to the image's end.
    let to = (HEADER_LEN as u32 + length).clamp(head, end);
    if head < to {
        let o = (head - at) as usize;
        read(
            head - HEADER_LEN as u32,
            &mut out[o..o + (to - head) as usize],
        )?;
    }
    Ok(())
}

/// Block 0 of an image a node holds: the name, then 8 bytes of the
/// SHA-256 of each other block's pieces, the last padded with zeros as it
/// travels, made by a [`Tabling`].
pub struct Table {
    image: Image,
    bytes: [u8; TABLE_PIECES * PIECE],
}

impl Table {
    /// The table of `image`, named `name`, its entries yet to be made.
    pub fn new(image: Image, name: &[u8; HASH_LEN]) -> Table {
        let mut bytes = [0u8; TABLE_PIECES * PIECE];
        bytes[..HASH_LEN].copy_from_slice(name);
        Table { image, bytes }
    }

    /// The image it is of.
    pub fn image(&self) -> Image {
        self.image
    }

    /// Data piece `i` of block 0.
    pub fn piece(&self, i: usize) -> Option<&[u8; PIECE]> {
        if i >= self.image.pieces(0) {
            return None;
        }
        self.bytes[i * PIECE..(i + 1) * PIECE].try_into().ok()
    }
}

/// The making of a [`Table`] from the packed image's bytes, fed in order,
/// and the SHA-256 of its firmware, to check against its header's. It
/// holds only the hashes, so that the table can lie where it is kept.
pub struct Tabling {
    block: Sha256,
    firmware: Sha256,
    /// Bytes fed.
    fed: u32,
}

impl Tabling {
    /// Before any byte is fed.
    pub fn new() -> Tabling {
        Tabling {
            block: Sha256::new(),
            firmware: Sha256::new(),
            fed: 0,
        }
    }

    /// Feed the next bytes of the packed image into `table`: its header,
    /// then its firmware.
    pub fn feed(&mut self, table: &mut Table, bytes: &[u8]) {
        let past_header = (HEADER_LEN as u32).saturating_sub(self.fed) as usize;
        if past_header < bytes.len() {
            self.firmware.update(&bytes[past_header..]);
        }
        self.hash(table, bytes);
    }

    fn hash(&mut self, table: &mut Table, mut bytes: &[u8]) {
        while !bytes.is_empty() {
            let room = BLOCK_BYTES - self.fed % BLOCK_BYTES;
            let n = bytes.len().min(room as usize);
            self.block.update(&bytes[..n]);
            self.fed += n as u32;
            bytes = &bytes[n..];
            if self.fed.is_multiple_of(BLOCK_BYTES) {
                self.entry(table);
            }
        }
    }

    /// Once every byte was fed: the last block padded into `table`, and
    /// the SHA-256 of the firmware fed.
    pub fn finish(mut self, table: &mut Table) -> [u8; HASH_LEN] {
        let end = table.image.len();
        let zeros = [0u8; PIECE];
        while self.fed < end {
            let n = (end - self.fed).min(PIECE as u32) as usize;
            self.hash(table, &zeros[..n]);
        }
        if !self.fed.is_multiple_of(BLOCK_BYTES) {
            self.entry(table);
        }
        self.firmware.finalize().into()
    }

    /// The block just fed whole: its entry.
    fn entry(&mut self, table: &mut Table) {
        let block = (self.fed - 1) / BLOCK_BYTES;
        let at = HASH_LEN + ENTRY * block as usize;
        let hash = core::mem::take(&mut self.block).finalize();
        if let Some(entry) = table.bytes.get_mut(at..at + ENTRY) {
            entry.copy_from_slice(&hash[..ENTRY]);
        }
    }
}

impl Default for Tabling {
    fn default() -> Self {
        Tabling::new()
    }
}

/// What a step did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Poll {
    /// Nothing to do until a piece is offered.
    Idle,
    /// The chip is still at a program or an erase: poll again later.
    Busy,
    /// A step was done: poll again.
    Again,
    /// The block was decoded and written, and stays so across a reset.
    Decoded(u8),
    /// The block failed its check, and its pieces are erased: its pieces
    /// are wanted again.
    Failed(u8),
}

/// What became of a piece offered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Offer {
    /// Queued, to be written.
    Kept,
    /// The lane holds this piece already.
    Held,
    /// The block has as many pieces as it needs.
    Enough,
    /// The block is decoded already.
    Decoded,
    /// No block of the image, or not a piece index.
    Outside,
    /// No room just now: the queue is full, or the lane is being erased.
    Busy,
    /// No image is gathered, or the store is still clearing or reading.
    NotReady,
}

/// What [`Store::begin`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Begin {
    /// The store was gathering this image: it goes on.
    Resumed,
    /// A new image: the store clears its regions first.
    Started,
    /// An image with no header or too large for the running slot.
    Unfit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// No image.
    Empty,
    /// Erasing the head, the lanes and the image's sectors of [`DFU`],
    /// this many done.
    Clear(u32),
    /// Writing the image's record into the head.
    Record,
    /// Reading the slots after a reset.
    Scan { lane: u8, slot: u8 },
    /// Taking pieces.
    Gather,
    /// Erasing the head.
    Forget,
}

#[derive(Clone, Copy, Debug)]
struct Lane {
    /// The block whose pieces it holds.
    block: Option<u8>,
    /// The pieces its slots hold or are queued for, a bit an index: which
    /// slot holds which is read from the slots when the block is decoded.
    held: [u8; CODED.div_ceil(8)],
    /// The first slot not written since the lane was erased.
    next: u8,
    /// Distinct pieces in its slots or queued for them.
    taken: u8,
    /// Of them, those written whole.
    whole: u8,
    /// Found holding pieces of two blocks.
    mixed: bool,
    /// Being erased: the next sector, and the block to report failed.
    erase: Option<(u8, Option<u8>)>,
}

impl Lane {
    const FREE: Lane = Lane {
        block: None,
        held: [0; CODED.div_ceil(8)],
        next: 0,
        taken: 0,
        whole: 0,
        mixed: false,
        erase: None,
    };

    fn holds(&self, index: u8) -> bool {
        self.held[usize::from(index / 8)] >> (index % 8) & 1 == 1
    }

    fn hold(&mut self, index: u8) {
        self.held[usize::from(index / 8)] |= 1 << (index % 8);
    }
}

#[derive(Clone, Copy, Debug)]
struct Queued {
    lane: u8,
    slot: u8,
    bytes: [u8; SLOT as usize],
}

/// A flash operation started and not yet seen done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Slot {
        lane: u8,
    },
    LaneSector {
        lane: u8,
    },
    Clear,
    Record,
    Write,
    Mark {
        block: u8,
    },
    Forget,
    /// Started for an image since given up.
    Orphan,
}

/// A block being decoded, a group of data pieces at a time.
struct Decode {
    lane: u8,
    block: u8,
    n: u8,
    /// The pieces used, and their slots: read from the lane's slots first,
    /// one a step, until `n` are found.
    points: Points,
    indices: [u8; DATA],
    slots: [u8; DATA],
    found: u8,
    /// The next slot to read for them.
    scan: u8,
    /// Second time through: writing.
    writing: bool,
    /// The group: data pieces from `group * GROUP`.
    group: u8,
    /// The next of the pieces used to read for this group.
    next: u8,
    /// Bytes of the group written.
    wrote: u16,
    /// The factors for each data piece of the group.
    at: [Option<At>; GROUP],
    sha: Sha256,
    /// Block 1's header hashed to the name.
    header_ok: bool,
    out: [[u8; PIECE]; GROUP],
}

impl Decode {
    fn first(&self) -> usize {
        usize::from(self.group) * GROUP
    }

    fn count(&self) -> usize {
        min(GROUP, usize::from(self.n) - self.first())
    }

    fn start_group(&mut self, group: u8) {
        self.group = group;
        self.next = 0;
        self.wrote = 0;
        self.out = [[0; PIECE]; GROUP];
        let first = self.first();
        let count = self.count();
        for (o, at) in self.at.iter_mut().enumerate() {
            *at = (o < count).then(|| self.points.at((first + o) as u8));
        }
    }

    /// Whether piece `j` counts towards this group: every piece if a data
    /// piece of the group is not held, else only those of the group.
    fn wants(&self, j: usize) -> bool {
        let first = self.first();
        let group = first..first + self.count();
        let lacking = group
            .clone()
            .any(|i| self.points.position(i as u8).is_none());
        lacking || group.contains(&usize::from(self.points.point(j)))
    }
}

/// An image's pieces and its decoded blocks on the external flash.
pub struct Store {
    image: Option<Image>,
    mode: Mode,
    /// A bit for each block decoded and written.
    done: u64,
    /// Block 0, once checked.
    table: [u8; TABLE_PIECES * PIECE],
    table_known: bool,
    lanes: [Lane; LANES],
    queue: [Option<Queued>; QUEUE],
    op: Option<Op>,
    decode: Option<Decode>,
}

fn lane_address(lane: usize) -> u32 {
    LANES_START + lane as u32 * LANE_SECTORS * SECTOR
}

fn slot_address(lane: usize, slot: usize) -> u32 {
    lane_address(lane) + slot as u32 * SLOT
}

impl Store {
    /// A store holding nothing, for a static the firmware opens in place
    /// ([`Store::reopen`]): at about 4 KB it is never built on the stack.
    pub const EMPTY: Store = Store {
        image: None,
        mode: Mode::Empty,
        done: 0,
        table: [0; TABLE_PIECES * PIECE],
        table_known: false,
        lanes: [Lane::FREE; LANES],
        queue: [None; QUEUE],
        op: None,
        decode: None,
    };

    /// The store as the flash holds it after a reset: the image and what
    /// is done from the head; its slots are read by the steps that follow.
    pub fn open<F: Flash>(flash: &mut F) -> Result<Store, F::Error> {
        let mut s = Store::EMPTY;
        s.reopen(flash)?;
        Ok(s)
    }

    /// [`Store::open`], in place.
    pub fn reopen<F: Flash>(&mut self, flash: &mut F) -> Result<(), F::Error> {
        self.drop_all();
        self.op = None;
        let mut record = [0u8; RECORD_LEN];
        flash.read(HEAD, &mut record)?;
        let Some(image) = Image::from_record(&record) else {
            return Ok(());
        };
        self.image = Some(image);
        let mut done = [0xffu8; MAX_BLOCKS];
        let blocks = usize::from(image.blocks);
        flash.read(HEAD + DONE_AT, &mut done[..blocks])?;
        for (b, &d) in done[..blocks].iter().enumerate() {
            // A byte cut short is cleared only once its block was written.
            if d != 0xff {
                self.done |= 1 << b;
            }
        }
        if self.is_done(0) {
            let len = image.pieces(0) * PIECE;
            flash.read(HEAD + TABLE_AT, &mut self.table[..len])?;
            self.table_known = true;
        }
        self.mode = Mode::Scan { lane: 0, slot: 0 };
        Ok(())
    }

    /// Gather `image`: go on with it if it is the one gathered, else clear
    /// what is kept and start.
    pub fn begin(&mut self, image: Image) -> Begin {
        if !image.fits() {
            return Begin::Unfit;
        }
        if self.image == Some(image) {
            return Begin::Resumed;
        }
        self.drop_all();
        self.image = Some(image);
        self.mode = Mode::Clear(0);
        Begin::Started
    }

    /// Give up the image: the head is erased, so that a reset finds none.
    pub fn forget(&mut self) {
        self.drop_all();
        self.mode = Mode::Forget;
    }

    /// Hold nothing, field by field, so that no whole store passes
    /// through the stack.
    fn drop_all(&mut self) {
        self.op = self.op.map(|_| Op::Orphan);
        self.image = None;
        self.mode = Mode::Empty;
        self.done = 0;
        self.table_known = false;
        self.lanes.fill(Lane::FREE);
        self.queue.fill(None);
        self.decode = None;
    }

    /// The image gathered.
    pub fn image(&self) -> Option<Image> {
        self.image
    }

    /// Whether it takes pieces: an image is gathered, and what a new one
    /// clears or a reset left is cleared and read.
    pub fn ready(&self) -> bool {
        self.image.is_some() && self.mode == Mode::Gather
    }

    /// Whether a flash operation it started may still run, or one is due:
    /// the head's erase after [`Store::forget`].
    pub fn busy(&self) -> bool {
        self.op.is_some() || self.mode == Mode::Forget
    }

    /// Data piece `i` of block `block`, once the block is decoded and
    /// written: what a node that gathers sends of it. False if it is not.
    pub fn data_piece<F: Flash>(
        &self,
        flash: &mut F,
        block: u8,
        i: usize,
        out: &mut [u8; PIECE],
    ) -> Result<bool, F::Error> {
        let Some(image) = self.image else {
            return Ok(false);
        };
        if !self.is_done(block) || i >= image.pieces(block) {
            return Ok(false);
        }
        if block == 0 {
            out.copy_from_slice(&self.table[i * PIECE..(i + 1) * PIECE]);
            return Ok(true);
        }
        let at = piece_offset(block, i);
        // The header's part lies in the head, the rest in the staged slot.
        let head = (HEADER_LEN as u32).saturating_sub(at).min(PIECE as u32) as usize;
        if head > 0 {
            flash.read(HEAD + HEADER_AT + at, &mut out[..head])?;
        }
        if head < PIECE {
            let from = at + head as u32 - HEADER_LEN as u32;
            flash.read(DFU.start + from, &mut out[head..])?;
        }
        Ok(true)
    }

    fn is_done(&self, block: u8) -> bool {
        self.done >> block & 1 == 1
    }

    /// The first block not yet decoded; `None` when the image is whole or
    /// none is gathered.
    pub fn lacks(&self) -> Option<u8> {
        let image = self.image?;
        (0..image.blocks).find(|&b| !self.is_done(b))
    }

    /// How many blocks are decoded and written.
    pub fn decoded(&self) -> u8 {
        self.done.count_ones() as u8
    }

    /// Whether every block is decoded and written.
    pub fn whole(&self) -> bool {
        self.image.is_some() && self.lacks().is_none() && self.mode == Mode::Gather
    }

    /// The block table, once block 0 is decoded: the name, then 8 bytes of
    /// each other block's hash.
    pub fn table(&self) -> Option<&[u8]> {
        let image = self.image?;
        let len = HASH_LEN + ENTRY * usize::from(image.blocks - 1);
        self.table_known.then(|| &self.table[..len])
    }

    /// The image's header, once block 1 is decoded.
    pub fn header<F: Flash>(&self, flash: &mut F) -> Result<Option<[u8; HEADER_LEN]>, F::Error> {
        if self.image.is_none() || !self.is_done(1) {
            return Ok(None);
        }
        let mut h = [0u8; HEADER_LEN];
        flash.read(HEAD + HEADER_AT, &mut h)?;
        Ok(Some(h))
    }

    /// Keep piece `index` of block `block`.
    pub fn offer(&mut self, block: u8, index: u8, piece: &[u8; PIECE]) -> Offer {
        let Some(image) = self.image else {
            return Offer::NotReady;
        };
        if self.mode != Mode::Gather {
            return Offer::NotReady;
        }
        if index == NONE || block >= image.blocks {
            return Offer::Outside;
        }
        if self.is_done(block) {
            return Offer::Decoded;
        }
        let lane_no = usize::from(block);
        let lane = &mut self.lanes[lane_no];
        if lane.erase.is_some() || lane.block.is_some_and(|b| b != block) {
            return Offer::Busy;
        }
        let next = usize::from(lane.next);
        if lane.holds(index) {
            return Offer::Held;
        }
        if usize::from(lane.taken) >= image.pieces(block) {
            return Offer::Enough;
        }
        if next >= DATA {
            return Offer::Busy;
        }
        let Some(q) = self.queue.iter_mut().find(|q| q.is_none()) else {
            return Offer::Busy;
        };
        let mut bytes = [0xffu8; SLOT as usize];
        bytes[..PIECE].copy_from_slice(piece);
        bytes[BLOCK_AT] = block;
        bytes[INDEX_AT] = index;
        let check = crc32(&bytes[..CHECK_AT]);
        bytes[CHECK_AT..CHECK_AT + 4].copy_from_slice(&check.to_le_bytes());
        *q = Some(Queued {
            lane: lane_no as u8,
            slot: next as u8,
            bytes,
        });
        lane.block = Some(block);
        lane.hold(index);
        lane.next += 1;
        lane.taken += 1;
        Offer::Kept
    }

    /// Do one step.
    pub fn poll<F: Flash>(&mut self, flash: &mut F) -> Result<Poll, F::Error> {
        if let Some(op) = self.op {
            if !flash.done()? {
                return Ok(Poll::Busy);
            }
            self.op = None;
            if let Some(event) = self.finished(op) {
                return Ok(event);
            }
        }
        let image = match (self.image, self.mode) {
            (_, Mode::Forget) => {
                flash.start_erase(HEAD)?;
                return self.started(Op::Forget);
            }
            (Some(image), _) => image,
            _ => return Ok(Poll::Idle),
        };
        match self.mode {
            Mode::Empty | Mode::Forget => Ok(Poll::Idle),
            Mode::Clear(done) => {
                let lanes = u32::from(image.blocks) * LANE_SECTORS;
                let address = match done {
                    0 => HEAD,
                    _ if done <= lanes => LANES_START + (done - 1) * SECTOR,
                    _ if done <= lanes + image.dfu_sectors() => {
                        DFU.start + (done - 1 - lanes) * SECTOR
                    }
                    _ => {
                        self.mode = Mode::Record;
                        return Ok(Poll::Again);
                    }
                };
                flash.start_erase(address)?;
                self.mode = Mode::Clear(done + 1);
                self.started(Op::Clear)
            }
            Mode::Record => {
                flash.start_program(HEAD, &image.record())?;
                self.started(Op::Record)
            }
            Mode::Scan { lane, slot } => {
                self.scan(flash, usize::from(lane), usize::from(slot))?;
                Ok(Poll::Again)
            }
            Mode::Gather => self.gather(flash, image),
        }
    }

    fn started<E>(&mut self, op: Op) -> Result<Poll, E> {
        self.op = Some(op);
        Ok(Poll::Again)
    }

    /// What follows `op` once the chip is done with it.
    fn finished(&mut self, op: Op) -> Option<Poll> {
        match op {
            Op::Slot { lane } => self.lanes[usize::from(lane)].whole += 1,
            Op::LaneSector { lane } => {
                let l = &mut self.lanes[usize::from(lane)];
                let (sector, failed) = l.erase?;
                if u32::from(sector) + 1 < LANE_SECTORS {
                    l.erase = Some((sector + 1, failed));
                } else {
                    *l = Lane::FREE;
                    return failed.map(Poll::Failed);
                }
            }
            Op::Record => self.mode = Mode::Gather,
            Op::Mark { block } => {
                self.done |= 1 << block;
                if block == 0 {
                    self.table_known = true;
                }
                self.decode = None;
                self.lanes[usize::from(block)].erase = Some((0, None));
                return Some(Poll::Decoded(block));
            }
            Op::Forget => self.mode = Mode::Empty,
            Op::Clear | Op::Write | Op::Orphan => {}
        }
        None
    }

    /// Read one slot after a reset.
    fn scan<F: Flash>(&mut self, flash: &mut F, lane: usize, slot: usize) -> Result<(), F::Error> {
        let mut bytes = [0u8; SLOT as usize];
        flash.read(slot_address(lane, slot), &mut bytes)?;
        let l = &mut self.lanes[lane];
        if bytes.iter().any(|&b| b != 0xff) {
            l.next = slot as u8 + 1;
            let check = u32::from_le_bytes([
                bytes[CHECK_AT],
                bytes[CHECK_AT + 1],
                bytes[CHECK_AT + 2],
                bytes[CHECK_AT + 3],
            ]);
            let (block, index) = (bytes[BLOCK_AT], bytes[INDEX_AT]);
            if crc32(&bytes[..CHECK_AT]) == check && index != NONE {
                match l.block {
                    None => l.block = Some(block),
                    Some(b) if b != block => l.mixed = true,
                    _ => {}
                }
                if !l.holds(index) {
                    l.hold(index);
                    l.taken += 1;
                    l.whole += 1;
                }
            }
        }
        self.mode = match (slot + 1 < DATA, lane + 1 < LANES) {
            (true, _) => Mode::Scan {
                lane: lane as u8,
                slot: slot as u8 + 1,
            },
            (false, more) => {
                self.judge(lane);
                match more {
                    true => Mode::Scan {
                        lane: lane as u8 + 1,
                        slot: 0,
                    },
                    false => Mode::Gather,
                }
            }
        };
        Ok(())
    }

    /// After a reset, erase a lane that holds what is not wanted.
    fn judge(&mut self, lane: usize) {
        let Some(image) = self.image else {
            return;
        };
        let l = self.lanes[lane];
        let keep = match l.block {
            None => l.next == 0,
            Some(b) => {
                let need = image.pieces(b);
                !l.mixed
                    && b < image.blocks
                    && usize::from(b) == lane
                    && !self.is_done(b)
                    && need - min(need, usize::from(l.whole)) <= DATA - usize::from(l.next)
            }
        };
        if !keep {
            self.lanes[lane].erase = Some((0, None));
        }
    }

    fn gather<F: Flash>(&mut self, flash: &mut F, image: Image) -> Result<Poll, F::Error> {
        if let Some(q) = self.queue[0] {
            self.queue.rotate_left(1);
            self.queue[QUEUE - 1] = None;
            flash.start_program(
                slot_address(usize::from(q.lane), usize::from(q.slot)),
                &q.bytes,
            )?;
            return self.started(Op::Slot { lane: q.lane });
        }
        if let Some(lane) = self.lanes.iter().position(|l| l.erase.is_some()) {
            let (sector, _) = self.lanes[lane].erase.unwrap_or_default();
            flash.start_erase(lane_address(lane) + u32::from(sector) * SECTOR)?;
            return self.started(Op::LaneSector { lane: lane as u8 });
        }
        if self.decode.is_some() {
            return self.decode_step(flash, image);
        }
        let ready = self.lanes.iter().enumerate().find_map(|(lane, l)| {
            let b = l.block?;
            let n = image.pieces(b);
            (usize::from(l.whole) >= n && l.taken == l.whole && (b == 0 || self.table_known))
                .then_some((lane, b, n))
        });
        match ready {
            Some((lane, block, n)) => {
                self.start_decode(lane, block, n);
                Ok(Poll::Again)
            }
            None => Ok(Poll::Idle),
        }
    }

    fn start_decode(&mut self, lane: usize, block: u8, n: usize) {
        self.decode = Some(Decode {
            lane: lane as u8,
            block,
            n: n as u8,
            points: Points::EMPTY,
            indices: [0; DATA],
            slots: [0; DATA],
            found: 0,
            scan: 0,
            writing: false,
            group: 0,
            next: 0,
            wrote: 0,
            at: [None; GROUP],
            sha: Sha256::new(),
            header_ok: block != 1,
            out: [[0; PIECE]; GROUP],
        });
    }

    /// Read the next slot of the lane decoded for the pieces to use: one
    /// written whole, of the block, not taken already.
    fn find_step<F: Flash>(&mut self, flash: &mut F) -> Result<Poll, F::Error> {
        let Some(d) = self.decode.as_mut() else {
            return Ok(Poll::Again);
        };
        let lane = usize::from(d.lane);
        if d.scan >= self.lanes[lane].next {
            // Not reached: the lane holds as many whole pieces as the block
            // has, each counted once.
            self.lanes[lane].erase = Some((0, Some(d.block)));
            self.decode = None;
            return Ok(Poll::Again);
        }
        let slot = usize::from(d.scan);
        d.scan += 1;
        let mut bytes = [0u8; SLOT as usize];
        flash.read(slot_address(lane, slot), &mut bytes)?;
        let check = u32::from_le_bytes([
            bytes[CHECK_AT],
            bytes[CHECK_AT + 1],
            bytes[CHECK_AT + 2],
            bytes[CHECK_AT + 3],
        ]);
        let index = bytes[INDEX_AT];
        let found = usize::from(d.found);
        if crc32(&bytes[..CHECK_AT]) == check
            && bytes[BLOCK_AT] == d.block
            && index != NONE
            && !d.indices[..found].contains(&index)
        {
            d.indices[found] = index;
            d.slots[found] = slot as u8;
            d.found += 1;
        }
        if d.found == d.n {
            match Points::new(&d.indices[..usize::from(d.n)]) {
                Ok(points) => {
                    d.points = points;
                    d.start_group(0);
                }
                // Not reached: the indices are distinct and below `CODED`.
                Err(_) => {
                    self.lanes[lane].erase = Some((0, Some(d.block)));
                    self.decode = None;
                }
            }
        }
        Ok(Poll::Again)
    }

    fn decode_step<F: Flash>(&mut self, flash: &mut F, image: Image) -> Result<Poll, F::Error> {
        let Store {
            decode,
            table,
            lanes,
            op,
            ..
        } = self;
        let Some(d) = decode.as_mut() else {
            return Ok(Poll::Again);
        };
        if d.found < d.n {
            return self.find_step(flash);
        }
        let n = usize::from(d.n);
        let count = d.count();
        // Read the next piece the group wants and add it in.
        if let Some(j) = (usize::from(d.next)..n).find(|&j| d.wants(j)) {
            let mut piece = [0u8; PIECE];
            let slot = usize::from(d.slots[j]);
            flash.read(slot_address(usize::from(d.lane), slot), &mut piece)?;
            for (out, at) in d.out.iter_mut().zip(&d.at).take(count) {
                if let Some(at) = at {
                    mul_add(out, d.points.factor(at, j), &piece);
                }
            }
            d.next = j as u8 + 1;
            return Ok(Poll::Again);
        }
        let more = d.first() + count < n;
        if !d.writing {
            if d.block == 0 {
                table[..count * PIECE].copy_from_slice(d.out[..count].as_flattened());
            } else {
                if d.block == 1 && d.group == 0 {
                    let signed = &d.out.as_flattened()[..SIGNED_LEN];
                    // The name block 0 carried, the table's start.
                    d.header_ok =
                        <[u8; HASH_LEN]>::from(Sha256::digest(signed))[..] == table[..HASH_LEN];
                }
                d.sha.update(d.out[..count].as_flattened());
            }
            if more {
                d.start_group(d.group + 1);
                return Ok(Poll::Again);
            }
            let ok = match d.block {
                0 => table[..TAG_LEN] == image.tag,
                b => {
                    let at = HASH_LEN + ENTRY * usize::from(b - 1);
                    let hash = core::mem::take(&mut d.sha).finalize();
                    d.header_ok && hash[..ENTRY] == table[at..at + ENTRY]
                }
            };
            if ok {
                d.writing = true;
                d.start_group(0);
            } else {
                lanes[usize::from(d.lane)].erase = Some((0, Some(d.block)));
                *decode = None;
            }
            return Ok(Poll::Again);
        }
        // Write the group, a page of the destination at a time.
        let bytes = count * PIECE;
        let wrote = usize::from(d.wrote);
        if wrote < bytes {
            let at = d.first() * PIECE + wrote;
            let (address, room) = match d.block {
                0 => (HEAD + TABLE_AT + at as u32, usize::MAX),
                b => {
                    let o = (u32::from(b) - 1) * BLOCK_BYTES + at as u32;
                    match o.checked_sub(HEADER_LEN as u32) {
                        None => (HEAD + HEADER_AT + o, HEADER_LEN - o as usize),
                        Some(o) => (DFU.start + o, usize::MAX),
                    }
                }
            };
            let len = (bytes - wrote)
                .min(room)
                .min((PROGRAM_PAGE - address % PROGRAM_PAGE) as usize);
            flash.start_program(address, &d.out.as_flattened()[wrote..wrote + len])?;
            d.wrote += len as u16;
            *op = Some(Op::Write);
            return Ok(Poll::Again);
        }
        if more {
            d.start_group(d.group + 1);
            return Ok(Poll::Again);
        }
        let block = d.block;
        flash.start_program(HEAD + DONE_AT + u32::from(block), &[0])?;
        *op = Some(Op::Mark { block });
        Ok(Poll::Again)
    }
}

#[cfg(test)]
mod tests;
