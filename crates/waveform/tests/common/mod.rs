//! Hand-built simulation results for the integration tests.
//!
//! Tests never run the engine: they construct results whose samples follow
//! closed-form functions, so every expected value is computed analytically.

// Each integration test crate uses a different subset of these helpers.
#![allow(dead_code)]

use num_complex::Complex64;
use proptest::test_runner::{Config, RngSeed};
use xsim_spice_engine::{
    AnalysisKind, Axis, Quantity, Signal, SignalData, SignalOrigin, SimulationResult,
};

/// Proptest configuration with a fixed seed, so every run explores the same
/// cases (Q108: deterministic results), and without regression files.
pub fn proptest_config(cases: u32) -> Config {
    Config {
        cases,
        rng_seed: RngSeed::Fixed(0x5EED_C1C0),
        failure_persistence: None,
        ..Config::default()
    }
}

fn signal(index: usize, name: &str, data: SignalData) -> Signal {
    let is_voltage = name.starts_with("V(");
    Signal {
        name: name.to_string(),
        quantity: if is_voltage {
            Quantity::Voltage
        } else {
            Quantity::Current
        },
        origin: if is_voltage {
            SignalOrigin::Node(index + 1)
        } else {
            SignalOrigin::TerminalCurrent {
                element: index,
                terminal: 0,
            }
        },
        data,
    }
}

fn result(
    analysis: AnalysisKind,
    name: &str,
    unit: &str,
    values: Vec<f64>,
    signals: Vec<Signal>,
) -> SimulationResult {
    SimulationResult {
        analysis,
        axis: Axis {
            name: name.to_string(),
            unit: unit.to_string(),
            values,
        },
        signals,
    }
}

/// A transient result with real signals sampled at `time`.
pub fn transient(time: Vec<f64>, signals: &[(&str, Vec<f64>)]) -> SimulationResult {
    let signals = signals
        .iter()
        .enumerate()
        .map(|(i, (name, values))| signal(i, name, SignalData::Real(values.clone())))
        .collect();
    result(AnalysisKind::Transient, "time", "s", time, signals)
}

/// A DC sweep of `source` (in volts) with real signals.
pub fn dc_sweep(source: &str, values: Vec<f64>, signals: &[(&str, Vec<f64>)]) -> SimulationResult {
    let mut sweep = transient(values, signals);
    sweep.analysis = AnalysisKind::DcSweep;
    sweep.axis.name = source.to_string();
    sweep.axis.unit = "V".to_string();
    sweep
}

/// An operating point: one sample per signal at `point = 0`.
pub fn operating_point(signals: &[(&str, f64)]) -> SimulationResult {
    let signals = signals
        .iter()
        .enumerate()
        .map(|(i, (name, value))| signal(i, name, SignalData::Real(vec![*value])))
        .collect();
    result(
        AnalysisKind::OperatingPoint,
        "point",
        "",
        vec![0.0],
        signals,
    )
}

/// An AC result with complex signals sampled at `frequency`.
pub fn ac(frequency: Vec<f64>, signals: &[(&str, Vec<Complex64>)]) -> SimulationResult {
    let signals = signals
        .iter()
        .enumerate()
        .map(|(i, (name, values))| {
            let data = SignalData::Complex {
                re: values.iter().map(|z| z.re).collect(),
                im: values.iter().map(|z| z.im).collect(),
            };
            signal(i, name, data)
        })
        .collect();
    result(AnalysisKind::Ac, "frequency", "Hz", frequency, signals)
}

/// `count` strictly increasing samples over `[start, stop]` with
/// deterministic, deliberately uneven spacing (step ratios up to about 3:1),
/// as a variable-step transient produces.
pub fn uneven_grid(start: f64, stop: f64, count: usize) -> Vec<f64> {
    assert!(count >= 2);
    // Weights cycle through an irregular pattern; their running sum maps
    // onto [start, stop] so both ends are hit exactly.
    let weights: Vec<f64> = (0..count - 1)
        .map(|i| 1.0 + ((i * 7) % 5) as f64 * 0.5)
        .collect();
    let total: f64 = weights.iter().sum();
    let mut grid = Vec::with_capacity(count);
    let mut sum = 0.0;
    grid.push(start);
    for weight in &weights[..count - 2] {
        sum += weight;
        grid.push(start + (stop - start) * (sum / total));
    }
    grid.push(stop);
    grid
}

/// Logarithmically spaced points from `start` to `stop`.
pub fn log_grid(start: f64, stop: f64, count: usize) -> Vec<f64> {
    let (a, b) = (start.log10(), stop.log10());
    (0..count)
        .map(|i| 10f64.powf(a + (b - a) * i as f64 / (count - 1) as f64))
        .collect()
}

pub fn assert_close(actual: f64, expected: f64, tolerance: f64, what: &str) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{what}: got {actual}, expected {expected} (tolerance {tolerance})"
    );
}
