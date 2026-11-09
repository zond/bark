//! The node's side of an update over the mesh
//! (spec "Updates over the mesh"; `bark-fw/README.md`, "Updates"): it
//! tells the core the image it runs, answers each image a neighbour offers by its policy
//! (`bark_image::policy`), keeps the pieces the core hands it in the store
//! on the external flash (`bark_image::pieces`), fills the pieces the core
//! serves, and stages a gathered image as the phone's update does, to
//! reboot into it at a quiet moment.
//!
//! Its task steps the store one flash operation or one piece read at a
//! time and lets the executor run between, waiting out the chip's erases
//! and programs with a timer. The radio's task hands pieces straight into
//! the store's queue, and asks this task for the bytes of a piece to
//! serve, which it reads from the running image, memory-mapped in the
//! internal flash, or from the blocks of the gathered image on the
//! external one.

use core::cell::{Cell, RefCell};

use bark_driver::Request;
use bark_image::code::{Encoder, PIECE};
use bark_image::layout::{ACTIVE, RECORD, STATE};
use bark_image::pieces::{self, Begin, Poll, Store, Table, Tabling};
use bark_image::policy::{self, Moment};
use bark_image::{sign, Header, HEADER_LEN};
use bark_proto::update::{Image, ImageTag, Piece};
use bark_proto::wire::{Packet, Update, PIECE_BYTES, PIECE_HEADER_LEN};
use bark_proto::Frame;
use embassy_embedded_hal::flash::partition::Partition;
use embassy_futures::select::{select4, Either4};
use embassy_sync::blocking_mutex::raw::{CriticalSectionRawMutex, ThreadModeRawMutex};
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::channel::Channel;
use embassy_sync::signal::Signal;
use embassy_time::{with_timeout, Duration, Timer};
use embedded_storage::nor_flash::ReadNorFlash;

use super::{Power, BARK_IMAGE_VERSION, BARK_SIGNING_KEYS};
use crate::ext_flash::Dfu;
use crate::store::SharedFlash;

const _: () = assert!(PIECE == PIECE_BYTES);

/// Raised once the image has confirmed itself, so that no swap is asked
/// for by the trial, and the external flash is up (`main.rs`).
pub static START: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// The pieces of the image gathered: the radio's task offers, this one
/// steps. At about 4 KB it is built in place, never on the stack.
static STORE: Mutex<ThreadModeRawMutex, RefCell<Store>> = Mutex::new(RefCell::new(Store::EMPTY));

/// The image the node runs, as it serves it, once checked at boot.
static RUNNING: Mutex<ThreadModeRawMutex, RefCell<Option<Running>>> =
    Mutex::new(RefCell::new(None));

struct Running {
    header: [u8; HEADER_LEN],
    length: u32,
    table: Table,
    /// The slot hashed to the header, and the table is whole.
    checked: bool,
}

/// What the core is to be told: read by the driver's application
/// (`node.rs`). A block decoded is never dropped, or the core would wait
/// for it for ever, so the task waits for room.
static FOR_CORE: Channel<CriticalSectionRawMutex, Request, 4> = Channel::new();

/// The newest image a neighbour's section offered.
static OFFERED: Signal<CriticalSectionRawMutex, Image> = Signal::new();

/// A piece went into the store's queue.
static PIECE_IN: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// A piece the core serves: its image, block and index; and, for the
/// piece named, whether its bytes were written into [`OUT`], so that a
/// fill that came too late is never taken for the next piece's.
type Named = (ImageTag, u8, u8);
static SERVE: Signal<CriticalSectionRawMutex, Named> = Signal::new();
static SERVED: Signal<CriticalSectionRawMutex, (Named, bool)> = Signal::new();
static OUT: Mutex<ThreadModeRawMutex, RefCell<[u8; PIECE]>> = Mutex::new(RefCell::new([0; PIECE]));

/// Where the update stands, for the log and the quiet moment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// No image held or gathered.
    Idle,
    /// It holds the running image whole and serves it.
    Holds(u32),
    /// It gathers this version: its blocks, and those decoded.
    Gathering { version: u32, blocks: u8, held: u8 },
    /// It checks the gathered image and records it.
    Installing(u32),
    /// The image is staged and the swap asked for: it reboots at a quiet
    /// moment.
    Staged(u32),
}

static PHASE: Mutex<CriticalSectionRawMutex, Cell<Phase>> = Mutex::new(Cell::new(Phase::Idle));

/// The pieces served, of the last block served.
static SERVING: Mutex<CriticalSectionRawMutex, Cell<(u8, u32)>> = Mutex::new(Cell::new((0, 0)));

fn set_phase(p: Phase) {
    PHASE.lock(|c| c.set(p));
}

/// Where the update stands.
pub fn phase() -> Phase {
    PHASE.lock(Cell::get)
}

impl core::fmt::Display for Phase {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Phase::Idle => f.write_str("idle"),
            Phase::Holds(v) => {
                let (block, pieces) = SERVING.lock(Cell::get);
                write!(f, "holds:{} serving:{}:{}", v, block, pieces)
            }
            Phase::Gathering {
                version,
                blocks,
                held,
            } => write!(f, "gathering:{} blocks:{}/{}", version, held, blocks),
            Phase::Installing(v) => write!(f, "installing:{}", v),
            Phase::Staged(v) => write!(f, "staged:{}", v),
        }
    }
}

// The radio's task's side (`node.rs`).

/// Resolves when a request for the core waits.
pub async fn ready() {
    FOR_CORE.ready_to_receive().await
}

/// The next request for the core.
pub fn request() -> Option<Request> {
    FOR_CORE.try_receive().ok()
}

/// A neighbour's image newer than this node's (`Action::Offered`).
pub fn offered(image: &Image) {
    OFFERED.signal(*image);
}

/// Keep a piece (`Action::StorePiece`). One the store has no room for
/// now is lost, as on the air: the holders send others.
pub fn store_piece(piece: &Piece) {
    STORE.lock(|s| s.borrow_mut().offer(piece.block, piece.index, &piece.bytes));
    PIECE_IN.signal(());
}

/// How long a piece may take to fill: a check of the channel may be held
/// that long for a packet under way (core-interface §3 rule 8), so one
/// sent later goes on a check older than any the core allows for.
const FILL_WITHIN: Duration = Duration::from_micros(bark_driver::MAX_AIRTIME.as_micros());

/// Write the bytes of the piece `frame` names into it (`Action::Serve`);
/// whether they came in time.
pub async fn serve(frame: &mut Frame) -> bool {
    let Ok(Packet::Update(Update::Piece {
        tag, block, index, ..
    })) = Packet::decode(frame.payload())
    else {
        return false;
    };
    let named = (tag, block, index);
    SERVED.reset();
    SERVE.signal(named);
    let filled = async {
        loop {
            let (n, ok) = SERVED.wait().await;
            if n == named {
                return ok;
            }
        }
    };
    let Ok(true) = with_timeout(FILL_WITHIN, filled).await else {
        log::warn!(
            "update: piece block={} index={} not filled; not sent",
            block,
            index
        );
        return false;
    };
    let end = usize::from(frame.len);
    OUT.lock(|o| frame.bytes[PIECE_HEADER_LEN..end].copy_from_slice(&o.borrow()[..]));
    true
}

/// The version staged, once the swap is asked for: the node reboots into
/// it at a quiet moment.
pub fn staged() -> Option<u32> {
    match phase() {
        Phase::Staged(v) => Some(v),
        _ => None,
    }
}

/// Whether the node may reboot into the staged image now
/// ([`policy::quiet`]): `holding`, whether it holds a message a reset
/// would lose.
pub fn quiet(holding: bool) -> bool {
    let power = super::power();
    policy::quiet(Moment {
        links: crate::ble::links(),
        holding,
        phone_update: super::phone::holds_slot(),
        swap_allowed: super::swap_allowed(power),
    })
}

// The phone's side (`phone.rs`).

/// The phone's update begins: the phone wins the staged slot, and the
/// image gathered is given up, unless it is being staged or is: then the
/// slot is the mesh's and the phone is refused. Resolves once the store
/// has let go of the slot.
pub async fn yield_to_phone() -> bool {
    if matches!(phase(), Phase::Installing(_) | Phase::Staged(_)) {
        return false;
    }
    if STORE.lock(|s| s.borrow().image().is_some()) {
        log::info!("update: the phone's update replaces the image gathered");
        STORE.lock(|s| s.borrow_mut().forget());
        PIECE_IN.signal(());
    }
    while STORE.lock(|s| s.borrow().busy()) {
        Timer::after_micros(bark_image::ext_flash::PROGRAM_TYP_US.into()).await;
    }
    true
}

// The task.

/// Run the mesh's update for ever, once [`START`] is raised.
pub async fn run(flash: &'static super::Flash, internal: &'static SharedFlash) -> ! {
    START.wait().await;
    if crate::ext_flash::with(|_| ()).is_none() {
        log::error!("update: no external flash; nothing gathered or served");
        core::future::pending::<()>().await;
    }
    work(flash, internal).await
}

/// Fill the piece the radio's task asks for. Between the store's steps,
/// in the same future as they, so that its state shares room with the
/// staging's: a piece asked for while a whole image is checked and
/// staged, about 3 s, is not filled in time.
async fn answer(flash: &super::Flash, named: Named) {
    let (tag, block, index) = named;
    let ok = fill(flash, tag, block, index).await;
    SERVED.signal((named, ok));
}

/// Piece `index` of block `block` of the image named `tag` into [`OUT`]:
/// coded from the block's data pieces, read one at a time.
async fn fill(flash: &super::Flash, tag: ImageTag, block: u8, index: u8) -> bool {
    let running = RUNNING.lock(|r| {
        r.borrow()
            .as_ref()
            .filter(|r| r.checked)
            .map(|r| r.table.image())
    });
    let gathered = STORE.lock(|s| s.borrow().image());
    let (n, from_store) = match (running, gathered) {
        (Some(i), _) if i.tag == tag.0 => (i.pieces(block), false),
        (_, Some(i)) if i.tag == tag.0 => (i.pieces(block), true),
        _ => return false,
    };
    let Ok(mut encoder) = Encoder::new(n, index) else {
        return false;
    };
    for i in 0..n {
        if !encoder.wants(i) {
            continue;
        }
        let mut piece = [0u8; PIECE];
        let read = match from_store {
            true => crate::ext_flash::with(|chip| {
                STORE.lock(|s| s.borrow().data_piece(chip, block, i, &mut piece))
            })
            .and_then(Result::ok)
            .unwrap_or(false),
            false => running_piece(flash, block, i, &mut piece),
        };
        if !read {
            return false;
        }
        encoder.add(i, &piece);
        // A piece of the external flash is a 2 ms read.
        if from_store {
            embassy_futures::yield_now().await;
        }
    }
    let out = encoder.finish();
    OUT.lock(|o| *o.borrow_mut() = out);
    let (last, count) = SERVING.lock(Cell::get);
    let count = if last == block { count + 1 } else { 1 };
    SERVING.lock(|c| c.set((block, count)));
    log::info!(
        "update: serving block={} piece={} ({} of this block)",
        block,
        index,
        count
    );
    true
}

/// Data piece `i` of block `block` of the running image: the table's, or
/// the packed image's, its header then the active slot, padded with zeros.
fn running_piece(flash: &super::Flash, block: u8, i: usize, out: &mut [u8; PIECE]) -> bool {
    RUNNING.lock(|r| {
        let r = r.borrow();
        let Some(r) = r.as_ref() else {
            return false;
        };
        if block == 0 {
            return r.table.piece(i).map(|p| *out = *p).is_some();
        }
        pieces::held_piece(&r.header, r.length, block, i, out, |at, bytes| {
            flash.lock(|f| f.borrow_mut().read(ACTIVE.start + at, bytes))
        })
        .is_ok()
    })
}

/// The running image, if the record holds its header and the active slot
/// hashes to it: what the node holds whole and serves. The record keeps
/// the header of the image swapped in; a UF2 leaves none, and a swap
/// back leaves the newer one's, which the version tells.
async fn running(flash: &super::Flash) -> Option<Image> {
    // In a scope of its own, so that the header is not kept in the task's
    // future across the hashing's waits: it is kept once, in `RUNNING`.
    let (image, h) = {
        let mut header = [0u8; HEADER_LEN];
        let at = RECORD.start + bark_image::record::HEADER_AT;
        flash.lock(|f| f.borrow_mut().read(at, &mut header)).ok()?;
        let Ok(h) = Header::decode(&header) else {
            log::info!("update: no header for the running image; it serves none");
            return None;
        };
        if h.version != BARK_IMAGE_VERSION || h.length > ACTIVE.len {
            log::info!(
                "update: the record's header is version {}, not the running {}; it serves none",
                h.version,
                BARK_IMAGE_VERSION
            );
            return None;
        }
        // The key the section names: one this firmware checks it with,
        // else the first slot's.
        let key = sign::verify(&header, &BARK_SIGNING_KEYS).unwrap_or(header[sign::SIGNED_LEN]);
        let image = pieces::held(&header, h.length, h.version, key)?;
        RUNNING.lock(|r| {
            *r.borrow_mut() = Some(Running {
                header,
                length: h.length,
                table: Table::new(image, &pieces::name(&header)),
                checked: false,
            })
        });
        (image, h)
    };
    let mut tabling = Tabling::new();
    let mut done = 0;
    // The header, then the slot, a few words at a time from flash.
    let total = HEADER_LEN as u32 + h.length;
    while done < total {
        let read = RUNNING.lock(|r| {
            let mut r = r.borrow_mut();
            let r = r.as_mut()?;
            let mut buf = [0u8; 64];
            let n = (total - done).min(buf.len() as u32) as usize;
            if done < HEADER_LEN as u32 {
                let n = n.min(HEADER_LEN - done as usize);
                buf[..n].copy_from_slice(&r.header[done as usize..done as usize + n]);
                tabling.feed(&mut r.table, &buf[..n]);
                return Some(n);
            }
            let at = ACTIVE.start + done - HEADER_LEN as u32;
            flash
                .lock(|f| f.borrow_mut().read(at, &mut buf[..n]))
                .ok()?;
            tabling.feed(&mut r.table, &buf[..n]);
            Some(n)
        });
        let Some(n) = read else {
            RUNNING.lock(|r| *r.borrow_mut() = None);
            return None;
        };
        done += n as u32;
        // A yield every 4 KiB: hashing them takes about a millisecond.
        if done.is_multiple_of(4096) {
            embassy_futures::yield_now().await;
        }
    }
    let ok = RUNNING.lock(|r| {
        let mut r = r.borrow_mut();
        let r = r.as_mut()?;
        let hash = tabling.finish(&mut r.table);
        r.checked = hash == h.hash;
        Some(r.checked)
    });
    if ok != Some(true) {
        log::warn!(
            "update: the running image does not hash to its record's header; it serves none"
        );
        RUNNING.lock(|r| *r.borrow_mut() = None);
        return None;
    }
    Some(core_image(&image))
}

fn core_image(i: &pieces::Image) -> Image {
    Image {
        name: ImageTag(i.tag),
        version: i.version,
        blocks: i.blocks,
        last: i.last,
        key: i.key,
    }
}

fn store_image(i: &Image) -> pieces::Image {
    pieces::Image {
        tag: i.name.0,
        blocks: i.blocks,
        last: i.last,
        version: i.version,
        key: i.key,
    }
}

/// One step of the store: `None` without the chip.
fn step() -> Option<Result<Poll, bark_image::ext_flash::Error>> {
    crate::ext_flash::with(|chip| STORE.lock(|s| s.borrow_mut().poll(chip)))
}

/// Whether the phone's update or a swap holds the staged slot.
fn slot_taken(flash: &super::Flash) -> bool {
    super::phone::holds_slot() || bark_image::boot::swap_requested(flash) == Ok(true)
}

/// Tell the core, waiting for room.
async fn tell(r: Request) {
    FOR_CORE.send(r).await;
}

async fn work(flash: &'static super::Flash, internal: &'static SharedFlash) -> ! {
    let held = running(flash).await;
    match held {
        Some(image) => {
            log::info!(
                "update: holds version={} blocks={} name={:02x?} key={}",
                image.version,
                image.blocks,
                image.name.0,
                image.key
            );
            set_phase(Phase::Holds(image.version));
            tell(Request::Holds(image)).await;
        }
        None => set_phase(Phase::Idle),
    }
    let idle = || match held {
        Some(i) => Phase::Holds(i.version),
        None => Phase::Idle,
    };
    match crate::ext_flash::with(|chip| STORE.lock(|s| s.borrow_mut().reopen(chip))) {
        Some(Ok(())) => {}
        _ => log::error!("update: the store's head could not be read"),
    }
    // What a reset left: gone on with if it is still wanted, else given
    // up, as after the swap into it.
    let mut gather = STORE.lock(|s| s.borrow().image());
    if let Some(i) = gather {
        if i.version <= BARK_IMAGE_VERSION || slot_taken(flash) {
            log::info!(
                "update: version {} gathered before is not wanted; forgotten",
                i.version
            );
            STORE.lock(|s| s.borrow_mut().forget());
            gather = None;
        } else {
            log::info!(
                "update: going on gathering version={} blocks={}",
                i.version,
                i.blocks
            );
        }
    }
    // The image to tell the core of once the store takes pieces.
    let mut told = false;
    // An image whose header failed its checks: not taken again until a
    // reboot.
    let mut refused: Option<[u8; pieces::TAG_LEN]> = None;
    loop {
        if let Some(i) = gather.filter(|_| !told) {
            if STORE.lock(|s| s.borrow().ready()) {
                let from = STORE.lock(|s| s.borrow().lacks()).unwrap_or(i.blocks);
                tell(Request::Gather {
                    image: core_image(&i),
                    from,
                })
                .await;
                told = true;
                set_phase(gathering(&i));
            }
        }
        if let Some(i) = gather.filter(|_| told && STORE.lock(|s| s.borrow().whole())) {
            match install(internal, &i).await {
                Ok(()) => {
                    set_phase(Phase::Staged(i.version));
                    // The pieces are spent; the staged slot keeps the image.
                    STORE.lock(|s| s.borrow_mut().forget());
                }
                Err(()) => {
                    refused = Some(i.tag);
                    STORE.lock(|s| s.borrow_mut().forget());
                    set_phase(idle());
                }
            }
            gather = None;
            told = false;
        }
        if let Some(named) = SERVE.try_take() {
            answer(flash, named).await;
        }
        let polled = match step() {
            Some(Ok(p)) => p,
            Some(Err(e)) => {
                log::error!("update: the store's flash failed: {:?}", e);
                Timer::after_secs(1).await;
                continue;
            }
            None => Poll::Idle,
        };
        match polled {
            Poll::Again => embassy_futures::yield_now().await,
            // The shortest operation's typical time, then again: an erase
            // is looked at about eight times.
            Poll::Busy => Timer::after_micros(bark_image::ext_flash::PROGRAM_TYP_US.into()).await,
            Poll::Decoded(b) => {
                let p = gather.map(|i| gathering(&i));
                if let Some(p) = p {
                    set_phase(p);
                }
                let held = match p {
                    Some(Phase::Gathering { held, .. }) => held,
                    _ => 0,
                };
                log::info!(
                    "update: block {} decoded and written, {} of {}",
                    b,
                    held,
                    gather.map_or(0, |i| i.blocks)
                );
                tell(Request::Block { index: b, ok: true }).await;
            }
            Poll::Failed(b) => {
                log::warn!("update: block {} failed its check; gathered again", b);
                tell(Request::Block {
                    index: b,
                    ok: false,
                })
                .await;
            }
            Poll::Idle => {
                // Staged, it only serves until the reboot.
                while staged().is_some() {
                    answer(flash, SERVE.wait().await).await;
                }
                let wait = select4(
                    OFFERED.wait(),
                    PIECE_IN.wait(),
                    until_ready(gather, told),
                    SERVE.wait(),
                )
                .await;
                match wait {
                    Either4::First(image) => {
                        if let Some(i) = offer(flash, &image, refused) {
                            gather = Some(i);
                            told = false;
                        }
                    }
                    Either4::Fourth(named) => answer(flash, named).await,
                    _ => {}
                }
            }
        }
    }
}

/// While a new image's regions are cleared the store is busy, never
/// idle; this only covers the moment between.
async fn until_ready(gather: Option<pieces::Image>, told: bool) {
    match gather.is_some() && !told {
        true => Timer::after_millis(10).await,
        false => core::future::pending().await,
    }
}

/// The phase of gathering `i`: the blocks decoded so far.
fn gathering(i: &pieces::Image) -> Phase {
    let held = STORE.lock(|s| s.borrow().decoded());
    Phase::Gathering {
        version: i.version,
        blocks: i.blocks,
        held,
    }
}

/// A neighbour offers `image`: the image to gather, if the policy takes
/// it and the store can.
fn offer(
    flash: &super::Flash,
    image: &Image,
    refused: Option<[u8; pieces::TAG_LEN]>,
) -> Option<pieces::Image> {
    let o = policy::Offer {
        version: image.version,
        key: image.key,
        running: BARK_IMAGE_VERSION,
        slot_taken: slot_taken(flash),
    };
    let takes = policy::takes(o, &BARK_SIGNING_KEYS) && refused != Some(image.name.0);
    log::info!(
        "update: offered version={} blocks={} key={} name={:02x?}: {}",
        image.version,
        image.blocks,
        image.key,
        image.name.0,
        if takes { "taken" } else { "not taken" }
    );
    if !takes {
        return None;
    }
    let i = store_image(image);
    match STORE.lock(|s| s.borrow_mut().begin(i)) {
        Begin::Unfit => {
            log::warn!("update: version {} does not fit the slot", image.version);
            None
        }
        b => {
            log::info!(
                "update: gathering version={} blocks={} ({:?})",
                image.version,
                image.blocks,
                b
            );
            Some(i)
        }
    }
}

/// The gathered image is whole: check its header and its bytes, record
/// it and ask for the swap, as the phone's update does (`super::commit`).
async fn install(internal: &'static SharedFlash, i: &pieces::Image) -> Result<(), ()> {
    set_phase(Phase::Installing(i.version));
    let header = crate::ext_flash::with(|chip| STORE.lock(|s| s.borrow().header(chip)));
    let Some(Ok(Some(header))) = header else {
        log::error!("update: version {} whole, its header unread", i.version);
        return Err(());
    };
    // The record is erased as the staging begins, so the power is checked
    // first, as the phone's commit would.
    let mut told = false;
    while !super::swap_allowed(super::power()) {
        if !core::mem::replace(&mut told, true) {
            log::info!(
                "update: version {} whole; waiting for the power a swap needs",
                i.version
            );
        }
        Timer::after_secs(60).await;
    }
    let part = |r: bark_image::layout::Region| Partition::new(internal, r.start, r.len);
    let written = bark_image::boot::Stager::written(
        part(RECORD),
        part(STATE),
        Dfu,
        &header,
        BARK_IMAGE_VERSION,
        &BARK_SIGNING_KEYS,
    )
    .await;
    let mut staging = match written {
        Ok(s) => Some(s),
        Err(e) => {
            log::error!("update: version {} refused={:?}", i.version, e);
            return Err(());
        }
    };
    let power: Power = super::power();
    match super::commit(&mut staging, power).await {
        Ok(h) => {
            log::info!(
                "update: version={} length={} whole and checked: the loader swaps it in at the next boot",
                h.version,
                h.length
            );
            Ok(())
        }
        Err(e) => {
            log::error!("update: version {} refused={:?}", i.version, e);
            Err(())
        }
    }
}
