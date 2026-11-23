//! Moving a selection while keeping its connections (Q48, Q60).
//!
//! Selected parts and labels translate rigidly. Wires stretch and reroute so
//! that horizontal and vertical segments stay horizontal and vertical:
//! * an unselected wire whose end touches something that moves follows with
//!   that end; a moved pin or label on a wire's interior first splits the
//!   wire there, so both halves follow;
//! * a selected wire translates, except ends that touch fixed geometry
//!   (unselected pins, labels or wires), which stay and stretch;
//! * a stretched end first shifts its free neighbouring bend, else a Z or L
//!   jog is inserted; diagonal segments simply follow their end;
//! * a moved part's pin that sits directly on a pin of a part that stays
//!   (connected without a wire) gets a new orthogonal wire to that pin, so
//!   the connection follows (D20). An unselected wire ending at that point
//!   stays with the pin that stays; the new wire carries the connection.
//!   It avoids other pins, labels and wires, which it would connect to.
//!   Once inserted it is an ordinary wire, stretched by later moves.
//!
//! A [`MovePlan`] is built once when a drag starts and applied to the total
//! displacement on every mouse move, so the preview never accumulates jogs.
//! Arrow-key nudges use the same plan; rotating and mirroring do not, and
//! still disconnect (Q60).

use crate::model::cleanup::normalize;
use crate::model::geometry::{on_segment, on_segment_interior};
use crate::model::ops;
use crate::model::routing::{Axis, reconnect};
use crate::model::selection::Selection;
use std::collections::{BTreeMap, BTreeSet};
use xsim_project::{Component, Id, Point, Schematic, Wire};

/// Which vertices of one wire move with the selection.
#[derive(Debug, Clone)]
struct WirePlan {
    id: Id,
    moved: Vec<bool>,
    /// A selected straight wire held at both ends: it moves sideways along
    /// this axis's normal, joined to its ends by two perpendicular legs.
    sideways: Option<Axis>,
}

/// A point where a pin of a moving part touches a pin of a part that
/// stays, connected without a wire (D20).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PinBridge {
    /// Where the pins touch before the move; the pin that stays is here.
    at: Point,
    /// The axis the inserted wire leaves the pin that stays along.
    axis: Option<Axis>,
}

/// A prepared move of a selection.
#[derive(Debug, Clone)]
pub struct MovePlan {
    base: Schematic,
    selection: Selection,
    wires: Vec<WirePlan>,
    /// Points other objects touch, which must not be shifted.
    anchors: BTreeSet<Point>,
    /// Pin-to-pin connections that get a wire once the move is non-zero.
    bridges: Vec<PinBridge>,
}

impl MovePlan {
    /// Prepares moving `selection` within `schematic`.
    pub fn new(schematic: &Schematic, selection: &Selection) -> Self {
        let mut base = schematic.clone();
        let rigid_moving = rigid_points(&base, selection, true);
        let bridges = pin_bridges(&base, selection);
        // Unselected wires at a bridged point stay with the pin that stays.
        let held: BTreeSet<Point> = bridges.iter().map(|b| b.at).collect();
        let pulling: BTreeSet<Point> = rigid_moving.difference(&held).copied().collect();
        split_unselected_wires(&mut base, selection, &pulling);
        let mut moving = Moving {
            points: rigid_moving,
            segments: Vec::new(),
            held,
        };
        let mut wires = selected_wire_plans(&base, selection, &mut moving);
        wires.extend(follower_plans(&base, selection, &moving));
        let anchors = all_anchors(&base);
        Self {
            base,
            selection: selection.clone(),
            wires,
            anchors,
            bridges,
        }
    }

    /// The schematic with the selection moved by `delta`, wires stretched
    /// and cleaned up.
    pub fn apply(&self, delta: Point) -> Schematic {
        let mut s = self.base.clone();
        if delta == Point::default() {
            return s;
        }
        for c in &mut s.components {
            if self.selection.has_component(c.id) {
                c.position = c.position + delta;
            }
        }
        for l in &mut s.labels {
            if self.selection.has_label(l.id) {
                l.position = l.position + delta;
            }
        }
        for plan in &self.wires {
            if let Some(w) = s.wires.iter_mut().find(|w| w.id == plan.id) {
                w.points = match plan.sideways {
                    Some(axis) => sideways(&w.points, axis, delta),
                    None => stretch(&w.points, &plan.moved, delta, |p| self.anchors.contains(&p)),
                };
            }
        }
        for bridge in &self.bridges {
            let route = bridge_route(*bridge, delta, &s);
            ops::add_wire(&mut s, route);
        }
        normalize(&mut s);
        s
    }
}

/// Every point where a pin of a selected part touches a pin of an
/// unselected one: one bridge per point, however many pins meet there.
fn pin_bridges(s: &Schematic, selection: &Selection) -> Vec<PinBridge> {
    let mut moving: BTreeMap<Point, &Component> = BTreeMap::new();
    for c in s
        .components
        .iter()
        .filter(|c| selection.has_component(c.id))
    {
        for pin in c.pins() {
            moving.entry(pin).or_insert(c);
        }
    }
    let mut bridges: BTreeMap<Point, PinBridge> = BTreeMap::new();
    for c in s
        .components
        .iter()
        .filter(|c| !selection.has_component(c.id))
    {
        for pin in c.pins() {
            if let Some(mover) = moving.get(&pin) {
                bridges.entry(pin).or_insert_with(|| PinBridge {
                    at: pin,
                    axis: outward_axis(c, pin).or_else(|| outward_axis(mover, pin)),
                });
            }
        }
    }
    bridges.into_values().collect()
}

/// The axis a wire leaves `pin` of `component` along: away from the centre
/// of the symbol's body box. `None` for a pin at that centre.
fn outward_axis(component: &Component, pin: Point) -> Option<Axis> {
    let (a, b) = component.bounds();
    // Twice the offset from the centre, so half units stay exact.
    let offset = Point::new(2 * pin.x - a.x - b.x, 2 * pin.y - a.y - b.y);
    (offset != Point::default()).then(|| Axis::dominant(offset))
}

/// The wire keeping a bridged pin pair connected after a move by `delta`,
/// in `s` (the drawing with everything else already moved): from the pin
/// that stays to the moved one, straight when they line up, else a Z (an L
/// when there is no room) leaving the pin that stays along its axis.
///
/// A wire touching any other pin, label or wire there would connect it
/// (Q50) and could short a part, for example when a part is dragged past
/// its partner. Then the shortest orthogonal route with at most two bends
/// that touches nothing but the two pins is taken instead. Only when there
/// is none does the first route stay.
fn bridge_route(bridge: PinBridge, delta: Point, s: &Schematic) -> Vec<Point> {
    let moved = bridge.at + delta;
    let axis = bridge.axis.unwrap_or_else(|| Axis::dominant(delta));
    let mut preferred = vec![bridge.at];
    preferred.extend(reconnect(moved, bridge.at, axis).into_iter().rev());
    preferred.push(moved);
    preferred.dedup();
    if is_clear(&preferred, s) {
        return preferred;
    }
    detours(bridge.at, moved)
        .into_iter()
        .find(|route| is_clear(route, s))
        .unwrap_or(preferred)
}

/// How far, in grid units, a detour may run outside the box spanned by the
/// two pins it joins.
const DETOUR_MARGIN: i32 = 4;

/// Orthogonal routes from `a` to `b` with at most two bends: each runs from
/// `a` to a horizontal or vertical line, along it, then to `b`, with the
/// line at most [`DETOUR_MARGIN`] outside the box spanned by `a` and `b`.
/// Shortest first, then with fewer bends, then with the line nearest the
/// middle. Routes doubling back on themselves are left out.
fn detours(a: Point, b: Point) -> Vec<Vec<Point>> {
    let mut routes: Vec<((i32, usize, i32), Vec<Point>)> = Vec::new();
    let mut push = |mut points: Vec<Point>, off_middle: i32| {
        points.dedup();
        let doubles_back = points
            .windows(3)
            .any(|g| Axis::of(g[0], g[1]) == Axis::of(g[1], g[2]));
        if doubles_back {
            return;
        }
        let length = points
            .windows(2)
            .map(|g| (g[1].x - g[0].x).abs() + (g[1].y - g[0].y).abs())
            .sum();
        let bends = points.len().saturating_sub(2);
        routes.push(((length, bends, off_middle), points));
    };
    for y in a.y.min(b.y) - DETOUR_MARGIN..=a.y.max(b.y) + DETOUR_MARGIN {
        let points = vec![a, Point::new(a.x, y), Point::new(b.x, y), b];
        push(points, (2 * y - a.y - b.y).abs());
    }
    for x in a.x.min(b.x) - DETOUR_MARGIN..=a.x.max(b.x) + DETOUR_MARGIN {
        let points = vec![a, Point::new(x, a.y), Point::new(x, b.y), b];
        push(points, (2 * x - a.x - b.x).abs());
    }
    routes.sort_by_key(|(key, _)| *key);
    routes.into_iter().map(|(_, points)| points).collect()
}

/// Whether a new wire along `route` would touch nothing in `s` but at its
/// two ends: no pin, label or wire vertex lies on it, and none of its bends
/// lies on a wire.
fn is_clear(route: &[Point], s: &Schematic) -> bool {
    let (Some(&a), Some(&b)) = (route.first(), route.last()) else {
        return true;
    };
    let on_route = |p: Point| route.windows(2).any(|g| on_segment(p, g[0], g[1]));
    let mut touched = s
        .components
        .iter()
        .flat_map(|c| c.pins())
        .chain(s.labels.iter().map(|l| l.position))
        .chain(s.wires.iter().flat_map(|w| w.points.iter().copied()))
        .filter(|&p| p != a && p != b);
    if touched.any(on_route) {
        return false;
    }
    let bends = route.get(1..route.len().saturating_sub(1)).unwrap_or(&[]);
    !bends.iter().any(|&bend| {
        s.wires
            .iter()
            .any(|w| w.points.windows(2).any(|g| on_segment(bend, g[0], g[1])))
    })
}

/// Everything that moves with the selection, before the move.
struct Moving {
    points: BTreeSet<Point>,
    /// Segments of selected wires whose both ends move.
    segments: Vec<(Point, Point)>,
    /// Bridged points ([`PinBridge`]): unselected wire ends there stay.
    held: BTreeSet<Point>,
}

impl Moving {
    /// Whether an unselected wire end at `p` must follow the move.
    fn pulls(&self, p: Point) -> bool {
        if self.held.contains(&p) {
            return false;
        }
        self.points.contains(&p)
            || self
                .segments
                .iter()
                .any(|&(a, b)| on_segment_interior(p, a, b))
    }
}

/// Plans for the selected wires: everything moves except ends held by
/// fixed geometry. Records what moves in `moving`.
fn selected_wire_plans(
    base: &Schematic,
    selection: &Selection,
    moving: &mut Moving,
) -> Vec<WirePlan> {
    let fixed = fixed_geometry(base, selection);
    let rigid_moving = moving.points.clone();
    let mut plans = Vec::new();
    for w in base.wires.iter().filter(|w| selection.has_wire(w.id)) {
        let n = w.points.len();
        let moved: Vec<bool> = w
            .points
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let end = i == 0 || i + 1 == n;
                !(end && fixed.touches(*p) && !rigid_moving.contains(p))
            })
            .collect();
        for (i, p) in w.points.iter().enumerate() {
            if moved[i] {
                moving.points.insert(*p);
            }
            if i + 1 < n && moved[i] && moved[i + 1] {
                moving.segments.push((*p, w.points[i + 1]));
            }
        }
        let sideways = (n == 2 && !moved[0] && !moved[1])
            .then(|| Axis::of(w.points[0], w.points[1]))
            .flatten();
        plans.push(WirePlan {
            id: w.id,
            moved,
            sideways,
        });
    }
    plans
}

/// Plans for unselected wires with an end pulled along by the move; a wire
/// pulled at both ends simply translates.
fn follower_plans(base: &Schematic, selection: &Selection, moving: &Moving) -> Vec<WirePlan> {
    base.wires
        .iter()
        .filter(|w| !selection.has_wire(w.id))
        .filter_map(|w| {
            let n = w.points.len();
            let start = moving.pulls(*w.points.first()?);
            let end = moving.pulls(*w.points.last()?);
            if !start && !end {
                return None;
            }
            let moved = (0..n)
                .map(|i| (start && end) || (i == 0 && start) || (i + 1 == n && end))
                .collect();
            Some(WirePlan {
                id: w.id,
                moved,
                sideways: None,
            })
        })
        .collect()
}

/// Pins and label positions of selected (`selected = true`) or unselected
/// parts and labels.
fn rigid_points(s: &Schematic, selection: &Selection, selected: bool) -> BTreeSet<Point> {
    let pins = s
        .components
        .iter()
        .filter(|c| selection.has_component(c.id) == selected)
        .flat_map(|c| c.pins());
    let labels = s
        .labels
        .iter()
        .filter(|l| selection.has_label(l.id) == selected)
        .map(|l| l.position);
    pins.chain(labels).collect()
}

/// Geometry that stays put during the move.
struct FixedGeometry<'a> {
    points: BTreeSet<Point>,
    wires: Vec<&'a Wire>,
}

impl FixedGeometry<'_> {
    fn touches(&self, p: Point) -> bool {
        self.points.contains(&p)
            || self.wires.iter().any(|w| {
                w.points.contains(&p)
                    || w.points
                        .windows(2)
                        .any(|s| on_segment_interior(p, s[0], s[1]))
            })
    }
}

fn fixed_geometry<'a>(s: &'a Schematic, selection: &Selection) -> FixedGeometry<'a> {
    FixedGeometry {
        points: rigid_points(s, selection, false),
        wires: s
            .wires
            .iter()
            .filter(|w| !selection.has_wire(w.id))
            .collect(),
    }
}

/// Points other objects touch: pins, labels and vertices shared by wires.
fn all_anchors(s: &Schematic) -> BTreeSet<Point> {
    let mut owners: BTreeMap<Point, BTreeSet<Id>> = BTreeMap::new();
    for w in &s.wires {
        for &p in &w.points {
            owners.entry(p).or_default().insert(w.id);
        }
    }
    let shared = owners
        .into_iter()
        .filter(|(_, ids)| ids.len() > 1)
        .map(|(p, _)| p);
    rigid_points(s, &Selection::default(), false)
        .into_iter()
        .chain(shared)
        .collect()
}

/// A straight wire held at both ends, its middle dragged by the
/// perpendicular part of `d`.
fn sideways(points: &[Point], axis: Axis, d: Point) -> Vec<Point> {
    let offset = match axis {
        Axis::Horizontal => Point::new(0, d.y),
        Axis::Vertical => Point::new(d.x, 0),
    };
    match points {
        [a, b] => vec![*a, *a + offset, *b + offset, *b],
        _ => points.to_vec(),
    }
}

/// Splits unselected wires wherever a moving pin or label touches their
/// interior, so each piece can follow with its end.
fn split_unselected_wires(s: &mut Schematic, selection: &Selection, at: &BTreeSet<Point>) {
    let mut index = 0;
    while index < s.wires.len() {
        let wire = &s.wires[index];
        if selection.has_wire(wire.id) {
            index += 1;
            continue;
        }
        let mut pieces = split_points(&wire.points, at).into_iter();
        if let Some(first) = pieces.next() {
            s.wires[index].points = first;
        }
        let rest: Vec<Vec<Point>> = pieces.collect();
        for points in rest {
            let id = s.allocate_id();
            s.wires.insert(index + 1, Wire { id, points });
            index += 1;
        }
        index += 1;
    }
}

/// Cuts a polyline at every interior point listed in `at`.
fn split_points(points: &[Point], at: &BTreeSet<Point>) -> Vec<Vec<Point>> {
    let Some(&first) = points.first() else {
        return Vec::new();
    };
    let mut pieces = vec![vec![first]];
    let last_vertex = points.len().saturating_sub(1);
    for (i, segment) in points.windows(2).enumerate() {
        let (a, b) = (segment[0], segment[1]);
        let mut inner: Vec<Point> = at
            .iter()
            .copied()
            .filter(|&p| on_segment_interior(p, a, b))
            .collect();
        inner.sort_by_key(|p| (p.x - a.x).abs() + (p.y - a.y).abs());
        for p in inner {
            push_point(&mut pieces, p);
            pieces.push(vec![p]);
        }
        push_point(&mut pieces, b);
        if i + 1 < last_vertex && at.contains(&b) {
            pieces.push(vec![b]);
        }
    }
    pieces
}

fn push_point(pieces: &mut [Vec<Point>], p: Point) {
    if let Some(piece) = pieces.last_mut() {
        piece.push(p);
    }
}

/// Moves the flagged vertices of a polyline by `d`, keeping originally
/// horizontal/vertical segments orthogonal. `anchored` tells which points
/// other objects touch (those bends are never shifted).
fn stretch(
    points: &[Point],
    moved: &[bool],
    d: Point,
    anchored: impl Fn(Point) -> bool,
) -> Vec<Point> {
    let shift = vertex_shifts(points, moved, d, anchored);
    let mut out = Vec::with_capacity(points.len() + 4);
    for (i, &original) in points.iter().enumerate() {
        let p = original + shift[i];
        if i > 0 {
            out.extend(jog(
                points[i - 1],
                original,
                points[i - 1] + shift[i - 1],
                p,
                moved[i - 1],
            ));
        }
        out.push(p);
    }
    out
}

/// Displacement of every vertex: `d` for moved ones, plus the perpendicular
/// part of `d` for a free bend next to a moved end, which keeps both of its
/// segments straight.
fn vertex_shifts(
    points: &[Point],
    moved: &[bool],
    d: Point,
    anchored: impl Fn(Point) -> bool,
) -> Vec<Point> {
    let n = points.len();
    let mut shift: Vec<Point> = moved
        .iter()
        .map(|&m| if m { d } else { Point::default() })
        .collect();
    for i in 0..n.saturating_sub(1) {
        if moved[i] == moved[i + 1] {
            continue;
        }
        let Some(axis) = Axis::of(points[i], points[i + 1]) else {
            continue;
        };
        let (fixed, outward) = if moved[i] {
            (i + 1, i + 2)
        } else {
            (i, i.wrapping_sub(1))
        };
        let interior = fixed > 0 && fixed + 1 < n;
        let free = interior && !anchored(points[fixed]) && shift[fixed] == Point::default();
        let perpendicular_next = outward < n
            && !moved[outward]
            && Axis::of(points[fixed], points[outward]) == Some(axis.other());
        if free && perpendicular_next {
            shift[fixed] = match axis {
                Axis::Horizontal => Point::new(0, d.y),
                Axis::Vertical => Point::new(d.x, 0),
            };
        }
    }
    shift
}

/// Corners needed between the moved ends `prev` and `p` of an originally
/// orthogonal segment `a`–`b` that would otherwise turn diagonal.
fn jog(a: Point, b: Point, prev: Point, p: Point, prev_moved: bool) -> Vec<Point> {
    let Some(axis) = Axis::of(a, b) else {
        return Vec::new();
    };
    if prev == p || Axis::of(prev, p).is_some() {
        return Vec::new();
    }
    if prev_moved {
        reconnect(prev, p, axis)
    } else {
        let mut back = reconnect(p, prev, axis);
        back.reverse();
        back
    }
}

#[cfg(test)]
mod tests {
    use super::{MovePlan, split_points, stretch};
    use crate::model::selection::{ObjectRef, Selection};
    use std::collections::BTreeSet;
    use xsim_project::{
        Component, NetLabel, Orientation, Part, Point, Schematic, Wire, connectivity,
    };

    fn p(x: i32, y: i32) -> Point {
        Point::new(x, y)
    }

    fn resistor(id: u64, at: Point) -> Component {
        Component {
            id,
            name: format!("R{id}"),
            position: at,
            orientation: Orientation::default(),
            part: Part::Resistor { resistance: 1e3 },
        }
    }

    fn wire(id: u64, points: &[Point]) -> Wire {
        Wire {
            id,
            points: points.to_vec(),
        }
    }

    fn is_orthogonal(a: Point, b: Point) -> bool {
        a.x == b.x || a.y == b.y
    }

    fn assert_orthogonal(s: &Schematic) {
        for w in &s.wires {
            for seg in w.points.windows(2) {
                assert!(is_orthogonal(seg[0], seg[1]), "diagonal in {:?}", w.points);
            }
        }
    }

    /// R1 at the origin wired to R2 at (8, 0): pins (2,0) and (6,0).
    fn two_resistors() -> Schematic {
        Schematic {
            components: vec![resistor(1, p(0, 0)), resistor(2, p(8, 0))],
            wires: vec![wire(3, &[p(2, 0), p(6, 0)])],
            next_id: 4,
            ..Schematic::default()
        }
    }

    fn select(objects: &[ObjectRef]) -> Selection {
        objects.iter().copied().collect()
    }

    #[test]
    fn moving_along_a_wire_just_lengthens_it() {
        let s = two_resistors();
        let moved = MovePlan::new(&s, &select(&[ObjectRef::Component(1)])).apply(p(-3, 0));
        assert_eq!(moved.wires[0].points, vec![p(-1, 0), p(6, 0)]);
    }

    #[test]
    fn moving_sideways_inserts_an_orthogonal_jog() {
        let s = two_resistors();
        let moved = MovePlan::new(&s, &select(&[ObjectRef::Component(1)])).apply(p(0, -2));
        assert_orthogonal(&moved);
        let w = &moved.wires[0];
        assert_eq!(w.points.first(), Some(&p(2, -2)));
        assert_eq!(w.points.last(), Some(&p(6, 0)));
        assert_eq!(w.points, vec![p(2, -2), p(4, -2), p(4, 0), p(6, 0)]);
    }

    #[test]
    fn a_free_bend_shifts_instead_of_adding_a_jog() {
        let mut s = two_resistors();
        s.components[1] = Component {
            orientation: Orientation {
                rotation: 1,
                ..Orientation::default()
            },
            ..resistor(2, p(6, 6))
        };
        s.wires[0] = wire(3, &[p(2, 0), p(6, 0), p(6, 4)]);
        let moved = MovePlan::new(&s, &select(&[ObjectRef::Component(1)])).apply(p(1, -2));
        assert_eq!(moved.wires[0].points, vec![p(3, -2), p(6, -2), p(6, 4)]);
    }

    #[test]
    fn moving_everything_translates_wires() {
        let s = two_resistors();
        let all = Selection::all(&s);
        let moved = MovePlan::new(&s, &all).apply(p(3, 5));
        assert_eq!(moved.wires[0].points, vec![p(5, 5), p(9, 5)]);
        assert_eq!(moved.components[1].position, p(11, 5));
    }

    #[test]
    fn an_unselected_wire_between_two_moved_parts_translates() {
        let s = two_resistors();
        let both = select(&[ObjectRef::Component(1), ObjectRef::Component(2)]);
        let moved = MovePlan::new(&s, &both).apply(p(0, 3));
        assert_eq!(moved.wires[0].points, vec![p(2, 3), p(6, 3)]);
    }

    #[test]
    fn a_selected_wire_keeps_ends_on_fixed_pins() {
        let s = two_resistors();
        let moved = MovePlan::new(&s, &select(&[ObjectRef::Wire(3)])).apply(p(0, 2));
        assert_orthogonal(&moved);
        assert_eq!(
            moved.wires[0].points,
            vec![p(2, 0), p(2, 2), p(6, 2), p(6, 0)]
        );
    }

    #[test]
    fn a_pin_on_a_wire_interior_splits_the_wire_so_both_halves_follow() {
        let s = Schematic {
            components: vec![resistor(1, p(0, 0))],
            wires: vec![wire(2, &[p(2, -4), p(2, 4)])],
            next_id: 3,
            ..Schematic::default()
        };
        let moved = MovePlan::new(&s, &select(&[ObjectRef::Component(1)])).apply(p(-2, 0));
        assert_orthogonal(&moved);
        let ends: BTreeSet<Point> = moved
            .wires
            .iter()
            .flat_map(|w| [w.points[0], w.points[w.points.len() - 1]])
            .collect();
        assert!(ends.contains(&p(0, 0)), "both halves end at the moved pin");
        assert!(ends.contains(&p(2, -4)) && ends.contains(&p(2, 4)));
    }

    #[test]
    fn labels_move_with_the_selection_and_pull_their_wire() {
        let s = Schematic {
            wires: vec![wire(1, &[p(0, 0), p(4, 0)])],
            labels: vec![NetLabel {
                id: 2,
                name: "out".into(),
                position: p(4, 0),
            }],
            next_id: 3,
            ..Schematic::default()
        };
        let moved = MovePlan::new(&s, &select(&[ObjectRef::Label(2)])).apply(p(2, 0));
        assert_eq!(moved.labels[0].position, p(6, 0));
        assert_eq!(moved.wires[0].points, vec![p(0, 0), p(6, 0)]);
    }

    #[test]
    fn a_zero_move_changes_nothing() {
        let s = two_resistors();
        let moved = MovePlan::new(&s, &select(&[ObjectRef::Component(1)])).apply(p(0, 0));
        assert_eq!(moved, s);
    }

    #[test]
    fn degenerate_wires_from_a_file_do_not_break_a_move() {
        let mut s = two_resistors();
        s.wires.push(wire(9, &[]));
        let moved = MovePlan::new(&s, &select(&[ObjectRef::Component(1)])).apply(p(0, 1));
        assert_eq!(moved.components[0].position, p(0, 1));
    }

    #[test]
    fn diagonal_segments_just_follow_their_end() {
        let out = stretch(&[p(0, 0), p(4, 4)], &[true, false], p(1, 0), |_| false);
        assert_eq!(out, vec![p(1, 0), p(4, 4)]);
    }

    #[test]
    fn splitting_cuts_at_interior_vertices_and_segment_points() {
        let at: BTreeSet<Point> = [p(2, 0), p(4, 0), p(4, 3)].into_iter().collect();
        let pieces = split_points(&[p(0, 0), p(4, 0), p(4, 6)], &at);
        assert_eq!(
            pieces,
            vec![
                vec![p(0, 0), p(2, 0)],
                vec![p(2, 0), p(4, 0)],
                vec![p(4, 0), p(4, 3)],
                vec![p(4, 3), p(4, 6)],
            ]
        );
    }

    /// R1 at the origin and R2 at (4, 0), touching pin to pin at (2, 0)
    /// with no wire.
    fn touching_resistors() -> Schematic {
        Schematic {
            components: vec![resistor(1, p(0, 0)), resistor(2, p(4, 0))],
            next_id: 3,
            ..Schematic::default()
        }
    }

    fn connected(s: &Schematic, a: (u64, usize), b: (u64, usize)) -> bool {
        let c = connectivity(s);
        c.net_of_pin(a.0, a.1).is_some() && c.net_of_pin(a.0, a.1) == c.net_of_pin(b.0, b.1)
    }

    fn points_of(s: &Schematic) -> Vec<Vec<Point>> {
        s.wires.iter().map(|w| w.points.clone()).collect()
    }

    #[test]
    fn touching_pins_get_a_wire_when_one_part_moves_along_their_axis() {
        let s = touching_resistors();
        assert!(connected(&s, (1, 1), (2, 0)), "touching pins connect");
        let moved = MovePlan::new(&s, &select(&[ObjectRef::Component(2)])).apply(p(3, 0));
        assert_eq!(moved.components[1].position, p(7, 0));
        assert_eq!(points_of(&moved), vec![vec![p(2, 0), p(5, 0)]]);
        assert!(connected(&moved, (1, 1), (2, 0)));
        assert!(!connected(&moved, (1, 0), (2, 1)), "R2 is not shorted");
    }

    #[test]
    fn a_part_moved_down_off_a_touching_pin_hangs_on_a_straight_wire() {
        let s = touching_resistors();
        let moved = MovePlan::new(&s, &select(&[ObjectRef::Component(2)])).apply(p(0, 3));
        assert_eq!(points_of(&moved), vec![vec![p(2, 0), p(2, 3)]]);
        assert!(connected(&moved, (1, 1), (2, 0)));
    }

    #[test]
    fn a_diagonal_move_routes_a_z_that_leaves_both_pins_along_their_axis() {
        let s = touching_resistors();
        let moved = MovePlan::new(&s, &select(&[ObjectRef::Component(2)])).apply(p(4, 3));
        assert_eq!(
            points_of(&moved),
            vec![vec![p(2, 0), p(4, 0), p(4, 3), p(6, 3)]]
        );
        assert_orthogonal(&moved);
        assert!(connected(&moved, (1, 1), (2, 0)));
        let one_unit = MovePlan::new(&s, &select(&[ObjectRef::Component(1)])).apply(p(-1, 3));
        assert_orthogonal(&one_unit);
        assert!(
            connected(&one_unit, (1, 1), (2, 0)),
            "no room for a Z: an L"
        );
    }

    #[test]
    fn a_part_touching_two_others_keeps_both_connections() {
        let mut s = touching_resistors();
        s.components.push(resistor(3, p(8, 0)));
        s.next_id = 4;
        let moved = MovePlan::new(&s, &select(&[ObjectRef::Component(2)])).apply(p(0, -2));
        assert_eq!(
            points_of(&moved),
            vec![vec![p(2, 0), p(2, -2)], vec![p(6, 0), p(6, -2)]]
        );
        assert!(connected(&moved, (1, 1), (2, 0)));
        assert!(connected(&moved, (2, 1), (3, 0)));
        assert!(!connected(&moved, (1, 1), (3, 0)), "R2 still sits between");
    }

    #[test]
    fn a_selection_bridges_only_to_partners_left_behind() {
        let mut s = touching_resistors();
        s.components.push(resistor(3, p(8, 0)));
        s.next_id = 4;
        let both = select(&[ObjectRef::Component(1), ObjectRef::Component(2)]);
        let moved = MovePlan::new(&s, &both).apply(p(0, 2));
        assert_eq!(
            points_of(&moved),
            vec![vec![p(6, 0), p(6, 2)]],
            "R1–R2 move together; only R3 needs a wire"
        );
        assert!(connected(&moved, (1, 1), (2, 0)));
        assert!(connected(&moved, (2, 1), (3, 0)));
        let all = MovePlan::new(&s, &Selection::all(&s)).apply(p(0, 2));
        assert!(all.wires.is_empty(), "nothing is left behind");
    }

    #[test]
    fn several_pins_leaving_one_point_get_a_single_wire() {
        let mut s = touching_resistors();
        s.components.push(Component {
            id: 3,
            name: "GND1".into(),
            position: p(2, 0),
            orientation: Orientation::default(),
            part: Part::Ground,
        });
        s.next_id = 4;
        let both = select(&[ObjectRef::Component(1), ObjectRef::Component(2)]);
        let moved = MovePlan::new(&s, &both).apply(p(0, -3));
        assert_eq!(
            points_of(&moved),
            vec![vec![p(2, 0), p(2, -3)]],
            "the ground pin leaves vertically"
        );
        assert!(connected(&moved, (1, 1), (3, 0)));
        assert!(connected(&moved, (2, 0), (3, 0)));
    }

    #[test]
    fn a_wire_ending_on_touching_pins_stays_with_the_pin_that_stays() {
        let mut s = touching_resistors();
        s.wires.push(wire(3, &[p(2, 0), p(2, 5)]));
        s.next_id = 4;
        let moved = MovePlan::new(&s, &select(&[ObjectRef::Component(1)])).apply(p(-3, 0));
        assert!(
            moved
                .wires
                .iter()
                .any(|w| w.points == vec![p(2, 0), p(2, 5)]),
            "the wire did not move: {:?}",
            moved.wires
        );
        assert!(
            moved
                .wires
                .iter()
                .any(|w| w.points == vec![p(2, 0), p(-1, 0)]),
            "a new wire reaches R1: {:?}",
            moved.wires
        );
        assert!(connected(&moved, (1, 1), (2, 0)));
        let c = connectivity(&moved);
        assert!(
            c.nets
                .iter()
                .any(|n| n.wires.len() == 2 && n.pins.len() == 2),
            "one net holds both pins and both wires"
        );
    }

    #[test]
    fn moving_back_onto_the_partner_removes_the_inserted_wire() {
        let s = touching_resistors();
        let r1 = select(&[ObjectRef::Component(1)]);
        let once = MovePlan::new(&s, &r1).apply(p(-1, 0));
        assert_eq!(points_of(&once), vec![vec![p(2, 0), p(1, 0)]]);
        let twice = MovePlan::new(&once, &r1).apply(p(-1, 0));
        assert_eq!(
            points_of(&twice),
            vec![vec![p(2, 0), p(0, 0)]],
            "the inserted wire stretches like any other"
        );
        let back = MovePlan::new(&twice, &r1).apply(p(2, 0));
        assert!(back.wires.is_empty(), "{:?}", back.wires);
        assert_eq!(back.components, s.components);
        assert!(connected(&back, (1, 1), (2, 0)));
    }

    /// Review: the inserted wire used to run over the far pins of both
    /// parts when one was dragged past the other, shorting each.
    #[test]
    fn a_part_dragged_past_its_partner_shorts_neither() {
        let s = touching_resistors();
        let r2 = select(&[ObjectRef::Component(2)]);
        for d in [p(-8, 3), p(-8, 0), p(-9, -1), p(6, 0), p(0, 1)] {
            let moved = MovePlan::new(&s, &r2).apply(d);
            assert_orthogonal(&moved);
            assert!(
                connected(&moved, (1, 1), (2, 0)),
                "{d:?}: {:?}",
                moved.wires
            );
            assert!(!connected(&moved, (1, 0), (1, 1)), "{d:?}: R1 shorted");
            assert!(!connected(&moved, (2, 0), (2, 1)), "{d:?}: R2 shorted");
        }
        let past = MovePlan::new(&s, &r2).apply(p(-8, 0));
        assert_eq!(
            points_of(&past),
            vec![vec![p(2, 0), p(2, -1), p(-6, -1), p(-6, 0)]],
            "the shortest detour clear of both parts"
        );
        let along = MovePlan::new(&s, &r2).apply(p(3, 0));
        assert_eq!(
            points_of(&along),
            vec![vec![p(2, 0), p(5, 0)]],
            "a clear route is kept"
        );
    }

    #[test]
    fn the_inserted_wire_avoids_other_wires_and_labels() {
        let mut s = touching_resistors();
        // A labelled wire under the parts ending right below the touching
        // point, where a straight wire down would join it (a T-junction).
        s.wires.push(wire(3, &[p(-4, 2), p(2, 2)]));
        s.labels.push(NetLabel {
            id: 4,
            name: "bus".into(),
            position: p(-4, 2),
        });
        s.next_id = 5;
        let moved = MovePlan::new(&s, &select(&[ObjectRef::Component(2)])).apply(p(0, 4));
        assert_orthogonal(&moved);
        assert!(connected(&moved, (1, 1), (2, 0)));
        let c = connectivity(&moved);
        assert!(
            !moved
                .wires
                .iter()
                .any(|w| w.id != 3 && w.points.contains(&p(2, 2))),
            "nothing ends on the bus: {:?}",
            moved.wires
        );
        for pin in [(1, 0), (1, 1), (2, 0), (2, 1)] {
            assert_ne!(
                c.net_of_wire(3),
                c.net_of_pin(pin.0, pin.1),
                "{pin:?} joined the bus: {:?}",
                moved.wires
            );
        }
    }

    #[test]
    fn a_label_on_the_touching_point_names_both_pins_after_the_move() {
        let mut s = touching_resistors();
        s.labels.push(NetLabel {
            id: 3,
            name: "mid".into(),
            position: p(2, 0),
        });
        s.next_id = 4;
        let moved = MovePlan::new(&s, &select(&[ObjectRef::Component(2)])).apply(p(2, 3));
        assert_eq!(moved.labels[0].position, p(2, 0), "the label stays");
        let c = connectivity(&moved);
        let mid = c.net_at(p(2, 0));
        assert_eq!(c.net_of_pin(1, 1), mid);
        assert_eq!(c.net_of_pin(2, 0), mid);
    }

    #[test]
    fn a_wire_passing_through_the_touching_point_stays_whole() {
        let mut s = touching_resistors();
        s.wires.push(wire(3, &[p(2, -4), p(2, 4)]));
        s.next_id = 4;
        let moved = MovePlan::new(&s, &select(&[ObjectRef::Component(2)])).apply(p(3, 0));
        assert!(
            moved
                .wires
                .iter()
                .any(|w| w.points == vec![p(2, -4), p(2, 4)]),
            "the wire is not split or pulled: {:?}",
            moved.wires
        );
        assert!(connected(&moved, (1, 1), (2, 0)));
        let c = connectivity(&moved);
        assert_eq!(c.net_at(p(2, -4)), c.net_of_pin(2, 0));
    }

    #[test]
    fn a_selected_wire_ending_on_the_touching_point_moves_with_the_part() {
        let mut s = touching_resistors();
        s.wires.push(wire(3, &[p(2, 0), p(2, 4)]));
        s.next_id = 4;
        let selection = select(&[ObjectRef::Component(2), ObjectRef::Wire(3)]);
        let moved = MovePlan::new(&s, &selection).apply(p(3, 0));
        assert!(
            moved
                .wires
                .iter()
                .any(|w| w.points == vec![p(5, 0), p(5, 4)]),
            "{:?}",
            moved.wires
        );
        assert!(connected(&moved, (1, 1), (2, 0)));
        let c = connectivity(&moved);
        assert_eq!(c.net_at(p(5, 4)), c.net_of_pin(1, 1));
    }

    #[test]
    fn a_wire_end_on_a_pin_still_just_stretches() {
        let s = two_resistors();
        let moved = MovePlan::new(&s, &select(&[ObjectRef::Component(2)])).apply(p(2, 0));
        assert_eq!(
            points_of(&moved),
            vec![vec![p(2, 0), p(8, 0)]],
            "no extra wire"
        );
    }
}
