//! The board's external SPI NOR flash, a Zetta ZD25WQ16B
//! (`docs/external-flash-2026-11-06.md`), through its driver
//! (`bark_image::ext_flash`), on its pins.
//!
//! Idle it stands by at 12 µA typical, a tenth of the radio's 0.11 to
//! 0.14 mA on a sleepy node (README, "The current on battery"); in deep
//! power-down it takes 0.5 µA typical, 1.5 µA max (ZD25WQ16B datasheet,
//! Table 4-2, ISB and IDPD). So [`init`] puts it there once the rail is
//! up, and the driver wakes it for each use and puts it back.
//!
//! One chip, kept here: its users all run in thread mode, so a read
//! that waits out an erase, up to 12 ms, holds up the executor but masks
//! no interrupt.

use core::cell::RefCell;

use bark_image::ext_flash::{BitBang, Chip as Driver, Error};
use embassy_nrf::gpio::{Input, Level, Output, OutputDrive, Pull};
use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embedded_hal::delay::DelayNs;

use crate::board;
use crate::radio::CPU_HZ;

/// A wait counted in the CPU's cycles: embassy-time's ticks of 30.5 µs
/// are too coarse for the chip's 8 µs.
pub struct Cycles;

impl DelayNs for Cycles {
    fn delay_ns(&mut self, ns: u32) {
        cortex_m::asm::delay((u64::from(ns) * CPU_HZ).div_ceil(1_000_000_000) as u32);
    }
}

/// The chip on its pins.
pub type Chip =
    Driver<BitBang<Output<'static>, Output<'static>, Output<'static>, Input<'static>, Cycles>>;

static CHIP: Mutex<ThreadModeRawMutex, RefCell<Option<Chip>>> = Mutex::new(RefCell::new(None));

/// Take the chip's pins, check the chip is there, put it in deep
/// power-down and keep it; log what it answered.
///
/// Call only while the switched rail that feeds the chip is up (after
/// `radio::boot` succeeds): a pin driven high into an unpowered chip
/// feeds it through its protection diodes. The rail stays up from then
/// on.
pub fn init(pins: board::Flash) {
    let bus = BitBang {
        cs: Output::new(pins.cs, Level::High, OutputDrive::Standard),
        clk: Output::new(pins.sck, Level::Low, OutputDrive::Standard),
        di: Output::new(pins.io0, Level::Low, OutputDrive::Standard),
        // DO floats while the chip sleeps; pulled down, the input stays at
        // a rail, and the pull costs current only while the chip drives a
        // one.
        dout: Input::new(pins.io1, Pull::Down),
        delay: Cycles,
    };
    // The board has no pull-ups on WP# and HOLD#: a low WP# locks the
    // status register, a low HOLD# pauses a command. Held high for good;
    // dropping an Output would disconnect its pin.
    core::mem::forget((
        Output::new(pins.io2, Level::High, OutputDrive::Standard),
        Output::new(pins.io3, Level::High, OutputDrive::Standard),
    ));
    let mut chip = Driver::new(bus);
    match chip.init() {
        Ok(status) => {
            log::info!("ext flash: id ba 60 15, status {:#04x}, asleep", status);
            CHIP.lock(|c| *c.borrow_mut() = Some(chip));
        }
        Err(e) => {
            match e {
                Error::Absent(id) => log::error!("ext flash: no chip, id {:02x?}", id),
                e => log::error!("ext flash: {:?}", e),
            }
            // Not used, but its pins held where its current needs them,
            // should it be there after all.
            core::mem::forget(chip);
        }
    }
}
