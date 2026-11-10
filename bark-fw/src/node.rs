//! The protocol core on the board, a relay on USB and on battery what its
//! motion and its battery make it (`power.rs`, spec "Nodes"), driven by
//! `bark_driver::Driver` against the SX1262. Its node ID is the chip's
//! (`crate::node_id`).
//!
//! Its application is the phone link (`ble.rs`) and the update over the
//! mesh (`update/mesh.rs`). The node's place comes
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
    /// The core's inbox's oldest message while the node places it
    /// ([`LinkApp::place`]).
    placing: Option<Placing>,
    /// The core's inbox at the last step, its length and oldest message,
    /// to publish at once when the phone clears the home.
    core_inbox: (u8, Option<bark_proto::MsgId>),
    /// The mailbox's reply the core holds, to tell its given back from
    /// the phone's.
    replying: Option<bark_proto::MsgId>,
}

/// The core's inbox's oldest message on a node whose ring keeps what
/// arrives, a home or a public node (spec "Mailbox"; "Phone link",
/// Public): into the ring, the owner's and a guest's alike, and confirmed
/// to the core once its slot is written, so that one taken is not lost to
/// a reboot meanwhile.
#[derive(Clone, Copy)]
struct Placing {
    id: bark_proto::MsgId,
}

impl App for LinkApp<'_> {
    /// At each signal, resolves if the driver gets something for it, and
    /// otherwise takes it and waits on: the driver offers a message that
    /// waits, the mailbox's reply among them, by itself.
    async fn ready(&mut self, offer: bool) {
        use embassy_futures::select::{select6, Either6};
        loop {
            let fix = crate::gps::FOR_CORE.wait();
            let new = match select6(
                crate::power::FOR_CORE.wait(),
                crate::ble::FOR_CORE.wait(),
                fix,
                crate::mailbox::FOR_CORE.wait(),
                crate::update::mesh::ready(),
                crate::button::FOR_CORE.wait(),
            )
            .await
            {
                Either6::First(()) => Some(crate::power::reported()) != self.power,
                Either6::Second(()) => {
                    self.serving = self.serve_own(offer);
                    self.serving
                }
                Either6::Third(()) => crate::gps::waiting(),
                Either6::Fourth(()) => {
                    self.follow_home();
                    self.placed().is_some()
                }
                Either6::Fifth(()) | Either6::Sixth(()) => true,
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
        if crate::button::take_force() {
            return Some(Request::ForceRole);
        }
        if let Some(r) = crate::update::mesh::request() {
            return Some(r);
        }
        if crate::gps::take_lost() {
            return Some(Request::NoFix);
        }
        if let Some(position) = crate::gps::take_fix() {
            return Some(Request::Fix(position));
        }
        // What is placed for good, confirmed to the core.
        if let Some(id) = self.placed() {
            self.placing = None;
            return Some(Request::Confirm(id));
        }
        // The rest is the application's own (`serve_own`).
        let (head, ring) = crate::ble::LINK.lock(|l| {
            let l = l.borrow();
            (l.inbox, l.ring_head)
        });
        crate::ble::take_request(|_, r| match *r {
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
            Link::Ask(id) => {
                log::info!("ble: ask {:02x?}", id.0);
                Some((Request::Ask(id), true))
            }
            Link::Send { .. }
            | Link::Confirm(_)
            | Link::Clock(_)
            | Link::Neighbours(_)
            | Link::Inbox(_)
            | Link::Guest { .. } => None,
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
            AppEvent::Heard => {
                log::info!("heard a node");
                crate::update::heard();
            }
            AppEvent::Status { from, status } => {
                log::info!("status of {:02x?}: {:?}", from.0, status);
                crate::ble::told_status(bark_link::Answer {
                    from: *from,
                    status: *status,
                });
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

    fn offered(&mut self, image: &bark_proto::update::Image) {
        crate::update::mesh::offered(image);
    }

    fn store_piece(&mut self, piece: &bark_proto::update::Piece) {
        crate::update::mesh::store_piece(piece);
    }

    async fn serve(&mut self, frame: &mut bark_proto::Frame) -> bool {
        crate::update::mesh::serve(frame).await
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
        let home = crate::mailbox::keeps();
        if home == ring {
            return;
        }
        if home {
            publish_home(status);
        } else {
            let (len, head) = self.core_inbox;
            let status = Status {
                inbox: len,
                ..status
            };
            crate::ble::publish(status, head);
        }
    }

    /// Serve the phone's request waiting if the driver gets nothing of it,
    /// and answer it at once: the clock; a guest's confirmation; a ring's
    /// confirmation, which moves the ring's mark; a confirmation of a
    /// message the inbox does not show; and a message while the core
    /// takes none. Whether a request for the driver waits.
    fn serve_own(&mut self, offer: bool) -> bool {
        use crate::ble::Request as Link;
        let (head, ring) = crate::ble::LINK.lock(|l| {
            let l = l.borrow();
            (l.inbox, l.ring_head)
        });
        let own = crate::ble::take_request(|_, r| match *r {
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
        self.placed().is_some()
    }

    /// The message placed, once the ring has it written, to confirm to
    /// the core.
    fn placed(&self) -> Option<bark_proto::MsgId> {
        let p = self.placing?;
        crate::mailbox::idle().then_some(p.id)
    }

    /// Place the core's inbox's oldest message as it arrives ([`Placing`]),
    /// once, on a node whose ring keeps what arrives; on any other the
    /// core's inbox is the phone's, as without the mailbox.
    fn place(&mut self, oldest: Option<(bark_proto::MsgId, &[u8])>) {
        if self.placing.is_some() || !crate::mailbox::keeps() {
            return;
        }
        let Some((id, payload)) = oldest else {
            return;
        };
        crate::mailbox::arrive(id, payload);
        if crate::ble::public() {
            log::info!(
                "public: id={:08x} tag={:02x?} kept",
                id.0,
                bark_app_layer::seal::recipient_of(payload)
            );
        }
        self.placing = Some(Placing { id });
    }

    /// The phone's message, if it wrote one (`ble.rs`, Send).
    fn phone_message(&mut self, payload: &mut [u8; MAX_PAYLOAD]) -> Option<Outgoing> {
        crate::ble::take_request(|_, r| match *r {
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
        heard: core.neighbour_counts(now).heard,
    }
}

/// The page of the neighbour table of `core` from slot `from`, as the
/// phone reads it, into `b`: the live neighbours from that slot on, as
/// many as fit, and the slot to ask for next, 0 at the table's end (spec
/// "Phone link", Neighbours); its length.
fn neighbours_page(
    core: &Role,
    now: Instant,
    from: u8,
    b: &mut [u8; bark_link::neighbours::MAX_LEN],
) -> usize {
    use bark_link::neighbours::{ddbm, encode, Header, Neighbour, PER_PAGE};
    let table = core.neighbour_table();
    let anchor = core.anchor().map(|a| a.id);
    let mut page = [Neighbour::default(); PER_PAGE];
    let mut len = 0;
    let mut next = 0;
    for (slot, n) in table.live(now).filter(|&(s, _)| s >= usize::from(from)) {
        if len == page.len() {
            next = slot as u8;
            break;
        }
        let age_s = now.since(n.last_heard).as_micros() / 1_000_000;
        page[len] = Neighbour {
            id: n.id,
            position: n.position,
            rssi_ddbm: ddbm(n.rssi),
            age_s: age_s.min(u64::from(u16::MAX)) as u16,
            sleepy: n.sleepy,
            map_point: n.map_point,
            demoted: table.is_demoted(n, now),
            banned: table.is_banned(n, now),
            anchor: Some(n.id) == anchor,
            carried: n.carried,
            anchorless: n.anchorless,
            proven: n.taken_at > Instant::ZERO,
        };
        len += 1;
    }
    let header = Header {
        floor_dbm: core
            .busy()
            .noise_floor_dbm()
            .clamp(i8::MIN.into(), i8::MAX.into()) as i8,
        next,
        own: match core.placed() {
            true => core.config().position,
            false => Position::default(),
        },
    };
    encode(&header, &page[..len], b)
}

/// Publish a ring's inbox, its oldest unread item, with `status`.
fn publish_home(mut status: Status) {
    let mut item = [0u8; MAX_PAYLOAD];
    let unread = crate::mailbox::unread(&mut item);
    status.inbox = crate::mailbox::with(|m| m.unread_count())
        .unwrap_or(0)
        .min(u32::from(u8::MAX)) as u8;
    crate::ble::publish_ring(status, unread.map(|(n, id, _)| (n, id)));
}

/// The Inbox as the paired phone reads it now, into `b`: the ring's oldest
/// unread item where the ring keeps the owner's, else the core's inbox's
/// oldest; its length.
fn inbox_message(oldest: Option<(bark_proto::MsgId, &[u8])>, b: &mut [u8]) -> usize {
    let at: &mut [u8; bark_link::Message::MAX_LEN] = (&mut b[..bark_link::Message::MAX_LEN])
        .try_into()
        .expect("it fits");
    let mut item = [0u8; MAX_PAYLOAD];
    let shown = match crate::mailbox::keeps() {
        true => crate::mailbox::unread(&mut item).map(|(_, id, len)| (id, &item[..len])),
        false => oldest,
    };
    shown.map_or(0, |(msg_id, payload)| {
        bark_link::Message { msg_id, payload }.encode(at)
    })
}

/// What a guest reads from Guest now, into `b`: the oldest item in the
/// ring after `after` for its `tag` (spec "Phone link", Public); its
/// length.
fn guest_item(tag: u8, after: u32, b: &mut [u8]) -> usize {
    let at: &mut [u8; bark_link::GuestItem::MAX_LEN] = (&mut b[..bark_link::GuestItem::MAX_LEN])
        .try_into()
        .expect("it fits");
    let mut item = [0u8; MAX_PAYLOAD];
    crate::mailbox::with(|m| m.for_tag(tag, after, &mut item))
        .flatten()
        .map_or(0, |(number, msg_id, len)| {
            bark_link::GuestItem {
                number,
                msg_id,
                payload: &item[..len],
            }
            .encode(at)
        })
}

/// What the e-paper shows of `core` now (`display.rs`, `bark-screen`):
/// the status the node tells a neighbour that asks
/// (spec "Neighbour status"), so that the two agree.
fn screen(core: &Role, now: Instant) -> bark_screen::State {
    use bark_proto::status::Battery as Told;
    use bark_screen::Battery;
    let s = core.status(now);
    let battery = match s.battery {
        Told::Usb => Battery::Usb,
        Told::NotRead => Battery::NotRead,
        Told::Low => Battery::Low,
        Told::Percent(p) => Battery::Percent(p),
    };
    bark_screen::State {
        starting: core.booting(),
        placed: s.placed,
        sleepy: s.sleepy,
        anchored: s.anchored,
        carried: s.carried,
        battery,
        heard: s.neighbours.heard,
        strong: s.neighbours.strong,
        both_ways: s.neighbours.both_ways,
        // The phone link's, which the screen fills in (`display::phone`).
        phone: bark_screen::Phone::Unpaired,
        // Not the pieces of the next block, which would redraw the
        // screen each gap for a figure nobody acts on.
        getting: (s.gathering.version != 0).then_some(bark_screen::Getting {
            version: s.gathering.version,
            complete: s.gathering.complete,
            blocks: s.gathering.blocks,
        }),
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
        version: crate::update::BARK_IMAGE_VERSION,
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
        placing: None,
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
        crate::update::note_power(crate::update::Power {
            external: status.external_power,
            battery_mv: core.battery_millivolts(driver.app().battery_mv),
        });
        // A home's or a public node's messages are placed one at a time,
        // each confirmed to the core once it is (`request`); the paired
        // phone reads its own from the ring.
        let head = core.inbox().map(|(id, p)| {
            let mut payload = [0; MAX_PAYLOAD];
            payload[..p.len()].copy_from_slice(p);
            (id, p.len() as u8, payload)
        });
        let oldest = head.as_ref().map(|(id, n, p)| (*id, &p[..usize::from(*n)]));
        driver.app_mut().place(oldest);
        let core = &driver.core().0;
        if crate::mailbox::keeps() {
            publish_home(status);
        } else {
            crate::ble::publish(status, core.inbox().map(|(id, _)| id));
        }
        let core_inbox = (status.inbox, head.as_ref().map(|(id, ..)| *id));
        driver.app_mut().core_inbox = core_inbox;
        let core = &driver.core().0;
        crate::ble::serve_read(|r, b| match *r {
            crate::ble::Request::Neighbours(from) => neighbours_page(core, now, from, b),
            crate::ble::Request::Inbox(_) => inbox_message(oldest, b),
            crate::ble::Request::Guest { tag, after, .. } => guest_item(tag, after, b),
            _ => 0,
        });
        crate::display::publish(screen(core, now));
        // A request the step was woken for and did not take is answered
        // no: a message the core does not take now.
        crate::ble::answer(core::mem::take(&mut driver.app_mut().serving));
        // An image gathered over the mesh and staged: the reboot into it
        // waits for a moment that loses nothing (`update::mesh::quiet`),
        // and ends the phones' links first.
        if let Some(version) = crate::update::mesh::staged() {
            let core = &driver.core().0;
            let holding =
                core.holding().next().is_some() || core.inbox_len() > 0 || !crate::mailbox::idle();
            if crate::update::mesh::quiet(holding) {
                log::info!(
                    "update: rebooting into version {} at a quiet moment",
                    version
                );
                crate::ble::close_all().await;
                crate::update::reboot().await;
            }
        }
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
resets={} configured={} cold_starts={} tcxo_us={} tcxo_errors={} stack_used={} unserved={} update={}{}",
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
                s.unserved,
                crate::update::mesh::phase(),
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

/// The core in static memory: at 93 KiB a relay is kept out of every
/// task's future and stack.
struct Static(&'static mut Role);

impl Core for Static {
    fn handle(&mut self, event: Event<'_>, now: Instant, emit: &mut dyn FnMut(bark_proto::Action)) {
        // A bench image's deafness: the frame never reached the core.
        if crate::update::deaf() && matches!(event, Event::Received { .. }) {
            return;
        }
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
