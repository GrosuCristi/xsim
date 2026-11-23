//! Source waveforms in transient analysis: smooth sources are resolved in
//! time even when no reactive state sees them (T2), a SINE with a cycle
//! count holds its final value (T7) and a repeating PWL follows LTspice's
//! `REPEAT` timing (T8). LTspice values come from batch runs of the decks in
//! `tests/reference/` (LTspice 24.1.9).

use std::f64::consts::TAU;
use xsim_spice_engine::*;

fn transient(stop: f64, step: f64, initial_state: InitialState) -> Analysis {
    Analysis::Transient(Transient {
        stop,
        step,
        start_save: 0.0,
        max_step: None,
        initial_state,
    })
}

fn run(circuit: &Circuit, analysis: &Analysis, options: &SimOptions) -> SimulationResult {
    simulate(circuit, analysis, options, &SimControl::new()).unwrap_or_else(|e| panic!("{e}"))
}

fn samples<'a>(result: &'a SimulationResult, name: &str) -> &'a [f64] {
    result
        .signal(name)
        .unwrap_or_else(|| panic!("no signal {name}"))
        .real()
        .unwrap()
}

/// Linear interpolation of `name` at `t` (what a plot or a cursor shows).
fn interpolate(result: &SimulationResult, name: &str, t: f64) -> f64 {
    let times = &result.axis.values;
    let values = samples(result, name);
    let k = times.partition_point(|&x| x <= t);
    if k == 0 {
        return values[0];
    }
    if k == times.len() {
        return values[k - 1];
    }
    let (t0, t1) = (times[k - 1], times[k]);
    values[k - 1] + (values[k] - values[k - 1]) * (t - t0) / (t1 - t0)
}

/// Largest deviation of the linearly interpolated `name` from `exact` at
/// `points` evenly spaced times in `[from, to]`.
fn worst_interpolated(
    result: &SimulationResult,
    name: &str,
    from: f64,
    to: f64,
    points: usize,
    exact: impl Fn(f64) -> f64,
) -> f64 {
    (0..=points)
        .map(|k| from + (to - from) * k as f64 / points as f64)
        .map(|t| (interpolate(result, name, t) - exact(t)).abs())
        .fold(0.0, f64::max)
}

/// `source` across `R1 = 1 kΩ` in series with `R2 = 1 kΩ` to ground.
fn divider(source: Source) -> Circuit {
    let mut c = Circuit::new();
    let (a, b) = (c.node("in"), c.node("out"));
    c.add(Element::new(
        "V1",
        [a, GROUND],
        Device::VoltageSource(source),
    ));
    c.add(Element::new("R1", [a, b], Device::resistor(1e3)));
    c.add(Element::new("R2", [b, GROUND], Device::resistor(1e3)));
    c
}

fn sine(frequency: f64) -> Waveform {
    Waveform::Sine {
        offset: 0.0,
        amplitude: 1.0,
        frequency,
        delay: 0.0,
        damping: 0.0,
        phase: 0.0,
        cycles: None,
    }
}

#[test]
fn a_sine_through_resistors_is_resolved_between_output_steps() {
    // tests/reference/tran_sine_divider.cir (`.tran 0 0.1`, no maximum
    // step): LTspice measures MAX V(out) = 0.499993853191003 and V(out) =
    // 0.499885528771284 at 50.25 ms, where the exact value is 0.5.
    let lt_max = 0.499993853191003;
    let circuit = divider(Source::with_waveform(sine(1e3)));
    let exact = |t: f64| 0.5 * (TAU * 1e3 * t).sin();
    for step in [1e-3, 0.1] {
        for method in [IntegrationMethod::Trapezoidal, IntegrationMethod::Gear2] {
            let options = SimOptions {
                method,
                ..SimOptions::default()
            };
            let r = run(
                &circuit,
                &transient(0.1, step, InitialState::OperatingPoint),
                &options,
            );
            let v = samples(&r, "V(out)");
            let max = v.iter().copied().fold(f64::MIN, f64::max);
            let min = v.iter().copied().fold(f64::MAX, f64::min);
            // The D18 tolerance: reltol·swing + vntol, here 1.001e-6 V. The
            // samples hold the peaks within it; between samples the step
            // limit's chord error and the thinning each add up to one more.
            let tolerance = 1e-6 * 1.0 + 1e-9;
            assert!(
                (max - 0.5).abs() <= tolerance && (min + 0.5).abs() <= tolerance,
                "{method:?}, step {step}: {} samples span [{min}, {max}]",
                r.len()
            );
            assert!((max - lt_max).abs() < 1e-5, "{method:?}: {max} vs LTspice");
            // What a plot shows between the samples is the waveform too.
            let error = worst_interpolated(&r, "V(out)", 0.0, 0.1, 200_003, exact);
            assert!(
                error <= 2.0 * tolerance,
                "{method:?}, step {step}: interpolation error {error:e}"
            );
            assert!(
                (interpolate(&r, "V(out)", 50.25e-3) - 0.5).abs() <= 2.0 * tolerance,
                "{method:?} at 50.25 ms"
            );
        }
    }
}

#[test]
fn a_damped_sine_an_sffm_and_an_exp_source_are_resolved_too() {
    let waveforms = [
        Waveform::Sine {
            offset: 0.2,
            amplitude: 2.0,
            frequency: 5e3,
            delay: 0.1e-3,
            damping: 2e3,
            phase: 30.0,
            cycles: None,
        },
        Waveform::Sffm {
            offset: 0.0,
            amplitude: 1.0,
            carrier: 10e3,
            index: 3.0,
            signal: 1e3,
        },
        Waveform::Exp {
            initial: 0.0,
            pulsed: 1.0,
            rise_delay: 0.1e-3,
            rise_tau: 20e-6,
            fall_delay: 1e-3,
            fall_tau: 50e-6,
        },
    ];
    for waveform in waveforms {
        let circuit = divider(Source::with_waveform(waveform.clone()));
        let r = run(
            &circuit,
            &transient(2e-3, 1e-3, InitialState::OperatingPoint),
            &SimOptions::default(),
        );
        let v = samples(&r, "V(in)");
        let swing =
            v.iter().copied().fold(f64::MIN, f64::max) - v.iter().copied().fold(f64::MAX, f64::min);
        let error = worst_interpolated(&r, "V(in)", 0.0, 2e-3, 100_000, |t| waveform.value(t));
        // Chord error of the steps plus the thinning tolerance (D18).
        assert!(
            error <= 2.0 * (1e-6 * swing + 1e-9),
            "{waveform:?}: interpolation error {error:e} over {} samples",
            r.len()
        );
    }
}

#[test]
fn a_sine_with_a_cycle_count_holds_its_final_value_like_ltspice() {
    // tests/reference/tran_sine_cycles.cir: V1 SINE(0.5 2 50 10m 30 30 2)
    // and V2 SINE(0 1 1k 0 0 0 2.25).
    let w = Waveform::Sine {
        offset: 0.5,
        amplitude: 2.0,
        frequency: 50.0,
        delay: 10e-3,
        damping: 30.0,
        phase: 30.0,
        cycles: Some(2.0),
    };
    let lt = [
        (23e-3, -0.84669577152422),
        (49e-3, 0.629057830944554),
        (55e-3, 0.801194211912202),
        (60e-3, 0.801194211912202),
    ];
    for (t, expected) in lt {
        assert!(
            (w.value(t) - expected).abs() < 1e-9,
            "at {t}: {} vs LTspice {expected}",
            w.value(t)
        );
    }
    // Continuous at the end of the last cycle: no jump to the offset.
    assert!((w.value(50e-3) - w.value_before(50e-3)).abs() < 1e-12);
    let fractional = Waveform::Sine {
        offset: 0.0,
        amplitude: 1.0,
        frequency: 1e3,
        delay: 0.0,
        damping: 0.0,
        phase: 0.0,
        cycles: Some(2.25),
    };
    for t in [3e-3, 10e-3] {
        assert!((fractional.value(t) - 1.0).abs() < 1e-12, "at {t}");
    }
    // The circuit sees the held value.
    let r = run(
        &divider(Source::with_waveform(w)),
        &transient(70e-3, 5e-3, InitialState::OperatingPoint),
        &SimOptions::default(),
    );
    let k = r.axis.values.iter().position(|&t| t == 60e-3).unwrap();
    assert!((samples(&r, "V(in)")[k] - 0.801194211912202).abs() < 1e-9);
}

#[test]
fn a_repeating_pwl_follows_ltspice_repeat_timing() {
    // tests/reference/tran_pwl_repeat.cir: PWL REPEAT FOREVER (1m 0 2m 4 3m 2)
    // and (1m 1 2m 4 3m 2); tests/reference/tran_pwl_repeat_zero.cir:
    // (0 1 1m 4 2m 1) and (0.5m 1 1m 3).
    let pwl = |points: &[(f64, f64)]| Waveform::Pwl {
        points: points.to_vec(),
        repeat: true,
    };
    let cases = [
        (
            pwl(&[(1e-3, 0.0), (2e-3, 4.0), (3e-3, 2.0)]),
            vec![
                (0.5e-3, 0.0),
                (1.5e-3, 2.0),
                (3.5e-3, 1.0),
                (4.5e-3, 2.0),
                (6.5e-3, 1.0),
                (8.5e-3, 3.0),
            ],
        ),
        (
            pwl(&[(1e-3, 1.0), (2e-3, 4.0), (3e-3, 2.0)]),
            vec![(0.0, 1.0), (0.5e-3, 1.0), (3.5e-3, 1.5), (3.9e-3, 1.1)],
        ),
        (
            pwl(&[(0.0, 1.0), (1e-3, 4.0), (2e-3, 1.0)]),
            vec![(0.5e-3, 2.5), (1.9e-3, 1.3), (2.5e-3, 2.5), (4.5e-3, 2.5)],
        ),
        (
            pwl(&[(0.5e-3, 1.0), (1e-3, 3.0)]),
            vec![
                (0.2e-3, 1.0),
                (1.2e-3, 2.2),
                (1.75e-3, 2.0),
                (2.5e-3, 1.0),
                (4e-3, 3.0),
            ],
        ),
    ];
    for (w, expected) in cases {
        for (t, lt) in expected {
            assert!(
                (w.value(t) - lt).abs() < 1e-12,
                "{w:?} at {t}: {} vs LTspice {lt}",
                w.value(t)
            );
        }
        // Every corner is a breakpoint, and the solver lands on each one.
        let r = run(
            &divider(Source::with_waveform(w.clone())),
            &transient(10e-3, 1e-3, InitialState::OperatingPoint),
            &SimOptions::default(),
        );
        for t in w.breakpoints(10e-3) {
            let k = r
                .axis
                .values
                .iter()
                .position(|&x| x == t)
                .unwrap_or_else(|| panic!("{w:?}: no sample at {t}"));
            assert!((samples(&r, "V(in)")[k] - w.value_before(t)).abs() < 1e-12);
        }
        let error = worst_interpolated(&r, "V(in)", 0.0, 10e-3, 20_000, |t| w.value(t));
        assert!(error < 1e-9, "{w:?}: {error:e}");
    }
}
