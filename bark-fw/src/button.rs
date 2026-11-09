//! The board's two inputs a person uses: the user button
//! (`board::Buttons`, P1.10), held for [`LONG_PRESS`] to force the other
//! role (spec "Nodes", roadmap D160), and the touch pad (P0.11), whose
//! touch flips the screen between the code and the lines
//! (`bark_screen::schedule`).
//!
//! The board's UF2 bootloader reads its buttons through the internal
//! pull-up, pressed pulling the pin to ground; P1.10 is wired the same
//! way, active low, as a press on the bench (2026-11-06) showed. The pad
//! is a touch sensor chip's output, a level while touched; its polarity
//! is not in the repository's notes. Such chips drive their output high
//! while touched by default, so this takes high as touched, with a
//! pull-down so that a pin left floating reads untouched; the log's
//! `pad:` lines show which it is on the bench.

use core::sync::atomic::{AtomicBool, Ordering};

use embassy_nrf::gpio::{Input, Pull};
use embassy_nrf::peripherals::{P0_11, P1_10};
use embassy_nrf::Peri;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::{with_timeout, Duration, Timer};

/// How long the pin must stay pressed, and stay released after, to count.
/// The T-Echo's switch has no datasheet in this repository; tactile
/// switches' datasheets give their contacts' bounce as 5 to 10 ms at most,
/// and a person's press lasts about 100 ms. 20 ms is twice the one and a
/// fifth of the other.
const DEBOUNCE: Duration = Duration::from_millis(20);

/// How long the button is held to force the other role: a choice, thirty
/// times a person's ordinary press of about 100 ms, so that a bump, a
/// press in a bag or a curious finger does not change what the node does
/// for its neighbours, and short enough to hold on purpose.
const LONG_PRESS: Duration = Duration::from_secs(3);

/// A long press waits for the core (`node.rs`, `LinkApp::request`).
static FORCE: AtomicBool = AtomicBool::new(false);
/// Wakes the core's application for a long press.
pub static FOR_CORE: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Whether the button asked to force the other role since the last call.
pub fn take_force() -> bool {
    FORCE.swap(false, Ordering::Relaxed)
}

/// Wait for presses of the user button forever. Each wait is on the
/// GPIOTE PORT event, so the button wakes nothing while untouched.
pub async fn run(pin: Peri<'static, P1_10>) -> ! {
    let mut button = Input::new(pin, Pull::Up);
    loop {
        button.wait_for_low().await;
        Timer::after(DEBOUNCE).await;
        if button.is_low()
            && with_timeout(LONG_PRESS, button.wait_for_high())
                .await
                .is_err()
        {
            log::info!(
                "button: held {} s, forcing the other role",
                LONG_PRESS.as_secs()
            );
            FORCE.store(true, Ordering::Relaxed);
            FOR_CORE.signal(());
            // So that the person holding it sees the role line.
            crate::display::lines();
        }
        button.wait_for_high().await;
        Timer::after(DEBOUNCE).await;
    }
}

/// Wait for touches of the pad forever: each one flips the screen.
pub async fn pad(pin: Peri<'static, P0_11>) -> ! {
    let mut pad = Input::new(pin, Pull::Down);
    log::info!(
        "pad: reads {} untouched",
        if pad.is_high() { "high" } else { "low" }
    );
    loop {
        pad.wait_for_high().await;
        Timer::after(DEBOUNCE).await;
        if pad.is_high() {
            log::info!("pad: touched");
            crate::display::touched();
        }
        pad.wait_for_low().await;
        Timer::after(DEBOUNCE).await;
    }
}
