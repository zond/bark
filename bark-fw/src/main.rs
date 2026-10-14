//! bark firmware for the LILYGO T-Echo. Placeholder entry point: proves the
//! target, the linker layout and that `bark-proto` builds for the target.

#![no_std]
#![no_main]

// Links the nRF52840 interrupt vector table.
use embassy_nrf as _;
use panic_persist as _;

/// Kept in the image so the core is linked for the target.
#[used]
static SYMBOL_TIME_US: u64 = bark_proto::radio::SYMBOL_TIME.as_micros();

#[cortex_m_rt::entry]
fn main() -> ! {
    loop {
        cortex_m::asm::wfi();
    }
}
