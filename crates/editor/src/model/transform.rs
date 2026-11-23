//! Rotation and mirroring of selections (Q53).
//!
//! Per Q60 a rotation disconnects: wires that are not part of the selection
//! stay where they are. Mirroring behaves the same way.

use crate::model::selection::Selection;
use xsim_project::{Orientation, Point, Schematic};

/// Screen axis of a mirror operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mirror {
    /// Flip left–right (negate x).
    Horizontal,
    /// Flip top–bottom (negate y).
    Vertical,
}

/// `orientation` followed by a screen-space mirror. `Orientation` mirrors
/// before it rotates, so a screen mirror also reverses the rotation.
pub fn mirrored(orientation: Orientation, mirror: Mirror) -> Orientation {
    let rotation = (4 - orientation.rotation % 4) % 4;
    match mirror {
        Mirror::Horizontal => Orientation {
            rotation,
            mirror_x: !orientation.mirror_x,
            ..orientation
        },
        Mirror::Vertical => Orientation {
            rotation,
            mirror_y: !orientation.mirror_y,
            ..orientation
        },
    }
}

/// A point transform about a grid pivot.
#[derive(Debug, Clone, Copy)]
enum Transform {
    /// Clockwise quarter turn about the pivot.
    Rotate(Point),
    /// Mirror about the vertical (Horizontal) or horizontal (Vertical) line
    /// through the pivot; `twice_pivot` keeps half-grid pivots exact.
    Mirror { mirror: Mirror, twice_pivot: Point },
}

impl Transform {
    fn apply(self, p: Point) -> Point {
        match self {
            Transform::Rotate(c) => Point::new(c.x - (p.y - c.y), c.y + (p.x - c.x)),
            Transform::Mirror {
                mirror: Mirror::Horizontal,
                twice_pivot,
            } => Point::new(twice_pivot.x - p.x, p.y),
            Transform::Mirror {
                mirror: Mirror::Vertical,
                twice_pivot,
            } => Point::new(p.x, twice_pivot.y - p.y),
        }
    }

    fn orientation(self, o: Orientation) -> Orientation {
        match self {
            Transform::Rotate(_) => o.rotated(),
            Transform::Mirror { mirror, .. } => mirrored(o, mirror),
        }
    }
}

/// Grid extent `(min, max)` of the selected objects' positions and points.
fn extent(schematic: &Schematic, selection: &Selection) -> Option<(Point, Point)> {
    let components = schematic
        .components
        .iter()
        .filter(|c| selection.has_component(c.id))
        .flat_map(|c| {
            let (a, b) = c.bounds();
            [a, b]
        });
    let wires = schematic
        .wires
        .iter()
        .filter(|w| selection.has_wire(w.id))
        .flat_map(|w| w.points.iter().copied());
    let labels = schematic
        .labels
        .iter()
        .filter(|l| selection.has_label(l.id))
        .map(|l| l.position);
    components.chain(wires).chain(labels).fold(None, |acc, p| {
        Some(match acc {
            None => (p, p),
            Some((a, b)) => (
                Point::new(a.x.min(p.x), a.y.min(p.y)),
                Point::new(b.x.max(p.x), b.y.max(p.y)),
            ),
        })
    })
}

/// The pivot for a rotation: a single part turns about its own origin,
/// anything else about the grid point nearest the selection centre.
fn rotation_pivot(schematic: &Schematic, selection: &Selection) -> Option<Point> {
    if let Some(c) = selection
        .single_component()
        .and_then(|id| schematic.component(id))
    {
        return Some(c.position);
    }
    let (a, b) = extent(schematic, selection)?;
    Some(Point::new(
        (a.x + b.x).div_euclid(2),
        (a.y + b.y).div_euclid(2),
    ))
}

/// Twice the mirror axis position: the part origin for a single part, else
/// the selection centre (which may fall between grid points).
fn twice_mirror_pivot(schematic: &Schematic, selection: &Selection) -> Option<Point> {
    if let Some(c) = selection
        .single_component()
        .and_then(|id| schematic.component(id))
    {
        return Some(Point::new(c.position.x * 2, c.position.y * 2));
    }
    let (a, b) = extent(schematic, selection)?;
    Some(Point::new(a.x + b.x, a.y + b.y))
}

fn transform_selection(schematic: &mut Schematic, selection: &Selection, t: Transform) {
    for c in &mut schematic.components {
        if selection.has_component(c.id) {
            c.position = t.apply(c.position);
            c.orientation = t.orientation(c.orientation);
        }
    }
    for w in &mut schematic.wires {
        if selection.has_wire(w.id) {
            for p in &mut w.points {
                *p = t.apply(*p);
            }
        }
    }
    for l in &mut schematic.labels {
        if selection.has_label(l.id) {
            l.position = t.apply(l.position);
        }
    }
}

/// Rotates the selection a quarter turn clockwise. Returns `false` when
/// nothing is selected.
pub fn rotate_selection(schematic: &mut Schematic, selection: &Selection) -> bool {
    let Some(pivot) = rotation_pivot(schematic, selection) else {
        return false;
    };
    transform_selection(schematic, selection, Transform::Rotate(pivot));
    true
}

/// Mirrors the selection on screen. Returns `false` when nothing is selected.
pub fn mirror_selection(schematic: &mut Schematic, selection: &Selection, mirror: Mirror) -> bool {
    let Some(twice_pivot) = twice_mirror_pivot(schematic, selection) else {
        return false;
    };
    transform_selection(
        schematic,
        selection,
        Transform::Mirror {
            mirror,
            twice_pivot,
        },
    );
    true
}

#[cfg(test)]
mod tests {
    use super::{Mirror, mirror_selection, mirrored, rotate_selection};
    use crate::model::selection::{ObjectRef, Selection};
    use xsim_project::{Component, Orientation, Part, PartKind, Point, Schematic, Wire};

    fn part(kind: PartKind, position: Point, orientation: Orientation) -> Component {
        Component {
            id: 1,
            name: "X1".into(),
            position,
            orientation,
            part: match kind {
                PartKind::NpnBjt => Part::Bjt {
                    polarity: xsim_spice_engine::Polarity::N,
                    model: "M".into(),
                },
                _ => Part::Resistor { resistance: 1.0 },
            },
        }
    }

    fn all_orientations() -> impl Iterator<Item = Orientation> {
        (0..4).flat_map(|rotation| {
            [(false, false), (true, false), (false, true), (true, true)]
                .into_iter()
                .map(move |(mirror_x, mirror_y)| Orientation {
                    rotation,
                    mirror_x,
                    mirror_y,
                })
        })
    }

    #[test]
    fn screen_mirrors_reflect_the_pins_for_all_sixteen_orientations() {
        let origin = Point::new(3, -2);
        for o in all_orientations() {
            let c = part(PartKind::NpnBjt, origin, o);
            for mirror in [Mirror::Horizontal, Mirror::Vertical] {
                let m = Component {
                    orientation: mirrored(o, mirror),
                    ..c.clone()
                };
                let expected: Vec<Point> = c
                    .pins()
                    .into_iter()
                    .map(|p| match mirror {
                        Mirror::Horizontal => Point::new(2 * origin.x - p.x, p.y),
                        Mirror::Vertical => Point::new(p.x, 2 * origin.y - p.y),
                    })
                    .collect();
                assert_eq!(m.pins(), expected, "{o:?} mirrored {mirror:?}");
            }
        }
    }

    fn schematic() -> Schematic {
        Schematic {
            components: vec![part(
                PartKind::Resistor,
                Point::new(0, 0),
                Orientation::default(),
            )],
            wires: vec![
                Wire {
                    id: 2,
                    points: vec![Point::new(2, 0), Point::new(6, 0)],
                },
                Wire {
                    id: 3,
                    points: vec![Point::new(-6, 0), Point::new(-2, 0)],
                },
            ],
            next_id: 4,
            ..Schematic::default()
        }
    }

    #[test]
    fn rotating_a_part_disconnects_it_from_unselected_wires() {
        let mut s = schematic();
        let selection: Selection = [ObjectRef::Component(1)].into_iter().collect();
        assert!(rotate_selection(&mut s, &selection));
        let c = &s.components[0];
        assert_eq!(c.position, Point::new(0, 0));
        assert_eq!(c.orientation.rotation, 1);
        assert_eq!(c.pins(), vec![Point::new(0, -2), Point::new(0, 2)]);
        assert_eq!(s.wires, schematic().wires, "wires stay where they were");
    }

    #[test]
    fn rotating_a_group_turns_everything_about_its_centre() {
        let mut s = schematic();
        let selection = Selection::all(&s);
        rotate_selection(&mut s, &selection);
        assert_eq!(s.components[0].position, Point::new(0, 0));
        assert_eq!(s.wires[0].points, vec![Point::new(0, 2), Point::new(0, 6)]);
        assert_eq!(s.components[0].pins()[1], Point::new(0, 2));
    }

    #[test]
    fn mirroring_a_group_reflects_about_its_centre() {
        let mut s = schematic();
        s.wires.truncate(1);
        let selection = Selection::all(&s);
        mirror_selection(&mut s, &selection, Mirror::Horizontal);
        assert_eq!(s.components[0].position, Point::new(4, 0));
        assert_eq!(s.wires[0].points, vec![Point::new(2, 0), Point::new(-2, 0)]);
        assert!(s.components[0].pins().contains(&Point::new(2, 0)));
    }

    #[test]
    fn nothing_selected_is_a_no_op() {
        let mut s = schematic();
        assert!(!rotate_selection(&mut s, &Selection::default()));
        assert!(!mirror_selection(
            &mut s,
            &Selection::default(),
            Mirror::Vertical
        ));
        assert_eq!(s, schematic());
    }
}
