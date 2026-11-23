//! Analysis drivers.
//!
//! Every analysis prepares an [`Mna`] for the circuit, solves it, and
//! records one sample per axis value with a [`Recorder`]. The signal list is
//! the same for every analysis: node voltages in node order (ground
//! excluded), then the recorded terminal currents of each element in element
//! order, named with [`crate::Device::current_prefixes`].

pub(crate) mod ac;
pub(crate) mod dc;
pub(crate) mod op;
pub(crate) mod transient;

use crate::circuit::Circuit;
use crate::error::{CircuitError, SimError};
use crate::mna::Mna;
use crate::newton::DcFailure;
use crate::result::{
    AnalysisKind, Axis, Quantity, Signal, SignalData, SignalOrigin, SimulationResult,
};
use num_complex::Complex64;

/// Most samples a sweep or a transient output grid may request, and most
/// steps an explicit transient maximum step may imply. It bounds the
/// requested axis, not the stored result: a transient also keeps the
/// breakpoints and the points linear interpolation needs (D18), and
/// [`MAX_VALUES`] bounds what it stores.
pub(crate) const MAX_POINTS: usize = 10_000_000;

/// Most values a transient result may store: samples × (signals + 1 for the
/// time axis), 8 bytes each, so 800 MB (D18). A run that reaches it stops
/// with [`crate::SimError::ResultTooLarge`].
pub(crate) const MAX_VALUES: usize = 100_000_000;

/// An invalid analysis specification.
pub(crate) fn invalid(message: impl Into<String>) -> SimError {
    SimError::InvalidCircuit(CircuitError::new(message))
}

/// Converts a failed DC solve into the public error, naming the singular
/// unknown and reporting the progress at failure (Q162). `at` is the
/// simulation time of a transient's starting point (see
/// [`op::dc_failure`] for other locations).
pub(crate) fn dc_error(failure: DcFailure, mna: &Mna, at: Option<f64>, progress: f64) -> SimError {
    op::dc_failure(
        failure,
        mna,
        at.map(crate::error::FailurePoint::time),
        progress,
    )
}

/// The public error for a transient step or AC point whose matrix is
/// singular (or whose solution is not finite) at unknown `row`.
pub(crate) fn singular(mna: &Mna, row: usize, progress: f64) -> SimError {
    op::step_failure(mna, row, progress)
}

/// Builds the signal list of a result and appends samples to it.
pub(crate) struct Recorder {
    signals: Vec<Signal>,
    /// Per element: terminal count and number of recorded currents.
    elements: Vec<(usize, usize)>,
    /// Scratch space for one element's terminal currents.
    terminals: Vec<f64>,
}

impl Recorder {
    /// An empty recorder for `circuit`, with complex data for AC results.
    pub(crate) fn new(circuit: &Circuit, complex: bool) -> Self {
        let empty = || {
            if complex {
                SignalData::Complex {
                    re: Vec::new(),
                    im: Vec::new(),
                }
            } else {
                SignalData::Real(Vec::new())
            }
        };
        let mut signals: Vec<Signal> = circuit.nodes[1..]
            .iter()
            .enumerate()
            .map(|(i, name)| Signal {
                name: format!("V({name})"),
                quantity: Quantity::Voltage,
                origin: SignalOrigin::Node(i + 1),
                data: empty(),
            })
            .collect();
        let mut elements = Vec::with_capacity(circuit.elements.len());
        for (index, element) in circuit.elements.iter().enumerate() {
            let prefixes = element.device.current_prefixes();
            elements.push((element.device.terminal_count(), prefixes.len()));
            signals.extend(
                prefixes
                    .iter()
                    .enumerate()
                    .map(|(terminal, prefix)| Signal {
                        name: format!("{prefix}({})", element.name),
                        quantity: Quantity::Current,
                        origin: SignalOrigin::TerminalCurrent {
                            element: index,
                            terminal,
                        },
                        data: empty(),
                    }),
            );
        }
        let widest = elements
            .iter()
            .map(|&(terminals, _)| terminals)
            .max()
            .unwrap_or(0);
        Self {
            signals,
            elements,
            terminals: vec![0.0; widest],
        }
    }

    /// Number of signals, i.e. the length of one sample row.
    pub(crate) fn width(&self) -> usize {
        self.signals.len()
    }

    /// Whether each signal of a row is a voltage or a current, in row order.
    pub(crate) fn quantities(&self) -> impl Iterator<Item = Quantity> + '_ {
        self.signals.iter().map(|s| s.quantity)
    }

    /// Writes the value of every signal at solution `x` into `row`. The
    /// devices must have been loaded at `x`.
    pub(crate) fn sample_into(&mut self, mna: &Mna, x: &[f64], row: &mut Vec<f64>) {
        row.clear();
        row.extend_from_slice(&x[..mna.layout().circuit_node_unknowns()]);
        for (index, &(terminals, recorded)) in self.elements.iter().enumerate() {
            let currents = &mut self.terminals[..terminals];
            mna.currents(index, currents);
            row.extend_from_slice(&currents[..recorded]);
        }
    }

    /// Appends one sample row (see [`Self::sample_into`]).
    pub(crate) fn push(&mut self, row: &[f64]) {
        for (signal, &value) in self.signals.iter_mut().zip(row) {
            if let SignalData::Real(data) = &mut signal.data {
                data.push(value);
            }
        }
    }

    /// Samples the solution `x` and appends it.
    pub(crate) fn record(&mut self, mna: &Mna, x: &[f64]) {
        let mut row = Vec::with_capacity(self.width());
        self.sample_into(mna, x, &mut row);
        self.push(&row);
    }

    /// Appends one complex sample row: node voltages, then currents.
    pub(crate) fn push_complex(&mut self, row: &[Complex64]) {
        for (signal, value) in self.signals.iter_mut().zip(row) {
            if let SignalData::Complex { re, im } = &mut signal.data {
                re.push(value.re);
                im.push(value.im);
            }
        }
    }

    /// Recorded currents per element, in element order.
    pub(crate) fn recorded_currents(&self) -> impl Iterator<Item = usize> + '_ {
        self.elements.iter().map(|&(_, recorded)| recorded)
    }

    pub(crate) fn finish(self, analysis: AnalysisKind, axis: Axis) -> SimulationResult {
        debug_assert!(
            self.signals
                .iter()
                .all(|s| s.data.len() == axis.values.len())
        );
        SimulationResult {
            analysis,
            axis,
            signals: self.signals,
        }
    }
}
