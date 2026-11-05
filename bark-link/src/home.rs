//! Home: the characteristic by which the app makes its node a person's
//! home and reads its mailbox (spec "Mailbox"). The values are fixed
//! here; the firmware that serves it gives it `28bc0009-…` and link
//! version 3, as a new characteristic takes a new version (roadmap
//! D117), and not before, so that no node says version 3 without it.
//!
//! | | value | bytes |
//! |---|---|---|
//! | write | 0, no home; or 1 then the mailbox key | 1, 33 |
//! | read | 1 if a home, else 0; the oldest item's number (3); the newest's (3) | 7 |
//!
//! The numbers are 0 while the mailbox holds nothing.

use crate::Malformed;

/// Bytes of the mailbox key.
pub const KEY_LEN: usize = 32;

/// Home, written: the node is nobody's home, or this person's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetHome {
    /// No home: the mailbox forgets its key.
    Clear,
    /// Home, with this mailbox key.
    Set([u8; KEY_LEN]),
}

impl SetHome {
    const CLEAR: u8 = 0;
    const SET: u8 = 1;
    /// Bytes in the longest value.
    pub const MAX_LEN: usize = 1 + KEY_LEN;

    /// The value, into `b`; returns its length.
    pub fn encode(&self, b: &mut [u8; Self::MAX_LEN]) -> usize {
        match self {
            SetHome::Clear => {
                b[0] = Self::CLEAR;
                1
            }
            SetHome::Set(k) => {
                b[0] = Self::SET;
                b[1..].copy_from_slice(k);
                Self::MAX_LEN
            }
        }
    }

    /// Take a written value whole.
    pub fn decode(b: &[u8]) -> Result<SetHome, Malformed> {
        match (b.first(), b.len()) {
            (Some(&Self::CLEAR), 1) => Ok(SetHome::Clear),
            (Some(&Self::SET), Self::MAX_LEN) => {
                let mut k = [0u8; KEY_LEN];
                k.copy_from_slice(&b[1..]);
                Ok(SetHome::Set(k))
            }
            (Some(&Self::CLEAR | &Self::SET), _) | (None, _) => Err(Malformed::Length),
            (Some(_), _) => Err(Malformed::Value),
        }
    }
}

/// Home, read: whether the node is a home and which items it holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Home {
    /// The node holds a mailbox key.
    pub set: bool,
    /// The oldest item's number, 0 when none.
    pub oldest: u32,
    /// The newest item's number, 0 when none.
    pub newest: u32,
}

impl Home {
    /// Bytes in the value.
    pub const LEN: usize = 1 + 3 + 3;

    /// The value.
    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut b = [0u8; Self::LEN];
        b[0] = u8::from(self.set);
        b[1..4].copy_from_slice(&self.oldest.to_be_bytes()[1..]);
        b[4..].copy_from_slice(&self.newest.to_be_bytes()[1..]);
        b
    }

    /// The value the node wrote; bytes it appends later are ignored.
    pub fn decode(b: &[u8]) -> Result<Home, Malformed> {
        if b.len() < Self::LEN {
            return Err(Malformed::Length);
        }
        let n = |x: &[u8]| u32::from_be_bytes([0, x[0], x[1], x[2]]);
        Ok(Home {
            set: match b[0] {
                0 => false,
                1 => true,
                _ => return Err(Malformed::Value),
            },
            oldest: n(&b[1..4]),
            newest: n(&b[4..7]),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vectors() {
        let mut b = [0u8; SetHome::MAX_LEN];
        assert_eq!(SetHome::Clear.encode(&mut b), 1);
        assert_eq!(b[0], 0);
        let k = core::array::from_fn(|i| i as u8);
        assert_eq!(SetHome::Set(k).encode(&mut b), 33);
        assert_eq!(b[0], 1);
        assert_eq!(&b[1..], &k);
        assert_eq!(SetHome::decode(&b), Ok(SetHome::Set(k)));
        assert_eq!(SetHome::decode(&[0]), Ok(SetHome::Clear));
        assert_eq!(SetHome::decode(&[1, 2]), Err(Malformed::Length));
        assert_eq!(SetHome::decode(&[2]), Err(Malformed::Value));
        let h = Home {
            set: true,
            oldest: 17,
            newest: 0x01_0203,
        };
        assert_eq!(h.encode(), [1, 0, 0, 17, 1, 2, 3]);
        assert_eq!(Home::decode(&[1, 0, 0, 17, 1, 2, 3, 9]), Ok(h));
        assert_eq!(Home::decode(&[2, 0, 0, 0, 0, 0, 0]), Err(Malformed::Value));
        assert_eq!(Home::decode(&[1]), Err(Malformed::Length));
    }
}
