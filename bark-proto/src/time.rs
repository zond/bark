//! Local monotonic time in integer microseconds.
//!
//! The protocol has no network time (design §11); every node has its own
//! clock and nothing depends on two clocks agreeing. One integer unit
//! everywhere, so that a millisecond can never be added to a tick.

use core::ops::{Add, AddAssign, Sub};

/// A point on the local monotonic clock, in microseconds since an arbitrary
/// epoch chosen by the driver. Only differences are meaningful.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Instant(u64);

/// A span of time in microseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Duration(u64);

impl Instant {
    /// The epoch.
    pub const ZERO: Instant = Instant(0);

    /// Construct from microseconds since the epoch.
    pub const fn from_micros(us: u64) -> Self {
        Instant(us)
    }

    /// Microseconds since the epoch.
    pub const fn as_micros(self) -> u64 {
        self.0
    }

    /// `self - earlier`, saturating at zero if `earlier` is later.
    pub const fn since(self, earlier: Instant) -> Duration {
        Duration(self.0.saturating_sub(earlier.0))
    }
}

impl Duration {
    /// Zero.
    pub const ZERO: Duration = Duration(0);

    /// Construct from microseconds.
    pub const fn from_micros(us: u64) -> Self {
        Duration(us)
    }

    /// Construct from milliseconds.
    pub const fn from_millis(ms: u64) -> Self {
        Duration(ms * 1_000)
    }

    /// Construct from seconds.
    pub const fn from_secs(s: u64) -> Self {
        Duration(s * 1_000_000)
    }

    /// Microseconds.
    pub const fn as_micros(self) -> u64 {
        self.0
    }

    /// Whole milliseconds, rounded down.
    pub const fn as_millis(self) -> u64 {
        self.0 / 1_000
    }

    /// Multiply by an integer.
    pub const fn mul(self, n: u64) -> Duration {
        Duration(self.0 * n)
    }
}

impl Add<Duration> for Instant {
    type Output = Instant;
    fn add(self, d: Duration) -> Instant {
        Instant(self.0 + d.0)
    }
}

impl AddAssign<Duration> for Instant {
    fn add_assign(&mut self, d: Duration) {
        self.0 += d.0;
    }
}

impl Sub<Instant> for Instant {
    type Output = Duration;
    fn sub(self, earlier: Instant) -> Duration {
        self.since(earlier)
    }
}

impl Add for Duration {
    type Output = Duration;
    fn add(self, d: Duration) -> Duration {
        Duration(self.0 + d.0)
    }
}

impl Sub for Duration {
    type Output = Duration;
    fn sub(self, d: Duration) -> Duration {
        Duration(self.0.saturating_sub(d.0))
    }
}
