//! What the node still holds when it reboots into an image gathered over
//! the mesh, kept across the reboot (spec "Updates over the mesh", the
//! install; `bark_app_layer::carry`): written to its region of the
//! external flash just before the reset, and after it offered to the core
//! again as the node's own, one at a time through the application's send
//! slot, before the phone's and the mailbox's, since the network took
//! them first. A message given back is offered again after its wait. Once
//! every one has gone the region is erased; a reset before then offers
//! them again, and the network knows each by its ID.

use bark_app_layer::carry::{decode, encode, HELD_MAX, SLOT_LEN};
use bark_driver::Outgoing;
use bark_image::layout::external::{CARRY, SECTOR};
use bark_proto::wire::MAX_PAYLOAD;
use bark_proto::{Held, MsgId};
use core::cell::Cell;
use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embedded_storage::nor_flash::ReadNorFlash;

use crate::ext_flash;

const SLOTS: usize = (CARRY.len as usize) / SLOT_LEN;
const _: () = assert!(SLOTS >= HELD_MAX);
const PER_SECTOR: usize = SECTOR as usize / SLOT_LEN;

/// Where the offering stands.
#[derive(Clone, Copy)]
struct State {
    /// The next slot to offer.
    next: usize,
    /// The slot offered last and its message, until the next offer shows
    /// it was taken, or its given back says it was not.
    offered: Option<(usize, MsgId)>,
    /// The region holds slots not yet erased.
    kept: bool,
}

static STATE: Mutex<ThreadModeRawMutex, Cell<State>> = Mutex::new(Cell::new(State {
    next: 0,
    offered: None,
    kept: false,
}));

fn read(slot: usize, out: &mut [u8; SLOT_LEN]) -> bool {
    let at = CARRY.start + (slot * SLOT_LEN) as u32;
    ext_flash::with(|c| c.read(at, out)) == Some(Ok(()))
}

/// At boot, after `ext_flash::init`: whether the region keeps messages
/// from before the reboot.
pub fn open() {
    let mut s = [0; SLOT_LEN];
    let kept = read(0, &mut s) && decode(&s).is_some();
    STATE.lock(|c| c.set(State { kept, ..c.get() }));
    if kept {
        log::info!("carry: messages kept across the reboot, to offer again");
    }
}

/// The next message to offer the core, into `payload`, as
/// `bark_driver::App::take` hands it.
pub fn take(payload: &mut [u8; MAX_PAYLOAD]) -> Option<Outgoing> {
    let mut st = STATE.lock(|c| c.get());
    if !st.kept {
        return None;
    }
    // The one offered before and not given back was taken.
    if let Some((slot, _)) = st.offered.take() {
        st.next = slot + 1;
    }
    let mut s = [0; SLOT_LEN];
    let out = (st.next < SLOTS && read(st.next, &mut s))
        .then(|| decode(&s))
        .flatten()
        .map(|m| {
            payload[..m.payload.len()].copy_from_slice(m.payload);
            log::info!("carry: id={:08x} offered again", m.msg_id.0);
            st.offered = Some((st.next, m.msg_id));
            Outgoing {
                dst_pos: m.dst_pos,
                dst_id: m.dst_id,
                len: m.payload.len(),
            }
        });
    STATE.lock(|c| c.set(st));
    out
}

/// The core gave `msg_id` back: whether it was the one offered, which is
/// then offered again.
pub fn given_back(msg_id: MsgId) -> bool {
    STATE.lock(|c| {
        let mut st = c.get();
        let ours = st.offered.is_some_and(|(_, id)| id == msg_id);
        if let Some((slot, _)) = st.offered.filter(|_| ours) {
            st.next = slot;
            st.offered = None;
            c.set(st);
        }
        ours
    })
}

/// Erase the region once every message kept has been taken.
pub async fn tidy() {
    let st = STATE.lock(|c| c.get());
    let mut s = [0; SLOT_LEN];
    let done = st.offered.is_none()
        && st.kept
        && (st.next >= SLOTS || !read(st.next, &mut s) || decode(&s).is_none());
    if !done {
        return;
    }
    erase(st.next.div_ceil(PER_SECTOR).max(1)).await;
    STATE.lock(|c| {
        c.set(State {
            kept: false,
            next: 0,
            offered: None,
        })
    });
    log::info!("carry: every message offered again; {} kept", st.next);
}

async fn erase(sectors: usize) {
    for k in 0..sectors.min(CARRY.len as usize / SECTOR as usize) {
        if let Err(e) = ext_flash::erase(CARRY.start + k as u32 * SECTOR).await {
            log::error!("carry: erase {:?}", e);
        }
    }
}

/// Keep what the core holds, just before the reset: each message logged
/// by its ID.
pub async fn keep<'a>(held: impl Iterator<Item = Held<'a>>) {
    erase(CARRY.len as usize / SECTOR as usize).await;
    let mut n = 0;
    for m in held.take(SLOTS) {
        let Some(s) = encode(&m) else { continue };
        let at = CARRY.start + (n * SLOT_LEN) as u32;
        match ext_flash::program(at, &s).await {
            Ok(()) => log::info!("carry: id={:08x} kept across the reboot", m.msg_id.0),
            Err(e) => log::error!("carry: id={:08x} lost: {:?}", m.msg_id.0, e),
        }
        n += 1;
    }
    log::info!("carry: {} messages kept across the reboot", n);
}
