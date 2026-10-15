//! Radio physics: every number the simulator takes from the SX1262, in one
//! place, each with its source.
//!
//! Sources: SX1261/2 datasheet Rev 1.2 (June 2019), cited as "DS" with the
//! table or section. Numbers the datasheet does not give are marked as
//! assumptions to be replaced by Phase 2 bench measurements. Every value is
//! a default in [`RadioParams`]; scenarios override them.

use bark_proto::radio::{time_on_air, BANDWIDTH_HZ, SPREADING_FACTOR, SYMBOL_TIME};
use bark_proto::Duration;
use serde::{Deserialize, Serialize};

/// Thermal noise density at 290 K, kTB per hertz, in dBm/Hz.
pub const THERMAL_NOISE_DBM_PER_HZ: f64 = -174.0;

/// DS Table 3-8, `RXS_LB`: sensitivity at SF7, 125 kHz, boosted gain.
pub const SENSITIVITY_SF7_DBM: f64 = -124.0;

/// DS Table 6-1: typical demodulator SNR, by spreading factor 5..=12.
pub const DEMOD_SNR_DB: [f64; 8] = [-2.5, -5.0, -7.5, -10.0, -12.5, -15.0, -17.5, -20.0];

/// Demodulator SNR at our spreading factor (DS Table 6-1: −2.5 dB at SF5).
pub fn demod_snr_db() -> f64 {
    DEMOD_SNR_DB[(SPREADING_FACTOR - 5) as usize]
}

/// Thermal noise in the channel bandwidth, kTB, in dBm (−123.0 at 125 kHz).
pub fn thermal_noise_dbm() -> f64 {
    THERMAL_NOISE_DBM_PER_HZ + 10.0 * (BANDWIDTH_HZ as f64).log10()
}

/// Receiver noise figure implied by the datasheet: the SF7 sensitivity is
/// kTB + NF + SNR(SF7), so NF = −124 − (−123.0) + 7.5 ≈ 6.5 dB. Same
/// receiver chain at every SF, so this carries over to SF5.
pub fn datasheet_noise_figure_db() -> f64 {
    SENSITIVITY_SF7_DBM - thermal_noise_dbm() - DEMOD_SNR_DB[2]
}

/// The datasheet states sensitivity at this packet error rate (DS §3.5
/// conditions: "LoRa PER = 1%, packet 64 bytes, preamble 8 symbols,
/// CR = 4/5, CRC on payload enabled, explicit header mode").
pub const SENSITIVITY_PER: f64 = 0.01;
/// Payload of the datasheet's sensitivity packet, bytes (DS §3.5).
pub const SENSITIVITY_PACKET_BYTES: u8 = 64;
/// Preamble of the datasheet's sensitivity packet, symbols (DS §3.5).
pub const SENSITIVITY_PREAMBLE_SYMBOLS: u16 = 8;

/// Probability that a receiver picks the wrong one of the 2^SF symbols at
/// symbol energy to noise density `es_n0` (linear), in white noise. LoRa
/// symbols are orthogonal and the receiver detects them without phase,
/// so this is the exact error rate of noncoherent detection of M
/// orthogonal signals (Proakis and Salehi, Digital Communications, 5th
/// ed., §4.5): the sum over k of (−1)^(k+1) C(M−1, k) / (k+1) ·
/// exp(−k/(k+1) · Es/N0).
pub fn symbol_error(es_n0: f64) -> f64 {
    // The alternating sum cancels terms up to C(31, 15) / 16 ≈ 2^24 at
    // SF5, which leaves eight of f64's sixteen digits. At SF6 it would
    // leave none.
    const {
        assert!(
            SPREADING_FACTOR <= 5,
            "use a cancellation-free form above SF5"
        )
    };
    let m = 1u32 << SPREADING_FACTOR;
    let (mut binom, mut sum) = (1.0, 0.0);
    for k in 1..m {
        binom *= (m - k) as f64 / k as f64;
        let term = binom / (k + 1) as f64 * (-(k as f64) / (k + 1) as f64 * es_n0).exp();
        sum += if k % 2 == 1 { term } else { -term };
    }
    sum.clamp(0.0, 1.0)
}

/// How a receiver decides whether a packet survives noise and interference.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reception {
    /// Every symbol may be lost, with a probability that falls with its
    /// signal to noise and interference ratio; the packet survives if all
    /// its symbols do (docs/reception-research-2026-10-15.md).
    #[default]
    Curve,
    /// A packet survives if it is at or above sensitivity and above its
    /// interference by the capture threshold, and is lost otherwise.
    Threshold,
}

/// The per-symbol error hazard of a packet, from its signal, noise and
/// interference.
#[derive(Clone, Copy, Debug)]
pub struct ErrorCurve {
    reception: Reception,
    sens_dbm: f64,
    capture_db: f64,
    noise_mw: f64,
    /// Interference counts as noise this many times its power, so that
    /// the capture threshold against interference alone does what the
    /// sensitivity does against noise alone.
    interference_weight: f64,
    /// Es/N0 per unit of signal to effective noise: 2^SF chips per symbol
    /// times the offset that puts the datasheet's sensitivity packet at
    /// its stated error rate at sensitivity.
    gain: f64,
}

impl ErrorCurve {
    /// The curve for `p`.
    pub fn new(p: &RadioParams) -> ErrorCurve {
        let sens_dbm = p.sensitivity_dbm();
        let noise_dbm = p.noise_floor_dbm();
        let demod_snr_db = sens_dbm - noise_dbm;
        // Symbol error rate that loses SENSITIVITY_PER of reference
        // packets, and the Es/N0 that gives it.
        let n_ref = time_on_air(SENSITIVITY_PACKET_BYTES, SENSITIVITY_PREAMBLE_SYMBOLS).as_micros()
            as f64
            / SYMBOL_TIME.as_micros() as f64;
        let p_sym = 1.0 - (1.0 - SENSITIVITY_PER).powf(1.0 / n_ref);
        let (mut lo, mut hi) = (0.0f64, 1e3f64);
        for _ in 0..200 {
            let mid = 0.5 * (lo + hi);
            if symbol_error(mid) > p_sym {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        ErrorCurve {
            reception: p.reception,
            sens_dbm,
            capture_db: p.capture_db,
            noise_mw: mw(noise_dbm),
            interference_weight: mw(p.capture_db - demod_snr_db),
            gain: hi / mw(demod_snr_db),
        }
    }

    /// The offset, in dB, between the datasheet's sensitivity and ideal
    /// noncoherent detection: positive if the datasheet is better.
    pub fn offset_db(&self) -> f64 {
        dbm(self.gain / (1u32 << SPREADING_FACTOR) as f64)
    }

    /// Whether the threshold rule passes a packet at `s_dbm` against
    /// interference `i_mw`.
    pub fn passes_threshold(&self, s_dbm: f64, i_mw: f64) -> bool {
        // Tolerance so that an exact 5.0 dB margin computed through
        // milliwatts counts as 5.0 dB.
        const DB_EPS: f64 = 1e-9;
        s_dbm >= self.sens_dbm && (i_mw <= 0.0 || s_dbm - dbm(i_mw) >= self.capture_db - DB_EPS)
    }

    /// Error hazard per symbol, −ln(1 − symbol error), of a signal at
    /// `s_dbm` against co-channel interference `co_mw` and adjacent
    /// interference `adj_mw` after rejection, with what the loss of the
    /// packet at this hazard is counted as.
    pub fn hazard(&self, s_dbm: f64, co_mw: f64, adj_mw: f64) -> (f64, Loss) {
        let i_mw = co_mw + adj_mw;
        let interference = if adj_mw > co_mw {
            Loss::Adjacent
        } else {
            Loss::CoChannel
        };
        match self.reception {
            Reception::Threshold => {
                let cause = if s_dbm < self.sens_dbm {
                    Loss::Noise
                } else {
                    interference
                };
                let rate = if self.passes_threshold(s_dbm, i_mw) {
                    0.0
                } else {
                    f64::INFINITY
                };
                (rate, cause)
            }
            Reception::Curve => {
                let wi = self.interference_weight * i_mw;
                let cause = if self.noise_mw >= wi {
                    Loss::Noise
                } else {
                    interference
                };
                let es_n0 = self.gain * mw(s_dbm) / (self.noise_mw + wi);
                (-(-symbol_error(es_n0)).ln_1p(), cause)
            }
        }
    }
}

/// What a lost packet is counted as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Loss {
    /// Receiver noise was the larger part of what it faced.
    Noise,
    /// Co-channel interference was.
    CoChannel,
    /// Adjacent-channel leakage was.
    Adjacent,
}

/// Why a receiver could not take a packet it would otherwise have heard.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Deaf {
    /// Synthesiser retune or mode switch into or out of an excursion.
    Retune,
    /// Channel activity detection in progress.
    Cad,
    /// RSSI sample in progress.
    Rssi,
    /// RX→TX or TX→RX turnaround around a transmission.
    Tx,
    /// Own transmission on air (half duplex).
    HalfDuplex,
    /// Demodulating a packet corrupted by a stronger interferer.
    Corrupt,
    /// The node went out of radio reach (dark leaf) mid-packet.
    Absent,
}

impl Deaf {
    /// Number of variants, for counter arrays.
    pub const COUNT: usize = 7;
    /// All variants in counter order.
    pub const ALL: [Deaf; Deaf::COUNT] = [
        Deaf::Retune,
        Deaf::Cad,
        Deaf::Rssi,
        Deaf::Tx,
        Deaf::HalfDuplex,
        Deaf::Corrupt,
        Deaf::Absent,
    ];
    /// Counter index.
    pub fn index(self) -> usize {
        self as usize
    }
    /// Name used in summaries.
    pub fn name(self) -> &'static str {
        [
            "retune",
            "cad",
            "rssi",
            "tx",
            "half_duplex",
            "corrupt",
            "absent",
        ][self as usize]
    }
}

/// One phase of an excursion: how long, and why the node is deaf during it.
pub type Phase = (Duration, Deaf);

/// Radio parameters. Defaults are datasheet values or values derived from
/// them; the ones the datasheet does not give are marked.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RadioParams {
    /// Receiver noise figure in dB. Default derived from DS Table 3-8 and
    /// Table 6-1, see [`datasheet_noise_figure_db`].
    pub noise_figure_db: f64,
    /// Sensitivity in dBm, where one 64-byte packet in a hundred is lost
    /// (DS §3.5). `None`: kTB + NF + SNR(SF5) = −119.0 dBm with the default
    /// NF (DS Table 6-1: −2.5 dB SNR at SF5; SF5 is not in Table 3-8).
    pub sensitivity_dbm: Option<f64>,
    /// Same-SF capture threshold in dB: interference this far below a
    /// packet costs it what noise at sensitivity does, and a second
    /// preamble steals the lock if stronger by this much. Default 5 dB from DS Table 3-8
    /// `CCR_L` at SF7. Caveat: the datasheet measures `CCR_L` with a CW
    /// tone, not a LoRa interferer, and gives no SF5 figure; bench-measure.
    pub capture_db: f64,
    /// Adjacent-channel rejection at ±1 channel (125 kHz = 1.0 × BW) in dB.
    /// Default 60 dB is DS Table 3-8 `ACR_L` at ±1.5 × BW (SF7); 1.0 × BW
    /// is not characterised (bench item C1), so this default is optimistic.
    pub acr1_db: f64,
    /// Adjacent-channel rejection at ±2 channels (2.0 × BW) in dB. Default
    /// 60 dB: at least the ±1.5 × BW figure.
    pub acr2_db: f64,
    /// CAD length in symbols, one of 1, 2, 4, 8, 16 (DS Table 13-72).
    /// Default 8: the value the `lora-phy` driver hard-codes.
    pub cad_symbols: u32,
    /// Probability that CAD misses a LoRa signal above sensitivity.
    /// Assumption: 0 until measured.
    pub cad_miss_probability: f64,
    /// Dwell on a channel before `GetRssiInst` is read, in µs. The
    /// datasheet gives no settle time (DS §13.5.4). Assumption: 300 µs,
    /// the design's figure; measure in Phase 2.
    pub rssi_dwell_us: u64,
    /// DS Table 8-2: RX to FS, µs.
    pub rx_to_fs_us: u64,
    /// DS Table 8-2: FS to RX, µs (also DS Table 3-8 `TS_RX`).
    pub fs_to_rx_us: u64,
    /// DS Table 8-2: RX to TX, µs.
    pub rx_to_tx_us: u64,
    /// DS Table 8-2: STBY_RC to RX, µs. CAD and TX both end in STBY_RC
    /// (DS §13.1.8 SetCAD; the default RxTxFallbackMode), so this is the
    /// cost of returning to listen after either.
    pub stby_to_rx_us: u64,
    /// DS Table 3-7 `TS_HOP`: synthesiser hop time, µs, for a 10 MHz step;
    /// an upper bound for our ≤ 1.5 MHz steps.
    pub synth_hop_us: u64,
    /// Cut-off below sensitivity, in dB, under which a link is not stored.
    /// Default 10 dB: −129 dBm, 12.5 dB under the noise floor, where one
    /// omitted transmitter raises the floor by at most 0.24 dB.
    pub interference_margin_db: f64,
    /// How a packet survives noise and interference. Default: the error
    /// curve.
    pub reception: Reception,
}

impl Default for RadioParams {
    fn default() -> Self {
        RadioParams {
            noise_figure_db: datasheet_noise_figure_db(),
            sensitivity_dbm: None,
            capture_db: 5.0,
            acr1_db: 60.0,
            acr2_db: 60.0,
            cad_symbols: 8,
            cad_miss_probability: 0.0,
            rssi_dwell_us: 300,
            rx_to_fs_us: 15,
            fs_to_rx_us: 41,
            rx_to_tx_us: 92,
            stby_to_rx_us: 83,
            synth_hop_us: 30,
            interference_margin_db: 10.0,
            reception: Reception::Curve,
        }
    }
}

impl RadioParams {
    /// Noise floor in dBm: kTB + NF (≈ −116.5 dBm by default).
    pub fn noise_floor_dbm(&self) -> f64 {
        thermal_noise_dbm() + self.noise_figure_db
    }

    /// Sensitivity in dBm.
    pub fn sensitivity_dbm(&self) -> f64 {
        self.sensitivity_dbm
            .unwrap_or(self.noise_floor_dbm() + demod_snr_db())
    }

    /// Link-storage cut-off in dBm.
    pub fn cutoff_dbm(&self) -> f64 {
        self.sensitivity_dbm() - self.interference_margin_db
    }

    /// Rejection in dB of a signal `offset` channels away, or `None` if it
    /// is too far to matter (beyond ±2 channels).
    pub fn rejection_db(&self, offset: u8) -> Option<f64> {
        match offset {
            0 => Some(0.0),
            1 => Some(self.acr1_db),
            2 => Some(self.acr2_db),
            _ => None,
        }
    }

    /// Retune from RX on one channel to RX on another: RX→FS, hop, FS→RX
    /// (86 µs by default).
    pub fn retune(&self) -> Duration {
        Duration::from_micros(self.rx_to_fs_us + self.synth_hop_us + self.fs_to_rx_us)
    }

    /// CAD sampling time: the configured symbols plus about half a symbol
    /// of post-processing (DS §6.1.5). 2.18 ms at SF5 with 8 symbols.
    pub fn cad_time(&self) -> Duration {
        Duration::from_micros((2 * self.cad_symbols as u64 + 1) * SYMBOL_TIME.as_micros() / 2)
    }

    /// Phases of a CAD on `channel`: leave RX (and hop if off the resting
    /// channel), start the receiver, sample, return from STBY_RC to RX.
    pub fn cad_phases(&self, off_channel: bool) -> [Phase; 3] {
        let hop = if off_channel { self.synth_hop_us } else { 0 };
        [
            (
                Duration::from_micros(self.rx_to_fs_us + hop + self.fs_to_rx_us),
                Deaf::Retune,
            ),
            (self.cad_time(), Deaf::Cad),
            (Duration::from_micros(self.stby_to_rx_us), Deaf::Retune),
        ]
    }

    /// Phases of an RSSI sample: retune there (if off channel), dwell,
    /// retune back. On the resting channel there is no retune, and the
    /// node keeps receiving through the dwell (docs/core-interface.md §3
    /// rule 7): the phases then only time the reading.
    pub fn rssi_phases(&self, off_channel: bool) -> [Phase; 3] {
        let r = if off_channel {
            self.retune()
        } else {
            Duration::ZERO
        };
        [
            (r, Deaf::Retune),
            (Duration::from_micros(self.rssi_dwell_us), Deaf::Rssi),
            (r, Deaf::Retune),
        ]
    }

    /// Phases of a transmission with `airtime` on air: RX→TX (plus a hop if
    /// off the resting channel), airtime, STBY_RC→RX.
    pub fn tx_phases(&self, off_channel: bool, airtime: Duration) -> [Phase; 3] {
        let hop = if off_channel { self.synth_hop_us } else { 0 };
        [
            (Duration::from_micros(self.rx_to_tx_us + hop), Deaf::Tx),
            (airtime, Deaf::HalfDuplex),
            (Duration::from_micros(self.stby_to_rx_us), Deaf::Tx),
        ]
    }
}

/// dBm to milliwatts.
pub fn mw(dbm: f64) -> f64 {
    10f64.powf(dbm / 10.0)
}

/// Milliwatts to dBm.
pub fn dbm(mw: f64) -> f64 {
    10.0 * mw.log10()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bark_proto::radio::{time_on_air, MIN_PREAMBLE_SYMBOLS};

    #[test]
    fn derived_noise_and_sensitivity() {
        let p = RadioParams::default();
        assert!((thermal_noise_dbm() - -123.03).abs() < 0.01);
        assert!((p.noise_figure_db - 6.53).abs() < 0.01);
        assert!((p.noise_floor_dbm() - -116.5).abs() < 0.01);
        assert!(
            (p.sensitivity_dbm() - -119.0).abs() < 1e-9,
            "SF5 sensitivity follows from SF7 and Table 6-1"
        );
    }

    #[test]
    fn symbol_error_matches_its_limits() {
        // No signal: a uniform guess among 32 symbols is right 1 time in 32
        // (to the alternating sum's rounding).
        assert!((symbol_error(0.0) - 31.0 / 32.0).abs() < 1e-7);
        // High SNR: the union bound (M − 1)/2 · exp(−Es/2N0) is tight.
        let g: f64 = 40.0;
        let union = 15.5 * (-g / 2.0).exp();
        assert!((symbol_error(g) / union - 1.0).abs() < 0.02);
        // Falls monotonically.
        let mut last = 1.0;
        for i in 0..400 {
            let p = symbol_error(i as f64 * 0.1);
            assert!(p <= last + 1e-12, "at {}", i as f64 * 0.1);
            last = p;
        }
    }

    #[test]
    fn curve_meets_the_datasheet_at_sensitivity() {
        let p = RadioParams::default();
        let c = ErrorCurve::new(&p);
        let n_ref = time_on_air(64, 8).as_micros() as f64 / 256.0;
        assert_eq!(n_ref, 152.25);
        let per_at = |dbm: f64| 1.0 - (-c.hazard(dbm, 0.0, 0.0).0 * n_ref).exp();
        let s = p.sensitivity_dbm();
        assert!((per_at(s) - SENSITIVITY_PER).abs() < 1e-9);
        // The capture threshold against interference alone gives the same.
        let at_capture = c.hazard(-80.0, mw(-80.0 - p.capture_db), 0.0).0;
        let noise_only = c.hazard(s, 0.0, 0.0).0;
        assert!((at_capture / noise_only - 1.0).abs() < 0.05);
        // Steep: half lost 2 dB below, nearly all 4 dB below, nearly none
        // 1 dB above.
        assert!((per_at(s - 2.0) - 0.51).abs() < 0.01);
        assert!(per_at(s - 4.0) > 0.999);
        assert!(per_at(s + 1.0) < 1e-3);
        // The datasheet sits 1.34 dB better than ideal detection.
        assert!((c.offset_db() - 1.34).abs() < 0.01);
    }

    #[test]
    fn threshold_reception_is_a_step() {
        let p = RadioParams {
            reception: Reception::Threshold,
            ..RadioParams::default()
        };
        let c = ErrorCurve::new(&p);
        let s = p.sensitivity_dbm();
        assert_eq!(c.hazard(s, 0.0, 0.0), (0.0, Loss::CoChannel));
        assert_eq!(c.hazard(s - 0.01, 0.0, 0.0), (f64::INFINITY, Loss::Noise));
        assert_eq!(c.hazard(-100.0, mw(-105.0), 0.0).0, 0.0);
        assert_eq!(
            c.hazard(-100.0, mw(-110.0), mw(-104.9)),
            (f64::INFINITY, Loss::Adjacent)
        );
    }

    #[test]
    fn excursion_costs() {
        let p = RadioParams::default();
        assert_eq!(p.retune().as_micros(), 86);
        assert_eq!(p.cad_time().as_micros(), 8 * 256 + 128);
        let total = |ph: [Phase; 3]| ph.iter().map(|x| x.0.as_micros()).sum::<u64>();
        assert_eq!(total(p.cad_phases(true)), 86 + 2176 + 83);
        assert_eq!(total(p.rssi_phases(false)), 300);
        assert_eq!(total(p.rssi_phases(true)), 86 + 300 + 86);
        let air = time_on_air(8, MIN_PREAMBLE_SYMBOLS);
        assert_eq!(total(p.tx_phases(false, air)), 92 + 11_840 + 83);
    }
}
