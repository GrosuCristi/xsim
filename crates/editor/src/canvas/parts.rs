//! Part symbols, their open-pin markers and their designator/value text.

use crate::canvas::primitives::{Align, Text, circle, rect, text};
use crate::canvas::scene::{Frame, Scene};
use crate::model::geometry::{Rect, Vec2};
use crate::model::text_layout::{TextSide, place_text};
use crate::symbol_art::shapes::oriented;
use crate::symbol_art::{SymbolPlacement, paint_symbol};
use gpui_kit::{App, FontWeight, Hsla, Window};
use xsim_project::{Component, Connectivity, Orientation, PartKind, Point};

/// Paints the symbol of `kind` at `position` in `orientation`.
pub fn paint_part_symbol(
    window: &mut Window,
    frame: &Frame,
    kind: PartKind,
    position: Point,
    orientation: Orientation,
    color: Hsla,
) {
    let origin = Vec2::from_grid(position);
    let map = |local: Vec2| frame.screen(origin + oriented(orientation, local));
    let placement = SymbolPlacement {
        map: &map,
        scale: frame.scale(),
    };
    paint_symbol(window, kind, &placement, color);
}

/// Halos behind selected and hovered parts.
pub fn paint_part_halos(window: &mut Window, frame: &Frame, scene: &Scene) {
    for c in &scene.schematic.components {
        let selected = scene.selection.has_component(c.id);
        let hovered = scene.hovered == Some(crate::model::selection::ObjectRef::Component(c.id));
        if !selected && !hovered {
            continue;
        }
        let (a, b) = c.bounds();
        let r = Rect::from_corners(Vec2::from_grid(a), Vec2::from_grid(b)).inflate(0.35, 0.35);
        let (min, max) = (frame.screen(r.min), frame.screen(r.max));
        let (fill, border) = if selected {
            (scene.palette.selection_halo, Some(scene.palette.selection))
        } else {
            (scene.palette.hover_halo, None)
        };
        rect(window, min, max, fill, border);
    }
}

/// Symbols, open-pin markers and text of every part.
pub fn paint_parts(window: &mut Window, cx: &mut App, frame: &Frame, scene: &Scene) {
    for c in &scene.schematic.components {
        let color = if scene.selection.has_component(c.id) {
            scene.palette.selection
        } else {
            scene.palette.symbol
        };
        paint_part_symbol(
            window,
            frame,
            c.part.kind(),
            c.position,
            c.orientation,
            color,
        );
        paint_open_pins(
            window,
            frame,
            &scene.connectivity,
            c,
            scene.palette.pin_marker,
        );
        if frame.shows_text() {
            paint_part_text(window, cx, frame, c, scene);
        }
    }
}

/// A small ring on every pin nothing is connected to.
fn paint_open_pins(
    window: &mut Window,
    frame: &Frame,
    connectivity: &Connectivity,
    c: &Component,
    color: Hsla,
) {
    let radius = (frame.scale() * 0.16).clamp(2.0, 4.5);
    for (pin, p) in c.pins().into_iter().enumerate() {
        let open = connectivity
            .net_of_pin(c.id, pin)
            .and_then(|n| connectivity.nets.get(n))
            .is_none_or(|net| net.pins.len() <= 1 && net.wires.is_empty() && net.points.len() <= 1);
        if open {
            circle(window, frame.grid(p), radius, 1.0, color);
        }
    }
}

/// Designator and value, horizontal and outside the body.
fn paint_part_text(window: &mut Window, cx: &mut App, frame: &Frame, c: &Component, scene: &Scene) {
    if c.part.kind() == PartKind::Ground {
        return;
    }
    let size = (frame.scale() * 0.6).clamp(9.0, 15.0);
    let summary = c.part.summary();
    let mut lines = vec![Text {
        content: &c.name,
        size,
        color: scene.palette.designator,
        weight: FontWeight::SEMIBOLD,
    }];
    if !summary.is_empty() {
        lines.push(Text {
            content: &summary,
            size: size * 0.92,
            color: scene.palette.value,
            weight: FontWeight::NORMAL,
        });
    }
    let placement = place_text(c);
    let anchor = frame.screen(placement.anchor);
    let line_height = size * 1.25;
    let count = lines.len() as f32;
    for (i, line) in lines.iter().enumerate() {
        let i = i as f32;
        let (dy, align) = match placement.side {
            TextSide::Right => ((i - (count - 1.0) * 0.5) * line_height, Align::Left),
            TextSide::Left => ((i - (count - 1.0) * 0.5) * line_height, Align::Right),
            TextSide::Above => (-(count - i - 0.5) * line_height, Align::Center),
            TextSide::Below => ((i + 0.5) * line_height, Align::Center),
        };
        text(window, cx, line, anchor + Vec2::new(0.0, dy), align);
    }
}
