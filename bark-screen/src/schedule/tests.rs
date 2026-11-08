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

/// A schedule whose last picture was of `state`, over at `at`.
fn shown(state: State, at: u64) -> Schedule {
    let mut s = Schedule::new();
    s.drawn(state, at);
    s
}

#[test]
fn the_first_picture_at_once() {
    assert_eq!(Schedule::new().due(&usb(), 5_000), 5_000);
}

#[test]
fn unchanged_redrawn_when_stale() {
    assert_eq!(shown(usb(), 1_000).due(&usb(), 2_000), 1_000 + USB_STALE_MS);
    let mut s = shown(battery(), 1_000);
    assert_eq!(s.due(&battery(), 2_000), 1_000 + BATTERY_STALE_MS);
}

#[test]
fn counts_and_level_wait_for_the_gap() {
    let counted = State {
        heard: 3,
        strong: 1,
        ..usb()
    };
    assert_eq!(shown(usb(), 1_000).due(&counted, 2_000), 1_000 + USB_GAP_MS);
    let lower = State {
        battery: Battery::Low,
        ..battery()
    };
    let mut s = shown(battery(), 1_000);
    assert_eq!(s.due(&lower, 2_000), 1_000 + BATTERY_GAP_MS);
    let read = State {
        battery: Battery::NotRead,
        ..battery()
    };
    assert_eq!(s.due(&read, 2_000), 1_000 + BATTERY_GAP_MS);
}

#[test]
fn the_role_phone_and_power_one_refresh_after() {
    let changes = [
        State {
            sleepy: true,
            ..battery()
        },
        State {
            carried: true,
            ..battery()
        },
        State {
            starting: true,
            ..battery()
        },
        State {
            phone: Phone::Code(123_456),
            ..battery()
        },
        State {
            phone: Phone::Public,
            ..battery()
        },
        usb(),
    ];
    for c in changes {
        let mut s = shown(battery(), 1_000);
        assert_eq!(s.due(&c, 5_000), 5_000 + REFRESH_MS, "{c:?}");
    }
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
    let mut s = shown(sleepy, 1_000);
    assert_eq!(s.due(&carried, 5_000), 1_000 + BATTERY_GAP_MS);
}

#[test]
fn quick_changes_drawn_once() {
    let mut s = shown(battery(), 1_000);
    let off = usb();
    let paired = State {
        phone: Phone::Paired,
        ..off
    };
    assert_eq!(s.due(&off, 5_000), 5_000 + REFRESH_MS);
    // A second change within the refresh joins the first's picture.
    assert_eq!(s.due(&paired, 6_000), 5_000 + REFRESH_MS);
    // And a stream of them does not hold it back.
    let code = State {
        phone: Phone::Code(1),
        ..off
    };
    assert_eq!(s.due(&code, 6_900), 5_000 + REFRESH_MS);
    s.drawn(code, 8_000);
    assert_eq!(s.due(&code, 8_000), 8_000 + USB_STALE_MS);
}

#[test]
fn a_change_undone_within_the_refresh_is_not_drawn() {
    let mut s = shown(battery(), 1_000);
    assert_eq!(s.due(&usb(), 5_000), 5_000 + REFRESH_MS);
    assert_eq!(s.due(&battery(), 6_000), 1_000 + BATTERY_STALE_MS);
    // Undone and done again: counted from the second time.
    assert_eq!(s.due(&usb(), 6_500), 6_500 + REFRESH_MS);
}

#[test]
fn never_later_than_the_schedule() {
    // Past the gap already: the change waits for no refresh.
    let mut s = shown(battery(), 1_000);
    let now = 1_000 + BATTERY_GAP_MS + 1;
    assert_eq!(s.due(&usb(), now), 1_000 + USB_GAP_MS);
}

#[test]
fn a_press_keeps_the_schedule() {
    let mut s = shown(battery(), 1_000);
    let counted = State {
        heard: 2,
        ..battery()
    };
    s.pressed(counted, 30_000);
    assert_eq!(s.due(&counted, 31_000), 1_000 + BATTERY_STALE_MS);
    let mut first = Schedule::new();
    first.pressed(counted, 30_000);
    assert_eq!(first.due(&counted, 31_000), 30_000 + BATTERY_STALE_MS);
}
