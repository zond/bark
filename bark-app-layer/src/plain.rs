//! Inside a boxed message or an introduction: a header, then records
//! (spec "The payload").

use unicode_segmentation::UnicodeSegmentation;

use crate::card::{self, Card};
use crate::text::Encoding;
use crate::{be, Address, Error, Identity};

const FLAG_ADDRESS: u8 = 1;
const FLAG_GREATER: u8 = 2;

const TYPE_UTF8: u8 = 1;
const TYPE_1251: u8 = 2;
const TYPE_REACTION: u8 = 3;
const TYPE_CARD: u8 = 4;
const TYPE_UTF8_PART: u8 = 5;
const TYPE_1251_PART: u8 = 6;

/// The most bytes of a reaction's emoji: the longest in Unicode 17.0's
/// emoji list, a kiss of two people each with a skin tone, ten code
/// points joined. A family or a subdivision's flag is shorter.
pub const MAX_REACTION: usize = 35;

/// Whether `emoji` may be a reaction: one character as a person sees it,
/// an extended grapheme cluster, of at most [`MAX_REACTION`] bytes. That
/// it is an emoji and not a letter is the app's to offer.
pub fn is_reaction(emoji: &str) -> bool {
    emoji.len() <= MAX_REACTION && emoji.graphemes(true).count() == 1
}

/// The most parts a text is split into: a part's number and the count
/// share one byte, four bits each.
pub const MAX_PARTS: u8 = 16;

/// Where a text too long for one message stands among its parts. The
/// parts' `sent` are consecutive, so the first's is a part's `sent` less
/// its number and one, and names the text without a byte more.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Part {
    /// From 1.
    pub number: u8,
    /// From 2 to [`MAX_PARTS`].
    pub of: u8,
}

impl Part {
    fn encode(self) -> Result<u8, Error> {
        if self.of < 2 || self.of > MAX_PARTS || self.number < 1 || self.number > self.of {
            return Err(Error::Malformed);
        }
        Ok((self.number - 1) << 4 | (self.of - 1))
    }

    fn decode(b: u8) -> Result<Part, Error> {
        let p = Part {
            number: (b >> 4) + 1,
            of: (b & 0x0f) + 1,
        };
        p.encode().map(|_| p)
    }
}

/// What every boxed plaintext begins with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    /// The sender's identity is the greater of the two, byte by byte:
    /// a box's key serves both directions, and this drops a message sent
    /// back to its own writer.
    pub greater: bool,
    /// The sender's clock: orders, names a text, stops a replay.
    pub sent: u32,
    /// The address of the node the sender writes from, when the receiver
    /// may not have it; not the card's, which may be a home's mailbox, so
    /// that a reply can go straight to where the sender is.
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
    /// A text, or a part of one, in its encoding's bytes.
    Text {
        /// Its encoding.
        encoding: Encoding,
        /// Which part, for a text split over several messages.
        part: Option<Part>,
        /// Its bytes.
        bytes: &'a [u8],
    },
    /// The person reacts to the text they were sent at this `sent`.
    React {
        /// The `sent` of the text's first part.
        sent: u32,
        /// How: one emoji ([`is_reaction`]).
        emoji: &'a str,
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

    /// Adds `record`; a text or a reaction must be the last.
    pub fn push(&mut self, record: &Record) -> Result<(), Error> {
        if self.ended {
            return Err(Error::Malformed);
        }
        match record {
            Record::Text {
                encoding,
                part,
                bytes,
            } => {
                let kind = match (encoding, part) {
                    (Encoding::Utf8, None) => TYPE_UTF8,
                    (Encoding::Windows1251, None) => TYPE_1251,
                    (Encoding::Utf8, Some(_)) => TYPE_UTF8_PART,
                    (Encoding::Windows1251, Some(_)) => TYPE_1251_PART,
                };
                self.put(&[kind])?;
                if let Some(p) = part {
                    self.put(&[p.encode()?])?;
                }
                self.put(bytes)?;
                self.ended = true;
            }
            Record::React { sent, emoji } => {
                if !is_reaction(emoji) {
                    return Err(Error::Malformed);
                }
                self.put(&[TYPE_REACTION])?;
                self.put(&sent.to_be_bytes())?;
                self.put(emoji.as_bytes())?;
                self.ended = true;
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
            TYPE_UTF8 | TYPE_1251 | TYPE_UTF8_PART | TYPE_1251_PART => {
                let encoding = if kind == TYPE_UTF8 || kind == TYPE_UTF8_PART {
                    Encoding::Utf8
                } else {
                    Encoding::Windows1251
                };
                let text = if kind == TYPE_UTF8 || kind == TYPE_1251 {
                    Ok((None, body))
                } else {
                    match body.split_first() {
                        Some((&b, bytes)) => Part::decode(b).map(|p| (Some(p), bytes)),
                        None => Err(Error::Malformed),
                    }
                };
                (
                    text.map(|(part, bytes)| Record::Text {
                        encoding,
                        part,
                        bytes,
                    }),
                    &[][..],
                )
            }
            TYPE_REACTION if body.len() >= 4 => (
                match core::str::from_utf8(&body[4..]) {
                    Ok(emoji) if is_reaction(emoji) => Ok(Record::React {
                        sent: be(&body[..4]),
                        emoji,
                    }),
                    _ => Err(Error::Malformed),
                },
                &[][..],
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
