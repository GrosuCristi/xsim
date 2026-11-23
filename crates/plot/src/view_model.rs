//! Turns the viewer state (result, setup, evaluated traces, colours) into
//! the scene inputs both the screen and the exporters lay out.

use crate::model::axis::AxisRange;
use crate::model::palette::PlotColors;
use crate::model::ranges::{self, AxisExtent};
use crate::model::setup;
use crate::model::traces::TraceStore;
use crate::model::units::{PaneUnits, YAxis};
use crate::scene::layout::{LegendEntry, SceneAxis, ScenePane, SceneTrace, SceneX};
use xsim_project::{PlotPane, PlotSetup, Trace, YScale};
use xsim_spice_engine::{Axis, SimulationResult};

/// Borrowed viewer state for building scenes.
#[derive(Clone, Copy)]
pub struct PlotModel<'a> {
    pub result: &'a SimulationResult,
    /// Extent of `result.axis`, computed once per result.
    pub x_extent: AxisExtent,
    pub setup: &'a PlotSetup,
    pub traces: &'a TraceStore,
    pub colors: &'a PlotColors,
}

impl<'a> PlotModel<'a> {
    /// Whether the shared x axis is logarithmic.
    pub fn x_log(&self) -> bool {
        setup::x_log(self.setup, self.result.analysis)
    }

    /// The x range shown by every pane.
    pub fn x_range(&self) -> AxisRange {
        let stored = self.setup.panes.first().and_then(|p| p.x_range);
        ranges::x_range(stored, self.x_extent, self.x_log())
    }

    /// The widest x range zooming out and panning may show: the full data
    /// extent, which is also the automatic x range (D14).
    pub fn x_limit(&self) -> AxisRange {
        ranges::x_range(None, self.x_extent, self.x_log())
    }

    /// The shared x axis.
    pub fn x(&self) -> SceneX<'a> {
        let axis = &self.result.axis;
        SceneX {
            values: &axis.values,
            range: self.x_range(),
            unit: &axis.unit,
            title: x_axis_title(axis),
        }
    }

    /// The units of `pane`'s y axes (D9).
    pub fn units(&self, pane: &PlotPane) -> PaneUnits {
        PaneUnits::of(pane, |expression| self.traces.unit(expression))
    }

    /// The y axis `trace` of a pane with axes `units` is drawn against. A
    /// trace with a third unit, which `setup` moves to another pane, would
    /// share the left axis.
    pub fn axis_of(&self, units: &PaneUnits, trace: &Trace) -> YAxis {
        units
            .axis(self.traces.unit(&trace.expression))
            .unwrap_or(YAxis::Left)
    }

    /// The range of `axis` of pane `index`: its stored range (unless
    /// `fitted`), or one fitted to the visible traces drawn against it.
    fn axis_range(&self, index: usize, axis: YAxis, fitted: bool) -> AxisRange {
        let pane = &self.setup.panes[index];
        let units = self.units(pane);
        let log = pane.y_scale == YScale::Log;
        let extents = self
            .visible(pane)
            .filter(|(_, t)| self.axis_of(&units, t) == axis)
            .filter_map(|(_, t)| self.traces.values(&t.expression, pane.y_scale)?.extent);
        let stored = match axis {
            _ if fitted => None,
            YAxis::Left => pane.y_range,
            YAxis::Right => pane.y2_range,
        };
        ranges::y_range(stored, extents, log)
    }

    /// The y range pane `index` shows on its left axis.
    pub fn y_range(&self, index: usize) -> AxisRange {
        self.axis_range(index, YAxis::Left, false)
    }

    /// The range of the right y axis of pane `index`, when it has one.
    pub fn y2_range(&self, index: usize) -> Option<AxisRange> {
        let pane = &self.setup.panes[index];
        self.units(pane).right?;
        Some(self.axis_range(index, YAxis::Right, false))
    }

    /// The widest range zooming out and panning may show on the left y axis
    /// of pane `index`: its automatic range, fitted to the visible traces
    /// (D14).
    pub fn y_limit(&self, index: usize) -> AxisRange {
        self.axis_range(index, YAxis::Left, true)
    }

    /// Like [`Self::y_limit`], for the right y axis when there is one.
    pub fn y2_limit(&self, index: usize) -> Option<AxisRange> {
        let pane = &self.setup.panes[index];
        self.units(pane).right?;
        Some(self.axis_range(index, YAxis::Right, true))
    }

    /// The content of pane `index`.
    pub fn pane(&self, index: usize) -> ScenePane {
        let pane = &self.setup.panes[index];
        let units = self.units(pane);
        let traces: Vec<SceneTrace> = self
            .visible(pane)
            .filter_map(|(slot, trace)| {
                Some(SceneTrace {
                    color: self.colors.trace_color(trace, slot),
                    values: self.traces.values(&trace.expression, pane.y_scale)?.clone(),
                    axis: self.axis_of(&units, trace),
                })
            })
            .collect();
        let legend = self
            .visible(pane)
            .filter(|(_, t)| self.traces.values(&t.expression, pane.y_scale).is_some())
            .map(|(slot, trace)| LegendEntry {
                label: trace.expression.clone(),
                color: self.colors.trace_color(trace, slot),
                axis: self.axis_of(&units, trace),
            })
            .collect();
        let y2 = self.y2_range(index).map(|range| SceneAxis {
            range,
            unit: units.right.unwrap_or_default().to_string(),
        });
        ScenePane {
            traces,
            y: self.y_range(index),
            y_unit: self.y_unit(pane),
            legend,
            y2,
        }
    }

    /// Every pane's content, top to bottom.
    pub fn panes(&self) -> Vec<ScenePane> {
        (0..self.setup.panes.len()).map(|i| self.pane(i)).collect()
    }

    /// Visible traces of `pane` with their index in the pane.
    fn visible(&self, pane: &'a PlotPane) -> impl Iterator<Item = (usize, &'a Trace)> {
        pane.traces.iter().enumerate().filter(|(_, t)| t.visible)
    }

    /// The unit of the left y tick labels: that of the first trace with a
    /// known unit (D9), or the unit a decibel or phase scale implies.
    pub fn y_unit(&self, pane: &PlotPane) -> String {
        self.units(pane).left.to_string()
    }
}

/// The title under the plot's x axis: a sweep's source name (`"V1"`).
/// Time and frequency have none, as their tick labels carry the unit and
/// nothing else is left to say (D20).
pub fn x_axis_title(axis: &Axis) -> String {
    match axis.name.as_str() {
        "time" | "frequency" | "point" => String::new(),
        source => source.to_string(),
    }
}

/// The x column's heading in the cursor readout: `"Time (s)"`,
/// `"Frequency (Hz)"`, `"V1 (V)"`.
pub fn x_title(axis: &Axis) -> String {
    let name = match axis.name.as_str() {
        "time" => "Time".to_string(),
        "frequency" => "Frequency".to_string(),
        "point" => "Operating point".to_string(),
        other => other.to_string(),
    };
    if axis.unit.is_empty() {
        name
    } else {
        format!("{name} ({})", axis.unit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn axis(name: &str, unit: &str) -> Axis {
        Axis {
            name: name.into(),
            unit: unit.into(),
            values: vec![],
        }
    }

    #[test]
    fn readout_headings_name_quantities_and_units() {
        assert_eq!(x_title(&axis("time", "s")), "Time (s)");
        assert_eq!(x_title(&axis("frequency", "Hz")), "Frequency (Hz)");
        assert_eq!(x_title(&axis("V1", "V")), "V1 (V)");
        assert_eq!(x_title(&axis("temp", "")), "temp");
    }

    /// D20: no plot title only names a quantity and its unit; a sweep's
    /// x axis keeps the name of the swept source.
    #[test]
    fn only_a_sweep_titles_its_x_axis() {
        assert_eq!(x_axis_title(&axis("time", "s")), "");
        assert_eq!(x_axis_title(&axis("frequency", "Hz")), "");
        assert_eq!(x_axis_title(&axis("V1", "V")), "V1");
        assert_eq!(x_axis_title(&axis("I2", "A")), "I2");
    }
}
