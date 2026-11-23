//! Copy, cut, paste and duplicate through the system clipboard (Q58).

use super::fixtures::{rc_low_pass, two_resistors};
use super::harness::{open, primary};
use gpui_kit::{ClipboardItem, TestAppContext};
use std::collections::BTreeSet;
use xsim_project::{Point, Schematic};

fn ids(s: &Schematic) -> BTreeSet<u64> {
    s.components
        .iter()
        .map(|c| c.id)
        .chain(s.wires.iter().map(|w| w.id))
        .chain(s.labels.iter().map(|l| l.id))
        .collect()
}

#[gpui_kit::test]
fn copy_and_paste_between_two_editors(cx: &mut TestAppContext) {
    let mut source = open(cx, rc_low_pass());
    let mut target = open(cx, two_resistors());
    source.keys(&primary("a"));
    source.keys(&primary("c"));
    target.focus();
    let p = target.at(0, 24);
    target.hover(p);
    target.keys(&primary("v"));
    let s = target.schematic();
    assert_eq!(s.components.len(), 6);
    assert_eq!(s.wires.len(), 4);
    assert_eq!(s.labels.len(), 2);
    assert_eq!(ids(&s).len(), 12, "pasted objects get fresh ids");
    let names: BTreeSet<&str> = s.components.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names.len(), 6, "designators stay unique: {names:?}");
    assert!(
        names.contains("R3"),
        "the pasted R1 was renumbered: {names:?}"
    );
    assert!(names.contains("V1") && names.contains("C1"));
    let pasted_r = s
        .components
        .iter()
        .find(|c| c.name == "R3")
        .expect("pasted resistor");
    assert_eq!(
        pasted_r.position,
        Point::new(0, 24 - 8),
        "centred under the mouse"
    );
    assert_eq!(
        target.read(|e| e.selection()).len(),
        9,
        "the pasted objects are selected"
    );
    assert_eq!(
        source.schematic(),
        rc_low_pass(),
        "copying does not change the source"
    );
    source.close();
    target.close();
}

#[gpui_kit::test]
fn cut_removes_and_paste_restores_into_the_same_editor(cx: &mut TestAppContext) {
    let mut h = open(cx, two_resistors());
    h.click(0, 0);
    h.keys(&primary("x"));
    assert_eq!(h.schematic().components.len(), 1);
    let p = h.at(0, 0);
    h.hover(p);
    h.keys(&primary("v"));
    let s = h.schematic();
    assert_eq!(s.components.len(), 2);
    let r1 = s
        .components
        .iter()
        .find(|c| c.name == "R1")
        .expect("R1 back");
    assert_eq!(r1.position, Point::new(0, 0));
    h.close();
}

#[gpui_kit::test]
fn duplicate_offsets_the_copy(cx: &mut TestAppContext) {
    let mut h = open(cx, two_resistors());
    h.click(0, 0);
    h.keys(&primary("d"));
    let s = h.schematic();
    assert_eq!(s.components.len(), 3);
    let copy = &s.components[2];
    assert_eq!(
        (copy.name.as_str(), copy.position),
        ("R3", Point::new(4, 4))
    );
    h.close();
}

#[gpui_kit::test]
fn foreign_clipboard_text_is_ignored(cx: &mut TestAppContext) {
    let mut h = open(cx, two_resistors());
    h.cx.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string("hello".into())));
    h.keys(&primary("v"));
    assert_eq!(h.schematic(), two_resistors());
    assert!(!h.read(|e| e.can_undo()));
    h.close();
}
