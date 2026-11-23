//! Symbol pin geometry. Shared by netlisting and the schematic editor so the
//! drawn pins and the electrical pins can never disagree.
//!
//! Local coordinates, before [`crate::Orientation`]:
//!
//! | Part | Pins (terminal order) |
//! |---|---|
//! | R, C, L, V, I, D | `(−2, 0)` positive/anode, `(2, 0)` negative/cathode |
//! | BJT | collector `(2, −2)`, base `(−2, 0)`, emitter `(2, 2)` |
//! | JFET | drain `(2, −2)`, gate `(−2, 0)`, source `(2, 2)` |
//! | Voltage switch (S) | switch `P (2, −2)`, `N (2, 2)`; control `C+ (−2, −1)`, `C− (−2, 1)` |
//! | Current switch (W) | `P (−2, 0)`, `N (2, 0)`; the control is a voltage source, by name |
//! | Ground | `(0, 0)` |
//!
//! The voltage switch's switch path runs down its right side, like a
//! transistor's collector and emitter, with the control input on the left.
//! Every pin of a part other than ground is at least two grid units from
//! the centre of its body, so pressing the centre grabs the part.

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

const TWO_PIN: [Point; 2] = [Point::new(-2, 0), Point::new(2, 0)];
const THREE_PIN: [Point; 3] = [Point::new(2, -2), Point::new(-2, 0), Point::new(2, 2)];
const VOLTAGE_SWITCH_PINS: [Point; 4] = [
    Point::new(2, -2),
    Point::new(2, 2),
    Point::new(-2, -1),
    Point::new(-2, 1),
];
const GROUND_PIN: [Point; 1] = [Point::new(0, 0)];

pub fn geometry(kind: PartKind) -> SymbolGeometry {
    match kind {
        PartKind::Resistor | PartKind::Capacitor | PartKind::Inductor => SymbolGeometry {
            pins: &TWO_PIN,
            pin_names: &["1", "2"],
            body_min: Point::new(-2, -1),
            body_max: Point::new(2, 1),
        },
        PartKind::VoltageSource | PartKind::CurrentSource => SymbolGeometry {
            pins: &TWO_PIN,
            pin_names: &["+", "−"],
            body_min: Point::new(-2, -1),
            body_max: Point::new(2, 1),
        },
        PartKind::Diode => SymbolGeometry {
            pins: &TWO_PIN,
            pin_names: &["A", "K"],
            body_min: Point::new(-2, -1),
            body_max: Point::new(2, 1),
        },
        PartKind::NpnBjt | PartKind::PnpBjt => SymbolGeometry {
            pins: &THREE_PIN,
            pin_names: &["C", "B", "E"],
            body_min: Point::new(-2, -2),
            body_max: Point::new(2, 2),
        },
        PartKind::NJfet | PartKind::PJfet => SymbolGeometry {
            pins: &THREE_PIN,
            pin_names: &["D", "G", "S"],
            body_min: Point::new(-2, -2),
            body_max: Point::new(2, 2),
        },
        PartKind::VoltageSwitch => SymbolGeometry {
            pins: &VOLTAGE_SWITCH_PINS,
            pin_names: &["P", "N", "C+", "C−"],
            body_min: Point::new(-2, -2),
            body_max: Point::new(2, 2),
        },
        PartKind::CurrentSwitch => SymbolGeometry {
            pins: &TWO_PIN,
            pin_names: &["P", "N"],
            body_min: Point::new(-2, -1),
            body_max: Point::new(2, 1),
        },
        PartKind::Ground => SymbolGeometry {
            pins: &GROUND_PIN,
            pin_names: &["0"],
            body_min: Point::new(-1, 0),
            body_max: Point::new(1, 1),
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
                    kind == PartKind::Ground || distance >= 2,
                    "{kind:?} pin {a:?} is next to the body centre"
                );
            }
        }
        let s = geometry(PartKind::VoltageSwitch);
        assert_eq!(s.pins.len(), 4);
        assert_eq!(s.pin_names, ["P", "N", "C+", "C−"]);
        assert_eq!(geometry(PartKind::CurrentSwitch).pin_names, ["P", "N"]);
    }
}
