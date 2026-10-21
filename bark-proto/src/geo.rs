//! Distances in a local tangent plane centred on this node (spec "Positions on the wire").
//!
//! Routing compares distances; it never needs global coordinates. Each node
//! builds one [`LocalPlane`] from its own position and maps every position
//! it handles to metres east and north of itself:
//!
//! ```text
//! north = Δlat · R
//! east  = Δlon · R · cos(lat_self)
//! ```
//!
//! with `R` the mean Earth radius of [`crate::position::UNIT_M`] and
//! `cos(lat_self)` computed once, when the plane is built. A conversion is
//! two integer subtractions and two `f32` multiplications.
//!
//! # Accuracy
//!
//! `north` is the exact meridian arc. `east` uses the cosine of the plane's
//! own latitude for every point, so a point `Δφ` radians north or south of
//! the centre has its east offset off by a relative `≈ tan(lat_self) · Δφ`.
//! Measured against the haversine distance at 50.45° N (the tests):
//!
//! | radius from centre | centre to point | point to point |
//! |---|---|---|
//! | 5 km | 0.02 % | 0.09 % |
//! | 20 km | 0.07 % | 0.37 % |
//! | 100 km | 0.37 % | 1.9 % |
//! | 300 km | 1.1 % | 5.9 % |
//!
//! So the 0.1 % holds for a city-sized neighbourhood, the 5 km of spec
//! "Positions on the wire", not out to hundreds of km; a hop is never more than a few km,
//! and for a distant destination the error is a smooth distortion that
//! changes only near-tied greedy choices.
//!
//! # Far away
//!
//! Longitude differences wrap to `[-180°, 180°)`, so the antimeridian is not
//! a cliff. Every point maps to `|north| ≤ π R ≈ 20 015 km` and
//! `|east| ≤ π R cos(lat_self)`: finite, never NaN, and monotone in the
//! angular offsets, but not a distance on the sphere. The antipode of the
//! centre lands at the edge of that box, about 20 000 km away; beyond a few
//! hundred km the numbers are an ordering hint, nothing more. At a pole
//! `cos(lat_self)` is zero and every east offset collapses; there are no
//! relays there.

use crate::position::{Position, UNIT_DEG, UNIT_M};

/// Longitude counts in a full turn: 2²⁶ (`position.rs`: 26-bit longitude).
const LON_TURN: i64 = 1 << 26;

/// A point in a local plane, metres east and north of its centre.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Point {
    /// Metres east.
    pub east: f32,
    /// Metres north.
    pub north: f32,
}

impl Point {
    /// Squared distance in m², for comparisons.
    pub fn distance2(self, other: Point) -> f32 {
        let de = self.east - other.east;
        let dn = self.north - other.north;
        de * de + dn * dn
    }

    /// Distance in metres.
    pub fn distance_m(self, other: Point) -> f32 {
        libm::sqrtf(self.distance2(other))
    }
}

/// The tangent plane at one position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LocalPlane {
    centre: Position,
    /// Metres per latitude count.
    north_per_count: f32,
    /// Metres per longitude count at the centre's latitude.
    east_per_count: f32,
}

impl LocalPlane {
    /// The plane at latitude and longitude 0, where a longitude count is as
    /// long as a latitude count: what [`LocalPlane::new`] gives there, as a
    /// constant.
    pub const ORIGIN: LocalPlane = LocalPlane {
        centre: Position { lat: 0, lon: 0 },
        north_per_count: UNIT_M as f32,
        east_per_count: UNIT_M as f32,
    };

    /// The plane centred on `centre`.
    pub fn new(centre: Position) -> LocalPlane {
        let lat_rad = centre.lat as f64 * UNIT_DEG * (core::f64::consts::PI / 180.0);
        LocalPlane {
            centre,
            north_per_count: UNIT_M as f32,
            east_per_count: (UNIT_M * libm::cos(lat_rad)) as f32,
        }
    }

    /// The centre.
    pub fn centre(&self) -> Position {
        self.centre
    }

    /// `p` in this plane.
    pub fn point(&self, p: Position) -> Point {
        let dlat = p.lat as i64 - self.centre.lat as i64;
        let mut dlon = p.lon as i64 - self.centre.lon as i64;
        if dlon >= LON_TURN / 2 {
            dlon -= LON_TURN;
        } else if dlon < -LON_TURN / 2 {
            dlon += LON_TURN;
        }
        Point {
            east: dlon as f32 * self.east_per_count,
            north: dlat as f32 * self.north_per_count,
        }
    }

    /// Squared distance between two positions in this plane, in m².
    pub fn distance2(&self, a: Position, b: Position) -> f32 {
        self.point(a).distance2(self.point(b))
    }

    /// Distance between two positions in this plane, in metres.
    pub fn distance_m(&self, a: Position, b: Position) -> f32 {
        self.point(a).distance_m(self.point(b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mean Earth radius, the one `UNIT_M` is built from.
    const R: f64 = 6_371_000.0;
    const KYIV: (f64, f64) = (50.45, 30.52);

    fn rad(d: f64) -> f64 {
        d * core::f64::consts::PI / 180.0
    }

    fn deg(r: f64) -> f64 {
        r * 180.0 / core::f64::consts::PI
    }

    fn haversine(a: (f64, f64), b: (f64, f64)) -> f64 {
        let (la1, lo1, la2, lo2) = (rad(a.0), rad(a.1), rad(b.0), rad(b.1));
        let h = libm::pow(libm::sin((la2 - la1) / 2.0), 2.0)
            + libm::cos(la1) * libm::cos(la2) * libm::pow(libm::sin((lo2 - lo1) / 2.0), 2.0);
        2.0 * R * libm::asin(libm::sqrt(h))
    }

    /// The point `d` metres from `o` on initial bearing `brg` degrees.
    fn destination(o: (f64, f64), d: f64, brg: f64) -> (f64, f64) {
        let (la, lo, b, dd) = (rad(o.0), rad(o.1), rad(brg), d / R);
        let la2 = libm::asin(
            libm::sin(la) * libm::cos(dd) + libm::cos(la) * libm::sin(dd) * libm::cos(b),
        );
        let lo2 = lo
            + libm::atan2(
                libm::sin(b) * libm::sin(dd) * libm::cos(la),
                libm::cos(dd) - libm::sin(la) * libm::sin(la2),
            );
        (deg(la2), deg(lo2))
    }

    fn pos(p: (f64, f64)) -> Position {
        Position::from_degrees(p.0, p.1)
    }

    /// Worst relative error, centre to a ring of points at `d` metres.
    fn worst_from_centre(d: f64) -> f64 {
        let plane = LocalPlane::new(pos(KYIV));
        (0..72)
            .map(|i| {
                let p = destination(KYIV, d, i as f64 * 5.0);
                let truth = haversine(pos(KYIV).degrees(), pos(p).degrees());
                (plane.distance_m(pos(KYIV), pos(p)) as f64 - truth).abs() / truth
            })
            .fold(0.0, f64::max)
    }

    /// Worst relative error between pairs of points on a ring of radius `d`.
    fn worst_between(d: f64) -> f64 {
        let plane = LocalPlane::new(pos(KYIV));
        let ring: [(f64, f64); 24] =
            core::array::from_fn(|i| destination(KYIV, d, i as f64 * 15.0));
        let mut worst: f64 = 0.0;
        for a in ring {
            for b in ring {
                let truth = haversine(pos(a).degrees(), pos(b).degrees());
                if truth > 1.0 {
                    worst =
                        worst.max((plane.distance_m(pos(a), pos(b)) as f64 - truth).abs() / truth);
                }
            }
        }
        worst
    }

    impl Position {
        fn degrees(self) -> (f64, f64) {
            (self.lat_deg(), self.lon_deg())
        }
    }

    #[test]
    fn city_scale_within_a_tenth_of_a_percent() {
        for d in [300.0, 1_000.0, 3_000.0, 10_000.0] {
            assert!(
                worst_from_centre(d) < 0.001,
                "{d} m from centre: {}",
                worst_from_centre(d)
            );
        }
        // Two points anywhere within 2.5 km of the centre, so up to 5 km apart.
        for d in [500.0, 1_000.0, 2_500.0] {
            assert!(worst_between(d) < 0.001, "ring {d} m: {}", worst_between(d));
        }
    }

    #[test]
    fn error_grows_as_documented() {
        // The table in the module documentation.
        let rows = [
            (5e3, 0.0002, 0.0009),
            (20e3, 0.0007, 0.0037),
            (100e3, 0.0037, 0.019),
            (300e3, 0.0114, 0.0594),
        ];
        for (d, centre, between) in rows {
            let (c, b) = (worst_from_centre(d), worst_between(d));
            assert!(
                (c - centre).abs() < 0.0001 + centre * 0.05,
                "{d}: centre {c}"
            );
            assert!(
                (b - between).abs() < 0.0001 + between * 0.05,
                "{d}: between {b}"
            );
        }
    }

    #[test]
    fn axes_and_wrap() {
        let plane = LocalPlane::new(Position::from_degrees(0.0, 179.99));
        // 0.02° east across the antimeridian is about 2.2 km east, not 40 000 km west.
        let p = plane.point(Position::from_degrees(0.0, -179.99));
        assert!((p.east - 2_224.0).abs() < 2.0 && p.north == 0.0, "{p:?}");
        let n = plane.point(Position::from_degrees(0.01, 179.99));
        assert!((n.north - 1_112.0).abs() < 1.0 && n.east == 0.0, "{n:?}");
        assert_eq!(plane.point(plane.centre()), Point::default());
    }

    #[test]
    fn antipode_is_finite_and_bounded() {
        let plane = LocalPlane::new(pos(KYIV));
        let half_turn = core::f64::consts::PI * R;
        let east_max = half_turn * libm::cos(rad(KYIV.0));
        for (lat, lon) in [
            (-KYIV.0, KYIV.1 - 180.0),
            (-90.0, -180.0),
            (90.0, 180.0),
            (-50.0, 0.0),
        ] {
            let p = plane.point(Position::from_degrees(lat, lon));
            assert!(p.east.is_finite() && p.north.is_finite());
            assert!((p.east.abs() as f64) <= east_max * 1.0001, "{p:?}");
            assert!((p.north.abs() as f64) <= half_turn * 1.0001, "{p:?}");
        }
        // The antipode sits at the corner of the box: ≈20 000 km away, not 0.
        let anti = plane.distance_m(pos(KYIV), Position::from_degrees(-KYIV.0, KYIV.1 - 180.0));
        assert!(anti > 1.5e7, "{anti}");
    }

    #[test]
    fn distance2_orders_like_distance() {
        let plane = LocalPlane::new(pos(KYIV));
        let a = pos(destination(KYIV, 700.0, 30.0));
        let b = pos(destination(KYIV, 900.0, 200.0));
        let o = plane.centre();
        assert!(plane.distance2(o, a) < plane.distance2(o, b));
        let d = plane.distance_m(o, a);
        assert!((d * d - plane.distance2(o, a)).abs() < 1.0);
    }
}
