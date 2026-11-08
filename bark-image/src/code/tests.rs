//! The field, and blocks of every size coded and given back from many
//! choices of their pieces.

use std::vec;
use std::vec::Vec;

use super::*;

/// A fixed sequence of pseudo-random numbers (xorshift64), so that a
/// failure repeats.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn fill(&mut self, b: &mut [u8]) {
        for x in b {
            *x = self.next() as u8;
        }
    }

    /// `n` distinct indices of `0..CODED`, in random order.
    fn subset(&mut self, n: usize) -> Vec<u8> {
        let mut all: Vec<u8> = (0..CODED as u8).collect();
        for i in 0..n {
            let j = i + self.below(CODED - i);
            all.swap(i, j);
        }
        all.truncate(n);
        all
    }
}

/// Multiplication as polynomials over GF(2), reduced by the field's
/// polynomial: what the tables must agree with.
fn slow_mul(a: u8, b: u8) -> u8 {
    let mut p: u16 = 0;
    for bit in 0..8 {
        if b >> bit & 1 == 1 {
            p ^= u16::from(a) << bit;
        }
    }
    for bit in (8..16).rev() {
        if p >> bit & 1 == 1 {
            p ^= POLY << (bit - 8);
        }
    }
    p as u8
}

fn block(n: usize, rng: &mut Rng) -> Vec<[u8; PIECE]> {
    let mut data = vec![[0u8; PIECE]; n];
    for d in &mut data {
        rng.fill(d);
    }
    data
}

fn coded(data: &[[u8; PIECE]]) -> Vec<[u8; PIECE]> {
    (0..CODED as u8)
        .map(|k| encode_piece(data, k).unwrap())
        .collect()
}

/// The block back from the pieces of `indices`.
fn back(all: &[[u8; PIECE]], indices: &[u8], n: usize) -> Vec<[u8; PIECE]> {
    let pieces: Vec<_> = indices.iter().map(|&k| all[usize::from(k)]).collect();
    let mut out = vec![[0u8; PIECE]; n];
    decode(indices, &pieces, &mut out).unwrap();
    out
}

#[test]
fn x_generates_the_field() {
    let mut seen = [false; 256];
    for i in 0..ORDER {
        let e = EXP[i];
        assert!(e != 0 && !seen[usize::from(e)], "x^{i} repeats");
        seen[usize::from(e)] = true;
        assert_eq!(EXP[i + ORDER], e);
    }
    assert!(EXP[2 * ORDER..].iter().all(|&e| e == 0));
}

#[test]
fn the_tables_multiply() {
    for a in 0..=255u8 {
        for b in 0..=255u8 {
            assert_eq!(mul(a, b), slow_mul(a, b), "{a} × {b}");
        }
    }
}

#[test]
fn mul_add_adds_a_product() {
    let mut rng = Rng(7);
    let mut piece = [0u8; PIECE];
    rng.fill(&mut piece);
    piece[0] = 0;
    for f in [0u8, 1, 2, 0x8e, 255] {
        let mut acc = [0u8; PIECE];
        rng.fill(&mut acc);
        let before = acc;
        mul_add(&mut acc, f, &piece);
        for i in 0..PIECE {
            assert_eq!(acc[i], before[i] ^ slow_mul(f, piece[i]));
        }
    }
}

#[test]
fn data_pieces_are_the_data() {
    let mut rng = Rng(1);
    for n in [1, 2, 33, DATA] {
        let data = block(n, &mut rng);
        for (i, d) in data.iter().enumerate() {
            assert_eq!(&encode_piece(&data, i as u8).unwrap(), d);
        }
    }
}

#[test]
fn every_block_size_round_trips() {
    let mut rng = Rng(2);
    for n in 1..=DATA {
        let data = block(n, &mut rng);
        let all = coded(&data);
        for _ in 0..3 {
            let indices = rng.subset(n);
            assert_eq!(back(&all, &indices, n), data, "n {n}, {indices:?}");
        }
    }
}

#[test]
fn any_64_of_255_give_a_block_back() {
    let mut rng = Rng(3);
    let data = block(DATA, &mut rng);
    let all = coded(&data);
    for _ in 0..20 {
        let indices = rng.subset(DATA);
        assert_eq!(back(&all, &indices, DATA), data);
    }
}

#[test]
fn erasures_at_the_ends_and_interleaved_all_recover() {
    let mut rng = Rng(4);
    for n in [1, 7, 33, DATA] {
        let data = block(n, &mut rng);
        let all = coded(&data);
        let everything: Vec<u8> = (0..CODED as u8).collect();
        let patterns: Vec<Vec<u8>> = vec![
            // The first lost: only the last pieces came.
            everything[CODED - n..].to_vec(),
            // The data lost but its last piece.
            everything[n - 1..2 * n - 1].to_vec(),
            // Every other lost, from either end.
            everything.iter().copied().step_by(2).take(n).collect(),
            everything
                .iter()
                .rev()
                .copied()
                .step_by(2)
                .take(n)
                .collect(),
            // Every third kept.
            everything.iter().copied().step_by(3).take(n).collect(),
            // Half the data and half from the end, in reverse order.
            (0..n.div_ceil(2) as u8)
                .chain((CODED - n / 2) as u8..CODED as u8)
                .rev()
                .collect(),
        ];
        for indices in patterns {
            assert_eq!(indices.len(), n);
            assert_eq!(back(&all, &indices, n), data, "n {n}, {indices:?}");
        }
    }
}

#[test]
fn an_encoder_takes_the_data_in_any_order() {
    let mut rng = Rng(5);
    let data = block(40, &mut rng);
    for k in [3u8, 40, 200, 254] {
        let mut e = Encoder::new(data.len(), k).unwrap();
        for i in (0..data.len()).rev() {
            if e.wants(i) {
                e.add(i, &data[i]);
            }
        }
        assert_eq!(e.finish(), encode_piece(&data, k).unwrap());
    }
    let e = Encoder::new(data.len(), 3).unwrap();
    assert_eq!((0..40).filter(|&i| e.wants(i)).count(), 1);
}

#[test]
fn wrong_pieces_are_refused() {
    let p = [[0u8; PIECE]; 3];
    let mut out = [[0u8; PIECE]; 2];
    assert_eq!(
        decode(&[1, 255], &p[..2], &mut out),
        Err(CodeError::Index(255))
    );
    assert_eq!(
        decode(&[9, 9], &p[..2], &mut out),
        Err(CodeError::Duplicate(9))
    );
    assert_eq!(decode(&[1], &p[..1], &mut out), Err(CodeError::Count));
    assert_eq!(decode(&[], &[], &mut []), Err(CodeError::Count));
    assert!(matches!(encode_piece(&p, 255), Err(CodeError::Index(255))));
    assert!(matches!(Encoder::new(DATA + 1, 0), Err(CodeError::Count)));
    assert!(matches!(Points::new(&[0; DATA + 1]), Err(CodeError::Count)));
}
