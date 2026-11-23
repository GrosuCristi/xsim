//! The schematic camera: world ↔ screen mapping, zoom and pan, fit to view.
//!
//! Pure maths on [`Vec2`]; the canvas converts to GPUI pixels at the edge.
//! Zoom is stored as pixels per grid unit at UI scale 1 (as in
//! [`SchematicView`]), so the drawing scales with the UI scale setting (Q30).
//! A grid unit is the dot pitch, half the schema-2 unit (D22): every limit
//! here is the schema-2 one converted, so the same pictures are reachable.

use crate::model::geometry::{Rect, Vec2};
use xsim_project::SchematicView;

/// Smallest zoom, in pixels per grid unit at UI scale 1.
pub const MIN_ZOOM: f32 = 1.5;
/// Largest zoom, in pixels per grid unit at UI scale 1.
pub const MAX_ZOOM: f32 = 60.0;
/// Fitting never zooms in further than this, so tiny circuits stay readable.
const FIT_MAX_ZOOM: f32 = 18.0;
/// Margin around the drawing when fitting, in grid units.
const FIT_MARGIN: f32 = 6.0;

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

/// Minimum screen distance between dots on every grid unit, in pixels. They
/// show from 7 pixels per grid unit up, which includes the default zoom at
/// every interface scale from 75 % (D14, D22).
pub const MIN_FINE_DOT_SPACING: f32 = 7.0;

/// Minimum screen distance between dots once the grid has thinned out, in
/// pixels.
pub const MIN_DOT_SPACING: f32 = 9.0;

/// Pitches of the thinned-out dot grid, in grid units: 2, 4, 10, 20, 40, 100
/// and 200 units, the 1-2-5 steps of the schema-2 unit the grid thinned to
/// before D22, so zooming out shows the same dots as before.
const COARSE_DOT_STEPS: [i32; 7] = [2, 4, 10, 20, 40, 100, 200];

/// Grid units between drawn dots at `pixels_per_unit` screen pixels per
/// grid unit. While dots stay [`MIN_FINE_DOT_SPACING`] apart there is one
/// on every grid unit, which is every point parts, wires and labels snap to
/// (D22); further out the grid thins out keeping dots [`MIN_DOT_SPACING`]
/// apart. The thresholds are screen pixels, like the dots themselves.
pub fn dot_step(pixels_per_unit: f32) -> i32 {
    if pixels_per_unit >= MIN_FINE_DOT_SPACING {
        return 1;
    }
    let coarsest = COARSE_DOT_STEPS[COARSE_DOT_STEPS.len() - 1];
    COARSE_DOT_STEPS
        .into_iter()
        .find(|&step| step as f32 * pixels_per_unit >= MIN_DOT_SPACING)
        .unwrap_or(coarsest)
}

#[cfg(test)]
mod tests {
    use super::{
        Camera, MAX_ZOOM, MIN_DOT_SPACING, MIN_FINE_DOT_SPACING, MIN_ZOOM, Viewport, dot_step,
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
            center: Vec2::new(9.0, -4.0),
            zoom: 7.0,
        };
        let vp = viewport();
        let world = Vec2::new(-20.0, 30.0);
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
            zoom: 10.0,
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
            zoom: 5.0,
        };
        let vp = viewport();
        let world = Vec2::new(6.0, 6.0);
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
        let extent = Rect::from_corners(Vec2::new(-40.0, -10.0), Vec2::new(80.0, 20.0));
        let camera = Camera::fit(Some(extent), &vp);
        let visible = camera.visible(&vp);
        assert!(visible.contains_rect(extent));
        assert_eq!(camera.center, extent.center());
        assert_eq!(Camera::fit(None, &vp).zoom, SchematicView::default().zoom);
    }

    /// D22 halved the grid unit: a drawing fits exactly as large on screen
    /// as its schema-2 original did, the same margin around it included.
    #[test]
    fn fitting_frames_a_drawing_as_before_the_finer_grid() {
        let vp = viewport();
        for (min, max) in [
            (Vec2::new(-40.0, -10.0), Vec2::new(80.0, 20.0)),
            (Vec2::new(0.0, 0.0), Vec2::new(8.0, 0.0)),
            (Vec2::new(-400.0, -300.0), Vec2::new(400.0, 300.0)),
            (Vec2::new(-4000.0, 0.0), Vec2::new(4000.0, 16.0)),
        ] {
            let camera = Camera::fit(Some(Rect::from_corners(min, max)), &vp);
            // Schema 2: half the coordinates, a 3-unit margin, zoom 3 to 36.
            let (w, h) = ((max.x - min.x) / 2.0 + 6.0, (max.y - min.y) / 2.0 + 6.0);
            let old = ((vp.size.x / w).min(vp.size.y / h) / vp.ui_scale).clamp(3.0, 36.0);
            assert!(
                (camera.zoom * 2.0 - old).abs() < 1e-4,
                "{min:?}–{max:?}: {} vs {old}",
                camera.zoom
            );
        }
    }

    #[test]
    fn the_zoom_range_is_the_schema2_range_in_finer_units() {
        // 3 to 120 pixels per schema-2 unit, 20 by default.
        assert_eq!((MIN_ZOOM * 2.0, MAX_ZOOM * 2.0), (3.0, 120.0));
        assert_eq!(SchematicView::default().zoom * 2.0, 20.0);
    }

    #[test]
    fn invalid_saved_zoom_falls_back_to_the_default() {
        let view = SchematicView {
            zoom: f32::NAN,
            ..SchematicView::default()
        };
        assert_eq!(Camera::from(view).zoom, SchematicView::default().zoom);
    }

    /// The thinning before D22, in half schema-2 units at `pixels_per_unit`
    /// pixels per schema-2 unit: a dot every half unit down to 7 px apart,
    /// then every 1, 2, 5, 10, ... units keeping dots at least 9 px apart.
    fn schema2_dot_step(pixels_per_unit: f32) -> i32 {
        if 0.5 * pixels_per_unit >= 7.0 {
            return 1;
        }
        let mut step = 1;
        for next in [2, 5, 10, 20, 50, 100] {
            if step as f32 * pixels_per_unit >= 9.0 {
                break;
            }
            step = next;
        }
        2 * step
    }

    #[test]
    fn there_is_a_dot_on_every_unit_until_they_get_too_close() {
        assert_eq!(dot_step(10.0), 1, "10 px apart: every unit");
        assert_eq!(dot_step(7.0), 1, "exactly the fine minimum");
        assert_eq!(dot_step(6.95), 2, "every 2 units");
        assert_eq!(dot_step(4.5), 2, "exactly the coarse minimum");
        assert_eq!(dot_step(4.45), 4, "every 4 units");
        assert_eq!(dot_step(2.5), 4);
        assert_eq!(dot_step(0.5), 20, "every 20 units");
        assert_eq!(dot_step(0.005), 200, "the coarsest pitch");
        assert_eq!(dot_step(f32::NAN), 200);
    }

    #[test]
    fn the_dots_are_the_ones_drawn_before_the_finer_grid() {
        let mut pixels_per_unit = 0.25;
        while pixels_per_unit <= 2.0 * MAX_ZOOM {
            assert_eq!(
                dot_step(pixels_per_unit),
                schema2_dot_step(2.0 * pixels_per_unit),
                "{pixels_per_unit} px per unit"
            );
            pixels_per_unit *= 1.01;
        }
    }

    #[test]
    fn dots_thin_out_monotonically_and_never_crowd() {
        let mut previous = 1;
        let mut pixels_per_unit = 2.0 * MAX_ZOOM;
        while pixels_per_unit >= MIN_ZOOM * 0.25 {
            let step = dot_step(pixels_per_unit);
            let spacing = step as f32 * pixels_per_unit;
            assert!(step >= previous, "{pixels_per_unit}: {step} < {previous}");
            assert!(
                spacing >= MIN_FINE_DOT_SPACING,
                "{pixels_per_unit}: crowded"
            );
            assert!(step == 1 || spacing >= MIN_DOT_SPACING, "{pixels_per_unit}");
            previous = step;
            pixels_per_unit *= 0.97;
        }
    }
}
