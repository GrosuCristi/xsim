//! Dragging single wire segments to realign them (D14), KiCad/LTspice
//! style: the pressed segment moves perpendicular to itself in whole grid
//! units, its neighbours stretch, held ends get a bend, T-junctions stay
//! connected, and the drag is one undo step.

use super::fixtures::rc_low_pass;
use super::harness::{Harness, open, open_with_view, primary, shift};
use super::probing::{probe_context, probes};
use crate::{EditorEvent, ObjectRef, ProbeRequest};
use gpui_kit::{CursorStyle, Modifiers, MouseButton, TestAppContext};
use xsim_project::{
    Component, Orientation, Part, PartKind, Point, Schematic, SchematicView, Wire, connectivity,
};
use xsim_spice_engine::AnalysisKind;

fn p(x: i32, y: i32) -> Point {
    Point::new(x, y)
}

fn resistor(id: u64, name: &str, at: Point, upright: bool) -> Component {
    Component {
        id,
        name: name.into(),
        position: at,
        orientation: Orientation {
            rotation: u8::from(upright),
            ..Orientation::default()
        },
        part: Part::default_for(PartKind::Resistor),
    }
}

fn wire(id: u64, points: &[Point]) -> Wire {
    Wire {
        id,
        points: points.to_vec(),
    }
}

fn points(s: &Schematic, id: u64) -> Vec<Point> {
    s.wire(id).map(|w| w.points.clone()).unwrap_or_default()
}

fn assert_orthogonal(s: &Schematic) {
    for w in &s.wires {
        for seg in w.points.windows(2) {
            assert!(
                seg[0].x == seg[1].x || seg[0].y == seg[1].y,
                "diagonal segment in {:?}",
                w.points
            );
        }
    }
}

/// Whether the grid points `a` and `b` are on one net.
fn connected(s: &Schematic, a: Point, b: Point) -> bool {
    let c = connectivity(s);
    matches!((c.net_at(a), c.net_at(b)), (Some(x), Some(y)) if x == y)
}

fn changes(h: &mut Harness) -> usize {
    h.events()
        .into_iter()
        .filter(|e| *e == EditorEvent::Changed)
        .count()
}

/// R1 (pins (-4,0), (4,0)) and R2 at (24,0) (pins (20,0), (28,0)), joined
/// by a U-shaped wire whose bottom runs along y = 8.
pub(super) fn u_route() -> Schematic {
    Schematic {
        components: vec![
            resistor(1, "R1", p(0, 0), false),
            resistor(2, "R2", p(24, 0), false),
        ],
        wires: vec![wire(
            3,
            &[p(4, 0), p(8, 0), p(8, 8), p(16, 8), p(16, 0), p(20, 0)],
        )],
        labels: Vec::new(),
        next_id: 4,
    }
}

#[gpui_kit::test]
fn dragging_a_middle_segment_moves_only_that_segment(cx: &mut TestAppContext) {
    let mut h = open(cx, u_route());
    h.drag((12, 8), (12, 4));
    let s = h.schematic();
    assert_eq!(
        points(&s, 3),
        vec![p(4, 0), p(8, 0), p(8, 4), p(16, 4), p(16, 0), p(20, 0)],
        "the bottom moved up four units and its legs got shorter"
    );
    assert_orthogonal(&s);
    assert!(connected(&s, p(4, 0), p(20, 0)));
    assert_eq!(h.read(|e| e.selection()), vec![ObjectRef::Wire(3)]);
    assert_eq!(changes(&mut h), 1, "one change for the whole drag");
    h.keys(&primary("z"));
    assert_eq!(h.schematic(), u_route(), "the drag is one undo step");
    h.close();
}

#[gpui_kit::test]
fn the_segment_moves_in_whole_units_perpendicular_to_itself(cx: &mut TestAppContext) {
    let mut h = open(cx, u_route());
    // Sideways and fractional mouse movement: only the vertical part,
    // snapped to whole units, moves a horizontal segment.
    let (a, b) = (h.at_f(12.4, 8.2), h.at_f(18.6, 2.4));
    h.drag_px(
        a,
        b,
        gpui_kit::MouseButton::Left,
        gpui_kit::Modifiers::default(),
    );
    assert_eq!(
        points(&h.schematic(), 3),
        vec![p(4, 0), p(8, 0), p(8, 2), p(16, 2), p(16, 0), p(20, 0)]
    );
    h.close();
}

#[gpui_kit::test]
fn dragging_along_the_segment_changes_nothing(cx: &mut TestAppContext) {
    let mut h = open(cx, u_route());
    h.events();
    h.drag((10, 8), (14, 8));
    assert_eq!(h.schematic(), u_route());
    assert_eq!(changes(&mut h), 0, "no change event");
    assert!(!h.read(|e| e.can_undo()), "no undo step");
    h.close();
}

/// R1's right pin (4,0) wired to an upright R2 at (16,12) (pins (16,8) and
/// (16,16)) by a wire with three segments.
fn three_segments() -> Schematic {
    Schematic {
        components: vec![
            resistor(1, "R1", p(0, 0), false),
            resistor(2, "R2", p(16, 12), true),
        ],
        wires: vec![wire(3, &[p(4, 0), p(10, 0), p(10, 8), p(16, 8)])],
        labels: Vec::new(),
        next_id: 4,
    }
}

#[gpui_kit::test]
fn an_end_segment_on_a_pin_gets_a_bend_so_the_pin_stays_connected(cx: &mut TestAppContext) {
    let mut h = open(cx, three_segments());
    h.drag((6, 0), (6, -4));
    let s = h.schematic();
    assert_eq!(
        points(&s, 3),
        vec![p(4, 0), p(4, -4), p(10, -4), p(10, 8), p(16, 8)],
        "the wire leaves the pin with a new bend; the far segments stay"
    );
    assert_orthogonal(&s);
    assert!(connected(&s, p(4, 0), p(16, 8)));
    h.close();
}

/// An L-shaped wire between R1's right pin (4,0) and an upright R2 at
/// (12,12) (pins (12,8) and (12,16)).
fn l_route() -> Schematic {
    Schematic {
        components: vec![
            resistor(1, "R1", p(0, 0), false),
            resistor(2, "R2", p(12, 12), true),
        ],
        wires: vec![wire(3, &[p(4, 0), p(12, 0), p(12, 8)])],
        labels: Vec::new(),
        next_id: 4,
    }
}

#[gpui_kit::test]
fn either_leg_of_an_l_shaped_wire_can_be_dragged(cx: &mut TestAppContext) {
    let mut h = open(cx, l_route());
    h.drag((12, 4), (16, 4));
    let s = h.schematic();
    assert_eq!(
        points(&s, 3),
        vec![p(4, 0), p(16, 0), p(16, 8), p(12, 8)],
        "the vertical leg moved right; the pin end got a bend"
    );
    assert_orthogonal(&s);
    assert!(connected(&s, p(4, 0), p(12, 8)));
    h.keys(&primary("z"));
    assert_eq!(h.schematic(), l_route());
    h.drag((8, 0), (8, -4));
    let s = h.schematic();
    assert_eq!(
        points(&s, 3),
        vec![p(4, 0), p(4, -4), p(12, -4), p(12, 8)],
        "the horizontal leg moved up; the vertical leg got longer"
    );
    assert_orthogonal(&s);
    assert!(connected(&s, p(4, 0), p(12, 8)));
    h.close();
}

/// A straight wire from R1 (4,0) to R2 (20,0) with a T-junction at (12,0):
/// a second wire down to an upright R3 at (12,16) (pins (12,12) and (12,20)).
fn t_junction() -> Schematic {
    Schematic {
        components: vec![
            resistor(1, "R1", p(0, 0), false),
            resistor(2, "R2", p(24, 0), false),
            resistor(3, "R3", p(12, 16), true),
        ],
        wires: vec![
            wire(4, &[p(4, 0), p(20, 0)]),
            wire(5, &[p(12, 0), p(12, 12)]),
        ],
        labels: Vec::new(),
        next_id: 6,
    }
}

#[gpui_kit::test]
fn a_wire_ending_on_the_dragged_segment_follows_it(cx: &mut TestAppContext) {
    let mut h = open(cx, t_junction());
    h.drag((8, 0), (8, -4));
    let s = h.schematic();
    assert_eq!(points(&s, 4), vec![p(4, 0), p(4, -4), p(20, -4), p(20, 0)]);
    assert_eq!(
        points(&s, 5),
        vec![p(12, -4), p(12, 12)],
        "the T-junction wire stretches"
    );
    assert_orthogonal(&s);
    assert!(connected(&s, p(4, 0), p(12, 12)), "R3 is still on the net");
    assert_eq!(connectivity(&s).junctions, vec![p(12, -4)]);
    h.keys(&primary("z"));
    h.drag((8, 0), (8, 4));
    let s = h.schematic();
    assert_eq!(points(&s, 5), vec![p(12, 4), p(12, 12)], "and shrinks");
    assert!(connected(&s, p(4, 0), p(12, 12)));
    h.close();
}

/// The usual drawn corner: two separate wires meeting end to end, from
/// R1's right pin (4,0) to an upright R2 at (12,12) (pins (12,8) and (12,16)).
fn two_wire_corner() -> Schematic {
    Schematic {
        components: vec![
            resistor(1, "R1", p(0, 0), false),
            resistor(2, "R2", p(12, 12), true),
        ],
        wires: vec![
            wire(3, &[p(4, 0), p(12, 0)]),
            wire(4, &[p(12, 0), p(12, 8)]),
        ],
        labels: Vec::new(),
        next_id: 5,
    }
}

#[gpui_kit::test]
fn a_corner_of_two_wires_slides_without_overlapping_legs(cx: &mut TestAppContext) {
    let mut h = open(cx, two_wire_corner());
    h.drag((12, 4), (8, 4));
    let s = h.schematic();
    assert_eq!(
        points(&s, 3),
        vec![p(4, 0), p(8, 0)],
        "the joined wire's end follows"
    );
    assert_eq!(points(&s, 4), vec![p(8, 0), p(8, 8), p(12, 8)]);
    assert_orthogonal(&s);
    assert!(connected(&s, p(4, 0), p(12, 8)));
    h.close();
}

#[gpui_kit::test]
fn a_pin_on_the_dragged_segment_keeps_a_stub(cx: &mut TestAppContext) {
    // R3's upper pin (12,0) sits on the interior of the straight wire.
    let mut s = t_junction();
    s.wires.truncate(1);
    s.components[2].position = p(12, 4);
    let mut h = open(cx, s);
    h.drag((8, 0), (8, -6));
    let s = h.schematic();
    assert_eq!(points(&s, 4), vec![p(4, 0), p(4, -6), p(20, -6), p(20, 0)]);
    assert!(
        s.wires
            .iter()
            .any(|w| w.points == vec![p(12, 0), p(12, -6)] || w.points == vec![p(12, -6), p(12, 0)]),
        "a stub joins the pin to the moved segment: {:?}",
        s.wires
    );
    assert!(connected(&s, p(4, 0), p(12, 0)));
    h.close();
}

#[gpui_kit::test]
fn a_wire_touching_nothing_moves_freely(cx: &mut TestAppContext) {
    let s = Schematic {
        wires: vec![wire(1, &[p(0, 0), p(8, 0)])],
        next_id: 2,
        ..Schematic::default()
    };
    let mut h = open(cx, s);
    h.drag((4, 0), (6, 4));
    assert_eq!(points(&h.schematic(), 1), vec![p(2, 4), p(10, 4)]);
    h.close();
}

#[gpui_kit::test]
fn dragging_a_wire_in_a_bigger_selection_moves_the_whole_selection(cx: &mut TestAppContext) {
    let mut h = open(cx, super::fixtures::two_resistors());
    h.click(0, 0);
    h.click_with(8, 0, shift());
    assert_eq!(h.read(|e| e.selection()).len(), 2);
    h.drag((8, 0), (8, -4));
    let s = h.schematic();
    assert_eq!(s.components[0].position, p(0, -4), "R1 moved with the wire");
    assert_eq!(s.components[1].position, p(16, 0), "R2 stayed");
    let w = &s.wires[0].points;
    assert_eq!(w.first(), Some(&p(4, -4)));
    assert_eq!(w.last(), Some(&p(12, 0)));
    assert_orthogonal(&s);
    h.keys(&primary("z"));
    assert_eq!(h.schematic(), super::fixtures::two_resistors());
    h.close();
}

#[gpui_kit::test]
fn hovering_a_selected_segment_shows_which_way_it_drags(cx: &mut TestAppContext) {
    let mut h = open(cx, l_route());
    let at = h.at(8, 0);
    h.hover(at);
    assert_eq!(
        h.read(|e| e.cursor_style()),
        gpui_kit::CursorStyle::Arrow,
        "an unselected wire shows the plain arrow"
    );
    h.click(8, 0);
    h.hover(at);
    assert_eq!(
        h.read(|e| e.cursor_style()),
        gpui_kit::CursorStyle::ResizeUpDown
    );
    let at = h.at(12, 4);
    h.hover(at);
    assert_eq!(
        h.read(|e| e.cursor_style()),
        gpui_kit::CursorStyle::ResizeLeftRight
    );
    h.close();
}

#[gpui_kit::test]
fn escape_aborts_a_segment_drag(cx: &mut TestAppContext) {
    let mut h = open(cx, u_route());
    let (a, b) = (h.at(12, 8), h.at(12, 2));
    h.cx.simulate_mouse_move(a, None, Modifiers::default());
    h.cx.simulate_mouse_down(a, MouseButton::Left, Modifiers::default());
    h.cx.simulate_mouse_move(b, Some(MouseButton::Left), Modifiers::default());
    assert_eq!(
        h.read(|e| e.cursor_style()),
        CursorStyle::ResizeUpDown,
        "the drag shows its direction"
    );
    h.keys("escape");
    h.cx.simulate_mouse_up(b, MouseButton::Left, Modifiers::default());
    assert_eq!(h.schematic(), u_route());
    assert!(!h.read(|e| e.can_undo()));
    h.close();
}

/// The RC fixture with a result shown. Wire 6 runs from R1's right pin
/// (4,-8) to the `out` label (12,-8) and down to C1 (12,-4).
fn probed(cx: &mut TestAppContext) -> Harness {
    let mut h = open(cx, rc_low_pass());
    let context = probe_context(&rc_low_pass(), AnalysisKind::Transient);
    h.update(|e, cx| e.set_probe_context(Some(context), cx));
    h.events();
    h
}

fn voltage(node: &str) -> ProbeRequest {
    ProbeRequest::Voltage { node: node.into() }
}

#[gpui_kit::test]
fn with_results_shown_an_unselected_wire_is_the_probe(cx: &mut TestAppContext) {
    let mut h = probed(cx);
    let at = h.at(8, -8);
    h.hover(at);
    assert_eq!(h.read(|e| e.cursor_style()), CursorStyle::Crosshair);
    assert!(h.read(|e| e.probe_glyph()).is_some());
    h.drag((8, -8), (-8, -8));
    assert_eq!(
        probes(&mut h),
        vec![ProbeRequest::Differential {
            plus: "out".into(),
            minus: "in".into()
        }]
    );
    assert_eq!(h.schematic(), rc_low_pass(), "probing never edits");
    assert!(
        h.read(|e| e.selection()).is_empty(),
        "a differential probe selects nothing"
    );
    h.close();
}

#[gpui_kit::test]
fn with_results_shown_a_clicked_wire_is_selected_and_then_drags(cx: &mut TestAppContext) {
    let mut h = probed(cx);
    h.click(8, -8);
    assert_eq!(probes(&mut h), vec![voltage("out")], "a click probes");
    assert_eq!(h.read(|e| e.selection()), vec![ObjectRef::Wire(6)]);
    let at = h.at(8, -8);
    h.hover(at);
    assert_eq!(
        h.read(|e| e.cursor_style()),
        CursorStyle::ResizeUpDown,
        "the cursor tells that a press now drags"
    );
    assert!(h.read(|e| e.probe_glyph()).is_none());
    h.click(8, -8);
    assert_eq!(
        probes(&mut h),
        vec![voltage("out")],
        "a click on the selected wire still probes"
    );
    assert_eq!(h.schematic(), rc_low_pass(), "and edits nothing");
    h.drag((8, -8), (8, -12));
    assert_eq!(
        h.events(),
        vec![EditorEvent::Changed],
        "dragging edits instead of probing"
    );
    let s = h.schematic();
    assert_eq!(
        points(&s, 6),
        vec![p(4, -8), p(4, -12), p(12, -12), p(12, -4)],
        "R1's pin keeps a bend, C1's leg stretches"
    );
    assert_eq!(
        s.label(9).map(|l| l.position),
        Some(p(12, -12)),
        "the out label moves with the segment"
    );
    assert!(connected(&s, p(4, -8), p(12, -4)));
    h.close();
}

#[gpui_kit::test]
fn with_results_shown_a_selection_containing_the_wire_moves(cx: &mut TestAppContext) {
    let mut h = probed(cx);
    h.update(|e, cx| e.set_selection([ObjectRef::Wire(6), ObjectRef::Component(2)], cx));
    h.drag((8, -8), (8, -10));
    assert!(probes(&mut h).is_empty());
    assert_eq!(
        h.schematic().components[1].position,
        p(0, -10),
        "R1 moved with the wire"
    );
    h.close();
}

#[gpui_kit::test]
fn read_only_editors_probe_even_on_a_selected_wire(cx: &mut TestAppContext) {
    let mut h = probed(cx);
    h.update(|e, cx| {
        e.set_read_only(true, cx);
        e.set_selection([ObjectRef::Wire(6)], cx);
    });
    let at = h.at(8, -8);
    h.hover(at);
    assert_eq!(h.read(|e| e.cursor_style()), CursorStyle::Crosshair);
    h.drag((8, -8), (-8, -8));
    assert_eq!(
        probes(&mut h),
        vec![ProbeRequest::Differential {
            plus: "out".into(),
            minus: "in".into()
        }]
    );
    assert_eq!(h.schematic(), rc_low_pass());
    h.close();
}

// Regressions found in review.

/// R1 (pins (-4,0), (4,0)) and R2 at (24,8) (pins (20,8), (28,8)), joined
/// by wire 3 from R1 to (12,0) and wire 4 from there down and across to R2.
fn corner_and_step() -> Schematic {
    Schematic {
        components: vec![
            resistor(1, "R1", p(0, 0), false),
            resistor(2, "R2", p(24, 8), false),
        ],
        wires: vec![
            wire(3, &[p(4, 0), p(12, 0)]),
            wire(4, &[p(12, 0), p(12, 8), p(20, 8)]),
        ],
        labels: Vec::new(),
        next_id: 5,
    }
}

#[gpui_kit::test]
fn a_wire_that_cleanup_merges_into_its_neighbour_stays_selected(cx: &mut TestAppContext) {
    let mut h = open(cx, corner_and_step());
    // Raising wire 4's bottom onto R1's row makes it continue wire 3 in a
    // straight line, so cleanup joins the two into wire 3.
    h.drag((16, 8), (16, 0));
    let s = h.schematic();
    assert_eq!(points(&s, 3), vec![p(4, 0), p(20, 0), p(20, 8)]);
    assert!(s.wire(4).is_none(), "wire 4 was joined into wire 3");
    assert_eq!(
        h.read(|e| e.selection()),
        vec![ObjectRef::Wire(3)],
        "the dragged run stays selected"
    );
    let at = h.at(20, 4);
    h.hover(at);
    assert_eq!(
        h.read(|e| e.cursor_style()),
        CursorStyle::ResizeLeftRight,
        "and can be dragged again straight away"
    );
    h.keys(&primary("z"));
    assert_eq!(h.schematic(), corner_and_step(), "one undo step");
    h.keys(&primary("y"));
    assert_eq!(points(&h.schematic(), 3), vec![p(4, 0), p(20, 0), p(20, 8)]);
    h.close();
}

#[gpui_kit::test]
fn zoomed_out_the_segment_follows_the_mouse_without_jumping(cx: &mut TestAppContext) {
    // At 5 px per unit the 7 px hit tolerance exceeds a unit, so a press
    // 1.2 below the wire is nearer the next grid line. The segment used to
    // jump a unit away from the mouse once it came back.
    let view = SchematicView {
        zoom: 5.0,
        ..super::harness::fixed_view()
    };
    let mut h = open_with_view(cx, u_route(), view);
    for (to, bottom) in [
        ((16.0, 9.2), 8),
        ((16.0, 8.9), 8),
        ((12.0, 10.4), 9),
        ((12.0, 11.9), 11),
    ] {
        let (a, b) = (h.at_f(12.0, 9.2), h.at_f(to.0, to.1));
        h.drag_px(a, b, MouseButton::Left, Modifiers::default());
        let expected = vec![
            p(4, 0),
            p(8, 0),
            p(8, bottom),
            p(16, bottom),
            p(16, 0),
            p(20, 0),
        ];
        assert_eq!(
            points(&h.schematic(), 3),
            expected,
            "pressed at y = 9.2, released at {to:?}"
        );
        if bottom != 8 {
            h.keys(&primary("z"));
        } else {
            assert!(!h.read(|e| e.can_undo()), "{to:?} changed nothing");
        }
        assert_eq!(h.schematic(), u_route());
    }
    h.close();
}

#[gpui_kit::test]
fn a_middle_press_during_a_segment_drag_does_not_drop_it(cx: &mut TestAppContext) {
    let mut h = open(cx, u_route());
    let (a, b, c) = (h.at(12, 8), h.at(12, 4), h.at(12, 2));
    let none = Modifiers::default();
    h.cx.simulate_mouse_move(a, None, none);
    h.cx.simulate_mouse_down(a, MouseButton::Left, none);
    h.cx.simulate_mouse_move(b, Some(MouseButton::Left), none);
    h.cx.simulate_mouse_down(b, MouseButton::Middle, none);
    h.cx.simulate_mouse_up(b, MouseButton::Middle, none);
    h.cx.simulate_mouse_move(c, Some(MouseButton::Left), none);
    h.cx.simulate_mouse_up(c, MouseButton::Left, none);
    assert_eq!(
        points(&h.schematic(), 3),
        vec![p(4, 0), p(8, 0), p(8, 2), p(16, 2), p(16, 0), p(20, 0)]
    );
    assert_eq!(h.read(|e| e.view()), super::harness::fixed_view(), "no pan");
    h.close();
}

#[gpui_kit::test]
fn dragging_out_and_back_changes_nothing(cx: &mut TestAppContext) {
    let mut h = open(cx, u_route());
    h.events();
    let (a, b) = (h.at(12, 8), h.at(12, 2));
    let none = Modifiers::default();
    h.cx.simulate_mouse_move(a, None, none);
    h.cx.simulate_mouse_down(a, MouseButton::Left, none);
    h.cx.simulate_mouse_move(b, Some(MouseButton::Left), none);
    h.cx.simulate_mouse_move(a, Some(MouseButton::Left), none);
    h.cx.simulate_mouse_up(a, MouseButton::Left, none);
    assert_eq!(h.schematic(), u_route());
    assert_eq!(changes(&mut h), 0);
    assert!(!h.read(|e| e.can_undo()));
    h.close();
}

#[gpui_kit::test]
fn segment_drags_work_at_every_ui_scale(cx: &mut TestAppContext) {
    let mut h = open(cx, u_route());
    for scale in [0.75, 2.0] {
        super::grid::set_ui_scale(&mut h, scale);
        h.drag((12, 8), (12, 4));
        assert_eq!(
            points(&h.schematic(), 3),
            vec![p(4, 0), p(8, 0), p(8, 4), p(16, 4), p(16, 0), p(20, 0)],
            "UI scale {scale}"
        );
        h.keys(&primary("z"));
        assert_eq!(h.schematic(), u_route(), "UI scale {scale}");
    }
    h.close();
}
