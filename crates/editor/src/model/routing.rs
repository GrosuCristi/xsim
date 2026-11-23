//! Wire routing: orthogonal paths between grid points (Q49).

use xsim_project::{Component, Point};

/// Direction of a straight wire leg.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Horizontal,
    Vertical,
}

impl Axis {
    pub fn other(self) -> Axis {
        match self {
            Axis::Horizontal => Axis::Vertical,
            Axis::Vertical => Axis::Horizontal,
        }
    }

    /// Orientation of the segment `a`–`b`, `None` for diagonal or empty ones.
    pub fn of(a: Point, b: Point) -> Option<Axis> {
        match (a.x == b.x, a.y == b.y) {
            (false, true) => Some(Axis::Horizontal),
            (true, false) => Some(Axis::Vertical),
            _ => None,
        }
    }

    /// The dominant axis of a displacement; horizontal on ties.
    pub fn dominant(delta: Point) -> Axis {
        if delta.x.abs() >= delta.y.abs() {
            Axis::Horizontal
        } else {
            Axis::Vertical
        }
    }
}

/// The points after `from` of a path to `to`: one straight segment when
/// `any_angle` is set or the points share a row or column, else an L whose
/// first leg runs along `first`. `from` itself is not included.
pub fn route(from: Point, to: Point, first: Axis, any_angle: bool) -> Vec<Point> {
    if from == to {
        return Vec::new();
    }
    if any_angle || from.x == to.x || from.y == to.y {
        return vec![to];
    }
    let corner = match first {
        Axis::Horizontal => Point::new(to.x, from.y),
        Axis::Vertical => Point::new(from.x, to.y),
    };
    vec![corner, to]
}

/// The axis a pin's wire naturally leaves along: away from the symbol centre.
pub fn pin_axis(component: &Component, pin: usize) -> Option<Axis> {
    let p = *component.pins().get(pin)?;
    let offset = p - component.position;
    Some(Axis::dominant(offset))
}

/// Chooses the first leg of the next wire segment: perpendicular to the
/// previous segment when there is one, along the pin axis when the wire
/// starts at a pin, else along the dominant direction of the mouse movement.
pub fn first_leg(previous: Option<(Point, Point)>, start_axis: Option<Axis>, delta: Point) -> Axis {
    if let Some(axis) = previous.and_then(|(a, b)| Axis::of(a, b)) {
        return axis.other();
    }
    start_axis.unwrap_or_else(|| Axis::dominant(delta))
}

/// A connection between a moved point and a fixed one that stays orthogonal:
/// a Z with both ends running along `axis` when there is room for the middle
/// leg, else an L that keeps `axis` at the fixed end. Returns the points
/// strictly between `moved` and `fixed`.
pub fn reconnect(moved: Point, fixed: Point, axis: Axis) -> Vec<Point> {
    match axis {
        Axis::Horizontal if moved.y == fixed.y => Vec::new(),
        Axis::Vertical if moved.x == fixed.x => Vec::new(),
        Axis::Horizontal => {
            let span = fixed.x - moved.x;
            if span.abs() >= 2 {
                let mid = moved.x + span / 2;
                vec![Point::new(mid, moved.y), Point::new(mid, fixed.y)]
            } else {
                vec![Point::new(moved.x, fixed.y)]
            }
        }
        Axis::Vertical => {
            let span = fixed.y - moved.y;
            if span.abs() >= 2 {
                let mid = moved.y + span / 2;
                vec![Point::new(moved.x, mid), Point::new(fixed.x, mid)]
            } else {
                vec![Point::new(fixed.x, moved.y)]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Axis, first_leg, reconnect, route};
    use xsim_project::Point;

    #[test]
    fn routes_are_orthogonal_unless_any_angle_is_requested() {
        let (a, b) = (Point::new(-4, 2), Point::new(7, 5));
        assert_eq!(
            route(a, b, Axis::Horizontal, false),
            vec![Point::new(7, 2), b]
        );
        assert_eq!(
            route(a, b, Axis::Vertical, false),
            vec![Point::new(-4, 5), b]
        );
        assert_eq!(route(a, b, Axis::Horizontal, true), vec![b]);
        assert_eq!(
            route(a, Point::new(-4, 9), Axis::Horizontal, false).len(),
            1
        );
        assert!(route(a, a, Axis::Horizontal, false).is_empty());
    }

    #[test]
    fn the_bend_follows_the_previous_segment_then_the_pin_then_the_mouse() {
        let previous = Some((Point::new(0, 0), Point::new(4, 0)));
        assert_eq!(first_leg(previous, None, Point::new(9, 1)), Axis::Vertical);
        assert_eq!(
            first_leg(None, Some(Axis::Vertical), Point::new(9, 1)),
            Axis::Vertical
        );
        assert_eq!(first_leg(None, None, Point::new(1, -3)), Axis::Vertical);
        assert_eq!(first_leg(None, None, Point::new(3, 3)), Axis::Horizontal);
    }

    #[test]
    fn reconnection_keeps_both_ends_along_the_axis_when_there_is_room() {
        let z = reconnect(Point::new(2, -2), Point::new(6, 0), Axis::Horizontal);
        assert_eq!(z, vec![Point::new(4, -2), Point::new(4, 0)]);
        let l = reconnect(Point::new(5, -2), Point::new(6, 0), Axis::Horizontal);
        assert_eq!(l, vec![Point::new(5, 0)]);
        let v = reconnect(Point::new(3, 0), Point::new(0, 5), Axis::Vertical);
        assert_eq!(v, vec![Point::new(3, 2), Point::new(0, 2)]);
        assert!(reconnect(Point::new(9, 0), Point::new(6, 0), Axis::Horizontal).is_empty());
    }
}
