//! Plain geometry on the schematic grid and in fractional world space.
//!
//! Grid coordinates are [`xsim_project::Point`]s (integers, `y` down). World
//! coordinates are the same space with fractional values, used for the mouse
//! position and for drawing.

use std::ops::{Add, Mul, Sub};
use xsim_project::Point;

/// A point with fractional coordinates, in grid units or pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// The exact position of a grid point.
    pub fn from_grid(p: Point) -> Self {
        Self::new(p.x as f32, p.y as f32)
    }

    /// The nearest grid point (Q48: everything snaps to the grid).
    pub fn snapped(self) -> Point {
        Point::new(self.x.round() as i32, self.y.round() as i32)
    }

    pub fn length(self) -> f32 {
        self.x.hypot(self.y)
    }

    pub fn distance(self, other: Vec2) -> f32 {
        (self - other).length()
    }
}

impl Add for Vec2 {
    type Output = Vec2;
    fn add(self, rhs: Vec2) -> Vec2 {
        Vec2::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, rhs: Vec2) -> Vec2 {
        Vec2::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl Mul<f32> for Vec2 {
    type Output = Vec2;
    fn mul(self, rhs: f32) -> Vec2 {
        Vec2::new(self.x * rhs, self.y * rhs)
    }
}

/// An axis-aligned rectangle with fractional coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub min: Vec2,
    pub max: Vec2,
}

impl Rect {
    /// The rectangle spanned by two opposite corners in any order.
    pub fn from_corners(a: Vec2, b: Vec2) -> Self {
        Self {
            min: Vec2::new(a.x.min(b.x), a.y.min(b.y)),
            max: Vec2::new(a.x.max(b.x), a.y.max(b.y)),
        }
    }

    /// The smallest rectangle containing every point, `None` without points.
    pub fn enclosing(points: impl IntoIterator<Item = Vec2>) -> Option<Self> {
        let mut points = points.into_iter();
        let first = points.next()?;
        Some(points.fold(Rect::from_corners(first, first), |r, p| {
            r.union(Rect::from_corners(p, p))
        }))
    }

    pub fn union(self, other: Rect) -> Rect {
        Rect {
            min: Vec2::new(self.min.x.min(other.min.x), self.min.y.min(other.min.y)),
            max: Vec2::new(self.max.x.max(other.max.x), self.max.y.max(other.max.y)),
        }
    }

    /// Grows (or with a negative amount shrinks) every side by `amount`.
    pub fn inflate(self, dx: f32, dy: f32) -> Rect {
        Rect {
            min: Vec2::new(self.min.x - dx, self.min.y - dy),
            max: Vec2::new(self.max.x + dx, self.max.y + dy),
        }
    }

    pub fn width(self) -> f32 {
        self.max.x - self.min.x
    }

    pub fn height(self) -> f32 {
        self.max.y - self.min.y
    }

    pub fn center(self) -> Vec2 {
        Vec2::new(
            (self.min.x + self.max.x) * 0.5,
            (self.min.y + self.max.y) * 0.5,
        )
    }

    pub fn contains(self, p: Vec2) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }

    pub fn contains_rect(self, other: Rect) -> bool {
        self.contains(other.min) && self.contains(other.max)
    }

    pub fn intersects(self, other: Rect) -> bool {
        self.min.x <= other.max.x
            && other.min.x <= self.max.x
            && self.min.y <= other.max.y
            && other.min.y <= self.max.y
    }

    /// Whether the segment `a`–`b` touches the rectangle.
    pub fn intersects_segment(self, a: Vec2, b: Vec2) -> bool {
        if self.contains(a) || self.contains(b) {
            return true;
        }
        let corners = [
            self.min,
            Vec2::new(self.max.x, self.min.y),
            self.max,
            Vec2::new(self.min.x, self.max.y),
        ];
        (0..4).any(|i| segments_intersect(a, b, corners[i], corners[(i + 1) % 4]))
    }
}

fn cross(o: Vec2, a: Vec2, b: Vec2) -> f32 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

/// Whether two closed segments share at least one point.
pub fn segments_intersect(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    let d1 = cross(c, d, a);
    let d2 = cross(c, d, b);
    let d3 = cross(a, b, c);
    let d4 = cross(a, b, d);
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
    {
        return true;
    }
    let within = |p: Vec2, q: Vec2, r: Vec2| {
        r.x >= p.x.min(q.x) && r.x <= p.x.max(q.x) && r.y >= p.y.min(q.y) && r.y <= p.y.max(q.y)
    };
    (d1 == 0.0 && within(c, d, a))
        || (d2 == 0.0 && within(c, d, b))
        || (d3 == 0.0 && within(a, b, c))
        || (d4 == 0.0 && within(a, b, d))
}

/// Distance from `p` to the closed segment `a`–`b`.
pub fn distance_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let length_sq = ab.x * ab.x + ab.y * ab.y;
    if length_sq == 0.0 {
        return p.distance(a);
    }
    let ap = p - a;
    let t = ((ap.x * ab.x + ap.y * ab.y) / length_sq).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// Whether grid point `p` lies on the closed segment `a`–`b` (exact).
pub fn on_segment(p: Point, a: Point, b: Point) -> bool {
    let (abx, aby) = (i64::from(b.x - a.x), i64::from(b.y - a.y));
    let (apx, apy) = (i64::from(p.x - a.x), i64::from(p.y - a.y));
    abx * apy - aby * apx == 0
        && p.x >= a.x.min(b.x)
        && p.x <= a.x.max(b.x)
        && p.y >= a.y.min(b.y)
        && p.y <= a.y.max(b.y)
}

/// Whether `p` lies on the segment `a`–`b` but is neither end.
pub fn on_segment_interior(p: Point, a: Point, b: Point) -> bool {
    p != a && p != b && on_segment(p, a, b)
}

/// Whether `a`, `b`, `c` lie on one line.
pub fn collinear(a: Point, b: Point, c: Point) -> bool {
    let (abx, aby) = (i64::from(b.x - a.x), i64::from(b.y - a.y));
    let (acx, acy) = (i64::from(c.x - a.x), i64::from(c.y - a.y));
    abx * acy - aby * acx == 0
}

#[cfg(test)]
mod tests {
    use super::{Rect, Vec2, distance_to_segment, on_segment, on_segment_interior};
    use xsim_project::Point;

    #[test]
    fn snapping_rounds_to_the_nearest_grid_point() {
        assert_eq!(Vec2::new(1.49, -2.51).snapped(), Point::new(1, -3));
    }

    #[test]
    fn segment_membership_is_exact() {
        let (a, b) = (Point::new(0, 0), Point::new(4, 0));
        assert!(on_segment(Point::new(2, 0), a, b));
        assert!(on_segment(a, a, b));
        assert!(!on_segment_interior(a, a, b));
        assert!(!on_segment(Point::new(5, 0), a, b));
        assert!(!on_segment(Point::new(2, 1), a, b));
        assert!(on_segment(
            Point::new(1, 1),
            Point::new(0, 0),
            Point::new(3, 3)
        ));
    }

    #[test]
    fn distance_to_segment_clamps_to_the_ends() {
        let (a, b) = (Vec2::new(0.0, 0.0), Vec2::new(4.0, 0.0));
        assert_eq!(distance_to_segment(Vec2::new(2.0, 1.0), a, b), 1.0);
        assert_eq!(distance_to_segment(Vec2::new(7.0, 4.0), a, b), 5.0);
    }

    #[test]
    fn rectangles_detect_crossing_segments() {
        let r = Rect::from_corners(Vec2::new(0.0, 0.0), Vec2::new(2.0, 2.0));
        assert!(r.intersects_segment(Vec2::new(-1.0, 1.0), Vec2::new(3.0, 1.0)));
        assert!(!r.intersects_segment(Vec2::new(-1.0, 3.0), Vec2::new(3.0, 3.0)));
        assert!(r.contains_rect(Rect::from_corners(Vec2::new(0.5, 0.5), Vec2::new(1.0, 1.0))));
    }
}
