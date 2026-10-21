//! The protocol core on the board, a relay on USB and sleepy on battery
//! (`power.rs`, spec "Hardware"), driven by `bark_driver::Driver` against
//! the SX1262. Its node ID is the
//! low word of the chip's factory device ID (nRF52840 Product
//! Specification, FICR DEVICEID), the same at every boot.
//!
//! The node's position comes from `BARK_LAT` and `BARK_LON` (degrees) at
//! build time until the phone app or the GPS can set it
//! (spec "Node identity", "Position and time"). Its seed comes from the
//! chip's random number generator (`docs/core-interface.md` §5).

use bark_driver::{App, Clock, Driver, Outgoing};
use bark_proto::wire::MAX_PAYLOAD;
use bark_proto::{AppEvent, Config, Core, Event, Instant, NodeId, Position};
use embassy_time::{Duration, Timer, TICK_HZ};

use crate::radio::{Radio, MAX_TX_DBM, MIN_TX_DBM};

/// The node: every board runs the full one, which can be a relay.
pub type Role = bark_proto::Relay;

/// How often the driver's counters are logged; the uptime line's period.
const REPORT_EVERY: Duration = Duration::from_secs(10);

/// After a radio failure, how long to wait before trying again, so that a
/// radio that stays broken logs once a second instead of filling the log.
const RETRY_PAUSE: Duration = Duration::from_secs(1);

/// The embassy clock in the core's microseconds.
pub struct Monotonic;

impl Clock for Monotonic {
    fn now(&self) -> Instant {
        Instant::from_micros(embassy_time::Instant::now().as_micros())
    }

    async fn at(&self, t: Instant) {
        // Round up to the next tick: a tick rounded down would fire before
        // `t`, and the core must never be woken early (core-interface §2).
        let ticks = (u128::from(t.as_micros()) * u128::from(TICK_HZ)).div_ceil(1_000_000);
        Timer::at(embassy_time::Instant::from_ticks(ticks as u64)).await
    }
}

/// No application yet (BLE comes later, roadmap D5): nothing to send, and
/// what the core tells the application goes to the log. The power comes
/// from USB (`power.rs`).
pub struct LogApp;

impl App for LogApp {
    async fn ready(&mut self, _offer: bool) {
        crate::power::FOR_CORE.wait().await
    }

    fn power(&mut self) -> Option<bool> {
        Some(crate::power::reported())
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
                let now = Monotonic.now();
                log::warn!(
                    "given back id={:08x} reason=refused retry_in_ms={}",
                    msg_id.0,
                    retry_at.since(now).as_micros() / 1000
                )
            }
        }
    }
}

/// The position set at build time, or none.
fn position() -> Option<Position> {
    let lat = option_env!("BARK_LAT")?.parse::<f64>().ok()?;
    let lon = option_env!("BARK_LON")?.parse::<f64>().ok()?;
    Some(Position::from_degrees(lat, lon))
}

/// The protocol configuration for this board.
pub fn config() -> Config {
    let position = position().unwrap_or_else(|| {
        log::warn!("no BARK_LAT/BARK_LON at build time; position 0, 0");
        Position::default()
    });
    Config {
        position,
        node_id: NodeId(embassy_nrf::pac::FICR.deviceid(0).read().to_be_bytes()),
        tx_power_dbm: MAX_TX_DBM,
        ..Config::default()
    }
}

/// The node, in static memory built at compile time and set up in place:
/// `Relay::new` would need 150 KiB more stack than the board has
/// (`docs/core-interface.md` §6). Once only.
pub fn core(config: Config, seed: u64) -> &'static mut Role {
    static CORE: static_cell::ConstStaticCell<Role> =
        static_cell::ConstStaticCell::new(Role::UNSET);
    let relay = CORE.take();
    relay.reset(config, seed);
    relay
}

/// Run the core forever.
pub async fn run(core: &'static mut Role, radio: Radio<'_>, config: &Config) -> ! {
    let dbm = config.tx_power_dbm.clamp(MIN_TX_DBM, MAX_TX_DBM);
    let mut driver = Driver::new(Static(core), radio, Monotonic, LogApp, dbm);
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
            log::info!(
                "driver events={} frames={} failed={} held={} radio_errors={} sniffs={} longest_handle_us={} longest_away_us={}",
                s.events,
                s.frames,
                s.failed,
                s.held,
                s.radio_errors,
                s.sniffs,
                s.longest_handle.as_micros(),
                s.longest_away.as_micros()
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

/// The core in static memory: at 150 KiB a relay is kept out of every
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
