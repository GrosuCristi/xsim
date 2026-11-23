//! Dragging one straight run of a wire sideways to realign it (D14),
//! KiCad/LTspice style.
//!
//! The dragged segment is the straight run through the pressed segment:
//! consecutive segments of the wire that continue in the same direction. A
//! vertex that only exists because something touches it (a label, a
//! T-junction, a pin) therefore does not split what looks like one line.
//! The run moves perpendicular to itself, by whole grid units, and the rest
//! of the wire stays where it is.
//!
//! Connections follow the netlist rules (Q50): pins, labels and other
//! wires' vertices at a vertex of the run, pins, labels and other wires'
//! *ends* on its interior, and the wire's own ends lying on another wire.
//! A bend of another wire on the run's interior, or a plain crossing, is not
//! a connection and is ignored. At each connected point of the run:
//! * a point that cannot follow is **held**: a pin, another wire's bend at a
//!   vertex of the run, another wire's end whose last segment does not run
//!   along the drag direction, or a label or following wire end that also
//!   joins a wire which does not follow (a crossing, a bend). A held end of
//!   the run keeps its vertex and gets a new bend; a held point inside the
//!   run gets a stub wire to the moved run;
//! * otherwise other wires ending there follow (their last segment runs
//!   along the drag direction, so it just gets longer or shorter; only that
//!   end moves, even when both ends of a wire are on the run), and labels
//!   there move with the run.
//!
//! An end of the wire lying on another wire's segment that runs along the
//! drag direction (a branch joining a trunk) slides along that segment while
//! it stays on it, and keeps its vertex with a bend beyond. An end of the
//! run whose neighbouring segment runs along the drag direction moves with
//! the run, so that neighbour stretches. Connections that a shortened
//! neighbour, or a following wire's shortened end segment, leaves behind
//! are kept by a stub up to the farthest one. Any other end
//! keeps its vertex and gets a bend. Wires that touch nothing, and diagonal
//! runs, are not segment-dragged: the caller moves the whole wire instead.
//! The result is cleaned up by [`normalize`], which merges zero-length and
//! collinear leftovers (a run dragged onto another leg of its own wire
//! folds away with it, and a stub keeps whatever was connected there).
//!
//! As a safety net, an offset at which following would still disconnect
//! something (a follower collapsing onto a mere crossing, say) is drawn
//! holding everything instead: both ends keep their vertices and every
//! connected point of the run gets a stub. The preview can therefore change
//! style at that offset. Joins the drag adds are not prevented: the moved
//! run, or a stub or bend drawn straight from a pin, connects to a pin or
//! wire end it passes over, by the connection rules (Q50), and the preview
//! shows the junction dot.

use crate::model::cleanup::normalize;
use crate::model::geometry::{on_segment, on_segment_interior};
use crate::model::routing::Axis;
use std::collections::{BTreeMap, BTreeSet};
use xsim_project::{Id, Point, Schematic, Wire, connectivity};

/// What is connected at one point of the dragged run.
#[derive(Debug, Default)]
struct Attachments {
    /// Something there cannot follow the run.
    held: bool,
    /// Other wires' ends there that can follow, as (wire, vertex index).
    ends: Vec<(Id, usize)>,
    labels: Vec<Id>,
    /// Segments of other wires along the drag axis whose interior holds
    /// this end of the wire.
    trunks: Vec<(Point, Point)>,
}

/// How an end of the run behaves.
#[derive(Debug, Clone, PartialEq)]
enum End {
    /// Moves with the run.
    Moves,
    /// Keeps its vertex and gets a bend to the moved run.
    Keeps,
    /// Slides along these segments while it stays on all of them, else
    /// keeps its vertex.
    Slides(Vec<(Point, Point)>),
}

impl End {
    fn keeps(&self, at: Point, offset: Point) -> bool {
        match self {
            End::Moves => false,
            End::Keeps => true,
            End::Slides(trunks) => !trunks.iter().all(|&(a, b)| on_segment(at + offset, a, b)),
        }
    }
}

/// The neighbouring leg of a moving end of the run, with the connections
/// on its interior.
#[derive(Debug, Clone)]
struct Tail {
    /// The leg's far end, which stays.
    far: Point,
    /// The run's end before the move.
    corner: Point,
    /// Connections strictly between `far` and `corner`.
    points: Vec<Point>,
}

impl Tail {
    /// A stub that keeps the connections the leg no longer reaches once
    /// the corner has moved by `offset`.
    fn stub(&self, offset: Point) -> Option<[Point; 2]> {
        let corner = self.corner + offset;
        // The point of the new leg nearest to the left-behind connections.
        let start = if on_segment(corner, self.far, self.corner) {
            corner
        } else {
            self.far
        };
        let end = self
            .points
            .iter()
            .copied()
            .filter(|&p| !on_segment(p, self.far, corner))
            .max_by_key(|&p| manhattan(p, start))?;
        Some([start, end])
    }
}

/// A prepared drag of one straight run of a wire.
#[derive(Debug, Clone)]
pub struct SegmentPlan {
    base: Schematic,
    wire: Id,
    /// Vertex indices of the run's ends (`first < last`).
    first: usize,
    last: usize,
    /// The axis the run moves along (perpendicular to the run).
    normal: Axis,
    first_end: End,
    last_end: End,
    /// Held points inside the run, joined to the moved run by stubs.
    stubs: Vec<Point>,
    /// Other wires' ends that follow the run.
    ends: Vec<(Id, usize)>,
    /// Labels that move with the run.
    labels: Vec<Id>,
    tails: Vec<Tail>,
    /// Every connected point strictly inside the run, for the fallback that
    /// holds everything (see [`Self::apply`]).
    rigid_stubs: Vec<Point>,
    /// Pins that share a net before the drag, net by net.
    nets: Vec<Vec<(Id, usize)>>,
}

impl SegmentPlan {
    /// Prepares dragging the run through segment `segment` of wire `wire`.
    /// `None` when the segment is diagonal or empty, or when the wire
    /// touches nothing (then it simply moves as a whole).
    pub fn new(schematic: &Schematic, wire: Id, segment: usize) -> Option<Self> {
        let normal = drag_axis(schematic, wire, segment)?;
        let pts = &schematic.wire(wire)?.points;
        let n = pts.len();
        let (first, last) = run(pts, segment);
        let at = |j: usize| {
            let wire_end = j == 0 || j + 1 == n;
            attachments(schematic, wire, pts[j], true, wire_end, normal)
        };
        let along = |a: Point, b: Point| Axis::of(a, b) == Some(normal);
        let start = at(first);
        let end = at(last);
        let first_end = end_mode(&start, first == 0 || along(pts[first - 1], pts[first]));
        let last_end = end_mode(&end, last + 1 == n || along(pts[last], pts[last + 1]));
        let mut plan = Self {
            base: schematic.clone(),
            wire,
            first,
            last,
            normal,
            first_end,
            last_end,
            stubs: Vec::new(),
            ends: Vec::new(),
            labels: Vec::new(),
            tails: Vec::new(),
            rigid_stubs: Vec::new(),
            nets: connectivity(schematic)
                .nets
                .into_iter()
                .map(|n| n.pins)
                .filter(|pins| pins.len() > 1)
                .collect(),
        };
        if plan.first_end == End::Moves {
            plan.follow(start);
            if first > 0 {
                plan.tails
                    .push(tail(schematic, wire, pts[first - 1], pts[first]));
            }
        }
        if plan.last_end == End::Moves {
            plan.follow(end);
            if last + 1 < n {
                plan.tails
                    .push(tail(schematic, wire, pts[last + 1], pts[last]));
            }
        }
        for (j, &q) in pts.iter().enumerate().take(last).skip(first + 1) {
            let a = at(j);
            if a.held || !a.ends.is_empty() || !a.labels.is_empty() {
                plan.rigid_stubs.push(q);
            }
            plan.attach(q, a);
        }
        for q in interior_connections(schematic, wire, &pts[first..=last]) {
            plan.rigid_stubs.push(q);
            plan.attach(q, attachments(schematic, wire, q, false, false, normal));
        }
        plan.tails.extend(follower_tails(schematic, &plan.ends));
        Some(plan)
    }

    /// The axis the run moves along.
    pub fn normal(&self) -> Axis {
        self.normal
    }

    /// Vertex indices of the run's ends.
    #[cfg(test)]
    pub fn run_ends(&self) -> (usize, usize) {
        (self.first, self.last)
    }

    /// The offset `delta` moves the run by: its part along the drag axis.
    fn offset(&self, delta: Point) -> Point {
        match self.normal {
            Axis::Horizontal => Point::new(delta.x, 0),
            Axis::Vertical => Point::new(0, delta.y),
        }
    }

    /// The wire of [`Self::apply`]'s result that holds the moved run: the
    /// dragged wire, or the wire cleanup joined it into. `None` when the
    /// run collapsed (e.g. onto another leg of its wire).
    pub fn dragged_wire(&self, moved: &Schematic, delta: Point) -> Option<Id> {
        let pts = &self.base.wire(self.wire)?.points;
        let offset = self.offset(delta);
        let ends = [
            *pts.get(self.first)? + offset,
            *pts.get(self.last)? + offset,
        ];
        let holds = |w: &Wire| {
            ends.iter()
                .all(|&e| w.points.windows(2).any(|g| on_segment(e, g[0], g[1])))
        };
        match moved.wire(self.wire) {
            Some(w) if holds(w) => Some(w.id),
            _ => moved.wires.iter().find(|w| holds(w)).map(|w| w.id),
        }
    }

    fn follow(&mut self, a: Attachments) {
        self.ends.extend(a.ends);
        self.labels.extend(a.labels);
    }

    fn attach(&mut self, q: Point, a: Attachments) {
        if a.held {
            self.stubs.push(q);
        } else {
            self.follow(a);
        }
    }

    /// The schematic with the run moved by the part of `delta` along the
    /// drag axis, cleaned up. A zero offset returns the schematic untouched.
    ///
    /// Should following wires and labels ever cost a connection (say, a
    /// follower collapsing onto a crossing), the drag falls back to holding
    /// everything: both ends keep their vertices and every connected point
    /// of the run gets a stub, which never disconnects anything.
    pub fn apply(&self, delta: Point) -> Schematic {
        let offset = self.offset(delta);
        if offset == Point::default() {
            return self.base.clone();
        }
        let out = self.build(offset, false);
        if self.keeps_connections(&out) {
            out
        } else {
            self.build(offset, true)
        }
    }

    /// Whether every pair of pins that shared a net before shares one in
    /// `out`.
    fn keeps_connections(&self, out: &Schematic) -> bool {
        let c = connectivity(out);
        self.nets.iter().all(|pins| {
            let mut nets = pins.iter().map(|&(id, pin)| c.net_of_pin(id, pin));
            let first = nets.next().flatten();
            first.is_some() && nets.all(|n| n == first)
        })
    }

    fn build(&self, offset: Point, rigid: bool) -> Schematic {
        let mut s = self.base.clone();
        let (first_end, last_end) = if rigid {
            (&End::Keeps, &End::Keeps)
        } else {
            (&self.first_end, &self.last_end)
        };
        if let Some(w) = s.wires.iter_mut().find(|w| w.id == self.wire) {
            w.points = self.moved_points(&w.points, offset, first_end, last_end);
        }
        let mut ends: BTreeMap<Id, BTreeSet<usize>> = BTreeMap::new();
        for &(id, index) in self.ends.iter().filter(|_| !rigid) {
            ends.entry(id).or_default().insert(index);
        }
        // Only the following ends move, even when both ends of a wire follow:
        // moving the whole wire would drop connections along it.
        for w in &mut s.wires {
            let Some(indices) = ends.get(&w.id) else {
                continue;
            };
            for (i, p) in w.points.iter_mut().enumerate() {
                if indices.contains(&i) {
                    *p = *p + offset;
                }
            }
        }
        for l in &mut s.labels {
            if !rigid && self.labels.contains(&l.id) {
                l.position = l.position + offset;
            }
        }
        let stubs: Vec<[Point; 2]> = if rigid {
            self.rigid_stubs.iter().map(|&q| [q, q + offset]).collect()
        } else {
            self.stubs
                .iter()
                .map(|&q| [q, q + offset])
                .chain(self.tails.iter().filter_map(|t| t.stub(offset)))
                .collect()
        };
        for points in stubs {
            let id = s.allocate_id();
            s.wires.push(Wire {
                id,
                points: points.to_vec(),
            });
        }
        normalize(&mut s);
        s
    }

    fn moved_points(
        &self,
        pts: &[Point],
        offset: Point,
        first_end: &End,
        last_end: &End,
    ) -> Vec<Point> {
        let (first, last) = (self.first, self.last);
        if last >= pts.len() {
            return pts.to_vec();
        }
        let mut out = Vec::with_capacity(pts.len() + 2);
        out.extend_from_slice(&pts[..first]);
        if first_end.keeps(pts[first], offset) {
            out.push(pts[first]);
        }
        out.extend(pts[first..=last].iter().map(|&p| p + offset));
        if last_end.keeps(pts[last], offset) {
            out.push(pts[last]);
        }
        out.extend_from_slice(&pts[last + 1..]);
        out
    }
}

/// The axis segment `segment` of wire `wire` would be dragged along, or
/// `None` when it is not segment-dragged (diagonal or empty segment, or a
/// wire that touches nothing).
pub fn drag_axis(schematic: &Schematic, wire: Id, segment: usize) -> Option<Axis> {
    let w = schematic.wire(wire)?;
    let (a, b) = (*w.points.get(segment)?, *w.points.get(segment + 1)?);
    let axis = Axis::of(a, b)?;
    touches_anything(schematic, w).then(|| axis.other())
}

/// How an end of the run with attachments `a` behaves; `neighbour_moves`
/// tells whether the wire continues from it along the drag axis (or ends).
fn end_mode(a: &Attachments, neighbour_moves: bool) -> End {
    if a.held || !neighbour_moves {
        End::Keeps
    } else if a.trunks.is_empty() {
        End::Moves
    } else {
        End::Slides(a.trunks.clone())
    }
}

/// Vertex indices of the straight run through segment `segment`.
fn run(pts: &[Point], segment: usize) -> (usize, usize) {
    let step = |a: Point, b: Point| Point::new((b.x - a.x).signum(), (b.y - a.y).signum());
    let dir = step(pts[segment], pts[segment + 1]);
    let mut first = segment;
    while first > 0 && step(pts[first - 1], pts[first]) == dir {
        first -= 1;
    }
    let mut last = segment + 1;
    while last + 1 < pts.len() && step(pts[last], pts[last + 1]) == dir {
        last += 1;
    }
    (first, last)
}

fn manhattan(a: Point, b: Point) -> i64 {
    i64::from(a.x - b.x).abs() + i64::from(a.y - b.y).abs()
}

fn other_wires(s: &Schematic, wire: Id) -> impl Iterator<Item = &Wire> {
    s.wires
        .iter()
        .filter(move |w| w.id != wire && w.points.len() >= 2)
}

/// Both ends of a wire with at least two points.
fn ends_of(w: &Wire) -> [Point; 2] {
    [w.points[0], w.points[w.points.len() - 1]]
}

/// Whether anything is connected to wire `w` (per the netlist rules).
fn touches_anything(s: &Schematic, w: &Wire) -> bool {
    let on_wire =
        |p: Point| w.points.contains(&p) || w.points.windows(2).any(|g| on_segment(p, g[0], g[1]));
    let pins = s.components.iter().flat_map(|c| c.pins()).any(on_wire);
    let labels = s.labels.iter().any(|l| on_wire(l.position));
    let own_ends = match w.points.as_slice() {
        [a, .., b] => [*a, *b],
        _ => return pins || labels,
    };
    let wires = other_wires(s, w.id).any(|o| {
        o.points.iter().any(|p| w.points.contains(p))
            || ends_of(o).into_iter().any(on_wire)
            || own_ends.iter().any(|&e| {
                o.points
                    .windows(2)
                    .any(|g| on_segment_interior(e, g[0], g[1]))
            })
    });
    pins || labels || wires
}

/// What is connected at `q`, a point of the run of wire `wire`: a vertex
/// of it (`vertex`), possibly one of its ends (`wire_end`), or a point of a
/// segment interior.
fn attachments(
    s: &Schematic,
    wire: Id,
    q: Point,
    vertex: bool,
    wire_end: bool,
    normal: Axis,
) -> Attachments {
    let mut a = Attachments {
        held: s.components.iter().any(|c| c.pins().contains(&q)),
        labels: s
            .labels
            .iter()
            .filter(|l| l.position == q)
            .map(|l| l.id)
            .collect(),
        ..Attachments::default()
    };
    // Wires along whose segment this end of the wire slides.
    let mut trunk_wires = Vec::new();
    for w in other_wires(s, wire) {
        let n = w.points.len();
        for (i, &p) in w.points.iter().enumerate() {
            if p != q {
                continue;
            }
            if i == 0 || i + 1 == n {
                let next = if i == 0 { w.points[1] } else { w.points[n - 2] };
                if Axis::of(p, next) == Some(normal) {
                    a.ends.push((w.id, i));
                } else {
                    a.held = true;
                }
            } else if vertex {
                // A vertex where the other wire runs straight on along the
                // drag axis (kept only because something touches it) is a
                // point of one trunk segment, as on a segment interior.
                let (before, after) = (w.points[i - 1], w.points[i + 1]);
                let straight = Axis::of(before, p) == Some(normal)
                    && Axis::of(p, after) == Some(normal)
                    && on_segment(p, before, after);
                if wire_end && straight {
                    a.trunks.push((before, after));
                    trunk_wires.push(w.id);
                } else {
                    a.held = true;
                }
            }
        }
        if wire_end {
            for g in w.points.windows(2) {
                if on_segment_interior(q, g[0], g[1]) {
                    if Axis::of(g[0], g[1]) == Some(normal) {
                        a.trunks.push((g[0], g[1]));
                        trunk_wires.push(w.id);
                    } else {
                        a.held = true;
                    }
                }
            }
        }
    }
    // A label names every wire through its point, bends and crossings
    // included, and a following wire's end joins any wire through it. Such a
    // wire that does not follow the run keeps them, and the point, in place.
    // A trunk the end slides along is fine: they stay on it, and so does
    // the end.
    if !a.labels.is_empty() || !a.ends.is_empty() {
        let follows = |id: Id| a.ends.iter().any(|&(e, _)| e == id);
        let stays = other_wires(s, wire).any(|w| {
            !follows(w.id)
                && !trunk_wires.contains(&w.id)
                && w.points.windows(2).any(|g| on_segment(q, g[0], g[1]))
        });
        a.held |= stays;
    }
    a
}

/// Points strictly inside the segments of `run` where pins, labels or
/// other wires' ends connect.
fn interior_connections(s: &Schematic, wire: Id, run: &[Point]) -> BTreeSet<Point> {
    let candidates = s
        .components
        .iter()
        .flat_map(|c| c.pins())
        .chain(s.labels.iter().map(|l| l.position))
        .chain(other_wires(s, wire).flat_map(ends_of));
    candidates
        .filter(|&p| run.windows(2).any(|g| on_segment_interior(p, g[0], g[1])))
        .collect()
}

/// The end segments of wires that follow the run: like a neighbouring leg
/// of the run, they shorten and may leave connections behind.
fn follower_tails(s: &Schematic, ends: &[(Id, usize)]) -> Vec<Tail> {
    let mut by_wire: BTreeMap<Id, BTreeSet<usize>> = BTreeMap::new();
    for &(id, index) in ends {
        by_wire.entry(id).or_default().insert(index);
    }
    let mut tails = Vec::new();
    for (id, indices) in by_wire {
        let Some(w) = s.wire(id).filter(|w| w.points.len() >= 2) else {
            continue;
        };
        for index in indices {
            let next = if index == 0 { 1 } else { index - 1 };
            tails.push(tail(s, id, w.points[next], w.points[index]));
        }
    }
    tails
}

/// The leg from `far` to the run's end `corner` and its connections.
fn tail(s: &Schematic, wire: Id, far: Point, corner: Point) -> Tail {
    Tail {
        far,
        corner,
        points: interior_connections(s, wire, &[far, corner])
            .into_iter()
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::{SegmentPlan, drag_axis};
    use crate::model::routing::Axis;
    use std::collections::BTreeSet;
    use xsim_project::{
        Component, Id, NetLabel, Orientation, Part, Point, Schematic, Wire, connectivity,
    };

    fn p(x: i32, y: i32) -> Point {
        Point::new(x, y)
    }

    fn resistor(id: u64, at: Point, upright: bool) -> Component {
        Component {
            id,
            name: format!("R{id}"),
            position: at,
            orientation: Orientation {
                rotation: u8::from(upright),
                ..Orientation::default()
            },
            part: Part::Resistor { resistance: 1e3 },
        }
    }

    fn wire(id: u64, points: &[Point]) -> Wire {
        Wire {
            id,
            points: points.to_vec(),
        }
    }

    fn points(s: &Schematic, id: Id) -> Vec<Point> {
        s.wire(id).map(|w| w.points.clone()).unwrap_or_default()
    }

    /// Which pins share a net.
    fn partition(s: &Schematic) -> BTreeSet<BTreeSet<(Id, usize)>> {
        connectivity(s)
            .nets
            .into_iter()
            .map(|n| n.pins.into_iter().collect())
            .collect()
    }

    fn diagonals(s: &Schematic) -> usize {
        s.wires
            .iter()
            .flat_map(|w| w.points.windows(2))
            .filter(|g| g[0].x != g[1].x && g[0].y != g[1].y)
            .count()
    }

    /// Drags and checks what every segment drag must keep: no new diagonal,
    /// the same pins on the same nets.
    fn drag(s: &Schematic, wire: Id, segment: usize, delta: Point) -> Schematic {
        let plan = SegmentPlan::new(s, wire, segment).expect("segment-draggable");
        let out = plan.apply(delta);
        assert!(diagonals(&out) <= diagonals(s), "diagonal: {:?}", out.wires);
        assert_eq!(
            partition(&out),
            partition(s),
            "connections changed by {delta:?}: {:?}",
            out.wires
        );
        out
    }

    /// R1 (pins (-2,0), (2,0)) and R2 (pins (10,0), (14,0)) joined by a U.
    fn u_route() -> Schematic {
        Schematic {
            components: vec![resistor(1, p(0, 0), false), resistor(2, p(12, 0), false)],
            wires: vec![wire(
                3,
                &[p(2, 0), p(4, 0), p(4, 4), p(8, 4), p(8, 0), p(10, 0)],
            )],
            labels: Vec::new(),
            next_id: 4,
        }
    }

    #[test]
    fn the_run_moves_perpendicular_only() {
        let s = u_route();
        let plan = SegmentPlan::new(&s, 3, 2).expect("plan");
        assert_eq!(plan.normal(), Axis::Vertical);
        assert_eq!(plan.apply(p(5, 0)), s, "sideways motion changes nothing");
        let out = drag(&s, 3, 2, p(7, -1));
        assert_eq!(
            points(&out, 3),
            vec![p(2, 0), p(4, 0), p(4, 3), p(8, 3), p(8, 0), p(10, 0)]
        );
    }

    #[test]
    fn any_distance_in_either_direction_keeps_the_circuit() {
        let s = u_route();
        for dy in -8..=8 {
            let out = drag(&s, 3, 2, p(0, dy));
            let w = points(&out, 3);
            assert_eq!(w.first(), Some(&p(2, 0)), "dy {dy}");
            assert_eq!(w.last(), Some(&p(10, 0)), "dy {dy}");
        }
        // Moved onto the pins' row, the U collapses into a straight wire.
        assert_eq!(
            points(&drag(&s, 3, 2, p(0, -4)), 3),
            vec![p(2, 0), p(10, 0)]
        );
        // Every leg of the U, both ways and past the far end of its legs.
        for (segment, delta) in [(1, p(-3, 0)), (1, p(6, 0)), (3, p(3, 0)), (3, p(-9, 0))] {
            drag(&s, 3, segment, delta);
        }
    }

    #[test]
    fn a_vertex_that_only_marks_a_junction_does_not_split_the_run() {
        // The T-junction vertex (6,0) sits inside one straight line.
        let s = Schematic {
            components: vec![
                resistor(1, p(0, 0), false),
                resistor(2, p(12, 0), false),
                resistor(3, p(6, 8), true),
            ],
            wires: vec![
                wire(4, &[p(2, 0), p(6, 0), p(10, 0)]),
                wire(5, &[p(6, 0), p(6, 6)]),
            ],
            labels: Vec::new(),
            next_id: 6,
        };
        for segment in [0, 1] {
            let out = drag(&s, 4, segment, p(0, -2));
            assert_eq!(
                points(&out, 4),
                vec![p(2, 0), p(2, -2), p(6, -2), p(10, -2), p(10, 0)]
            );
            assert_eq!(points(&out, 5), vec![p(6, -2), p(6, 6)]);
        }
    }

    #[test]
    fn labels_on_the_run_move_with_it() {
        let mut s = u_route();
        s.labels.push(NetLabel {
            id: 9,
            name: "mid".into(),
            position: p(6, 4),
        });
        s.wires[0].points.insert(3, p(6, 4));
        s.next_id = 10;
        let out = drag(&s, 3, 2, p(0, -2));
        assert_eq!(out.label(9).map(|l| l.position), Some(p(6, 2)));
        assert!(
            points(&out, 3).contains(&p(6, 2)),
            "the label keeps a vertex"
        );
    }

    #[test]
    fn a_crossing_or_a_foreign_bend_is_not_a_connection() {
        let mut s = u_route();
        // A wire crossing the bottom run, and an any-angle wire bending on it.
        s.components.push(resistor(5, p(6, 10), true));
        s.components.push(resistor(6, p(10, 9), true));
        s.wires.push(wire(7, &[p(6, 2), p(6, 8)]));
        s.wires.push(wire(8, &[p(7, 1), p(7, 4), p(10, 7)]));
        s.next_id = 9;
        let out = drag(&s, 3, 2, p(0, 3));
        assert_eq!(points(&out, 7), points(&s, 7), "the crossing stays");
        assert_eq!(points(&out, 8), points(&s, 8), "the foreign bend stays");
        assert_eq!(out.wires.len(), 3, "no stubs: {:?}", out.wires);
    }

    #[test]
    fn connections_a_shrinking_leg_leaves_behind_keep_a_stub() {
        let mut s = u_route();
        // R5's pin wired to the interior of the U's left leg (x = 4).
        s.components.push(resistor(5, p(-2, 3), false));
        s.wires.push(wire(6, &[p(0, 3), p(4, 3)]));
        s.next_id = 7;
        for dy in [-1, -3, -4, -6] {
            let out = drag(&s, 3, 2, p(0, dy));
            assert_eq!(points(&out, 6), vec![p(0, 3), p(4, 3)], "dy {dy}");
        }
        let out = drag(&s, 3, 2, p(0, 2));
        assert_eq!(out.wires.len(), 2, "a growing leg needs no stub");
    }

    #[test]
    fn connections_a_shrinking_follower_leaves_behind_keep_a_stub() {
        let mut s = u_route();
        // A branch from the U's bottom run down to R5's pin (6,10), and a
        // wire from R6's pin T'd onto the branch at (6,6).
        s.components.push(resistor(5, p(6, 12), true));
        s.components.push(resistor(6, p(11, 6), false));
        s.wires.push(wire(7, &[p(6, 4), p(6, 10)]));
        s.wires.push(wire(8, &[p(6, 6), p(9, 6)]));
        s.next_id = 9;
        for dy in [1, 2, 3, 6, 8, -2] {
            let out = drag(&s, 3, 2, p(0, dy));
            assert_eq!(points(&out, 8), points(&s, 8), "dy {dy}: R6's wire stays");
        }
    }

    #[test]
    fn a_held_end_gets_a_bend_and_a_free_end_moves() {
        // A stub from R1's pin to nowhere.
        let s = Schematic {
            components: vec![resistor(1, p(0, 0), false)],
            wires: vec![wire(2, &[p(2, 0), p(6, 0)])],
            labels: Vec::new(),
            next_id: 3,
        };
        let out = drag(&s, 2, 0, p(0, 3));
        assert_eq!(points(&out, 2), vec![p(2, 0), p(2, 3), p(6, 3)]);
    }

    #[test]
    fn wires_touching_nothing_and_diagonals_are_not_segment_dragged() {
        let s = Schematic {
            components: vec![resistor(1, p(0, 0), false)],
            wires: vec![wire(2, &[p(10, 0), p(14, 0)]), wire(3, &[p(2, 0), p(6, 4)])],
            labels: Vec::new(),
            next_id: 4,
        };
        assert_eq!(drag_axis(&s, 2, 0), None, "touches nothing");
        assert_eq!(drag_axis(&s, 3, 0), None, "diagonal");
        assert_eq!(drag_axis(&s, 3, 5), None, "no such segment");
        assert!(SegmentPlan::new(&s, 9, 0).is_none(), "no such wire");
    }

    #[test]
    fn a_branch_end_slides_along_the_wire_it_joins() {
        // Wire 4 leaves the U's bottom run at (6,4) for R5's pin (6,8).
        let mut s = u_route();
        s.components.push(resistor(5, p(6, 10), true));
        s.wires.push(wire(4, &[p(6, 4), p(6, 8)]));
        s.next_id = 6;
        let out = drag(&s, 4, 0, p(1, 0));
        assert_eq!(points(&out, 4), vec![p(7, 4), p(7, 8), p(6, 8)]);
        assert_eq!(points(&out, 3), points(&s, 3), "the trunk stays");
        let out = drag(&s, 4, 0, p(2, 0));
        assert_eq!(
            points(&out, 4),
            vec![p(8, 4), p(8, 8), p(6, 8)],
            "onto the trunk's corner"
        );
        let out = drag(&s, 4, 0, p(3, 0));
        assert_eq!(
            points(&out, 4).first(),
            Some(&p(6, 4)),
            "off the trunk the end keeps its place"
        );
    }

    // Regressions found by randomised drags (review of D14).

    #[test]
    fn a_leg_dragged_onto_the_other_leg_keeps_a_t_junction_there() {
        // R5's wire tees onto the U's right leg at (8,2); the left leg is
        // dragged onto the right one, folding the U flat.
        let mut s = u_route();
        s.components.push(resistor(5, p(14, 2), false));
        s.wires.push(wire(6, &[p(8, 2), p(12, 2)]));
        s.next_id = 7;
        let out = drag(&s, 3, 1, p(4, 0));
        assert_eq!(points(&out, 6), vec![p(8, 2), p(12, 2)], "R5's wire stays");
    }

    #[test]
    fn a_branch_with_a_label_at_its_junction_still_slides() {
        // As in `a_branch_end_slides_along_the_wire_it_joins`, with the net
        // named by a label at the junction (6,4).
        let mut s = u_route();
        s.components.push(resistor(5, p(6, 10), true));
        s.wires.push(wire(4, &[p(6, 4), p(6, 8)]));
        s.labels.push(NetLabel {
            id: 6,
            name: "mid".into(),
            position: p(6, 4),
        });
        s.wires[0].points.insert(3, p(6, 4));
        s.next_id = 7;
        let out = drag(&s, 4, 0, p(1, 0));
        assert_eq!(points(&out, 4), vec![p(7, 4), p(7, 8), p(6, 8)]);
        assert_eq!(
            out.label(6).map(|l| l.position),
            Some(p(6, 4)),
            "on the trunk"
        );
    }

    #[test]
    fn a_label_on_the_run_stays_where_a_crossing_wire_needs_it() {
        // Wire 7 crosses the U's left leg at (4,2), where the label `n`
        // joins the two; R5 hangs off wire 7.
        let mut s = u_route();
        s.components.push(resistor(5, p(9, 2), false));
        s.wires.push(wire(7, &[p(1, 2), p(4, 2), p(7, 2)]));
        s.labels.push(NetLabel {
            id: 8,
            name: "n".into(),
            position: p(4, 2),
        });
        s.next_id = 9;
        for dx in [-4, -1, 2, 5] {
            let out = drag(&s, 3, 1, p(dx, 0));
            assert_eq!(out.label(8).map(|l| l.position), Some(p(4, 2)), "dx {dx}");
        }
    }

    #[test]
    fn a_loop_hanging_off_the_run_stretches_instead_of_moving_away() {
        // Wire 9 leaves the U's bottom at (5,4) and returns at (7,4); R5's
        // pin (6,8) sits on its bottom.
        let mut s = u_route();
        s.components.push(resistor(5, p(6, 10), true));
        s.wires.push(wire(9, &[p(5, 4), p(5, 8), p(7, 8), p(7, 4)]));
        s.next_id = 10;
        let out = drag(&s, 3, 2, p(0, -2));
        assert_eq!(
            points(&out, 9),
            vec![p(5, 2), p(5, 8), p(7, 8), p(7, 2)],
            "only the ends on the run follow it"
        );
    }

    #[test]
    fn a_rung_collapsing_onto_a_crossing_falls_back_to_holding_everything() {
        // A vertical run (8,0)–(8,8) of wire 3 between R1 and R3, and a rung
        // (wire 6) from it to the vertical wire 5 at x = 4, R2's only link.
        // Dragged onto x = 4 the rung would vanish and the run would merely
        // lie over wire 5.
        let s = Schematic {
            components: vec![
                resistor(1, p(12, 0), false),
                resistor(2, p(4, 12), true),
                resistor(3, p(12, 8), false),
            ],
            wires: vec![
                wire(3, &[p(10, 0), p(8, 0), p(8, 8), p(10, 8)]),
                wire(5, &[p(4, -2), p(4, 10)]),
                wire(6, &[p(4, 4), p(8, 4)]),
            ],
            labels: Vec::new(),
            next_id: 7,
        };
        for dx in [-2, -4, -5] {
            drag(&s, 3, 1, p(dx, 0));
        }
    }
}
