//! The dot grid in a real window: a dot every half unit at the default zoom,
//! while everything still snaps to whole units (D14).

use super::fixtures::two_resistors;
use super::harness::{Harness, open, open_with_view};
use crate::canvas::grid::DotGrid;
use crate::canvas::palette::Palette;
use crate::canvas::scene::Frame;
use gpui_kit::{Modifiers, TestAppContext, px};
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

/// Sets the interface scale the way the desktop does: through the theme's
/// font size, which becomes the window's rem size.
pub(super) fn set_ui_scale(h: &mut Harness, scale: f32) {
    h.cx.update(|window, cx| {
        gpui_kit::component::Theme::global_mut(cx).font_size = px(16.0 * scale);
        window.refresh();
    });
    h.cx.run_until_parked();
}

#[gpui_kit::test]
fn a_new_circuit_shows_a_dot_every_half_unit_at_every_ui_scale(cx: &mut TestAppContext) {
    let mut h = open_with_view(cx, Schematic::default(), SchematicView::default());
    assert_eq!(h.read(|e| e.view().zoom), SchematicView::default().zoom);
    let (grid, ui_scale) = painted_grid(&mut h);
    assert_eq!(ui_scale, 1.0);
    assert_eq!(grid.pitch(), 0.5, "twice the old whole-unit density");
    for scale in [0.75, 1.25, 2.0] {
        set_ui_scale(&mut h, scale);
        let (grid, ui_scale) = painted_grid(&mut h);
        assert_eq!(ui_scale, scale, "the canvas sees the new UI scale");
        assert_eq!(grid.pitch(), 0.5, "UI scale {scale}");
    }
    h.close();
}

#[gpui_kit::test]
fn zooming_out_thins_the_dots_and_zooming_in_brings_them_back(cx: &mut TestAppContext) {
    let mut h = open(cx, two_resistors());
    assert_eq!(h.read(|e| e.view().zoom), SchematicView::default().zoom);
    assert_eq!(painted_grid(&mut h).0.pitch(), 0.5);
    let at = h.at(4, 0);
    h.wheel_notches(at, -2);
    assert_eq!(
        painted_grid(&mut h).0.pitch(),
        0.5,
        "two notches out (14.4 px per unit) keep the half-unit dots"
    );
    h.wheel_notches(at, -1);
    assert_eq!(painted_grid(&mut h).0.pitch(), 1.0, "12.2 px per unit");
    h.wheel_notches(at, -15);
    assert_eq!(h.read(|e| e.view().zoom), crate::camera::MIN_ZOOM);
    assert_eq!(painted_grid(&mut h).0.pitch(), 5.0, "fully zoomed out");
    h.wheel_notches(at, 30);
    assert_eq!(h.read(|e| e.view().zoom), crate::camera::MAX_ZOOM);
    assert_eq!(painted_grid(&mut h).0.pitch(), 0.5);
    h.close();
}

#[gpui_kit::test]
fn one_wheel_notch_zooms_by_the_documented_step(cx: &mut TestAppContext) {
    // A Windows notch arrives as three lines (`WheelScrollLines`); it used
    // to zoom 1.18³ ≈ 1.64×, and now zooms 1.18× (ui/navigation.rs).
    let mut h = open(cx, two_resistors());
    let at = h.at(4, 0);
    h.wheel(
        at,
        -crate::ui::navigation::LINES_PER_NOTCH,
        Modifiers::default(),
    );
    let zoom = h.read(|e| e.view().zoom);
    assert!((zoom - 20.0 / 1.18).abs() < 0.01, "zoom {zoom}");
    assert_eq!(
        painted_grid(&mut h).0.pitch(),
        0.5,
        "one notch out keeps the half-unit dots"
    );
    h.wheel(
        at,
        crate::ui::navigation::LINES_PER_NOTCH,
        Modifiers::default(),
    );
    assert!((h.read(|e| e.view().zoom) - 20.0).abs() < 0.01);
    h.close();
}

#[gpui_kit::test]
fn snap_dots_are_stronger_than_half_unit_dots_in_both_themes(cx: &mut TestAppContext) {
    use gpui_kit::component::{ActiveTheme, Theme, ThemeMode};
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        let (palette, background) = cx.update(|cx| {
            if !cx.has_global::<Theme>() {
                gpui_kit::init(cx);
            }
            Theme::change(mode, None, cx);
            (Palette::from_theme(cx.theme()), cx.theme().background)
        });
        assert!(
            palette.grid.a > palette.grid_half.a && palette.grid_half.a > 0.0,
            "{mode:?}: {palette:?}"
        );
        assert_eq!(
            (palette.grid.h, palette.grid.s, palette.grid.l),
            (
                palette.grid_half.h,
                palette.grid_half.s,
                palette.grid_half.l
            ),
            "{mode:?}: one theme token, two strengths"
        );
        assert!(
            (palette.grid.l - background.l).abs() > 0.2,
            "{mode:?}: the dots contrast with the canvas"
        );
    }
}

#[gpui_kit::test]
fn wires_still_snap_to_whole_units_next_to_half_unit_dots(cx: &mut TestAppContext) {
    let mut h = open(cx, Schematic::default());
    assert_eq!(painted_grid(&mut h).0.pitch(), 0.5);
    h.keys("w");
    // Each click lands just off a half-unit dot, nearer one whole unit.
    for (x, y) in [(0.4, 0.1), (5.6, -0.1), (6.4, 3.6)] {
        let p = h.at_f(x, y);
        h.click_px(p, Modifiers::default());
    }
    h.keys("enter");
    let s = h.schematic();
    assert_eq!(
        s.wires[0].points,
        vec![Point::new(0, 0), Point::new(6, 0), Point::new(6, 4)]
    );
    h.close();
}
