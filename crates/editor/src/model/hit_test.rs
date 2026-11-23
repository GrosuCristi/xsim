//! Finding the object under the mouse and inside a selection box.
//!
//! Priority under the mouse: pin, then label, then wire, then part body.
//! Part bodies are shrunk so their pins stay reachable.

use crate::model::geometry::{Rect, Vec2, distance_to_segment};
use crate::model::label_geometry::label_hit_rect;
use crate::model::selection::ObjectRef;
use xsim_project::{Component, Id, Point, Schematic};

/// What lies under a world position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Pin {
        component: Id,
        pin: usize,
        point: Point,
    },
    Label(Id),
    Wire {
        wire: Id,
        segment: usize,
    },
    Component(Id),
}

impl Hit {
    /// The object a click on this hit selects.
    pub fn object(self) -> ObjectRef {
        match self {
            Hit::Pin { component, .. } | Hit::Component(component) => {
                ObjectRef::Component(component)
            }
            Hit::Label(id) => ObjectRef::Label(id),
            Hit::Wire { wire, .. } => ObjectRef::Wire(wire),
        }
    }
}

/// Clickable body of a part: its bounds pulled away from the pins, but at
/// least `MIN_HALF` thick.
pub fn body_rect(component: &Component) -> Rect {
    const INSET: f32 = 1.0;
    const MIN_HALF: f32 = 1.2;
    let (a, b) = component.bounds();
    let r = Rect::from_corners(Vec2::from_grid(a), Vec2::from_grid(b)).inflate(-INSET, -INSET);
    let c = r.center();
    let half_w = (r.width() * 0.5).max(MIN_HALF);
    let half_h = (r.height() * 0.5).max(MIN_HALF);
    Rect::from_corners(c - Vec2::new(half_w, half_h), c + Vec2::new(half_w, half_h))
}

/// The topmost object within `tolerance` grid units of `p`.
pub fn hit_test(schematic: &Schematic, p: Vec2, tolerance: f32) -> Option<Hit> {
    hit_pin(schematic, p, tolerance)
        .or_else(|| hit_label(schematic, p))
        .or_else(|| hit_wire(schematic, p, tolerance))
        .or_else(|| hit_body(schematic, p))
}

fn hit_pin(schematic: &Schematic, p: Vec2, tolerance: f32) -> Option<Hit> {
    let mut best: Option<(f32, Hit)> = None;
    for c in schematic.components.iter().rev() {
        for (pin, point) in c.pins().into_iter().enumerate() {
            let d = p.distance(Vec2::from_grid(point));
            if d <= tolerance && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((
                    d,
                    Hit::Pin {
                        component: c.id,
                        pin,
                        point,
                    },
                ));
            }
        }
    }
    best.map(|(_, hit)| hit)
}

fn hit_label(schematic: &Schematic, p: Vec2) -> Option<Hit> {
    schematic
        .labels
        .iter()
        .rev()
        .find(|l| label_hit_rect(schematic, l).contains(p))
        .map(|l| Hit::Label(l.id))
}

fn hit_wire(schematic: &Schematic, p: Vec2, tolerance: f32) -> Option<Hit> {
    let mut best: Option<(f32, Hit)> = None;
    for w in schematic.wires.iter().rev() {
        for (segment, s) in w.points.windows(2).enumerate() {
            let d = distance_to_segment(p, Vec2::from_grid(s[0]), Vec2::from_grid(s[1]));
            if d <= tolerance && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((
                    d,
                    Hit::Wire {
                        wire: w.id,
                        segment,
                    },
                ));
            }
        }
    }
    best.map(|(_, hit)| hit)
}

fn hit_body(schematic: &Schematic, p: Vec2) -> Option<Hit> {
    schematic
        .components
        .iter()
        .rev()
        .find(|c| body_rect(c).contains(p))
        .map(|c| Hit::Component(c.id))
}

/// How a selection box picks objects (the KiCad convention).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxMode {
    /// Dragged left to right: objects entirely inside.
    Enclosed,
    /// Dragged right to left: objects the box touches.
    Touching,
}

impl BoxMode {
    /// The mode for a box dragged from `start` to `end`.
    pub fn for_drag(start: Vec2, end: Vec2) -> BoxMode {
        if end.x >= start.x {
            BoxMode::Enclosed
        } else {
            BoxMode::Touching
        }
    }
}

/// Objects selected by `rect`.
pub fn objects_in_box(schematic: &Schematic, rect: Rect, mode: BoxMode) -> Vec<ObjectRef> {
    let picks = |area: Rect| match mode {
        BoxMode::Enclosed => rect.contains_rect(area),
        BoxMode::Touching => rect.intersects(area),
    };
    let components = schematic.components.iter().filter(|c| {
        let (a, b) = c.bounds();
        picks(Rect::from_corners(Vec2::from_grid(a), Vec2::from_grid(b)))
    });
    let wires = schematic.wires.iter().filter(|w| match mode {
        BoxMode::Enclosed => w.points.iter().all(|&q| rect.contains(Vec2::from_grid(q))),
        BoxMode::Touching => w
            .points
            .windows(2)
            .any(|s| rect.intersects_segment(Vec2::from_grid(s[0]), Vec2::from_grid(s[1]))),
    });
    let labels = schematic.labels.iter().filter(|l| {
        rect.contains(Vec2::from_grid(l.position))
            || (mode == BoxMode::Touching && rect.intersects(label_hit_rect(schematic, l)))
    });
    components
        .map(|c| ObjectRef::Component(c.id))
        .chain(wires.map(|w| ObjectRef::Wire(w.id)))
        .chain(labels.map(|l| ObjectRef::Label(l.id)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{BoxMode, Hit, hit_test, objects_in_box};
    use crate::model::geometry::{Rect, Vec2};
    use crate::model::selection::ObjectRef;
    use xsim_project::{Component, NetLabel, Orientation, Part, Point, Schematic, Wire};

    fn schematic() -> Schematic {
        Schematic {
            components: vec![Component {
                id: 1,
                name: "R1".into(),
                position: Point::new(0, 0),
                orientation: Orientation::default(),
                part: Part::Resistor { resistance: 1e3 },
            }],
            wires: vec![Wire {
                id: 2,
                points: vec![Point::new(4, 0), Point::new(16, 0), Point::new(16, 12)],
            }],
            labels: vec![NetLabel {
                id: 3,
                name: "out".into(),
                position: Point::new(16, 12),
            }],
            next_id: 4,
        }
    }

    #[test]
    fn pins_win_over_wires_and_bodies() {
        let s = schematic();
        assert_eq!(
            hit_test(&s, Vec2::new(4.2, 0.2), 0.8),
            Some(Hit::Pin {
                component: 1,
                pin: 1,
                point: Point::new(4, 0)
            })
        );
        assert_eq!(
            hit_test(&s, Vec2::new(0.6, 0.4), 0.8),
            Some(Hit::Component(1))
        );
        assert_eq!(
            hit_test(&s, Vec2::new(10.0, 0.4), 0.8),
            Some(Hit::Wire {
                wire: 2,
                segment: 0
            })
        );
        assert_eq!(
            hit_test(&s, Vec2::new(16.2, 6.0), 0.8),
            Some(Hit::Wire {
                wire: 2,
                segment: 1
            })
        );
        assert_eq!(
            hit_test(&s, Vec2::new(19.0, 12.0), 0.8),
            Some(Hit::Label(3))
        );
        assert_eq!(hit_test(&s, Vec2::new(8.0, 8.0), 0.8), None);
    }

    #[test]
    fn enclosing_boxes_need_whole_objects_and_crossing_boxes_any_part() {
        let s = schematic();
        let r = Rect::from_corners(Vec2::new(-6.0, -4.0), Vec2::new(10.0, 4.0));
        assert_eq!(
            objects_in_box(&s, r, BoxMode::Enclosed),
            vec![ObjectRef::Component(1)]
        );
        assert_eq!(
            objects_in_box(&s, r, BoxMode::Touching),
            vec![ObjectRef::Component(1), ObjectRef::Wire(2)]
        );
        assert_eq!(
            BoxMode::for_drag(Vec2::new(5.0, 0.0), Vec2::new(1.0, 3.0)),
            BoxMode::Touching
        );
    }
}
