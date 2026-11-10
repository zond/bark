//! How the firmware reads the touch pad (`bark-fw/src/button.rs`; roadmap
//! D175; `docs/touch-pad-2026-11-10.md`). The pad is a TTP223-BA6 touch
//! sensor whose output drives the pin both ways. Its wiring makes it
//! rest low and go high while touched, but the bench found boards whose
//! pin rested high, so the resting level is read, not assumed:
//!
//! - nothing is read until the sensor has calibrated after power-on
//!   ([`SETTLE_MS`]);
//! - then the pin is read with the chip's pull up and then down: a pin
//!   that follows the pull is driven by nothing, and the pad is taken as
//!   dead; else the level it holds is its resting level;
//! - a touch is a change from the resting level that lasts at least
//!   [`BOUNCE_MS`] and at most [`HOLD_MS`], counted when the pin has
//!   been back for [`BOUNCE_MS`];
//! - a change that lasts longer than [`HOLD_MS`] is not a person's
//!   touch: the level it holds becomes the resting level.
//!
//! Times are milliseconds since boot.

/// How long after power-on the sensor is not read: twice the "about
/// 0.5 sec stable-time" in which the sensor calibrates and does not
/// detect (TTP223-BA6 datasheet, Features), as "about" is not bounded
/// there. The clock starts after the bootloader, so this is at least
/// as long after the sensor's power-on.
pub const SETTLE_MS: u64 = 1_000;

/// How long a level must hold to count: two of the sensor's samples in
/// its fast mode, about 20 ms apart (TTP223-BA6 datasheet, Low power
/// mode figure), so that one sample's flicker is not a touch; well under
/// the 100 ms or so of a quick tap.
pub const BOUNCE_MS: u64 = 40;

/// The longest a touch lasts: a choice, over three times the longest
/// hold the node asks of a person, the user button's 3 s (D160). The
/// sensor itself has no such limit, its maximum on time being disabled
/// in this package (TTP223 datasheet, Maximum key on duration time and
/// the TTP223-BA6 option table), so a pad that reports a touch it cannot
/// end stays at that level.
pub const HOLD_MS: u64 = 10_000;

/// What reading the pad found, for the firmware to act on or log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// The pin is driven and rests at this level: `true` high.
    Rest(bool),
    /// The pin follows the pull: nothing drives it.
    Dead,
    /// A person touched the pad.
    Touch,
    /// The pin held a change longer than a touch: it rests at this
    /// level now.
    Held(bool),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Settling,
    Dead,
    Watching {
        rest: bool,
        /// The level last held for [`BOUNCE_MS`], and since when.
        stable: bool,
        stable_since: u64,
        /// The level last read, and since when.
        raw: bool,
        raw_since: u64,
    },
}

/// The pad's reading, fed with the pin's levels and their times.
#[derive(Clone, Copy, Debug)]
pub struct Pad {
    state: State,
}

impl Default for Pad {
    fn default() -> Self {
        Self::new()
    }
}

impl Pad {
    pub const fn new() -> Pad {
        Pad {
            state: State::Settling,
        }
    }

    /// When to read the pin next if it does not change before:
    /// [`SETTLE_MS`] to probe it, the end of a bounce or of the longest
    /// touch while a change runs, `None` while it rests or is dead.
    pub fn deadline(&self) -> Option<u64> {
        match self.state {
            State::Settling => Some(SETTLE_MS),
            State::Dead => None,
            State::Watching {
                rest,
                stable,
                stable_since,
                raw,
                raw_since,
            } => {
                if raw != stable {
                    Some(raw_since + BOUNCE_MS)
                } else if stable != rest {
                    Some(stable_since + HOLD_MS + 1)
                } else {
                    None
                }
            }
        }
    }

    /// The pin read at `now`, no earlier than [`SETTLE_MS`], with the
    /// chip's pull up (`with_up` the level read) and then down. A driven
    /// pin holds its level against the pull, which is far weaker than
    /// the sensor's output; one that reads high pulled up and low pulled
    /// down is driven by nothing.
    pub fn probe(&mut self, with_up: bool, with_down: bool, now: u64) -> Event {
        if with_up && !with_down {
            self.state = State::Dead;
            return Event::Dead;
        }
        // Low pulled up or high pulled down: the sensor drives it. Should
        // it have moved between the two reads, the later is the newer.
        self.state = State::Watching {
            rest: with_down,
            stable: with_down,
            stable_since: now,
            raw: with_down,
            raw_since: now,
        };
        Event::Rest(with_down)
    }

    /// The pin read `high` at `now`: at an edge, or at [`Pad::deadline`].
    pub fn read(&mut self, high: bool, now: u64) -> Option<Event> {
        let State::Watching {
            mut rest,
            mut stable,
            mut stable_since,
            mut raw,
            mut raw_since,
        } = self.state
        else {
            return None;
        };
        let mut event = None;
        if high != raw {
            raw = high;
            raw_since = now;
        }
        if raw != stable && now >= raw_since + BOUNCE_MS {
            // The new level held from `raw_since`; the old one ended there.
            let held = raw_since - stable_since;
            if stable != rest {
                if held > HOLD_MS {
                    rest = stable;
                    event = Some(Event::Held(rest));
                } else {
                    event = Some(Event::Touch);
                }
            }
            stable = raw;
            stable_since = raw_since;
        }
        if stable != rest && now > stable_since + HOLD_MS {
            rest = stable;
            event = Some(Event::Held(rest));
        }
        self.state = State::Watching {
            rest,
            stable,
            stable_since,
            raw,
            raw_since,
        };
        event
    }
}

#[cfg(test)]
mod tests;
