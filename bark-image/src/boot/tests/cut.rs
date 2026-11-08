//! The bench loader's cut (`boot::cut`) on the flashes in RAM: where the
//! moment falls, that each phase is stopped once and ends at the next
//! boot with the image expected whole, and the external chip left
//! running by its part of an operation; that a chosen moment names the
//! operation the host test's power cut of the same index takes.

use super::*;
use crate::boot::cut::{
    choose, cost, moment, operations, phase, phase_us, prepare_cut, recorded, written_by,
    ChooseError, Cutting, ExternalPartial, Fell, Moment, Op, Partial, Phase, Plan, Recorded, Side,
};
use std::string::ToString;

/// What a cut leaves on a flash in RAM: the start of an erase's page
/// erased in proportion to its time, or the words of a write that were
/// whole.
struct RamPartial(Side);

impl<const W: usize> Partial<RamFlash<W>> for RamPartial {
    fn erase(&mut self, flash: &mut RamFlash<W>, from: u32, us: u32) -> Result<(), Cut> {
        let n = u64::from(PAGE) * u64::from(us) / u64::from(cost(self.0, Op::Erase, from, PAGE));
        let at = from as usize;
        flash.bytes[at..at + n as usize].fill(0xff);
        Ok(())
    }

    fn write(
        &mut self,
        flash: &mut RamFlash<W>,
        offset: u32,
        bytes: &[u8],
        us: u32,
    ) -> Result<(), Cut> {
        let (done, _) = written_by(self.0, offset, bytes.len() as u32, us);
        flash.write(offset, &bytes[..done as usize])
    }
}

/// One boot of a bench loader whose draw is `draw`: what it returns and
/// where it stopped the copy.
fn boot_cut(board: &Board, draw: u32) -> (Result<Boot, FlashError>, Option<(Phase, Fell)>) {
    let plan = Plan::new(draw);
    let internal = board
        .internal
        .lock(|f| f.replace(RamFlash::new(0, Side::Internal, &board.power)));
    let external = board
        .external
        .lock(|f| f.replace(RamFlash::new(0, Side::External, &board.power)));
    let internal = Mutex::new(RefCell::new(Cutting::new(
        internal,
        RamPartial(Side::Internal),
        Side::Internal,
        &plan,
    )));
    let external = Mutex::new(RefCell::new(Cutting::new(
        external,
        RamPartial(Side::External),
        Side::External,
        &plan,
    )));
    let mut page = [0u8; PAGE as usize];
    let got = prepare_cut(&internal, Some(&external), &mut page, &plan);
    board.internal.lock(|f| *f.borrow_mut() = back(internal));
    board.external.lock(|f| *f.borrow_mut() = back(external));
    (got, plan.fell())
}

fn back<F, P>(s: Shared<Cutting<'_, F, P>>) -> F {
    s.into_inner().into_inner().into_inner()
}

/// Draws spread over the whole range, the ends included.
fn draws(n: u32) -> impl Iterator<Item = u32> {
    (0..n)
        .map(move |i| (u64::from(u32::MAX) * u64::from(i) / u64::from(n - 1)) as u32)
        .map(|d| d ^ (d >> 7 & 0xffff))
}

#[test]
fn the_moment_spreads_a_draw_evenly() {
    let total = phase_us(Phase::Swap);
    assert_eq!(moment(0, total), 0);
    assert_eq!(moment(u32::MAX, total), total - 1);
    // Each of ten moments takes a tenth of the draws, to within one.
    let mut counts = [0u64; 10];
    for d in (0..=u32::MAX).step_by(4099) {
        counts[moment(d, 10) as usize] += 1;
    }
    let (lo, hi) = (counts.iter().min().unwrap(), counts.iter().max().unwrap());
    assert!(hi - lo <= 1, "{counts:?}");
}

#[test]
fn a_write_is_whole_by_the_word_or_by_the_page_program() {
    assert_eq!(written_by(Side::Internal, 0, PAGE, 0), (0, 0));
    assert_eq!(written_by(Side::Internal, 0, PAGE, 3 * 41 + 40), (12, 0));
    assert_eq!(written_by(Side::Internal, 0, 4, 1_000_000), (4, 0));
    assert_eq!(written_by(Side::External, 0, PAGE, 1_299), (0, 1_299));
    assert_eq!(written_by(Side::External, 0, PAGE, 2 * 1_300 + 5), (512, 5));
    assert_eq!(written_by(Side::External, 4096, 300, 1_300), (256, 0));
}

#[test]
fn the_phase_is_read_from_the_state_page() {
    let (a, b) = (a(), b());
    let board = board(&a);
    assert_eq!(phase(&board.internal), Ok(None));
    stage(&board, &b).unwrap();
    assert_eq!(phase(&board.internal), Ok(Some(Phase::Swap)));
    assert_eq!(boot(&board), Ok(Boot::Trial));
    assert_eq!(phase(&board.internal), Ok(Some(Phase::Back)));
    assert_eq!(boot(&board), Ok(Boot::Reverted));
    assert_eq!(phase(&board.internal), Ok(None));
}

/// The last moment falls in each phase's last operation, the word that
/// ends it, 40 µs into its 41: the phase's time is the sum of what its
/// operations take.
#[test]
fn the_last_moment_falls_in_the_last_word() {
    let (a, b) = (a(), b());
    let board = board(&a);
    stage(&board, &b).unwrap();
    // The swap's last write is its last progress word.
    let (got, fell) = boot_cut(&board, u32::MAX);
    assert!(got.is_err());
    let word = Fell {
        side: Side::Internal,
        op: Op::Write,
        page: STATE.start / PAGE,
        permille: 40 * 1000 / 41,
    };
    assert_eq!(fell, Some((Phase::Swap, word)));
    assert_eq!(boot_cut(&board, u32::MAX), (Ok(Boot::Trial), None));
    // The swap back's is the state word written after the state page's
    // erase.
    let (got, fell) = boot_cut(&board, u32::MAX);
    assert!(got.is_err());
    assert_eq!(fell, Some((Phase::Back, word)));
    // The state page then reads as no swap at all, with the old image
    // whole: it boots, as after a power cut there.
    assert_eq!(boot_cut(&board, u32::MAX), (Ok(Boot::Normal), None));
    assert_eq!(active(&board, a.len()), a);
}

/// Each swap is stopped once, at its moment, and the next boot ends it
/// with the new image whole and on trial; each swap back likewise, with
/// the old image whole.
#[test]
fn each_phase_is_stopped_once_and_ends_at_the_next_boot() {
    let (a, b) = (a(), b());
    let mut sides = [0u32; 4];
    for (i, draw) in draws(48).enumerate() {
        let board = board(&a);
        stage(&board, &b).unwrap();
        let (got, fell) = boot_cut(&board, draw);
        let Some((Phase::Swap, swap)) = fell else {
            panic!("draw {draw:#x}: {fell:?}")
        };
        assert!(got.is_err(), "draw {draw:#x}");
        sides[usize::from(swap.side == Side::External) * 2 + usize::from(swap.op == Op::Erase)] +=
            1;
        let again = draw.rotate_left(16);
        assert_eq!(
            boot_cut(&board, again),
            (Ok(Boot::Trial), None),
            "draw {draw:#x}"
        );
        assert_eq!(active(&board, b.len()), b, "draw {draw:#x}");
        let noted = recorded(&board.internal).unwrap();
        assert_eq!(
            noted[0].unwrap().at,
            Moment::Us(moment(draw, phase_us(Phase::Swap)))
        );
        assert_eq!(noted[0].unwrap().fell, Some(swap));
        assert_eq!(noted[1], None);
        if i % 2 == 0 {
            assert_eq!(confirm(&board.internal), Ok(Started::Confirmed));
            assert_eq!(boot_cut(&board, draw), (Ok(Boot::Normal), None));
            assert_eq!(active(&board, b.len()), b);
            continue;
        }
        // Not confirmed: the swap back, stopped once too.
        let (got, fell) = boot_cut(&board, again);
        assert!(got.is_err(), "draw {draw:#x}");
        let Some((Phase::Back, back)) = fell else {
            panic!("draw {draw:#x}: {fell:?}")
        };
        assert_eq!(boot_cut(&board, draw), (Ok(Boot::Reverted), None));
        assert_eq!(active(&board, a.len()), a, "draw {draw:#x}");
        assert_eq!(
            recorded(&board.internal).unwrap()[1].unwrap().fell,
            Some(back)
        );
        assert_eq!(confirm(&board.internal), Ok(Started::Reverted));
        assert_eq!(boot_cut(&board, draw), (Ok(Boot::Normal), None));
    }
    // The cuts fell on both flashes, in erases and in writes.
    assert!(sides.iter().all(|&n| n > 0), "{sides:?}");
}

/// A swap begun by a loader without the bench, then taken up by one,
/// draws a moment the rest of the copy may never reach, and is not
/// stopped twice.
#[test]
fn a_swap_taken_up_from_a_power_cut_is_stopped_at_most_once() {
    let (a, b) = (a(), b());
    let board = board(&a);
    stage(&board, &b).unwrap();
    power(&board, Some(3 * (ACTIVE.len / PAGE) as usize));
    let _ = boot(&board);
    power(&board, None);
    let (first, _) = boot_cut(&board, 0);
    let second = boot_cut(&board, 0);
    assert!(first.is_err() || first == Ok(Boot::Trial));
    assert!(matches!(second, (Ok(Boot::Trial), None)), "{second:?}");
    assert_eq!(active(&board, b.len()), b);
}

#[test]
fn the_external_chip_is_left_running() {
    let mut chip = fake::chip();
    ExternalPartial.erase(&mut chip, 0, 5_000).unwrap();
    assert!(!chip.done());
    chip.finish().unwrap();
    let bytes = [0u8; PAGE as usize];
    ExternalPartial
        .write(&mut chip, PAGE, &bytes, 3 * ext_flash::PROGRAM_TYP_US + 100)
        .unwrap();
    assert!(!chip.done());
    assert_eq!(chip.fake().programs, 4);
}

#[test]
fn a_cut_reads_as_the_bench_script_parses_it() {
    let fell = Fell {
        side: Side::External,
        op: Op::Erase,
        page: 113,
        permille: 370,
    };
    let swap = Recorded {
        phase: Phase::Swap,
        at: Moment::Us(12_345_678),
        fell: Some(fell),
    };
    assert_eq!(
        swap.to_string(),
        "bench reset in the swap at 12345678 us: external erase of page 113, 37.0 % through"
    );
    let back = Recorded {
        phase: Phase::Back,
        at: Moment::Us(5),
        fell: None,
    };
    assert_eq!(
        back.to_string(),
        "bench reset in the swap back at 5 us: not reached"
    );
}

/// A board with the image staged, both moments chosen, and the swap
/// done if `phase` is the swap back: the phase about to start.
fn chosen(a: &[u8], b: &[u8], phase: Phase, op: Option<u32>) -> Board {
    let board = board(a);
    stage(&board, b).unwrap();
    let swap = (phase == Phase::Swap).then_some(op).flatten();
    let back = (phase == Phase::Back).then_some(op).flatten();
    choose(&board.internal, Phase::Swap, swap).unwrap();
    choose(&board.internal, Phase::Back, back).unwrap();
    if phase == Phase::Back {
        assert_eq!(boot_cut(&board, 0), (Ok(Boot::Trial), None));
    }
    board
}

/// The operations `phase` makes at the boot that starts it, as the
/// power counts them: the host test's enumeration.
fn enumerate(phase: Phase) -> Vec<Done> {
    let board = chosen(&a(), &b(), phase, None);
    board.power.borrow_mut().log.clear();
    let _ = boot(&board);
    let log = board.power.borrow().log.clone();
    assert_eq!(log.len(), operations(phase) as usize);
    log
}

/// Each operation of each phase chosen in turn: the loader stops the
/// copy in the operation a power cut at that index takes, notes it, and
/// the next boot ends the phase with the image expected whole.
#[test]
fn a_chosen_moment_falls_in_the_operation_the_power_cut_of_its_index_takes() {
    let (a, b) = (a(), b());
    for phase in [Phase::Swap, Phase::Back] {
        let ops = enumerate(phase);
        for (n, done) in ops.iter().enumerate() {
            let n = n as u32;
            let board = chosen(&a, &b, phase, Some(n));
            let (got, fell) = boot_cut(&board, 0);
            assert!(got.is_err(), "{phase:?} {n}");
            let (at, fell) = fell.unwrap_or_else(|| panic!("{phase:?} {n}: not stopped"));
            assert_eq!(at, phase, "{n}");
            let fell_as = Done {
                side: fell.side,
                op: fell.op,
                page: fell.page,
            };
            assert_eq!(fell_as, *done, "{phase:?} {n}");
            let noted = recorded(&board.internal).unwrap()[phase as usize].unwrap();
            assert_eq!((noted.at, noted.fell), (Moment::Op(n), Some(fell)));
            let (again, none) = boot_cut(&board, 0);
            assert_eq!(none, None, "{phase:?} {n}");
            match phase {
                Phase::Swap => {
                    assert_eq!(again, Ok(Boot::Trial), "{n}");
                    assert_eq!(active(&board, b.len()), b, "{phase:?} {n}");
                }
                Phase::Back => {
                    // A cut in the swap back's last word leaves the state
                    // page as with no swap asked for.
                    let again = again.unwrap();
                    assert!(
                        matches!(again, Boot::Reverted | Boot::Normal),
                        "{n}: {again:?}"
                    );
                    assert_eq!(active(&board, a.len()), a, "{phase:?} {n}");
                }
            }
        }
    }
}

#[test]
fn a_moment_is_chosen_only_once_for_a_swap_not_begun() {
    let (a, b) = (a(), b());
    let board = board(&a);
    assert_eq!(
        choose(&board.internal, Phase::Swap, Some(0)),
        Err(ChooseError::NoSwap)
    );
    stage(&board, &b).unwrap();
    let last = operations(Phase::Back);
    assert_eq!(
        choose(&board.internal, Phase::Back, Some(last)),
        Err(ChooseError::Beyond(last))
    );
    choose(&board.internal, Phase::Back, Some(3)).unwrap();
    assert_eq!(
        choose(&board.internal, Phase::Back, Some(4)),
        Err(ChooseError::Chosen)
    );
    // No moment for the swap: none is drawn either.
    choose(&board.internal, Phase::Swap, None).unwrap();
    assert_eq!(boot_cut(&board, 0), (Ok(Boot::Trial), None));
    assert_eq!(
        choose(&board.internal, Phase::Back, Some(3)),
        Err(ChooseError::NoSwap)
    );
    let noted = recorded(&board.internal).unwrap()[0].unwrap();
    assert_eq!(
        noted.to_string(),
        "bench reset in the swap at operation 1363: not reached"
    );
}
