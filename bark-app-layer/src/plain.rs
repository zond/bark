//! Inside a boxed message or an introduction: a header, then records
//! (spec "Phone link").
//!
//! | field | bytes | |
//! |---|---|---|
//! | flags | 1 | bit 0: an address follows; bit 1: the sender's identity is the greater of the two |
//! | sent | 4 | the sender's clock, seconds since 2026-01-01 00:00 UTC |
//! | address | 0 or 13 | the address to answer to |
//!
//! | record | bytes | |
//! |---|---|---|
//! | text | 1 + rest | type 1 (UTF-8) or 2 (windows-1251); last |
//! | acknowledge | 1 + 4 | type 3, the `sent` of the text it answers |
//! | card | 1 + 1 + 50 to 82 | type 4, the card's length, its bytes |

use crate::card::{self, Card};
use crate::text::Encoding;
use crate::{be, Address, Error, Identity};

const FLAG_ADDRESS: u8 = 1;
const FLAG_GREATER: u8 = 2;

const TYPE_UTF8: u8 = 1;
const TYPE_1251: u8 = 2;
const TYPE_ACK: u8 = 3;
const TYPE_CARD: u8 = 4;

/// What every boxed plaintext begins with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    /// The sender's identity is the greater of the two, byte by byte:
    /// a box's key serves both directions, and this drops a message sent
    /// back to its own writer.
    pub greater: bool,
    /// The sender's clock: orders, names a text, stops a replay.
    pub sent: u32,
    /// The address to answer to, when the receiver may not have it.
    pub address: Option<Address>,
}

/// Bytes of the header without an address.
pub const HEADER_LEN: usize = 1 + 4;

/// Whether a message from `sender` to `receiver` sets the direction bit:
/// the sender's identity is the greater. A message to oneself does not.
pub fn greater(sender: &Identity, receiver: &Identity) -> bool {
    sender > receiver
}

/// A record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Record<'a> {
    /// A text, in its encoding's bytes.
    Text {
        /// Its encoding.
        encoding: Encoding,
        /// Its bytes.
        bytes: &'a [u8],
    },
    /// The person acknowledges the text they were sent at this `sent`.
    Acknowledge {
        /// The text's `sent`.
        sent: u32,
    },
    /// A card: the sender's own, or one passed on.
    Card(Card<'a>),
}

/// Writes a plaintext: the header, then records.
pub struct Writer<'a> {
    buf: &'a mut [u8],
    len: usize,
    ended: bool,
}

impl<'a> Writer<'a> {
    /// A plaintext in `buf` that begins with `header`.
    pub fn new(buf: &'a mut [u8], header: &Header) -> Result<Writer<'a>, Error> {
        let mut w = Writer {
            buf,
            len: 0,
            ended: false,
        };
        let flags = if header.address.is_some() {
            FLAG_ADDRESS
        } else {
            0
        } | if header.greater { FLAG_GREATER } else { 0 };
        w.put(&[flags])?;
        w.put(&header.sent.to_be_bytes())?;
        if let Some(a) = header.address {
            w.put(&a.encode())?;
        }
        Ok(w)
    }

    fn put(&mut self, b: &[u8]) -> Result<(), Error> {
        let end = self.len + b.len();
        self.buf
            .get_mut(self.len..end)
            .ok_or(Error::TooLong)?
            .copy_from_slice(b);
        self.len = end;
        Ok(())
    }

    /// Adds `record`; a text must be the last.
    pub fn push(&mut self, record: &Record) -> Result<(), Error> {
        if self.ended {
            return Err(Error::Malformed);
        }
        match record {
            Record::Text { encoding, bytes } => {
                self.put(&[match encoding {
                    Encoding::Utf8 => TYPE_UTF8,
                    Encoding::Windows1251 => TYPE_1251,
                }])?;
                self.put(bytes)?;
                self.ended = true;
            }
            Record::Acknowledge { sent } => {
                self.put(&[TYPE_ACK])?;
                self.put(&sent.to_be_bytes())?;
            }
            Record::Card(c) => {
                let mut b = [0u8; card::MAX_LEN];
                let n = c.encode(&mut b)?;
                self.put(&[TYPE_CARD, n as u8])?;
                self.put(&b[..n])?;
            }
        }
        Ok(())
    }

    /// The plaintext's length.
    pub fn finish(self) -> usize {
        self.len
    }
}

/// Reads a plaintext's header; the records follow in the iterator.
pub fn read(plain: &[u8]) -> Result<(Header, Records<'_>), Error> {
    let flags = *plain.first().ok_or(Error::Malformed)?;
    if flags & !(FLAG_ADDRESS | FLAG_GREATER) != 0 || plain.len() < HEADER_LEN {
        return Err(Error::Malformed);
    }
    let mut at = HEADER_LEN;
    let address = if flags & FLAG_ADDRESS != 0 {
        at += Address::LEN;
        Some(Address::decode(&plain[HEADER_LEN..])?)
    } else {
        None
    };
    Ok((
        Header {
            greater: flags & FLAG_GREATER != 0,
            sent: be(&plain[1..HEADER_LEN]),
            address,
        },
        Records { rest: &plain[at..] },
    ))
}

/// A plaintext's records, in order; one that does not read ends them
/// with an error.
pub struct Records<'a> {
    rest: &'a [u8],
}

impl<'a> Iterator for Records<'a> {
    type Item = Result<Record<'a>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        let (&kind, body) = self.rest.split_first()?;
        let (record, rest) = match kind {
            TYPE_UTF8 | TYPE_1251 => {
                let encoding = if kind == TYPE_UTF8 {
                    Encoding::Utf8
                } else {
                    Encoding::Windows1251
                };
                (
                    Ok(Record::Text {
                        encoding,
                        bytes: body,
                    }),
                    &[][..],
                )
            }
            TYPE_ACK if body.len() >= 4 => (
                Ok(Record::Acknowledge {
                    sent: be(&body[..4]),
                }),
                &body[4..],
            ),
            TYPE_CARD if !body.is_empty() && body.len() > usize::from(body[0]) => {
                let n = usize::from(body[0]);
                (
                    Card::decode(&body[1..1 + n]).map(Record::Card),
                    &body[1 + n..],
                )
            }
            _ => (Err(Error::Malformed), &[][..]),
        };
        self.rest = if record.is_ok() { rest } else { &[] };
        Some(record)
    }
}
