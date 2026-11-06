//! The user button (`board::Buttons`, P1.10): a press redraws the screen
//! at once (`display::redraw`).
//!
//! The board's UF2 bootloader reads its buttons through the internal
//! pull-up, pressed pulling the pin to ground; P1.10 is wired the same
//! way, active low, as a press on the bench (2026-11-06) showed.

use embassy_nrf::gpio::{Input, Pull};
use embassy_nrf::peripherals::P1_10;
use embassy_nrf::Peri;
use embassy_time::{Duration, Timer};

/// How long the pin must stay pressed, and stay released after, to count.
/// The T-Echo's switch has no datasheet in this repository; tactile
/// switches' datasheets give their contacts' bounce as 5 to 10 ms at most,
/// and a person's press lasts about 100 ms. 20 ms is twice the one and a
/// fifth of the other.
const DEBOUNCE: Duration = Duration::from_millis(20);

/// Wait for presses forever. Each wait is on the GPIOTE PORT event, so
/// the button wakes nothing while untouched.
pub async fn run(pin: Peri<'static, P1_10>) -> ! {
    let mut button = Input::new(pin, Pull::Up);
    loop {
        button.wait_for_low().await;
        Timer::after(DEBOUNCE).await;
        if button.is_low() {
            log::info!("button: pressed, redrawing the screen");
            crate::display::redraw();
        }
        button.wait_for_high().await;
        Timer::after(DEBOUNCE).await;
    }
}
