//! Wire normalisation after every edit.
//!
//! Wires keep the connections of the netlist rules (Q50): a wire vertex
//! connects to pins, labels and other wires' vertices; a wire end or a pin on
//! a segment interior forms a T-junction. Cleanup therefore never removes a
//! vertex that something else touches:
//! * consecutive duplicate points are dropped (zero-length segments);
//! * a label on a segment interior gets a vertex there, so it names the net;
//! * collinear interior vertices that nothing touches are merged away; a
//!   wire doubling back on itself is cut back, and when something connects
//!   to the part that goes (a T-junction, a pin), a stub wire of its own
//!   keeps reaching it;
//! * two wires meeting end to end in a straight line, with nothing else at
//!   the joint, become one wire;
//! * wires with fewer than two points are removed.
//!
//! A removed wire can free a vertex of another wire or leave a label on a
//! segment interior, so the steps repeat until nothing changes: the result
//! is stable, and the next edit does not silently rewrite unrelated wires.

use crate::model::geometry::{collinear, on_segment, on_segment_interior};
use std::collections::{BTreeMap, BTreeSet};
use xsim_project::{Id, Point, Schematic, Wire};

/// Passes after which [`normalize`] stops even without a fixed point (each
/// pass only removes vertices or wires, or adds a label's vertex, so a few
/// suffice).
const MAX_PASSES: usize = 4;

/// Normalises every wire of `schematic` in place.
pub fn normalize(schematic: &mut Schematic) {
    for _ in 0..MAX_PASSES {
        let before = schematic.wires.clone();
        normalize_once(schematic);
        if schematic.wires == before {
            break;
        }
    }
}

fn normalize_once(schematic: &mut Schematic) {
    for wire in &mut schematic.wires {
        wire.points.dedup();
    }
    schematic.wires.retain(|w| w.points.len() >= 2);
    split_at_labels(schematic);
    merge_collinear_vertices(schematic);
    join_straight_wires(schematic);
    schematic.wires.retain(|w| w.points.len() >= 2);
}

/// Which wires have a vertex at each point.
fn vertex_owners(schematic: &Schematic) -> BTreeMap<Point, Vec<Id>> {
    let mut owners: BTreeMap<Point, Vec<Id>> = BTreeMap::new();
    for wire in &schematic.wires {
        for &p in &wire.points {
            owners.entry(p).or_default().push(wire.id);
        }
    }
    owners
}

/// Pin and label positions.
fn fixed_anchors(schematic: &Schematic) -> BTreeSet<Point> {
    schematic
        .components
        .iter()
        .flat_map(|c| c.pins())
        .chain(schematic.labels.iter().map(|l| l.position))
        .collect()
}

fn split_at_labels(schematic: &mut Schematic) {
    let positions: Vec<Point> = schematic.labels.iter().map(|l| l.position).collect();
    for p in positions {
        if schematic.wires.iter().any(|w| w.points.contains(&p)) {
            continue;
        }
        if let Some(wire) = schematic
            .wires
            .iter_mut()
            .find(|w| interior_index(w, p).is_some())
            && let Some(index) = interior_index(wire, p)
        {
            wire.points.insert(index + 1, p);
        }
    }
}

/// Index of the segment whose interior contains `p`.
pub fn interior_index(wire: &Wire, p: Point) -> Option<usize> {
    wire.points
        .windows(2)
        .position(|s| on_segment_interior(p, s[0], s[1]))
}

fn merge_collinear_vertices(schematic: &mut Schematic) {
    let fixed = fixed_anchors(schematic);
    let owners = vertex_owners(schematic);
    let anchored = |p: Point| fixed.contains(&p) || owners.get(&p).is_some_and(|ids| ids.len() > 1);
    let ends: Vec<(Id, [Point; 2])> = schematic
        .wires
        .iter()
        .filter_map(|w| Some((w.id, [*w.points.first()?, *w.points.last()?])))
        .collect();
    let mut stubs = Vec::new();
    for wire in &mut schematic.wires {
        // Points that connect to this wire's segment interiors: pins, labels
        // and the ends of other wires (Q50).
        let touches: BTreeSet<Point> = fixed
            .iter()
            .copied()
            .chain(
                ends.iter()
                    .filter(|(id, _)| *id != wire.id)
                    .flat_map(|(_, e)| *e),
            )
            .collect();
        wire.points = simplify(&wire.points, anchored, &touches, &mut stubs);
    }
    for points in stubs {
        let id = schematic.allocate_id();
        schematic.wires.push(Wire {
            id,
            points: points.to_vec(),
        });
    }
}

/// Drops duplicate points and collinear interior vertices unless `anchored`
/// says something touches them. A backtracking spike goes too; if one of
/// `touches` lies on it, a stub (pushed to `stubs`) from where the wire now
/// turns back to the farthest such point keeps that T-junction connected.
fn simplify(
    points: &[Point],
    anchored: impl Fn(Point) -> bool,
    touches: &BTreeSet<Point>,
    stubs: &mut Vec<[Point; 2]>,
) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::with_capacity(points.len());
    // Where this wire's new stubs start: the wire must keep reaching them
    // when a later spike folds over them too.
    let mut roots: Vec<Point> = Vec::new();
    for &p in points {
        while let [.., a, b] = out[..] {
            if b == p || !collinear(a, b, p) || anchored(b) {
                break;
            }
            let candidates = touches.iter().chain(&roots).copied();
            if let Some(stub) = spike_stub(a, b, p, candidates) {
                roots.push(stub[0]);
                stubs.push(stub);
            }
            out.pop();
        }
        if out.last() != Some(&p) {
            out.push(p);
        }
    }
    out
}

/// For collinear `a`→`b`→`p`: `None` when dropping `b` loses nothing (it
/// lies between `a` and `p`, or the part of the spike beyond them touches
/// nothing), else the stub from the nearer of `a` and `p` (which stays a
/// point of the wire) to the farthest point of `touches` on that part.
fn spike_stub(
    a: Point,
    b: Point,
    p: Point,
    touches: impl Iterator<Item = Point>,
) -> Option<[Point; 2]> {
    if on_segment(b, a, p) {
        return None;
    }
    // `a` and `p` lie on the same side of `b`: the nearer one is where the
    // remaining path stops.
    let near = if on_segment(p, a, b) { p } else { a };
    let length = |q: Point| i64::from(q.x - near.x).abs() + i64::from(q.y - near.y).abs();
    touches
        .filter(|&t| t != near && on_segment(t, near, b))
        .max_by_key(|&t| length(t))
        .map(|tip| [near, tip])
}

/// Joins pairs of wires whose ends meet in a straight line at a point that
/// nothing else touches.
fn join_straight_wires(schematic: &mut Schematic) {
    while let Some((keep, absorb, joint)) = find_straight_joint(schematic) {
        let Some(absorbed) = schematic.wires.iter().position(|w| w.id == absorb) else {
            return;
        };
        let other = schematic.wires.remove(absorbed);
        let Some(target) = schematic.wires.iter_mut().find(|w| w.id == keep) else {
            return;
        };
        let mut head = target.points.clone();
        if head.first() == Some(&joint) {
            head.reverse();
        }
        let mut tail = other.points;
        if tail.last() == Some(&joint) {
            tail.reverse();
        }
        head.extend(tail.into_iter().skip(1));
        target.points = head;
    }
    merge_collinear_vertices(schematic);
}

fn find_straight_joint(schematic: &Schematic) -> Option<(Id, Id, Point)> {
    let fixed = fixed_anchors(schematic);
    let owners = vertex_owners(schematic);
    let mut ends: BTreeMap<Point, Vec<(Id, Point)>> = BTreeMap::new();
    for wire in schematic.wires.iter().filter(|w| w.points.len() >= 2) {
        let n = wire.points.len();
        ends.entry(wire.points[0])
            .or_default()
            .push((wire.id, wire.points[1]));
        ends.entry(wire.points[n - 1])
            .or_default()
            .push((wire.id, wire.points[n - 2]));
    }
    ends.into_iter().find_map(|(joint, at)| {
        let [(a, before), (b, after)] = at.as_slice() else {
            return None;
        };
        let lone = owners.get(&joint).is_some_and(|ids| ids.len() == 2);
        let touched = fixed.contains(&joint)
            || schematic
                .wires
                .iter()
                .any(|w| interior_index(w, joint).is_some());
        let (u, v) = (*before - joint, *after - joint);
        let opposite = i64::from(u.x) * i64::from(v.x) + i64::from(u.y) * i64::from(v.y) < 0;
        let straight = collinear(*before, joint, *after) && opposite;
        (a != b && lone && !touched && straight).then(|| (*a.min(b), *a.max(b), joint))
    })
}

#[cfg(test)]
mod tests {
    use super::normalize;
    use xsim_project::{NetLabel, Point, Schematic, Wire};

    fn wire(id: u64, points: &[(i32, i32)]) -> Wire {
        Wire {
            id,
            points: points.iter().map(|&(x, y)| Point::new(x, y)).collect(),
        }
    }

    fn points(s: &Schematic, id: u64) -> Vec<(i32, i32)> {
        s.wire(id)
            .map(|w| w.points.iter().map(|p| (p.x, p.y)).collect())
            .unwrap_or_default()
    }

    #[test]
    fn removes_zero_length_segments_and_merges_collinear_vertices() {
        let mut s = Schematic {
            wires: vec![wire(1, &[(0, 0), (0, 0), (4, 0), (10, 0), (10, 6)])],
            ..Schematic::default()
        };
        normalize(&mut s);
        assert_eq!(points(&s, 1), vec![(0, 0), (10, 0), (10, 6)]);
    }

    #[test]
    fn removes_backtracking_spikes() {
        let mut s = Schematic {
            wires: vec![wire(1, &[(0, 0), (12, 0), (8, 0)])],
            ..Schematic::default()
        };
        normalize(&mut s);
        assert_eq!(points(&s, 1), vec![(0, 0), (8, 0)]);
    }

    #[test]
    fn keeps_vertices_that_other_objects_touch() {
        let mut s = Schematic {
            wires: vec![
                wire(1, &[(0, 0), (4, 0), (10, 0)]),
                wire(2, &[(4, 0), (4, 8)]),
            ],
            ..Schematic::default()
        };
        normalize(&mut s);
        assert_eq!(points(&s, 1), vec![(0, 0), (4, 0), (10, 0)]);
    }

    #[test]
    fn a_label_on_a_segment_interior_gets_a_vertex() {
        let mut s = Schematic {
            wires: vec![wire(1, &[(0, 0), (12, 0)])],
            labels: vec![NetLabel {
                id: 2,
                name: "out".into(),
                position: Point::new(6, 0),
            }],
            ..Schematic::default()
        };
        normalize(&mut s);
        assert_eq!(points(&s, 1), vec![(0, 0), (6, 0), (12, 0)]);
    }

    #[test]
    fn straight_wires_meeting_end_to_end_become_one() {
        let mut s = Schematic {
            wires: vec![wire(4, &[(0, 0), (6, 0)]), wire(2, &[(12, 0), (6, 0)])],
            ..Schematic::default()
        };
        normalize(&mut s);
        assert_eq!(s.wires.len(), 1);
        let mut joined = points(&s, 2);
        joined.sort_unstable();
        assert_eq!(joined, vec![(0, 0), (12, 0)]);
    }

    #[test]
    fn wires_meeting_at_an_angle_stay_separate() {
        let mut s = Schematic {
            wires: vec![wire(1, &[(0, 0), (6, 0)]), wire(2, &[(6, 0), (6, 8)])],
            ..Schematic::default()
        };
        normalize(&mut s);
        assert_eq!(s.wires.len(), 2);
    }

    /// Every wire's points except wire `except`, sorted by id.
    fn others(s: &Schematic, except: u64) -> Vec<Vec<(i32, i32)>> {
        let mut ids: Vec<u64> = s
            .wires
            .iter()
            .map(|w| w.id)
            .filter(|&i| i != except)
            .collect();
        ids.sort_unstable();
        ids.into_iter().map(|id| points(s, id)).collect()
    }

    #[test]
    fn a_folded_back_spike_leaves_a_stub_to_a_t_junction_on_it() {
        // Wire 1 doubles back from (16,8) to (16,0); wire 2 tees onto the
        // doubled part at (16,4).
        let mut s = Schematic {
            wires: vec![
                wire(1, &[(4, 0), (16, 0), (16, 8), (16, 0), (20, 0)]),
                wire(2, &[(16, 4), (24, 4)]),
            ],
            next_id: 3,
            ..Schematic::default()
        };
        normalize(&mut s);
        assert_eq!(
            points(&s, 1),
            vec![(4, 0), (16, 0), (20, 0)],
            "the spike goes; the stub's end keeps a vertex"
        );
        assert_eq!(
            others(&s, 1),
            vec![vec![(16, 4), (24, 4)], vec![(16, 0), (16, 4)]],
            "a stub keeps reaching the T-junction"
        );
        let c = xsim_project::connectivity(&s);
        assert_eq!(c.junctions, vec![Point::new(16, 0)], "one T, no corner dot");
    }

    #[test]
    fn a_pin_on_a_folded_back_spike_keeps_a_stub() {
        // R3's left pin (16,2) lies on the doubled part.
        let mut s = Schematic {
            components: vec![xsim_project::Component {
                id: 3,
                name: "R3".into(),
                position: Point::new(20, 2),
                orientation: xsim_project::Orientation::default(),
                part: xsim_project::Part::Resistor { resistance: 1e3 },
            }],
            wires: vec![wire(1, &[(4, 0), (16, 0), (16, 8), (16, 0), (20, 0)])],
            next_id: 4,
            ..Schematic::default()
        };
        normalize(&mut s);
        assert_eq!(points(&s, 1), vec![(4, 0), (16, 0), (20, 0)]);
        assert_eq!(others(&s, 1), vec![vec![(16, 0), (16, 2)]]);
    }

    #[test]
    fn nested_folds_keep_the_stub_on_the_wire() {
        // (0,0) → (30,0) → (10,0) → (4,0): two folds; wire 2 tees on at
        // (24,0), which the first fold cuts off, and the second fold removes
        // where the first fold's stub starts.
        let mut s = Schematic {
            wires: vec![
                wire(1, &[(0, 0), (20, 0), (30, 0), (10, 0), (4, 0)]),
                wire(2, &[(24, 0), (24, 8)]),
            ],
            next_id: 3,
            ..Schematic::default()
        };
        normalize(&mut s);
        assert_eq!(
            points(&s, 1),
            vec![(0, 0), (24, 0)],
            "the stubs joined back into the wire"
        );
        assert_eq!(points(&s, 2), vec![(24, 0), (24, 8)]);
        assert_eq!(s.wires.len(), 2, "no stub is left over");
    }

    #[test]
    fn a_fold_with_nothing_on_it_simply_goes() {
        let mut s = Schematic {
            wires: vec![wire(1, &[(4, 0), (16, 0), (16, 8), (16, 0), (20, 0)])],
            ..Schematic::default()
        };
        normalize(&mut s);
        assert_eq!(points(&s, 1), vec![(4, 0), (20, 0)]);
        assert_eq!(s.wires.len(), 1);
    }

    #[test]
    fn normalizing_twice_changes_nothing() {
        // Wire 2 collapses to a point; its vertex anchored (4,28) of wire 1,
        // and a label sat on it.
        let mut s = Schematic {
            wires: vec![
                wire(1, &[(24, 28), (4, 28), (2, 28)]),
                wire(2, &[(4, 28), (4, 28)]),
                wire(3, &[(6, 26), (18, 26), (6, 26)]),
                wire(4, &[(6, 14), (6, 40)]),
            ],
            labels: vec![NetLabel {
                id: 5,
                name: "out".into(),
                position: Point::new(6, 26),
            }],
            ..Schematic::default()
        };
        normalize(&mut s);
        let once = s.clone();
        normalize(&mut s);
        assert_eq!(s, once);
        assert_eq!(points(&s, 1), vec![(24, 28), (2, 28)]);
        assert_eq!(points(&s, 4), vec![(6, 14), (6, 26), (6, 40)]);
    }

    #[test]
    fn degenerate_wires_are_removed() {
        let mut s = Schematic {
            wires: vec![wire(1, &[(2, 2), (2, 2)])],
            ..Schematic::default()
        };
        normalize(&mut s);
        assert!(s.wires.is_empty());
    }
}
