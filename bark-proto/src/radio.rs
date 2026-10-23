//! The physical layer as the core sees it: channel plan and modulation
//! constants from spec "Physical layer" and "Channel plan". Everything here is a number the radio
//! or the regulator fixes, not a tunable.

use crate::time::Duration;

/// Number of channels in the plan (spec "Channel plan").
pub const CHANNELS: u8 = 13;

/// Centre frequency of channel 0 in Hz (spec "Channel plan": f_k = 433.170 MHz + k × 125 kHz).
pub const CHANNEL_0_HZ: u32 = 433_170_000;

/// Channel spacing in Hz, equal to the signal bandwidth.
pub const CHANNEL_SPACING_HZ: u32 = 125_000;

/// LoRa bandwidth in Hz.
pub const BANDWIDTH_HZ: u32 = 125_000;

/// LoRa spreading factor.
pub const SPREADING_FACTOR: u8 = 5;

/// Coding rate denominator for CR 4/5.
pub const CODING_RATE_DENOMINATOR: u8 = 5;

/// Symbol time: 2^SF / BW = 32 / 125 000 s = 256 µs.
pub const SYMBOL_TIME: Duration =
    Duration::from_micros((1u64 << SPREADING_FACTOR) * 1_000_000 / BANDWIDTH_HZ as u64);

/// Minimum preamble the SX126x accepts at SF5 and SF6 (datasheet §6.1.4:
/// "for SF5 and SF6 the minimum preamble length is 12 symbols").
pub const MIN_PREAMBLE_SYMBOLS: u16 = 12;

/// Maximum ERP in dBm (ERC/REC 70-03 Annex 1 band g1, 10 mW; spec "Regulatory limits").
pub const MAX_ERP_DBM: i8 = 10;

/// Duty-cycle cap as a fraction of the budget window (band g1: 10 %).
pub const DUTY_CYCLE_PERCENT: u8 = 10;

/// Duty-cycle budget window. ERC/REC 70-03 defines duty cycle over one hour.
pub const DUTY_CYCLE_WINDOW: Duration = Duration::from_secs(3600);

/// A channel index in `0..CHANNELS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Channel(u8);

impl Channel {
    /// Construct from an index, or `None` if out of the plan.
    pub const fn new(index: u8) -> Option<Channel> {
        if index < CHANNELS {
            Some(Channel(index))
        } else {
            None
        }
    }

    /// Index in `0..CHANNELS`.
    pub const fn index(self) -> u8 {
        self.0
    }

    /// Centre frequency in Hz.
    pub const fn hz(self) -> u32 {
        CHANNEL_0_HZ + self.0 as u32 * CHANNEL_SPACING_HZ
    }

    /// The next channel in the heartbeat rotation, `(k + 1) mod 13` (spec "Heartbeat").
    pub const fn next(self) -> Channel {
        Channel((self.0 + 1) % CHANNELS)
    }

    /// All channels in index order.
    pub fn all() -> impl Iterator<Item = Channel> {
        (0..CHANNELS).map(Channel)
    }
}

/// Sync word and start-of-frame delimiter after the preamble at SF5 and
/// SF6, in quarter symbols (datasheet §6.1.4: 6.25 symbols).
const SYNC_QUARTER_SYMBOLS: u64 = 25;

/// Symbols of the first block after the sync, sent at coding rate 4/8. In
/// explicit header mode it carries the header and its checksum (datasheet
/// §6.1.4: the `8` term of the time-on-air formula).
const HEADER_BLOCK_SYMBOLS: u64 = 8;

/// Time from a packet's start to the end of its header block, after which
/// the receiver knows the header is sound and the packet's length.
pub const fn header_end(preamble_symbols: u16) -> Duration {
    let quarter_symbols =
        4 * preamble_symbols as u64 + SYNC_QUARTER_SYMBOLS + 4 * HEADER_BLOCK_SYMBOLS;
    Duration::from_micros(quarter_symbols * SYMBOL_TIME.as_micros() / 4)
}

/// Time on air of a LoRa packet with `payload_len` bytes and
/// `preamble_symbols` preamble symbols, from the SX1261/2 datasheet §6.1.4,
/// for SF5/SF6 (header on, CRC on, low data rate optimisation off):
///
/// ```text
/// n_symbols = preamble + 6.25 + 8 + ceil(max(8·len + 16 − 4·SF + 8, 0) / (4·SF)) · CR_denominator
/// ```
///
/// The 6.25-symbol term replaces the 4.25 of SF7+ and the `+8` inside the
/// `max` is the SF5/SF6 header correction, both per datasheet. Result in µs.
pub const fn time_on_air(payload_len: u8, preamble_symbols: u16) -> Duration {
    let sf = SPREADING_FACTOR as i64;
    let bits = 8 * payload_len as i64 + 16 - 4 * sf + 8;
    let bits = if bits < 0 { 0 } else { bits };
    let payload_symbols = (bits + 4 * sf - 1) / (4 * sf) * CODING_RATE_DENOMINATOR as i64;
    let payload = SYMBOL_TIME.mul(payload_symbols as u64);
    Duration::from_micros(header_end(preamble_symbols).as_micros() + payload.as_micros())
}

/// Signal to noise ratio at sensitivity at SF5 (datasheet Table 6-1:
/// −2.5 dB). The datasheet states sensitivity as the level where a 64-byte
/// packet is lost 1 % of the time (§3.5).
pub const DEMOD_SNR_DB: f32 = -2.5;

/// How far above sensitivity a 255-byte frame must be received to be lost
/// no more than 1 % of the time, the datasheet's own standard (§3.5). A
/// packet arrives only if every symbol does, and longer packets have more
/// symbols; the symbol error rate is that of noncoherent detection of 32
/// orthogonal signals, anchored at the datasheet's 64-byte point
/// (`docs/reception-research-2026-10-15.md`). Derived in the tests below.
pub const FULL_FRAME_ABOVE_SENSITIVITY_DB: f32 = 0.44;

/// The fade a link must ride out: the standard deviation of fading over
/// time and channel in the scenarios, 4 dB (`docs/roadmap.md` D38). Not
/// measured; to be replaced by the bench and field measurements of Phase 2.
pub const FADE_DEPTH_DB: f32 = 4.0;

/// The packet RSSI, in dB above the noise floor, at which a frame of full
/// length still arrives 99 times in 100 through a fade of
/// [`FADE_DEPTH_DB`]: about 4.1 dB. The radio's packet RSSI is the power
/// in the channel, signal plus noise (datasheet §13.5.3, RssiPkt), so the
/// signal's level above the floor, 1.9 dB, reads higher.
pub fn full_frame_rssi_above_noise_db() -> f32 {
    let signal = DEMOD_SNR_DB + FULL_FRAME_ABOVE_SENSITIVITY_DB + FADE_DEPTH_DB;
    10.0 * libm::log10f(1.0 + libm::powf(10.0, signal / 10.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_time_is_256_us() {
        assert_eq!(SYMBOL_TIME.as_micros(), 256);
    }

    #[test]
    fn channel_plan_matches_design() {
        assert_eq!(Channel::new(0).unwrap().hz(), 433_170_000);
        assert_eq!(Channel::new(12).unwrap().hz(), 434_670_000);
        assert!(Channel::new(13).is_none());
        assert_eq!(Channel::new(12).unwrap().next().index(), 0);
    }

    #[test]
    fn heartbeat_airtime() {
        // 11-byte heartbeat, 12-symbol preamble. Payload bits:
        // 8·11 + 16 − 4·5 + 8 = 92 → ceil(92 / 20) = 5 → ×5 = 25 symbols.
        // Total 12 + 6.25 + 8 + 25 = 51.25 symbols × 256 µs = 13.12 ms,
        // the 13.1 ms of spec "Heartbeat".
        let t = time_on_air(crate::wire::HEARTBEAT_LEN as u8, MIN_PREAMBLE_SYMBOLS);
        assert_eq!(t.as_micros(), 13_120);
    }

    /// Symbol error of noncoherent detection of M = 32 orthogonal signals
    /// at symbol energy to noise density `es_n0` (Proakis and Salehi,
    /// Digital Communications, 5th ed., §4.5).
    fn symbol_error(es_n0: f64) -> f64 {
        let m = 1u32 << SPREADING_FACTOR;
        let (mut binom, mut sum) = (1.0, 0.0);
        for k in 1..m {
            binom *= (m - k) as f64 / k as f64;
            let term = binom / (k + 1) as f64 * libm::exp(-(k as f64) / (k + 1) as f64 * es_n0);
            sum += if k % 2 == 1 { term } else { -term };
        }
        sum
    }

    /// Es/N0 at which a packet of `len` bytes with `preamble` symbols is
    /// lost with probability `per`: every symbol must arrive.
    fn es_n0_for(len: u8, preamble: u16, per: f64) -> f64 {
        let n = time_on_air(len, preamble).as_micros() as f64 / SYMBOL_TIME.as_micros() as f64;
        let p_sym = 1.0 - libm::pow(1.0 - per, 1.0 / n);
        let (mut lo, mut hi) = (0.0f64, 1e3f64);
        for _ in 0..200 {
            let mid = 0.5 * (lo + hi);
            if symbol_error(mid) > p_sym {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        hi
    }

    #[test]
    fn full_frame_level_follows_the_symbol_error_rate() {
        // The datasheet's sensitivity packet: 64 bytes, 8-symbol preamble,
        // 1 % lost (§3.5). Its Es/N0 is sensitivity.
        let sens = es_n0_for(64, 8, 0.01);
        let above = |len: u8, per: f64| {
            10.0 * libm::log10(es_n0_for(len, MIN_PREAMBLE_SYMBOLS, per) / sens)
        };
        let full = above(255, 0.01);
        assert!(
            (full - FULL_FRAME_ABOVE_SENSITIVITY_DB as f64).abs() < 0.005,
            "{full}"
        );
        // An 11-byte heartbeat is lost 1 % of the time 0.86 dB lower, and
        // half the time 3.16 dB lower.
        let hb = crate::wire::HEARTBEAT_LEN as u8;
        assert!((full - above(hb, 0.01) - 0.86).abs() < 0.01);
        assert!((full - above(hb, 0.5) - 3.16).abs() < 0.01);
        let rssi = full_frame_rssi_above_noise_db();
        assert!((rssi - 4.09).abs() < 0.01, "{rssi}");
    }
}
