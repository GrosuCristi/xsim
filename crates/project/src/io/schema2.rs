//! Migration from schema 2, whose grid unit was twice as large (D22).
//!
//! Schema 3 stores coordinates at the pitch of the editor's dot grid, half a
//! schema-2 unit, so parts, wires and labels snap to every dot. A schema-2
//! document becomes schema 3 by doubling every schematic coordinate (part
//! positions, wire vertices and label positions) and the view's centre,
//! and by halving the view's zoom (pixels per unit). Symbols are twice as
//! large in schema-3 units, so every pin stays where it was: the drawing,
//! its connectivity and its netlist are unchanged (and with it a stored
//! result's circuit fingerprint, which hashes the netlisted circuit), and
//! the view shows the same picture.
//!
//! The document is migrated as JSON, before it is decoded. Fields a file
//! leaves out therefore take their schema-3 defaults afterwards, which is
//! what their schema-2 defaults mean once converted: a view without a zoom
//! still opens at the default zoom. Values that are not numbers are left as
//! they are, for decoding to reject. A coordinate outside the schema-2
//! drawing area is an error, as it was in schema 2, so doubling never
//! overflows and every valid schema-2 file migrates to a valid schema-3 one.

use crate::document::SchematicView;
use crate::error::ProjectError;
use crate::io::validate::MAX_COORDINATE;
use serde_json::{Map, Value};

/// The schema this step produces.
const TARGET_SCHEMA: u32 = 3;

/// Largest coordinate magnitude of a valid schema-2 file: half the
/// schema-3 limit, the same drawing area.
const SCHEMA2_MAX_COORDINATE: u32 = MAX_COORDINATE / 2;

/// The default view of schema 2, in schema-2 units: the origin at 20 pixels
/// per unit. Documents migrated from schema 1 start from it.
pub(super) fn default_view() -> SchematicView {
    SchematicView {
        center: [0.0, 0.0],
        zoom: 20.0,
        user_set: false,
    }
}

/// Lifts the schema-2 `document` to schema 3 in place.
pub(super) fn migrate(document: &mut Value) -> Result<(), ProjectError> {
    let fields = document.as_object_mut().ok_or_else(super::not_a_project)?;
    for circuit in array_items(fields, "circuits").filter_map(Value::as_object_mut) {
        migrate_circuit(circuit)?;
    }
    fields.insert("schema_version".into(), Value::from(TARGET_SCHEMA));
    Ok(())
}

fn migrate_circuit(circuit: &mut Map<String, Value>) -> Result<(), ProjectError> {
    let name = circuit
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let outside = |coordinate: i64| {
        ProjectError::Invalid(format!(
            "Circuit \"{name}\": the coordinate {coordinate} lies outside the drawing area of \
             ±{SCHEMA2_MAX_COORDINATE}."
        ))
    };
    if let Some(schematic) = circuit.get_mut("schematic").and_then(Value::as_object_mut) {
        for key in ["components", "labels"] {
            for item in array_items(schematic, key) {
                if let Some(position) = item.get_mut("position") {
                    double_point(position).map_err(outside)?;
                }
            }
        }
        for wire in array_items(schematic, "wires") {
            let points = wire.get_mut("points").and_then(Value::as_array_mut);
            for point in points.into_iter().flatten() {
                double_point(point).map_err(outside)?;
            }
        }
    }
    if let Some(view) = circuit.get_mut("view").and_then(Value::as_object_mut) {
        let center = view.get_mut("center").and_then(Value::as_array_mut);
        for value in center.into_iter().flatten() {
            double_center(value);
        }
        if let Some(zoom) = view.get_mut("zoom") {
            scale_number(zoom, 0.5);
        }
    }
    Ok(())
}

/// The elements of the array `fields[key]`, if it is one.
fn array_items<'a>(
    fields: &'a mut Map<String, Value>,
    key: &str,
) -> impl Iterator<Item = &'a mut Value> {
    fields
        .get_mut(key)
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
}

/// Doubles the integer `x` and `y` of a point object. `Err` carries a
/// coordinate outside the schema-2 drawing area.
fn double_point(point: &mut Value) -> Result<(), i64> {
    let Some(fields) = point.as_object_mut() else {
        return Ok(());
    };
    for axis in ["x", "y"] {
        let Some(value) = fields.get_mut(axis) else {
            continue;
        };
        if let Some(coordinate) = value.as_i64() {
            if coordinate.unsigned_abs() > u64::from(SCHEMA2_MAX_COORDINATE) {
                return Err(coordinate);
            }
            *value = Value::from(2 * coordinate);
        }
    }
    Ok(())
}

/// Multiplies a JSON number by `factor`, a power of two, so a value read
/// back as `f32` is scaled exactly.
fn scale_number(value: &mut Value, factor: f64) {
    if let Some(number) = value.as_f64() {
        *value = Value::from(number * factor);
    }
}

/// Doubles a view-centre coordinate. A schema-2 centre beyond half the
/// `f32` range (valid, if far from any drawing) would double to infinity,
/// which no view can hold: it stops at the largest finite `f32` instead,
/// as far out, so the file still opens. A value already outside the `f32`
/// range was invalid in schema 2 and is left for validation to reject.
fn double_center(value: &mut Value) {
    let limit = f64::from(f32::MAX);
    if let Some(number) = value.as_f64().filter(|n| n.abs() <= limit) {
        *value = Value::from((2.0 * number).clamp(-limit, limit));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn coordinates_double_and_the_zoom_halves() {
        let mut document = json!({
            "schema_version": 2,
            "circuits": [{
                "name": "c",
                "schematic": {
                    "components": [{"position": {"x": -3, "y": 5}}, {}],
                    "wires": [{"points": [{"x": 1, "y": 0}, {"x": 1, "y": -7}]}],
                    "labels": [{"position": {"x": 0, "y": 100000000}}]
                },
                "view": {"center": [1.25, -0.5], "zoom": 27.5, "user_set": true}
            }]
        });
        migrate(&mut document).unwrap();
        assert_eq!(
            document,
            json!({
                "schema_version": 3,
                "circuits": [{
                    "name": "c",
                    "schematic": {
                        "components": [{"position": {"x": -6, "y": 10}}, {}],
                        "wires": [{"points": [{"x": 2, "y": 0}, {"x": 2, "y": -14}]}],
                        "labels": [{"position": {"x": 0, "y": 200000000}}]
                    },
                    "view": {"center": [2.5, -1.0], "zoom": 13.75, "user_set": true}
                }]
            })
        );
    }

    #[test]
    fn missing_and_mistyped_values_are_left_for_decoding() {
        let mut document = json!({
            "schema_version": 2,
            "circuits": [
                {"name": "no view or schematic"},
                {"schematic": {"components": "none", "wires": [{"points": [{"x": 1.5}]}]},
                 "view": {"center": "middle"}},
                7
            ]
        });
        let mut expected = document.clone();
        expected["schema_version"] = json!(3);
        migrate(&mut document).unwrap();
        assert_eq!(document, expected);
    }

    #[test]
    fn a_far_view_centre_stays_finite_as_f32() {
        let max = f64::from(f32::MAX);
        let mut document = json!({
            "circuits": [{"view": {"center": [3.0e38, -2.0e38], "zoom": 20.0}}]
        });
        migrate(&mut document).unwrap();
        assert_eq!(
            document["circuits"][0]["view"]["center"],
            json!([max, -max])
        );
        // Half the f32 range and less doubles exactly.
        let mut document = json!({"circuits": [{"view": {"center": [1.5e38, -0.75]}}]});
        migrate(&mut document).unwrap();
        assert_eq!(
            document["circuits"][0]["view"]["center"],
            json!([3.0e38, -1.5])
        );
        // Beyond f32 the schema-2 file was already invalid: left as it is.
        let mut document = json!({"circuits": [{"view": {"center": [1.0e300, 1.0e308]}}]});
        migrate(&mut document).unwrap();
        assert_eq!(
            document["circuits"][0]["view"]["center"],
            json!([1.0e300, 1.0e308])
        );
    }

    #[test]
    fn coordinates_outside_the_schema_2_drawing_area_are_errors() {
        for coordinate in [100_000_001_i64, -100_000_001, 2_000_000_000, i64::MIN] {
            let mut document = json!({
                "circuits": [{
                    "name": "far",
                    "schematic": {"wires": [{"points": [{"x": 0, "y": coordinate}]}]}
                }]
            });
            match migrate(&mut document) {
                Err(ProjectError::Invalid(message)) => {
                    assert!(message.contains("Circuit \"far\""), "{message}");
                    assert!(message.contains(&coordinate.to_string()), "{message}");
                }
                other => panic!("{coordinate}: expected Invalid, got {other:?}"),
            }
        }
    }
}
