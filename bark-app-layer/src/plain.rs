//! Inside a boxed message or an introduction: a header, then records
//! (spec "Phone link").
//!
//! | field | bytes | |
//! |---|---|---|
//! | flags | 1 | bit 0: an address follows; bit 1: the sender's identity is the greater of the two |
//! | sent | 4 | the sender's clock, seconds since 2026-01-01 00:00 UTC |
//! | address | 0 or 13 | the address of the node the sender writes from |
//!
//! | record | bytes | |
//! |---|---|---|
//! | text | 1 + rest | type 1 (UTF-8) or 2 (windows-1251); last |
//! | text part | 1 + 1 + rest | type 5 (UTF-8) or 6 (windows-1251), the part's number and the parts' count less one in the high and low four bits; last |
//! | reaction | 1 + 4 | type 3 (thumbs up) or 7 to 11 (the others of [`Reaction`]), the `sent` of the text it answers |
//! | card | 1 + 1 + 38 to 100 | type 4, the card's length, its bytes |

use crate::card::{self, Card};
use crate::text::Encoding;
use crate::{be, Address, Error, Identity};

const FLAG_ADDRESS: u8 = 1;
const FLAG_GREATER: u8 = 2;

const TYPE_UTF8: u8 = 1;
const TYPE_1251: u8 = 2;
const TYPE_THUMBS_UP: u8 = 3;
const TYPE_CARD: u8 = 4;
const TYPE_UTF8_PART: u8 = 5;
const TYPE_1251_PART: u8 = 6;
const TYPE_HEART: u8 = 7;
const TYPE_LAUGH: u8 = 8;
const TYPE_SURPRISE: u8 = 9;
const TYPE_SAD: u8 = 10;
const TYPE_THANKS: u8 = 11;

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

/// A person's answer to a text, one of a fixed few. The thumbs up keeps
/// the type of the acknowledgement it replaced, so an older app reads it
/// as one, and reads every acknowledgement it gets as a thumbs up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reaction {
    /// 👍
    ThumbsUp,
    /// ❤️
    Heart,
    /// 😂
    Laugh,
    /// 😮
    Surprise,
    /// 😢
    Sad,
    /// 🙏
    Thanks,
}

impl Reaction {
    /// Every reaction, in the order the app offers them.
    pub const ALL: [Reaction; 6] = [
        Reaction::ThumbsUp,
        Reaction::Heart,
        Reaction::Laugh,
        Reaction::Surprise,
        Reaction::Sad,
        Reaction::Thanks,
    ];

    fn kind(self) -> u8 {
        match self {
            Reaction::ThumbsUp => TYPE_THUMBS_UP,
            Reaction::Heart => TYPE_HEART,
            Reaction::Laugh => TYPE_LAUGH,
            Reaction::Surprise => TYPE_SURPRISE,
            Reaction::Sad => TYPE_SAD,
            Reaction::Thanks => TYPE_THANKS,
        }
    }

    fn of_kind(kind: u8) -> Option<Reaction> {
        Reaction::ALL.into_iter().find(|r| r.kind() == kind)
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
        /// The `sent` of the text's first part, or of any of its parts.
        sent: u32,
        /// How.
        reaction: Reaction,
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
            Record::React { sent, reaction } => {
                self.put(&[reaction.kind()])?;
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
        let (record, rest) = match (kind, Reaction::of_kind(kind)) {
            (TYPE_UTF8 | TYPE_1251 | TYPE_UTF8_PART | TYPE_1251_PART, _) => {
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
            (_, Some(reaction)) if body.len() >= 4 => (
                Ok(Record::React {
                    sent: be(&body[..4]),
                    reaction,
                }),
                &body[4..],
            ),
            (TYPE_CARD, _) if !body.is_empty() && body.len() > usize::from(body[0]) => {
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
