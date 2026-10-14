//! Radio physics: every number the simulator takes from the SX1262, in one
//! place, each with its source.
//!
//! Sources: SX1261/2 datasheet Rev 1.2 (June 2019), cited as "DS" with the
//! table or section. Numbers the datasheet does not give are marked as
//! assumptions to be replaced by Phase 2 bench measurements. Every value is
//! a default in [`RadioParams`]; scenarios override them.

use bark_proto::radio::{BANDWIDTH_HZ, SPREADING_FACTOR, SYMBOL_TIME};
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
    /// Sensitivity in dBm. `None`: kTB + NF + SNR(SF5) = −119.0 dBm with the
    /// default NF (DS Table 6-1: −2.5 dB SNR at SF5; SF5 is not in Table 3-8).
    pub sensitivity_dbm: Option<f64>,
    /// Same-SF capture threshold in dB: a packet survives if it exceeds the
    /// sum of its interferers by this much, and a second preamble steals the
    /// lock if stronger by this much. Default 5 dB from DS Table 3-8
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
