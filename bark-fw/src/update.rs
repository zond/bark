//! The running image's side of an update (`bark_image::boot`,
//! `docs/mesh-update-2026-10-25.md`): it confirms itself once the node
//! has started, so that the loader keeps it, and it feeds the watchdog
//! the loader started for its trial, which runs on until the next reset
//! (nRF52840 Product Specification, WDT: once started it cannot be
//! stopped). The console build can also stage an image
//! (`console.rs`, `stage`).

use core::cell::RefCell;

use bark_image::boot::{confirm, Shared, Started};
use embassy_nrf::nvmc::Nvmc;
use embassy_nrf::{peripherals, wdt, Peri};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Timer};

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

/// What a bench image does instead of confirming itself, from
/// `BARK_UPDATE_BENCH` at build time (roadmap Phase 2, the loader's rows).
enum Bench {
    /// Runs on and never confirms: the watchdog ends its trial.
    Unconfirmed,
    /// Panics: the soft reset ends its trial.
    Panic,
    /// Spins with the executor blocked: the watchdog ends its trial.
    Hang,
}

const BENCH: Option<Bench> = match option_env!("BARK_UPDATE_BENCH") {
    None => None,
    Some(v) if same(v, "unconfirmed") => Some(Bench::Unconfirmed),
    Some(v) if same(v, "panic") => Some(Bench::Panic),
    Some(v) if same(v, "hang") => Some(Bench::Hang),
    Some(_) => panic!("BARK_UPDATE_BENCH is unconfirmed, panic or hang"),
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

const fn decimal(s: &str) -> u32 {
    let b = s.as_bytes();
    assert!(!b.is_empty(), "BARK_IMAGE_VERSION is empty");
    let mut v: u32 = 0;
    let mut i = 0;
    while i < b.len() {
        assert!(
            b[i].is_ascii_digit(),
            "BARK_IMAGE_VERSION is a decimal number"
        );
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

/// Set once the image has confirmed itself, for [`watchdog`].
static CONFIRMED: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// The node has started, its core set up and its radio up: keep this
/// image. Until this runs, a reset swaps the previous image back.
pub fn started(flash: &Flash) {
    match BENCH {
        None => {}
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
    match confirm(flash) {
        Ok(Started::Normal) => {}
        Ok(Started::Confirmed) => log::info!("update: version {} confirmed", BARK_IMAGE_VERSION),
        Ok(Started::Reverted) => log::warn!(
            "update: the new image did not confirm itself; version {} is back",
            BARK_IMAGE_VERSION
        ),
        Ok(Started::Refused) => log::warn!("update: the loader refused the staged image"),
        Err(e) => log::error!("update: confirming failed: {:?}", e),
    }
    CONFIRMED.signal(());
}

/// Feed the watchdog, if the loader started one, from the confirmation on:
/// twice a period, so that one late wakeup still passes.
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
