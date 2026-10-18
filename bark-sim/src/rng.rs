//! Random streams.
//!
//! A run has one seed. Every consumer of randomness (each core, link
//! shadowing, fading, each leaf's traffic, each interferer, CAD misses) draws
//! from its own stream, derived from `(run seed, purpose, index)`. Adding a
//! draw in one consumer therefore never shifts the numbers another consumer
//! sees, and two runs that differ only in a protocol parameter see the same
//! channel and the same traffic.
//!
//! The generator is SplitMix64, the same as the core's: 64 bits of state,
//! full period, every output bit usable.

/// What a stream is for. The discriminant is mixed into the stream seed, so
/// it is part of the run's reproducibility contract: never renumber.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u64)]
pub enum Purpose {
    /// Seed handed to a protocol core at construction.
    CoreSeed = 1,
    /// Lattice jitter and density placement.
    Placement = 2,
    /// Log-normal shadowing per node pair.
    Shadowing = 3,
    /// Gauss-Markov fading per node pair and channel (counter-based).
    Fading = 4,
    /// Traffic arrivals, payload lengths and destinations, per flow or leaf.
    Traffic = 5,
    /// Interferer on/off process.
    Interferer = 6,
    /// CAD miss draws per node.
    CadMiss = 7,
    /// Leaf availability (dark phone) process.
    Availability = 8,
    /// Boot time spread per node.
    Boot = 9,
    /// Symbol error draws per receiving node.
    Reception = 10,
    /// Node IDs, when drawn at random.
    NodeId = 11,
    /// Each node's clock rate.
    Clock = 12,
    /// Whether a sleepy node walks, and its stops and walks.
    Walk = 13,
}

/// SplitMix64 output function: a bijective 64-bit mixer.
pub const fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Seed of the stream for `(run_seed, purpose, index)`.
pub fn stream_seed(run_seed: u64, purpose: Purpose, index: u64) -> u64 {
    const GOLDEN: u64 = 0x9E37_79B9_7F4A_7C15;
    let a = mix(run_seed ^ (purpose as u64).wrapping_mul(GOLDEN));
    mix(a ^ mix(index.wrapping_add(GOLDEN)))
}

/// One random stream.
#[derive(Clone, Debug)]
pub struct Stream(u64);

impl Stream {
    /// The stream for `(run_seed, purpose, index)`.
    pub fn new(run_seed: u64, purpose: Purpose, index: u64) -> Stream {
        Stream(stream_seed(run_seed, purpose, index))
    }

    /// Next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        mix(self.0)
    }

    /// Uniform in `[0, 1)` with 53 bits.
    pub fn uniform(&mut self) -> f64 {
        unit(self.next_u64())
    }

    /// Uniform integer in `[lo, hi]`.
    pub fn range(&mut self, lo: u64, hi: u64) -> u64 {
        let span = hi - lo + 1;
        lo + ((self.next_u64() as u128 * span as u128) >> 64) as u64
    }

    /// Exponential with the given mean.
    pub fn exponential(&mut self, mean: f64) -> f64 {
        // 1 - u is in (0, 1], so the log is finite.
        -mean * (1.0 - self.uniform()).ln()
    }

    /// Standard normal.
    pub fn normal(&mut self) -> f64 {
        box_muller(self.next_u64(), self.next_u64())
    }

    /// Whether an event of probability `p` happens.
    pub fn chance(&mut self, p: f64) -> bool {
        p > 0.0 && self.uniform() < p
    }
}

/// `[0, 1)` from the top 53 bits.
fn unit(bits: u64) -> f64 {
    (bits >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
}

/// One standard normal from two words (Box-Muller, cosine branch).
fn box_muller(a: u64, b: u64) -> f64 {
    let u1 = 1.0 - unit(a); // (0, 1]
    let u2 = unit(b);
    (-2.0 * u1.ln()).sqrt() * (core::f64::consts::TAU * u2).cos()
}

/// Counter-based standard normal: a pure function of `(seed, a, b)`. Used
/// where the draw must not depend on the order in which values are asked
/// for (fading: pair, channel and tick index).
pub fn normal_at(seed: u64, a: u64, b: u64) -> f64 {
    let k = mix(seed ^ mix(a ^ mix(b)));
    box_muller(mix(k ^ 1), mix(k ^ 2))
}

/// Counter-based approximate standard normal: the sum of four 16-bit
/// uniforms from one hash, centred and scaled to unit variance (Irwin-Hall
/// with n = 4, bounded at ±3.46). Several times cheaper than
/// [`normal_at`]; used for fading innovations, where the process sums many
/// of them and is Gaussian by the central limit theorem regardless.
pub fn normal_fast_at(seed: u64, a: u64, b: u64) -> f64 {
    let k = mix(seed ^ mix(a ^ mix(b)));
    let s = (k & 0xffff) + ((k >> 16) & 0xffff) + ((k >> 32) & 0xffff) + (k >> 48);
    // Each 16-bit part is uniform on {0..65535}: mean 32767.5, variance
    // (65536² − 1) / 12. Four of them: four times both.
    const MEAN: f64 = 4.0 * 32767.5;
    const VAR: f64 = 4.0 * (65536.0 * 65536.0 - 1.0) / 12.0;
    (s as f64 - MEAN) / VAR.sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streams_are_independent_of_each_other() {
        let mut a = Stream::new(7, Purpose::Traffic, 3);
        let mut b = Stream::new(7, Purpose::Traffic, 4);
        let mut c = Stream::new(7, Purpose::Shadowing, 3);
        let (x, y, z) = (a.next_u64(), b.next_u64(), c.next_u64());
        assert!(x != y && y != z && x != z);
        // Same triple, same sequence.
        assert_eq!(Stream::new(7, Purpose::Traffic, 3).next_u64(), x);
    }

    #[test]
    fn fast_normal_has_unit_variance() {
        let n = 200_000u64;
        let (mut sum, mut sq) = (0.0, 0.0);
        for i in 0..n {
            let x = normal_fast_at(5, i, 7);
            sum += x;
            sq += x * x;
        }
        let mean = sum / n as f64;
        assert!(mean.abs() < 0.01, "mean {mean}");
        assert!((sq / n as f64 - mean * mean - 1.0).abs() < 0.01);
    }

    #[test]
    fn normal_has_unit_variance() {
        let mut s = Stream::new(1, Purpose::Fading, 0);
        let n = 200_000;
        let (mut sum, mut sq) = (0.0, 0.0);
        for _ in 0..n {
            let x = s.normal();
            sum += x;
            sq += x * x;
        }
        let mean = sum / n as f64;
        let var = sq / n as f64 - mean * mean;
        assert!(mean.abs() < 0.01, "mean {mean}");
        assert!((var - 1.0).abs() < 0.01, "var {var}");
    }
}
