//! The mailbox's [`Ring`] in a region of NOR flash: its first page a log
//! of the state, the rest the ring's pages (spec "Mailbox";
//! `bark-image/src/layout.rs`, `MAILBOX`).
//!
//! The flash is read where it lies, but written later than the mailbox
//! asks: a write or an erase is queued as an [`Op`], read back through the
//! queue until it is done, and done by the driver when it can, which on
//! the node is between the radio's events (`bark-fw/src/mailbox.rs`). The
//! mailbox runs in a few microseconds and never waits on the flash.
//!
//! The state page is a log, as the node's stored state is: each state
//! goes into the next erased record with a check of its own, the last
//! whole record is the state, and only a full page is erased. A record
//! cut short by a power cut fails its check and the one before it is
//! taken; a cut between the erase and the write that follows it loses
//! the state, as the store's does. A slot cut short fails the mailbox's
//! own check and holds no item.

use crate::mailbox::{Ring, SLOTS_PER_PAGE, SLOT_LEN, STATE_LEN};

/// Bytes of an erase page (nRF52840 Product Specification, NVMC).
pub const PAGE_LEN: u32 = 4096;

/// Bytes of a state record: the state, its check, and erased bytes to a
/// power of two, 32 to a page.
pub const RECORD_LEN: usize = 128;
const RECORDS: u32 = PAGE_LEN / RECORD_LEN as u32;
const CHECK_AT: usize = STATE_LEN;
const _: () = assert!(CHECK_AT + 4 <= RECORD_LEN);

/// Ops queued at most. The mailbox queues at most two at a time, an erase
/// and a write, and the driver does them before it hands the mailbox the
/// next message, so this is room for a phone's write between.
pub const OPS: usize = 8;

/// The region's flash, read where it lies; offsets from its start.
pub trait Region {
    /// Pages in the region: the state's and the ring's.
    fn pages(&self) -> u32;
    /// The bytes at `offset`, into `out`.
    fn read(&self, offset: u32, out: &mut [u8]);
}

/// A write or an erase the driver is to do, at an offset in the region.
// Ops lie in a fixed queue whichever they are: an erase's room is a
// write's.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// Erase the page at `offset`.
    Erase {
        /// The page's first byte.
        offset: u32,
    },
    /// Write `bytes[..len]` at `offset`, erased since it was last written.
    Write {
        /// The first byte written.
        offset: u32,
        /// Bytes written.
        len: u16,
        /// What is written, its first `len` bytes.
        bytes: [u8; SLOT_LEN],
    },
}

impl Op {
    /// The bytes of a write.
    pub fn data(&self) -> &[u8] {
        match self {
            Op::Erase { .. } => &[],
            Op::Write { len, bytes, .. } => &bytes[..usize::from(*len)],
        }
    }
}

/// The ring and its state in a region of flash, with the writes and
/// erases not yet done.
pub struct FlashRing<F: Region> {
    flash: F,
    ops: [Option<Op>; OPS],
    /// The next erased state record; [`RECORDS`] when the page is full.
    next_record: u32,
    state: Option<[u8; STATE_LEN]>,
    /// Ops that found the queue full and were dropped.
    lost: u32,
}

fn fnv1a(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c_9dc5, |h, &b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    })
}

impl<F: Region> FlashRing<F> {
    /// The ring in `flash`, with the state its last whole record holds.
    pub fn open(flash: F) -> FlashRing<F> {
        assert!(flash.pages() >= 3, "a state page and two of the ring");
        let mut state = None;
        let mut next_record = RECORDS;
        for i in 0..RECORDS {
            let mut r = [0u8; RECORD_LEN];
            flash.read(i * RECORD_LEN as u32, &mut r);
            if r.iter().all(|&b| b == 0xff) {
                next_record = i;
                break;
            }
            let check = u32::from_be_bytes([
                r[CHECK_AT],
                r[CHECK_AT + 1],
                r[CHECK_AT + 2],
                r[CHECK_AT + 3],
            ]);
            if fnv1a(&r[..CHECK_AT]) == check {
                let mut s = [0u8; STATE_LEN];
                s.copy_from_slice(&r[..STATE_LEN]);
                state = Some(s);
            }
        }
        FlashRing {
            flash,
            ops: [None; OPS],
            next_record,
            state,
            lost: 0,
        }
    }

    /// The flash, for the driver.
    pub fn flash(&self) -> &F {
        &self.flash
    }

    /// The op to do next, the oldest queued.
    pub fn op(&self) -> Option<Op> {
        self.ops[0]
    }

    /// The op [`FlashRing::op`] gave is done.
    pub fn op_done(&mut self) {
        self.ops.rotate_left(1);
        self.ops[OPS - 1] = None;
    }

    /// Whether every write and erase asked for is done.
    pub fn idle(&self) -> bool {
        self.ops[0].is_none()
    }

    /// Ops dropped for want of room: none, unless a driver lets them pile
    /// up.
    pub fn lost(&self) -> u32 {
        self.lost
    }

    fn push(&mut self, op: Op) {
        match self.ops.iter_mut().find(|o| o.is_none()) {
            Some(o) => *o = Some(op),
            None => self.lost += 1,
        }
    }

    fn slot_offset(slot: u32) -> u32 {
        PAGE_LEN * (1 + slot / SLOTS_PER_PAGE) + (slot % SLOTS_PER_PAGE) * SLOT_LEN as u32
    }
}

impl<F: Region> Ring for FlashRing<F> {
    fn pages(&self) -> u32 {
        self.flash.pages() - 1
    }

    fn read(&self, slot: u32, out: &mut [u8; SLOT_LEN]) {
        let at = Self::slot_offset(slot);
        self.flash.read(at, out);
        // What is queued, in order, over what the flash holds.
        for op in self.ops.iter().flatten() {
            match *op {
                Op::Erase { offset } if offset == at - at % PAGE_LEN => *out = [0xff; SLOT_LEN],
                Op::Write { offset, len, bytes } if offset == at => {
                    for (o, b) in out.iter_mut().zip(&bytes[..usize::from(len)]) {
                        *o &= b;
                    }
                }
                _ => {}
            }
        }
    }

    fn write(&mut self, slot: u32, bytes: &[u8; SLOT_LEN]) {
        self.push(Op::Write {
            offset: Self::slot_offset(slot),
            len: SLOT_LEN as u16,
            bytes: *bytes,
        });
    }

    fn erase(&mut self, page: u32) {
        self.push(Op::Erase {
            offset: PAGE_LEN * (1 + page),
        });
    }

    fn state(&self) -> Option<[u8; STATE_LEN]> {
        self.state
    }

    fn keep_state(&mut self, state: &[u8; STATE_LEN]) {
        if self.next_record >= RECORDS {
            self.push(Op::Erase { offset: 0 });
            self.next_record = 0;
        }
        let mut bytes = [0xffu8; SLOT_LEN];
        bytes[..STATE_LEN].copy_from_slice(state);
        bytes[CHECK_AT..CHECK_AT + 4].copy_from_slice(&fnv1a(state).to_be_bytes());
        self.push(Op::Write {
            offset: self.next_record * RECORD_LEN as u32,
            len: RECORD_LEN as u16,
            bytes,
        });
        self.next_record += 1;
        self.state = Some(*state);
    }
}
