//! The board's external SPI NOR flash, a Zetta ZD25WQ16B
//! (`docs/external-flash-2026-11-06.md`), as the loader and the firmware
//! both drive it: SPI mode 0 on four pins, one data line each way
//! ([`BitBang`]), and the few commands a staged image and the mailbox's
//! ring need ([`Chip`]).
//!
//! The chip sleeps in deep power-down between uses, 0.5 µA against 12 µA
//! in standby (datasheet Table 4-2, IDPD and ISB): every use wakes it and
//! puts it back, but a program or an erase keeps it awake until it ends,
//! since the chip ignores deep power-down while it writes.
//!
//! The pins are driven by software, not by an SPIM: so every input of the
//! chip stays at a rail between uses, which deep power-down's current
//! needs, the same code runs in the loader and the firmware with no
//! peripheral or interrupt to share, and the loop's few megahertz are
//! close to the 8 MHz of SPIM0 to SPIM2 (nRF52840 Product Specification,
//! SPIM); SPIM3's 32 MHz comes with an erratum on its transmit buffer.

use core::convert::Infallible;

use embedded_hal::delay::DelayNs;
use embedded_hal::digital::{InputPin, OutputPin};
use embedded_storage::nor_flash::{
    ErrorType, NorFlash, NorFlashError, NorFlashErrorKind, ReadNorFlash,
};

/// Bytes on the chip, 16 Mbit (datasheet, "Features").
pub const CAPACITY: u32 = 2 * 1024 * 1024;

/// The erase unit, a sector (datasheet, "Sector Erase (SE)").
pub const SECTOR: u32 = 4096;

/// A Page Program writes within one page of this many bytes, wrapping
/// at its end (datasheet, "Page Program (PP)").
pub const PROGRAM_PAGE: u32 = 256;

/// What Read Identification answers: manufacturer, memory type and
/// density (datasheet, "Table ID Definitions").
pub const JEDEC_ID: [u8; 3] = [0xBA, 0x60, 0x15];

// Commands (datasheet, "Commands listing").
const WRITE_ENABLE: u8 = 0x06;
const READ_STATUS: u8 = 0x05;
const READ: u8 = 0x03;
const PAGE_PROGRAM: u8 = 0x02;
const SECTOR_ERASE: u8 = 0x20;
const READ_ID: u8 = 0x9F;
const DEEP_POWER_DOWN: u8 = 0xB9;
const RELEASE: u8 = 0xAB;

/// The status register's write-in-progress bit (datasheet, "Status
/// Register").
const WIP: u8 = 0x01;

/// The switched rail that feeds the chip, from its enable to the chip's
/// supply: no source gives the ramp; 10 ms is far beyond a load switch's
/// typical sub-millisecond one and costs nothing once per boot
/// (`docs/external-flash-2026-11-06.md`, "The supply"). The radio waits
/// it too.
pub const RAIL_SETTLE_US: u32 = 10_000;

/// From the supply's minimum to the first command, tVSL (datasheet 4.5,
/// "Operation Conditions").
const VSL_US: u32 = 70;
/// From Release's CS# rising to the next command, tRES1 (Table 4-3).
const RES1_US: u32 = 8;
/// From Deep Power-down's CS# rising to the chip down, tDP (Table 4-3).
const DP_US: u32 = 3;

/// A Page Program's time, typical and maximum, tPP (Table 4-3).
pub const PROGRAM_TYP_US: u32 = 1_300;
/// See [`PROGRAM_TYP_US`].
pub const PROGRAM_MAX_US: u32 = 3_000;
/// A Sector Erase's time, typical and maximum, tSE (Table 4-3).
pub const ERASE_TYP_US: u32 = 10_000;
/// See [`ERASE_TYP_US`].
pub const ERASE_MAX_US: u32 = 12_000;

/// How often a write in progress is polled: a twenty-sixth of a page
/// program, so the wait overshoots its end by at most 4 %.
pub const POLL_US: u32 = PROGRAM_TYP_US / 26;

/// One SPI device: its select line, a byte exchanged, and a clock to
/// wait by.
pub trait Bus {
    /// CS# low: a command starts.
    fn select(&mut self);
    /// CS# high: the command ends, and the chip acts on it.
    fn deselect(&mut self);
    /// Send `out`, most significant bit first, and return the byte the
    /// chip sent in the same eight clocks.
    fn transfer(&mut self, out: u8) -> u8;
    /// Wait at least `us` microseconds.
    fn delay_us(&mut self, us: u32);
}

/// SPI mode 0 on four pins (datasheet, "SPI Modes"): the chip takes DI
/// on the clock's rising edge and moves DO on its falling one, so DI is
/// set and DO read while the clock is low and high.
pub struct BitBang<Cs, Clk, Di, Do, D> {
    /// CS#, idle high.
    pub cs: Cs,
    /// CLK, idle low.
    pub clk: Clk,
    /// The chip's DI.
    pub di: Di,
    /// The chip's DO.
    pub dout: Do,
    /// The clock to wait by.
    pub delay: D,
}

impl<Cs, Clk, Di, Do, D> Bus for BitBang<Cs, Clk, Di, Do, D>
where
    Cs: OutputPin<Error = Infallible>,
    Clk: OutputPin<Error = Infallible>,
    Di: OutputPin<Error = Infallible>,
    Do: InputPin<Error = Infallible>,
    D: DelayNs,
{
    fn select(&mut self) {
        let Ok(()) = self.cs.set_low();
    }

    fn deselect(&mut self) {
        let Ok(()) = self.cs.set_high();
    }

    #[inline(always)]
    fn transfer(&mut self, out: u8) -> u8 {
        let mut back = 0;
        for bit in (0..8).rev() {
            let Ok(()) = match out >> bit & 1 {
                1 => self.di.set_high(),
                _ => self.di.set_low(),
            };
            let Ok(()) = self.clk.set_high();
            let Ok(high) = self.dout.is_high();
            back = back << 1 | u8::from(high);
            let Ok(()) = self.clk.set_low();
        }
        back
    }

    fn delay_us(&mut self, us: u32) {
        self.delay.delay_us(us);
    }
}

/// Why the chip failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Read Identification answered this, not [`JEDEC_ID`]: no chip, an
    /// unpowered one, or another.
    Absent([u8; 3]),
    /// A program or an erase still in progress past its maximum time.
    Timeout,
    /// Past the chip's end, or a program across a page's end.
    OutOfBounds,
    /// An erase not on whole sectors.
    NotAligned,
}

impl NorFlashError for Error {
    fn kind(&self) -> NorFlashErrorKind {
        match self {
            Error::OutOfBounds => NorFlashErrorKind::OutOfBounds,
            Error::NotAligned => NorFlashErrorKind::NotAligned,
            _ => NorFlashErrorKind::Other,
        }
    }
}

/// The chip on its bus: asleep between uses, or awake while a program
/// or an erase runs.
pub struct Chip<B: Bus> {
    bus: B,
    /// The maximum time of the program or erase in progress.
    busy_max_us: Option<u32>,
}

impl<B: Bus> Chip<B> {
    /// The chip on `bus`, its CS# already high. Call [`Chip::init`] before
    /// anything else.
    pub fn new(bus: B) -> Self {
        Chip {
            bus,
            busy_max_us: None,
        }
    }

    /// Check that the chip is there and put it in deep power-down; its
    /// status register, whose block-protect bits, if any is set, make it
    /// ignore programs and erases. Call once its supply has been up for
    /// [`RAIL_SETTLE_US`]. A chip powered up comes up in standby, one woken
    /// by a reset of the processor alone may still be in deep power-down:
    /// Release wakes either.
    pub fn init(&mut self) -> Result<u8, Error> {
        self.bus.delay_us(VSL_US);
        self.wake();
        let mut id = [0u8; 3];
        self.command(&[READ_ID], &[], &mut id);
        let status = self.status();
        self.sleep();
        match id == JEDEC_ID {
            true => Ok(status),
            false => Err(Error::Absent(id)),
        }
    }

    /// Start programming `data` at `address`, within one page of
    /// [`PROGRAM_PAGE`] bytes, erased since it was last programmed. The
    /// chip stays awake until [`Chip::done`] or [`Chip::finish`] sees the
    /// program end.
    pub fn start_program(&mut self, address: u32, data: &[u8]) -> Result<(), Error> {
        let len = data.len() as u32;
        if address % PROGRAM_PAGE + len > PROGRAM_PAGE || address + len > CAPACITY {
            return Err(Error::OutOfBounds);
        }
        if data.is_empty() {
            return Ok(());
        }
        self.ready()?;
        self.command(&[WRITE_ENABLE], &[], &mut []);
        self.command(&addressed(PAGE_PROGRAM, address), data, &mut []);
        self.busy_max_us = Some(PROGRAM_MAX_US);
        Ok(())
    }

    /// Start erasing the sector at `address`; as [`Chip::start_program`].
    pub fn start_erase(&mut self, address: u32) -> Result<(), Error> {
        if !address.is_multiple_of(SECTOR) {
            return Err(Error::NotAligned);
        }
        if address >= CAPACITY {
            return Err(Error::OutOfBounds);
        }
        self.ready()?;
        self.command(&[WRITE_ENABLE], &[], &mut []);
        self.command(&addressed(SECTOR_ERASE, address), &[], &mut []);
        self.busy_max_us = Some(ERASE_MAX_US);
        Ok(())
    }

    /// Whether the last program or erase has ended, without waiting; once
    /// it has, the chip sleeps.
    pub fn done(&mut self) -> bool {
        if self.busy_max_us.is_none() {
            return true;
        }
        if self.status() & WIP != 0 {
            return false;
        }
        self.busy_max_us = None;
        self.sleep();
        true
    }

    /// Wait `us` microseconds by the bus's clock, a program or erase
    /// running on meanwhile.
    pub fn pause_us(&mut self, us: u32) {
        self.bus.delay_us(us);
    }

    /// Wait for the last program or erase to end, then put the chip to
    /// sleep.
    pub fn finish(&mut self) -> Result<(), Error> {
        if self.busy_max_us.is_none() {
            return Ok(());
        }
        let ended = self.wait();
        self.sleep();
        ended
    }

    /// The chip awake for a command: woken, or, if a program or an erase
    /// keeps it awake, once that has ended.
    fn ready(&mut self) -> Result<(), Error> {
        match self.busy_max_us {
            None => {
                self.wake();
                Ok(())
            }
            Some(_) => self.wait(),
        }
    }

    /// Poll the program or erase in progress until it ends, or its
    /// maximum time has passed. The polls' own time is not counted, so
    /// the wait is never shorter than that maximum.
    fn wait(&mut self) -> Result<(), Error> {
        let max = self.busy_max_us.take().unwrap_or(0);
        let mut waited = 0;
        while self.status() & WIP != 0 {
            if waited >= max {
                return Err(Error::Timeout);
            }
            self.bus.delay_us(POLL_US);
            waited += POLL_US;
        }
        Ok(())
    }

    fn status(&mut self) -> u8 {
        let mut s = [0u8];
        self.command(&[READ_STATUS], &[], &mut s);
        s[0]
    }

    fn wake(&mut self) {
        self.command(&[RELEASE], &[], &mut []);
        self.bus.delay_us(RES1_US);
    }

    fn sleep(&mut self) {
        self.command(&[DEEP_POWER_DOWN], &[], &mut []);
        self.bus.delay_us(DP_US);
    }

    /// One command: `head`, then `out` sent, then `back` read.
    fn command(&mut self, head: &[u8], out: &[u8], back: &mut [u8]) {
        self.bus.select();
        for &b in head.iter().chain(out) {
            self.bus.transfer(b);
        }
        for b in back {
            *b = self.bus.transfer(0);
        }
        self.bus.deselect();
    }
}

/// A command with a 24-bit address, most significant byte first.
fn addressed(command: u8, address: u32) -> [u8; 4] {
    let [_, a2, a1, a0] = address.to_be_bytes();
    [command, a2, a1, a0]
}

impl<B: Bus> ErrorType for Chip<B> {
    type Error = Error;
}

impl<B: Bus> ReadNorFlash for Chip<B> {
    const READ_SIZE: usize = 1;

    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Error> {
        if offset as usize + bytes.len() > CAPACITY as usize {
            return Err(Error::OutOfBounds);
        }
        self.ready()?;
        self.command(&addressed(READ, offset), &[], bytes);
        self.sleep();
        Ok(())
    }

    fn capacity(&self) -> usize {
        CAPACITY as usize
    }
}

impl<B: Bus> NorFlash for Chip<B> {
    // A Page Program sets any bytes of a page.
    const WRITE_SIZE: usize = 1;
    const ERASE_SIZE: usize = SECTOR as usize;

    fn erase(&mut self, from: u32, to: u32) -> Result<(), Error> {
        if !from.is_multiple_of(SECTOR) || !to.is_multiple_of(SECTOR) {
            return Err(Error::NotAligned);
        }
        if from > to || to > CAPACITY {
            return Err(Error::OutOfBounds);
        }
        for sector in (from..to).step_by(SECTOR as usize) {
            self.start_erase(sector)?;
        }
        self.finish()
    }

    fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Error> {
        if offset as usize + bytes.len() > CAPACITY as usize {
            return Err(Error::OutOfBounds);
        }
        let mut at = offset;
        let mut rest = bytes;
        while !rest.is_empty() {
            let n = rest.len().min((PROGRAM_PAGE - at % PROGRAM_PAGE) as usize);
            self.start_program(at, &rest[..n])?;
            at += n as u32;
            rest = &rest[n..];
        }
        self.finish()
    }
}

#[cfg(test)]
pub(crate) mod fake;

#[cfg(test)]
mod tests;
