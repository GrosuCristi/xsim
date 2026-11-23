//! The columns of a CSV export, written by `xsim_waveform::export::csv`.

use crate::model::setup::same_expression;
use crate::model::traces::{TraceStore, evaluate};
use xsim_project::PlotSetup;
use xsim_spice_engine::SimulationResult;
use xsim_waveform::Series;

/// Every visible trace that could be evaluated, once each (a Bode layout
/// shows the same trace in two panes).
pub fn trace_columns(setup: &PlotSetup, store: &TraceStore) -> Vec<(String, Series)> {
    let mut columns: Vec<(String, Series)> = Vec::new();
    for trace in setup.panes.iter().flat_map(|p| &p.traces) {
        if !trace.visible
            || columns
                .iter()
                .any(|(name, _)| same_expression(name, &trace.expression))
        {
            continue;
        }
        if let Some(evaluated) = store.evaluated(&trace.expression) {
            columns.push((trace.expression.clone(), evaluated.series.clone()));
        }
    }
    columns
}

/// Every recorded signal of `result`, for operating points (which have a
/// table instead of traces).
pub fn signal_columns(result: &SimulationResult) -> Vec<(String, Series)> {
    result
        .signals
        .iter()
        .filter_map(|signal| {
            let evaluated = evaluate(&signal.name, result).ok()?;
            Some((signal.name.clone(), evaluated.series))
        })
        .collect()
}

/// CSV text of `columns` against the x axis of `result`.
pub fn csv(result: &SimulationResult, columns: &[(String, Series)]) -> String {
    xsim_waveform::export::csv(result, columns)
}
