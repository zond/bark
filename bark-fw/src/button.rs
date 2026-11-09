//! The board's two inputs a person uses: the user button
//! (`board::Buttons`, P1.10), held for [`LONG_PRESS`] to flip the
//! role (spec "Nodes", roadmap D160) and pressed shorter as a touch, and
//! the touch pad (P0.11), whose touch flips the screen between the code
//! and the lines (`bark_screen::schedule`).
//!
//! The board's UF2 bootloader reads its buttons through the internal
//! pull-up, pressed pulling the pin to ground; P1.10 is wired the same
//! way, active low, as a press on the bench (2026-11-06) showed.
//!
//! The pad is a TTP223 touch sensor's output, powered from the chip's own
//! supply, its mode pins left open (`docs/touch-pad-2026-11-09.md`). Its
//! idle level differed between boards on the bench, so the level read at
//! boot, when nobody touches, is idle, and a touch is a change away from
//! it.

use core::sync::atomic::{AtomicBool, Ordering};

use embassy_nrf::gpio::{Input, Pull};
use embassy_nrf::peripherals::{P0_11, P1_10};
use embassy_nrf::Peri;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
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
/// Wakes the core's application for a long press.
pub static FOR_CORE: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Whether the button asked to flip the role since the last call.
pub fn take_force() -> bool {
    FORCE.swap(false, Ordering::Relaxed)
}

/// Wait for presses of the user button forever. Each wait is on the
/// GPIOTE PORT event, so the button wakes nothing while untouched.
pub async fn run(pin: Peri<'static, P1_10>) -> ! {
    let mut button = Input::new(pin, Pull::Up);
    loop {
        button.wait_for_low().await;
        Timer::after(TOUCH_DEBOUNCE).await;
        if !button.is_low() {
            continue;
        }
        // Released before the hold: a touch, for a board whose pad does
        // not answer.
        if with_timeout(LONG_PRESS, button.wait_for_high())
            .await
            .is_ok()
        {
            log::info!("button: pressed, as a touch");
            crate::display::touched();
        } else {
            log::info!("button: held {} s, flipping the role", LONG_PRESS.as_secs());
            FORCE.store(true, Ordering::Relaxed);
            FOR_CORE.signal(());
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
        Timer::after(TOUCH_DEBOUNCE).await;
    }
}

/// How long the pad must stay away from its idle level to count as a
/// touch, and back at it to count as released, and the button pressed
/// and released alike: a choice. The sensor filters its own input, so its
/// output does not bounce; tactile switches' datasheets give their
/// contacts' bounce as 5 to 10 ms at most; this is four times that, and
/// well under the 100 ms or so of a quick tap or press.
const TOUCH_DEBOUNCE: Duration = Duration::from_millis(40);

/// Wait for touches of the pad forever: each one flips the screen. No
/// pull: the sensor drives its output both ways (`docs/touch-pad-2026-11-09.md`),
/// and a pull against it would only cost current while it drives the
/// other way. An open build logs every edge, for the bench.
pub async fn pad(pin: Peri<'static, P0_11>) -> ! {
    use embassy_nrf::gpio::Level;
    let bench = crate::protect::BARK_OPEN == 1;
    let mut pad = Input::new(pin, Pull::None);
    let idle = pad.get_level();
    log::info!(
        "pad: idle {}",
        if idle == Level::High { "high" } else { "low" }
    );
    let away = async |pad: &mut Input<'static>| match idle {
        Level::High => pad.wait_for_low().await,
        Level::Low => pad.wait_for_high().await,
    };
    let back = async |pad: &mut Input<'static>| match idle {
        Level::High => pad.wait_for_high().await,
        Level::Low => pad.wait_for_low().await,
    };
    loop {
        away(&mut pad).await;
        if bench {
            log::info!("pad: edge, {:?}", pad.get_level());
        }
        Timer::after(TOUCH_DEBOUNCE).await;
        if pad.get_level() != idle {
            log::info!("pad: touched");
            crate::display::touched();
        }
        // Back at idle for a debounce before the next.
        loop {
            back(&mut pad).await;
            if bench {
                log::info!("pad: edge, {:?}", pad.get_level());
            }
            Timer::after(TOUCH_DEBOUNCE).await;
            if pad.get_level() == idle {
                break;
            }
        }
    }
}
