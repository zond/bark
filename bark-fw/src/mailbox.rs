//! The mailbox (spec "Mailbox"; `bark_app_layer::mailbox`): an
//! application on the node beside the phone link, kept in the flash
//! region `bark_image::layout::MAILBOX`, the ring and then a page of state
//! (`bark_app_layer::flash_ring`).
//!
//! The mailbox lives in a static, used in a few microseconds by the
//! node's loop (`node.rs`: what the core delivers, the replies it sends)
//! and by the phone link (`ble.rs`: Home, the Inbox read from the ring),
//! and never waits on the flash: what it writes is queued, and [`run`]
//! does it through the MPSL's flash timeslots, as the store does, between
//! the BLE controller's radio events. A message the core delivered is
//! confirmed to the core only once its slot is written, so that one taken
//! is not lost to a reboot meanwhile (`node.rs`).
//!
//! While the node is nobody's home nothing changes: the core's inbox is
//! the phone's, as without the mailbox.

use core::cell::RefCell;

use bark_app_layer::flash_ring::{FlashRing, Op, Region};
use bark_app_layer::mailbox::{Arrival, Mailbox};
use bark_image::layout::{MAILBOX, PAGE};
use bark_proto::wire::MAX_PAYLOAD;
use bark_proto::MsgId;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::signal::Signal;

use crate::store::SharedFlash;

/// The region, read where it lies in the chip's address space.
pub struct Mapped;

impl Region for Mapped {
    fn pages(&self) -> u32 {
        MAILBOX.len / PAGE
    }

    fn read(&self, offset: u32, out: &mut [u8]) {
        let at = (MAILBOX.start + offset) as *const u8;
        // SAFETY: flash is mapped from address 0 (nRF52840 Product
        // Specification, Memory), and the region lies in it; `Ring`'s
        // callers keep to its pages.
        let bytes = unsafe { core::slice::from_raw_parts(at, out.len()) };
        out.copy_from_slice(bytes);
    }
}

type Home = Mailbox<FlashRing<Mapped>>;

// The mailbox is built at boot into a cell in zeroed RAM: a static with a
// non-zero initial value would keep a copy of it in flash.
static HOME: Mutex<CriticalSectionRawMutex, RefCell<Option<&'static mut Home>>> =
    Mutex::new(RefCell::new(None));
static HOME_CELL: static_cell::StaticCell<Home> = static_cell::StaticCell::new();

/// Raised when the mailbox has queued a write.
static WRITE: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Raised when the queued writes are done, and when the phone sets or
/// clears the home: the node then confirms what was kept, and publishes
/// the change. A reply due needs no signal: the driver offers before every
/// wait.
pub static FOR_CORE: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Open the mailbox as it was before the reboot, once at boot.
pub fn open() {
    let m = Mailbox::new(FlashRing::open(Mapped));
    log::info!(
        "mailbox: home={} oldest={} newest={} unread={}",
        m.is_home(),
        m.oldest(),
        m.newest(),
        m.unread_count()
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

/// Whether the node is someone's home.
pub fn is_home() -> bool {
    with(|m| m.is_home()).unwrap_or(false)
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

/// Write what the mailbox queues, for ever: one op at a time, each in an
/// MPSL flash timeslot, the flash shared with the store.
#[embassy_executor::task]
pub async fn run(flash: &'static SharedFlash) -> ! {
    loop {
        WRITE.wait().await;
        while let Some(op) = with(|m| m.ring().op()).flatten() {
            {
                let mut f = flash.lock().await;
                let done = match op {
                    Op::Erase { offset } => {
                        let at = MAILBOX.start + offset;
                        f.erase(at, at + PAGE).await
                    }
                    Op::Write { offset, .. } => f.write(MAILBOX.start + offset, op.data()).await,
                };
                if let Err(e) = done {
                    log::error!("mailbox: flash {:?}", e);
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
