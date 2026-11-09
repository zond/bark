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
        [
            "TIME   12:35 UTC",
            "TIME   14:35 +02",
            "TIME 09:05 -0330",
            "TIME   02:35 +14"
        ]
    );
    // Unknown, no line.
    assert_eq!(
        texts(&State::default(), 0).last().unwrap(),
        "UP         0 MIN"
    );
}

#[test]
fn a_sleepy_carried_node_on_battery() {
    let state = State {
        starting: false,
        placed: true,
        sleepy: true,
        anchored: true,
        carried: true,
        forced: false,
        battery: Battery::Percent(73),
        heard: 5,
        strong: 4,
        both_ways: 2,
        phone: Phone::Private,
        getting: None,
    };
    assert_eq!(
        texts(&state, 12),
        [
            "SLEEPY ANCHORED ",
            "ID  BAE6E6CBF4F5",
            "NODE         V12",
            "PHONE    PRIVATE",
            "KNOWN      5/4/2",
            "BATTERY      73%",
            "UP        12 MIN",
        ]
    );
}

/// The image a node gathers, under the one it runs: its version and the
/// blocks it holds of all.
#[test]
fn a_relay_gathering_an_image() {
    let state = State {
        placed: true,
        battery: Battery::Usb,
        getting: Some(Getting {
            version: 14,
            complete: 14,
            blocks: 31,
        }),
        ..State::default()
    };
    assert_eq!(
        texts(&state, 0),
        [
            "FIXED RELAY     ",
            "ID  BAE6E6CBF4F5",
            "NODE         V12",
            "GET V14    14/31",
            "PHONE   UNPAIRED",
            "KNOWN      0/0/0",
            "POWER        USB",
            "UP         0 MIN",
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
            "RELAY NO PLACE  ",
            "STARTING        ",
            "ID  BAE6E6CBF4F5",
            "NODE         V12",
            "PHONE   UNPAIRED",
            "KNOWN      0/0/0",
            "POWER        USB",
            "UP         0 MIN",
        ]
    );
}

#[test]
fn a_fixed_relay_on_usb() {
    let state = State {
        placed: true,
        battery: Battery::Usb,
        heard: 12,
        strong: 9,
        both_ways: 3,
        ..State::default()
    };
    let shown = lines(&FIXED, &state, 90, CLOCKS[1]);
    let shown: Vec<String> = shown
        .iter()
        .flatten()
        .map(|(t, _)| String::from_utf8(t.bytes().to_vec()).unwrap())
        .collect();
    assert_eq!(
        shown,
        [
            "FIXED RELAY     ",
            "ID  BAE6E6CBF4F5",
            "NODE         V12",
            "PHONE   UNPAIRED",
            "KNOWN     12/9/3",
            "POWER        USB",
            "UP        90 MIN",
            "TIME   14:35 +02",
        ]
    );
}

/// The header of each role, a sleepy node's place not shown.
#[test]
fn the_role_and_the_place() {
    let role = |sleepy, anchored, placed, carried| {
        let state = State {
            sleepy,
            anchored,
            placed,
            carried,
            ..State::default()
        };
        String::from(texts(&state, 0)[0].trim_end())
    };
    assert_eq!(role(false, false, true, false), "FIXED RELAY");
    assert_eq!(role(false, false, true, true), "CARRIED RELAY");
    assert_eq!(role(false, false, false, false), "RELAY NO PLACE");
    for (placed, carried) in [(false, false), (true, false), (true, true)] {
        assert_eq!(role(true, true, placed, carried), "SLEEPY ANCHORED");
        assert_eq!(role(true, false, placed, carried), "SLEEPY NO ANCHOR");
    }    // A forced role says so in place of the rest.
    let forced = |sleepy| {
        let state = State {
            sleepy,
            forced: true,
            anchored: true,
            placed: true,
            ..State::default()
        };
        String::from(texts(&state, 0)[0].trim_end())
    };
    assert_eq!(forced(true), "SLEEPY FORCED");
    assert_eq!(forced(false), "RELAY FORCED");
}

#[test]
fn uptime_in_minutes_hours_and_days() {
    let up = |m| texts(&State::default(), m).last().unwrap().clone();
    assert_eq!(up(119), "UP       119 MIN");
    assert_eq!(up(120), "UP       2 HOURS");
    assert_eq!(up(48 * 60 - 1), "UP      47 HOURS");
    assert_eq!(up(48 * 60), "UP        2 DAYS");
}

/// Every line of every state fits across the panel, every character in
/// it is in the font, and the lines stay on the panel.
#[test]
fn every_line_fits_and_is_drawable() {
    // The counts at the neighbour table's size, 64
    // (`bark_proto::tables::neighbours::NEIGHBOURS`), and the longest role.
    let widest = State {
        starting: true,
        sleepy: true,
        battery: Battery::NotRead,
        heard: 64,
        strong: 64,
        both_ways: 64,
        getting: Some(Getting {
            version: 999,
            complete: 47,
            blocks: 48,
        }),
        ..State::default()
    };
    let relay = State {
        sleepy: false,
        ..widest
    };
    assert_eq!(texts(&widest, 0)[4], "GET V999   47/48");
    assert_eq!(texts(&widest, 0)[6], "KNOWN   64/64/64");
    let pairing = State {
        phone: Phone::Code(999_999),
        ..widest
    };
    for state in [widest, relay, pairing, State::default()] {
        for (up, clock) in [0, 119, 47 * 60, u32::MAX].into_iter().zip(CLOCKS) {
            for (t, y) in lines(&FIXED, &state, up, clock).iter().flatten() {
                assert_eq!(t.bytes().len(), LINE, "{:?}", t.bytes());
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
    draw(&mut p, Face::Lines, &FIXED, &state, 3, CLOCKS[0]);
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
    draw(
        &mut q,
        Face::Lines,
        &FIXED,
        &State { heard: 9, ..state },
        3,
        None,
    );
    draw(&mut q, Face::Lines, &FIXED, &state, 3, CLOCKS[0]);
    assert!(q.bytes == p.bytes);
}

#[test]
fn line_width_and_count() {
    assert_eq!(line_chars(2), 16);
    assert_eq!(text_width(16, 2), 190);
}

/// Every line starts at the same left margin, and a value ends at the
/// same right margin, the time's included.
#[test]
fn lines_align_left_and_values_right() {
    let state = State {
        placed: true,
        battery: Battery::Percent(7),
        heard: 3,
        ..State::default()
    };
    let margin = (WIDTH - text_width(LINE, SCALE)) / 2;
    for clock in CLOCKS {
        let mut p = Picture::white();
        draw(&mut p, Face::Lines, &FIXED, &state, 5, clock);
        for (t, y) in lines(&FIXED, &state, 5, clock).iter().flatten() {
            let band = *y..*y + line_height(SCALE);
            let inked = |x: usize| band.clone().any(|y| p.black(x, y));
            let left = (0..WIDTH).find(|&x| inked(x)).unwrap();
            // A glyph's first column may be blank: within one cell.
            assert!(
                left >= margin && left < margin + CELL * SCALE,
                "{:?} at {left}",
                t.bytes()
            );
            let right = (0..WIDTH).rev().find(|&x| inked(x)).unwrap();
            if t.bytes()[LINE - 1] != b' ' {
                assert!(right < WIDTH - margin && right >= WIDTH - margin - CELL * SCALE);
            }
        }
    }
}

/// The header, the role and `STARTING` while it shows, stands apart by
/// the gap; the rows below it follow one another, the first the ID.
#[test]
fn the_header_stands_apart() {
    for starting in [false, true] {
        let state = State {
            starting,
            ..State::default()
        };
        let shown = lines(&FIXED, &state, 0, CLOCKS[0]);
        let shown: Vec<&(Text, usize)> = shown.iter().flatten().collect();
        let header = if starting { 2 } else { 1 };
        let ys: Vec<usize> = shown.iter().map(|(_, y)| *y).collect();
        for i in 1..ys.len() {
            let gap = if i == header { GAP } else { 0 };
            assert_eq!(ys[i], ys[i - 1] + line_height(SCALE) + gap);
        }
        assert!(shown[header].0.bytes().starts_with(b"ID  "));
        assert_eq!(ys[0], TOP);
    }
}

/// The phone's line: none, paired, or the code to type while one pairs,
/// six digits with its leading zeros.
#[test]
fn the_phone_paired_or_its_code() {
    let phone = |phone| {
        texts(
            &State {
                phone,
                ..State::default()
            },
            0,
        )[3]
        .clone()
    };
    assert_eq!(phone(Phone::Unpaired), "PHONE   UNPAIRED");
    assert_eq!(phone(Phone::Private), "PHONE    PRIVATE");
    assert_eq!(phone(Phone::Public), "PHONE     PUBLIC");
    assert_eq!(phone(Phone::Code(42)), "PAIR CODE 000042");
    assert_eq!(phone(Phone::Code(999_999)), "PAIR CODE 999999");
}
