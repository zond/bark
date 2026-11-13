//! Per-channel busy estimate (spec "Busy estimate"), in integer arithmetic.
//!
//! `busy[c]` is fixed point with [`ONE`] meaning "always busy". Its
//! first `busy_ema_inverse` samples use `1/n` for the `n`th, a plain
//! running mean, so a node just booted has an unbiased estimate after its
//! one-rotation listen (spec "Channel at boot") rather than one dragged
//! towards a start value. Integer division biases it by less than
//! `busy_ema_inverse / ONE` (8.2 × 10⁻⁶ at 1/138). Which home readings are
//! the node's own traffic and set aside is the relay's call
//! (`node/relay.rs`).
//!
//! Each channel keeps its own readings. Listen-before-talk judges a
//! reading against its own channel's floor, which interference near the
//! receiver raises on some channels and not on others. A busy reading of
//! the sweep is judged against all channels' readings together, so that
//! a channel louder than the others counts as busy and is not chosen. A
//! 10th percentile fails only if nine-tenths of the readings are
//! signals; carrier sense keeps load far below that
//! (spec "Capacity and sizing"). The percentile and the 1 dB bins are
//! first choices for open item O11.

use crate::node::Config;
use crate::radio::{Channel, CHANNELS};

/// Fixed-point one: a channel that is always busy.
pub const ONE: u32 = 1 << 24;

/// `percent` %, in fixed point.
pub const fn percent(percent: u8) -> u32 {
    (ONE as u64 * percent as u64 / 100) as u32
}

/// Noise floor before any sample: thermal noise in 125 kHz plus the
/// receiver's noise figure (spec "Physical layer": ≈−118 dBm).
pub const NOMINAL_NOISE_FLOOR_DBM: i16 = -118;

/// Lowest RSSI the SX1262 reports: `−RssiInst / 2` with an 8-bit register,
/// so −127.5 dBm (datasheet §13.5.2, GetRssiInst). The bottom bin.
const HIST_LOW_DBM: i16 = -128;

/// Histogram bins, 1 dB each, −128 to −65 dBm. Anything louder is not
/// noise and lands in the top bin, which a 10th percentile never reaches
/// unless nine-tenths of samples are that loud.
const HIST_BINS: usize = 64;

/// Percentile of RSSI samples taken as the noise floor.
const FLOOR_PERCENTILE: u64 = 10;

/// The noise-floor estimator.
#[derive(Clone, Copy, Debug)]
pub struct NoiseFloor {
    /// No bin counts past `halve_at`, which `new` keeps within `u16`.
    bins: [u16; HIST_BINS],
    total: u32,
    halve_at: u32,
    /// The bin the percentile falls in, and the samples below it: kept as
    /// samples come, since the floor is read on every event.
    floor: usize,
    below: u32,
}

impl NoiseFloor {
    /// Empty; halves its histogram whenever it holds `halve_at` samples.
    pub const fn new(halve_at: u32) -> NoiseFloor {
        NoiseFloor {
            bins: [0; HIST_BINS],
            total: 0,
            halve_at: if halve_at > u16::MAX as u32 {
                u16::MAX as u32
            } else {
                halve_at
            },
            floor: 0,
            below: 0,
        }
    }

    /// Add one RSSI reading.
    pub fn add(&mut self, dbm: i16) {
        let bin = (dbm.saturating_sub(HIST_LOW_DBM)).clamp(0, HIST_BINS as i16 - 1) as usize;
        self.bins[bin] += 1;
        self.total += 1;
        if bin < self.floor {
            self.below += 1;
        }
        if self.total >= self.halve_at {
            self.halve();
        }
        self.settle();
    }

    /// Add one reading to a histogram kept to `window` samples: what it
    /// holds is halved first until it is below that.
    pub fn add_within(&mut self, dbm: i16, window: u32) {
        while self.total >= window.clamp(1, self.halve_at) {
            self.halve();
        }
        self.add(dbm);
    }

    fn halve(&mut self) {
        self.total = 0;
        for b in &mut self.bins {
            *b /= 2;
            self.total += *b as u32;
        }
        (self.floor, self.below) = (0, 0);
    }

    /// Move the floor to the lowest bin where the bins up to it hold the
    /// percentile's rank of samples, stepping from where it was: after one
    /// sample it is close, after a halving it climbs from the bottom.
    fn settle(&mut self) {
        if self.total == 0 {
            (self.floor, self.below) = (0, 0);
            return;
        }
        let rank = rank(self.total);
        loop {
            if self.floor > 0 && self.below as u64 >= rank {
                self.floor -= 1;
                self.below -= self.bins[self.floor] as u32;
            } else if self.floor + 1 < HIST_BINS
                && ((self.below + self.bins[self.floor] as u32) as u64) < rank
            {
                self.below += self.bins[self.floor] as u32;
                self.floor += 1;
            } else {
                return;
            }
        }
    }

    /// The floor in dBm.
    pub fn dbm(&self) -> i16 {
        if self.total == 0 {
            return NOMINAL_NOISE_FLOOR_DBM;
        }
        HIST_LOW_DBM + self.floor as i16
    }
}

/// The floor of several histograms' readings taken together.
fn pooled(floors: &[NoiseFloor]) -> i16 {
    let total: u32 = floors.iter().map(|n| n.total).sum();
    if total == 0 {
        return NOMINAL_NOISE_FLOOR_DBM;
    }
    let rank = rank(total);
    let mut seen = 0u64;
    for i in 0..HIST_BINS {
        seen += floors.iter().map(|n| n.bins[i] as u64).sum::<u64>();
        if seen >= rank {
            return HIST_LOW_DBM + i as i16;
        }
    }
    HIST_LOW_DBM + HIST_BINS as i16 - 1
}

/// How many of `total` samples lie at or below the floor.
fn rank(total: u32) -> u64 {
    (total as u64 * FLOOR_PERCENTILE).div_ceil(100).max(1)
}

/// The busy estimates for all channels.
#[derive(Clone, Debug)]
pub struct BusyEstimator {
    busy: [u32; CHANNELS as usize],
    samples: [u16; CHANNELS as usize],
    inverse: u16,
    threshold_db: u8,
    /// Each channel's readings.
    noise: [NoiseFloor; CHANNELS as usize],
}

impl BusyEstimator {
    /// Estimator with EMA coefficient `1 / inverse` and busy threshold
    /// `threshold_db` above the noise floor.
    pub const fn new(inverse: u16, threshold_db: u8) -> BusyEstimator {
        let inverse = if inverse == 0 { 1 } else { inverse };
        BusyEstimator {
            busy: [0; CHANNELS as usize],
            samples: [0; CHANNELS as usize],
            inverse,
            threshold_db,
            // Two windows of a channel's readings, about an hour.
            noise: [NoiseFloor::new(2 * inverse as u32); CHANNELS as usize],
        }
    }

    /// From `config.busy_ema_inverse` and `config.busy_threshold_db`.
    pub const fn from_config(config: &Config) -> BusyEstimator {
        BusyEstimator::new(config.busy_ema_inverse, config.busy_threshold_db)
    }

    /// Busy fraction of `c`, in units of [`ONE`].
    pub fn fraction(&self, c: Channel) -> u32 {
        self.busy[c.index() as usize]
    }

    /// The noise floor of `c` in dBm: of its own readings, or of all
    /// channels' while it has none.
    pub fn noise_floor_dbm(&self, c: Channel) -> i16 {
        let own = &self.noise[c.index() as usize];
        match own.total {
            0 => self.pooled_floor_dbm(),
            _ => own.dbm(),
        }
    }

    /// The noise floor of all channels' readings together, in dBm.
    pub fn pooled_floor_dbm(&self) -> i16 {
        pooled(&self.noise)
    }

    /// RSSI above which listen-before-talk finds `c` busy, in dBm.
    pub fn threshold_dbm(&self, c: Channel) -> i16 {
        self.noise_floor_dbm(c)
            .saturating_add(self.threshold_db as i16)
    }

    /// A reading of `c` that is no part of a sweep, for its floor alone:
    /// a listen-before-talk reading of a node that does not sweep. The
    /// channel's floor then holds `window` readings at most, so that it
    /// follows them at their own rate.
    pub fn floor_reading(&mut self, c: Channel, dbm: i16, window: u32) {
        self.noise[c.index() as usize].add_within(dbm, window);
    }

    /// One sweep reading of a channel, home included: busy if above all
    /// channels' floor, so that a channel louder than the others is not
    /// chosen.
    pub fn sample(&mut self, c: Channel, dbm: i16) {
        let threshold = self
            .pooled_floor_dbm()
            .saturating_add(self.threshold_db as i16);
        let x = if dbm > threshold { ONE } else { 0 };
        self.noise[c.index() as usize].add(dbm);
        self.update(c, x);
    }

    fn update(&mut self, c: Channel, x: u32) {
        let i = c.index() as usize;
        let n = (self.samples[i] + 1).min(self.inverse);
        self.samples[i] = n;
        let b = self.busy[i] as i64;
        self.busy[i] = (b + (x as i64 - b) / n as i64) as u32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(i: u8) -> Channel {
        Channel::new(i).unwrap()
    }

    #[test]
    fn running_mean_then_ema() {
        let mut b = BusyEstimator::from_config(&Config::default());
        // Floor stays at the nominal −118 dBm while quiet samples dominate.
        for _ in 0..9 {
            b.sample(ch(0), -120);
        }
        b.sample(ch(0), -100);
        assert_eq!(b.fraction(ch(0)), ONE / 10, "running mean of 1 in 10");
        assert_eq!(b.fraction(ch(1)), 0);
        // Long after: an always-busy channel converges to ONE within the bias.
        for _ in 0..10_000 {
            b.sample(ch(2), -60);
            for c in 3..CHANNELS {
                b.sample(ch(c), -122);
            }
        }
        let n = Config::default().busy_ema_inverse as u32;
        assert!(b.fraction(ch(2)) > ONE - n);
        // And decays with time constant n samples: (1 − 1/n)ⁿ ≈ e⁻¹ after n.
        for _ in 0..n {
            b.sample(ch(2), -125);
        }
        let f = b.fraction(ch(2)) as f64 / ONE as f64;
        let want = (1.0 - 1.0 / n as f64).powi(n as i32);
        assert!((f - want).abs() < 0.002, "{f} against {want}");
    }

    #[test]
    fn ten_percent_channel_estimates_ten_percent() {
        let mut b = BusyEstimator::from_config(&Config::default());
        for i in 0..20_000u32 {
            for c in 0..CHANNELS {
                let busy = c == 6 && i % 10 == 0;
                b.sample(ch(c), if busy { -90 } else { -121 + (i % 3) as i16 });
            }
        }
        let f = b.fraction(ch(6));
        assert!(
            f.abs_diff(percent(10)) < percent(2),
            "{}",
            f as f64 / ONE as f64
        );
        assert!(b.fraction(ch(5)) < percent(1));
    }

    #[test]
    fn noise_floor() {
        let mut n = NoiseFloor::new(1000);
        assert_eq!(n.dbm(), NOMINAL_NOISE_FLOOR_DBM);
        // 30 % of samples at −121, the rest busy at −80: floor −121.
        for i in 0..100 {
            n.add(if i % 10 < 3 { -121 } else { -80 });
        }
        assert_eq!(n.dbm(), -121);
        // Out-of-range readings clamp into the end bins.
        let mut m = NoiseFloor::new(1000);
        m.add(-200);
        assert_eq!(m.dbm(), HIST_LOW_DBM);
        m.add(i16::MAX);
        m.add(i16::MIN);
        // The floor drifts: after the halvings a raised floor takes over.
        let mut d = NoiseFloor::new(100);
        for _ in 0..100 {
            d.add(-122);
        }
        for _ in 0..400 {
            d.add(-110);
        }
        assert_eq!(d.dbm(), -110);
    }

    /// The floor counted out of the histogram, as defined.
    fn counted(n: &NoiseFloor) -> i16 {
        if n.total == 0 {
            return NOMINAL_NOISE_FLOOR_DBM;
        }
        let rank = rank(n.total);
        let mut seen = 0;
        for (i, &b) in n.bins.iter().enumerate() {
            seen += b as u64;
            if seen >= rank {
                return HIST_LOW_DBM + i as i16;
            }
        }
        HIST_LOW_DBM + HIST_BINS as i16 - 1
    }

    #[test]
    fn kept_floor_is_the_counted_one() {
        // Readings over the whole range and past both ends, narrow and
        // wide in turn and drifting, with halvings from every sample to
        // the estimator's own.
        let own = 2 * CHANNELS as u32 * Config::default().busy_ema_inverse as u32;
        for halve_at in [1, 2, 7, 100, own] {
            let mut n = NoiseFloor::new(halve_at);
            let mut x: u32 = 1;
            for i in 0..50_000u32 {
                x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                let spread = if (i / 5000) % 2 == 0 { 8 } else { 90 };
                let dbm = -135 + ((x >> 16) % spread) as i16 + (i / 2000) as i16;
                n.add(dbm);
                assert_eq!(n.dbm(), counted(&n), "halve_at {halve_at}, sample {i}");
            }
        }
    }

    #[test]
    fn threshold_follows_floor() {
        let mut b = BusyEstimator::new(600, 6);
        assert_eq!(
            b.threshold_dbm(ch(0)),
            -112,
            r#"spec "Busy estimate": ≈−112 dBm"#
        );
        for _ in 0..100 {
            b.sample(ch(0), -124);
        }
        assert_eq!(b.threshold_dbm(ch(0)), -118);
        // A channel not yet read takes all channels' floor.
        assert_eq!(b.threshold_dbm(ch(1)), -118);
        b.sample(ch(1), -117);
        assert_eq!(b.fraction(ch(1)), ONE);
    }

    /// The bench of 2026-11-12: channels whose quiet readings differ by
    /// 8 dB. Listen-before-talk on the loud one finds it clear at its usual
    /// level, while the sweep still counts it busy against the others.
    #[test]
    fn a_loud_channel_is_clear_to_its_checks_and_busy_to_the_sweep() {
        let mut b = BusyEstimator::from_config(&Config::default());
        let (quiet, loud) = (ch(7), ch(9));
        for i in 0..2000 {
            for c in Channel::all() {
                let level = if c == loud { -83 } else { -91 };
                b.sample(c, level - (i % 3) as i16);
            }
        }
        assert_eq!(b.pooled_floor_dbm(), -93);
        assert_eq!(b.noise_floor_dbm(quiet), -93);
        assert_eq!(b.noise_floor_dbm(loud), -85);
        assert!(
            -83 <= b.threshold_dbm(loud),
            "a quiet reading of it is clear"
        );
        assert!(b.fraction(loud) > percent(99));
        assert!(b.fraction(quiet) < percent(1));
    }

    /// A carrier on one channel moves that channel's floor only once it
    /// fills nine-tenths of what the floor holds: over an hour of sweeps,
    /// longer than the move rule takes to leave it (spec "Channel moves").
    #[test]
    fn a_carrier_moves_its_channel_s_floor_after_an_hour() {
        let config = Config::default();
        let mut b = BusyEstimator::from_config(&config);
        let jammed = ch(4);
        let rounds_per_hour = 3600 / config.sweep_interval.as_micros().div_ceil(1_000_000);
        let round = |b: &mut BusyEstimator| {
            for c in Channel::all() {
                b.sample(c, if c == jammed { -60 } else { -120 });
            }
        };
        for _ in 0..2 * rounds_per_hour {
            for c in Channel::all() {
                b.sample(c, -120);
            }
        }
        for _ in 0..rounds_per_hour {
            round(&mut b);
        }
        assert_eq!(b.threshold_dbm(jammed), -114, "an hour in");
        for _ in 0..rounds_per_hour {
            round(&mut b);
        }
        assert!(b.threshold_dbm(jammed) > -60, "two hours in");
        assert_eq!(b.pooled_floor_dbm(), -120, "the others hold theirs");
        // The sweep still counts it busy: (1 − 1/138)^276 of the quiet
        // past is left, e^−2.
        assert!(b.fraction(jammed) > percent(85));
    }

    /// A node that does not sweep: its checks keep its home's floor, at
    /// most `window` of them, so that a few cannot move it and a few
    /// windows' worth do.
    #[test]
    fn checks_keep_their_channel_s_floor_within_their_window() {
        let mut b = BusyEstimator::from_config(&Config::default());
        let (home, window) = (ch(2), 24);
        for _ in 0..300 {
            for c in Channel::all() {
                b.sample(c, -93);
            }
        }
        for _ in 0..window / 4 {
            b.floor_reading(home, -85, window);
        }
        assert_eq!(b.noise_floor_dbm(home), -93);
        for _ in 0..4 * window {
            b.floor_reading(home, -85, window);
        }
        assert_eq!(b.noise_floor_dbm(home), -85);
        assert_eq!(b.noise_floor_dbm(ch(3)), -93);
    }
}
