//! What a node says of itself when a neighbour asks
//! (spec "Neighbour status"). The frames are in `wire.rs`; asking and
//! answering in `node/relay/status.rs`.

use crate::node::NeighbourCounts;

/// The battery as a node's screen says it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Battery {
    /// On external power, USB: the reading would be the charger's.
    Usb,
    /// On battery, not read yet.
    #[default]
    NotRead,
    /// At or below the level at which a fixed relay turns sleepy, about
    /// 20 % ([`crate::BatteryLevels::DESIGN`]).
    Low,
    /// About this much of its charge left, in percent, above the low
    /// level ([`crate::node::battery_percent`]).
    Percent(u8),
}

/// What a node's screen shows of its state (spec "Neighbour status").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Status {
    /// It has a place (spec "Position").
    pub placed: bool,
    /// It is sleepy (spec "Nodes").
    pub sleepy: bool,
    /// Sleepy, with an anchor that holds its mail (spec "Relay choice").
    pub anchored: bool,
    /// It is carried (spec "Nodes").
    pub carried: bool,
    /// Its power and battery.
    pub battery: Battery,
    /// Its live neighbours: heard, heard with margin, proven.
    pub neighbours: NeighbourCounts,
    /// The mean C/N0, dB-Hz, of the satellites its GPS used in its latest
    /// reading, whether or not the fix was kept, the GPS running or at
    /// rest since; 0 when that reading used none, or there was none.
    pub gps_cn0: u8,
    /// How long ago that reading was.
    pub gps_age: ReadingAge,
    /// The version of the firmware image it runs, 0 if not known
    /// ([`crate::Config::version`]).
    pub version: u32,
    /// The image it gathers over the mesh, all zero when none.
    pub gathering: Gathering,
}

/// How far a node is in gathering an image (spec "Neighbour status"), as
/// its heartbeats' section says it (spec "Updates over the mesh").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Gathering {
    /// The image's version, 0 when it gathers none.
    pub version: u32,
    /// The image's blocks, the table's among them.
    pub blocks: u8,
    /// The blocks it holds from the first.
    pub complete: u8,
    /// The pieces it holds of the block after them.
    pub have: u8,
}

/// How long ago a node's GPS gave its latest reading, in one byte of its
/// status (spec "Neighbour status"), counted on the node's own clock from
/// when its driver handed the reading over; a restart forgets it.
///
/// A byte `c` below 255 is `e = c >> 4` and `m = c & 15`, an age of
/// `(16 + m) · 2^e − 16` minutes. Each `e` covers 16 codes from
/// `16 · 2^e − 16` minutes in steps of `2^e`, ending where the next
/// begins: 0 to 15 minutes exactly, then steps that double every 16
/// codes. An age is told as the longest code not older than it, so it
/// errs short by under `2^e`, at most a sixteenth of the age plus 16
/// minutes. Code 254, `e` 15 and `m` 14, is `30 · 2^15 − 16` = 983 024
/// minutes, 682.7 days, and older ages are told as it. 255 is no reading
/// since the node started.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReadingAge(pub u8);

impl ReadingAge {
    /// No reading since the node started.
    pub const NONE: ReadingAge = ReadingAge(u8::MAX);
    /// The oldest age a code tells.
    pub const MAX: ReadingAge = ReadingAge(u8::MAX - 1);

    /// The code for a reading `minutes` old.
    pub const fn from_minutes(minutes: u32) -> ReadingAge {
        let u = minutes as u64 + 16;
        let e = 63 - u.leading_zeros() - 4;
        let code = 16 * e as u64 + ((u >> e) - 16);
        if code > Self::MAX.0 as u64 {
            Self::MAX
        } else {
            ReadingAge(code as u8)
        }
    }

    /// The age the code tells, in minutes; `None` for no reading.
    pub const fn minutes(self) -> Option<u32> {
        if self.0 == Self::NONE.0 {
            return None;
        }
        let (e, m) = ((self.0 >> 4) as u32, (self.0 & 15) as u32);
        Some(((16 + m) << e) - 16)
    }
}

impl Default for ReadingAge {
    fn default() -> ReadingAge {
        Self::NONE
    }
}

#[cfg(test)]
mod tests {
    use super::ReadingAge;
    use proptest::prelude::*;

    #[test]
    fn the_codes_ends() {
        assert_eq!(ReadingAge::from_minutes(0), ReadingAge(0));
        assert_eq!(ReadingAge::from_minutes(15), ReadingAge(15));
        assert_eq!(ReadingAge::from_minutes(16).minutes(), Some(16));
        assert_eq!(ReadingAge::from_minutes(17).minutes(), Some(16));
        assert_eq!(ReadingAge::MAX.minutes(), Some(983_024));
        assert_eq!(ReadingAge::from_minutes(983_024), ReadingAge::MAX);
        assert_eq!(ReadingAge::from_minutes(u32::MAX), ReadingAge::MAX);
        assert_eq!(ReadingAge::NONE.minutes(), None);
        assert_eq!(ReadingAge::default(), ReadingAge::NONE);
    }

    #[test]
    fn every_code_tells_an_age_longer_than_the_one_before() {
        let mut last = None;
        for c in 0..=ReadingAge::MAX.0 {
            let m = ReadingAge(c).minutes().unwrap();
            assert!(last < Some(m), "code {c}");
            last = Some(m);
        }
    }

    proptest! {
        #[test]
        fn a_code_round_trips(c in 0..=ReadingAge::MAX.0) {
            let m = ReadingAge(c).minutes().unwrap();
            prop_assert_eq!(ReadingAge::from_minutes(m), ReadingAge(c));
        }

        #[test]
        fn an_age_is_told_by_the_longest_code_not_older(t in any::<u32>()) {
            let c = ReadingAge::from_minutes(t);
            let told = c.minutes().unwrap();
            prop_assert!(told <= t);
            if c < ReadingAge::MAX {
                prop_assert!(ReadingAge(c.0 + 1).minutes().unwrap() > t);
                prop_assert!((t - told) * 16 < t + 16, "{} as {}", t, told);
            }
        }

        #[test]
        fn an_older_age_is_never_told_younger(a in any::<u32>(), b in any::<u32>()) {
            let (a, b) = (a.min(b), a.max(b));
            prop_assert!(ReadingAge::from_minutes(a) <= ReadingAge::from_minutes(b));
        }
    }
}
