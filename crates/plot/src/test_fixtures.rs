//! Hand-built simulation results for tests (no simulator involved).

use xsim_spice_engine::{
    AnalysisKind, Axis, Quantity, Signal, SignalData, SignalOrigin, SimulationResult,
};

/// Time constant of the RC fixtures.
pub const TAU: f64 = 1e-3;
/// Sample spacing of [`transient`].
pub const STEP: f64 = 5e-5;

fn voltage(name: &str, node: usize, data: SignalData) -> Signal {
    Signal {
        name: name.into(),
        quantity: Quantity::Voltage,
        origin: SignalOrigin::Node(node),
        data,
    }
}

fn current(name: &str, element: usize, data: SignalData) -> Signal {
    Signal {
        name: name.into(),
        quantity: Quantity::Current,
        origin: SignalOrigin::TerminalCurrent {
            element,
            terminal: 0,
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
            name: name.into(),
            unit: unit.into(),
            values,
        },
        signals,
    }
}

/// An RC step response sampled every 50 µs from 0 to 5 ms: `V(in)` = 1 V,
/// `V(out)` = 1 − e^(−t/τ), `I(R1)` through 1 kΩ.
pub fn transient() -> SimulationResult {
    let t: Vec<f64> = (0..=100).map(|i| f64::from(i) * STEP).collect();
    let out: Vec<f64> = t.iter().map(|t| 1.0 - (-t / TAU).exp()).collect();
    let current_r1: Vec<f64> = out.iter().map(|v| (1.0 - v) / 1e3).collect();
    result(
        AnalysisKind::Transient,
        "time",
        "s",
        t.clone(),
        vec![
            voltage("V(in)", 1, SignalData::Real(vec![1.0; t.len()])),
            voltage("V(out)", 2, SignalData::Real(out)),
            current("I(R1)", 1, SignalData::Real(current_r1)),
        ],
    )
}

/// A 5 ms transient with one 40 ns spike, like a switching edge (D20):
/// `V(sw)` is 0 V up to 1 ms, then rises to 2.28 V and falls back to
/// 0.28 V within 40 ns and settles to 0.25 V. Sampled every 1 µs, and every
/// 1 ns for 200 ns from 1 ms, as a simulator steps through an edge.
pub fn spike() -> SimulationResult {
    let edge = 1e-3;
    let mut t: Vec<f64> = (0..=5000).map(|i| f64::from(i) * 1e-6).collect();
    t.extend((1..200).map(|i| edge + f64::from(i) * 1e-9));
    t.sort_by(f64::total_cmp);
    t.dedup();
    let v: Vec<f64> = t
        .iter()
        .map(|&t| {
            if t < edge {
                return 0.0;
            }
            let phase = (t - edge) / 40e-9;
            let bump = if phase < 1.0 {
                (std::f64::consts::PI * phase).sin().powi(2)
            } else {
                0.0
            };
            0.25 + 0.03 * (-(t - edge) / 1e-6).exp() + 2.0 * bump
        })
        .collect();
    result(
        AnalysisKind::Transient,
        "time",
        "s",
        t,
        vec![voltage("V(sw)", 1, SignalData::Real(v))],
    )
}

/// Three samples: `V(out)` = 0, 0.5, 1 V at 0, 1 and 2 ms.
pub fn tiny() -> SimulationResult {
    result(
        AnalysisKind::Transient,
        "time",
        "s",
        vec![0.0, 1e-3, 2e-3],
        vec![voltage("V(out)", 1, SignalData::Real(vec![0.0, 0.5, 1.0]))],
    )
}

/// A first-order low-pass with a 1 kHz corner, 1 Hz to 1 MHz.
pub fn ac() -> SimulationResult {
    let f: Vec<f64> = (0..=60).map(|i| 10f64.powf(f64::from(i) / 10.0)).collect();
    let (re, im): (Vec<f64>, Vec<f64>) = f
        .iter()
        .map(|f| {
            let x = f / 1e3;
            (1.0 / (1.0 + x * x), -x / (1.0 + x * x))
        })
        .unzip();
    let ones = vec![1.0; f.len()];
    result(
        AnalysisKind::Ac,
        "frequency",
        "Hz",
        f.clone(),
        vec![
            voltage(
                "V(in)",
                1,
                SignalData::Complex {
                    re: ones,
                    im: vec![0.0; f.len()],
                },
            ),
            voltage("V(out)", 2, SignalData::Complex { re, im }),
        ],
    )
}

/// A three-pole RC ladder (three 1 kΩ / 1 µF sections), 1 Hz to 100 kHz at
/// 10 points per decade: H = 1 / (1 + 6sτ + 5(sτ)² + (sτ)³) with τ = 1 ms.
/// Its phase falls past −180° (to about −269.5° at 100 kHz), so a wrapped
/// phase jumps by 360° once.
pub fn ladder() -> SimulationResult {
    let f: Vec<f64> = (0..=50).map(|i| 10f64.powf(f64::from(i) / 10.0)).collect();
    let (re, im): (Vec<f64>, Vec<f64>) = f
        .iter()
        .map(|f| {
            let w = std::f64::consts::TAU * f * TAU;
            // 1/D = conj(D)/|D|² with D = 1 − 5w² + j(6w − w³).
            let (d_re, d_im) = (1.0 - 5.0 * w * w, 6.0 * w - w * w * w);
            let norm = d_re * d_re + d_im * d_im;
            (d_re / norm, -d_im / norm)
        })
        .unzip();
    let ones = vec![1.0; f.len()];
    result(
        AnalysisKind::Ac,
        "frequency",
        "Hz",
        f.clone(),
        vec![
            voltage(
                "V(in)",
                1,
                SignalData::Complex {
                    re: ones,
                    im: vec![0.0; f.len()],
                },
            ),
            voltage("V(out)", 2, SignalData::Complex { re, im }),
        ],
    )
}

/// An operating point: a 5 V divider.
pub fn operating_point() -> SimulationResult {
    result(
        AnalysisKind::OperatingPoint,
        "point",
        "",
        vec![0.0],
        vec![
            voltage("V(in)", 1, SignalData::Real(vec![5.0])),
            voltage("V(out)", 2, SignalData::Real(vec![2.5])),
            current("I(R1)", 0, SignalData::Real(vec![2.5e-3])),
        ],
    )
}
