//! Measurements of analytic signals on non-uniform grids (Q135), and the
//! quantities the schematic hover box shows (Q62).
//!
//! Every expected value is a closed-form result evaluated here. Tolerances
//! come from the grid: sampling a smooth signal and joining the samples with
//! straight lines is accurate to `h²·max|y''|/8`, so each bound below is
//! derived from the largest step `h` of the grid in use.

mod common;

use common::{assert_close, transient, uneven_grid};
use std::f64::consts::{PI, SQRT_2};
use xsim_waveform::Expression;
use xsim_waveform::Series;
use xsim_waveform::measure::{
    Edge, Window, crossings, fall_time, frequency, rise_time, settling_time, stats, value_at,
};

fn max_step(x: &[f64]) -> f64 {
    x.windows(2).map(|w| w[1] - w[0]).fold(0.0, f64::max)
}

fn sampled(x: &[f64], f: impl Fn(f64) -> f64) -> Vec<f64> {
    x.iter().map(|&t| f(t)).collect()
}

const AMPLITUDE: f64 = 2.5;
const OMEGA: f64 = 2.0 * PI * 1e3;

/// Three periods of a 1 kHz, 2.5 V sine on an uneven grid.
fn sine() -> (Vec<f64>, Vec<f64>) {
    let x = uneven_grid(0.0, 3e-3, 30_001);
    let y = sampled(&x, |t| AMPLITUDE * (OMEGA * t).sin());
    (x, y)
}

/// Bound on the error of a mean of the sine (or of its square, relative to
/// the amplitude) caused by the piecewise-linear interpolation.
fn sine_tolerance(x: &[f64]) -> f64 {
    let h = max_step(x);
    AMPLITUDE * (OMEGA * h).powi(2) / 4.0
}

#[test]
fn sine_rms_is_amplitude_over_root_two() {
    let (x, y) = sine();
    let tolerance = sine_tolerance(&x);
    let s = stats(&x, &y, None).unwrap();
    assert_close(s.rms, AMPLITUDE / SQRT_2, tolerance, "rms");
    assert_close(s.average, 0.0, tolerance, "average");
    assert_close(s.integral, 0.0, tolerance * 3e-3, "integral");
    assert_close(s.max, AMPLITUDE, tolerance, "max");
    assert_close(s.min, -AMPLITUDE, tolerance, "min");
    assert_eq!(s.peak_to_peak, s.max - s.min);
    // The bound is tight enough for the six decimals of Q153.
    assert!(tolerance < 1e-6, "tolerance {tolerance}");
}

#[test]
fn windowed_sine_statistics_ignore_sample_alignment() {
    let (x, y) = sine();
    let tolerance = sine_tolerance(&x);
    // Two whole periods starting a quarter period in, between samples.
    let window = Window {
        from: 0.25e-3 + 1.234e-9,
        to: 2.25e-3 + 1.234e-9,
    };
    let s = stats(&x, &y, Some(window)).unwrap();
    assert_close(s.rms, AMPLITUDE / SQRT_2, tolerance, "rms");
    assert_close(s.average, 0.0, tolerance, "average");
    // The first half period: ∫A·sin(ωt) dt over (0, π/ω) is 2A/ω, a mean of 2A/π.
    let half = stats(
        &x,
        &y,
        Some(Window {
            from: 0.0,
            to: 0.5e-3,
        }),
    )
    .unwrap();
    assert_close(
        half.integral,
        2.0 * AMPLITUDE / OMEGA,
        tolerance * 0.5e-3,
        "half-period area",
    );
    assert_close(
        half.average,
        2.0 * AMPLITUDE / PI,
        tolerance,
        "half-period mean",
    );
}

#[test]
fn triangle_wave_statistics_are_exact() {
    // A 0 → 4 V → 0 triangle repeated three times, sampled at its corners
    // and at uneven points along each ramp, is exactly piecewise linear:
    // average = A/2 and RMS = A/√3 hold to rounding.
    let peak = 4.0;
    let period = 2e-3;
    let x = uneven_grid(0.0, 3.0 * period, 3001);
    let mut x: Vec<f64> = x
        .into_iter()
        .chain((0..=6).map(|k| f64::from(k) * period / 2.0))
        .collect();
    x.sort_by(f64::total_cmp);
    x.dedup();
    let y = sampled(&x, |t| {
        let phase = (t / period).fract() * 2.0;
        peak * if phase <= 1.0 { phase } else { 2.0 - phase }
    });
    let s = stats(&x, &y, None).unwrap();
    assert_close(s.average, peak / 2.0, 1e-12, "average");
    assert_close(s.rms, peak / 3f64.sqrt(), 1e-12, "rms");
    assert_close(s.integral, peak / 2.0 * 3.0 * period, 1e-14, "integral");
}

#[test]
fn sine_crossings_and_values() {
    let (x, y) = sine();
    let h = max_step(&x);
    // Crossing error is the interpolation error divided by the slope.
    let level = AMPLITUDE / 2.0;
    let slope = AMPLITUDE * OMEGA * (PI / 6.0).cos();
    let tolerance = AMPLITUDE * (OMEGA * h).powi(2) / 8.0 / slope * 2.0;
    let rising = crossings(&x, &y, level, Edge::Rising, None);
    let falling = crossings(&x, &y, level, Edge::Falling, None);
    assert_eq!((rising.len(), falling.len()), (3, 3));
    for k in 0..3 {
        let cycle = 2.0 * PI * k as f64;
        assert_close(rising[k], (PI / 6.0 + cycle) / OMEGA, tolerance, "rising");
        assert_close(
            falling[k],
            (5.0 * PI / 6.0 + cycle) / OMEGA,
            tolerance,
            "falling",
        );
    }
    let either = crossings(&x, &y, level, Edge::Either, None);
    assert_eq!(either.len(), 6);
    assert!(either.windows(2).all(|w| w[0] < w[1]));

    let at = 0.123_456e-3;
    let interpolation = AMPLITUDE * (OMEGA * h).powi(2) / 8.0;
    assert_close(
        value_at(&x, &y, at).unwrap(),
        AMPLITUDE * (OMEGA * at).sin(),
        interpolation,
        "value_at",
    );
}

const TAU: f64 = 1e-3;

/// `1 − e^(−t/τ)` over ten time constants: an RC step response.
fn charging() -> (Vec<f64>, Vec<f64>) {
    let x = uneven_grid(0.0, 10.0 * TAU, 20_001);
    let y = sampled(&x, |t| 1.0 - (-t / TAU).exp());
    (x, y)
}

/// Error of a crossing time of the exponentials below: the interpolation
/// error `h²·|y''|/8` over the slope `|y'|`, with `|y''|/|y'| = 1/τ`.
fn exponential_tolerance(x: &[f64]) -> f64 {
    max_step(x).powi(2) / (8.0 * TAU) * 2.0
}

#[test]
fn rise_time_of_an_rc_step() {
    let (x, y) = charging();
    let top = y[y.len() - 1];
    // Levels are 10 % and 90 % of [0, top]; 1 − e^(−t/τ) = L at t = −τ·ln(1 − L).
    let at = |level: f64| -TAU * (1.0 - level).ln();
    let expected = at(0.9 * top) - at(0.1 * top);
    let measured = rise_time(&x, &y, None).unwrap();
    assert_close(measured, expected, exponential_tolerance(&x), "rise time");
    // Close to the textbook τ·ln 9 because the response has nearly settled.
    assert_close(measured, TAU * 9f64.ln(), 1e-6, "τ·ln 9");
    assert_eq!(fall_time(&x, &y, None), None);
}

#[test]
fn fall_time_of_an_rc_discharge() {
    let x = uneven_grid(0.0, 10.0 * TAU, 20_001);
    let y = sampled(&x, |t| (-t / TAU).exp());
    let (top, bottom) = (1.0, (-10.0f64).exp());
    let level = |fraction: f64| bottom + fraction * (top - bottom);
    let expected = TAU * (level(0.9) / level(0.1)).ln();
    let measured = fall_time(&x, &y, None).unwrap();
    assert_close(measured, expected, exponential_tolerance(&x), "fall time");
    assert_eq!(rise_time(&x, &y, None), None);
}

#[test]
fn settling_time_of_an_rc_step() {
    let (x, y) = charging();
    let tolerance = 0.02;
    // The band is ±2 % of the window's total change, around its final value
    // y(T) = 1 − e^(−T/τ): e^(−t/τ) − e^(−T/τ) = band.
    let tail = (-10.0f64).exp();
    let band = tolerance * (1.0 - tail);
    let expected = -TAU * (band + tail).ln();
    let measured = settling_time(&x, &y, tolerance, None).unwrap();
    assert_close(measured, expected, exponential_tolerance(&x), "settling");

    // In a window starting at τ the initial value and band change, and the
    // time is counted from the window start.
    let window = Window {
        from: TAU,
        to: 10.0 * TAU,
    };
    let initial = 1.0 - (-1.0f64).exp();
    let band = tolerance * ((1.0 - tail) - initial);
    let expected = -TAU * (band + tail).ln() - TAU;
    let measured = settling_time(&x, &y, tolerance, Some(window)).unwrap();
    assert_close(
        measured,
        expected,
        exponential_tolerance(&x),
        "windowed settling",
    );
}

/// A trapezoidal square wave between −0.7 V and 1.3 V: 12.5 kHz, edges 5 %
/// of the period, sampled unevenly with many samples on every edge.
#[test]
fn square_wave_frequency() {
    let frequency_hz = 12.5e3;
    let period = 1.0 / frequency_hz;
    let edge = 0.05 * period;
    let (low, high) = (-0.7, 1.3);
    let shape = |t: f64| {
        let phase = (t / period).fract() * period;
        let level = if phase < edge {
            phase / edge
        } else if phase < period / 2.0 {
            1.0
        } else if phase < period / 2.0 + edge {
            1.0 - (phase - period / 2.0) / edge
        } else {
            0.0
        };
        low + (high - low) * level
    };
    let x = uneven_grid(0.0, 8.0 * period, 40_001);
    let y = sampled(&x, shape);
    // A window of 7.3 periods starting mid-cycle: the average is not the
    // midpoint, but every rising edge crosses it at the same phase.
    let window = Window {
        from: 0.37 * period,
        to: 7.67 * period,
    };
    let measured = frequency(&x, &y, Some(window)).unwrap();
    // Interpolation is exact on the linear edges; only the
    // piecewise-linear sampling of the corners can perturb the average.
    assert_close(measured / frequency_hz, 1.0, 1e-9, "frequency");
    // Fewer than two rising crossings: no frequency.
    let short = Window {
        from: 0.0,
        to: 0.9 * period,
    };
    assert_eq!(frequency(&x, &y, Some(short)), None);
}

#[test]
fn sine_frequency_with_offset() {
    let x = uneven_grid(0.0, 5.5e-3, 50_001);
    let y = sampled(&x, |t| 0.8 + 0.1 * (OMEGA * t + 0.3).sin());
    let measured = frequency(&x, &y, None).unwrap();
    assert_close(measured / 1e3, 1.0, 1e-6, "frequency");
}

/// Q62: hovering a resistor shows its RMS voltage, RMS current and average
/// power. R1 = 2 kΩ sits between `a` and `b`, with V(a) = 1 + 3·sin(ωt) and
/// V(b) = 0.25·V(a), so the voltage across it is 0.75 + 2.25·sin(ωt).
#[test]
fn hover_box_quantities_for_a_resistor() {
    let resistance = 2e3;
    let x = uneven_grid(0.0, 4e-3, 40_001);
    let v_a = sampled(&x, |t| 1.0 + 3.0 * (OMEGA * t).sin());
    let v_b: Vec<f64> = v_a.iter().map(|v| 0.25 * v).collect();
    let current: Vec<f64> = v_a
        .iter()
        .zip(&v_b)
        .map(|(a, b)| (a - b) / resistance)
        .collect();
    let result = transient(
        x.clone(),
        &[("V(a)", v_a), ("V(b)", v_b), ("I(R1)", current)],
    );

    let measure = |source: &str| {
        let expression = Expression::parse(source).unwrap();
        let Series::Real(values) = expression.evaluate(&result).unwrap() else {
            panic!("transient expressions are real");
        };
        (stats(&x, &values, None).unwrap(), expression.unit())
    };
    let (dc, amplitude) = (0.75, 2.25);
    // Over whole periods: mean(v²) = dc² + amplitude²/2.
    let mean_square = dc * dc + amplitude * amplitude / 2.0;
    // The power samples v²/R are integrated by trapezoids: the error is at
    // most h²·max|(v²)''|/12, and |(v²)''| = 2|v'² + v·v''| is at most
    // 2ω²·amplitude·(2·amplitude + dc). That bound also covers the RMS.
    let tolerance = amplitude * (2.0 * amplitude + dc) * (OMEGA * max_step(&x)).powi(2) / 4.0;

    let (voltage, unit) = measure("V(a, b)");
    assert_eq!(unit, "V");
    assert_close(voltage.rms, mean_square.sqrt(), tolerance, "RMS voltage");
    let (current, unit) = measure("I(R1)");
    assert_eq!(unit, "A");
    assert_close(
        current.rms,
        mean_square.sqrt() / resistance,
        tolerance / resistance,
        "RMS current",
    );
    let (power, unit) = measure("V(a, b) * I(R1)");
    assert_eq!(unit, "W");
    assert_close(
        power.average,
        mean_square / resistance,
        tolerance / resistance,
        "average power",
    );
}
