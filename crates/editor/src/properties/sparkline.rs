//! A small anti-aliased line chart of a waveform preview.

use super::preview;
use gpui_kit::{
    Bounds, Hsla, IntoElement, PathBuilder, Pixels, Point, Styled as _, Window, canvas, point,
};
use std::rc::Rc;

/// An element drawing `points` (time, value) across exactly its bounds in
/// `line`: the first point's time on the left edge, the last point's on the
/// right, the largest value on the top edge and the smallest on the bottom
/// (see [`preview::level`]). A zero line in `axis` marks zero when it is
/// within range. Give it a size.
pub(crate) fn sparkline(points: Rc<Vec<(f64, f64)>>, line: Hsla, axis: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| paint(&points, bounds, line, axis, window),
    )
    .size_full()
}

fn paint(
    points: &[(f64, f64)],
    bounds: Bounds<Pixels>,
    line: Hsla,
    axis: Hsla,
    window: &mut Window,
) {
    let (Some(&(start, _)), Some(&(end, _))) = (points.first(), points.last()) else {
        return;
    };
    let Some((lo, hi)) = preview::extremes(points) else {
        return;
    };
    let duration = end - start;
    let to_screen = |(t, v): (f64, f64)| {
        let x = if duration > 0.0 {
            (t - start) / duration
        } else {
            0.0
        };
        let y = preview::level(v, lo, hi);
        point(
            bounds.origin.x + bounds.size.width * x as f32,
            bounds.origin.y + bounds.size.height * y as f32,
        )
    };
    if lo <= 0.0 && hi >= 0.0 && hi > lo {
        let left = to_screen((start, 0.0));
        let right = point(bounds.origin.x + bounds.size.width, left.y);
        stroke(window, [left, right], axis, AXIS_WIDTH);
    }
    let curve = points
        .iter()
        .filter(|p| p.1.is_finite())
        .map(|&p| to_screen(p));
    stroke(window, curve, line, LINE_WIDTH);
}

/// Stroke widths in rems, so the lines thicken with the UI scale (1 px and
/// 1.5 px at 100%).
const AXIS_WIDTH: f32 = 1. / 16.;
const LINE_WIDTH: f32 = 1.5 / 16.;

fn stroke(
    window: &mut Window,
    points: impl IntoIterator<Item = Point<Pixels>>,
    color: Hsla,
    width: f32,
) {
    let mut points = points.into_iter();
    let Some(start) = points.next() else {
        return;
    };
    let mut path = PathBuilder::stroke(window.rem_size() * width);
    path.move_to(start);
    for p in points {
        path.line_to(p);
    }
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}
