//! What the node still holds when it reboots into an image gathered over
//! the mesh, kept across the reboot (spec "Updates over the mesh", the
//! install; roadmap D201; `bark_app_layer::carry`): written to its region
//! of the external flash just before the reset, and at the boot after it
//! put back where it was, after the tables kept with it. The application's
//! own message, if the node held one, is offered to the core again as the
//! application's, through its send slot, before the phone's and the
//! mailbox's; given back, it is offered again after its wait. Once every
//! message is back the region is erased; a reset before then puts them
//! back again, and the network knows each by its ID.

use bark_app_layer::carry::{decode, encode, HELD_MAX, SLOT_LEN};
use bark_driver::Outgoing;
use bark_image::layout::external::{CARRY, SECTOR};
use bark_image::layout::Region;
use bark_proto::wire::MAX_PAYLOAD;
use bark_proto::{Duration, Held, HeldAt, MsgId};
use embedded_storage::nor_flash::ReadNorFlash;

use crate::ext_flash;
use crate::node::Role;

const SLOTS: usize = (CARRY.len as usize) / SLOT_LEN;
const _: () = assert!(SLOTS >= HELD_MAX);
const PER_SECTOR: usize = SECTOR as usize / SLOT_LEN;

/// Where the region stands: the node's loop's own, from the boot's
/// [`Carry::put_back`] on.
pub struct Carry {
    /// The slots written, while the region holds them.
    written: usize,
    /// The slot of the application's own message, until it is taken.
    own: Option<usize>,
    /// It has been offered and not given back: the next offer shows it
    /// was taken.
    offered: bool,
}

fn read(slot: usize, out: &mut [u8; SLOT_LEN]) -> bool {
    let at = CARRY.start + (slot * SLOT_LEN) as u32;
    ext_flash::with(|c| c.read(at, out)) == Some(Ok(()))
}

impl Carry {
    /// Nothing kept.
    pub const fn new() -> Carry {
        Carry {
            written: 0,
            own: None,
            offered: false,
        }
    }

    /// At boot, after the tables (`tables::restore`): put back into `core`
    /// what the region keeps, taken `ago` before now, and keep the
    /// application's own message to offer.
    pub async fn put_back(&mut self, core: &mut Role, ago: Duration) {
        let now = crate::clock::core_now();
        let mut s = [0; SLOT_LEN];
        let st = &mut *self;
        let (mut back, mut lost) = (0, 0);
        while st.written < SLOTS && read(st.written, &mut s) {
            let Some(m) = decode(&s) else { break };
            if m.at == HeldAt::Own {
                st.own = Some(st.written);
            } else if core.put_back(&m, ago, now) {
                back += 1;
            } else {
                log::error!("carry: id={:08x} not put back", m.msg_id.0);
                lost += 1;
            }
            st.written += 1;
        }
        if st.written > 0 {
            log::info!(
                "carry: {} messages put back, {} not, {} ms away assumed, own to offer: {}",
                back,
                lost,
                ago.as_millis(),
                st.own.is_some()
            );
        }
        self.tidy().await;
    }

    /// The application's own message kept, to offer the core, into
    /// `payload`, as `bark_driver::App::take` hands it.
    pub fn take(&mut self, payload: &mut [u8; MAX_PAYLOAD]) -> Option<Outgoing> {
        let st = self;
        if core::mem::take(&mut st.offered) {
            // Not given back: taken.
            st.own = None;
        }
        let mut s = [0; SLOT_LEN];
        let out = st
            .own
            .filter(|&slot| read(slot, &mut s))
            .and_then(|_| decode(&s))
            .map(|m| {
                payload[..m.payload.len()].copy_from_slice(m.payload);
                log::info!("carry: id={:08x} offered again", m.msg_id.0);
                st.offered = true;
                Outgoing {
                    dst_pos: m.dst_pos,
                    dst_id: m.dst_id,
                    len: m.payload.len(),
                }
            });
        if out.is_none() {
            st.own = None;
        }
        out
    }

    /// The core gave `msg_id` back: whether it was the application's own
    /// message kept, which is then offered again.
    pub fn given_back(&mut self, msg_id: MsgId) -> bool {
        let mut s = [0; SLOT_LEN];
        let ours = self.offered
            && self
                .own
                .filter(|&slot| read(slot, &mut s))
                .and_then(|_| decode(&s))
                .is_some_and(|m| m.msg_id == msg_id);
        if ours {
            self.offered = false;
        }
        ours
    }

    /// Erase the region once every message kept is back.
    pub async fn tidy(&mut self) {
        let st = &*self;
        if st.written == 0 || st.own.is_some() {
            return;
        }
        let sectors = st.written.div_ceil(PER_SECTOR) as u32;
        let written = Region {
            len: (sectors * SECTOR).min(CARRY.len),
            ..CARRY
        };
        let kept = st.written;
        ext_flash::erase_region(written, "carry").await;
        *self = Carry::new();
        log::info!("carry: every message back; {} kept", kept);
    }
}

/// Keep what the core holds, just before the reset: each message logged
/// by its ID.
pub async fn keep<'a>(held: impl Iterator<Item = Held<'a>>) {
    ext_flash::erase_region(CARRY, "carry").await;
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
