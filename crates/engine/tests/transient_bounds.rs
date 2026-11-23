//! Step control and result size: closely spaced breakpoints (T3), bounded
//! results (D18, T4, SR-3) and the accuracy of resonant circuits (T9).

use std::sync::Arc;
use std::thread;
use std::time::Duration;
use xsim_spice_engine::*;

fn spec(stop: f64, step: f64, initial_state: InitialState) -> Transient {
    Transient {
        stop,
        step,
        start_save: 0.0,
        max_step: None,
        initial_state,
    }
}

fn samples<'a>(result: &'a SimulationResult, name: &str) -> &'a [f64] {
    result
        .signal(name)
        .unwrap_or_else(|| panic!("no signal {name}"))
        .real()
        .unwrap()
}

/// Linear interpolation of `name` at `t`.
fn interpolate(result: &SimulationResult, name: &str, t: f64) -> f64 {
    let times = &result.axis.values;
    let values = samples(result, name);
    let k = times.partition_point(|&x| x <= t).clamp(1, times.len() - 1);
    let (t0, t1) = (times[k - 1], times[k]);
    values[k - 1] + (values[k] - values[k - 1]) * (t - t0) / (t1 - t0)
}

/// Largest deviation of `name` from `expected` over every sample.
fn worst_error(result: &SimulationResult, name: &str, expected: impl Fn(f64) -> f64) -> f64 {
    result
        .axis
        .values
        .iter()
        .zip(samples(result, name))
        .map(|(&t, &v)| (v - expected(t)).abs())
        .fold(0.0, f64::max)
}

/// Runs `analysis` on another thread and cancels it after `patience`, so a
/// regression that accepts a hopeless request cannot hang the suite.
fn run_with_patience(
    circuit: Circuit,
    analysis: Analysis,
    options: SimOptions,
    patience: Duration,
) -> Result<SimulationResult, SimError> {
    let control = Arc::new(SimControl::new());
    let shared = Arc::clone(&control);
    let handle = thread::spawn(move || simulate(&circuit, &analysis, &options, &shared));
    let start = std::time::Instant::now();
    while !handle.is_finished() && start.elapsed() < patience {
        thread::sleep(Duration::from_millis(5));
    }
    control.cancel();
    handle.join().unwrap()
}

/// 1 kΩ and 1 µF (τ = 1 ms) driven by `source`.
fn rc(source: Source) -> Circuit {
    let mut c = Circuit::new();
    let (a, b) = (c.node("in"), c.node("out"));
    c.add(Element::new(
        "V1",
        [a, GROUND],
        Device::VoltageSource(source),
    ));
    c.add(Element::new("R1", [a, b], Device::resistor(1e3)));
    c.add(Element::new("C1", [b, GROUND], Device::capacitor(1e-6)));
    c
}

#[test]
fn picosecond_edges_on_long_runs_do_not_stop_the_step() {
    // A 1 ps edge is ten minimum steps (1e-15 × 100 s); the edges used to
    // collapse the restart step below the minimum without any rejection.
    // On the 1000 s run the 10 ps edge is rejected once, and the shrink the
    // error estimate asks for (to 0.2 ps) undershoots the 1 ps minimum: the
    // minimum step itself is tried, and passes, before the run gives up.
    for (stop, delay, rise) in [
        (100.0, 0.0, 1e-12),
        (100.0, 1e-3, 1e-12),
        (10.0, 1e-3, 1e-12),
        (1.0, 1e-3, 1e-14),
        (1000.0, 1e-3, 1e-11),
    ] {
        let pulse = Waveform::Pulse {
            initial: 0.0,
            pulsed: 1.0,
            delay,
            rise,
            fall: rise,
            width: 1e9,
            period: 0.0,
            cycles: None,
        };
        let r = simulate(
            &rc(Source::with_waveform(pulse)),
            &Analysis::Transient(spec(stop, stop / 100.0, InitialState::OperatingPoint)),
            &SimOptions::default(),
            &SimControl::new(),
        )
        .unwrap_or_else(|e| panic!("stop {stop}, delay {delay}, rise {rise}: {e}"));
        let exact = |t: f64| {
            if t <= delay {
                0.0
            } else {
                -(-(t - delay) / 1e-3_f64).exp_m1()
            }
        };
        for t in [0.5e-3, 1e-3, 2e-3, 5e-3, stop / 2.0] {
            let value = interpolate(&r, "V(out)", delay + t);
            assert!(
                (value - exact(delay + t)).abs() < 2e-6,
                "stop {stop}, rise {rise}: {value} at {}",
                delay + t
            );
        }
    }
}

/// Response of an RC (`tau`) at rest to a 1 V ramp from `delay` over `rise`.
fn ramp_response(t: f64, delay: f64, rise: f64, tau: f64) -> f64 {
    let u = t - delay;
    if u <= 0.0 {
        0.0
    } else if u <= rise {
        (u + tau * (-u / tau).exp_m1()) / rise
    } else {
        1.0 - tau / rise * (rise / tau).exp_m1() * (-u / tau).exp()
    }
}

#[test]
fn picosecond_edges_into_low_impedances_run_with_every_method() {
    // The same 1 ms time constant from 1 Ω and 1 mF (10 mΩ and 0.1 F): the
    // edge drives up to 1e14 A/s into the capacitor, so the restart step
    // just after the corner moved more charge than the absolute chgtol
    // (1e-16 C) admits even at the minimum step, and every method stopped
    // with a timestep too small at the edge. The truncation-error floor of a
    // large capacitor is now the charge that moves it by vntol, and that of
    // a large inductor the flux that moves its current by abstol: the 1 V
    // edge across 1 H (with 1 kΩ, the same 1 ms) failed alike. Backward
    // Euler also failed the 10 ps edge on the 1 kΩ, 1 µF RC over 1000 s, one
    // of the review's examples. LTspice 24.1.9 runs the 1 Ω case
    // (`tests/reference/tran_lowz_edge.cir`) and reads V(out) =
    // 0.632019355697866 at 2 ms, 1e-4 below the closed form, from its
    // coarse output; xsim matches the closed form.
    let methods = [
        IntegrationMethod::Trapezoidal,
        IntegrationMethod::ModifiedTrapezoidal,
        IntegrationMethod::Gear2,
        IntegrationMethod::BackwardEuler,
    ];
    // Resistance, the reactive part, stop time, edge, methods; every time
    // constant is 1 ms. The RC is read as V(out), the RL as R·I(L1).
    let cases = [
        (1.0, Device::capacitor(1e-3), 100.0, 1e-12, &methods[..]),
        (1e-2, Device::capacitor(0.1), 1.0, 1e-13, &methods[..]),
        (1e3, Device::capacitor(1e-6), 1000.0, 1e-11, &methods[3..]),
        (1e3, Device::inductor(1.0), 100.0, 1e-12, &methods[..]),
    ];
    for (resistance, part, stop, rise, methods) in cases {
        let delay = 1e-3;
        let inductive = matches!(part, Device::Inductor { .. });
        let pulse = Waveform::Pulse {
            initial: 0.0,
            pulsed: 1.0,
            delay,
            rise,
            fall: rise,
            width: 1e9,
            period: 0.0,
            cycles: None,
        };
        let mut c = rc(Source::with_waveform(pulse));
        let nodes = [c.elements[1].nodes[0], c.elements[1].nodes[1]];
        c.elements[1] = Element::new("R1", nodes, Device::resistor(resistance));
        let nodes = [c.elements[2].nodes[0], c.elements[2].nodes[1]];
        let name = if inductive { "L1" } else { "C1" };
        c.elements[2] = Element::new(name, nodes, part.clone());
        for &method in methods {
            // Backward Euler needs ~1/reltol steps per time constant.
            let (reltol, tolerance) = if method == IntegrationMethod::BackwardEuler {
                (1e-4, 5e-4)
            } else {
                (1e-6, 2e-6)
            };
            let options = SimOptions {
                method,
                reltol,
                ..SimOptions::default()
            };
            let r = simulate(
                &c,
                &Analysis::Transient(spec(stop, stop / 100.0, InitialState::OperatingPoint)),
                &options,
                &SimControl::new(),
            )
            .unwrap_or_else(|e| panic!("{name}, {resistance} Ω, {stop} s, {method:?}: {e}"));
            for t in [delay + rise / 2.0, 1.5e-3, 2e-3, 5e-3] {
                let value = if inductive {
                    resistance * interpolate(&r, "I(L1)", t)
                } else {
                    interpolate(&r, "V(out)", t)
                };
                let exact = ramp_response(t, delay, rise, 1e-3);
                assert!(
                    (value - exact).abs() < tolerance,
                    "{name}, {resistance} Ω, {stop} s, {method:?}: {value} at {t}, exact {exact}"
                );
            }
        }
    }
}

#[test]
fn picosecond_edges_into_small_inductors_run_with_every_method() {
    // The dual of the low-impedance RC case above: a 1 V, 1 ps edge at 1 ms
    // across R1 in series with L1 from rest, on a 100 s run (a 0.1 ps
    // minimum step). The inductor takes the whole slew, so the restart step
    // at the corner moves its flux by 2.5e-15 V·s even at the minimum step,
    // more than the floor of any inductor below 2.5 mH (the flux that moves
    // its current by abstol): every method stopped with "Timestep too small
    // at t = 1 ms" for 1 mH and 1 µH with 1 kΩ or 1 Ω, and backward Euler
    // also for 10 mH. At the minimum step a flux may now move by the dual of
    // chgtol; the run warns with the current resolution that implies. With
    // τ = L/R from 1 ns to 10 µs, R·I(L1) is compared with the closed form
    // in the ramp and at 0.1 to 5 τ after it. 1 µH with 1 kΩ (τ = 1 ns) is
    // limited by the minimum step: the warning estimates its current error
    // at 2.9 nA (2.9 µV across R1); measured 3.8e-6 to 8.8e-6 off right after
    // the edge, short of six digits.
    let methods = [
        IntegrationMethod::Trapezoidal,
        IntegrationMethod::ModifiedTrapezoidal,
        IntegrationMethod::Gear2,
        IntegrationMethod::BackwardEuler,
    ];
    let (stop, delay, rise) = (100.0, 1e-3, 1e-12);
    for (resistance, inductance, tolerance) in [
        (1e3, 1e-3, 2e-6),
        (1.0, 1e-3, 2e-6),
        (1.0, 1e-6, 2e-6),
        (1e3, 10e-3, 2e-6),
        (1e3, 1e-6, 2e-5),
    ] {
        let pulse = Waveform::Pulse {
            initial: 0.0,
            pulsed: 1.0,
            delay,
            rise,
            fall: rise,
            width: 1e9,
            period: 0.0,
            cycles: None,
        };
        let mut c = Circuit::new();
        let (a, b) = (c.node("in"), c.node("out"));
        c.add(Element::new(
            "V1",
            [a, GROUND],
            Device::VoltageSource(Source::with_waveform(pulse)),
        ));
        c.add(Element::new("R1", [a, b], Device::resistor(resistance)));
        c.add(Element::new(
            "L1",
            [b, GROUND],
            Device::inductor(inductance),
        ));
        let tau = inductance / resistance;
        for method in methods {
            let (reltol, tolerance) = if method == IntegrationMethod::BackwardEuler {
                (1e-4, 5e-4)
            } else {
                (1e-6, tolerance)
            };
            let options = SimOptions {
                method,
                reltol,
                ..SimOptions::default()
            };
            let what = format!("{resistance} Ω, {inductance} H, {method:?}");
            let control = SimControl::new();
            let r = simulate(
                &c,
                &Analysis::Transient(spec(stop, stop / 100.0, InitialState::OperatingPoint)),
                &options,
                &control,
            )
            .unwrap_or_else(|e| panic!("{what}: {e}"));
            let mut times = vec![delay + rise / 2.0];
            times.extend([0.1, 0.5, 1.0, 2.0, 5.0].map(|k| delay + rise + k * tau));
            for t in times {
                let value = resistance * interpolate(&r, "I(L1)", t);
                let exact = ramp_response(t, delay, rise, tau);
                assert!(
                    (value - exact).abs() < tolerance,
                    "{what}: {value} at {t}, exact {exact}"
                );
            }
            // Only the edges the minimum step cannot resolve to abstol warn.
            let warnings = control.warnings();
            let coarse = inductance < 2.5e-3 || method == IntegrationMethod::BackwardEuler;
            assert!(
                warnings.len() == usize::from(coarse)
                    && warnings
                        .iter()
                        .all(|w| w.contains("minimum timestep (100 fs)")),
                "{what}: {warnings:?}"
            );
        }
    }
}

#[test]
fn an_ideal_edge_after_a_long_quiet_stretch_resolves_the_decay_behind_it() {
    // An ideal 0 → 1 V edge through 1 Ω into 1 kΩ ‖ 1 nF (τ = 0.999 ns)
    // after a quiet stretch: the restart step after the edge is a hundredth
    // of the step before it (0.4 ms on a 1 s run). Both of its backward-Euler
    // results had settled, so the step-doubling check passed it and V(o)
    // ramped linearly over the first 0.2 ms instead of rising in a
    // nanosecond: 0.999 V off between the samples with every method, even at
    // the default tolerances (0.987 V on a 10 ms run at reltol 1e-3), with no
    // error. The curvature through the restart step's three points now sends
    // it back until it resolves the decay. Measured: within 0.93·reltol of
    // the closed form with the second-order methods, 1.3·reltol with
    // backward Euler, from 0.5 to 20 τ after the edge.
    let tau = 1e3 / 1001.0 * 1e-9;
    let settled = 1e3 / 1001.0;
    let methods = [
        (IntegrationMethod::Trapezoidal, 1e-6),
        (IntegrationMethod::Gear2, 1e-6),
        (IntegrationMethod::ModifiedTrapezoidal, 1e-6),
        (IntegrationMethod::BackwardEuler, 1e-4),
        (IntegrationMethod::Trapezoidal, 1e-3),
    ];
    for (stop, delay, step) in [(1.0, 0.5, 0.1), (10e-3, 5e-3, 1e-3)] {
        let edge = Waveform::Pulse {
            initial: 0.0,
            pulsed: 1.0,
            delay,
            rise: 0.0,
            fall: 0.0,
            width: 1e9,
            period: 0.0,
            cycles: None,
        };
        let mut c = Circuit::new();
        let (a, o) = (c.node("a"), c.node("o"));
        c.add(Element::new(
            "V1",
            [a, GROUND],
            Device::VoltageSource(Source::with_waveform(edge)),
        ));
        c.add(Element::new("R1", [a, o], Device::resistor(1.0)));
        c.add(Element::new("R2", [o, GROUND], Device::resistor(1e3)));
        c.add(Element::new("C2", [o, GROUND], Device::capacitor(1e-9)));
        for (method, reltol) in methods {
            let options = SimOptions {
                method,
                reltol,
                ..SimOptions::default()
            };
            let what = format!("{stop} s run, {method:?}, reltol {reltol}");
            let r = simulate(
                &c,
                &Analysis::Transient(spec(stop, step, InitialState::OperatingPoint)),
                &options,
                &SimControl::new(),
            )
            .unwrap_or_else(|e| panic!("{what}: {e}"));
            for k in [0.5_f64, 1.0, 2.0, 5.0, 20.0] {
                let value = interpolate(&r, "V(o)", delay + k * tau);
                let exact = -settled * (-k).exp_m1();
                assert!(
                    (value - exact).abs() < 2.0 * reltol,
                    "{what}: {value} at {k} τ, exact {exact}"
                );
            }
        }
    }
}

/// An LC tank (1 mH, 1 µF) ringing from an initial inductor current of
/// 1 mA: `V(n) = −(I0/(ω0·C))·sin(ω0·t)`, 31.6 mV amplitude.
fn tank() -> (Circuit, f64) {
    let (l, c_farad): (f64, f64) = (1e-3, 1e-6);
    let mut c = Circuit::new();
    let n = c.node("n");
    c.add(Element::new(
        "L1",
        [n, GROUND],
        Device::Inductor {
            inductance: l,
            initial_current: Some(1e-3),
        },
    ));
    c.add(Element::new("C1", [n, GROUND], Device::capacitor(c_farad)));
    (c, 1.0 / (l * c_farad).sqrt())
}

#[test]
fn results_keep_only_the_points_interpolation_needs() {
    // Backward Euler takes ~6·10⁴ steps per period of the tank at reltol
    // 1e-4; the result keeps the output grid, the breakpoints and what
    // linear interpolation needs (D18), not every solver step.
    let (circuit, w0) = tank();
    let period = std::f64::consts::TAU / w0;
    let options = SimOptions {
        method: IntegrationMethod::BackwardEuler,
        reltol: 1e-4,
        ..SimOptions::default()
    };
    let r = simulate(
        &circuit,
        &Analysis::Transient(spec(2.0 * period, period / 20.0, InitialState::ZeroState)),
        &options,
        &SimControl::new(),
    )
    .unwrap();
    assert!(r.len() < 4000, "{} samples for two periods", r.len());
    // Second-order methods at the default reltol: the stored points still
    // reproduce the waveform by interpolation within reltol·swing + vntol of
    // the exact solution, plus the solver's own error.
    for method in [
        IntegrationMethod::Trapezoidal,
        IntegrationMethod::ModifiedTrapezoidal,
        IntegrationMethod::Gear2,
    ] {
        let options = SimOptions {
            method,
            ..SimOptions::default()
        };
        let r = simulate(
            &circuit,
            &Analysis::Transient(spec(10.0 * period, period / 20.0, InitialState::ZeroState)),
            &options,
            &SimControl::new(),
        )
        .unwrap();
        let amplitude = 1e-3 / (w0 * 1e-6);
        let exact = |t: f64| -amplitude * (w0 * t).sin();
        let error = (0..=100_000)
            .map(|k| 10.0 * period * k as f64 / 100_000.0)
            .map(|t| (interpolate(&r, "V(n)", t) - exact(t)).abs())
            .fold(0.0, f64::max);
        assert!(
            error < 1e-5 * amplitude,
            "{method:?}: {error:e} with {} samples",
            r.len()
        );
        assert!(r.len() < 40_000, "{method:?}: {} samples", r.len());
    }
}

#[test]
fn a_maximum_step_beyond_the_result_limit_is_rejected_up_front() {
    // 10⁹ steps of 1 ns over 1 s: rejected before simulating anything
    // instead of filling memory for minutes.
    let transient = Transient {
        max_step: Some(1e-9),
        ..spec(1.0, 1e-3, InitialState::ZeroState)
    };
    let error = run_with_patience(
        rc(Source::dc(1.0)),
        Analysis::Transient(transient),
        SimOptions::default(),
        Duration::from_secs(5),
    )
    .unwrap_err();
    assert!(
        matches!(&error, SimError::InvalidCircuit(e) if e.message.contains("maximum step")),
        "{error}"
    );
}

#[test]
fn an_output_grid_beyond_the_result_limit_is_rejected_up_front() {
    // 5·10⁶ grid samples of 42 signals and the time axis exceed the stored
    // value limit; the grid is always kept, so this is known before running.
    let mut c = Circuit::new();
    let top = c.node("top");
    c.add(Element::new(
        "V1",
        [top, GROUND],
        Device::VoltageSource(Source::dc(1.0)),
    ));
    let mut previous = top;
    for k in 0..20 {
        let n = c.node(&format!("n{k}"));
        c.add(Element::new(
            format!("R{k}"),
            [previous, n],
            Device::resistor(1e3),
        ));
        previous = n;
    }
    let error = run_with_patience(
        c,
        Analysis::Transient(spec(1.0, 2e-7, InitialState::ZeroState)),
        SimOptions::default(),
        Duration::from_secs(5),
    )
    .unwrap_err();
    assert!(
        matches!(&error, SimError::InvalidCircuit(e) if e.message.contains("output step")),
        "{error}"
    );
}

/// Series RLC (1 mH, 1 µF, `R = √(L/C)/Q`) switched onto 1 V from rest.
fn series_rlc(q: f64) -> (Circuit, impl Fn(f64) -> f64) {
    let (l, c_farad): (f64, f64) = (1e-3, 1e-6);
    let r_ohm = (l / c_farad).sqrt() / q;
    let mut c = Circuit::new();
    let (input, a, out) = (c.node("in"), c.node("a"), c.node("out"));
    c.add(Element::new(
        "V1",
        [input, GROUND],
        Device::VoltageSource(Source::dc(1.0)),
    ));
    c.add(Element::new("R1", [input, a], Device::resistor(r_ohm)));
    c.add(Element::new("L1", [a, out], Device::inductor(l)));
    c.add(Element::new(
        "C1",
        [out, GROUND],
        Device::capacitor(c_farad),
    ));
    let alpha = r_ohm / (2.0 * l);
    let w0 = 1.0 / (l * c_farad).sqrt();
    let wd = (w0 * w0 - alpha * alpha).sqrt();
    let exact =
        move |t: f64| 1.0 - (-alpha * t).exp() * ((wd * t).cos() + alpha / wd * (wd * t).sin());
    (c, exact)
}

#[test]
fn resonant_errors_follow_the_documented_limits() {
    // The limits documented on the transient's error target (T9): a 1 V
    // step into a series RLC over 25 periods stays within 1.06e-6 V at
    // Q = 10 and 6.0e-6 V at Q = 100 (release-mode figures for every method
    // are in `measure_resonant_accuracy`).
    let cases = [
        (10.0, IntegrationMethod::Trapezoidal, 1.2e-6),
        (10.0, IntegrationMethod::ModifiedTrapezoidal, 1e-6),
        (10.0, IntegrationMethod::Gear2, 1.2e-6),
        (100.0, IntegrationMethod::Trapezoidal, 7e-6),
    ];
    for (q, method, limit) in cases {
        let (circuit, exact) = series_rlc(q);
        let options = SimOptions {
            method,
            ..SimOptions::default()
        };
        let r = simulate(
            &circuit,
            &Analysis::Transient(spec(5e-3, 1e-5, InitialState::ZeroState)),
            &options,
            &SimControl::new(),
        )
        .unwrap();
        let error = worst_error(&r, "V(out)", &exact);
        assert!(error < limit, "Q = {q}, {method:?}: error {error:e}");
    }
    // An undamped tank accumulates about 0.35·reltol·amplitude per period,
    // and a tighter reltol is the remedy for long runs.
    let (circuit, w0) = tank();
    let period = std::f64::consts::TAU / w0;
    let amplitude = 1e-3 / (w0 * 1e-6);
    for reltol in [1e-6, 1e-7] {
        let options = SimOptions {
            reltol,
            ..SimOptions::default()
        };
        let periods = 10.0;
        let r = simulate(
            &circuit,
            &Analysis::Transient(spec(
                periods * period,
                period / 20.0,
                InitialState::ZeroState,
            )),
            &options,
            &SimControl::new(),
        )
        .unwrap();
        let error = worst_error(&r, "V(n)", |t| -amplitude * (w0 * t).sin());
        let rate = error / (reltol * amplitude * periods);
        assert!(
            rate < 0.4,
            "reltol {reltol}: {rate} reltol·amplitude per period"
        );
    }
}

/// Release-only measurement behind the `ERROR_TARGET` documentation (T9):
/// `cargo test --release --test transient_bounds -- --ignored --nocapture`.
#[test]
#[ignore = "release-only measurement; prints the accuracy table"]
fn measure_resonant_accuracy() {
    for q in [1.0, 10.0, 100.0] {
        let (circuit, exact) = series_rlc(q);
        for method in [
            IntegrationMethod::Trapezoidal,
            IntegrationMethod::ModifiedTrapezoidal,
            IntegrationMethod::Gear2,
        ] {
            let options = SimOptions {
                method,
                ..SimOptions::default()
            };
            let start = std::time::Instant::now();
            let r = simulate(
                &circuit,
                &Analysis::Transient(spec(5e-3, 1e-5, InitialState::ZeroState)),
                &options,
                &SimControl::new(),
            )
            .unwrap();
            println!(
                "RLC Q={q:<5} {method:?}: error {:.3e} V, {} samples, {:.3} s",
                worst_error(&r, "V(out)", &exact),
                r.len(),
                start.elapsed().as_secs_f64()
            );
        }
    }
    let (circuit, w0) = tank();
    let period = std::f64::consts::TAU / w0;
    let amplitude = 1e-3 / (w0 * 1e-6);
    for periods in [10.0, 100.0, 1000.0] {
        for method in [
            IntegrationMethod::Trapezoidal,
            IntegrationMethod::ModifiedTrapezoidal,
            IntegrationMethod::Gear2,
        ] {
            let options = SimOptions {
                method,
                ..SimOptions::default()
            };
            let start = std::time::Instant::now();
            let r = simulate(
                &circuit,
                &Analysis::Transient(spec(
                    periods * period,
                    period / 20.0,
                    InitialState::ZeroState,
                )),
                &options,
                &SimControl::new(),
            )
            .unwrap();
            let error = worst_error(&r, "V(n)", |t| -amplitude * (w0 * t).sin());
            println!(
                "LC {periods:>5} periods {method:?}: error {:.3e} V = {:.3} reltol·amplitude per period, {} samples, {:.3} s",
                error,
                error / (1e-6 * amplitude * periods),
                r.len(),
                start.elapsed().as_secs_f64()
            );
        }
    }
}
