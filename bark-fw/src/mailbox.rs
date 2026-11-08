//! The mailbox (spec "Mailbox"; `bark_app_layer::mailbox`): an
//! application on the node beside the phone link, its ring on the
//! external flash (`bark_image::layout::external::MAILBOX`) and its state,
//! which holds the mailbox key, in a page inside
//! (`bark_image::layout::MAILBOX_STATE`; `bark_app_layer::flash_ring`).
//!
//! The mailbox lives in a static, used by the node's loop (`node.rs`:
//! what the core delivers, the replies it sends) and by the phone link
//! (`ble.rs`: Home, the Inbox read from the ring). It reads the ring when
//! asked, about a millisecond a slot on the external flash's bus, and
//! never waits on a write: what it writes is queued, and [`run`] does it,
//! the ring's ops on the external flash with the executor free while the
//! chip writes (`ext_flash.rs`), the state's through the MPSL's flash
//! timeslots, as the store does, between the BLE controller's radio
//! events. A message the core delivered is confirmed to the core only
//! once its slot is written, so that one taken is not lost to a reboot
//! meanwhile (`node.rs`).
//!
//! While the node is nobody's home and not public nothing changes: the
//! core's inbox is the phone's, as without the mailbox. A public node's
//! ring keeps its owner's messages as a home's does (spec "Phone link",
//! Public).

use core::cell::RefCell;

use bark_app_layer::flash_ring::{Area, FlashRing, Op, Region};
use bark_app_layer::mailbox::{Arrival, Mailbox, SLOTS_PER_PAGE};
use bark_image::layout::{self, external, MAILBOX_STATE, PAGE};
use bark_proto::wire::MAX_PAYLOAD;
use bark_proto::MsgId;
use embassy_sync::blocking_mutex::raw::{CriticalSectionRawMutex, ThreadModeRawMutex};
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::signal::Signal;
use embassy_time::Instant;
use embedded_storage::nor_flash::ReadNorFlash;

use crate::ext_flash;
use crate::store::SharedFlash;

/// A region of the internal flash, read where it lies in the chip's
/// address space: the state's page.
pub struct Mapped(layout::Region);

impl Region for Mapped {
    fn pages(&self) -> u32 {
        self.0.len / PAGE
    }

    fn read(&self, offset: u32, out: &mut [u8]) {
        let at = (self.0.start + offset) as *const u8;
        // SAFETY: flash is mapped from address 0 (nRF52840 Product
        // Specification, Memory), and the region lies in it; `Ring`'s
        // callers keep to its pages.
        let bytes = unsafe { core::slice::from_raw_parts(at, out.len()) };
        out.copy_from_slice(bytes);
    }
}

/// The ring, on the external flash.
pub struct Ring;

impl Region for Ring {
    fn pages(&self) -> u32 {
        external::MAILBOX.len / PAGE
    }

    fn read(&self, offset: u32, out: &mut [u8]) {
        let read = ext_flash::with(|c| c.read(external::MAILBOX.start + offset, out));
        if read != Some(Ok(())) {
            // As erased: no item.
            out.fill(0xff);
        }
    }
}

type Home = Mailbox<FlashRing<Ring>>;

// The mailbox is built at boot into a cell in zeroed RAM: a static with a
// non-zero initial value would keep a copy of it in flash. Its users all
// run in thread mode, and a read of the ring holds it for a millisecond,
// or until an erase under way ends: so it masks no interrupt.
static HOME: Mutex<ThreadModeRawMutex, RefCell<Option<&'static mut Home>>> =
    Mutex::new(RefCell::new(None));
static HOME_CELL: static_cell::StaticCell<Home> = static_cell::StaticCell::new();

/// Raised when the mailbox has queued a write.
static WRITE: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Raised when the queued writes are done, and when the phone sets or
/// clears the home: the node then confirms what was kept, and publishes
/// the change. A reply due needs no signal: the driver offers before every
/// wait.
pub static FOR_CORE: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Open the mailbox as it was before the reboot, once at boot, after
/// `ext_flash::init`: every slot of the ring is read, and the time it
/// takes is logged, the external flash's bus speed on the board.
pub fn open() {
    let t0 = Instant::now();
    let m = Mailbox::new(FlashRing::open(Ring, &Mapped(MAILBOX_STATE)));
    log::info!(
        "mailbox: home={} oldest={} newest={} unread={} unsent={}, {} slots read in {} ms",
        m.is_home(),
        m.oldest(),
        m.newest(),
        m.unread_count(),
        m.unsent(),
        Ring.pages() * SLOTS_PER_PAGE,
        t0.elapsed().as_millis()
    );
    let m = HOME_CELL.init(m);
    HOME.lock(|c| *c.borrow_mut() = Some(m));
}

/// Use the mailbox; what `f` queues is written soon after.
pub fn with<R>(f: impl FnOnce(&mut Home) -> R) -> Option<R> {
    let (r, queued) = HOME.lock(|c| {
        let mut c = c.borrow_mut();
        let m = c.as_mut()?;
        let r = f(m);
        Some((r, !m.ring().idle()))
    })?;
    if queued {
        WRITE.signal(());
    }
    Some(r)
}

/// Whether the ring keeps what arrives: on a home or a public node.
pub fn keeps() -> bool {
    with(|m| m.keeps()).unwrap_or(false)
}

/// Whether every write asked for is done.
pub fn idle() -> bool {
    with(|m| m.ring().idle()).unwrap_or(true)
}

/// A payload the core delivered, into the mailbox.
pub fn arrive(msg_id: MsgId, payload: &[u8]) {
    match with(|m| m.arrive(msg_id, payload)) {
        Some(Arrival::Kept(n)) => log::info!("mailbox: id={:08x} kept as {}", msg_id.0, n),
        Some(Arrival::Fetch) => log::info!("mailbox: fetch id={:08x} taken", msg_id.0),
        Some(a) => log::info!("mailbox: id={:08x} {:?}", msg_id.0, a),
        None => {}
    }
}

/// The oldest item the phone at the node has not confirmed: the phone's
/// Inbox when the node is a home. Its number, the message ID it arrived
/// with, and its length in `out`.
pub fn unread(out: &mut [u8; MAX_PAYLOAD]) -> Option<(u32, MsgId, usize)> {
    with(|m| m.unread(out)).flatten()
}

/// Write what the mailbox queues, for ever, one op at a time: the ring's
/// on the external flash, the state's each in an MPSL flash timeslot, the
/// flash shared with the store.
#[embassy_executor::task]
pub async fn run(flash: &'static SharedFlash) -> ! {
    loop {
        WRITE.wait().await;
        while let Some(op) = with(|m| m.ring().op()).flatten() {
            match op {
                Op::Erase {
                    area: Area::Ring,
                    offset,
                } => {
                    if let Err(e) = ext_flash::erase(external::MAILBOX.start + offset).await {
                        log::error!("mailbox: ring {:?}", e);
                    }
                }
                Op::Write {
                    area: Area::Ring,
                    offset,
                    ..
                } => {
                    let at = external::MAILBOX.start + offset;
                    if let Err(e) = ext_flash::program(at, op.data()).await {
                        log::error!("mailbox: ring {:?}", e);
                    }
                }
                Op::Erase { offset, .. } => {
                    let at = MAILBOX_STATE.start + offset;
                    if let Err(e) = flash.lock().await.erase(at, at + PAGE).await {
                        log::error!("mailbox: state {:?}", e);
                    }
                }
                Op::Write { offset, .. } => {
                    let at = MAILBOX_STATE.start + offset;
                    if let Err(e) = flash.lock().await.write(at, op.data()).await {
                        log::error!("mailbox: state {:?}", e);
                    }
                }
            }
            with(|m| m.ring_mut().op_done());
        }
        if let Some(lost) = with(|m| m.ring().lost()).filter(|&l| l > 0) {
            log::error!("mailbox: {} writes lost", lost);
        }
        FOR_CORE.signal(());
    }
}
