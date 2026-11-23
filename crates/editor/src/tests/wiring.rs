//! Drawing wires: orthogonal bends, any-angle segments, T-junctions.

use super::fixtures::two_resistors;
use super::harness::{alt, open};
use crate::Tool;
use gpui_kit::TestAppContext;
use xsim_project::{Point, Schematic, connectivity};

fn points(s: &Schematic, index: usize) -> Vec<(i32, i32)> {
    s.wires[index].points.iter().map(|p| (p.x, p.y)).collect()
}

#[gpui_kit::test]
fn wiring_two_pins_bends_once_and_connects_them(cx: &mut TestAppContext) {
    let mut s = two_resistors();
    s.wires.clear();
    s.components[1].position = Point::new(8, 4);
    let mut h = open(cx, s);
    h.keys("w");
    assert_eq!(h.read(|e| e.tool()), Tool::Wire);
    h.click(2, 0);
    let p = h.at(6, 4);
    h.hover(p);
    assert_eq!(
        h.read(|e| e.wire_preview()),
        vec![Point::new(2, 0), Point::new(6, 0), Point::new(6, 4)],
        "the wire leaves the pin along its axis"
    );
    h.click(6, 4);
    let s = h.schematic();
    assert_eq!(
        s.wires.len(),
        1,
        "clicking the second pin finishes the wire"
    );
    assert_eq!(points(&s, 0), vec![(2, 0), (6, 0), (6, 4)]);
    let c = connectivity(&s);
    assert_eq!(c.net_of_pin(1, 1), c.net_of_pin(2, 0));
    assert_eq!(
        h.read(|e| e.tool()),
        Tool::Wire,
        "the wire tool stays active"
    );
    h.close();
}

#[gpui_kit::test]
fn corners_are_added_by_clicking_and_enter_finishes(cx: &mut TestAppContext) {
    let mut h = open(cx, Schematic::default());
    h.keys("w");
    h.click(0, 0);
    h.click(6, 0);
    h.click(6, 5);
    assert!(
        h.schematic().wires.is_empty(),
        "nothing is committed while drawing"
    );
    h.keys("enter");
    let s = h.schematic();
    assert_eq!(points(&s, 0), vec![(0, 0), (6, 0), (6, 5)]);
    h.close();
}

#[gpui_kit::test]
fn alt_draws_any_angle(cx: &mut TestAppContext) {
    let mut h = open(cx, Schematic::default());
    h.keys("w");
    h.click(0, 0);
    h.click_with(4, 3, alt());
    h.keys("enter");
    assert_eq!(points(&h.schematic(), 0), vec![(0, 0), (4, 3)]);
    h.close();
}

#[gpui_kit::test]
fn ending_on_a_wire_forms_a_t_junction_with_a_dot(cx: &mut TestAppContext) {
    let mut h = open(cx, Schematic::default());
    h.keys("w");
    h.click(0, 0);
    h.click(8, 0);
    h.keys("enter");
    h.click(4, 4);
    h.click(4, 0);
    let s = h.schematic();
    assert_eq!(
        s.wires.len(),
        2,
        "landing on the first wire finishes the second"
    );
    assert_eq!(connectivity(&s).junctions, vec![Point::new(4, 0)]);
    h.close();
}

#[gpui_kit::test]
fn crossing_wires_do_not_connect(cx: &mut TestAppContext) {
    let mut s = two_resistors();
    s.wires.clear();
    s.components[0].position = Point::new(0, -6);
    s.components[1].position = Point::new(-6, 0);
    let mut h = open(cx, s);
    h.keys("w");
    h.click(2, -6);
    h.click(2, 6);
    h.keys("enter");
    h.click(-4, 0);
    h.click(8, 0);
    h.keys("enter");
    let s = h.schematic();
    assert_eq!(s.wires.len(), 2);
    let c = connectivity(&s);
    assert!(c.junctions.is_empty(), "no dot where wires merely cross");
    assert_ne!(c.net_of_pin(1, 1), c.net_of_pin(2, 1));
    h.close();
}

#[gpui_kit::test]
fn double_click_finishes_and_escape_cancels(cx: &mut TestAppContext) {
    let mut h = open(cx, Schematic::default());
    h.keys("w");
    h.click(0, 0);
    let p = h.at(0, 6);
    h.double_click_px(p);
    assert_eq!(points(&h.schematic(), 0), vec![(0, 0), (0, 6)]);
    h.click(3, 0);
    h.click(3, 3);
    h.keys("escape");
    assert_eq!(
        h.schematic().wires.len(),
        1,
        "Esc drops the unfinished wire"
    );
    assert_eq!(h.read(|e| e.tool()), Tool::Wire);
    h.keys("escape");
    assert_eq!(h.read(|e| e.tool()), Tool::Select);
    h.close();
}

#[gpui_kit::test]
fn alt_pressed_in_place_switches_the_rubber_band_to_any_angle(cx: &mut TestAppContext) {
    let mut h = open(cx, Schematic::default());
    h.keys("w");
    h.click(0, 0);
    let p = h.at(4, 3);
    h.hover(p);
    assert_eq!(
        h.read(|e| e.wire_preview()).len(),
        3,
        "orthogonal by default"
    );
    h.cx.simulate_modifiers_change(alt());
    assert_eq!(
        h.read(|e| e.wire_preview()),
        vec![Point::new(0, 0), Point::new(4, 3)]
    );
    h.cx.simulate_click(p, alt());
    h.keys("enter");
    assert_eq!(points(&h.schematic(), 0), vec![(0, 0), (4, 3)]);
    h.close();
}
