//! Migration from schema 1, the format of the first (linear-only) prototype.
//!
//! The result is a schema-2 document, which the schema-2 migration then
//! lifts to the current schema (doubling every coordinate, D22).
//!
//! What carries over:
//! * parts (R, C, L, V, I, ground) with their value, position, rotation and
//!   mirroring — the prototype used the symbol pin layout and transform of
//!   schema 2, so every pin stays where it was;
//! * wires (consecutive duplicate points, which the prototype tolerated,
//!   are collapsed);
//! * the analysis, which becomes the selected [`SimulationSetup`] choice;
//! * the plot's visible x and y ranges and the diagnostics dock state.
//!
//! The prototype stored no view, so the schematic opens at the default
//! view, fitted to the drawing.
//!
//! What is approximated:
//! * A prototype source value was at once its DC value, its in-phase AC
//!   amplitude and its transient step. It becomes a DC source with an equal
//!   AC stimulus (phase 180° for negative values); its transient value is
//!   the DC value.
//! * Prototype transients started from rest with every source switched on
//!   at `t = 0`, which is [`InitialState::ZeroState`].
//! * Sweeps were given as a total number of points. A DC sweep keeps its
//!   endpoints with the equivalent step. An AC sweep (logarithmic) becomes a
//!   per-decade sweep with the nearest whole number of points per decade.
//!
//! What is dropped:
//! * Plotted node indices and stored results: they refer to the
//!   prototype's internal node numbering, which has no reliable mapping to
//!   net names. The user re-runs the simulation and probes again.
//! * Pins the prototype marked as disconnected after a rotation: the
//!   current schema has no such state, so they connect to whatever touches
//!   them.
//! * The dark-mode and UI-scale preferences stored in the layout: they are
//!   application settings now, not part of a project.

use crate::document::{
    AnalysisChoice, CircuitDocument, PlotPane, PlotSetup, Project, SimulationSetup, WorkspaceLayout,
};
use crate::error::ProjectError;
use crate::schematic::{Component, Id, Orientation, Part, Point, Schematic, Wire};
use serde::Deserialize;
use serde_json::Value;
use xsim_spice_engine::{AcSweep, DcSweep, InitialState, Source, SweepScale, Transient};

/// The schema-2 project equivalent to the schema-1 document `value`.
pub(super) fn migrate(value: &Value) -> Result<Project, ProjectError> {
    let old = ProjectV1::deserialize(value)?;
    Ok(Project {
        schema_version: 2,
        name: old.name,
        circuits: old.circuits.into_iter().map(CircuitV1::migrate).collect(),
        active_circuit: old.active_circuit,
        layout: WorkspaceLayout {
            diagnostics_open: old.layout.diagnostics_open,
            ..WorkspaceLayout::default()
        },
    })
}

#[derive(Deserialize)]
struct ProjectV1 {
    name: String,
    circuits: Vec<CircuitV1>,
    #[serde(default)]
    active_circuit: usize,
    #[serde(default)]
    layout: LayoutV1,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct LayoutV1 {
    diagnostics_open: bool,
}

#[derive(Deserialize)]
struct CircuitV1 {
    name: String,
    schematic: SchematicV1,
    analysis: AnalysisV1,
    #[serde(default)]
    plot: PlotV1,
}

#[derive(Deserialize)]
struct SchematicV1 {
    components: Vec<ComponentV1>,
    wires: Vec<WireV1>,
}

#[derive(Deserialize)]
struct ComponentV1 {
    id: Id,
    name: String,
    kind: KindV1,
    position: Point,
    rotation: u8,
    mirror_x: bool,
    mirror_y: bool,
    value: f64,
}

#[derive(Clone, Copy, Deserialize)]
enum KindV1 {
    Resistor,
    Capacitor,
    Inductor,
    VoltageSource,
    CurrentSource,
    Ground,
}

#[derive(Deserialize)]
struct WireV1 {
    id: Id,
    points: Vec<Point>,
}

#[derive(Deserialize)]
enum AnalysisV1 {
    OperatingPoint,
    DcSweep {
        source: String,
        start: f64,
        stop: f64,
        points: usize,
    },
    AcSweep {
        start_hz: f64,
        stop_hz: f64,
        points: usize,
    },
    Transient {
        stop: f64,
        step: f64,
    },
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct PlotV1 {
    x_range: Option<[f64; 2]>,
    y_range: Option<[f64; 2]>,
}

impl CircuitV1 {
    fn migrate(self) -> CircuitDocument {
        CircuitDocument {
            name: self.name,
            schematic: self.schematic.migrate(),
            view: super::schema2::default_view(),
            simulation: self.analysis.migrate(),
            plot: PlotSetup {
                panes: vec![PlotPane {
                    x_range: self.plot.x_range,
                    y_range: self.plot.y_range,
                    ..PlotPane::default()
                }],
                active_pane: 0,
            },
            result: None,
        }
    }
}

impl SchematicV1 {
    fn migrate(self) -> Schematic {
        let components: Vec<Component> = self
            .components
            .into_iter()
            .map(ComponentV1::migrate)
            .collect();
        let wires: Vec<Wire> = self
            .wires
            .into_iter()
            .map(|mut w| {
                w.points.dedup();
                Wire {
                    id: w.id,
                    points: w.points,
                }
            })
            .collect();
        let next_id = components
            .iter()
            .map(|c| c.id)
            .chain(wires.iter().map(|w| w.id))
            .max()
            .map_or(1, |max| max.saturating_add(1));
        Schematic {
            components,
            wires,
            labels: Vec::new(),
            next_id,
        }
    }
}

impl ComponentV1 {
    fn migrate(self) -> Component {
        let value = self.value;
        let part = match self.kind {
            KindV1::Resistor => Part::Resistor { resistance: value },
            KindV1::Capacitor => Part::Capacitor {
                capacitance: value,
                initial_voltage: None,
            },
            KindV1::Inductor => Part::Inductor {
                inductance: value,
                initial_current: None,
            },
            KindV1::VoltageSource => Part::VoltageSource {
                source: source(value),
            },
            KindV1::CurrentSource => Part::CurrentSource {
                source: source(value),
            },
            KindV1::Ground => Part::Ground,
        };
        Component {
            id: self.id,
            name: self.name,
            position: self.position,
            orientation: Orientation {
                rotation: self.rotation,
                mirror_x: self.mirror_x,
                mirror_y: self.mirror_y,
            },
            part,
        }
    }
}

/// A prototype source value: DC value, signed AC amplitude and step height.
fn source(value: f64) -> Source {
    let phase = if value < 0.0 { 180.0 } else { 0.0 };
    Source::ac(value, value.abs(), phase)
}

impl AnalysisV1 {
    fn migrate(self) -> SimulationSetup {
        let mut setup = SimulationSetup::default();
        setup.selected = match self {
            Self::OperatingPoint => AnalysisChoice::OperatingPoint,
            Self::DcSweep {
                source,
                start,
                stop,
                points,
            } => {
                let step = linear_step(start, stop, points).unwrap_or(setup.dc.step);
                setup.dc = DcSweep {
                    source,
                    start,
                    stop,
                    step,
                };
                AnalysisChoice::DcSweep
            }
            Self::AcSweep {
                start_hz,
                stop_hz,
                points,
            } => {
                setup.ac = AcSweep {
                    scale: SweepScale::Decade,
                    points: points_per_decade(start_hz, stop_hz, points).unwrap_or(setup.ac.points),
                    start: start_hz,
                    stop: stop_hz,
                };
                AnalysisChoice::Ac
            }
            Self::Transient { stop, step } => {
                setup.transient = Transient {
                    stop,
                    step,
                    start_save: 0.0,
                    max_step: None,
                    initial_state: InitialState::ZeroState,
                };
                AnalysisChoice::Transient
            }
        };
        setup
    }
}

/// Step between `points` evenly spaced values from `start` to `stop`, when
/// that is a usable non-zero step.
fn linear_step(start: f64, stop: f64, points: usize) -> Option<f64> {
    let intervals = points.checked_sub(1).filter(|&n| n > 0)?;
    let step = (stop - start) / intervals as f64;
    (step.is_finite() && step != 0.0).then_some(step)
}

/// Whole points per decade closest to `points` logarithmically spaced
/// frequencies from `start` to `stop`.
fn points_per_decade(start: f64, stop: f64, points: usize) -> Option<usize> {
    let decades = (stop / start).log10();
    let intervals = points.checked_sub(1).filter(|&n| n > 0)?;
    let per_decade = (intervals as f64 / decades).round();
    (per_decade.is_finite() && per_decade >= 1.0).then_some(per_decade as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dc_points_become_the_equivalent_step() {
        assert_eq!(linear_step(0.0, 5.0, 51), Some(0.1));
        assert_eq!(linear_step(5.0, 0.0, 6), Some(-1.0));
        assert_eq!(linear_step(1.0, 1.0, 5), None);
        assert_eq!(linear_step(0.0, 1.0, 1), None);
        assert_eq!(linear_step(0.0, 1.0, 0), None);
    }

    #[test]
    fn ac_points_become_points_per_decade() {
        // 1 Hz to 1 MHz is six decades; 61 points are 10 per decade.
        assert_eq!(points_per_decade(1.0, 1e6, 61), Some(10));
        assert_eq!(points_per_decade(10.0, 1e3, 7), Some(3));
        assert_eq!(points_per_decade(10.0, 10.0, 7), None);
        assert_eq!(points_per_decade(10.0, 1e3, 1), None);
        assert_eq!(points_per_decade(-1.0, 1e3, 10), None);
    }

    #[test]
    fn negative_source_values_become_antiphase_ac_stimuli() {
        let s = source(-2.0);
        assert_eq!((s.dc, s.ac_magnitude, s.ac_phase), (-2.0, 2.0, 180.0));
        assert!(s.waveform.is_none());
        let s = source(0.0);
        assert_eq!((s.dc, s.ac_magnitude, s.ac_phase), (0.0, 0.0, 0.0));
    }
}
