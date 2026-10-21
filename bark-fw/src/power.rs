//! Whether USB power is present, from the nRF52840's VBUS detection: the
//! USBREGSTATUS register's VBUSDETECT field, and the USBDETECTED and
//! USBREMOVED events at each change (nRF52840 Product Specification,
//! POWER, USB supply). The core takes its role from it, a relay on USB and
//! sleepy on battery (spec "Hardware", roadmap D71); the LED and the 32 MHz
//! crystal follow it too (`main.rs`).
//!
//! The USB driver enables the two events and its own handler clears them
//! (embassy-nrf `vbus_detect`); the handler here, bound to the same
//! interrupt, only tells the waiters to read the register again.

use embassy_nrf::interrupt::typelevel::{Handler, CLOCK_POWER};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;

/// Whether USB power is present now.
pub fn usb() -> bool {
    embassy_nrf::pac::POWER.usbregstatus().read().vbusdetect()
}

/// The power the node reports to its core: USB's, or for bench work,
/// built with `BARK_ON_BATTERY=1`, battery whatever USB says, so that a
/// node on the bench's USB runs sleepy and logs (`scripts/measure.md`
/// step 5).
#[cfg(not(feature = "console"))]
pub fn reported() -> bool {
    usb() && option_env!("BARK_ON_BATTERY") != Some("1")
}

/// Raised at every USB power event, one for each waiter: the driver's
/// application and the LED.
pub static FOR_CORE: Signal<CriticalSectionRawMutex, ()> = Signal::new();
/// See [`FOR_CORE`].
pub static FOR_LED: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// The CLOCK_POWER interrupt, beside the USB driver's handler.
pub struct InterruptHandler;

impl Handler<CLOCK_POWER> for InterruptHandler {
    unsafe fn on_interrupt() {
        FOR_CORE.signal(());
        FOR_LED.signal(());
    }
}
