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
//! The CPU stops while the NVMC writes, 41 µs a word, so 0.33 ms a
//! record, and 85 ms for an erase (Product Specification, NVMC
//! electrical specification, tWRITE and tERASEPAGE): the radio keeps
//! receiving meanwhile, and a frame that ends then waits in it.

use bark_image::boot::Shared;
use bark_image::layout::STORE;
use bark_proto::store::{Stored, RECORD_LEN};
use embassy_embedded_hal::flash::partition::BlockingPartition;
use embassy_nrf::nvmc::Nvmc;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embedded_storage::nor_flash::{NorFlash, ReadNorFlash};

/// Records in the page.
const SLOTS: u32 = STORE.len / RECORD_LEN as u32;

/// The page of flash and the next erased slot in it.
pub struct Flash {
    page: BlockingPartition<'static, NoopRawMutex, Nvmc<'static>>,
    next: u32,
}

impl Flash {
    /// The page in `flash`, and the state its last whole record holds.
    pub fn open(flash: &'static Shared<Nvmc<'static>>) -> (Flash, Option<Stored>) {
        let mut page = BlockingPartition::new(flash, STORE.start, STORE.len);
        let mut last = None;
        let mut next = SLOTS;
        for slot in 0..SLOTS {
            let mut r = [0u8; RECORD_LEN];
            if page.read(slot * RECORD_LEN as u32, &mut r).is_err() {
                break;
            }
            if r.iter().all(|&b| b == 0xff) {
                next = slot;
                break;
            }
            if let Some(s) = Stored::from_record(&r) {
                last = Some(s);
            }
        }
        (Flash { page, next }, last)
    }

    /// Keep `stored` in the next slot, erasing the page when it is full.
    pub fn keep(&mut self, stored: &Stored) {
        if self.next >= SLOTS {
            if let Err(e) = self.page.erase(0, STORE.len) {
                log::error!("store erase failed: {:?}", e);
                return;
            }
            self.next = 0;
        }
        let at = self.next * RECORD_LEN as u32;
        self.next += 1;
        if let Err(e) = self.page.write(at, &stored.record()) {
            log::error!("store write failed: {:?}", e);
        }
    }
}
