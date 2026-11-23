//! Rubber-band sweep (D20): parts touching pin to pin, moved by every
//! displacement within twenty grid units (ten schema-2 units, so every dot
//! in between), alone and as part of a selection. Every
//! move must keep every pin on the net it was on (the touching pins stay
//! joined, and no part is shorted or joined to anything new) and add no
//! diagonal. Moves that land a moved pin on another pin, a label or a wire
//! that stays are left out: it connects there by the connection rules
//! (Q50). The review found
//! that one move in five shorted a part before inserted wires avoided
//! other pins.

use super::drag::MovePlan;
use super::geometry::on_segment;
use super::selection::{ObjectRef, Selection};
use std::collections::BTreeMap;
use xsim_project::{
    Component, Id, NetLabel, Orientation, Part, PartKind, Point, Schematic, Wire, connectivity,
};

/// Farthest displacement tried along each axis.
const REACH: i32 = 20;

fn p(x: i32, y: i32) -> Point {
    Point::new(x, y)
}

fn part(id: Id, kind: PartKind, at: Point, rotation: u8) -> Component {
    Component {
        id,
        name: format!("X{id}"),
        position: at,
        orientation: Orientation {
            rotation,
            ..Orientation::default()
        },
        part: Part::default_for(kind),
    }
}

fn drawing(components: Vec<Component>) -> Schematic {
    let next_id = components.iter().map(|c| c.id).max().unwrap_or(0) + 1;
    Schematic {
        components,
        next_id,
        ..Schematic::default()
    }
}

/// Parts touching pin to pin: end to end, a chain of three, at a right
/// angle, a transistor's base, a ground under a capacitor, and a pair with
/// a labelled wire hanging off the touching point.
fn fixtures() -> Vec<(&'static str, Schematic)> {
    use PartKind::*;
    let mut labelled = drawing(vec![
        part(1, Resistor, p(0, 0), 0),
        part(2, Resistor, p(8, 0), 0),
    ]);
    labelled.wires.push(Wire {
        id: 3,
        points: vec![p(4, 0), p(4, 8)],
    });
    labelled.labels.push(NetLabel {
        id: 4,
        name: "mid".into(),
        position: p(4, 8),
    });
    labelled.next_id = 5;
    vec![
        (
            "pair",
            drawing(vec![
                part(1, Resistor, p(0, 0), 0),
                part(2, Resistor, p(8, 0), 0),
            ]),
        ),
        (
            "chain",
            drawing(vec![
                part(1, Resistor, p(0, 0), 0),
                part(2, Resistor, p(8, 0), 0),
                part(3, Resistor, p(16, 0), 0),
            ]),
        ),
        (
            "corner",
            drawing(vec![
                part(1, Resistor, p(0, 0), 0),
                part(2, Capacitor, p(4, 4), 1),
            ]),
        ),
        (
            "base",
            drawing(vec![
                part(1, Resistor, p(-8, 0), 0),
                part(2, NpnBjt, p(0, 0), 0),
            ]),
        ),
        (
            "ground",
            drawing(vec![
                part(1, Capacitor, p(0, -4), 1),
                part(2, Ground, p(0, 0), 0),
            ]),
        ),
        ("labelled", labelled),
    ]
}

/// The net of every pin, named by the smallest pin on it.
fn nets(s: &Schematic) -> BTreeMap<(Id, usize), (Id, usize)> {
    let c = connectivity(s);
    let mut out = BTreeMap::new();
    for component in &s.components {
        for pin in 0..component.pins().len() {
            let net = c
                .net_of_pin(component.id, pin)
                .and_then(|n| c.nets[n].pins.iter().min().copied())
                .unwrap_or((component.id, pin));
            out.insert((component.id, pin), net);
        }
    }
    out
}

/// Every proper, non-empty subset of the parts.
fn selections(s: &Schematic) -> Vec<Selection> {
    let n = s.components.len();
    (1..(1u32 << n) - 1)
        .map(|mask| {
            s.components
                .iter()
                .enumerate()
                .filter(|(i, _)| mask & (1 << i) != 0)
                .map(|(_, c)| ObjectRef::Component(c.id))
                .collect()
        })
        .collect()
}

#[test]
fn moving_touching_parts_anywhere_keeps_every_pin_on_its_net() {
    let mut failures = Vec::new();
    let mut moves = 0;
    for (name, s) in fixtures() {
        let before = nets(&s);
        for selection in selections(&s) {
            let plan = MovePlan::new(&s, &selection);
            assert_eq!(plan.apply(p(0, 0)), s, "{name}: a zero move");
            let (moving, staying): (Vec<&Component>, Vec<&Component>) = s
                .components
                .iter()
                .partition(|c| selection.has_component(c.id));
            let staying_pins: Vec<Point> = staying.iter().flat_map(|c| c.pins()).collect();
            for dx in -REACH..=REACH {
                for dy in -REACH..=REACH {
                    let d = p(dx, dy);
                    let lands = moving.iter().flat_map(|c| c.pins()).any(|q| {
                        let q = q + d;
                        staying_pins.contains(&q)
                            || s.labels.iter().any(|l| l.position == q)
                            || s.wires
                                .iter()
                                .any(|w| w.points.windows(2).any(|g| on_segment(q, g[0], g[1])))
                    });
                    if d == p(0, 0) || lands {
                        continue;
                    }
                    moves += 1;
                    let moved = plan.apply(d);
                    let diagonal = moved.wires.iter().any(|w| {
                        w.points
                            .windows(2)
                            .any(|g| g[0].x != g[1].x && g[0].y != g[1].y)
                    });
                    if diagonal || nets(&moved) != before {
                        let wires: Vec<&Vec<Point>> =
                            moved.wires.iter().map(|w| &w.points).collect();
                        failures.push(format!("{name}, {selection:?} by {d:?}: {wires:?}"));
                    }
                }
            }
        }
    }
    assert!(moves > 5_000, "the sweep ran: {moves} moves");
    assert!(
        failures.is_empty(),
        "{} of {moves} moves broke a net, e.g. {:#?}",
        failures.len(),
        &failures[..failures.len().min(10)]
    );
}
