//! # xsim SPICE engine
//!
//! An independent circuit simulator library (Q09, Q98). It has no knowledge of
//! the desktop application: callers build a [`Circuit`], choose an
//! [`Analysis`] and [`SimOptions`], and call [`simulate`].
//!
//! * Formulation: modified nodal analysis in `f64` (Q99, Q100).
//! * Linear algebra: sparse LU (faer) with a dense LU (nalgebra) reference
//!   implementation (Q101, Q102). See [`solver`].
//! * Nonlinear solution: Newton–Raphson, then GMIN stepping, then source
//!   stepping, then pseudo-transient continuation (Q103).
//! * Transient: backward Euler, trapezoidal, modified trapezoidal (TR-BDF2)
//!   and Gear-2 with local-truncation-error timestep control (Q105, Q106).
//! * Analyses: operating point, DC sweep, AC sweep, transient (Q115).
//!
//! All quantities are SI units. Node `0` is ground. Results are deterministic
//! for identical inputs (Q108).
//!
//! ```
//! use xsim_spice_engine::*;
//! let mut c = Circuit::new();
//! let n_in = c.node("in");
//! let n_out = c.node("out");
//! c.add(Element::new("V1", [n_in, GROUND], Device::VoltageSource(Source::dc(1.0))));
//! c.add(Element::new("R1", [n_in, n_out], Device::resistor(1e3)));
//! c.add(Element::new("R2", [n_out, GROUND], Device::resistor(1e3)));
//! let result = simulate(&c, &Analysis::OperatingPoint, &SimOptions::default(), &SimControl::new()).unwrap();
//! let v_out = result.signal("V(out)").unwrap().real().unwrap()[0];
//! assert!((v_out - 0.5).abs() < 1e-9);
//! ```

pub mod analysis;
pub mod circuit;
pub mod control;
pub mod error;
pub mod library;
pub mod models;
pub mod result;
pub mod source;
pub mod units;

// Internal machinery. `solver` is public so the dense reference and sparse
// production solvers can be cross-checked from integration tests.
pub(crate) mod analyses;
pub(crate) mod devices;
pub(crate) mod integration;
pub(crate) mod mna;
pub(crate) mod newton;
pub mod solver;

pub use analysis::{
    AcSweep, Analysis, DcSweep, InitialState, IntegrationMethod, SimOptions, SolverKind,
    SweepScale, Transient,
};
pub use circuit::{Circuit, Device, Element, GROUND, NodeIndex};
pub use control::SimControl;
pub use error::{CircuitError, FailurePoint, SimError};
pub use models::{
    BjtModel, CurrentSwitchModel, DiodeModel, JfetModel, OpAmpKind, OpAmpModel, Polarity,
    SwitchModel,
};
pub use result::{
    AnalysisKind, Axis, Quantity, Signal, SignalData, SignalOrigin, SimulationResult,
};
pub use source::{Source, Waveform};

/// Runs one analysis of `circuit`.
///
/// `control` lets another thread observe progress, pause, resume or cancel
/// the run (Q110, Q139). The function blocks while the run is paused.
pub fn simulate(
    circuit: &Circuit,
    analysis: &Analysis,
    options: &SimOptions,
    control: &SimControl,
) -> Result<SimulationResult, SimError> {
    control.reset_progress();
    control.clear_warnings();
    circuit.validate()?;
    options.validate()?;
    let result = match analysis {
        Analysis::OperatingPoint => analyses::op::run(circuit, options, control),
        Analysis::DcSweep(spec) => analyses::dc::run(circuit, spec, options, control),
        Analysis::Ac(spec) => analyses::ac::run(circuit, spec, options, control),
        Analysis::Transient(spec) => analyses::transient::run(circuit, spec, options, control),
    }?;
    control.set_progress(1.0);
    Ok(result)
}
