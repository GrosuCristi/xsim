//! Net labels and double-click editing.

use super::fixtures::{rc_low_pass, two_resistors};
use super::harness::open;
use crate::{ObjectRef, Tool};
use gpui_kit::TestAppContext;
use std::cell::RefCell;
use std::rc::Rc;
use xsim_project::{Point, connectivity};

#[gpui_kit::test]
fn the_label_tool_names_the_net_it_is_placed_on(cx: &mut TestAppContext) {
    let mut h = open(cx, two_resistors());
    h.keys("n");
    assert_eq!(h.read(|e| e.tool()), Tool::Label);
    h.click(8, 0);
    assert!(
        h.control("label-editor").is_some(),
        "the inline field opens"
    );
    h.type_text("vout");
    h.keys("enter");
    let s = h.schematic();
    assert_eq!(s.labels.len(), 1);
    assert_eq!(
        (s.labels[0].name.as_str(), s.labels[0].position),
        ("vout", Point::new(8, 0))
    );
    assert!(
        s.wires[0].points.contains(&Point::new(8, 0)),
        "the wire got a vertex"
    );
    let c = connectivity(&s);
    let net = c.net_of_pin(1, 1).expect("R1 is wired");
    assert_eq!(c.nets[net].name, "vout");
    assert!(h.control("label-editor").is_none());
    h.close();
}

#[gpui_kit::test]
fn typing_a_name_never_triggers_editor_shortcuts(cx: &mut TestAppContext) {
    let mut h = open(cx, two_resistors());
    h.keys("n");
    h.click(8, 0);
    h.update(|e, cx| e.set_selection([ObjectRef::Component(1)], cx));
    h.type_text("rxyw");
    h.keys("backspace");
    h.keys("enter");
    let s = h.schematic();
    assert_eq!(s.labels[0].name, "rxy");
    assert_eq!(s.components.len(), 2, "nothing was deleted");
    assert_eq!(
        s.components[0].orientation,
        two_resistors().components[0].orientation
    );
    assert_eq!(h.read(|e| e.tool()), Tool::Label, "w did not switch tools");
    h.close();
}

#[gpui_kit::test]
fn invalid_names_are_explained_and_escape_cancels(cx: &mut TestAppContext) {
    let mut h = open(cx, two_resistors());
    h.keys("n");
    h.click(8, 0);
    h.keys("enter");
    assert!(
        h.read(|e| e.label_editor.as_ref().and_then(|f| f.error.clone()))
            .is_some()
    );
    assert!(h.schematic().labels.is_empty());
    h.keys("escape");
    assert!(h.control("label-editor").is_none());
    assert!(h.schematic().labels.is_empty());
    h.close();
}

#[gpui_kit::test]
fn double_clicking_a_label_renames_it(cx: &mut TestAppContext) {
    let mut h = open(cx, rc_low_pass());
    let p = h.at_f(15.0, -8.0);
    h.double_click_px(p);
    assert!(h.control("label-editor").is_some());
    h.keys(&super::harness::primary("a"));
    h.type_text("filtered");
    h.keys("enter");
    let names: Vec<String> = h
        .schematic()
        .labels
        .iter()
        .map(|l| l.name.clone())
        .collect();
    assert_eq!(names, ["in", "filtered"]);
    h.close();
}

#[gpui_kit::test]
fn double_clicking_a_part_opens_its_properties(cx: &mut TestAppContext) {
    let mut h = open(cx, two_resistors());
    let opened = Rc::new(RefCell::new(Vec::new()));
    let log = opened.clone();
    h.update(|e, _| e.set_properties_opener(move |_, id, _, _| log.borrow_mut().push(id)));
    let p = h.at(16, 0);
    h.double_click_px(p);
    assert_eq!(*opened.borrow(), vec![2]);
    assert_eq!(h.read(|e| e.selection()), vec![ObjectRef::Component(2)]);
    h.update(|e, cx| e.set_read_only(true, cx));
    h.double_click_px(p);
    assert_eq!(
        opened.borrow().len(),
        1,
        "read-only parts do not open for editing"
    );
    h.close();
}
