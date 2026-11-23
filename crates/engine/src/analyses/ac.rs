//! Small-signal AC sweep.
//!
//! The operating point is solved first (sources at their DC values) and
//! every device is linearised there. Each frequency then solves the complex
//! system driven by the sources' AC magnitude and phase. Frequencies include
//! both endpoints exactly; logarithmic sweeps place points at
//! `start·10^(k/points)` (or `2^(k/points)`) and end with `stop` even when the
//! span is not a whole number of steps.
//!
//! Terminal currents come from each device's own small-signal branch
//! currents ([`crate::devices::DeviceInstance::ac_currents`]), combined per
//! terminal exactly like the DC and transient currents. They are therefore
//! right when terminals share a node (a diode-connected transistor, a JFET
//! with gate tied to source) or several are grounded, and the BJT's
//! substrate current stays in the collector current instead of being
//! closed through a grounded terminal. Devices without that method (the
//! two-terminal passives and sources) use the terminal's row of
//! `A_dev·x − b_dev`, the device's contribution to the node's KCL equation,
//! with a single grounded terminal closed by KCL.

use super::{MAX_POINTS, Recorder, invalid, op, singular};
use crate::analysis::{AcSweep, SimOptions, SweepScale};
use crate::circuit::Circuit;
use crate::control::SimControl;
use crate::error::SimError;
use crate::mna::{Mna, Unknown};
use crate::result::{AnalysisKind, Axis, SimulationResult};
use crate::solver::{System, solver_for};
use num_complex::Complex64;
use std::f64::consts::TAU;

/// Logarithmic points closer to `stop` than this relative distance are
/// replaced by `stop`.
const ENDPOINT_TOLERANCE: f64 = 1e-9;

pub(crate) fn run(
    circuit: &Circuit,
    spec: &AcSweep,
    options: &SimOptions,
    control: &SimControl,
) -> Result<SimulationResult, SimError> {
    let frequencies = frequencies(spec)?;
    let mut mna = Mna::new(circuit, options);
    op::solve(&mut mna, control)?;

    let n = mna.size();
    let mut solver = solver_for::<Complex64>(options.solver, n);
    let mut system = System::new(n);
    let mut device_system = System::new(n);
    let mut recorder = Recorder::new(circuit, true);
    let mut row = Vec::with_capacity(recorder.width());
    let terminals: Vec<Vec<Unknown>> = circuit
        .elements
        .iter()
        .map(|e| {
            e.nodes
                .iter()
                .map(|&node| mna.layout().node(node))
                .collect()
        })
        .collect();
    let recorded: Vec<usize> = recorder.recorded_currents().collect();
    let widest = terminals.iter().map(Vec::len).max().unwrap_or(0);
    let mut currents = vec![Complex64::new(0.0, 0.0); widest];
    let mut x = Vec::with_capacity(n);

    for (k, &frequency) in frequencies.iter().enumerate() {
        let progress = k as f64 / frequencies.len() as f64;
        control.set_progress(progress);
        control.checkpoint()?;
        let omega = TAU * frequency;
        mna.load_ac(&mut system, omega);
        solver
            .solve_into(&system, &mut x)
            .map_err(|e| singular(&mna, e.row, progress))?;
        row.clear();
        row.extend_from_slice(&x[..mna.layout().circuit_node_unknowns()]);
        for (index, element_terminals) in terminals.iter().enumerate() {
            if recorded[index] == 0 {
                continue;
            }
            let currents = &mut currents[..element_terminals.len()];
            if !mna.devices_mut()[index].ac_currents(&x, omega, currents) {
                mna.load_ac_device(index, &mut device_system, omega);
                stamped_currents(&device_system, element_terminals, &x, currents);
            }
            row.extend_from_slice(&currents[..recorded[index]]);
        }
        recorder.push_complex(&row);
    }
    Ok(recorder.finish(
        AnalysisKind::Ac,
        Axis {
            name: "frequency".into(),
            unit: "Hz".into(),
            values: frequencies,
        },
    ))
}

/// Currents into each terminal from one device's stamps in `system`, for
/// devices without [`crate::devices::DeviceInstance::ac_currents`].
///
/// Terminals that share a node report their combined current on the first
/// of them. When exactly one terminal is grounded its current is the negated
/// sum of the others; with several grounded terminals those report zero.
/// For a two-terminal device this is exact unless both terminals are on one
/// node.
fn stamped_currents(
    system: &System<Complex64>,
    terminals: &[Unknown],
    x: &[Complex64],
    currents: &mut [Complex64],
) {
    currents.fill(Complex64::new(0.0, 0.0));
    for (k, &terminal) in terminals.iter().enumerate() {
        let Some(row) = terminal else { continue };
        if terminals[..k].contains(&terminal) {
            continue;
        }
        let stamped: Complex64 = system
            .entries()
            .iter()
            .filter(|&&(r, _, _)| r == row)
            .map(|&(_, col, value)| value * x[col])
            .sum();
        currents[k] = stamped - system.rhs()[row];
    }
    let mut grounded = terminals.iter().enumerate().filter(|(_, t)| t.is_none());
    if let (Some((k, _)), None) = (grounded.next(), grounded.next()) {
        currents[k] = -currents.iter().sum::<Complex64>();
    }
}

/// The swept frequencies, both endpoints included.
fn frequencies(spec: &AcSweep) -> Result<Vec<f64>, SimError> {
    let AcSweep {
        scale,
        points,
        start,
        stop,
    } = *spec;
    if !(start.is_finite() && stop.is_finite() && start > 0.0 && stop >= start) {
        return Err(invalid(format!(
            "AC sweep needs 0 < start ≤ stop with finite frequencies (got {start} Hz to {stop} Hz)"
        )));
    }
    if points == 0 {
        return Err(invalid("AC sweep needs at least one point"));
    }
    if start == stop {
        return Ok(vec![start]);
    }
    let base = match scale {
        SweepScale::Linear => {
            if points < 2 {
                return Err(invalid(
                    "a linear AC sweep between two frequencies needs at least 2 points",
                ));
            }
            if points > MAX_POINTS {
                return Err(invalid(format!(
                    "AC sweep needs more than {MAX_POINTS} points"
                )));
            }
            let span = stop - start;
            let last = points - 1;
            let mut values: Vec<f64> = (0..last)
                .map(|k| start + span * k as f64 / last as f64)
                .collect();
            values.push(stop);
            return Ok(values);
        }
        SweepScale::Decade => 10.0_f64,
        SweepScale::Octave => 2.0_f64,
    };
    let steps = points as f64 * (stop / start).log(base);
    if steps >= MAX_POINTS as f64 {
        return Err(invalid(format!(
            "AC sweep needs more than {MAX_POINTS} points"
        )));
    }
    let whole_steps = (steps + ENDPOINT_TOLERANCE).floor() as usize;
    let mut values: Vec<f64> = (0..=whole_steps)
        .map(|k| {
            // Whole decades (octaves) use exact integer powers.
            let (whole, part) = (k / points, k % points);
            start * base.powi(whole as i32) * base.powf(part as f64 / points as f64)
        })
        .collect();
    let last = values.len() - 1;
    if (values[last] - stop).abs() <= ENDPOINT_TOLERANCE * stop {
        values[last] = stop;
    } else {
        values.push(stop);
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sweep(
        scale: SweepScale,
        points: usize,
        start: f64,
        stop: f64,
    ) -> Result<Vec<f64>, SimError> {
        frequencies(&AcSweep {
            scale,
            points,
            start,
            stop,
        })
    }

    #[test]
    fn decade_sweeps_hit_every_decade_exactly() {
        let f = sweep(SweepScale::Decade, 10, 1.0, 1e3).unwrap();
        assert_eq!(f.len(), 31);
        assert_eq!([f[0], f[10], f[20], f[30]], [1.0, 10.0, 100.0, 1000.0]);
        assert!(f.windows(2).all(|w| w[1] > w[0]));
        let ratio = f[1] / f[0];
        assert!((ratio - 10f64.powf(0.1)).abs() < 1e-15);
    }

    #[test]
    fn partial_last_steps_end_at_stop() {
        let f = sweep(SweepScale::Decade, 1, 1.0, 50.0).unwrap();
        assert_eq!(f, [1.0, 10.0, 50.0]);
        let f = sweep(SweepScale::Octave, 2, 100.0, 400.0).unwrap();
        assert_eq!(f.len(), 5);
        assert_eq!(f[4], 400.0);
        assert!((f[1] - 100.0 * 2f64.sqrt()).abs() < 1e-12);
    }

    #[test]
    fn linear_sweeps_space_points_evenly() {
        assert_eq!(
            sweep(SweepScale::Linear, 5, 10.0, 50.0).unwrap(),
            [10.0, 20.0, 30.0, 40.0, 50.0]
        );
        assert_eq!(sweep(SweepScale::Linear, 1, 7.0, 7.0).unwrap(), [7.0]);
    }

    #[test]
    fn rejects_meaningless_sweeps() {
        assert!(sweep(SweepScale::Decade, 10, 0.0, 1.0).is_err());
        assert!(sweep(SweepScale::Decade, 10, 10.0, 1.0).is_err());
        assert!(sweep(SweepScale::Decade, 0, 1.0, 10.0).is_err());
        assert!(sweep(SweepScale::Linear, 1, 1.0, 10.0).is_err());
        assert!(sweep(SweepScale::Octave, 10, 1.0, f64::INFINITY).is_err());
        assert!(sweep(SweepScale::Decade, usize::MAX, 1.0, 10.0).is_err());
    }

    #[test]
    fn grounded_terminal_current_follows_from_kcl() {
        // A 2 S conductance from node 0 (unknown 0) to ground, at 1 V.
        let mut system = System::new(1);
        system.add(Some(0), Some(0), Complex64::new(2.0, 0.0));
        let x = [Complex64::new(1.0, 0.0)];
        let mut currents = [Complex64::new(7.0, 0.0); 2];
        stamped_currents(&system, &[Some(0), None], &x, &mut currents);
        assert_eq!(
            currents,
            [Complex64::new(2.0, 0.0), Complex64::new(-2.0, 0.0)]
        );
        stamped_currents(&system, &[None, Some(0)], &x, &mut currents);
        assert_eq!(
            currents,
            [Complex64::new(-2.0, 0.0), Complex64::new(2.0, 0.0)]
        );
    }
}
