//! The schematic camera: world ↔ screen mapping, zoom and pan, fit to view.
//!
//! Pure maths on [`Vec2`]; the canvas converts to GPUI pixels at the edge.
//! Zoom is stored as pixels per grid unit at UI scale 1 (as in
//! [`SchematicView`]), so the drawing scales with the UI scale setting (Q30).

use crate::model::geometry::{Rect, Vec2};
use xsim_project::SchematicView;

/// Smallest zoom, in pixels per grid unit at UI scale 1.
pub const MIN_ZOOM: f32 = 3.0;
/// Largest zoom, in pixels per grid unit at UI scale 1.
pub const MAX_ZOOM: f32 = 120.0;
/// Fitting never zooms in further than this, so tiny circuits stay readable.
const FIT_MAX_ZOOM: f32 = 36.0;
/// Margin around the drawing when fitting, in grid units.
const FIT_MARGIN: f32 = 3.0;

/// The canvas rectangle on screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    /// Top-left corner in window pixels.
    pub origin: Vec2,
    /// Size in pixels.
    pub size: Vec2,
    /// UI scale factor (1.0 = 100 %).
    pub ui_scale: f32,
}

impl Viewport {
    pub fn center(&self) -> Vec2 {
        self.origin + self.size * 0.5
    }

    pub fn is_empty(&self) -> bool {
        self.size.x <= 0.0 || self.size.y <= 0.0
    }
}

/// Camera position and zoom.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// World point at the centre of the viewport.
    pub center: Vec2,
    /// Pixels per grid unit at UI scale 1.
    pub zoom: f32,
}

impl From<SchematicView> for Camera {
    fn from(view: SchematicView) -> Self {
        Self {
            center: Vec2::new(view.center[0], view.center[1]),
            zoom: sanitize_zoom(view.zoom),
        }
    }
}

impl Camera {
    /// The saved form of this camera.
    pub fn to_view(self, user_set: bool) -> SchematicView {
        SchematicView {
            center: [self.center.x, self.center.y],
            zoom: self.zoom,
            user_set,
        }
    }

    /// Screen pixels per grid unit.
    pub fn scale(&self, viewport: &Viewport) -> f32 {
        self.zoom * viewport.ui_scale
    }

    pub fn to_screen(self, viewport: &Viewport, world: Vec2) -> Vec2 {
        viewport.center() + (world - self.center) * self.scale(viewport)
    }

    pub fn to_world(self, viewport: &Viewport, screen: Vec2) -> Vec2 {
        self.center + (screen - viewport.center()) * (1.0 / self.scale(viewport))
    }

    /// Zooms by `factor`, keeping the world point under `screen` in place.
    pub fn zoomed_at(&self, viewport: &Viewport, screen: Vec2, factor: f32) -> Camera {
        let anchor = self.to_world(viewport, screen);
        let zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let scale = zoom * viewport.ui_scale;
        Camera {
            center: anchor - (screen - viewport.center()) * (1.0 / scale),
            zoom,
        }
    }

    /// Moves the view by a screen displacement (content follows the mouse).
    pub fn panned(&self, viewport: &Viewport, delta: Vec2) -> Camera {
        Camera {
            center: self.center - delta * (1.0 / self.scale(viewport)),
            zoom: self.zoom,
        }
    }

    /// The camera that shows `extent` (world units) whole, or the default
    /// camera around the origin for an empty drawing.
    pub fn fit(extent: Option<Rect>, viewport: &Viewport) -> Camera {
        let default = SchematicView::default();
        let Some(extent) = extent else {
            return Camera {
                center: Vec2::default(),
                zoom: default.zoom,
            };
        };
        if viewport.is_empty() {
            return Camera {
                center: extent.center(),
                zoom: default.zoom,
            };
        }
        let width = extent.width() + 2.0 * FIT_MARGIN;
        let height = extent.height() + 2.0 * FIT_MARGIN;
        let zoom = (viewport.size.x / width).min(viewport.size.y / height) / viewport.ui_scale;
        Camera {
            center: extent.center(),
            zoom: zoom.clamp(MIN_ZOOM, FIT_MAX_ZOOM),
        }
    }

    /// The world rectangle visible in the viewport.
    pub fn visible(&self, viewport: &Viewport) -> Rect {
        Rect::from_corners(
            self.to_world(viewport, viewport.origin),
            self.to_world(viewport, viewport.origin + viewport.size),
        )
    }
}

fn sanitize_zoom(zoom: f32) -> f32 {
    if zoom.is_finite() {
        zoom.clamp(MIN_ZOOM, MAX_ZOOM)
    } else {
        SchematicView::default().zoom
    }
}

/// Minimum screen distance between half-unit grid dots, in pixels. They
/// show from 14 pixels per grid unit up, which includes the default zoom at
/// every interface scale from 75 % (D14).
pub const MIN_HALF_DOT_SPACING: f32 = 7.0;

/// Minimum screen distance between whole-unit and coarser grid dots, in
/// pixels: the thinning the grid had before the half-unit dots.
pub const MIN_DOT_SPACING: f32 = 9.0;

/// Pitches of the dot grid coarser than half a unit, in half grid units:
/// 1, 2, 5, 10, 20, 50 and 100 grid units.
const COARSE_DOT_STEPS: [i32; 7] = [2, 4, 10, 20, 40, 100, 200];

/// Half grid units between drawn dots (1 = a dot every half unit) at
/// `pixels_per_unit` screen pixels per grid unit. The grid is drawn twice as
/// dense as it snaps while half-unit dots stay [`MIN_HALF_DOT_SPACING`]
/// apart, then thins out 1-2-5 style keeping dots [`MIN_DOT_SPACING`]
/// apart. The thresholds are screen pixels, like the dots themselves.
pub fn dot_step(pixels_per_unit: f32) -> i32 {
    if 0.5 * pixels_per_unit >= MIN_HALF_DOT_SPACING {
        return 1;
    }
    let coarsest = COARSE_DOT_STEPS[COARSE_DOT_STEPS.len() - 1];
    COARSE_DOT_STEPS
        .into_iter()
        .find(|&step| step as f32 * 0.5 * pixels_per_unit >= MIN_DOT_SPACING)
        .unwrap_or(coarsest)
}

#[cfg(test)]
mod tests {
    use super::{
        Camera, MAX_ZOOM, MIN_DOT_SPACING, MIN_HALF_DOT_SPACING, MIN_ZOOM, Viewport, dot_step,
    };
    use crate::model::geometry::{Rect, Vec2};
    use xsim_project::SchematicView;

    fn viewport() -> Viewport {
        Viewport {
            origin: Vec2::new(100.0, 40.0),
            size: Vec2::new(500.0, 300.0),
            ui_scale: 1.25,
        }
    }

    fn close(a: Vec2, b: Vec2) -> bool {
        a.distance(b) < 1e-3
    }

    #[test]
    fn screen_and_world_round_trip() {
        let camera = Camera {
            center: Vec2::new(4.5, -2.0),
            zoom: 14.0,
        };
        let vp = viewport();
        let world = Vec2::new(-10.0, 15.0);
        assert!(close(
            camera.to_world(&vp, camera.to_screen(&vp, world)),
            world
        ));
        assert!(close(camera.to_screen(&vp, camera.center), vp.center()));
    }

    #[test]
    fn zooming_keeps_the_point_under_the_cursor() {
        let camera = Camera {
            center: Vec2::new(0.0, 0.0),
            zoom: 20.0,
        };
        let vp = viewport();
        let cursor = Vec2::new(180.0, 90.0);
        let before = camera.to_world(&vp, cursor);
        let zoomed = camera.zoomed_at(&vp, cursor, 1.7);
        assert!(close(zoomed.to_world(&vp, cursor), before));
        assert_eq!(camera.zoomed_at(&vp, cursor, 1e6).zoom, MAX_ZOOM);
        assert_eq!(camera.zoomed_at(&vp, cursor, 1e-6).zoom, MIN_ZOOM);
    }

    #[test]
    fn panning_moves_content_with_the_mouse() {
        let camera = Camera {
            center: Vec2::new(0.0, 0.0),
            zoom: 10.0,
        };
        let vp = viewport();
        let world = Vec2::new(3.0, 3.0);
        let before = camera.to_screen(&vp, world);
        let panned = camera.panned(&vp, Vec2::new(25.0, -10.0));
        assert!(close(
            panned.to_screen(&vp, world),
            before + Vec2::new(25.0, -10.0)
        ));
    }

    #[test]
    fn fitting_shows_the_whole_extent() {
        let vp = viewport();
        let extent = Rect::from_corners(Vec2::new(-20.0, -5.0), Vec2::new(40.0, 10.0));
        let camera = Camera::fit(Some(extent), &vp);
        let visible = camera.visible(&vp);
        assert!(visible.contains_rect(extent));
        assert_eq!(camera.center, extent.center());
        assert_eq!(Camera::fit(None, &vp).zoom, SchematicView::default().zoom);
    }

    #[test]
    fn invalid_saved_zoom_falls_back_to_the_default() {
        let view = SchematicView {
            zoom: f32::NAN,
            ..SchematicView::default()
        };
        assert_eq!(Camera::from(view).zoom, SchematicView::default().zoom);
    }

    /// The grid before D14, in whole grid units: 1, 2, 5, 10, ... keeping
    /// dots at least 9 px apart.
    fn whole_unit_dot_step(pixels_per_unit: f32) -> i32 {
        let mut step = 1;
        for next in [2, 5, 10, 20, 50, 100] {
            if step as f32 * pixels_per_unit >= 9.0 {
                break;
            }
            step = next;
        }
        step
    }

    #[test]
    fn dots_are_half_a_unit_apart_until_they_get_too_close() {
        assert_eq!(dot_step(20.0), 1, "10 px apart: every half unit");
        assert_eq!(dot_step(14.0), 1, "exactly the half-unit minimum");
        assert_eq!(dot_step(13.9), 2, "every unit");
        assert_eq!(dot_step(9.0), 2, "exactly the whole-unit minimum");
        assert_eq!(dot_step(8.9), 4, "every 2 units");
        assert_eq!(dot_step(5.0), 4);
        assert_eq!(dot_step(1.0), 20, "every 10 units");
        assert_eq!(dot_step(0.01), 200, "the coarsest pitch");
        assert_eq!(dot_step(f32::NAN), 200);
    }

    #[test]
    fn only_the_half_unit_dots_are_new() {
        let mut pixels_per_unit = 0.5;
        while pixels_per_unit <= 2.0 * MAX_ZOOM {
            let step = dot_step(pixels_per_unit);
            if pixels_per_unit >= 2.0 * MIN_HALF_DOT_SPACING {
                assert_eq!(step, 1, "{pixels_per_unit} px per unit");
            } else {
                assert_eq!(
                    step,
                    2 * whole_unit_dot_step(pixels_per_unit),
                    "{pixels_per_unit} px per unit: the old thinning"
                );
            }
            pixels_per_unit *= 1.01;
        }
    }

    #[test]
    fn dots_thin_out_monotonically_and_never_crowd() {
        let mut previous = 1;
        let mut pixels_per_unit = 2.0 * MAX_ZOOM;
        while pixels_per_unit >= MIN_ZOOM * 0.25 {
            let step = dot_step(pixels_per_unit);
            let spacing = step as f32 * 0.5 * pixels_per_unit;
            assert!(step >= previous, "{pixels_per_unit}: {step} < {previous}");
            assert!(
                spacing >= MIN_HALF_DOT_SPACING,
                "{pixels_per_unit}: crowded"
            );
            assert!(step == 1 || spacing >= MIN_DOT_SPACING, "{pixels_per_unit}");
            previous = step;
            pixels_per_unit *= 0.97;
        }
    }
}
