//! Local metric coordinates, their wire positions, and a spatial grid.
//!
//! Scenarios place nodes in metres east (`x`) and north (`y`) of an origin
//! given in degrees. The simulator measures distances in those metres and
//! hands each core a real [`Position`] so the wire codec runs on it.

use bark_proto::Position;
use serde::{Deserialize, Serialize};

/// Earth radius used for the local tangent plane, the same as the core's
/// position unit (`bark_proto::position::UNIT_M`).
pub const EARTH_RADIUS_M: f64 = 6_371_000.0;

/// A point on the local grid, in metres.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Xy {
    /// Metres east of the origin.
    pub x: f64,
    /// Metres north of the origin.
    pub y: f64,
}

impl Xy {
    /// Euclidean distance in metres.
    pub fn dist(self, o: Xy) -> f64 {
        ((self.x - o.x).powi(2) + (self.y - o.y).powi(2)).sqrt()
    }
}

/// Origin of the local grid in degrees.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Origin {
    /// Latitude in degrees.
    pub lat: f64,
    /// Longitude in degrees.
    pub lon: f64,
}

impl Default for Origin {
    /// Central Kyiv.
    fn default() -> Self {
        Origin {
            lat: 50.45,
            lon: 30.52,
        }
    }
}

impl Origin {
    /// Wire position of a local point (equirectangular projection about the
    /// origin; exact to well under the 0.6 m position unit over a city).
    pub fn position(self, p: Xy) -> Position {
        let lat = self.lat + (p.y / EARTH_RADIUS_M).to_degrees();
        let lon = self.lon + (p.x / (EARTH_RADIUS_M * self.lat.to_radians().cos())).to_degrees();
        Position::from_degrees(lat, lon)
    }
}

/// Uniform grid over points with square cells of a given side. A query for
/// everything within `r ≤ side` of a point visits the 3 × 3 block of cells
/// around it and nothing else.
pub struct Grid {
    side: f64,
    x0: f64,
    y0: f64,
    nx: usize,
    ny: usize,
    /// Point indices, grouped by cell (counting-sort layout).
    items: Vec<u32>,
    /// `items[start[c]..start[c + 1]]` are the points in cell `c`.
    start: Vec<u32>,
}

impl Grid {
    /// Build a grid with cell side `side` metres over `points`.
    pub fn new(points: &[Xy], side: f64) -> Grid {
        assert!(
            side > 0.0 && side.is_finite(),
            "grid cell side must be positive"
        );
        let (mut x0, mut y0, mut x1, mut y1) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        if let Some(p) = points.first() {
            (x0, y0, x1, y1) = (p.x, p.y, p.x, p.y);
        }
        for p in points {
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
        }
        let nx = ((x1 - x0) / side) as usize + 1;
        let ny = ((y1 - y0) / side) as usize + 1;
        let mut g = Grid {
            side,
            x0,
            y0,
            nx,
            ny,
            items: vec![0; points.len()],
            start: vec![0; nx * ny + 1],
        };
        let cells: Vec<usize> = points.iter().map(|&p| g.cell(p)).collect();
        for &c in &cells {
            g.start[c + 1] += 1;
        }
        for c in 0..nx * ny {
            g.start[c + 1] += g.start[c];
        }
        let mut fill = g.start.clone();
        for (i, &c) in cells.iter().enumerate() {
            g.items[fill[c] as usize] = i as u32;
            fill[c] += 1;
        }
        g
    }

    fn coords(&self, p: Xy) -> (usize, usize) {
        let cx = (((p.x - self.x0) / self.side).max(0.0) as usize).min(self.nx - 1);
        let cy = (((p.y - self.y0) / self.side).max(0.0) as usize).min(self.ny - 1);
        (cx, cy)
    }

    fn cell(&self, p: Xy) -> usize {
        let (cx, cy) = self.coords(p);
        cy * self.nx + cx
    }

    /// Call `f` with every point in the 3 × 3 cells around `p`: a superset
    /// of the points within `side` of `p`. Points outside the grid's extent
    /// are clamped onto its edge cells, so a query far outside the grid sees
    /// the nearest edge cells; callers filter by distance.
    pub fn neighbourhood(&self, p: Xy, mut f: impl FnMut(u32)) {
        let (cx, cy) = self.coords(p);
        for y in cy.saturating_sub(1)..=(cy + 1).min(self.ny - 1) {
            for x in cx.saturating_sub(1)..=(cx + 1).min(self.nx - 1) {
                let c = y * self.nx + x;
                for &i in &self.items[self.start[c] as usize..self.start[c + 1] as usize] {
                    f(i);
                }
            }
        }
    }

    /// Cell side in metres.
    pub fn side(&self) -> f64 {
        self.side
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bark_proto::position::UNIT_M;

    #[test]
    fn position_offsets_match_metres() {
        let o = Origin::default();
        let p0 = o.position(Xy { x: 0.0, y: 0.0 });
        let p1 = o.position(Xy { x: 0.0, y: 1000.0 });
        let counts = (p1.lat - p0.lat) as f64;
        assert!(
            (counts * UNIT_M - 1000.0).abs() < 1.0,
            "1 km north is {} m",
            counts * UNIT_M
        );
    }

    #[test]
    fn neighbourhood_is_superset_of_disc() {
        let pts: Vec<Xy> = (0..400)
            .map(|i| Xy {
                x: (i % 20) as f64 * 37.0,
                y: (i / 20) as f64 * 41.0,
            })
            .collect();
        let g = Grid::new(&pts, 100.0);
        for &q in &pts {
            let mut got = Vec::new();
            g.neighbourhood(q, |i| got.push(i));
            for (i, &p) in pts.iter().enumerate() {
                if p.dist(q) <= 100.0 {
                    assert!(got.contains(&(i as u32)));
                }
            }
        }
    }
}
