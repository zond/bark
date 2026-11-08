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
//!
//! The link is the paired phone's (spec "Phone link", Pairing; roadmap
//! D135): every characteristic of the service needs a link encrypted
//! with an authenticated key, the node shows the passkey of a pairing on
//! its screen, and it keeps one bond, in flash (`store.rs`), which a new
//! pairing replaces. On a public node guests share it (spec "Phone link",
//! Public; roadmap D150): up to [`LINKS`] links at once, each served by
//! a slot of its own ([`slot`]), the characteristics a guest needs open
//! on any of them ([`allowed`]), the payloads for the node's application
//! handed out by their recipient tags (`node.rs`), and the one message
//! the node holds for its phones taken in turn ([`Turn`]).
//!
//! The paired phone keeps the attribute table it discovered; the node
//! tells it when the table has changed since, by the link version the
//! bond records ([`tell_of_change`]; spec "Phone link", Changes to the
//! table).
//!
//! The phone stages a new image through Update and Image, which this
//! task serves itself, across its connections (`update::Phone`; spec
//! "Phone link", Updates).

use core::cell::RefCell;

use bark_link::bond::Bond;
use bark_link::neighbours;
use bark_link::update::{Command, Piece, Progress, Refusal};
use bark_link::{
    le_bytes, seconds_left, uuid, GivenBack, Malformed, Message, Node, Status, ATT_MTU,
};
use bark_proto::tables::neighbours::NEIGHBOURS;
use bark_proto::wire::MAX_PAYLOAD;
use bark_proto::{Instant, MsgId, NodeId, Position};
use bark_screen::Phone;
use embassy_futures::select::{select, select3, Either, Either3};
use embassy_nrf::{peripherals, rng, Peri};
use embassy_sync::blocking_mutex::raw::{CriticalSectionRawMutex, NoopRawMutex};
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::signal::Signal;
use nrf_mpsl::{raw as mpsl_raw, MultiprotocolServiceLayer};
use nrf_sdc::{self as sdc, SoftdeviceController};
use static_cell::StaticCell;
use trouble_host::att::{AttClient, AttCmd, AttReq};
use trouble_host::prelude::*;

/// Links at once: one on a node that is not public, the owner's and two
/// guests' on a public one (spec "Phone link", Public; roadmap D150).
/// Each costs 5.6 KB of static RAM, the controller's [`PER_LINK`], the
/// host's 1.2 KB and its slot's task, all taken from the stack; three
/// leave it 17.9 KB against the 11.8 KB of the deepest poll, the phone
/// link's set-up, and a fourth would leave 12.4 KB, less than that
/// set-up then takes, 14.2 KB.
pub const LINKS: usize = 3;
/// L2CAP channels: the signalling channel and the ATT channel.
const CHANNELS: usize = 2;
/// Advertising sets: the one advertisement.
const ADV_SETS: usize = 1;
/// Bonds the host holds: the one kept, and the one a pairing adds before
/// [`settle`] takes the other away.
const BONDS: usize = 2;
/// Link-layer buffers each way: the controller's default for transmit,
/// three (`SDC_DEFAULT_TX_PACKET_COUNT`, `sdc.h`), and as many to receive.
const LL_BUFFERS: u8 = 3;
/// The link-layer payload: the ATT MTU and the 4-byte L2CAP header in one
/// packet (roadmap D119).
const LL_PAYLOAD: u16 = ATT_MTU + 4;

// The controller's memory for the peripheral links, one advertising set of
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
const SDC_MEM: usize = LINKS * PER_LINK + LINKS_SHARED + ADV_SET + FAL;

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
/// The GATT server: the GAP and GATT services, which [`server`] builds,
/// and the link's.
#[gatt_server(attribute_table_size = GAP_GATT_ATTRIBUTES + Link::ATTRIBUTE_COUNT, connections_max = LINKS)]
pub struct Server {
    pub link: Link,
}

/// Attributes of the GAP and GATT services: each service's declaration;
/// the device name and appearance, a declaration and a value each; Service
/// Changed, a declaration, a value and its descriptor; the database hash,
/// a declaration and a value.
const GAP_GATT_ATTRIBUTES: usize = 1 + 2 * 2 + 1 + 3 + 2;

/// The GATT service's characteristics (Vol 3 Part G §7).
struct Gatt {
    /// Service Changed, indicated to a bonded phone whose table is old
    /// ([`tell_of_change`]).
    service_changed: Characteristic<[u8; 4]>,
}

/// Every handle, the range Service Changed names (Vol 3 Part G §7.1):
/// the phone discovers the whole table again, which costs it a few round
/// trips once per version.
const ALL_HANDLES: [u8; 4] = {
    let [a, b] = 0x0001u16.to_le_bytes();
    let [c, d] = 0xffffu16.to_le_bytes();
    [a, b, c, d]
};

/// A client characteristic configuration descriptor, whose value the
/// server keeps for each client (Vol 3 Part G §3.3.3.3).
struct Cccd(u16);

impl AttributeHandle for Cccd {
    type Value = [u8; 2];
    fn handle(&self) -> u16 {
        self.0
    }
}

/// Build the attribute table: the GAP service, with `name`; the GATT
/// service, with Service Changed (Vol 3 Part G §7.1) and the database
/// hash (Bluetooth Core 5.1, Vol 3 Part G §7.3); and the link's
/// service.
///
/// The host's own GAP builder leaves the GATT service empty, which tells
/// a phone that the table never changes, so the phone would keep a table
/// it cached for good. The hash lets a phone that reads it see a change
/// without waiting for an indication.
fn server(name: &str) -> (Server<'_>, Gatt) {
    let mut table = AttributeTable::new();
    let mut gap = table.add_service(Service::new(service::GAP));
    gap.add_characteristic_ro(characteristic::DEVICE_NAME, name);
    gap.add_characteristic_ro(characteristic::APPEARANCE, &appearance::UNKNOWN);
    gap.build();
    let mut gatt = table.add_service(Service::new(service::GATT));
    let service_changed = gatt
        .add_characteristic_small(
            characteristic::SERVICE_CHANGED,
            [CharacteristicProp::Indicate],
            ALL_HANDLES,
        )
        .build();
    // Readable by any link: discovery shows the same table to anyone.
    static HASH: StaticCell<[u8; 16]> = StaticCell::new();
    let hash = gatt
        .add_characteristic_ro(characteristic::DATABASE_HASH, &[0u8; 16])
        .build();
    gatt.build();
    let server = Server::new(table);
    // The hash covers the hash's declaration but not its value (5.1,
    // Vol 3 Part G §7.3.1), so it is taken once the table is whole.
    let value = HASH.init(server.table().hash().to_le_bytes());
    hash.set_ro(&server, value)
        .unwrap_or_else(|e| panic!("the database hash: {}", HostError(&e)));
    (server, Gatt { service_changed })
}

/// The service and its characteristics, each with room for its value's
/// longest form (spec "Phone link"), and each open only to a link
/// encrypted with an authenticated key, which only a pairing by the code
/// on the node's screen makes: any other read, write or subscription is
/// answered with ATT's insufficient authentication (Vol 3 Part F
/// §3.4.1.1), the phone's cue to pair. Those a guest needs open on a
/// public node, which the host cannot tell: they are open here, and
/// [`allowed`] answers for them.
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
    #[characteristic(uuid = le_bytes(uuid::MAP_POINT), write, permissions(authenticated))]
    pub map_point: (),
    #[characteristic(uuid = le_bytes(uuid::CLOCK), read, write, permissions(authenticated))]
    pub clock: [u8; bark_link::NodeTime::LEN],
    #[characteristic(uuid = le_bytes(uuid::HOME), read, write, permissions(authenticated))]
    pub home: [u8; bark_link::home::Home::LEN],
    #[characteristic(uuid = le_bytes(uuid::NEIGHBOURS), read, write, permissions(authenticated))]
    pub neighbours: heapless::Vec<u8, { neighbours::MAX_LEN }>,
    #[characteristic(uuid = le_bytes(uuid::UPDATE), read, write, permissions(authenticated))]
    pub update: heapless::Vec<u8, { Progress::LEN }>,
    #[characteristic(uuid = le_bytes(uuid::IMAGE), write, write_without_response, permissions(authenticated))]
    pub image: (),
    #[characteristic(uuid = le_bytes(uuid::RECIPIENT), write)]
    pub recipient: (),
    #[characteristic(uuid = le_bytes(uuid::PUBLIC), read, write, permissions(authenticated))]
    pub public: [u8; bark_link::Public::LEN],
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
/// (`bark_image::lfclk`): it starts the clock again on any other source,
/// which a running watchdog's hold on the clock does not allow. On the
/// crystal, its accuracy is taken as ±250 ppm, the tolerance the Product
/// Specification requires of a crystal for a BLE stack (32.768 kHz
/// crystal oscillator, fTOL,LFXO,BLE): the fitted
/// crystal's own is not known, and a figure too small would narrow the
/// windows in which the controller listens for the phone. On the RC
/// oscillator, it is calibrated as often as the MPSL recommends, every 4 s
/// and at least every 8 s, which holds it to ±500 ppm (Product
/// Specification, LFRC).
pub fn mpsl(
    p: MpslParts,
    source: bark_image::lfclk::Source,
) -> &'static MultiprotocolServiceLayer<'static> {
    static MPSL: StaticCell<MultiprotocolServiceLayer<'static>> = StaticCell::new();
    let lfclk = match source {
        bark_image::lfclk::Source::Crystal => mpsl_raw::mpsl_clock_lfclk_cfg_t {
            source: mpsl_raw::MPSL_CLOCK_LF_SRC_XTAL as u8,
            rc_ctiv: 0,
            rc_temp_ctiv: 0,
            accuracy_ppm: mpsl_raw::MPSL_DEFAULT_CLOCK_ACCURACY_PPM as u16,
            skip_wait_lfclk_started: mpsl_raw::MPSL_DEFAULT_SKIP_WAIT_LFCLK_STARTED != 0,
        },
        bark_image::lfclk::Source::Rc => mpsl_raw::mpsl_clock_lfclk_cfg_t {
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
    // The errno as a number: the error's `Debug` names every errno.
    .unwrap_or_else(|e| panic!("the MPSL did not start: {}", i32::from(e)));
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
    // Privacy: once a phone bonds, the host puts its identity key in the
    // controller's resolving list, and its run ends if a command of the
    // list fails; the controller takes them only with privacy (`sdc.h`,
    // `sdc_support_le_privacy`).
    let builder = sdc::Builder::new()?
        .support_adv()
        .support_peripheral()
        .support_dle_peripheral()
        .support_le_privacy()
        .peripheral_count(LINKS as u8)?
        .buffer_cfg(LL_PAYLOAD, LL_PAYLOAD, LL_BUFFERS, LL_BUFFERS)?;
    let need = builder.required_memory()?;
    log::info!("ble: controller memory {} of {} bytes", need, SDC_MEM);
    crate::boot::note(|f| f.controller = Some((need, SDC_MEM)));
    // Bias correction makes the generator's bits uniform (nRF52840 Product
    // Specification, RNG); it is off at reset. The host's keys and nonces
    // come from it through LE Rand, with no generator of the host's own
    // between.
    let rng = rng::Rng::new_blocking(rng);
    rng.set_bias_correction(true);
    builder.build(p, RNG.init(rng), mpsl, MEM.init(sdc::Mem::new()))
}

/// The BLE host's AES-128 (`vendor/README.md`), on the chip's AES block.
/// While the controller is enabled, the block is reached only through the
/// MPSL's function, which shares it with the controller (`sdc_soc.h`).
#[no_mangle]
fn trouble_host_aes128(key: &[u8; 16], block: &mut [u8; 16]) {
    let clear = *block;
    // SAFETY: the MPSL is started in `main` before the host exists, and the
    // three buffers are 16 bytes each, as the function reads and writes.
    // `clear` is on the stack, in the RAM the block reads directly.
    unsafe {
        mpsl_raw::mpsl_ecb_block_encrypt_extended(
            key.as_ptr(),
            clear.as_ptr(),
            block.as_mut_ptr(),
            mpsl_raw::MPSL_ECB_CLEARTEXT_IN_LOCAL_RAM,
        )
    }
}

/// The BLE host's random numbers (`vendor/README.md`), from the
/// controller's LE Rand (Bluetooth Core Vol 4 Part E §7.8.23), which draws
/// on the chip's generator given to the controller in [`controller`] and is
/// specified fit for keys (Vol 2 Part H §2). The host draws them in its
/// pairing's synchronous steps, so the controller's command is called
/// directly, not through the host's asynchronous HCI path.
#[no_mangle]
fn trouble_host_random(dest: &mut [u8]) {
    for chunk in dest.chunks_mut(8) {
        let mut out = sdc::raw::sdc_hci_cmd_le_rand_return_t { random_number: 0 };
        // SAFETY: the controller is built before the host exists, and its
        // commands are synchronous calls from thread mode, so none is in
        // progress when this one starts.
        let status = unsafe { sdc::raw::sdc_hci_cmd_le_rand(&mut out) };
        // A key from a failed draw would be guessable.
        assert!(status == 0, "LE Rand failed: {status:#x}");
        chunk.copy_from_slice(&out.random_number.to_le_bytes()[..chunk.len()]);
    }
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

/// Which phone a request is from: its link's slot, and whether it is the
/// paired phone's link, encrypted with the bond.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct From {
    pub slot: usize,
    pub owner: bool,
}

/// A phone connected in a slot.
#[derive(Clone, Copy)]
struct Peer {
    /// A link is up in the slot.
    live: bool,
    /// Counts the slot's links, so that a message given back is not handed
    /// to a later phone in the slot.
    link: u32,
    /// The link is encrypted with the bond: the paired phone's.
    owner: bool,
    /// The recipient tag it wrote (spec "Phone link", Public).
    tag: Option<u8>,
    /// A guest's message given back, and the end of its wait; the paired
    /// phone's is [`State::given_back`].
    given_back: Option<(MsgId, Instant)>,
    /// The slot it reads the neighbour table from (spec "Phone link",
    /// Neighbours); 0 at each connection.
    neighbours_from: u8,
}

impl Peer {
    const NONE: Peer = Peer {
        live: false,
        link: 0,
        owner: false,
        tag: None,
        given_back: None,
        neighbours_from: 0,
    };
}

/// What the node publishes and the phones read, and the request waiting.
pub struct State {
    /// The status but for the countdown, which is read against `boot_ends`,
    /// and what each phone reads of it alone.
    pub status: Status,
    /// When the first rotation ends, while it runs.
    pub boot_ends: Option<Instant>,
    /// What the paired phone's Inbox shows: the core's inbox's oldest
    /// message, or the ring's, its ID, length and payload.
    pub inbox: Option<(MsgId, u8, [u8; MAX_PAYLOAD])>,
    /// Of a ring, the item the inbox shows: its number and ID
    /// (`mailbox.rs`); `None` while the inbox is the core's.
    pub ring_head: Option<(u32, MsgId)>,
    /// On a public node, the core's inbox's oldest message while it waits
    /// for guests, and the slots of the guests it waits for (`node.rs`).
    guests: Option<(MsgId, u8, [u8; MAX_PAYLOAD])>,
    guests_for: u8,
    /// The paired phone's message last given back and the end of its
    /// wait, until the node takes another; kept across its links.
    pub given_back: Option<(MsgId, Instant)>,
    /// The phones, by their links' slots.
    phones: [Peer; LINKS],
    /// The message the node took last from a phone: its ID, slot and link,
    /// to give it back to that phone.
    took: Option<(MsgId, From, u32)>,
    /// The phone whose turn it is ([`Turn`]).
    turn: Turn,
    /// The node is public, and the paired phone's recipient tag, as the
    /// bond keeps them.
    public: bool,
    owner_tag: Option<u8>,
    /// The phone's request, until the node answers it.
    pub request: Option<(From, Request)>,
    /// The node's answer, until the step that gave it has published what
    /// it changed ([`answer`]).
    answer: Option<bool>,
    /// What the phones were last told of the status, for when to tell
    /// them again.
    told: Status,
    /// The flags byte last logged.
    flags: Option<u8>,
}

/// Whose turn it is at the one message the node holds for its phones
/// (spec "Phone link", Public): Ready is that phone's alone. The turn goes
/// to the next phone, in the order of the links' slots, when the node
/// takes the phone's message, when its link ends, or when it has had
/// Ready for a ban and not written.
#[derive(Clone, Copy)]
struct Turn {
    /// The phone's slot.
    slot: Option<usize>,
    /// Since when it has had Ready.
    since: Option<Instant>,
}

/// How long a turn lasts unused, a ban: the shortest wait the protocol
/// tells, as nothing in it acts sooner (spec "Bans").
const BAN: bark_proto::Duration = bark_proto::tables::budget::BAN;

impl State {
    /// The next phone after `after` in the order of the links' slots, or
    /// the first; `after` itself if it is the only one.
    fn next(&self, after: Option<usize>) -> Option<usize> {
        let from = after.map_or(0, |a| a + 1);
        (from..from + LINKS)
            .map(|i| i % LINKS)
            .find(|&i| self.phones[i].live)
    }

    /// Pass the turn on from the phone that has it.
    fn pass(&mut self) {
        self.turn = Turn {
            slot: self.next(self.turn.slot),
            since: None,
        };
    }

    /// Move the turn on to `now`: to a phone if none has it or its link
    /// has ended, and on from one that has had Ready for a ban unused; the
    /// ban counts only while the node takes a message. Whether the turn
    /// moved.
    fn advance(&mut self, now: Instant) -> bool {
        let before = self.turn.slot;
        if before.is_none_or(|s| !self.phones[s].live) {
            self.pass();
        }
        if !self.status.ready {
            self.turn.since = None;
        } else {
            let since = *self.turn.since.get_or_insert(now);
            if now.since(since) >= BAN {
                self.pass();
                self.turn.since = Some(now);
            }
        }
        self.turn.slot != before
    }

    /// When the turn next moves by itself, if another phone could take it.
    fn turn_ends(&self) -> Option<Instant> {
        let others = self.next(self.turn.slot) != self.turn.slot;
        self.turn.since.filter(|_| others).map(|t| t + BAN)
    }

    /// Whether `slot` is a guest's link: the node is public and the link
    /// is not the paired phone's.
    fn guest(&self, slot: usize) -> bool {
        self.public && !self.phones[slot].owner
    }
}

/// The neighbour table as the phone reads it, by slot.
pub struct Table {
    /// The node's noise floor, dBm.
    pub floor_dbm: i8,
    /// The node's own position; zero while it has no place.
    pub own: Position,
    /// Each slot's neighbour, if [`Slot::live`].
    pub slots: [Slot; NEIGHBOURS],
}

/// The neighbour table as the last step left it, apart from [`LINK`],
/// which has fields that are not zero at the start: all zeros, it takes
/// no flash.
static TABLE: Mutex<CriticalSectionRawMutex, RefCell<Table>> = Mutex::new(RefCell::new(Table {
    floor_dbm: 0,
    own: Position { lat: 0, lon: 0 },
    slots: [Slot::EMPTY; NEIGHBOURS],
}));

/// A slot of the neighbour table; all zeros is an empty one.
#[derive(Clone, Copy)]
pub struct Slot {
    /// A live neighbour is in the slot.
    pub live: bool,
    /// The neighbour, but its age.
    pub neighbour: neighbours::Neighbour,
    /// When it was last heard, which its age is read against.
    pub heard: Instant,
}

impl Slot {
    /// An empty slot.
    pub const EMPTY: Slot = Slot {
        live: false,
        neighbour: neighbours::Neighbour {
            id: NodeId([0; NodeId::LEN]),
            position: Position { lat: 0, lon: 0 },
            rssi_ddbm: 0,
            age_s: 0,
            sleepy: false,
            map_point: false,
            demoted: false,
            banned: false,
            anchor: false,
            carried: false,
            anchorless: false,
            proven: false,
        },
        heard: Instant::ZERO,
    };
}

/// The state both sides share.
pub static LINK: Mutex<CriticalSectionRawMutex, RefCell<State>> = Mutex::new(RefCell::new(State {
    status: Status::EMPTY,
    boot_ends: None,
    inbox: None,
    ring_head: None,
    guests: None,
    guests_for: 0,
    given_back: None,
    phones: [Peer::NONE; LINKS],
    took: None,
    turn: Turn {
        slot: None,
        since: None,
    },
    public: false,
    owner_tag: None,
    request: None,
    answer: None,
    told: Status::EMPTY,
    flags: None,
}));

/// Raised when a request waits for the node.
pub static FOR_CORE: Signal<CriticalSectionRawMutex, ()> = Signal::new();
/// The node's answer to the request: taken, or not.
static ANSWER: Signal<CriticalSectionRawMutex, bool> = Signal::new();
/// One request at a time goes to the node, from whichever link.
static ASKING: embassy_sync::mutex::Mutex<CriticalSectionRawMutex, ()> =
    embassy_sync::mutex::Mutex::new(());
/// Raised for each link when its phone is to be notified: the status or a
/// message given back.
static NOTIFY: [Signal<CriticalSectionRawMutex, Notify>; LINKS] = [const { Signal::new() }; LINKS];
/// Raised when the advertisement is to change or may start again: the
/// public setting changed, or a link ended.
static ADVERTISE: Signal<CriticalSectionRawMutex, ()> = Signal::new();
/// Raised once the image has confirmed itself (`update.rs`): the
/// controller starts only then, since the confirmation writes the flash
/// directly, which the controller's radio timing could not wait for.
pub static START: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// What to notify, and whether a guest's link is to close.
#[derive(Clone, Copy, Default)]
struct Notify {
    status: bool,
    given_back: bool,
    close_guests: bool,
}

/// Notify every link's phone of `n`.
fn notify(n: Notify) {
    for slot in 0..LINKS {
        notify_one(slot, n);
    }
}

fn notify_one(slot: usize, n: Notify) {
    let both = match NOTIFY[slot].try_take() {
        Some(o) => Notify {
            status: o.status || n.status,
            given_back: o.given_back || n.given_back,
            close_guests: o.close_guests || n.close_guests,
        },
        None => n,
    };
    NOTIFY[slot].signal(both);
}

/// The battery step the status is notified at: 1 % of a cell's charge
/// between 20 % and full, from the open-circuit voltages the core's
/// levels use (`BatteryLevels::DESIGN`, `BATTERY_FULL_MV`), rounded up.
const BATTERY_STEP_MV: u16 = (bark_proto::node::BATTERY_FULL_MV
    - bark_proto::BatteryLevels::DESIGN.low_mv)
    .div_ceil(100 - 20);

/// The node's side: publish the status and the inbox's oldest message
/// after a step, and notify the phones of a change of the flags, the
/// relays, the inbox's count or the address, or of the battery by a step
/// (spec "Phone link", Status).
pub fn publish(status: Status, inbox: Option<(MsgId, &[u8])>) {
    LINK.lock(|l| l.borrow_mut().ring_head = None);
    publish_inbox(status, inbox);
}

/// The node's side, for a ring, a home's or a public node's: [`publish`]
/// with the inbox the ring's oldest unread item, its number beside (spec
/// "Mailbox").
pub fn publish_ring(status: Status, item: Option<(u32, MsgId, &[u8])>) {
    LINK.lock(|l| {
        l.borrow_mut().ring_head = Some(item.map_or((0, MsgId(0)), |(n, id, _)| (n, id)))
    });
    publish_inbox(status, item.map(|(_, id, p)| (id, p)));
}

fn publish_inbox(status: Status, inbox: Option<(MsgId, &[u8])>) {
    let changed = LINK.lock(|l| {
        let mut l = l.borrow_mut();
        l.inbox = inbox.map(|(id, p)| copy(id, p));
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
        let turn = l.advance(now());
        // The flags as the phone reads them, at the first publish and each
        // change, for the bench to check reads against.
        let flags = status.encode()[0];
        let new_flags = l.flags != Some(flags);
        l.flags = Some(flags);
        (changed || turn, new_flags.then_some(flags))
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
            ..Notify::default()
        });
    }
}

fn copy(id: MsgId, p: &[u8]) -> (MsgId, u8, [u8; MAX_PAYLOAD]) {
    let mut payload = [0; MAX_PAYLOAD];
    payload[..p.len()].copy_from_slice(p);
    (id, p.len() as u8, payload)
}

/// The node's side, on a public node: the core's inbox's oldest message
/// while it waits for the guests in `slots` (`node.rs`), whose Inbox
/// shows it; a guest told of a change by its status.
pub fn publish_guests(waiting: Option<(MsgId, &[u8], u8)>) {
    let changed = LINK.lock(|l| {
        let mut l = l.borrow_mut();
        let before = (l.guests.map(|(id, ..)| id), l.guests_for);
        l.guests = waiting.map(|(id, p, _)| copy(id, p));
        l.guests_for = waiting.map_or(0, |(.., s)| s);
        before != (l.guests.map(|(id, ..)| id), l.guests_for)
    });
    if changed {
        notify(Notify {
            status: true,
            ..Notify::default()
        });
    }
}

/// The node's side: the guests a message waits for now, after one has
/// confirmed it (`node.rs`).
pub fn guests_waiting(slots: u8) {
    let changed = LINK.lock(|l| {
        let mut l = l.borrow_mut();
        let before = l.guests_for;
        l.guests_for &= slots;
        before != l.guests_for
    });
    if changed {
        notify(Notify {
            status: true,
            ..Notify::default()
        });
    }
}

/// Whether the node is public (spec "Phone link", Public).
pub fn public() -> bool {
    LINK.lock(|l| l.borrow().public)
}

/// Whether `tag` is the paired phone's, as the bond keeps it; any is,
/// while the bond keeps none, so that nothing of the owner's is lost
/// before its phone has written one.
pub fn owner_tag_is(tag: Option<u8>) -> bool {
    LINK.lock(|l| l.borrow().owner_tag.is_none_or(|t| Some(t) == tag))
}

/// The slots of the guests connected whose recipient tag is `tag`.
pub fn guests_with(tag: Option<u8>) -> u8 {
    LINK.lock(|l| {
        let l = l.borrow();
        (0..LINKS)
            .filter(|&i| {
                let p = &l.phones[i];
                p.live && l.guest(i) && tag.is_some() && p.tag == tag
            })
            .fold(0, |m, i| m | 1 << i)
    })
}

/// The node's side: publish the neighbour table after a step, which `fill`
/// writes.
pub fn publish_neighbours(fill: impl FnOnce(&mut Table)) {
    TABLE.lock(|t| fill(&mut t.borrow_mut()));
}

/// The node's side: the first rotation's end, from `AppEvent::Boot`.
pub fn booting(ends: Option<Instant>) {
    LINK.lock(|l| l.borrow_mut().boot_ends = ends);
}

/// The node's side: a phone's message was given back, to the phone it
/// came from while its link lasts, the paired phone's kept for its next
/// (spec "Phone link", Given back, Public).
pub fn given_back(msg_id: MsgId, retry_at: Instant) {
    let to = LINK.lock(|l| {
        let mut l = l.borrow_mut();
        match l.took {
            Some((id, from, link)) if id == msg_id && !from.owner => {
                let p = &mut l.phones[from.slot];
                let here = p.live && p.link == link;
                if here {
                    p.given_back = Some((msg_id, retry_at));
                }
                here.then_some(from.slot)
            }
            _ => {
                l.given_back = Some((msg_id, retry_at));
                (0..LINKS).find(|&i| l.phones[i].live && l.phones[i].owner)
            }
        }
    });
    if let Some(slot) = to {
        notify_one(
            slot,
            Notify {
                given_back: true,
                ..Notify::default()
            },
        );
    }
}

/// Whether a request waits for the node.
pub fn request_waiting() -> bool {
    LINK.lock(|l| l.borrow().request.is_some())
}

/// The node's side: take the request waiting if `f` takes it, with the
/// answer `f` gives, which goes to the phone after the step ([`answer`]).
pub fn take_request<R>(f: impl FnOnce(From, &Request) -> Option<(R, bool)>) -> Option<R> {
    let taken = LINK.lock(|l| {
        let mut l = l.borrow_mut();
        let (from, request) = l.request?;
        let (r, answer) = f(from, &request)?;
        if let Request::Send {
            dst_pos,
            dst_id,
            len,
            payload,
        } = request
        {
            if answer {
                // A phone's message taken: what it was given back before
                // is over, and the turn passes on.
                match from.owner {
                    true => l.given_back = None,
                    false => l.phones[from.slot].given_back = None,
                }
                let id = MsgId::of(dst_pos, dst_id, &payload[..usize::from(len)]);
                l.took = Some((id, from, l.phones[from.slot].link));
                l.pass();
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

/// Hand `request` from `from` to the node and wait for its answer, after
/// any other link's.
async fn ask(from: From, request: Request) -> bool {
    let _one = ASKING.lock().await;
    ANSWER.reset();
    LINK.lock(|l| l.borrow_mut().request = Some((from, request)));
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

/// The status as the phone in `slot` reads it now: ready in its turn
/// alone, and a guest's inbox the message waiting for it.
fn status_now(slot: usize) -> Status {
    LINK.lock(|l| {
        let mut l = l.borrow_mut();
        let now = now();
        l.advance(now);
        let inbox = match l.guest(slot) {
            true => u8::from(l.guests_for & 1 << slot != 0),
            false => l.status.inbox,
        };
        Status {
            ready: l.status.ready && l.request.is_none() && l.turn.slot == Some(slot),
            boot_left_s: l.boot_ends.map_or(0, |e| seconds_left(now, e)),
            inbox,
            ..l.status
        }
    })
}

fn given_back_now(slot: usize) -> heapless::Vec<u8, { GivenBack::LEN }> {
    LINK.lock(|l| {
        let l = l.borrow();
        let given = match l.guest(slot) {
            true => l.phones[slot].given_back,
            false => l.given_back,
        };
        match given {
            Some((msg_id, retry_at)) => {
                let g = GivenBack {
                    msg_id,
                    wait_s: seconds_left(now(), retry_at),
                };
                heapless::Vec::from_slice(&g.encode()).expect("it fits")
            }
            None => heapless::Vec::new(),
        }
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
        offset_min: t.offset_min.unwrap_or(0),
        source: match t.source {
            None => TimeSource::None,
            Some(Source::Gps) => TimeSource::Gps,
            Some(Source::Phone) => TimeSource::Phone,
        },
    }
    .encode()
}

/// The page of the neighbour table the phone reads now: the live
/// neighbours from the slot it wrote on, as many as fit.
fn neighbours_now(slot: usize) -> heapless::Vec<u8, { neighbours::MAX_LEN }> {
    let mut page = [neighbours::Neighbour::default(); neighbours::PER_PAGE];
    let mut len = 0;
    let from = LINK.lock(|l| l.borrow().phones[slot].neighbours_from);
    let header = TABLE.lock(|t| {
        let t = t.borrow();
        let mut next = 0;
        let live = t
            .slots
            .iter()
            .enumerate()
            .skip(usize::from(from))
            .filter(|(_, s)| s.live);
        for (slot, s) in live {
            if len == page.len() {
                next = slot as u8;
                break;
            }
            let age_s = now().since(s.heard).as_micros() / 1_000_000;
            page[len] = neighbours::Neighbour {
                age_s: age_s.min(u64::from(u16::MAX)) as u16,
                ..s.neighbour
            };
            len += 1;
        }
        neighbours::Header {
            floor_dbm: t.floor_dbm,
            next,
            own: t.own,
        }
    });
    let mut b = [0; neighbours::MAX_LEN];
    let n = neighbours::encode(&header, &page[..len], &mut b);
    heapless::Vec::from_slice(&b[..n]).expect("it fits")
}

/// The Inbox the phone in `slot` reads: the paired phone's, or on a
/// public node a guest's, the message waiting for it.
fn inbox_now(slot: usize) -> heapless::Vec<u8, { Message::MAX_LEN }> {
    LINK.lock(|l| {
        let l = l.borrow();
        let shown = match l.guest(slot) {
            true => l.guests.filter(|_| l.guests_for & 1 << slot != 0),
            false => l.inbox,
        };
        match shown {
            Some((msg_id, len, payload)) => {
                let mut b = [0; Message::MAX_LEN];
                let n = Message {
                    msg_id,
                    payload: &payload[..len as usize],
                }
                .encode(&mut b);
                heapless::Vec::from_slice(&b[..n]).expect("it fits")
            }
            None => heapless::Vec::new(),
        }
    })
}

/// The bond kept in flash, handed once at boot when the store has opened
/// (`store.rs`).
pub static STORED_BOND: Signal<CriticalSectionRawMutex, Option<Bond>> = Signal::new();

type Stack = trouble_host::Stack<'static, SoftdeviceController<'static>, DefaultPacketPool>;

/// The update the paired phone stages, across its links.
type Update = embassy_sync::mutex::Mutex<NoopRawMutex, crate::update::Phone>;

/// The advertising, which one slot does at a time.
type Advertising = embassy_sync::mutex::Mutex<
    NoopRawMutex,
    Peripheral<'static, SoftdeviceController<'static>, DefaultPacketPool>,
>;

/// Run the host, and the links' slots each in a task of its own
/// ([`slot`]), for ever. `internal` and `flash` are the internal flash,
/// through the MPSL and read directly, for the update (`update::Phone`).
pub async fn run(
    spawner: embassy_executor::Spawner,
    controller: SoftdeviceController<'static>,
    id: NodeId,
    image: u32,
    internal: &'static crate::store::SharedFlash,
    flash: &'static crate::update::Flash,
) {
    static RESOURCES: StaticCell<
        HostResources<DefaultPacketPool, LINKS, CHANNELS, ADV_SETS, BONDS>,
    > = StaticCell::new();
    static STACK: StaticCell<Stack> = StaticCell::new();
    let resources = RESOURCES.init(HostResources::new());
    // The node shows a passkey and takes none: passkey entry, the phone
    // typing what the node displays (Vol 3 Part H §2.3.5.1, Table 2.8).
    let stack: &'static Stack = STACK.init(
        trouble_host::new(controller, resources)
            .set_random_address(address())
            .set_io_capabilities(IoCapabilities::DisplayOnly)
            .build(),
    );
    if let Some(b) = STORED_BOND.wait().await {
        KEPT.lock(|k| k.replace(Some(b)));
        match stack.add_bond_information(information(&b)) {
            Ok(()) => log::info!(
                "ble: bond kept, table seen at version {}, service changed {}, public {}",
                b.seen,
                b.service_changed,
                b.public
            ),
            Err(e) => log::error!("ble: the kept bond {}", HostError(&e)),
        }
    }
    follow_bond();
    static NAME: StaticCell<[u8; bark_link::NAME_LEN]> = StaticCell::new();
    let name = core::str::from_utf8(NAME.init(bark_link::name(id))).expect("the name is ASCII");
    static SERVER: StaticCell<(Server<'static>, Gatt)> = StaticCell::new();
    let (server, gatt) = SERVER.init(server(name));
    let node = Node {
        version: bark_link::VERSION,
        id,
        image,
    };
    let _ = server.set(&server.link.node, &node.encode());
    static ADVERTISING: StaticCell<Advertising> = StaticCell::new();
    let advertising = ADVERTISING.init(Advertising::new(stack.peripheral()));
    static UPDATE: StaticCell<Update> = StaticCell::new();
    let update = UPDATE.init(Update::new(crate::update::Phone::new(internal, flash)));
    for i in 0..LINKS {
        let token = slot(i, stack, advertising, server, gatt, name, update);
        spawner.spawn(token.expect("one task a slot"));
    }
    let mut runner = stack.runner();
    loop {
        if let Err(e) = runner.run().await {
            log::error!("ble: host {}", StackError(&e));
        }
    }
}

/// A slot for one link, for ever: advertise while the node may take a
/// link, one slot at a time, and serve the link that comes. Each is a task
/// of its own, so that its future lies in its own static storage, and the
/// stack holds one poll at a time.
#[embassy_executor::task(pool_size = LINKS)]
async fn slot(
    i: usize,
    stack: &'static Stack,
    advertising: &'static Advertising,
    server: &'static Server<'static>,
    gatt: &'static Gatt,
    name: &'static str,
    update: &'static Update,
) {
    loop {
        let conn = {
            let mut p = advertising.lock().await;
            loop {
                let public = public();
                if links() > 0 && !public {
                    ADVERTISE.wait().await;
                    continue;
                }
                // Stopped and begun again as the setting changes or a
                // link ends, which may make a public node's free.
                match select(advertise(&mut p, server, name, public), ADVERTISE.wait()).await {
                    Either::First(Ok(conn)) => break conn,
                    Either::First(Err(e)) => {
                        log::error!("ble: advertising {}", StackError(&e));
                        embassy_time::Timer::after(ADV_RETRY).await;
                    }
                    Either::Second(()) => {}
                }
            }
        };
        let link = LINK.lock(|l| {
            let mut l = l.borrow_mut();
            let p = &mut l.phones[i];
            *p = Peer {
                live: true,
                link: p.link.wrapping_add(1),
                ..Peer::NONE
            };
            p.link
        });
        log::info!("ble: link {} in slot {}", link, i);
        serve(i, stack, server, gatt, &conn, update).await;
        LINK.lock(|l| {
            let mut l = l.borrow_mut();
            let link = l.phones[i].link;
            l.phones[i] = Peer { link, ..Peer::NONE };
        });
        // A message waiting for this guest alone is now nobody's, and its
        // turn passes on.
        crate::mailbox::FOR_CORE.signal(());
        ADVERTISE.signal(());
        if LINK.lock(|l| l.borrow_mut().advance(now())) {
            notify(Notify {
                status: true,
                ..Notify::default()
            });
        }
    }
}

/// Links up now.
fn links() -> usize {
    LINK.lock(|l| l.borrow().phones.iter().filter(|p| p.live).count())
}

/// Take up what the kept bond says: whether the node is public and the
/// owner's recipient tag, for the links, the mailbox and the screen.
fn follow_bond() {
    let bond = KEPT.lock(|k| *k.borrow());
    let public = bond.is_some_and(|b| b.public);
    LINK.lock(|l| {
        let mut l = l.borrow_mut();
        l.public = public;
        l.owner_tag = bond.and_then(|b| b.owner);
    });
    crate::mailbox::with(|m| m.set_public(public));
}

/// Keep `bond` in place of the one kept, in the host's copy and in flash,
/// and follow it.
fn keep(bond: Bond) {
    KEPT.lock(|k| k.replace(Some(bond)));
    crate::store::keep_bond(bond);
    follow_bond();
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

/// Advertise until a phone connects (roadmap D120): the service's UUID,
/// or on a public node the public node's (spec "Phone link", Public).
async fn advertise<'v, 's>(
    peripheral: &mut Peripheral<'v, SoftdeviceController<'static>, DefaultPacketPool>,
    server: &'s Server<'v>,
    name: &str,
    public: bool,
) -> Result<GattConnection<'v, 's, DefaultPacketPool>, BleHostError<sdc::Error>> {
    let uuid = match public {
        true => uuid::PUBLIC_NODE,
        false => uuid::SERVICE,
    };
    let mut adv = [0; ADV_DATA];
    let adv_len = AdStructure::encode_slice(
        &[
            AdStructure::Flags(LE_GENERAL_DISCOVERABLE | BR_EDR_NOT_SUPPORTED),
            AdStructure::CompleteServiceUuids128(&[le_bytes(uuid)]),
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
    log::info!(
        "ble: advertising every {} us, public {}",
        ADV_UNITS * 625,
        public
    );
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

/// Whether the link is the paired phone's: encrypted with an
/// authenticated key, which only the kept bond gives, as the host holds
/// no other ([`settle`]).
fn owner(conn: &GattConnection<'_, '_, DefaultPacketPool>) -> bool {
    conn.raw().security_level().ok() == Some(SecurityLevel::EncryptedAuthenticated)
}

/// The handles a guest may use on a public node: the values of Node,
/// Status, Send, Inbox, Given back and Recipient, and the subscriptions
/// to Status and Given back (spec "Phone link", Public).
fn open_handles(link: &Link) -> [u16; 8] {
    let cccd = |h: Option<u16>| h.expect("it notifies");
    [
        link.node.handle,
        link.status.handle,
        cccd(link.status.cccd_handle),
        link.send.handle,
        link.inbox.handle,
        link.given_back.handle,
        cccd(link.given_back.cccd_handle),
        link.recipient.handle,
    ]
}

/// Whether the link may make the request `att`: the paired phone's may
/// make any the host allows; on a public node, any link may use the open
/// handles; on any node, any link may discover the table and read what
/// the host leaves open. A read by type reads an open value only by the
/// type of one (Vol 3 Part F §3.4.4.1).
fn allowed(owner: bool, public: bool, link: &Link, att: &AttClient<'_>) -> bool {
    if owner || public {
        return true;
    }
    let open = open_handles(link);
    let touches = |h: &u16| open.contains(h);
    match att {
        AttClient::Request(
            AttReq::Read { handle }
            | AttReq::ReadBlob { handle, .. }
            | AttReq::Write { handle, .. }
            | AttReq::PrepareWrite { handle, .. },
        )
        | AttClient::Command(AttCmd::Write { handle, .. }) => !touches(handle),
        AttClient::Request(AttReq::ReadMultiple { handles }) => !handles
            .as_chunks::<2>()
            .0
            .iter()
            .any(|h| touches(&u16::from_le_bytes(*h))),
        AttClient::Request(AttReq::ReadByType {
            start,
            end,
            attribute_type,
        }) => {
            let open_types = [
                uuid::NODE,
                uuid::STATUS,
                uuid::SEND,
                uuid::INBOX,
                uuid::GIVEN_BACK,
                uuid::RECIPIENT,
            ]
            .map(|u| Uuid::new_long(le_bytes(u)));
            let typed = open_types.contains(attribute_type)
                || *attribute_type == Uuid::new_short(CCCD_TYPE);
            !typed || !open.iter().any(|h| (*start..=*end).contains(h))
        }
        _ => true,
    }
}

/// The client characteristic configuration descriptor's type (Bluetooth
/// Assigned Numbers, Descriptors).
const CCCD_TYPE: u16 = 0x2902;

/// Serve the link in `slot` until it ends: answer its reads and writes,
/// notify what changes, and pair.
async fn serve(
    slot: usize,
    stack: &Stack,
    server: &Server<'_>,
    gatt: &Gatt,
    conn: &GattConnection<'_, '_, DefaultPacketPool>,
    update: &Update,
) {
    let changes = Cccd(
        gatt.service_changed
            .cccd_handle
            .expect("Service Changed indicates"),
    );
    // A pairing that does not bond is refused ([`settle`]); the phone asks
    // to bond, and so the node offers it.
    if let Err(e) = conn.raw().set_bondable(true) {
        log::error!("ble: bondable {}", HostError(&e));
    }
    // What changed while no phone was connected is read, not notified.
    NOTIFY[slot].reset();
    let link = &server.link;
    refresh(slot, server, conn, update);
    loop {
        let next = LINK.lock(|l| l.borrow().turn_ends());
        let turn = async {
            match next {
                Some(t) => {
                    embassy_time::Timer::at(embassy_time::Instant::from_micros(t.as_micros())).await
                }
                None => core::future::pending().await,
            }
        };
        let event = match select3(conn.next(), NOTIFY[slot].wait(), turn).await {
            Either3::First(e) => e,
            Either3::Second(n) => {
                // A guest's link ends with the public setting.
                if n.close_guests && !owner(conn) {
                    log::info!("ble: no longer public; the guest in slot {} closed", slot);
                    conn.raw().disconnect();
                }
                refresh(slot, server, conn, update);
                if n.status {
                    let _ = link
                        .status
                        .notify(conn, &status_now(slot).encode(), true)
                        .await;
                }
                if n.given_back {
                    let _ = link
                        .given_back
                        .notify(conn, &given_back_now(slot), true)
                        .await;
                }
                continue;
            }
            Either3::Third(()) => {
                // The turn passed on unused.
                if LINK.lock(|l| l.borrow_mut().advance(now())) {
                    notify(Notify {
                        status: true,
                        ..Notify::default()
                    });
                }
                continue;
            }
        };
        let event = match event {
            GattConnectionEvent::Disconnected { reason } => {
                log::info!("ble: disconnected 0x{:02x} slot {}", u8::from(reason), slot);
                if owner(conn) {
                    update.lock().await.link_closed();
                }
                // A pairing cut short leaves no code on the screen.
                crate::display::phone(phone_now(stack));
                return;
            }
            GattConnectionEvent::PassKeyDisplay(key) => {
                log::info!("ble: pairing, the code shown");
                crate::display::phone(Phone::Code(key.value()));
                continue;
            }
            GattConnectionEvent::PairingComplete {
                security_level,
                bond,
            } => {
                settle(stack, conn, security_level, bond);
                LINK.lock(|l| l.borrow_mut().phones[slot].owner = owner(conn));
                continue;
            }
            GattConnectionEvent::PairingFailed(e) => {
                log::info!("ble: pairing failed {}", HostError(&e));
                settle(stack, conn, SecurityLevel::NoEncryption, None);
                continue;
            }
            GattConnectionEvent::Encrypted { security_level, .. } => {
                log::info!("ble: encrypted {:?}", security_level);
                LINK.lock(|l| l.borrow_mut().phones[slot].owner = owner(conn));
                tell_of_change(gatt, &changes, conn, false).await;
                continue;
            }
            GattConnectionEvent::BondLost => {
                log::info!("ble: a phone the node holds a bond with pairs again");
                continue;
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
        // The update's pieces and its progress reads come many to a
        // second; the update logs its own changes instead.
        let incoming = event.payload().incoming();
        let piece = written(&incoming) == Some(link.image.handle);
        if !piece && read_of(&incoming) != Some(link.update.handle) {
            log_request(&incoming);
        }
        let from = From {
            slot,
            owner: owner(conn),
        };
        if !allowed(from.owner, public(), link, &incoming) {
            if let Ok(reply) = event.reject(AttErrorCode::INSUFFICIENT_AUTHENTICATION) {
                reply.send().await;
            }
            continue;
        }
        // The attribute server answers every kind of read from the values
        // it holds, a read by handle or by type, of one value or several:
        // they are brought up to date before each request is answered, but
        // for the later parts of a long read, which must read on in the
        // value its first part began, and a piece, which reads nothing.
        // Each phone's values are its own, set for its link before each
        // request, with no wait between.
        let continued = matches!(
            incoming,
            AttClient::Request(AttReq::ReadBlob { offset, .. }) if offset > 0
        );
        if !continued && !piece {
            refresh(slot, server, conn, update);
        }
        let mut wrote_changes = false;
        let mut reboot = false;
        let reply = match event {
            GattEvent::Write(w) => {
                let h = w.handle();
                wrote_changes = h == changes.0;
                let answer = if h == link.image.handle {
                    let taken = write_image(&w, &mut *update.lock().await).await;
                    // A write without response has no answer to refuse
                    // with; the phone reads Update instead.
                    match w.payload().incoming() {
                        AttClient::Command(_) => Some(Ok(())),
                        _ => Some(taken),
                    }
                } else if h == link.update.handle {
                    let mut u = update.lock().await;
                    let answer = write_update(&w, &mut u).await;
                    reboot = answer == Ok(true);
                    let _ = conn.set(&link.update, &progress(&u));
                    Some(answer.map(|_| ()))
                } else if h == link.send.handle {
                    Some(write_send(from, &w).await)
                } else if h == link.inbox.handle {
                    Some(write_confirm(from, &w).await)
                } else if h == link.map_point.handle {
                    Some(write_map_point(from, &w).await)
                } else if h == link.clock.handle {
                    Some(write_clock(from, &w).await)
                } else if h == link.home.handle {
                    Some(write_home(&w))
                } else if h == link.neighbours.handle {
                    Some(write_neighbours(slot, &w))
                } else if h == link.recipient.handle {
                    Some(write_recipient(from, &w))
                } else if h == link.public.handle {
                    Some(write_public(&w))
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
        if wrote_changes {
            tell_of_change(gatt, &changes, conn, true).await;
        }
        if reboot {
            // Closed first, so that the phone has the answer and sees the
            // node leave, not lose it.
            conn.raw().disconnect();
            let closed = async {
                loop {
                    if let GattConnectionEvent::Disconnected { reason } = conn.next().await {
                        log::info!("ble: disconnected 0x{:02x}", u8::from(reason));
                        break;
                    }
                }
            };
            let _ = embassy_time::with_timeout(SUPERVISION_MAX, closed).await;
            crate::update::reboot().await;
        }
    }
}

/// The longest supervision timeout a link has, 32 s (Bluetooth Core
/// Vol 6 Part B §4.5.2): by then a link whose end the phone does not
/// acknowledge has ended anyway.
const SUPERVISION_MAX: embassy_time::Duration = embassy_time::Duration::from_secs(32);

/// Update as the phone reads it.
fn progress(update: &crate::update::Phone) -> heapless::Vec<u8, { Progress::LEN }> {
    heapless::Vec::from_slice(&update.progress().encode()).expect("it fits")
}

/// The handle an ATT write writes, by request or command.
fn written(att: &AttClient<'_>) -> Option<u16> {
    match att {
        AttClient::Request(AttReq::Write { handle, .. })
        | AttClient::Command(AttCmd::Write { handle, .. }) => Some(*handle),
        _ => None,
    }
}

/// The handle an ATT read reads, whole or in part.
fn read_of(att: &AttClient<'_>) -> Option<u16> {
    match att {
        AttClient::Request(AttReq::Read { handle } | AttReq::ReadBlob { handle, .. }) => {
            Some(*handle)
        }
        _ => None,
    }
}

/// Bring the paired phone's attribute table up to date (spec
/// "Phone link", Changes to the table): on its link, encrypted with the
/// kept bond, keep its subscription to Service Changed with the bond, as
/// a bonded client's is kept across connections (Vol 3 Part G
/// §3.3.3.3), and while it is subscribed and the table it last saw is of
/// another link version, indicate Service Changed over every handle; once
/// the phone confirms, it has the table of this version. `written` is
/// whether the phone has just written its subscription, which then
/// stands as written; else the kept one is put back.
async fn tell_of_change(
    gatt: &Gatt,
    changes: &Cccd,
    conn: &GattConnection<'_, '_, DefaultPacketPool>,
    written: bool,
) {
    let raw = conn.raw();
    if raw.security_level().ok() != Some(SecurityLevel::EncryptedAuthenticated) {
        return;
    }
    let Some(mut bond) = KEPT.lock(|k| *k.borrow()) else {
        return;
    };
    if !information(&bond)
        .identity
        .match_identity(&raw.peer_identity())
    {
        return;
    }
    let before = bond;
    let live = conn
        .get(changes)
        .is_ok_and(|v| CCCD::from(u16::from_le_bytes(v)).should_indicate());
    bond.service_changed = live || (!written && bond.service_changed);
    if bond.service_changed && !live {
        let mut on = CCCD::default();
        on.set_indicate(true);
        if let Err(e) = conn.set(changes, &on.raw().to_le_bytes()) {
            log::error!("ble: the kept subscription {}", HostError(&e));
        }
    }
    if bond.service_changed && bond.seen != bark_link::VERSION {
        log::info!(
            "ble: service changed: the phone saw version {}, this is {}",
            bond.seen,
            bark_link::VERSION
        );
        match gatt
            .service_changed
            .indicate(conn, &ALL_HANDLES, false)
            .await
        {
            Ok(()) => {
                log::info!("ble: service changed confirmed");
                bond.seen = bark_link::VERSION;
            }
            Err(e) => log::warn!("ble: service changed {}", HostError(&e)),
        }
    }
    if bond != before {
        keep(bond);
    }
}

/// After a pairing: keep its bond if it is the paired phone's from now
/// on, an authenticated pairing that bonds, in place of the one kept
/// before, since its code was on the node's screen and so someone was at
/// the node. The host then holds the kept bond and no other, so that a
/// Just Works pairing, or one that does not bond, leaves the kept bond as
/// it was, even one that replaced it in the host as the same phone's; and
/// its link is closed.
fn settle(
    stack: &Stack,
    conn: &GattConnection<'_, '_, DefaultPacketPool>,
    level: SecurityLevel,
    bond: Option<BondInformation>,
) {
    let new = bond.filter(|b| level == SecurityLevel::EncryptedAuthenticated && b.is_bonded);
    if let Some(b) = &new {
        log::info!("ble: paired; the bond replaces any before it");
        let public = KEPT.lock(|k| k.borrow().is_some_and(|b| b.public));
        keep(bond_of(b, public));
    } else if level != SecurityLevel::NoEncryption {
        log::warn!("ble: a pairing that is not authenticated or does not bond; closed");
        conn.raw().disconnect();
    }
    let keep = KEPT.lock(|k| *k.borrow()).map(|b| information(&b));
    let mut other: heapless::Vec<Identity, BONDS> = heapless::Vec::new();
    let mut held = false;
    stack.with_bond_information(|bonds| {
        for b in bonds {
            match Some(b) == keep.as_ref() {
                true => held = true,
                false => {
                    let _ = other.push(b.identity);
                }
            }
        }
    });
    for i in other {
        if let Err(e) = stack.remove_bond_information(i) {
            log::error!("ble: removing a bond {}", HostError(&e));
        }
    }
    if let Some(b) = keep.filter(|_| !held) {
        if let Err(e) = stack.add_bond_information(b) {
            log::error!("ble: the kept bond {}", HostError(&e));
        }
    }
    crate::display::phone(phone_now(stack));
}

/// The bond kept, which the host is to hold alone ([`settle`]), as
/// kept in flash.
static KEPT: Mutex<CriticalSectionRawMutex, RefCell<Option<Bond>>> = Mutex::new(RefCell::new(None));

/// The screen's phone line when no pairing runs.
fn phone_now(stack: &Stack) -> Phone {
    if public() {
        return Phone::Public;
    }
    match stack.with_bond_information(|b| b.is_empty()) {
        true => Phone::None,
        false => Phone::Paired,
    }
}

/// The host's bond of a kept one.
fn information(b: &Bond) -> BondInformation {
    BondInformation::new(
        Identity {
            addr: Address::new(AddrKind::new(b.kind), BdAddr::new(b.addr)),
            irk: b.irk.and_then(IdentityResolvingKey::from_le_bytes),
        },
        LongTermKey::from_le_bytes(b.ltk),
        SecurityLevel::EncryptedAuthenticated,
        true,
    )
}

/// The bond to keep of a new pairing's: the phone discovers the table
/// of this version as it pairs, and has not subscribed yet; the node stays
/// `public` or not, and the new phone's tag comes once it writes it.
fn bond_of(b: &BondInformation, public: bool) -> Bond {
    let mut addr = [0u8; 6];
    addr.copy_from_slice(b.identity.addr.addr.raw());
    Bond {
        kind: b.identity.addr.kind.into_inner(),
        addr,
        irk: b.identity.irk.map(|k| k.to_le_bytes()),
        ltk: b.ltk.to_le_bytes(),
        seen: bark_link::VERSION,
        service_changed: false,
        public,
        owner: None,
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

/// Put what the node publishes now, as the phone in `slot` reads it,
/// into the attribute server's values. Update's is left as it was while
/// an update's write holds it, on the paired phone's link, which sets it
/// after.
fn refresh(
    slot: usize,
    server: &Server<'_>,
    conn: &GattConnection<'_, '_, DefaultPacketPool>,
    update: &Update,
) {
    let link = &server.link;
    let status = status_now(slot).encode();
    let staged = update
        .try_lock()
        .ok()
        .and_then(|u| conn.set(&link.update, &progress(&u)).err());
    for e in [
        staged,
        conn.set(&link.status, &status).err(),
        conn.set(&link.inbox, &inbox_now(slot)).err(),
        conn.set(&link.given_back, &given_back_now(slot)).err(),
        conn.set(&link.clock, &clock_now()).err(),
        conn.set(&link.home, &home_now().encode()).err(),
        conn.set(&link.neighbours, &neighbours_now(slot)).err(),
        conn.set(&link.public, &bark_link::Public(public()).encode())
            .err(),
    ]
    .into_iter()
    .flatten()
    {
        log::error!("ble: setting a value {}", HostError(&e));
    }
}

fn malformed(m: Malformed) -> AttErrorCode {
    AttErrorCode::new(m.att_error())
}

const NOT_READY: AttErrorCode = AttErrorCode::new(bark_link::att::NOT_READY);
const REFUSED: AttErrorCode = AttErrorCode::new(bark_link::att::REFUSED);

/// Update, written: begin, commit or reboot (spec "Phone link", Updates);
/// whether a reboot was asked for and may go ahead, or the refusal, which
/// Update's read then says.
async fn write_update(
    w: &WriteEvent<'_, '_, DefaultPacketPool>,
    update: &mut crate::update::Phone,
) -> Result<bool, AttErrorCode> {
    enum Asked {
        Begin,
        Commit,
        Reboot,
    }
    let mut header = [0u8; bark_link::update::HEADER_LEN];
    let asked = w.with_data(|offset, data| match offset {
        0 => Ok(match Command::decode(data).map_err(malformed)? {
            Command::Begin(h) => {
                header.copy_from_slice(h);
                Asked::Begin
            }
            Command::Commit => Asked::Commit,
            Command::Reboot => Asked::Reboot,
        }),
        _ => Err(AttErrorCode::INVALID_OFFSET),
    })?;
    let done: Result<bool, Refusal> = match asked {
        Asked::Begin => update.begin(&header).await.map(|()| false),
        Asked::Commit => update.commit().await.map(|()| false),
        Asked::Reboot => update.reboot().map(|()| true),
    };
    done.map_err(|_| REFUSED)
}

/// Image, written: a piece, taken where the bytes held end; one written
/// with a response elsewhere is answered 0x13.
async fn write_image(
    w: &WriteEvent<'_, '_, DefaultPacketPool>,
    update: &mut crate::update::Phone,
) -> Result<(), AttErrorCode> {
    let mut bytes = [0u8; Piece::MAX_BYTES];
    let (offset, len) = w.with_data(|offset, data| match offset {
        0 => {
            let p = Piece::decode(data).map_err(malformed)?;
            bytes[..p.bytes.len()].copy_from_slice(p.bytes);
            Ok((p.offset, p.bytes.len()))
        }
        _ => Err(AttErrorCode::INVALID_OFFSET),
    })?;
    match update.piece(offset, &bytes[..len]).await {
        true => Ok(()),
        false => Err(malformed(Malformed::Value)),
    }
}

/// Send, written: the node takes the message, or answers not ready, as
/// it does a phone whose turn it is not (spec "Phone link", Public).
async fn write_send(
    from: From,
    w: &WriteEvent<'_, '_, DefaultPacketPool>,
) -> Result<(), AttErrorCode> {
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
    if !status_now(from.slot).ready {
        return Err(NOT_READY);
    }
    match ask(from, request).await {
        true => Ok(()),
        false => Err(NOT_READY),
    }
}

async fn write_confirm(
    from: From,
    w: &WriteEvent<'_, '_, DefaultPacketPool>,
) -> Result<(), AttErrorCode> {
    let c = w.with_data(|offset, data| match offset {
        0 => bark_link::Confirm::decode(data).map_err(malformed),
        _ => Err(AttErrorCode::INVALID_OFFSET),
    })?;
    match ask(from, Request::Confirm(c.0)).await {
        true => Ok(()),
        false => Err(malformed(Malformed::Value)),
    }
}

async fn write_map_point(
    from: From,
    w: &WriteEvent<'_, '_, DefaultPacketPool>,
) -> Result<(), AttErrorCode> {
    let m = w.with_data(|offset, data| match offset {
        0 => bark_link::MapPoint::decode(data).map_err(malformed),
        _ => Err(AttErrorCode::INVALID_OFFSET),
    })?;
    ask(from, Request::MapPoint(m)).await;
    Ok(())
}

/// Recipient, written: the phone's recipient tag at the node; the paired
/// phone's is kept with the bond, so that the node places its owner's
/// messages while the owner is away (spec "Phone link", Public).
fn write_recipient(
    from: From,
    w: &WriteEvent<'_, '_, DefaultPacketPool>,
) -> Result<(), AttErrorCode> {
    let tag = w.with_data(|offset, data| match offset {
        0 => bark_link::Recipient::decode(data).map_err(malformed),
        _ => Err(AttErrorCode::INVALID_OFFSET),
    })?;
    LINK.lock(|l| l.borrow_mut().phones[from.slot].tag = Some(tag.0));
    if from.owner {
        if let Some(b) = KEPT
            .lock(|k| *k.borrow())
            .filter(|b| b.owner != Some(tag.0))
        {
            keep(Bond {
                owner: Some(tag.0),
                ..b
            });
        }
    }
    log::info!(
        "ble: recipient {:02x} slot {} owner {}",
        tag.0,
        from.slot,
        from.owner
    );
    Ok(())
}

/// Public, written by the paired phone: the node serves guests, or no
/// longer, at once and across reboots; turned off, its guests' links
/// close (spec "Phone link", Public).
fn write_public(w: &WriteEvent<'_, '_, DefaultPacketPool>) -> Result<(), AttErrorCode> {
    let set = w.with_data(|offset, data| match offset {
        0 => bark_link::Public::decode(data).map_err(malformed),
        _ => Err(AttErrorCode::INVALID_OFFSET),
    })?;
    let Some(b) = KEPT.lock(|k| *k.borrow()) else {
        // Only a bonded link writes it, and it is the bond's.
        return Err(AttErrorCode::INSUFFICIENT_AUTHENTICATION);
    };
    if b.public != set.0 {
        keep(Bond { public: set.0, ..b });
        log::info!("ble: public {}", set.0);
        ADVERTISE.signal(());
        crate::mailbox::FOR_CORE.signal(());
        notify(Notify {
            status: true,
            close_guests: !set.0,
            ..Notify::default()
        });
        crate::display::phone(match set.0 {
            true => Phone::Public,
            false => Phone::Paired,
        });
    }
    Ok(())
}

/// Home as the phone reads it: whether the node is a home, and the
/// numbers its ring holds.
fn home_now() -> bark_link::home::Home {
    crate::mailbox::with(|m| bark_link::home::Home {
        set: m.is_home(),
        oldest: m.oldest(),
        newest: m.newest(),
    })
    .unwrap_or_default()
}

/// Home, written: the mailbox takes the key, or forgets it, at once
/// (`mailbox.rs`), and the node publishes its inbox as a home's or not
/// (`node.rs`, `follow_home`).
fn write_home(w: &WriteEvent<'_, '_, DefaultPacketPool>) -> Result<(), AttErrorCode> {
    use bark_link::home::SetHome;
    let set = w.with_data(|offset, data| match offset {
        0 => SetHome::decode(data).map_err(malformed),
        _ => Err(AttErrorCode::INVALID_OFFSET),
    })?;
    let key = match set {
        SetHome::Set(k) => Some(k),
        SetHome::Clear => None,
    };
    crate::mailbox::with(|m| m.set_key(key));
    crate::mailbox::FOR_CORE.signal(());
    log::info!(
        "ble: home {}",
        if key.is_some() { "set" } else { "cleared" }
    );
    Ok(())
}

/// Neighbours, written: the next read on the link in `slot` starts at
/// this slot of the table.
fn write_neighbours(
    slot: usize,
    w: &WriteEvent<'_, '_, DefaultPacketPool>,
) -> Result<(), AttErrorCode> {
    let from = w.with_data(|offset, data| match offset {
        0 => neighbours::decode_from(data).map_err(malformed),
        _ => Err(AttErrorCode::INVALID_OFFSET),
    })?;
    LINK.lock(|l| l.borrow_mut().phones[slot].neighbours_from = from);
    Ok(())
}

async fn write_clock(
    from: From,
    w: &WriteEvent<'_, '_, DefaultPacketPool>,
) -> Result<(), AttErrorCode> {
    let c = w.with_data(|offset, data| match offset {
        0 => bark_link::Clock::decode(data).map_err(malformed),
        _ => Err(AttErrorCode::INVALID_OFFSET),
    })?;
    ask(from, Request::Clock(c)).await;
    Ok(())
}

/// A host error as its variant's name and, where it carries one, the
/// code a specification gives it. The derived `Debug` would bring in the
/// controller's status names and every nested error's `Debug`, kilobytes
/// of flash for lines that rarely print.
struct HostError<'a>(&'a trouble_host::Error);

/// [`HostError`], or the controller's error: an nRF errno, negated
/// (`nrf_mpsl::Error`).
struct StackError<'a>(&'a BleHostError<sdc::Error>);

impl core::fmt::Display for StackError<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.0 {
            BleHostError::Controller(c) => write!(f, "controller {}", i32::from(*c)),
            BleHostError::BleHost(e) => HostError(e).fmt(f),
        }
    }
}

impl core::fmt::Display for HostError<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        use trouble_host::Error as H;
        let name = match self.0 {
            // Bluetooth Core, Vol 1, Part F: controller error codes.
            H::Hci(s) => return write!(f, "hci 0x{:02x}", u8::from(*s)),
            // Bluetooth Core, Vol 3, Part F, 3.4.1.1: ATT error codes.
            H::Att(c) => return write!(f, "att 0x{:02x}", c.to_u8()),
            H::Security(r) => return write!(f, "security {r:?}"),
            H::HciDecode(_) => "HciDecode",
            H::InsufficientSpace => "InsufficientSpace",
            H::InvalidValue => "InvalidValue",
            H::UnexpectedDataLength { .. } => "UnexpectedDataLength",
            H::CannotConstructGattValue(_) => "CannotConstructGattValue",
            H::ConfigFilterAcceptListIsEmpty => "ConfigFilterAcceptListIsEmpty",
            H::UnexpectedGattResponse => "UnexpectedGattResponse",
            H::MalformedCharacteristicDeclaration { .. } => "MalformedCharacteristicDeclaration",
            H::InvalidCharacteristicDeclarationData => "InvalidCharacteristicDeclarationData",
            H::FailedToFinalize { .. } => "FailedToFinalize",
            H::CodecError(_) => "CodecError",
            H::ExtendedAdvertisingNotSupported => "ExtendedAdvertisingNotSupported",
            H::InvalidUuidLength(_) => "InvalidUuidLength",
            H::Advertisement(_) => "Advertisement",
            H::L2capConnectError(_) => "L2capConnectError",
            H::InvalidChannelId => "InvalidChannelId",
            H::NoChannelAvailable => "NoChannelAvailable",
            H::NotFound => "NotFound",
            H::InvalidState => "InvalidState",
            H::OutOfMemory => "OutOfMemory",
            H::NotSupported => "NotSupported",
            H::ChannelClosed => "ChannelClosed",
            H::Timeout => "Timeout",
            H::Busy => "Busy",
            H::NoPermits => "NoPermits",
            H::Disconnected => "Disconnected",
            H::ConnectionLimitReached => "ConnectionLimitReached",
            H::GattSubscriberLimitReached => "GattSubscriberLimitReached",
            H::AlreadyInUse => "AlreadyInUse",
            H::Other => "Other",
        };
        f.write_str(name)
    }
}
