extern crate std;

use std::vec::Vec;

use super::*;
use crate::{code_line, Face, Fixed, Phone, State, LINE, SCALE};

const FIXED: Fixed<'static> = Fixed {
    id: b"bae6e6cbf4f5",
    image: "node",
    version: 12,
    discharge_bench: false,
};

/// The screen's code drawn as the node draws it, on the board's axes.
fn drawn(state: &State) -> Picture {
    let mut p = Picture::white();
    crate::draw(&mut p, Face::Code, &FIXED, state, 0, None);
    p
}

/// The symbol checked in is the address encoded again by another
/// encoder at the same version, level and mask: a changed address, or a
/// hand-edited module, fails here (`scripts/qr.py generate` writes it).
#[test]
fn the_symbol_is_the_address() {
    use qrcodegen::{QrCode, QrCodeEcc, QrSegment, Version};
    let segs = [QrSegment::make_bytes(ADDRESS.as_bytes())];
    let v = Version::new(VERSION);
    let mask = Some(qrcodegen::Mask::new(MASK));
    let q = QrCode::encode_segments_advanced(&segs, QrCodeEcc::High, v, v, mask, false).unwrap();
    assert_eq!(q.size() as usize, SIZE);
    for row in 0..SIZE {
        for col in 0..SIZE {
            assert_eq!(
                q.get_module(col as i32, row as i32),
                dark(col, row),
                "({col}, {row})"
            );
        }
    }
    // The smallest version at level H that holds it.
    let smaller = Version::new(VERSION - 1);
    assert!(QrCode::encode_segments_advanced(
        &segs,
        QrCodeEcc::High,
        Version::MIN,
        smaller,
        None,
        false
    )
    .is_err());
}

/// The emblem covers at most about 8 % of the modules, and none of the
/// finder, timing, alignment or format patterns.
#[test]
fn the_emblem_is_small_and_spares_the_patterns() {
    let under: Vec<(usize, usize)> = (0..SIZE)
        .flat_map(|r| (0..SIZE).map(move |c| (c, r)))
        .filter(|&(c, r)| under_emblem(c, r))
        .collect();
    std::println!("under the emblem: {} of {}", under.len(), SIZE * SIZE);
    assert!(under.len() * 100 <= 8 * SIZE * SIZE, "{}", under.len());
    for (c, r) in under {
        // The finders with their separators and the format beside them,
        // in three 9 × 9 corners; the timing on row and column 6; the
        // alignment pattern at 20 to 24.
        let near = |a: usize| a < 9;
        let far = |a: usize| a >= SIZE - 8;
        assert!(!(near(c) && near(r) || far(c) && near(r) || near(c) && far(r)));
        assert!(c != 6 && r != 6);
        assert!(!((20..=24).contains(&c) && (20..=24).contains(&r)));
    }
}

/// The threads stay out of the code and its quiet zone: there the
/// picture is the modules, or white under the emblem's circle.
#[test]
fn the_threads_keep_out_of_the_code() {
    let p = drawn(&State::default());
    let q = (QUIET * MODULE) as isize;
    for y in TOP as isize - q..(TOP + SIDE) as isize + q {
        for x in LEFT as isize - q..(LEFT + SIDE) as isize + q {
            let (x, y) = (x as usize, y as usize);
            let inside = (LEFT..LEFT + SIDE).contains(&x) && (TOP..TOP + SIDE).contains(&y);
            if !inside {
                assert!(!p.black(x, y), "the quiet zone at ({x}, {y})");
                continue;
            }
            let (c, r) = ((x - LEFT) / MODULE, (y - TOP) / MODULE);
            if !in_emblem(x, y) {
                assert_eq!(p.black(x, y), dark(c, r), "({x}, {y})");
            }
        }
    }
    // Something drawn in the margin: the threads.
    assert!((0..HEIGHT).any(|y| p.black(2, y)));
}

/// The picture as the screen shows it reads as the address, emblem,
/// threads and line included. With `BARK_SCREEN_PICTURE` set it is
/// written there as a PBM, for `scripts/qr.py check` to decode with other
/// decoders.
#[test]
fn the_code_reads() {
    for state in [
        State::default(),
        State {
            phone: Phone::Code(123_456),
            ..State::default()
        },
    ] {
        let p = drawn(&state);
        let mut img = rqrr::PreparedImage::prepare_from_greyscale(WIDTH, HEIGHT, |x, y| {
            if p.black(x, y) {
                0
            } else {
                255
            }
        });
        let grids = img.detect_grids();
        assert_eq!(grids.len(), 1);
        let (meta, text) = grids[0].decode().unwrap();
        assert_eq!(text, ADDRESS);
        assert_eq!(meta.version.0, usize::from(VERSION));
        if let Some(path) = std::env::var_os("BARK_SCREEN_PICTURE") {
            if state.phone == Phone::Unpaired {
                std::fs::write(path, pbm(&p)).unwrap();
            }
        }
    }
}

/// The picture as a raw PBM, 1 black.
pub(crate) fn pbm(p: &Picture) -> Vec<u8> {
    let mut out = std::format!("P4\n{WIDTH} {HEIGHT}\n").into_bytes();
    for y in 0..HEIGHT {
        for x in (0..WIDTH).step_by(8) {
            let byte = (0..8).fold(0u8, |b, i| b | (u8::from(p.black(x + i, y)) << (7 - i)));
            out.push(byte);
        }
    }
    out
}

/// The line under the code: the ID, or the pairing code while a phone
/// pairs, drawn in its band and nowhere else below the code.
#[test]
fn the_line_under_the_code() {
    let text = |state: &State| {
        std::string::String::from_utf8(code_line(&FIXED, state).bytes().to_vec()).unwrap()
    };
    assert_eq!(text(&State::default()), "ID  BAE6E6CBF4F5");
    let pairing = State {
        phone: Phone::Code(42),
        ..State::default()
    };
    assert_eq!(text(&pairing), "PAIR CODE 000042");
    assert_eq!(code_line(&FIXED, &pairing).bytes().len(), LINE);
    let p = drawn(&pairing);
    let band = LINE_TOP..LINE_TOP + crate::line_height(SCALE);
    let inked = |y: usize| (2..WIDTH - 2).any(|x| p.black(x, y));
    assert!(band.clone().any(inked));
}
