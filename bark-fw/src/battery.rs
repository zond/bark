//! The battery's voltage, read on AIN2 behind the board's 2:1 divider
//! (`board.rs`, Battery) by the SAADC at gain 1/6 against its internal
//! 0.6 V reference: 3.6 V full scale at the pin, 7.2 V at the cell, in
//! 12 bits (nRF52840 Product Specification, SAADC).
//!
//! The reading is uncalibrated: the converter's gain error, up to ±3 %
//! at gain 1/6, and the divider's tolerance stay in it, and the core
//! scales it at a rested full charge (spec "Sleepy nodes",
//! `docs/core-interface.md` §4). The driver reads it only between radio
//! operations, so never while the radio transmits (`bark-driver`).

use embassy_nrf::saadc::{ChannelConfig, Config, Oversample, Saadc, Time};

/// The battery's converter.
pub struct Battery<'a> {
    saadc: Saadc<'a, 1>,
}

impl<'a> Battery<'a> {
    /// The converter on the board's battery input. Eight samples averaged
    /// in hardware take a reading's noise below a step; 40 µs of
    /// acquisition suit a source of up to 800 kΩ (Product Specification,
    /// SAADC, acquisition time), which a divider drawing 12 µA from 4 V,
    /// about 330 kΩ, is.
    pub fn new(saadc: Saadc<'a, 1>) -> Self {
        Battery { saadc }
    }

    /// The SAADC configuration for [`Battery::new`].
    pub fn config() -> Config {
        let mut c = Config::default();
        c.oversample = Oversample::Over8x;
        c
    }

    /// The channel configuration for [`Battery::new`].
    pub fn channel<'d>(input: impl embassy_nrf::saadc::Input + 'd) -> ChannelConfig<'d> {
        let mut c = ChannelConfig::single_ended(input);
        c.time = Time::_40US;
        c
    }

    /// The cell's voltage, millivolts, uncalibrated. The converter's offset
    /// moves with temperature, so it is calibrated before each reading,
    /// which a reading a minute affords (Product Specification, SAADC,
    /// calibration).
    pub async fn read(&mut self) -> u16 {
        self.saadc.calibrate().await;
        let mut raw = [0i16; 1];
        self.saadc.sample(&mut raw).await;
        millivolts(raw[0])
    }
}

/// A sample's millivolts at the cell: raw × 2 × 3600 / 4096 (`board.rs`,
/// Battery).
pub fn millivolts(raw: i16) -> u16 {
    (i32::from(raw).max(0) * 7200 / 4096) as u16
}
