//! The T-Echo's second-stage loader, where the board's bootloader jumps.
//!
//! At each boot it swaps in an image the firmware staged on the external
//! flash, once that image matches the length and hash the firmware
//! recorded; goes on with a swap a power cut stopped; or swaps the
//! previous image back when the new one did not confirm itself
//! (`bark_image::boot`). While a new image is on trial the watchdog runs,
//! so that one that hangs is reset and swapped back too; it holds the
//! 32.768 kHz clock on its source until a reset, so the loader first
//! starts the clock as the firmware would (`bark_image::lfclk`). Then it
//! write-protects itself and everything that boots the board, and jumps
//! to the firmware. It holds no key and checks no signature: the running
//! firmware checks those before it stages.
//!
//! Only on the boots with a swap to do does it power the external flash:
//! it raises the switched rail, finds the chip, and lets the rail fall
//! again before the jump, so that the firmware starts from the board's
//! state at reset.
//!
//! Built with `BARK_LOADER_BENCH=reset`, it resets the chip once in each
//! swap and once in each swap back, at a moment drawn from the RNG at the
//! boot that starts it (`bark_image::boot::cut`, roadmap D143).

#![no_std]
#![no_main]

use core::cell::RefCell;

use bark_image::boot::cut::{
    prepare_cut, written_by, Cutting, ExternalPartial, Partial, Plan, Side,
};
use bark_image::boot::{prepare, swap_requested, Boot, FlashError, Shared};
use bark_image::ext_flash::{BitBang, Chip, RAIL_SETTLE_US};
use bark_image::layout::{Region, ACTIVE, PAGE, PROTECT_HIGH, PROTECT_LOW};
use bark_image::{lfclk, TRIAL_SECONDS};
use cortex_m::peripheral::SCB;
use cortex_m_rt::entry;
use embassy_boot_nrf::WatchdogFlash;
use embassy_nrf::gpio::{Input, Level, Output, OutputDrive, Pull};
use embassy_nrf::nvmc::Nvmc;
use embassy_nrf::pac::acl::vals::Write;
use embassy_nrf::pac::nvmc::vals::Wen;
use embassy_nrf::peripherals::{P0_05, P0_07, P0_12, P0_13, P1_12, P1_13, P1_14, P1_15};
use embassy_nrf::{wdt, Peri};
use embassy_sync::blocking_mutex::Mutex;
use embedded_hal::delay::DelayNs;
use embedded_storage::nor_flash::NorFlash;

/// The watchdog's clock, the 32.768 kHz low-frequency clock (nRF52840
/// Product Specification, WDT).
const WDT_HZ: u32 = 32_768;

/// The CPU's clock (nRF52840 Product Specification, CLOCK).
const CPU_HZ: u64 = 64_000_000;

/// Whether this loader stops each swap and swap back once, from
/// `BARK_LOADER_BENCH=reset` at build time. Only an open build takes it
/// (`BARK_OPEN=1`, bark-fw/README.md, "Read-out protection"), so that a
/// bench loader never goes out with a board that leaves the bench.
const CUT: bool = match (option_env!("BARK_LOADER_BENCH"), option_env!("BARK_OPEN")) {
    (None, _) => false,
    (Some(b), Some(o)) if same(b, "reset") && same(o, "1") => true,
    (Some(b), _) if same(b, "reset") => panic!("BARK_LOADER_BENCH=reset needs BARK_OPEN=1"),
    _ => panic!("BARK_LOADER_BENCH is reset or unset"),
};

/// [`CUT`] as `bark-fw/scripts/uf2.py` reads it from the ELF: it packs a
/// bench loader only with an open firmware.
#[used]
#[no_mangle]
static BARK_LOADER_BENCH: u32 = CUT as u32;

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

#[entry]
fn main() -> ! {
    // SAFETY: nothing else runs: the board's bootloader has handed over,
    // and the firmware starts only when this function jumps to it.
    let p = unsafe { embassy_nrf::Peripherals::steal() };
    let flash: Shared<Nvmc> = Mutex::new(RefCell::new(Nvmc::new(p.NVMC)));
    let mut page = [0u8; PAGE as usize];
    let done = match swap_requested(&flash) {
        Ok(true) => {
            // The watchdog's clock: once the watchdog runs, the image can
            // no longer change its source.
            lfclk::probe(&mut Cycles);
            // The timeout is (CRV + 1) / 32 768 s (Product Specification,
            // WDT). Counting on while the CPU sleeps: a sleepy node sleeps
            // nearly all the time, and its trial must end all the same.
            let mut trial = wdt::Config::default();
            trial.timeout_ticks = TRIAL_SECONDS * WDT_HZ - 1;
            trial.action_during_sleep = wdt::SleepConfig::Run;
            trial.action_during_debug_halt = wdt::HaltConfig::Pause;
            let nvmc = flash.into_inner().into_inner();
            let watched = WatchdogFlash::start(nvmc, p.WDT, trial);
            let pins = External {
                pwr_en: p.P0_13,
                pwr_on: p.P0_12,
                cs: p.P1_15,
                clk: p.P1_14,
                di: p.P1_12,
                dout: p.P1_13,
                wp: p.P0_07,
                hold: p.P0_05,
            };
            if CUT {
                let plan = Plan::new(draw());
                let internal = Cutting::new(watched, InternalPartial, Side::Internal, &plan);
                swap(
                    &Mutex::new(RefCell::new(internal)),
                    pins,
                    &mut page,
                    |chip| Cutting::new(chip, ExternalPartial, Side::External, &plan),
                    |i, e, page| prepare_cut(i, e, page, &plan),
                )
            } else {
                swap(
                    &Mutex::new(RefCell::new(watched)),
                    pins,
                    &mut page,
                    |chip| chip,
                    prepare,
                )
            }
            .map(|_| ())
        }
        Ok(false) => Ok(()),
        Err(e) => Err(e),
    };
    // A flash that fails half way through a swap leaves no image to run;
    // the swap goes on from where it stopped at the next boot. A bench
    // loader's cut returns here too.
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

/// The external flash's pins and its rail's
/// (`docs/external-flash-2026-11-06.md`, "The pins" and "The supply").
struct External {
    /// `PWR_EN`: the rail is `PWR_EN AND (PWR_ON OR VBUS)`.
    pwr_en: Peri<'static, P0_13>,
    pwr_on: Peri<'static, P0_12>,
    cs: Peri<'static, P1_15>,
    clk: Peri<'static, P1_14>,
    di: Peri<'static, P1_12>,
    dout: Peri<'static, P1_13>,
    /// WP# and HOLD#, held high, as no pull-up holds them.
    wp: Peri<'static, P0_07>,
    hold: Peri<'static, P0_05>,
}

/// The external flash's driver on its pins.
type ExternalChip =
    Chip<BitBang<Output<'static>, Output<'static>, Output<'static>, Input<'static>, Cycles>>;

/// Prepare the boot with the external flash powered: the rail raised,
/// the chip found, and everything let go again on return, the rail with
/// it, whose enable has a pull-down. `wrap` takes the chip as the swap
/// sees it, `run` is the swap.
fn swap<I: NorFlash, X: NorFlash>(
    internal: &Shared<I>,
    pins: External,
    page: &mut [u8; PAGE as usize],
    wrap: impl FnOnce(ExternalChip) -> X,
    run: impl FnOnce(
        &Shared<I>,
        Option<&Shared<X>>,
        &mut [u8; PAGE as usize],
    ) -> Result<Boot, FlashError>,
) -> Result<Boot, FlashError> {
    let _rail = (
        Output::new(pins.pwr_en, Level::High, OutputDrive::Standard),
        Output::new(pins.pwr_on, Level::High, OutputDrive::Standard),
    );
    Cycles.delay_us(RAIL_SETTLE_US);
    let _held = (
        Output::new(pins.wp, Level::High, OutputDrive::Standard),
        Output::new(pins.hold, Level::High, OutputDrive::Standard),
    );
    let mut chip = Chip::new(BitBang {
        cs: Output::new(pins.cs, Level::High, OutputDrive::Standard),
        clk: Output::new(pins.clk, Level::Low, OutputDrive::Standard),
        di: Output::new(pins.di, Level::Low, OutputDrive::Standard),
        dout: Input::new(pins.dout, Pull::Down),
        delay: Cycles,
    });
    let found = chip.init().is_ok();
    let external = Mutex::new(RefCell::new(wrap(chip)));
    run(internal, found.then_some(&external), page)
}

/// 32 random bits from the RNG, its bias corrected so that each bit is 0
/// or 1 evenly (nRF52840 Product Specification, RNG): a fresh draw at
/// every boot, with nothing kept.
fn draw() -> u32 {
    let rng = embassy_nrf::pac::RNG;
    rng.config().write(|w| w.set_dercen(true));
    rng.tasks_start().write_value(1);
    let mut v = 0;
    for _ in 0..4 {
        while rng.events_valrdy().read() == 0 {}
        rng.events_valrdy().write_value(0);
        v = v << 8 | u32::from(rng.value().read().value());
    }
    rng.tasks_stop().write_value(1);
    v
}

/// The internal flash's part of an operation (`bark_image::boot::cut`):
/// an erase as a partial erase as long as the erase had run, a write as
/// the words written by then.
struct InternalPartial;

impl<F: NorFlash> Partial<F> for InternalPartial {
    fn erase(&mut self, _: &mut F, from: u32, us: u32) -> Result<(), F::Error> {
        // A partial erase lasts whole milliseconds (Product Specification,
        // NVMC, ERASEPAGEPARTIALCFG).
        let ms = us / 1000;
        if ms > 0 {
            let nvmc = embassy_nrf::pac::NVMC;
            nvmc.config().write(|w| w.set_wen(Wen::Een));
            nvmc.erasepagepartialcfg()
                .write(|w| w.set_duration(ms as u8));
            nvmc.erasepagepartial().write_value(from);
            while !nvmc.ready().read().ready() {}
            nvmc.config().write(|w| w.set_wen(Wen::Ren));
        }
        Ok(())
    }

    fn write(&mut self, flash: &mut F, offset: u32, bytes: &[u8], us: u32) -> Result<(), F::Error> {
        let (done, _) = written_by(Side::Internal, offset, bytes.len() as u32, us);
        flash.write(offset, &bytes[..done as usize])
    }
}

/// A wait counted in the CPU's cycles: nothing else runs.
struct Cycles;

impl DelayNs for Cycles {
    fn delay_ns(&mut self, ns: u32) {
        cortex_m::asm::delay((u64::from(ns) * CPU_HZ).div_ceil(1_000_000_000) as u32);
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
