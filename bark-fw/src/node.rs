//! The protocol core on the board, a relay on USB and sleepy on battery
//! (`power.rs`, spec "Nodes"), driven by `bark_driver::Driver` against
//! the SX1262. Its node ID is the chip's (`crate::node_id`).
//!
//! Its application is the phone link (`ble.rs`). The node's place comes
//! from `BARK_LAT` and `BARK_LON` (degrees) at build time, or from a map
//! point the phone sets (spec "Node identity", "Position"). Built without
//! them, and with no place stored as a fixed node, the node has none: it
//! sends no heartbeat until its first accepted fix or a map point (roadmap
//! D83, D133). Its seed comes from the chip's random number generator
//! (`docs/core-interface.md` §5).

use core::mem::MaybeUninit;

use bark_driver::{App, Driver, Outgoing, Request};
use bark_link::Status;
use bark_proto::wire::MAX_PAYLOAD;
use bark_proto::{AppEvent, BatteryLevels, Config, Core, Event, Instant, Position, Stored};
use embassy_time::{Duration, Timer};

use crate::battery::Battery;
use crate::clock::Clock;
use crate::radio::{Radio, MAX_TX_DBM, MIN_TX_DBM};

/// The node: every board runs the full one, which can be a relay.
pub type Role = bark_proto::Relay;

/// How often the driver's counters are logged; the uptime line's period.
const REPORT_EVERY: Duration = Duration::from_secs(10);

/// After a radio failure, how long to wait before trying again, so that a
/// radio that stays broken logs once a second instead of filling the log.
const RETRY_PAUSE: Duration = Duration::from_secs(1);

/// The node's application: the phone link (`ble.rs`), which offers the
/// phone's messages and requests and reads what the core tells it; the
/// power from USB (`power.rs`); and what the core keeps, written to flash
/// after the step that asked (`store.rs`).
pub struct LinkApp<'a> {
    battery: Battery<'a>,
    clock: &'a Clock,
    /// The last battery reading, for the status.
    battery_mv: u16,
    /// The step was woken for the link's request.
    serving: bool,
    /// A message the core delivered and the mailbox took, to confirm to
    /// the core once its slot is written (`mailbox.rs`).
    kept: Option<bark_proto::MsgId>,
    /// The mailbox's reply the core holds, to tell its given back from
    /// the phone's.
    replying: Option<bark_proto::MsgId>,
}

impl App for LinkApp<'_> {
    async fn ready(&mut self, offer: bool) {
        if crate::ble::request_waiting() {
            self.serving = true;
            return;
        }
        // The mailbox's reply goes once the core takes one, unless the
        // phone's waits (`take`).
        if offer && crate::mailbox::with(|m| m.reply_due()).unwrap_or(false) {
            return;
        }
        use embassy_futures::select::{select4, Either4};
        let fix = crate::gps::FOR_CORE.wait();
        match select4(
            crate::power::FOR_CORE.wait(),
            crate::ble::FOR_CORE.wait(),
            fix,
            crate::mailbox::FOR_CORE.wait(),
        )
        .await
        {
            Either4::Second(()) => self.serving = true,
            Either4::First(()) | Either4::Third(()) | Either4::Fourth(()) => {}
        }
    }

    fn power(&mut self) -> Option<bool> {
        Some(crate::power::reported())
    }

    async fn battery(&mut self) -> Option<u16> {
        let mv = self.battery.read().await;
        self.battery_mv = mv;
        Some(mv)
    }

    fn request(&mut self) -> Option<Request> {
        use crate::ble::Request as Link;
        if let Some(position) = crate::gps::take_fix() {
            return Some(Request::Fix(position));
        }
        // What the mailbox has written for good, confirmed to the core.
        if let Some(id) = self.kept.filter(|_| crate::mailbox::idle()) {
            self.kept = None;
            return Some(Request::Confirm(id));
        }
        let head = crate::ble::LINK.lock(|l| l.borrow().inbox.map(|(id, _, _)| id));
        let ring = crate::ble::LINK.lock(|l| l.borrow().ring_head);
        crate::ble::take_request(|r| match *r {
            Link::Send { .. } => None,
            // A home's inbox is the ring's: the phone's confirmation moves
            // its mark there, and the core holds nothing for it.
            Link::Confirm(id) if ring.is_some() => {
                let ok = ring.is_some_and(|(n, h)| {
                    n > 0 && h == id && {
                        crate::mailbox::with(|m| m.confirm_read(n));
                        true
                    }
                });
                log::info!("ble: confirm id={:08x} ok={} (mailbox)", id.0, ok);
                Some((None, ok))
            }
            Link::Confirm(id) => {
                let ok = head == Some(id);
                log::info!("ble: confirm id={:08x} ok={}", id.0, ok);
                Some((ok.then_some(Request::Confirm(id)), ok))
            }
            Link::MapPoint(bark_link::MapPoint::Set(position)) => {
                log::info!("ble: map point {}", position);
                Some((Some(Request::SetPosition(position)), true))
            }
            Link::MapPoint(bark_link::MapPoint::Clear) => {
                log::info!("ble: map point cleared");
                Some((Some(Request::ClearMapPoint), true))
            }
            Link::Clock(c) => {
                if crate::utc::phone(c.utc_s.into(), c.offset_min) {
                    crate::store::keep_offset(c.offset_min);
                }
                Some((None, true))
            }
        })
        .flatten()
    }

    /// The one place the core's application send slot is shared: the
    /// phone's message first, then the mailbox's reply (spec "Mailbox").
    fn take(&mut self, payload: &mut [u8; MAX_PAYLOAD]) -> Option<Outgoing> {
        self.phone_message(payload).or_else(|| {
            let out = crate::mailbox::with(|m| m.next(payload)).flatten()?;
            if out.number == 0 {
                log::info!(
                    "mailbox: reply id={:08x} len={} nothing new, taken",
                    out.msg_id.0,
                    out.len
                );
            } else {
                log::info!(
                    "mailbox: reply id={:08x} len={} item {} taken",
                    out.msg_id.0,
                    out.len,
                    out.number
                );
            }
            self.replying = Some(out.msg_id);
            Some(Outgoing {
                dst_pos: out.address.position,
                dst_id: out.address.node,
                len: out.len,
            })
        })
    }
    fn message(&mut self, event: &AppEvent) {
        let now = bark_driver::Clock::now(&self.clock);
        match event {
            AppEvent::Message { msg_id } => log::info!("message id={:08x}", msg_id.0),
            // The person is to be told and may send again after the wait
            // (spec "A sleepy node's own message").
            AppEvent::GivenBack { msg_id, .. } if self.replying == Some(*msg_id) => {
                log::info!("mailbox: reply id={:08x} given back", msg_id.0);
                crate::mailbox::with(|m| m.given_back(*msg_id));
            }
            AppEvent::GivenBack { msg_id, retry_at } => {
                log::warn!(
                    "given back id={:08x} reason=refused retry_in_ms={}",
                    msg_id.0,
                    retry_at.since(now).as_micros() / 1000
                );
                crate::ble::given_back(*msg_id, *retry_at);
            }
            AppEvent::Boot { relays, ends } => {
                let left = ends.map(|t| t.since(now).as_micros() / 1_000_000);
                log::info!("boot relays={} first_rotation_left_s={:?}", relays, left);
                crate::ble::booting(*ends);
            }
        }
    }

    fn store(&mut self, stored: &Stored) {
        log::info!("store {:?}", stored);
        crate::store::keep(*stored);
    }

    fn gps(&mut self, gps: bark_driver::Gps) {
        crate::gps::want(gps);
    }
}

impl LinkApp<'_> {
    /// The phone's message, if it wrote one (`ble.rs`, Send).
    fn phone_message(&mut self, payload: &mut [u8; MAX_PAYLOAD]) -> Option<Outgoing> {
        crate::ble::take_request(|r| match *r {
            crate::ble::Request::Send {
                dst_pos,
                dst_id,
                len,
                payload: p,
            } => {
                payload[..len as usize].copy_from_slice(&p[..len as usize]);
                let id = bark_proto::MsgId::of(dst_pos, dst_id, &p[..len as usize]);
                log::info!("ble: send id={:08x} len={} taken", id.0, len);
                Some((
                    Outgoing {
                        dst_pos,
                        dst_id,
                        len: len as usize,
                    },
                    true,
                ))
            }
            _ => None,
        })
    }
}

/// What the phone reads of `core` now (spec "Phone link", Status).
fn status(core: &Role, now: Instant, battery_mv: u16) -> Status {
    let external_power = crate::power::reported();
    Status {
        ready: core.accepts_send(now),
        external_power,
        sleepy: core.sleepy(),
        carried: core.carried(),
        map_point: core.on_map_point(),
        placed: core.placed(),
        booting: core.booting(),
        anchorless: core.anchorless(),
        relays: core.relays_known(now),
        boot_left_s: 0,
        address: core.address(),
        battery_mv: if external_power { 0 } else { battery_mv },
        inbox: core.inbox_len().min(u8::MAX as usize) as u8,
    }
}

/// What the e-paper shows of `core` now (`display.rs`, `bark-screen`).
fn screen(core: &Role, now: Instant, battery_mv: u16) -> bark_screen::State {
    use bark_screen::Battery;
    let counts = core.neighbour_counts(now);
    let battery = if crate::power::reported() {
        Battery::Usb
    } else if battery_mv == 0 {
        Battery::NotRead
    } else {
        match bark_proto::node::battery_percent(core.battery_millivolts(battery_mv)) {
            Some(p) => Battery::Percent(p),
            None => Battery::Low,
        }
    };
    bark_screen::State {
        starting: core.booting(),
        placed: core.placed(),
        sleepy: core.sleepy(),
        anchored: core.sleepy() && !core.anchorless(),
        carried: core.carried(),
        battery,
        heard: counts.heard,
        strong: counts.strong,
        both_ways: counts.both_ways,
    }
}

/// The position set at build time, or none, converted by `build.rs`.
const BUILT: Option<Position> = include!(concat!(env!("OUT_DIR"), "/place.rs"));

/// The protocol configuration for this board, with what it stored before
/// its last reboot.
pub fn config(stored: Option<Stored>) -> Config {
    let built = BUILT;
    if built.is_none() {
        log::warn!("no BARK_LAT/BARK_LON at build time: no place until a fix");
    }
    let position = built.unwrap_or_default();
    log::info!("stored {:?}", stored);
    Config {
        position,
        placed: built.is_some(),
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
    battery: Battery<'a>,
    config: &Config,
    clock: &'a Clock,
) -> ! {
    let dbm = config.tx_power_dbm.clamp(MIN_TX_DBM, MAX_TX_DBM);
    let app = LinkApp {
        battery,
        clock,
        battery_mv: 0,
        serving: false,
        kept: None,
        replying: None,
    };
    let mut driver = Driver::new(Static(core), radio, clock, app, dbm);
    log::info!(
        "node id={:02x?} at {} power_dbm={} usb={}",
        config.node_id.0,
        config.position,
        dbm,
        crate::power::reported()
    );
    let mut report_at = embassy_time::Instant::now() + REPORT_EVERY;
    let mut first_report = true;
    loop {
        if driver.step().await.is_err() {
            Timer::after(RETRY_PAUSE).await;
        }
        let now = bark_driver::Clock::now(&clock);
        let core = &driver.core().0;
        let mut status = status(core, now, driver.app().battery_mv);
        // A home's messages go into its mailbox, one at a time, each
        // confirmed to the core once it is written (`request`); its phone
        // reads them from the ring.
        let home = crate::mailbox::is_home();
        if home && driver.app().kept.is_none() {
            if let Some((id, payload)) = core.inbox() {
                crate::mailbox::arrive(id, payload);
                driver.app_mut().kept = Some(id);
            }
        }
        let core = &driver.core().0;
        if home {
            let mut item = [0u8; MAX_PAYLOAD];
            let unread = crate::mailbox::unread(&mut item);
            status.inbox = crate::mailbox::with(|m| m.unread_count())
                .unwrap_or(0)
                .min(u32::from(u8::MAX)) as u8;
            crate::ble::publish_ring(status, unread.map(|(n, id, len)| (n, id, &item[..len])));
        } else {
            crate::ble::publish(status, core.inbox());
        }
        crate::display::publish(screen(core, now, driver.app().battery_mv));
        // A request the step was woken for and did not take is answered
        // no: a message the core does not take now.
        crate::ble::answer(core::mem::take(&mut driver.app_mut().serving));
        if embassy_time::Instant::now() >= report_at {
            report_at += REPORT_EVERY;
            if core::mem::take(&mut first_report) {
                let mut facts = crate::boot::facts();
                facts.display = crate::display::OUTCOME.lock(core::cell::Cell::get);
                // A map point keeps the GPS at rest by rule (spec
                // "Position").
                let map_point = driver.core().0.on_map_point();
                log::info!("boot: {} map_point={}", facts, map_point);
            }
            let s = driver.stats();
            let r = driver.radio().stats();
            let t = driver.radio().tcxo();
            log::info!(
                "driver events={} frames={} failed={} held={} radio_errors={} sniffs={} longest_handle_us={} longest_away_us={} \
resets={} configured={} cold_starts={} tcxo_us={} tcxo_errors={} stack_used={}{}",
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
                t.errors,
                crate::stack::used(),
                Time(crate::utc::time())
            );
        }
    }
}

/// The time for the 10 s line, once known.
struct Time(crate::utc::Time);

impl core::fmt::Display for Time {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match (self.0.utc_s, self.0.source) {
            (Some(s), Some(source)) => write!(
                f,
                " utc_s={} offset_min={:?} from={:?}",
                s, self.0.offset_min, source
            ),
            _ => Ok(()),
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

    fn inbox(&self) -> Option<(bark_proto::MsgId, &[u8])> {
        self.0.inbox()
    }

    fn inbox_len(&self) -> usize {
        self.0.inbox_len()
    }

    fn confirm(&mut self, msg_id: bark_proto::MsgId) -> bool {
        self.0.confirm(msg_id)
    }

    fn gps_tracks(&self) -> bool {
        self.0.gps_tracks()
    }

    fn fix_in_reach(&self, position: Position) -> bool {
        self.0.fix_in_reach(position)
    }
}
