//! The code an update's blocks travel in (spec "Updates over the mesh"):
//! Reed-Solomon over GF(2⁸) in evaluation form. A block of `n` data pieces
//! is the polynomial of degree below `n` whose value at point `i` is data
//! piece `i`, a byte position at a time; coded piece `k` is its value at
//! point `k`. So pieces `0..n` are the data themselves, and any `n`
//! distinct pieces give the block back by Lagrange interpolation.
//!
//! [`Encoder`] and [`Points`] take pieces one at a time, as they are read
//! from flash; [`encode_piece`] and [`decode`] take them in RAM.
//!
//! **Cost.** Everything is [`mul_add`], 247 table look-ups, about 14
//! cycles each at 64 MHz (Cortex-M4 Technical Reference Manual,
//! instruction timings). A coded piece takes `n` of them, about 3.5 ms; a
//! block of 64 given back from none of its data pieces 64 × 64, about
//! 220 ms. Not measured on the chip.

/// Bytes of a piece: a full frame's 255 less its 8-byte header.
pub const PIECE: usize = 247;

/// Data pieces of a whole block.
pub const DATA: usize = 64;

/// Coded pieces a block can be sent as: an index a byte, one value spare
/// for "none".
pub const CODED: usize = 255;

/// The field's polynomial, x⁸ + x⁴ + x³ + x² + 1: primitive (Lin and
/// Costello, "Error Control Coding"), so the powers of x are every
/// non-zero element, which the tables need.
const POLY: u16 = 0x11D;

/// Non-zero elements: the powers of x repeat after this many.
const ORDER: usize = 255;

/// What [`LOG`] gives for zero: an index into [`EXP`]'s zeros, far enough
/// that a sum of two logarithms of which one is zero lands there.
const ZERO_LOG: u16 = 2 * ORDER as u16;

/// A power of two at or above `2 * ZERO_LOG`, so that an index masked
/// with `EXP_LEN - 1` needs no bounds check.
const EXP_LEN: usize = 1024;

/// x to the power of the index, twice over so that a sum of two
/// logarithms needs no reduction, then zeros for sums with [`ZERO_LOG`].
static EXP: [u8; EXP_LEN] = exp_table();

/// The power of x each element is; [`ZERO_LOG`] for zero.
static LOG: [u16; 256] = log_table();

const fn exp_table() -> [u8; EXP_LEN] {
    let mut t = [0u8; EXP_LEN];
    let mut x: u16 = 1;
    let mut i = 0;
    while i < ORDER {
        t[i] = x as u8;
        t[i + ORDER] = x as u8;
        x <<= 1;
        if x & 0x100 != 0 {
            x ^= POLY;
        }
        i += 1;
    }
    t
}

const fn log_table() -> [u16; 256] {
    let exp = exp_table();
    let mut t = [ZERO_LOG; 256];
    let mut i = 0;
    while i < ORDER {
        t[exp[i] as usize] = i as u16;
        i += 1;
    }
    t
}

fn exp(log: usize) -> u8 {
    EXP[log & (EXP_LEN - 1)]
}

fn log(x: u8) -> usize {
    usize::from(LOG[usize::from(x)])
}

/// The product of two elements.
pub fn mul(a: u8, b: u8) -> u8 {
    exp(log(a) + log(b))
}

/// `acc += factor × piece`, a byte at a time.
pub fn mul_add(acc: &mut [u8; PIECE], factor: u8, piece: &[u8; PIECE]) {
    if factor == 0 {
        return;
    }
    let f = log(factor);
    for (a, &b) in acc.iter_mut().zip(piece) {
        *a ^= exp(f + log(b));
    }
}

/// Why pieces were refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeError {
    /// No data pieces, more than [`DATA`], or fewer pieces than the block
    /// has.
    Count,
    /// A piece index at or above [`CODED`].
    Index(u8),
    /// The same piece twice.
    Duplicate(u8),
}

/// Up to [`DATA`] distinct points of a polynomial of degree below their
/// count: the factors that give its value anywhere from theirs.
///
/// The value at `x` is the sum over `j` of the value at `x_j` times
/// `L_j(x) = P(x) / ((x − x_j) w_j)`, `P(x)` the product of `x − x_m` over
/// every point and `w_j` that of `x_j − x_m` over every other; subtraction
/// is XOR. The `w_j` are kept as logarithms.
#[derive(Clone, Debug)]
pub struct Points {
    x: [u8; DATA],
    log_w: [u8; DATA],
    len: usize,
}

/// A point at which [`Points::factor`] gives the factors.
#[derive(Clone, Copy, Debug)]
pub struct At {
    x: u8,
    /// The logarithm of `P(x)`, or the point's position if `x` is one.
    p: Result<usize, usize>,
}

impl Points {
    /// No points.
    pub const EMPTY: Points = Points {
        x: [0; DATA],
        log_w: [0; DATA],
        len: 0,
    };

    /// The points, each a piece index below [`CODED`], all distinct.
    pub fn new(points: &[u8]) -> Result<Points, CodeError> {
        if points.len() > DATA {
            return Err(CodeError::Count);
        }
        let mut x = [0u8; DATA];
        let mut log_w = [0u8; DATA];
        for (j, &p) in points.iter().enumerate() {
            if usize::from(p) >= CODED {
                return Err(CodeError::Index(p));
            }
            if points[..j].contains(&p) {
                return Err(CodeError::Duplicate(p));
            }
            x[j] = p;
        }
        for (j, &xj) in points.iter().enumerate() {
            let sum: usize = points
                .iter()
                .filter(|&&xm| xm != xj)
                .map(|&xm| log(xj ^ xm))
                .sum();
            log_w[j] = (sum % ORDER) as u8;
        }
        Ok(Points {
            x,
            log_w,
            len: points.len(),
        })
    }

    /// How many points.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Point `j`.
    pub fn point(&self, j: usize) -> u8 {
        self.x[j]
    }

    /// Where `x` is among the points, if it is one.
    pub fn position(&self, x: u8) -> Option<usize> {
        self.x[..self.len].iter().position(|&p| p == x)
    }

    /// Prepare the factors for the value at `x`: `n` additions.
    pub fn at(&self, x: u8) -> At {
        let p = match self.position(x) {
            Some(j) => Err(j),
            None => Ok(self.x[..self.len].iter().map(|&xm| log(x ^ xm)).sum()),
        };
        At { x, p }
    }

    /// The factor of the value at point `j` in the value at `at`.
    pub fn factor(&self, at: &At, j: usize) -> u8 {
        match at.p {
            Err(hit) => u8::from(hit == j),
            Ok(log_p) => {
                let down = log(at.x ^ self.x[j]) + usize::from(self.log_w[j]);
                exp((log_p + 2 * ORDER - down) % ORDER)
            }
        }
    }
}

/// Coded piece `k` of a block of `n` data pieces, from the data pieces
/// fed to it one at a time in any order.
pub struct Encoder {
    points: Points,
    at: At,
    acc: [u8; PIECE],
}

impl Encoder {
    /// Piece `k` of a block of `n` data pieces.
    pub fn new(n: usize, k: u8) -> Result<Encoder, CodeError> {
        if n == 0 || n > DATA {
            return Err(CodeError::Count);
        }
        if usize::from(k) >= CODED {
            return Err(CodeError::Index(k));
        }
        let mut data = [0u8; DATA];
        for (i, d) in data.iter_mut().enumerate() {
            *d = i as u8;
        }
        let points = Points::new(&data[..n])?;
        let at = points.at(k);
        Ok(Encoder {
            points,
            at,
            acc: [0; PIECE],
        })
    }

    /// Whether data piece `i` counts towards this piece: for a data piece
    /// only itself, else all of them.
    pub fn wants(&self, i: usize) -> bool {
        i < self.points.len() && self.points.factor(&self.at, i) != 0
    }

    /// Feed data piece `i`, once.
    pub fn add(&mut self, i: usize, piece: &[u8; PIECE]) {
        if i < self.points.len() {
            mul_add(&mut self.acc, self.points.factor(&self.at, i), piece);
        }
    }

    /// The piece, once every data piece it wants was fed.
    pub fn finish(self) -> [u8; PIECE] {
        self.acc
    }
}

/// Coded piece `k` of the block `data`.
pub fn encode_piece(data: &[[u8; PIECE]], k: u8) -> Result<[u8; PIECE], CodeError> {
    let mut e = Encoder::new(data.len(), k)?;
    for (i, piece) in data.iter().enumerate() {
        e.add(i, piece);
    }
    Ok(e.finish())
}

/// The `out.len()` data pieces of a block from as many distinct coded
/// pieces, `pieces[j]` the one of index `indices[j]`; pieces beyond those
/// are not used.
pub fn decode(
    indices: &[u8],
    pieces: &[[u8; PIECE]],
    out: &mut [[u8; PIECE]],
) -> Result<(), CodeError> {
    let n = out.len();
    if n == 0 || n > DATA || indices.len() < n || pieces.len() < n {
        return Err(CodeError::Count);
    }
    let points = Points::new(&indices[..n])?;
    for (i, o) in out.iter_mut().enumerate() {
        let at = points.at(i as u8);
        *o = [0; PIECE];
        for (j, piece) in pieces[..n].iter().enumerate() {
            mul_add(o, points.factor(&at, j), piece);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
