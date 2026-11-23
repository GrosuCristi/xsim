//! The measurement cursors (Q131, D14, D19). A cursor moves on the x axis
//! only: it is a vertical line at x across every pane, dragged along x and
//! snapped to samples, with no horizontal line (D19). Each cursor follows a
//! trace, and its y is that trace's value at x, in the trace's own unit, so
//! a right-axis trace of a mixed-unit pane (D9) reads in its unit too.
//!
//! * A cursor follows the trace it is attached to (the readout's `1`/`2`
//!   buttons), also when its chip is dragged to another pane. A cursor
//!   turned on, or whose trace is removed, is attached to the selected
//!   trace, else the first visible trace of the active pane, and stays
//!   there when the selection changes. While its trace is hidden or
//!   unavailable it shows that default instead.
//!
//! One resolver, [`WaveformViewer::cursor_point`], feeds the overlay, the
//! hit test, the drags and the readout, so line and number always agree.

use super::state::PaneGeometry;
use crate::WaveformViewer;
use crate::model::cursors::{CursorId, CursorTrace, snap, value_at};
use crate::model::palette::Color;
use crate::model::setup;
use gpui_kit::*;
use xsim_project::YScale;

/// How close (in pixels) a press must be to grab a cursor line.
pub const CURSOR_GRAB: f32 = 6.0;

/// Where a cursor is.
#[derive(Debug, Clone, PartialEq)]
pub struct CursorPoint {
    pub id: CursorId,
    pub x: f64,
    /// The trace it follows, when there is one to follow.
    pub on: Option<OnTrace>,
}

/// A cursor's point on the trace it follows.
#[derive(Debug, Clone, PartialEq)]
pub struct OnTrace {
    pub pane: usize,
    pub trace: usize,
    pub expression: String,
    /// The trace's value at the cursor's x, in `unit` (the trace's own,
    /// also on a right axis, D9); `None` in a gap.
    pub y: Option<f64>,
    pub unit: String,
    pub color: Color,
    /// A decibel or phase pane, which the readout names.
    pub scale: YScale,
}

impl WaveformViewer {
    /// The trace cursor `id` follows, as `(pane, trace)`: the one it was
    /// attached to while that is plotted, otherwise the default.
    pub(crate) fn cursor_trace(&self, id: CursorId) -> Option<(usize, usize)> {
        let attached = self.view.cursor_traces[id.index()]
            .as_ref()
            .and_then(|followed| {
                let pane = self.setup.panes.get(followed.pane)?;
                let trace = pane
                    .traces
                    .iter()
                    .position(|t| t.expression == followed.expression)?;
                Some((followed.pane, trace))
            })
            .filter(|&(pane, trace)| self.plottable(pane, trace));
        attached.or_else(|| self.default_cursor_trace())
    }

    /// The selected trace, else the first visible trace of the active pane,
    /// else the first plottable trace.
    fn default_cursor_trace(&self) -> Option<(usize, usize)> {
        if let Some((pane, trace)) = self.view.selected
            && self.plottable(pane, trace)
        {
            return Some((pane, trace));
        }
        let active = setup::active_pane(&self.setup);
        let in_pane = |pane: usize| {
            let count = self.setup.panes.get(pane).map_or(0, |p| p.traces.len());
            (0..count)
                .find(|&trace| self.plottable(pane, trace))
                .map(|trace| (pane, trace))
        };
        in_pane(active).or_else(|| (0..self.setup.panes.len()).find_map(in_pane))
    }

    /// Makes cursor `id` follow trace `trace` of pane `pane`.
    pub(crate) fn attach_cursor(
        &mut self,
        id: CursorId,
        pane: usize,
        trace: usize,
        cx: &mut Context<Self>,
    ) {
        if !self.plottable(pane, trace) {
            return;
        }
        let expression = self.setup.panes[pane].traces[trace].expression.clone();
        self.view.cursor_traces[id.index()] = Some(CursorTrace { pane, expression });
        cx.notify();
    }

    /// Pins each shown cursor that follows no trace of the setup (none yet,
    /// or its trace was removed) to the trace it shows now, the default, so
    /// selecting another trace later does not move it. The viewer calls this
    /// after every setup edit and new result, and before a selection
    /// changes. A cursor whose trace is only hidden or unavailable keeps it,
    /// and shows the default meanwhile.
    pub(crate) fn pin_cursor_traces(&mut self) {
        if !self.view.cursors.enabled {
            return;
        }
        for id in CursorId::ALL {
            let live = self.view.cursor_traces[id.index()]
                .as_ref()
                .is_some_and(|followed| {
                    self.setup.panes.get(followed.pane).is_some_and(|pane| {
                        pane.traces
                            .iter()
                            .any(|t| t.expression == followed.expression)
                    })
                });
            if !live {
                self.view.cursor_traces[id.index()] =
                    self.default_cursor_trace()
                        .map(|(pane, trace)| CursorTrace {
                            pane,
                            expression: self.setup.panes[pane].traces[trace].expression.clone(),
                        });
            }
        }
    }

    /// Keeps cursor attachments on their traces after pane `removed` is
    /// gone.
    pub(crate) fn cursor_traces_after_pane_removal(&mut self, removed: usize) {
        for followed in &mut self.view.cursor_traces {
            *followed = match followed.take() {
                Some(t) if t.pane == removed => None,
                Some(t) if t.pane > removed => Some(CursorTrace {
                    pane: t.pane - 1,
                    ..t
                }),
                other => other,
            };
        }
    }

    /// Cursors following `expression` in pane `from` follow it to `to`.
    pub(crate) fn cursor_traces_after_move(&mut self, from: usize, to: usize, expression: &str) {
        for followed in self.view.cursor_traces.iter_mut().flatten() {
            if followed.pane == from && followed.expression == expression {
                followed.pane = to;
            }
        }
    }

    /// Where cursor `id` is, when cursors are shown and it is placed.
    pub(crate) fn cursor_point(&self, id: CursorId) -> Option<CursorPoint> {
        if !self.view.cursors.enabled {
            return None;
        }
        let x = self.view.cursors.get(id)?;
        let on = self.cursor_trace(id).and_then(|(pane, trace)| {
            let model = self.model()?;
            let p = self.setup.panes.get(pane)?;
            let t = p.traces.get(trace)?;
            let values = self.traces.values(&t.expression, p.y_scale)?;
            let y_log = p.y_scale == YScale::Log;
            Some(OnTrace {
                pane,
                trace,
                expression: t.expression.clone(),
                y: value_at(
                    &model.result.axis.values,
                    &values.y,
                    x,
                    model.x_log(),
                    y_log,
                ),
                unit: values.unit.clone(),
                color: self.colors.trace_color(t, trace),
                scale: p.y_scale,
            })
        });
        Some(CursorPoint { id, x, on })
    }

    /// The cursor whose line a press at `position` over pane `pane` grabs:
    /// the line inside the plot nearest the pointer's x, within
    /// [`CURSOR_GRAB`] pixels, cursor 1 when both are as near. Only x
    /// counts, as a cursor has no horizontal line (D19).
    pub(crate) fn cursor_hit(&self, pane: usize, position: Point<Pixels>) -> Option<CursorId> {
        let geometry = self.view.pane_geometry(pane)?;
        let cursors = &self.view.cursors;
        if !cursors.enabled {
            return None;
        }
        let plot = geometry.plot;
        let inside = plot.left().as_f32()..=plot.right().as_f32();
        let pointer = position.x.as_f32();
        CursorId::ALL
            .into_iter()
            .filter_map(|id| {
                let line = geometry.x.px(cursors.get(id)?);
                inside
                    .contains(&line)
                    .then_some((id, (line - pointer).abs()))
            })
            .filter(|&(_, distance)| distance <= CURSOR_GRAB)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    /// Moves cursor `id`'s line to the sample under `position`'s x; the
    /// pointer's y plays no part.
    pub(crate) fn move_cursor_x(&mut self, id: CursorId, position: Point<Pixels>) {
        let geometry = self
            .view
            .hover
            .and_then(|h| self.view.pane_geometry(h.pane))
            .or_else(|| self.any_geometry());
        let (Some(geometry), Some(result)) = (geometry, self.result.clone()) else {
            return;
        };
        let plot = geometry.plot;
        let x = geometry
            .x
            .value(position.x.clamp(plot.left(), plot.right()).as_f32());
        let axis = &result.axis.values;
        if let Some(index) = snap(axis, x, geometry.x.range().log) {
            self.view.cursors.set(id, axis[index]);
        }
    }

    /// Some painted pane's geometry; every pane maps x the same way.
    fn any_geometry(&self) -> Option<PaneGeometry> {
        self.view.geometry.borrow().iter().flatten().next().copied()
    }
}
