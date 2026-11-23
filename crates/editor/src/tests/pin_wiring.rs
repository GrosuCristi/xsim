//! Starting a wire by pressing a pin in the select tool (D20).

use super::fixtures::{rc_low_pass, two_resistors};
use super::harness::{Harness, open, open_with_view, primary, shift};
use super::probing::{probe_context, probes};
use crate::model::hit_test::body_rect;
use crate::{EditorEvent, ObjectRef, ProbeRequest, Tool};
use gpui_kit::{CursorStyle, Modifiers, MouseButton, TestAppContext};
use std::cell::RefCell;
use std::rc::Rc;
use xsim_project::{
    Component, Id, NetLabel, Orientation, Part, PartKind, Point, Schematic, SchematicView,
    connectivity,
};
use xsim_spice_engine::AnalysisKind;

fn p(x: i32, y: i32) -> Point {
    Point::new(x, y)
}

/// The vertices of the wire being drawn, empty when none is.
fn draft(h: &mut Harness) -> Vec<Point> {
    h.read(|e| {
        e.interaction
            .wire
            .as_ref()
            .map(|d| d.points.clone())
            .unwrap_or_default()
    })
}

#[gpui_kit::test]
fn clicking_a_pin_starts_a_wire_that_finishes_on_another_pin(cx: &mut TestAppContext) {
    let mut s = two_resistors();
    s.wires.clear();
    s.components[1].position = p(8, 4);
    let mut h = open(cx, s.clone());
    h.click(0, 0);
    assert_eq!(h.read(|e| e.selection()), vec![ObjectRef::Component(1)]);
    h.click(2, 0);
    assert_eq!(h.read(|e| e.tool()), Tool::Select, "the select tool stays");
    assert_eq!(
        draft(&mut h),
        vec![p(2, 0)],
        "a click on a pin starts a wire"
    );
    assert!(
        h.read(|e| e.selection()).is_empty(),
        "the press neither selects nor keeps the part selected"
    );
    assert_eq!(h.schematic(), s, "nothing is committed or moved yet");
    let at = h.at(6, 4);
    h.hover(at);
    assert_eq!(
        h.read(|e| e.wire_preview()),
        vec![p(2, 0), p(6, 0), p(6, 4)],
        "the rubber band leaves the pin along its axis, as with the wire tool"
    );
    assert_eq!(h.read(|e| e.cursor_style()), CursorStyle::Crosshair);
    assert_eq!(
        h.read(|e| e.hover.object),
        None,
        "no hover highlight while drawing"
    );
    assert!(h.read(|e| e.hint()).contains("Click to add a corner"));
    h.click(6, 4);
    let after = h.schematic();
    assert_eq!(after.wires.len(), 1, "clicking the second pin finishes");
    assert_eq!(after.wires[0].points, vec![p(2, 0), p(6, 0), p(6, 4)]);
    let c = connectivity(&after);
    assert_eq!(c.net_of_pin(1, 1), c.net_of_pin(2, 0));
    assert!(draft(&mut h).is_empty());
    assert_eq!(h.read(|e| e.tool()), Tool::Select);
    h.keys(&primary("z"));
    assert_eq!(h.schematic(), s, "the wire is one undo step");
    h.close();
}

#[gpui_kit::test]
fn dragging_from_a_pin_starts_a_wire_that_keeps_drawing_after_the_release(cx: &mut TestAppContext) {
    let s = two_resistors();
    let mut h = open(cx, s.clone());
    h.drag((-2, 0), (-2, 4));
    assert_eq!(h.schematic(), s, "the part did not move");
    assert_eq!(draft(&mut h), vec![p(-2, 0)]);
    assert_eq!(
        h.read(|e| e.wire_preview()),
        vec![p(-2, 0), p(-2, 4)],
        "the wire follows the mouse"
    );
    h.click(-2, 4);
    assert_eq!(
        draft(&mut h),
        vec![p(-2, 0), p(-2, 4)],
        "a click adds a corner"
    );
    h.click(-5, 4);
    h.keys("enter");
    let after = h.schematic();
    assert_eq!(after.wires.len(), 2);
    assert!(
        after
            .wires
            .iter()
            .any(|w| w.points == vec![p(-2, 0), p(-2, 4), p(-5, 4)]),
        "{:?}",
        after.wires
    );
    assert!(draft(&mut h).is_empty(), "Enter finishes");
    h.keys(&primary("z"));
    assert_eq!(h.schematic(), s);
    h.close();
}

#[gpui_kit::test]
fn escape_cancels_the_wire_and_part_bodies_still_move(cx: &mut TestAppContext) {
    let s = two_resistors();
    let mut h = open(cx, s.clone());
    h.click(2, 0);
    assert_eq!(
        draft(&mut h),
        vec![p(2, 0)],
        "a pin that already has a wire starts a branch"
    );
    h.click(2, 3);
    h.keys("escape");
    assert!(draft(&mut h).is_empty(), "Esc drops the unfinished wire");
    assert_eq!(h.schematic(), s);
    assert_eq!(h.read(|e| e.tool()), Tool::Select);
    h.drag((0, 0), (0, -2));
    let moved = h.schematic();
    assert_eq!(
        moved.components[0].position,
        p(0, -2),
        "the body drags the part"
    );
    assert!(draft(&mut h).is_empty());
    assert_eq!(h.read(|e| e.selection()), vec![ObjectRef::Component(1)]);
    h.close();
}

#[gpui_kit::test]
fn double_clicking_a_pin_opens_the_part_instead_of_wiring(cx: &mut TestAppContext) {
    let mut h = open(cx, two_resistors());
    let opened = Rc::new(RefCell::new(Vec::new()));
    let log = opened.clone();
    h.update(|e, _| e.set_properties_opener(move |_, id, _, _| log.borrow_mut().push(id)));
    let pin = h.at(10, 0);
    h.double_click_px(pin);
    assert_eq!(*opened.borrow(), vec![2]);
    assert!(draft(&mut h).is_empty(), "no wire is left drawing");
    assert_eq!(h.read(|e| e.selection()), vec![ObjectRef::Component(2)]);
    assert_eq!(h.schematic(), two_resistors());
    h.close();
}

#[gpui_kit::test]
fn shift_on_a_pin_toggles_the_part(cx: &mut TestAppContext) {
    let mut h = open(cx, two_resistors());
    h.click_with(-2, 0, shift());
    assert!(draft(&mut h).is_empty());
    assert_eq!(h.read(|e| e.selection()), vec![ObjectRef::Component(1)]);
    h.click_with(10, 0, shift());
    assert_eq!(
        h.read(|e| e.selection()),
        vec![ObjectRef::Component(1), ObjectRef::Component(2)]
    );
    h.close();
}

#[gpui_kit::test]
fn the_cursor_shows_wiring_over_a_pin_only_without_results(cx: &mut TestAppContext) {
    let mut h = open(cx, rc_low_pass());
    let pin = h.at(2, -4);
    h.hover(pin);
    assert_eq!(h.read(|e| e.hover.wire_start), Some(p(2, -4)));
    assert_eq!(h.read(|e| e.cursor_style()), CursorStyle::Crosshair);
    let body = h.at(0, -4);
    h.hover(body);
    assert_eq!(h.read(|e| e.hover.wire_start), None);
    assert_eq!(h.read(|e| e.cursor_style()), CursorStyle::Arrow);
    let context = probe_context(&rc_low_pass(), AnalysisKind::Transient);
    h.update(|e, cx| e.set_probe_context(Some(context), cx));
    h.hover(pin);
    assert_eq!(
        h.read(|e| e.hover.wire_start),
        None,
        "with results shown the pin probes"
    );
    assert!(h.read(|e| e.probe_glyph()).is_some());
    h.update(|e, cx| e.set_probe_context(None, cx));
    h.update(|e, cx| e.set_read_only(true, cx));
    h.hover(body);
    h.hover(pin);
    assert_eq!(h.read(|e| e.hover.wire_start), None, "read-only");
    assert_eq!(h.read(|e| e.cursor_style()), CursorStyle::Arrow);
    h.close();
}

#[gpui_kit::test]
fn with_results_shown_a_pin_probes_and_wires_again_without_them(cx: &mut TestAppContext) {
    let mut h = open(cx, rc_low_pass());
    let context = probe_context(&rc_low_pass(), AnalysisKind::Transient);
    h.update(|e, cx| e.set_probe_context(Some(context), cx));
    h.events();
    h.click(2, -4);
    assert_eq!(
        probes(&mut h),
        vec![ProbeRequest::Voltage { node: "out".into() }]
    );
    assert!(draft(&mut h).is_empty(), "the probe does not start a wire");
    assert_eq!(h.read(|e| e.selection()), vec![ObjectRef::Component(2)]);
    h.drag((2, -4), (-4, -4));
    assert_eq!(
        probes(&mut h),
        vec![ProbeRequest::Differential {
            plus: "out".into(),
            minus: "in".into()
        }],
        "dragging from a pin is still the differential probe"
    );
    assert_eq!(h.schematic(), rc_low_pass());
    h.update(|e, cx| e.set_probe_context(None, cx));
    h.click(2, -4);
    assert_eq!(draft(&mut h), vec![p(2, -4)]);
    assert!(probes(&mut h).is_empty());
    h.close();
}

#[gpui_kit::test]
fn read_only_pins_never_start_wires(cx: &mut TestAppContext) {
    let mut h = open(cx, two_resistors());
    h.update(|e, cx| e.set_read_only(true, cx));
    h.click(-2, 0);
    assert!(draft(&mut h).is_empty());
    h.keys("enter");
    assert_eq!(h.schematic(), two_resistors());
    h.close();
}

/// Records the parts whose properties a double-click opens.
fn record_opened(h: &mut Harness) -> Rc<RefCell<Vec<Id>>> {
    let opened = Rc::new(RefCell::new(Vec::new()));
    let log = opened.clone();
    h.update(|e, _| e.set_properties_opener(move |_, id, _, _| log.borrow_mut().push(id)));
    opened
}

/// Review: the second press of a double-click that finished the wire on a
/// pin opened that part's properties, and on a label renamed it.
#[gpui_kit::test]
fn double_clicking_to_finish_on_a_pin_or_label_opens_nothing(cx: &mut TestAppContext) {
    let mut s = two_resistors();
    s.wires.clear();
    s.components[1].position = p(8, 4);
    let mut h = open(cx, s.clone());
    let opened = record_opened(&mut h);
    h.click(2, 0);
    let pin = h.at(6, 4);
    h.double_click_px(pin);
    let after = h.schematic();
    assert_eq!(after.wires.len(), 1, "the first click finished the wire");
    assert!(draft(&mut h).is_empty(), "the second press starts no wire");
    assert!(
        opened.borrow().is_empty(),
        "no properties: {:?}",
        opened.borrow()
    );
    assert!(h.read(|e| e.selection()).is_empty());
    h.keys(&primary("z"));
    assert_eq!(h.schematic(), s, "one undo step");
    // A fresh double-click on the pin still opens the part.
    h.double_click_px(pin);
    assert_eq!(*opened.borrow(), vec![2]);
    h.close();

    let id = s.allocate_id();
    s.labels.push(NetLabel {
        id,
        name: "out".into(),
        position: p(2, 6),
    });
    let mut h = open(cx, s);
    h.click(2, 0);
    let label = h.at(2, 6);
    h.double_click_px(label);
    assert!(draft(&mut h).is_empty());
    let wires: Vec<Vec<Point>> = h.schematic().wires.into_iter().map(|w| w.points).collect();
    assert_eq!(wires, vec![vec![p(2, 0), p(2, 6)]], "finished on the label");
    assert!(
        h.read(|e| e.label_editor.is_none()),
        "the label is not renamed"
    );
    h.close();
}

#[gpui_kit::test]
fn a_pin_click_then_enter_commits_nothing(cx: &mut TestAppContext) {
    let mut h = open(cx, two_resistors());
    h.events();
    h.click(-2, 0);
    h.keys("enter");
    assert!(draft(&mut h).is_empty(), "Enter ends the draft");
    assert_eq!(h.schematic(), two_resistors());
    assert!(!h.read(|e| e.can_undo()), "no empty undo step");
    assert!(!h.events().contains(&EditorEvent::Changed));
    h.close();
}

/// Review: the wiring cursor was cached on mouse moves, so it showed the
/// arrow over a pin after finishing a wire there, after Esc and after the
/// results were cleared, until the mouse moved again.
#[gpui_kit::test]
fn the_cursor_is_right_without_a_mouse_move(cx: &mut TestAppContext) {
    let mut s = two_resistors();
    s.wires.clear();
    s.components[1].position = p(8, 4);
    let mut h = open(cx, s);
    h.click(2, 0);
    h.click(6, 4);
    assert_eq!(h.schematic().wires.len(), 1);
    assert_eq!(
        h.read(|e| e.cursor_style()),
        CursorStyle::Crosshair,
        "after finishing on a pin"
    );
    h.click(-2, 0);
    h.keys("escape");
    assert!(draft(&mut h).is_empty());
    assert_eq!(
        h.read(|e| e.cursor_style()),
        CursorStyle::Crosshair,
        "after Esc"
    );
    h.close();

    let mut h = open(cx, rc_low_pass());
    let context = probe_context(&rc_low_pass(), AnalysisKind::Transient);
    h.update(|e, cx| e.set_probe_context(Some(context), cx));
    let pin = h.at(2, -4);
    h.hover(pin);
    h.update(|e, cx| e.set_probe_context(None, cx));
    assert_eq!(
        h.read(|e| e.cursor_style()),
        CursorStyle::Crosshair,
        "after the results are cleared"
    );
    h.space(true);
    assert_eq!(h.read(|e| e.cursor_style()), CursorStyle::OpenHand);
    h.space(false);
    h.update(|e, cx| e.set_read_only(true, cx));
    assert_eq!(
        h.read(|e| e.cursor_style()),
        CursorStyle::Arrow,
        "read-only"
    );
    h.close();
}

fn single(kind: PartKind) -> Schematic {
    let mut s = Schematic::default();
    let id = s.allocate_id();
    s.components.push(Component {
        id,
        name: "X1".into(),
        position: p(0, 0),
        orientation: Orientation::default(),
        part: Part::default_for(kind),
    });
    s
}

/// Review: zoomed out, a press on a ground symbol's top bar started a wire
/// from its pin, so it could not be moved by its body.
#[gpui_kit::test]
fn every_part_moves_by_its_body_and_wires_from_its_pins_at_every_zoom(cx: &mut TestAppContext) {
    // Hit testing depends only on a symbol's geometry: one part of each.
    let shape = |kind| {
        let g = xsim_project::symbols::geometry(kind);
        (g.pins, g.body_min, g.body_max)
    };
    let mut kinds: Vec<PartKind> = Vec::new();
    for kind in PartKind::ALL {
        if !kinds.iter().any(|&k| shape(k) == shape(kind)) {
            kinds.push(kind);
        }
    }
    assert!(kinds.contains(&PartKind::Ground), "{kinds:?}");
    let mut failures = Vec::new();
    for zoom in [3.0, 10.0, 20.0, 120.0] {
        let view = SchematicView {
            center: [0.0, 0.0],
            zoom,
            user_set: true,
        };
        for &kind in &kinds {
            let s = single(kind);
            let mut h = open_with_view(cx, s.clone(), view);
            let c = body_rect(&s.components[0]).center();
            let (a, b) = (h.at_f(c.x, c.y), h.at_f(c.x, c.y + 3.0));
            h.drag_px(a, b, MouseButton::Left, Modifiers::default());
            if h.schematic().components[0].position != p(0, 3) || !draft(&mut h).is_empty() {
                failures.push(format!("{kind:?} at zoom {zoom}: the body did not move"));
            }
            h.keys("escape");
            h.keys(&primary("z"));
            for pin in s.components[0].pins() {
                let at = h.at_f(pin.x as f32, pin.y as f32);
                h.click_px(at, Modifiers::default());
                if draft(&mut h) != vec![pin] {
                    failures.push(format!(
                        "{kind:?} at zoom {zoom}: pin {pin:?} starts no wire"
                    ));
                }
                h.keys("escape");
            }
            if h.schematic() != s {
                failures.push(format!("{kind:?} at zoom {zoom}: the drawing changed"));
            }
            h.close();
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[gpui_kit::test]
fn panning_tools_undo_and_read_only_leave_no_stuck_draft(cx: &mut TestAppContext) {
    let s = two_resistors();
    let mut h = open(cx, s.clone());
    h.click(-2, 0);
    h.space(true);
    h.drag((5, 5), (6, 6));
    h.space(false);
    assert_eq!(
        draft(&mut h),
        vec![p(-2, 0)],
        "Space-drag pans and keeps drawing"
    );
    let (a, b) = (h.at(5, 5), h.at(4, 4));
    h.drag_px(a, b, MouseButton::Middle, Modifiers::default());
    assert_eq!(
        draft(&mut h),
        vec![p(-2, 0)],
        "middle-drag pans and keeps drawing"
    );
    h.keys(&primary("a"));
    assert!(draft(&mut h).is_empty(), "select all drops the draft");
    for key in ["w", "s", "n"] {
        h.keys("escape escape");
        h.click(-2, 0);
        assert_eq!(draft(&mut h), vec![p(-2, 0)]);
        h.keys(key);
        assert!(draft(&mut h).is_empty(), "{key} drops the draft");
    }
    h.keys("escape escape");
    assert_eq!(h.schematic(), s, "nothing was committed");
    // Undo drops a draft once there is something to undo.
    h.drag((0, 0), (0, 2));
    h.click(-2, 2);
    h.keys(&primary("z"));
    assert!(draft(&mut h).is_empty(), "undo");
    assert_eq!(h.schematic(), s);
    h.click(-2, 0);
    h.update(|e, cx| e.set_read_only(true, cx));
    assert!(draft(&mut h).is_empty(), "read-only drops the draft");
    h.keys("enter");
    assert_eq!(h.schematic(), s);
    h.close();
}

#[gpui_kit::test]
fn a_wire_being_drawn_when_results_arrive_is_finished_as_usual(cx: &mut TestAppContext) {
    let mut h = open(cx, rc_low_pass());
    h.click(-2, -4);
    h.click(-2, -8);
    let context = probe_context(&rc_low_pass(), AnalysisKind::Transient);
    h.update(|e, cx| e.set_probe_context(Some(context), cx));
    h.events();
    h.click(2, -4);
    assert!(draft(&mut h).is_empty());
    assert!(
        probes(&mut h).is_empty(),
        "the click finishes the wire, it does not probe"
    );
    let s = h.schematic();
    assert!(
        s.wires
            .iter()
            .any(|w| w.points.first() == Some(&p(-2, -4)) && w.points.last() == Some(&p(2, -4))),
        "{:?}",
        s.wires
    );
    h.close();
}

#[gpui_kit::test]
fn zoomed_far_out_only_a_press_right_on_the_pin_wires(cx: &mut TestAppContext) {
    let view = SchematicView {
        center: [0.0, 0.0],
        zoom: 5.0,
        user_set: true,
    };
    let mut h = open_with_view(cx, two_resistors(), view);
    assert!(
        h.read(|e| e.hit_tolerance()) > 1.0,
        "the pin hit area covers most of the part"
    );
    let (near, up) = (h.at_f(1.0, 0.0), h.at_f(1.0, -3.0));
    h.drag_px(near, up, gpui_kit::MouseButton::Left, Modifiers::default());
    assert!(draft(&mut h).is_empty());
    assert_eq!(
        h.schematic().components[0].position,
        p(0, -3),
        "a press a unit from the pin moves the part"
    );
    let on_pin = h.at_f(2.3, -3.1);
    h.click_px(on_pin, Modifiers::default());
    assert_eq!(draft(&mut h), vec![p(2, -3)]);
    h.close();
}
