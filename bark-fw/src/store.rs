//! The node's stored state in a page of flash, roadmap D73 and
//! spec "Implementation notes": what the core asks to keep
//! (`Action::Store`), handed back at the next boot in `Config::stored`.
//!
//! The page, `bark_image::layout::STORE`, is a log of 32-byte records
//! (`bark_proto::store::RECORD_LEN`), each written into the next erased
//! slot; the last whole record is the state. Only when the page is full
//! is it erased, so a page takes 128 writes for each of the 10 000 erase
//! cycles the nRF52840's flash is rated for (Product Specification,
//! NVMC, endurance). A record cut short by a power cut fails its checksum
//! and the one before it is taken; a cut between the erase and the write
//! that follows it loses the state, and the node boots as a fresh one.
//!
//! The writes go through the MPSL's flash timeslots (`nrf_mpsl::Flash`),
//! which fit each between the BLE controller's radio events: the CPU
//! stops while the NVMC writes, 41 µs a word, 0.33 ms a record, and 85 ms
//! for an erase (Product Specification, NVMC electrical specification,
//! tWRITE and tERASEPAGE), which the controller's timing could not wait
//! for. The LoRa radio keeps receiving meanwhile, and a frame that ends
//! then waits in it.
//!
//! The writes run in a task of their own ([`run`]), handed the latest of
//! each kind by the node after the step that asked: an erase's and a
//! write's futures nested in the node's loop grew the main task's stack
//! frame by 2.4 KB.
//!
//! The MPSL's flash is one, taken once ([`SharedFlash`]): the mailbox
//! writes through it too (`mailbox.rs`), each op a timeslot of its own.
//!
//! The page also keeps the phone's UTC offset (`utc.rs`), in records of
//! its own beside the core's: [`ZONE`] and the offset in the first word,
//! its complement in the second, the rest left erased. The core's records
//! start with their layout's version, counted from 1, and a cut record
//! leaves the second word erased, which is no complement of the first.
//!
//! And it keeps the node's phones (`phones.rs`): their records, their
//! progress and the node's settings, entries of four slots or one each
//! (`bark_link::phones`); the console reads them to say whether the node
//! is public ([`scan`]).
//!
//! The page's first slot says its layout, [`LAYOUT`], in a record of its
//! own: [`HEAD`] and the layout in the first word, its complement in the
//! second. A page of another layout is read as empty, and its first write
//! erases it, so a node with state it would misread starts clean
//! (roadmap D188).

use bark_image::layout::STORE;
use bark_link::phones::{self, Load};
use bark_proto::store::{Stored, RECORD_LEN};
#[cfg(not(feature = "console"))]
use embassy_futures::select::{select3, Either3};
#[cfg(not(feature = "console"))]
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
#[cfg(not(feature = "console"))]
use embassy_sync::signal::Signal;

/// What the store's task is to write next: the core's state and the
/// offset, the latest of each, and that the phones changed.
#[cfg(not(feature = "console"))]
struct ToWrite {
    stored: Signal<CriticalSectionRawMutex, Stored>,
    offset: Signal<CriticalSectionRawMutex, i16>,
    phones: Signal<CriticalSectionRawMutex, ()>,
}

#[cfg(not(feature = "console"))]
static TO_WRITE: ToWrite = ToWrite {
    stored: Signal::new(),
    offset: Signal::new(),
    phones: Signal::new(),
};

#[cfg(not(feature = "console"))]
/// The node's side: keep `stored`, in place of the state kept before.
pub fn keep(stored: Stored) {
    TO_WRITE.stored.signal(stored);
}

#[cfg(not(feature = "console"))]
/// The node's side: keep the UTC offset, in place of the one kept before.
pub fn keep_offset(offset_min: i16) {
    TO_WRITE.offset.signal(offset_min);
}

#[cfg(not(feature = "console"))]
/// The phones' side (`phones.rs`): keep what changed in their records.
pub fn keep_phones() {
    TO_WRITE.phones.signal(());
}

#[cfg(not(feature = "console"))]
/// Write what the node and the phone link hand, for ever.
#[embassy_executor::task]
pub async fn run(mut flash: Flash) -> ! {
    loop {
        let w = &TO_WRITE;
        match select3(w.stored.wait(), w.offset.wait(), w.phones.wait()).await {
            Either3::First(stored) => flash.keep(&stored).await,
            Either3::Second(offset_min) => flash.keep_offset(offset_min).await,
            Either3::Third(()) => flash.keep_phones().await,
        }
    }
}

#[cfg(not(feature = "console"))]
/// The MPSL's flash, shared by the store and the mailbox.
pub type SharedFlash =
    embassy_sync::mutex::Mutex<CriticalSectionRawMutex, nrf_mpsl::Flash<'static>>;

#[cfg(not(feature = "console"))]
/// The shared flash, once.
pub fn shared(flash: nrf_mpsl::Flash<'static>) -> &'static SharedFlash {
    static FLASH: static_cell::StaticCell<SharedFlash> = static_cell::StaticCell::new();
    FLASH.init(embassy_sync::mutex::Mutex::new(flash))
}

/// Records in the page.
const SLOTS: u32 = STORE.len / RECORD_LEN as u32;

/// The first byte of an offset's record.
const ZONE: u8 = 0x80;

/// The first byte of the page's head.
const HEAD: u8 = 0x85;

/// The page's layout, which its head says: a change to what any of its
/// entries means takes the next.
const LAYOUT: u8 = 2;

/// A record of a word and its complement, the rest erased: a cut record
/// leaves the second word erased, which is no complement of the first.
#[cfg(not(feature = "console"))]
fn word_record(word: [u8; 4]) -> [u8; RECORD_LEN] {
    let mut r = [0xff; RECORD_LEN];
    r[..4].copy_from_slice(&word);
    r[4..8].copy_from_slice(&word.map(|b| !b));
    r
}

/// The word of a whole record of [`word_record`]'s whose first byte is
/// `tag`.
fn word(r: &[u8; RECORD_LEN], tag: u8) -> Option<[u8; 4]> {
    let whole = r[0] == tag && (0..4).all(|i| r[4 + i] == !r[i]);
    whole.then(|| [r[0], r[1], r[2], r[3]])
}

#[cfg(not(feature = "console"))]
fn zone_record(offset_min: i16) -> [u8; RECORD_LEN] {
    let [hi, lo] = offset_min.to_be_bytes();
    word_record([ZONE, hi, lo, 0])
}

fn zone(r: &[u8; RECORD_LEN]) -> Option<i16> {
    word(r, ZONE).map(|w| i16::from_be_bytes([w[1], w[2]]))
}

/// The page's head.
#[cfg(not(feature = "console"))]
fn head() -> [u8; RECORD_LEN] {
    word_record([HEAD, LAYOUT, 0, 0])
}

const _: () = assert!(phones::SLOT_LEN == RECORD_LEN);

/// What the page holds last of each kind. The console reads the phones
/// alone.
#[derive(Clone, Copy, Default)]
#[cfg_attr(feature = "console", allow(dead_code))]
pub struct Kept {
    pub stored: Option<Stored>,
    pub offset_min: Option<i16>,
    pub phones: Load,
    /// The page held another layout's state, read as none.
    pub discarded: bool,
}

/// The page as `read` reads it, the bytes at an address of the flash:
/// its next erased slot, and the last whole record of each kind it holds.
/// A page of another layout holds nothing and has no slot free, so that
/// the next write erases it.
pub fn scan(mut read: impl FnMut(u32, &mut [u8]) -> bool) -> (u32, Kept) {
    let mut kept = Kept::default();
    let mut first = [0u8; RECORD_LEN];
    if !read(STORE.start, &mut first) || first.iter().all(|&b| b == 0xff) {
        return (0, kept);
    }
    if word(&first, HEAD) != Some([HEAD, LAYOUT, 0, 0]) {
        kept.discarded = true;
        return (SLOTS, kept);
    }
    let mut slot = 1;
    while slot < SLOTS {
        let mut r = [0u8; phones::RECORD_LEN];
        let at = STORE.start + slot * RECORD_LEN as u32;
        if !read(at, &mut r[..RECORD_LEN]) {
            break;
        }
        if r[..RECORD_LEN].iter().all(|&b| b == 0xff) {
            return (slot, kept);
        }
        // An entry of several slots: one cut short leaves its later slots
        // erased, and the next entry goes into the first of them.
        let slots = (phones::entry_len(r[0]) / RECORD_LEN) as u32;
        if slots > 1 {
            let len = phones::entry_len(r[0]);
            if slot + slots > SLOTS || !read(at + RECORD_LEN as u32, &mut r[RECORD_LEN..len]) {
                return (slot + 1, kept);
            }
            kept.phones.entry(&r[..len]);
            let erased = (1..slots).find(|&k| {
                let k = (k as usize) * RECORD_LEN;
                r[k..k + RECORD_LEN].iter().all(|&b| b == 0xff)
            });
            if let Some(k) = erased {
                return (slot + k, kept);
            }
            slot += slots;
            continue;
        }
        let first: &[u8; RECORD_LEN] = (&r[..RECORD_LEN]).try_into().expect("a slot");
        kept.stored = Stored::from_record(first).or(kept.stored);
        kept.offset_min = zone(first).or(kept.offset_min);
        kept.phones.entry(first);
        slot += 1;
    }
    (SLOTS, kept)
}

/// The page of flash and the next erased slot in it.
#[cfg(not(feature = "console"))]
pub struct Flash {
    flash: &'static SharedFlash,
    next: u32,
    /// The core's state and the offset the page holds last, written again
    /// after an erase, with the phones as `phones.rs` holds them then.
    stored: Option<Stored>,
    offset_min: Option<i16>,
}

#[cfg(not(feature = "console"))]
impl Flash {
    /// The page, and the last whole record of each kind it holds.
    pub fn open(flash: &'static SharedFlash) -> (Flash, Kept) {
        let mut f = flash
            .try_lock()
            .expect("nothing writes before the store opens");
        let (next, kept) = scan(|at, r| f.read(at, r).is_ok());
        drop(f);
        if kept.discarded {
            log::warn!("store: the page is of another layout; its state is discarded");
        }
        let flash = Flash {
            flash,
            next,
            stored: kept.stored,
            offset_min: kept.offset_min,
        };
        (flash, kept)
    }

    /// Keep `stored`, in place of the state kept before.
    async fn keep(&mut self, stored: &Stored) {
        self.stored = Some(*stored);
        self.write(&stored.record()).await;
    }

    /// Keep the UTC offset, in place of the one kept before.
    async fn keep_offset(&mut self, offset_min: i16) {
        self.offset_min = Some(offset_min);
        self.write(&zone_record(offset_min)).await;
    }

    /// Keep what changed in the phones' records: a record whole, or its
    /// progress alone, and the settings.
    async fn keep_phones(&mut self) {
        let dirty = crate::phones::take_dirty();
        if dirty.settings {
            let s = crate::phones::read(|p| p.settings().encode());
            self.write(&s).await;
        }
        for i in 0..phones::RECORDS {
            let bit = 1 << i;
            if dirty.records & bit != 0 {
                let r = crate::phones::read(|p| phones::Record::encode(i, p.get(i)));
                self.write(&r).await;
            } else if dirty.progress & bit != 0 {
                if let Some(e) = crate::phones::read(|p| p.progress_of(i)) {
                    self.write(&e.encode()).await;
                }
            }
        }
    }

    /// Write `entry`, of one slot or more, into the next slots, after
    /// the head on an erased page. A page without room is erased first,
    /// and every kind's latest written again, the phones' as they stand,
    /// which hold what `entry` says, so that each survives.
    async fn write(&mut self, entry: &[u8]) {
        let slots = (entry.len() / RECORD_LEN) as u32;
        if self.next > 0 && self.next + slots <= SLOTS {
            self.put(entry).await;
            return;
        }
        if self.next == 0 {
            self.put(&head()).await;
            self.put(entry).await;
            return;
        }
        let erased = self
            .flash
            .lock()
            .await
            .erase(STORE.start, STORE.end())
            .await;
        if let Err(e) = erased {
            log::error!("store: erase failed: {:?}", e);
            return;
        }
        self.next = 0;
        self.put(&head()).await;
        if let Some(s) = self.stored {
            self.put(&s.record()).await;
        }
        if let Some(o) = self.offset_min {
            self.put(&zone_record(o)).await;
        }
        let settings = crate::phones::read(|p| p.settings().encode());
        self.put(&settings).await;
        for i in 0..phones::RECORDS {
            let r = crate::phones::read(|p| p.get(i).map(|r| phones::Record::encode(i, Some(r))));
            if let Some(r) = r {
                self.put(&r).await;
            }
        }
    }

    async fn put(&mut self, record: &[u8]) {
        let at = STORE.start + self.next * RECORD_LEN as u32;
        self.next += (record.len() / RECORD_LEN) as u32;
        let written = self.flash.lock().await.write(at, record).await;
        if let Err(e) = written {
            log::error!("store: write failed: {:?}", e);
        }
    }
}
