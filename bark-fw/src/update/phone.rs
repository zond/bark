//! The phone link's update (spec "Phone link", Updates): the image
//! staged as the app writes it, through the same [`Stager`] as the
//! console's, committed through [`super::commit`].
//!
//! Its internal flash, the record and the loader's state page, is written
//! through the MPSL's flash timeslots, between the BLE controller's radio
//! events, as the store writes (`store.rs`); its external flash, the
//! staged slot, with the executor free while the chip erases or programs,
//! as the mailbox writes (`ext_flash.rs`, `mailbox.rs`). The BLE task
//! awaits each piece's writes before it takes the phone's next request,
//! so the phone's pieces wait in the host's queue meanwhile.

use bark_image::boot::{swap_requested, StageError, Stager};
use bark_image::layout::{RECORD, STATE};
use bark_image::{Header, HEADER_LEN};
use bark_link::update::{Progress, Refusal, State};
use embassy_embedded_hal::flash::partition::Partition;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embedded_storage::nor_flash::ReadNorFlash;

use super::{Power, Refused, BARK_IMAGE_VERSION};
use crate::ext_flash::Dfu;
use crate::store::SharedFlash;

const _: () = assert!(bark_link::update::HEADER_LEN == HEADER_LEN);
const _: () = assert!(bark_link::update::HASH_LEN == bark_image::HASH_LEN);

type Internal = Partition<'static, CriticalSectionRawMutex, nrf_mpsl::Flash<'static>>;

/// The stager of the node build.
type NodeStager = Stager<Internal, Internal, Dfu>;

/// The power the node last saw, from its loop (`node.rs`).
static POWER: Mutex<CriticalSectionRawMutex, core::cell::Cell<Power>> =
    Mutex::new(core::cell::Cell::new(Power {
        external: false,
        battery_mv: 0,
    }));

/// The node's side: the power after a step, the battery reading
/// calibrated as the core reads it.
pub fn note_power(power: Power) {
    POWER.lock(|p| p.set(power));
}

fn power() -> Power {
    POWER.lock(|p| p.get())
}

/// The update the phone link runs, kept across its connections, so that
/// a phone that loses the link goes on from the bytes held.
pub struct Phone {
    internal: &'static SharedFlash,
    /// The NVMC's handle for reading the state page (`update::started`).
    flash: &'static super::Flash,
    staging: Option<NodeStager>,
    /// The image committed: the swap is asked for.
    committed: Option<Header>,
    refused: Option<Refusal>,
}

impl Phone {
    /// The update at boot: committed if a swap is still asked for, as when
    /// the node was committed and not rebooted, else idle.
    pub fn new(internal: &'static SharedFlash, flash: &'static super::Flash) -> Phone {
        let committed = match swap_requested(flash) {
            Ok(true) => recorded(flash),
            _ => None,
        };
        Phone {
            internal,
            flash,
            staging: None,
            committed,
            refused: None,
        }
    }

    /// Where the update stands, as the phone reads it.
    pub fn progress(&self) -> Progress {
        let (state, held, header) = match (&self.committed, &self.staging) {
            (Some(h), _) => (State::Committed, h.length, Some(*h)),
            (None, Some(s)) if s.next() == s.header().length => {
                (State::Staged, s.next(), Some(*s.header()))
            }
            (None, Some(s)) => (State::Receiving, s.next(), Some(*s.header())),
            (None, None) => (State::Idle, 0, None),
        };
        Progress {
            state,
            refused: self.refused,
            held,
            version: header.map_or(0, |h| h.version),
            hash: header.map_or([0; bark_image::HASH_LEN], |h| h.hash),
        }
    }

    /// Begin staging the image `header` describes, forgetting any staged
    /// before.
    pub async fn begin(&mut self, header: &[u8; HEADER_LEN]) -> Result<(), Refusal> {
        self.staging = None;
        let internal = self.internal;
        let part = |r: bark_image::layout::Region| Partition::new(internal, r.start, r.len);
        let begun = Stager::begin(part(RECORD), part(STATE), Dfu, header, BARK_IMAGE_VERSION).await;
        match begun {
            Ok(s) => {
                log::info!(
                    "update: receiving version={} length={} from the phone",
                    s.header().version,
                    s.header().length
                );
                self.staging = Some(s);
                self.refused = None;
                Ok(())
            }
            Err(e) => {
                log::warn!("update: begin refused={:?}", e);
                Err(self.refuse(refusal(Refused::Stage(e))))
            }
        }
    }

    /// Write a piece at `offset`; whether it was taken. A piece taken
    /// only where the bytes held end; any other is left, and the phone
    /// sends again from what it reads.
    pub async fn piece(&mut self, offset: u32, bytes: &[u8]) -> bool {
        let Some(s) = self.staging.as_mut() else {
            return false;
        };
        match s.write(offset, bytes).await {
            Ok(next) => {
                if next == s.header().length {
                    log::info!(
                        "update: staged version={}, all {} bytes held; commit next",
                        s.header().version,
                        next
                    );
                }
                true
            }
            Err(StageError::Offset { .. } | StageError::Chunk) => false,
            Err(e) => {
                log::error!("update: staging stopped={:?}", e);
                self.staging = None;
                self.refuse(refusal(Refused::Stage(e)));
                false
            }
        }
    }

    /// Commit the image held ([`super::commit`]).
    pub async fn commit(&mut self) -> Result<(), Refusal> {
        let power = power();
        match super::commit(&mut self.staging, power).await {
            Ok(h) => {
                log::info!(
                    "update: committed version={} length={}: the loader swaps it in at the next boot",
                    h.version,
                    h.length
                );
                self.committed = Some(h);
                self.refused = None;
                Ok(())
            }
            Err(e) => {
                log::warn!(
                    "update: commit refused={:?} usb={} battery_mv={}",
                    e,
                    power.external,
                    power.battery_mv
                );
                Err(self.refuse(refusal(e)))
            }
        }
    }

    /// Whether the phone's reboot may go ahead ([`super::reboot_allowed`]).
    pub fn reboot(&mut self) -> Result<(), Refusal> {
        let power = power();
        if super::reboot_allowed(self.flash, power) {
            log::info!("update: reboot asked by the phone");
            return Ok(());
        }
        log::warn!(
            "update: reboot refused=BatteryLow battery_mv={}: no swap on battery at or below the low level",
            power.battery_mv
        );
        Err(self.refuse(Refusal::Battery))
    }

    /// The link closed: one line on where the update stands.
    pub fn link_closed(&self) {
        let p = self.progress();
        if p.state == State::Receiving {
            log::info!(
                "update: link closed with {} bytes of version {} held",
                p.held,
                p.version
            );
        }
    }

    fn refuse(&mut self, why: Refusal) -> Refusal {
        self.refused = Some(why);
        why
    }
}

/// The header of the record, which a commit wrote.
fn recorded(flash: &super::Flash) -> Option<Header> {
    let mut b = [0u8; HEADER_LEN];
    let at = RECORD.start + bark_image::record::HEADER_AT;
    flash.lock(|f| f.borrow_mut().read(at, &mut b)).ok()?;
    Header::decode(&b).ok()
}

/// The phone's word for a refusal.
fn refusal(e: Refused) -> Refusal {
    match e {
        Refused::Nothing => Refusal::Incomplete,
        Refused::BatteryLow => Refusal::Battery,
        Refused::Stage(e) => match e {
            StageError::Header(_) => Refusal::Header,
            StageError::Version { .. } => Refusal::Version,
            StageError::Busy => Refusal::Busy,
            StageError::Offset { .. } | StageError::Chunk | StageError::Incomplete { .. } => {
                Refusal::Incomplete
            }
            StageError::Hash => Refusal::Hash,
            StageError::Flash(_) => Refusal::Flash,
        },
    }
}
