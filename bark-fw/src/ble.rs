//! The phone link (spec "Phone link"): Nordic's SoftDevice Controller
//! under its multiprotocol service layer (MPSL), and TrouBLE's host over
//! it, serving the GATT service whose values `bark-link` encodes.
//!
//! The MPSL takes RTC0, TIMER0, the TEMP sensor, PPI channels 19, 30 and
//! 31 and the clocks, and runs the RADIO, RTC0 and TIMER0 interrupts at
//! priority 0; the controller takes PPI channels 17, 18 and 20 to 29
//! (nrf-sdc's documentation). So the embassy clock runs on RTC1, every
//! other interrupt runs below priority 0 (`main.rs`), the die temperature
//! comes from the MPSL (`radio.rs`), and the 32 MHz crystal is asked of
//! it (`main.rs`).
//!
//! The node's side (`node.rs`) and this side meet in [`LINK`]: the node
//! publishes what the phone reads after every step of its driver, and
//! takes what the phone writes as requests, one at a time, each answered
//! before the phone's write is (roadmap D117).

use core::cell::RefCell;

use bark_link::{
    le_bytes, seconds_left, uuid, GivenBack, Malformed, Message, Node, Status, ATT_MTU,
};
use bark_proto::wire::MAX_PAYLOAD;
use bark_proto::{Instant, MsgId, NodeId, Position};
use embassy_futures::join::join;
use embassy_futures::select::{select, Either};
use embassy_nrf::{peripherals, rng, Peri};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::signal::Signal;
use nrf_mpsl::{raw as mpsl_raw, MultiprotocolServiceLayer};
use nrf_sdc::{self as sdc, SoftdeviceController};
use static_cell::StaticCell;
use trouble_host::att::{AttClient, AttCmd, AttReq};
use trouble_host::prelude::*;

/// The link's one connection (spec "Phone link").
const CONNECTIONS: usize = 1;
/// L2CAP channels: the signalling channel and the ATT channel.
const CHANNELS: usize = 2;
/// Link-layer buffers each way: the controller's default for transmit,
/// three (`SDC_DEFAULT_TX_PACKET_COUNT`, `sdc.h`), and as many to receive.
const LL_BUFFERS: u8 = 3;
/// The link-layer payload: the ATT MTU and the 4-byte L2CAP header in one
/// packet (roadmap D119).
const LL_PAYLOAD: u16 = ATT_MTU + 4;

// The controller's memory for one peripheral link, one advertising set of
// a legacy advertisement's 31 bytes and the default filter accept list of
// 8, by the formulas of `sdc.h` (`SDC_MEM_PER_PERIPHERAL_LINK`,
// `SDC_MEM_PERIPHERAL_LINKS_SHARED`, `SDC_MEM_PER_ADV_SET`, `SDC_MEM_FAL`),
// which are upper bounds; the controller's own count is logged at boot.
const LL_OVERHEAD: usize = 15;
const LL_DEFAULT: usize = 27;
const PER_LINK: usize = 923
    + 2 * (LL_BUFFERS as usize * (LL_PAYLOAD as usize + LL_OVERHEAD) - (LL_DEFAULT + LL_OVERHEAD));
const LINKS_SHARED: usize = 17;
const ADV_DATA: usize = 31;
const ADV_SET: usize = (5184 + ADV_DATA * 18) / 10;
const FAL: usize = 4 + 8 * 8;
const SDC_MEM: usize = PER_LINK + LINKS_SHARED + ADV_SET + FAL;

/// The advertising interval, 1022.5 ms, 1636 of the 0.625 ms units of the
/// advertising parameters (Vol 4 Part E §7.8.5; roadmap D120). The host
/// takes it as an embassy duration, in ticks of 1/32 768 s, and truncates
/// it to the units: so it is given as the ticks just over 1636 units,
/// since 1 022 500 µs truncated to ticks would make 1635.
///
/// For the bench, `BARK_ADV_INTERVAL_MS` at build time sets another, in
/// whole ms that are whole units (multiples of 5 ms), from the 20 ms the
/// advertising parameters allow (Vol 4 Part E §7.8.5): to see whether how
/// often the node advertises changes how often a central connects.
const ADV_UNITS: u64 = match option_env!("BARK_ADV_INTERVAL_MS") {
    None => 1636,
    Some(ms) => {
        let us = crate::update::decimal(ms) as u64 * 1000;
        assert!(
            us.is_multiple_of(625),
            "BARK_ADV_INTERVAL_MS is a multiple of 5"
        );
        assert!(
            us >= 20_000 && us <= 10_240_000,
            "BARK_ADV_INTERVAL_MS is 20 to 10240"
        );
        us / 625
    }
};
const ADV_INTERVAL: embassy_time::Duration = embassy_time::Duration::from_ticks(
    (ADV_UNITS * 625 * embassy_time::TICK_HZ).div_ceil(1_000_000),
);
const _: () = assert!(ADV_INTERVAL.as_micros() / 625 == ADV_UNITS);

/// A write's values are not kept: the node takes each as a request
/// (`Request`), so Send and Map point have no storage.
///
/// The GATT server: one service.
#[gatt_server]
pub struct Server {
    pub link: Link,
}

/// The service and its characteristics, each with room for its value's
/// longest form (spec "Phone link").
#[gatt_service(uuid = le_bytes(uuid::SERVICE))]
pub struct Link {
    #[characteristic(uuid = le_bytes(uuid::NODE), read)]
    pub node: [u8; Node::LEN],
    #[characteristic(uuid = le_bytes(uuid::STATUS), read, notify)]
    pub status: [u8; Status::LEN],
    #[characteristic(uuid = le_bytes(uuid::SEND), write)]
    pub send: (),
    #[characteristic(uuid = le_bytes(uuid::INBOX), read, write)]
    pub inbox: heapless::Vec<u8, { bark_link::Message::MAX_LEN }>,
    #[characteristic(uuid = le_bytes(uuid::GIVEN_BACK), read, notify)]
    pub given_back: heapless::Vec<u8, { bark_link::GivenBack::LEN }>,
    #[characteristic(uuid = le_bytes(uuid::MAP_POINT), write)]
    pub map_point: (),
    #[characteristic(uuid = le_bytes(uuid::CLOCK), read, write)]
    pub clock: [u8; bark_link::NodeTime::LEN],
}

/// What the MPSL takes.
pub struct MpslParts {
    pub rtc0: Peri<'static, peripherals::RTC0>,
    pub timer0: Peri<'static, peripherals::TIMER0>,
    pub temp: Peri<'static, peripherals::TEMP>,
    pub ppi19: Peri<'static, peripherals::PPI_CH19>,
    pub ppi30: Peri<'static, peripherals::PPI_CH30>,
    pub ppi31: Peri<'static, peripherals::PPI_CH31>,
}

/// Start the MPSL, once, on the 32.768 kHz clock's source found at boot
/// (`lfclk.rs`). On the crystal, its accuracy is taken as ±250 ppm, the
/// tolerance the Product Specification requires of a crystal for a BLE
/// stack (32.768 kHz crystal oscillator, fTOL,LFXO,BLE): the fitted
/// crystal's own is not known, and a figure too small would narrow the
/// windows in which the controller listens for the phone. On the RC
/// oscillator, it is calibrated as often as the MPSL recommends, every 4 s
/// and at least every 8 s, which holds it to ±500 ppm (Product
/// Specification, LFRC).
pub fn mpsl(
    p: MpslParts,
    source: crate::lfclk::Source,
) -> &'static MultiprotocolServiceLayer<'static> {
    static MPSL: StaticCell<MultiprotocolServiceLayer<'static>> = StaticCell::new();
    let lfclk = match source {
        crate::lfclk::Source::Crystal { .. } => mpsl_raw::mpsl_clock_lfclk_cfg_t {
            source: mpsl_raw::MPSL_CLOCK_LF_SRC_XTAL as u8,
            rc_ctiv: 0,
            rc_temp_ctiv: 0,
            accuracy_ppm: mpsl_raw::MPSL_DEFAULT_CLOCK_ACCURACY_PPM as u16,
            skip_wait_lfclk_started: mpsl_raw::MPSL_DEFAULT_SKIP_WAIT_LFCLK_STARTED != 0,
        },
        crate::lfclk::Source::Rc => mpsl_raw::mpsl_clock_lfclk_cfg_t {
            source: mpsl_raw::MPSL_CLOCK_LF_SRC_RC as u8,
            rc_ctiv: mpsl_raw::MPSL_RECOMMENDED_RC_CTIV as u8,
            rc_temp_ctiv: mpsl_raw::MPSL_RECOMMENDED_RC_TEMP_CTIV as u8,
            accuracy_ppm: mpsl_raw::MPSL_WORST_CLOCK_ACCURACY_PPM as u16,
            skip_wait_lfclk_started: mpsl_raw::MPSL_DEFAULT_SKIP_WAIT_LFCLK_STARTED != 0,
        },
    };
    // One timeslot session: the stored state's flash writes (`store.rs`).
    static SESSIONS: StaticCell<nrf_mpsl::SessionMem<1>> = StaticCell::new();
    let parts = nrf_mpsl::Peripherals::new(p.rtc0, p.timer0, p.temp, p.ppi19, p.ppi30, p.ppi31);
    let mpsl = MultiprotocolServiceLayer::with_timeslots(
        parts,
        crate::Irqs,
        lfclk,
        SESSIONS.init(nrf_mpsl::SessionMem::new()),
    )
    .expect("the MPSL starts once, with its peripherals");
    MPSL.init(mpsl)
}

/// Build the controller: a peripheral that advertises, with the data
/// length extension (roadmap D119).
pub fn controller(
    p: sdc::Peripherals<'static>,
    rng: Peri<'static, peripherals::RNG>,
    mpsl: &'static MultiprotocolServiceLayer<'static>,
) -> Result<SoftdeviceController<'static>, sdc::Error> {
    static RNG: StaticCell<rng::Rng<'static, embassy_nrf::mode::Blocking>> = StaticCell::new();
    static MEM: StaticCell<sdc::Mem<SDC_MEM>> = StaticCell::new();
    let builder = sdc::Builder::new()?
        .support_adv()
        .support_peripheral()
        .support_dle_peripheral()
        .peripheral_count(CONNECTIONS as u8)?
        .buffer_cfg(LL_PAYLOAD, LL_PAYLOAD, LL_BUFFERS, LL_BUFFERS)?;
    let need = builder.required_memory()?;
    log::info!("ble: controller memory {} of {} bytes", need, SDC_MEM);
    crate::boot::note(|f| f.controller = Some((need, SDC_MEM)));
    builder.build(
        p,
        RNG.init(rng::Rng::new_blocking(rng)),
        mpsl,
        MEM.init(sdc::Mem::new()),
    )
}

/// What the phone asks of the node, one at a time: the link answers the
/// phone's write once the node has answered it.
// One request at a time lives in [`LINK`]; the small ones costing a
// message's room there is what a single place for any request takes.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Copy)]
pub enum Request {
    /// A message to send (spec "Phone link", Sending).
    Send {
        dst_pos: Position,
        dst_id: NodeId,
        len: u8,
        payload: [u8; MAX_PAYLOAD],
    },
    /// The app has the inbox's oldest message.
    Confirm(MsgId),
    /// Set or clear the map point.
    MapPoint(bark_link::MapPoint),
    /// The phone's time and UTC offset.
    Clock(bark_link::Clock),
}

/// What the node publishes and the phone reads, and the request waiting.
pub struct State {
    /// The status but for the countdown, which is read against `boot_ends`.
    pub status: Status,
    /// When the first rotation ends, while it runs.
    pub boot_ends: Option<Instant>,
    /// The inbox's oldest message: its ID, length and payload.
    pub inbox: Option<(MsgId, u8, [u8; MAX_PAYLOAD])>,
    /// The message last given back and the end of its wait, until the node
    /// takes another.
    pub given_back: Option<(MsgId, Instant)>,
    /// The phone's request, until the node answers it.
    pub request: Option<Request>,
    /// The node's answer, until the step that gave it has published what
    /// it changed ([`answer`]).
    answer: Option<bool>,
    /// What the phone was last told of the status, for when to tell it again.
    told: Status,
    /// The flags byte last logged.
    flags: Option<u8>,
}

/// The state both sides share.
pub static LINK: Mutex<CriticalSectionRawMutex, RefCell<State>> = Mutex::new(RefCell::new(State {
    status: Status::EMPTY,
    boot_ends: None,
    inbox: None,
    given_back: None,
    request: None,
    answer: None,
    told: Status::EMPTY,
    flags: None,
}));

/// Raised when a request waits for the node.
pub static FOR_CORE: Signal<CriticalSectionRawMutex, ()> = Signal::new();
/// The node's answer to the request: taken, or not.
static ANSWER: Signal<CriticalSectionRawMutex, bool> = Signal::new();
/// Raised when the phone is to be notified: the status or a message given
/// back.
static NOTIFY: Signal<CriticalSectionRawMutex, Notify> = Signal::new();
/// Raised once the image has confirmed itself (`update.rs`): the
/// controller starts only then, since the confirmation writes the flash
/// directly, which the controller's radio timing could not wait for.
pub static START: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// What to notify.
#[derive(Clone, Copy, Default)]
struct Notify {
    status: bool,
    given_back: bool,
}

fn notify(n: Notify) {
    let both = match NOTIFY.try_take() {
        Some(o) => Notify {
            status: o.status || n.status,
            given_back: o.given_back || n.given_back,
        },
        None => n,
    };
    NOTIFY.signal(both);
}

/// The battery step the status is notified at: 1 % of a cell's charge
/// between 20 % and full, from the open-circuit voltages the core's
/// levels use (`BatteryLevels::DESIGN`, `BATTERY_FULL_MV`), rounded up.
const BATTERY_STEP_MV: u16 = (bark_proto::node::BATTERY_FULL_MV
    - bark_proto::BatteryLevels::DESIGN.low_mv)
    .div_ceil(100 - 20);

/// The node's side: publish the status and the inbox's oldest message
/// after a step, and notify the phone of a change of the flags, the
/// relays, the inbox's count or the address, or of the battery by a step
/// (spec "Phone link", Status).
pub fn publish(status: Status, inbox: Option<(MsgId, &[u8])>) {
    let changed = LINK.lock(|l| {
        let mut l = l.borrow_mut();
        l.inbox = inbox.map(|(id, p)| {
            let mut payload = [0; MAX_PAYLOAD];
            payload[..p.len()].copy_from_slice(p);
            (id, p.len() as u8, payload)
        });
        l.status = status;
        let told = l.told;
        let quiet = |s: Status| Status {
            boot_left_s: 0,
            battery_mv: 0,
            ..s
        };
        let changed = quiet(told) != quiet(status)
            || told.battery_mv.abs_diff(status.battery_mv) >= BATTERY_STEP_MV;
        if changed {
            l.told = status;
        }
        // The flags as the phone reads them, at the first publish and each
        // change, for the bench to check reads against.
        let flags = status.encode()[0];
        let new_flags = l.flags != Some(flags);
        l.flags = Some(flags);
        (changed, new_flags.then_some(flags))
    });
    let (changed, flags) = changed;
    if let Some(flags) = flags {
        log::info!(
            "ble: status flags={:#04x} relays={} inbox={} address={}",
            flags,
            status.relays,
            status.inbox,
            status.address
        );
    }
    if changed {
        notify(Notify {
            status: true,
            given_back: false,
        });
    }
}

/// The node's side: the first rotation's end, from `AppEvent::Boot`.
pub fn booting(ends: Option<Instant>) {
    LINK.lock(|l| l.borrow_mut().boot_ends = ends);
}

/// The node's side: its application's message was given back.
pub fn given_back(msg_id: MsgId, retry_at: Instant) {
    LINK.lock(|l| l.borrow_mut().given_back = Some((msg_id, retry_at)));
    notify(Notify {
        status: false,
        given_back: true,
    });
}

/// Whether a request waits for the node.
pub fn request_waiting() -> bool {
    LINK.lock(|l| l.borrow().request.is_some())
}

/// The node's side: take the request waiting if `f` takes it, with the
/// answer `f` gives, which goes to the phone after the step ([`answer`]).
pub fn take_request<R>(f: impl FnOnce(&Request) -> Option<(R, bool)>) -> Option<R> {
    let taken = LINK.lock(|l| {
        let mut l = l.borrow_mut();
        let (r, answer) = f(l.request.as_ref()?)?;
        if let Some(Request::Send { .. }) = l.request {
            if answer {
                l.given_back = None;
            }
        }
        l.request = None;
        l.answer = Some(answer);
        Some(r)
    });
    taken
}

/// The node's side, after a step and its [`publish`]: answer the request
/// the step took, or one it was woken for and did not take, no; so that
/// what the phone reads after its write is answered shows the write.
pub fn answer(woken: bool) {
    let answer = LINK.lock(|l| {
        let mut l = l.borrow_mut();
        if woken && l.request.take().is_some() {
            l.answer = Some(false);
        }
        l.answer.take()
    });
    if let Some(a) = answer {
        ANSWER.signal(a);
    }
}

/// How long the link waits for the node's answer: half the ATT
/// transaction's 30 s (Vol 3 Part F §3.3.3), so that the phone has its
/// answer before it gives up. The node answers within one step of its
/// driver, at most a frame's airtime and a flash erase.
const ANSWER_WAIT: embassy_time::Duration = embassy_time::Duration::from_secs(15);

/// Hand `request` to the node and wait for its answer.
async fn ask(request: Request) -> bool {
    ANSWER.reset();
    LINK.lock(|l| l.borrow_mut().request = Some(request));
    FOR_CORE.signal(());
    match embassy_time::with_timeout(ANSWER_WAIT, ANSWER.wait()).await {
        Ok(answer) => answer,
        Err(_) => {
            LINK.lock(|l| l.borrow_mut().request = None);
            log::warn!("ble: the node did not answer");
            false
        }
    }
}

fn now() -> Instant {
    Instant::from_micros(embassy_time::Instant::now().as_micros())
}

/// The status as the phone reads it now.
fn status_now() -> Status {
    LINK.lock(|l| {
        let l = l.borrow();
        Status {
            ready: l.status.ready && l.request.is_none(),
            boot_left_s: l.boot_ends.map_or(0, |e| seconds_left(now(), e)),
            ..l.status
        }
    })
}

fn given_back_now() -> heapless::Vec<u8, { GivenBack::LEN }> {
    LINK.lock(|l| match l.borrow().given_back {
        Some((msg_id, retry_at)) => {
            let g = GivenBack {
                msg_id,
                wait_s: seconds_left(now(), retry_at),
            };
            heapless::Vec::from_slice(&g.encode()).expect("it fits")
        }
        None => heapless::Vec::new(),
    })
}

/// The node's time as the phone reads it (`utc.rs`).
fn clock_now() -> [u8; bark_link::NodeTime::LEN] {
    use bark_driver::utc::Source;
    use bark_link::TimeSource;
    let t = crate::utc::time();
    bark_link::NodeTime {
        // Seconds since 1970 fit 32 bits until 2106.
        utc_s: t.utc_s.unwrap_or(0) as u32,
        offset_min: t.offset_min,
        source: match t.source {
            None => TimeSource::None,
            Some(Source::Gps) => TimeSource::Gps,
            Some(Source::Phone) => TimeSource::Phone,
        },
    }
    .encode()
}

fn inbox_now() -> heapless::Vec<u8, { Message::MAX_LEN }> {
    LINK.lock(|l| match &l.borrow().inbox {
        Some((msg_id, len, payload)) => {
            let mut b = [0; Message::MAX_LEN];
            let n = Message {
                msg_id: *msg_id,
                payload: &payload[..*len as usize],
            }
            .encode(&mut b);
            heapless::Vec::from_slice(&b[..n]).expect("it fits")
        }
        None => heapless::Vec::new(),
    })
}

/// Run the host and the advertising, for ever.
pub async fn run(controller: SoftdeviceController<'static>, id: NodeId, image: u32) {
    static RESOURCES: StaticCell<HostResources<DefaultPacketPool, CONNECTIONS, CHANNELS>> =
        StaticCell::new();
    let resources = RESOURCES.init(HostResources::new());
    let stack = trouble_host::new(controller, resources)
        .set_random_address(address())
        .build();
    let mut peripheral = stack.peripheral();
    let mut runner = stack.runner();
    let name = bark_link::name(id);
    let name = core::str::from_utf8(&name).expect("the name is ASCII");
    let server = Server::new_with_config(GapConfig::Peripheral(PeripheralConfig {
        name,
        appearance: &appearance::UNKNOWN,
    }))
    .expect("the attribute table fits");
    let node = Node {
        version: bark_link::VERSION,
        id,
        image,
    };
    let _ = server.set(&server.link.node, &node.encode());

    let host = async {
        loop {
            if let Err(e) = runner.run().await {
                log::error!("ble: host {:?}", e);
            }
        }
    };
    let link = async {
        loop {
            match advertise(&mut peripheral, &server, name).await {
                Ok(conn) => serve(&server, &conn).await,
                Err(e) => {
                    log::error!("ble: advertising {:?}", e);
                    embassy_time::Timer::after(ADV_RETRY).await;
                }
            }
        }
    };
    join(host, link).await;
}

/// The node's random static address: the factory's DEVICEADDR (nRF52840
/// Product Specification, FICR), its top two bits set as a static address
/// has them (Vol 6 Part B §1.3.2.1).
fn address() -> Address {
    let ficr = embassy_nrf::pac::FICR;
    let low = ficr.deviceaddr(0).read().to_le_bytes();
    let high = ficr.deviceaddr(1).read().to_le_bytes();
    Address::random([
        low[0],
        low[1],
        low[2],
        low[3],
        high[0],
        high[1] | 0b1100_0000,
    ])
}

/// The pause after advertising fails to start, so that a controller that
/// refuses keeps the log readable and the executor free.
const ADV_RETRY: embassy_time::Duration = embassy_time::Duration::from_secs(1);

/// Advertise until a phone connects (roadmap D120).
async fn advertise<'v, 's>(
    peripheral: &mut Peripheral<'v, SoftdeviceController<'static>, DefaultPacketPool>,
    server: &'s Server<'v>,
    name: &str,
) -> Result<GattConnection<'v, 's, DefaultPacketPool>, BleHostError<sdc::Error>> {
    let mut adv = [0; ADV_DATA];
    let adv_len = AdStructure::encode_slice(
        &[
            AdStructure::Flags(LE_GENERAL_DISCOVERABLE | BR_EDR_NOT_SUPPORTED),
            AdStructure::CompleteServiceUuids128(&[le_bytes(uuid::SERVICE)]),
        ],
        &mut adv,
    )?;
    let mut scan = [0; ADV_DATA];
    let scan_len = AdStructure::encode_slice(
        &[AdStructure::CompleteLocalName(name.as_bytes())],
        &mut scan,
    )?;
    let params = AdvertisementParameters {
        interval_min: ADV_INTERVAL,
        interval_max: ADV_INTERVAL,
        ..Default::default()
    };
    let advertiser = peripheral
        .advertise(
            &params,
            Advertisement::ConnectableScannableUndirected {
                adv_data: &adv[..adv_len],
                scan_data: &scan[..scan_len],
            },
        )
        .await?;
    log::info!("ble: advertising every {} us", ADV_UNITS * 625);
    let conn = advertiser.accept().await?;
    let p = conn.params();
    log::info!(
        "ble: connected peer={:?} interval_us={} latency={} timeout_ms={}",
        conn.peer_address(),
        p.conn_interval.as_micros(),
        p.peripheral_latency,
        p.supervision_timeout.as_millis()
    );
    Ok(conn.with_attribute_server(server)?)
}

/// Serve one connection until it ends: answer its reads and writes, and
/// notify what changes.
async fn serve(server: &Server<'_>, conn: &GattConnection<'_, '_, DefaultPacketPool>) {
    // What changed while no phone was connected is read, not notified.
    NOTIFY.reset();
    let link = &server.link;
    refresh(server, conn);
    loop {
        let event = match select(conn.next(), NOTIFY.wait()).await {
            Either::First(e) => e,
            Either::Second(n) => {
                refresh(server, conn);
                if n.status {
                    let _ = link.status.notify(conn, &status_now().encode(), true).await;
                }
                if n.given_back {
                    let _ = link.given_back.notify(conn, &given_back_now(), true).await;
                }
                continue;
            }
        };
        let event = match event {
            GattConnectionEvent::Disconnected { reason } => {
                log::info!("ble: disconnected {:?}", reason);
                return;
            }
            GattConnectionEvent::Gatt { event } => event,
            GattConnectionEvent::ConnectionParamsUpdated {
                conn_interval,
                peripheral_latency,
                supervision_timeout,
            } => {
                log::info!(
                    "ble: parameters interval_us={} latency={} timeout_ms={}",
                    conn_interval.as_micros(),
                    peripheral_latency,
                    supervision_timeout.as_millis()
                );
                continue;
            }
            GattConnectionEvent::PhyUpdated { tx_phy, rx_phy } => {
                log::info!("ble: phy tx={:?} rx={:?}", tx_phy, rx_phy);
                continue;
            }
            GattConnectionEvent::DataLengthUpdated {
                max_tx_octets,
                max_rx_octets,
                ..
            } => {
                log::info!("ble: data length tx={} rx={}", max_tx_octets, max_rx_octets);
                continue;
            }
            _ => {
                log::info!("ble: other connection event");
                continue;
            }
        };
        log_request(&event.payload().incoming());
        // The attribute server answers every kind of read from the values
        // it holds, a read by handle or by type, of one value or several:
        // they are brought up to date before each request is answered, but
        // for the later parts of a long read, which must read on in the
        // value its first part began.
        let continued = matches!(
            event.payload().incoming(),
            AttClient::Request(AttReq::ReadBlob { offset, .. }) if offset > 0
        );
        if !continued {
            refresh(server, conn);
        }
        let reply = match event {
            GattEvent::Write(w) => {
                let h = w.handle();
                let answer = if h == link.send.handle {
                    Some(write_send(&w).await)
                } else if h == link.inbox.handle {
                    Some(write_confirm(&w).await)
                } else if h == link.map_point.handle {
                    Some(write_map_point(&w).await)
                } else if h == link.clock.handle {
                    Some(write_clock(&w).await)
                } else {
                    // Not a request: the server takes it by its own rules,
                    // a subscription's descriptor above all.
                    None
                };
                match answer {
                    Some(Ok(())) => w.accept_unprocessed(),
                    Some(Err(e)) => w.reject(e),
                    None => w.accept(),
                }
            }
            other => other.accept(),
        };
        if let Ok(reply) = reply {
            reply.send().await;
        }
    }
}

/// Log an ATT request from the phone, the written bytes only by count, so
/// that the bench sees each step of a connection.
fn log_request(att: &AttClient<'_>) {
    match att {
        AttClient::Request(AttReq::Write { handle, data })
        | AttClient::Command(AttCmd::Write { handle, data }) => {
            log::info!("ble: att write handle={} len={}", handle, data.len())
        }
        AttClient::Request(AttReq::PrepareWrite {
            handle,
            offset,
            value,
        }) => log::info!(
            "ble: att prepare write handle={} offset={} len={}",
            handle,
            offset,
            value.len()
        ),
        other => log::info!("ble: att {:?}", other),
    }
}

/// Put what the node publishes now into the attribute server's values.
fn refresh(server: &Server<'_>, conn: &GattConnection<'_, '_, DefaultPacketPool>) {
    let link = &server.link;
    let status = status_now().encode();
    for e in [
        conn.set(&link.status, &status).err(),
        conn.set(&link.inbox, &inbox_now()).err(),
        conn.set(&link.given_back, &given_back_now()).err(),
        conn.set(&link.clock, &clock_now()).err(),
    ]
    .into_iter()
    .flatten()
    {
        log::error!("ble: setting a value {:?}", e);
    }
}

fn malformed(m: Malformed) -> AttErrorCode {
    AttErrorCode::new(m.att_error())
}

const NOT_READY: AttErrorCode = AttErrorCode::new(bark_link::att::NOT_READY);

async fn write_send(w: &WriteEvent<'_, '_, DefaultPacketPool>) -> Result<(), AttErrorCode> {
    let request = w.with_data(|offset, data| {
        if offset != 0 {
            return Err(AttErrorCode::INVALID_OFFSET);
        }
        let s = bark_link::Send::decode(data).map_err(malformed)?;
        let mut payload = [0; MAX_PAYLOAD];
        payload[..s.payload.len()].copy_from_slice(s.payload);
        Ok(Request::Send {
            dst_pos: s.dst_pos,
            dst_id: s.dst_id,
            len: s.payload.len() as u8,
            payload,
        })
    })?;
    if !status_now().ready {
        return Err(NOT_READY);
    }
    match ask(request).await {
        true => Ok(()),
        false => Err(NOT_READY),
    }
}

async fn write_confirm(w: &WriteEvent<'_, '_, DefaultPacketPool>) -> Result<(), AttErrorCode> {
    let c = w.with_data(|offset, data| match offset {
        0 => bark_link::Confirm::decode(data).map_err(malformed),
        _ => Err(AttErrorCode::INVALID_OFFSET),
    })?;
    match ask(Request::Confirm(c.0)).await {
        true => Ok(()),
        false => Err(malformed(Malformed::Value)),
    }
}

async fn write_map_point(w: &WriteEvent<'_, '_, DefaultPacketPool>) -> Result<(), AttErrorCode> {
    let m = w.with_data(|offset, data| match offset {
        0 => bark_link::MapPoint::decode(data).map_err(malformed),
        _ => Err(AttErrorCode::INVALID_OFFSET),
    })?;
    ask(Request::MapPoint(m)).await;
    Ok(())
}

async fn write_clock(w: &WriteEvent<'_, '_, DefaultPacketPool>) -> Result<(), AttErrorCode> {
    let c = w.with_data(|offset, data| match offset {
        0 => bark_link::Clock::decode(data).map_err(malformed),
        _ => Err(AttErrorCode::INVALID_OFFSET),
    })?;
    ask(Request::Clock(c)).await;
    Ok(())
}
