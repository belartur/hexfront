//! Small scalar helpers shared by the simulation and the mesh builders.
//!
//! These are the two-argument forms of "square" and "distance" that the game
//! rules and the world geometry ask for over and over (rules.md section 9
//! measures reach in j). They live in their own module so no logic module has
//! to own them, and every call site reads as the rule it implements instead of
//! as an open-coded `(a - b) * (a - b)` sum.

/// Square of `x`.
///
/// The readable stand-in for `x.powi(2)`: one multiplication, no argument
/// parsing, and it says "square" at the call site. Inlined by the compiler
/// into whatever expression uses it.
#[inline]
pub fn sqr(x: f64) -> f64 {
    x * x
}

/// Squared distance between two world points.
///
/// Compared against a squared radius this saves the square root, so the
/// per-tile reach and snap tests in the AI and on the board cost one multiply
/// per axis instead of one multiply and a `sqrt` per candidate.
#[inline]
pub fn dist2(a: (f64, f64), b: (f64, f64)) -> f64 {
    sqr(a.0 - b.0) + sqr(a.1 - b.1)
}

/// Euclidean distance between two world points (rules.md section 9).
#[inline]
pub fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    dist2(a, b).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqr_multiplies_instead_of_calling_powi() {
        for x in [0.0, 1.0, -1.0, 2.5, -7.25, 1e3, -1e-3] {
            assert_eq!(sqr(x), x.powi(2), "sqr({x})");
        }
    }

    #[test]
    fn distances_agree_with_the_open_coded_form() {
        let pts: [(f64, f64); 4] = [(0.0, 0.0), (3.0, 4.0), (-1.5, 2.25), (120.0, -45.0)];
        for a in pts {
            for b in pts {
                let open = ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();
                assert_eq!(dist(a, b), open, "dist({a:?}, {b:?})");
                // `open * open` rounds the root back up, so this is an
                // approximate identity, not an exact one: the point of the
                // helpers is that they skip the root, not that they reproduce
                // it bit for bit.
                assert!(
                    (dist2(a, b) - open * open).abs() < 1e-9,
                    "dist2({a:?}, {b:?})"
                );
            }
        }
    }

    #[test]
    fn a_squared_radius_test_agrees_with_the_distance_one() {
        // The reach and snap checks compare against a radius. Squaring both
        // sides must accept exactly the same points, otherwise a vehicle would
        // cross a trigger radius at a different tile than it used to.
        let centre = (10.0, -4.0);
        for r in [0.5, 1.0, 5.0, 36.0, 150.0] {
            for (x, y) in [
                (0.0, 0.0),
                (1.0, 1.0),
                (0.001, 0.001),
                (-7.5, 3.25),
                (12.0, -4.0),
                (10.0 + r, -4.0),
                (10.0, -4.0 + r),
                (10.0 + r * 0.999, -4.0 + r * 0.999),
            ] {
                let by_distance = dist(centre, (x, y)) <= r;
                let by_square = dist2(centre, (x, y)) <= sqr(r);
                assert_eq!(by_distance, by_square, "radius {r}, point ({x}, {y})");
            }
        }
    }

    #[test]
    fn a_point_is_zero_from_itself() {
        assert_eq!(dist((2.0, 7.0), (2.0, 7.0)), 0.0);
        assert_eq!(dist2((2.0, 7.0), (2.0, 7.0)), 0.0);
    }
}
