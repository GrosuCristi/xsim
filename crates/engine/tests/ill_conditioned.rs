//! Well-posed circuits whose matrices span many decades (SR-1): a node held
//! only by junction gmin next to a milliohm resistor. The pivot test works
//! on the row- and column-equilibrated matrix, so these solve; only a matrix
//! singular to double precision is refused, and then the error says so
//! instead of pointing at floating nodes.
//!
//! A matrix that is singular to double precision while every source is zero
//! (so is every node voltage) has its operating point accepted by
//! inspection, as LTspice does ("OP point found by inspection").
//!
//! LTspice 24.1.9 values come from `tests/reference/solver_doubler*.cir` and
//! `solver_clamp*.cir`. The transient references were run with a 0.2 µs
//! (doubler) or 0.1 µs (clamp) maximum step, where LTspice has converged; at
//! its default step LTspice is 5e-5 (doubler) to 1.5e-4 (clamp) off.

use xsim_spice_engine::*;

fn sine(amplitude: f64) -> Source {
    let mut s = Source::with_waveform(Waveform::Sine {
        offset: 0.0,
        amplitude,
        frequency: 1e3,
        delay: 0.0,
        damping: 0.0,
        phase: 0.0,
        cycles: None,
    });
    s.ac_magnitude = 1.0;
    s
}

/// Greinacher voltage doubler with the capacitors' ESR as explicit
/// resistors; node m is held at DC only by junction gmin.
fn doubler(esr: f64) -> Circuit {
    let mut c = Circuit::new();
    let (s, a, m, out, k) = (
        c.node("s"),
        c.node("a"),
        c.node("m"),
        c.node("out"),
        c.node("k"),
    );
    let diode = || Device::diode(DiodeModel::default());
    c.add(Element::new(
        "V1",
        [s, GROUND],
        Device::VoltageSource(sine(10.0)),
    ));
    c.add(Element::new("C1", [s, a], Device::capacitor(10e-6)));
    c.add(Element::new("RESR1", [a, m], Device::resistor(esr)));
    c.add(Element::new("D1", [GROUND, m], diode()));
    c.add(Element::new("D2", [m, out], diode()));
    c.add(Element::new("RESR2", [out, k], Device::resistor(esr)));
    c.add(Element::new("C2", [k, GROUND], Device::capacitor(10e-6)));
    c.add(Element::new("RL", [out, GROUND], Device::resistor(10e3)));
    c
}

/// AC-coupled clamp: `V1 → 1 µF → R1 → b`, antiparallel diodes from b to
/// ground, no load.
fn clamp(r1: f64) -> Circuit {
    let mut c = Circuit::new();
    let (s, a, b) = (c.node("s"), c.node("a"), c.node("b"));
    let diode = || Device::diode(DiodeModel::default());
    c.add(Element::new(
        "V1",
        [s, GROUND],
        Device::VoltageSource(sine(1.0)),
    ));
    c.add(Element::new("C1", [s, a], Device::capacitor(1e-6)));
    c.add(Element::new("R1", [a, b], Device::resistor(r1)));
    c.add(Element::new("D1", [b, GROUND], diode()));
    c.add(Element::new("D2", [GROUND, b], diode()));
    c
}

fn run(circuit: &Circuit, analysis: &Analysis) -> Result<SimulationResult, SimError> {
    run_with_warnings(circuit, analysis).0
}

fn run_with_warnings(
    circuit: &Circuit,
    analysis: &Analysis,
) -> (Result<SimulationResult, SimError>, Vec<String>) {
    let control = SimControl::new();
    let result = simulate(circuit, analysis, &SimOptions::default(), &control);
    (result, control.warnings())
}

fn transient(stop: f64) -> Analysis {
    Analysis::Transient(Transient {
        stop,
        step: 1e-5,
        start_save: 0.0,
        max_step: None,
        initial_state: InitialState::OperatingPoint,
    })
}

/// Linear interpolation of a real signal at time `t`.
fn at(result: &SimulationResult, name: &str, t: f64) -> f64 {
    let values = result.signal(name).unwrap().real().unwrap();
    let times = &result.axis.values;
    let k = times.iter().position(|&s| s >= t).unwrap();
    if times[k] == t || k == 0 {
        return values[k];
    }
    let f = (t - times[k - 1]) / (times[k] - times[k - 1]);
    values[k - 1] + f * (values[k] - values[k - 1])
}

fn close(actual: f64, expected: f64, tolerance: f64, what: &str) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{what}: expected {expected}, got {actual} (tolerance {tolerance})"
    );
}

#[test]
fn milliohm_resistors_next_to_gmin_held_nodes_have_an_operating_point() {
    for r in [10e-3, 1e-3] {
        for solver in [SolverKind::Dense, SolverKind::Sparse] {
            let options = SimOptions {
                solver,
                ..SimOptions::default()
            };
            let control = SimControl::new();
            let op = |c: &Circuit| {
                simulate(c, &Analysis::OperatingPoint, &options, &control)
                    .unwrap_or_else(|e| panic!("{r} Ω, {solver:?}: {e}"))
            };
            // LTspice: "OP point found by inspection", every node at 0 V.
            let d = op(&doubler(r));
            for node in ["V(s)", "V(a)", "V(m)", "V(out)", "V(k)"] {
                close(d.signal(node).unwrap().real().unwrap()[0], 0.0, 1e-15, node);
            }
            let c = op(&clamp(r));
            close(
                c.signal("V(b)").unwrap().real().unwrap()[0],
                0.0,
                1e-15,
                "V(b)",
            );
        }
    }
}

#[test]
fn a_milliohm_clamp_has_an_ac_response() {
    // LTspice .ac list 1 1k 1Meg: |V(b)| = 1 to within 1e-12 dB at 10 mΩ and
    // 1 mΩ, at a phase of 2.53e-5° (10 mΩ, 1 Hz).
    let analysis = Analysis::Ac(AcSweep {
        scale: SweepScale::Decade,
        points: 1,
        start: 1.0,
        stop: 1e6,
    });
    for r in [10e-3, 1e-3] {
        let result = run(&clamp(r), &analysis).unwrap_or_else(|e| panic!("{r} Ω: {e}"));
        let SignalData::Complex { re, im } = &result.signal("V(b)").unwrap().data else {
            panic!("AC data is complex");
        };
        for k in 0..re.len() {
            close(re[k].hypot(im[k]), 1.0, 1e-12, "|V(b)|");
        }
        let phase = im[0].atan2(re[0]).to_degrees();
        let ltspice = if r == 10e-3 {
            2.526_955_388_650_14e-5
        } else {
            2.488_079_151_901_68e-5
        };
        close(phase, ltspice, 1e-6, "phase at 1 Hz");
    }
}

#[test]
fn a_doubler_with_milliohm_esr_runs_a_transient_from_its_operating_point() {
    let (r, warnings) = run_with_warnings(&doubler(10e-3), &transient(5e-3));
    let r = r.unwrap();
    // LTspice gives no warning; neither may xsim (the node's solution is 0).
    assert!(warnings.is_empty(), "{warnings:?}");
    // LTspice (0.2 µs maximum step): 0 at t = 0, 6.54221932837188 at 1 ms,
    // 17.4335925171187 at 5 ms. Measured agreement: 5e-7 and 1.3e-6 V.
    close(at(&r, "V(out)", 0.0), 0.0, 1e-15, "V(out) at 0");
    close(
        at(&r, "V(out)", 1e-3),
        6.542_219_328_371_88,
        2e-6,
        "V(out) at 1 ms",
    );
    close(
        at(&r, "V(out)", 5e-3),
        17.433_592_517_118_7,
        5e-6,
        "V(out) at 5 ms",
    );
}

#[test]
fn a_clamp_with_milliohm_resistance_runs_a_transient_from_its_operating_point() {
    let (r, warnings) = run_with_warnings(&clamp(10e-3), &transient(2e-3));
    let r = r.unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    // LTspice (0.1 µs maximum step): 0 at t = 0, −0.36845569057469 at
    // 0.5 ms, 0.368455673460924 at 2 ms. Measured agreement: 2.5e-8 V.
    close(at(&r, "V(b)", 0.0), 0.0, 1e-15, "V(b) at 0");
    close(
        at(&r, "V(b)", 0.5e-3),
        -0.368_455_690_574_69,
        1e-7,
        "V(b) at 0.5 ms",
    );
    close(
        at(&r, "V(b)", 2e-3),
        0.368_455_673_460_924,
        1e-7,
        "V(b) at 2 ms",
    );
}

/// [`clamp`] with only `D1` (b to ground): node b is held at DC by a single
/// junction, 1.4 pS.
fn half_wave_clamp(r1: f64) -> Circuit {
    let mut c = clamp(r1);
    c.elements.retain(|e| e.name != "D2");
    c
}

#[test]
fn an_unexcited_circuit_that_is_singular_to_double_precision_has_its_operating_point_by_inspection()
{
    // Every source is 0 at the operating point, so every node is at 0 V; LTspice
    // reports ".OP point found by inspection" without factoring the matrix. In
    // double precision these DC matrices are singular (1 µΩ: G + g rounds to
    // G) or have a pivot of only a few rounding units (1 mΩ next to one
    // junction: g/G = 1.4e-15).
    let circuits = [
        ("one diode, 1 mΩ", half_wave_clamp(1e-3)),
        ("one diode, 1 µΩ", half_wave_clamp(1e-6)),
        ("two diodes, 1 µΩ", clamp(1e-6)),
        ("doubler, 1 µΩ", doubler(1e-6)),
    ];
    for (what, circuit) in &circuits {
        for solver in [SolverKind::Dense, SolverKind::Sparse] {
            let options = SimOptions {
                solver,
                ..SimOptions::default()
            };
            let control = SimControl::new();
            let r = simulate(circuit, &Analysis::OperatingPoint, &options, &control)
                .unwrap_or_else(|e| panic!("{what}, {solver:?}: {e}"));
            for signal in &r.signals {
                assert_eq!(signal.real().unwrap(), [0.0], "{what}: {}", signal.name);
            }
            assert!(control.warnings().is_empty(), "{:?}", control.warnings());
        }
    }
}

#[test]
fn clamps_behind_milliohm_and_microohm_resistances_run_ac_and_transient_analyses() {
    // LTspice: tests/reference/solver_clamp_inspection_ac.cir (.ac list 1 1k
    // 1Meg) and solver_clamp_inspection.cir (0.1 µs maximum step).
    let ac = Analysis::Ac(AcSweep {
        scale: SweepScale::Decade,
        points: 1,
        start: 1.0,
        stop: 1e6,
    });
    let r = run(&half_wave_clamp(1e-3), &ac).unwrap();
    let SignalData::Complex { re, im } = &r.signal("V(b)").unwrap().data else {
        panic!("AC data is complex");
    };
    for k in 0..re.len() {
        close(re[k].hypot(im[k]), 1.0, 1e-12, "|V(b)|");
    }
    // The phase at 1 Hz is g/(ωC) with the junction's g = Is/Vt + gmin =
    // 1.39 pS, but the matrix holds G + g with G = 1000 S, which resolves g
    // only to ulp(G)/2 ≈ 4 % of itself. LTspice: 1.2440395759509e-5°, 1.6 %
    // below the exact 1.2645e-5°; xsim lands 4.5 % below.
    let vt = 1.380_649e-23 * 300.15 / 1.602_176_634e-19;
    let exact = ((1e-14 / vt + 1e-12) / (std::f64::consts::TAU * 1e-6)).to_degrees();
    let phase = im[0].atan2(re[0]).to_degrees();
    close(phase, exact, 0.06 * exact, "phase of V(b) at 1 Hz");
    close(
        phase,
        1.244_039_575_950_9e-5,
        0.06 * exact,
        "phase vs LTspice",
    );
    for circuit in [half_wave_clamp(1e-6), clamp(1e-6)] {
        let r = run(&circuit, &ac).unwrap();
        let SignalData::Complex { re, im } = &r.signal("V(b)").unwrap().data else {
            panic!("AC data is complex");
        };
        // LTspice: 0 dB at 0°. The junction's 1.4 pS is below half an ulp
        // of G = 1e6 S, so neither simulator can resolve the exact phase
        // (1.26e-5° at 1 Hz, g/(ωC)).
        for k in 0..re.len() {
            close(re[k].hypot(im[k]), 1.0, 1e-12, "|V(b)|");
            close(im[k].atan2(re[k]).to_degrees(), 0.0, 2e-5, "phase of V(b)");
        }
    }
    // LTspice also runs the 1 µΩ clamps' transients (nodes c and e of the
    // deck); xsim's operating point and AC handle them, but its transient
    // truncation-error control stops near 20 µs on the charge noise there
    // (outside the operating point), so only the 1 mΩ transient is checked.
    let (r, warnings) = run_with_warnings(&half_wave_clamp(1e-3), &transient(2e-3));
    let r = r.unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    close(at(&r, "V(b)", 0.0), 0.0, 1e-15, "V(b) at 0");
    // Measured agreement: 1.2e-9 and 1.6e-8 V.
    close(
        at(&r, "V(b)", 0.5e-3),
        -0.368_463_908_884_766,
        1e-7,
        "V(b) at 0.5 ms",
    );
    close(
        at(&r, "V(b)", 2e-3),
        -0.386_392_446_667_413,
        1e-7,
        "V(b) at 2 ms",
    );
}

#[test]
fn a_doubler_with_one_milliohm_esr_runs_without_precision_warnings() {
    // A branch current near a zero crossing carries absolute rounding noise
    // of about ε·G·|v| (1 mΩ at 10 V: picoamperes), above abstol but no loss
    // of meaningful precision. LTspice (solver_doubler_1m.cir, 0.2 µs
    // maximum step): 6.54241612872899 at 1 ms, 17.434083334505 at 5 ms.
    for initial_state in [InitialState::OperatingPoint, InitialState::ZeroState] {
        let analysis = Analysis::Transient(Transient {
            stop: 5e-3,
            step: 1e-5,
            start_save: 0.0,
            max_step: None,
            initial_state,
        });
        let (r, warnings) = run_with_warnings(&doubler(1e-3), &analysis);
        let r = r.unwrap();
        assert!(warnings.is_empty(), "{initial_state:?}: {warnings:?}");
        close(
            at(&r, "V(out)", 1e-3),
            6.542_416_128_728_99,
            2e-6,
            "V(out) at 1 ms",
        );
        close(
            at(&r, "V(out)", 5e-3),
            17.434_083_334_505,
            5e-6,
            "V(out) at 5 ms",
        );
    }
}

#[test]
fn a_current_resolved_only_to_its_rounding_floor_at_the_operating_point_is_warned_about() {
    // 1 V → 1 µΩ → 1 MΩ: the node voltages are exact to rounding, but I(V1)
    // = G·(V(a) − V(b)) with V(a) − V(b) = 1e-12 V is known only to about
    // 1e-3 of itself (xsim lands 7.6e-6 off).
    let mut c = Circuit::new();
    let (a, b) = (c.node("a"), c.node("b"));
    c.add(Element::new(
        "V1",
        [a, GROUND],
        Device::VoltageSource(Source::dc(1.0)),
    ));
    c.add(Element::new("R1", [a, b], Device::resistor(1e-6)));
    c.add(Element::new("R2", [b, GROUND], Device::resistor(1e6)));
    let (r, warnings) = run_with_warnings(&c, &Analysis::OperatingPoint);
    let current = r.unwrap().signal("I(V1)").unwrap().real().unwrap()[0];
    let exact = -1.0 / (1e6 + 1e-6);
    close(current, exact, 1e-4 * exact.abs(), "I(V1)");
    assert!(
        warnings
            .iter()
            .any(|w| w.starts_with("I(V1) could be resolved only to about ±")),
        "{warnings:?}"
    );
}

#[test]
fn a_matrix_singular_to_double_precision_is_called_that() {
    // 1 µΩ next to a node held by 2.8 pS: G + g rounds to G. With a 0.5 V
    // offset the operating point is not all zeros, so it cannot be found by
    // inspection; LTspice too reports "Singular matrix: Check node a" and
    // fails (V1 s 0 SINE(0.5 1 1k), otherwise as solver_clamp.cir with R1 =
    // 1u).
    let mut circuit = clamp(1e-6);
    let Device::VoltageSource(source) = &mut circuit.elements[0].device else {
        panic!("V1 is a voltage source");
    };
    source.waveform = Some(Waveform::Sine {
        offset: 0.5,
        amplitude: 1.0,
        frequency: 1e3,
        delay: 0.0,
        damping: 0.0,
        phase: 0.0,
        cycles: None,
    });
    let error = run(&circuit, &transient(2e-3)).unwrap_err();
    let SimError::SingularMatrix {
        unknown, reason, ..
    } = &error
    else {
        panic!("expected a singular matrix, got {error}");
    };
    assert!(unknown == "V(a)" || unknown == "V(b)", "{error}");
    assert!(
        reason.contains("singular only to double precision"),
        "{error}"
    );
    assert!(!error.to_string().contains("floating"), "{error}");
}

#[test]
fn a_linear_ill_conditioned_circuit_solves_with_a_precision_warning() {
    // 1 nA → 1 mΩ → 100 GΩ: the exact V(b) is 100 V; double precision
    // resolves it only to about ε·G/g ≈ 2 % (xsim lands 4.4e-4 below, as
    // LTspice does). This used to be "Singular matrix at V(b) (check for
    // floating nodes ...)"; now it solves and warns.
    let mut c = Circuit::new();
    let (a, b) = (c.node("a"), c.node("b"));
    c.add(Element::new(
        "I1",
        [GROUND, a],
        Device::CurrentSource(Source::dc(1e-9)),
    ));
    c.add(Element::new("R1", [a, b], Device::resistor(1e-3)));
    c.add(Element::new("R2", [b, GROUND], Device::resistor(100e9)));
    let control = SimControl::new();
    let r = simulate(
        &c,
        &Analysis::OperatingPoint,
        &SimOptions::default(),
        &control,
    )
    .unwrap();
    close(
        r.signal("V(b)").unwrap().real().unwrap()[0],
        100.0,
        0.1,
        "V(b)",
    );
    let warnings = control.warnings();
    assert!(
        warnings
            .iter()
            .any(|w| w.starts_with("V(a) could be resolved only to about ±")
                || w.starts_with("V(b) could be resolved only to about ±")),
        "{warnings:?}"
    );
}
