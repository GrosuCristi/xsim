//! Small schematics shared by the interaction tests.

use xsim_project::{Component, NetLabel, Orientation, Part, PartKind, Point, Schematic, Wire};

fn p(x: i32, y: i32) -> Point {
    Point::new(x, y)
}

fn upright() -> Orientation {
    Orientation {
        rotation: 1,
        ..Orientation::default()
    }
}

fn component(
    id: u64,
    name: &str,
    kind: PartKind,
    position: Point,
    orientation: Orientation,
) -> Component {
    Component {
        id,
        name: name.into(),
        position,
        orientation,
        part: Part::default_for(kind),
    }
}

fn wire(id: u64, points: &[Point]) -> Wire {
    Wire {
        id,
        points: points.to_vec(),
    }
}

/// R1 at the origin wired to R2 at (8, 0): pins (2, 0) and (6, 0).
pub fn two_resistors() -> Schematic {
    Schematic {
        components: vec![
            component(1, "R1", PartKind::Resistor, p(0, 0), Orientation::default()),
            component(2, "R2", PartKind::Resistor, p(8, 0), Orientation::default()),
        ],
        wires: vec![wire(3, &[p(2, 0), p(6, 0)])],
        labels: Vec::new(),
        next_id: 4,
    }
}

/// R1 at the origin, R2 at (4, 0) and R3 at (8, 0) end to end, each pair
/// touching pin to pin with no wire: at (2, 0) and (6, 0).
pub fn touching_chain() -> Schematic {
    Schematic {
        components: vec![
            component(1, "R1", PartKind::Resistor, p(0, 0), Orientation::default()),
            component(2, "R2", PartKind::Resistor, p(4, 0), Orientation::default()),
            component(3, "R3", PartKind::Resistor, p(8, 0), Orientation::default()),
        ],
        wires: Vec::new(),
        labels: Vec::new(),
        next_id: 4,
    }
}

/// R1 at the origin and R2 at (4, 0), touching pin to pin at (2, 0).
pub fn touching_pair() -> Schematic {
    let mut s = touching_chain();
    s.components.truncate(2);
    s
}

/// An RC low-pass: V1 → R1 → C1 to ground, nets labelled `in` and `out`.
///
/// ```text
///  (-6,-4) in ── R1 ── (6,-4) out
///    │                   │
///    V1 (-6,0)           C1 (6,0)
///    │                   │
///  (-6,4) ─── GND (0,4) ─ (6,4)
/// ```
pub fn rc_low_pass() -> Schematic {
    Schematic {
        components: vec![
            component(1, "V1", PartKind::VoltageSource, p(-6, 0), upright()),
            component(
                2,
                "R1",
                PartKind::Resistor,
                p(0, -4),
                Orientation::default(),
            ),
            component(3, "C1", PartKind::Capacitor, p(6, 0), upright()),
            component(4, "GND1", PartKind::Ground, p(0, 4), Orientation::default()),
        ],
        wires: vec![
            wire(5, &[p(-6, -2), p(-6, -4), p(-2, -4)]),
            wire(6, &[p(2, -4), p(6, -4), p(6, -2)]),
            wire(7, &[p(-6, 2), p(-6, 4), p(6, 4), p(6, 2)]),
        ],
        labels: vec![
            NetLabel {
                id: 8,
                name: "in".into(),
                position: p(-6, -4),
            },
            NetLabel {
                id: 9,
                name: "out".into(),
                position: p(6, -4),
            },
        ],
        next_id: 10,
    }
}
