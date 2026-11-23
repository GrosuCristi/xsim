//! Structural validation of a project, applied on load and before saving.
//!
//! Only what keeps the data usable is checked: numbers JSON can represent
//! (finite), indices in range, unique ids, well-formed wires and bounded
//! coordinates. Electrical validity — a negative resistance, an empty sweep
//! — belongs to the netlister and the engine and is reported when the user
//! simulates, so such a project can still be opened, viewed and fixed.

use crate::document::{
    CircuitDocument, MAX_FINGERPRINT_LEN, PlotPane, PlotSetup, Project, SchematicView,
    SimulationSetup, StoredResult,
};
use crate::error::ProjectError;
use crate::schematic::{Part, Point, Schematic};
use std::collections::BTreeSet;
use xsim_spice_engine::{
    AcSweep, Analysis, DcSweep, SignalData, SimOptions, SimulationResult, Source, Transient,
    Waveform,
};

/// Largest accepted coordinate magnitude in grid units: far beyond any real
/// drawing, and small enough that symbol offsets and editor moves cannot
/// overflow `i32`. It is the drawing area of schema 2 (±10⁸ units twice as
/// large, D22), so every valid schema-2 file migrates to a valid one.
pub(crate) const MAX_COORDINATE: u32 = 200_000_000;

/// Checks the invariants every loaded or saved project must satisfy.
pub(crate) fn validate(project: &Project) -> Result<(), ProjectError> {
    check_project(project).map_err(ProjectError::Invalid)
}

/// `Err` carries a user-facing description of the first problem found.
type Check = Result<(), String>;

fn check_project(project: &Project) -> Check {
    if project.circuits.is_empty() {
        return Err("The project contains no circuits.".into());
    }
    if project.active_circuit >= project.circuits.len() {
        return Err(format!(
            "The active circuit {} does not exist (the project has {}).",
            project.active_circuit,
            project.circuits.len()
        ));
    }
    finite(
        "The workspace layout",
        [f64::from(project.layout.schematic_fraction)],
    )?;
    for circuit in &project.circuits {
        check_circuit(circuit).map_err(|e| format!("Circuit \"{}\": {e}.", circuit.name))?;
    }
    Ok(())
}

fn check_circuit(circuit: &CircuitDocument) -> Check {
    check_schematic(&circuit.schematic)?;
    check_view(&circuit.view)?;
    check_setup(&circuit.simulation)?;
    check_plot(&circuit.plot)?;
    if let Some(stored) = &circuit.result {
        finite("the stored analysis", analysis_values(&stored.analysis))?;
        if let Some(fingerprint) = &stored.circuit_fingerprint
            && !StoredResult::is_valid_fingerprint(fingerprint)
        {
            return Err(format!(
                "the stored result has a malformed circuit fingerprint (at most \
                 {MAX_FINGERPRINT_LEN} ASCII letters, digits and dashes)"
            ));
        }
        check_result(&stored.result)?;
    }
    Ok(())
}

fn check_schematic(schematic: &Schematic) -> Check {
    let mut ids = BTreeSet::new();
    let all_ids = schematic
        .components
        .iter()
        .map(|c| c.id)
        .chain(schematic.wires.iter().map(|w| w.id))
        .chain(schematic.labels.iter().map(|l| l.id));
    for id in all_ids {
        if !ids.insert(id) {
            return Err(format!("the object id {id} is used more than once"));
        }
    }
    for component in &schematic.components {
        let name = &component.name;
        check_point(component.position).map_err(|e| format!("{name} {e}"))?;
        if component.orientation.rotation > 3 {
            return Err(format!(
                "{name} has rotation {}, which is not 0 to 3",
                component.orientation.rotation
            ));
        }
        finite(name, part_values(&component.part))?;
    }
    for wire in &schematic.wires {
        if wire.points.len() < 2 {
            return Err(format!("wire {} has fewer than two points", wire.id));
        }
        if wire.points.windows(2).any(|w| w[0] == w[1]) {
            return Err(format!("wire {} repeats a point", wire.id));
        }
        for &p in &wire.points {
            check_point(p).map_err(|e| format!("wire {} {e}", wire.id))?;
        }
    }
    for label in &schematic.labels {
        check_point(label.position).map_err(|e| format!("label \"{}\" {e}", label.name))?;
    }
    Ok(())
}

fn check_point(p: Point) -> Check {
    if p.x.unsigned_abs() > MAX_COORDINATE || p.y.unsigned_abs() > MAX_COORDINATE {
        return Err(format!(
            "lies at ({}, {}), outside the drawing area of ±{MAX_COORDINATE}",
            p.x, p.y
        ));
    }
    Ok(())
}

fn check_view(view: &SchematicView) -> Check {
    finite(
        "the schematic view",
        [view.center[0], view.center[1], view.zoom].map(f64::from),
    )
}

fn check_setup(setup: &SimulationSetup) -> Check {
    let values = dc_values(&setup.dc)
        .into_iter()
        .chain(ac_values(&setup.ac))
        .chain(transient_values(&setup.transient))
        .chain(option_values(&setup.options));
    finite("the simulation settings", values)
}

fn check_plot(plot: &PlotSetup) -> Check {
    if plot.active_pane >= plot.panes.len().max(1) {
        return Err(format!(
            "the active plot pane {} does not exist",
            plot.active_pane
        ));
    }
    let ranges = plot
        .panes
        .iter()
        .flat_map(|p| p.x_range.into_iter().chain(p.y_range).chain(p.y2_range))
        .flatten();
    finite("a plot range", ranges)?;
    // Pane heights are relative weights (D23): a zero, negative or infinite
    // weight would give a pane no height or all of it.
    for (index, pane) in plot.panes.iter().enumerate() {
        if let Some(height) = pane.height
            && !PlotPane::is_valid_height(height)
        {
            return Err(format!(
                "plot pane {} has the height {height}, which is not a positive finite number",
                index + 1
            ));
        }
    }
    Ok(())
}

fn check_result(result: &SimulationResult) -> Check {
    let samples = result.axis.values.len();
    finite("the stored result axis", result.axis.values.iter().copied())?;
    for signal in &result.signals {
        let channels: Vec<&[f64]> = match &signal.data {
            SignalData::Real(values) => vec![values],
            SignalData::Complex { re, im } => vec![re, im],
        };
        if channels.iter().any(|c| c.len() != samples) {
            return Err(format!(
                "the stored signal {} does not have {samples} samples",
                signal.name
            ));
        }
        let values = channels.iter().flat_map(|c| c.iter().copied());
        finite(&format!("the stored signal {}", signal.name), values)?;
    }
    Ok(())
}

fn finite(what: &str, values: impl IntoIterator<Item = f64>) -> Check {
    if values.into_iter().all(f64::is_finite) {
        Ok(())
    } else {
        Err(format!(
            "{what} contains a value that is not a finite number"
        ))
    }
}

fn part_values(part: &Part) -> Vec<f64> {
    match part {
        Part::Resistor { resistance } => vec![*resistance],
        Part::Capacitor {
            capacitance,
            initial_voltage,
        } => [*capacitance].into_iter().chain(*initial_voltage).collect(),
        Part::Inductor {
            inductance,
            initial_current,
        } => [*inductance].into_iter().chain(*initial_current).collect(),
        Part::VoltageSource { source } | Part::CurrentSource { source } => source_values(source),
        // A switch's control names another part; whether that part exists
        // and is a voltage source is the netlister's check, like a model
        // name, so a file with a dangling control still opens.
        Part::VoltageSwitch { model } => vec![model.ron, model.roff, model.vt, model.vh],
        Part::CurrentSwitch { model, .. } => vec![model.ron, model.roff, model.it, model.ih],
        Part::Diode { .. } | Part::Bjt { .. } | Part::Jfet { .. } | Part::Ground => Vec::new(),
    }
}

fn source_values(source: &Source) -> Vec<f64> {
    let mut values = vec![source.dc, source.ac_magnitude, source.ac_phase];
    match &source.waveform {
        None => {}
        Some(Waveform::Pulse {
            initial,
            pulsed,
            delay,
            rise,
            fall,
            width,
            period,
            cycles: _,
        }) => values.extend([initial, pulsed, delay, rise, fall, width, period]),
        Some(Waveform::Sine {
            offset,
            amplitude,
            frequency,
            delay,
            damping,
            phase,
            cycles,
        }) => {
            values.extend([offset, amplitude, frequency, delay, damping, phase]);
            values.extend(cycles);
        }
        Some(Waveform::Exp {
            initial,
            pulsed,
            rise_delay,
            rise_tau,
            fall_delay,
            fall_tau,
        }) => values.extend([initial, pulsed, rise_delay, rise_tau, fall_delay, fall_tau]),
        Some(Waveform::Pwl { points, repeat: _ }) => {
            values.extend(points.iter().flat_map(|&(t, v)| [t, v]));
        }
        Some(Waveform::Sffm {
            offset,
            amplitude,
            carrier,
            index,
            signal,
        }) => values.extend([offset, amplitude, carrier, index, signal]),
    }
    values
}

fn analysis_values(analysis: &Analysis) -> Vec<f64> {
    match analysis {
        Analysis::OperatingPoint => Vec::new(),
        Analysis::DcSweep(dc) => dc_values(dc).to_vec(),
        Analysis::Ac(ac) => ac_values(ac).to_vec(),
        Analysis::Transient(transient) => transient_values(transient),
    }
}

fn dc_values(dc: &DcSweep) -> [f64; 3] {
    [dc.start, dc.stop, dc.step]
}

fn ac_values(ac: &AcSweep) -> [f64; 2] {
    [ac.start, ac.stop]
}

fn transient_values(transient: &Transient) -> Vec<f64> {
    [transient.stop, transient.step, transient.start_save]
        .into_iter()
        .chain(transient.max_step)
        .collect()
}

fn option_values(options: &SimOptions) -> [f64; 8] {
    [
        options.reltol,
        options.abstol,
        options.vntol,
        options.chgtol,
        options.trtol,
        options.gmin,
        options.temperature,
        options.min_step_ratio,
    ]
}
