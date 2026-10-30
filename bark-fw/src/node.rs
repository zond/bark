//! The protocol core on the board, a relay on USB and sleepy on battery
//! (`power.rs`, spec "Nodes"), driven by `bark_driver::Driver` against
//! the SX1262. Its node ID is the chip's (`crate::node_id`).
//!
//! The node's position comes from `BARK_LAT` and `BARK_LON` (degrees) at
//! build time until the phone app or the GPS can set it
//! (spec "Node identity", "Position"). Its seed comes from the
//! chip's random number generator (`docs/core-interface.md` §5).

use core::mem::MaybeUninit;

use bark_driver::{App, Driver, Outgoing};
use bark_proto::wire::MAX_PAYLOAD;
use bark_proto::{AppEvent, BatteryLevels, Config, Core, Event, Instant, Position, Stored};
use embassy_time::{Duration, Timer};

use crate::battery::Battery;
use crate::clock::Clock;
use crate::radio::{Radio, MAX_TX_DBM, MIN_TX_DBM};
use crate::store::Flash;

/// The node: every board runs the full one, which can be a relay.
pub type Role = bark_proto::Relay;

/// How often the driver's counters are logged; the uptime line's period.
const REPORT_EVERY: Duration = Duration::from_secs(10);

/// After a radio failure, how long to wait before trying again, so that a
/// radio that stays broken logs once a second instead of filling the log.
const RETRY_PAUSE: Duration = Duration::from_secs(1);

/// No application yet (BLE comes later, roadmap D5): nothing to send, and
/// what the core tells the application goes to the log. The power comes
/// from USB (`power.rs`); what the core keeps goes to flash (`store.rs`).
pub struct LogApp<'a> {
    flash: Flash,
    battery: Battery<'a>,
    clock: &'a Clock,
}

impl App for LogApp<'_> {
    async fn ready(&mut self, _offer: bool) {
        crate::power::FOR_CORE.wait().await
    }

    fn power(&mut self) -> Option<bool> {
        Some(crate::power::reported())
    }

    async fn battery(&mut self) -> Option<u16> {
        Some(self.battery.read().await)
    }

    fn take(&mut self, _: &mut [u8; MAX_PAYLOAD]) -> Option<Outgoing> {
        None
    }

    fn message(&mut self, event: &AppEvent) {
        match event {
            AppEvent::Message {
                msg_id,
                len,
                payload,
            } => log::info!(
                "message id={:08x} len={} bytes={:02x?}",
                msg_id.0,
                len,
                &payload[..*len as usize]
            ),
            // The person is to be told and may send again after the wait
            // (spec "A sleepy node's own message").
            AppEvent::GivenBack { msg_id, retry_at } => {
                let now = bark_driver::Clock::now(&self.clock);
                log::warn!(
                    "given back id={:08x} reason=refused retry_in_ms={}",
                    msg_id.0,
                    retry_at.since(now).as_micros() / 1000
                )
            }
            AppEvent::Boot { relays, ends } => {
                let now = bark_driver::Clock::now(&self.clock);
                let left = ends.map(|t| t.since(now).as_micros() / 1_000_000);
                log::info!("boot relays={} first_rotation_left_s={:?}", relays, left)
            }
        }
    }

    fn store(&mut self, stored: &Stored) {
        log::info!("store {:?}", stored);
        self.flash.keep(stored);
    }
}

/// The position set at build time, or none.
fn position() -> Option<Position> {
    let lat = option_env!("BARK_LAT")?.parse::<f64>().ok()?;
    let lon = option_env!("BARK_LON")?.parse::<f64>().ok()?;
    Some(Position::from_degrees(lat, lon))
}

/// The protocol configuration for this board, with what it stored before
/// its last reboot.
pub fn config(stored: Option<Stored>) -> Config {
    let position = position().unwrap_or_else(|| {
        log::warn!("no BARK_LAT/BARK_LON at build time; position 0, 0");
        Position::default()
    });
    log::info!("stored {:?}", stored);
    Config {
        position,
        node_id: crate::node_id(),
        tx_power_dbm: MAX_TX_DBM,
        stored,
        // The board reads its battery (`battery.rs`). On the bench's USB,
        // built to report battery (`power.rs`), the reading is the
        // charger's and never low, so a fixed node would relay for ever:
        // without levels it is sleepy, as the bench wants.
        battery: (option_env!("BARK_ON_BATTERY") != Some("1")).then_some(BatteryLevels::DESIGN),
        ..Config::default()
    }
}

/// All zero bytes are a node, though not a set-up one. rustc checks that
/// the value of a constant is valid for its type, so this fails to build
/// once a field of the core cannot be zero (a `NonZero`, a reference, an
/// enum without a zero discriminant), and [`core`] must change with it.
/// It is never used, so it is never copied, the lint's concern.
#[allow(clippy::large_const_arrays)]
const _: Role = unsafe { MaybeUninit::zeroed().assume_init() };

/// The node, in static memory and set up in place: `Relay::new` builds
/// the relay on the stack, which cannot hold it (`docs/core-interface.md`
/// §6). The static is all zeros, so it lives in `.bss`, which the startup
/// code zeroes, instead of being stored in flash and copied to RAM at
/// boot. Once only.
pub fn core(config: Config, seed: u64) -> &'static mut Role {
    static CORE: static_cell::ConstStaticCell<MaybeUninit<Role>> =
        static_cell::ConstStaticCell::new(MaybeUninit::zeroed());
    // SAFETY: the slot holds zero bytes, a valid `Role` (the constant
    // above), and `reset` sets every field the node reads to what
    // `Relay::new` would (bark-driver's test
    // `a_relay_reset_in_zeroed_memory_is_a_new_one`).
    let relay = unsafe { CORE.take().assume_init_mut() };
    relay.reset(config, seed);
    relay
}

/// Run the core forever.
pub async fn run<'a>(
    core: &'static mut Role,
    radio: Radio<'a>,
    flash: Flash,
    battery: Battery<'a>,
    config: &Config,
    clock: &'a Clock,
) -> ! {
    let dbm = config.tx_power_dbm.clamp(MIN_TX_DBM, MAX_TX_DBM);
    let app = LogApp {
        flash,
        battery,
        clock,
    };
    let mut driver = Driver::new(Static(core), radio, clock, app, dbm);
    log::info!(
        "node id={:02x?} at {} {} power_dbm={} usb={}",
        config.node_id.0,
        config.position.lat_deg(),
        config.position.lon_deg(),
        dbm,
        crate::power::reported()
    );
    let mut report_at = embassy_time::Instant::now() + REPORT_EVERY;
    loop {
        if driver.step().await.is_err() {
            Timer::after(RETRY_PAUSE).await;
        }
        if embassy_time::Instant::now() >= report_at {
            report_at += REPORT_EVERY;
            let s = driver.stats();
            let r = driver.radio().stats();
            let t = driver.radio().tcxo();
            log::info!(
                "driver events={} frames={} failed={} held={} radio_errors={} sniffs={} longest_handle_us={} longest_away_us={} \
resets={} configured={} cold_starts={} tcxo_us={} tcxo_errors={}",
                s.events,
                s.frames,
                s.failed,
                s.held,
                s.radio_errors,
                s.sniffs,
                s.longest_handle.as_micros(),
                s.longest_away.as_micros(),
                r.resets,
                r.configured,
                r.cold_starts,
                t.delay.as_micros(),
                t.errors
            );
        }
    }
}

/// Bytes the host types while the node runs: there is no console to take
/// them, and they are dropped.
pub struct IgnoreInput;

impl embassy_usb_logger::ReceiverHandler for IgnoreInput {
    async fn handle_data(&self, _: &[u8]) {}

    fn new() -> Self {
        IgnoreInput
    }
}

/// The core in static memory: at 168 KiB a relay is kept out of every
/// task's future and stack.
struct Static(&'static mut Role);

impl Core for Static {
    fn handle(&mut self, event: Event<'_>, now: Instant, emit: &mut dyn FnMut(bark_proto::Action)) {
        self.0.handle(event, now, emit)
    }

    fn next_wakeup(&self) -> Option<Instant> {
        self.0.next_wakeup()
    }

    fn accepts_send(&self, now: Instant) -> bool {
        self.0.accepts_send(now)
    }
}
