//! Best-effort reading of files written by a newer xsim (Q45).
//!
//! Unknown fields are ignored. When a newer version changed or extended a
//! known structure (a new part kind, a new analysis), the parse degrades
//! instead of failing: list entries this version cannot read (circuits,
//! parts, wires, labels, plot panes, traces) are left out, and fields it
//! cannot read fall back to their defaults; a result's circuit fingerprint
//! this version cannot read becomes one that never matches, so the result
//! shows as out of date (INT-1), and a plot pane height it cannot use (not
//! a positive finite number, D23) becomes the default. The project keeps
//! the newer `schema_version`, so it cannot be saved over the original by
//! accident.

use crate::document::{
    CircuitDocument, PlotPane, PlotSetup, Project, SimulationSetup, StoredResult,
};
use crate::error::ProjectError;
use crate::schematic::Schematic;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

pub(super) fn parse(value: &Value, version: u32) -> Result<Project, ProjectError> {
    let mut project = match (parse_as::<Project>(value), value.as_object()) {
        (Some(project), _) => project,
        (None, Some(fields)) => project(fields)?,
        (None, None) => return Err(super::not_a_project()),
    };
    project.schema_version = version;
    for stored in project
        .circuits
        .iter_mut()
        .filter_map(|c| c.result.as_mut())
    {
        stored.forget_unreadable_fingerprint();
    }
    for pane in project
        .circuits
        .iter_mut()
        .flat_map(|c| c.plot.panes.iter_mut())
    {
        forget_unreadable_height(pane);
    }
    Ok(project)
}

/// A pane height this version cannot use (zero, negative or infinite, D23)
/// falls back to the default, so the file still opens, with that pane at
/// the default height.
fn forget_unreadable_height(pane: &mut PlotPane) {
    pane.height = pane.height.filter(|&h| PlotPane::is_valid_height(h));
}

fn project(fields: &Map<String, Value>) -> Result<Project, ProjectError> {
    let defaults = Project::default();
    let circuits = list(fields, "circuits", circuit);
    if circuits.is_empty() {
        return Err(ProjectError::Invalid(
            "This project was created by a newer version of xsim and contains no circuit this \
             version can read."
                .into(),
        ));
    }
    // Unreadable circuits may have been left out, so the index is clamped.
    let active_circuit = field(fields, "active_circuit")
        .unwrap_or(defaults.active_circuit)
        .min(circuits.len() - 1);
    Ok(Project {
        schema_version: defaults.schema_version,
        name: field(fields, "name").unwrap_or(defaults.name),
        circuits,
        active_circuit,
        layout: field(fields, "layout").unwrap_or(defaults.layout),
    })
}

fn circuit(value: &Value) -> Option<CircuitDocument> {
    if let Some(circuit) = parse_as(value) {
        return Some(circuit);
    }
    let fields = value.as_object()?;
    let defaults = CircuitDocument::default();
    Some(CircuitDocument {
        name: field(fields, "name").unwrap_or(defaults.name),
        schematic: fields
            .get("schematic")
            .map_or(defaults.schematic, schematic),
        view: field(fields, "view").unwrap_or(defaults.view),
        simulation: fields
            .get("simulation")
            .map_or(defaults.simulation, simulation),
        plot: fields.get("plot").map_or(defaults.plot, plot),
        result: field::<StoredResult>(fields, "result"),
    })
}

fn schematic(value: &Value) -> Schematic {
    if let Some(schematic) = parse_as(value) {
        return schematic;
    }
    let Some(fields) = value.as_object() else {
        return Schematic::default();
    };
    Schematic {
        components: list(fields, "components", parse_as),
        wires: list(fields, "wires", parse_as),
        labels: list(fields, "labels", parse_as),
        next_id: field(fields, "next_id").unwrap_or_default(),
    }
}

fn simulation(value: &Value) -> SimulationSetup {
    if let Some(setup) = parse_as(value) {
        return setup;
    }
    let defaults = SimulationSetup::default();
    let Some(fields) = value.as_object() else {
        return defaults;
    };
    SimulationSetup {
        selected: field(fields, "selected").unwrap_or(defaults.selected),
        dc: field(fields, "dc").unwrap_or(defaults.dc),
        ac: field(fields, "ac").unwrap_or(defaults.ac),
        transient: field(fields, "transient").unwrap_or(defaults.transient),
        options: field(fields, "options").unwrap_or(defaults.options),
    }
}

fn plot(value: &Value) -> PlotSetup {
    if let Some(plot) = parse_as(value) {
        return plot;
    }
    let Some(fields) = value.as_object() else {
        return PlotSetup::default();
    };
    let panes = list(fields, "panes", pane);
    let active_pane = field(fields, "active_pane")
        .unwrap_or(0)
        .min(panes.len().saturating_sub(1));
    PlotSetup { panes, active_pane }
}

fn pane(value: &Value) -> Option<PlotPane> {
    if let Some(pane) = parse_as(value) {
        return Some(pane);
    }
    let fields = value.as_object()?;
    let defaults = PlotPane::default();
    Some(PlotPane {
        traces: list(fields, "traces", parse_as),
        x_range: field(fields, "x_range").unwrap_or(defaults.x_range),
        y_range: field(fields, "y_range").unwrap_or(defaults.y_range),
        x_log: field(fields, "x_log").unwrap_or(defaults.x_log),
        y_scale: field(fields, "y_scale").unwrap_or(defaults.y_scale),
        y2_range: field(fields, "y2_range").unwrap_or(defaults.y2_range),
        height: field(fields, "height").unwrap_or(defaults.height),
    })
}

fn parse_as<T: DeserializeOwned>(value: &Value) -> Option<T> {
    T::deserialize(value).ok()
}

fn field<T: DeserializeOwned>(fields: &Map<String, Value>, key: &str) -> Option<T> {
    fields.get(key).and_then(parse_as)
}

/// The readable entries of the array `fields[key]`.
fn list<T>(fields: &Map<String, Value>, key: &str, entry: impl Fn(&Value) -> Option<T>) -> Vec<T> {
    fields
        .get(key)
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(entry).collect())
        .unwrap_or_default()
}
