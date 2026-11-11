//! Update and Image ([`crate::uuid::UPDATE`], [`crate::uuid::IMAGE`]): the
//! app stages a new firmware image on its node and asks for the swap
//! (spec "Updates from the phone").
//!
//! | | value | bytes |
//! |---|---|---|
//! | Update, write | 0 then the image's header: begin; 1: commit; 2: reboot | 257, 1 |
//! | Update, read | [`Progress`]: state, refusal, bytes held, version, hash | 42 |
//! | Image, write | the image's offset, then its bytes from there | 5–244 |
//!
//! The node takes a piece only at the count it holds, so the app's read of
//! Update is the flow control and the point to resend from.

use crate::Malformed;

/// Bytes of the image's header (`bark_image::HEADER_LEN`).
pub const HEADER_LEN: usize = 256;

/// Bytes of the image's hash, SHA-256 (FIPS 180-4).
pub const HASH_LEN: usize = 32;

/// Update, written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command<'a> {
    /// Start staging the image this header describes, forgetting any
    /// image staged before.
    Begin(&'a [u8; HEADER_LEN]),
    /// Check the image held and ask for the swap at the next boot.
    Commit,
    /// Reset the node, to let the loader swap.
    Reboot,
}

impl<'a> Command<'a> {
    const BEGIN: u8 = 0;
    const COMMIT: u8 = 1;
    const REBOOT: u8 = 2;
    /// Bytes in the longest value, a begin: longer than one write at the
    /// node's MTU, so a long write (spec "The service").
    pub const MAX_LEN: usize = 1 + HEADER_LEN;

    /// The value, into `b`; returns its length.
    pub fn encode(&self, b: &mut [u8; Command::<'static>::MAX_LEN]) -> usize {
        match self {
            Command::Begin(h) => {
                b[0] = Self::BEGIN;
                b[1..].copy_from_slice(*h);
                Self::MAX_LEN
            }
            Command::Commit => {
                b[0] = Self::COMMIT;
                1
            }
            Command::Reboot => {
                b[0] = Self::REBOOT;
                1
            }
        }
    }

    /// Take a written value whole.
    pub fn decode(b: &'a [u8]) -> Result<Command<'a>, Malformed> {
        match (b.first(), b.len()) {
            (Some(&Self::BEGIN), Self::MAX_LEN) => Ok(Command::Begin(
                b[1..].try_into().expect("the header's bytes"),
            )),
            (Some(&Self::COMMIT), 1) => Ok(Command::Commit),
            (Some(&Self::REBOOT), 1) => Ok(Command::Reboot),
            (Some(&(Self::BEGIN | Self::COMMIT | Self::REBOOT)), _) | (None, _) => {
                Err(Malformed::Length)
            }
            (Some(_), _) => Err(Malformed::Value),
        }
    }
}

/// Image, written: bytes of the image from `offset`, counted from the
/// image's first byte after the header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Piece<'a> {
    /// Where the bytes start.
    pub offset: u32,
    /// The bytes.
    pub bytes: &'a [u8],
}

impl<'a> Piece<'a> {
    /// Bytes before the image's.
    pub const HEADER: usize = 4;
    /// The most bytes of the image a piece carries: one write at the
    /// node's MTU.
    pub const MAX_BYTES: usize = crate::value_room(crate::ATT_MTU) - Self::HEADER;
    /// Bytes in the longest value.
    pub const MAX_LEN: usize = Self::HEADER + Self::MAX_BYTES;

    /// The value, into `b`; returns its length.
    pub fn encode(&self, b: &mut [u8; Piece::<'static>::MAX_LEN]) -> usize {
        assert!(
            (1..=Self::MAX_BYTES).contains(&self.bytes.len()),
            "a piece of 1 to {} bytes",
            Self::MAX_BYTES
        );
        let len = Self::HEADER + self.bytes.len();
        b[..Self::HEADER].copy_from_slice(&self.offset.to_be_bytes());
        b[Self::HEADER..len].copy_from_slice(self.bytes);
        len
    }

    /// Take a written value whole.
    pub fn decode(b: &'a [u8]) -> Result<Piece<'a>, Malformed> {
        if !(Self::HEADER + 1..=Self::MAX_LEN).contains(&b.len()) {
            return Err(Malformed::Length);
        }
        Ok(Piece {
            offset: u32::from_be_bytes(b[..Self::HEADER].try_into().expect("four bytes")),
            bytes: &b[Self::HEADER..],
        })
    }
}

/// Where the node's update stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// No image is being staged.
    Idle = 0,
    /// A header is taken and the image's bytes are coming.
    Receiving = 1,
    /// Every byte is held; the app commits next.
    Staged = 2,
    /// The swap is asked for: the loader swaps the image in at the next
    /// boot.
    Committed = 3,
}

/// Why the node refused a begin, a commit or a reboot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Not a header this node reads: its magic, its format, or a length
    /// the running slot does not hold.
    Header = 1,
    /// The image's version is not above the running image's.
    Version = 2,
    /// A swap is asked for already: reboot first.
    Busy = 3,
    /// Not every byte of the image is held.
    Incomplete = 4,
    /// The bytes held do not hash to the header's hash; they are
    /// forgotten.
    Hash = 5,
    /// On battery at or below the low level; the image stays staged, to
    /// commit on external power (spec "Implementation notes").
    Battery = 6,
    /// The flash failed; what was staged is forgotten.
    Flash = 7,
    /// No signature in the header verifies under a key the node holds.
    Signature = 8,
}

/// Update, read: where the update stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    /// Where the update stands.
    pub state: State,
    /// Why the last begin, commit or reboot was refused, until the next
    /// is taken.
    pub refused: Option<Refusal>,
    /// The image's bytes the node holds, in order from the first; the
    /// next piece starts here.
    pub held: u32,
    /// The header's version, 0 while idle.
    pub version: u32,
    /// The header's hash, zero while idle: by it the app knows the image
    /// held is the one it sends.
    pub hash: [u8; HASH_LEN],
}

impl Progress {
    /// Bytes in the value.
    pub const LEN: usize = 1 + 1 + 4 + 4 + HASH_LEN;

    /// Nothing staged, nothing refused.
    pub const IDLE: Progress = Progress {
        state: State::Idle,
        refused: None,
        held: 0,
        version: 0,
        hash: [0; HASH_LEN],
    };

    /// The value.
    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut b = [0u8; Self::LEN];
        b[0] = self.state as u8;
        b[1] = self.refused.map_or(0, |r| r as u8);
        b[2..6].copy_from_slice(&self.held.to_be_bytes());
        b[6..10].copy_from_slice(&self.version.to_be_bytes());
        b[10..].copy_from_slice(&self.hash);
        b
    }

    /// Read a value, ignoring what a later version appends.
    pub fn decode(b: &[u8]) -> Result<Progress, Malformed> {
        if b.len() < Self::LEN {
            return Err(Malformed::Length);
        }
        let state = match b[0] {
            0 => State::Idle,
            1 => State::Receiving,
            2 => State::Staged,
            3 => State::Committed,
            _ => return Err(Malformed::Value),
        };
        let refused = match b[1] {
            0 => None,
            1 => Some(Refusal::Header),
            2 => Some(Refusal::Version),
            3 => Some(Refusal::Busy),
            4 => Some(Refusal::Incomplete),
            5 => Some(Refusal::Hash),
            6 => Some(Refusal::Battery),
            7 => Some(Refusal::Flash),
            8 => Some(Refusal::Signature),
            _ => return Err(Malformed::Value),
        };
        let u32_at = |at: usize| u32::from_be_bytes(b[at..at + 4].try_into().expect("four bytes"));
        Ok(Progress {
            state,
            refused,
            held: u32_at(2),
            version: u32_at(6),
            hash: b[10..Self::LEN].try_into().expect("the hash's bytes"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vectors() {
        let header: [u8; HEADER_LEN] = core::array::from_fn(|i| i as u8);
        let mut b = [0u8; Command::MAX_LEN];
        assert_eq!(Command::Begin(&header).encode(&mut b), 257);
        assert_eq!(b[0], 0);
        assert_eq!(&b[1..], &header);
        assert_eq!(Command::decode(&b), Ok(Command::Begin(&header)));
        assert_eq!(Command::Commit.encode(&mut b), 1);
        assert_eq!(b[0], 1);
        assert_eq!(Command::Reboot.encode(&mut b), 1);
        assert_eq!(b[0], 2);
        assert_eq!(Command::decode(&[1]), Ok(Command::Commit));
        assert_eq!(Command::decode(&[2]), Ok(Command::Reboot));
        assert_eq!(Command::decode(&[0, 1, 2]), Err(Malformed::Length));
        assert_eq!(Command::decode(&[1, 0]), Err(Malformed::Length));
        assert_eq!(Command::decode(&[]), Err(Malformed::Length));
        assert_eq!(Command::decode(&[3]), Err(Malformed::Value));

        let mut b = [0u8; Piece::MAX_LEN];
        let bytes = [0xAB; 3];
        let p = Piece {
            offset: 0x0102_0304,
            bytes: &bytes,
        };
        assert_eq!(p.encode(&mut b), 7);
        assert_eq!(&b[..7], &[1, 2, 3, 4, 0xAB, 0xAB, 0xAB]);
        assert_eq!(Piece::decode(&b[..7]), Ok(p));
        assert_eq!(Piece::decode(&[0, 0, 0, 0]), Err(Malformed::Length));
        assert_eq!(Piece::decode(&[0; 245]), Err(Malformed::Length));
        assert_eq!(Piece::MAX_BYTES, 240);

        let progress = Progress {
            state: State::Receiving,
            refused: Some(Refusal::Battery),
            held: 0x0001_E000,
            version: 7,
            hash: core::array::from_fn(|i| i as u8),
        };
        let v = progress.encode();
        assert_eq!(&v[..10], &[1, 6, 0, 1, 0xE0, 0, 0, 0, 0, 7]);
        assert_eq!(v[10], 0);
        assert_eq!(v[41], 31);
        assert_eq!(Progress::decode(&v), Ok(progress));
        let mut longer = [0u8; Progress::LEN + 3];
        longer[..Progress::LEN].copy_from_slice(&v);
        assert_eq!(Progress::decode(&longer), Ok(progress));
        assert_eq!(Progress::decode(&v[..41]), Err(Malformed::Length));
        let mut bad = v;
        bad[0] = 4;
        assert_eq!(Progress::decode(&bad), Err(Malformed::Value));
        let mut bad = v;
        bad[1] = 9;
        assert_eq!(Progress::decode(&bad), Err(Malformed::Value));
        assert_eq!(
            Progress::decode(&Progress::IDLE.encode()),
            Ok(Progress::IDLE)
        );
    }
}
