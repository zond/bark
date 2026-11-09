//! The running image's side of an update (`bark_image::boot`,
//! `docs/mesh-update-2026-10-25.md`): it confirms itself, so that the
//! loader keeps it, once the node has started if a person staged it, and
//! once the node has heard another node if it came over the mesh ([`trial`];
//! spec "Updates over the mesh", the install); from then on it feeds the
//! watchdog the loader started for its trial, which runs on until the next
//! reset (nRF52840 Product Specification, WDT: once started it cannot be
//! stopped). An image is staged on the external flash through the
//! console (`console.rs`, `stage`) or the phone link ([`Phone`];
//! `ble.rs`), or gathered over the mesh (`mesh`), and all commit and
//! reboot through the same functions here ([`commit`], [`reboot`]), so
//! that they check the same.

use core::cell::RefCell;

use bark_image::boot::{cut, start, swap_requested, Shared, StageError, Stager, Started};
use bark_image::layout::{LOADER, PAGE};
use bark_image::sign::{Key, SLOTS};
use bark_image::{Header, HEADER_LEN};
use bark_proto::BatteryLevels;
use embassy_nrf::nvmc::Nvmc;
use embassy_nrf::{peripherals, wdt, Peri};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Timer};

#[cfg(not(feature = "console"))]
use core::sync::atomic::{AtomicBool, Ordering};
use embedded_storage::nor_flash::NorFlash;
use embedded_storage_async::nor_flash::NorFlash as AsyncNorFlash;

/// This image's version, from `BARK_IMAGE_VERSION` at build time, 0
/// without it. An image is staged only above the running one's. Kept
/// under its own symbol so that `scripts/image.py` reads it from the ELF
/// and the header can never say another.
#[used]
#[no_mangle]
pub static BARK_IMAGE_VERSION: u32 = match option_env!("BARK_IMAGE_VERSION") {
    Some(v) => decimal(v),
    None => 0,
};

/// The keys an image to stage must be signed by one of: the project's,
/// and in an open build the bench key as well (`build.rs`). Kept under
/// its own symbol so that `scripts/uf2.py` and `scripts/image.py` read
/// them from the ELF.
#[used]
#[no_mangle]
pub static BARK_SIGNING_KEYS: [Key; SLOTS] = include!(concat!(env!("OUT_DIR"), "/keys.rs"));

/// What a bench image does instead of confirming itself, from
/// `BARK_UPDATE_BENCH` at build time (roadmap Phase 2, the loader's rows).
enum Bench {
    /// Runs on and never confirms: the watchdog ends its trial.
    Unconfirmed,
    /// Panics: the soft reset ends its trial.
    Panic,
    /// Spins with the executor blocked: the watchdog ends its trial.
    Hang,
    /// Tries to erase the loader's first page, which the loader's write
    /// protection must stop with a bus fault. Should it get through, the
    /// board's bootloader finds nothing to start and waits for a UF2.
    EraseLoader,
    /// On the boot that swapped it in, drops every frame it receives
    /// ([`deaf`]): gathered over the mesh, it hears no node and resets
    /// itself, so that the loader swaps the previous image back, which
    /// gives it up; staged from the phone, it keeps itself and hears no one
    /// until the next reset, for the app's warning. Any other boot hears,
    /// so a holder flashed by UF2 serves it.
    Deaf,
}

const BENCH: Option<Bench> = match option_env!("BARK_UPDATE_BENCH") {
    None => None,
    Some(v) if same(v, "unconfirmed") => Some(Bench::Unconfirmed),
    Some(v) if same(v, "panic") => Some(Bench::Panic),
    Some(v) if same(v, "hang") => Some(Bench::Hang),
    Some(v) if same(v, "erase-loader") => Some(Bench::EraseLoader),
    Some(v) if same(v, "deaf") => Some(Bench::Deaf),
    Some(_) => panic!("BARK_UPDATE_BENCH is unconfirmed, panic, hang, erase-loader or deaf"),
};

const fn same(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// A build-time variable as a number; the build fails on anything else.
pub const fn decimal(s: &str) -> u32 {
    let b = s.as_bytes();
    assert!(!b.is_empty(), "a build-time number is empty");
    let mut v: u32 = 0;
    let mut i = 0;
    while i < b.len() {
        assert!(b[i].is_ascii_digit(), "a build-time number has a non-digit");
        v = v * 10 + (b[i] - b'0') as u32;
        i += 1;
    }
    v
}

/// The flash, shared by the stored state, the update and the console.
pub type Flash = Shared<Nvmc<'static>>;

/// The flash behind `nvmc`, once.
pub fn flash(nvmc: Nvmc<'static>) -> &'static Flash {
    static FLASH: static_cell::StaticCell<Flash> = static_cell::StaticCell::new();
    FLASH.init(Mutex::new(RefCell::new(nvmc)))
}

/// The external flash, where an image is staged.
#[cfg(feature = "console")]
pub type External = Shared<crate::ext_flash::Kept>;

/// The console's stager: both flashes blocking, the CPU its own.
#[cfg(feature = "console")]
pub type ConsoleStager = Stager<
    bark_image::boot::Blocking<'static, Nvmc<'static>>,
    bark_image::boot::Blocking<'static, Nvmc<'static>>,
    bark_image::boot::Blocking<'static, crate::ext_flash::Kept>,
>;

/// The external flash, once.
#[cfg(feature = "console")]
pub fn external() -> &'static External {
    static EXTERNAL: static_cell::StaticCell<External> = static_cell::StaticCell::new();
    EXTERNAL.init(Mutex::new(RefCell::new(crate::ext_flash::Kept)))
}

/// Set once the image has confirmed itself, for [`watchdog`].
static CONFIRMED: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// The node has started, its core set up and its radio up: keep this
/// image, if a person staged it, or wait for the node to hear another
/// ([`trial`]), if it came over the mesh. Until it is kept, a reset swaps
/// the previous image back. It writes the internal flash through the
/// NVMC directly, so it runs before the phone link's controller starts.
pub fn started(flash: &Flash) {
    // What the loader protected, as the ACL reads back (nRF52840 Product
    // Specification, ACL): all zeros if this image runs without it.
    for n in 0..2 {
        let acl = embassy_nrf::pac::ACL.acl(n);
        log::info!(
            "update: acl {} start={:#x} len={:#x} write={:?}",
            n,
            acl.addr().read(),
            acl.size().read(),
            acl.perm().read().write()
        );
    }
    match BENCH {
        None | Some(Bench::Deaf) => {}
        Some(Bench::EraseLoader) => {
            log::warn!("bench image: erasing the loader's first page");
            let page = LOADER.start..LOADER.start + PAGE;
            let erased = flash.lock(|f| f.borrow_mut().erase(page.start, page.end));
            log::error!(
                "the loader's page erase returned {:?}: not protected",
                erased
            );
            return;
        }
        Some(Bench::Unconfirmed) => {
            log::warn!("bench image: not confirming");
            return;
        }
        Some(Bench::Panic) => panic!("bench image: panicking before it confirms"),
        Some(Bench::Hang) => {
            log::warn!("bench image: hanging before it confirms");
            loop {
                cortex_m::asm::nop();
            }
        }
    }
    let on_trial = match start(flash) {
        Ok(Started::Normal) => false,
        Ok(Started::Confirmed) => {
            log::info!("update: version {} confirmed", BARK_IMAGE_VERSION);
            swapped_in();
            false
        }
        Ok(Started::OnTrial) => {
            log::info!(
                "update: version {} on trial: it confirms once it hears a node, within {} s",
                BARK_IMAGE_VERSION,
                HEARD_WITHIN.as_secs()
            );
            swapped_in();
            true
        }
        Ok(Started::Reverted { gave_up }) => {
            log::warn!(
                "update: the new image did not confirm itself; version {} is back",
                BARK_IMAGE_VERSION
            );
            match gave_up {
                Some(v) => log::warn!("update: version {} given up: never gathered again", v),
                None => log::warn!("update: the image swapped back is not named; none given up"),
            }
            false
        }
        Ok(Started::Refused) => {
            log::warn!("update: the loader refused the staged image");
            false
        }
        Err(e) => {
            log::error!("update: confirming failed: {:?}", e);
            false
        }
    };
    // What a bench loader noted in the record of the last update's resets;
    // nothing from any other loader.
    match cut::recorded(flash) {
        Ok(cuts) => cuts
            .iter()
            .flatten()
            .for_each(|c| log::info!("update: {}", c)),
        Err(e) => log::error!("update: reading the record failed: {:?}", e),
    }
    match on_trial {
        true => on_trial_now(flash),
        false => kept(),
    }
}

/// The image is kept: the watchdog is fed from now on, and the update over
/// the mesh starts, which must not ask for a swap while one is on trial.
fn kept() {
    CONFIRMED.signal(());
    #[cfg(not(feature = "console"))]
    mesh::START.signal(());
}

/// How long an image gathered over the mesh waits to hear a node, from its
/// start (`bark_proto::update::heard_within`): 585.9 s.
const HEARD_WITHIN: Duration = Duration::from_micros(
    bark_proto::update::heard_within(&bark_proto::Config::DEFAULT).as_micros(),
);

// The watchdog, fed only once the image is kept, bounds the wait: it
// resets an image that hangs, or whose own deadline never fires, 600 s
// after the loader's last feed, just before the jump.
const _: () = assert!(HEARD_WITHIN.as_micros() < bark_image::TRIAL_SECONDS as u64 * 1_000_000);

/// The image on trial, for [`trial`].
#[cfg(not(feature = "console"))]
static ON_TRIAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// A node was heard (`AppEvent::Heard`), for [`trial`].
#[cfg(not(feature = "console"))]
static HEARD: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Set on the boot that swapped in a [`Bench::Deaf`] image.
#[cfg(not(feature = "console"))]
static DEAF: AtomicBool = AtomicBool::new(false);

/// This is the boot that swapped the image in.
fn swapped_in() {
    #[cfg(not(feature = "console"))]
    if matches!(BENCH, Some(Bench::Deaf)) {
        log::warn!("bench image: deaf to every frame until the next reset");
        DEAF.store(true, Ordering::Relaxed);
    }
}

/// Whether the node drops every frame it receives ([`Bench::Deaf`]).
#[cfg(not(feature = "console"))]
pub fn deaf() -> bool {
    DEAF.load(Ordering::Relaxed)
}

/// The core heard another node (`node.rs`).
#[cfg(not(feature = "console"))]
pub fn heard() {
    HEARD.signal(());
}

#[cfg(not(feature = "console"))]
fn on_trial_now(_: &Flash) {
    ON_TRIAL.signal(());
}

/// The console has no core to hear with: it keeps itself. No console
/// image comes over the mesh, as no console gathers or serves one.
#[cfg(feature = "console")]
fn on_trial_now(flash: &Flash) {
    use bark_image::boot::Part;
    use bark_image::layout::STATE;
    use embassy_embedded_hal::adapter::BlockingAsync;
    let state = BlockingAsync::new(Part::new(flash, STATE.start, STATE.len));
    match embassy_futures::block_on(bark_image::boot::confirm(state)) {
        Ok(_) => log::info!("update: version {} confirmed", BARK_IMAGE_VERSION),
        Err(e) => log::error!("update: confirming failed: {:?}", e),
    }
    kept();
}

/// An image gathered over the mesh, on trial: keep it once the core has
/// heard a node, through the MPSL's flash timeslots now that the phone
/// link may run; reset, for the loader to swap the previous image back,
/// if it hears none within [`HEARD_WITHIN`] of its start
/// (spec "Updates over the mesh", the install).
#[cfg(not(feature = "console"))]
pub async fn trial(internal: &'static crate::store::SharedFlash) {
    use bark_image::layout::STATE;
    use embassy_embedded_hal::flash::partition::Partition;
    use embassy_futures::select::{select, Either};
    ON_TRIAL.wait().await;
    let deadline = embassy_time::Instant::from_micros(HEARD_WITHIN.as_micros());
    if let Either::Second(()) = select(HEARD.wait(), Timer::at(deadline)).await {
        log::warn!(
            "update: version {} heard no node within {} s; resetting so that the loader swaps the previous image back",
            BARK_IMAGE_VERSION,
            HEARD_WITHIN.as_secs()
        );
        reboot().await;
    }
    loop {
        let state = Partition::new(internal, STATE.start, STATE.len);
        match bark_image::boot::confirm(state).await {
            Ok(_) => {
                log::info!(
                    "update: version {} confirmed, a node heard {} s after its start",
                    BARK_IMAGE_VERSION,
                    embassy_time::Instant::now().as_secs()
                );
                kept();
                return;
            }
            // Again a second later, until the deadline.
            Err(e) => log::error!("update: confirming failed: {:?}", e),
        }
        if embassy_time::Instant::now() >= deadline {
            log::warn!(
                "update: version {} not confirmed in time; resetting",
                BARK_IMAGE_VERSION
            );
            reboot().await;
        }
        Timer::after_secs(1).await;
    }
}

/// Feed the watchdog, if the loader started one, from the confirmation on:
/// twice a period, so that one late wakeup still passes. Not before: the
/// loader's watchdog is what ends a trial whose image hangs.
pub async fn watchdog(dog: Peri<'static, peripherals::WDT>) {
    CONFIRMED.wait().await;
    let Some(config) = wdt::Config::try_new(&dog) else {
        return;
    };
    // The timeout is (CRV + 1) / 32 768 s (Product Specification, WDT).
    let period = Duration::from_hz(32_768) * (config.timeout_ticks + 1);
    // The configuration read back is the running one, so this cannot fail.
    let Ok((_dog, [mut handle])) = wdt::Watchdog::try_new::<_, 1>(dog, config) else {
        log::error!("update: the watchdog runs with handles this image does not know");
        return;
    };
    log::info!(
        "update: feeding the watchdog every {} s",
        period.as_secs() / 2
    );
    loop {
        handle.pet();
        Timer::after(period / 2).await;
    }
}

/// Start staging the image `header` describes on `record`, `state` and
/// `dfu`, if this image accepts it: above its own version and signed by
/// one of its keys (`bark_image::sign`). The console's `stage` and the
/// phone link's begin both come here. The hash is checked when the
/// staging finishes ([`commit`]).
pub async fn stage<R, S, D>(
    record: R,
    state: S,
    dfu: D,
    header: &[u8; HEADER_LEN],
) -> Result<Stager<R, S, D>, StageError>
where
    R: AsyncNorFlash,
    S: AsyncNorFlash,
    D: AsyncNorFlash,
{
    Stager::begin(
        record,
        state,
        dfu,
        header,
        BARK_IMAGE_VERSION,
        &BARK_SIGNING_KEYS,
    )
    .await
}

/// What the node runs on, for the rule on a swap.
#[derive(Clone, Copy, Debug, Default)]
pub struct Power {
    /// On external power, USB.
    pub external: bool,
    /// The battery's last reading, millivolts; 0 before one.
    pub battery_mv: u16,
}

/// Whether a swap may start on `power`: on external power, or on a
/// battery above the low level of spec "Sleepy nodes" (spec
/// "Implementation notes"; roadmap D143).
pub fn swap_allowed(power: Power) -> bool {
    BatteryLevels::DESIGN.swap_allowed(power.external, power.battery_mv)
}

/// Why a commit was refused.
#[derive(Clone, Copy, Debug)]
pub enum Refused {
    /// Nothing is being staged.
    Nothing,
    /// On battery at or below the low level ([`swap_allowed`]).
    BatteryLow,
    /// The staged image's own checks ([`Stager::finish`]).
    Stage(StageError),
}

/// Check the image `staging` holds and ask the loader to swap it in at
/// the next boot, on `power`. The console's `commit` and the phone
/// link's both come here, so that every check applies to both. An image
/// not yet whole, or refused for the battery, stays staged, to be
/// finished or committed later; one its checks refuse is forgotten.
pub async fn commit<R, S, D>(
    staging: &mut Option<Stager<R, S, D>>,
    power: Power,
) -> Result<Header, Refused>
where
    R: AsyncNorFlash,
    S: AsyncNorFlash,
    D: AsyncNorFlash,
{
    let Some(s) = staging.as_ref() else {
        return Err(Refused::Nothing);
    };
    if s.next() != s.header().length {
        return Err(Refused::Stage(StageError::Incomplete { got: s.next() }));
    }
    if !swap_allowed(power) {
        return Err(Refused::BatteryLow);
    }
    match staging.take() {
        Some(s) => s.finish().await.map_err(Refused::Stage),
        None => Err(Refused::Nothing),
    }
}

/// Whether a reboot may go ahead on `power`: refused only when it would
/// start a swap on a low battery ([`swap_allowed`]).
pub fn reboot_allowed(flash: &Flash, power: Power) -> bool {
    swap_requested(flash) != Ok(true) || swap_allowed(power)
}

/// How long [`reboot`] waits for the log to reach the host: the log's
/// buffer at one 64-byte packet per 1 ms frame (USB 2.0 full speed, bulk).
const LOG_DRAIN: Duration =
    Duration::from_millis((crate::LOG_BUFFER / crate::USB_PACKET as usize) as u64);

/// Reset the chip, to let the loader swap, once the log has reached the
/// host.
pub async fn reboot() -> ! {
    log::info!("reboot");
    Timer::after(LOG_DRAIN).await;
    cortex_m::peripheral::SCB::sys_reset()
}

#[cfg(not(feature = "console"))]
pub use phone::{note_power, Phone};

#[cfg(not(feature = "console"))]
use phone::power;

#[cfg(not(feature = "console"))]
mod phone;

#[cfg(not(feature = "console"))]
pub mod mesh;
