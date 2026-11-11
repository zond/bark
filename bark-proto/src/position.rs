//! WGS-84 position in the 7-byte fixed-point wire form, and its 4-byte
//! short form (spec "Positions on the wire"). Latitude (25 bits) and
//! longitude (26 bits) share one unit, 180° / 2²⁵ = 360° / 2²⁶.

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

    /// Latitude and longitude in millionths of a degree, rounded, in
    /// integers alone, so firmware prints a position without float
    /// formatting. A count is 5.36 µdeg, so six decimals tell counts apart.
    pub fn micro_degrees(self) -> (i64, i64) {
        // 180 · 10⁶ / 2²⁵ µdeg a count, rounded half away from zero.
        let udeg = |c: i32| {
            let num = c as i64 * 180_000_000;
            let half = 1i64 << (LAT_BITS - 1);
            if num >= 0 {
                (num + half) >> LAT_BITS
            } else {
                -((-num + half) >> LAT_BITS)
            }
        };
        (udeg(self.lat), udeg(self.lon))
    }

    /// Encode: 56-bit big-endian value, latitude in the top 25 bits,
    /// longitude in the next 26, five spare bits zero.
    /// (Bit layout is this implementation's choice; the spec leaves it open.)
    pub fn encode(self) -> [u8; Self::WIRE_LEN] {
        let lat = (self.lat as u64) & ((1 << LAT_BITS) - 1);
        let lon = (self.lon as u64) & ((1 << LON_BITS) - 1);
        let v = (lat << (LON_BITS + 5)) | (lon << 5);
        let b = v.to_be_bytes();
        [b[1], b[2], b[3], b[4], b[5], b[6], b[7]]
    }

    /// Decode the wire form. Spare bits are ignored.
    pub fn decode(bytes: &[u8; Self::WIRE_LEN]) -> Position {
        let v = u64::from_be_bytes([
            0, bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6],
        ]);
        let lat = ((v >> (LON_BITS + 5)) & ((1 << LAT_BITS) - 1)) as u32;
        let lon = ((v >> 5) & ((1 << LON_BITS) - 1)) as u32;
        Position {
            lat: sign_extend(lat, LAT_BITS),
            lon: sign_extend(lon, LON_BITS),
        }
    }

    /// This position's short name; see [`ShortName`].
    pub fn short(self) -> ShortName {
        ShortName(((self.lat as u32 & SHORT_MASK) << 16) | (self.lon as u32 & SHORT_MASK))
    }

    /// The position named `short` nearest to `near`: on each axis the one
    /// candidate within [`SHORT_PERIOD`] / 2 counts. Both axes wrap as their
    /// wire fields do; a name that wraps latitude past a pole is garbage
    /// and rebuilds near the other pole, where no table holds a relay.
    pub fn from_short(short: ShortName, near: Position) -> Position {
        // The wrapping i16 difference is the signed offset within half a period.
        let dlat = ((short.0 >> 16) as u16).wrapping_sub(near.lat as u16) as i16;
        let dlon = (short.0 as u16).wrapping_sub(near.lon as u16) as i16;
        Position {
            lat: wrap(near.lat + dlat as i32, LAT_BITS),
            lon: wrap(near.lon + dlon as i32, LON_BITS),
        }
    }
}

const SHORT_MASK: u32 = 0xffff;

/// Counts after which a short name repeats on each axis: 2¹⁶.
pub const SHORT_PERIOD: i32 = 1 << 16;

/// A position's 4-byte short form, which the heartbeat carries (spec
/// "Positions on the wire"): the low 16 bits of each count. It is
/// unambiguous within the ≈3 km a node hears up to about 80° N or S, where
/// the east-west half period shrinks to 3.4 km.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct ShortName(pub u32);

/// Degrees to six decimals, latitude then longitude
/// ([`Position::micro_degrees`]).
impl core::fmt::Display for Position {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let (lat, lon) = self.micro_degrees();
        let deg = |f: &mut core::fmt::Formatter<'_>, u: i64| {
            let sign = if u < 0 { "-" } else { "" };
            let u = u.unsigned_abs();
            write!(f, "{sign}{}.{:06}", u / 1_000_000, u % 1_000_000)
        };
        deg(f, lat)?;
        f.write_str(" ")?;
        deg(f, lon)
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

/// `v` wrapped into the signed range of a `bits`-wide field.
fn wrap(v: i32, bits: u32) -> i32 {
    sign_extend(v as u32 & ((1 << bits) - 1), bits)
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;

    const _: () = assert!(UNIT_M > 0.596 && UNIT_M < 0.597, "one count is about 0.6 m");

    #[test]
    fn micro_degrees_match_the_degrees() {
        for (lat, lon) in [
            (50.4501, 30.5234),
            (-33.8688, -151.2093),
            (0.0, 0.0),
            (89.9, -179.9),
        ] {
            let p = Position::from_degrees(lat, lon);
            let (a, b) = p.micro_degrees();
            assert_eq!(a, (p.lat_deg() * 1e6).round() as i64);
            assert_eq!(b, (p.lon_deg() * 1e6).round() as i64);
        }
        let p = Position::from_degrees(-0.25, 30.5);
        let want = std::format!("{:.6} {:.6}", p.lat_deg(), p.lon_deg());
        assert_eq!(std::format!("{p}"), want);
    }

    #[test]
    fn round_trips_and_clamps() {
        for &(lat, lon) in &[
            (0.0, 0.0),
            (50.45, 30.52),
            (-89.9, 179.9),
            (89.9, -179.9),
            (-90.0, -180.0),
        ] {
            let p = Position::from_degrees(lat, lon);
            assert_eq!(Position::decode(&p.encode()), p);
            assert!(p.lat_deg() - lat < UNIT_DEG && lat - p.lat_deg() < UNIT_DEG);
            assert!(p.lon_deg() - lon < UNIT_DEG && lon - p.lon_deg() < UNIT_DEG);
        }
        let spare = Position::from_degrees(1.0, 2.0).encode();
        assert_eq!(spare[6] & 0x1f, 0, "spare bits must be zero");
    }

    #[test]
    fn short_names() {
        let p = Position {
            lat: 0x0012_3456,
            lon: -0x0001_0002,
        };
        assert_eq!(p.short(), ShortName(0x3456_fffe));
        assert_eq!(Position::from_short(p.short(), p), p);
        // A name half a period away in each direction.
        let half = SHORT_PERIOD / 2;
        let q = Position {
            lat: p.lat + half - 1,
            lon: p.lon - half,
        };
        assert_eq!(Position::from_short(q.short(), p), q);
        // Across the antimeridian, both ways.
        let east = Position {
            lat: 0,
            lon: (1 << 25) - 10,
        };
        let west = Position {
            lat: 0,
            lon: -(1 << 25) + 10,
        };
        assert_eq!(Position::from_short(west.short(), east), west);
        assert_eq!(Position::from_short(east.short(), west), east);
        // Beyond a pole a name rebuilds near the other one.
        let pole = Position {
            lat: (1 << 24) - 10,
            lon: 0,
        };
        let garbage = Position::from_short(ShortName(20 << 16), pole);
        assert_eq!(
            garbage,
            Position {
                lat: -(1 << 24) + 20,
                lon: 0
            }
        );
    }

    /// `near` moved `north` and `east` metres in its local plane, wrapped
    /// across the antimeridian.
    fn offset(near: Position, north: f64, east: f64) -> Position {
        let cos = libm::cos(near.lat_deg().to_radians());
        let lat = near.lat + round(north / UNIT_M) as i32;
        let lon = near.lon + round(east / (UNIT_M * cos)) as i32;
        Position {
            lat,
            lon: wrap(lon, LON_BITS),
        }
    }

    fn lat_within(deg: f64) -> core::ops::Range<i32> {
        let c = (deg / UNIT_DEG) as i32;
        -c..c
    }

    proptest::proptest! {
        #[test]
        fn short_round_trips_within_half_a_period(
            lat in -(1i32 << 24)..(1 << 24),
            lon in -(1i32 << 25)..(1 << 25),
            dlat in -(SHORT_PERIOD / 2)..(SHORT_PERIOD / 2),
            dlon in -(SHORT_PERIOD / 2)..(SHORT_PERIOD / 2),
        ) {
            let p = Position { lat, lon };
            let near = Position { lat: (lat - dlat).clamp(-(1 << 24), (1 << 24) - 1), lon: wrap(lon - dlon, LON_BITS) };
            proptest::prop_assert_eq!(Position::from_short(p.short(), near), p);
        }

        /// 15 km on every bearing fits half an east-west period up to
        /// 39° N or S.
        #[test]
        fn short_round_trips_within_15_km_to_35_degrees(
            lat in lat_within(35.0),
            lon in -(1i32 << 25)..(1 << 25),
            antimeridian: bool,
            bearing in 0.0..core::f64::consts::TAU,
            r in 0.0..15_000.0f64,
        ) {
            let lon = if antimeridian { wrap((1 << 25) + lon % 50_000, LON_BITS) } else { lon };
            let near = Position { lat, lon };
            let p = offset(near, r * libm::cos(bearing), r * libm::sin(bearing));
            proptest::prop_assert_eq!(Position::from_short(p.short(), near), p);
        }

        /// The hearing radius, ≈3 km (spec "Capacity and sizing"), fits up to 80° N or S.
        #[test]
        fn short_round_trips_within_3_km_to_80_degrees(
            lat in lat_within(80.0),
            lon in -(1i32 << 25)..(1 << 25),
            antimeridian: bool,
            bearing in 0.0..core::f64::consts::TAU,
            r in 0.0..3_000.0f64,
        ) {
            let lon = if antimeridian { wrap((1 << 25) + lon % 50_000, LON_BITS) } else { lon };
            let near = Position { lat, lon };
            let p = offset(near, r * libm::cos(bearing), r * libm::sin(bearing));
            proptest::prop_assert_eq!(Position::from_short(p.short(), near), p);
        }
    }
}
