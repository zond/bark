//! Per-channel busy estimate (spec "Busy estimate"), in integer arithmetic.
//!
//! # Busy fraction
//!
//! Each channel keeps `busy[c]`, the moving average of a busy indicator
//! sampled once per sweep: 1 if the RSSI exceeds the noise floor by
//! `busy_threshold_db` (spec "Busy estimate": 6 dB), else 0. Values are fixed point with
//! [`ONE`] = 2²⁴ meaning "always busy". The average is an EMA with
//! coefficient `1 / busy_ema_inverse` (spec "Busy estimate": 1/600, ten minutes at one
//! sample per second), except that its first `busy_ema_inverse` samples
//! use `1/n` for the `n`th sample: a plain running mean, so a node that has
//! just booted has an unbiased estimate after its one-rotation listen
//! (spec "Channel at boot") instead of one dragged towards an arbitrary start value. The
//! integer division truncates, which biases the estimate by less than
//! `busy_ema_inverse / ONE` (3.6 × 10⁻⁵ at 1/600).
//!
//! The home estimate counts only what would stay on the channel if the
//! node moved (spec "Busy estimate", roadmap D24). Packets addressed to the node, and the
//! ACKs of its own hand-overs, would follow it to its new channel, so
//! counting them would make a busy relay flee its own traffic. A home
//! reading taken while one of them was on the air is therefore not taken;
//! any other is the same indicator as a reading of another channel, so
//! that the home channel and the channels it is compared with are measured
//! alike (roadmap D55). The relay decides which readings are its own
//! (`node/relay.rs`).
//!
//! # Noise floor
//!
//! Spec "Busy estimate" says: a low percentile of RSSI readings over the first rotation,
//! recalibrated slowly; the method is open item O11. Here: every sample,
//! from every channel, goes into a histogram of 1 dB bins, and the floor
//! is its 10th percentile. The receiver's noise is the same on all 13
//! channels (1.6 MHz of one front end), so pooling them is sound, and a
//! 10th percentile is right as long as a tenth of the samples hit a quiet
//! channel. If nine-tenths of all readings on all channels are signals,
//! the floor rises to the signals and every channel looks idle; carrier
//! sense keeps real load far below that (≈35 % usable, spec "Capacity and sizing"). When the histogram holds two busy windows of samples
//! (2 × 13 × `busy_ema_inverse`), every bin is halved, so the floor tracks
//! drift on the busy estimate's own time scale. Until the first sample the
//! floor is the design's nominal −118 dBm (spec "Physical layer"). The 10th percentile and the
//! 1 dB bins are first choices for O11, to be checked on the bench.

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
#[derive(Clone, Debug)]
pub struct NoiseFloor {
    bins: [u32; HIST_BINS],
    total: u32,
    halve_at: u32,
}

impl NoiseFloor {
    /// Empty; halves its histogram whenever it holds `halve_at` samples.
    pub const fn new(halve_at: u32) -> NoiseFloor {
        NoiseFloor {
            bins: [0; HIST_BINS],
            total: 0,
            halve_at,
        }
    }

    /// Add one RSSI reading.
    pub fn add(&mut self, dbm: i16) {
        let bin = (dbm.saturating_sub(HIST_LOW_DBM)).clamp(0, HIST_BINS as i16 - 1) as usize;
        self.bins[bin] += 1;
        self.total += 1;
        if self.total >= self.halve_at {
            self.total = 0;
            for b in &mut self.bins {
                *b /= 2;
                self.total += *b;
            }
        }
    }

    /// The floor in dBm.
    pub fn dbm(&self) -> i16 {
        if self.total == 0 {
            return NOMINAL_NOISE_FLOOR_DBM;
        }
        let rank = (self.total as u64 * FLOOR_PERCENTILE).div_ceil(100).max(1);
        let mut seen = 0;
        for (i, &b) in self.bins.iter().enumerate() {
            seen += b as u64;
            if seen >= rank {
                return HIST_LOW_DBM + i as i16;
            }
        }
        HIST_LOW_DBM + HIST_BINS as i16 - 1
    }
}

/// The busy estimates for all channels.
#[derive(Clone, Debug)]
pub struct BusyEstimator {
    busy: [u32; CHANNELS as usize],
    samples: [u16; CHANNELS as usize],
    inverse: u16,
    threshold_db: u8,
    noise: NoiseFloor,
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
            noise: NoiseFloor::new(2 * CHANNELS as u32 * inverse as u32),
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

    /// Current noise floor in dBm.
    pub fn noise_floor_dbm(&self) -> i16 {
        self.noise.dbm()
    }

    /// RSSI above which a channel counts as busy, in dBm.
    pub fn threshold_dbm(&self) -> i16 {
        self.noise_floor_dbm()
            .saturating_add(self.threshold_db as i16)
    }

    /// One sweep reading of a channel, home included.
    pub fn sample(&mut self, c: Channel, dbm: i16) {
        let x = if dbm > self.threshold_dbm() { ONE } else { 0 };
        self.noise.add(dbm);
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
        // The other channels are swept too and stay quiet, holding the floor.
        for _ in 0..10_000 {
            b.sample(ch(2), -60);
            for c in 3..CHANNELS {
                b.sample(ch(c), -122);
            }
        }
        assert!(b.fraction(ch(2)) > ONE - 600);
        // And decays with time constant 600 samples: e⁻¹ after 600.
        for _ in 0..600 {
            b.sample(ch(2), -125);
        }
        let f = b.fraction(ch(2)) as f64 / ONE as f64;
        assert!((f - 0.3677).abs() < 0.002, "{f}");
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

    #[test]
    fn threshold_follows_floor() {
        let mut b = BusyEstimator::new(600, 6);
        assert_eq!(
            b.threshold_dbm(),
            -112,
            r#"spec "Busy estimate": ≈−112 dBm"#
        );
        for _ in 0..100 {
            b.sample(ch(0), -124);
        }
        assert_eq!(b.threshold_dbm(), -118);
        b.sample(ch(1), -117);
        assert_eq!(b.fraction(ch(1)), ONE);
    }
}
