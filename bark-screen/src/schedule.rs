//! When the screen is drawn, and which picture (`bark-fw/README.md`,
//! Display; roadmap D160). The code is the default; a touch shows the
//! lines for [`LINES_MS`] or until the next touch, and a pairing or a
//! removal code shows them for as long as it does. A change of picture is
//! drawn at once, and so is a new code or a pairing refused for want of
//! room, since a person stands at the node for each; a change of what the
//! picture shows that a person acts on is drawn about one refresh after
//! it; nothing else is drawn.
//! Times are milliseconds since boot.

use crate::{role, Battery, Face, Phone, State};

/// A full refresh as the board's label gives it, 2 s (`bark-fw`,
/// `board::Eink`). Changes a person acts on that come within one are
/// drawn together.
pub const REFRESH_MS: u64 = 2_000;

/// How long a touch shows the lines: a choice, long enough to read ten
/// lines of two or three words, some 25 words, which at a reading pace of
/// about four words a second take under 10 s, three times over; short
/// enough that the next person to pass finds the code again.
pub const LINES_MS: u64 = 30_000;

/// Whether going from `shown` to `now` changes what a person acts on in
/// the lines: the header (what the node is, and whether it is starting),
/// the phone's line (a pairing code is typed while the phone waits, spec
/// "Phone link", Pairing), or the power (a node taken off USB runs on
/// its cell). The counts, the uptime and the battery's level wait for
/// the next touch.
pub fn urgent(shown: &State, now: &State) -> bool {
    role(shown) != role(now)
        || shown.starting != now.starting
        || shown.phone != now.phone
        || (shown.battery == Battery::Usb) != (now.battery == Battery::Usb)
}

/// What the code's line shows of the phone, if anything
/// (`crate::code_line`): all of the state the code's picture shows that
/// changes, each a thing a person at the node acts on at once.
fn on_code(state: &State) -> Option<Phone> {
    match state.phone {
        p @ (Phone::Code(_) | Phone::Remove(_) | Phone::Full) => Some(p),
        _ => None,
    }
}

/// The code a person at the node types into a phone, if one shows.
fn pairing(state: &State) -> Option<u32> {
    state.phone.code()
}

/// What was last drawn, and when the lines were asked for.
#[derive(Clone, Copy, Debug, Default)]
pub struct Schedule {
    /// The last picture and the state it was drawn from.
    shown: Option<(Face, State)>,
    /// Since when the state has differed from the picture in a way a
    /// person acts on.
    urgent_since: Option<u64>,
    /// When a touch last asked for the lines.
    touched: Option<u64>,
    /// A person flipped the role: the lines are drawn at once, whatever
    /// shows.
    flipped: bool,
}

impl Schedule {
    pub const fn new() -> Schedule {
        Schedule {
            shown: None,
            urgent_since: None,
            touched: None,
            flipped: false,
        }
    }

    /// The pad was touched at `now`, the node's state being `state`: the
    /// lines, or the code if the lines show.
    pub fn touched(&mut self, state: &State, now: u64) {
        self.touched = match self.face(state, now) {
            Face::Code => Some(now),
            Face::Lines => None,
        };
    }

    /// A person flipped the role at `now`: the lines, to show it.
    pub fn lines(&mut self, now: u64) {
        self.touched = Some(now);
        self.flipped = true;
    }

    /// The picture the screen is to show at `now`: the lines while a
    /// pairing or a removal code shows, since the person typing it stands
    /// at the node and may want the rest, else the code but for a while
    /// after a touch.
    pub fn face(&self, state: &State, now: u64) -> Face {
        if pairing(state).is_some() {
            return Face::Lines;
        }
        match self.touched {
            Some(t) if now < t + LINES_MS => Face::Lines,
            _ => Face::Code,
        }
    }

    /// When the screen is next to be drawn, the node's state being
    /// `state` at `now`: at or before `now` means at once, `None` not
    /// until something changes.
    pub fn due(&mut self, state: &State, now: u64) -> Option<u64> {
        let face = self.face(state, now);
        let Some((shown_face, shown)) = self.shown else {
            return Some(now);
        };
        // A person touched the pad, or one is to type a new code into the
        // waiting phone, or was just refused a pairing: none waits.
        if self.flipped
            || shown_face != face
            || (on_code(state).is_some() && state.phone != shown.phone)
        {
            return Some(now);
        }
        let differs = match face {
            Face::Code => on_code(&shown) != on_code(state),
            Face::Lines => urgent(&shown, state),
        };
        // Counted from the first such change, so that a stream of them
        // cannot hold the picture back; forgotten when the state goes
        // back to the picture's.
        self.urgent_since = match differs {
            true => Some(self.urgent_since.unwrap_or(now)),
            false => None,
        };
        let urgent = self.urgent_since.map(|since| since + REFRESH_MS);
        let back = match face {
            Face::Lines => self.touched.map(|t| t + LINES_MS).filter(|&t| t > now),
            Face::Code => None,
        };
        urgent.into_iter().chain(back).min()
    }

    /// `face` was drawn from `state`.
    pub fn drawn(&mut self, face: Face, state: State) {
        self.shown = Some((face, state));
        self.urgent_since = None;
        self.flipped = false;
    }
}

#[cfg(test)]
mod tests;
