//! Command lines from what a host types on the serial port, one byte at a
//! time, for the console and the bench build.

/// A line that cannot be a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineError {
    /// Longer than the buffer; the rest up to its end was dropped.
    TooLong,
    NotUtf8,
}

/// One line of at most `N` bytes, its end a CR or an LF.
pub struct Line<const N: usize> {
    buf: [u8; N],
    len: usize,
    overflow: bool,
    complete: bool,
}

impl<const N: usize> Line<N> {
    pub const fn new() -> Line<N> {
        Line {
            buf: [0; N],
            len: 0,
            overflow: false,
            complete: false,
        }
    }

    /// Takes one byte; true if it ended a line, which [`Line::text`] gives
    /// until the next byte. An empty line ends nothing, so a CR LF is one
    /// end.
    pub fn push(&mut self, byte: u8) -> bool {
        if self.complete {
            self.len = 0;
            self.overflow = false;
            self.complete = false;
        }
        match byte {
            b'\r' | b'\n' if self.len == 0 && !self.overflow => false,
            b'\r' | b'\n' => {
                self.complete = true;
                true
            }
            b => {
                if self.len < N {
                    self.buf[self.len] = b;
                    self.len += 1;
                } else {
                    self.overflow = true;
                }
                false
            }
        }
    }

    /// The line just ended, without its end.
    pub fn text(&self) -> Result<&str, LineError> {
        if self.overflow {
            return Err(LineError::TooLong);
        }
        core::str::from_utf8(&self.buf[..self.len]).map_err(|_| LineError::NotUtf8)
    }
}

impl<const N: usize> Default for Line<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines<const N: usize>(bytes: &[u8]) -> Vec<Result<String, LineError>> {
        let mut line = Line::<N>::new();
        let mut out = Vec::new();
        for &b in bytes {
            if line.push(b) {
                out.push(line.text().map(String::from));
            }
        }
        out
    }

    #[test]
    fn a_line_ends_at_either_end_and_blank_lines_are_none() {
        assert_eq!(
            lines::<16>(b"\r\nab\r\n\ncd\re"),
            [Ok("ab".into()), Ok("cd".into())]
        );
    }

    #[test]
    fn a_line_too_long_is_refused_and_the_next_is_whole() {
        assert_eq!(
            lines::<3>(b"abcd\nxyz\n"),
            [Err(LineError::TooLong), Ok("xyz".into())]
        );
    }

    #[test]
    fn a_line_not_utf8_is_refused() {
        assert_eq!(lines::<8>(b"a\xff\n"), [Err(LineError::NotUtf8)]);
    }
}
