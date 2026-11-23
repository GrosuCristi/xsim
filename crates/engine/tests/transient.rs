//! Transient analysis against closed-form solutions (Q153: six decimals) and
//! LTspice cross-checks, plus sampling rules and validation.

use std::f64::consts::TAU;
use xsim_spice_engine::*;

const METHODS: [IntegrationMethod; 4] = [
    IntegrationMethod::BackwardEuler,
    IntegrationMethod::Trapezoidal,
    IntegrationMethod::ModifiedTrapezoidal,
    IntegrationMethod::Gear2,
];

/// Six-decimal accuracy (Q153).
const VOLTS: f64 = 1e-6;

fn spec(stop: f64, step: f64, initial_state: InitialState) -> Transient {
    Transient {
        stop,
        step,
        start_save: 0.0,
        max_step: None,
        initial_state,
    }
}

fn run(circuit: &Circuit, spec: Transient, method: IntegrationMethod) -> SimulationResult {
    let options = SimOptions {
        method,
        ..SimOptions::default()
    };
    simulate(
        circuit,
        &Analysis::Transient(spec),
        &options,
        &SimControl::new(),
    )
    .unwrap_or_else(|e| panic!("{method:?}: {e}"))
}

fn samples<'a>(result: &'a SimulationResult, name: &str) -> &'a [f64] {
    result
        .signal(name)
        .unwrap_or_else(|| panic!("no signal {name}"))
        .real()
        .unwrap()
}

/// Index of the sample at exactly `t`.
fn at(result: &SimulationResult, t: f64) -> usize {
    result
        .axis
        .values
        .iter()
        .position(|&x| x == t)
        .unwrap_or_else(|| panic!("no sample at t = {t}"))
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

fn assert_sampling_rules(result: &SimulationResult, stop: f64, step: f64, start_save: f64) {
    let t = &result.axis.values;
    assert_eq!(result.axis.name, "time");
    assert_eq!(result.axis.unit, "s");
    assert!(
        t.windows(2).all(|w| w[1] > w[0]),
        "times must increase strictly"
    );
    assert_eq!(t[0], start_save);
    assert_eq!(*t.last().unwrap(), stop);
    let mut k = (start_save / step).ceil();
    while k * step < stop {
        at(result, k * step);
        k += 1.0;
    }
    assert!(result.signals.iter().all(|s| s.data.len() == t.len()));
}

/// 1 V source, 1 kΩ, 1 µF (τ = 1 ms).
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
fn rc_step_from_rest_is_exact_to_six_decimals_with_every_method() {
    let tau = 1e-3;
    for method in METHODS {
        let r = run(
            &rc(Source::dc(1.0)),
            spec(5e-3, 1e-3, InitialState::ZeroState),
            method,
        );
        assert_sampling_rules(&r, 5e-3, 1e-3, 0.0);
        let v = samples(&r, "V(out)");
        for k in 1..=5 {
            let t = k as f64 * 1e-3;
            let expected = 1.0 - (-t / tau).exp();
            let error = v[at(&r, t)] - expected;
            assert!(error.abs() < VOLTS, "{method:?} at {t}: error {error:e}");
        }
        // The prototype's published six-decimal values.
        assert!((v[at(&r, 1e-3)] - 0.632121).abs() < VOLTS);
        assert!((v[at(&r, 5e-3)] - 0.993262).abs() < VOLTS);
        // The capacitor current is the integrated charge derivative; the
        // t = 0 sample already shows the charging current (T6).
        let current_error = worst_error(&r, "I(C1)", |t| (-t / tau).exp() / 1e3);
        assert!(
            current_error < 1e-8,
            "{method:?}: current error {current_error:e}"
        );
    }
}

#[test]
fn rc_step_from_rest_agrees_with_ltspice() {
    // LTspice 24.1.9 batch run of tests/reference/core_rc_uic.cir
    // (`uic`, reltol = 1e-9, 100 ns maximum step); these are its .meas
    // results for V(out) at 1, 2, 3, 4 and 5 ms. The prototype measured the
    // same values.
    let ltspice = [
        0.63212055874,
        0.864664716842,
        0.950212931702,
        0.981684361151,
        0.993262053027,
    ];
    let r = run(
        &rc(Source::dc(1.0)),
        spec(5e-3, 1e-3, InitialState::ZeroState),
        IntegrationMethod::Trapezoidal,
    );
    let v = samples(&r, "V(out)");
    for (k, expected) in ltspice.iter().enumerate() {
        let t = (k + 1) as f64 * 1e-3;
        assert!((v[at(&r, t)] - expected).abs() < VOLTS, "at {t}");
    }
}

#[test]
fn rc_pulse_edge_matches_the_ramp_solution_and_ltspice() {
    // PULSE(0 1 0 1n 1n 1 2) as in tests/reference/core_rc_pulse.cir. Its
    // values at 1..5 ms from an LTspice 24.1.9 batch run (default tolerances,
    // plotwinsize = 0, 1 µs maximum step):
    let ltspice = [
        0.63212037755,
        0.864664661353,
        0.950212915397,
        0.981684356665,
        0.993262052439,
    ];
    let rise = 1e-9;
    let pulse = Waveform::Pulse {
        initial: 0.0,
        pulsed: 1.0,
        delay: 0.0,
        rise,
        fall: 1e-9,
        width: 1.0,
        period: 2.0,
        cycles: None,
    };
    let tau: f64 = 1e-3;
    // A linear ramp over `rise`, then constant: 1 − (τ/tr)(e^(tr/τ) − 1)e^(−t/τ).
    let exact = |t: f64| 1.0 - tau / rise * (rise / tau).exp_m1() * (-t / tau).exp();
    let transient = Transient {
        max_step: Some(1e-6),
        ..spec(5e-3, 1e-3, InitialState::OperatingPoint)
    };
    for method in [IntegrationMethod::Trapezoidal, IntegrationMethod::Gear2] {
        let r = run(
            &rc(Source::with_waveform(pulse.clone())),
            transient.clone(),
            method,
        );
        let v = samples(&r, "V(out)");
        assert_eq!(v[0], 0.0);
        at(&r, rise);
        for (k, lt) in ltspice.iter().enumerate() {
            let t = (k + 1) as f64 * 1e-3;
            let value = v[at(&r, t)];
            assert!(
                (value - exact(t)).abs() < VOLTS,
                "{method:?} at {t}: {value} vs {}",
                exact(t)
            );
            assert!(
                (value - lt).abs() < VOLTS,
                "{method:?} at {t}: {value} vs LTspice {lt}"
            );
        }
    }
}

#[test]
fn every_grid_time_and_the_short_final_interval_are_recorded() {
    // Port of the prototype: a stop time that is not a multiple of the step.
    let r = run(
        &rc(Source::dc(1.0)),
        spec(4.5e-3, 1e-3, InitialState::ZeroState),
        IntegrationMethod::Trapezoidal,
    );
    assert_sampling_rules(&r, 4.5e-3, 1e-3, 0.0);
    for t in [0.0, 1e-3, 2e-3, 3e-3, 4e-3, 4.5e-3] {
        let k = at(&r, t);
        assert!((samples(&r, "V(out)")[k] - (1.0 - (-t / 1e-3_f64).exp())).abs() < VOLTS);
    }
}

#[test]
fn rl_current_rises_exponentially() {
    // Port of the prototype: 1 V, 1 kΩ, 1 H (τ = 1 ms).
    let mut c = Circuit::new();
    let (a, b) = (c.node("1"), c.node("2"));
    c.add(Element::new(
        "V1",
        [a, GROUND],
        Device::VoltageSource(Source::dc(1.0)),
    ));
    c.add(Element::new("R1", [a, b], Device::resistor(1e3)));
    c.add(Element::new("L1", [b, GROUND], Device::inductor(1.0)));
    for method in METHODS {
        let r = run(&c, spec(3e-3, 1e-3, InitialState::ZeroState), method);
        let v = worst_error(&r, "V(2)", |t| (-t / 1e-3_f64).exp());
        let i = worst_error(&r, "I(L1)", |t| (1.0 - (-t / 1e-3_f64).exp()) / 1e3);
        assert!(v < VOLTS, "{method:?}: voltage error {v:e}");
        assert!(i < 1e-9, "{method:?}: current error {i:e}");
    }
}

#[test]
fn current_source_charges_a_parallel_rc_and_kcl_holds() {
    // Port of the prototype: 2 mA into 600 Ω ∥ 1 µF.
    let mut c = Circuit::new();
    let n = c.node("1");
    c.add(Element::new(
        "I1",
        [GROUND, n],
        Device::CurrentSource(Source::dc(2e-3)),
    ));
    c.add(Element::new("R1", [n, GROUND], Device::resistor(600.0)));
    c.add(Element::new("C1", [n, GROUND], Device::capacitor(1e-6)));
    let r = run(
        &c,
        spec(6e-4, 6e-4, InitialState::ZeroState),
        IntegrationMethod::Trapezoidal,
    );
    let last = r.len() - 1;
    assert!((samples(&r, "V(1)")[last] - 1.2 * (1.0 - (-1.0_f64).exp())).abs() < VOLTS);
    for k in 0..r.len() {
        let total = samples(&r, "I(R1)")[k] + samples(&r, "I(C1)")[k];
        assert!(
            (total - 2e-3).abs() < 1e-15,
            "KCL residual {} at sample {k}",
            total - 2e-3
        );
    }
}

#[test]
fn a_current_source_into_a_capacitor_ramps_linearly() {
    // Port of the prototype: no DC path, so only a transient from rest works.
    let mut c = Circuit::new();
    let n = c.node("1");
    c.add(Element::new(
        "I1",
        [GROUND, n],
        Device::CurrentSource(Source::dc(1e-3)),
    ));
    c.add(Element::new("C1", [n, GROUND], Device::capacitor(1e-6)));
    for method in METHODS {
        let r = run(&c, spec(2e-3, 1e-3, InitialState::ZeroState), method);
        assert!(worst_error(&r, "V(1)", |t| 1e3 * t) < 1e-12, "{method:?}");
        assert!((samples(&r, "V(1)")[at(&r, 2e-3)] - 2.0).abs() < 1e-12);
    }
}

#[test]
fn series_rlc_rings_like_the_analytic_solution() {
    // 10 Ω, 1 mH, 1 µF: α = 5000 /s, ω₀ = 31623 rad/s, underdamped.
    let (r_ohm, l, c_farad): (f64, f64, f64) = (10.0, 1e-3, 1e-6);
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
    let voltage =
        |t: f64| 1.0 - (-alpha * t).exp() * ((wd * t).cos() + alpha / wd * (wd * t).sin());
    let current = |t: f64| c_farad * w0 * w0 / wd * (-alpha * t).exp() * (wd * t).sin();
    for method in [
        IntegrationMethod::Trapezoidal,
        IntegrationMethod::Gear2,
        IntegrationMethod::ModifiedTrapezoidal,
    ] {
        let r = run(&c, spec(1e-3, 1e-5, InitialState::ZeroState), method);
        let v = worst_error(&r, "V(out)", voltage);
        let i = worst_error(&r, "I(L1)", current);
        assert!(v < VOLTS, "{method:?}: voltage error {v:e}");
        assert!(i < 1e-8, "{method:?}: current error {i:e}");
    }
}

#[test]
fn sine_through_an_rc_follows_transient_plus_steady_state() {
    // 1 kHz, 2 V amplitude into 100 Ω and 1 µF from rest.
    let (amplitude, f, tau): (f64, f64, f64) = (2.0, 1e3, 1e-4);
    let source = Source::with_waveform(Waveform::Sine {
        offset: 0.0,
        amplitude,
        frequency: f,
        delay: 0.0,
        damping: 0.0,
        phase: 0.0,
        cycles: None,
    });
    let mut c = Circuit::new();
    let (a, b) = (c.node("in"), c.node("out"));
    c.add(Element::new(
        "V1",
        [a, GROUND],
        Device::VoltageSource(source),
    ));
    c.add(Element::new("R1", [a, b], Device::resistor(100.0)));
    c.add(Element::new("C1", [b, GROUND], Device::capacitor(1e-6)));
    let wt = TAU * f * tau;
    let exact = |t: f64| {
        let w = TAU * f;
        amplitude / (1.0 + wt * wt) * ((w * t).sin() - wt * (w * t).cos() + wt * (-t / tau).exp())
    };
    for method in METHODS[1..].iter().copied() {
        let r = run(&c, spec(3e-3, 1e-4, InitialState::ZeroState), method);
        let error = worst_error(&r, "V(out)", exact);
        assert!(error < VOLTS, "{method:?}: error {error:e}");
    }
}

/// Response of a 1 ms RC to a unit-slope ramp starting at 0: `t − τ + τe^(−t/τ)`.
fn ramp_response(t: f64, tau: f64) -> f64 {
    if t <= 0.0 {
        0.0
    } else {
        tau * (t / tau + (-t / tau).exp_m1())
    }
}

#[test]
fn pulse_train_corners_are_sampled_and_its_response_is_exact() {
    let (delay, rise, fall, width, period) = (50e-6, 40e-6, 60e-6, 100e-6, 400e-6);
    let pulse = Waveform::Pulse {
        initial: 0.0,
        pulsed: 1.0,
        delay,
        rise,
        fall,
        width,
        period,
        cycles: Some(3),
    };
    let stop = 1.5e-3;
    let tau = 1e-3;
    // The trapezoid is a sum of four ramps per pulse.
    let exact = |t: f64| {
        (0..3)
            .map(|k| {
                let start = delay + k as f64 * period;
                (ramp_response(t - start, tau) - ramp_response(t - start - rise, tau)) / rise
                    - (ramp_response(t - start - rise - width, tau)
                        - ramp_response(t - start - rise - width - fall, tau))
                        / fall
            })
            .sum::<f64>()
    };
    let corners = pulse.breakpoints(stop);
    assert_eq!(corners.len(), 12);
    for method in METHODS[1..].iter().copied() {
        let r = run(
            &rc(Source::with_waveform(pulse.clone())),
            spec(stop, 100e-6, InitialState::ZeroState),
            method,
        );
        assert_sampling_rules(&r, stop, 100e-6, 0.0);
        for &t in &corners {
            let k = at(&r, t);
            assert!(
                (samples(&r, "V(in)")[k] - pulse.value(t)).abs() < 1e-15,
                "source at corner {t}"
            );
        }
        let error = worst_error(&r, "V(out)", exact);
        assert!(error < VOLTS, "{method:?}: error {error:e}");
    }
}

#[test]
fn ideal_edges_switch_between_samples() {
    // Zero rise and fall: the edge sample still shows the old level (left
    // limit) and the response is a sum of exact step responses.
    let pulse = Waveform::Pulse {
        initial: 0.0,
        pulsed: 1.0,
        delay: 0.2e-3,
        rise: 0.0,
        fall: 0.0,
        width: 0.5e-3,
        period: 1e-3,
        cycles: None,
    };
    let tau: f64 = 1e-3;
    let step = |t: f64, edge: f64| {
        if t > edge {
            -(-(t - edge) / tau).exp_m1()
        } else {
            0.0
        }
    };
    let exact = |t: f64| {
        (0..3)
            .map(|k| {
                let on = 0.2e-3 + k as f64 * 1e-3;
                step(t, on) - step(t, on + 0.5e-3)
            })
            .sum::<f64>()
    };
    // The exact edge times, as the solver lands on them.
    let edges = pulse.breakpoints(2.5e-3);
    assert_eq!(edges.len(), 5);
    for method in METHODS[1..].iter().copied() {
        let r = run(
            &rc(Source::with_waveform(pulse.clone())),
            spec(2.5e-3, 0.1e-3, InitialState::ZeroState),
            method,
        );
        let input = samples(&r, "V(in)");
        for (n, &edge) in edges.iter().enumerate() {
            let k = at(&r, edge);
            let (before, after) = if n % 2 == 0 { (0.0, 1.0) } else { (1.0, 0.0) };
            assert!(
                (input[k] - before).abs() < 1e-15,
                "{method:?}: level at the edge {edge}"
            );
            assert!(
                (input[k + 1] - after).abs() < 1e-15,
                "{method:?}: level after the edge {edge}"
            );
        }
        let error = worst_error(&r, "V(out)", exact);
        assert!(error < VOLTS, "{method:?}: error {error:e}");
    }
}

#[test]
fn pwl_and_exp_sources_drive_the_circuit_exactly() {
    let pwl = Waveform::Pwl {
        points: vec![(0.0, 0.0), (1e-3, 2.0), (2e-3, 2.0), (2.5e-3, -1.0)],
        repeat: false,
    };
    let exp = Waveform::Exp {
        initial: 0.0,
        pulsed: 1.0,
        rise_delay: 0.1e-3,
        rise_tau: 0.3e-3,
        fall_delay: 1e-3,
        fall_tau: 0.2e-3,
    };
    for waveform in [pwl, exp] {
        let r = run(
            &rc(Source::with_waveform(waveform.clone())),
            spec(3e-3, 0.1e-3, InitialState::ZeroState),
            IntegrationMethod::Trapezoidal,
        );
        for (k, &t) in r.axis.values.iter().enumerate().skip(1) {
            assert!(
                (samples(&r, "V(in)")[k] - waveform.value(t)).abs() < 1e-15,
                "{waveform:?} at {t}"
            );
        }
        for t in waveform.breakpoints(3e-3) {
            at(&r, t);
        }
    }
}

#[test]
fn operating_point_start_differs_from_starting_at_rest() {
    let c = rc(Source::dc(1.0));
    let settled = run(
        &c,
        spec(2e-3, 1e-3, InitialState::OperatingPoint),
        IntegrationMethod::Trapezoidal,
    );
    assert!(worst_error(&settled, "V(out)", |_| 1.0) < 1e-12);
    assert!(worst_error(&settled, "I(C1)", |_| 0.0) < 1e-15);
    let rest = run(
        &c,
        spec(2e-3, 1e-3, InitialState::ZeroState),
        IntegrationMethod::Trapezoidal,
    );
    assert!(samples(&rest, "V(out)")[0].abs() < 1e-12);
    assert!((samples(&rest, "V(out)")[at(&rest, 2e-3)] - (1.0 - (-2.0_f64).exp())).abs() < VOLTS);
}

#[test]
fn operating_point_start_uses_the_waveform_value_at_zero() {
    // The DC value differs from the waveform at t = 0; the transient's
    // operating point follows the waveform.
    let mut source = Source::with_waveform(Waveform::Pwl {
        points: vec![(0.0, 3.0), (1.0, 3.0)],
        repeat: false,
    });
    source.dc = 7.0;
    let r = run(
        &rc(source),
        spec(1e-3, 1e-3, InitialState::OperatingPoint),
        IntegrationMethod::Trapezoidal,
    );
    assert!(worst_error(&r, "V(out)", |_| 3.0) < 1e-12);
}

#[test]
fn a_precharged_capacitor_discharges() {
    let mut c = Circuit::new();
    let n = c.node("out");
    c.add(Element::new("R1", [n, GROUND], Device::resistor(1e3)));
    c.add(Element::new(
        "C1",
        [n, GROUND],
        Device::Capacitor {
            capacitance: 1e-6,
            initial_voltage: Some(2.0),
        },
    ));
    for method in METHODS[1..].iter().copied() {
        let r = run(&c, spec(3e-3, 1e-3, InitialState::ZeroState), method);
        // The t = 0 sample shows the initial voltage and its discharge
        // current (T6).
        assert!((samples(&r, "V(out)")[0] - 2.0).abs() < 1e-12);
        assert!((samples(&r, "I(C1)")[0] + 2e-3).abs() < 1e-15);
        let error = worst_error(&r, "V(out)", |t| 2.0 * (-t / 1e-3_f64).exp());
        assert!(error < 2.0 * VOLTS, "{method:?}: error {error:e}");
    }
}

#[test]
fn an_inductor_with_initial_current_decays_through_a_resistor() {
    let mut c = Circuit::new();
    let n = c.node("n");
    c.add(Element::new(
        "L1",
        [n, GROUND],
        Device::Inductor {
            inductance: 1e-3,
            initial_current: Some(1e-3),
        },
    ));
    c.add(Element::new("R1", [n, GROUND], Device::resistor(1.0)));
    for method in METHODS[1..].iter().copied() {
        let r = run(&c, spec(3e-3, 1e-3, InitialState::ZeroState), method);
        // The initial-current source's gmin shunt moves it by 1e-15 A.
        assert!((samples(&r, "I(L1)")[0] - 1e-3).abs() < 1e-14);
        assert!((samples(&r, "V(n)")[0] + 1e-3).abs() < 1e-14);
        let error = worst_error(&r, "I(L1)", |t| 1e-3 * (-t / 1e-3_f64).exp());
        assert!(error < 1e-9, "{method:?}: error {error:e}");
    }
}

/// Every step respecting the maximum step is checked on the unthinned
/// solver output in the analysis' unit tests; the result keeps fewer points
/// (D18).
#[test]
fn saving_starts_at_start_save_and_steps_respect_max_step() {
    let transient = Transient {
        stop: 5e-3,
        step: 0.5e-3,
        start_save: 1.25e-3,
        max_step: Some(20e-6),
        initial_state: InitialState::ZeroState,
    };
    let r = run(
        &rc(Source::dc(1.0)),
        transient,
        IntegrationMethod::Trapezoidal,
    );
    assert_sampling_rules(&r, 5e-3, 0.5e-3, 1.25e-3);
    assert!(worst_error(&r, "V(out)", |t| 1.0 - (-t / 1e-3_f64).exp()) < VOLTS);
}

#[test]
fn a_settled_circuit_keeps_only_its_grid_and_end_points() {
    // The default maximum step (a fiftieth of the saved interval, checked on
    // the solver output in the unit tests) still steps 50 times, but nothing
    // changes, so the result keeps only t = 0 and the stop time (D18).
    let r = run(
        &rc(Source::dc(1.0)),
        spec(50.0, 50.0, InitialState::OperatingPoint),
        IntegrationMethod::Trapezoidal,
    );
    assert_eq!(r.axis.values, [0.0, 50.0]);
    assert!(worst_error(&r, "V(out)", |_| 1.0) < 1e-12);
}

#[test]
fn invalid_transients_are_rejected() {
    let c = rc(Source::dc(1.0));
    let base = spec(1e-3, 1e-4, InitialState::ZeroState);
    let cases = [
        Transient {
            stop: 0.0,
            ..base.clone()
        },
        Transient {
            stop: f64::NAN,
            ..base.clone()
        },
        Transient {
            step: 0.0,
            ..base.clone()
        },
        Transient {
            step: -1e-4,
            ..base.clone()
        },
        Transient {
            start_save: 1e-3,
            ..base.clone()
        },
        Transient {
            start_save: -1e-4,
            ..base.clone()
        },
        Transient {
            max_step: Some(0.0),
            ..base.clone()
        },
        Transient {
            max_step: Some(f64::INFINITY),
            ..base.clone()
        },
        Transient {
            stop: 1.0,
            step: 1e-16,
            ..base.clone()
        },
    ];
    for transient in cases {
        let error = simulate(
            &c,
            &Analysis::Transient(transient.clone()),
            &SimOptions::default(),
            &SimControl::new(),
        );
        assert!(
            matches!(error, Err(SimError::InvalidCircuit(_))),
            "{transient:?}: {error:?}"
        );
    }
    let absurd = Waveform::Pulse {
        initial: 0.0,
        pulsed: 1.0,
        delay: 0.0,
        rise: 0.0,
        fall: 0.0,
        width: 0.5e-12,
        period: 1e-12,
        cycles: None,
    };
    let error = simulate(
        &rc(Source::with_waveform(absurd)),
        &Analysis::Transient(spec(1.0, 0.1, InitialState::ZeroState)),
        &SimOptions::default(),
        &SimControl::new(),
    )
    .unwrap_err();
    assert!(
        matches!(&error, SimError::InvalidCircuit(e) if e.message.contains("corners")),
        "{error}"
    );
}

#[test]
fn a_node_floating_even_in_transient_is_named() {
    let mut c = rc(Source::dc(1.0));
    let n = c.node("island");
    c.add(Element::new(
        "I9",
        [GROUND, n],
        Device::CurrentSource(Source::dc(1e-3)),
    ));
    let error = simulate(
        &c,
        &Analysis::Transient(spec(1e-3, 1e-4, InitialState::ZeroState)),
        &SimOptions::default(),
        &SimControl::new(),
    )
    .unwrap_err();
    assert!(
        matches!(&error, SimError::SingularMatrix { unknown, .. } if unknown == "V(island)"),
        "{error}"
    );
}

#[test]
fn a_minimum_step_that_cannot_be_met_is_reported_with_progress() {
    // The minimum step is a third of the run, far above what the RC needs.
    let options = SimOptions {
        min_step_ratio: 0.3,
        ..SimOptions::default()
    };
    let transient = Transient {
        max_step: Some(1e-3),
        ..spec(1e-3, 1e-3, InitialState::ZeroState)
    };
    let error = simulate(
        &rc(Source::dc(1.0)),
        &Analysis::Transient(transient),
        &options,
        &SimControl::new(),
    )
    .unwrap_err();
    match error {
        SimError::TimestepTooSmall { time, progress } => {
            assert!((0.0..1.0).contains(&progress));
            assert_eq!(progress, time / 1e-3);
        }
        other => panic!("unexpected {other}"),
    }
}
