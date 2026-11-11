//! What a node knows of its battery: its power source, its readings and
//! their calibration, the role a person forced, and how fast the cell
//! drains. The role the readings decide is in `relay/power.rs`.

use super::Forced;
use crate::drain::{Drain, Running};
use crate::event::{Action, Trace};
use crate::node::{
    BatteryLevels, BATTERY_CHARGED_AFTER, BATTERY_FULL_MV, BATTERY_GAIN_BOUND_PERMILLE,
    BATTERY_REST, BATTERY_SCALE_ONE,
};
use crate::store::Stored;
use crate::time::{Duration, Instant};

/// What a node's power says of its cell. Times are whole seconds of the
/// node's clock: half the room of an `Instant` in the node's 1 KiB outside
/// its tables (core-interface §6), and a u32 of seconds lasts 136 years.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Charge {
    /// On external power since then.
    Usb(u32),
    /// On battery.
    Battery,
    /// On battery after a full charge: its first reading from then is a
    /// full cell's, rested.
    Rested(u32),
}

impl Charge {
    fn secs(t: Instant) -> u32 {
        Self::secs_of(t.since(Instant::ZERO))
    }

    fn secs_of(d: Duration) -> u32 {
        (d.as_micros() / 1_000_000) as u32
    }
}

/// What the node knows of its battery and its power.
#[derive(Clone, Copy, Debug)]
pub(super) struct Battery {
    /// Read at or below the low level since it last read at or above the
    /// high one, or since its external power.
    low: bool,
    /// The scale of the driver's readings ([`BATTERY_SCALE_ONE`] is one);
    /// `None` before the first calibration.
    scale: Option<u16>,
    /// Since when it has been on external power as last reported, or
    /// when on battery its next reading is taken as a full cell's.
    charge: Charge,
    /// The driver's last reading on battery, uncalibrated; 0 before the
    /// first. Its status tells it (spec "Neighbour status").
    last_mv: u16,
    /// The role a person forced; the power's next change ends it.
    forced: Option<Forced>,
    drain: Drain,
}

impl Battery {
    /// At boot: on battery, not low, nothing learned.
    pub(super) const UNSET: Battery = Battery {
        low: false,
        scale: None,
        charge: Charge::Battery,
        last_mv: 0,
        forced: None,
        drain: Drain::NEW,
    };

    /// At boot, with the scale and the drains kept from before the reboot.
    pub(super) fn at_boot(stored: Option<Stored>) -> Battery {
        Battery {
            scale: stored.and_then(|s| s.battery_scale),
            drain: Drain::from_kept(stored.and_then(|s| s.drain)),
            ..Battery::UNSET
        }
    }

    /// The scale and the drains, to keep across a reboot (`store.rs`).
    pub(super) fn kept(&self) -> (Option<u16>, Option<[i8; 2]>) {
        (self.scale, self.drain.kept())
    }

    pub(super) fn on_usb(&self) -> bool {
        matches!(self.charge, Charge::Usb(_))
    }

    pub(super) fn low(&self) -> bool {
        self.low
    }

    pub(super) fn forced(&self) -> Option<Forced> {
        self.forced
    }

    pub(super) fn force(&mut self, role: Option<Forced>) {
        self.forced = role;
    }

    /// The driver reports whether external power is present. A change
    /// ends a force.
    pub(super) fn power(&mut self, external: bool, now: Instant) {
        let s = Charge::secs(now);
        if external != self.on_usb() {
            self.forced = None;
            // The charger holds the cell up, and it settles after.
            self.drain.charged(s);
        }
        self.charge = match (external, self.charge) {
            (true, Charge::Usb(since)) => Charge::Usb(since),
            (true, _) => {
                self.low = false;
                Charge::Usb(s)
            }
            // Long enough on the charger to be full: its first reading
            // after a rest calibrates.
            (false, Charge::Usb(since)) if s >= since + Charge::secs_of(BATTERY_CHARGED_AFTER) => {
                Charge::Rested(s + Charge::secs_of(BATTERY_REST))
            }
            (false, Charge::Usb(_)) => Charge::Battery,
            (false, c) => c,
        };
    }

    /// A battery reading. The first one a rest after a full charge
    /// calibrates; `levels` apply to the calibrated value; `running` says
    /// which drain the reading teaches.
    pub(super) fn read(
        &mut self,
        raw_mv: u16,
        levels: Option<BatteryLevels>,
        running: Option<Running>,
        now: Instant,
        emit: &mut dyn FnMut(Action),
    ) {
        if self.on_usb() || raw_mv == 0 {
            // On external power the reading is the charger's.
            return;
        }
        self.last_mv = raw_mv;
        let Some(levels) = levels else {
            return;
        };
        let s = Charge::secs(now);
        if matches!(self.charge, Charge::Rested(t) if s >= t) {
            self.charge = Charge::Battery;
            let scale = u32::from(BATTERY_FULL_MV) * BATTERY_SCALE_ONE / u32::from(raw_mv);
            let bound = BATTERY_SCALE_ONE * BATTERY_GAIN_BOUND_PERMILLE / 1000;
            if scale.abs_diff(BATTERY_SCALE_ONE) <= bound {
                self.scale = Some(scale as u16);
                // The levels before and after it are not comparable.
                self.drain.interrupt(s);
            }
        }
        let millivolts = self.millivolts(raw_mv);
        if millivolts <= levels.low_mv {
            self.low = true;
        } else if millivolts >= levels.high_mv {
            self.low = false;
        }
        let level = crate::node::battery_percent(millivolts);
        if let Some(step) = self.drain.read(level, running, s) {
            let e4 = |pph: f32| (pph * 10_000.0) as i32;
            emit(Action::Trace(Trace::Drain {
                sleepy: step.running == Running::Sleepy,
                measured: e4(step.measured),
                rate: e4(step.rate) as u32,
            }));
        }
    }

    /// How long the node can go on: as a relay (`relay`) until its battery
    /// is low, else as a sleepy node until the cell is flat. `None` on
    /// external power and before a reading.
    pub(super) fn left(&self, relay: bool, now: Instant) -> Option<Duration> {
        if self.on_usb() || self.last_mv == 0 {
            return None;
        }
        let level = crate::node::battery_percent(self.millivolts(self.last_mv));
        self.drain.left(level, relay, Charge::secs(now))
    }

    /// The drains learned, a relay's and a sleepy node's, in percent of the
    /// cell an hour.
    pub(super) fn drains(&self) -> [f32; 2] {
        [Running::Relay, Running::Sleepy].map(|r| self.drain.rate(r))
    }

    /// The battery as the node's screen says it: on USB, else its last
    /// reading's charge left (spec "Neighbour status").
    pub(super) fn status(&self) -> crate::status::Battery {
        use crate::status::Battery;
        if self.on_usb() {
            return Battery::Usb;
        }
        if self.last_mv == 0 {
            return Battery::NotRead;
        }
        match crate::node::battery_percent(self.millivolts(self.last_mv)) {
            Some(p) => Battery::Percent(p),
            None => Battery::Low,
        }
    }

    /// A driver's reading, calibrated once there is a calibration.
    pub(super) fn millivolts(&self, raw_mv: u16) -> u16 {
        match self.scale {
            Some(s) => (u32::from(raw_mv) * u32::from(s) / BATTERY_SCALE_ONE) as u16,
            None => raw_mv,
        }
    }
}
