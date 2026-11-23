//! Schematic symbol artwork, shared by the canvas and the library preview.
//!
//! The shapes live in [`shapes`] (symbol-local grid units, free of GPUI);
//! this module paints them, and [`svg`] renders them as toolbar icons.

pub(crate) mod shapes;
pub(crate) mod svg;

use crate::canvas::primitives::{circle, polygon, polyline};
use crate::model::geometry::Vec2;
use gpui_kit::{component::ActiveTheme, *};
use shapes::{Shape, extent};
use xsim_project::PartKind;

/// Pen width relative to one grid unit.
const STROKE_RATIO: f32 = 0.085;

/// How symbol-local points map to the screen.
pub(crate) struct SymbolPlacement<'a> {
    /// Local grid point → screen pixel.
    pub map: &'a dyn Fn(Vec2) -> Vec2,
    /// Pixels per grid unit.
    pub scale: f32,
}

/// Pen width in pixels for symbols drawn at `scale` pixels per grid unit.
pub(crate) fn stroke_width(scale: f32) -> f32 {
    (scale * STROKE_RATIO).clamp(1.0, 3.2)
}

/// Paints the symbol of `kind`.
pub(crate) fn paint_symbol(
    window: &mut Window,
    kind: PartKind,
    placement: &SymbolPlacement<'_>,
    color: Hsla,
) {
    let width = stroke_width(placement.scale);
    for shape in shapes::shapes(kind) {
        paint_shape(window, &shape, placement, width, color);
    }
}

fn paint_shape(
    window: &mut Window,
    shape: &Shape,
    placement: &SymbolPlacement<'_>,
    width: f32,
    color: Hsla,
) {
    let map = placement.map;
    match shape {
        Shape::Line(points) => {
            let screen: Vec<Vec2> = points.iter().map(|&p| map(p)).collect();
            polyline(window, &screen, width, color);
        }
        Shape::Fill(points) => {
            let screen: Vec<Vec2> = points.iter().map(|&p| map(p)).collect();
            polygon(window, &screen, color);
            let mut closed = screen.clone();
            closed.extend(screen.first().copied());
            polyline(window, &closed, width * 0.6, color);
        }
        Shape::Circle { center, radius } => {
            circle(window, map(*center), radius * placement.scale, width, color);
        }
        Shape::Sign { at, plus } => {
            paint_sign(window, map(*at), *plus, placement.scale, width, color)
        }
    }
}

/// `+` or `−`, upright on screen.
fn paint_sign(window: &mut Window, at: Vec2, plus: bool, scale: f32, width: f32, color: Hsla) {
    let arm = 0.2 * scale;
    let w = width * 0.85;
    polyline(
        window,
        &[at - Vec2::new(arm, 0.0), at + Vec2::new(arm, 0.0)],
        w,
        color,
    );
    if plus {
        polyline(
            window,
            &[at - Vec2::new(0.0, arm), at + Vec2::new(0.0, arm)],
            w,
            color,
        );
    }
}

/// Paints `kind` scaled to fit and centred in `bounds`, leaving `padding`
/// pixels free on every side.
pub(crate) fn paint_fitted(
    window: &mut Window,
    kind: PartKind,
    bounds: Bounds<Pixels>,
    padding: f32,
    color: Hsla,
) {
    let art = shapes::shapes(kind);
    let Some(local) = extent(&art) else {
        return;
    };
    let width = f32::from(bounds.size.width) - 2.0 * padding;
    let height = f32::from(bounds.size.height) - 2.0 * padding;
    if width <= 0.0 || height <= 0.0 {
        return;
    }
    let scale = (width / local.width().max(0.1)).min(height / local.height().max(0.1));
    let origin = Vec2::new(f32::from(bounds.origin.x), f32::from(bounds.origin.y));
    let screen_center =
        origin + Vec2::new(f32::from(bounds.size.width), f32::from(bounds.size.height)) * 0.5;
    let local_center = local.center();
    let map = move |p: Vec2| screen_center + (p - local_center) * scale;
    let placement = SymbolPlacement { map: &map, scale };
    for shape in &art {
        paint_shape(
            window,
            shape,
            &placement,
            stroke_width(scale).min(scale * 0.2).max(1.0),
            color,
        );
    }
}

/// An element that draws the schematic symbol of `kind`, scaled to fit and
/// centred in its bounds, in the theme's foreground colour. Give it a size,
/// e.g. `div().size(px(96.)).child(symbol_preview(kind))`.
pub fn symbol_preview(kind: PartKind) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, cx| {
            let color = cx.theme().foreground;
            let padding = (f32::from(bounds.size.width.min(bounds.size.height)) * 0.08).max(1.0);
            paint_fitted(window, kind, bounds, padding, color);
        },
    )
    .size_full()
}
