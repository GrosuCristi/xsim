//! Symbol pin geometry. Shared by netlisting and the schematic editor so the
//! drawn pins and the electrical pins can never disagree.
//!
//! Local coordinates in grid units, before [`crate::Orientation`]. A grid
//! unit is the editor's dot pitch, half the unit of schema-2 files (D22),
//! so a two-pin part is eight units long:
//!
//! | Part | Pins (terminal order) |
//! |---|---|
//! | R, C, L, V, I, D | `(−4, 0)` positive/anode, `(4, 0)` negative/cathode |
//! | BJT | collector `(4, −4)`, base `(−4, 0)`, emitter `(4, 4)` |
//! | JFET | drain `(4, −4)`, gate `(−4, 0)`, source `(4, 4)` |
//! | Voltage switch (S) | switch `P (4, −4)`, `N (4, 4)`; control `C+ (−4, −2)`, `C− (−4, 2)` |
//! | Current switch (W) | `P (−4, 0)`, `N (4, 0)`; the control is a voltage source, by name |
//! | Op-amp (U) | `+ (−4, 2)`, `− (−4, −2)`, `OUT (4, 0)`, `V+ (0, −4)`, `V− (0, 4)` |
//! | Ground | `(0, 0)` |
//!
//! The voltage switch's switch path runs down its right side, like a
//! transistor's collector and emitter, with the control input on the left.
//! Every pin of a part other than ground is at least four grid units from
//! the centre of its body, so pressing the centre grabs the part.
//!
//! The op-amp (D26) is a triangle pointing right with its inputs on the
//! left, the inverting input on top as in LTspice's op-amp symbols (so the
//! feedback of an inverting stage runs over the top), the output at the
//! apex and the supplies on the top (V+) and bottom (V−) edges.

use crate::schematic::{PartKind, Point};

/// Static pin layout and body extents of a symbol in local coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SymbolGeometry {
    /// Pin positions in engine terminal order.
    pub pins: &'static [Point],
    /// Pin names in the same order (`"+"`, `"−"`, `"A"`, `"K"`, `"C"`, ...).
    pub pin_names: &'static [&'static str],
    /// Bounding box of the drawn body including pins.
    pub body_min: Point,
    pub body_max: Point,
}

const TWO_PIN: [Point; 2] = [Point::new(-4, 0), Point::new(4, 0)];
const THREE_PIN: [Point; 3] = [Point::new(4, -4), Point::new(-4, 0), Point::new(4, 4)];
const VOLTAGE_SWITCH_PINS: [Point; 4] = [
    Point::new(4, -4),
    Point::new(4, 4),
    Point::new(-4, -2),
    Point::new(-4, 2),
];
const OPAMP_PINS: [Point; 5] = [
    Point::new(-4, 2),
    Point::new(-4, -2),
    Point::new(4, 0),
    Point::new(0, -4),
    Point::new(0, 4),
];
const GROUND_PIN: [Point; 1] = [Point::new(0, 0)];

pub fn geometry(kind: PartKind) -> SymbolGeometry {
    match kind {
        PartKind::Resistor | PartKind::Capacitor | PartKind::Inductor => SymbolGeometry {
            pins: &TWO_PIN,
            pin_names: &["1", "2"],
            body_min: Point::new(-4, -2),
            body_max: Point::new(4, 2),
        },
        PartKind::VoltageSource | PartKind::CurrentSource => SymbolGeometry {
            pins: &TWO_PIN,
            pin_names: &["+", "−"],
            body_min: Point::new(-4, -2),
            body_max: Point::new(4, 2),
        },
        PartKind::Diode => SymbolGeometry {
            pins: &TWO_PIN,
            pin_names: &["A", "K"],
            body_min: Point::new(-4, -2),
            body_max: Point::new(4, 2),
        },
        PartKind::NpnBjt | PartKind::PnpBjt => SymbolGeometry {
            pins: &THREE_PIN,
            pin_names: &["C", "B", "E"],
            body_min: Point::new(-4, -4),
            body_max: Point::new(4, 4),
        },
        PartKind::NJfet | PartKind::PJfet => SymbolGeometry {
            pins: &THREE_PIN,
            pin_names: &["D", "G", "S"],
            body_min: Point::new(-4, -4),
            body_max: Point::new(4, 4),
        },
        PartKind::VoltageSwitch => SymbolGeometry {
            pins: &VOLTAGE_SWITCH_PINS,
            pin_names: &["P", "N", "C+", "C−"],
            body_min: Point::new(-4, -4),
            body_max: Point::new(4, 4),
        },
        PartKind::CurrentSwitch => SymbolGeometry {
            pins: &TWO_PIN,
            pin_names: &["P", "N"],
            body_min: Point::new(-4, -2),
            body_max: Point::new(4, 2),
        },
        PartKind::OpAmp => SymbolGeometry {
            pins: &OPAMP_PINS,
            pin_names: &["+", "−", "OUT", "V+", "V−"],
            body_min: Point::new(-4, -4),
            body_max: Point::new(4, 4),
        },
        PartKind::Ground => SymbolGeometry {
            pins: &GROUND_PIN,
            pin_names: &["0"],
            body_min: Point::new(-2, 0),
            body_max: Point::new(2, 2),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pin_has_a_name_and_lies_within_the_body_box() {
        for kind in PartKind::ALL {
            let g = geometry(kind);
            assert_eq!(g.pins.len(), g.pin_names.len(), "{kind:?}");
            for p in g.pins {
                let inside = (g.body_min.x..=g.body_max.x).contains(&p.x)
                    && (g.body_min.y..=g.body_max.y).contains(&p.y);
                assert!(inside, "{kind:?} pin {p:?}");
            }
        }
    }

    #[test]
    fn pins_are_distinct_and_clear_of_the_body_centre() {
        for kind in PartKind::ALL {
            let g = geometry(kind);
            let centre = Point::new(
                (g.body_min.x + g.body_max.x) / 2,
                (g.body_min.y + g.body_max.y) / 2,
            );
            for (i, a) in g.pins.iter().enumerate() {
                assert!(
                    g.pins[i + 1..].iter().all(|b| b != a),
                    "{kind:?}: two pins at {a:?}"
                );
                let distance = (a.x - centre.x).abs().max((a.y - centre.y).abs());
                assert!(
                    kind == PartKind::Ground || distance >= 4,
                    "{kind:?} pin {a:?} is next to the body centre"
                );
            }
        }
        let s = geometry(PartKind::VoltageSwitch);
        assert_eq!(s.pins.len(), 4);
        assert_eq!(s.pin_names, ["P", "N", "C+", "C−"]);
        assert_eq!(geometry(PartKind::CurrentSwitch).pin_names, ["P", "N"]);
    }

    /// D22: the grid unit is half the schema-2 unit, so every symbol is
    /// twice the schema-2 geometry: two-pin parts are eight units long, and
    /// every pin and body corner lies on even coordinates, where a migrated
    /// schema-2 drawing puts them.
    #[test]
    fn symbols_are_twice_the_schema_2_geometry() {
        assert_eq!(
            geometry(PartKind::Resistor).pins,
            [Point::new(-4, 0), Point::new(4, 0)]
        );
        assert_eq!(
            geometry(PartKind::NpnBjt).pins,
            [Point::new(4, -4), Point::new(-4, 0), Point::new(4, 4)]
        );
        for kind in PartKind::ALL {
            let g = geometry(kind);
            let corners = [g.body_min, g.body_max];
            for p in g.pins.iter().chain(&corners) {
                assert!(p.x % 2 == 0 && p.y % 2 == 0, "{kind:?}: {p:?}");
            }
        }
    }
}
