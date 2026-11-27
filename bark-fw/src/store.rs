//! The node's stored state in flash (roadmap D73, D241; spec
//! "Implementation notes"): what the core asks to keep (`Action::Store`),
//! handed back at the next boot in `Config::stored`; the phone's UTC
//! offset (`utc.rs`); and the node's phones (`phones.rs`), their records,
//! progress and settings. `bark_fw_logic::store` keeps them on two pages,
//! `bark_image::layout::STORE`, that take turns, so a power cut at any
//! moment leaves one whole; this is its task on the board.
//!
//! The writes go through the MPSL's flash timeslots (`nrf_mpsl::Flash`),
//! which fit each between the BLE controller's radio events: the CPU
//! stops while the NVMC writes, 41 µs a word, 0.33 ms a slot, and 85 ms
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
//! The MPSL's flash is one, taken once ([`SharedFlash`]): an update
//! writes the loader's record and state through it too
//! (`update/phone.rs`), each op a timeslot of its own.

use bark_fw_logic::store::{Kept, Latest, Store, SLOT_LEN};
use bark_image::layout::STORE;
use bark_link::phones;
use bark_proto::store::Stored;
use core::sync::atomic::{AtomicU32, Ordering::Relaxed};
use embassy_embedded_hal::flash::partition::{self, Partition};
use embassy_futures::select::{select3, Either3};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embedded_storage_async::nor_flash::{ErrorType, MultiwriteNorFlash, NorFlash, ReadNorFlash};

/// What the store's task is to write next: the core's state and the
/// offset, the latest of each, and that the phones changed.
struct ToWrite {
    stored: Signal<CriticalSectionRawMutex, Stored>,
    offset: Signal<CriticalSectionRawMutex, i16>,
    phones: Signal<CriticalSectionRawMutex, ()>,
}

static TO_WRITE: ToWrite = ToWrite {
    stored: Signal::new(),
    offset: Signal::new(),
    phones: Signal::new(),
};

/// The node's side: keep `stored`, in place of the state kept before.
pub fn keep(stored: Stored) {
    TO_WRITE.stored.signal(stored);
}

/// The node's side: keep the UTC offset, in place of the one kept before.
pub fn keep_offset(offset_min: i16) {
    TO_WRITE.offset.signal(offset_min);
}

/// The phones' changes asked to be kept, and of those the ones on flash:
/// equal when every change is kept.
static PHONES_ASKED: AtomicU32 = AtomicU32::new(0);
static PHONES_KEPT: AtomicU32 = AtomicU32::new(0);

/// The phones' side (`phones.rs`): keep what changed in their records.
pub fn keep_phones() {
    PHONES_ASKED.fetch_add(1, Relaxed);
    TO_WRITE.phones.signal(());
}

/// Whether every change to the phones' records is on flash. A mailbox
/// reply waits for it: a reboot before a fetch's `sent` is kept would let
/// the same fetch be taken again and its round's places carry other items
/// under the same nonces (spec "Mailbox").
pub fn phones_kept() -> bool {
    PHONES_ASKED.load(Relaxed) == PHONES_KEPT.load(Relaxed)
}

/// Write what the node and the phone link hand, for ever.
#[embassy_executor::task]
pub fn run(flash: Flash) -> impl core::future::Future<Output = ()> {
    timed!(STORE, work(flash))
}

async fn work(mut flash: Flash) {
    loop {
        let w = &TO_WRITE;
        let written = match select3(w.stored.wait(), w.offset.wait(), w.phones.wait()).await {
            Either3::First(stored) => flash.0.keep(stored, &Held).await,
            Either3::Second(offset_min) => flash.0.keep_offset(offset_min, &Held).await,
            Either3::Third(()) => {
                // Read before the write takes what changed: a change
                // after it is not counted as kept until the next write.
                let asked = PHONES_ASKED.load(Relaxed);
                let written = flash.keep_phones().await;
                // Not kept after a failed write: a reply waits for the
                // next write, which turns the page and writes every
                // record again.
                if written.is_ok() {
                    PHONES_KEPT.store(asked, Relaxed);
                }
                crate::node::WAKE.mailbox_changed();
                written
            }
        };
        if let Err(e) = written {
            log::error!("store: write failed: {}", Failed(e));
        }
    }
}

/// The tasks that may wait for the flash at once: the store's, the mesh
/// update's and its trial's, the phone link whose turn the update is
/// (`ble::UPDATING`), and the bench's `forget`.
const USERS: usize = 5;

type Mpsl = nrf_mpsl::Flash<'static>;

/// The MPSL's flash, shared by the store and the updates, a region each
/// ([`SharedFlash::part`]).
pub struct SharedFlash {
    flash: embassy_sync::mutex::Mutex<CriticalSectionRawMutex, Mpsl>,
    /// Only the task whose turn it is waits for `flash`.
    turn: crate::turn::Queue<USERS>,
}

/// The shared flash, once.
pub fn shared(flash: Mpsl) -> &'static SharedFlash {
    static FLASH: static_cell::StaticCell<SharedFlash> = static_cell::StaticCell::new();
    FLASH.init(SharedFlash {
        flash: embassy_sync::mutex::Mutex::new(flash),
        turn: crate::turn::Queue::new(),
    })
}

impl SharedFlash {
    /// The region `r` of the flash.
    pub fn part(&'static self, r: bark_image::layout::Region) -> Part {
        Part {
            pages: Partition::new(&self.flash, r.start, r.len),
            turn: &self.turn,
        }
    }
}

/// A region of the shared flash. Each read, write and erase waits its
/// turn at the flash.
pub struct Part {
    pages: Partition<'static, CriticalSectionRawMutex, Mpsl>,
    turn: &'static crate::turn::Queue<USERS>,
}

impl ErrorType for Part {
    type Error = partition::Error<<Mpsl as ErrorType>::Error>;
}

impl ReadNorFlash for Part {
    const READ_SIZE: usize = <Mpsl as ReadNorFlash>::READ_SIZE;

    async fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Self::Error> {
        let _turn = self.turn.turn().await;
        self.pages.read(offset, bytes).await
    }

    fn capacity(&self) -> usize {
        self.pages.capacity()
    }
}

impl NorFlash for Part {
    const WRITE_SIZE: usize = <Mpsl as NorFlash>::WRITE_SIZE;
    const ERASE_SIZE: usize = <Mpsl as NorFlash>::ERASE_SIZE;

    async fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Self::Error> {
        let _turn = self.turn.turn().await;
        self.pages.write(offset, bytes).await
    }

    async fn erase(&mut self, from: u32, to: u32) -> Result<(), Self::Error> {
        let _turn = self.turn.turn().await;
        self.pages.erase(from, to).await
    }
}

impl MultiwriteNorFlash for Part {}

/// The store's pages.
pub struct Flash(Store<Part>);

/// The phones as `phones.rs` holds them, which a turn writes again.
struct Held;

impl Latest for Held {
    fn settings(&self) -> [u8; SLOT_LEN] {
        crate::phones::read(|p| p.settings().encode())
    }

    fn record(&self, i: usize) -> Option<[u8; phones::RECORD_LEN]> {
        crate::phones::read(|p| Latest::record(p, i))
    }
}

impl Flash {
    /// The pages, and the last whole entry of each kind they hold.
    pub fn open(flash: &'static SharedFlash) -> (Flash, Kept) {
        let mut f = flash
            .flash
            .try_lock()
            .expect("nothing writes before the store opens");
        let (log, kept) = bark_fw_logic::store::scan(|at, r| f.read(STORE.start + at, r).is_ok());
        drop(f);
        if kept.before {
            log::info!("store: the page is of the layout before; what it holds is carried over");
        }
        if kept.discarded {
            log::warn!("store: no page of this layout; what the pages held is discarded");
        }
        let pages = flash.part(STORE);
        (Flash(Store::new(pages, log, &kept)), kept)
    }

    /// Keep what changed in the phones' records: a record whole, or its
    /// progress alone, and the settings.
    async fn keep_phones(&mut self) -> Result<(), Error> {
        let dirty = crate::phones::take_dirty();
        let mut written = Ok(());
        if dirty.settings {
            let s = Held.settings();
            written = written.and(self.0.write(&s, &Held).await);
        }
        for i in 0..phones::RECORDS {
            let bit = 1 << i;
            if dirty.records & bit != 0 {
                let r = crate::phones::read(|p| phones::Record::encode(i, p.get(i)));
                written = written.and(self.0.write(&r, &Held).await);
            } else if dirty.progress & bit != 0 {
                if let Some(e) = crate::phones::read(|p| p.progress_of(i)) {
                    written = written.and(self.0.write(&e.encode(), &Held).await);
                }
            }
        }
        written
    }
}

type Error = partition::Error<nrf_mpsl::FlashError>;

/// A flash operation's failure, the MPSL's error as its nRF errno,
/// negated: its `Debug` names every errno, 0.8 KB of flash for a line
/// that should never print.
struct Failed(Error);

impl core::fmt::Display for Failed {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        use nrf_mpsl::FlashError;
        match self.0 {
            partition::Error::Flash(FlashError::Mpsl(e)) => write!(f, "mpsl {}", i32::from(e)),
            partition::Error::Flash(FlashError::Unaligned) => f.write_str("unaligned"),
            partition::Error::Flash(FlashError::OutOfBounds) | partition::Error::OutOfBounds => {
                f.write_str("out of bounds")
            }
        }
    }
}
