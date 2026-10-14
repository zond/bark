//! LILYGO T-Echo pin map.
//!
//! Every pin the board wires to the nRF52840, grouped by function, as the
//! embassy peripheral that owns it. Groups this firmware does not use yet are
//! listed anyway so later work starts from here instead of re-researching the
//! board. Source: `docs/implementation-research-2026-10-14.md` §2. None of
//! it has been checked against a board yet.
//!
//! [`take_board!`] moves the pins out of [`Peripherals`] so each driver takes
//! exactly the pins it owns, by name, and the compiler rejects two drivers
//! claiming one pin. Peripheral *instances* (SPIM, UARTE, TWIM, SAADC, QSPI)
//! are a firmware design choice, not board wiring, and are taken from
//! `Peripherals` by whoever builds that driver; only USBD is fixed by
//! hardware and lives here.
//!
//! Nothing is configured here: a pin is in its reset state (input,
//! disconnected) until a driver takes it.

// Most groups are documentation until their driver exists.
#![allow(dead_code)]

use embassy_nrf::peripherals::{
    P0_02, P0_03, P0_04, P0_11, P0_12, P0_13, P0_14, P0_15, P0_16, P0_17, P0_18, P0_19, P0_20,
    P0_22, P0_23, P0_24, P0_25, P0_26, P0_27, P0_28, P0_29, P0_30, P0_31, P1_02, P1_04, P1_05,
    P1_08, P1_09, P1_10, P1_11, P1_12, P1_13, P1_14, P1_15, USBD,
};
use embassy_nrf::Peri;
#[cfg(doc)]
use embassy_nrf::Peripherals;

/// SX1262 LoRa transceiver on SPI.
///
/// DIO2 is wired inside the module to its RF switch (enable with
/// `SetDio2AsRfSwitchCtrl`); the board has no RXEN/TXEN pins. DIO3 supplies
/// the TCXO at 1.8 V (`SetDio3AsTcxoCtrl`). Neither reaches the nRF52840.
pub struct Radio {
    /// SPI clock, P0.19.
    pub sck: Peri<'static, P0_19>,
    /// SPI MISO, P0.23.
    pub miso: Peri<'static, P0_23>,
    /// SPI MOSI, P0.22.
    pub mosi: Peri<'static, P0_22>,
    /// SPI chip select (NSS), active low, P0.24.
    pub nss: Peri<'static, P0_24>,
    /// NRESET, active low, P0.25.
    pub reset: Peri<'static, P0_25>,
    /// BUSY, high while the chip cannot take a command, P0.17.
    pub busy: Peri<'static, P0_17>,
    /// DIO1, IRQ line (TxDone, RxDone, CadDone, timeouts), P0.20.
    pub dio1: Peri<'static, P0_20>,
}

/// Power switching.
///
/// The switched peripheral rail is up when `PWR_EN AND (PWR_ON OR VBUS)`.
/// This skeleton drives neither line: it does not touch the radio.
pub struct Power {
    /// `PWR_EN`, P0.13, active high: radio (and switched-rail) power enable.
    /// Left floating it leaves the rail half up; whoever brings up the radio
    /// must drive it explicitly. Some published pin lists call this a red LED;
    /// it is not.
    pub radio_en: Peri<'static, P0_13>,
    /// `PWR_ON`, P0.12, active high: peripheral rail enable when not on USB.
    pub rail_on: Peri<'static, P0_12>,
}

/// Quectel L76K GNSS on UART.
///
/// NMEA arrives on P1.09 whatever the net names suggest, so P1.09
/// is the nRF52840's RX and P1.08 its TX. Do not hold `reset` low while on
/// USB.
pub struct Gps {
    /// nRF52840 RXD (GNSS TX), P1.09.
    pub rx: Peri<'static, P1_09>,
    /// nRF52840 TXD (GNSS RX), P1.08.
    pub tx: Peri<'static, P1_08>,
    /// Standby control, P1.02.
    pub standby: Peri<'static, P1_02>,
    /// Reset, active low, P1.05.
    pub reset: Peri<'static, P1_05>,
    /// One pulse per second, P1.04.
    pub pps: Peri<'static, P1_04>,
}

/// 1.54" e-paper, SSD1681 controller (GDEH0154D67, 200 × 200), on SPI.
pub struct Eink {
    /// SPI clock, P0.31.
    pub sclk: Peri<'static, P0_31>,
    /// SPI MOSI (write-only panel), P0.29.
    pub mosi: Peri<'static, P0_29>,
    /// Chip select, active low, P0.30.
    pub cs: Peri<'static, P0_30>,
    /// Data/command select, P0.28.
    pub dc: Peri<'static, P0_28>,
    /// Reset, active low, P0.02.
    pub reset: Peri<'static, P0_02>,
    /// BUSY, P0.03.
    pub busy: Peri<'static, P0_03>,
    /// Enable (`EN`), P1.11. What it switches (panel supply or front light)
    /// and its polarity are unverified.
    pub en: Peri<'static, P1_11>,
}

/// Buttons.
pub struct Buttons {
    /// User button, P1.10.
    pub user: Peri<'static, P1_10>,
    /// Button labelled RESET, P0.18. P0.18 is also the nRF52840's pin-reset
    /// input when UICR.PSELRESET selects it; in that case it resets the chip
    /// and reads as nothing. Which state the shipped UICR is in is
    /// unverified; this firmware does not change it (see `Cargo.toml`).
    pub reset: Peri<'static, P0_18>,
    /// Capacitive touch pad, P0.11.
    pub touch: Peri<'static, P0_11>,
}

/// LEDs. Polarity unverified: this firmware assumes active low (cathode on
/// the pin), the usual wiring for an LED on an nRF52 GPIO.
pub struct Leds {
    /// Green LED, P0.15.
    pub green: Peri<'static, P0_15>,
    /// Blue LED, P0.14.
    pub blue: Peri<'static, P0_14>,
}

/// Battery voltage sense.
pub struct Battery {
    /// AIN2 on P0.04, behind a 2:1 divider (about 12 µA constant drain).
    /// With the SAADC at its default 3.6 V full scale and 12-bit resolution,
    /// battery mV = raw × 2 × 3600 / 4096 = raw × 7200 / 4096. Unreliable
    /// while on USB.
    pub adc: Peri<'static, P0_04>,
}

/// I2C bus: PCF8563 real-time clock.
pub struct I2c {
    /// SDA, P0.26.
    pub sda: Peri<'static, P0_26>,
    /// SCL, P0.27.
    pub scl: Peri<'static, P0_27>,
    /// PCF8563 interrupt output, open drain, active low, P0.16.
    pub rtc_int: Peri<'static, P0_16>,
}

/// External QSPI flash, MX25R1635F (16 Mbit). Only IO0 and IO1 are listed in
/// the research doc; IO2/IO3 are unconfirmed, so this is single/dual-SPI
/// until someone reads the schematic.
pub struct Flash {
    /// Clock, P1.14.
    pub sck: Peri<'static, P1_14>,
    /// Chip select, active low, P1.15.
    pub cs: Peri<'static, P1_15>,
    /// IO0 (MOSI in single SPI), P1.12.
    pub io0: Peri<'static, P1_12>,
    /// IO1 (MISO in single SPI), P1.13.
    pub io1: Peri<'static, P1_13>,
}

/// The whole board.
pub struct Board {
    /// SX1262 transceiver.
    pub radio: Radio,
    /// Rail switches.
    pub power: Power,
    /// GNSS receiver.
    pub gps: Gps,
    /// E-paper display.
    pub eink: Eink,
    /// Buttons.
    pub buttons: Buttons,
    /// LEDs.
    pub leds: Leds,
    /// Battery sense.
    pub battery: Battery,
    /// I2C bus.
    pub i2c: I2c,
    /// External flash.
    pub flash: Flash,
    /// Native USB (VBUS, D+ and D− are dedicated pins on the nRF52840).
    pub usbd: Peri<'static, USBD>,
}

/// Move the board's pins out of a [`Peripherals`] value into a [`Board`].
///
/// A macro rather than a function so the moves are partial moves out of the
/// caller's `Peripherals`: the pins become owned by the `Board`, the rest of
/// it (peripheral instances, unwired pins) stays usable, and the compiler
/// rejects any later use of a moved pin. No `unsafe`.
macro_rules! take_board {
    ($p:ident) => {
        $crate::board::Board {
            radio: $crate::board::Radio {
                sck: $p.P0_19,
                miso: $p.P0_23,
                mosi: $p.P0_22,
                nss: $p.P0_24,
                reset: $p.P0_25,
                busy: $p.P0_17,
                dio1: $p.P0_20,
            },
            power: $crate::board::Power {
                radio_en: $p.P0_13,
                rail_on: $p.P0_12,
            },
            gps: $crate::board::Gps {
                rx: $p.P1_09,
                tx: $p.P1_08,
                standby: $p.P1_02,
                reset: $p.P1_05,
                pps: $p.P1_04,
            },
            eink: $crate::board::Eink {
                sclk: $p.P0_31,
                mosi: $p.P0_29,
                cs: $p.P0_30,
                dc: $p.P0_28,
                reset: $p.P0_02,
                busy: $p.P0_03,
                en: $p.P1_11,
            },
            buttons: $crate::board::Buttons {
                user: $p.P1_10,
                reset: $p.P0_18,
                touch: $p.P0_11,
            },
            leds: $crate::board::Leds {
                green: $p.P0_15,
                blue: $p.P0_14,
            },
            battery: $crate::board::Battery { adc: $p.P0_04 },
            i2c: $crate::board::I2c {
                sda: $p.P0_26,
                scl: $p.P0_27,
                rtc_int: $p.P0_16,
            },
            flash: $crate::board::Flash {
                sck: $p.P1_14,
                cs: $p.P1_15,
                io0: $p.P1_12,
                io1: $p.P1_13,
            },
            usbd: $p.USBD,
        }
    };
}
pub(crate) use take_board;
