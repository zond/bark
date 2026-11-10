//! The mailbox's [`Ring`] in a region of NOR flash: the ring's pages
//! (spec "Mailbox"; `bark-image/src/layout.rs`), which may lie on a chip
//! anyone with a clip can read, as its items are sealed to the phone.
//! The people's mailbox keys are kept with their records inside the
//! chip (`bark-link` `phones`).
//!
//! The flash is read when the mailbox asks, but written later: a write
//! or an erase is queued as an [`Op`], read back through the queue until
//! it is done, and done by the driver when it can, which on the node is
//! between the radio's events (`bark-fw/src/mailbox.rs`). The mailbox
//! never waits on a write. A slot cut short by a power cut fails the
//! mailbox's own check and holds no item.

use crate::mailbox::{Ring, SLOTS_PER_PAGE, SLOT_LEN};

/// Bytes of an erase page: the nRF52840's (Product Specification, NVMC)
/// and the external flash's sector (ZD25WQ16B datasheet, "Sector Erase")
/// alike.
pub const PAGE_LEN: u32 = 4096;

/// Ops queued at most. A step of the node queues at most four: a
/// message's erase and write, a confirmation's mark and a reply's; the
/// driver does them as soon as the step ends, and the node confirms the
/// next message to the core only once they are done, so a few more than
/// that is room.
pub const OPS: usize = 6;

/// A region's flash, read when asked; offsets from its start.
pub trait Region {
    /// Pages in the region.
    fn pages(&self) -> u32;
    /// The bytes at `offset`, into `out`.
    fn read(&self, offset: u32, out: &mut [u8]);
}

/// A write or an erase the driver is to do, at an offset in the ring's
/// region.
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
    /// Write `bytes[..len]` at `offset`: bytes erased since they were last
    /// written, or a mark, which only clears bits.
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

/// The ring in its region of flash, with the writes and erases not yet
/// done.
pub struct FlashRing<R: Region> {
    ring: R,
    ops: [Option<Op>; OPS],
    /// Ops that found the queue full and were dropped.
    lost: u32,
}

impl<R: Region> FlashRing<R> {
    /// The ring in `ring`.
    pub fn open(ring: R) -> FlashRing<R> {
        assert!(
            ring.pages() >= 2,
            "one page of the ring is erased to make room"
        );
        FlashRing {
            ring,
            ops: [None; OPS],
            lost: 0,
        }
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
        PAGE_LEN * (slot / SLOTS_PER_PAGE) + (slot % SLOTS_PER_PAGE) * SLOT_LEN as u32
    }

    /// What is queued for the ring's bytes from `at`, in order, over what
    /// the flash holds in `out`.
    fn queued_over(&self, at: u32, out: &mut [u8]) {
        let end = at + out.len() as u32;
        for op in self.ops.iter().flatten() {
            match *op {
                Op::Erase { offset } if offset == at - at % PAGE_LEN => out.fill(0xff),
                Op::Write { offset, len, bytes } => {
                    for i in offset.max(at)..(offset + u32::from(len)).min(end) {
                        out[(i - at) as usize] &= bytes[(i - offset) as usize];
                    }
                }
                Op::Erase { .. } => {}
            }
        }
    }
}

impl<R: Region> Ring for FlashRing<R> {
    fn pages(&self) -> u32 {
        self.ring.pages()
    }

    fn read(&self, slot: u32, out: &mut [u8; SLOT_LEN]) {
        let at = Self::slot_offset(slot);
        self.ring.read(at, out);
        self.queued_over(at, out);
    }

    fn read_byte(&self, slot: u32, at: usize) -> u8 {
        let at = Self::slot_offset(slot) + at as u32;
        let mut b = [0u8; 1];
        self.ring.read(at, &mut b);
        self.queued_over(at, &mut b);
        b[0]
    }

    fn write(&mut self, slot: u32, bytes: &[u8; SLOT_LEN]) {
        self.push(Op::Write {
            offset: Self::slot_offset(slot),
            len: SLOT_LEN as u16,
            bytes: *bytes,
        });
    }

    fn clear(&mut self, slot: u32, at: usize, bits: u8) {
        let mut bytes = [0xffu8; SLOT_LEN];
        bytes[0] = !bits;
        self.push(Op::Write {
            offset: Self::slot_offset(slot) + at as u32,
            len: 1,
            bytes,
        });
    }

    fn erase(&mut self, page: u32) {
        self.push(Op::Erase {
            offset: PAGE_LEN * page,
        });
    }

    fn written(&self, slot: u32) -> bool {
        let at = Self::slot_offset(slot);
        !self.ops.iter().flatten().any(|op| match *op {
            Op::Erase { offset } => offset == at - at % PAGE_LEN,
            Op::Write { offset, len, .. } => offset == at && usize::from(len) == SLOT_LEN,
        })
    }
}
