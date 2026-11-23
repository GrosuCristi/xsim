//! Schematic edit operations, free of any UI.
//!
//! Each function edits a [`Schematic`] in place; the editor wraps them into
//! undoable steps and runs [`crate::model::cleanup::normalize`] afterwards.

use crate::model::selection::Selection;
use xsim_project::{Component, Id, NetLabel, Orientation, Part, Point, Schematic, Wire};

/// Places a copy of `part` with the next free designator and returns its id.
pub fn place_component(
    schematic: &mut Schematic,
    part: Part,
    position: Point,
    orientation: Orientation,
) -> Id {
    let id = schematic.allocate_id();
    let name = schematic.next_designator(part.kind());
    schematic.components.push(Component {
        id,
        name,
        position,
        orientation,
        part,
    });
    id
}

/// Adds a wire through `points`, or nothing if it has no length.
pub fn add_wire(schematic: &mut Schematic, mut points: Vec<Point>) -> Option<Id> {
    points.dedup();
    if points.len() < 2 {
        return None;
    }
    let id = schematic.allocate_id();
    schematic.wires.push(Wire { id, points });
    Some(id)
}

/// Adds a net label and returns its id.
pub fn add_label(schematic: &mut Schematic, name: String, position: Point) -> Id {
    let id = schematic.allocate_id();
    schematic.labels.push(NetLabel { id, name, position });
    id
}

/// Renames label `id`. Returns `false` if it does not exist.
pub fn rename_label(schematic: &mut Schematic, id: Id, name: String) -> bool {
    match schematic.labels.iter_mut().find(|l| l.id == id) {
        Some(label) => {
            label.name = name;
            true
        }
        None => false,
    }
}

/// Checks a net name typed by the user and returns it trimmed.
pub fn validate_net_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Enter a net name".into());
    }
    if name.chars().count() > 40 {
        return Err("Use at most 40 characters".into());
    }
    if let Some(bad) = name
        .chars()
        .find(|c| c.is_whitespace() || matches!(c, '(' | ')' | ',' | '"' | '\''))
    {
        let shown = if bad.is_whitespace() {
            "spaces".to_string()
        } else {
            format!("\"{bad}\"")
        };
        return Err(format!("Net names cannot contain {shown}"));
    }
    Ok(name.to_string())
}

/// Checks a designator typed by the user and returns it trimmed. Only ASCII
/// letters, digits and `_` are allowed (INT-4), because the designator
/// becomes a probe argument such as `I(R1)`, whose grammar ends a name at
/// `,` or `)`; the netlister enforces the same rule
/// ([`xsim_project::netlist::is_valid_part_name`]).
pub fn validate_designator(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Enter a designator".into());
    }
    if xsim_project::netlist::is_valid_part_name(name) {
        return Ok(name.to_string());
    }
    match name
        .chars()
        .find(|&c| !(c.is_ascii_alphanumeric() || c == '_'))
    {
        Some(c) if c.is_whitespace() => {
            Err("Designators cannot contain spaces: use only letters, digits and _".into())
        }
        Some(c) => Err(format!(
            "Designators cannot contain \"{c}\": use only letters, digits and _"
        )),
        None => Err("Use only letters, digits and _".into()),
    }
}

/// Removes every selected object. Returns `false` if nothing was removed.
pub fn delete_selection(schematic: &mut Schematic, selection: &Selection) -> bool {
    let before = (
        schematic.components.len(),
        schematic.wires.len(),
        schematic.labels.len(),
    );
    schematic
        .components
        .retain(|c| !selection.has_component(c.id));
    schematic.wires.retain(|w| !selection.has_wire(w.id));
    schematic.labels.retain(|l| !selection.has_label(l.id));
    before
        != (
            schematic.components.len(),
            schematic.wires.len(),
            schematic.labels.len(),
        )
}

/// Moves every object of `schematic` by `delta`.
pub fn translate_all(schematic: &mut Schematic, delta: Point) {
    for c in &mut schematic.components {
        c.position = c.position + delta;
    }
    for w in &mut schematic.wires {
        for p in &mut w.points {
            *p = *p + delta;
        }
    }
    for l in &mut schematic.labels {
        l.position = l.position + delta;
    }
}

/// Replaces the designator and parameters of component `id`, validating per
/// [`crate::SchematicEditor::apply_component_edit`]. Returns whether anything
/// changed.
pub fn edit_component(
    schematic: &mut Schematic,
    id: Id,
    name: &str,
    part: Part,
) -> Result<bool, String> {
    let Some(current) = schematic.component(id) else {
        return Err("The part no longer exists".into());
    };
    let name = validate_designator(name)?;
    let name = name.as_str();
    if part.kind() != current.part.kind() {
        return Err(format!(
            "A {} cannot become a {}",
            current.part.kind().label().to_lowercase(),
            part.kind().label().to_lowercase()
        ));
    }
    if schematic
        .components
        .iter()
        .any(|c| c.id != id && c.name == name)
    {
        return Err(format!("{name} is already used by another part"));
    }
    let Some(component) = schematic.component_mut(id) else {
        return Err("The part no longer exists".into());
    };
    let changed = component.name != name || component.part != part;
    component.name = name.to_string();
    component.part = part;
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::{
        add_label, add_wire, delete_selection, edit_component, place_component,
        validate_designator, validate_net_name,
    };
    use crate::model::selection::{ObjectRef, Selection};
    use xsim_project::{Orientation, Part, PartKind, Point, Schematic};

    fn resistor() -> Part {
        Part::Resistor { resistance: 1e3 }
    }

    #[test]
    fn placing_assigns_fresh_ids_and_designators() {
        let mut s = Schematic::default();
        let a = place_component(&mut s, resistor(), Point::new(0, 0), Orientation::default());
        let b = place_component(&mut s, resistor(), Point::new(0, 8), Orientation::default());
        assert_ne!(a, b);
        let names: Vec<_> = s.components.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["R1", "R2"]);
        let g = place_component(
            &mut s,
            Part::Ground,
            Point::new(0, 16),
            Orientation::default(),
        );
        assert_eq!(s.component(g).map(|c| c.name.as_str()), Some("GND1"));
    }

    #[test]
    fn zero_length_wires_are_not_added() {
        let mut s = Schematic::default();
        assert!(add_wire(&mut s, vec![Point::new(2, 2), Point::new(2, 2)]).is_none());
        assert!(add_wire(&mut s, vec![Point::new(2, 2), Point::new(6, 2)]).is_some());
        assert_eq!(s.wires.len(), 1);
    }

    #[test]
    fn deleting_removes_only_the_selection() {
        let mut s = Schematic::default();
        let r = place_component(&mut s, resistor(), Point::new(0, 0), Orientation::default());
        let w = add_wire(&mut s, vec![Point::new(4, 0), Point::new(12, 0)]);
        let l = add_label(&mut s, "out".into(), Point::new(12, 0));
        let selection: Selection = [ObjectRef::Component(r), ObjectRef::Label(l)]
            .into_iter()
            .collect();
        assert!(delete_selection(&mut s, &selection));
        assert!(s.components.is_empty() && s.labels.is_empty());
        assert_eq!(s.wires.first().map(|w| w.id), w);
        assert!(!delete_selection(&mut s, &selection));
    }

    #[test]
    fn component_edits_are_validated() {
        let mut s = Schematic::default();
        let r1 = place_component(&mut s, resistor(), Point::new(0, 0), Orientation::default());
        place_component(&mut s, resistor(), Point::new(0, 8), Orientation::default());
        assert!(edit_component(&mut s, r1, " ", resistor()).is_err());
        assert!(edit_component(&mut s, r1, "R2", resistor()).is_err());
        assert!(edit_component(&mut s, r1, "R1", Part::default_for(PartKind::Ground)).is_err());
        let before = s.clone();
        assert_eq!(edit_component(&mut s, r1, "R1", resistor()), Ok(false));
        assert_eq!(s, before);
        let bigger = Part::Resistor { resistance: 2.2e3 };
        assert_eq!(
            edit_component(&mut s, r1, "Rload", bigger.clone()),
            Ok(true)
        );
        let c = s.component(r1).expect("still there");
        assert_eq!((c.name.as_str(), &c.part), ("Rload", &bigger));
    }

    /// INT-4: a designator becomes a probe argument such as `I(R1)`.
    #[test]
    fn designators_that_would_break_probe_expressions_are_refused_with_a_reason() {
        let mut s = Schematic::default();
        let r1 = place_component(&mut s, resistor(), Point::new(0, 0), Orientation::default());
        let before = s.clone();
        for (name, reason) in [
            ("R(1)", "\"(\""),
            ("R1,2", "\",\""),
            ("R1)", "\")\""),
            ("R 1", "spaces"),
            ("R-1", "\"-\""),
            ("R\u{e4}1", "\"\u{e4}\""),
            ("   ", "Enter a designator"),
        ] {
            let error = edit_component(&mut s, r1, name, resistor()).err();
            let error = error.unwrap_or_else(|| panic!("{name:?} was accepted"));
            assert!(error.contains(reason), "{name:?}: {error}");
            assert_eq!(s, before, "{name:?} changed the schematic");
        }
        for name in ["Rload_2", "_R9", " R7 "] {
            assert_eq!(validate_designator(name), Ok(name.trim().to_string()));
        }
        assert_eq!(edit_component(&mut s, r1, " R_2 ", resistor()), Ok(true));
        assert_eq!(s.component(r1).map(|c| c.name.as_str()), Some("R_2"));
    }

    #[test]
    fn net_names_reject_characters_that_break_expressions() {
        assert_eq!(validate_net_name("  out "), Ok("out".into()));
        assert!(validate_net_name("").is_err());
        assert!(validate_net_name("a b").is_err());
        assert!(validate_net_name("v(out)").is_err());
        assert!(validate_net_name("in,2").is_err());
    }
}
