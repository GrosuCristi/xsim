//! Error types.

use std::fmt;

/// A structural problem in a [`crate::Circuit`] or invalid options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CircuitError {
    pub message: String,
}

impl CircuitError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for CircuitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CircuitError {}

/// Significant digits of the axis value in failure messages.
const POINT_DIGITS: usize = 6;

/// Where on the analysis axis a run failed (D7): the transient time or the
/// value of the swept source.
#[derive(Debug, Clone, PartialEq)]
pub struct FailurePoint {
    /// `"t"` for the transient time, otherwise the name of the swept source.
    pub quantity: String,
    pub value: f64,
    /// SI unit of `value`: `"s"`, `"V"` or `"A"`.
    pub unit: String,
}

impl FailurePoint {
    /// Transient time `t` in seconds.
    pub fn time(t: f64) -> Self {
        Self {
            quantity: "t".into(),
            value: t,
            unit: "s".into(),
        }
    }

    /// The value of the swept source `source` (unit `"V"` or `"A"`).
    pub fn sweep(source: &str, value: f64, unit: &str) -> Self {
        Self {
            quantity: source.into(),
            value,
            unit: unit.into(),
        }
    }
}

impl fmt::Display for FailurePoint {
    /// `t = 1.23 ms`, `V1 = 50 V`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} = {}",
            self.quantity,
            crate::units::format_value_digits(self.value, &self.unit, POINT_DIGITS)
        )
    }
}

/// Why a simulation did not produce a result.
///
/// Failures report how far the run got as `progress` in `0.0..=1.0` (Q162)
/// and, where known, where it failed (D7): the time or sweep value (`at`)
/// and the node or branch (`unknown`).
#[derive(Debug, Clone, PartialEq)]
pub enum SimError {
    /// The circuit or options are invalid; nothing was simulated.
    InvalidCircuit(CircuitError),
    /// The MNA matrix is structurally or numerically singular.
    SingularMatrix {
        /// The offending equation, e.g. `V(out)` for a floating node or
        /// `I(V1)` for a loop of voltage sources.
        unknown: String,
        /// Why, e.g. "node m has no DC path to ground (only current source I1
        /// feeds it, so its voltage is undefined)". Floating nodes and
        /// source loops are found from the circuit's connectivity; without
        /// such a cause the reason says the circuit is too ill-conditioned
        /// for double precision.
        reason: String,
        /// Simulation time or sweep value of the failing solve.
        at: Option<FailurePoint>,
        progress: f64,
    },
    /// The equations could be factored, but their solution overflowed or
    /// became NaN at `unknown` (extreme element values or initial
    /// conditions).
    NumericalOverflow {
        unknown: String,
        at: Option<FailurePoint>,
        progress: f64,
    },
    /// Newton–Raphson and every recovery strategy failed.
    NoConvergence {
        /// Simulation time (transient) or sweep value where it failed.
        at: Option<FailurePoint>,
        /// The node or branch whose Newton update was largest relative to
        /// its tolerance in the last iteration of the direct Newton attempt.
        unknown: Option<String>,
        progress: f64,
        /// How far each recovery strategy got.
        detail: String,
    },
    /// The transient timestep fell below the minimum.
    TimestepTooSmall { time: f64, progress: f64 },
    /// The transient result reached its memory limit (D18): `limit` stored
    /// values (samples × (signals + 1)), although it keeps only the output
    /// grid, the breakpoints and the points linear interpolation needs.
    ResultTooLarge {
        /// Simulation time at which the limit was reached.
        time: f64,
        progress: f64,
        limit: usize,
    },
    /// [`crate::SimControl::cancel`] was called.
    Cancelled { progress: f64 },
}

impl SimError {
    /// Fraction of the run completed before the failure.
    pub fn progress(&self) -> f64 {
        match self {
            Self::InvalidCircuit(_) => 0.0,
            Self::SingularMatrix { progress, .. }
            | Self::NumericalOverflow { progress, .. }
            | Self::NoConvergence { progress, .. }
            | Self::TimestepTooSmall { progress, .. }
            | Self::ResultTooLarge { progress, .. }
            | Self::Cancelled { progress } => *progress,
        }
    }
}

/// ` when V1 = 50 V`, or nothing.
fn when(at: &Option<FailurePoint>) -> String {
    at.as_ref()
        .map(|p| format!(" when {p}"))
        .unwrap_or_default()
}

impl fmt::Display for SimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let pct = |p: f64| p * 100.0;
        match self {
            Self::InvalidCircuit(e) => write!(f, "Invalid circuit: {e}"),
            Self::SingularMatrix {
                unknown,
                reason,
                at,
                progress,
            } => {
                write!(
                    f,
                    "Singular matrix at {unknown}{} ({:.1}% complete)",
                    when(at),
                    pct(*progress)
                )?;
                if !reason.is_empty() {
                    write!(f, ": {reason}")?;
                }
                Ok(())
            }
            Self::NumericalOverflow {
                unknown,
                at,
                progress,
            } => write!(
                f,
                "The solution overflowed or became NaN at {unknown}{} ({:.1}% complete): check \
                 for extreme element values or initial conditions",
                when(at),
                pct(*progress)
            ),
            Self::NoConvergence {
                at,
                unknown,
                progress,
                detail,
            } => {
                write!(f, "Simulation did not converge")?;
                if let Some(at) = at {
                    write!(f, " at {at}")?;
                }
                if let Some(unknown) = unknown {
                    write!(f, " near {unknown}")?;
                }
                write!(f, " ({:.1}% complete)", pct(*progress))?;
                if !detail.is_empty() {
                    write!(f, ": {detail}")?;
                }
                Ok(())
            }
            Self::TimestepTooSmall { time, progress } => write!(
                f,
                "Timestep too small at t = {} ({:.1}% complete)",
                crate::units::format_value(*time, "s"),
                pct(*progress)
            ),
            Self::ResultTooLarge {
                time,
                progress,
                limit,
            } => write!(
                f,
                "Result too large at t = {} ({:.1}% complete): the waveforms need more than {limit} \
                 stored values; shorten the run, start saving later, raise the output step or \
                 loosen reltol",
                crate::units::format_value(*time, "s"),
                pct(*progress)
            ),
            Self::Cancelled { progress } => {
                write!(f, "Simulation cancelled at {:.1}%", pct(*progress))
            }
        }
    }
}

impl std::error::Error for SimError {}

impl From<CircuitError> for SimError {
    fn from(value: CircuitError) -> Self {
        Self::InvalidCircuit(value)
    }
}
