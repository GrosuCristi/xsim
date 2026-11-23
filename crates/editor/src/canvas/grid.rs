//! The dotted grid: a dot on every grid unit at the default zoom, thinned
//! out as the view zooms out. Parts, pins, wires and labels snap to every
//! grid unit, so every dot is a snap point (D22) and every dot looks the
//! same. The spacing thresholds are in screen pixels (see [`dot_step`]),
//! like the dots themselves.

use crate::camera::dot_step;
use crate::canvas::palette::Palette;
use crate::canvas::scene::Frame;
use crate::model::geometry::Vec2;
use gpui_kit::{Bounds, Window, fill, point, px, size};
use xsim_project::Point;

/// The dots painted in one frame, on grid points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DotGrid {
    /// Grid units between neighbouring dots (1 = on every grid unit).
    pub step: i32,
    /// First and last dot column, in grid units.
    pub columns: (i32, i32),
    /// First and last dot row, in grid units.
    pub rows: (i32, i32),
}

impl DotGrid {
    /// The dots covering the visible area of `frame`.
    pub fn of(frame: &Frame) -> Self {
        let step = dot_step(frame.scale());
        let visible = frame.camera.visible(&frame.viewport);
        // On the dot lattice, at or before the visible edge.
        let first = |v: f32| ((v / step as f32).floor() as i32).saturating_mul(step);
        let last = |v: f32| v.ceil() as i32;
        Self {
            step,
            columns: (first(visible.min.x), last(visible.max.x)),
            rows: (first(visible.min.y), last(visible.max.y)),
        }
    }

    /// Distance between neighbouring dots, in grid units.
    pub fn pitch(&self) -> f32 {
        self.step as f32
    }

    /// Whether there is a dot on every grid point, so on every point that
    /// parts, wires and labels snap to.
    #[cfg(test)]
    pub fn on_every_grid_point(&self) -> bool {
        self.step == 1
    }

    /// The grid point of every dot, column by column.
    pub fn dots(&self) -> impl Iterator<Item = Point> + use<> {
        let step = usize::try_from(self.step).unwrap_or(1).max(1);
        let rows = self.rows;
        (self.columns.0..=self.columns.1)
            .step_by(step)
            .flat_map(move |x| {
                (rows.0..=rows.1)
                    .step_by(step)
                    .map(move |y| Point::new(x, y))
            })
    }
}

/// Side of a grid dot in window pixels at `pixels_per_unit`.
fn dot_size(grid: &DotGrid, pixels_per_unit: f32) -> f32 {
    if pixels_per_unit * grid.pitch() > 24.0 {
        2.0
    } else {
        1.5
    }
}

/// A usable display scale factor.
fn sanitized(scale_factor: f32) -> f32 {
    if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    }
}

/// Side in device pixels of the dots of `grid`, about [`dot_size`]. Halves
/// round down, as GPUI rounds them: a 1.5 px dot is one device pixel at
/// 100 %, two at 125 % and 150 %, three at 175 % and 200 %.
fn device_side(grid: &DotGrid, pixels_per_unit: f32, scale_factor: f32) -> f32 {
    let side = dot_size(grid, pixels_per_unit);
    (side * sanitized(scale_factor) - 0.5).ceil().max(1.0)
}

/// Top-left corner and side, in window pixels, of a square dot
/// `device_side` device pixels wide centred on `center`, on whole device
/// pixels.
///
/// GPUI rounds each edge of a quad to a device pixel on its own and does not
/// antialias square quads, so a dot 1.5 px wide at a fractional position
/// would cover one or two pixels depending on where it falls. The dot pitch
/// is often fractional (7.5 px at 75 % UI, 12.5 device px at 125 % display
/// scale), which would make neighbouring dots alternate in size.
fn dot_square(center: Vec2, device_side: f32, scale_factor: f32) -> (Vec2, f32) {
    let scale_factor = sanitized(scale_factor);
    let corner = |c: f32| (c * scale_factor - device_side * 0.5).round() / scale_factor;
    (
        Vec2::new(corner(center.x), corner(center.y)),
        device_side / scale_factor,
    )
}

/// Paints grid dots over the visible area, in the palette's grid colour.
pub fn paint_grid(window: &mut Window, frame: &Frame, palette: &Palette) {
    let grid = DotGrid::of(frame);
    let scale_factor = window.scale_factor();
    let side = device_side(&grid, frame.scale(), scale_factor);
    for dot in grid.dots() {
        let (corner, side) = dot_square(frame.grid(dot), side, scale_factor);
        let bounds = Bounds::new(point(px(corner.x), px(corner.y)), size(px(side), px(side)));
        window.paint_quad(fill(bounds, palette.grid));
    }
}

#[cfg(test)]
mod tests {
    use super::{DotGrid, device_side, dot_size, dot_square};
    use crate::camera::{
        Camera, MAX_ZOOM, MIN_DOT_SPACING, MIN_FINE_DOT_SPACING, MIN_ZOOM, Viewport,
    };
    use crate::canvas::scene::Frame;
    use crate::model::geometry::Vec2;
    use std::collections::BTreeSet;
    use xsim_project::{Point, SchematicView};

    /// The interface scales the settings offer.
    const UI_SCALES: [f32; 8] = [0.75, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0];

    fn frame(zoom: f32, ui_scale: f32) -> Frame {
        Frame {
            camera: Camera {
                center: Vec2::new(6.6, -3.4),
                zoom,
            },
            viewport: Viewport {
                origin: Vec2::new(40.0, 60.0),
                size: Vec2::new(900.0, 600.0),
                ui_scale,
            },
        }
    }

    fn default_zoom() -> f32 {
        SchematicView::default().zoom
    }

    #[test]
    fn the_default_zoom_has_a_dot_on_every_grid_unit_at_every_ui_scale() {
        for ui_scale in UI_SCALES {
            let grid = DotGrid::of(&frame(default_zoom(), ui_scale));
            assert_eq!(grid.pitch(), 1.0, "UI scale {ui_scale}");
            assert!(grid.on_every_grid_point(), "UI scale {ui_scale}");
        }
    }

    #[test]
    fn the_default_grid_has_a_dot_every_10_pixels() {
        // D14's density: twice the 20 px of the grid before it.
        let f = frame(default_zoom(), 1.0);
        assert_eq!(DotGrid::of(&f).pitch() * f.scale(), 10.0);
        let grid = DotGrid::of(&f);
        let span = 0..=20;
        let across = grid
            .dots()
            .filter(|d| d.y == 0 && span.contains(&d.x))
            .count();
        let down = grid
            .dots()
            .filter(|d| d.x == 0 && span.contains(&-d.y))
            .count();
        assert_eq!((across, down), (21, 21));
    }

    #[test]
    fn every_visible_grid_point_has_a_dot() {
        let f = frame(default_zoom(), 1.0);
        let grid = DotGrid::of(&f);
        let dots: BTreeSet<Point> = grid.dots().collect();
        let visible = f.camera.visible(&f.viewport);
        let (x0, x1) = (visible.min.x.ceil() as i32, visible.max.x.floor() as i32);
        let (y0, y1) = (visible.min.y.ceil() as i32, visible.max.y.floor() as i32);
        for x in x0..=x1 {
            for y in y0..=y1 {
                assert!(dots.contains(&Point::new(x, y)), "no dot at ({x}, {y})");
            }
        }
        // Odd points too, which only half a schema-2 unit can reach.
        for (x, y) in [(1, 0), (7, -3), (-9, 5)] {
            assert!(dots.contains(&Point::new(x, y)), "no dot at ({x}, {y})");
        }
    }

    #[test]
    fn the_dots_cover_the_visible_area() {
        for zoom in [MIN_ZOOM, 2.5, 6.0, default_zoom(), 25.0, MAX_ZOOM] {
            let f = frame(zoom, 1.25);
            let visible = f.camera.visible(&f.viewport);
            let grid = DotGrid::of(&f);
            let (min_x, max_x) = (grid.columns.0 as f32, grid.columns.1 as f32);
            let (min_y, max_y) = (grid.rows.0 as f32, grid.rows.1 as f32);
            assert!(
                min_x <= visible.min.x && min_y <= visible.min.y,
                "zoom {zoom}"
            );
            assert!(
                max_x >= visible.max.x && max_y >= visible.max.y,
                "zoom {zoom}"
            );
            assert_eq!(grid.columns.0 % grid.step, 0, "columns on the lattice");
            assert_eq!(grid.rows.0 % grid.step, 0, "rows on the lattice");
        }
    }

    #[test]
    fn zooming_out_thins_the_dots_without_crowding_them() {
        for ui_scale in UI_SCALES {
            let mut previous = 1.0;
            let mut zoom = MAX_ZOOM;
            while zoom >= MIN_ZOOM {
                let f = frame(zoom, ui_scale);
                let grid = DotGrid::of(&f);
                let spacing = grid.pitch() * f.scale();
                let minimum = if grid.on_every_grid_point() {
                    MIN_FINE_DOT_SPACING
                } else {
                    MIN_DOT_SPACING
                };
                assert!(
                    spacing >= minimum,
                    "zoom {zoom} at UI scale {ui_scale}: dots {spacing} px apart"
                );
                assert!(grid.pitch() >= previous, "zoom {zoom}: pitch shrank");
                previous = grid.pitch();
                zoom *= 0.95;
            }
            assert!(previous > 2.0, "fully zoomed out, the grid has thinned");
        }
    }

    #[test]
    fn a_far_away_camera_paints_few_dots_without_overflowing() {
        let mut f = frame(MIN_ZOOM, 1.0);
        f.camera.center = Vec2::new(1e12, -1e12);
        let grid = DotGrid::of(&f);
        assert!(grid.dots().count() <= 4, "{grid:?}");
    }

    /// What GPUI makes of a filled square: each edge is rounded to a device
    /// pixel on its own (half toward zero) and the quad is not antialiased.
    /// Returns the covered device pixels as (left, top, width, height).
    fn rasterized(corner: Vec2, side: f32, scale_factor: f32) -> (i32, i32, i32, i32) {
        let edge = |v: f32| {
            let d = v * scale_factor;
            let rounded = if (d - d.trunc()).abs() == 0.5 {
                d.trunc()
            } else {
                d.round()
            };
            rounded as i32
        };
        let (left, top) = (edge(corner.x), edge(corner.y));
        let (right, bottom) = (edge(corner.x + side), edge(corner.y + side));
        (left, top, right - left, bottom - top)
    }

    /// Windows display scales (100 % to 200 %).
    const DISPLAY_SCALES: [f32; 5] = [1.0, 1.25, 1.5, 1.75, 2.0];

    #[test]
    fn every_dot_covers_the_same_device_pixels() {
        // The dot pitch is often a fractional number of device pixels
        // (10 px at 125 % is 12.5): dots must not alternate in size.
        for scale_factor in DISPLAY_SCALES {
            for (zoom, ui_scale) in [
                (default_zoom(), 1.0),
                (default_zoom(), 0.75),
                (8.65, 1.1),
                (5.0, 1.0),
                (30.0, 1.0),
                (38.85, 1.25),
            ] {
                let f = frame(zoom, ui_scale);
                let grid = DotGrid::of(&f);
                if zoom >= 30.0 {
                    assert_eq!(dot_size(&grid, f.scale()), 2.0, "zoom {zoom}: larger dots");
                }
                let side = device_side(&grid, f.scale(), scale_factor);
                let sizes: BTreeSet<(i32, i32)> = grid
                    .dots()
                    .map(|dot| {
                        let (corner, s) = dot_square(f.grid(dot), side, scale_factor);
                        let (_, _, w, h) = rasterized(corner, s, scale_factor);
                        (w, h)
                    })
                    .collect();
                let context = format!("display {scale_factor}, zoom {zoom}");
                assert_eq!(sizes.len(), 1, "{context}: {sizes:?}");
                let (w, h) = sizes.first().copied().unwrap_or_default();
                assert!(w >= 1 && w == h, "{context}: {w}×{h}");
                assert_eq!(w as f32, side, "{context}: whole device pixels");
            }
        }
    }

    #[test]
    fn dots_have_the_size_snap_dots_had_before() {
        // A dot every 10 px is one device pixel at 100 %, about 1.5 px at
        // every display scale.
        for scale_factor in DISPLAY_SCALES {
            for ui_scale in UI_SCALES {
                let f = frame(default_zoom(), ui_scale);
                let grid = DotGrid::of(&f);
                let side = device_side(&grid, f.scale(), scale_factor) / scale_factor;
                assert!(
                    (side - 1.5).abs() <= 0.5 / scale_factor,
                    "display {scale_factor}, UI {ui_scale}: {side}"
                );
            }
        }
        let f = frame(default_zoom(), 1.0);
        assert_eq!(device_side(&DotGrid::of(&f), f.scale(), 1.0), 1.0);
    }

    #[test]
    fn dots_stay_centred_on_their_grid_points() {
        for scale_factor in DISPLAY_SCALES {
            let f = frame(default_zoom(), 1.0);
            let grid = DotGrid::of(&f);
            let device = device_side(&grid, f.scale(), scale_factor);
            for dot in grid.dots().take(500) {
                let center = f.grid(dot);
                let (corner, side) = dot_square(center, device, scale_factor);
                let middle = Vec2::new(corner.x + side * 0.5, corner.y + side * 0.5);
                assert!(
                    middle.distance(center) <= 0.75 / scale_factor,
                    "display {scale_factor}: {middle:?} vs {center:?}"
                );
            }
        }
    }

    #[test]
    fn the_thinning_is_decided_on_screen_pixels() {
        // Dots are a fixed size on screen, so their screen spacing decides.
        let pitch = |zoom: f32, ui_scale: f32| DotGrid::of(&frame(zoom, ui_scale)).pitch();
        assert_eq!(pitch(default_zoom(), 0.75), 1.0, "7.5 px apart");
        assert_eq!(pitch(9.0, 0.75), 2.0, "every unit would be 6.75 px apart");
        assert_eq!(pitch(6.5, 1.0), 2.0);
        assert_eq!(pitch(4.0, 2.0), 1.0, "8 px apart");
        assert_eq!(pitch(MIN_ZOOM, 2.0), 4.0, "12 px apart");
    }
}
