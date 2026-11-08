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

/// Bytes of the image name that a heartbeat and a piece carry: the first
/// four of the image's 32-byte name (spec "Updates over the mesh").
pub const TAG_LEN: usize = 4;

/// The first [`TAG_LEN`] bytes of an image's name: what the mesh names it
/// by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct ImageTag(pub [u8; TAG_LEN]);

/// An image as a heartbeat's section describes it
/// (spec "Updates over the mesh").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Image {
    /// The first bytes of its name, the SHA-256 of the 48 bytes of its
    /// header that its signatures cover (`bark-image`, `sign.rs`); block
    /// 0 carries the whole name.
    pub name: ImageTag,
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

    /// The name the mesh carries.
    pub fn tag(&self) -> ImageTag {
        self.name
    }
}

/// Coded pieces a whole block is made of: any this many distinct pieces
/// of a block give it back (spec "Updates over the mesh").
pub const BLOCK_PIECES: usize = 64;

/// Pieces a block can be sent as, indices 0 to 254: the code works in a
/// field of 256 elements, one of which is not a piece.
pub const PIECES: usize = 255;

/// What a node's heartbeat says of its images, in a section after its
/// first 13 bytes (spec "Updates over the mesh", "Packet formats").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Section {
    /// The version of the image it runs, 0 for none known.
    pub runs: u32,
    /// The newest image it holds or gathers, if any.
    pub image: Option<Image>,
    /// The blocks of that image it holds from the first: all of them when
    /// it holds it whole, else the first block it lacks.
    pub complete: u8,
    /// The pieces it holds of block `complete`, the first it lacks.
    pub have: u8,
}

impl Section {
    /// Whether it gathers `tag` and lacks a block.
    pub fn gathers(&self, tag: ImageTag) -> bool {
        self.image
            .is_some_and(|i| i.tag() == tag && self.complete < i.blocks)
    }
}

/// The most blocks an image the mesh carries has, the table's among
/// them: the lanes the firmware keeps pieces in, one a block
/// (`bark-image`, `pieces.rs`, `MAX_BLOCKS`; roadmap D150).
pub const MAX_BLOCKS: usize = 48;

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

/// How often a node's heartbeats carry its section when nothing makes
/// them carry it every time: for one rotation of its heartbeats, so on
/// each channel, once an hour (spec "Updates over the mesh").
pub const SECTION_EVERY: Duration = Duration::from_secs(3600);

/// Bytes of a heartbeat's section: the version it runs (4), the newest
/// image's version (4), name (4), key (1), blocks (1), last block's
/// pieces (1), the blocks it holds from the first (1), and the pieces it
/// holds of the block after them (1).
pub const SECTION_LEN: usize = 17;

/// The share of pieces a relay that lacked them missed, in percent: 20.5
/// in the district at the design rate, seed 1, over the first 6 hours
/// of an update (`docs/mesh-update-2026-11-07.md`, "Heartbeat sections
/// and a sweep"). A helper sends a block's pieces over 1 less this share.
pub const PIECE_LOSS_PERCENT: u64 = 20;

/// The pieces a helper sends of a block that a neighbour lacks `lacks`
/// of: enough at [`PIECE_LOSS_PERCENT`] lost.
pub fn sweep_pieces(lacks: u8) -> u8 {
    (u64::from(lacks) * 100)
        .div_ceil(100 - PIECE_LOSS_PERCENT)
        .min(PIECES as u64) as u8
}

/// How long a sleepy node that gathers listens after its anchor's slot
/// heartbeat for the update frame the anchor sends then: the anchor's
/// check of the channel, about 3 ms, the transmission's start, under 1 ms
/// (spec "Heartbeat"), and a full frame.
pub const SLOT_LISTEN: Duration = Duration::from_micros(FULL_FRAME.as_micros() + 5_000);

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
