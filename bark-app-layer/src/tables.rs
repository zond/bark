//! What a node keeps of its tables across a reboot into a new image (spec
//! "Updates over the mesh"): the entries its core gives
//! ([`bark_proto::Node::remembered`]), neighbours then message records, in
//! the core's encoding, behind a header.
//!
//! The header is written last and holds a check of the entries, so a write
//! a power cut stopped, or an erased region, keeps nothing.

use bark_proto::tables::neighbours::{KEPT_LEN as NEIGHBOUR_LEN, NEIGHBOURS};
use bark_proto::tables::records::{KEPT_LEN as RECORD_LEN, RECORDS};
use bark_proto::Kept;
use sha2::{Digest, Sha256};

/// Bytes of the header: a program page of the external flash
/// (`carry::SLOT_LEN`), so that it is one write.
pub const HEADER_LEN: usize = crate::carry::SLOT_LEN;

/// The most the tables take: the header, a full neighbour table and every
/// record. 25 248 bytes.
pub const MAX_LEN: usize = HEADER_LEN + NEIGHBOURS * NEIGHBOUR_LEN + RECORDS * RECORD_LEN;

const MAGIC: [u8; 4] = *b"tabl";

/// Bytes of the check: the first of the entries' SHA-256.
const CHECK_LEN: usize = 8;

/// What the header says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    /// Neighbour entries, first.
    pub neighbours: u16,
    /// Records, after them.
    pub records: u16,
    /// µs from when the core gave the tables to the header's write: time
    /// away the core is told of after the reboot.
    pub written_us: u32,
    check: [u8; CHECK_LEN],
}

impl Header {
    /// The header of the entries `check` was fed.
    pub fn new(neighbours: u16, records: u16, written_us: u32, check: Check) -> Header {
        let full = check.0.finalize();
        let mut c = [0; CHECK_LEN];
        c.copy_from_slice(&full[..CHECK_LEN]);
        Header {
            neighbours,
            records,
            written_us,
            check: c,
        }
    }

    /// Its bytes.
    pub fn encode(&self) -> [u8; HEADER_LEN] {
        let mut b = [0xFF; HEADER_LEN];
        b[0..4].copy_from_slice(&MAGIC);
        b[4..6].copy_from_slice(&self.neighbours.to_be_bytes());
        b[6..8].copy_from_slice(&self.records.to_be_bytes());
        b[8..12].copy_from_slice(&self.written_us.to_be_bytes());
        b[12..12 + CHECK_LEN].copy_from_slice(&self.check);
        b
    }

    /// The header in `b`: `None` for an erased or torn one, or counts
    /// beyond the tables'.
    pub fn decode(b: &[u8; HEADER_LEN]) -> Option<Header> {
        let h = Header {
            neighbours: u16::from_be_bytes([b[4], b[5]]),
            records: u16::from_be_bytes([b[6], b[7]]),
            written_us: u32::from_be_bytes([b[8], b[9], b[10], b[11]]),
            check: b[12..12 + CHECK_LEN].try_into().ok()?,
        };
        let fits = usize::from(h.neighbours) <= NEIGHBOURS && usize::from(h.records) <= RECORDS;
        (b[0..4] == MAGIC && fits).then_some(h)
    }

    /// The entries it counts.
    pub fn entries(&self) -> usize {
        usize::from(self.neighbours) + usize::from(self.records)
    }

    /// Where entry `i` lies, from the header's start, and its length.
    pub fn place(&self, i: usize) -> (usize, usize) {
        let n = usize::from(self.neighbours);
        match i < n {
            true => (HEADER_LEN + i * NEIGHBOUR_LEN, NEIGHBOUR_LEN),
            false => (
                HEADER_LEN + n * NEIGHBOUR_LEN + (i - n) * RECORD_LEN,
                RECORD_LEN,
            ),
        }
    }

    /// Entry `i`, from its bytes at [`Header::place`].
    pub fn entry(&self, i: usize, b: &[u8]) -> Option<Kept> {
        match i < usize::from(self.neighbours) {
            true => b.try_into().ok().map(Kept::Neighbour),
            false => b.try_into().ok().map(Kept::Record),
        }
    }

    /// Whether `check`, fed the entries read, matches.
    pub fn matches(&self, check: Check) -> bool {
        check.0.finalize()[..CHECK_LEN] == self.check
    }
}

/// An entry's bytes, as they are written.
pub fn bytes(k: &Kept) -> &[u8] {
    match k {
        Kept::Neighbour(b) => b,
        Kept::Record(b) => b,
    }
}

/// The check of the entries, fed each in turn.
#[derive(Clone, Default)]
pub struct Check(Sha256);

impl Check {
    /// Fed nothing yet.
    pub fn new() -> Check {
        Check(Sha256::new())
    }

    /// Feed one entry's bytes.
    pub fn add(&mut self, b: &[u8]) {
        self.0.update(b);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    fn written(entries: &[Kept], written_us: u32) -> Vec<u8> {
        let mut check = Check::new();
        let mut body = Vec::new();
        for k in entries {
            check.add(bytes(k));
            body.extend_from_slice(bytes(k));
        }
        let n = entries
            .iter()
            .filter(|k| matches!(k, Kept::Neighbour(_)))
            .count();
        let h = Header::new(n as u16, (entries.len() - n) as u16, written_us, check);
        let mut out = h.encode().to_vec();
        out.extend(body);
        out
    }

    fn read(b: &[u8]) -> Option<(Header, Vec<Kept>)> {
        let h = Header::decode(b[..HEADER_LEN].try_into().ok()?)?;
        let mut check = Check::new();
        let mut out = Vec::new();
        for i in 0..h.entries() {
            let (at, len) = h.place(i);
            let e = b.get(at..at + len)?;
            check.add(e);
            out.push(h.entry(i, e)?);
        }
        h.matches(check).then_some((h, out))
    }

    fn entries() -> Vec<Kept> {
        let mut v = Vec::new();
        for i in 0..3u8 {
            v.push(Kept::Neighbour([i; NEIGHBOUR_LEN]));
        }
        for i in 0..5u8 {
            v.push(Kept::Record([0x40 + i; RECORD_LEN]));
        }
        v
    }

    #[test]
    fn the_tables_read_back_as_written() {
        let e = entries();
        let (h, back) = read(&written(&e, 123_456)).expect("whole");
        assert_eq!((h.neighbours, h.records, h.written_us), (3, 5, 123_456));
        assert_eq!(back, e);
    }

    #[test]
    fn a_torn_write_or_an_erased_region_keeps_nothing() {
        let e = entries();
        let mut b = written(&e, 0);
        assert!(read(&[0xFF; MAX_LEN]).is_none(), "erased");
        let end = b.len();
        b[end - 7..].fill(0xFF);
        assert!(read(&b).is_none(), "the last entry cut short");
        let mut b = written(&e, 0);
        b[..HEADER_LEN].fill(0xFF);
        assert!(read(&b).is_none(), "no header");
    }

    #[test]
    fn the_most_it_takes() {
        assert_eq!(MAX_LEN, 256 + 64 * 57 + 667 * 32);
    }
}
