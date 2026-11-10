extern crate std;

use std::vec;
use std::vec::Vec;

use super::*;

/// A pad probed at [`SETTLE_MS`] and found driven at `rest`.
fn resting(rest: bool) -> Pad {
    let mut pad = Pad::new();
    assert_eq!(pad.probe(rest, rest, SETTLE_MS), Event::Rest(rest));
    pad
}

/// Feed `pad` the levels at their times, each read at its edge and at
/// every deadline that falls before the next, as the firmware does; the
/// events in order, with their times.
fn run(pad: &mut Pad, edges: &[(u64, bool)], until: u64) -> Vec<(u64, Event)> {
    let mut events = Vec::new();
    let mut level = None;
    let mut edges = edges.iter().peekable();
    loop {
        let next_edge = edges.peek().map(|&&(t, _)| t);
        let deadline = pad.deadline();
        let (t, high) = match (next_edge, deadline) {
            (Some(e), Some(d)) if d < e => (d, level.unwrap()),
            (Some(_), _) => {
                let &(t, h) = edges.next().unwrap();
                level = Some(h);
                (t, h)
            }
            (None, Some(d)) if d <= until => (d, level.unwrap()),
            _ => return events,
        };
        if let Some(e) = pad.read(high, t) {
            events.push((t, e));
        }
    }
}

#[test]
fn settling_reads_nothing() {
    let mut pad = Pad::new();
    assert_eq!(pad.deadline(), Some(SETTLE_MS));
    // The sensor's own power-on edge, before it has calibrated.
    assert_eq!(pad.read(true, 20), None);
    assert_eq!(pad.read(false, 40), None);
    assert_eq!(pad.deadline(), Some(SETTLE_MS));
}

#[test]
fn probe_tells_driven_from_undriven() {
    // Follows the pull: nothing drives it.
    let mut pad = Pad::new();
    assert_eq!(pad.probe(true, false, SETTLE_MS), Event::Dead);
    assert_eq!(pad.deadline(), None);
    assert_eq!(pad.read(true, 2_000), None);
    assert_eq!(pad.read(false, 3_000), None);
    assert_eq!(pad.deadline(), None);
    // Holds against both pulls.
    assert_eq!(Pad::new().probe(false, false, SETTLE_MS), Event::Rest(false));
    assert_eq!(Pad::new().probe(true, true, SETTLE_MS), Event::Rest(true));
    // Moved between the reads: driven, the later read the level.
    assert_eq!(Pad::new().probe(false, true, SETTLE_MS), Event::Rest(true));
}

#[test]
fn touch_from_either_rest() {
    for rest in [false, true] {
        let mut pad = resting(rest);
        assert_eq!(pad.deadline(), None);
        let events = run(&mut pad, &[(2_000, !rest), (2_150, rest)], 20_000);
        assert_eq!(events, vec![(2_150 + BOUNCE_MS, Event::Touch)], "rest {rest}");
        assert_eq!(pad.deadline(), None);
        // And the next.
        let events = run(&mut pad, &[(3_000, !rest), (3_500, rest)], 20_000);
        assert_eq!(events, vec![(3_500 + BOUNCE_MS, Event::Touch)]);
    }
}

#[test]
fn bounce_is_not_a_touch() {
    let mut pad = resting(false);
    let events = run(&mut pad, &[(2_000, true), (2_000 + BOUNCE_MS - 1, false)], 20_000);
    assert_eq!(events, vec![]);
    assert_eq!(pad.deadline(), None);
}

#[test]
fn bounce_within_a_touch_is_one_touch() {
    let mut pad = resting(false);
    let edges = [(2_000, true), (2_100, false), (2_110, true), (2_300, false)];
    let events = run(&mut pad, &edges, 20_000);
    assert_eq!(events, vec![(2_300 + BOUNCE_MS, Event::Touch)]);
}

#[test]
fn hold_up_to_the_longest_touch_is_a_touch() {
    let mut pad = resting(false);
    let events = run(&mut pad, &[(2_000, true), (2_000 + HOLD_MS, false)], 30_000);
    assert_eq!(events, vec![(2_000 + HOLD_MS + BOUNCE_MS, Event::Touch)]);
}

#[test]
fn long_hold_becomes_the_rest() {
    let mut pad = resting(false);
    let events = run(&mut pad, &[(2_000, true)], 30_000);
    assert_eq!(events, vec![(2_000 + HOLD_MS + 1, Event::Held(true))]);
    assert_eq!(pad.deadline(), None);
    // Rests high now: a short low is a touch, as on a board that rests
    // high.
    let events = run(&mut pad, &[(40_000, false), (40_200, true)], 60_000);
    assert_eq!(events, vec![(40_200 + BOUNCE_MS, Event::Touch)]);
    // And a long low makes it rest low again.
    let events = run(&mut pad, &[(50_000, false)], 70_000);
    assert_eq!(events, vec![(50_000 + HOLD_MS + 1, Event::Held(false))]);
}

#[test]
fn long_hold_read_late_is_the_same() {
    // The deadline missed: the return after the longest touch is read
    // first. The hold still becomes the rest, and the return a change
    // from it.
    let mut pad = resting(false);
    assert_eq!(pad.read(true, 2_000), None);
    assert_eq!(pad.read(true, 2_000 + BOUNCE_MS), None);
    assert_eq!(pad.read(false, 2_000 + HOLD_MS + 500), Some(Event::Held(true)));
    assert_eq!(pad.read(false, 2_000 + HOLD_MS + 500 + BOUNCE_MS), None);
    assert_eq!(pad.deadline(), Some(2_000 + HOLD_MS + 500 + HOLD_MS + 1));
}

#[test]
fn pin_that_never_moves_gives_nothing() {
    for rest in [false, true] {
        let mut pad = resting(rest);
        assert_eq!(run(&mut pad, &[], 1_000_000), vec![]);
        assert_eq!(pad.read(rest, 1_000_000), None);
    }
}
