//! Zoom, pan and fit (Q48), and the saved camera (Q39).

use super::fixtures::{rc_low_pass, two_resistors};
use super::harness::{fixed_view, open, open_sized, open_with_view, shift};
use crate::EditorEvent;
use gpui_kit::{Modifiers, MouseButton, ScrollDelta, TestAppContext, point, px};
use xsim_project::{Schematic, SchematicView};

#[gpui_kit::test]
fn the_wheel_zooms_about_the_cursor(cx: &mut TestAppContext) {
    let mut h = open(cx, two_resistors());
    let anchor = h.at(8, 0);
    h.wheel(anchor, 2.0, Modifiers::default());
    let view = h.read(|e| e.view());
    assert!(view.zoom > fixed_view().zoom, "{view:?}");
    assert!(view.user_set);
    let after = h.at(8, 0);
    assert!((after.x - anchor.x).abs() < px(1.0) && (after.y - anchor.y).abs() < px(1.0));
    assert!(h.events().contains(&EditorEvent::ViewChanged));
    h.wheel_notches(anchor, -20);
    assert_eq!(
        h.read(|e| e.view().zoom),
        crate::camera::MIN_ZOOM,
        "zoom is clamped"
    );
    h.close();
}

#[gpui_kit::test]
fn odd_wheel_events_zoom_by_at_most_a_capped_step(cx: &mut TestAppContext) {
    let mut h = open(cx, two_resistors());
    let at = h.at(8, 0);
    // Windows' "one screen at a time" setting: u32::MAX lines per notch.
    h.wheel(at, 4_294_967_295.0, Modifiers::default());
    let zoom = h.read(|e| e.view().zoom);
    assert!((zoom - 20.0 * 1.18).abs() < 0.01, "one notch: {zoom}");
    h.wheel(at, f32::NAN, Modifiers::default());
    assert_eq!(h.read(|e| e.view().zoom), zoom, "a broken event is ignored");
    h.close();
}

#[gpui_kit::test]
fn trackpad_deltas_pan_or_zoom_smoothly(cx: &mut TestAppContext) {
    let mut h = open(cx, two_resistors());
    let at = h.at(8, 0);
    h.scroll(
        at,
        ScrollDelta::Pixels(point(px(20.0), px(-10.0))),
        Modifiers::default(),
    );
    let view = h.read(|e| e.view());
    assert_eq!(view.zoom, fixed_view().zoom, "pixel deltas pan");
    assert_eq!(view.center, [-1.0, 0.5], "content follows the fingers");
    let ctrl = Modifiers {
        control: true,
        ..Modifiers::default()
    };
    h.scroll(at, ScrollDelta::Pixels(point(px(0.0), px(5.0))), ctrl);
    let zoom = h.read(|e| e.view().zoom);
    assert!(
        (zoom - 20.0 * 0.05f32.exp()).abs() < 1e-3,
        "with Ctrl a small delta zooms a little: {zoom}"
    );
    h.close();
}

#[gpui_kit::test]
fn shift_and_the_wheel_scroll_sideways_as_windows_reports_it(cx: &mut TestAppContext) {
    let mut h = open(cx, two_resistors());
    let at = h.at(8, 0);
    let notch = crate::ui::navigation::LINES_PER_NOTCH;
    // Windows and X11 turn Shift+wheel into a horizontal delta.
    h.scroll(at, ScrollDelta::Lines(point(-notch, 0.0)), shift());
    let view = h.read(|e| e.view());
    assert_eq!(view.zoom, fixed_view().zoom, "no zoom");
    assert!(view.center[0] > 0.0, "the view moved right: {view:?}");
    assert_eq!(view.center[1], 0.0);
    let before = view.center[0];
    h.scroll(at, ScrollDelta::Lines(point(0.0, -notch)), shift());
    assert!(
        h.read(|e| e.view()).center[0] > before,
        "a vertical delta with Shift scrolls sideways too"
    );
    h.close();
}

#[gpui_kit::test]
fn middle_drag_and_space_drag_pan(cx: &mut TestAppContext) {
    let mut h = open(cx, two_resistors());
    let start = h.at(0, 0);
    let end = start + point(px(40.0), px(-20.0));
    h.drag_px(start, end, MouseButton::Middle, Modifiers::default());
    let view = h.read(|e| e.view());
    assert_eq!(view.center, [-2.0, 1.0], "content follows the mouse");
    h.space(true);
    h.drag_px(end, start, MouseButton::Left, Modifiers::default());
    h.space(false);
    assert_eq!(h.read(|e| e.view()).center, [0.0, 0.0]);
    assert_eq!(
        h.schematic(),
        two_resistors(),
        "panning never selects or moves"
    );
    assert!(h.read(|e| e.selection()).is_empty());
    h.close();
}

#[gpui_kit::test]
fn an_unset_view_fits_the_drawing_on_first_show(cx: &mut TestAppContext) {
    let mut h = open_with_view(cx, rc_low_pass(), SchematicView::default());
    let view = h.read(|e| e.view());
    assert!(!view.user_set, "fitting automatically is not a user change");
    assert_ne!(view.zoom, SchematicView::default().zoom);
    assert!(
        view.center[0].abs() < 1.0 && view.center[1].abs() < 1.0,
        "{view:?}"
    );
    assert!(!h.events().contains(&EditorEvent::ViewChanged));
    h.close();
}

#[gpui_kit::test]
fn zoom_to_fit_recentres_the_drawing(cx: &mut TestAppContext) {
    let mut h = open(cx, two_resistors());
    let start = h.at(0, 0);
    h.drag_px(
        start,
        start + point(px(300.0), px(0.0)),
        MouseButton::Middle,
        Modifiers::default(),
    );
    h.click_control("toolbar-fit");
    let view = h.read(|e| e.view());
    assert_eq!(view.center, [4.0, 0.0]);
    h.keys("=");
    assert!(h.read(|e| e.view().zoom) > view.zoom);
    h.keys("f");
    assert_eq!(h.read(|e| e.view()), view);
    h.close();
}

#[gpui_kit::test]
fn an_empty_canvas_says_what_to_do(cx: &mut TestAppContext) {
    let mut h = open(cx, Schematic::default());
    assert!(h.control("empty-state").is_some());
    h.keys("shift-r");
    h.click(0, 0);
    h.keys("escape");
    assert!(h.control("empty-state").is_none());
    h.close();
}

#[gpui_kit::test]
fn a_narrow_editor_wraps_the_toolbar_instead_of_clipping_it(cx: &mut TestAppContext) {
    let width = px(450.0);
    let mut h = open_sized(
        cx,
        two_resistors(),
        fixed_view(),
        gpui_kit::size(width, px(600.0)),
    );
    for selector in [
        "toolbar-select",
        "toolbar-part-gnd",
        "toolbar-library",
        "toolbar-redo",
        "toolbar-fit",
    ] {
        let bounds =
            h.cx.debug_bounds(selector)
                .unwrap_or_else(|| panic!("{selector} missing"));
        assert!(bounds.right() <= width, "{selector} is clipped: {bounds:?}");
    }
    let canvas = h.cx.debug_bounds("schematic-canvas").expect("canvas");
    assert!(
        canvas.size.height > px(300.0),
        "the canvas keeps most of the height"
    );
    h.close();
}
