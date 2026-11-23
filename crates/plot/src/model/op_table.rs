//! The operating-point table: one row per node voltage and element current
//! of a single-sample result.

use xsim_spice_engine::units::format_value_digits;
use xsim_spice_engine::{Quantity, SignalData, SimulationResult};

/// Significant digits shown for operating-point values.
const DIGITS: usize = 5;

/// One row of the table.
#[derive(Debug, Clone, PartialEq)]
pub struct OpRow {
    pub name: String,
    pub value: f64,
    /// The value with SI prefix and unit, e.g. `"2.5 mA"`.
    pub text: String,
}

/// The table, split into node voltages and currents (in result order).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OpTable {
    pub voltages: Vec<OpRow>,
    pub currents: Vec<OpRow>,
}

/// Whether `result` holds a single operating point rather than a waveform.
pub fn is_operating_point(result: &SimulationResult) -> bool {
    result.len() == 1
}

/// Builds the table from the first sample of every signal.
pub fn op_table(result: &SimulationResult) -> OpTable {
    let mut table = OpTable::default();
    for signal in &result.signals {
        let Some(value) = first_sample(&signal.data) else {
            continue;
        };
        let row = OpRow {
            name: signal.name.clone(),
            value,
            text: format_value_digits(value, signal.unit(), DIGITS),
        };
        match signal.quantity {
            Quantity::Voltage => table.voltages.push(row),
            Quantity::Current => table.currents.push(row),
        }
    }
    table
}

fn first_sample(data: &SignalData) -> Option<f64> {
    match data {
        SignalData::Real(values) => values.first().copied(),
        SignalData::Complex { re, .. } => re.first().copied(),
    }
}
