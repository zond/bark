//! The bootloader from the serial port: a host that sets the port to
//! 1200 baud (`stty -F /dev/ttyACM0 1200`) asks the board to reset into
//! its UF2 bootloader, the `TECHOBOOT` drive, as a double tap of the reset
//! button does. The log runs at any other rate, so no rate the host uses
//! to read it does this.
//!
//! The board's bootloader (Adafruit's nRF52 UF2 bootloader, 0.6.1,
//! `docs/bench-2026-10-30.md`) runs first after every reset, before
//! `bark-loader`, and reads the general purpose retention register: 0x57
//! keeps it resident with its drive (`DFU_MAGIC_UF2_RESET` in its
//! `main.c`). The register survives a soft reset (nRF52840 Product
//! Specification, POWER, GPREGRET).

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Timer};
use embassy_usb::control::{OutResponse, Recipient, Request, RequestType};
use embassy_usb::Handler;

/// The rate that asks for the bootloader, by the convention of boards
/// with this bootloader.
const TOUCH_BAUD: u32 = 1200;

/// What the bootloader looks for in GPREGRET to stay with its drive.
const DFU_MAGIC_UF2_RESET: u8 = 0x57;

/// CDC SET_LINE_CODING (USB CDC PSTN Subclass 1.2, §6.3.10): the rate is
/// the data's first four bytes, little-endian.
const SET_LINE_CODING: u8 = 0x20;

/// Time for the control transfer that set the rate to finish before the
/// reset, so that the host's `stty` returns cleanly: several 1 ms frames.
const SETTLE: Duration = Duration::from_millis(50);

static TOUCHED: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Watches the CDC interface's line coding. Registered before the CDC
/// class, it sees each request first and leaves it to the class.
pub struct Touch;

impl Handler for Touch {
    fn control_out(&mut self, req: Request, data: &[u8]) -> Option<OutResponse> {
        if req.request_type == RequestType::Class
            && req.recipient == Recipient::Interface
            && req.request == SET_LINE_CODING
            && data.len() >= 4
            && u32::from_le_bytes([data[0], data[1], data[2], data[3]]) == TOUCH_BAUD
        {
            TOUCHED.signal(());
        }
        None
    }
}

/// Reset into the bootloader once the host has asked.
pub async fn run() {
    TOUCHED.wait().await;
    log::warn!("touch: 1200 baud, resetting into the bootloader");
    Timer::after(SETTLE).await;
    embassy_nrf::pac::POWER
        .gpregret()
        .write(|w| w.set_gpregret(DFU_MAGIC_UF2_RESET));
    cortex_m::peripheral::SCB::sys_reset();
}
