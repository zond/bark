//! The user button (`board::Buttons`, P1.10), the one input a person
//! uses: pressed briefly it flips the screen between the code and the
//! lines (`bark_screen::schedule`), held for [`LONG_PRESS`] it flips the
//! role (spec "Nodes", roadmap D160, D206).
//!
//! The board's UF2 bootloader reads its buttons through the internal
//! pull-up, pressed pulling the pin to ground; P1.10 is wired the same
//! way, active low, as a press on the bench (2026-11-06) showed.

use core::sync::atomic::{AtomicBool, Ordering};

use embassy_nrf::gpio::{Input, Pull};
use embassy_nrf::peripherals::P1_10;
use embassy_nrf::Peri;
use embassy_time::{with_timeout, Duration, Timer};

/// How long the button is held to flip the role: a choice, thirty
/// times a person's ordinary press of about 100 ms, so that a bump, a
/// press in a bag or a curious finger does not change what the node does
/// for its neighbours, and short enough to hold on purpose.
const LONG_PRESS: Duration = Duration::from_secs(3);

/// How often, and how many times, the button looks whether the core has
/// taken its flip before it shows the lines: up to 200 ms, far longer
/// than a step of the core (`longest_handle_us` in the node's log).
const TAKEN_POLL: Duration = Duration::from_millis(10);
const TAKEN_POLLS: u32 = 20;

/// A long press waits for the core (`node.rs`, `LinkApp::request`).
static FORCE: AtomicBool = AtomicBool::new(false);

/// Whether the button asked to flip the role since the last call.
pub fn take_force() -> bool {
    FORCE.swap(false, Ordering::Relaxed)
}

/// Whether a flip of the role waits, without taking it.
pub fn force_waiting() -> bool {
    FORCE.load(Ordering::Relaxed)
}

/// Wait for presses of the user button forever. Each wait is on the
/// GPIOTE PORT event, so the button wakes nothing while untouched.
pub async fn run(pin: Peri<'static, P1_10>) -> ! {
    let mut button = Input::new(pin, Pull::Up);
    loop {
        button.wait_for_low().await;
        Timer::after(DEBOUNCE).await;
        if !button.is_low() {
            continue;
        }
        if with_timeout(LONG_PRESS, button.wait_for_high())
            .await
            .is_ok()
        {
            log::info!("button: pressed");
            crate::display::pressed();
        } else {
            log::info!("button: held {} s, flipping the role", LONG_PRESS.as_secs());
            FORCE.store(true, Ordering::Relaxed);
            crate::node::WAKE.button_held();
            // The core takes it at its next step, within milliseconds;
            // the lines are drawn once it has, so that they show the
            // role it took, while the button is still held.
            for _ in 0..TAKEN_POLLS {
                if !FORCE.load(Ordering::Relaxed) {
                    break;
                }
                Timer::after(TAKEN_POLL).await;
            }
            Timer::after(TAKEN_POLL).await;
            // So that the person holding it sees the role line.
            crate::display::lines();
        }
        button.wait_for_high().await;
        Timer::after(DEBOUNCE).await;
    }
}

/// How long the button must stay pressed to count, and released before
/// the next: a choice. Tactile switches' datasheets give their contacts'
/// bounce as 5 to 10 ms at most; this is four times that, and well under
/// the 100 ms or so of a quick press.
const DEBOUNCE: Duration = Duration::from_millis(40);
