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
//! And it keeps the bond with the paired phone (`ble.rs`), a record of
//! two slots (`bark_link::bond`), with the public setting; the console
//! reads it to say whether the node is public ([`scan`]).

use bark_image::layout::STORE;
use bark_link::bond::Bond;
use bark_proto::store::{Stored, RECORD_LEN};
#[cfg(not(feature = "console"))]
use embassy_futures::select::{select3, Either3};
#[cfg(not(feature = "console"))]
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
#[cfg(not(feature = "console"))]
use embassy_sync::signal::Signal;

/// The state, the offset and the bond to keep next, the latest of each.
#[cfg(not(feature = "console"))]
static STORED: Signal<CriticalSectionRawMutex, Stored> = Signal::new();
#[cfg(not(feature = "console"))]
static OFFSET: Signal<CriticalSectionRawMutex, i16> = Signal::new();
#[cfg(not(feature = "console"))]
static BOND: Signal<CriticalSectionRawMutex, Option<Bond>> = Signal::new();

#[cfg(not(feature = "console"))]
/// The node's side: keep `stored`, in place of the state kept before.
pub fn keep(stored: Stored) {
    STORED.signal(stored);
}

#[cfg(not(feature = "console"))]
/// The node's side: keep the UTC offset, in place of the one kept before.
pub fn keep_offset(offset_min: i16) {
    OFFSET.signal(offset_min);
}

#[cfg(not(feature = "console"))]
/// The phone link's side: keep `bond`, in place of the bond kept before;
/// none once the owner unpairs.
pub fn keep_bond(bond: Option<Bond>) {
    BOND.signal(bond);
}

#[cfg(not(feature = "console"))]
/// Write what the node and the phone link hand, for ever.
#[embassy_executor::task]
pub async fn run(mut flash: Flash) -> ! {
    loop {
        match select3(STORED.wait(), OFFSET.wait(), BOND.wait()).await {
            Either3::First(stored) => flash.keep(&stored).await,
            Either3::Second(offset_min) => flash.keep_offset(offset_min).await,
            Either3::Third(bond) => flash.keep_bond(&bond).await,
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

#[cfg(not(feature = "console"))]
fn zone_record(offset_min: i16) -> [u8; RECORD_LEN] {
    let [hi, lo] = offset_min.to_be_bytes();
    let word = [ZONE, hi, lo, 0];
    let mut r = [0xff; RECORD_LEN];
    r[..4].copy_from_slice(&word);
    r[4..8].copy_from_slice(&word.map(|b| !b));
    r
}

fn zone(r: &[u8; RECORD_LEN]) -> Option<i16> {
    let whole = r[0] == ZONE && (0..4).all(|i| r[4 + i] == !r[i]);
    whole.then(|| i16::from_be_bytes([r[1], r[2]]))
}

/// Slots a bond's record takes.
const BOND_SLOTS: u32 = (Bond::LEN / RECORD_LEN) as u32;
const _: () = assert!(Bond::LEN == 2 * RECORD_LEN);

/// What the page holds last of each kind. The console reads the bond
/// alone.
#[derive(Clone, Copy, Default)]
#[cfg_attr(feature = "console", allow(dead_code))]
pub struct Kept {
    pub stored: Option<Stored>,
    pub offset_min: Option<i16>,
    pub bond: Option<Bond>,
}

/// The page as `read` reads it, the bytes at an address of the flash:
/// its next erased slot, and the last whole record of each kind it holds.
pub fn scan(mut read: impl FnMut(u32, &mut [u8]) -> bool) -> (u32, Kept) {
    let mut kept = Kept::default();
    let mut slot = 0;
    while slot < SLOTS {
        let mut r = [0u8; Bond::LEN];
        let at = STORE.start + slot * RECORD_LEN as u32;
        let both = slot + BOND_SLOTS <= SLOTS;
        let len = if both { Bond::LEN } else { RECORD_LEN };
        if !read(at, &mut r[..len]) {
            break;
        }
        let (first, second) = r.split_at(RECORD_LEN);
        if first.iter().all(|&b| b == 0xff) {
            return (slot, kept);
        }
        let first: &[u8; RECORD_LEN] = first.try_into().expect("a slot");
        if first[0] == Bond::TAG {
            if let Some(bond) = bark_link::bond::read(&r) {
                kept.bond = bond;
            }
            // A bond cut short in its second slot leaves it erased: the
            // next record goes there.
            if !both || second.iter().all(|&b| b == 0xff) {
                return (slot + 1, kept);
            }
            slot += BOND_SLOTS;
            continue;
        }
        kept.stored = Stored::from_record(first).or(kept.stored);
        kept.offset_min = zone(first).or(kept.offset_min);
        slot += 1;
    }
    (SLOTS, kept)
}

/// The page of flash and the next erased slot in it.
#[cfg(not(feature = "console"))]
pub struct Flash {
    flash: &'static SharedFlash,
    next: u32,
    /// What the page holds last, written again after an erase.
    kept: Kept,
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
        (Flash { flash, next, kept }, kept)
    }

    /// Keep `stored`, in place of the state kept before.
    async fn keep(&mut self, stored: &Stored) {
        self.kept.stored = Some(*stored);
        self.write(&stored.record()).await;
    }

    /// Keep the UTC offset, in place of the one kept before.
    async fn keep_offset(&mut self, offset_min: i16) {
        self.kept.offset_min = Some(offset_min);
        self.write(&zone_record(offset_min)).await;
    }

    /// Keep the bond, in place of the one kept before, or a forgotten
    /// one's record for none.
    async fn keep_bond(&mut self, bond: &Option<Bond>) {
        self.kept.bond = *bond;
        let record = bond.map_or_else(bark_link::bond::forgotten, |b| b.record());
        self.write(&record).await;
    }

    /// Write `record`, of one slot or more, into the next slots. A page
    /// without room is erased first, and the other kinds' records
    /// written again, so that each survives.
    async fn write(&mut self, record: &[u8]) {
        let slots = (record.len() / RECORD_LEN) as u32;
        if self.next + slots > SLOTS {
            let erased = self
                .flash
                .lock()
                .await
                .erase(STORE.start, STORE.end())
                .await;
            if let Err(e) = erased {
                log::error!("store erase failed: {:?}", e);
                return;
            }
            self.next = 0;
            let kind = record[0];
            let core = kind != ZONE && kind != Bond::TAG;
            if !core {
                if let Some(s) = self.kept.stored {
                    self.put(&s.record()).await;
                }
            }
            if kind != ZONE {
                if let Some(o) = self.kept.offset_min {
                    self.put(&zone_record(o)).await;
                }
            }
            if kind != Bond::TAG {
                if let Some(b) = self.kept.bond {
                    self.put(&b.record()).await;
                }
            }
        }
        self.put(record).await;
    }

    async fn put(&mut self, record: &[u8]) {
        let at = STORE.start + self.next * RECORD_LEN as u32;
        self.next += (record.len() / RECORD_LEN) as u32;
        let written = self.flash.lock().await.write(at, record).await;
        if let Err(e) = written {
            log::error!("store write failed: {:?}", e);
        }
    }
}
