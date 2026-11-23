//! The dot grid in a real window, and snapping to it (D14, D22): a dot on
//! every grid unit at the default zoom, and parts, wires and labels snap to
//! every one of those dots — including the ones half-way between the
//! schema-2 grid points, which only the finer grid unit can reach.

use super::fixtures::two_resistors;
use super::harness::{Harness, fixed_view, open, open_with_view};
use crate::canvas::grid::DotGrid;
use crate::canvas::palette::Palette;
use crate::canvas::scene::Frame;
use gpui_kit::{Modifiers, MouseButton, TestAppContext, px};
use xsim_project::{Point, Schematic, SchematicView};

/// The dot grid of the canvas's last frame, and the UI scale it saw.
fn painted_grid(h: &mut Harness) -> (DotGrid, f32) {
    h.read(|e| {
        let viewport = e.viewport.get().expect("the canvas has been laid out");
        let frame = Frame {
            camera: e.camera(),
            viewport,
        };
        (DotGrid::of(&frame), viewport.ui_scale)
    })
}

/// Whether the last frame painted a dot at `p`.
fn has_dot(h: &mut Harness, p: Point) -> bool {
    painted_grid(h).0.dots().any(|dot| dot == p)
}

/// Sets the interface scale the way the desktop does: through the theme's
/// font size, which becomes the window's rem size.
pub(super) fn set_ui_scale(h: &mut Harness, scale: f32) {
    h.cx.update(|window, cx| {
        gpui_kit::component::Theme::global_mut(cx).font_size = px(16.0 * scale);
        window.refresh();
    });
    h.cx.run_until_parked();
}

/// The interface scales the snapping tests run at.
const UI_SCALES: [f32; 4] = [0.75, 1.0, 1.25, 2.0];

#[gpui_kit::test]
fn a_new_circuit_shows_a_dot_on_every_grid_unit_at_every_ui_scale(cx: &mut TestAppContext) {
    let mut h = open_with_view(cx, Schematic::default(), SchematicView::default());
    assert_eq!(h.read(|e| e.view().zoom), SchematicView::default().zoom);
    let (grid, ui_scale) = painted_grid(&mut h);
    assert_eq!(ui_scale, 1.0);
    assert_eq!(grid.pitch(), 1.0, "a dot on every snap point");
    for scale in UI_SCALES {
        set_ui_scale(&mut h, scale);
        let (grid, ui_scale) = painted_grid(&mut h);
        assert_eq!(ui_scale, scale, "the canvas sees the new UI scale");
        assert_eq!(grid.pitch(), 1.0, "UI scale {scale}");
    }
    h.close();
}

#[gpui_kit::test]
fn zooming_out_thins_the_dots_and_zooming_in_brings_them_back(cx: &mut TestAppContext) {
    let mut h = open(cx, two_resistors());
    assert_eq!(h.read(|e| e.view().zoom), SchematicView::default().zoom);
    assert_eq!(painted_grid(&mut h).0.pitch(), 1.0);
    let at = h.at(8, 0);
    h.wheel_notches(at, -2);
    assert_eq!(
        painted_grid(&mut h).0.pitch(),
        1.0,
        "two notches out (7.2 px per unit) keep a dot on every unit"
    );
    h.wheel_notches(at, -1);
    assert_eq!(painted_grid(&mut h).0.pitch(), 2.0, "6.1 px per unit");
    h.wheel_notches(at, -15);
    assert_eq!(h.read(|e| e.view().zoom), crate::camera::MIN_ZOOM);
    assert_eq!(painted_grid(&mut h).0.pitch(), 10.0, "fully zoomed out");
    h.wheel_notches(at, 30);
    assert_eq!(h.read(|e| e.view().zoom), crate::camera::MAX_ZOOM);
    assert_eq!(painted_grid(&mut h).0.pitch(), 1.0);
    h.close();
}

#[gpui_kit::test]
fn one_wheel_notch_zooms_by_the_documented_step(cx: &mut TestAppContext) {
    // A Windows notch arrives as three lines (`WheelScrollLines`); it used
    // to zoom 1.18³ ≈ 1.64×, and now zooms 1.18× (ui/navigation.rs).
    let mut h = open(cx, two_resistors());
    let at = h.at(8, 0);
    h.wheel(
        at,
        -crate::ui::navigation::LINES_PER_NOTCH,
        Modifiers::default(),
    );
    let zoom = h.read(|e| e.view().zoom);
    assert!((zoom - 10.0 / 1.18).abs() < 0.01, "zoom {zoom}");
    assert_eq!(
        painted_grid(&mut h).0.pitch(),
        1.0,
        "one notch out keeps a dot on every unit"
    );
    h.wheel(
        at,
        crate::ui::navigation::LINES_PER_NOTCH,
        Modifiers::default(),
    );
    assert!((h.read(|e| e.view().zoom) - 10.0).abs() < 0.01);
    h.close();
}

#[gpui_kit::test]
fn grid_dots_contrast_with_the_canvas_in_both_themes(cx: &mut TestAppContext) {
    use gpui_kit::component::{ActiveTheme, Theme, ThemeMode};
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        let (palette, theme, background) = cx.update(|cx| {
            if !cx.has_global::<Theme>() {
                gpui_kit::init(cx);
            }
            Theme::change(mode, None, cx);
            let theme = cx.theme();
            (
                Palette::from_theme(theme),
                theme.muted_foreground,
                theme.background,
            )
        });
        assert_eq!(
            (palette.grid.h, palette.grid.s, palette.grid.l),
            (theme.h, theme.s, theme.l),
            "{mode:?}: the theme's muted foreground"
        );
        assert!(palette.grid.a > 0.0, "{mode:?}: {palette:?}");
        assert!(
            (palette.grid.l - background.l).abs() > 0.2,
            "{mode:?}: the dots contrast with the canvas"
        );
    }
}

/// D22: at the default zoom, at every interface scale, a part placed near
/// a dot lands on it, whichever dot it is.
#[gpui_kit::test]
fn parts_snap_to_every_dot_at_every_ui_scale(cx: &mut TestAppContext) {
    let mut h = open(cx, Schematic::default());
    assert_eq!(fixed_view().zoom, SchematicView::default().zoom);
    for (i, scale) in UI_SCALES.into_iter().enumerate() {
        set_ui_scale(&mut h, scale);
        assert!(painted_grid(&mut h).0.on_every_grid_point(), "UI {scale}");
        // Odd and even coordinates, each clicked off its dot.
        let base = 12 * i32::try_from(i).unwrap_or_default() - 18;
        for (dot, off) in [((base + 1, -7), (0.3, 0.2)), ((base, 5), (-0.4, 0.45))] {
            let target = Point::new(dot.0, dot.1);
            assert!(has_dot(&mut h, target), "UI {scale}: no dot at {target:?}");
            h.keys("shift-r");
            let p = h.at_f(dot.0 as f32 + off.0, dot.1 as f32 + off.1);
            h.click_px(p, Modifiers::default());
            h.keys("escape");
            let s = h.schematic();
            let placed = s.components.last().expect("a part was placed");
            assert_eq!(placed.position, target, "UI scale {scale}");
        }
    }
    h.close();
}

/// D22: a drag moves a part by whole dots, one dot as well as several, and
/// an arrow key nudges it by exactly one dot.
#[gpui_kit::test]
fn moves_snap_to_every_dot(cx: &mut TestAppContext) {
    let mut h = open(cx, two_resistors());
    // From R1's body to near the dot one unit right and one down.
    let (from, to) = (h.at_f(0.2, 0.3), h.at_f(1.3, 0.6));
    h.drag_px(from, to, gpui_kit::MouseButton::Left, Modifiers::default());
    assert_eq!(h.schematic().components[0].position, Point::new(1, 1));
    assert!(has_dot(&mut h, Point::new(1, 1)));
    h.drag((1, 1), (4, -2));
    assert_eq!(h.schematic().components[0].position, Point::new(4, -2));
    h.keys("left");
    assert_eq!(h.schematic().components[0].position, Point::new(3, -2));
    h.keys("down");
    assert_eq!(h.schematic().components[0].position, Point::new(3, -1));
    let pins = h.schematic().components[0].pins();
    assert_eq!(pins, [Point::new(-1, -1), Point::new(7, -1)]);
    for pin in pins {
        assert!(has_dot(&mut h, pin), "pin {pin:?} is on a dot");
    }
    h.close();
}

/// D22: wire corners land on the dot nearest each click, odd ones too.
#[gpui_kit::test]
fn wire_corners_snap_to_every_dot(cx: &mut TestAppContext) {
    let mut h = open(cx, Schematic::default());
    assert_eq!(painted_grid(&mut h).0.pitch(), 1.0);
    h.keys("w");
    // Each click lands just off a dot, between it and its neighbour.
    for (x, y) in [(0.4, 0.1), (5.4, -0.3), (5.3, 3.4), (-1.45, 3.2)] {
        let p = h.at_f(x, y);
        h.click_px(p, Modifiers::default());
    }
    h.keys("enter");
    let s = h.schematic();
    let corners = [(0, 0), (5, 0), (5, 3), (-1, 3)].map(|(x, y)| Point::new(x, y));
    assert_eq!(s.wires[0].points, corners);
    for corner in corners {
        assert!(has_dot(&mut h, corner), "{corner:?} is on a dot");
    }
    h.close();
}

/// D22: a dragged wire segment lands on the dot nearest the mouse's
/// displacement, on odd rows and columns too, and its neighbours follow it
/// there.
#[gpui_kit::test]
fn segment_drags_snap_to_every_dot(cx: &mut TestAppContext) {
    let mut h = open(cx, super::segment_drag::u_route());
    assert_eq!(painted_grid(&mut h).0.pitch(), 1.0);
    let wire = |h: &mut Harness| h.schematic().wires[0].points.clone();
    let p = Point::new;
    // The U's bottom, y = 8, up by 4.8 units: to y = 3.
    let (from, to) = (h.at_f(12.2, 8.1), h.at_f(12.6, 3.3));
    h.drag_px(from, to, MouseButton::Left, Modifiers::default());
    assert_eq!(
        wire(&mut h),
        [p(4, 0), p(8, 0), p(8, 3), p(16, 3), p(16, 0), p(20, 0)]
    );
    // Its left leg, x = 8, left by 2.9 units: to x = 5.
    let (from, to) = (h.at_f(8.2, 1.6), h.at_f(5.3, 1.9));
    h.drag_px(from, to, MouseButton::Left, Modifiers::default());
    let points = wire(&mut h);
    assert_eq!(
        points,
        [p(4, 0), p(5, 0), p(5, 3), p(16, 3), p(16, 0), p(20, 0)]
    );
    for corner in points {
        assert!(has_dot(&mut h, corner), "{corner:?} is on a dot");
    }
    h.close();
}

/// D22: a label lands on the dot nearest the click, between the schema-2
/// grid points as well.
#[gpui_kit::test]
fn labels_snap_to_every_dot(cx: &mut TestAppContext) {
    let mut h = open(cx, two_resistors());
    h.keys("n");
    // On the wire from (4, 0) to (12, 0), nearest (7, 0).
    let p = h.at_f(7.4, 0.3);
    h.click_px(p, Modifiers::default());
    h.type_text("mid");
    h.keys("enter");
    let s = h.schematic();
    assert_eq!(s.labels.len(), 1);
    assert_eq!(s.labels[0].position, Point::new(7, 0));
    assert!(s.wires.iter().any(|w| w.points.contains(&Point::new(7, 0))));
    assert!(has_dot(&mut h, Point::new(7, 0)));
    h.close();
}
