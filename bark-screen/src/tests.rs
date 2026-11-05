extern crate std;

use std::string::String;
use std::vec::Vec;

use super::*;

const FIXED: Fixed<'static> = Fixed {
    id: b"bae6e6cbf4f5",
    image: "node",
    version: 12,
};

fn texts(state: &State, up_min: u32) -> Vec<String> {
    lines(&FIXED, state, up_min, None)
        .iter()
        .flatten()
        .map(|(t, _)| String::from_utf8(t.bytes().to_vec()).unwrap())
        .collect()
}

/// 2026-11-05 12:35:19 UTC: as UTC, at UTC+02:00, at UTC−03:30 and at
/// UTC+14:00, past midnight.
const AT: u64 = 1_793_882_119;
const CLOCKS: [Option<Clock>; 4] = [
    Some(Clock {
        utc_s: AT,
        offset_min: None,
    }),
    Some(Clock {
        utc_s: AT,
        offset_min: Some(120),
    }),
    Some(Clock {
        utc_s: AT,
        offset_min: Some(-210),
    }),
    Some(Clock {
        utc_s: AT,
        offset_min: Some(14 * 60),
    }),
];

#[test]
fn the_time_as_utc_or_local_with_the_offset() {
    let shown: Vec<String> = CLOCKS
        .iter()
        .map(|c| {
            let t = lines(&FIXED, &State::default(), 0, *c);
            let (last, _) = t.iter().flatten().last().unwrap();
            String::from_utf8(last.bytes().to_vec()).unwrap()
        })
        .collect();
    assert_eq!(
        shown,
        ["UTC 12:35", "14:35 +02", "09:05 -03:30", "02:35 +14"]
    );
    // Unknown, no line.
    assert_eq!(texts(&State::default(), 0).last().unwrap(), "UP 0 MIN");
}

#[test]
fn a_sleepy_carried_node_on_battery() {
    let state = State {
        starting: false,
        placed: true,
        sleepy: true,
        anchored: true,
        carried: true,
        battery: Battery::Percent(73),
        heard: 5,
        strong: 4,
        both_ways: 2,
    };
    assert_eq!(
        texts(&state, 12),
        [
            "BAE6E6CBF4F5",
            "NODE V12",
            "SLEEPY ANCHORED",
            "CARRIED",
            "HEARD 5",
            "STRONG 4",
            "BOTH WAYS 2",
            "BATTERY 73%",
            "UP 12 MIN",
        ]
    );
}

#[test]
fn a_relay_starting_without_a_place() {
    let state = State {
        starting: true,
        battery: Battery::Usb,
        ..State::default()
    };
    assert_eq!(
        texts(&state, 0),
        [
            "BAE6E6CBF4F5",
            "NODE V12",
            "RELAY",
            "NO PLACE YET",
            "STARTING",
            "HEARD 0",
            "STRONG 0",
            "BOTH WAYS 0",
            "ON USB",
            "UP 0 MIN",
        ]
    );
}

#[test]
fn uptime_in_minutes_hours_and_days() {
    let up = |m| texts(&State::default(), m).last().unwrap().clone();
    assert_eq!(up(119), "UP 119 MIN");
    assert_eq!(up(120), "UP 2 HOURS");
    assert_eq!(up(48 * 60 - 1), "UP 47 HOURS");
    assert_eq!(up(48 * 60), "UP 2 DAYS");
}

/// Every line of every state fits across the panel, every character in
/// it is in the font, and the lines stay on the panel.
#[test]
fn every_line_fits_and_is_drawable() {
    let widest = State {
        starting: true,
        sleepy: true,
        battery: Battery::NotRead,
        heard: 64,
        strong: 64,
        both_ways: 64,
        ..State::default()
    };
    for state in [widest, State::default()] {
        for (up, clock) in [0, 119, 47 * 60, u32::MAX].into_iter().zip(CLOCKS) {
            for (t, y) in lines(&FIXED, &state, up, clock).iter().flatten() {
                assert!(t.bytes().len() <= LINE, "{:?}", t.bytes());
                assert!(text_width(t.bytes().len(), SCALE) <= WIDTH);
                assert!(y + line_height(SCALE) <= HEIGHT);
                for &c in t.bytes() {
                    assert!(glyph(c).is_some(), "no glyph for {}", c as char);
                }
            }
        }
    }
}

/// The picture is the lines drawn: something black in each line's band,
/// white outside them.
#[test]
fn the_picture_is_the_lines() {
    let state = State::default();
    let mut p = Picture::white();
    draw(&mut p, &FIXED, &state, 3, CLOCKS[0]);
    let rows: Vec<usize> = lines(&FIXED, &state, 3, CLOCKS[0])
        .iter()
        .flatten()
        .map(|(_, y)| *y)
        .collect();
    let inked = |y: usize| (0..WIDTH).any(|x| p.black(x, y));
    for &y in &rows {
        assert!(
            (y..y + line_height(SCALE)).any(inked),
            "line at {y} is blank"
        );
    }
    let last = rows.last().unwrap() + line_height(SCALE);
    assert!(!(last..HEIGHT).any(inked));
    assert!(!(0..rows[0]).any(inked));
    // Drawing again over an old picture leaves nothing of it.
    let mut q = Picture::white();
    draw(&mut q, &FIXED, &State { heard: 9, ..state }, 3, None);
    draw(&mut q, &FIXED, &state, 3, CLOCKS[0]);
    assert!(q.bytes == p.bytes);
}

#[test]
fn line_width_and_count() {
    assert_eq!(line_chars(2), 16);
    assert_eq!(text_width(16, 2), 190);
}
