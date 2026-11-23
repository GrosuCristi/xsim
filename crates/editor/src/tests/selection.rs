//! Selecting, moving, rotating, mirroring, deleting; undo/redo; read-only.

use super::fixtures::two_resistors;
use super::harness::{open, primary, shift};
use crate::{EditorEvent, ObjectRef};
use gpui_kit::{Modifiers, MouseButton, TestAppContext};
use xsim_project::Point;

fn p(x: i32, y: i32) -> Point {
    Point::new(x, y)
}

#[gpui_kit::test]
fn clicking_selects_and_shift_click_toggles(cx: &mut TestAppContext) {
    let mut h = open(cx, two_resistors());
    h.click(0, 0);
    assert_eq!(h.read(|e| e.selection()), vec![ObjectRef::Component(1)]);
    h.click_with(16, 0, shift());
    assert_eq!(
        h.read(|e| e.selection()),
        vec![ObjectRef::Component(1), ObjectRef::Component(2)]
    );
    h.click_with(0, 0, shift());
    assert_eq!(h.read(|e| e.selection()), vec![ObjectRef::Component(2)]);
    h.click(8, 0);
    assert_eq!(h.read(|e| e.selection()), vec![ObjectRef::Wire(3)]);
    h.click(8, 12);
    assert!(
        h.read(|e| e.selection()).is_empty(),
        "clicking empty canvas clears"
    );
    h.keys(&primary("a"));
    assert_eq!(h.read(|e| e.selection()).len(), 3);
    h.keys("escape");
    assert!(h.read(|e| e.selection()).is_empty());
    h.close();
}

#[gpui_kit::test]
fn box_selection_encloses_left_to_right_and_touches_right_to_left(cx: &mut TestAppContext) {
    let mut h = open(cx, two_resistors());
    h.drag((-6, -4), (6, 4));
    assert_eq!(h.read(|e| e.selection()), vec![ObjectRef::Component(1)]);
    h.drag((6, 4), (-6, -4));
    assert_eq!(
        h.read(|e| e.selection()),
        vec![ObjectRef::Component(1), ObjectRef::Wire(3)]
    );
    h.close();
}

#[gpui_kit::test]
fn moving_a_part_stretches_its_wires_orthogonally_in_one_step(cx: &mut TestAppContext) {
    let mut h = open(cx, two_resistors());
    h.drag((0, 0), (0, -4));
    let s = h.schematic();
    assert_eq!(s.components[0].position, p(0, -4));
    let w = &s.wires[0].points;
    assert_eq!(w.first(), Some(&p(4, -4)));
    assert_eq!(w.last(), Some(&p(12, 0)));
    assert!(
        w.windows(2).all(|s| s[0].x == s[1].x || s[0].y == s[1].y),
        "{w:?}"
    );
    h.keys(&primary("z"));
    assert_eq!(
        h.schematic(),
        two_resistors(),
        "the whole drag is one undo step"
    );
    h.close();
}

#[gpui_kit::test]
fn box_selected_groups_move_together(cx: &mut TestAppContext) {
    let mut h = open(cx, two_resistors());
    h.drag((-6, -4), (22, 4));
    assert_eq!(h.read(|e| e.selection()).len(), 3);
    h.drag((16, 0), (16, 6));
    let s = h.schematic();
    assert_eq!(s.components[0].position, p(0, 6));
    assert_eq!(s.components[1].position, p(16, 6));
    assert_eq!(s.wires[0].points, vec![p(4, 6), p(12, 6)]);
    h.close();
}

#[gpui_kit::test]
fn rotating_disconnects_and_mirroring_flips(cx: &mut TestAppContext) {
    let mut h = open(cx, two_resistors());
    h.click(0, 0);
    h.keys("r");
    let s = h.schematic();
    assert_eq!(s.components[0].orientation.rotation, 1);
    assert_eq!(
        s.wires,
        two_resistors().wires,
        "the wire stays where it was"
    );
    h.keys("x");
    let o = h.schematic().components[0].orientation;
    assert!(o.mirror_x);
    h.keys("y");
    assert!(h.schematic().components[0].orientation.mirror_y);
    h.keys(&format!(
        "{} {} {}",
        primary("z"),
        primary("z"),
        primary("z")
    ));
    assert_eq!(h.schematic(), two_resistors());
    h.close();
}

#[gpui_kit::test]
fn delete_and_backspace_remove_the_selection(cx: &mut TestAppContext) {
    let mut h = open(cx, two_resistors());
    h.click(0, 0);
    h.keys("delete");
    assert_eq!(h.schematic().components.len(), 1);
    h.click(8, 0);
    h.keys("backspace");
    assert!(h.schematic().wires.is_empty());
    h.close();
}

#[gpui_kit::test]
fn undo_and_redo_walk_a_chain_of_edits(cx: &mut TestAppContext) {
    let mut h = open(cx, two_resistors());
    assert!(!h.read(|e| e.can_undo()));
    h.click(0, 0);
    h.keys("delete");
    h.click(16, 0);
    h.keys("r");
    let after = h.schematic();
    h.keys(&primary("z"));
    h.keys(&primary("z"));
    assert_eq!(h.schematic(), two_resistors());
    assert!(!h.read(|e| e.can_undo()) && h.read(|e| e.can_redo()));
    h.keys(&primary("shift-z"));
    h.keys(&primary("y"));
    assert_eq!(h.schematic(), after);
    assert!(!h.read(|e| e.can_redo()));
    let changes = h
        .events()
        .into_iter()
        .filter(|e| *e == EditorEvent::Changed)
        .count();
    assert_eq!(changes, 6, "every edit, undo and redo reports a change");
    h.close();
}

#[gpui_kit::test]
fn toolbar_buttons_edit_and_reflect_their_state(cx: &mut TestAppContext) {
    let mut h = open(cx, two_resistors());
    assert!(h.control("toolbar-rotate").is_some());
    h.click(0, 0);
    h.click_control("toolbar-rotate");
    assert_eq!(h.schematic().components[0].orientation.rotation, 1);
    h.click_control("toolbar-undo");
    assert_eq!(h.schematic(), two_resistors());
    h.click_control("toolbar-redo");
    h.click_control("toolbar-delete");
    assert_eq!(h.schematic().components.len(), 1);
    h.close();
}

#[gpui_kit::test]
fn read_only_mode_blocks_every_edit(cx: &mut TestAppContext) {
    let mut h = open(cx, two_resistors());
    h.update(|e, cx| e.set_read_only(true, cx));
    assert!(
        h.control("toolbar-part-r").is_none(),
        "editing tools are hidden"
    );
    assert!(h.control("read-only-badge").is_some());
    h.click(0, 0);
    h.keys(&format!("delete r x {} shift-r w", primary("v")));
    let (a, b) = (h.at(0, 0), h.at(0, 6));
    h.drag_px(a, b, MouseButton::Left, Modifiers::default());
    assert_eq!(h.schematic(), two_resistors());
    assert_eq!(h.read(|e| e.tool()), crate::Tool::Select);
    assert!(
        h.update(|e, cx| e.apply_component_edit(
            1,
            "R9".into(),
            e.schematic().components[0].part.clone(),
            cx
        ))
        .is_err()
    );
    h.close();
}

#[gpui_kit::test]
fn component_edits_are_validated_and_undoable(cx: &mut TestAppContext) {
    let mut h = open(cx, two_resistors());
    let part = xsim_project::Part::Resistor { resistance: 2.2e3 };
    assert!(
        h.update(|e, cx| e.apply_component_edit(1, "R2".into(), part.clone(), cx))
            .is_err()
    );
    assert!(
        h.update(|e, cx| e.apply_component_edit(1, "".into(), part.clone(), cx))
            .is_err()
    );
    assert_eq!(h.schematic(), two_resistors());
    h.update(|e, cx| e.apply_component_edit(1, "Rload".into(), part.clone(), cx))
        .expect("valid edit");
    assert_eq!(h.schematic().components[0].name, "Rload");
    h.keys(&primary("z"));
    assert_eq!(h.schematic(), two_resistors());
    h.close();
}

/// INT-4: designators become probe arguments such as `I(R1)`.
#[gpui_kit::test]
fn designators_that_would_break_probes_are_refused(cx: &mut TestAppContext) {
    let mut h = open(cx, two_resistors());
    let part = xsim_project::Part::Resistor { resistance: 2.2e3 };
    for name in ["R(1)", "R1,2", "R1)", "R 1", "R\u{3a9}"] {
        let result = h.update(|e, cx| e.apply_component_edit(1, name.into(), part.clone(), cx));
        let error = result.expect_err(name);
        assert!(error.contains("letters, digits and _"), "{name}: {error}");
    }
    assert_eq!(h.schematic(), two_resistors(), "nothing changed");
    assert!(!h.read(|e| e.can_undo()), "no undo step");
    h.close();
}

#[gpui_kit::test]
fn arrow_keys_nudge_the_selection_or_scroll_the_view(cx: &mut TestAppContext) {
    let mut h = open(cx, two_resistors());
    h.click(0, 0);
    h.keys("left up");
    let s = h.schematic();
    // One grid unit, the next dot, per key (D22).
    assert_eq!(s.components[0].position, p(-1, -1));
    assert!(
        s.wires[0]
            .points
            .windows(2)
            .all(|s| s[0].x == s[1].x || s[0].y == s[1].y)
    );
    h.keys(&primary("z"));
    assert_eq!(
        h.schematic().components[0].position,
        p(-1, 0),
        "one step per key"
    );
    h.keys("escape escape right");
    assert_eq!(h.read(|e| e.view()).center, [64.0 / 10.0, 0.0]);
    h.close();
}
