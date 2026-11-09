//! The screen's default picture (roadmap D160): a QR code of the project's
//! page, where a passer-by gets the app, with a small orb web in its
//! centre and the web's threads in the margin, and one line under it.
//!
//! The symbol is version 3 at level H, the smallest that holds the
//! address's 19 bytes at H, whose 24 bytes version 2 does not reach at 14
//! (ISO/IEC 18004, the data capacity table). Level H restores about 30 %
//! of the codewords, so the emblem may cover some modules: it covers about
//! 7 % of them, which the decoders read through (`scripts/qr.py check`).
//! The threads stay out of the code and the quiet zone around it.

use crate::{Picture, HEIGHT, WIDTH};

mod symbol;
pub use symbol::{ADDRESS, MASK, MODULES, SIZE, VERSION};

/// Pixels a module: the largest whole number that leaves room for the
/// quiet zone across the 200-pixel panel and a line of text below.
pub const MODULE: usize = 5;
/// The code's side, pixels.
pub const SIDE: usize = SIZE * MODULE;
/// The quiet zone kept clear of the threads, in modules. The standard asks
/// for 4 (ISO/IEC 18004, quiet zone); the panel's height leaves 2 above
/// and below beside the line of text, and phones' readers take 2; there
/// are more than 4 at the sides.
pub const QUIET: usize = 2;
/// The code's top left corner: centred across, and high enough that the
/// line under it is clear of the quiet zone.
pub const LEFT: usize = (WIDTH - SIDE) / 2;
pub const TOP: usize = 12;
/// The top of the line under the code.
pub const LINE_TOP: usize = TOP + SIDE + QUIET * MODULE + 5;

const _: () = assert!(TOP >= QUIET * MODULE);
const _: () = assert!(LINE_TOP + crate::line_height(crate::SCALE) <= HEIGHT);

/// The emblem's radius, pixels from the code's centre, inside which the
/// modules are cleared: a module whose centre falls inside is one of
/// about π (22/5)² ≈ 61, 7 % of the 841.
const EMBLEM: i32 = 22;

/// The web's radials: twelve, every 30°, as cos and sin × 1000.
const COS: [i32; 12] = [
    1000, 866, 500, 0, -500, -866, -1000, -866, -500, 0, 500, 866,
];
fn sin(k: usize) -> i32 {
    COS[(k + 9) % 12]
}

/// The steps of drawing the code ([`draw`]), each short enough to give the
/// executor back between them.
pub const PARTS: usize = SIZE + 2;

/// The centre of the code, the hub of the web.
const fn hub() -> (i32, i32) {
    let half = (SIZE / 2 * MODULE + MODULE / 2) as i32;
    (LEFT as i32 + half, TOP as i32 + half)
}

/// Whether the module at `(col, row)` is dark.
pub fn dark(col: usize, row: usize) -> bool {
    MODULES[row] & (1 << col) != 0
}

/// Whether the module at `(col, row)` lies under the emblem.
pub fn under_emblem(col: usize, row: usize) -> bool {
    let (hx, hy) = hub();
    let x = (LEFT + col * MODULE + MODULE / 2) as i32 - hx;
    let y = (TOP + row * MODULE + MODULE / 2) as i32 - hy;
    x * x + y * y <= EMBLEM * EMBLEM
}

/// Whether the board's pixel `(x, y)` lies in the emblem's circle.
pub fn in_emblem(x: usize, y: usize) -> bool {
    let (hx, hy) = hub();
    (x as i32 - hx).pow(2) + (y as i32 - hy).pow(2) <= EMBLEM * EMBLEM
}

/// Step `part` of the code into `picture`, which starts white: the rows of
/// modules, then the emblem, then the threads in the margin.
pub fn draw(picture: &mut Picture, part: usize) {
    match part {
        row if row < SIZE => {
            for col in (0..SIZE).filter(|&c| dark(c, row)) {
                for dy in 0..MODULE {
                    for dx in 0..MODULE {
                        let (x, y) = (LEFT + col * MODULE + dx, TOP + row * MODULE + dy);
                        if !in_emblem(x, y) {
                            picture.plot(x, y);
                        }
                    }
                }
            }
        }
        p if p == SIZE => emblem(picture),
        _ => threads(picture),
    }
}

/// A web inside the circle the modules leave white: the radials, a spiral of straight
/// threads between them as an orb web's capture spiral is, and the hub.
fn emblem(picture: &mut Picture) {
    let (hx, hy) = hub();
    let reach = EMBLEM - 3;
    for k in 0..12 {
        line(picture, (hx, hy), at(k, reach * 1000), |_, _| true);
    }
    // From 4 pixels out, 4 pixels a turn, to the radials' ends.
    let (r0, pitch) = (4 * 1000, 4 * 1000 / 12);
    let mut k = 0;
    while r0 + (k as i32 + 1) * pitch <= reach * 1000 {
        let a = at(k % 12, r0 + k as i32 * pitch);
        let b = at((k + 1) % 12, r0 + (k as i32 + 1) * pitch);
        line(picture, a, b, |_, _| true);
        k += 1;
    }
    for (dx, dy) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)] {
        picture.plot((hx + dx) as usize, (hy + dy) as usize);
    }
}

/// The web beyond the code: the radials continued to the panel's edge and
/// two frame threads across them, drawn only outside the code's quiet zone
/// and the line of text.
fn threads(picture: &mut Picture) {
    let (hx, hy) = hub();
    let clear = (QUIET * MODULE) as i32;
    let (l, t) = (LEFT as i32 - clear, TOP as i32 - clear);
    let (r, b) = (l + (SIDE as i32) + 2 * clear, t + (SIDE as i32) + 2 * clear);
    let text = LINE_TOP as i32 - 2..(LINE_TOP + crate::line_height(crate::SCALE)) as i32;
    let margin =
        |x: i32, y: i32| !((l..r).contains(&x) && (t..b).contains(&y)) && !text.contains(&y);
    // Past the panel's farthest corner from the hub, 100 √2 pixels.
    let far = 150 * 1000;
    for k in 0..12 {
        line(picture, (hx, hy), at(k, far), margin);
    }
    for ring in [112, 128] {
        for k in 0..12 {
            line(
                picture,
                at(k, ring * 1000),
                at((k + 1) % 12, ring * 1000),
                margin,
            );
        }
    }
}

/// The point on radial `k` at `milli` thousandths of a pixel from the hub.
fn at(k: usize, milli: i32) -> (i32, i32) {
    let (hx, hy) = hub();
    let r = i64::from(milli);
    let x = r * i64::from(COS[k]) / 1_000_000;
    let y = r * i64::from(sin(k)) / 1_000_000;
    (hx + x as i32, hy - y as i32)
}

/// A line of single pixels from `a` to `b` (Bresenham's), each where
/// `keep` allows and on the panel.
fn line(picture: &mut Picture, a: (i32, i32), b: (i32, i32), keep: impl Fn(i32, i32) -> bool) {
    let (mut x, mut y) = a;
    let (dx, dy) = ((b.0 - x).abs(), -(b.1 - y).abs());
    let (sx, sy) = (if x < b.0 { 1 } else { -1 }, if y < b.1 { 1 } else { -1 });
    let mut err = dx + dy;
    loop {
        let on = (0..WIDTH as i32).contains(&x) && (0..HEIGHT as i32).contains(&y);
        if on && keep(x, y) {
            picture.plot(x as usize, y as usize);
        }
        if (x, y) == b {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

#[cfg(test)]
mod tests;
