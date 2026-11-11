//! A log line that reaches the host whole or not at all.
//!
//! Each record is formatted into one [`Line`] and copied into the USB ring
//! only if all of it fits ([`put_whole`]); otherwise it is counted
//! ([`Drops`]) and the count logged later. A record copied piecemeal loses
//! its tail when the ring fills, and the next record joins the stump.

use core::fmt;
use core::sync::atomic::{AtomicU32, Ordering};

/// What a line cut short ends with, before the line end.
const CUT: &str = "...";
/// The line end a terminal and `readline` both expect.
const END: &str = "\r\n";

/// One record being formatted, at most `N` bytes with its line end.
pub struct Line<const N: usize> {
    buf: [u8; N],
    len: usize,
    cut: bool,
}

impl<const N: usize> Line<N> {
    /// Room for the mark and the line end, always kept.
    const ROOM: usize = N - CUT.len() - END.len();

    /// An empty line.
    pub const fn new() -> Self {
        Line {
            buf: [0; N],
            len: 0,
            cut: false,
        }
    }

    /// Empties it for the next record.
    pub fn clear(&mut self) {
        self.len = 0;
        self.cut = false;
    }

    /// The record with its line end, and `...` before it if it was cut.
    pub fn finish(&mut self) -> &[u8] {
        let tail = if self.cut { CUT } else { "" };
        for s in [tail, END] {
            self.buf[self.len..self.len + s.len()].copy_from_slice(s.as_bytes());
            self.len += s.len();
        }
        &self.buf[..self.len]
    }
}

impl<const N: usize> Default for Line<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> fmt::Write for Line<N> {
    /// Never fails: what does not fit is left out and marked, so a long
    /// record is still seen.
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if self.cut {
            return Ok(());
        }
        let room = Self::ROOM - self.len;
        let mut take = s.len().min(room);
        // Cut between characters, so the line stays UTF-8.
        while !s.is_char_boundary(take) {
            take -= 1;
        }
        self.buf[self.len..self.len + take].copy_from_slice(&s.as_bytes()[..take]);
        self.len += take;
        self.cut = take < s.len();
        Ok(())
    }
}

/// Copies `record` through `write` if `free` bytes hold all of it, in
/// pieces of at most `piece` bytes; `write` takes what it can and says how
/// much. False, with nothing written, if it does not fit.
///
/// Whole only if the caller is the ring's one writer: then its free room
/// only grows while the record is copied, and every piece goes in, however
/// the ring wraps.
pub fn put_whole(
    record: &[u8],
    free: usize,
    piece: usize,
    mut write: impl FnMut(&[u8]) -> usize,
) -> bool {
    if record.len() > free {
        return false;
    }
    let mut rest = record;
    while !rest.is_empty() {
        let n = write(&rest[..rest.len().min(piece)]);
        // Only a second writer can fill the room: stop rather than spin.
        if n == 0 {
            break;
        }
        rest = &rest[n..];
    }
    true
}

/// Records dropped since the count was last taken.
pub struct Drops(AtomicU32);

impl Drops {
    /// None dropped.
    pub const fn new() -> Self {
        Drops(AtomicU32::new(0))
    }

    /// One more record dropped.
    pub fn count(&self) {
        self.add(1);
    }

    /// `n` more records dropped: a count taken and then not reported.
    pub fn add(&self, n: u32) {
        self.0.fetch_add(n, Ordering::Relaxed);
    }

    /// The records dropped since the last take, and the count back to 0.
    pub fn take(&self) -> u32 {
        self.0.swap(0, Ordering::Relaxed)
    }
}

impl Default for Drops {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::fmt::Write as _;
    use std::vec::Vec;

    #[test]
    fn a_short_line_is_kept_with_its_end() {
        let mut line = Line::<32>::new();
        let what = "heard";
        write!(line, "[{:>3}] {}", 7, what).unwrap();
        assert_eq!(line.finish(), b"[  7] heard\r\n");
        line.clear();
        write!(line, "next").unwrap();
        assert_eq!(line.finish(), b"next\r\n");
    }

    #[test]
    fn a_line_that_just_fits_is_not_cut() {
        let mut line = Line::<12>::new();
        write!(line, "1234567").unwrap();
        assert_eq!(line.finish(), b"1234567\r\n");
    }

    #[test]
    fn a_long_line_is_cut_and_marked_within_its_size() {
        let mut line = Line::<12>::new();
        // Two pieces, as a format string with an argument gives them.
        let (a, b) = ("12345", "678901234");
        write!(line, "{}{}", a, b).unwrap();
        let out = line.finish();
        assert_eq!(out, b"1234567...\r\n");
        assert_eq!(out.len(), 12);
    }

    #[test]
    fn a_cut_falls_between_characters() {
        let mut line = Line::<12>::new();
        // Six bytes of ASCII, then a two-byte character across the cut.
        write!(line, "123456é").unwrap();
        let out = line.finish();
        assert_eq!(out, b"123456...\r\n");
        assert!(core::str::from_utf8(out).is_ok());
    }

    /// A ring of `cap` bytes that takes at most up to its end at a time,
    /// as embassy's pipe does.
    struct Ring {
        cap: usize,
        head: usize,
        bytes: Vec<u8>,
    }

    impl Ring {
        fn write(&mut self, b: &[u8]) -> usize {
            let n = b.len().min(self.cap - self.head);
            self.bytes.extend_from_slice(&b[..n]);
            self.head = (self.head + n) % self.cap;
            n
        }
    }

    #[test]
    fn a_record_that_fits_goes_in_whole_across_the_wrap() {
        let mut ring = Ring {
            cap: 16,
            head: 13,
            bytes: Vec::new(),
        };
        let record = b"0123456789";
        assert!(put_whole(record, 16, 4, |b| ring.write(b)));
        assert_eq!(ring.bytes, record);
    }

    #[test]
    fn a_record_that_does_not_fit_leaves_nothing() {
        let mut written = 0;
        assert!(!put_whole(b"0123456789", 9, 4, |b| {
            written += b.len();
            b.len()
        }));
        assert_eq!(written, 0);
    }

    #[test]
    fn drops_are_counted_and_taken_once() {
        let drops = Drops::new();
        assert_eq!(drops.take(), 0);
        drops.count();
        drops.count();
        drops.count();
        assert_eq!(drops.take(), 3);
        assert_eq!(drops.take(), 0);
        // A report of 3 that was itself dropped: 3 and the report.
        drops.add(3 + 1);
        assert_eq!(drops.take(), 4);
    }
}
