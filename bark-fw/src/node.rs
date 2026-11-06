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
    /// The power last told the driver.
    power: Option<bool>,
    /// A message the core delivered and the mailbox took, to confirm to
    /// the core once its slot is written (`mailbox.rs`).
    kept: Option<bark_proto::MsgId>,
    /// The core's inbox at the last step, its length and oldest message,
    /// to publish at once when the phone clears the home.
    core_inbox: (u8, Option<(bark_proto::MsgId, u8, [u8; MAX_PAYLOAD])>),
    /// The mailbox's reply the core holds, to tell its given back from
    /// the phone's.
    replying: Option<bark_proto::MsgId>,
}

impl App for LinkApp<'_> {
    /// At each signal, resolves if the driver gets something for it, and
    /// otherwise takes it and waits on: the driver offers a message that
    /// waits, the mailbox's reply among them, by itself.
    async fn ready(&mut self, offer: bool) {
        use embassy_futures::select::{select4, Either4};
        loop {
            let fix = crate::gps::FOR_CORE.wait();
            let new = match select4(
                crate::power::FOR_CORE.wait(),
                crate::ble::FOR_CORE.wait(),
                fix,
                crate::mailbox::FOR_CORE.wait(),
            )
            .await
            {
                Either4::First(()) => Some(crate::power::reported()) != self.power,
                Either4::Second(()) => {
                    self.serving = self.serve_own(offer);
                    self.serving
                }
                Either4::Third(()) => crate::gps::waiting(),
                Either4::Fourth(()) => {
                    self.follow_home();
                    self.kept.is_some() && crate::mailbox::idle()
                }
            };
            if new {
                return;
            }
        }
    }

    fn power(&mut self) -> Option<bool> {
        self.power = Some(crate::power::reported());
        self.power
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
        // The rest is the application's own (`serve_own`).
        let (head, ring) = crate::ble::LINK.lock(|l| {
            let l = l.borrow();
            (l.inbox.map(|(id, _, _)| id), l.ring_head)
        });
        crate::ble::take_request(|r| match *r {
            Link::Confirm(id) if ring.is_none() && head == Some(id) => {
                log::info!("ble: confirm id={:08x} ok=true", id.0);
                Some((Request::Confirm(id), true))
            }
            Link::MapPoint(bark_link::MapPoint::Set(position)) => {
                log::info!("ble: map point {}", position);
                Some((Request::SetPosition(position), true))
            }
            Link::MapPoint(bark_link::MapPoint::Clear) => {
                log::info!("ble: map point cleared");
                Some((Request::ClearMapPoint, true))
            }
            Link::Send { .. } | Link::Confirm(_) | Link::Clock(_) => None,
        })
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
    /// Publish a home set or cleared since the last step at once, so that
    /// what the phone reads after its write shows it: the ring, or the
    /// core's inbox as the last step left it.
    fn follow_home(&self) {
        let (status, ring) = crate::ble::LINK.lock(|l| {
            let l = l.borrow();
            (l.status, l.ring_head.is_some())
        });
        let home = crate::mailbox::is_home();
        if home == ring {
            return;
        }
        if home {
            publish_home(status);
        } else {
            let (len, head) = &self.core_inbox;
            let status = Status {
                inbox: *len,
                ..status
            };
            crate::ble::publish(
                status,
                head.as_ref().map(|(id, n, p)| (*id, &p[..*n as usize])),
            );
        }
    }

    /// Serve the phone's request waiting if the driver gets nothing of it,
    /// and answer it at once: the clock; a home's confirmation, which
    /// moves the ring's mark; a confirmation of a message the inbox does
    /// not show; and a message while the core takes none. Whether a
    /// request for the driver waits.
    fn serve_own(&mut self, offer: bool) -> bool {
        use crate::ble::Request as Link;
        let (head, ring) = crate::ble::LINK.lock(|l| {
            let l = l.borrow();
            (l.inbox.map(|(id, _, _)| id), l.ring_head)
        });
        let own = crate::ble::take_request(|r| match *r {
            Link::Send { .. } if !offer => {
                log::info!("ble: send refused, the node takes none now");
                Some((false, false))
            }
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
                Some((true, ok))
            }
            Link::Confirm(id) if head != Some(id) => {
                log::info!("ble: confirm id={:08x} ok=false", id.0);
                Some((false, false))
            }
            Link::Clock(c) => {
                if crate::utc::phone(c.utc_s.into(), c.offset_min) {
                    crate::store::keep_offset(c.offset_min);
                }
                Some((false, true))
            }
            _ => None,
        });
        let Some(ring) = own else {
            return crate::ble::request_waiting();
        };
        // What the phone reads after its answer shows its write.
        if ring {
            publish_home(crate::ble::LINK.lock(|l| l.borrow().status));
        }
        crate::ble::answer(false);
        false
    }

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

/// Publish a home's inbox, its ring's oldest unread item, with `status`.
fn publish_home(mut status: Status) {
    let mut item = [0u8; MAX_PAYLOAD];
    let unread = crate::mailbox::unread(&mut item);
    status.inbox = crate::mailbox::with(|m| m.unread_count())
        .unwrap_or(0)
        .min(u32::from(u8::MAX)) as u8;
    crate::ble::publish_ring(status, unread.map(|(n, id, len)| (n, id, &item[..len])));
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
        // The phone link's, which the screen fills in (`display::phone`).
        phone: bark_screen::Phone::None,
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
    // A place given at build time is where the bench says the board
    // stands: it wins over the place stored, and the board is fixed there,
    // so that boards built in a line are a line at every boot.
    let stored = match built {
        Some(place) => stored.map(|s| Stored {
            place,
            carried: false,
            map_point: false,
            ..s
        }),
        None => stored,
    };
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
        power: None,
        kept: None,
        replying: None,
        core_inbox: (0, None),
    };
    let mut driver = Driver::new(Static(core), radio, clock, app, dbm);
    log::info!(
        "node id={:02x?} at {} power_dbm={} usb={}",
        config.node_id.0,
        config.position,
        dbm,
        crate::power::reported()
    );
    if let Some(id) = bark_driver::BENCH_DEAF_TO {
        log::warn!("bench build: deaf to {:02x?} (BARK_IGNORE)", id.0);
    }
    let mut report_at = embassy_time::Instant::now() + REPORT_EVERY;
    let mut first_report = true;
    loop {
        if driver.step().await.is_err() {
            Timer::after(RETRY_PAUSE).await;
        }
        let now = bark_driver::Clock::now(&clock);
        let core = &driver.core().0;
        let status = status(core, now, driver.app().battery_mv);
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
            publish_home(status);
        } else {
            crate::ble::publish(status, core.inbox());
        }
        let head = core.inbox().map(|(id, p)| {
            let mut payload = [0; MAX_PAYLOAD];
            payload[..p.len()].copy_from_slice(p);
            (id, p.len() as u8, payload)
        });
        let core_inbox = (status.inbox, head);
        driver.app_mut().core_inbox = core_inbox;
        let core = &driver.core().0;
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
                "driver events={} frames={} failed={} held={} radio_errors={} sniffs={} longest_handle_us={} longest_away_us={} empty_readies={} \
resets={} configured={} cold_starts={} tcxo_us={} tcxo_errors={} stack_used={}{}",
                s.events,
                s.frames,
                s.failed,
                s.held,
                s.radio_errors,
                s.sniffs,
                s.longest_handle.as_micros(),
                s.longest_away.as_micros(),
                s.empty_readies,
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
