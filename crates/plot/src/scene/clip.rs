//! Polyline clipping against the plot rectangle (Liang–Barsky), so traces
//! that leave a zoomed-in view are cut exactly at its edge instead of being
//! drawn with absurd coordinates. Clipping runs in `f64`: a deep zoom puts
//! neighbouring samples millions of pixels off screen, and `f32` would bend
//! the visible part of such segments.

use super::Rect;

type Point = [f64; 2];

/// Splits `points` into runs at non-finite points and clips every segment to
/// `rect`. Isolated finite points inside `rect` become one-point runs.
pub fn clip_polyline(points: &[Point], rect: Rect) -> Vec<Vec<[f32; 2]>> {
    let bounds = Bounds::from(rect);
    let mut clipper = Clipper::default();
    for &point in points {
        if point[0].is_finite() && point[1].is_finite() {
            clipper.point(point, &bounds);
        } else {
            clipper.flush(&bounds);
        }
    }
    clipper.flush(&bounds);
    clipper.runs
}

/// The clip rectangle in `f64`.
struct Bounds {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

impl From<Rect> for Bounds {
    fn from(rect: Rect) -> Self {
        Self {
            left: f64::from(rect.x),
            top: f64::from(rect.y),
            right: f64::from(rect.right()),
            bottom: f64::from(rect.bottom()),
        }
    }
}

impl Bounds {
    fn contains(&self, p: Point) -> bool {
        (self.left..=self.right).contains(&p[0]) && (self.top..=self.bottom).contains(&p[1])
    }
}

#[derive(Default)]
struct Clipper {
    runs: Vec<Vec<[f32; 2]>>,
    current: Vec<[f32; 2]>,
    previous: Option<Point>,
    /// Whether `previous` started a run (no segment drawn from it yet).
    lone: bool,
}

impl Clipper {
    fn point(&mut self, point: Point, bounds: &Bounds) {
        match self.previous {
            Some(start) => self.segment(start, point, bounds),
            None => self.lone = true,
        }
        self.previous = Some(point);
    }

    fn segment(&mut self, start: Point, end: Point, bounds: &Bounds) {
        self.lone = false;
        let Some((a, b)) = clip_segment(start, end, bounds) else {
            self.end_run();
            return;
        };
        let a = to_f32(a);
        if self.current.last() != Some(&a) {
            self.end_run();
            self.current.push(a);
        }
        self.current.push(to_f32(b));
        if b != end {
            self.end_run();
        }
    }

    fn end_run(&mut self) {
        if self.current.len() > 1 {
            self.runs.push(std::mem::take(&mut self.current));
        }
        self.current.clear();
    }

    /// Ends the current run at a break; a lone point inside the bounds
    /// becomes a one-point run so it can be drawn as a dot.
    fn flush(&mut self, bounds: &Bounds) {
        self.end_run();
        if self.lone
            && let Some(point) = self.previous
            && bounds.contains(point)
        {
            self.runs.push(vec![to_f32(point)]);
        }
        self.previous = None;
        self.lone = false;
    }
}

fn to_f32(p: Point) -> [f32; 2] {
    [p[0] as f32, p[1] as f32]
}

/// The part of segment `a`–`b` inside `bounds`, if any.
fn clip_segment(a: Point, b: Point, bounds: &Bounds) -> Option<(Point, Point)> {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let mut t0 = 0.0f64;
    let mut t1 = 1.0f64;
    let edges = [
        (-dx, a[0] - bounds.left),
        (dx, bounds.right - a[0]),
        (-dy, a[1] - bounds.top),
        (dy, bounds.bottom - a[1]),
    ];
    for (p, q) in edges {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
            continue;
        }
        let t = q / p;
        if p < 0.0 {
            t0 = t0.max(t);
        } else {
            t1 = t1.min(t);
        }
        if t0 > t1 {
            return None;
        }
    }
    let at = |t: f64| [a[0] + t * dx, a[1] + t * dy];
    let start = if t0 > 0.0 { at(t0) } else { a };
    let end = if t1 < 1.0 { at(t1) } else { b };
    Some((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect() -> Rect {
        Rect::new(0.0, 0.0, 100.0, 100.0)
    }

    #[test]
    fn inside_polyline_is_one_run() {
        let points = [[10.0, 10.0], [50.0, 50.0], [90.0, 20.0]];
        assert_eq!(
            clip_polyline(&points, rect()),
            vec![vec![[10.0, 10.0], [50.0, 50.0], [90.0, 20.0]]]
        );
    }

    #[test]
    fn nan_breaks_the_line() {
        let points = [
            [10.0, 10.0],
            [20.0, 20.0],
            [f64::NAN, 0.0],
            [30.0, 30.0],
            [40.0, 40.0],
        ];
        let runs = clip_polyline(&points, rect());
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[1], vec![[30.0, 30.0], [40.0, 40.0]]);
    }

    #[test]
    fn excursions_are_cut_at_the_edge() {
        let points = [[10.0, 50.0], [50.0, -50.0], [90.0, 50.0]];
        let runs = clip_polyline(&points, rect());
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0], vec![[10.0, 50.0], [30.0, 0.0]]);
        assert_eq!(runs[1], vec![[70.0, 0.0], [90.0, 50.0]]);
    }

    #[test]
    fn far_off_screen_neighbours_keep_the_visible_slope() {
        let points = [[50.0, 50.0], [50.0 + 1e9, 50.0 + 1e9]];
        let runs = clip_polyline(&points, rect());
        assert_eq!(runs, vec![vec![[50.0, 50.0], [100.0, 100.0]]]);
    }

    #[test]
    fn isolated_points_become_dots() {
        let points = [
            [f64::NAN, 0.0],
            [10.0, 10.0],
            [f64::NAN, 0.0],
            [200.0, 10.0],
        ];
        assert_eq!(clip_polyline(&points, rect()), vec![vec![[10.0, 10.0]]]);
        assert_eq!(clip_polyline(&[[5.0, 5.0]], rect()), vec![vec![[5.0, 5.0]]]);
    }

    #[test]
    fn segment_outside_is_dropped() {
        let bounds = Bounds::from(rect());
        assert!(clip_segment([-10.0, -10.0], [-5.0, 50.0], &bounds).is_none());
        assert_eq!(
            clip_segment([-10.0, 50.0], [110.0, 50.0], &bounds),
            Some(([0.0, 50.0], [100.0, 50.0]))
        );
    }
}
