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
//! The page also keeps the phone's UTC offset (`utc.rs`), in records of
//! its own beside the core's: [`ZONE`] and the offset in the first word,
//! its complement in the second, the rest left erased. The core's records
//! start with their layout's version, counted from 1, and a cut record
//! leaves the second word erased, which is no complement of the first.

use bark_image::layout::STORE;
use bark_proto::store::{Stored, RECORD_LEN};

/// Records in the page.
const SLOTS: u32 = STORE.len / RECORD_LEN as u32;

/// The first byte of an offset's record.
const ZONE: u8 = 0x80;

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

/// The page of flash and the next erased slot in it.
pub struct Flash {
    flash: nrf_mpsl::Flash<'static>,
    next: u32,
    /// What the page holds last, written again after an erase.
    stored: Option<Stored>,
    offset_min: Option<i16>,
}

impl Flash {
    /// The page, the state its last whole record holds, and the UTC
    /// offset its last offset's record holds.
    pub fn open(mut flash: nrf_mpsl::Flash<'static>) -> (Flash, Option<Stored>, Option<i16>) {
        let mut last = None;
        let mut offset_min = None;
        let mut next = SLOTS;
        for slot in 0..SLOTS {
            let mut r = [0u8; RECORD_LEN];
            if flash
                .read(STORE.start + slot * RECORD_LEN as u32, &mut r)
                .is_err()
            {
                break;
            }
            if r.iter().all(|&b| b == 0xff) {
                next = slot;
                break;
            }
            if let Some(s) = Stored::from_record(&r) {
                last = Some(s);
            }
            offset_min = zone(&r).or(offset_min);
        }
        let page = Flash {
            flash,
            next,
            stored: last,
            offset_min,
        };
        (page, last, offset_min)
    }

    /// Keep `stored`, in place of the state kept before.
    pub async fn keep(&mut self, stored: &Stored) {
        self.stored = Some(*stored);
        self.write(&stored.record()).await;
    }

    /// Keep the UTC offset, in place of the one kept before.
    pub async fn keep_offset(&mut self, offset_min: i16) {
        self.offset_min = Some(offset_min);
        self.write(&zone_record(offset_min)).await;
    }

    /// Write `record` into the next slot. A full page is erased first,
    /// and the other kind of record written again, so that each survives.
    async fn write(&mut self, record: &[u8; RECORD_LEN]) {
        if self.next >= SLOTS {
            if let Err(e) = self.flash.erase(STORE.start, STORE.end()).await {
                log::error!("store erase failed: {:?}", e);
                return;
            }
            self.next = 0;
            let other = match zone(record) {
                Some(_) => self.stored.map(|s| s.record()),
                None => self.offset_min.map(zone_record),
            };
            if let Some(r) = other {
                self.put(&r).await;
            }
        }
        self.put(record).await;
    }

    async fn put(&mut self, record: &[u8; RECORD_LEN]) {
        let at = STORE.start + self.next * RECORD_LEN as u32;
        self.next += 1;
        if let Err(e) = self.flash.write(at, record).await {
            log::error!("store write failed: {:?}", e);
        }
    }
}
