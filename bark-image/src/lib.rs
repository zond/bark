//! What the T-Echo firmware and its loader must agree on: the flash
//! layout ([`layout`]), the header a staged image travels with
//! ([`Header`]) and the record of it the loader checks ([`record`]).
//! `docs/mesh-update-2026-10-25.md` is the design; `bark-fw/README.md`,
//! "Updates", how it is used.

#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

#[cfg(test)]
extern crate std;

#[cfg(feature = "boot")]
pub mod boot;
pub mod layout;
pub mod record;

/// Seconds a new image has to confirm itself before the watchdog resets
/// the node and the loader swaps the previous image back: the 10 minutes
/// of the note ("Revert"), two worst-case rotations and a margin, within
/// which an image will also have to hear a neighbour once the confirmation
/// asks for it. The loader is the part that is not updated, so it gives
/// the longest time the confirmation will need, not the time it needs now.
pub const TRIAL_SECONDS: u32 = 600;

/// Bytes of a header, which leave room for what the key policy adds: up
/// to three signatures, each an Ed25519 signature of 64 bytes (RFC 8032)
/// with a byte naming the key, 195 bytes after the 48 of format 1 (the
/// note, "Keys" and "Frames").
pub const HEADER_LEN: usize = 256;

/// The first bytes of every header.
pub const MAGIC: [u8; 4] = *b"bark";

/// This header's layout.
pub const FORMAT: u8 = 1;

/// The bytes of [`Header::hash`]: SHA-256 (FIPS 180-4).
pub const HASH_LEN: usize = 32;

/// What travels in front of an image to be staged, unsigned for now.
///
/// ```text
/// 0   magic "bark"
/// 4   format, 1
/// 5   3 bytes, zero
/// 8   version, u32 little-endian, raised with every image
/// 12  length of the image in bytes, u32 little-endian
/// 16  SHA-256 of the image's bytes
/// 48  208 bytes, zero: room for the signatures
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    /// The image's version: an image is staged only above the running one's.
    pub version: u32,
    /// The image's bytes, from the start of [`layout::ACTIVE`].
    pub length: u32,
    /// SHA-256 of those bytes.
    pub hash: [u8; HASH_LEN],
}

/// Why a header was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderError {
    /// Not a header: the magic is wrong.
    Magic,
    /// A format this firmware does not read.
    Format(u8),
    /// An image of no bytes, or larger than the slot it must run from.
    Length(u32),
}

impl Header {
    /// The header as bytes.
    pub fn encode(&self) -> [u8; HEADER_LEN] {
        let mut b = [0u8; HEADER_LEN];
        b[0..4].copy_from_slice(&MAGIC);
        b[4] = FORMAT;
        b[8..12].copy_from_slice(&self.version.to_le_bytes());
        b[12..16].copy_from_slice(&self.length.to_le_bytes());
        b[16..48].copy_from_slice(&self.hash);
        b
    }

    /// A header from bytes, if it is one this firmware can stage.
    pub fn decode(b: &[u8; HEADER_LEN]) -> Result<Header, HeaderError> {
        if b[0..4] != MAGIC {
            return Err(HeaderError::Magic);
        }
        if b[4] != FORMAT {
            return Err(HeaderError::Format(b[4]));
        }
        let word = |at: usize| u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]);
        let length = word(12);
        if length == 0 || length > layout::ACTIVE.len {
            return Err(HeaderError::Length(length));
        }
        let mut hash = [0u8; HASH_LEN];
        hash.copy_from_slice(&b[16..48]);
        Ok(Header {
            version: word(8),
            length,
            hash,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header() -> Header {
        Header {
            version: 7,
            length: 167_996,
            hash: core::array::from_fn(|i| i as u8),
        }
    }

    #[test]
    fn a_header_round_trips() {
        assert_eq!(Header::decode(&header().encode()), Ok(header()));
    }

    #[test]
    fn the_bytes_are_the_documented_ones() {
        let b = header().encode();
        assert_eq!(&b[0..8], b"bark\x01\0\0\0");
        assert_eq!(&b[8..12], &7u32.to_le_bytes());
        assert_eq!(&b[12..16], &167_996u32.to_le_bytes());
        assert_eq!(b[16], 0);
        assert_eq!(b[47], 31);
        assert!(b[48..].iter().all(|&x| x == 0));
    }

    #[test]
    fn a_wrong_header_is_refused() {
        let mut b = header().encode();
        b[0] = b'B';
        assert_eq!(Header::decode(&b), Err(HeaderError::Magic));
        let mut b = header().encode();
        b[4] = 2;
        assert_eq!(Header::decode(&b), Err(HeaderError::Format(2)));
        let too_long = layout::ACTIVE.len + 1;
        let b = Header {
            length: too_long,
            ..header()
        }
        .encode();
        assert_eq!(Header::decode(&b), Err(HeaderError::Length(too_long)));
        let b = Header {
            length: 0,
            ..header()
        }
        .encode();
        assert_eq!(Header::decode(&b), Err(HeaderError::Length(0)));
    }
}
