//! When the screen is drawn (`bark-fw/README.md`, Display): a change a
//! person acts on at once, about one refresh after it; any other change
//! no sooner than a gap after the last picture; and a picture at least
//! once a stale time, so that its uptime says the node runs. Times are
//! milliseconds since boot.

use crate::{role, Battery, State};

/// A full refresh as the board's label gives it, 2 s (`bark-fw`,
/// `board::Eink`). Changes a person acts on that come within one are
/// drawn together.
pub const REFRESH_MS: u64 = 2_000;

/// How often the screen may be drawn for a change it may wait on, and
/// how long it may go without a picture, on battery and on USB: the
/// display's cost against a sleepy node's battery (`bark-fw/README.md`,
/// Display).
pub const BATTERY_GAP_MS: u64 = 5 * 60_000;
pub const BATTERY_STALE_MS: u64 = 60 * 60_000;
pub const USB_GAP_MS: u64 = 60_000;
pub const USB_STALE_MS: u64 = 10 * 60_000;

/// Whether going from `shown` to `now` changes what a person acts on:
/// the header (what the node is, and whether it is starting), the
/// phone's line (a pairing code is typed while the phone waits, spec
/// "Phone link", Pairing), or the power (a node taken off USB runs on
/// its cell). The counts, the uptime and the battery's level wait.
pub fn urgent(shown: &State, now: &State) -> bool {
    role(shown) != role(now)
        || shown.starting != now.starting
        || shown.phone != now.phone
        || (shown.battery == Battery::Usb) != (now.battery == Battery::Usb)
}

/// What was last drawn, and when.
#[derive(Clone, Copy, Debug, Default)]
pub struct Schedule {
    /// The last picture's state, and when the schedule counts it from.
    shown: Option<(State, u64)>,
    /// Since when the state has differed from the picture in a way a
    /// person acts on.
    urgent_since: Option<u64>,
}

impl Schedule {
    pub const fn new() -> Schedule {
        Schedule {
            shown: None,
            urgent_since: None,
        }
    }

    /// When the screen is next to be drawn, the node's state being
    /// `state` at `now`; at or before `now` means at once.
    pub fn due(&mut self, state: &State, now: u64) -> u64 {
        let Some((shown, at)) = self.shown else {
            return now;
        };
        // Counted from the first such change, so that a stream of them
        // cannot hold the picture back; forgotten when the state goes
        // back to the picture's.
        self.urgent_since = match urgent(&shown, state) {
            true => Some(self.urgent_since.unwrap_or(now)),
            false => None,
        };
        let (gap, stale) = match state.battery {
            Battery::Usb => (USB_GAP_MS, USB_STALE_MS),
            _ => (BATTERY_GAP_MS, BATTERY_STALE_MS),
        };
        let scheduled = match shown == *state {
            true => at + stale,
            false => at + gap,
        };
        match self.urgent_since {
            Some(since) => scheduled.min(since + REFRESH_MS),
            None => scheduled,
        }
    }

    /// `state` was drawn, the refresh over at `at`: the gap and the stale
    /// time count from it.
    pub fn drawn(&mut self, state: State, at: u64) {
        self.shown = Some((state, at));
        self.urgent_since = None;
    }

    /// `state` was drawn for a press of the button, a bench aid that
    /// leaves the gap and the stale time as they were.
    pub fn pressed(&mut self, state: State, at: u64) {
        let at = self.shown.map_or(at, |(_, at)| at);
        self.drawn(state, at);
    }
}

#[cfg(test)]
mod tests;
