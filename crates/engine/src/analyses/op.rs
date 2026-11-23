//! DC operating point.
//!
//! Capacitors are open, inductors are shorts and sources apply their DC
//! values. The result has one sample on the axis `"point"` (unit `""`) at 0.
//!
//! Every operating-point solve (this analysis, each DC-sweep point, the AC
//! linearisation point and the transient's starting point) goes through
//! [`newton::operating_point`], which applies D15: a node reached only
//! through capacitors is tied to ground through gmin and the run warns
//! about it (see [`crate::SimControl::warnings`]), while a node fed only by
//! a current source, or connected to nothing, is reported as an error that
//! names it and says why. The conversion of solve failures into public
//! errors with their location (D7) also lives here.

use super::Recorder;
use crate::analysis::SimOptions;
use crate::circuit::Circuit;
use crate::circuit::topology::Regime;
use crate::control::SimControl;
use crate::devices::LoadMode;
use crate::error::{FailurePoint, SimError};
use crate::mna::{Conditions, Mna};
use crate::newton::{self, DcFailure};
use crate::result::{AnalysisKind, Axis, SimulationResult};
use crate::solver::SolveFailure;

pub(crate) fn run(
    circuit: &Circuit,
    options: &SimOptions,
    control: &SimControl,
) -> Result<SimulationResult, SimError> {
    let mut mna = Mna::new(circuit, options);
    let x = solve(&mut mna, control)?;
    let mut recorder = Recorder::new(circuit, false);
    recorder.record(&mna, &x);
    Ok(recorder.finish(
        AnalysisKind::OperatingPoint,
        Axis {
            name: "point".into(),
            unit: String::new(),
            values: vec![0.0],
        },
    ))
}

/// Solves and accepts the operating point with DC source values (also the
/// linearisation point of the AC analysis). Devices are left loaded at it.
pub(crate) fn solve(mna: &mut Mna, control: &SimControl) -> Result<Vec<f64>, SimError> {
    let mut x = vec![0.0; mna.size()];
    let conditions = Conditions::dc(mna.options(), false);
    newton::operating_point(mna, &mut x, conditions, true, control)
        .map_err(|failure| dc_failure(failure, mna, None, 0.0))?;
    mna.accept(&x, 0.0);
    Ok(x)
}

/// Converts a failed operating-point solve into the public error. `at` is
/// where on the analysis axis it happened (the swept source's value, or
/// `t = 0` for a transient's starting point).
pub(crate) fn dc_failure(
    failure: DcFailure,
    mna: &Mna,
    at: Option<FailurePoint>,
    progress: f64,
) -> SimError {
    match failure {
        DcFailure::Singular(row, kind) => solve_failure(mna, row, kind, None, at, progress),
        DcFailure::NoConvergence { detail, worst } => SimError::NoConvergence {
            at,
            unknown: worst.map(|i| mna.layout().name(i).to_string()),
            progress,
            detail,
        },
        DcFailure::Interrupted(error) => error,
    }
}

/// The public error for a linear solve that failed at unknown `row` after
/// the last [`Mna::solve_into`]: a transient step (located at its time) or an AC
/// frequency point (whose system is not the one [`Mna`] holds).
pub(crate) fn step_failure(mna: &Mna, row: usize, progress: f64) -> SimError {
    match mna.last_failure() {
        Some(kind) => {
            let at = mna
                .last_conditions()
                .filter(|c| c.mode == LoadMode::Transient)
                .map(|c| FailurePoint::time(c.time));
            solve_failure(mna, row, kind, None, at, progress)
        }
        // The last solve of the circuit's own system succeeded, so this is
        // the AC system, where capacitors and inductors conduct as in a
        // transient step.
        None => solve_failure(
            mna,
            row,
            SolveFailure::Singular,
            Some(Regime::Transient),
            None,
            progress,
        ),
    }
}

fn solve_failure(
    mna: &Mna,
    row: usize,
    kind: SolveFailure,
    regime: Option<Regime>,
    at: Option<FailurePoint>,
    progress: f64,
) -> SimError {
    let unknown = mna.layout().name(row).to_string();
    match kind {
        SolveFailure::Singular => SimError::SingularMatrix {
            unknown,
            reason: mna.singular_reason(row, regime),
            at,
            progress,
        },
        SolveFailure::NonFinite => SimError::NumericalOverflow {
            unknown,
            at,
            progress,
        },
    }
}
