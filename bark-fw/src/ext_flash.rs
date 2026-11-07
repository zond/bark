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
/// power-down and keep it for [`with`]; log what it answered.
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

/// Use the chip, if [`init`] found it.
pub fn with<R>(f: impl FnOnce(&mut Chip) -> R) -> Option<R> {
    CHIP.lock(|c| c.borrow_mut().as_mut().map(f))
}

/// What a use of a chip [`init`] did not find fails with.
const NO_CHIP: Error = Error::Absent([0; 3]);

#[cfg(not(feature = "console"))]
pub use mailbox::{erase, program};

#[cfg(not(feature = "console"))]
pub use dfu::Dfu;

/// What the mailbox's ring writes with.
#[cfg(not(feature = "console"))]
mod mailbox {
    use bark_image::ext_flash::{
        Error, ERASE_MAX_US, ERASE_TYP_US, POLL_US, PROGRAM_MAX_US, PROGRAM_TYP_US,
    };
    use embassy_time::Timer;

    use super::{with, NO_CHIP};

    /// Erase the sector at `address`, the executor free while the chip
    /// erases.
    pub async fn erase(address: u32) -> Result<(), Error> {
        let started = with(|c| c.start_erase(address));
        wait(started, ERASE_TYP_US, ERASE_MAX_US).await
    }

    /// Program `data` at `address`, within one page of the chip, the
    /// executor free while the chip programs.
    pub async fn program(address: u32, data: &[u8]) -> Result<(), Error> {
        let started = with(|c| c.start_program(address, data));
        wait(started, PROGRAM_TYP_US, PROGRAM_MAX_US).await
    }

    /// Wait out a program or an erase that `started`: its typical time, then
    /// a poll every [`POLL_US`] up to its maximum, then the driver's own
    /// bounded wait. A read meanwhile waits for it in the driver.
    async fn wait(
        started: Option<Result<(), Error>>,
        typ_us: u32,
        max_us: u32,
    ) -> Result<(), Error> {
        started.unwrap_or(Err(NO_CHIP))?;
        Timer::after_micros(typ_us.into()).await;
        let mut waited = typ_us;
        while waited < max_us && !with(|c| c.done()).unwrap_or(true) {
            Timer::after_micros(POLL_US.into()).await;
            waited += POLL_US;
        }
        with(|c| c.finish()).unwrap_or(Ok(()))
    }
}

/// What the phone link's staging writes with.
#[cfg(not(feature = "console"))]
mod dfu {
    use bark_image::ext_flash::{Error, PROGRAM_PAGE, SECTOR};
    use bark_image::layout::external::DFU;
    use embedded_storage_async::nor_flash::{ErrorType, NorFlash, ReadNorFlash};

    use super::{erase, program, with, NO_CHIP};

    /// The staged slot, addressed from its start, as the update's staging
    /// writes it (`bark_image::boot::Stager`): its erases and programs as
    /// the mailbox's, the executor free while the chip works; and each
    /// read, as the staged image is hashed, a chunk at a time with the
    /// executor let run between, so that the hash of a whole slot, some
    /// seconds on this bus, holds nothing up for longer than a chunk.
    pub struct Dfu;

    fn within(offset: u32, len: usize) -> Result<u32, Error> {
        match offset.checked_add(len as u32) {
            Some(end) if end <= DFU.len => Ok(DFU.start + offset),
            _ => Err(Error::OutOfBounds),
        }
    }

    impl ErrorType for Dfu {
        type Error = Error;
    }

    impl ReadNorFlash for Dfu {
        const READ_SIZE: usize = 1;

        async fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Error> {
            let at = within(offset, bytes.len())?;
            let read = with(|c| embedded_storage::nor_flash::ReadNorFlash::read(c, at, bytes))
                .unwrap_or(Err(NO_CHIP));
            embassy_futures::yield_now().await;
            read
        }

        fn capacity(&self) -> usize {
            DFU.len as usize
        }
    }

    impl NorFlash for Dfu {
        const WRITE_SIZE: usize = 1;
        const ERASE_SIZE: usize = SECTOR as usize;

        async fn erase(&mut self, from: u32, to: u32) -> Result<(), Error> {
            if !from.is_multiple_of(SECTOR) || !to.is_multiple_of(SECTOR) || from > to {
                return Err(Error::NotAligned);
            }
            let start = within(from, (to - from) as usize)?;
            for sector in (start..start + (to - from)).step_by(SECTOR as usize) {
                erase(sector).await?;
            }
            Ok(())
        }

        async fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Error> {
            let mut at = within(offset, bytes.len())?;
            let mut rest = bytes;
            while !rest.is_empty() {
                let n = rest.len().min((PROGRAM_PAGE - at % PROGRAM_PAGE) as usize);
                program(at, &rest[..n]).await?;
                at += n as u32;
                rest = &rest[n..];
            }
            Ok(())
        }
    }
}

#[cfg(feature = "console")]
pub use staging::Kept;

/// What the console's staging writes with.
#[cfg(feature = "console")]
mod staging {
    use bark_image::ext_flash::{Error, CAPACITY, SECTOR};
    use embedded_storage::nor_flash::{ErrorType, NorFlash, ReadNorFlash};

    use super::{with, NO_CHIP};

    /// The chip kept here, as a flash the update's staging writes
    /// (`bark_image::boot::Stager`), blocking.
    pub struct Kept;

    impl ErrorType for Kept {
        type Error = Error;
    }

    impl ReadNorFlash for Kept {
        const READ_SIZE: usize = 1;

        fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Error> {
            with(|c| c.read(offset, bytes)).unwrap_or(Err(NO_CHIP))
        }

        fn capacity(&self) -> usize {
            CAPACITY as usize
        }
    }

    impl NorFlash for Kept {
        const WRITE_SIZE: usize = 1;
        const ERASE_SIZE: usize = SECTOR as usize;

        fn erase(&mut self, from: u32, to: u32) -> Result<(), Error> {
            with(|c| c.erase(from, to)).unwrap_or(Err(NO_CHIP))
        }

        fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Error> {
            with(|c| c.write(offset, bytes)).unwrap_or(Err(NO_CHIP))
        }
    }
}
