//! Hexagonal grid helpers for a flat-top hex layout.
//!
//! The board uses *odd-q* offset coordinates (flat-top hexagons, odd columns
//! pushed down on screen):
//!
//! ```text
//! world.x = 1.5 * side * q
//! world.y = sqrt(3) * side * (r + 0.5 * (q & 1))
//! ```
//!
//! All functions here are pure geometry; they know nothing about game rules.

/// sqrt(3), the height/width ratio constant of a flat-top hexagon.
pub const SQRT3: f64 = 1.7320508075688772;

/// Geometric directions for tiles in even columns, ordered by angle:
/// 30, 90, 150, 210, 270, 330 deg (screen y grows downwards). Stepping
/// repeatedly in one direction follows a straight hex corridor.
pub const GEO_DIRS_EVEN: [(i32, i32); 6] = [(1, 0), (0, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];
/// Same angles for tiles in odd columns (the odd-q stagger swaps offsets).
pub const GEO_DIRS_ODD: [(i32, i32); 6] = [(1, 1), (0, 1), (-1, 1), (-1, 0), (0, -1), (1, 0)];
/// Index of the opposite (180 deg) geometric direction.
///
/// Test-only: the board expresses "the other end of this axis" as `d + 3`
/// inline, and only the geometry tests need a table to walk over. Compiled
/// for tests alone because nothing in the game reads it.
#[cfg(test)]
pub const OPPOSITE_DIR: [usize; 6] = [3, 4, 5, 0, 1, 2];

/// Tile coordinates: column `q`, row `r` (both zero-based).
pub type Tile = (i32, i32);

/// The 6 geometric direction offsets for a tile in column `q`.
pub fn dirs(q: i32) -> [(i32, i32); 6] {
    if q & 1 != 0 {
        GEO_DIRS_ODD
    } else {
        GEO_DIRS_EVEN
    }
}

/// Return the `(q, r)` coordinates of the neighbour in `direction`.
///
/// Directions are *geometric* (see [`GEO_DIRS_EVEN`]): stepping repeatedly
/// in one direction follows a straight line of hexes.
pub fn neighbor(q: i32, r: i32, direction: usize) -> Tile {
    let (dq, dr) = dirs(q)[direction % 6];
    (q + dq, r + dr)
}

/// Return the `(q, r)` coordinates of all six neighbours.
pub fn neighbors(q: i32, r: i32) -> [Tile; 6] {
    let d = dirs(q);
    [
        (q + d[0].0, r + d[0].1),
        (q + d[1].0, r + d[1].1),
        (q + d[2].0, r + d[2].1),
        (q + d[3].0, r + d[3].1),
        (q + d[4].0, r + d[4].1),
        (q + d[5].0, r + d[5].1),
    ]
}

/// Convert offset hex coordinates to world (x, y) of the tile centre.
pub fn hex_to_world(q: i32, r: i32, side: f64) -> (f64, f64) {
    let x = 1.5 * side * q as f64;
    let y = SQRT3 * side * (r as f64 + 0.5 * ((q & 1) as f64));
    (x, y)
}

#[allow(unused_assignments)]
fn axial_round(x: f64, y: f64) -> (i32, i32) {
    let (xc, zc) = (x, y);
    let yc = -xc - zc;
    let (mut rx, mut ry, mut rz) = (xc.round(), yc.round(), zc.round());
    let (dx, dy, dz) = ((rx - xc).abs(), (ry - yc).abs(), (rz - zc).abs());
    if dx > dy && dx > dz {
        rx = -ry - rz;
    } else if dy > dz {
        ry = -rx - rz;
    } else {
        rz = -rx - ry;
    }
    (rx as i32, rz as i32)
}

/// Convert world coordinates to the offset (q, r) of the containing hex.
pub fn world_to_hex(x: f64, y: f64, side: f64) -> Tile {
    let qf = 2.0 / 3.0 * x / side;
    let rf = SQRT3 / 3.0 * y / side - qf / 2.0;
    let (qa, ra) = axial_round(qf, rf);
    let q = qa;
    let r = ra + (q - (q & 1)) / 2;
    (q, r)
}

/// Convert odd-q offset coordinates to axial (q, r).
pub fn offset_to_axial(q: i32, r: i32) -> (i32, i32) {
    (q, r - (q - (q & 1)) / 2)
}

/// Hex-grid distance (number of steps) between two tiles.
pub fn hex_distance(q1: i32, r1: i32, q2: i32, r2: i32) -> i32 {
    let (qa1, ra1) = offset_to_axial(q1, r1);
    let (qa2, ra2) = offset_to_axial(q2, r2);
    let dx = qa1 - qa2;
    let dz = ra1 - ra2;
    let dy = -dx - dz;
    (dx.abs() + dy.abs() + dz.abs()) / 2
}

/// Tie-break key for routes of equal length: `4 * d^2 / side^2`, the squared
/// Euclidean distance between two tile centres scaled by a factor common to
/// every pair, so comparing keys orders the distances exactly (rules.md
/// section 4 resolves an ambiguous next field of a route by the squared
/// Euclidean distance to the destination).
///
/// Computed in `i64` from the odd-q layout rather than from
/// [`hex_to_world`], because `y` contains `sqrt(3)`: with
/// `dq = q2 - q1`, `dr = r2 - r1` and `dp = (q2 & 1) - (q1 & 1)` the world
/// offset is `(1.5 * dq, sqrt(3) * (dr + dp / 2))` sides, which squares to
/// `(9 * dq^2 + 3 * (2 * dr + dp)^2) / 4` sides squared. An integer key keeps
/// the ordering total and free of floating-point ties, so a queue ordered by
/// it stays deterministic.
pub fn sq_dist_key(q1: i32, r1: i32, q2: i32, r2: i32) -> i64 {
    let dq = q2 as i64 - q1 as i64;
    let dr = r2 as i64 - r1 as i64;
    let dp = (q2 & 1) as i64 - (q1 & 1) as i64;
    let s = 2 * dr + dp;
    9 * dq * dq + 3 * s * s
}

/// Return the six corners of a tile as world (x, y) pairs.
///
/// Corners are ordered clockwise starting at angle 0 deg (east). The edge
/// between corner `k` and corner `k + 1` faces the direction returned by
/// [`edge_dir_index`].
pub fn hex_corners(q: i32, r: i32, side: f64) -> [(f64, f64); 6] {
    let (cx, cy) = hex_to_world(q, r, side);
    let mut pts = [(0.0, 0.0); 6];
    for (k, slot) in pts.iter_mut().enumerate() {
        let ang = std::f64::consts::PI / 3.0 * k as f64;
        *slot = (cx + side * ang.cos(), cy + side * ang.sin());
    }
    pts
}

/// Map the edge between corner `k` and `k + 1` to a direction.
///
/// Edge 0 (corners 0-1) faces 30 deg = direction 0; the geometric direction
/// indices are ordered by angle, so the answer is simply `k`.
pub fn edge_dir_index(_q: i32, k: usize) -> usize {
    k % 6
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn neighbors_are_inverse() {
        for q in 0..6 {
            for r in 0..6 {
                for (d, opp) in OPPOSITE_DIR.iter().enumerate() {
                    let (nq, nr) = neighbor(q, r, d);
                    let (bq, br) = neighbor(nq, nr, *opp);
                    assert_eq!((bq, br), (q, r));
                }
            }
        }
    }
    #[test]
    fn roundtrip_world_hex() {
        let side = 36.0;
        for q in 0..8 {
            for r in 0..8 {
                let (x, y) = hex_to_world(q, r, side);
                assert_eq!(world_to_hex(x, y, side), (q, r));
            }
        }
    }

    #[test]
    fn sq_dist_key_is_four_times_the_squared_distance_in_sides() {
        // rules.md section 4 compares routes by the squared Euclidean
        // distance to the goal; the key must order exactly like the distance
        // it stands for, and stay an integer so a queue ordered by it is
        // free of float ties.
        let side = 36.0;
        for q1 in -3..4 {
            for r1 in -3..4 {
                for q2 in -3..4 {
                    for r2 in -3..4 {
                        let a = hex_to_world(q1, r1, side);
                        let b = hex_to_world(q2, r2, side);
                        let want = 4.0 * ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)) / side.powi(2);
                        let got = sq_dist_key(q1, r1, q2, r2) as f64;
                        assert!(
                            (got - want).abs() < 1e-9,
                            "sq_dist_key({q1},{r1},{q2},{r2}) = {got}, want {want}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn sq_dist_key_is_symmetric_and_zero_only_on_itself() {
        for q1 in -3..4 {
            for r1 in -3..4 {
                for q2 in -3..4 {
                    for r2 in -3..4 {
                        let k = sq_dist_key(q1, r1, q2, r2);
                        assert_eq!(k, sq_dist_key(q2, r2, q1, r1), "asymmetric");
                        assert_eq!(k == 0, (q1, r1) == (q2, r2), "zero distance");
                    }
                }
            }
        }
    }
}
