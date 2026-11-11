//! Firmware images spreading over the mesh (spec "Updates over the mesh"):
//! how an image is named, its pieces, and the pace of its frames. The
//! node's side is in `node/relay/update.rs`, the frames in `wire.rs`. The
//! pieces' bytes, their coding, flash and signature check are the driver's
//! (`docs/core-interface.md`).

use crate::event::MAX_FRAME;
use crate::radio::{time_on_air, MIN_PREAMBLE_SYMBOLS};
use crate::tables::budget;
use crate::time::Duration;

/// An image as a heartbeat's section describes it. A version names one
/// image; block 0 carries its whole name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Image {
    /// The signed version number.
    pub version: u32,
    /// Blocks the image travels in, block 0 the block table; a node takes
    /// at most [`MAX_BLOCKS`].
    pub blocks: u8,
    /// Pieces in the last block, 1 to [`BLOCK_PIECES`]: with `blocks`, the
    /// pieces of every block ([`Image::pieces`]).
    pub last: u8,
    /// Which of the signers' keys signed it, for the policy test.
    pub key: u8,
}

impl Image {
    /// The distinct pieces that give back block `b`, 0 past the last. The
    /// table holds the 32-byte name and 8 bytes of each block's hash.
    pub fn pieces(&self, b: u8) -> u8 {
        match b {
            0 => (32 + 8 * usize::from(self.blocks)).div_ceil(crate::wire::PIECE_BYTES) as u8,
            _ if b >= self.blocks => 0,
            _ if b + 1 == self.blocks => self.last,
            _ => BLOCK_PIECES as u8,
        }
    }
}

/// Any this many distinct pieces of a block give it back.
pub const BLOCK_PIECES: usize = 64;

/// Pieces a block can be sent as, indices 0 to 254: the code works in a
/// field of 256 elements, one of which is not a piece.
pub const PIECES: usize = 255;

/// What a node's heartbeat says of its images (spec "Packet formats").
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
    /// Whether it gathers the image of `version` and lacks a block.
    pub fn gathers(&self, version: u32) -> bool {
        self.image
            .is_some_and(|i| i.version == version && self.complete < i.blocks)
    }
}

/// The most blocks an image has, the table's among them: the lanes the
/// firmware keeps pieces in (`bark-image`, `pieces.rs`, `MAX_BLOCKS`).
pub const MAX_BLOCKS: usize = 48;

/// The airtime of a full frame, a piece's, 138.6 ms.
pub const FULL_FRAME: Duration = time_on_air(MAX_FRAME as u8, MIN_PREAMBLE_SYMBOLS);

/// The pace: a full frame's airtime a hundred times over, 1 % of a
/// channel.
pub const PACE: Duration = FULL_FRAME.mul(100);

/// The fastest the duty cap lets a holder send pieces: one a refill of a
/// full frame, 138.6 ms × 600 / 59 = 1.41 s (spec "Duty cycle cap").
pub const FASTEST: Duration = budget::refill_time(FULL_FRAME);

const _: () = assert!(FASTEST.as_micros() / 10_000 == 140);

/// The most of a channel one holder's pieces take faster than the pace,
/// as an inverse: 1 in 30, 3.3 %. In the district it kept channels 2.0 %
/// busy, clear of the 3 to 4 % where the network collapses.
pub const GAP_FRAMES: u64 = 30;

/// The least time between a node's pieces on a channel, 4.16 s.
pub const GAP: Duration = FULL_FRAME.mul(GAP_FRAMES);

/// How many frames of the send queue may go before an update frame that
/// is due: the full frames the bucket refills in one [`PACE`], 9.83,
/// rounded, less the update frame itself, so a node at its cap sends one
/// about once a pace, as an idle one does.
pub const PASS: u8 = {
    let refilled = budget::most_on_air(PACE).as_micros() - budget::DEPTH.as_micros();
    let full = FULL_FRAME.as_micros();
    ((refilled + full / 2) / full - 1) as u8
};

const _: () = assert!(PASS == 9);

/// Channels a node sends update frames on within one [`PACE`]: it is
/// deaf on its home channel while it sends elsewhere.
pub const SERVED_CHANNELS: usize = 2;

/// How often a node's heartbeats carry its section when nothing makes
/// them carry it every time: one rotation an hour, so once on each channel.
pub const SECTION_EVERY: Duration = Duration::from_secs(3600);

/// How many of a gatherer's rotations of heartbeats a helper streams to it
/// after it last heard its section, said once a rotation: two ride out
/// one lost, and a stream to a gatherer that went stops within two.
pub const LEASE_ROTATIONS: u64 = 2;

/// How many worst-case rotations of heartbeats an image gathered over the
/// mesh listens for a node before it gives itself up. A listening receiver
/// loses about one heartbeat in ten (spec "Channel at boot"), so three
/// miss a lone neighbour one time in a thousand: a revert gives the image
/// up for good.
pub const HEARD_ROTATIONS: u64 = 3;

/// How long an image gathered over the mesh may run without hearing a
/// node, from its start: [`HEARD_ROTATIONS`] worst-case rotations, 13 ×
/// `heartbeat_max` each, and a heartbeat's lateness
/// ([`crate::node::heartbeat_lag`]), 3 × 195 s + 872.8 ms = 585.9 s at the
/// design's timing (spec "Heartbeat").
pub const fn heard_within(config: &crate::Config) -> Duration {
    Duration::from_micros(
        config
            .heartbeat_max
            .mul(crate::radio::CHANNELS as u64)
            .mul(HEARD_ROTATIONS)
            .as_micros()
            + crate::node::heartbeat_lag(config).as_micros(),
    )
}

/// Bytes of a heartbeat's section: the version it runs (4), the newest
/// image's version (4), key (1), blocks (1), last block's pieces (1), the
/// blocks it holds from the first (1), and the pieces it holds of the
/// block after them (1).
pub const SECTION_LEN: usize = 13;

/// The share of pieces a relay that lacked them missed, in percent: 20.5
/// in the district at the design rate, seed 1, over an update's first 6 h
/// (`docs/mesh-update-2026-11-07.md`).
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
