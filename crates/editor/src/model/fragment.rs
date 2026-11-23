//! Clipboard fragments: copied parts, wires and labels (Q58).
//!
//! A fragment travels through the system clipboard as JSON text with a
//! marker, so it works between circuit tabs, xsim windows and projects, and
//! foreign clipboard text is ignored. Parts keep their parameters and model
//! names; pasted objects get fresh ids, and designators that clash with the
//! target schematic, or would break probe expressions (INT-4), are
//! renumbered. A current switch pasted with its control source follows
//! that source's new designator.
//!
//! Version 2 fragments hold coordinates in the grid units of schema 3, half
//! the unit of version 1 (D22). A version-1 fragment, copied from an xsim
//! still writing schema 2, is scaled on decoding so it pastes at its size.

use crate::model::geometry::{Rect, Vec2};
use crate::model::ops::translate_all;
use crate::model::selection::{ObjectRef, Selection};
use serde_json::{Value, json};
use std::collections::HashMap;
use xsim_project::{Part, PartKind, Point, Schematic};

/// Marker identifying xsim clipboard content.
const FORMAT: &str = "xsim/schematic-fragment";
/// Version of the fragment layout.
const VERSION: u64 = 2;
/// The last version with coordinates in schema-2 units.
const SCHEMA2_UNITS: u64 = 1;

/// The selected objects of `schematic` as a stand-alone schematic.
pub fn extract(schematic: &Schematic, selection: &Selection) -> Schematic {
    Schematic {
        components: schematic
            .components
            .iter()
            .filter(|c| selection.has_component(c.id))
            .cloned()
            .collect(),
        wires: schematic
            .wires
            .iter()
            .filter(|w| selection.has_wire(w.id))
            .cloned()
            .collect(),
        labels: schematic
            .labels
            .iter()
            .filter(|l| selection.has_label(l.id))
            .cloned()
            .collect(),
        next_id: 0,
    }
}

/// Clipboard text for a fragment.
pub fn encode(fragment: &Schematic) -> Option<String> {
    let body = serde_json::to_value(fragment).ok()?;
    serde_json::to_string(&json!({ "format": FORMAT, "version": VERSION, "fragment": body })).ok()
}

/// Parses clipboard text; `None` for anything that is not an xsim fragment.
pub fn decode(text: &str) -> Option<Schematic> {
    let value: Value = serde_json::from_str(text).ok()?;
    let version = value.get("version")?.as_u64()?;
    if value.get("format")?.as_str()? != FORMAT || version > VERSION {
        return None;
    }
    let mut fragment: Schematic = serde_json::from_value(value.get("fragment")?.clone()).ok()?;
    if version <= SCHEMA2_UNITS {
        fragment = doubled(fragment)?;
    }
    (!fragment.is_empty()).then_some(fragment)
}

/// A fragment in schema-2 units converted to grid units: every coordinate
/// doubled, as when a schema-2 project is opened. `None` when a coordinate
/// would overflow (clipboard text can hold anything).
fn doubled(mut fragment: Schematic) -> Option<Schematic> {
    let double = |p: &mut Point| -> Option<()> {
        *p = Point::new(p.x.checked_mul(2)?, p.y.checked_mul(2)?);
        Some(())
    };
    for c in &mut fragment.components {
        double(&mut c.position)?;
    }
    for p in fragment.wires.iter_mut().flat_map(|w| w.points.iter_mut()) {
        double(p)?;
    }
    for l in &mut fragment.labels {
        double(&mut l.position)?;
    }
    Some(fragment)
}

/// Grid point nearest the centre of everything in `fragment`.
pub fn center(fragment: &Schematic) -> Option<Point> {
    let points = fragment
        .components
        .iter()
        .flat_map(|c| {
            let (a, b) = c.bounds();
            [a, b]
        })
        .chain(fragment.wires.iter().flat_map(|w| w.points.iter().copied()))
        .chain(fragment.labels.iter().map(|l| l.position))
        .map(Vec2::from_grid);
    Rect::enclosing(points).map(|r| r.center().snapped())
}

/// Inserts `fragment` into `target` moved by `offset`, with fresh ids and
/// non-clashing designators. Returns the inserted objects.
///
/// A current switch pasted together with its control source senses the
/// pasted copy of that source (D17): when the source is renumbered, the
/// switch's control follows it. A switch pasted without its source keeps
/// naming it.
pub fn insert(target: &mut Schematic, fragment: &Schematic, offset: Point) -> Selection {
    let mut moved = fragment.clone();
    translate_all(&mut moved, offset);
    let mut inserted = Selection::default();
    // Copied designator → pasted designator of each renumbered voltage
    // source of the fragment.
    let mut renamed_sources: HashMap<String, String> = HashMap::new();
    let first = target.components.len();
    for mut c in moved.components {
        c.id = target.allocate_id();
        // Clipboard text can come from anywhere: a designator that would
        // break probe expressions (INT-4) is renumbered like a clash.
        let unusable = !xsim_project::netlist::is_valid_part_name(&c.name);
        if unusable || target.components.iter().any(|o| o.name == c.name) {
            let name = target.next_designator(c.part.kind());
            if c.part.kind() == PartKind::VoltageSource {
                renamed_sources
                    .entry(std::mem::take(&mut c.name))
                    .or_insert_with(|| name.clone());
            }
            c.name = name;
        }
        inserted.insert(ObjectRef::Component(c.id));
        target.components.push(c);
    }
    for c in target.components.iter_mut().skip(first) {
        if let Part::CurrentSwitch { control, .. } = &mut c.part
            && let Some(name) = renamed_sources.get(control.as_str())
        {
            control.clone_from(name);
        }
    }
    for mut w in moved.wires {
        w.id = target.allocate_id();
        inserted.insert(ObjectRef::Wire(w.id));
        target.wires.push(w);
    }
    for mut l in moved.labels {
        l.id = target.allocate_id();
        inserted.insert(ObjectRef::Label(l.id));
        target.labels.push(l);
    }
    inserted
}

#[cfg(test)]
mod tests {
    use super::{center, decode, encode, extract, insert};
    use crate::model::selection::{ObjectRef, Selection};
    use xsim_project::{Component, NetLabel, Orientation, Part, PartKind, Point, Schematic, Wire};

    fn source() -> Schematic {
        Schematic {
            components: vec![
                Component {
                    id: 1,
                    name: "R1".into(),
                    position: Point::new(0, 0),
                    orientation: Orientation::default(),
                    part: Part::Resistor { resistance: 4.7e3 },
                },
                Component {
                    id: 2,
                    name: "D7".into(),
                    position: Point::new(0, 12),
                    orientation: Orientation::default(),
                    part: Part::Diode {
                        model: "D_SIGNAL".into(),
                    },
                },
            ],
            wires: vec![Wire {
                id: 3,
                points: vec![Point::new(4, 0), Point::new(8, 0)],
            }],
            labels: vec![NetLabel {
                id: 4,
                name: "out".into(),
                position: Point::new(8, 0),
            }],
            next_id: 5,
        }
    }

    #[test]
    fn fragments_round_trip_through_clipboard_text() {
        let s = source();
        let fragment = extract(&s, &Selection::all(&s));
        let text = encode(&fragment).expect("serialisable");
        assert_eq!(decode(&text), Some(fragment));
    }

    /// D22: a fragment copied from an xsim still writing schema 2 has its
    /// coordinates in units twice as large; it pastes at its size.
    #[test]
    fn version_1_fragments_are_scaled_to_the_finer_grid() {
        let s = source();
        let mut old = extract(&s, &Selection::all(&s));
        for c in &mut old.components {
            c.position = Point::new(c.position.x / 2, c.position.y / 2);
        }
        for p in old.wires.iter_mut().flat_map(|w| w.points.iter_mut()) {
            *p = Point::new(p.x / 2, p.y / 2);
        }
        for l in &mut old.labels {
            l.position = Point::new(l.position.x / 2, l.position.y / 2);
        }
        let body = serde_json::to_value(&old).expect("serialisable");
        let text = serde_json::json!({
            "format": "xsim/schematic-fragment",
            "version": 1,
            "fragment": body,
        })
        .to_string();
        assert_eq!(decode(&text), Some(extract(&s, &Selection::all(&s))));
        // A coordinate that would overflow when doubled is not a fragment.
        old.components[0].position.x = i32::MAX;
        let body = serde_json::to_value(&old).expect("serialisable");
        let text = serde_json::json!({
            "format": "xsim/schematic-fragment", "version": 1, "fragment": body,
        })
        .to_string();
        assert_eq!(decode(&text), None);
        // Fragments of this version are taken as they are.
        let current = encode(&extract(&s, &Selection::all(&s))).expect("serialisable");
        assert!(current.contains("\"version\":2"), "{current}");
        assert_eq!(decode(&current), Some(extract(&s, &Selection::all(&s))));
    }

    #[test]
    fn foreign_text_is_ignored() {
        assert_eq!(decode("hello"), None);
        assert_eq!(
            decode(r#"{"format":"other","version":1,"fragment":{}}"#),
            None
        );
        assert_eq!(decode("{\"components\":[]}"), None);
    }

    #[test]
    fn pasting_remaps_ids_and_renumbers_clashing_designators() {
        let s = source();
        let fragment = extract(&s, &Selection::all(&s));
        let mut target = source();
        let inserted = insert(&mut target, &fragment, Point::new(20, 0));
        assert_eq!(inserted.len(), 4);
        let names: Vec<_> = target.components.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["R1", "D7", "R2", "D1"]);
        let mut ids: Vec<_> = target
            .components
            .iter()
            .map(|c| c.id)
            .chain(target.wires.iter().map(|w| w.id))
            .chain(target.labels.iter().map(|l| l.id))
            .collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 8, "every object has a unique id");
        assert_eq!(target.wires[1].points[0], Point::new(24, 0));
        assert_eq!(target.labels[1].name, "out", "labels keep their net name");
        assert!(inserted.contains(ObjectRef::Label(target.labels[1].id)));
    }

    #[test]
    fn pasted_designators_that_would_break_probes_are_renumbered() {
        let mut fragment = extract(&source(), &Selection::all(&source()));
        fragment.components[0].name = "R(1)".into();
        fragment.components[1].name = "D 7".into();
        let mut target = Schematic::default();
        insert(&mut target, &fragment, Point::new(0, 0));
        let names: Vec<_> = target.components.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["R1", "D1"]);
    }

    #[test]
    fn pasting_into_an_empty_schematic_keeps_designators() {
        let s = source();
        let fragment = extract(&s, &Selection::all(&s));
        let mut target = Schematic::default();
        insert(&mut target, &fragment, Point::new(0, 0));
        let names: Vec<_> = target.components.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["R1", "D7"]);
    }

    /// A voltage source `name` and a current switch controlled by
    /// `control`, the switch first.
    fn sensed(name: &str, switch: &str, control: &str) -> Schematic {
        let mut s = Schematic::default();
        let mut add = |name: &str, part: Part, x: i32| {
            let id = s.allocate_id();
            s.components.push(Component {
                id,
                name: name.into(),
                position: Point::new(x, 0),
                orientation: Orientation::default(),
                part,
            });
        };
        let Part::CurrentSwitch { model, .. } = Part::default_for(PartKind::CurrentSwitch) else {
            unreachable!()
        };
        let control = control.to_string();
        add(switch, Part::CurrentSwitch { control, model }, 0);
        add(name, Part::default_for(PartKind::VoltageSource), 20);
        s
    }

    fn control_of(s: &Schematic, switch: &str) -> String {
        match s
            .components
            .iter()
            .find(|c| c.name == switch)
            .map(|c| &c.part)
        {
            Some(Part::CurrentSwitch { control, .. }) => control.clone(),
            other => panic!("{switch}: {other:?}"),
        }
    }

    #[test]
    fn a_current_switch_pasted_with_its_control_source_senses_the_copy() {
        // Copying W1 with V1 into a schematic that already has both renames
        // them W2 and V2; W2 must sense V2, not the original V1.
        let s = sensed("V1", "W1", "V1");
        let fragment = extract(&s, &Selection::all(&s));
        let mut target = s.clone();
        insert(&mut target, &fragment, Point::new(0, 20));
        let names: Vec<_> = target.components.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["W1", "V1", "W2", "V2"]);
        assert_eq!(control_of(&target, "W1"), "V1", "the original is untouched");
        assert_eq!(control_of(&target, "W2"), "V2");
        // Into another schematic without a clash nothing is renamed.
        let mut other = Schematic::default();
        insert(&mut other, &fragment, Point::new(0, 0));
        assert_eq!(control_of(&other, "W1"), "V1");
    }

    #[test]
    fn a_current_switch_pasted_alone_keeps_its_control() {
        // Without its source the switch still names the target's source.
        let s = sensed("V1", "W1", "V1");
        let mut selection = Selection::default();
        selection.insert(ObjectRef::Component(s.components[0].id));
        let fragment = extract(&s, &selection);
        let mut target = s.clone();
        insert(&mut target, &fragment, Point::new(0, 20));
        assert_eq!(control_of(&target, "W2"), "V1");
        // Only a renamed voltage source is followed: the fragment's resistor
        // R1 clashes with the target's and becomes R2, but a (dangling)
        // control "R1" is left as it is, as is the source VS that keeps its
        // name.
        let mut s = sensed("VS", "W1", "R1");
        let id = s.allocate_id();
        s.components.push(Component {
            id,
            name: "R1".into(),
            position: Point::new(40, 0),
            orientation: Orientation::default(),
            part: Part::Resistor { resistance: 1e3 },
        });
        let fragment = extract(&s, &Selection::all(&s));
        let mut target = source();
        insert(&mut target, &fragment, Point::new(0, 20));
        let names: Vec<_> = target.components.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["R1", "D7", "W1", "VS", "R2"]);
        assert_eq!(control_of(&target, "W1"), "R1");
    }

    #[test]
    fn controls_follow_each_pasted_source_to_its_own_new_name() {
        // The target has V1 only. The fragment's V1 becomes V2, and then its
        // V2 clashes with that and becomes V3: each switch follows its own
        // source, never a chain of renames.
        let mut s = sensed("V1", "W1", "V1");
        let mut more = sensed("V2", "W2", "V2");
        for c in &mut more.components {
            c.id = s.allocate_id();
            c.position.y = 40;
        }
        s.components.extend(more.components);
        let fragment = extract(&s, &Selection::all(&s));
        let mut target = Schematic::default();
        let id = target.allocate_id();
        target.components.push(Component {
            id,
            name: "V1".into(),
            position: Point::new(100, 100),
            orientation: Orientation::default(),
            part: Part::default_for(PartKind::VoltageSource),
        });
        insert(&mut target, &fragment, Point::new(0, 0));
        let names: Vec<_> = target.components.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["V1", "W1", "V2", "W2", "V3"]);
        assert_eq!(control_of(&target, "W1"), "V2");
        assert_eq!(control_of(&target, "W2"), "V3");
    }

    #[test]
    fn the_centre_covers_every_object() {
        assert_eq!(center(&source()), Some(Point::new(2, 6)));
        assert_eq!(center(&Schematic::default()), None);
    }
}
