//! The node's stored state in a page of flash, roadmap D73 and
//! spec "Implementation notes": what the core asks to keep
//! (`Action::Store`), handed back at the next boot in `Config::stored`.
//!
//! The page, `STORE` in `memory.x`, is a log of 32-byte records
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

use bark_proto::store::{Stored, RECORD_LEN};
use embassy_nrf::nvmc::{Nvmc, PAGE_SIZE};
use embedded_storage::nor_flash::{NorFlash, ReadNorFlash};

/// The page's address: `STORE` in `memory.x`, the last page below the
/// bootloader.
const PAGE: u32 = 0xF3000;

/// Records in the page.
const SLOTS: u32 = (PAGE_SIZE / RECORD_LEN) as u32;

/// The page of flash and the next erased slot in it.
pub struct Flash {
    nvmc: Nvmc<'static>,
    next: u32,
}

impl Flash {
    /// The page behind `nvmc`, and the state its last whole record holds.
    pub fn open(mut nvmc: Nvmc<'static>) -> (Flash, Option<Stored>) {
        let mut last = None;
        let mut next = SLOTS;
        for slot in 0..SLOTS {
            let mut r = [0u8; RECORD_LEN];
            if nvmc.read(PAGE + slot * RECORD_LEN as u32, &mut r).is_err() {
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
        (Flash { nvmc, next }, last)
    }

    /// Keep `stored` in the next slot, erasing the page when it is full.
    pub fn keep(&mut self, stored: &Stored) {
        if self.next >= SLOTS {
            if let Err(e) = self.nvmc.erase(PAGE, PAGE + PAGE_SIZE as u32) {
                log::error!("store erase failed: {:?}", e);
                return;
            }
            self.next = 0;
        }
        let at = PAGE + self.next * RECORD_LEN as u32;
        self.next += 1;
        if let Err(e) = self.nvmc.write(at, &stored.record()) {
            log::error!("store write failed: {:?}", e);
        }
    }
}
