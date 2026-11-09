//! A bench loader's reset in the middle of a swap (`BARK_LOADER_BENCH`,
//! roadmap D143): once in each swap and once in each swap back it stops
//! the copy at a moment drawn evenly over the time the two flashes spend
//! erasing and writing, as a power cut would stop it, and the loader
//! resets the chip. The next boot goes on with the copy from where it
//! stopped, as after a power cut.
//!
//! A reset alone cannot land inside an operation of the internal flash:
//! the CPU halts while the NVMC erases or writes when it runs from flash
//! (nRF52840 Product Specification, NVMC). So the operation the moment
//! falls in is done only in part, as far as it had got: an erase as a
//! partial erase that lasts as long, which leaves the page's bits
//! undefined, as an incomplete erase does (same section); a write as the
//! words written by then. The external chip runs an operation on its own:
//! it is started and left running, and the reset drops the chip's rail
//! (`docs/external-flash-2026-11-06.md`, "The supply"). Reads count for no
//! time: a cut during one leaves the flashes as between two operations.
//!
//! The moment is drawn at the boot that starts a swap or a swap back, and
//! written to the record before the first copy; a boot that finds it
//! there draws none. So a swap is stopped at most once, and ends at the
//! next boot.
//!
//! Or the moment is chosen ([`choose`]): the running image writes it to
//! the record, on a bench script's behalf, before the boot that starts
//! the phase. It names an operation by its index in the phase's erases
//! and writes on both flashes, counted from the start of that boot, each
//! page of an erase one, as the host test's power cuts count them
//! (`boot/tests.rs`, `cut_everywhere`); [`operations`] says how many a
//! phase has. That operation is done half way, as the host test's cut
//! leaves it. Where it fell is written before the reset, and a boot that
//! finds it there stops nothing, so the phase then ends.

use core::cell::Cell;

use embedded_storage::nor_flash::{
    ErrorType, NorFlash, NorFlashError, NorFlashErrorKind, ReadNorFlash,
};

use super::{part, prepare_with, state, Boot, FlashError, Shared};
use crate::ext_flash::{Bus, Chip, ERASE_TYP_US, PROGRAM_PAGE, PROGRAM_TYP_US};
use crate::layout::{ACTIVE, PAGE, RECORD, STATE};
use crate::record::{CUTS_AT, CUTS_LEN};
use embassy_boot::State;

/// An erase of a page of the internal flash, tERASEPAGE (nRF52840 Product
/// Specification, NVMC); a partial erase is timed against it.
pub const INTERNAL_ERASE_US: u32 = 85_000;

/// A word written to the internal flash, tWRITE (same section).
pub const INTERNAL_WORD_US: u32 = 41;

/// The internal flash's word.
const WORD: u32 = 4;

/// The pages each slot holds; the swap copies every one, whatever the
/// image's length.
const PAGES: u32 = ACTIVE.len / PAGE;

/// A word of the record before anything is written to it.
const ERASED: u32 = u32::MAX;

/// Which half of an update the copy is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// The staged image in, the running one out to the external flash.
    Swap,
    /// Back again, as the new image did not confirm itself.
    Back,
}

impl Phase {
    fn index(self) -> u32 {
        match self {
            Phase::Swap => 0,
            Phase::Back => 1,
        }
    }
}

/// Which flash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// The nRF52840's.
    Internal,
    /// The external chip.
    External,
}

/// What a flash was doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// Erasing a page or sector.
    Erase,
    /// Writing.
    Write,
}

/// The time `op` takes on `side` over `len` bytes from `from`: an erase
/// a page or sector at a time, an internal write a word at a time, an
/// external one a page program at a time (ZD25WQ16B datasheet, Table 4-3,
/// typical: the chip's own time, which the wait for the cut must stay
/// inside).
pub const fn cost(side: Side, op: Op, from: u32, len: u32) -> u32 {
    match (side, op) {
        (Side::Internal, Op::Erase) => len / PAGE * INTERNAL_ERASE_US,
        (Side::Internal, Op::Write) => len / WORD * INTERNAL_WORD_US,
        (Side::External, Op::Erase) => len / PAGE * ERASE_TYP_US,
        (Side::External, Op::Write) => programs(from, len) * PROGRAM_TYP_US,
    }
}

/// The page programs a write of `len` bytes from `from` takes.
const fn programs(from: u32, len: u32) -> u32 {
    match len {
        0 => 0,
        _ => (from + len - 1) / PROGRAM_PAGE - from / PROGRAM_PAGE + 1,
    }
}

/// The erases, a page each, and the writes a phase makes at the boot
/// that starts it, in the order of embassy-boot's `prepare_boot` (as
/// [`phase_us`]): the swap's first is the record's word that notes the
/// image checked, then six a page, each copy an erase, a write and a
/// progress word; a swap back's are six a page, then the word that marks
/// its progress invalid, the state page's erase and its word.
pub const fn operations(phase: Phase) -> u32 {
    match phase {
        Phase::Swap => 1 + 6 * PAGES,
        Phase::Back => 6 * PAGES + 3,
    }
}

/// The time a whole phase spends erasing and writing, in the order of
/// embassy-boot's `prepare_boot` (its documentation, "SWAPPING" and
/// "REVERTING", read at 0.7.0): each page copied out to the external
/// flash and the staged one in, each copy a page erase, a page write and
/// a progress word; a swap back then marks its progress invalid, erases
/// the state page and writes its word.
pub const fn phase_us(phase: Phase) -> u32 {
    let word = cost(Side::Internal, Op::Write, 0, WORD);
    let out = cost(Side::External, Op::Erase, 0, PAGE) + cost(Side::External, Op::Write, 0, PAGE);
    let into = cost(Side::Internal, Op::Erase, 0, PAGE) + cost(Side::Internal, Op::Write, 0, PAGE);
    let copy = PAGES * (out + into + 2 * word);
    match phase {
        Phase::Swap => copy,
        Phase::Back => copy + 2 * word + cost(Side::Internal, Op::Erase, 0, STATE.len),
    }
}

/// The moment, in microseconds of a phase's `total`, a draw of 32 random
/// bits picks: each of the `total` by as many draws as any other, to
/// within one.
pub fn moment(draw: u32, total: u32) -> u32 {
    ((u64::from(draw) * u64::from(total)) >> 32) as u32
}

/// A record word with this bit holds a chosen moment, an operation's
/// index, in its other bits; without it, a drawn moment in microseconds,
/// which never reaches it.
pub const CHOSEN: u32 = 1 << 31;

const _: () = assert!(phase_us(Phase::Swap) < CHOSEN && phase_us(Phase::Back) < CHOSEN);

/// When in a phase the copy is stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Moment {
    /// Drawn: microseconds of erasing and writing.
    Us(u32),
    /// Chosen: the index of an operation (`operations`).
    Op(u32),
}

impl Moment {
    fn encode(self) -> u32 {
        match self {
            Moment::Us(us) => us,
            Moment::Op(n) => CHOSEN | n,
        }
    }

    fn decode(w: u32) -> Moment {
        match w & CHOSEN {
            0 => Moment::Us(w),
            _ => Moment::Op(w & !CHOSEN),
        }
    }
}

/// Where a copy was stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fell {
    /// On which flash.
    pub side: Side,
    /// In an erase or a write.
    pub op: Op,
    /// The page, or sector, by its number on that flash.
    pub page: u32,
    /// How far into the operation, in thousandths of its time.
    pub permille: u32,
}

impl Fell {
    /// As the record holds it: the page in the low 16 bits, the
    /// thousandths in the next 10, bit 30 for an erase and bit 31 for the
    /// external flash. Never all ones, an erased word.
    fn encode(self) -> u32 {
        (self.page & 0xffff)
            | self.permille << 16
            | u32::from(self.op == Op::Erase) << 30
            | u32::from(self.side == Side::External) << 31
    }

    fn decode(w: u32) -> Option<Fell> {
        (w != ERASED).then_some(Fell {
            side: match w >> 31 {
                1 => Side::External,
                _ => Side::Internal,
            },
            op: match (w >> 30) & 1 {
                1 => Op::Erase,
                _ => Op::Write,
            },
            page: w & 0xffff,
            permille: (w >> 16) & 0x3ff,
        })
    }
}

/// One boot's cut: what the loader drew, and, once the copy has come to
/// it, where it fell.
pub struct Plan {
    draw: u32,
    /// The phase and the moment, while the copy has yet to reach it.
    armed: Cell<Option<(Phase, Moment)>>,
    /// Erasing and writing so far in this boot: microseconds for a drawn
    /// moment, operations for a chosen one.
    elapsed: Cell<u32>,
    fell: Cell<Option<(Phase, Fell)>>,
}

impl Plan {
    /// A plan from 32 random bits, fresh at every boot.
    pub const fn new(draw: u32) -> Plan {
        Plan {
            draw,
            armed: Cell::new(None),
            elapsed: Cell::new(0),
            fell: Cell::new(None),
        }
    }

    /// Where the copy was stopped in this boot, if it was.
    pub fn fell(&self) -> Option<(Phase, Fell)> {
        self.fell.get()
    }

    /// Account for `op` over `len` bytes from `from`; the time into it
    /// the cut falls at, if it falls in it.
    fn step(&self, side: Side, op: Op, from: u32, len: u32) -> Option<u32> {
        let (phase, at) = self.armed.get()?;
        let spent = self.elapsed.get();
        let takes = cost(side, op, from, len);
        let into = match at {
            Moment::Us(at) if at >= spent + takes => {
                self.elapsed.set(spent + takes);
                return None;
            }
            Moment::Us(at) => at - spent,
            Moment::Op(n) if n > spent => {
                self.elapsed.set(spent + 1);
                return None;
            }
            Moment::Op(_) => takes / 2,
        };
        self.armed.set(None);
        let permille = (u64::from(into) * 1000 / u64::from(takes)) as u32;
        let page = from / PAGE;
        self.fell.set(Some((
            phase,
            Fell {
                side,
                op,
                page,
                permille,
            },
        )));
        Some(into)
    }
}

/// How a flash does part of an operation, as far as a power cut `us`
/// into it would leave it.
pub trait Partial<F: NorFlash> {
    /// Erase the page or sector at `from`, stopped `us` into it.
    fn erase(&mut self, flash: &mut F, from: u32, us: u32) -> Result<(), F::Error>;
    /// Write `bytes` at `offset`, stopped `us` into it.
    fn write(&mut self, flash: &mut F, offset: u32, bytes: &[u8], us: u32) -> Result<(), F::Error>;
}

/// The bytes of a write of `len` from `offset` that are whole `us` into
/// it, on `side`, and the time into the next piece.
pub fn written_by(side: Side, offset: u32, len: u32, us: u32) -> (u32, u32) {
    match side {
        Side::Internal => ((us / INTERNAL_WORD_US * WORD).min(len), 0),
        Side::External => {
            let whole = us / PROGRAM_TYP_US;
            let end = match whole {
                0 => offset,
                n => (offset / PROGRAM_PAGE + n) * PROGRAM_PAGE,
            };
            ((end - offset).min(len), us % PROGRAM_TYP_US)
        }
    }
}

/// The external chip's part of an operation: the program or erase in
/// progress started and left running for its share of the time.
pub struct ExternalPartial;

impl<B: Bus> Partial<Chip<B>> for ExternalPartial {
    fn erase(
        &mut self,
        chip: &mut Chip<B>,
        from: u32,
        us: u32,
    ) -> Result<(), crate::ext_flash::Error> {
        chip.start_erase(from)?;
        chip.pause_us(us);
        Ok(())
    }

    fn write(
        &mut self,
        chip: &mut Chip<B>,
        offset: u32,
        bytes: &[u8],
        us: u32,
    ) -> Result<(), crate::ext_flash::Error> {
        let (done, rest) = written_by(Side::External, offset, bytes.len() as u32, us);
        chip.write(offset, &bytes[..done as usize])?;
        let at = offset + done;
        if (done as usize) < bytes.len() {
            let piece = (PROGRAM_PAGE - at % PROGRAM_PAGE).min(bytes.len() as u32 - done);
            chip.start_program(at, &bytes[done as usize..(done + piece) as usize])?;
            chip.pause_us(rest);
        }
        Ok(())
    }
}

/// A flash whose erases and writes count towards the plan's moment and
/// whose operation at that moment is done only in part.
pub struct Cutting<'p, F, P> {
    flash: F,
    partial: P,
    side: Side,
    plan: &'p Plan,
}

impl<'p, F, P> Cutting<'p, F, P> {
    /// `flash`, which is the `side` flash, under `plan`.
    pub fn new(flash: F, partial: P, side: Side, plan: &'p Plan) -> Self {
        Cutting {
            flash,
            partial,
            side,
            plan,
        }
    }

    /// The flash, back.
    pub fn into_inner(self) -> F {
        self.flash
    }
}

/// A flash's error, or the cut.
#[derive(Debug)]
pub enum CutError<E> {
    /// The flash failed.
    Flash(E),
    /// The copy stopped here, as a power cut would stop it.
    Cut,
}

impl<E: NorFlashError> NorFlashError for CutError<E> {
    fn kind(&self) -> NorFlashErrorKind {
        match self {
            CutError::Flash(e) => e.kind(),
            CutError::Cut => NorFlashErrorKind::Other,
        }
    }
}

impl<F: ErrorType, P> ErrorType for Cutting<'_, F, P> {
    type Error = CutError<F::Error>;
}

impl<F: ReadNorFlash, P> ReadNorFlash for Cutting<'_, F, P> {
    const READ_SIZE: usize = F::READ_SIZE;

    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Self::Error> {
        self.flash.read(offset, bytes).map_err(CutError::Flash)
    }

    fn capacity(&self) -> usize {
        self.flash.capacity()
    }
}

impl<F: NorFlash, P: Partial<F>> NorFlash for Cutting<'_, F, P> {
    const WRITE_SIZE: usize = F::WRITE_SIZE;
    const ERASE_SIZE: usize = F::ERASE_SIZE;

    fn erase(&mut self, from: u32, to: u32) -> Result<(), Self::Error> {
        // A page at a time, as the flashes time it.
        for page in (from..to).step_by(PAGE as usize) {
            let end = (page + PAGE).min(to);
            match self.plan.step(self.side, Op::Erase, page, end - page) {
                None => self.flash.erase(page, end).map_err(CutError::Flash)?,
                Some(us) => {
                    self.partial
                        .erase(&mut self.flash, page, us)
                        .map_err(CutError::Flash)?;
                    return Err(CutError::Cut);
                }
            }
        }
        Ok(())
    }

    fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Self::Error> {
        match self
            .plan
            .step(self.side, Op::Write, offset, bytes.len() as u32)
        {
            None => self.flash.write(offset, bytes).map_err(CutError::Flash),
            Some(us) => {
                self.partial
                    .write(&mut self.flash, offset, bytes, us)
                    .map_err(CutError::Flash)?;
                Err(CutError::Cut)
            }
        }
    }
}

/// The phase a boot with a swap asked for is in, read as embassy-boot
/// reads its state page: the progress words follow the state word and a
/// word that marks them invalid, one for each copy of the swap, then one
/// for each of the swap back; the swap is done once the last of its own
/// is written, or the progress is marked invalid. `None` without a swap.
pub fn phase<F: NorFlash>(internal: &Shared<F>) -> Result<Option<Phase>, FlashError> {
    if state(internal)? != State::Swap {
        return Ok(None);
    }
    if state_word(internal, 1)? != ERASED {
        return Ok(Some(Phase::Back));
    }
    Ok(Some(match progress_done(internal, 2 * PAGES - 1)? {
        false => Phase::Swap,
        true => Phase::Back,
    }))
}

/// The state page's `n`th word. Its words are the internal flash's.
fn state_word<F: NorFlash>(internal: &Shared<F>, n: u32) -> Result<u32, FlashError> {
    const { assert!(F::WRITE_SIZE == WORD as usize) };
    let mut word = [0u8; WORD as usize];
    part(internal, STATE)
        .read(n * WORD, &mut word)
        .map_err(super::flash_error)?;
    Ok(u32::from_le_bytes(word))
}

/// Whether progress word `index` is written, as embassy-boot reads it:
/// no byte of it erased.
fn progress_done<F: NorFlash>(internal: &Shared<F>, index: u32) -> Result<bool, FlashError> {
    Ok(!state_word(internal, 2 + index)?
        .to_le_bytes()
        .contains(&0xff))
}

/// Whether nothing of `phase` has been done yet, so that this boot's
/// count of operations is the phase's from its start: for the swap, the
/// record's word not yet noting the image checked and the first copy
/// not done; for a swap back, its first copy not done and the progress
/// not marked invalid.
fn at_start<F: NorFlash>(internal: &Shared<F>, phase: Phase) -> Result<bool, FlashError> {
    Ok(match phase {
        Phase::Swap => !super::was_checked(internal)? && !progress_done(internal, 0)?,
        Phase::Back => state_word(internal, 1)? == ERASED && !progress_done(internal, 2 * PAGES)?,
    })
}

fn read_word<F: NorFlash>(internal: &Shared<F>, at: u32) -> Result<u32, FlashError> {
    let mut b = [0u8; 4];
    part(internal, RECORD)
        .read(at, &mut b)
        .map_err(super::flash_error)?;
    Ok(u32::from_le_bytes(b))
}

fn write_word<F: NorFlash>(internal: &Shared<F>, at: u32, w: u32) -> Result<(), FlashError> {
    part(internal, RECORD)
        .write(at, &w.to_le_bytes())
        .map_err(super::flash_error)
}

/// Where the record keeps `phase`'s moment; its fall is the word after.
fn at_of(phase: Phase) -> u32 {
    CUTS_AT + 8 * phase.index()
}

// Both phases' words lie within the record's room for them.
const _: () = assert!(8 * 2 == CUTS_LEN);

/// Draw the phase's moment, if no earlier boot did and none was chosen,
/// note it, and arm the plan.
fn arm<F: NorFlash>(internal: &Shared<F>, plan: &Plan) -> Result<(), FlashError> {
    let Some(phase) = phase(internal)? else {
        return Ok(());
    };
    if read_word(internal, at_of(phase))? != ERASED {
        return Ok(());
    }
    let at = Moment::Us(moment(plan.draw, phase_us(phase)));
    write_word(internal, at_of(phase), at.encode())?;
    plan.elapsed.set(0);
    plan.armed.set(Some((phase, at)));
    Ok(())
}

/// Arm the plan with the phase's chosen moment, if one is chosen, it has
/// not fallen yet, and this boot starts the phase.
fn arm_chosen<F: NorFlash>(internal: &Shared<F>, plan: &Plan) -> Result<(), FlashError> {
    let Some(phase) = phase(internal)? else {
        return Ok(());
    };
    let word = read_word(internal, at_of(phase))?;
    if word == ERASED || word & CHOSEN == 0 || read_word(internal, at_of(phase) + 4)? != ERASED {
        return Ok(());
    }
    if at_start(internal, phase)? {
        plan.elapsed.set(0);
        plan.armed.set(Some((phase, Moment::decode(word))));
    }
    Ok(())
}

/// Why a moment was not chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChooseError {
    /// No swap is asked for, or it has begun.
    NoSwap,
    /// The phase's moment is already in the record.
    Chosen,
    /// Past the phase's last operation, `operations`.
    Beyond(u32),
    /// The flash failed.
    Flash(FlashError),
}

impl From<FlashError> for ChooseError {
    fn from(e: FlashError) -> Self {
        ChooseError::Flash(e)
    }
}

/// Choose where a bench loader stops `phase` of the swap asked for: at
/// operation `op` (`operations`), or nowhere with `None`, which a drawn
/// moment would otherwise take. Written by the running image once it has
/// asked for the swap, as staging erases the record.
pub fn choose<F: NorFlash>(
    internal: &Shared<F>,
    phase: Phase,
    op: Option<u32>,
) -> Result<(), ChooseError> {
    if self::phase(internal)? != Some(Phase::Swap) || !at_start(internal, Phase::Swap)? {
        return Err(ChooseError::NoSwap);
    }
    let last = operations(phase);
    let op = match op {
        // The index after the last, which the copy never reaches.
        None => last,
        Some(n) if n < last => n,
        Some(_) => return Err(ChooseError::Beyond(last)),
    };
    if read_word(internal, at_of(phase))? != ERASED {
        return Err(ChooseError::Chosen);
    }
    write_word(internal, at_of(phase), Moment::Op(op).encode())?;
    Ok(())
}

/// [`super::prepare`] on flashes under `plan`. A boot that starts a swap
/// or a swap back stops it at the plan's moment, notes where in the
/// record, and returns an error, on which the loader resets as it does
/// on any flash error.
pub fn prepare_cut<I, PI, E, PE>(
    internal: &Shared<Cutting<'_, I, PI>>,
    external: Option<&Shared<Cutting<'_, E, PE>>>,
    page: &mut [u8; PAGE as usize],
    plan: &Plan,
) -> Result<Boot, FlashError>
where
    I: NorFlash,
    PI: Partial<I>,
    E: NorFlash,
    PE: Partial<E>,
{
    // A chosen moment counts from the boot's first operation, the
    // record's word among them, as the host test counts.
    arm_chosen(internal, plan)?;
    let boot = prepare_with(internal, external, page, |i| arm(i, plan));
    plan.armed.set(None);
    match plan.fell() {
        Some((phase, fell)) => {
            write_word(internal, at_of(phase) + 4, fell.encode())?;
            Err(FlashError(NorFlashErrorKind::Other))
        }
        None => boot,
    }
}

/// What the record says of a phase's cut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Recorded {
    /// Which phase.
    pub phase: Phase,
    /// The moment, drawn or chosen.
    pub at: Moment,
    /// Where the copy was stopped; `None` if it never came to the moment,
    /// as when the phase began at an earlier boot.
    pub fell: Option<Fell>,
}

/// The cuts the record notes, for the swap and the swap back; both
/// `None` from a loader built without the bench.
pub fn recorded<F: NorFlash>(internal: &Shared<F>) -> Result<[Option<Recorded>; 2], FlashError> {
    let mut out = [None; 2];
    for (slot, phase) in out.iter_mut().zip([Phase::Swap, Phase::Back]) {
        let at = read_word(internal, at_of(phase))?;
        if at != ERASED {
            let fell = Fell::decode(read_word(internal, at_of(phase) + 4)?);
            *slot = Some(Recorded {
                phase,
                at: Moment::decode(at),
                fell,
            });
        }
    }
    Ok(out)
}

impl core::fmt::Display for Recorded {
    /// As the firmware logs it, and `bark-fw/scripts/swapbench.py` and
    /// `cutbench.py` read it: `bench reset in the swap at 12345678 us:
    /// external erase of page 113, 37.0 % through`, `bench reset in the
    /// swap back at operation 4: ...`, or `...: not reached`.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let phase = match self.phase {
            Phase::Swap => "swap",
            Phase::Back => "swap back",
        };
        match self.at {
            Moment::Us(us) => write!(f, "bench reset in the {phase} at {us} us: ")?,
            Moment::Op(n) => write!(f, "bench reset in the {phase} at operation {n}: ")?,
        }
        let Some(fell) = self.fell else {
            return f.write_str("not reached");
        };
        let side = match fell.side {
            Side::Internal => "internal",
            Side::External => "external",
        };
        let op = match fell.op {
            Op::Erase => "erase",
            Op::Write => "write",
        };
        write!(
            f,
            "{side} {op} of page {}, {}.{} % through",
            fell.page,
            fell.permille / 10,
            fell.permille % 10
        )
    }
}
