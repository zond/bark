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
//! The pad is a TTP223-BA6 touch sensor's output, powered from the
//! chip's own supply (`docs/touch-pad-2026-11-10.md`); how it is read is
//! `bark_screen::pad`'s (roadmap D175).

use core::sync::atomic::{AtomicBool, Ordering};

use embassy_nrf::gpio::{Input, Pull};
use embassy_nrf::peripherals::{P0_11, P1_10};
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
        Timer::after(TOUCH_DEBOUNCE).await;
    }
}

/// How long the button must stay pressed to count, and released before
/// the next: a choice. Tactile switches' datasheets give their contacts'
/// bounce as 5 to 10 ms at most; this is four times that, and well under
/// the 100 ms or so of a quick press.
const TOUCH_DEBOUNCE: Duration = Duration::from_millis(40);

/// How long the pin is given to follow a pull when the pad is probed:
/// with the chip's 13 kΩ pull (nRF52840 Product Specification, GPIO
/// electrical specification) and up to 1 nF on the pin, five time
/// constants are 65 µs; this is fifteen times that.
const PROBE_SETTLE: Duration = Duration::from_millis(1);

/// Read the pad forever: each touch flips the screen. The rules are
/// `bark_screen::pad`'s, tested on the host; this feeds them the pin's
/// level at each edge and at each deadline they set. Read with no pull
/// once probed: the sensor drives its output both ways, and a pull
/// against it would only cost current. An open build logs every edge.
pub async fn pad(pin: Peri<'static, P0_11>) -> ! {
    use bark_screen::pad::{Event, Pad, SETTLE_MS};
    use embassy_nrf::gpio::Flex;
    use embassy_time::{with_deadline, Instant};
    let bench = crate::protect::BARK_OPEN == 1;
    let level = |high: bool| if high { "high" } else { "low" };
    let now = || Instant::now().as_millis();
    let mut pad = Pad::new();
    let mut pin = Flex::new(pin);
    Timer::at(Instant::from_millis(SETTLE_MS)).await;
    pin.set_as_input(Pull::Up);
    Timer::after(PROBE_SETTLE).await;
    let with_up = pin.is_high();
    pin.set_as_input(Pull::Down);
    Timer::after(PROBE_SETTLE).await;
    let with_down = pin.is_high();
    match pad.probe(with_up, with_down, now()) {
        Event::Rest(high) => {
            // Which level is rest is the board's to say: the bench boards
            // rest high and a touch pulls the pin low (docs, the pad note
            // of 2026-11-10).
            log::info!("pad: rests {}", level(high));
            pin.set_as_input(Pull::None);
        }
        _ => {
            log::warn!("pad: follows the pull, nothing drives it: taken as dead");
            pin.set_as_disconnected();
            core::future::pending::<()>().await;
        }
    }
    let mut high = pin.is_high();
    loop {
        if let Some(event) = pad.read(high, now()) {
            match event {
                Event::Touch => {
                    log::info!("pad: touched");
                    crate::display::touched();
                }
                Event::Held(rest) => log::info!(
                    "pad: held {} s, rests {} now",
                    bark_screen::pad::HOLD_MS / 1000,
                    level(rest)
                ),
                Event::Rest(_) | Event::Dead => {}
            }
        }
        let edge = async {
            if high {
                pin.wait_for_low().await
            } else {
                pin.wait_for_high().await
            }
        };
        match pad.deadline() {
            Some(at) => {
                let _ = with_deadline(Instant::from_millis(at), edge).await;
            }
            None => edge.await,
        }
        let was = high;
        high = pin.is_high();
        if bench && high != was {
            log::info!("pad: edge, {}", level(high));
        }
    }
}
