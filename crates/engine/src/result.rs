//! Simulation results.

use crate::circuit::NodeIndex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnalysisKind {
    OperatingPoint,
    DcSweep,
    Ac,
    Transient,
}

/// The independent variable of a result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Axis {
    /// `"time"`, `"frequency"`, the swept source name, or `"point"` for an
    /// operating point (a single sample at 0).
    pub name: String,
    /// `"s"`, `"Hz"`, `"V"`, `"A"` or `""`.
    pub unit: String,
    pub values: Vec<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quantity {
    Voltage,
    Current,
}

/// Where a signal comes from in the circuit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignalOrigin {
    /// Voltage of a circuit node relative to ground.
    Node(NodeIndex),
    /// Current flowing *into* terminal `terminal` of element `element`
    /// (index into `Circuit::elements`).
    TerminalCurrent { element: usize, terminal: usize },
}

/// Real samples for OP/DC/transient; complex for AC.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SignalData {
    Real(Vec<f64>),
    Complex { re: Vec<f64>, im: Vec<f64> },
}

impl SignalData {
    pub fn len(&self) -> usize {
        match self {
            Self::Real(v) => v.len(),
            Self::Complex { re, .. } => re.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// One recorded quantity. Names: `V(node)` for node voltages, `I(name)` for
/// two-terminal element currents, `Ic(Q1)`/`Ib(Q1)`/`Ie(Q1)` for BJT and
/// `Id(J1)`/`Ig(J1)`/`Is(J1)` for JFET terminal currents.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Signal {
    pub name: String,
    pub quantity: Quantity,
    pub origin: SignalOrigin,
    pub data: SignalData,
}

impl Signal {
    /// Real samples, if this is a real-valued signal.
    pub fn real(&self) -> Option<&[f64]> {
        match &self.data {
            SignalData::Real(v) => Some(v),
            SignalData::Complex { .. } => None,
        }
    }

    pub fn unit(&self) -> &'static str {
        match self.quantity {
            Quantity::Voltage => "V",
            Quantity::Current => "A",
        }
    }
}

/// Output of [`crate::simulate`].
///
/// Every signal has exactly `axis.values.len()` samples. Transient results
/// include `t = start_save` (or 0) and, from there on, every breakpoint,
/// every multiple of the output step and the stop time, plus the other
/// accepted solver points that
/// linear interpolation between samples needs to reproduce each signal
/// within `reltol·swing + vntol` (`abstol` for currents, D18); sample times
/// are strictly increasing. Between samples, linear interpolation is
/// therefore what the solution looks like: the solver's steps resolve smooth
/// source waveforms to the same tolerance, so interpolation stays within
/// twice it of the exact waveform where the solver itself is exact.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimulationResult {
    pub analysis: AnalysisKind,
    pub axis: Axis,
    /// Node voltages in node order (ground excluded), then element terminal
    /// currents in element order.
    pub signals: Vec<Signal>,
}

impl SimulationResult {
    /// Signal by exact, case-sensitive name.
    pub fn signal(&self, name: &str) -> Option<&Signal> {
        self.signals.iter().find(|s| s.name == name)
    }

    /// Number of samples per signal.
    pub fn len(&self) -> usize {
        self.axis.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.axis.values.is_empty()
    }
}
