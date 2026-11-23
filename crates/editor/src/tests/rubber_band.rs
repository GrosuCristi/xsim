//! Parts touching pin to pin stay connected when one moves away (D20).

use super::fixtures::{rc_low_pass, touching_chain, touching_pair};
use super::harness::{Harness, open, primary, shift};
use super::probing::probe_context;
use crate::ui::gesture::Gesture;
use gpui_kit::{Modifiers, MouseButton, TestAppContext};
use xsim_project::{Id, Point, Schematic, connectivity};
use xsim_spice_engine::AnalysisKind;

fn p(x: i32, y: i32) -> Point {
    Point::new(x, y)
}

fn wires(s: &Schematic) -> Vec<Vec<Point>> {
    s.wires.iter().map(|w| w.points.clone()).collect()
}

/// Whether pin `a.1` of part `a.0` and pin `b.1` of part `b.0` share a net.
fn connected(s: &Schematic, a: (Id, usize), b: (Id, usize)) -> bool {
    let c = connectivity(s);
    c.net_of_pin(a.0, a.1).is_some() && c.net_of_pin(a.0, a.1) == c.net_of_pin(b.0, b.1)
}

fn assert_orthogonal(s: &Schematic) {
    for w in &s.wires {
        assert!(
            w.points
                .windows(2)
                .all(|g| g[0].x == g[1].x || g[0].y == g[1].y),
            "diagonal in {:?}",
            w.points
        );
    }
}

fn undo(h: &mut Harness) {
    h.keys(&primary("z"));
}

#[gpui_kit::test]
fn dragging_a_part_off_a_touching_pin_inserts_a_wire_in_one_undo_step(cx: &mut TestAppContext) {
    let mut h = open(cx, touching_pair());
    h.drag((8, 0), (14, 0));
    let s = h.schematic();
    assert_eq!(s.components[1].position, p(14, 0));
    assert_eq!(wires(&s), vec![vec![p(4, 0), p(10, 0)]]);
    assert!(connected(&s, (1, 1), (2, 0)), "the connection follows");
    assert!(!connected(&s, (1, 0), (2, 1)));
    undo(&mut h);
    assert_eq!(
        h.schematic(),
        touching_pair(),
        "move and wire undo together"
    );
    h.drag((8, 0), (8, 6));
    let s = h.schematic();
    assert_eq!(s.components[1].position, p(8, 6));
    assert_eq!(wires(&s), vec![vec![p(4, 0), p(4, 6)]]);
    assert!(connected(&s, (1, 1), (2, 0)));
    undo(&mut h);
    assert_eq!(h.schematic(), touching_pair());
    h.close();
}

#[gpui_kit::test]
fn the_inserted_wire_shows_while_dragging_and_escape_drops_it(cx: &mut TestAppContext) {
    let mut h = open(cx, touching_pair());
    let (a, b) = (h.at(8, 0), h.at(16, 6));
    let none = Modifiers::default();
    h.cx.simulate_mouse_move(a, None, none);
    h.cx.simulate_mouse_down(a, MouseButton::Left, none);
    h.cx.simulate_mouse_move(b, Some(MouseButton::Left), none);
    let preview = h.read(|e| match &e.interaction.gesture {
        Gesture::Moving(drag) => Some(wires(&drag.preview)),
        _ => None,
    });
    assert_eq!(
        preview,
        Some(vec![vec![p(4, 0), p(8, 0), p(8, 6), p(12, 6)]]),
        "a Z leaves both pins along their axis"
    );
    h.keys("escape");
    h.cx.simulate_mouse_up(b, MouseButton::Left, none);
    assert_eq!(h.schematic(), touching_pair(), "Esc aborts the move");
    h.close();
}

#[gpui_kit::test]
fn a_part_touching_two_others_keeps_both_connections(cx: &mut TestAppContext) {
    let mut h = open(cx, touching_chain());
    h.drag((8, 0), (8, -4));
    let s = h.schematic();
    assert_eq!(
        wires(&s),
        vec![vec![p(4, 0), p(4, -4)], vec![p(12, 0), p(12, -4)]]
    );
    assert!(connected(&s, (1, 1), (2, 0)));
    assert!(connected(&s, (2, 1), (3, 0)));
    assert!(!connected(&s, (1, 1), (3, 0)), "R2 still sits between");
    undo(&mut h);
    assert_eq!(h.schematic(), touching_chain());
    h.close();
}

#[gpui_kit::test]
fn a_moved_selection_wires_only_to_partners_left_behind(cx: &mut TestAppContext) {
    let mut h = open(cx, touching_chain());
    h.click(0, 0);
    h.click_with(8, 0, shift());
    h.drag((8, 0), (8, 4));
    let s = h.schematic();
    assert_eq!(s.components[0].position, p(0, 4));
    assert_eq!(s.components[1].position, p(8, 4));
    assert_eq!(wires(&s), vec![vec![p(12, 0), p(12, 4)]]);
    assert!(connected(&s, (1, 1), (2, 0)), "R1 and R2 still touch");
    assert!(connected(&s, (2, 1), (3, 0)));
    undo(&mut h);
    assert_eq!(h.schematic(), touching_chain());
    h.close();
}

#[gpui_kit::test]
fn arrow_key_nudges_stretch_the_inserted_wire(cx: &mut TestAppContext) {
    let mut h = open(cx, touching_pair());
    h.click(8, 0);
    // Each key moves R2 one grid unit, to the next dot (D22).
    h.keys("right");
    assert_eq!(wires(&h.schematic()), vec![vec![p(4, 0), p(5, 0)]]);
    h.keys("right down");
    let s = h.schematic();
    assert_eq!(s.components[1].position, p(10, 1));
    assert_eq!(s.wires.len(), 1, "{:?}", s.wires);
    assert_orthogonal(&s);
    assert!(connected(&s, (1, 1), (2, 0)));
    undo(&mut h);
    undo(&mut h);
    assert_eq!(wires(&h.schematic()), vec![vec![p(4, 0), p(5, 0)]]);
    h.keys("left");
    let back = h.schematic();
    assert!(back.wires.is_empty(), "back on the pin: {:?}", back.wires);
    assert_eq!(back.components, touching_pair().components);
    assert!(connected(&back, (1, 1), (2, 0)));
    undo(&mut h);
    undo(&mut h);
    assert_eq!(h.schematic(), touching_pair(), "one undo step per nudge");
    h.close();
}

#[gpui_kit::test]
fn dragging_out_and_back_records_no_undo_step(cx: &mut TestAppContext) {
    let mut h = open(cx, touching_pair());
    h.events();
    let (a, b) = (h.at(8, 0), h.at(14, 4));
    let none = Modifiers::default();
    h.cx.simulate_mouse_move(a, None, none);
    h.cx.simulate_mouse_down(a, MouseButton::Left, none);
    h.cx.simulate_mouse_move(b, Some(MouseButton::Left), none);
    h.cx.simulate_mouse_move(a, Some(MouseButton::Left), none);
    h.cx.simulate_mouse_up(a, MouseButton::Left, none);
    assert_eq!(h.schematic(), touching_pair());
    assert!(!h.read(|e| e.can_undo()));
    assert!(h.events().is_empty(), "nothing changed");
    h.close();
}

#[gpui_kit::test]
fn redo_restores_the_move_and_its_wire(cx: &mut TestAppContext) {
    let mut h = open(cx, touching_pair());
    h.drag((8, 0), (16, 6));
    let moved = h.schematic();
    assert_eq!(moved.wires.len(), 1);
    undo(&mut h);
    assert_eq!(h.schematic(), touching_pair());
    h.keys(&primary("shift-z"));
    assert_eq!(h.schematic(), moved);
    h.close();
}

#[gpui_kit::test]
fn with_results_shown_a_body_drag_still_moves_and_wires(cx: &mut TestAppContext) {
    let mut h = open(cx, touching_pair());
    let context = probe_context(&rc_low_pass(), AnalysisKind::Transient);
    h.update(|e, cx| e.set_probe_context(Some(context), cx));
    h.drag((8, 0), (8, 6));
    let s = h.schematic();
    assert_eq!(s.components[1].position, p(8, 6));
    assert_eq!(wires(&s), vec![vec![p(4, 0), p(4, 6)]]);
    assert!(connected(&s, (1, 1), (2, 0)));
    h.close();
}

/// Review: dragging a part past its partner inserted a wire over both
/// parts' far pins, shorting each.
#[gpui_kit::test]
fn dragging_a_part_past_its_partner_shorts_neither(cx: &mut TestAppContext) {
    let mut h = open(cx, touching_pair());
    h.drag((8, 0), (-8, 6));
    let s = h.schematic();
    assert_eq!(s.components[1].position, p(-8, 6));
    assert_orthogonal(&s);
    assert!(connected(&s, (1, 1), (2, 0)), "{:?}", s.wires);
    assert!(!connected(&s, (1, 0), (1, 1)), "R1 is not shorted");
    assert!(!connected(&s, (2, 0), (2, 1)), "R2 is not shorted");
    undo(&mut h);
    h.drag((8, 0), (-8, 0));
    let s = h.schematic();
    assert!(connected(&s, (1, 1), (2, 0)), "{:?}", s.wires);
    assert!(!connected(&s, (1, 0), (1, 1)), "R1 is not shorted");
    h.close();
}

#[gpui_kit::test]
fn pin_wiring_and_rubber_bands_work_at_every_ui_scale(cx: &mut TestAppContext) {
    let mut h = open(cx, touching_pair());
    for scale in [0.75, 2.0] {
        super::grid::set_ui_scale(&mut h, scale);
        h.click(4, 0);
        let draft = h.read(|e| e.interaction.wire.as_ref().map(|d| d.points.clone()));
        assert_eq!(
            draft,
            Some(vec![p(4, 0)]),
            "UI scale {scale}: the pin wires"
        );
        h.keys("escape");
        h.drag((8, 0), (8, 6));
        let s = h.schematic();
        assert_eq!(wires(&s), vec![vec![p(4, 0), p(4, 6)]], "UI scale {scale}");
        assert!(connected(&s, (1, 1), (2, 0)), "UI scale {scale}");
        undo(&mut h);
        assert_eq!(h.schematic(), touching_pair(), "UI scale {scale}");
    }
    h.close();
}

#[gpui_kit::test]
fn rotating_still_disconnects_touching_pins(cx: &mut TestAppContext) {
    let mut h = open(cx, touching_pair());
    h.click(8, 0);
    h.keys("r");
    let s = h.schematic();
    assert_eq!(s.components[1].orientation.rotation, 1);
    assert!(s.wires.is_empty(), "no wire follows a rotation (Q60)");
    assert!(!connected(&s, (1, 1), (2, 0)));
    h.close();
}
