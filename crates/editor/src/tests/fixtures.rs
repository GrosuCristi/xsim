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

/// R1 at the origin wired to R2 at (16, 0): pins (4, 0) and (12, 0).
pub fn two_resistors() -> Schematic {
    Schematic {
        components: vec![
            component(1, "R1", PartKind::Resistor, p(0, 0), Orientation::default()),
            component(
                2,
                "R2",
                PartKind::Resistor,
                p(16, 0),
                Orientation::default(),
            ),
        ],
        wires: vec![wire(3, &[p(4, 0), p(12, 0)])],
        labels: Vec::new(),
        next_id: 4,
    }
}

/// R1 at the origin, R2 at (8, 0) and R3 at (16, 0) end to end, each pair
/// touching pin to pin with no wire: at (4, 0) and (12, 0).
pub fn touching_chain() -> Schematic {
    Schematic {
        components: vec![
            component(1, "R1", PartKind::Resistor, p(0, 0), Orientation::default()),
            component(2, "R2", PartKind::Resistor, p(8, 0), Orientation::default()),
            component(
                3,
                "R3",
                PartKind::Resistor,
                p(16, 0),
                Orientation::default(),
            ),
        ],
        wires: Vec::new(),
        labels: Vec::new(),
        next_id: 4,
    }
}

/// R1 at the origin and R2 at (8, 0), touching pin to pin at (4, 0).
pub fn touching_pair() -> Schematic {
    let mut s = touching_chain();
    s.components.truncate(2);
    s
}

/// An RC low-pass: V1 → R1 → C1 to ground, nets labelled `in` and `out`.
///
/// ```text
///  (-12,-8) in ── R1 ── (12,-8) out
///    │                   │
///    V1 (-12,0)           C1 (12,0)
///    │                   │
///  (-12,8) ─── GND (0,8) ─ (12,8)
/// ```
pub fn rc_low_pass() -> Schematic {
    Schematic {
        components: vec![
            component(1, "V1", PartKind::VoltageSource, p(-12, 0), upright()),
            component(
                2,
                "R1",
                PartKind::Resistor,
                p(0, -8),
                Orientation::default(),
            ),
            component(3, "C1", PartKind::Capacitor, p(12, 0), upright()),
            component(4, "GND1", PartKind::Ground, p(0, 8), Orientation::default()),
        ],
        wires: vec![
            wire(5, &[p(-12, -4), p(-12, -8), p(-4, -8)]),
            wire(6, &[p(4, -8), p(12, -8), p(12, -4)]),
            wire(7, &[p(-12, 4), p(-12, 8), p(12, 8), p(12, 4)]),
        ],
        labels: vec![
            NetLabel {
                id: 8,
                name: "in".into(),
                position: p(-12, -8),
            },
            NetLabel {
                id: 9,
                name: "out".into(),
                position: p(12, -8),
            },
        ],
        next_id: 10,
    }
}
