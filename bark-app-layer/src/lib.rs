//! What the app puts inside the network's opaque payload, and the mailbox
//! a home node keeps (spec "Phone link", "Mailbox";
//! `docs/app-layer-2026-11-05.md`). One codec for the node, the
//! simulator and the vectors the app's tests read (`vectors.json`).
//!
//! The cryptography is libsodium's constructions, here through
//! RustCrypto's implementations of them, which make the same bytes:
//! `crypto_box` between identities, the sealed box for a first message,
//! `crypto_secretbox` between a person and their mailbox, and the
//! BLAKE2b key derivation. Nothing allocates and nothing draws random
//! numbers: callers hand in every nonce and ephemeral key.

#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod card;
pub mod carry;
pub mod flash_ring;
pub mod keys;
pub mod mailbox;
pub mod plain;
pub mod seal;
pub mod tables;
pub mod text;

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod vectors;

use bark_proto::{NodeId, Position};

/// The format's version, the high four bits of a payload's first byte:
/// 2 since the recipient tag (spec "Phone link", The payload).
pub const VERSION: u8 = 2;

/// The longest payload the network carries (spec "Packet formats").
pub const MAX_PAYLOAD: usize = bark_proto::wire::MAX_PAYLOAD;

/// The longest payload the app writes: what a mailbox can still send on
/// as a reply, which adds [`seal::REPLY_OVERHEAD`].
pub const MAX_APP_PAYLOAD: usize = MAX_PAYLOAD - seal::REPLY_OVERHEAD;

/// `sent` counts seconds from 2026-01-01 00:00 UTC, this Unix time.
pub const EPOCH_UNIX_S: u64 = 1_767_225_600;

/// Why bytes were not taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// It does not fit where it goes.
    TooLong,
    /// Not in this layout, or a version or form this crate does not read.
    Malformed,
    /// The tag does not check: another key, or changed on the way.
    Open,
}

/// Where the network delivers: a position and a node ID (spec
/// "Addresses").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Address {
    /// The position, the node's own or its anchor's.
    pub position: Position,
    /// The node ID.
    pub node: NodeId,
}

impl Address {
    /// Bytes on the wire: the position, then the node ID.
    pub const LEN: usize = Position::WIRE_LEN + NodeId::LEN;

    /// The wire bytes.
    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut b = [0u8; Self::LEN];
        b[..Position::WIRE_LEN].copy_from_slice(&self.position.encode());
        b[Position::WIRE_LEN..].copy_from_slice(&self.node.0);
        b
    }

    /// The address in the first [`Address::LEN`] bytes of `b`.
    pub fn decode(b: &[u8]) -> Result<Address, Error> {
        if b.len() < Self::LEN {
            return Err(Error::Malformed);
        }
        let mut pos = [0u8; Position::WIRE_LEN];
        pos.copy_from_slice(&b[..Position::WIRE_LEN]);
        let mut node = [0u8; NodeId::LEN];
        node.copy_from_slice(&b[Position::WIRE_LEN..Self::LEN]);
        Ok(Address {
            position: Position::decode(&pos),
            node: NodeId(node),
        })
    }
}

/// An identity: a person's X25519 public key (`keys`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Identity(pub [u8; 32]);

impl Identity {
    /// Bytes in an identity.
    pub const LEN: usize = 32;
}

/// The big-endian number in `b`, at most four bytes.
pub(crate) fn be(b: &[u8]) -> u32 {
    b.iter().fold(0, |n, &x| (n << 8) | u32::from(x))
}
