//! The piece a node serves next, built while the one before is on the air
//! (`bark-fw/README.md`, "Updates"). The core streams a block's pieces in
//! order of their index, so after piece `i` it most often asks for piece
//! `i + 1` of the same block; a coded piece takes tens of ms to build,
//! longer than a sleepy node listens between two pieces of a run.
//!
//! One piece is kept, named by its image, block and index and by the
//! store's generation at its build. It is handed out only for exactly that
//! name and generation, and only once; anything else drops it, and the
//! piece asked for is built then.

use bark_proto::update::PIECES;
use bark_proto::wire::PIECE_BYTES;

/// A piece the core serves: its image's version, its block and its index.
pub type Named = (u32, u8, u8);

/// The piece the core most likely asks for after `n`: the next of its
/// block, from the last index round to the first.
pub fn next(n: Named) -> Named {
    let (version, block, index) = n;
    (version, block, ((usize::from(index) + 1) % PIECES) as u8)
}

/// The one piece built ahead.
pub struct Ahead {
    kept: Option<(Named, u32)>,
    bytes: [u8; PIECE_BYTES],
}

impl Default for Ahead {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl Ahead {
    /// Nothing built.
    pub const EMPTY: Ahead = Ahead {
        kept: None,
        bytes: [0; PIECE_BYTES],
    };

    /// Keep `bytes`, piece `named` as built from the store of `generation`.
    pub fn keep(&mut self, named: Named, generation: u32, bytes: &[u8; PIECE_BYTES]) {
        self.kept = Some((named, generation));
        self.bytes = *bytes;
    }

    /// Write the piece kept into `out` if it is `asked`, built from the
    /// store of `generation`; whether it was. The piece kept goes either
    /// way.
    pub fn take(&mut self, asked: Named, generation: u32, out: &mut [u8; PIECE_BYTES]) -> bool {
        let hit = self.kept.take() == Some((asked, generation));
        if hit {
            *out = self.bytes;
        }
        hit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn piece(b: u8) -> [u8; PIECE_BYTES] {
        [b; PIECE_BYTES]
    }

    #[test]
    fn next_is_the_following_index_of_the_same_block() {
        assert_eq!(next((7, 3, 0)), (7, 3, 1));
        assert_eq!(next((7, 3, 63)), (7, 3, 64));
        assert_eq!(next((7, 3, 200)), (7, 3, 201));
        // The core's index climbs modulo the pieces a block can be sent as.
        assert_eq!(next((7, 3, (PIECES - 1) as u8)), (7, 3, 0));
    }

    #[test]
    fn only_the_exact_piece_is_handed_out() {
        let mut out = piece(0);
        let named = (8, 4, 100);
        for asked in [(9, 4, 100), (8, 5, 100), (8, 4, 101), (8, 4, 99)] {
            let mut a = Ahead::EMPTY;
            a.keep(named, 1, &piece(0xaa));
            assert!(!a.take(asked, 1, &mut out), "{:?}", asked);
            assert_eq!(out, piece(0));
            // A miss drops it: the piece is not handed out later either.
            assert!(!a.take(named, 1, &mut out));
        }
        let mut a = Ahead::EMPTY;
        a.keep(named, 1, &piece(0xaa));
        assert!(a.take(named, 1, &mut out));
        assert_eq!(out, piece(0xaa));
        // Once.
        out = piece(0);
        assert!(!a.take(named, 1, &mut out));
        assert_eq!(out, piece(0));
    }

    #[test]
    fn a_piece_of_another_store_generation_is_dropped() {
        let mut out = piece(0);
        let mut a = Ahead::EMPTY;
        a.keep((8, 4, 100), 1, &piece(0xaa));
        assert!(!a.take((8, 4, 100), 2, &mut out));
        assert_eq!(out, piece(0));
    }

    #[test]
    fn nothing_kept_hands_out_nothing() {
        let mut out = piece(0);
        let mut a = Ahead::EMPTY;
        assert!(!a.take((0, 0, 0), 0, &mut out));
        assert_eq!(out, piece(0));
    }

    #[test]
    fn the_newest_kept_wins() {
        let mut out = piece(0);
        let mut a = Ahead::EMPTY;
        a.keep((8, 4, 100), 1, &piece(0xaa));
        a.keep((8, 4, 101), 1, &piece(0xbb));
        assert!(a.take((8, 4, 101), 1, &mut out));
        assert_eq!(out, piece(0xbb));
    }
}
