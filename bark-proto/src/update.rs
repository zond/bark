//! What the core knows of firmware images spreading over the mesh (spec
//! "Updates over the mesh"): how an image is named, the pieces it travels
//! in, and the pace its frames keep. The node's side is in
//! `node/relay/update.rs`; the frames are in `wire.rs`. The pieces' bytes,
//! the code that makes them, the flash they wait in and the signature
//! check are the driver's (`docs/core-interface.md` §4).

use crate::event::MAX_FRAME;
use crate::radio::{time_on_air, MIN_PREAMBLE_SYMBOLS};
use crate::tables::budget;
use crate::time::Duration;

/// Bytes of the image name that the request and the piece carry: the first
/// four of the image's 32-byte hash (spec "Updates over the mesh").
pub const TAG_LEN: usize = 4;

/// The first [`TAG_LEN`] bytes of an image's hash: what a request and a
/// piece name it by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct ImageTag(pub [u8; TAG_LEN]);

/// An image as its announcement describes it
/// (spec "Updates over the mesh").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Image {
    /// SHA-256 of the image's signed header, the 48 bytes its
    /// signatures cover (`bark-image`, `sign.rs`): its name everywhere.
    pub id: [u8; 32],
    /// The signed version number; only the policy test reads it.
    pub version: u32,
    /// Blocks the image travels in, block 0 the block table, 2 to 255.
    pub blocks: u8,
    /// Pieces in the last block, 1 to [`BLOCK_PIECES`]: with `blocks`, the
    /// pieces of every block ([`Image::pieces`]).
    pub last: u8,
    /// Which of the signers' keys signed it, for the policy test.
    pub key: u8,
}

impl Image {
    /// The distinct pieces that give back block `b`, 0 past the last: the
    /// table's, the image's hash and 8 bytes of each other block's hash in
    /// pieces of [`crate::wire::PIECE_BYTES`]; [`BLOCK_PIECES`]; and the
    /// last block's (spec "Updates over the mesh").
    pub fn pieces(&self, b: u8) -> u8 {
        let others = usize::from(self.blocks.saturating_sub(1));
        match b {
            0 => (32 + 8 * others).div_ceil(crate::wire::PIECE_BYTES) as u8,
            _ if b >= self.blocks => 0,
            _ if b + 1 == self.blocks => self.last,
            _ => BLOCK_PIECES as u8,
        }
    }

    /// The name a request and a piece carry.
    pub fn tag(&self) -> ImageTag {
        let mut t = [0; TAG_LEN];
        t.copy_from_slice(&self.id[..TAG_LEN]);
        ImageTag(t)
    }
}

/// Coded pieces a whole block is made of: any this many distinct pieces
/// of a block give it back (spec "Updates over the mesh").
pub const BLOCK_PIECES: usize = 64;

/// Pieces a block can be sent as, indices 0 to 254: the code works in a
/// field of 256 elements, one of which is not a piece.
pub const PIECES: usize = 255;

/// The frames a holder sends for a request that lacks [`BLOCK_PIECES`]
/// pieces ([`credit`]).
const CREDIT_PER_64: u32 = 99;

/// How many pieces a holder sends for a request that lacks `lacks`
/// pieces of a block: enough that every node on the channel that lacks
/// as many has them, at the most frames the first note on updates
/// derives for any loss and channel it tabulates, 99 for 64 pieces, 1.55
/// times as many (`docs/mesh-update-2026-10-25.md`, "Relays": 74
/// receivers losing a fifth of the frames, all done with 99 % odds).
pub fn credit(lacks: u8) -> u8 {
    (u32::from(lacks) * CREDIT_PER_64)
        .div_ceil(BLOCK_PIECES as u32)
        .clamp(1, PIECES as u32) as u8
}

/// The airtime of a full frame, a piece's, 138.6 ms.
pub const FULL_FRAME: Duration = time_on_air(MAX_FRAME as u8, MIN_PREAMBLE_SYMBOLS);

/// The pace of update frames: at most one a channel in this time, a full
/// frame's airtime a hundred times over, so that a node's update frames
/// take at most 1 % of a channel's time (spec "Updates over the mesh").
pub const PACE: Duration = FULL_FRAME.mul(100);

/// How many frames of the send queue may go before an update frame that
/// is due: the full frames the bucket refills in one [`PACE`], 9.83,
/// rounded, less the update frame itself. A node at its cap then sends an
/// update frame about once a pace, as an idle one does
/// (spec "Updates over the mesh").
pub const PASS: u8 = {
    let refilled = budget::most_on_air(PACE).as_micros() - budget::DEPTH.as_micros();
    let full = FULL_FRAME.as_micros();
    ((refilled + full / 2) / full - 1) as u8
};

const _: () = assert!(PASS == 9);

/// Channels a node sends update frames on within one [`PACE`]: a node is
/// deaf on its home channel while it sends elsewhere
/// (spec "Updates over the mesh").
pub const SERVED_CHANNELS: usize = 2;

/// How long a node that gathers waits without a piece it takes before it
/// asks again (spec "Updates over the mesh").
pub const REQUEST_WAIT: Duration = Duration::from_secs(600);

/// How often a node that holds an image announces it on its home channel,
/// and on one other channel (spec "Updates over the mesh").
pub const ANNOUNCE_EVERY: Duration = Duration::from_secs(3600);

/// How long a node that holds the image sends no pieces on its home
/// channel after it heard a piece of it there from another: two paces,
/// the most between two pieces of a holder that keeps the pace.
pub const HOLD_BACK: Duration = PACE.mul(2);

/// A piece of a block the node gathers, for the driver to keep
/// (`Action::StorePiece`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Piece {
    /// The block.
    pub block: u8,
    /// Which of the block's [`PIECES`] it is.
    pub index: u8,
    /// Its bytes, as the frame carried them.
    pub bytes: [u8; crate::wire::PIECE_BYTES],
}
