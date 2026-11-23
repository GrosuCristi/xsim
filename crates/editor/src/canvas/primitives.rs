//! Low-level anti-aliased drawing on the GPUI window (Q28).

use crate::model::geometry::Vec2;
use gpui_kit::{
    App, BorderStyle, Bounds, FontWeight, Hsla, PathBuilder, Pixels, Point, SharedString,
    StrokeOptions, TextAlign, Window, point, px, quad, size,
};

/// Converts a screen-space vector to a GPUI point.
pub fn to_px(p: Vec2) -> Point<Pixels> {
    point(px(p.x), px(p.y))
}

/// Strokes an open polyline.
pub fn polyline(window: &mut Window, points: &[Vec2], width: f32, color: Hsla) {
    let Some((first, rest)) = points.split_first() else {
        return;
    };
    if rest.is_empty() {
        return;
    }
    let mut path = stroke_builder(width);
    path.move_to(to_px(*first));
    for p in rest {
        path.line_to(to_px(*p));
    }
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

/// Strokes a dashed open polyline.
pub fn dashed_polyline(window: &mut Window, points: &[Vec2], width: f32, dash: f32, color: Hsla) {
    let Some((first, rest)) = points.split_first() else {
        return;
    };
    let mut path = stroke_builder(width).dash_array(&[px(dash), px(dash)]);
    path.move_to(to_px(*first));
    for p in rest {
        path.line_to(to_px(*p));
    }
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

/// Fills a closed polygon.
pub fn polygon(window: &mut Window, points: &[Vec2], color: Hsla) {
    if points.len() < 3 {
        return;
    }
    let pixels: Vec<Point<Pixels>> = points.iter().map(|p| to_px(*p)).collect();
    let mut path = PathBuilder::fill();
    path.add_polygon(&pixels, true);
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

/// Strokes a circle outline.
pub fn circle(window: &mut Window, center: Vec2, radius: f32, width: f32, color: Hsla) {
    let r = point(px(radius), px(radius));
    let mut path = stroke_builder(width);
    path.move_to(to_px(center + Vec2::new(radius, 0.0)));
    path.arc_to(
        r,
        px(0.0),
        false,
        true,
        to_px(center - Vec2::new(radius, 0.0)),
    );
    path.arc_to(
        r,
        px(0.0),
        false,
        true,
        to_px(center + Vec2::new(radius, 0.0)),
    );
    path.close();
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

/// Fills a disc (junction dots, pin markers).
pub fn disc(window: &mut Window, center: Vec2, radius: f32, color: Hsla) {
    let r = px(radius);
    let bounds = Bounds::new(
        point(px(center.x) - r, px(center.y) - r),
        size(r * 2.0, r * 2.0),
    );
    window.paint_quad(quad(bounds, r, color, px(0.0), color, BorderStyle::Solid));
}

/// Fills a rectangle with an optional border.
pub fn rect(window: &mut Window, min: Vec2, max: Vec2, fill: Hsla, border: Option<Hsla>) {
    let bounds = Bounds::new(to_px(min), size(px(max.x - min.x), px(max.y - min.y)));
    let (width, color) = border.map_or((px(0.0), fill), |c| (px(1.0), c));
    window.paint_quad(quad(
        bounds,
        px(0.0),
        fill,
        width,
        color,
        BorderStyle::Solid,
    ));
}

fn stroke_builder(width: f32) -> PathBuilder {
    PathBuilder::stroke(px(width)).with_style(gpui_kit::PathStyle::Stroke(
        StrokeOptions::default()
            .with_line_width(width)
            .with_tolerance(0.05),
    ))
}

/// Horizontal placement of a text line relative to its anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// A single line of text to draw.
pub struct Text<'a> {
    pub content: &'a str,
    pub size: f32,
    pub color: Hsla,
    pub weight: FontWeight,
}

/// Width of `text` in pixels.
pub fn text_width(window: &mut Window, text: &Text<'_>) -> f32 {
    let line = shape(window, text);
    f32::from(line.width())
}

/// Draws a text line whose vertical centre is at `anchor.y`.
pub fn text(window: &mut Window, cx: &mut App, text: &Text<'_>, anchor: Vec2, align: Align) {
    let line = shape(window, text);
    let width = f32::from(line.width());
    let x = match align {
        Align::Left => anchor.x,
        Align::Center => anchor.x - width * 0.5,
        Align::Right => anchor.x - width,
    };
    let line_height = text.size * 1.25;
    let origin = point(px(x), px(anchor.y - line_height * 0.5));
    // A failed glyph upload only loses this frame's text; nothing to recover.
    let _ = line.paint(origin, px(line_height), TextAlign::Left, None, window, cx);
}

fn shape(window: &mut Window, text: &Text<'_>) -> gpui_kit::ShapedLine {
    let content: SharedString = text.content.replace(['\r', '\n'], " ").into();
    let mut run = window.text_style().to_run(content.len());
    run.color = text.color;
    run.font.weight = text.weight;
    window
        .text_system()
        .shape_line(content, px(text.size), &[run], None)
}
