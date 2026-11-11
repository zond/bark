//! The card people exchange so that each can reach the other, its bytes
//! and its `BARK:` text (spec "The payload"). The base32 text (RFC 4648 §6)
//! stays in the QR code's alphanumeric set.

use crate::{be, Address, Error, Identity};

/// A card, its name borrowed from the bytes it was read from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Card<'a> {
    /// Whose card.
    pub identity: Identity,
    /// Their home node's address, whose mailbox keeps their mail.
    pub home: Option<Address>,
    /// The node their phone is linked to, or was last, their own or a
    /// public one, which keeps their mail by its recipient tag; none when
    /// it is the home.
    pub now: Option<Linked>,
    /// When the home was set or cleared, as `sent`: a later card replaces
    /// an earlier one, also one that drops the home.
    pub time: u32,
    /// The name they chose.
    pub name: &'a str,
}

/// A node a phone was linked to, and when.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Linked {
    /// The node's address.
    pub address: Address,
    /// When the phone was last linked to it, as `sent`.
    pub at: u32,
}

impl Linked {
    /// Its bytes: the address, then the time.
    pub const LEN: usize = Address::LEN + 4;
}

/// The card layout's version.
pub const CARD_VERSION: u8 = 3;
/// The longest name, in UTF-8 bytes.
pub const MAX_NAME: usize = 32;
/// Bytes before the addresses: the shortest card.
pub const FIXED_LEN: usize = 1 + 1 + Identity::LEN + 4;
/// The longest card, with the home and the node.
pub const MAX_LEN: usize = FIXED_LEN + Address::LEN + Linked::LEN + MAX_NAME;

/// What a card's text begins with.
pub const PREFIX: &str = "BARK:";
/// The longest text: the prefix and 5 bits a character.
pub const MAX_TEXT_LEN: usize = PREFIX.len() + (MAX_LEN * 8).div_ceil(5);

const FLAG_HOME: u8 = 1;
const FLAG_NOW: u8 = 2;

const BASE32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

impl<'a> Card<'a> {
    /// The card's bytes into `out`; their length.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, Error> {
        let name = self.name.as_bytes();
        let at = FIXED_LEN
            + if self.home.is_some() { Address::LEN } else { 0 }
            + if self.now.is_some() { Linked::LEN } else { 0 };
        let len = at + name.len();
        if name.len() > MAX_NAME || out.len() < len {
            return Err(Error::TooLong);
        }
        out[0] = CARD_VERSION;
        out[1] = if self.home.is_some() { FLAG_HOME } else { 0 }
            | if self.now.is_some() { FLAG_NOW } else { 0 };
        out[2..34].copy_from_slice(&self.identity.0);
        out[34..FIXED_LEN].copy_from_slice(&self.time.to_be_bytes());
        let mut i = FIXED_LEN;
        if let Some(h) = self.home {
            out[i..i + Address::LEN].copy_from_slice(&h.encode());
            i += Address::LEN;
        }
        if let Some(n) = self.now {
            out[i..i + Address::LEN].copy_from_slice(&n.address.encode());
            out[i + Address::LEN..i + Linked::LEN].copy_from_slice(&n.at.to_be_bytes());
        }
        out[at..len].copy_from_slice(name);
        Ok(len)
    }

    /// The card in `b`, all of it.
    pub fn decode(b: &'a [u8]) -> Result<Card<'a>, Error> {
        if b.first() != Some(&CARD_VERSION)
            || b.len() < FIXED_LEN
            || b[1] & !(FLAG_HOME | FLAG_NOW) != 0
        {
            return Err(Error::Malformed);
        }
        let mut at = FIXED_LEN;
        let home = if b[1] & FLAG_HOME != 0 {
            let a = Address::decode(b.get(at..).ok_or(Error::Malformed)?)?;
            at += Address::LEN;
            Some(a)
        } else {
            None
        };
        let now = if b[1] & FLAG_NOW != 0 {
            let l = b.get(at..at + Linked::LEN).ok_or(Error::Malformed)?;
            at += Linked::LEN;
            Some(Linked {
                address: Address::decode(l)?,
                at: be(&l[Address::LEN..]),
            })
        } else {
            None
        };
        if b.len() < at || b.len() > at + MAX_NAME {
            return Err(Error::Malformed);
        }
        let mut identity = [0u8; Identity::LEN];
        identity.copy_from_slice(&b[2..34]);
        Ok(Card {
            identity: Identity(identity),
            home,
            now,
            time: be(&b[34..FIXED_LEN]),
            name: core::str::from_utf8(&b[at..]).map_err(|_| Error::Malformed)?,
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
