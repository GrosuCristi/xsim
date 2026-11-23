//! Helpers shared by the integration tests.

#![allow(dead_code)] // each test binary uses a different subset

use xsim_project::{Component, Id, NetLabel, Orientation, Part, PartKind, Point, Schematic, Wire};
use xsim_spice_engine::Source;

pub const P: fn(i32, i32) -> Point = Point::new;

/// Builds schematics tersely. Parts get the next free designator.
#[derive(Debug, Default)]
pub struct Sketch {
    pub schematic: Schematic,
}

impl Sketch {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn part(&mut self, part: Part, at: (i32, i32), rotation: u8) -> Id {
        let id = self.schematic.allocate_id();
        let name = self.schematic.next_designator(part.kind());
        self.schematic.components.push(Component {
            id,
            name,
            position: Point::new(at.0, at.1),
            orientation: Orientation {
                rotation,
                ..Orientation::default()
            },
            part,
        });
        id
    }

    pub fn resistor(&mut self, at: (i32, i32), rotation: u8) -> Id {
        self.part(Part::Resistor { resistance: 1e3 }, at, rotation)
    }

    pub fn capacitor(&mut self, at: (i32, i32), rotation: u8) -> Id {
        self.part(Part::default_for(PartKind::Capacitor), at, rotation)
    }

    pub fn voltage(&mut self, volts: f64, at: (i32, i32), rotation: u8) -> Id {
        self.part(
            Part::VoltageSource {
                source: Source::dc(volts),
            },
            at,
            rotation,
        )
    }

    pub fn ground(&mut self, at: (i32, i32)) -> Id {
        self.part(Part::Ground, at, 0)
    }

    pub fn wire(&mut self, points: &[(i32, i32)]) -> Id {
        let id = self.schematic.allocate_id();
        self.schematic.wires.push(Wire {
            id,
            points: points.iter().map(|&(x, y)| Point::new(x, y)).collect(),
        });
        id
    }

    pub fn label(&mut self, name: &str, at: (i32, i32)) -> Id {
        let id = self.schematic.allocate_id();
        self.schematic.labels.push(NetLabel {
            id,
            name: name.into(),
            position: Point::new(at.0, at.1),
        });
        id
    }

    pub fn component(&self, id: Id) -> &Component {
        self.schematic.component(id).expect("component exists")
    }
}

/// The RC low-pass shipped as `examples/RC low-pass.xsim_proj`.
///
/// ```text
///   in ┌──[R1]──┐ out
///      │        │
///     (V1)     ═╪═ C1
///      │        │
///      └───┬────┘
///         GND
/// ```
pub fn rc_low_pass_schematic() -> Schematic {
    let mut s = Sketch::new();
    // Vertical source with + on top: pin + at (0, 4), pin − at (0, 12).
    s.voltage(1.0, (0, 8), 1);
    s.resistor((12, 0), 0); // pins (8, 0) and (16, 0)
    s.capacitor((24, 8), 1); // pins (24, 4) and (24, 12)
    s.ground((12, 16)); // on the interior of the bottom rail
    s.wire(&[(0, 4), (0, 0), (8, 0)]);
    s.wire(&[(16, 0), (24, 0), (24, 4)]);
    s.wire(&[(0, 12), (0, 16), (24, 16), (24, 12)]);
    s.label("in", (0, 0));
    s.label("out", (24, 0));
    s.schematic
}

/// Index of the sample at time `t`, which must be on the output grid.
pub fn sample_at(times: &[f64], t: f64) -> usize {
    let (index, nearest) = times
        .iter()
        .enumerate()
        .min_by(|a, b| (a.1 - t).abs().total_cmp(&(b.1 - t).abs()))
        .expect("the result has samples");
    assert!(
        (nearest - t).abs() <= 1e-12 * t.abs().max(1e-12),
        "no sample at t = {t}; nearest is {nearest}"
    );
    index
}
