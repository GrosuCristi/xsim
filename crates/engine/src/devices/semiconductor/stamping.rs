//! Stamping of linearised multi-terminal semiconductor branches.
//!
//! Devices evaluate their equations in *polarity-normalised* form (NPN /
//! N-channel orientation) and describe each internal current as a branch
//! between two unknowns whose value depends on one or more controlling
//! voltages. For a P-type device the actual branch current and controlling
//! voltages are the normalised ones times `−1`, while the partial derivatives
//! are unchanged; the helpers here apply that sign once.

use super::solution_value;
use crate::analysis::SimOptions;
use crate::devices::{AcContext, LoadContext, StateSlot};
use crate::mna::Unknown;
use num_complex::Complex64;

/// One controlling voltage `v(plus) − v(minus)` of a linearised branch, with
/// the partial derivative `g` of the branch current with respect to it and
/// its (normalised) value `v` at the linearisation point.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Control {
    pub(crate) plus: Unknown,
    pub(crate) minus: Unknown,
    pub(crate) g: f64,
    pub(crate) v: f64,
}

impl Control {
    pub(crate) fn new(plus: Unknown, minus: Unknown, g: f64, v: f64) -> Self {
        Self { plus, minus, g, v }
    }
}

/// Stamps a nonlinear branch current flowing from `from` to `to`, linearised
/// at the point where its normalised value is `current` and its controlling
/// voltages are `controls`. `sign` is `+1` for N-type and `−1` for P-type
/// devices.
pub(crate) fn stamp_branch(
    ctx: &mut LoadContext,
    sign: f64,
    from: Unknown,
    to: Unknown,
    current: f64,
    controls: &[Control],
) {
    let mut norton = current;
    for c in controls {
        ctx.stamp_transconductance(from, to, c.plus, c.minus, c.g);
        norton -= c.g * c.v;
    }
    ctx.stamp_current(from, to, sign * norton);
}

/// One small-signal controlling voltage of a branch with transadmittance `y`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct AcControl {
    pub(crate) plus: Unknown,
    pub(crate) minus: Unknown,
    pub(crate) y: Complex64,
}

impl AcControl {
    pub(crate) fn new(plus: Unknown, minus: Unknown, y: Complex64) -> Self {
        Self { plus, minus, y }
    }

    /// Conductance `g` plus capacitance `c` (admittance `g + jωc`).
    pub(crate) fn rc(plus: Unknown, minus: Unknown, g: f64, c: f64, omega: f64) -> Self {
        Self::new(plus, minus, Complex64::new(g, omega * c))
    }
}

/// Stamps the small-signal branch current from `from` to `to`.
pub(crate) fn stamp_ac_branch(
    ctx: &mut AcContext,
    from: Unknown,
    to: Unknown,
    controls: &[AcControl],
) {
    for c in controls {
        ctx.stamp_transadmittance(from, to, c.plus, c.minus, c.y);
    }
}

/// The small-signal branch current `Σ y·(v(plus) − v(minus))` that
/// [`stamp_ac_branch`] stamps for `controls`, evaluated at the AC solution
/// `x`.
pub(crate) fn ac_branch_current(x: &[Complex64], controls: &[AcControl]) -> Complex64 {
    controls
        .iter()
        .map(|c| c.y * (solution_value(x, c.plus) - solution_value(x, c.minus)))
        .sum()
}

/// Integrates a charge `q(v1, v2)` controlled by two voltages, given its
/// partial derivatives `dq_dv1` and `dq_dv2`. Returns `(i, g1, g2)`: the
/// charging current and its derivatives with respect to `v1` and `v2`.
///
/// [`LoadContext::integrate`] accepts one controlling quantity. Every
/// supported integration formula is affine in `q`, so its companion
/// conductance is a method coefficient times `dq/dv`; that coefficient is
/// recovered from the first derivative and applied to the second.
pub(crate) fn integrate_two_controls(
    ctx: &mut LoadContext,
    slot: StateSlot,
    q: f64,
    dq_dv1: f64,
    dq_dv2: f64,
) -> (f64, f64, f64) {
    let (i, g1) = ctx.integrate(slot, q, dq_dv1);
    let g2 = if dq_dv1 != 0.0 {
        g1 / dq_dv1 * dq_dv2
    } else {
        0.0
    };
    (i, g1, g2)
}

/// SPICE device convergence test: the change `delta` that the last Newton
/// update predicts for a branch current of value `current` must be within
/// `reltol·max(|current|, |current + delta|) + abstol`.
pub(crate) fn current_converged(current: f64, delta: f64, options: &SimOptions) -> bool {
    let predicted = current + delta;
    let tol = options.reltol * current.abs().max(predicted.abs()) + options.abstol;
    delta.is_finite() && delta.abs() <= tol
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convergence_test_scales_with_current() {
        let options = SimOptions::default();
        assert!(current_converged(1e-3, 1e-10, &options));
        assert!(!current_converged(1e-3, 1e-8, &options));
        // Near zero current the absolute tolerance governs.
        assert!(current_converged(0.0, 0.5e-12, &options));
        assert!(!current_converged(0.0, 2e-12, &options));
        assert!(!current_converged(1.0, f64::NAN, &options));
        assert!(!current_converged(1.0, f64::INFINITY, &options));
    }
}
