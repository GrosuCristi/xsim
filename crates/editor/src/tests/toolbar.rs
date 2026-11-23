//! The schematic toolbar has the waveform viewer toolbar's height, so the
//! two line up side by side (D14), at every UI scale, and still wraps in a
//! narrow editor.

use super::fixtures::two_resistors;
use super::grid::set_ui_scale;
use super::harness::{Harness, fixed_view, open, open_sized};
use crate::ui::toolbar::TOOLBAR_HEIGHT;
use gpui_kit::{Bounds, Pixels, TestAppContext, px, size};

/// The waveform viewer's toolbar height in rems, read from its source so a
/// change there fails here instead of silently misaligning the toolbars.
fn viewer_toolbar_height() -> f32 {
    let source = include_str!("../../../plot/src/ui/toolbar.rs");
    source
        .lines()
        .find_map(|line| {
            let value = line.trim().strip_prefix("const HEIGHT: f32 =")?;
            value.trim().trim_end_matches(';').parse().ok()
        })
        .unwrap_or_else(|| panic!("crates/plot/src/ui/toolbar.rs has no `const HEIGHT: f32`"))
}

fn bounds(h: &mut Harness, selector: &'static str) -> Bounds<Pixels> {
    h.cx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is not rendered"))
}

/// Buttons on the first toolbar row, by debug selector.
const BUTTONS: [&str; 5] = [
    "toolbar-select",
    "toolbar-part-r",
    "toolbar-library",
    "toolbar-undo",
    "toolbar-fit",
];

/// Checks one row: the bar is `rems` × the rem size tall and every button
/// is centred in it.
fn assert_single_row(h: &mut Harness, rem: f32, buttons: &[&'static str]) {
    let bar = bounds(h, "schematic-toolbar");
    let expected = TOOLBAR_HEIGHT * rem;
    assert!(
        (f32::from(bar.size.height) - expected).abs() < 0.5,
        "toolbar {:?} tall, expected {expected} px",
        bar.size.height
    );
    let mut heights = Vec::new();
    for &selector in buttons {
        let b = bounds(h, selector);
        let offset = f32::from(b.center().y) - f32::from(bar.center().y);
        // The bottom border takes one pixel of the bar.
        assert!(
            offset.abs() <= 1.0,
            "{selector} is off-centre by {offset} px"
        );
        heights.push(b.size.height);
    }
    assert!(
        heights.windows(2).all(|w| w[0] == w[1]),
        "buttons share one size: {heights:?}"
    );
}

#[gpui_kit::test]
fn the_toolbar_is_as_tall_as_the_waveform_viewer_toolbar(cx: &mut TestAppContext) {
    assert_eq!(TOOLBAR_HEIGHT, viewer_toolbar_height());
    let mut h = open(cx, two_resistors());
    assert_single_row(&mut h, 16.0, &BUTTONS);
    let canvas = bounds(&mut h, "schematic-canvas");
    let bar = bounds(&mut h, "schematic-toolbar");
    assert_eq!(
        canvas.top(),
        bar.bottom(),
        "the canvas starts below the bar"
    );
    h.close();
}

#[gpui_kit::test]
fn the_toolbar_height_follows_the_ui_scale(cx: &mut TestAppContext) {
    let mut h = open(cx, two_resistors());
    for scale in [0.75, 1.25] {
        set_ui_scale(&mut h, scale);
        assert_single_row(&mut h, 16.0 * scale, &BUTTONS);
    }
    h.close();
}

#[gpui_kit::test]
fn the_toolbar_height_follows_large_ui_scales(cx: &mut TestAppContext) {
    // Wide enough that every button still fits on one row at 200 %.
    let window = size(px(2600.0), px(900.0));
    let mut h = open_sized(cx, two_resistors(), fixed_view(), window);
    for scale in [1.5, 1.75, 2.0] {
        set_ui_scale(&mut h, scale);
        assert_single_row(&mut h, 16.0 * scale, &BUTTONS);
    }
    h.close();
}

#[gpui_kit::test]
fn a_read_only_toolbar_keeps_the_height(cx: &mut TestAppContext) {
    let mut h = open(cx, two_resistors());
    h.update(|e, cx| e.set_read_only(true, cx));
    assert_single_row(&mut h, 16.0, &["toolbar-fit"]);
    h.close();
}

#[gpui_kit::test]
fn a_narrow_toolbar_wraps_onto_taller_rows(cx: &mut TestAppContext) {
    let width = px(450.0);
    let mut h = open_sized(cx, two_resistors(), fixed_view(), size(width, px(600.0)));
    let bar = bounds(&mut h, "schematic-toolbar");
    let one_row = TOOLBAR_HEIGHT * 16.0;
    let button = f32::from(bounds(&mut h, "toolbar-select").size.height);
    assert!(
        f32::from(bar.size.height) >= one_row.max(2.0 * button + 8.0),
        "wrapped onto at least two rows: {:?}",
        bar.size
    );
    for selector in BUTTONS {
        let b = bounds(&mut h, selector);
        assert!(b.right() <= width, "{selector} is clipped: {b:?}");
        assert!(
            b.top() >= bar.top() && b.bottom() <= bar.bottom(),
            "{selector} lies inside the bar"
        );
    }
    let first = bounds(&mut h, "toolbar-select");
    let last = bounds(&mut h, "toolbar-fit");
    assert!(
        last.top() > first.top(),
        "the view buttons moved to a later row"
    );
    h.close();
}

/// Every editing button, in toolbar order.
const ALL_BUTTONS: [&str; 20] = [
    "toolbar-select",
    "toolbar-wire",
    "toolbar-label",
    "toolbar-part-v",
    "toolbar-part-i",
    "toolbar-part-r",
    "toolbar-part-l",
    "toolbar-part-c",
    "toolbar-part-d",
    "toolbar-part-q",
    "toolbar-part-qp",
    "toolbar-part-gnd",
    "toolbar-library",
    "toolbar-rotate",
    "toolbar-mirror-h",
    "toolbar-mirror-v",
    "toolbar-delete",
    "toolbar-undo",
    "toolbar-redo",
    "toolbar-fit",
];

#[gpui_kit::test]
fn no_two_buttons_touch(cx: &mut TestAppContext) {
    // The Library button used to touch the Ground button.
    let mut h = open(cx, two_resistors());
    for scale in [1.0, 0.75] {
        set_ui_scale(&mut h, scale);
        let gap = 0.125 * 16.0 * scale;
        for pair in ALL_BUTTONS.windows(2) {
            let (a, b) = (bounds(&mut h, pair[0]), bounds(&mut h, pair[1]));
            assert_eq!(a.top(), b.top(), "{pair:?} share a row at {scale}");
            let space = f32::from(b.left() - a.right());
            assert!(
                space >= gap - 0.01,
                "{} and {} are {space} px apart at UI scale {scale}",
                pair[0],
                pair[1]
            );
        }
    }
    h.close();
}

#[gpui_kit::test]
fn one_row_fits_the_editor_side_of_the_default_window(cx: &mut TestAppContext) {
    // The desktop opens 1360 px wide and gives the schematic half of it
    // while results are shown (crates/desktop: WINDOW_SIZE,
    // DEFAULT_FRACTION), next to the viewer toolbar.
    let window = size(px(680.0), px(700.0));
    let mut h = open_sized(cx, two_resistors(), fixed_view(), window);
    assert_single_row(&mut h, 16.0, &BUTTONS);
    h.close();
}
