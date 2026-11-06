//! The card: what people exchange so that each can reach the other
//! (spec "Phone link"). Its address is the home node's when
//! the person has one, else the carried node's current one.
//!
//! | field | bytes |
//! |---|---|
//! | version | 1 |
//! | flags | 1: bit 0, the address is a home node's; the others zero |
//! | identity | 32 |
//! | address | 13 |
//! | the address's time, as `sent` | 4 |
//! | display name, UTF-8 | 0 to 32 |
//!
//! As text, in a QR code or pasted: `BARK:` and the bytes in base32
//! (RFC 4648 §6) without padding, all in the QR code's alphanumeric set.

use crate::{be, Address, Error, Identity};

/// A card, its name borrowed from the bytes it was read from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Card<'a> {
    /// Whose card.
    pub identity: Identity,
    /// Where to reach them.
    pub address: Address,
    /// The address is their home node's, whose mailbox keeps their mail
    /// for as long as its ring holds it; else it is where their node was.
    pub home: bool,
    /// When that address became theirs, as `sent`: a later card replaces
    /// an earlier one's address.
    pub time: u32,
    /// The name they chose.
    pub name: &'a str,
}

/// The card layout's version.
pub const CARD_VERSION: u8 = 2;
/// The longest name, in UTF-8 bytes.
pub const MAX_NAME: usize = 32;
/// Bytes before the name.
pub const FIXED_LEN: usize = 1 + 1 + Identity::LEN + Address::LEN + 4;
/// The longest card.
pub const MAX_LEN: usize = FIXED_LEN + MAX_NAME;

/// What a card's text begins with.
pub const PREFIX: &str = "BARK:";
/// The longest text: the prefix and 5 bits a character.
pub const MAX_TEXT_LEN: usize = PREFIX.len() + (MAX_LEN * 8).div_ceil(5);

const FLAG_HOME: u8 = 1;

const BASE32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

impl<'a> Card<'a> {
    /// The card's bytes into `out`; their length.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, Error> {
        let name = self.name.as_bytes();
        let len = FIXED_LEN + name.len();
        if name.len() > MAX_NAME || out.len() < len {
            return Err(Error::TooLong);
        }
        out[0] = CARD_VERSION;
        out[1] = if self.home { FLAG_HOME } else { 0 };
        out[2..34].copy_from_slice(&self.identity.0);
        out[34..34 + Address::LEN].copy_from_slice(&self.address.encode());
        out[47..51].copy_from_slice(&self.time.to_be_bytes());
        out[FIXED_LEN..len].copy_from_slice(name);
        Ok(len)
    }

    /// The card in `b`, all of it.
    pub fn decode(b: &'a [u8]) -> Result<Card<'a>, Error> {
        if b.len() < FIXED_LEN
            || b.len() > MAX_LEN
            || b[0] != CARD_VERSION
            || b[1] & !FLAG_HOME != 0
        {
            return Err(Error::Malformed);
        }
        let mut identity = [0u8; Identity::LEN];
        identity.copy_from_slice(&b[2..34]);
        Ok(Card {
            identity: Identity(identity),
            address: Address::decode(&b[34..])?,
            home: b[1] & FLAG_HOME != 0,
            time: be(&b[47..51]),
            name: core::str::from_utf8(&b[FIXED_LEN..]).map_err(|_| Error::Malformed)?,
        })
    }

    /// The card as text into `out`; its length.
    pub fn to_text(&self, out: &mut [u8]) -> Result<usize, Error> {
        let mut bytes = [0u8; MAX_LEN];
        let n = self.encode(&mut bytes)?;
        let len = PREFIX.len() + (n * 8).div_ceil(5);
        if out.len() < len {
            return Err(Error::TooLong);
        }
        out[..PREFIX.len()].copy_from_slice(PREFIX.as_bytes());
        for (k, c) in out[PREFIX.len()..len].iter_mut().enumerate() {
            let v = (0..5).fold(0usize, |v, j| {
                let bit = k * 5 + j;
                let set = bit < n * 8 && bytes[bit / 8] >> (7 - bit % 8) & 1 == 1;
                v << 1 | usize::from(set)
            });
            *c = BASE32[v];
        }
        Ok(len)
    }

    /// The card in `text`, its bytes kept in `buf`; letters in either
    /// case, spaces around ignored.
    pub fn parse(text: &str, buf: &'a mut [u8; MAX_LEN]) -> Result<Card<'a>, Error> {
        let t = text.trim();
        let body = t
            .get(..PREFIX.len())
            .filter(|p| p.eq_ignore_ascii_case(PREFIX))
            .map(|_| &t[PREFIX.len()..])
            .ok_or(Error::Malformed)?;
        let bits = body.len() * 5;
        let n = bits / 8;
        // Base32 without padding leaves under a character of bits over.
        if n > MAX_LEN || bits - n * 8 >= 5 {
            return Err(Error::Malformed);
        }
        *buf = [0u8; MAX_LEN];
        for (k, c) in body.bytes().enumerate() {
            let v = BASE32
                .iter()
                .position(|&x| x == c.to_ascii_uppercase())
                .ok_or(Error::Malformed)?;
            for j in 0..5 {
                let bit = k * 5 + j;
                if v >> (4 - j) & 1 == 1 {
                    if bit >= n * 8 {
                        return Err(Error::Malformed);
                    }
                    buf[bit / 8] |= 0x80 >> (bit % 8);
                }
            }
        }
        Card::decode(&buf[..n])
    }
}
