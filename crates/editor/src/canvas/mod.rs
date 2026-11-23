//! The schematic canvas element, painted with GPUI's own APIs (Q28).
//!
//! Each frame the editor builds a [`Scene`]; the element measures the
//! viewport, resolves the camera (fitting the drawing when asked), registers
//! the mouse listeners and paints the layers back to front.

pub(crate) mod grid;
mod labels;
mod overlay;
pub(crate) mod palette;
mod parts;
pub(crate) mod primitives;
pub(crate) mod scene;
mod wires;

use crate::SchematicEditor;
use crate::camera::{Camera, Viewport};
use crate::model::geometry::Vec2;
use crate::ui::input;
use gpui_kit::{
    App, Bounds, Hitbox, HitboxBehavior, IntoElement, Pixels, Styled, WeakEntity, Window, canvas,
};
use scene::{AutoFit, Frame, Scene};
use std::cell::Cell;
use std::rc::Rc;

/// The last measured viewport, shared with the editor's event handlers.
pub(crate) type ViewportCell = Rc<Cell<Option<Viewport>>>;

/// State carried from prepaint to paint.
struct Prepared {
    scene: Scene,
    frame: Frame,
    hitbox: Hitbox,
}

/// The canvas element for one frame.
pub(crate) fn schematic_canvas(
    scene: Scene,
    editor: WeakEntity<SchematicEditor>,
    viewport_cell: ViewportCell,
) -> impl IntoElement {
    let for_paint = editor.clone();
    canvas(
        move |bounds, window, cx| prepare(scene, bounds, &editor, &viewport_cell, window, cx),
        move |_, prepared, window, cx| paint(prepared, &for_paint, window, cx),
    )
    .size_full()
}

/// The viewport of `bounds` at the window's UI scale (rem size / 16 px).
fn viewport_of(bounds: Bounds<Pixels>, window: &Window) -> Viewport {
    Viewport {
        origin: Vec2::new(f32::from(bounds.origin.x), f32::from(bounds.origin.y)),
        size: Vec2::new(f32::from(bounds.size.width), f32::from(bounds.size.height)),
        ui_scale: f32::from(window.rem_size()) / 16.0,
    }
}

fn prepare(
    scene: Scene,
    bounds: Bounds<Pixels>,
    editor: &WeakEntity<SchematicEditor>,
    viewport_cell: &ViewportCell,
    window: &mut Window,
    cx: &mut App,
) -> Prepared {
    let viewport = viewport_of(bounds, window);
    let resized = viewport_cell
        .get()
        .is_none_or(|old| old.size != viewport.size || old.ui_scale != viewport.ui_scale);
    viewport_cell.set(Some(viewport));
    let fit = match scene.auto_fit {
        AutoFit::Now(extent) => Some(extent),
        AutoFit::OnResize(extent) if resized => Some(extent),
        _ => None,
    };
    let camera = match fit {
        Some(extent) if !viewport.is_empty() => {
            let camera = Camera::fit(extent, &viewport);
            let editor = editor.clone();
            cx.defer(move |cx| {
                editor
                    .update(cx, |editor, cx| editor.finish_auto_fit(camera, cx))
                    .ok();
            });
            camera
        }
        _ => scene.camera,
    };
    let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
    Prepared {
        scene,
        frame: Frame { camera, viewport },
        hitbox,
    }
}

fn paint(
    prepared: Prepared,
    editor: &WeakEntity<SchematicEditor>,
    window: &mut Window,
    cx: &mut App,
) {
    let Prepared {
        scene,
        frame,
        hitbox,
    } = prepared;
    input::register_listeners(window, &hitbox, editor.clone());
    window.set_cursor_style(scene.cursor, &hitbox);
    grid::paint_grid(window, &frame, &scene.palette);
    wires::paint_wire_halos(window, &frame, &scene);
    parts::paint_part_halos(window, &frame, &scene);
    wires::paint_net_highlight(window, &frame, &scene);
    wires::paint_wires(window, &frame, &scene);
    parts::paint_parts(window, cx, &frame, &scene);
    wires::paint_junctions(window, &frame, &scene);
    labels::paint_labels(window, cx, &frame, &scene);
    overlay::paint_overlays(window, &frame, &scene);
}
