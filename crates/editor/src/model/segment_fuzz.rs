//! Randomised segment drags (D14): on small random drawings, dragging any
//! straight run of any wire by any distance must keep every connection,
//! add no diagonal, leave a normalised drawing, and leave the run where it
//! was dragged. The review found four bugs this way (see the regression
//! tests in `segment.rs` and `cleanup.rs`).
//!
//! Joins the drag *adds* are not checked: the moved run, or a stub or bend
//! drawn straight from a pin, can pass over a pin or a wire end and connect
//! there, by the connection rules (Q50).
//!
//! The drawings are generated twice: once on even coordinates, the
//! schema-2 drawings of before D22 at the finer grid unit, and once on
//! every grid unit, where parts, wires and drags can now land on any dot.

use super::segment::SegmentPlan;
use crate::model::cleanup::normalize;
use crate::model::geometry::{collinear, on_segment};
use crate::model::routing::Axis;
use std::collections::BTreeMap;
use xsim_project::{
    Component, Id, NetLabel, Orientation, Part, Point, Schematic, Wire, connectivity,
};

/// Drawings per run; a few seconds in a debug build. Raise it locally to
/// search harder.
const CASES: usize = 120;

/// A xorshift generator, so every run checks the same drawings.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn range(&mut self, lo: i32, hi: i32) -> i32 {
        lo + (self.next() % (hi - lo + 1) as u64) as i32
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Which grid points a random drawing uses.
#[derive(Clone, Copy)]
enum Lattice {
    /// Even coordinates only, with parts every six units: the schema-2
    /// drawings, doubled.
    Schema2,
    /// Every grid unit (D22).
    Fine,
}

impl Lattice {
    /// A part position.
    fn position(self, rng: &mut Rng) -> Point {
        match self {
            Lattice::Schema2 => Point::new(rng.range(0, 8) * 6, rng.range(0, 8) * 6),
            Lattice::Fine => Point::new(rng.range(0, 48), rng.range(0, 48)),
        }
    }

    /// A free coordinate for a wire point.
    fn coordinate(self, rng: &mut Rng) -> i32 {
        match self {
            Lattice::Schema2 => 2 * rng.range(-2, 26),
            Lattice::Fine => rng.range(-4, 52),
        }
    }
}

/// Two to four resistors and up to four orthogonal wires between pins,
/// points on earlier wires (T-junctions) and free points, sometimes a label.
fn random_schematic(rng: &mut Rng, lattice: Lattice) -> Schematic {
    let mut s = Schematic::default();
    for _ in 0..rng.range(2, 4) {
        let id = s.allocate_id();
        let c = Component {
            id,
            name: format!("R{id}"),
            position: lattice.position(rng),
            orientation: Orientation {
                rotation: u8::from(rng.below(2) == 1),
                ..Orientation::default()
            },
            part: Part::Resistor { resistance: 1e3 },
        };
        let clash = s
            .components
            .iter()
            .flat_map(|o| o.pins())
            .any(|p| c.pins().contains(&p));
        if !clash {
            s.components.push(c);
        }
    }
    for _ in 0..rng.range(1, 4) {
        let pins: Vec<Point> = s.components.iter().flat_map(|c| c.pins()).collect();
        let segments: Vec<(Point, Point)> = s
            .wires
            .iter()
            .flat_map(|w| w.points.windows(2).map(|g| (g[0], g[1])))
            .collect();
        let pick = |rng: &mut Rng| match rng.below(10) {
            0..6 if !pins.is_empty() => pins[rng.below(pins.len())],
            6..8 if !segments.is_empty() => {
                let (a, b) = segments[rng.below(segments.len())];
                let t = rng.range(0, 4);
                Point::new(a.x + (b.x - a.x) * t / 4, a.y + (b.y - a.y) * t / 4)
            }
            _ => Point::new(lattice.coordinate(rng), lattice.coordinate(rng)),
        };
        let (a, b) = (pick(rng), pick(rng));
        let mut points = vec![a];
        match rng.below(3) {
            0 => points.push(Point::new(b.x, a.y)),
            1 => points.push(Point::new(a.x, b.y)),
            _ => {
                let x = lattice.coordinate(rng);
                points.extend([Point::new(x, a.y), Point::new(x, b.y)]);
            }
        }
        points.push(b);
        points.dedup();
        if points.len() >= 2 {
            let id = s.allocate_id();
            s.wires.push(Wire { id, points });
        }
    }
    if rng.below(10) < 4 && !s.wires.is_empty() {
        let w = &s.wires[rng.below(s.wires.len())];
        let position = w.points[rng.below(w.points.len())];
        let id = s.allocate_id();
        s.labels.push(NetLabel {
            id,
            name: "n".into(),
            position,
        });
    }
    normalize(&mut s);
    s
}

type PinRef = (Id, usize);

/// The net of every pin.
fn pin_nets(s: &Schematic) -> BTreeMap<PinRef, usize> {
    connectivity(s)
        .nets
        .iter()
        .enumerate()
        .flat_map(|(i, n)| n.pins.iter().map(move |&p| (p, i)))
        .collect()
}

/// Pairs of pins on one net in `before` but not in `after`.
fn lost(before: &Schematic, after: &Schematic) -> Vec<(PinRef, PinRef)> {
    let (b, a) = (pin_nets(before), pin_nets(after));
    let pins: Vec<_> = b.keys().copied().collect();
    let mut out = Vec::new();
    for (i, &x) in pins.iter().enumerate() {
        for &y in &pins[i + 1..] {
            let apart = a.get(&x).zip(a.get(&y)).is_none_or(|(p, q)| p != q);
            if b[&x] == b[&y] && apart {
                out.push((x, y));
            }
        }
    }
    out
}

fn diagonals(s: &Schematic) -> usize {
    s.wires
        .iter()
        .flat_map(|w| w.points.windows(2))
        .filter(|g| g[0].x != g[1].x && g[0].y != g[1].y)
        .count()
}

fn on_any_wire(s: &Schematic, p: Point) -> bool {
    s.wires
        .iter()
        .any(|w| w.points.windows(2).any(|g| on_segment(p, g[0], g[1])))
}

/// What is wrong with dragging the run `first..=last` of `wire` by `d`.
fn problems(
    s: &Schematic,
    wire: &Wire,
    plan: &SegmentPlan,
    out: &Schematic,
    d: i32,
) -> Vec<String> {
    let mut problems = Vec::new();
    let gone = lost(s, out);
    if !gone.is_empty() {
        problems.push(format!("lost {gone:?}"));
    }
    if diagonals(out) > diagonals(s) {
        problems.push("a new diagonal".to_string());
    }
    let mut again = out.clone();
    normalize(&mut again);
    if again != *out {
        problems.push("not normalised".to_string());
    }
    let offset = match plan.normal() {
        Axis::Horizontal => Point::new(d, 0),
        Axis::Vertical => Point::new(0, d),
    };
    let (first, last) = plan.run_ends();
    let pts = &wire.points;
    let (a, b) = (pts[first] + offset, pts[last] + offset);
    // A run dragged onto another leg of its own wire folds away with it;
    // anywhere else some wire holds it.
    let folds = (0..pts.len() - 1)
        .filter(|&k| k < first || k >= last)
        .any(|k| {
            let (g0, g1) = (pts[k], pts[k + 1]);
            collinear(g0, g1, a)
                && collinear(g0, g1, b)
                && (on_segment(a, g0, g1) || on_segment(b, g0, g1) || on_segment(g0, a, b))
        });
    if d != 0 && !folds {
        if !on_any_wire(out, a) || !on_any_wire(out, b) {
            problems.push("the run is not where it was dragged".to_string());
        }
        if plan.dragged_wire(out, Point::new(d, d)).is_none() {
            problems.push("no wire holds the run".to_string());
        }
    }
    problems
}

/// Drags every run of every wire of [`CASES`] random drawings on
/// `lattice` by every distance up to 12 units.
fn drag_randomly(lattice: Lattice) {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut failures: Vec<String> = Vec::new();
    let mut drags = 0;
    for _ in 0..CASES {
        let s = random_schematic(&mut rng, lattice);
        for w in &s.wires {
            for segment in 0..w.points.len() - 1 {
                let Some(plan) = SegmentPlan::new(&s, w.id, segment) else {
                    continue;
                };
                for d in -12..=12 {
                    let out = plan.apply(Point::new(d, d));
                    drags += 1;
                    let found = problems(&s, w, &plan, &out, d);
                    if !found.is_empty() {
                        let pins: Vec<_> = s.components.iter().map(|c| c.pins()).collect();
                        failures.push(format!(
                            "wire {} segment {segment} by {d}: {found:?}\n  before {:?}\n  after {:?}\n  pins {pins:?}\n  labels {:?}",
                            w.id, s.wires, out.wires, s.labels,
                        ));
                    }
                }
            }
        }
    }
    assert!(drags > 1000, "only {drags} drags were tried");
    assert!(
        failures.is_empty(),
        "{} of {drags} drags failed, e.g.\n{}",
        failures.len(),
        failures
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn random_segment_drags_keep_every_connection() {
    drag_randomly(Lattice::Schema2);
}

/// D22: the same on drawings using every grid unit, odd ones included.
#[test]
fn random_segment_drags_on_every_dot_keep_every_connection() {
    drag_randomly(Lattice::Fine);
}
