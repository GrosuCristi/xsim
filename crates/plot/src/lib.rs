//! # xsim waveform viewer
//!
//! A GPUI view that plots a [`SimulationResult`] (Q127–Q140): one or more
//! synchronized plot panes, expression traces, linear/log/dB/phase axes, a
//! second y axis for a second unit in one pane (D9), trace colours pinned
//! to palette slots (D11), two vertical cursors with delta readouts,
//! interactive measurements, zoom/pan, and export to the clipboard, PNG,
//! JPG, SVG and CSV.
//!
//! ## Pointer gestures (D14, D19)
//! * Wheel zooms x at the pointer, Shift/Ctrl+wheel zooms y (over an axis's
//!   labels, only that axis). Zooming out stops at the data: x at the full
//!   data extent, y at the automatic range, which the axis then returns to.
//! * Left-drag on the plot pans (stopping at the data the same way). The
//!   **magnifier** tool ("Zoom to area") turns left-drag into a box zoom; it
//!   stays on for further zooms until it is clicked again or Escape is
//!   pressed. Middle-drag and Space+drag always pan; double-click fits.
//! * Each cursor moves on the x axis only (D19): it is a vertical line
//!   across every pane, dragged along x and snapped to samples, with no
//!   horizontal line. Its y in the readout is the value of the trace it
//!   follows at x; the readout's `1`/`2` buttons choose the trace. Cursor
//!   lines can be dragged with either tool.
//! * A legend chip dragged onto another pane moves its trace there, keeping
//!   its colour; a pane that would get a third unit (D9) refuses it. A
//!   press on a chip's hide or remove button never starts that drag.
//! * Escape cancels the gesture in progress and puts back what it changed:
//!   a box zoom or chip move is not applied, a pan returns to the view it
//!   started from and a cursor drag to the cursor's earlier place. Without
//!   a gesture, Escape turns the magnifier off.
//! * A drag belongs to the button that began it: the other button (left or
//!   middle) does nothing until that button is released.
//! * The `1`/`2` flags at the top of the cursor lines never cover each
//!   other: when the lines are closer than a flag, cursor 2's drops one row.
//!
//! ## Panes (D22, D23)
//! The panes stack under the toolbar and share the x axis. The **plus** at
//! the very bottom of the dock, below the last pane and the cursor readout,
//! adds an empty pane (Enter or Space when it has focus). With more panes
//! than fit, the stack scrolls with the wheel over a pane's header, the
//! plus stays in view, and a new pane is scrolled into view. The wheel over
//! a plot always zooms and never scrolls the stack.
//!
//! The panes share the dock's height in proportion to relative heights
//! stored with the plot setup ([`xsim_project::PlotPane::height`],
//! [`model::heights`]), so a taller or shorter dock scales them alike, and
//! no pane is shorter than 7.5 rem (its legend row, a few y ticks and the x
//! tick labels). Dragging the **divider** between two panes (a 0.5 rem band
//! on the line between them, with a row-resize pointer and a grip under
//! the pointer) moves height from one to the other and stops at that
//! minimum; it never pans, zooms or moves a cursor, and Escape puts the
//! heights back. A **double click** on a divider makes its two panes
//! equally high. Either changes the setup once, at the end
//! ([`ViewerEvent::SetupChanged`]). With every pane at its minimum (a
//! scrolling stack) the dividers cannot move, and a double click only makes
//! the two panes' stored heights equal. While a legend chip is dragged the
//! dividers make way, so the chip drops on either pane up to the line. A
//! new pane gets an average share; closing a pane gives its height to the
//! others in proportion.
//!
//! Exported images (PNG, JPEG, SVG and the clipboard) show the panes,
//! traces, axes and legends, but not the cursors or the hover line: Q07,
//! Q137 and D14 do not ask for them in images. They keep the panes'
//! relative heights (D23).
//!
//! On a phase axis every trace is unwrapped, a typed `ph(…)` included, so
//! it matches the complex trace it was taken from (INT-9); `ph(…)` itself
//! stays wrapped to (−180°, 180°] elsewhere.
//!
//! The viewer edits a [`PlotSetup`]; the shell stores it in the project and
//! keeps it across simulation runs (Q136).
//!
//! No axis has a title that only names a quantity and its unit: every tick
//! label carries its unit ("2.2 W", "500 µs"), and every pane shows the x
//! tick labels (D20). Only a sweep's x axis keeps a title, the swept
//! source's name, under the bottom pane.
//!
//! The plot is built on GPUI Kit's plot infrastructure (Q127): every pane is
//! a `gpui_kit::base::plot::Plot` painted with its `Grid` and `PlotLabel`
//! primitives and its `PathCaches`, extended with a logarithmic scale
//! ([`model::axis::AxisMap`]), level-of-detail decimation and clipping.
//! Traces are stroked with GPUI's `PathBuilder` rather than the kit's
//! `Line`, because a trace must be cut where it turns straight back: a
//! single stroke pinches there, so a narrow spike looked like a hair (D20,
//! [`scene::stroke`]).
//!
//! ## Layers
//! * [`model`] — GPUI-free logic: axes, ticks, ranges, traces, setup edits,
//!   cursors, measurements, completion, colours;
//! * [`scene`] — window-independent layout of panes into lines and labels;
//! * [`export`] — SVG, PNG, JPEG, clipboard and CSV output of those scenes;
//! * [`view_model`] — viewer state → scene inputs;
//! * `ui` — the GPUI view: toolbar, panes, legends, cursors, panels.
//!
//! ## Wiring the viewer into the shell
//! * Call [`WaveformViewer::set_result`] after every run and when switching
//!   circuits; on [`ViewerEvent::SetupChanged`] store
//!   [`WaveformViewer::setup`] in the circuit document.
//! * Subscribe to [`ViewerRequest`]: `ExportImage(format)` and `ExportCsv`
//!   come from the toolbar's export menu — ask for a path (e.g. with `rfd`)
//!   and call [`WaveformViewer::export_image`] (with
//!   [`WaveformViewer::default_image_size`]) or
//!   [`WaveformViewer::export_csv`]. `Status(text)` is a message for the
//!   status bar (copy confirmations and errors).
//! * Probing a schematic net calls [`WaveformViewer::add_trace`].
//! * The toolbar uses Lucide icons beyond the default component bundle;
//!   register `gpui_kit::assets::AllAssets` so they render.

extern crate gpui_kit as gpui;

pub mod export;
pub mod model;
pub mod scene;
mod ui;
pub mod view_model;
mod viewer_ops;

#[cfg(test)]
mod pipeline_tests;
#[cfg(test)]
mod test_fixtures;

pub use ui::actions::{CancelInteraction, CopyPlot, RemoveSelectedTrace, ZoomToFit, init};

use gpui_kit::component::ActiveTheme;
use gpui_kit::{prelude::*, *};
use model::cursors::Cursors;
use model::palette::PlotColors;
use model::ranges::AxisExtent;
use model::traces::TraceStore;
use std::path::Path;
use std::sync::Arc;
use xsim_project::PlotSetup;
use xsim_spice_engine::SimulationResult;
use xsim_waveform::ExprError;

#[derive(Debug, Clone, PartialEq)]
pub enum ViewerEvent {
    /// Traces, panes, ranges or axis modes changed; persist the new setup.
    SetupChanged,
}

/// Requests the viewer cannot fulfil on its own; the shell handles them.
#[derive(Debug, Clone, PartialEq)]
pub enum ViewerRequest {
    /// The user chose an image format in the export menu: ask for a file
    /// path and call [`WaveformViewer::export_image`].
    ExportImage(ImageFormat),
    /// The user chose CSV in the export menu: ask for a file path and call
    /// [`WaveformViewer::export_csv`].
    ExportCsv,
    /// A short message for the status bar, e.g. "Plot copied to clipboard".
    Status(String),
}

/// Image formats for [`WaveformViewer::export_image`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Svg,
}

/// Plots one simulation result.
pub struct WaveformViewer {
    result: Option<Arc<SimulationResult>>,
    setup: PlotSetup,
    focus: FocusHandle,
    /// The "Add pane" plus at the bottom of the dock (D22), owned here so
    /// keyboard focus stays on it while panes are added.
    add_pane_focus: FocusHandle,
    traces: TraceStore,
    /// Extent of the result's x axis, computed once per result.
    x_extent: AxisExtent,
    colors: PlotColors,
    view: ui::state::ViewState,
}

impl EventEmitter<ViewerEvent> for WaveformViewer {}

impl EventEmitter<ViewerRequest> for WaveformViewer {}

impl Focusable for WaveformViewer {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl WaveformViewer {
    pub fn new(cx: &mut Context<Self>) -> Self {
        init(cx);
        Self {
            result: None,
            setup: PlotSetup::default(),
            focus: cx.focus_handle(),
            add_pane_focus: cx.focus_handle(),
            traces: TraceStore::default(),
            x_extent: AxisExtent::default(),
            colors: ui::theme::plot_colors(cx.theme()),
            view: ui::state::ViewState::default(),
        }
    }

    /// Shows `result` with `setup`. Traces whose signals are missing from the
    /// new result stay in the setup and are reported as unavailable.
    ///
    /// The setup is adapted to the result (an AC result gets the Bode
    /// layout, stale x ranges return to auto, a third unit in a pane moves
    /// to another pane) and every trace without a palette slot gets one
    /// (D11, deterministically: the lowest slot free in its pane, in pane
    /// order); when that changes it, [`ViewerEvent::SetupChanged`] is
    /// emitted.
    pub fn set_result(
        &mut self,
        result: Option<Arc<SimulationResult>>,
        setup: PlotSetup,
        cx: &mut Context<Self>,
    ) {
        self.result = result;
        self.setup = setup;
        self.x_extent = self
            .result
            .as_ref()
            .map(|r| AxisExtent::of(&r.axis.values))
            .unwrap_or_default();
        let adapted = match &self.result {
            Some(result) => model::setup::adapt_to_result(&mut self.setup, result),
            None => model::setup::assign_slots(&mut self.setup),
        };
        self.traces.clear();
        self.traces.sync(&self.setup, self.result.as_deref());
        self.view.reset_for_result();
        self.place_cursors();
        self.validate_selection();
        self.pin_cursor_traces();
        if adapted {
            cx.emit(ViewerEvent::SetupChanged);
        }
        cx.notify();
    }

    pub fn result(&self) -> Option<&Arc<SimulationResult>> {
        self.result.as_ref()
    }

    pub fn setup(&self) -> &PlotSetup {
        &self.setup
    }

    /// Adds a trace to the active pane unless it is already plotted there.
    /// A trace naming signals the current result lacks is added and listed
    /// as unavailable, like traces kept from an earlier run; expressions that
    /// do not parse are ignored ([`Self::try_add_trace`] reports why).
    ///
    /// The trace pins the lowest palette slot free in its pane (D11). Its
    /// unit picks its y axis (D9): the pane's first unit is on the left, a
    /// second one on the right. A trace whose unit would be a third one in
    /// the active pane goes to the first pane already showing that unit, or
    /// else to a new pane appended below; the active pane stays the same.
    pub fn add_trace(&mut self, expression: &str, cx: &mut Context<Self>) {
        if let Err(ExprError::UnknownSignal(_)) = self.try_add_trace(expression, cx) {
            self.insert_trace(expression.trim(), cx);
        }
    }

    /// Adds a trace like [`Self::add_trace`], but rejects expressions that
    /// cannot be plotted against the current result (the add-trace field
    /// shows the error inline). Returns whether the setup changed (`false`
    /// for a duplicate).
    pub fn try_add_trace(
        &mut self,
        expression: &str,
        cx: &mut Context<Self>,
    ) -> Result<bool, ExprError> {
        let expression = expression.trim();
        match &self.result {
            Some(result) => self.traces.check(expression, result)?,
            None => {
                xsim_waveform::Expression::parse(expression)?;
            }
        }
        Ok(self.insert_trace(expression, cx))
    }

    fn insert_trace(&mut self, expression: &str, cx: &mut Context<Self>) -> bool {
        let ac = self.is_ac();
        self.edit_setup(cx, |setup| model::setup::add_trace(setup, expression, ac))
    }

    /// Why `expression` cannot be plotted against the current result.
    pub fn trace_error(&self, expression: &str) -> Option<&ExprError> {
        self.traces.error(expression)
    }

    /// The cursor state: whether the cursors are shown and each one's x.
    pub fn cursors(&self) -> Cursors {
        self.view.cursors
    }

    /// Shows or hides the two cursors (Q131, D14, D19). Cursors shown for
    /// the first time sit at a quarter and three quarters of the visible
    /// range, on the selected trace (else the first visible trace of the
    /// active pane), and stay on that trace when another one is selected.
    pub fn set_cursors_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        self.view.cursors.enabled = visible;
        if visible && self.view.cursors.window().is_none() {
            self.place_cursors();
        }
        if visible {
            self.pin_cursor_traces();
        }
        cx.notify();
    }

    /// Shows or hides the measurements panel (Q135).
    pub fn set_measurements_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        self.view.measurements_open = visible;
        cx.notify();
    }

    /// Renders the panes to an image file (`width`×`height` pixels for raster
    /// formats).
    pub fn export_image(
        &self,
        path: &Path,
        format: ImageFormat,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        let bytes = self.image_bytes(format, width, height)?;
        export::write_file(path, &bytes)
    }

    /// The panes encoded as `format` at `width`×`height` pixels.
    pub fn image_bytes(
        &self,
        format: ImageFormat,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>, String> {
        let svg = self.svg(width, height)?;
        export::encode(&svg, format, width, height)
    }

    /// Writes every visible trace to CSV.
    pub fn export_csv(&self, path: &Path) -> Result<(), String> {
        export::write_file(path, self.csv()?.as_bytes())
    }

    /// Places a PNG rendering of the panes on the system clipboard (Q07).
    pub fn copy_image_to_clipboard(&self) -> Result<(), String> {
        let image = self.clipboard_image()?;
        arboard::Clipboard::new()
            .and_then(|mut clipboard| clipboard.set_image(image))
            .map_err(|e| format!("Could not copy the plot: {e}"))
    }
}

impl Render for WaveformViewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        ui::render(self, window, cx)
    }
}
