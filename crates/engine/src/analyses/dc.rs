//! DC sweep.
//!
//! The swept source's DC value takes `start + k·step` for `k = 0, 1, …` and
//! finally exactly `stop`, so both endpoints are included even when the span
//! is not a whole number of steps. Each point starts Newton from the
//! previous solution (continuation) and falls back to the full convergence
//! ladder when that fails. The axis is named after the source, in `V` or `A`.
//! A failure names the source and the value it failed at (D7), for example
//! "did not converge at V1 = 50 V".

use super::op::dc_failure;
use super::{MAX_POINTS, Recorder, invalid};
use crate::analysis::{DcSweep, SimOptions};
use crate::circuit::{Circuit, Device};
use crate::control::SimControl;
use crate::error::{FailurePoint, SimError};
use crate::mna::{Conditions, Mna};
use crate::newton;
use crate::result::{AnalysisKind, Axis, SimulationResult};

/// Sweep values closer to `stop` than this fraction of a step are replaced
/// by `stop`, absorbing the rounding of `start + k·step`.
const ENDPOINT_TOLERANCE: f64 = 1e-9;

pub(crate) fn run(
    circuit: &Circuit,
    spec: &DcSweep,
    options: &SimOptions,
    control: &SimControl,
) -> Result<SimulationResult, SimError> {
    let (source, unit) = swept_source(circuit, &spec.source)?;
    let values = sweep_values(spec)?;
    let mut mna = Mna::new(circuit, options);
    let mut recorder = Recorder::new(circuit, false);
    let mut x = vec![0.0; mna.size()];
    let conditions = Conditions::dc(options, false);
    for (k, &value) in values.iter().enumerate() {
        let progress = k as f64 / values.len() as f64;
        control.set_progress(progress);
        control.checkpoint()?;
        mna.devices_mut()[source].set_dc(value);
        newton::operating_point(&mut mna, &mut x, conditions, k == 0, control).map_err(
            |failure| {
                let at = FailurePoint::sweep(&spec.source, value, unit);
                dc_failure(failure, &mna, Some(at), progress)
            },
        )?;
        mna.accept(&x, 0.0);
        recorder.record(&mna, &x);
    }
    Ok(recorder.finish(
        AnalysisKind::DcSweep,
        Axis {
            name: spec.source.clone(),
            unit: unit.into(),
            values,
        },
    ))
}

/// Index and unit of the independent source called `name`.
fn swept_source(circuit: &Circuit, name: &str) -> Result<(usize, &'static str), SimError> {
    let index = circuit
        .elements
        .iter()
        .position(|e| e.name == name)
        .ok_or_else(|| invalid(format!("DC sweep source {name} does not exist")))?;
    match circuit.elements[index].device {
        Device::VoltageSource(_) => Ok((index, "V")),
        Device::CurrentSource(_) => Ok((index, "A")),
        _ => Err(invalid(format!(
            "DC sweep source {name} is not an independent voltage or current source"
        ))),
    }
}

/// The swept values, both endpoints included.
fn sweep_values(spec: &DcSweep) -> Result<Vec<f64>, SimError> {
    let DcSweep {
        start, stop, step, ..
    } = *spec;
    if !(start.is_finite() && stop.is_finite() && step.is_finite()) {
        return Err(invalid("DC sweep start, stop and step must be finite"));
    }
    if step == 0.0 {
        return Err(invalid("DC sweep step must not be zero"));
    }
    let span = stop - start;
    if span == 0.0 {
        return Ok(vec![start]);
    }
    if span.signum() != step.signum() {
        return Err(invalid(format!(
            "DC sweep step {step} moves away from the stop value {stop} (start {start})"
        )));
    }
    let intervals = span / step;
    if intervals >= MAX_POINTS as f64 {
        return Err(invalid(format!(
            "DC sweep from {start} to {stop} in steps of {step} needs more than {MAX_POINTS} points"
        )));
    }
    let whole_steps = (intervals + ENDPOINT_TOLERANCE).floor() as usize;
    let mut values: Vec<f64> = (0..=whole_steps).map(|k| start + k as f64 * step).collect();
    let last = values.len() - 1;
    if (values[last] - stop).abs() <= ENDPOINT_TOLERANCE * step.abs() {
        values[last] = stop;
    } else {
        values.push(stop);
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sweep(start: f64, stop: f64, step: f64) -> Result<Vec<f64>, SimError> {
        sweep_values(&DcSweep {
            source: "V1".into(),
            start,
            stop,
            step,
        })
    }

    #[test]
    fn includes_both_endpoints_exactly() {
        assert_eq!(sweep(0.0, 1.0, 0.25).unwrap(), [0.0, 0.25, 0.5, 0.75, 1.0]);
        let tenths = sweep(0.0, 1.0, 0.1).unwrap();
        assert_eq!(tenths.len(), 11);
        assert_eq!(tenths[10], 1.0);
        assert_eq!(sweep(3.0, -3.0, -3.0).unwrap(), [3.0, 0.0, -3.0]);
        assert_eq!(
            sweep(0.0, 1.0, 0.3).unwrap(),
            [0.0, 0.3, 0.6, 0.8999999999999999, 1.0]
        );
        assert_eq!(sweep(2.0, 2.0, 5.0).unwrap(), [2.0]);
    }

    #[test]
    fn rejects_unusable_steps() {
        assert!(sweep(0.0, 1.0, 0.0).is_err());
        assert!(sweep(0.0, 1.0, -0.1).is_err());
        assert!(sweep(1.0, 0.0, 0.1).is_err());
        assert!(sweep(0.0, f64::NAN, 0.1).is_err());
        assert!(sweep(0.0, 1.0, 1e-12).is_err());
    }
}
