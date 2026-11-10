//! How long a node's battery lasts (spec "Sleepy nodes", roadmap D169):
//! a running estimate of the drain, percent of the cell an hour, for each
//! way the node runs on battery, seeded with what the currents say and
//! corrected by what the readings show.
//!
//! The board has no fuel gauge. The level is read off the cell's voltage
//! with the core's coarse mapping ([`battery_percent`]), in whole percent,
//! a straight line between three points of a typical cell's curve; and a
//! lithium cell's voltage is flat over most of its charge. So a drop seen
//! over minutes is noise: a sample runs until even a sleepy node has
//! drained a step of the mapping ([`SAMPLE`]), and the estimate averages
//! samples over days ([`TIME_CONSTANT`]).
//!
//! What it learns is in the mapping's percent, not the cell's: where the
//! true curve is flatter than the straight line, between 30 and 50 %, a
//! drop of one percent of charge reads as about 0.4 of the mapping's, and
//! near full as about 1.6. The mapping is exact at 20, 30 and 100 %, so
//! over a discharge the errors cancel, and the time constant spans that.
//!
//! [`battery_percent`]: crate::node::battery_percent

use crate::gps::GPS_STANDBY_UA;
use crate::node::BATTERY_LOW_PERCENT;
use crate::time::Duration;

/// The cell, mAh (LILYGO T-Echo README).
pub const CELL_MAH: u32 = 850;

/// The battery divider's constant drain, µA (`bark-fw/src/board.rs`).
const DIVIDER_UA: u32 = 12;

/// A relay's radio on battery, µA: listening at 5.8 mA and transmitting
/// at 32 mA for its airtime, 6.38 mA on average over the relays of S4's
/// settled hour (`docs/battery-role-2026-10-25.md`, Currents).
const RELAY_RADIO_UA: u32 = 6_380;

/// A sleepy node's radio with an anchor at the design rate, µA: 0.56 mA
/// in the simulator (spec "Delivery", roadmap D92).
const SLEEPY_RADIO_UA: u32 = 560;

/// What a relay on battery draws with its GPS at rest, µA: its radio, the
/// divider and the GPS in standby. Its GPS tracks on battery only for the
/// grace and after a far fix (spec "Position"), and its searches add at
/// most 0.2 mA, within the spread of relays' loads.
pub const RELAY_UA: u32 = RELAY_RADIO_UA + DIVIDER_UA + GPS_STANDBY_UA as u32;

/// What a sleepy node draws, µA: its radio, the divider and the GPS in
/// standby, as a sleepy node's GPS is with an anchor (spec "Beats").
pub const SLEEPY_UA: u32 = SLEEPY_RADIO_UA + DIVIDER_UA + GPS_STANDBY_UA as u32;

/// Percent of the cell an hour at `ua` µA: 100 % / (850 mAh / the current).
pub const fn percent_per_hour(ua: u32) -> f32 {
    ua as f32 * 100.0 / (CELL_MAH as f32 * 1000.0)
}

/// How a node runs on battery, each with a drain of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Running {
    /// A relay with its GPS at rest.
    Relay = 0,
    /// A sleepy node.
    Sleepy = 1,
}

impl Running {
    /// The drain the currents say, percent an hour: 0.7544 for a relay,
    /// 0.0696 for a sleepy node. The microcontroller is not counted
    /// (roadmap D92); the readings correct for it.
    pub const fn seed(self) -> f32 {
        match self {
            Running::Relay => percent_per_hour(RELAY_UA),
            Running::Sleepy => percent_per_hour(SLEEPY_UA),
        }
    }
}

/// The smallest drop the mapping shows, percent.
const STEP_PERCENT: f32 = 1.0;

/// How long a sample runs at least: until a sleepy node, the slowest
/// drain, has drained one step of the mapping, 1 % / 0.0696 % an hour =
/// 14.4 h, in whole hours. A shorter sample of a sleepy node would most
/// often see no step at all. A step is about 5.7 mV of the cell between
/// 30 % and full, three of the converter's 1.76 mV steps
/// (`bark-fw/src/battery.rs`), so a finer mapping would show noise.
pub const SAMPLE: Duration = {
    let hours = STEP_PERCENT / Running::Sleepy.seed();
    Duration::from_secs((hours as u64 + 1) * 3600)
};

const _: () = assert!(SAMPLE.as_micros() == 15 * 3600 * 1_000_000);

/// The estimate's time constant: how long a relay at its seed takes to
/// drain the mapping's straight segment from full to 30 %, 70 % / 0.7544 %
/// an hour = 92.8 h, 3.9 days. The segment's slope is off the curve's by
/// 0.4 to 1.6 times along it, so a shorter one would follow those bends;
/// over it, a sleepy node's samples, one step each give or take one, are
/// averaged over about twelve, to about 12 %.
pub const TIME_CONSTANT: Duration = {
    let segment = 100.0 - 30.0;
    Duration::from_secs((segment / Running::Relay.seed() * 3600.0) as u64)
};

/// A learned drain is kept across a reboot as its seed times
/// 2^(k / [`KEPT_STEPS`]): a step of 2.2 %, finer than a sample can tell,
/// and up to 16 times the seed either way in a byte.
const KEPT_STEPS: i32 = 32;

/// The same in memory, finer, so that a small move of the average is
/// not lost to the rounding: a step of 0.017 %.
const STEPS: i32 = KEPT_STEPS * 128;

/// The most steps from the seed either way, what a byte keeps.
const BOUND: i32 = 127 * (STEPS / KEPT_STEPS);

/// No drain learned.
const UNLEARNED: i16 = i16::MIN;

/// A sample closed: the drain it measured and the estimate it left,
/// percent an hour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Step {
    /// How the node ran.
    pub running: Running,
    /// The sample's drain.
    pub measured: f32,
    /// The estimate after it.
    pub rate: f32,
}

/// Between samples, or one under way: since when, at what level,
/// running how.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// No sample starts before this.
    Quiet { until_s: u32 },
    Sampling {
        from_s: u32,
        level: u8,
        running: Running,
    },
}

/// The estimate (module documentation). Times are whole seconds of the
/// node's clock, as in `relay/power.rs`, and its size counts beside the
/// node's tables (`docs/core-interface.md` §6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Drain {
    /// Each [`Running`]'s learned drain, as steps from its seed, 2^(k /
    /// [`STEPS`]) times it; [`UNLEARNED`] while it is the seed.
    steps: [i16; 2],
    phase: Phase,
    /// Since when the readings have been at or below the low level, where
    /// the mapping shows no level; 0 for none.
    low_since_s: u32,
}

impl Drain {
    /// Nothing learned.
    pub const NEW: Drain = Drain {
        steps: [UNLEARNED; 2],
        phase: Phase::Quiet { until_s: 0 },
        low_since_s: 0,
    };

    /// What was kept before a reboot ([`Drain::kept`]).
    pub fn from_kept(kept: Option<[i8; 2]>) -> Drain {
        let mut d = Drain::NEW;
        if let Some(k) = kept {
            d.steps = k.map(|k| (i32::from(k) * (STEPS / KEPT_STEPS)) as i16);
        }
        d
    }

    /// What to keep across a reboot: each drain as steps from its seed,
    /// once one has been learned.
    pub fn kept(&self) -> Option<[i8; 2]> {
        if self.steps == [UNLEARNED; 2] {
            return None;
        }
        let per = STEPS / KEPT_STEPS;
        Some(self.steps.map(|k| match k {
            UNLEARNED => 0,
            k => (i32::from(k) + per / 2).div_euclid(per) as i8,
        }))
    }

    /// The drain of `running`, percent an hour.
    pub fn rate(&self, running: Running) -> f32 {
        match self.steps[running as usize] {
            UNLEARNED => running.seed(),
            k => running.seed() * libm::exp2f(f32::from(k) / STEPS as f32),
        }
    }

    /// The readings say nothing for a while: the load changed, the cell
    /// was charging, or the scale changed. The cell's voltage settles an
    /// hour after a change of load, as AN4189's readings rest.
    pub fn interrupt(&mut self, now_s: u32) {
        let rest_s = crate::node::BATTERY_REST.as_micros() / 1_000_000;
        self.phase = Phase::Quiet {
            until_s: now_s.saturating_add(rest_s as u32),
        };
    }

    /// On external power: the cell charges.
    pub fn charged(&mut self, now_s: u32) {
        self.interrupt(now_s);
        self.low_since_s = 0;
    }

    /// A reading of `level` percent, `None` at or below the low level,
    /// with the node running as `running`, `None` for a way not learned.
    /// A sample that closes moves its drain's estimate.
    pub fn read(
        &mut self,
        level: Option<u8>,
        running: Option<Running>,
        now_s: u32,
    ) -> Option<Step> {
        if level.is_none() && self.low_since_s == 0 {
            self.low_since_s = now_s.max(1);
        }
        let (Some(level), Some(running)) = (level, running) else {
            self.interrupt(now_s);
            return None;
        };
        let restart = Phase::Sampling {
            from_s: now_s,
            level,
            running,
        };
        match self.phase {
            Phase::Quiet { until_s } => {
                if now_s >= until_s {
                    self.phase = restart;
                }
                None
            }
            Phase::Sampling { running: was, .. } if was != running => {
                self.interrupt(now_s);
                None
            }
            Phase::Sampling {
                from_s,
                level: from,
                ..
            } => {
                let elapsed = now_s.saturating_sub(from_s);
                if u64::from(elapsed) * 1_000_000 < SAMPLE.as_micros() {
                    return None;
                }
                let measured = (f32::from(from) - f32::from(level)) * 3600.0 / elapsed as f32;
                let rate = self.learn(running, measured, elapsed);
                self.phase = restart;
                Some(Step {
                    running,
                    measured,
                    rate,
                })
            }
        }
    }

    /// Move the drain of `running` toward `measured`, a sample of
    /// `elapsed_s` seconds, as a moving average of [`TIME_CONSTANT`].
    fn learn(&mut self, running: Running, measured: f32, elapsed_s: u32) -> f32 {
        let tau_s = (TIME_CONSTANT.as_micros() / 1_000_000) as f32;
        let weight = 1.0 - libm::expf(-(elapsed_s as f32) / tau_s);
        let r = self.rate(running);
        let seed = running.seed();
        // A sample may measure no drop, or a rise within the readings'
        // noise; the average stays above the least a byte keeps.
        let rate =
            (r + weight * (measured - r)).max(seed * libm::exp2f(-(BOUND as f32) / STEPS as f32));
        let k = libm::roundf(STEPS as f32 * libm::log2f(rate / seed)) as i32;
        self.steps[running as usize] = k.clamp(-BOUND, BOUND) as i16;
        self.rate(running)
    }

    /// How long the cell lasts from a reading of `level` percent, `None`
    /// at or below the low level, under the node's rules: a relay relays
    /// to the low level and is sleepy from there, a sleepy node is sleepy
    /// to the end (spec "Sleepy nodes"). `None` once that time is past.
    pub fn left(&self, level: Option<u8>, relay: bool, now_s: u32) -> Option<Duration> {
        let low = f32::from(BATTERY_LOW_PERCENT);
        let sleepy = self.rate(Running::Sleepy);
        let hours = match level {
            Some(l) if relay => {
                (f32::from(l) - low).max(0.0) / self.rate(Running::Relay) + low / sleepy
            }
            Some(l) => f32::from(l) / sleepy,
            None => {
                let since = match self.low_since_s {
                    0 => 0,
                    t => now_s.saturating_sub(t),
                };
                low / sleepy - since as f32 / 3600.0
            }
        };
        (hours > 0.0).then(|| Duration::from_secs((hours * 3600.0) as u64))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::battery_percent;

    #[test]
    fn the_derived_numbers() {
        assert!((Running::Relay.seed() - 0.7544).abs() < 1e-4);
        assert!((Running::Sleepy.seed() - 0.06965).abs() < 1e-4);
        assert_eq!(SAMPLE, Duration::from_secs(15 * 3600));
        let tau_h = TIME_CONSTANT.as_micros() as f32 / 3.6e9;
        assert!((tau_h - 92.8).abs() < 0.1, "{tau_h}");
    }

    /// A cell drained at `true_rate` percent an hour from full, read once
    /// a minute through the mapping's whole percent, as the core reads
    /// it, and charged for three hours whenever it reaches the low level;
    /// the estimate after `at` hours and after `hours`.
    fn discharge(running: Running, true_rate: f32, at: u32, hours: u32) -> (f32, f32) {
        let mut d = Drain::NEW;
        let mut rate = d.rate(running);
        let mut rate_at = rate;
        let mut from_s = 0;
        let mut t = 0;
        while t < hours * 3600 {
            let level = 100.0 - true_rate * t.saturating_sub(from_s) as f32 / 3600.0;
            let level = libm::roundf(level) as u8;
            if level <= BATTERY_LOW_PERCENT {
                d.charged(t);
                from_s = t + 3 * 3600;
                t = from_s;
                continue;
            }
            if let Some(step) = d.read(Some(level), Some(running), t) {
                rate = step.rate;
            }
            if t <= at * 3600 {
                rate_at = rate;
            }
            t += 60;
        }
        (rate_at, rate)
    }

    /// A board that draws other than the seed says: the estimate starts at
    /// the seed and closes on the true drain as a moving average of its
    /// time constant, about 63 % of the way within one, all but 5 %
    /// within three.
    #[test]
    fn the_estimate_converges_from_the_seed() {
        let tau_h = (TIME_CONSTANT.as_micros() / 3_600_000_000) as u32;
        let seed = Running::Relay.seed();
        for truth in [seed * 0.8, seed * 1.25] {
            let (after_one, after_three) = discharge(Running::Relay, truth, tau_h, 3 * tau_h);
            let closed = (after_one - seed) / (truth - seed);
            assert!(
                (0.4..=0.8).contains(&closed),
                "{closed} of the way after {tau_h} h"
            );
            assert!(
                ((after_three - truth) / truth).abs() < 0.03,
                "{after_three} for {truth}"
            );
        }
        // A sleepy node, a step or two a sample, the same, if noisier.
        let seed = Running::Sleepy.seed();
        for truth in [seed * 0.6, seed * 1.5] {
            let (_, rate) = discharge(Running::Sleepy, truth, 0, 3 * tau_h);
            assert!(((rate - truth) / truth).abs() < 0.1, "{rate} for {truth}");
        }
    }

    /// No sample closes before its length, and a change of how the node
    /// runs starts afresh after a quiet hour.
    #[test]
    fn a_sample_is_long_and_of_one_way() {
        let mut d = Drain::NEW;
        let h = |n: u32| n * 3600;
        assert_eq!(d.read(Some(90), Some(Running::Relay), 0), None);
        assert_eq!(d.read(Some(80), Some(Running::Relay), h(14)), None);
        assert_eq!(d.read(Some(70), Some(Running::Sleepy), h(15)), None);
        assert_eq!(d.read(Some(70), Some(Running::Sleepy), h(15) + 3599), None);
        assert_eq!(
            d.read(Some(70), Some(Running::Sleepy), h(16)),
            None,
            "starts"
        );
        let step = d.read(Some(69), Some(Running::Sleepy), h(31)).unwrap();
        assert!((step.measured - 1.0 / 15.0).abs() < 1e-6);
        assert_eq!(d.rate(Running::Relay), Running::Relay.seed());
    }

    /// The time left adds the relay's segment to the low level and the
    /// sleepy node's from there; below it, what is left of that.
    #[test]
    fn the_time_left_sums_the_segments() {
        let d = Drain::NEW;
        let (relay, sleepy) = (Running::Relay.seed(), Running::Sleepy.seed());
        let hours = |t: Option<Duration>| t.unwrap().as_micros() as f32 / 3.6e9;
        let full = hours(d.left(Some(100), true, 0));
        assert!(
            (full - (80.0 / relay + 20.0 / sleepy)).abs() < 0.01,
            "{full}"
        );
        // 106 h relaying and 287 h asleep: 16 days.
        assert!((full / 24.0 - 16.4).abs() < 0.1, "{full}");
        let asleep = hours(d.left(Some(100), false, 0));
        assert!((asleep - 100.0 / sleepy).abs() < 0.01);
        let mut d = Drain::NEW;
        d.read(None, Some(Running::Sleepy), 3600);
        let low = hours(d.left(None, false, 3600 + 10 * 3600));
        assert!((low - (20.0 / sleepy - 10.0)).abs() < 0.01, "{low}");
        assert_eq!(d.left(None, false, 400 * 3600), None);
    }

    /// What is kept comes back within a step.
    #[test]
    fn a_learned_drain_is_kept() {
        let mut d = Drain::NEW;
        assert_eq!(d.kept(), None);
        d.learn(Running::Sleepy, 0.2, 15 * 3600);
        let kept = d.kept().unwrap();
        assert_eq!(kept[0], 0);
        let back = Drain::from_kept(Some(kept));
        let (r, b) = (d.rate(Running::Sleepy), back.rate(Running::Sleepy));
        assert!(((b - r) / r).abs() < 0.012, "{r} {b}");
        assert_eq!(back.rate(Running::Relay), Running::Relay.seed());
    }

    #[test]
    fn the_mapping_has_the_low_level() {
        assert_eq!(battery_percent(3742), Some(BATTERY_LOW_PERCENT));
    }
}
