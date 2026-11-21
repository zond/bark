//! The mailbox (spec "Mailbox"; `bark_app_layer::mailbox`): an
//! application on the node beside the phone link, its ring on the
//! external flash (`bark_image::layout::external::MAILBOX`). Its people,
//! their tags and keys, come from the phones' records (`phones.rs`),
//! which keep what it moves on. Every node keeps every message in it, its
//! phones' and its guests' (spec "Phone link", Receiving).
//!
//! The mailbox lives in a static, used by the node's loop (`node.rs`:
//! what the core delivers, the replies it sends) and by the phone link
//! (`ble.rs`: Home, the Inbox read from the ring). It reads the ring when
//! asked, about a millisecond a slot on the external flash's bus, and
//! never waits on a write: what it writes is queued, and [`run`] does it
//! on the external flash with the executor free while the chip writes
//! (`ext_flash.rs`). A message the core delivered is confirmed to the
//! core only once its slot is written, so that one taken is not lost to a
//! reboot meanwhile (`node.rs`).

use core::cell::RefCell;

use bark_app_layer::flash_ring::{FlashRing, Op, Region};
use bark_app_layer::mailbox::{Arrival, Mailbox, SLOTS_PER_PAGE};
use bark_image::layout::{external, PAGE};
use bark_proto::wire::MAX_PAYLOAD;
use bark_proto::MsgId;
use embassy_sync::blocking_mutex::raw::{CriticalSectionRawMutex, ThreadModeRawMutex};
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::signal::Signal;
use embassy_time::Instant;
use embedded_storage::nor_flash::ReadNorFlash;

use crate::ext_flash;

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

/// Open the mailbox as it was before the reboot, once at boot, after
/// `ext_flash::init`, with its people from their records: every slot of
/// the ring is read, and the time it takes is logged, the external
/// flash's bus speed on the board.
pub fn open() {
    let t0 = Instant::now();
    let m = Mailbox::new(FlashRing::open(Ring), crate::node_id());
    let ms = t0.elapsed().as_millis();
    let m = HOME_CELL.init(m);
    HOME.lock(|c| *c.borrow_mut() = Some(m));
    crate::phones::to_mailbox();
    let (home, oldest, newest) =
        with(|m| (m.is_home(), m.oldest(), m.newest())).unwrap_or_default();
    log::info!(
        "mailbox: home={} oldest={} newest={}, {} slots read in {} ms",
        home,
        oldest,
        newest,
        Ring.pages() * SLOTS_PER_PAGE,
        ms
    );
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

/// Whether every write asked for is done.
pub fn idle() -> bool {
    with(|m| m.ring().idle()).unwrap_or(true)
}

/// A payload the core delivered, into the mailbox; a fetch taken moves
/// its person's last fetch, which their record keeps, and its command
/// runs on the records (`phones::fetched`).
pub fn arrive(msg_id: MsgId, payload: &[u8]) {
    match with(|m| m.arrive(msg_id, payload)) {
        Some(Arrival::Kept(n)) => log::info!("mailbox: id={:08x} kept as {}", msg_id.0, n),
        Some(Arrival::Fetch {
            person,
            removed: true,
            ..
        }) => {
            log::info!(
                "mailbox: fetch id={:08x} taken for record {}, whose home was removed: told so, nothing run",
                msg_id.0,
                person
            );
            crate::phones::fetched(person, None);
        }
        Some(Arrival::Fetch {
            person, command, ..
        }) => {
            log::info!(
                "mailbox: fetch id={:08x} taken for record {}, command {:?}",
                msg_id.0,
                person,
                command.map(|c| c.op())
            );
            crate::phones::fetched(person, command);
        }
        Some(a) => log::info!("mailbox: id={:08x} {:?}", msg_id.0, a),
        None => {}
    }
}

/// The oldest of person `i`'s items their phone has not confirmed: its
/// number, the message ID it arrived with, and its length in `out`.
pub fn unread(i: usize, out: &mut [u8; MAX_PAYLOAD]) -> Option<(u32, MsgId, usize)> {
    with(|m| m.unread(i, out)).flatten()
}

/// Write what the mailbox queues, for ever, one op at a time, on the
/// external flash.
#[embassy_executor::task]
pub fn run() -> impl core::future::Future<Output = ()> {
    timed!(MAILBOX, work())
}

async fn work() {
    loop {
        WRITE.wait().await;
        while let Some(op) = with(|m| m.ring().op()).flatten() {
            match op {
                Op::Erase { offset } => {
                    if let Err(e) = ext_flash::erase(external::MAILBOX.start + offset).await {
                        log::error!("mailbox: ring {:?}", e);
                    }
                }
                Op::Write { offset, .. } => {
                    // A mark is a byte programmed again over a written one,
                    // which the chip takes, keeping the AND of the two
                    // (`README.md`, "Verified and unverified").
                    let at = external::MAILBOX.start + offset;
                    if let Err(e) = ext_flash::program(at, op.data()).await {
                        log::error!("mailbox: ring {:?}", e);
                    }
                }
            }
            with(|m| m.ring_mut().op_done());
        }
        if let Some(lost) = with(|m| m.ring().lost()).filter(|&l| l > 0) {
            log::error!("mailbox: {} writes lost", lost);
        }
        crate::node::WAKE.mailbox_changed();
    }
}
