//! Exact integer tests for points on wire segments.
//!
//! Coordinates are `i32`, so differences need 33 bits and their products 66:
//! every cross product is evaluated in `i128` and never overflows.

use crate::schematic::Point;

/// Whether `p` lies on the closed segment `a`–`b` (endpoints included).
pub(super) fn on_segment(p: Point, a: Point, b: Point) -> bool {
    let within = |v: i32, l: i32, r: i32| l.min(r) <= v && v <= l.max(r);
    within(p.x, a.x, b.x) && within(p.y, a.y, b.y) && cross(a, b, p) == 0
}

/// Whether `p` lies strictly between the endpoints of segment `a`–`b`.
pub(super) fn on_segment_interior(p: Point, a: Point, b: Point) -> bool {
    p != a && p != b && on_segment(p, a, b)
}

/// `(b − a) × (p − a)`: zero exactly when the three points are collinear.
fn cross(a: Point, b: Point, p: Point) -> i128 {
    let d = |l: i32, r: i32| i128::from(l) - i128::from(r);
    d(b.x, a.x) * d(p.y, a.y) - d(b.y, a.y) * d(p.x, a.x)
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: fn(i32, i32) -> Point = Point::new;

    #[test]
    fn axis_aligned_segments() {
        assert!(on_segment_interior(P(2, 0), P(0, 0), P(4, 0)));
        assert!(on_segment_interior(P(0, 3), P(0, 5), P(0, 1)));
        assert!(!on_segment_interior(P(0, 0), P(0, 0), P(4, 0)));
        assert!(on_segment(P(0, 0), P(0, 0), P(4, 0)));
        assert!(!on_segment(P(5, 0), P(0, 0), P(4, 0)));
        assert!(!on_segment(P(2, 1), P(0, 0), P(4, 0)));
    }

    #[test]
    fn diagonal_segments_hit_only_exact_lattice_points() {
        assert!(on_segment_interior(P(2, 1), P(0, 0), P(4, 2)));
        assert!(!on_segment_interior(P(1, 1), P(0, 0), P(4, 2)));
        assert!(on_segment_interior(P(1, -1), P(0, 0), P(3, -3)));
        assert!(!on_segment(P(6, 3), P(0, 0), P(4, 2)));
    }

    #[test]
    fn degenerate_segment_has_no_interior() {
        assert!(!on_segment_interior(P(1, 1), P(1, 1), P(1, 1)));
        assert!(on_segment(P(1, 1), P(1, 1), P(1, 1)));
    }

    #[test]
    fn extreme_coordinates_do_not_overflow() {
        let a = P(i32::MIN, i32::MIN);
        let b = P(i32::MAX, i32::MAX);
        assert!(on_segment_interior(P(0, 0), a, b));
        assert!(!on_segment_interior(P(0, 1), a, b));
        assert!(!on_segment(P(i32::MAX, i32::MIN), a, b));
    }
}
