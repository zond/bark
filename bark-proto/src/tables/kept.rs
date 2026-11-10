//! Instants as the tables keep them across a reset
//! (spec "Updates over the mesh", the install; `Node::remembered`):
//! signed whole milliseconds from the moment the tables were taken, since
//! the clock after the reset counts from another epoch. A millisecond is
//! far below anything an instant times there: a heartbeat's window is
//! about 1.8 s (spec "Heartbeat").

use crate::time::{Duration, Instant};

/// Bytes of a kept instant.
pub const LEN: usize = 4;

/// The moment the tables were taken, on the clock after the reset, µs:
/// before that clock's epoch if the clock started less than that long
/// ago.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Then(i64);

impl Then {
    /// `ago` before `now`.
    pub fn before(now: Instant, ago: Duration) -> Then {
        Then(now.as_micros() as i64 - ago.as_micros() as i64)
    }
}

/// Which way an instant is rounded to the millisecond: so that nothing
/// kept is taken as fresher than it is, a moment something happened is
/// rounded earlier, and the end of a wait later.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Round {
    /// When something happened.
    Earlier,
    /// When a wait ends.
    Later,
}

/// `t`, taken at `now`, into `b`.
pub fn put(b: &mut [u8], t: Instant, now: Instant, round: Round) {
    let us = t.as_micros() as i128 - now.as_micros() as i128;
    let ms = match round {
        Round::Earlier => us.div_euclid(1000),
        Round::Later => -(-us).div_euclid(1000),
    };
    let ms = ms.clamp(i32::MIN as i128, i32::MAX as i128) as i32;
    b[..LEN].copy_from_slice(&ms.to_be_bytes());
}

/// The instant `b` keeps, taken at `then`; `None` if it is before the
/// clock's epoch.
pub fn get(b: &[u8], then: Then) -> Option<Instant> {
    let ms = i32::from_be_bytes([b[0], b[1], b[2], b[3]]);
    let us = then.0.checked_add(i64::from(ms) * 1000)?;
    u64::try_from(us).ok().map(Instant::from_micros)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding_never_makes_an_instant_fresher() {
        let now = Instant::from_micros(10_000_000);
        let mut b = [0; LEN];
        let t = Instant::from_micros(9_998_500);
        put(&mut b, t, now, Round::Earlier);
        let then = Then::before(now, Duration::ZERO);
        assert_eq!(get(&b, then), Some(Instant::from_micros(9_998_000)));
        let end = Instant::from_micros(10_001_500);
        put(&mut b, end, now, Round::Later);
        assert_eq!(get(&b, then), Some(Instant::from_micros(10_002_000)));
    }

    #[test]
    fn an_instant_shifts_by_the_time_away() {
        let now = Instant::from_micros(500_000_000);
        let mut b = [0; LEN];
        put(
            &mut b,
            Instant::from_micros(400_000_000),
            now,
            Round::Earlier,
        );
        // After a reset the clock reads 150 s; the tables were taken 50 s
        // before, so the instant 100 s before that is the epoch.
        let then = Then::before(Instant::from_micros(150_000_000), Duration::from_secs(50));
        assert_eq!(get(&b, then), Some(Instant::from_micros(0)));
        let then = Then::before(Instant::from_micros(150_000_000), Duration::from_secs(51));
        assert_eq!(get(&b, then), None, "before the epoch");
    }
}
