use super::*;
use crate::Phone;

/// A fixed relay on USB with no phone, and the same node on battery.
fn usb() -> State {
    State {
        placed: true,
        battery: Battery::Usb,
        ..State::default()
    }
}

fn battery() -> State {
    State {
        battery: Battery::Percent(80),
        ..usb()
    }
}

fn code(c: u32) -> State {
    State {
        phone: Phone::Code(c),
        ..battery()
    }
}

/// A schedule whose last picture was the code, of `state`.
fn shown(state: State) -> Schedule {
    let mut s = Schedule::new();
    s.drawn(Face::Code, state);
    s
}

/// A schedule showing the lines of `state`, touched at `at`.
fn lines(state: State, at: u64) -> Schedule {
    let mut s = shown(state);
    s.touched(at);
    assert_eq!(s.due(&state, at), Some(at));
    s.drawn(Face::Lines, state);
    s
}

#[test]
fn the_first_picture_at_once_and_the_code() {
    let mut s = Schedule::new();
    assert_eq!(s.due(&usb(), 5_000), Some(5_000));
    assert_eq!(s.face(5_000), Face::Code);
}

/// The code shows nothing that changes but the pairing code: no other
/// change is drawn, nor is it drawn again to look fresh.
#[test]
fn the_code_waits_for_nothing_else() {
    let changes = [
        State {
            sleepy: true,
            ..battery()
        },
        State {
            heard: 3,
            ..battery()
        },
        State {
            phone: Phone::Private,
            ..battery()
        },
        usb(),
    ];
    for c in changes {
        assert_eq!(shown(battery()).due(&c, 5_000), None, "{c:?}");
    }
    assert_eq!(shown(battery()).due(&battery(), u64::MAX / 2), None);
}

/// A touch shows the lines at once, for `LINES_MS`, then the code again;
/// a second touch brings the code back sooner.
#[test]
fn a_touch_shows_the_lines_for_a_while() {
    let mut s = lines(battery(), 10_000);
    assert_eq!(s.face(10_000 + LINES_MS - 1), Face::Lines);
    assert_eq!(s.due(&battery(), 12_000), Some(10_000 + LINES_MS));
    let back = 10_000 + LINES_MS;
    assert_eq!(s.face(back), Face::Code);
    assert_eq!(s.due(&battery(), back), Some(back));
    s.drawn(Face::Code, battery());
    assert_eq!(s.due(&battery(), back + 1), None);
    // Touched again while the lines show.
    let mut s = lines(battery(), 10_000);
    s.touched(15_000);
    assert_eq!(s.face(15_000), Face::Code);
    assert_eq!(s.due(&battery(), 15_000), Some(15_000));
    s.drawn(Face::Code, battery());
    // And the next touch shows the lines again.
    s.touched(20_000);
    assert_eq!(s.due(&battery(), 20_000), Some(20_000));
}

/// While the lines show, a change of the role, the phone or the power is
/// drawn one refresh after it; the counts and the level wait.
#[test]
fn the_lines_keep_the_urgent_changes() {
    let changes = [
        State {
            sleepy: true,
            ..battery()
        },
        State {
            starting: true,
            ..battery()
        },
        State {
            phone: Phone::Public,
            ..battery()
        },
        usb(),
    ];
    for c in changes {
        let mut s = lines(battery(), 1_000);
        assert_eq!(s.due(&c, 5_000), Some(5_000 + REFRESH_MS), "{c:?}");
    }
    let counted = State {
        heard: 3,
        battery: Battery::Low,
        ..battery()
    };
    let mut s = lines(battery(), 1_000);
    assert_eq!(s.due(&counted, 5_000), Some(1_000 + LINES_MS));
}

/// A new pairing code is drawn at once on either picture, and the end of
/// the pairing one refresh after.
#[test]
fn a_pairing_code_at_once() {
    for mut s in [shown(battery()), lines(battery(), 1_000)] {
        assert_eq!(s.due(&code(123_456), 5_000), Some(5_000));
        let face = s.face(5_000);
        s.drawn(face, code(123_456));
        assert_eq!(s.due(&code(7), 6_000), Some(6_000));
        s.drawn(face, code(7));
        let paired = State {
            phone: Phone::Private,
            ..battery()
        };
        assert_eq!(s.due(&paired, 7_000), Some(7_000 + REFRESH_MS));
    }
    // The code line shows on the code too: drawn after the refresh.
    let mut s = shown(code(5));
    assert_eq!(s.due(&code(5), 9_000), None);
}

#[test]
fn a_sleepy_nodes_carried_flag_is_not_its_role() {
    let sleepy = State {
        sleepy: true,
        ..battery()
    };
    let carried = State {
        carried: true,
        ..sleepy
    };
    let mut s = lines(sleepy, 1_000);
    assert_eq!(s.due(&carried, 5_000), Some(1_000 + LINES_MS));
}

#[test]
fn quick_changes_drawn_once() {
    let mut s = lines(battery(), 1_000);
    let off = usb();
    let paired = State {
        phone: Phone::Private,
        ..off
    };
    assert_eq!(s.due(&off, 5_000), Some(5_000 + REFRESH_MS));
    // A second change within the refresh joins the first's picture.
    assert_eq!(s.due(&paired, 6_000), Some(5_000 + REFRESH_MS));
    // And a stream of them does not hold it back.
    let public = State {
        phone: Phone::Public,
        ..off
    };
    assert_eq!(s.due(&public, 6_900), Some(5_000 + REFRESH_MS));
    s.drawn(Face::Lines, public);
    assert_eq!(s.due(&public, 8_000), Some(1_000 + LINES_MS));
}

#[test]
fn a_change_undone_within_the_refresh_is_not_drawn() {
    let mut s = lines(battery(), 1_000);
    assert_eq!(s.due(&usb(), 5_000), Some(5_000 + REFRESH_MS));
    assert_eq!(s.due(&battery(), 6_000), Some(1_000 + LINES_MS));
    // Undone and done again: counted from the second time.
    assert_eq!(s.due(&usb(), 6_500), Some(6_500 + REFRESH_MS));
}

/// A flip of the role redraws the lines at once, even while they show,
/// and the 30 s count from it.
#[test]
fn a_flip_draws_the_lines_at_once() {
    let mut s = lines(battery(), 1_000);
    s.lines(9_000);
    assert_eq!(s.face(9_000), Face::Lines);
    assert_eq!(s.due(&battery(), 9_000), Some(9_000));
    s.drawn(Face::Lines, battery());
    assert_eq!(s.due(&battery(), 9_500), Some(9_000 + LINES_MS));
}
