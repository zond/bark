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
//! Not yet wired to the node: the service answers reads with empty
//! values and refuses every write as not ready (roadmap D117).

use bark_link::{le_bytes, uuid, Node, Status, ATT_MTU};
use embassy_futures::join::join;
use embassy_nrf::{peripherals, rng, Peri};
use nrf_mpsl::{raw as mpsl_raw, MultiprotocolServiceLayer};
use nrf_sdc::{self as sdc, SoftdeviceController};
use static_cell::StaticCell;
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

/// The advertising interval, 1022.5 ms, in the 0.625 ms units of the
/// advertising parameters (Vol 4 Part E §7.8.5): 1636 (roadmap D120).
const ADV_INTERVAL: embassy_time::Duration = embassy_time::Duration::from_micros(1_022_500);

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
    pub send: heapless::Vec<u8, { bark_link::Send::MAX_LEN }>,
    #[characteristic(uuid = le_bytes(uuid::INBOX), read, write)]
    pub inbox: heapless::Vec<u8, { bark_link::Message::MAX_LEN }>,
    #[characteristic(uuid = le_bytes(uuid::GIVEN_BACK), read, notify)]
    pub given_back: heapless::Vec<u8, { bark_link::GivenBack::LEN }>,
    #[characteristic(uuid = le_bytes(uuid::MAP_POINT), write)]
    pub map_point: heapless::Vec<u8, { bark_link::MapPoint::MAX_LEN }>,
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

/// Start the MPSL, once. The 32.768 kHz clock stays the internal RC
/// oscillator (`main.rs`), calibrated as often as the MPSL recommends, at
/// the nRF52840's ±500 ppm after calibration (Product Specification,
/// LFRC).
pub fn mpsl(p: MpslParts) -> &'static MultiprotocolServiceLayer<'static> {
    static MPSL: StaticCell<MultiprotocolServiceLayer<'static>> = StaticCell::new();
    let lfclk = mpsl_raw::mpsl_clock_lfclk_cfg_t {
        source: mpsl_raw::MPSL_CLOCK_LF_SRC_RC as u8,
        rc_ctiv: mpsl_raw::MPSL_RECOMMENDED_RC_CTIV as u8,
        rc_temp_ctiv: mpsl_raw::MPSL_RECOMMENDED_RC_TEMP_CTIV as u8,
        accuracy_ppm: mpsl_raw::MPSL_WORST_CLOCK_ACCURACY_PPM as u16,
        skip_wait_lfclk_started: mpsl_raw::MPSL_DEFAULT_SKIP_WAIT_LFCLK_STARTED != 0,
    };
    let parts = nrf_mpsl::Peripherals::new(p.rtc0, p.timer0, p.temp, p.ppi19, p.ppi30, p.ppi31);
    let mpsl = MultiprotocolServiceLayer::new(parts, crate::Irqs, lfclk)
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
    log::info!(
        "ble: controller memory {} of {} bytes",
        builder.required_memory()?,
        SDC_MEM
    );
    builder.build(
        p,
        RNG.init(rng::Rng::new_blocking(rng)),
        mpsl,
        MEM.init(sdc::Mem::new()),
    )
}

/// Run the host and the advertising, for ever.
pub async fn run(controller: SoftdeviceController<'static>, id: bark_proto::NodeId, image: u32) {
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
                Ok(conn) => serve(&conn).await,
                Err(e) => log::error!("ble: advertising {:?}", e),
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
    let conn = advertiser.accept().await?.with_attribute_server(server)?;
    log::info!("ble: connected");
    Ok(conn)
}

/// Serve one connection until it ends.
async fn serve(conn: &GattConnection<'_, '_, DefaultPacketPool>) {
    loop {
        match conn.next().await {
            GattConnectionEvent::Disconnected { reason } => {
                log::info!("ble: disconnected {:?}", reason);
                return;
            }
            GattConnectionEvent::Gatt { event } => {
                let reply = match event {
                    GattEvent::Write(w) => w.reject(AttErrorCode::new(bark_link::att::NOT_READY)),
                    other => other.accept(),
                };
                if let Ok(reply) = reply {
                    reply.send().await;
                }
            }
            _ => {}
        }
    }
}
