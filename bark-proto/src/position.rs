//! Global WGS-84 position in the 7-byte fixed-point wire form (design §7, §11).
//!
//! Latitude is 25 bits and longitude 26 bits of two's-complement fixed point.
//! Both use the same angular unit, 180° / 2²⁵ = 360° / 2²⁶ ≈ 5.36 µdeg,
//! which is ≈0.6 m along a meridian. A relay's position is its identity
//! (design §8.1), so equality here is identity equality.

/// Angular unit of one count, in degrees: 180 / 2²⁵.
pub const UNIT_DEG: f64 = 180.0 / (1u64 << 25) as f64;

/// Metres per count along a meridian: R · π / 2²⁵ with R = 6 371 000 m
/// (mean Earth radius). ≈ 0.5965 m.
pub const UNIT_M: f64 = 6_371_000.0 * core::f64::consts::PI / (1u64 << 25) as f64;

/// Latitude range: 25 bits signed.
const LAT_BITS: u32 = 25;
/// Longitude range: 26 bits signed.
const LON_BITS: u32 = 26;

/// A position: latitude and longitude in counts of [`UNIT_DEG`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Position {
    /// Latitude in `[-2²⁴, 2²⁴)` counts; `-2²⁴` is −90°.
    pub lat: i32,
    /// Longitude in `[-2²⁵, 2²⁵)` counts; `-2²⁵` is −180°.
    pub lon: i32,
}

impl Position {
    /// Wire size in bytes.
    pub const WIRE_LEN: usize = 7;

    /// Build from degrees, rounding to the nearest count and clamping to range.
    pub fn from_degrees(lat_deg: f64, lon_deg: f64) -> Position {
        let lat = round(lat_deg / UNIT_DEG);
        let lon = round(lon_deg / UNIT_DEG);
        let lat_max = (1i64 << (LAT_BITS - 1)) - 1;
        let lon_max = (1i64 << (LON_BITS - 1)) - 1;
        Position {
            lat: lat.clamp(-lat_max - 1, lat_max) as i32,
            lon: lon.clamp(-lon_max - 1, lon_max) as i32,
        }
    }

    /// Latitude in degrees.
    pub fn lat_deg(self) -> f64 {
        self.lat as f64 * UNIT_DEG
    }

    /// Longitude in degrees.
    pub fn lon_deg(self) -> f64 {
        self.lon as f64 * UNIT_DEG
    }

    /// Encode: 56-bit big-endian value, latitude in the top 25 bits,
    /// longitude in the next 26, five spare bits zero.
    /// (Bit layout is this implementation's choice; design O2 leaves it open.)
    pub fn encode(self) -> [u8; Self::WIRE_LEN] {
        let lat = (self.lat as u64) & ((1 << LAT_BITS) - 1);
        let lon = (self.lon as u64) & ((1 << LON_BITS) - 1);
        let v = (lat << (LON_BITS + 5)) | (lon << 5);
        let b = v.to_be_bytes();
        [b[1], b[2], b[3], b[4], b[5], b[6], b[7]]
    }

    /// Decode the wire form. Spare bits are ignored.
    pub fn decode(bytes: &[u8; Self::WIRE_LEN]) -> Position {
        let v = u64::from_be_bytes([0, bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6]]);
        let lat = ((v >> (LON_BITS + 5)) & ((1 << LAT_BITS) - 1)) as u32;
        let lon = ((v >> 5) & ((1 << LON_BITS) - 1)) as u32;
        Position {
            lat: sign_extend(lat, LAT_BITS),
            lon: sign_extend(lon, LON_BITS),
        }
    }

    /// 16-bit hash used in DFS visited lists (design §8.1, §14).
    /// Definition is part of the wire format; fixed in Phase 0 by the core.
    pub fn hash16(self) -> u16 {
        // FNV-1a over the 7 wire bytes, folded to 16 bits.
        let mut h: u32 = 0x811c_9dc5;
        for b in self.encode() {
            h ^= b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        ((h >> 16) ^ (h & 0xffff)) as u16
    }
}

/// Round half away from zero without `std`.
fn round(x: f64) -> i64 {
    if x >= 0.0 {
        (x + 0.5) as i64
    } else {
        (x - 0.5) as i64
    }
}

fn sign_extend(v: u32, bits: u32) -> i32 {
    let shift = 32 - bits;
    ((v << shift) as i32) >> shift
}

#[cfg(test)]
mod tests {
    use super::*;

    const _: () = assert!(UNIT_M > 0.596 && UNIT_M < 0.597, "one count is about 0.6 m");

    #[test]
    fn round_trips_and_clamps() {
        for &(lat, lon) in &[(0.0, 0.0), (50.45, 30.52), (-89.9, 179.9), (89.9, -179.9), (-90.0, -180.0)] {
            let p = Position::from_degrees(lat, lon);
            assert_eq!(Position::decode(&p.encode()), p);
            assert!(p.lat_deg() - lat < UNIT_DEG && lat - p.lat_deg() < UNIT_DEG);
            assert!(p.lon_deg() - lon < UNIT_DEG && lon - p.lon_deg() < UNIT_DEG);
        }
        let spare = Position::from_degrees(1.0, 2.0).encode();
        assert_eq!(spare[6] & 0x1f, 0, "spare bits must be zero");
    }
}
