//! The board's external SPI NOR flash, a Zetta ZD25WQ16B
//! (`docs/external-flash-2026-11-06.md`), which nothing uses yet.
//!
//! Idle it stands by at 12 µA typical, a tenth of the radio's 0.11 to
//! 0.14 mA on a sleepy node (README, "The current on battery"); in deep
//! power-down it takes 0.5 µA typical, 1.5 µA max (ZD25WQ16B datasheet,
//! Table 4-2, ISB and IDPD). So it is put there once, at boot.
//!
//! Whoever uses the chip later must first wake it: `ABh`, then 8 µs
//! (tRES1, Table 4-3) before the next command. Every command but `ABh` is
//! ignored until then (datasheet, "Deep Power-down (DP)").

use embassy_nrf::gpio::{Level, Output, OutputDrive};

use crate::board;

/// Deep Power-down (DP), datasheet 5.1, "Commands listing".
const DEEP_POWER_DOWN: u8 = 0xB9;

/// Send Deep Power-down, bit by bit in SPI mode 0, and leave every input
/// of the chip driven.
///
/// Call only while the switched rail that feeds the chip is up (after
/// `radio::boot` succeeds): a pin driven high into an unpowered chip feeds
/// it through its protection diodes. The rail stays up from then on, so
/// one command per boot holds; a power cycle would wake the chip.
pub fn power_down(pins: board::Flash) {
    let mut cs = Output::new(pins.cs, Level::High, OutputDrive::Standard);
    let mut sck = Output::new(pins.sck, Level::Low, OutputDrive::Standard);
    let mut si = Output::new(pins.io0, Level::Low, OutputDrive::Standard);
    // The board has no pull-ups on WP# and HOLD#; a low HOLD# pauses the
    // command.
    let wp = Output::new(pins.io2, Level::High, OutputDrive::Standard);
    let hold = Output::new(pins.io3, Level::High, OutputDrive::Standard);
    cs.set_low();
    for bit in (0..8).rev() {
        si.set_level(Level::from(DEEP_POWER_DOWN >> bit & 1 != 0));
        sck.set_high();
        sck.set_low();
    }
    // The chip is down 3 µs (tDP) after CS# rises; nothing here waits on it.
    cs.set_high();
    // Dropping an Output disconnects its pin. The datasheet's 0.5 µA holds
    // with CS# at VCC and every other input at a rail; a floating CS# and
    // clock could also clock in a wake-up.
    core::mem::forget((cs, sck, si, wp, hold));
}
