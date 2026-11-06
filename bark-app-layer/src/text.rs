//! A text's bytes: UTF-8, or the single-byte windows-1251 page when it
//! holds the whole text, one byte a Cyrillic letter instead of two
//! (`docs/app-layer-2026-11-05.md`, "The payload").

use crate::Error;

/// How a text record's bytes read; the record's type says which.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    /// UTF-8.
    Utf8,
    /// windows-1251.
    Windows1251,
}

/// windows-1251's bytes 0x80 to 0xFF as Unicode scalar values, 0 where
/// the page has none (0x98); below 0x80 it is ASCII. From the page's
/// published mapping.
const HIGH: [u16; 128] = [
    0x0402, 0x0403, 0x201A, 0x0453, 0x201E, 0x2026, 0x2020, 0x2021, 0x20AC, 0x2030, 0x0409, 0x2039,
    0x040A, 0x040C, 0x040B, 0x040F, 0x0452, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014,
    0, 0x2122, 0x0459, 0x203A, 0x045A, 0x045C, 0x045B, 0x045F, 0x00A0, 0x040E, 0x045E, 0x0408,
    0x00A4, 0x0490, 0x00A6, 0x00A7, 0x0401, 0x00A9, 0x0404, 0x00AB, 0x00AC, 0x00AD, 0x00AE, 0x0407,
    0x00B0, 0x00B1, 0x0406, 0x0456, 0x0491, 0x00B5, 0x00B6, 0x00B7, 0x0451, 0x2116, 0x0454, 0x00BB,
    0x0458, 0x0405, 0x0455, 0x0457, 0x0410, 0x0411, 0x0412, 0x0413, 0x0414, 0x0415, 0x0416, 0x0417,
    0x0418, 0x0419, 0x041A, 0x041B, 0x041C, 0x041D, 0x041E, 0x041F, 0x0420, 0x0421, 0x0422, 0x0423,
    0x0424, 0x0425, 0x0426, 0x0427, 0x0428, 0x0429, 0x042A, 0x042B, 0x042C, 0x042D, 0x042E, 0x042F,
    0x0430, 0x0431, 0x0432, 0x0433, 0x0434, 0x0435, 0x0436, 0x0437, 0x0438, 0x0439, 0x043A, 0x043B,
    0x043C, 0x043D, 0x043E, 0x043F, 0x0440, 0x0441, 0x0442, 0x0443, 0x0444, 0x0445, 0x0446, 0x0447,
    0x0448, 0x0449, 0x044A, 0x044B, 0x044C, 0x044D, 0x044E, 0x044F,
];

/// The windows-1251 byte of `c`, if the page has it.
fn byte_of(c: char) -> Option<u8> {
    if c.is_ascii() {
        return Some(c as u8);
    }
    HIGH.iter()
        .position(|&u| u != 0 && u32::from(u) == c as u32)
        .map(|i| 0x80 + i as u8)
}

/// The character of a windows-1251 byte, if the page has one.
fn char_of(b: u8) -> Option<char> {
    if b < 0x80 {
        return Some(b as char);
    }
    match HIGH[usize::from(b - 0x80)] {
        0 => None,
        u => char::from_u32(u32::from(u)),
    }
}

/// `text` into `out`: windows-1251 if the page holds every character,
/// else UTF-8; the encoding and the length.
pub fn encode(text: &str, out: &mut [u8]) -> Result<(Encoding, usize), Error> {
    if text.chars().all(|c| byte_of(c).is_some()) {
        let n = text.chars().count();
        let dst = out.get_mut(..n).ok_or(Error::TooLong)?;
        for (d, c) in dst.iter_mut().zip(text.chars()) {
            *d = byte_of(c).expect("checked above");
        }
        return Ok((Encoding::Windows1251, n));
    }
    let b = text.as_bytes();
    out.get_mut(..b.len())
        .ok_or(Error::TooLong)?
        .copy_from_slice(b);
    Ok((Encoding::Utf8, b.len()))
}

/// The characters of a text's `bytes`, or [`Error::Malformed`] if they
/// are not in `encoding`.
pub fn decode(encoding: Encoding, bytes: &[u8]) -> Result<Chars<'_>, Error> {
    match encoding {
        Encoding::Utf8 => core::str::from_utf8(bytes)
            .map(|s| Chars::Utf8(s.chars()))
            .map_err(|_| Error::Malformed),
        Encoding::Windows1251 => {
            if bytes.iter().all(|&b| char_of(b).is_some()) {
                Ok(Chars::Windows1251(bytes.iter()))
            } else {
                Err(Error::Malformed)
            }
        }
    }
}

/// A decoded text's characters.
pub enum Chars<'a> {
    /// From UTF-8.
    Utf8(core::str::Chars<'a>),
    /// From windows-1251.
    Windows1251(core::slice::Iter<'a, u8>),
}

impl Iterator for Chars<'_> {
    type Item = char;
    fn next(&mut self) -> Option<char> {
        match self {
            Chars::Utf8(c) => c.next(),
            Chars::Windows1251(b) => b.next().map(|&b| char_of(b).expect("checked in decode")),
        }
    }
}
