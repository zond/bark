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
//! The link is the paired phones' (spec "Phone link", Pairing; roadmap
//! D135, D168): every characteristic of the service needs a link
//! encrypted with an authenticated key, the node shows the passkey of a
//! pairing on its screen, and it keeps a record for each phone paired,
//! up to eight, in flash (`phones.rs`, `store.rs`), each phone an owner.
//! On a public node guests use it too (spec "Phone link", Public; roadmap
//! D150): a link for each paired phone and one more, [`LINKS`] at most,
//! each served by a slot of its own ([`slot`]), the characteristics a
//! guest needs open on any of them ([`allowed`]), a guest's mail read from
//! the ring by its recipient tag ([`Request::Guest`]; `mailbox.rs`), and
//! the one message the node holds for its phones taken in turn
//! ([`Turn`]).
//!
//! A paired phone keeps the attribute table it discovered; the node tells
//! it when the table has changed since, by the link version its record
//! keeps ([`tell_of_change`]; spec "Phone link", Changes to the table).
//!
//! A paired phone stages a new image through Update and Image, which this
//! task serves itself, across its connections (`update::Phone`; spec
//! "Phone link", Updates).

use core::cell::RefCell;

use bark_image::policy::Links;
use bark_link::neighbours;
use bark_link::phones::{Keys, RECORDS};
use bark_link::turn::Turn;
use bark_link::update::{Command, Piece, Progress, Refusal};
use bark_link::{
    le_bytes, seconds_left, uuid, GivenBack, Malformed, Message, Node, Status, ATT_MTU,
};
use bark_proto::wire::MAX_PAYLOAD;
use bark_proto::{Instant, MsgId, NodeId, Position};
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

/// Links at once: one for each phone the node keeps a record for and one
/// more, for a phone that pairs or a guest (spec "Phone link", Pairing;
/// roadmap D168). Each costs 5.6 KB of static RAM, the controller's
/// [`PER_LINK`], the host's 1.2 KB and its slot's task, all taken from the
/// stack (`bark-fw/README.md`, "Tasks and the stack").
pub const LINKS: usize = RECORDS + 1;
/// Room for L2CAP's connection-oriented channels (Vol 3 Part A §10),
/// none: the node registers no protocol for them, so it opens none and
/// refuses every one a phone asks for, and ATT and the signalling run on
/// fixed channels, which take no room (Vol 3 Part A §2.1).
const CHANNELS: usize = 0;
/// Advertising sets: the one advertisement.
const ADV_SETS: usize = 1;
/// Bonds the host holds: the records', and the one a pairing adds before
/// [`settle`] takes it into a record or away.
const BONDS: usize = RECORDS + 1;
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
    pub inbox: heapless::Vec<u8, { bark_link::GuestItem::MAX_LEN }>,
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
    #[characteristic(uuid = le_bytes(uuid::GUEST), write)]
    pub guest: (),
    #[characteristic(uuid = le_bytes(uuid::ASK), read, write, notify, permissions(authenticated))]
    pub ask: heapless::Vec<u8, { bark_link::Answer::LEN }>,
    #[characteristic(uuid = le_bytes(uuid::PHONES), read, write, permissions(authenticated))]
    pub phones: heapless::Vec<u8, { bark_link::phones::MAX_LEN }>,
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
    /// The page of the neighbour table from this slot, which the phone
    /// reads now.
    Neighbours(u8),
    /// The Inbox the paired phone in `slot`, of record `record`, reads
    /// now.
    Inbox { slot: usize, record: usize },
    /// What the guest in this link's slot reads from Guest now: the oldest
    /// item after `after` for its tag.
    Guest { slot: usize, tag: u8, after: u32 },
    /// Ask this neighbour for its status (spec "Neighbour status").
    Ask(NodeId),
}

/// Which phone a request is from: its link's slot, and the record whose
/// bond its link is encrypted with, a paired phone's.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct From {
    pub slot: usize,
    pub record: Option<usize>,
}

impl From {
    /// Whether it is a paired phone's.
    pub fn owner(&self) -> bool {
        self.record.is_some()
    }
}

/// A phone connected in a slot.
#[derive(Clone, Copy)]
struct Peer {
    /// A link is up in the slot.
    live: bool,
    /// Counts the slot's links, so that a message given back is not handed
    /// to a later phone in the slot.
    link: u32,
    /// The record whose bond the link is encrypted with: a paired
    /// phone's.
    record: Option<usize>,
    /// The recipient tag it wrote (spec "Phone link", Public).
    tag: Option<u8>,
    /// The number of the last item it took from Guest, as it wrote it.
    after: u32,
    /// A pairing runs on its link.
    pairing: bool,
    /// A pairing on its link would be refused, every record being in use
    /// ([`refresh`]).
    refuses: bool,
    /// A reply to its write or a notification is on its way to it, until
    /// it is sent.
    sending: bool,
    /// A guest's message given back, and the end of its wait; a paired
    /// phone's is its record's, [`State::given_back`].
    given_back: Option<(MsgId, Instant)>,
    /// The slot it reads the neighbour table from (spec "Phone link",
    /// Neighbours); 0 at each connection.
    neighbours_from: u8,
    /// The place it reads the records from (spec "Phone link", Phones); 0
    /// at each connection.
    phones_from: u8,
}

impl Peer {
    const NONE: Peer = Peer {
        live: false,
        link: 0,
        record: None,
        tag: None,
        after: 0,
        pairing: false,
        refuses: false,
        sending: false,
        given_back: None,
        neighbours_from: 0,
        phones_from: 0,
    };
}

/// What the node publishes and the phones read, and the request waiting.
pub struct State {
    /// The status but for the countdown, which is read against `boot_ends`,
    /// and what each phone reads of it alone.
    pub status: Status,
    /// When the first rotation ends, while it runs.
    pub boot_ends: Option<Instant>,
    /// For each record, the item its phone's Inbox shows, the oldest of
    /// its person's after the last confirmed: its number and the ID it
    /// arrived with (`mailbox.rs`), which the node writes as the phone
    /// reads it ([`Request::Inbox`]).
    pub heads: [Option<(u32, MsgId)>; RECORDS],
    /// For each record, its person's items not yet confirmed, as Status
    /// says them to its phone.
    counts: [u8; RECORDS],
    /// For each record, its phone's message last given back and the end of
    /// its wait, until the node takes another; kept across its links.
    given_back: [Option<(MsgId, Instant)>; RECORDS],
    /// The records' numbers as last followed ([`records_changed`]).
    numbers: [Option<u16>; RECORDS],
    /// The node's Node value but for `room`, which [`refresh`] reads from
    /// the records.
    node: Option<Node>,
    /// The last neighbour's status the node was told, which Ask reads
    /// (spec "Phone link", Ask).
    told_status: Option<bark_link::Answer>,
    /// The phones, by their links' slots.
    phones: [Peer; LINKS],
    /// The message the node took last from a phone: its ID, slot and link,
    /// to give it back to that phone.
    took: Option<(MsgId, From, u32)>,
    /// Whose turn it is at the one message the node holds for its phones,
    /// and so whose Ready is set (spec "Phone link", Sending).
    turn: Turn<LINKS>,
    /// The node is public, as it is unless its records say private.
    public: bool,
    /// The phone's request, until the node answers it.
    pub request: Option<(From, Request)>,
    /// The node's answer, until the step that gave it has published what
    /// it changed ([`answer`]).
    answer: Option<bool>,
    /// What the phones were last told of the status and of their counts,
    /// for when to tell them again.
    told: Status,
    told_counts: [u8; RECORDS],
    /// The flags byte last logged.
    flags: Option<u8>,
}

impl State {
    /// Move the turn on to `now` (`bark_link::turn`). Whether it moved.
    fn advance(&mut self, now: Instant) -> bool {
        let ready = self.status.ready;
        self.turn.advance(now, ready)
    }

    /// Whether `slot` is a guest's link: the node is public and the link
    /// is not a paired phone's.
    fn guest(&self, slot: usize) -> bool {
        self.public && self.phones[slot].record.is_none()
    }

    /// The live links of record `i`'s phone: one, as a link is known by
    /// its phone's identity.
    fn slots_of(&self, i: usize) -> impl Iterator<Item = usize> + '_ {
        (0..LINKS).filter(move |&s| self.phones[s].live && self.phones[s].record == Some(i))
    }
}

/// A value the node writes when a phone reads it, so that the link keeps
/// no copy of what the core holds: a page of the neighbour table (spec
/// "Phone link", Neighbours) or the message waiting for a guest (Public),
/// with which request it answers, as the value the phone reads is set from
/// it with no wait between ([`Request::Neighbours`], [`Request::Inbox`],
/// [`Request::Guest`]).
static READ: Mutex<CriticalSectionRawMutex, RefCell<(Option<Read>, ReadValue)>> =
    Mutex::new(RefCell::new((None, heapless::Vec::new())));

/// The bytes of a value read as [`READ`] holds them.
type ReadValue = heapless::Vec<u8, READ_LEN>;

/// What [`READ`] holds.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Read {
    Neighbours,
    /// The Inbox for the phone in this slot.
    Inbox(usize),
    /// Guest for the phone in this slot.
    Guest(usize),
}
const READ_LEN: usize = neighbours::MAX_LEN;
const _: () = assert!(Message::MAX_LEN <= READ_LEN);
const _: () = assert!(bark_link::GuestItem::MAX_LEN <= READ_LEN);

/// The state both sides share.
pub static LINK: Mutex<CriticalSectionRawMutex, RefCell<State>> = Mutex::new(RefCell::new(State {
    status: Status::EMPTY,
    boot_ends: None,
    heads: [None; RECORDS],
    counts: [0; RECORDS],
    given_back: [None; RECORDS],
    numbers: [None; RECORDS],
    node: None,
    told_status: None,
    phones: [Peer::NONE; LINKS],
    took: None,
    turn: Turn::new(),
    public: true,
    request: None,
    answer: None,
    told: Status::EMPTY,
    told_counts: [0; RECORDS],
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

/// What to notify, and whether a guest's link, or the link itself, is to
/// close.
#[derive(Clone, Copy, Default)]
struct Notify {
    status: bool,
    given_back: bool,
    answer: bool,
    close_guests: bool,
    /// The link is to end: its phone's bond was removed
    /// (spec "Phone link", Phones), or the node reboots.
    close: bool,
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
            answer: o.answer || n.answer,
            close_guests: o.close_guests || n.close_guests,
            close: o.close || n.close,
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

/// The node's side: publish the status and, for each record, the
/// oldest of its person's items its phone has not confirmed and their
/// count (spec "Phone link", Receiving; "Mailbox"), after a step, and
/// notify the phones of a change of the flags, the relays, the neighbours
/// heard, a count or the address, or of the battery by a step (spec
/// "Phone link", Status).
pub fn publish(status: Status, heads: [Option<(u32, MsgId)>; RECORDS], counts: [u8; RECORDS]) {
    let changed = LINK.lock(|l| {
        let mut l = l.borrow_mut();
        l.heads = heads;
        l.counts = counts;
        l.status = status;
        let told = l.told;
        let quiet = |s: Status| Status {
            boot_left_s: 0,
            battery_mv: 0,
            ..s
        };
        let changed = quiet(told) != quiet(status)
            || told.battery_mv.abs_diff(status.battery_mv) >= BATTERY_STEP_MV
            || l.told_counts != counts;
        if changed {
            l.told = status;
            l.told_counts = counts;
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
            "ble: status flags={:#04x} relays={} heard={} address={}",
            flags,
            status.relays,
            status.heard,
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

/// Whether the node is public (spec "Phone link", Public).
pub fn public() -> bool {
    LINK.lock(|l| l.borrow().public)
}

/// The node's side: the value a phone reads now, if one asks
/// ([`READ`]), written by `write` for the request into its buffer.
pub fn serve_read(write: impl FnOnce(&Request, &mut [u8; READ_LEN]) -> usize) {
    take_request(|_, r| match *r {
        Request::Neighbours(_) | Request::Inbox { .. } | Request::Guest { .. } => {
            let mut b = [0; READ_LEN];
            let n = write(r, &mut b);
            READ.lock(|v| {
                let mut v = v.borrow_mut();
                v.0 = Some(match *r {
                    Request::Inbox { slot, .. } => Read::Inbox(slot),
                    Request::Guest { slot, .. } => Read::Guest(slot),
                    _ => Read::Neighbours,
                });
                v.1.clear();
                let _ = v.1.extend_from_slice(&b[..n]);
            });
            Some(((), true))
        }
        _ => None,
    });
}

/// The node's side: the first rotation's end, from `AppEvent::Boot`.
pub fn booting(ends: Option<Instant>) {
    LINK.lock(|l| l.borrow_mut().boot_ends = ends);
}

/// The node's side: a phone's message was given back, to the phone it
/// came from: a guest's while its link lasts, a paired phone's kept with
/// its record for its next (spec "Phone link", Given back, Public).
pub fn given_back(msg_id: MsgId, retry_at: Instant) {
    let to = LINK.lock(|l| {
        let mut l = l.borrow_mut();
        match l.took.filter(|&(id, ..)| id == msg_id) {
            Some((_, from, link)) => match from.record {
                None => {
                    let p = &mut l.phones[from.slot];
                    let here = p.live && p.link == link;
                    if here {
                        p.given_back = Some((msg_id, retry_at));
                    }
                    here.then_some(from.slot)
                }
                Some(i) => {
                    l.given_back[i] = Some((msg_id, retry_at));
                    l.slots_of(i).next()
                }
            },
            None => {
                log::warn!("ble: given back id={:08x}, whose is not known", msg_id.0);
                None
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

/// The node's side: a neighbour told its status, for every paired phone
/// (spec "Phone link", Ask).
pub fn told_status(answer: bark_link::Answer) {
    let to = LINK.lock(|l| {
        let mut l = l.borrow_mut();
        l.told_status = Some(answer);
        let owners = (0..LINKS).filter(|&i| l.phones[i].live && l.phones[i].record.is_some());
        owners.fold(0u16, |m, i| m | 1 << i)
    });
    for slot in (0..LINKS).filter(|&i| to & (1 << i) != 0) {
        notify_one(
            slot,
            Notify {
                answer: true,
                ..Notify::default()
            },
        );
    }
}

/// The last answer as Ask reads it: empty before the first.
fn answer_now() -> heapless::Vec<u8, { bark_link::Answer::LEN }> {
    LINK.lock(|l| match l.borrow().told_status {
        Some(a) => heapless::Vec::from_slice(&a.encode()).expect("it fits"),
        None => heapless::Vec::new(),
    })
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
                match from.record {
                    Some(i) => l.given_back[i] = None,
                    None => l.phones[from.slot].given_back = None,
                }
                let id = MsgId::of(dst_pos, dst_id, &payload[..usize::from(len)]);
                l.took = Some((id, from, l.phones[from.slot].link));
                l.turn.taken();
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

/// Move the turn on to now, and tell every phone if it moved.
fn turn_moved() {
    if LINK.lock(|l| l.borrow_mut().advance(now())) {
        notify(Notify {
            status: true,
            ..Notify::default()
        });
    }
}

fn now() -> Instant {
    Instant::from_micros(embassy_time::Instant::now().as_micros())
}

/// The status as the phone in `slot` reads it now: ready in its turn
/// alone, and its count its own person's items not confirmed, none for a
/// guest.
fn status_now(slot: usize) -> Status {
    LINK.lock(|l| {
        let mut l = l.borrow_mut();
        let now = now();
        l.advance(now);
        let inbox = l.phones[slot].record.map_or(0, |i| l.counts[i]);
        Status {
            ready: l.status.ready && l.request.is_none() && l.turn.holder() == Some(slot),
            boot_left_s: l.boot_ends.map_or(0, |e| seconds_left(now, e)),
            inbox,
            ..l.status
        }
    })
}

fn given_back_now(slot: usize) -> heapless::Vec<u8, { GivenBack::LEN }> {
    LINK.lock(|l| {
        let l = l.borrow();
        let given = match l.phones[slot].record {
            Some(i) => l.given_back[i],
            None => l.phones[slot].given_back,
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

/// What the node last wrote, if it was `read`.
fn read_for(read: Read) -> heapless::Vec<u8, READ_LEN> {
    READ.lock(|v| {
        let v = v.borrow();
        match v.0 == Some(read) {
            true => v.1.clone(),
            false => heapless::Vec::new(),
        }
    })
}

/// The page of the neighbour table as the node last wrote it.
fn neighbours_now() -> heapless::Vec<u8, { neighbours::MAX_LEN }> {
    read_for(Read::Neighbours)
}

/// The Inbox the phone in `slot` reads, as the node wrote it as the phone
/// read: the paired phone's message, or a guest's item from the ring.
fn inbox_now(slot: usize) -> heapless::Vec<u8, { bark_link::GuestItem::MAX_LEN }> {
    let guest = LINK.lock(|l| l.borrow().guest(slot));
    let read = match guest {
        true => Read::Guest(slot),
        false => Read::Inbox(slot),
    };
    heapless::Vec::from_slice(&read_for(read)).expect("it fits")
}

type Stack = trouble_host::Stack<'static, SoftdeviceController<'static>, DefaultPacketPool>;

/// The update a paired phone stages, across its links.
type Update = embassy_sync::mutex::Mutex<NoopRawMutex, Staging>;

/// The update, and room for the header a begin carries, kept here rather
/// than in every link's task, as one is staged at a time.
pub struct Staging {
    phone: crate::update::Phone,
    header: [u8; bark_link::update::HEADER_LEN],
}

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
    resolving_list();
    for i in 0..RECORDS {
        let Some(r) = crate::phones::read(|p| p.get(i).copied()) else {
            continue;
        };
        let Some(b) = r.bond else {
            log::info!("ble: record {} number {}, a home, no bond", i, r.number);
            continue;
        };
        match stack.add_bond_information(information(&b)) {
            Ok(()) => log::info!(
                "ble: record {} number {} table seen at version {}, service changed {}, home {}, {}",
                i,
                r.number,
                b.seen,
                b.service_changed,
                r.key.is_some(),
                Whose(&b)
            ),
            Err(e) => log::error!("ble: record {}'s bond {}", i, HostError(&e)),
        }
    }
    records_changed();
    static NAME: StaticCell<[u8; bark_link::NAME_LEN]> = StaticCell::new();
    let name = core::str::from_utf8(NAME.init(bark_link::name(id))).expect("the name is ASCII");
    static SERVER: StaticCell<(Server<'static>, Gatt)> = StaticCell::new();
    let (server, gatt) = SERVER.init(server(name));
    let node = Node {
        version: bark_link::VERSION,
        id,
        image,
        room: None,
        oldest: Some(bark_link::OLDEST),
    };
    LINK.lock(|l| l.borrow_mut().node = Some(node));
    static ADVERTISING: StaticCell<Advertising> = StaticCell::new();
    let advertising = ADVERTISING.init(Advertising::new(stack.peripheral()));
    static UPDATE: StaticCell<Update> = StaticCell::new();
    let update = UPDATE.init(Update::new(Staging {
        phone: crate::update::Phone::new(internal, flash),
        header: [0; bark_link::update::HEADER_LEN],
    }));
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
                if links() >= room_for_links() {
                    ADVERTISE.wait().await;
                    continue;
                }
                // Stopped and begun again as the setting changes, a link
                // ends or a phone pairs, which may free a link.
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
            let link = p.link;
            l.turn.linked(i);
            link
        });
        log::info!("ble: link {} in slot {}", link, i);
        serve(i, stack, server, gatt, &conn, update).await;
        LINK.lock(|l| {
            let mut l = l.borrow_mut();
            let link = l.phones[i].link;
            l.phones[i] = Peer { link, ..Peer::NONE };
            l.turn.ended(i);
        });
        // A message waiting for this guest alone is now nobody's, and its
        // turn passes on.
        crate::mailbox::FOR_CORE.signal(());
        ADVERTISE.signal(());
        ENDED.signal(());
        turn_moved();
    }
}

/// Links up now.
pub fn links() -> usize {
    LINK.lock(|l| l.borrow().phones.iter().filter(|p| p.live).count())
}

/// Links the node takes: one for each phone paired and one more (spec
/// "Phone link", Finding the node).
fn room_for_links() -> usize {
    (crate::phones::read(|p| p.paired()) + 1).min(LINKS)
}

/// What the links are doing, for a reboot into an image gathered over
/// the mesh (`bark_image::policy::quiet`). Mid-way is what a reset would
/// cut: a request written that the node has not answered, or whose reply
/// is not yet sent, a notification not yet sent, such as a message given
/// back, and a pairing. A link that is up and doing none of these is
/// idle however long it stays.
pub fn doing() -> Links {
    LINK.lock(|l| {
        let l = l.borrow();
        let live = || (0..LINKS).filter(|&i| l.phones[i].live);
        let mid_way = l.request.is_some()
            || l.answer.is_some()
            || live().any(|i| {
                let p = &l.phones[i];
                p.sending || p.pairing || NOTIFY[i].signaled()
            });
        match (mid_way, live().next()) {
            (true, _) => Links::MidWay,
            (false, Some(_)) => Links::Idle,
            (false, None) => Links::None,
        }
    })
}

/// Raised when a link has ended.
static ENDED: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// End every link before a reboot, so that each phone sees the node leave
/// at once rather than after its link's supervision timeout; resolves
/// once they have ended, or after the longest such timeout.
pub async fn close_all() {
    notify(Notify {
        close: true,
        ..Notify::default()
    });
    let ended = async {
        while links() > 0 {
            ENDED.wait().await;
        }
    };
    let _ = embassy_time::with_timeout(SUPERVISION_MAX, ended).await;
}

/// The records changed: the links, the advertising, the node's loop and
/// the phones follow what they say now (the mailbox and the screen follow
/// in `phones.rs`).
fn records_changed() {
    let (public, numbers) =
        crate::phones::read(|p| (p.public(), p.records.map(|r| r.map(|r| r.number))));
    let was = LINK.lock(|l| {
        let mut l = l.borrow_mut();
        // A message given back to a record's phone is not a new record's.
        let l = &mut *l;
        for ((given, was), now) in l.given_back.iter_mut().zip(&l.numbers).zip(numbers) {
            if *was != now {
                *given = None;
            }
        }
        l.numbers = numbers;
        core::mem::replace(&mut l.public, public)
    });
    ADVERTISE.signal(());
    crate::mailbox::FOR_CORE.signal(());
    notify(Notify {
        status: true,
        close_guests: was && !public,
        ..Notify::default()
    });
}

/// The host holds the records' bonds and no other: one a pairing added
/// that no record took goes, and one a record holds that the host does
/// not comes back.
fn hold_records(stack: &Stack) {
    let mut want: heapless::Vec<BondInformation, RECORDS> = heapless::Vec::new();
    for b in crate::phones::read(|p| p.records.map(|r| r.and_then(|r| r.bond)))
        .into_iter()
        .flatten()
    {
        let _ = want.push(information(&b));
    }
    let mut other: heapless::Vec<Identity, BONDS> = heapless::Vec::new();
    let mut held: heapless::Vec<BondInformation, BONDS> = heapless::Vec::new();
    stack.with_bond_information(|bonds| {
        for b in bonds {
            match want.contains(b) {
                true => {
                    let _ = held.push(b.clone());
                }
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
    for b in want.into_iter().filter(|b| !held.contains(b)) {
        if let Err(e) = stack.add_bond_information(b) {
            log::error!("ble: a record's bond {}", HostError(&e));
        }
    }
}

/// The phone of record `i` unpairs (spec "Phone link", Public): its bond
/// goes, in the host and in flash, and its home stays; with the last bond
/// gone the node is public, as one nobody has paired with is, and its
/// screen says `PHONE UNPAIRED`.
fn unpair(stack: &Stack, i: usize) {
    crate::phones::with(|p| p.unpair(i));
    hold_records(stack);
    log::info!("ble: record {} unpaired by its phone; its home stays", i);
    records_changed();
}

/// The controller's resolving list, which holds each bonded phone's
/// identity key so that the controller knows the phone's changing address
/// (Bluetooth Core Vol 6 Part B §4.7): logged at boot against the
/// records, for the bench, as the controller's documentation does not
/// give its size.
fn resolving_list() {
    let mut out = sdc::raw::sdc_hci_cmd_le_read_resolving_list_size_return_t {
        resolving_list_size: 0,
    };
    // SAFETY: the controller is built before the host runs, and its
    // commands are synchronous calls from thread mode, so none is in
    // progress when this one starts.
    let status = unsafe { sdc::raw::sdc_hci_cmd_le_read_resolving_list_size(&mut out) };
    let size = out.resolving_list_size;
    match (status, usize::from(size) >= RECORDS) {
        (0, true) => log::info!(
            "ble: resolving list holds {} keys, the {} records need {}",
            size,
            RECORDS,
            RECORDS
        ),
        (0, false) => log::error!(
            "ble: resolving list holds {} keys, the {} records need {}: phones past it are not known by their address",
            size,
            RECORDS,
            RECORDS
        ),
        (e, _) => log::error!("ble: LE Read Resolving List Size failed: {:#x}", e),
    }
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
    // The record whose bond's identity, by its address or its IRK, is
    // this peer's: a bonded phone that has none is refused its key by the
    // host, and its link ends at once (spec "Phone link", The removed
    // phone).
    let peer = conn.peer_address();
    let bonded = crate::phones::read(|p| {
        (0..RECORDS).find(|&i| {
            p.get(i)
                .and_then(|r| r.bond)
                .is_some_and(|b| information(&b).identity.match_address(&peer))
        })
    });
    log::info!(
        "ble: connected peer={:?} record={:?} interval_us={} latency={} timeout_ms={}",
        peer,
        bonded,
        p.conn_interval.as_micros(),
        p.peripheral_latency,
        p.supervision_timeout.as_millis()
    );
    Ok(conn.with_attribute_server(server)?)
}

/// The record whose phone the link is: encrypted with an authenticated
/// key, which only a record's bond gives, as the host holds no other
/// ([`hold_records`]), and the record whose bond names the peer.
fn owner(conn: &GattConnection<'_, '_, DefaultPacketPool>) -> Option<usize> {
    let raw = conn.raw();
    if raw.security_level().ok() != Some(SecurityLevel::EncryptedAuthenticated) {
        return None;
    }
    let peer = raw.peer_identity();
    crate::phones::read(|p| {
        (0..RECORDS).find(|&i| {
            p.get(i)
                .and_then(|r| r.bond)
                .is_some_and(|b| information(&b).identity.match_identity(&peer))
        })
    })
}

/// The phone's identity address, as the host knows it: a pairing from a
/// phone a record's bond names is taken on a full node
/// (spec "Phone link", Pairing).
fn identity(conn: &GattConnection<'_, '_, DefaultPacketPool>) -> (u8, [u8; 6]) {
    let a = conn.raw().peer_identity().addr;
    let mut addr = [0u8; 6];
    addr.copy_from_slice(a.addr.raw());
    (a.kind.into_inner(), addr)
}

/// The handles a guest may use on a public node: the values of Node,
/// Status, Send, Inbox, Given back, Recipient and Guest, and the
/// subscriptions to Status and Given back (spec "Phone link", Public).
fn open_handles(link: &Link) -> [u16; 9] {
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
        link.guest.handle,
    ]
}

/// Whether the link may make the request `att`: a paired phone's may
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
    let ends = embassy_time::Instant::now() + GUEST_LINK;
    loop {
        // A guest's link, on a public node, ends at `ends`: neither a
        // paired phone's nor one that pairs.
        let pairing = LINK.lock(|l| l.borrow().phones[slot].pairing);
        let guest = public() && !pairing && owner(conn).is_none();
        if guest && embassy_time::Instant::now() >= ends {
            log::info!("ble: the guest in slot {} closed after its time", slot);
            conn.raw().disconnect();
        }
        let next = LINK.lock(|l| l.borrow().turn.ends());
        let next = match (next, guest) {
            (Some(t), true) => Some(embassy_time::Instant::from_micros(t.as_micros()).min(ends)),
            (Some(t), false) => Some(embassy_time::Instant::from_micros(t.as_micros())),
            (None, true) => Some(ends),
            (None, false) => None,
        };
        let turn = async {
            match next {
                Some(t) => embassy_time::Timer::at(t).await,
                None => core::future::pending().await,
            }
        };
        let event = match select3(conn.next(), NOTIFY[slot].wait(), turn).await {
            Either3::First(e) => e,
            Either3::Second(n) => {
                LINK.lock(|l| l.borrow_mut().phones[slot].sending = true);
                // A guest's link ends with the public setting.
                if n.close_guests && owner(conn).is_none() {
                    log::info!("ble: no longer public; the guest in slot {} closed", slot);
                    conn.raw().disconnect();
                }
                // Its phone's bond was removed by another phone, or the node
                // reboots.
                if n.close {
                    log::info!("ble: the link in slot {} closed", slot);
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
                if n.answer && owner(conn).is_some() {
                    let _ = link.ask.notify(conn, &answer_now(), true).await;
                }
                LINK.lock(|l| l.borrow_mut().phones[slot].sending = false);
                continue;
            }
            Either3::Third(()) => {
                // The turn passed on unused, or a guest's time is up, which
                // the loop's start sees.
                turn_moved();
                continue;
            }
        };
        let event = match event {
            GattConnectionEvent::Disconnected { reason } => {
                log::info!("ble: disconnected 0x{:02x} slot {}", u8::from(reason), slot);
                if owner(conn).is_some() {
                    update.lock().await.phone.link_closed();
                }
                // A pairing cut short leaves no code on the screen.
                if LINK.lock(|l| l.borrow().phones[slot].pairing) {
                    crate::display::pairing(None);
                }
                return;
            }
            GattConnectionEvent::PassKeyDisplay(key) => {
                // The code only in an open build, for the bench: a
                // protected node's log must not let a USB cable stand in
                // for being at the node (spec "Phone link", Pairing).
                match crate::protect::BUILD {
                    bark_image::protect::Build::Open => {
                        log::info!("ble: pairing, the code shown: {:06}", key.value())
                    }
                    bark_image::protect::Build::Protected => {
                        log::info!("ble: pairing, the code shown")
                    }
                }
                LINK.lock(|l| l.borrow_mut().phones[slot].pairing = true);
                crate::display::pairing(Some(key.value()));
                continue;
            }
            GattConnectionEvent::PairingComplete {
                security_level,
                bond,
            } => {
                settle(stack, conn, security_level, bond);
                crate::display::pairing(None);
                LINK.lock(|l| {
                    let p = &mut l.borrow_mut().phones[slot];
                    p.record = owner(conn);
                    p.pairing = false;
                });
                linked(conn);
                continue;
            }
            GattConnectionEvent::PairingFailed(e) => {
                let refused = LINK.lock(|l| {
                    let mut l = l.borrow_mut();
                    l.phones[slot].pairing = false;
                    l.phones[slot].refuses
                });
                if refused {
                    log::info!(
                        "ble: pairing refused, every record in use; the screen says PHONES FULL"
                    );
                    crate::display::full();
                } else {
                    log::info!("ble: pairing failed {}", HostError(&e));
                    crate::display::pairing(None);
                }
                settle(stack, conn, SecurityLevel::NoEncryption, None);
                continue;
            }
            GattConnectionEvent::Encrypted { security_level, .. } => {
                log::info!("ble: encrypted {:?}", security_level);
                LINK.lock(|l| l.borrow_mut().phones[slot].record = owner(conn));
                linked(conn);
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
            record: owner(conn),
        };
        if !allowed(from.owner(), public(), link, &incoming) {
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
        // A read of Neighbours asks the node for the page first: the table
        // is the core's, which only the node's task reads.
        if !continued && from.owner() && read_of(&incoming) == Some(link.neighbours.handle) {
            let start = LINK.lock(|l| l.borrow().phones[slot].neighbours_from);
            ask(from, Request::Neighbours(start)).await;
        }
        if !continued && read_of(&incoming) == Some(link.inbox.handle) {
            let (guest, tag, after) = LINK.lock(|l| {
                let l = l.borrow();
                (l.guest(slot), l.phones[slot].tag, l.phones[slot].after)
            });
            match (from.record, guest, tag) {
                (Some(record), ..) => {
                    ask(from, Request::Inbox { slot, record }).await;
                }
                (None, true, Some(tag)) => {
                    ask(from, Request::Guest { slot, tag, after }).await;
                }
                // Without a tag a guest has no mail to read, and a link
                // that is neither reads nothing.
                _ => {}
            }
        }
        if !continued && !piece {
            refresh(slot, server, conn, update);
        }
        let writing = written(&incoming).is_some();
        LINK.lock(|l| l.borrow_mut().phones[slot].sending = writing);
        let mut wrote_changes = false;
        let mut reboot = false;
        let mut close = false;
        let reply = match event {
            GattEvent::Write(w) => {
                let h = w.handle();
                wrote_changes = h == changes.0;
                let answer = if h == link.image.handle {
                    let taken = write_image(&w, &mut update.lock().await.phone).await;
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
                    let _ = conn.set(&link.update, &progress(&u.phone));
                    Some(answer.map(|_| ()))
                } else if h == link.send.handle {
                    Some(write_send(from, &w).await)
                } else if h == link.inbox.handle {
                    Some(write_confirm(from, &w).await)
                } else if h == link.map_point.handle {
                    Some(write_map_point(from, &w).await)
                } else if h == link.clock.handle {
                    let r = write_clock(from, &w).await;
                    // The node may have the time only now.
                    linked(conn);
                    Some(r)
                } else if h == link.home.handle {
                    Some(write_home(from, &w))
                } else if h == link.phones.handle {
                    Some(write_phones(from, stack, &w))
                } else if h == link.neighbours.handle {
                    Some(write_neighbours(slot, &w))
                } else if h == link.ask.handle {
                    Some(write_ask(from, &w).await)
                } else if h == link.recipient.handle {
                    let (r, end) = write_recipient(from, &w);
                    close = end;
                    Some(r)
                } else if h == link.guest.handle {
                    Some(write_guest(from, &w))
                } else if h == link.public.handle {
                    let r = write_public(from, &w);
                    if let (Ok(true), Some(i)) = (r, from.record) {
                        unpair(stack, i);
                        // The link was a paired phone's, which it is no
                        // longer.
                        close = true;
                    }
                    Some(r.map(|_| ()))
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
        LINK.lock(|l| l.borrow_mut().phones[slot].sending = false);
        if wrote_changes {
            tell_of_change(gatt, &changes, conn, true).await;
        }
        if close {
            conn.raw().disconnect();
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

/// How long a guest's link lasts at most, the ATT transaction's timeout
/// (Vol 3 Part F §3.3.3): longer than a guest's errand, a tag, a message
/// and a few reads, so that a guest that stays holds the one guest link
/// no longer (spec "Phone link", Public).
const GUEST_LINK: embassy_time::Duration = embassy_time::Duration::from_secs(30);

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

/// Bring a paired phone's attribute table up to date
/// (spec "Phone link", Changes to the table): on its link, encrypted with
/// its record's bond, keep its subscription to Service Changed with the
/// bond, as a bonded client's is kept across connections (Vol 3 Part G
/// §3.3.3.3), and while it is subscribed and the table it last saw is of
/// another link version, indicate Service Changed over every handle; once
/// the phone confirms, it has the table of this version. `written` is
/// whether the phone has just written its subscription, which then stands
/// as written; else the kept one is put back.
async fn tell_of_change(
    gatt: &Gatt,
    changes: &Cccd,
    conn: &GattConnection<'_, '_, DefaultPacketPool>,
    written: bool,
) {
    let Some(i) = owner(conn) else {
        return;
    };
    let Some(mut bond) = crate::phones::read(|p| p.get(i).and_then(|r| r.bond)) else {
        return;
    };
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
            "ble: service changed: the phone of record {} saw version {}, this is {}",
            i,
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
        crate::phones::with(|p| p.set_keys(i, bond));
    }
}

/// The paired phone on `conn` is linked today: its record keeps the day,
/// written when it changes (spec "Phone link", Phones).
fn linked(conn: &GattConnection<'_, '_, DefaultPacketPool>) {
    let Some(i) = owner(conn) else {
        return;
    };
    let day = crate::phones::today();
    let known = crate::phones::read(|p| p.get(i).map(|r| r.day));
    if day != 0 && known.is_some_and(|d| d != day) {
        crate::phones::with(|p| p.linked(i, day));
    }
}

/// After a pairing: an authenticated pairing that bonds, by the code on
/// the node's screen, so someone was at the node, takes a record, or
/// replaces the bond of the record whose bond names the same phone; on a
/// node with every record in use and a phone paired it is refused, and
/// its link closed (spec "Phone link", Pairing). A Just Works pairing, or
/// one that does not bond, changes no record, and its link is closed. The
/// host then holds the records' bonds and no other, so that a refused
/// pairing leaves every record's bond as it was, even one the host
/// replaced as the same phone's.
fn settle(
    stack: &Stack,
    conn: &GattConnection<'_, '_, DefaultPacketPool>,
    level: SecurityLevel,
    bond: Option<BondInformation>,
) {
    let authenticated = level == SecurityLevel::EncryptedAuthenticated;
    let paired = bond
        .as_ref()
        .filter(|b| b.is_bonded && authenticated)
        .map(keys_of);
    match paired {
        Some(keys) => {
            let newest = crate::mailbox::with(|m| m.newest()).unwrap_or(0);
            match crate::phones::with(|p| p.pair(keys, newest)) {
                Ok(how) => {
                    log::info!("ble: paired, {:?}, {}", how, Whose(&keys));
                    records_changed();
                }
                Err(bark_link::phones::Full) => {
                    log::warn!("ble: paired, but every record is in use; refused and closed");
                    crate::display::full();
                    conn.raw().disconnect();
                }
            }
        }
        None if level != SecurityLevel::NoEncryption => {
            log::warn!("ble: a pairing that is not authenticated or does not bond; closed");
            conn.raw().disconnect();
        }
        None => {}
    }
    hold_records(stack);
}

/// Whose a bond is, for the log: the identity address's kind and bytes,
/// most significant first as phones show them, and a fingerprint of its
/// IRK, the first byte of its sum, so that two logs tell whether the bond
/// is the same phone's.
struct Whose<'a>(&'a Keys);

impl core::fmt::Display for Whose<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let a = self.0.addr;
        write!(
            f,
            "phone kind={} addr={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} irk=",
            self.0.kind, a[5], a[4], a[3], a[2], a[1], a[0]
        )?;
        match self.0.irk {
            Some(k) => write!(f, "{:02x}", k.iter().fold(0u8, |s, b| s.wrapping_add(*b))),
            None => f.write_str("none"),
        }
    }
}

/// The host's bond of a record's.
fn information(b: &Keys) -> BondInformation {
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

/// The keys to keep of a new pairing's bond: the phone discovers the table
/// of this version as it pairs, and has not subscribed yet.
fn keys_of(b: &BondInformation) -> Keys {
    let mut addr = [0u8; 6];
    addr.copy_from_slice(b.identity.addr.addr.raw());
    Keys {
        kind: b.identity.addr.kind.into_inner(),
        addr,
        irk: b.identity.irk.map(|k| k.to_le_bytes()),
        ltk: b.ltk.to_le_bytes(),
        seen: bark_link::VERSION,
        service_changed: false,
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
    let record = owner(conn);
    // A pairing is refused while no record would take it: the phone's
    // own, a free one, or any on a node nobody is paired with.
    let refuses = !crate::phones::read(|p| p.takes(Some(identity(conn))));
    LINK.lock(|l| {
        let mut l = l.borrow_mut();
        l.phones[slot].record = record;
        l.phones[slot].refuses = refuses;
    });
    if let Err(e) = conn.raw().set_refuses_pairing(refuses) {
        log::error!("ble: refusing pairing {}", HostError(&e));
    }
    let status = status_now(slot).encode();
    let staged = update
        .try_lock()
        .ok()
        .and_then(|u| conn.set(&link.update, &progress(&u.phone)).err());
    for e in [
        staged,
        conn.set(&link.node, &node_now()).err(),
        conn.set(&link.status, &status).err(),
        conn.set(&link.inbox, &inbox_now(slot)).err(),
        conn.set(&link.given_back, &given_back_now(slot)).err(),
        conn.set(&link.clock, &clock_now()).err(),
        conn.set(&link.home, &home_now(record).encode()).err(),
        conn.set(&link.neighbours, &neighbours_now()).err(),
        conn.set(&link.ask, &answer_now()).err(),
        conn.set(&link.public, &bark_link::Public(public()).encode())
            .err(),
        conn.set(&link.phones, &phones_now(slot, record)).err(),
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
    staging: &mut Staging,
) -> Result<bool, AttErrorCode> {
    enum Asked {
        Begin,
        Commit,
        Reboot,
    }
    let Staging {
        phone: update,
        header,
    } = staging;
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
        Asked::Begin => update.begin(header).await.map(|()| false),
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
    // A piece is one PDU, by request or command, never a long write
    // (spec "Phone link", Updates), so its bytes are read where the event
    // holds them, with no copy in the link's task.
    let data = match w.payload().incoming() {
        AttClient::Request(AttReq::Write { data, .. })
        | AttClient::Command(AttCmd::Write { data, .. }) => data,
        _ => return Err(AttErrorCode::INVALID_OFFSET),
    };
    let p = Piece::decode(data).map_err(malformed)?;
    match update.piece(p.offset, p.bytes).await {
        true => Ok(()),
        false => Err(malformed(Malformed::Value)),
    }
}

/// Send, written: the node takes the message, or answers not ready, as
/// it does a phone whose turn it is not, which the write asks for (spec
/// "Phone link", Sending).
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
    // Out of turn, the write asks for the turn, which may pass to it now.
    LINK.lock(|l| l.borrow_mut().turn.ask(from.slot));
    turn_moved();
    if !status_now(from.slot).ready {
        return Err(NOT_READY);
    }
    match ask(from, request).await {
        true => Ok(()),
        false => Err(NOT_READY),
    }
}

/// Inbox, written: the paired phone has its oldest message. A guest
/// confirms nothing, as its reading takes nothing from the ring.
async fn write_confirm(
    from: From,
    w: &WriteEvent<'_, '_, DefaultPacketPool>,
) -> Result<(), AttErrorCode> {
    let c = w.with_data(|offset, data| match offset {
        0 => bark_link::Confirm::decode(data).map_err(malformed),
        _ => Err(AttErrorCode::INVALID_OFFSET),
    })?;
    if LINK.lock(|l| l.borrow().guest(from.slot)) {
        return Err(malformed(Malformed::Value));
    }
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

/// Recipient, written: the phone's recipient tag at the node, and from
/// link version 13 its person's name; a paired phone's record keeps both,
/// so that the node knows its person's messages while they are away and
/// the node's phones see who is paired (spec "Phone link", Pairing,
/// Public). A guest's name is not kept.
///
/// The answer, and whether to close the link: an app whose link version
/// and the node's do not serve each other is answered with the ATT error
/// 0x82 and its link ended (spec "Phone link", Versions); and one guest
/// at a time, so a phone's that is not paired is closed while another
/// guest's link is up, as the other links are the paired phones' to take.
fn write_recipient(
    from: From,
    w: &WriteEvent<'_, '_, DefaultPacketPool>,
) -> (Result<(), AttErrorCode>, bool) {
    let r = w.with_data(|offset, data| match offset {
        0 => bark_link::Recipient::decode(data).map_err(malformed),
        _ => Err(AttErrorCode::INVALID_OFFSET),
    });
    let r = match r {
        Ok(r) => r,
        Err(e) => return (Err(e), false),
    };
    if !r.compatible() {
        log::warn!(
            "ble: the app in slot {} speaks link version {} and serves nodes from version {}; \
this node speaks {} and serves apps from {}: they do not serve each other, closed",
            from.slot,
            r.version,
            r.oldest,
            bark_link::VERSION,
            bark_link::OLDEST
        );
        return (Err(AttErrorCode::new(bark_link::att::INCOMPATIBLE)), true);
    }
    let busy = LINK.lock(|l| {
        let mut l = l.borrow_mut();
        let other = (0..LINKS).any(|i| {
            let p = &l.phones[i];
            i != from.slot && p.live && p.record.is_none() && p.tag.is_some()
        });
        let busy = !from.owner() && other;
        if !busy {
            l.phones[from.slot].tag = Some(r.tag);
            l.turn.app(from.slot, r.version);
        }
        busy
    });
    // An app that cannot ask for the turn may now take it from one unused.
    turn_moved();
    if busy {
        log::info!("ble: a second guest in slot {}, closed", from.slot);
        return (Ok(()), true);
    }
    if let Some(i) = from.record {
        let changed = crate::phones::read(|p| {
            p.get(i)
                .is_some_and(|rec| rec.tag != Some(r.tag) || rec.name != r.name)
        });
        if changed {
            crate::phones::with(|p| p.set_recipient(i, r.tag, r.name));
        }
    }
    log::info!(
        "ble: recipient {:02x} slot {} record {:?} app version {} name {} bytes",
        r.tag,
        from.slot,
        from.record,
        r.version,
        r.name.as_bytes().len()
    );
    (Ok(()), false)
}

/// Guest, written: the number of the last item the guest took, after
/// which its next read of Guest looks.
fn write_guest(from: From, w: &WriteEvent<'_, '_, DefaultPacketPool>) -> Result<(), AttErrorCode> {
    let after = w.with_data(|offset, data| match offset {
        0 => bark_link::After::decode(data).map_err(malformed),
        _ => Err(AttErrorCode::INVALID_OFFSET),
    })?;
    LINK.lock(|l| l.borrow_mut().phones[from.slot].after = after.0);
    Ok(())
}

/// Public, written by a paired phone: 0 makes the node private, its
/// owners' alone, and 1 public again, as it is by default, at once and
/// across reboots, the node's one setting for all its phones; made
/// private, its guests' links close; 2 unpairs the writing phone, for the
/// caller to [`unpair`]. Whether it unpairs (spec "Phone link", Public).
fn write_public(
    from: From,
    w: &WriteEvent<'_, '_, DefaultPacketPool>,
) -> Result<bool, AttErrorCode> {
    let write = w.with_data(|offset, data| match offset {
        0 => bark_link::PublicWrite::decode(data).map_err(malformed),
        _ => Err(AttErrorCode::INVALID_OFFSET),
    })?;
    if !from.owner() {
        // Only a paired phone's link writes it.
        return Err(AttErrorCode::INSUFFICIENT_AUTHENTICATION);
    }
    let set = match write {
        bark_link::PublicWrite::Unpair => return Ok(true),
        bark_link::PublicWrite::Set(set) => set,
    };
    if crate::phones::read(|p| p.public()) != set.0 {
        crate::phones::with(|p| p.set_private(!set.0));
        log::info!("ble: public {}", set.0);
        records_changed();
    }
    Ok(false)
}

/// Node as the phone reads it: what the node is, and how many more
/// phones may pair with it.
fn node_now() -> [u8; Node::LEN] {
    let node = LINK.lock(|l| l.borrow().node);
    let room = crate::phones::read(|p| p.room());
    node.map_or([0; Node::LEN], |n| {
        Node {
            room: Some(room),
            ..n
        }
        .encode()
    })
}

/// Home as the phone of record `record` reads it: whether the node is
/// anyone's home, the numbers its ring holds, and whether it is this
/// phone's person's home.
fn home_now(record: Option<usize>) -> bark_link::home::Home {
    let mine =
        record.is_some_and(|i| crate::phones::read(|p| p.get(i).is_some_and(|r| r.key.is_some())));
    crate::mailbox::with(|m| bark_link::home::Home {
        set: m.is_home(),
        oldest: m.oldest(),
        newest: m.newest(),
        mine: Some(mine),
    })
    .unwrap_or_default()
}

/// Phones as the phone in `slot`, of record `record`, reads it: the
/// records from the place it wrote (spec "Phone link", Phones).
fn phones_now(
    slot: usize,
    record: Option<usize>,
) -> heapless::Vec<u8, { bark_link::phones::MAX_LEN }> {
    let (from, linked) = LINK.lock(|l| {
        let l = l.borrow();
        let linked = (0..LINKS)
            .filter_map(|s| l.phones[s].record.filter(|_| l.phones[s].live))
            .fold(0u8, |m, i| m | 1 << i);
        (l.phones[slot].phones_from, linked)
    });
    let mut b = [0u8; bark_link::phones::MAX_LEN];
    let n = crate::phones::read(|p| p.page(from, record, linked, &mut b));
    heapless::Vec::from_slice(&b[..n]).expect("it fits")
}

/// Home, written by the phone of record `i`: its person's key, or none,
/// at once; another person's home stays (spec "Phone link", Home).
fn write_home(from: From, w: &WriteEvent<'_, '_, DefaultPacketPool>) -> Result<(), AttErrorCode> {
    use bark_link::home::SetHome;
    let set = w.with_data(|offset, data| match offset {
        0 => SetHome::decode(data).map_err(malformed),
        _ => Err(AttErrorCode::INVALID_OFFSET),
    })?;
    let Some(i) = from.record else {
        return Err(AttErrorCode::INSUFFICIENT_AUTHENTICATION);
    };
    let key = match set {
        SetHome::Set(k) => Some(k),
        SetHome::Clear => None,
    };
    let now = crate::phones::with(|p| p.set_home(i, key));
    if now != i {
        log::info!(
            "ble: record {} joins the home record {} left by its person's phone",
            i,
            now
        );
        LINK.lock(|l| l.borrow_mut().phones[from.slot].record = Some(now));
    }
    crate::mailbox::FOR_CORE.signal(());
    log::info!(
        "ble: home of record {} {}",
        now,
        if key.is_some() { "set" } else { "cleared" }
    );
    Ok(())
}

/// The removal code on the screen, if one is (spec "Phone link", Phones).
static REMOVAL: Mutex<CriticalSectionRawMutex, core::cell::Cell<Option<bark_link::phones::Code>>> =
    Mutex::new(core::cell::Cell::new(None));

fn ms_now() -> u64 {
    embassy_time::Instant::now().as_millis()
}

/// Phones, written by a paired phone: the place to read from, a code
/// drawn on the screen, or a removal with that code, taken within 30 s
/// and on one try (spec "Phone link", Phones).
fn write_phones(
    from: From,
    stack: &Stack,
    w: &WriteEvent<'_, '_, DefaultPacketPool>,
) -> Result<(), AttErrorCode> {
    use bark_link::phones::{Code, PhonesWrite};
    let write = w.with_data(|offset, data| match offset {
        0 => PhonesWrite::decode(data).map_err(malformed),
        _ => Err(AttErrorCode::INVALID_OFFSET),
    })?;
    if from.record.is_none() {
        return Err(AttErrorCode::INSUFFICIENT_AUTHENTICATION);
    }
    match write {
        PhonesWrite::From(place) => {
            LINK.lock(|l| l.borrow_mut().phones[from.slot].phones_from = place);
            Ok(())
        }
        PhonesWrite::Code => {
            let code = loop {
                let mut r = [0u8; 4];
                trouble_host_random(&mut r);
                if let Some(c) = Code::draw(u32::from_le_bytes(r), ms_now()) {
                    break c;
                }
            };
            REMOVAL.lock(|c| c.set(Some(code)));
            // The code only in an open build, as a pairing's
            // (spec "Phone link", Pairing).
            match crate::protect::BUILD {
                bark_image::protect::Build::Open => {
                    log::info!("ble: removal, the code shown: {:06}", code.code)
                }
                bark_image::protect::Build::Protected => log::info!("ble: removal, the code shown"),
            }
            crate::display::removal(Some(code.code));
            Ok(())
        }
        PhonesWrite::Remove {
            number,
            parts,
            code,
        } => {
            if crate::phones::read(|p| p.holding(number, parts)).is_none() {
                return Err(malformed(Malformed::Value));
            }
            let taken = REMOVAL.lock(|c| {
                let mut drawn = c.get();
                let taken = Code::take(&mut drawn, code, ms_now());
                c.set(drawn);
                taken
            });
            crate::display::removal(None);
            if !taken {
                log::info!("ble: removal refused: not the code shown, or too late");
                return Err(REFUSED);
            }
            let Some(removed) = crate::phones::with(|p| p.remove(number, parts)) else {
                return Err(malformed(Malformed::Value));
            };
            log::info!(
                "ble: record {} number {} removed: bond {}, home {}",
                removed.index,
                number,
                parts.bond,
                parts.home
            );
            if removed.bond.is_some() {
                hold_records(stack);
                let slots = LINK.lock(|l| {
                    let l = l.borrow();
                    l.slots_of(removed.index).fold(0u16, |m, s| m | 1 << s)
                });
                for s in (0..LINKS).filter(|&s| slots & (1 << s) != 0) {
                    notify_one(
                        s,
                        Notify {
                            close: true,
                            ..Notify::default()
                        },
                    );
                }
            }
            records_changed();
            Ok(())
        }
    }
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

/// Ask, written: the node asks the neighbour for its status, and the
/// answer, if one comes, is notified (spec "Phone link", Ask).
async fn write_ask(
    from: From,
    w: &WriteEvent<'_, '_, DefaultPacketPool>,
) -> Result<(), AttErrorCode> {
    let a = w.with_data(|offset, data| match offset {
        0 => bark_link::Ask::decode(data).map_err(malformed),
        _ => Err(AttErrorCode::INVALID_OFFSET),
    })?;
    ask(from, Request::Ask(a.0)).await;
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
