//! The physical layer as the core sees it: channel plan and modulation
//! constants from design §4 and §5. Everything here is a number the radio
//! or the regulator fixes, not a tunable.

use crate::time::Duration;

/// Number of channels in the plan (design §5).
pub const CHANNELS: u8 = 13;

/// Centre frequency of channel 0 in Hz (design §5: f_k = 433.170 MHz + k × 125 kHz).
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

/// Maximum ERP in dBm (ERC/REC 70-03 Annex 1 band g1, 10 mW; design §3).
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

    /// The next channel in the heartbeat rotation, `(k + 1) mod 13` (design §7).
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
        // 8-byte heartbeat, 12-symbol preamble. Payload bits:
        // 8·8 + 16 − 4·5 + 8 = 68 → ceil(68 / 20) = 4 → ×5 = 20 symbols.
        // Total 12 + 6.25 + 8 + 20 = 46.25 symbols × 256 µs = 11.84 ms,
        // which is the "≈12 ms" of design §7.
        let t = time_on_air(8, MIN_PREAMBLE_SYMBOLS);
        assert_eq!(t.as_micros(), 11_840);
    }
}
