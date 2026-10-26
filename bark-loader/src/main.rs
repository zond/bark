//! The T-Echo's second-stage loader, where the board's bootloader jumps.
//!
//! At each boot it swaps in an image the firmware staged, once that image
//! matches the length and hash the firmware recorded; goes on with a swap
//! a power cut stopped; or swaps the previous image back when the new one
//! did not confirm itself (`bark_image::boot`). While a new image is on
//! trial the watchdog runs, so that one that hangs is reset and swapped
//! back too. Then it write-protects itself and everything that boots the
//! board, and jumps to the firmware. It holds no key and checks no
//! signature: the running firmware checks those before it stages.

#![no_std]
#![no_main]

use core::cell::RefCell;

use bark_image::boot::{prepare, swap_requested, Shared};
use bark_image::layout::{Region, ACTIVE, PAGE, PROTECT_HIGH, PROTECT_LOW};
use bark_image::TRIAL_SECONDS;
use cortex_m::peripheral::SCB;
use cortex_m_rt::entry;
use embassy_boot_nrf::WatchdogFlash;
use embassy_nrf::nvmc::Nvmc;
use embassy_nrf::pac::acl::vals::Write;
use embassy_nrf::wdt;
use embassy_sync::blocking_mutex::Mutex;

/// The watchdog's clock, the 32.768 kHz low-frequency clock (nRF52840
/// Product Specification, WDT).
const WDT_HZ: u32 = 32_768;

#[entry]
fn main() -> ! {
    // SAFETY: nothing else runs: the board's bootloader has handed over,
    // and the firmware starts only when this function jumps to it.
    let p = unsafe { embassy_nrf::Peripherals::steal() };
    let flash: Shared<Nvmc> = Mutex::new(RefCell::new(Nvmc::new(p.NVMC)));
    let mut page = [0u8; PAGE as usize];
    let done = match swap_requested(&flash) {
        Ok(true) => {
            // The timeout is (CRV + 1) / 32 768 s (Product Specification,
            // WDT). Counting on while the CPU sleeps: a sleepy node sleeps
            // nearly all the time, and its trial must end all the same.
            let mut trial = wdt::Config::default();
            trial.timeout_ticks = TRIAL_SECONDS * WDT_HZ - 1;
            trial.action_during_sleep = wdt::SleepConfig::Run;
            trial.action_during_debug_halt = wdt::HaltConfig::Pause;
            let nvmc = flash.into_inner().into_inner();
            let flash = Mutex::new(RefCell::new(WatchdogFlash::start(nvmc, p.WDT, trial)));
            prepare(&flash, &mut page).map(|_| ())
        }
        Ok(false) => prepare(&flash, &mut page).map(|_| ()),
        Err(e) => Err(e),
    };
    // A flash that fails half way through a swap leaves no image to run;
    // the swap goes on from where it stopped at the next boot.
    if done.is_err() {
        SCB::sys_reset();
    }
    protect(0, PROTECT_LOW);
    protect(1, PROTECT_HIGH);
    // SAFETY: ACTIVE holds an image linked to run from its start, whose
    // first two words are its stack pointer and reset vector.
    unsafe {
        let core = cortex_m::Peripherals::steal();
        core.SCB.vtor.write(ACTIVE.start);
        cortex_m::asm::bootload(ACTIVE.start as *const u32)
    }
}

/// Block writes and erases of `region` until the next reset, through ACL
/// region `n` (Product Specification, ACL: each region's registers take
/// one write, and only a reset clears them).
fn protect(n: usize, region: Region) {
    let acl = embassy_nrf::pac::ACL.acl(n);
    acl.addr().write_value(region.start);
    acl.size().write_value(region.len);
    acl.perm().write(|w| w.set_write(Write::Disable));
}

/// A panic is a bug here; reset, so that a fault that passes, such as a
/// flash read during a brown-out, does not leave the board stopped. The
/// board's bootloader still takes a double tap.
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    SCB::sys_reset()
}
