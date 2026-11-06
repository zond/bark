//! The time of day a node keeps for the people who read it: UTC from a
//! GPS fix, or from the phone while it has none, carried on by the node's
//! clock (spec "Position", on time). The protocol reads none of it.
//!
//! A time is kept while its error can be under a minute, the least a
//! clock that shows minutes needs: the second its source may lag by,
//! and the drift of the node's clock since, at the tolerance the board
//! gives that clock. Past that the node has no time, and the phone's is
//! taken again.

use bark_proto::{Duration, Instant};

/// Where the time came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// A GPS fix's RMC sentence.
    Gps,
    /// The phone's write (spec "Phone link", Clock).
    Phone,
}

/// The error a time may reach and still keep the minute.
const MINUTE: Duration = Duration::from_secs(60);

/// How late either source's time can be when the node stamps it: a GPS
/// sentence comes within the second it reports, the receiver sending one
/// group a second; the phone's write within a connection event of its
/// whole second, under a second at any interval a phone uses.
const STAMP: Duration = Duration::from_secs(1);

/// The UTC time kept, and its source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Utc {
    /// The clock's tolerance, parts per million.
    ppm: u32,
    /// UTC in µs since 1970 at the clock's instant, and its source.
    kept: Option<(u64, Instant, Source)>,
}

impl Utc {
    /// No time yet, on a clock good to `ppm`.
    pub const fn new(ppm: u32) -> Utc {
        Utc { ppm, kept: None }
    }

    /// How long a time keeps the minute on this clock.
    pub const fn keeps(&self) -> Duration {
        Duration::from_micros(
            (MINUTE.as_micros() - STAMP.as_micros()) * 1_000_000 / self.ppm as u64,
        )
    }

    /// A GPS fix said `utc_s` at `now`.
    pub fn gps(&mut self, utc_s: u64, now: Instant) {
        self.kept = Some((utc_s * 1_000_000, now, Source::Gps));
    }

    /// The phone said `utc_s` at `now`: taken unless a GPS time still
    /// keeps the minute. Returns whether it was taken.
    pub fn phone(&mut self, utc_s: u64, now: Instant) -> bool {
        if matches!(self.now(now), Some((_, Source::Gps))) {
            return false;
        }
        self.kept = Some((utc_s * 1_000_000, now, Source::Phone));
        true
    }

    /// UTC seconds since 1970 at `now`, and the source, while the time
    /// keeps the minute.
    pub fn now(&self, now: Instant) -> Option<(u64, Source)> {
        let (utc_us, at, source) = self.kept?;
        let since = now.since(at);
        (since <= self.keeps()).then(|| ((utc_us + since.as_micros()) / 1_000_000, source))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: u64) -> Instant {
        Instant::ZERO + Duration::from_secs(s)
    }

    #[test]
    fn a_time_keeps_the_minute_for_as_long_as_the_clock_allows() {
        // 59 s at 250 ppm: 236 000 s, 2.7 days.
        let mut u = Utc::new(250);
        assert_eq!(u.keeps(), Duration::from_secs(236_000));
        assert_eq!(u.now(at(5)), None);
        u.gps(1_000, at(10));
        assert_eq!(u.now(at(10)), Some((1_000, Source::Gps)));
        assert_eq!(u.now(at(70)), Some((1_060, Source::Gps)));
        assert_eq!(u.now(at(10 + 236_000)), Some((237_000, Source::Gps)));
        assert_eq!(u.now(at(11 + 236_000)), None);
    }

    #[test]
    fn the_phone_is_taken_only_without_a_gps_time() {
        let mut u = Utc::new(500);
        assert!(u.phone(2_000, at(0)));
        assert!(u.phone(2_005, at(4)), "a phone's time replaces a phone's");
        assert_eq!(u.now(at(5)), Some((2_006, Source::Phone)));
        u.gps(3_000, at(10));
        assert!(!u.phone(9_999, at(20)));
        assert_eq!(u.now(at(20)), Some((3_010, Source::Gps)));
        let past = 10 + u.keeps().as_micros() / 1_000_000 + 1;
        assert!(u.phone(5_000, at(past)), "the GPS time no longer keeps it");
        assert_eq!(u.now(at(past)), Some((5_000, Source::Phone)));
    }
}
