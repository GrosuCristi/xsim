//! Viewer operations shared by the UI, the public API and the exporters.

use crate::export;
use crate::model::op_table::is_operating_point;
use crate::model::setup;
use crate::scene::Scene;
use crate::scene::layout::{Style, stacked_scene};
use crate::scene::lod::LodCache;
use crate::view_model::PlotModel;
use crate::{ViewerEvent, WaveformViewer};
use gpui_kit::Context;
use xsim_project::PlotSetup;
use xsim_spice_engine::AnalysisKind;

/// Export size before the viewer has been painted.
const DEFAULT_IMAGE_SIZE: (u32, u32) = (1200, 720);
/// Exports render at twice the on-screen size so pasted plots stay sharp.
const EXPORT_SCALE: f32 = 2.0;
/// The largest default export side.
const MAX_DEFAULT_SIDE: f32 = 4096.0;

impl WaveformViewer {
    /// The image size matching the panes on screen (at twice the size, for
    /// sharp pasting), or 1200×720 before the viewer was first painted.
    pub fn default_image_size(&self) -> (u32, u32) {
        let geometry = self.view.geometry.borrow();
        let mut frames = geometry.iter().flatten().map(|g| g.frame);
        let Some(first) = frames.next() else {
            return DEFAULT_IMAGE_SIZE;
        };
        let union = frames.fold(first, |acc, f| acc.union(&f));
        let side = |v: f32| (v * EXPORT_SCALE).clamp(64.0, MAX_DEFAULT_SIDE).round() as u32;
        (
            side(union.size.width.as_f32()),
            side(union.size.height.as_f32()),
        )
    }

    /// The panes as an SVG document of `width`×`height` pixels, drawn with
    /// the current theme colours.
    pub fn svg(&self, width: u32, height: u32) -> Result<String, String> {
        let scene = self.export_scene(width as f32, height as f32)?;
        Ok(export::svg::render(&scene))
    }

    /// The image [`Self::copy_image_to_clipboard`] places on the clipboard,
    /// at [`Self::default_image_size`].
    pub fn clipboard_image(&self) -> Result<arboard::ImageData<'static>, String> {
        let (width, height) = self.default_image_size();
        export::raster::clipboard_image(&self.svg(width, height)?, width, height)
    }

    /// CSV text of every visible trace, or of every signal for an
    /// operating point.
    pub fn csv(&self) -> Result<String, String> {
        let result = self.result.as_deref().ok_or(NO_RESULT)?;
        let columns = if is_operating_point(result) {
            export::table::signal_columns(result)
        } else {
            export::table::trace_columns(&self.setup, &self.traces)
        };
        if columns.is_empty() {
            return Err(NO_TRACES.to_string());
        }
        Ok(export::table::csv(result, &columns))
    }

    fn export_scene(&self, width: f32, height: f32) -> Result<Scene, String> {
        let model = self.model().ok_or(NO_RESULT)?;
        if is_operating_point(model.result) {
            return Err(
                "An operating point has no plot to export; export it as CSV instead.".into(),
            );
        }
        let panes = model.panes();
        if panes.iter().all(|p| p.traces.is_empty()) {
            return Err(NO_TRACES.to_string());
        }
        let font_size = (width.min(height * 1.8) / 85.0).clamp(10.0, 30.0).round();
        let style = Style {
            colors: &self.colors,
            font_size,
            line_width: (font_size / 8.0).clamp(1.25, 3.5),
        };
        let mut lod = LodCache::default();
        Ok(stacked_scene(
            &model.x(),
            &panes,
            width,
            height,
            &style,
            &mut lod,
        ))
    }

    /// The borrowed state scenes are built from, when there is a result.
    pub(crate) fn model(&self) -> Option<PlotModel<'_>> {
        Some(PlotModel {
            result: self.result.as_deref()?,
            x_extent: self.x_extent,
            setup: &self.setup,
            traces: &self.traces,
            colors: &self.colors,
        })
    }

    pub(crate) fn is_ac(&self) -> bool {
        self.result
            .as_ref()
            .is_some_and(|r| r.analysis == AnalysisKind::Ac)
    }

    /// Applies `edit` to the setup; when it reports a change, re-evaluates
    /// traces, emits [`ViewerEvent::SetupChanged`] and repaints.
    pub(crate) fn edit_setup(
        &mut self,
        cx: &mut Context<Self>,
        edit: impl FnOnce(&mut PlotSetup) -> bool,
    ) -> bool {
        if !edit(&mut self.setup) {
            return false;
        }
        self.traces.sync(&self.setup, self.result.as_deref());
        self.validate_selection();
        self.pin_cursor_traces();
        cx.emit(ViewerEvent::SetupChanged);
        cx.notify();
        true
    }

    /// Re-places enabled cursors on the samples of the current result.
    pub(crate) fn place_cursors(&mut self) {
        if !self.view.cursors.enabled {
            return;
        }
        let (Some(result), Some(visible)) =
            (self.result.clone(), self.model().map(|m| m.x_range()))
        else {
            return;
        };
        self.view.cursors.enable(&result.axis.values, visible);
    }

    /// Keeps the selection on an existing, plottable trace; falls back to
    /// the first plottable trace.
    pub(crate) fn validate_selection(&mut self) {
        let valid = self
            .view
            .selected
            .is_some_and(|(pane, trace)| self.plottable(pane, trace));
        if !valid {
            self.view.selected = self.first_plottable();
        }
    }

    /// Whether trace `trace` of pane `pane` is visible and evaluated.
    pub(crate) fn plottable(&self, pane: usize, trace: usize) -> bool {
        let Some(p) = self.setup.panes.get(pane) else {
            return false;
        };
        p.traces
            .get(trace)
            .is_some_and(|t| t.visible && self.traces.values(&t.expression, p.y_scale).is_some())
    }

    fn first_plottable(&self) -> Option<(usize, usize)> {
        self.setup.panes.iter().enumerate().find_map(|(pane, p)| {
            (0..p.traces.len())
                .find(|&trace| self.plottable(pane, trace))
                .map(|trace| (pane, trace))
        })
    }

    /// Returns every axis to its automatic range.
    pub(crate) fn zoom_to_fit(&mut self, cx: &mut Context<Self>) {
        self.edit_setup(cx, setup::reset_ranges);
        cx.notify();
    }
}

const NO_RESULT: &str = "There is nothing to export yet: run a simulation first.";
const NO_TRACES: &str = "There is nothing to export: add a visible trace first.";
