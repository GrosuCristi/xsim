//! Zoom, pan and fit to view; keeps [`SchematicView`] in sync (Q39).

use crate::camera::Camera;
use crate::canvas::scene::AutoFit;
use crate::model::geometry::{Rect, Vec2};
use crate::model::label_geometry::label_hit_rect;
use crate::{EditorEvent, SchematicEditor};
use gpui_kit::{Context, ScrollDelta, ScrollEvent};
use xsim_project::Schematic;

/// Zoom factor of one mouse-wheel notch.
pub(crate) const WHEEL_ZOOM: f32 = 1.18;
/// Pixels panned per wheel line with Shift held.
const LINE_PAN: f32 = 40.0;
/// Wheel lines GPUI reports for one notch. Windows multiplies notches by
/// the `WheelScrollLines` setting (3 unless the user changed it; GPUI does
/// not expose the value), X11 and Wayland report 3, and macOS about one
/// line per notch.
pub(crate) const LINES_PER_NOTCH: f32 = if cfg!(target_os = "macos") { 1.0 } else { 3.0 };
/// Most notches one wheel event may count, so a burst cannot jump across
/// the whole zoom range.
const MAX_NOTCHES: f32 = 5.0;
/// A line count this large comes from Windows' "one screen at a time"
/// wheel setting; it counts as one notch.
const PAGE_LINES: f32 = 1.0e4;

/// The lines of one wheel event, sanitised: non-finite counts are ignored,
/// a "one screen at a time" count is one notch, bursts are capped.
fn wheel_lines(lines: f32) -> f32 {
    if !lines.is_finite() {
        0.0
    } else if lines.abs() >= PAGE_LINES {
        lines.signum() * LINES_PER_NOTCH
    } else {
        let cap = MAX_NOTCHES * LINES_PER_NOTCH;
        lines.clamp(-cap, cap)
    }
}

/// Zoom factor for `lines` wheel lines: [`WHEEL_ZOOM`] per notch.
fn wheel_zoom(lines: f32) -> f32 {
    WHEEL_ZOOM.powf(wheel_lines(lines) / LINES_PER_NOTCH)
}

/// World extent of everything drawn, text margins included.
pub fn drawing_extent(schematic: &Schematic) -> Option<Rect> {
    let parts = schematic.components.iter().flat_map(|c| {
        let (a, b) = c.bounds();
        [Vec2::from_grid(a), Vec2::from_grid(b)]
    });
    let wires = schematic
        .wires
        .iter()
        .flat_map(|w| w.points.iter().map(|&p| Vec2::from_grid(p)));
    let labels = schematic.labels.iter().flat_map(|l| {
        let r = label_hit_rect(schematic, l);
        [r.min, r.max]
    });
    Rect::enclosing(parts.chain(wires).chain(labels)).map(|r| r.inflate(2.0, 2.0))
}

impl SchematicEditor {
    /// The current camera.
    pub(crate) fn camera(&self) -> Camera {
        Camera::from(self.view)
    }

    /// Moves the camera at the user's request and reports it.
    pub(crate) fn set_camera(&mut self, camera: Camera, cx: &mut Context<Self>) {
        let view = camera.to_view(true);
        if view != self.view {
            self.view = view;
            self.fit_pending = false;
            cx.emit(EditorEvent::ViewChanged);
            cx.notify();
        }
    }

    /// Fits the drawing now if the canvas has been measured, else on the
    /// next frame.
    pub(crate) fn fit_now(&mut self, cx: &mut Context<Self>) {
        match self.viewport.get() {
            Some(viewport) if !viewport.is_empty() => {
                let camera = Camera::fit(drawing_extent(&self.schematic), &viewport);
                self.set_camera(camera, cx);
            }
            _ => {
                self.fit_pending = true;
                cx.notify();
            }
        }
    }

    /// Stores the camera an automatic fit chose while painting. The view
    /// stays "not set by the user", so it keeps fitting on resize.
    pub(crate) fn finish_auto_fit(&mut self, camera: Camera, cx: &mut Context<Self>) {
        self.fit_pending = false;
        let view = camera.to_view(false);
        if !self.view.user_set && view != self.view {
            self.view = view;
            cx.notify();
        }
    }

    /// The automatic fit the next frame should apply.
    pub(crate) fn auto_fit(&self) -> AutoFit {
        if self.view.user_set && !self.fit_pending {
            return AutoFit::Off;
        }
        let extent = drawing_extent(&self.schematic);
        if self.fit_pending {
            AutoFit::Now(extent)
        } else {
            AutoFit::OnResize(extent)
        }
    }

    /// Zooms about the canvas centre (keyboard `+` / `-`).
    pub(crate) fn zoom_by(&mut self, factor: f32, cx: &mut Context<Self>) {
        let Some(viewport) = self.viewport.get() else {
            return;
        };
        let camera = self
            .camera()
            .zoomed_at(&viewport, viewport.center(), factor);
        self.set_camera(camera, cx);
    }

    /// Mouse wheel: zoom at the cursor by [`WHEEL_ZOOM`] per notch; Shift
    /// scrolls sideways; precise (trackpad) deltas pan unless Ctrl/Cmd is
    /// held, and then zoom smoothly.
    pub(crate) fn scroll(&mut self, event: &ScrollEvent, cx: &mut Context<Self>) {
        let Some(viewport) = self.viewport.get() else {
            return;
        };
        let at = Vec2::new(f32::from(event.position.x), f32::from(event.position.y));
        let zoom_modifier = event.modifiers.control || event.modifiers.platform;
        let camera = self.camera();
        let camera = match event.delta {
            ScrollDelta::Lines(lines) if event.modifiers.shift => {
                // Windows and X11 already turn a Shift+wheel turn into `x`.
                let lines = wheel_lines(lines.x + lines.y);
                camera.panned(&viewport, Vec2::new(lines * LINE_PAN, 0.0))
            }
            ScrollDelta::Lines(lines) => camera.zoomed_at(&viewport, at, wheel_zoom(lines.y)),
            ScrollDelta::Pixels(delta) if zoom_modifier => {
                camera.zoomed_at(&viewport, at, (f32::from(delta.y) * 0.01).exp())
            }
            ScrollDelta::Pixels(delta) => {
                camera.panned(&viewport, Vec2::new(f32::from(delta.x), f32::from(delta.y)))
            }
        };
        self.set_camera(camera, cx);
        if let Some(pointer) = &mut self.interaction.pointer {
            pointer.world = camera.to_world(&viewport, at);
        }
    }

    /// Pans by a screen displacement while dragging the view.
    pub(crate) fn pan_by(&mut self, delta: Vec2, cx: &mut Context<Self>) {
        let Some(viewport) = self.viewport.get() else {
            return;
        };
        let camera = self.camera().panned(&viewport, delta);
        self.set_camera(camera, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::{LINES_PER_NOTCH, WHEEL_ZOOM, wheel_lines, wheel_zoom};

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn one_notch_zooms_by_the_documented_step() {
        assert!(close(wheel_zoom(LINES_PER_NOTCH), WHEEL_ZOOM));
        assert!(close(wheel_zoom(-LINES_PER_NOTCH), 1.0 / WHEEL_ZOOM));
        assert!(close(
            wheel_zoom(2.0 * LINES_PER_NOTCH),
            WHEEL_ZOOM * WHEEL_ZOOM
        ));
        assert!(close(wheel_zoom(0.0), 1.0));
    }

    #[test]
    fn odd_wheel_counts_stay_bounded() {
        assert_eq!(wheel_lines(f32::NAN), 0.0);
        assert_eq!(wheel_lines(f32::INFINITY), 0.0);
        // Windows' "one screen at a time": u32::MAX lines per notch.
        assert!(close(wheel_zoom(4_294_967_295.0), WHEEL_ZOOM));
        assert!(close(wheel_zoom(-4_294_967_295.0), 1.0 / WHEEL_ZOOM));
        assert!(
            close(wheel_zoom(1000.0), WHEEL_ZOOM.powi(5)),
            "a burst is capped"
        );
        // A precision touchpad sends fractions of a notch: still smooth.
        assert!(close(
            wheel_zoom(0.3),
            WHEEL_ZOOM.powf(0.3 / LINES_PER_NOTCH)
        ));
    }
}
