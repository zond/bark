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
//! a channel louder than the others counts as busy and is not chosen;
//! a channel with no readings takes that floor too.
//!
//! A channel's floor is the median of its readings, so traffic moves it
//! only once it fills more than half the channel's time, which carrier
//! sense keeps far from (spec "Capacity and sizing"), and it follows a
//! rise as fast as a fall. A level that stays for about a half-life is
//! noise. Readings fade by the clock, halving every half-life, so a node
//! that reads a channel once a beat and one that sweeps it every few
//! seconds follow a change in the same time. The 1 dB bins are a first
//! choice for open item O11.

use crate::node::{noise_half_life, Config};
use crate::radio::{Channel, CHANNELS};
use crate::time::{Duration, Instant};

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
/// noise and lands in the top bin, which a median never reaches unless
/// half the readings are that loud.
const HIST_BINS: usize = 64;

/// A fresh reading's weight in its bin. A lone reading fades out after
/// eight half-lives, long after it last counted in a median; a channel no
/// longer read is empty within sixteen, its bins being two bytes. A bin
/// holds 255 fresh readings, more than the two half-lives' worth a sweep
/// gives one channel; a bin that would overflow halves the histogram early.
const READING_WEIGHT: u16 = 1 << 8;

/// The noise-floor estimator.
#[derive(Clone, Copy, Debug)]
pub struct NoiseFloor {
    bins: [u16; HIST_BINS],
    total: u32,
    /// The bin the median falls in, and the weight below it: kept as
    /// readings come, since the floor is read on every event.
    floor: usize,
    below: u32,
}

impl NoiseFloor {
    /// Empty.
    pub const fn new() -> NoiseFloor {
        NoiseFloor {
            bins: [0; HIST_BINS],
            total: 0,
            floor: 0,
            below: 0,
        }
    }

    /// Add one RSSI reading.
    pub fn add(&mut self, dbm: i16) {
        let bin = (dbm.saturating_sub(HIST_LOW_DBM)).clamp(0, HIST_BINS as i16 - 1) as usize;
        if self.bins[bin] > u16::MAX - READING_WEIGHT {
            self.halve(1);
        }
        self.bins[bin] += READING_WEIGHT;
        self.total += READING_WEIGHT as u32;
        if bin < self.floor {
            self.below += READING_WEIGHT as u32;
        }
        self.settle();
    }

    /// Halve every reading's weight `times` times.
    fn halve(&mut self, times: u64) {
        if self.total == 0 {
            return;
        }
        let shift = times.min(u16::BITS as u64) as u32;
        self.total = 0;
        for b in &mut self.bins {
            *b = b.checked_shr(shift).unwrap_or(0);
            self.total += *b as u32;
        }
        (self.floor, self.below) = (0, 0);
        self.settle();
    }

    /// Move the floor to the lowest bin where the bins up to it hold half
    /// the weight, stepping from where it was: after one reading it is
    /// close, after a halving it climbs from the bottom.
    fn settle(&mut self) {
        if self.total == 0 {
            (self.floor, self.below) = (0, 0);
            return;
        }
        let rank = rank(self.total, CHANNEL_PERCENT);
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

impl Default for NoiseFloor {
    fn default() -> NoiseFloor {
        NoiseFloor::new()
    }
}

/// The floor of several histograms' readings taken together: their 10th
/// percentile, so that it stays at the quiet channels' level while
/// interference sits on up to nine-tenths of them.
fn pooled(floors: &[NoiseFloor]) -> i16 {
    let total: u32 = floors.iter().map(|n| n.total).sum();
    if total == 0 {
        return NOMINAL_NOISE_FLOOR_DBM;
    }
    let rank = rank(total, POOLED_PERCENT);
    let mut seen = 0u64;
    for i in 0..HIST_BINS {
        seen += floors.iter().map(|n| n.bins[i] as u64).sum::<u64>();
        if seen >= rank {
            return HIST_LOW_DBM + i as i16;
        }
    }
    HIST_LOW_DBM + HIST_BINS as i16 - 1
}

/// The share of weight at or below a channel's own floor, in percent:
/// the median. At equal weight either side the quieter side is the floor.
const CHANNEL_PERCENT: u64 = 50;

/// The same for the floor of all channels together. A median suits one
/// channel, since traffic fills less than half its time, but interference
/// can sit on more than half the channels, and this floor is what compares
/// them.
const POOLED_PERCENT: u64 = 10;

/// How much of `total` weight lies at or below a floor at `percent`.
fn rank(total: u32, percent: u64) -> u64 {
    (total as u64 * percent).div_ceil(100).max(1)
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
    half_life: Duration,
    /// The half-lives since the clock's epoch at the last halving: every
    /// channel halves at the same instants.
    halved: u64,
}

impl BusyEstimator {
    /// Estimator with EMA coefficient `1 / inverse`, busy threshold
    /// `threshold_db` above the noise floor, and readings that keep half
    /// their weight for `half_life`.
    pub const fn new(inverse: u16, threshold_db: u8, half_life: Duration) -> BusyEstimator {
        let inverse = if inverse == 0 { 1 } else { inverse };
        BusyEstimator {
            busy: [0; CHANNELS as usize],
            samples: [0; CHANNELS as usize],
            inverse,
            threshold_db,
            noise: [NoiseFloor::new(); CHANNELS as usize],
            half_life: if half_life.as_micros() == 0 {
                Duration::from_micros(1)
            } else {
                half_life
            },
            halved: 0,
        }
    }

    /// From `config.busy_ema_inverse`, `config.busy_threshold_db` and
    /// [`noise_half_life`].
    pub const fn from_config(config: &Config) -> BusyEstimator {
        BusyEstimator::new(
            config.busy_ema_inverse,
            config.busy_threshold_db,
            noise_half_life(config),
        )
    }

    /// Fade the readings to `now`: the floors read until the next call
    /// are those of `now`.
    pub fn age(&mut self, now: Instant) {
        let period = now.as_micros() / self.half_life.as_micros();
        if period > self.halved {
            for n in &mut self.noise {
                n.halve(period - self.halved);
            }
            self.halved = period;
        }
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

    /// A reading of `c` at `now` that is no part of a sweep, for its floor
    /// alone: a listen-before-talk reading of a node that does not sweep.
    pub fn floor_reading(&mut self, c: Channel, dbm: i16, now: Instant) {
        self.age(now);
        self.noise[c.index() as usize].add(dbm);
    }

    /// One sweep reading of a channel at `now`, home included: busy if
    /// above all channels' floor, so that a channel louder than the others
    /// is not chosen.
    pub fn sample(&mut self, c: Channel, dbm: i16, now: Instant) {
        self.age(now);
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

    fn secs(s: u64) -> Instant {
        Instant::from_micros(s * 1_000_000)
    }

    /// A sweep of every channel at `t` seconds, each at `level(c)`.
    fn sweep(b: &mut BusyEstimator, t: u64, level: impl Fn(Channel) -> i16) {
        for c in Channel::all() {
            b.sample(c, level(c), secs(t));
        }
    }

    /// Seconds between two readings of a channel by the sweep.
    fn round() -> u64 {
        Config::default().sweep_interval.as_micros() / 1_000_000
    }

    fn half_life() -> u64 {
        noise_half_life(&Config::default()).as_micros() / 1_000_000
    }

    #[test]
    fn running_mean_then_ema() {
        let mut b = BusyEstimator::from_config(&Config::default());
        // The floor is the quiet readings' while they are most.
        for i in 0..9 {
            b.sample(ch(0), -120, secs(i));
        }
        b.sample(ch(0), -100, secs(9));
        assert_eq!(b.fraction(ch(0)), ONE / 10, "running mean of 1 in 10");
        assert_eq!(b.fraction(ch(1)), 0);
        // Long after: an always-busy channel converges to ONE within the bias.
        let mut t = 10;
        for _ in 0..10_000 {
            b.sample(ch(2), -60, secs(t));
            for c in 3..CHANNELS {
                b.sample(ch(c), -122, secs(t));
            }
            t += 1;
        }
        let n = Config::default().busy_ema_inverse as u32;
        assert!(b.fraction(ch(2)) > ONE - n);
        // And decays with time constant n samples: (1 − 1/n)ⁿ ≈ e⁻¹ after n.
        for _ in 0..n {
            b.sample(ch(2), -125, secs(t));
            t += 1;
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
                let dbm = if busy { -90 } else { -121 + (i % 3) as i16 };
                b.sample(ch(c), dbm, secs(i as u64 * round()));
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
        let mut n = NoiseFloor::new();
        assert_eq!(n.dbm(), NOMINAL_NOISE_FLOOR_DBM);
        // The median: 60 % of readings at −121, the rest at −80.
        for i in 0..100 {
            n.add(if i % 10 < 6 { -121 } else { -80 });
        }
        assert_eq!(n.dbm(), -121);
        // With as much weight either side, the quieter side.
        let mut e = NoiseFloor::new();
        e.add(-100);
        e.add(-90);
        assert_eq!(e.dbm(), -100);
        // Out-of-range readings clamp into the end bins.
        let mut m = NoiseFloor::new();
        m.add(-200);
        assert_eq!(m.dbm(), HIST_LOW_DBM);
        m.add(i16::MAX);
        m.add(i16::MAX);
        assert_eq!(m.dbm(), HIST_LOW_DBM + HIST_BINS as i16 - 1);
        m.add(i16::MIN);
        // A bin that would overflow halves the histogram and keeps the median.
        let mut f = NoiseFloor::new();
        for i in 0..1000 {
            f.add(if i % 3 == 0 { -95 } else { -100 });
        }
        assert_eq!(f.dbm(), -100);
        assert_eq!(f.dbm(), counted(&f));
    }

    /// The floor counted out of the histogram, as defined.
    fn counted(n: &NoiseFloor) -> i16 {
        assert_eq!(n.total, n.bins.iter().map(|&b| b as u32).sum::<u32>());
        if n.total == 0 {
            return NOMINAL_NOISE_FLOOR_DBM;
        }
        let rank = rank(n.total, CHANNEL_PERCENT);
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
        // wide in turn and drifting, with halvings of one to three steps
        // every so many readings.
        for every in [1, 2, 7, 100, 1000] {
            let mut n = NoiseFloor::new();
            let mut x: u32 = 1;
            for i in 0..50_000u32 {
                x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                let spread = if (i / 5000) % 2 == 0 { 8 } else { 90 };
                let dbm = -135 + ((x >> 16) % spread) as i16 + (i / 2000) as i16;
                n.add(dbm);
                assert_eq!(n.dbm(), counted(&n), "every {every}, reading {i}");
                if i % every == 0 {
                    n.halve(1 + (x >> 8) as u64 % 3);
                    assert_eq!(n.dbm(), counted(&n), "every {every}, halved at {i}");
                }
            }
        }
    }

    #[test]
    fn threshold_follows_floor() {
        let mut b = BusyEstimator::new(600, 6, Duration::from_secs(900));
        assert_eq!(
            b.threshold_dbm(ch(0)),
            -112,
            r#"spec "Busy estimate": ≈−112 dBm"#
        );
        for i in 0..100 {
            b.sample(ch(0), -124, secs(i));
        }
        assert_eq!(b.threshold_dbm(ch(0)), -118);
        // A channel not yet read takes all channels' floor.
        assert_eq!(b.threshold_dbm(ch(1)), -118);
        b.sample(ch(1), -117, secs(100));
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
            sweep(&mut b, i * round(), |c| {
                let level = if c == loud { -83 } else { -91 };
                level - (i % 3) as i16
            });
        }
        assert_eq!(b.pooled_floor_dbm(), -93);
        assert_eq!(b.noise_floor_dbm(quiet), -92);
        assert_eq!(b.noise_floor_dbm(loud), -84);
        assert!(
            -83 <= b.threshold_dbm(loud),
            "a quiet reading of it is clear"
        );
        assert!(b.fraction(loud) > percent(99));
        assert!(b.fraction(quiet) < percent(1));
    }

    /// Seconds from `from` until `floor` first reads `want`, with `read`
    /// called each second.
    fn reaches(
        b: &mut BusyEstimator,
        from: u64,
        mut read: impl FnMut(&mut BusyEstimator, u64),
        floor: impl Fn(&BusyEstimator) -> i16,
        want: i16,
    ) -> u64 {
        for t in from..from + 10 * half_life() {
            read(b, t);
            if floor(b) == want {
                return t - from;
            }
        }
        panic!("the floor never reached {want}");
    }

    /// The bench of roadmap D233: the noise fell 6 dB for hours and came
    /// back. A sweeping node's floor follows the fall and the rise in the
    /// same time, a half-life.
    #[test]
    fn a_rise_and_a_fall_are_followed_in_the_same_time() {
        let step = |from: i16, to: i16| {
            let mut b = BusyEstimator::from_config(&Config::default());
            let start = 2 * 3600;
            for t in (0..start).step_by(round() as usize) {
                sweep(&mut b, t, |_| from);
            }
            let read = |b: &mut BusyEstimator, t: u64| {
                if t.is_multiple_of(round()) {
                    sweep(b, t, |_| to);
                }
            };
            reaches(&mut b, start, read, |b| b.noise_floor_dbm(ch(4)), to)
        };
        let (rise, fall) = (step(-93, -87), step(-87, -93));
        assert_eq!(rise, fall);
        assert!(rise.abs_diff(half_life()) <= 2 * round(), "{rise} s");
    }

    /// A node that reads its channel only at its beats, a random half to
    /// one beat period apart, follows a change within a beat period of one
    /// that sweeps it every 13 s.
    #[test]
    fn a_node_reading_once_a_beat_follows_as_fast_as_one_that_sweeps() {
        let config = Config::default();
        let period = crate::tables::heard::beat_period(config.delivery_ttl).as_micros() / 1_000_000;
        let home = ch(3);
        let follow = |seed: Option<u32>| {
            let mut b = BusyEstimator::from_config(&config);
            let mut x = seed.unwrap_or(0);
            let mut due = 0;
            let mut read = |b: &mut BusyEstimator, t: u64, level: i16| match seed {
                None if t.is_multiple_of(round()) => sweep(b, t, |_| level),
                Some(_) if t == due => {
                    b.floor_reading(home, level, secs(t));
                    x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                    due = t + period / 2 + (x >> 16) as u64 % (period / 2 + 1);
                }
                _ => {}
            };
            let start = 4 * 3600;
            for t in 0..start {
                read(&mut b, t, -93);
            }
            let read = |b: &mut BusyEstimator, t| read(b, t, -87);
            reaches(&mut b, start, read, |b| b.noise_floor_dbm(home), -87)
        };
        let swept = follow(None);
        for seed in 1..=20 {
            let beats = follow(Some(seed));
            assert!(
                beats.abs_diff(swept) <= period,
                "seed {seed}: {beats} s against {swept} s"
            );
        }
    }

    /// Traffic on 30 % of a channel's readings, louder than any noise,
    /// never moves its floor; its busy estimate counts it.
    #[test]
    fn traffic_on_three_tenths_of_readings_does_not_move_the_floor() {
        let mut b = BusyEstimator::from_config(&Config::default());
        let busy = ch(6);
        for i in 0..4000 {
            let t = i * round();
            sweep(
                &mut b,
                t,
                |c| {
                    if c == busy && i % 10 >= 7 {
                        -60
                    } else {
                        -93
                    }
                },
            );
            assert_eq!(b.noise_floor_dbm(busy), -93, "at {t} s");
        }
        assert!(b.fraction(busy).abs_diff(percent(30)) < percent(5));
    }

    /// A channel no longer read fades to nothing and then takes all
    /// channels' floor; with nothing read anywhere, the nominal floor.
    #[test]
    fn an_emptied_channel_falls_back_to_the_pooled_floor() {
        let mut b = BusyEstimator::from_config(&Config::default());
        let alone = ch(0);
        let mut t = 0;
        while t < 3600 {
            sweep(&mut b, t, |c| if c == alone { -80 } else { -95 });
            t += round();
        }
        assert_eq!(b.noise_floor_dbm(alone), -80);
        // A bin holds at most `u16::MAX`, which sixteen halvings empty.
        let gone = t + 16 * half_life();
        while t + 2 * half_life() < gone {
            for c in Channel::all().filter(|&c| c != alone) {
                b.sample(c, -95, secs(t));
            }
            assert_eq!(b.noise_floor_dbm(alone), -80, "at {t} s");
            t += round();
        }
        b.age(secs(gone + half_life()));
        assert_eq!(b.noise_floor_dbm(alone), -95);
        assert_eq!(b.pooled_floor_dbm(), -95);
        b.age(secs(gone + 20 * half_life()));
        assert_eq!(b.noise_floor_dbm(alone), NOMINAL_NOISE_FLOOR_DBM);
        assert_eq!(b.pooled_floor_dbm(), NOMINAL_NOISE_FLOOR_DBM);
    }
}
