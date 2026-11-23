//! Interaction feedback drawn above the schematic: the placement ghost, the
//! wire rubber band, the selection box and the probe glyph.

use crate::canvas::parts::paint_part_symbol;
use crate::canvas::primitives::{dashed_polyline, disc, polygon, polyline, rect};
use crate::canvas::scene::{Frame, Scene};
use crate::model::geometry::Vec2;
use crate::model::hit_test::BoxMode;
use gpui_kit::{Hsla, Window};

/// Paints every overlay of the scene.
pub fn paint_overlays(window: &mut Window, frame: &Frame, scene: &Scene) {
    if let Some(ghost) = scene.ghost {
        paint_part_symbol(
            window,
            frame,
            ghost.kind,
            ghost.position,
            ghost.orientation,
            scene.palette.ghost,
        );
    }
    paint_wire_preview(window, frame, scene);
    if let Some(selection) = scene.box_selection {
        let (min, max) = (
            frame.screen(selection.rect.min),
            frame.screen(selection.rect.max),
        );
        rect(window, min, max, scene.palette.box_fill, None);
        let outline = [
            min,
            Vec2::new(max.x, min.y),
            max,
            Vec2::new(min.x, max.y),
            min,
        ];
        match selection.mode {
            BoxMode::Enclosed => polyline(window, &outline, 1.0, scene.palette.selection),
            BoxMode::Touching => {
                dashed_polyline(window, &outline, 1.0, 4.0, scene.palette.selection)
            }
        }
    }
    if let Some(at) = scene.probe_glyph {
        paint_probe(
            window,
            frame.screen(at),
            scene.palette.probe,
            scene.palette.symbol,
        );
    }
}

fn paint_wire_preview(window: &mut Window, frame: &Frame, scene: &Scene) {
    let points: Vec<Vec2> = scene.wire_preview.iter().map(|&p| frame.grid(p)).collect();
    let Some(&end) = points.last() else {
        return;
    };
    polyline(window, &points, frame.wire_width(), scene.palette.selection);
    let radius = frame.junction_radius() * 0.8;
    if let Some(&start) = points.first() {
        disc(window, start, radius, scene.palette.selection);
    }
    disc(window, end, radius, scene.palette.selection);
}

/// A small test probe whose tip is at `tip`.
pub fn paint_probe(window: &mut Window, tip: Vec2, body: Hsla, outline: Hsla) {
    let diagonal = std::f32::consts::FRAC_1_SQRT_2;
    let dir = Vec2::new(diagonal, -diagonal);
    let normal = Vec2::new(diagonal, diagonal);
    let needle_end = tip + dir * 7.0;
    polyline(window, &[tip, needle_end], 1.6, outline);
    let handle_end = needle_end + dir * 13.0;
    let shape = [
        needle_end + normal * 2.0,
        handle_end + normal * 3.5,
        handle_end - normal * 3.5,
        needle_end - normal * 2.0,
    ];
    polygon(window, &shape, body);
    let mut closed = shape.to_vec();
    closed.push(shape[0]);
    polyline(window, &closed, 1.0, outline);
}
