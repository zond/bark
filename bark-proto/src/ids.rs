//! Identifiers carried on the wire (design §9.1, §14).

/// A leaf's 4-byte identifier. Zero means "the relay itself".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct LeafId(pub [u8; 4]);

impl LeafId {
    /// The relay-itself address.
    pub const RELAY: LeafId = LeafId([0; 4]);
}

/// A 32-bit message identifier, random at origination and kept across hops.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct MsgId(pub u32);
