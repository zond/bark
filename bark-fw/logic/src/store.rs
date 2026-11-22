//! The node's stored state in flash (roadmap D73, D188, D241; spec
//! "Implementation notes"): the core's state, the UTC offset, and the
//! phones' records and settings (`bark_link::phones`), as a log on one of
//! two pages, [`bark_image::layout::STORE`], that take turns, so that a
//! power cut at any moment leaves one page whole.
//!
//! A page is a log of 32-byte slots. Each entry, of one slot or four, goes
//! into the next erased slot, and the last whole entry of each kind holds;
//! each ends in a check, and the flash writes a word at a time in address
//! order, so an entry cut short fails it. An entry spans its slots whether
//! whole or not, so that a later entry never lands inside a cut one.
//!
//! The first slot, the head, says the page's layout and its generation. It
//! is written last, so a page is valid once its head is whole. When the
//! page in use is full, the store turns: it erases the other page, writes
//! every kind's latest into it, then its head, one generation on. The page
//! it turned from stays whole until the next turn erases it. At boot the
//! valid page of the later generation holds. A turn cut short leaves the
//! new page without a whole head and the old one in use, and the next
//! turn starts over by erasing the new page: nothing needs repair.
//!
//! Each turn erases one page and the two take turns, so each page is erased
//! once in two turns: 20 000 turns of the 10 000 erase cycles a page is
//! rated for (nRF52840 Product Specification, NVMC, endurance), at least
//! 92 slots written between two.

use bark_image::layout::{PAGE, STORE};
use bark_link::phones::{self, Load, Phones, Record, RECORDS};
use bark_proto::store::{Stored, RECORD_LEN};
use embedded_storage_async::nor_flash::NorFlash;

/// Bytes of a slot: one of the core's records (`bark_proto::store`).
pub const SLOT_LEN: usize = RECORD_LEN;

/// Slots in a page.
pub const SLOTS: u32 = PAGE / SLOT_LEN as u32;

/// Pages, which take turns.
const PAGES: u32 = 2;

/// The first byte of an offset's entry.
const ZONE: u8 = 0x80;

/// The first byte of a page's head.
const HEAD: u8 = 0x85;

/// The pages' layout, which their heads say: a change to what any entry
/// means takes the next.
const LAYOUT: u8 = 3;

/// The layout before: the same entries on the second page alone, with no
/// generation, erased when full and written again from RAM. A node that
/// comes from it keeps what it held ([`scan_before`]).
const LAYOUT_BEFORE: u8 = 2;

/// Slots a turn writes at most: the head, the core's state, the offset,
/// the settings and every record.
const TURN_SLOTS: u32 = 4 + (RECORDS * phones::RECORD_LEN / SLOT_LEN) as u32;

const _: () = {
    assert!(phones::SLOT_LEN == SLOT_LEN);
    assert!(STORE.len == PAGES * PAGE);
    assert!(SLOTS - TURN_SLOTS == 92);
};

/// An entry of a word and its complement, the rest erased: one cut short
/// leaves the second word erased, which is no complement of the first.
fn word_entry(word: [u8; 4]) -> [u8; SLOT_LEN] {
    let mut r = [0xff; SLOT_LEN];
    r[..4].copy_from_slice(&word);
    r[4..8].copy_from_slice(&word.map(|b| !b));
    r
}

/// The word of a whole [`word_entry`] whose first byte is `tag`.
fn word(r: &[u8; SLOT_LEN], tag: u8) -> Option<[u8; 4]> {
    let whole = r[0] == tag && (0..4).all(|i| r[4 + i] == !r[i]);
    whole.then(|| [r[0], r[1], r[2], r[3]])
}

/// The entry that keeps the UTC offset.
fn zone_entry(offset_min: i16) -> [u8; SLOT_LEN] {
    let [hi, lo] = offset_min.to_be_bytes();
    word_entry([ZONE, hi, lo, 0])
}

fn zone(r: &[u8; SLOT_LEN]) -> Option<i16> {
    word(r, ZONE).map(|w| i16::from_be_bytes([w[1], w[2]]))
}

/// A page's head, of `generation`.
fn head_entry(generation: u16) -> [u8; SLOT_LEN] {
    let [hi, lo] = generation.to_be_bytes();
    word_entry([HEAD, LAYOUT, hi, lo])
}

/// What a page's first slot says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Head {
    /// Erased: a page never written, or erased by a turn.
    Erased,
    /// A whole head of this layout, of this generation.
    Valid(u16),
    /// Anything else: another layout's head, or one cut short.
    Invalid,
}

fn head(r: &[u8; SLOT_LEN]) -> Head {
    if r.iter().all(|&b| b == 0xff) {
        return Head::Erased;
    }
    match word(r, HEAD) {
        Some([_, LAYOUT, hi, lo]) => Head::Valid(u16::from_be_bytes([hi, lo])),
        _ => Head::Invalid,
    }
}

/// Whether generation `a` is later than `b`. By their difference, so the
/// count may wrap: the two pages' differ by one.
fn later(a: u16, b: u16) -> bool {
    (a.wrapping_sub(b) as i16) > 0
}

/// The page that holds, by the pages' heads, and its generation: the valid
/// one, or of two the later.
fn choose(heads: [Head; 2]) -> Option<(u32, u16)> {
    match heads {
        [Head::Valid(a), Head::Valid(b)] if later(b, a) => Some((1, b)),
        [Head::Valid(a), _] => Some((0, a)),
        [_, Head::Valid(b)] => Some((1, b)),
        _ => None,
    }
}

/// Where the store writes next: the page in use and its generation, none
/// before the first turn, and its next erased slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Log {
    page: Option<(u32, u16)>,
    next: u32,
}

/// What the store holds last of each kind.
#[derive(Clone, Copy, Debug, Default)]
pub struct Kept {
    /// The core's state (`Config::stored`).
    pub stored: Option<Stored>,
    /// The phone's UTC offset, in minutes.
    pub offset_min: Option<i16>,
    /// The phones' records and settings.
    pub phones: Load,
    /// No page is valid, and one holds something: another layout's state,
    /// read as none.
    pub discarded: bool,
    /// No page is valid, and the second is of the layout before: what it
    /// holds is read, and the first write turns to the first page.
    pub before: bool,
}

/// The pages as `read` reads them, `read(at, bytes)` the bytes at `at`
/// from the pages' start, false if it could not: where the log goes on,
/// and the last whole entry of each kind on the page that holds.
pub fn scan(mut read: impl FnMut(u32, &mut [u8]) -> bool) -> (Log, Kept) {
    let mut kept = Kept::default();
    let heads = [0, 1].map(|p| {
        let mut r = [0u8; SLOT_LEN];
        match read(p * PAGE, &mut r) {
            true => head(&r),
            false => Head::Invalid,
        }
    });
    let Some((page, generation)) = choose(heads) else {
        if scan_before(&mut read, &mut kept) {
            kept.before = true;
        } else {
            kept.discarded = heads.contains(&Head::Invalid);
        }
        return (
            Log {
                page: None,
                next: 0,
            },
            kept,
        );
    };
    let mut slot = 1;
    while slot < SLOTS {
        let mut e = [0u8; phones::RECORD_LEN];
        let at = page * PAGE + slot * SLOT_LEN as u32;
        if !read(at, &mut e[..SLOT_LEN]) {
            break;
        }
        if e[..SLOT_LEN].iter().all(|&b| b == 0xff) {
            let log = Log {
                page: Some((page, generation)),
                next: slot,
            };
            return (log, kept);
        }
        let len = phones::entry_len(e[0]);
        let slots = (len / SLOT_LEN) as u32;
        if slot + slots > SLOTS || !read(at + SLOT_LEN as u32, &mut e[SLOT_LEN..len]) {
            break;
        }
        let first: &[u8; SLOT_LEN] = (&e[..SLOT_LEN]).try_into().expect("a slot");
        kept.stored = Stored::from_record(first).or(kept.stored);
        kept.offset_min = zone(first).or(kept.offset_min);
        kept.phones.entry(&e[..len]);
        slot += slots;
    }
    let log = Log {
        page: Some((page, generation)),
        next: SLOTS,
    };
    (log, kept)
}

/// The second page as the layout before wrote it, into `kept`: false if
/// its head is not that layout's. The entries are this layout's; only an
/// entry of four slots cut short differs, whose erased slots that layout
/// wrote the next entry into.
///
/// The log is left with no page, so the first write turns to the first
/// page, and the page read here stays whole until the turn after: a power
/// cut in that first turn leaves it to be read again.
fn scan_before(read: &mut impl FnMut(u32, &mut [u8]) -> bool, kept: &mut Kept) -> bool {
    let mut first = [0u8; SLOT_LEN];
    if !read(PAGE, &mut first) || word(&first, HEAD) != Some([HEAD, LAYOUT_BEFORE, 0, 0]) {
        return false;
    }
    let mut slot = 1;
    while slot < SLOTS {
        let mut e = [0u8; phones::RECORD_LEN];
        let at = PAGE + slot * SLOT_LEN as u32;
        if !read(at, &mut e[..SLOT_LEN]) || e[..SLOT_LEN].iter().all(|&b| b == 0xff) {
            break;
        }
        let len = phones::entry_len(e[0]);
        let slots = (len / SLOT_LEN) as u32;
        if slots > 1 {
            if slot + slots > SLOTS || !read(at + SLOT_LEN as u32, &mut e[SLOT_LEN..len]) {
                break;
            }
            kept.phones.entry(&e[..len]);
            // Cut short: the next entry is in its first erased slot.
            let erased = (1..slots).find(|&k| {
                let k = k as usize * SLOT_LEN;
                e[k..k + SLOT_LEN].iter().all(|&b| b == 0xff)
            });
            slot += erased.unwrap_or(slots);
            continue;
        }
        let one: &[u8; SLOT_LEN] = (&e[..SLOT_LEN]).try_into().expect("a slot");
        kept.stored = Stored::from_record(one).or(kept.stored);
        kept.offset_min = zone(one).or(kept.offset_min);
        kept.phones.entry(one);
        slot += 1;
    }
    true
}

/// The phones' entries as they stand, which a turn writes again.
pub trait Latest {
    /// The settings' entry.
    fn settings(&self) -> [u8; SLOT_LEN];
    /// The entry of the record at place `i`; none for a free place.
    fn record(&self, i: usize) -> Option<[u8; phones::RECORD_LEN]>;
}

impl Latest for Phones {
    fn settings(&self) -> [u8; SLOT_LEN] {
        Phones::settings(self).encode()
    }

    fn record(&self, i: usize) -> Option<[u8; phones::RECORD_LEN]> {
        self.get(i).map(|r| Record::encode(i, Some(r)))
    }
}

/// The store on `F`, the two pages from its start.
pub struct Store<F> {
    flash: F,
    log: Log,
    /// The core's state and the offset as kept last, which a turn writes
    /// again.
    stored: Option<Stored>,
    offset_min: Option<i16>,
}

impl<F: NorFlash> Store<F> {
    /// The store as [`scan`] read it.
    pub fn new(flash: F, log: Log, kept: &Kept) -> Store<F> {
        Store {
            flash,
            log,
            stored: kept.stored,
            offset_min: kept.offset_min,
        }
    }

    /// Keep `stored`, in place of the state kept before.
    pub async fn keep(&mut self, stored: Stored, phones: &impl Latest) -> Result<(), F::Error> {
        self.stored = Some(stored);
        self.write(&stored.record(), phones).await
    }

    /// Keep the UTC offset, in place of the one kept before.
    pub async fn keep_offset(
        &mut self,
        offset_min: i16,
        phones: &impl Latest,
    ) -> Result<(), F::Error> {
        self.offset_min = Some(offset_min);
        self.write(&zone_entry(offset_min), phones).await
    }

    /// Write `entry`, of one slot or four, into the next slots; with no
    /// room, turn instead, `phones` holding what `entry` says.
    pub async fn write(&mut self, entry: &[u8], phones: &impl Latest) -> Result<(), F::Error> {
        let slots = (entry.len() / SLOT_LEN) as u32;
        match self.log.page {
            Some((page, _)) if self.log.next + slots <= SLOTS => {
                let at = page * PAGE + self.log.next * SLOT_LEN as u32;
                self.log.next += slots;
                self.flash.write(at, entry).await
            }
            _ => self.turn(phones).await,
        }
    }

    /// Erase the other page, write every kind's latest into it, then its
    /// head, one generation on.
    async fn turn(&mut self, phones: &impl Latest) -> Result<(), F::Error> {
        let (page, generation) = match self.log.page {
            Some((p, g)) => (1 - p, g.wrapping_add(1)),
            None => (0, 0),
        };
        // Until the head is written, every write turns again.
        self.log.next = SLOTS;
        let base = page * PAGE;
        self.flash.erase(base, base + PAGE).await?;
        let mut next = 1;
        if let Some(s) = self.stored {
            self.put(base, &mut next, &s.record()).await?;
        }
        if let Some(o) = self.offset_min {
            self.put(base, &mut next, &zone_entry(o)).await?;
        }
        self.put(base, &mut next, &phones.settings()).await?;
        for i in 0..RECORDS {
            if let Some(r) = phones.record(i) {
                self.put(base, &mut next, &r).await?;
            }
        }
        self.flash.write(base, &head_entry(generation)).await?;
        self.log = Log {
            page: Some((page, generation)),
            next,
        };
        Ok(())
    }

    async fn put(&mut self, base: u32, next: &mut u32, entry: &[u8]) -> Result<(), F::Error> {
        let at = base + *next * SLOT_LEN as u32;
        *next += (entry.len() / SLOT_LEN) as u32;
        self.flash.write(at, entry).await
    }
}

#[cfg(test)]
mod tests;
