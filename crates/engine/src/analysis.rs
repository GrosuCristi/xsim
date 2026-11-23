//! Analysis specifications and solver options.

use crate::error::CircuitError;
use serde::{Deserialize, Serialize};

/// What to simulate (Q115).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Analysis {
    /// DC operating point: capacitors open, inductors shorted.
    OperatingPoint,
    /// Repeated operating points while one independent source's DC value is swept.
    DcSweep(DcSweep),
    /// Small-signal frequency response linearised at the operating point.
    Ac(AcSweep),
    /// Time-domain simulation.
    Transient(Transient),
}

/// Linear DC sweep of an independent source's DC value. Includes both
/// endpoints; `step` may be negative to sweep downwards.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DcSweep {
    /// Name of a voltage or current source element.
    pub source: String,
    pub start: f64,
    pub stop: f64,
    pub step: f64,
}

/// Frequency spacing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SweepScale {
    /// `points` total, evenly spaced, including both endpoints.
    Linear,
    /// `points` per decade, logarithmically spaced, including both endpoints.
    Decade,
    /// `points` per octave, logarithmically spaced, including both endpoints.
    Octave,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AcSweep {
    pub scale: SweepScale,
    pub points: usize,
    /// Start and stop frequency in Hz (`0 < start ≤ stop`).
    pub start: f64,
    pub stop: f64,
}

/// How the transient analysis obtains its `t = 0` state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InitialState {
    /// Solve the DC operating point with sources at their `t = 0` values.
    /// Capacitor and inductor initial conditions constrain it (D16, as
    /// LTspice without `uic`): a capacitor with an initial voltage is held at
    /// it, an inductor with an initial current carries it, and the rest of
    /// the circuit is solved around them. Where other parts override one (a
    /// voltage source across the capacitor, a current source in series with
    /// the inductor), they win and the run warns, naming the part; an
    /// initial current with no path at all (only capacitors without an
    /// initial voltage around the inductor) is an error naming it.
    OperatingPoint,
    /// Skip the operating point: every capacitor starts at its
    /// `initial_voltage` (default 0 V) and every inductor at its
    /// `initial_current` (default 0 A), and sources apply their `t = 0` value
    /// immediately ("start from rest", SPICE `UIC`). The `t = 0` sample is
    /// the solution consistent with that state, which LTspice shows from its
    /// first step on.
    ZeroState,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transient {
    /// End time (s).
    pub stop: f64,
    /// Output sample spacing (s). Results contain every multiple of it from
    /// `start_save` on, every breakpoint and the stop time, plus the other
    /// accepted solver points at or after `start_save` that linear
    /// interpolation needs to reproduce each signal within
    /// `reltol·swing + vntol` (`abstol` for currents, D18); see
    /// [`crate::SimulationResult`] for exact sampling rules.
    pub step: f64,
    /// Results before this time are discarded (s).
    pub start_save: f64,
    /// Largest internal timestep (s). `None` means `(stop − start_save) / 50`,
    /// as in SPICE. An explicit value may imply at most 10 000 000 steps.
    pub max_step: Option<f64>,
    pub initial_state: InitialState,
}

/// Numerical integration methods for transient analysis (Q105).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntegrationMethod {
    /// First order, L-stable, strongly damped.
    BackwardEuler,
    /// Second order, A-stable; can ring on stiff discontinuities.
    Trapezoidal,
    /// TR-BDF2: second order and L-stable; trapezoidal accuracy without its
    /// ringing.
    ModifiedTrapezoidal,
    /// Second-order backward differentiation formula (variable step).
    Gear2,
}

/// Which linear solver factors the MNA matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SolverKind {
    /// Dense below a size threshold, sparse above it.
    Auto,
    Dense,
    Sparse,
}

/// Solver tolerances and limits. xsim defines its own documented defaults,
/// tuned for accuracy rather than speed (Q107, Q114).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SimOptions {
    /// Relative tolerance for Newton convergence and truncation error.
    pub reltol: f64,
    /// Absolute current tolerance (A).
    pub abstol: f64,
    /// Absolute voltage tolerance (V).
    pub vntol: f64,
    /// Absolute charge tolerance (C) for truncation-error control.
    pub chgtol: f64,
    /// Truncation-error overestimation factor; smaller is more accurate.
    pub trtol: f64,
    /// Minimum conductance added across every pn junction (S). Operating
    /// points also tie every node that has no DC path to ground and is
    /// reached only through capacitors to ground through it (1 pS when
    /// `gmin` is 0), with a warning (D15).
    pub gmin: f64,
    /// Circuit temperature (°C).
    pub temperature: f64,
    pub method: IntegrationMethod,
    pub solver: SolverKind,
    /// Newton iteration limit for DC operating points.
    pub dc_iterations: usize,
    /// Newton iteration limit per transient timestep.
    pub tran_iterations: usize,
    /// Smallest permitted transient timestep (s), relative to the stop time.
    pub min_step_ratio: f64,
}

impl Default for SimOptions {
    fn default() -> Self {
        Self {
            reltol: 1e-6,
            abstol: 1e-12,
            vntol: 1e-9,
            chgtol: 1e-16,
            trtol: 1.0,
            gmin: 1e-12,
            temperature: 27.0,
            method: IntegrationMethod::Trapezoidal,
            solver: SolverKind::Auto,
            dc_iterations: 200,
            tran_iterations: 50,
            min_step_ratio: 1e-15,
        }
    }
}

impl SimOptions {
    pub fn validate(&self) -> Result<(), CircuitError> {
        let positive = [
            ("reltol", self.reltol),
            ("abstol", self.abstol),
            ("vntol", self.vntol),
            ("chgtol", self.chgtol),
            ("trtol", self.trtol),
            ("min_step_ratio", self.min_step_ratio),
        ];
        for (name, v) in positive {
            if !(v.is_finite() && v > 0.0) {
                return Err(CircuitError::new(format!(
                    "option {name} must be positive, got {v}"
                )));
            }
        }
        if !(self.gmin.is_finite() && self.gmin >= 0.0) {
            return Err(CircuitError::new(format!(
                "option gmin must be non-negative and finite, got {}",
                self.gmin
            )));
        }
        // −273.15 °C itself is 0 K, where the thermal voltage vanishes.
        if !(self.temperature.is_finite() && self.temperature > -273.15) {
            return Err(CircuitError::new(format!(
                "option temperature must be above absolute zero (-273.15 °C), got {} °C",
                self.temperature
            )));
        }
        if self.dc_iterations == 0 || self.tran_iterations == 0 {
            return Err(CircuitError::new("iteration limits must be at least 1"));
        }
        Ok(())
    }
}
