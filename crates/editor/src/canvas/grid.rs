//! The dotted grid: a dot every half grid unit at the default zoom, thinned
//! out as the view zooms out (D14). Only the drawing is denser: parts, pins
//! and wires still snap to whole grid units. While the half-unit dots show,
//! the whole-unit (snap) dots are drawn one device pixel larger and in a
//! stronger colour, so snap points stay recognisable; once the grid has
//! thinned to whole units every dot looks the same. The spacing thresholds
//! are in screen pixels (see [`dot_step`]), like the dots themselves.

use crate::camera::dot_step;
use crate::canvas::palette::Palette;
use crate::canvas::scene::Frame;
use crate::model::geometry::Vec2;
use gpui_kit::{Bounds, Window, fill, point, px, size};

/// What a grid dot marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DotKind {
    /// A whole grid unit, where parts and wires snap.
    Snap,
    /// Half-way between snap points.
    Half,
}

/// The dots painted in one frame, on a lattice of half grid units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DotGrid {
    /// Half grid units between neighbouring dots (1 = every half unit).
    pub step: i32,
    /// First and last dot column, in half grid units.
    pub columns: (i32, i32),
    /// First and last dot row, in half grid units.
    pub rows: (i32, i32),
}

impl DotGrid {
    /// The dots covering the visible area of `frame`.
    pub fn of(frame: &Frame) -> Self {
        let step = dot_step(frame.scale());
        let visible = frame.camera.visible(&frame.viewport);
        // On the dot lattice, at or before the visible edge.
        let first = |v: f32| (((v * 2.0) / step as f32).floor() as i32).saturating_mul(step);
        let last = |v: f32| (v * 2.0).ceil() as i32;
        Self {
            step,
            columns: (first(visible.min.x), last(visible.max.x)),
            rows: (first(visible.min.y), last(visible.max.y)),
        }
    }

    /// Distance between neighbouring dots, in grid units.
    pub fn pitch(&self) -> f32 {
        self.step as f32 * 0.5
    }

    /// World positions of every dot, column by column.
    #[cfg(test)]
    pub fn dots(&self) -> impl Iterator<Item = Vec2> + use<> {
        self.lattice()
            .map(|(x, y)| Vec2::new(x as f32 * 0.5, y as f32 * 0.5))
    }

    /// Every dot with its kind, column by column.
    pub fn kinded_dots(&self) -> impl Iterator<Item = (Vec2, DotKind)> + use<> {
        self.lattice().map(|(x, y)| {
            let kind = if x % 2 == 0 && y % 2 == 0 {
                DotKind::Snap
            } else {
                DotKind::Half
            };
            (Vec2::new(x as f32 * 0.5, y as f32 * 0.5), kind)
        })
    }

    /// Lattice coordinates (half grid units) of every dot.
    fn lattice(&self) -> impl Iterator<Item = (i32, i32)> + use<> {
        let step = usize::try_from(self.step).unwrap_or(1).max(1);
        let rows = self.rows;
        (self.columns.0..=self.columns.1)
            .step_by(step)
            .flat_map(move |x| (rows.0..=rows.1).step_by(step).map(move |y| (x, y)))
    }

    /// Whether dots are drawn every half unit, and snap dots emphasised.
    pub fn shows_half_units(&self) -> bool {
        self.step == 1
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

/// Side in device pixels of a dot of `kind` in `grid`: about
/// [`dot_size`] for every dot, one device pixel more for the snap dots
/// while the half-unit dots show. Halves round down, as GPUI rounds them:
/// a 1.5 px dot is one device pixel at 100 %, two at 125 % and 150 %, three
/// at 175 % and 200 %.
fn device_side(grid: &DotGrid, kind: DotKind, pixels_per_unit: f32, scale_factor: f32) -> f32 {
    let side = dot_size(grid, pixels_per_unit);
    let base = (side * sanitized(scale_factor) - 0.5).ceil().max(1.0);
    if kind == DotKind::Snap && grid.shows_half_units() {
        base + 1.0
    } else {
        base
    }
}

/// Top-left corner and side, in window pixels, of a square dot
/// `device_side` device pixels wide centred on `center`, on whole device
/// pixels.
///
/// GPUI rounds each edge of a quad to a device pixel on its own and does not
/// antialias square quads, so a dot 1.5 px wide at a fractional position
/// would cover one or two pixels depending on where it falls. The half-unit
/// pitch is often fractional (7.5 px at 75 % UI, 12.5 device px at 125 %
/// display scale), which would make neighbouring dots alternate in size.
fn dot_square(center: Vec2, device_side: f32, scale_factor: f32) -> (Vec2, f32) {
    let scale_factor = sanitized(scale_factor);
    let corner = |c: f32| (c * scale_factor - device_side * 0.5).round() / scale_factor;
    (
        Vec2::new(corner(center.x), corner(center.y)),
        device_side / scale_factor,
    )
}

/// Paints grid dots over the visible area, in the palette's grid colours.
pub fn paint_grid(window: &mut Window, frame: &Frame, palette: &Palette) {
    let grid = DotGrid::of(frame);
    let scale_factor = window.scale_factor();
    let style = |kind: DotKind| {
        let color = match kind {
            DotKind::Snap => palette.grid,
            DotKind::Half => palette.grid_half,
        };
        (device_side(&grid, kind, frame.scale(), scale_factor), color)
    };
    let (snap, half) = (style(DotKind::Snap), style(DotKind::Half));
    for (world, kind) in grid.kinded_dots() {
        let (side, color) = match kind {
            DotKind::Snap => snap,
            DotKind::Half => half,
        };
        let (corner, side) = dot_square(frame.screen(world), side, scale_factor);
        let bounds = Bounds::new(point(px(corner.x), px(corner.y)), size(px(side), px(side)));
        window.paint_quad(fill(bounds, color));
    }
}

#[cfg(test)]
mod tests {
    use super::{DotGrid, DotKind, device_side, dot_size, dot_square};
    use crate::camera::{
        Camera, MAX_ZOOM, MIN_DOT_SPACING, MIN_HALF_DOT_SPACING, MIN_ZOOM, Viewport,
    };
    use crate::canvas::scene::Frame;
    use crate::model::geometry::Vec2;
    use std::collections::BTreeSet;
    use xsim_project::SchematicView;

    /// The interface scales the settings offer.
    const UI_SCALES: [f32; 8] = [0.75, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0];

    fn frame(zoom: f32, ui_scale: f32) -> Frame {
        Frame {
            camera: Camera {
                center: Vec2::new(3.3, -1.7),
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
    fn the_default_zoom_has_a_dot_every_half_unit_at_every_ui_scale() {
        for ui_scale in UI_SCALES {
            let grid = DotGrid::of(&frame(default_zoom(), ui_scale));
            assert_eq!(grid.pitch(), 0.5, "UI scale {ui_scale}");
        }
    }

    #[test]
    fn the_default_grid_is_twice_as_dense_along_each_axis() {
        // The old default had a dot every whole unit: 11 across 10 units.
        let grid = DotGrid::of(&frame(default_zoom(), 1.0));
        let span = 0.0..=10.0;
        let across = grid
            .dots()
            .filter(|d| d.y == 0.0 && span.contains(&d.x))
            .count();
        let down = grid
            .dots()
            .filter(|d| d.x == 0.0 && span.contains(&-d.y))
            .count();
        assert_eq!((across, down), (21, 21));
    }

    #[test]
    fn every_whole_grid_point_keeps_its_dot() {
        let grid = DotGrid::of(&frame(default_zoom(), 1.0));
        let dots: Vec<Vec2> = grid.dots().collect();
        for (x, y) in [(0, 0), (3, -1), (4, -2), (-5, 6)] {
            let p = Vec2::new(x as f32, y as f32);
            assert!(dots.contains(&p), "no dot at the snap point {p:?}");
        }
        for p in [
            Vec2::new(0.5, 0.0),
            Vec2::new(3.0, -1.5),
            Vec2::new(2.5, 2.5),
        ] {
            assert!(dots.contains(&p), "no half-unit dot at {p:?}");
        }
        assert!(
            dots.iter()
                .all(|d| (d.x * 2.0).fract() == 0.0 && (d.y * 2.0).fract() == 0.0),
            "dots sit on half units only"
        );
    }

    #[test]
    fn the_dots_cover_the_visible_area() {
        for zoom in [MIN_ZOOM, 5.0, 12.0, default_zoom(), 50.0, MAX_ZOOM] {
            let f = frame(zoom, 1.25);
            let visible = f.camera.visible(&f.viewport);
            let grid = DotGrid::of(&f);
            let (min_x, max_x) = (grid.columns.0 as f32 * 0.5, grid.columns.1 as f32 * 0.5);
            let (min_y, max_y) = (grid.rows.0 as f32 * 0.5, grid.rows.1 as f32 * 0.5);
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
            let mut previous = 0.5;
            let mut zoom = MAX_ZOOM;
            while zoom >= MIN_ZOOM {
                let f = frame(zoom, ui_scale);
                let grid = DotGrid::of(&f);
                let spacing = grid.pitch() * f.scale();
                let minimum = if grid.step == 1 {
                    MIN_HALF_DOT_SPACING
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
            assert!(previous > 1.0, "fully zoomed out, the grid has thinned");
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
    fn every_dot_of_a_kind_covers_the_same_device_pixels() {
        // The half-unit pitch is often a fractional number of device pixels
        // (10 px at 125 % is 12.5): dots must not alternate in size.
        for scale_factor in DISPLAY_SCALES {
            for (zoom, ui_scale) in [
                (default_zoom(), 1.0),
                (default_zoom(), 0.75),
                (17.3, 1.1),
                (10.0, 1.0),
                (60.0, 1.0),
                (77.7, 1.25),
            ] {
                let f = frame(zoom, ui_scale);
                let grid = DotGrid::of(&f);
                if zoom >= 60.0 {
                    assert_eq!(dot_size(&grid, f.scale()), 2.0, "zoom {zoom}: larger dots");
                }
                for kind in [DotKind::Snap, DotKind::Half] {
                    let side = device_side(&grid, kind, f.scale(), scale_factor);
                    let sizes: BTreeSet<(i32, i32)> = grid
                        .kinded_dots()
                        .filter(|&(_, k)| k == kind)
                        .map(|(world, _)| {
                            let (corner, s) = dot_square(f.screen(world), side, scale_factor);
                            let (_, _, w, h) = rasterized(corner, s, scale_factor);
                            (w, h)
                        })
                        .collect();
                    let context = format!("display {scale_factor}, zoom {zoom}, {kind:?}");
                    if sizes.is_empty() {
                        assert!(
                            kind == DotKind::Half && !grid.shows_half_units(),
                            "{context}: no dots"
                        );
                        continue;
                    }
                    assert_eq!(sizes.len(), 1, "{context}: {sizes:?}");
                    let (w, h) = sizes.first().copied().unwrap_or_default();
                    assert!(w >= 1 && w == h, "{context}: {w}×{h}");
                    assert_eq!(w as f32, side, "{context}: whole device pixels");
                }
            }
        }
    }

    #[test]
    fn snap_dots_stand_out_while_half_unit_dots_show() {
        for scale_factor in DISPLAY_SCALES {
            for ui_scale in UI_SCALES {
                let f = frame(default_zoom(), ui_scale);
                let grid = DotGrid::of(&f);
                assert!(grid.shows_half_units(), "UI {ui_scale}");
                let side = |kind| device_side(&grid, kind, f.scale(), scale_factor);
                let (snap, half) = (side(DotKind::Snap), side(DotKind::Half));
                assert_eq!(snap, half + 1.0, "display {scale_factor}, UI {ui_scale}");
                if scale_factor == 1.0 && ui_scale == 1.0 {
                    assert_eq!((half, snap), (1.0, 2.0), "device pixels at 100 %");
                }
            }
        }
        // Thinned out to whole units, every dot is a snap point and keeps the
        // look the grid had before D14.
        let f = frame(10.0, 1.0);
        let grid = DotGrid::of(&f);
        assert!(!grid.shows_half_units());
        assert!(grid.kinded_dots().all(|(_, kind)| kind == DotKind::Snap));
        assert_eq!(device_side(&grid, DotKind::Snap, f.scale(), 1.0), 1.0);
    }

    #[test]
    fn snap_dots_are_exactly_the_whole_units() {
        let grid = DotGrid::of(&frame(default_zoom(), 1.0));
        let mut kinds = BTreeSet::new();
        for (d, kind) in grid.kinded_dots() {
            let whole = d.x.fract() == 0.0 && d.y.fract() == 0.0;
            assert_eq!(kind == DotKind::Snap, whole, "{d:?}");
            kinds.insert(whole);
        }
        assert_eq!(kinds.len(), 2, "both kinds are painted");
    }

    #[test]
    fn dots_stay_centred_on_their_grid_points() {
        for scale_factor in DISPLAY_SCALES {
            let f = frame(default_zoom(), 1.0);
            let grid = DotGrid::of(&f);
            for (world, kind) in grid.kinded_dots().take(500) {
                let center = f.screen(world);
                let device = device_side(&grid, kind, f.scale(), scale_factor);
                let (corner, side) = dot_square(center, device, scale_factor);
                let middle = Vec2::new(corner.x + side * 0.5, corner.y + side * 0.5);
                assert!(
                    middle.distance(center) <= 0.75 / scale_factor,
                    "display {scale_factor}: {middle:?} vs {center:?}"
                );
                let half =
                    device_side(&grid, DotKind::Half, f.scale(), scale_factor) / scale_factor;
                assert!((half - 1.5).abs() <= 0.5 / scale_factor, "side {half}");
                if kind == DotKind::Snap {
                    assert!((side - half - 1.0 / scale_factor).abs() < 1e-5, "{side}");
                }
            }
        }
    }

    #[test]
    fn the_thinning_is_decided_on_screen_pixels() {
        // Dots are a fixed size on screen, so their screen spacing decides.
        let pitch = |zoom: f32, ui_scale: f32| DotGrid::of(&frame(zoom, ui_scale)).pitch();
        assert_eq!(pitch(default_zoom(), 0.75), 0.5, "7.5 px apart");
        assert_eq!(pitch(18.0, 0.75), 1.0, "half units would be 6.75 px apart");
        assert_eq!(pitch(13.0, 1.0), 1.0);
        assert_eq!(pitch(8.0, 2.0), 0.5, "8 px apart");
        assert_eq!(pitch(MIN_ZOOM, 2.0), 2.0, "the old thinning: 12 px apart");
    }
}
