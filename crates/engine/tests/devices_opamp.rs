//! Full-simulation tests of the op-amp (D26): closed-loop gains against the
//! exact finite-gain formulas (DC and AC), the single-pole response, slew
//! rate, rail clipping with and without headroom, input and output
//! resistance, supply currents and KCL, integrator and comparator
//! transients with every integration method, and an LTspice cross-check of
//! the macro-model built from behavioural sources (twice, independently).

use num_complex::Complex64;
use std::f64::consts::PI;
use xsim_spice_engine::library;
use xsim_spice_engine::*;

const METHODS: [IntegrationMethod; 4] = [
    IntegrationMethod::BackwardEuler,
    IntegrationMethod::Trapezoidal,
    IntegrationMethod::ModifiedTrapezoidal,
    IntegrationMethod::Gear2,
];

/// Default options with `method`. Backward Euler is first order, so at the
/// default reltol of 10⁻⁶ it needs steps a million times shorter than the
/// time scale of the waveform; it runs at reltol = 10⁻³ (as SPICE's default).
fn method_options(method: IntegrationMethod) -> SimOptions {
    SimOptions {
        method,
        reltol: if method == IntegrationMethod::BackwardEuler {
            1e-3
        } else {
            SimOptions::default().reltol
        },
        ..SimOptions::default()
    }
}

fn precise() -> SimOptions {
    SimOptions {
        reltol: 1e-9,
        abstol: 1e-15,
        vntol: 1e-12,
        ..SimOptions::default()
    }
}

fn run(circuit: &Circuit, analysis: &Analysis, options: &SimOptions) -> SimulationResult {
    simulate(circuit, analysis, options, &SimControl::new()).expect("simulation failed")
}

fn real<'a>(result: &'a SimulationResult, name: &str) -> &'a [f64] {
    result
        .signal(name)
        .and_then(Signal::real)
        .unwrap_or_else(|| panic!("no real signal {name}"))
}

fn op_value(circuit: &Circuit, name: &str) -> f64 {
    real(&run(circuit, &Analysis::OperatingPoint, &precise()), name)[0]
}

fn phasor(result: &SimulationResult, name: &str) -> Complex64 {
    match result.signal(name).map(|s| &s.data) {
        Some(SignalData::Complex { re, im }) => Complex64::new(re[0], im[0]),
        _ => panic!("no complex signal {name}"),
    }
}

fn ac_at(circuit: &Circuit, f: f64, name: &str) -> Complex64 {
    let spec = Analysis::Ac(AcSweep {
        scale: SweepScale::Linear,
        points: 1,
        start: f,
        stop: f,
    });
    phasor(&run(circuit, &spec, &precise()), name)
}

fn assert_close(actual: f64, expected: f64, rel: f64, abs: f64, what: &str) {
    assert!(
        (actual - expected).abs() <= rel * expected.abs() + abs,
        "{what}: got {actual:.15e}, expected {expected:.15e}"
    );
}

/// Linear interpolation of `y(t)` at `at`.
fn at_time(t: &[f64], y: &[f64], at: f64) -> f64 {
    let k = t.partition_point(|&s| s < at).clamp(1, t.len() - 1);
    let w = (at - t[k - 1]) / (t[k] - t[k - 1]);
    y[k - 1] + w * (y[k] - y[k - 1])
}

/// First time after `after` where `y` crosses `level`, interpolated.
fn crossing(t: &[f64], y: &[f64], level: f64, after: f64) -> f64 {
    for k in 1..t.len() {
        if t[k] > after && (y[k - 1] - level) * (y[k] - level) <= 0.0 && y[k] != y[k - 1] {
            return t[k - 1] + (level - y[k - 1]) / (y[k] - y[k - 1]) * (t[k] - t[k - 1]);
        }
    }
    panic!("no crossing of {level}");
}

fn gp() -> OpAmpModel {
    library::opamp("OA_GP").expect("OA_GP")
}

fn ideal() -> OpAmpModel {
    library::opamp("OA_IDEAL").expect("OA_IDEAL")
}

fn rrio() -> OpAmpModel {
    library::opamp("OA_RRIO").expect("OA_RRIO")
}

/// Supplies `VP` (node `vp`) and `VN` (node `vn`).
fn supplies(c: &mut Circuit, positive: f64, negative: f64) -> (NodeIndex, NodeIndex) {
    let vp = c.node("vp");
    let vn = c.node("vn");
    c.add(Element::new(
        "VP",
        [vp, GROUND],
        Device::VoltageSource(Source::dc(positive)),
    ));
    c.add(Element::new(
        "VN",
        [vn, GROUND],
        Device::VoltageSource(Source::dc(negative)),
    ));
    (vp, vn)
}

fn opamp(c: &mut Circuit, nodes: [NodeIndex; 5], model: OpAmpModel) {
    c.add(Element::new("U1", nodes, Device::OpAmp { model }));
}

/// A gain stage on ±15 V: non-inverting (`vin` at in+, R1 from in− to
/// ground) or inverting (in+ grounded, R1 from `vin` to in−); R2 from in−
/// to the output.
fn stage(model: OpAmpModel, source: Source, r1: f64, r2: f64, inverting: bool) -> Circuit {
    let mut c = Circuit::new();
    let (vp, vn) = supplies(&mut c, 15.0, -15.0);
    let input = c.node("in");
    let n = c.node("n");
    let out = c.node("out");
    c.add(Element::new(
        "VIN",
        [input, GROUND],
        Device::VoltageSource(source),
    ));
    if inverting {
        c.add(Element::new("R1", [input, n], Device::resistor(r1)));
        opamp(&mut c, [GROUND, n, out, vp, vn], model);
    } else {
        c.add(Element::new("R1", [n, GROUND], Device::resistor(r1)));
        opamp(&mut c, [input, n, out, vp, vn], model);
    }
    c.add(Element::new("R2", [n, out], Device::resistor(r2)));
    c
}

/// Open-loop gain of `model` at `f` (Hz): `aol/(1 + j·f·aol/gbw)`, or
/// `aol` for the ideal op-amp.
fn open_loop(model: &OpAmpModel, f: f64) -> Complex64 {
    match model.kind {
        OpAmpKind::Ideal => Complex64::new(model.aol, 0.0),
        OpAmpKind::Macro => {
            Complex64::new(model.aol, 0.0) / Complex64::new(1.0, f * model.aol / model.gbw)
        }
    }
}

/// Exact output of [`stage`] for a unit input with open-loop gain `a`,
/// including `rin` (infinite for the ideal op-amp) and `rout`: the two
/// linear equations `V(out)·(1 + rout/R2) + V(n)·(a − rout/R2) = a·V(in+)`
/// and the KCL of node `n`, solved by Cramer's rule.
fn closed_loop(model: &OpAmpModel, a: Complex64, r1: f64, r2: f64, inverting: bool) -> Complex64 {
    let gin = match model.kind {
        OpAmpKind::Ideal => 0.0,
        OpAmpKind::Macro => 1.0 / model.rin,
    };
    let rout = model.rout;
    let a11 = Complex64::new(1.0 + rout / r2, 0.0);
    let a12 = a - rout / r2;
    let a21 = Complex64::new(1.0 / r2, 0.0);
    let a22 = Complex64::new(-(gin + 1.0 / r1 + 1.0 / r2), 0.0);
    let (b1, b2) = if inverting {
        (Complex64::new(0.0, 0.0), Complex64::new(-1.0 / r1, 0.0))
    } else {
        (a, Complex64::new(-gin, 0.0))
    };
    (b1 * a22 - a12 * b2) / (a11 * a22 - a12 * a21)
}

#[test]
fn closed_loop_dc_gain_matches_the_finite_gain_formula() {
    for model in [ideal(), gp(), rrio()] {
        for (r1, r2, inverting) in [
            (1e3, 10e3, true),
            (1e3, 9e3, false),
            (10e3, 10e3, true),
            (1e3, 99e3, false),
        ] {
            let vin = 0.1;
            let circuit = stage(model.clone(), Source::dc(vin), r1, r2, inverting);
            let expected = vin * closed_loop(&model, open_loop(&model, 0.0), r1, r2, inverting).re;
            let what = format!("{} R1={r1} R2={r2} inverting={inverting}", model.name);
            assert_close(op_value(&circuit, "V(out)"), expected, 1e-9, 0.0, &what);
        }
    }
    // The finite gain shows: a gain-of-100 stage of OA_GP is 0.1 % low.
    let circuit = stage(gp(), Source::dc(0.01), 1e3, 99e3, false);
    let v = op_value(&circuit, "V(out)");
    assert!(v < 0.9995 && v > 0.998, "{v}");
}

#[test]
fn ac_response_follows_the_single_pole_formula() {
    let model = gp();
    let (r1, r2) = (1e3, 9e3);
    let circuit = stage(model.clone(), Source::ac(0.0, 1.0, 0.0), r1, r2, false);
    for f in [1.0, 10.0, 1e3, 1e4, 1e5, 3e5, 1e6, 1e7] {
        let expected = closed_loop(&model, open_loop(&model, f), r1, r2, false);
        let got = ac_at(&circuit, f, "V(out)");
        assert!(
            (got - expected).norm() <= 1e-9 * expected.norm(),
            "{f} Hz: got {got}, expected {expected}"
        );
    }
    // −3 dB at about GBW / closed-loop gain, with 45° of phase lag there.
    let dc = ac_at(&circuit, 1.0, "V(out)").norm();
    let corner = ac_at(&circuit, model.gbw / 10.0, "V(out)");
    // (rout loads the loop by 75 Ω/10 kΩ, which moves the corner by 0.75 %.)
    assert_close(corner.norm() / dc, 0.5f64.sqrt(), 1e-2, 0.0, "−3 dB");
    assert_close(corner.arg().to_degrees(), -45.0, 1e-2, 0.0, "phase");

    // Open loop: the pole at GBW/AOL with exactly −45° there.
    let mut c = Circuit::new();
    let (vp, vn) = supplies(&mut c, 15.0, -15.0);
    let input = c.node("in");
    let out = c.node("out");
    c.add(Element::new(
        "VIN",
        [input, GROUND],
        Device::VoltageSource(Source::ac(0.0, 1.0, 0.0)),
    ));
    opamp(&mut c, [input, GROUND, out, vp, vn], model.clone());
    let pole = model.gbw / model.aol;
    let a = ac_at(&c, pole, "V(out)");
    assert_close(a.arg().to_degrees(), -45.0, 1e-9, 0.0, "open-loop phase");
    assert_close(
        a.norm(),
        model.aol / 2f64.sqrt(),
        1e-9,
        0.0,
        "open-loop gain",
    );
    // Unity-gain crossing at GBW.
    let unity = ac_at(&c, model.gbw, "V(out)").norm();
    assert_close(unity, 1.0, 1e-9, 0.0, "gain at GBW");
}

/// A follower on ±15 V driven by `source`.
fn follower(model: OpAmpModel, source: Source, positive: f64, negative: f64) -> Circuit {
    let mut c = Circuit::new();
    let (vp, vn) = supplies(&mut c, positive, negative);
    let input = c.node("in");
    let out = c.node("out");
    c.add(Element::new(
        "VIN",
        [input, GROUND],
        Device::VoltageSource(source),
    ));
    opamp(&mut c, [input, out, out, vp, vn], model);
    c
}

fn step(low: f64, high: f64, delay: f64) -> Source {
    Source::with_waveform(Waveform::Pulse {
        initial: low,
        pulsed: high,
        delay,
        rise: 1e-9,
        fall: 1e-9,
        width: 1.0,
        period: 2.0,
        cycles: None,
    })
}

fn transient(stop: f64, step: f64) -> Analysis {
    Analysis::Transient(Transient {
        stop,
        step,
        start_save: 0.0,
        max_step: None,
        initial_state: InitialState::OperatingPoint,
    })
}

#[test]
fn large_steps_slew_at_the_slew_rate_with_every_method() {
    let model = gp();
    // A follower's output leaks through rin to its input, so the pin moves
    // at slew/(1 + rout/rin) while the internal state slews at `slew`.
    let expected = model.slew / (1.0 + model.rout / model.rin);
    for method in METHODS {
        let circuit = follower(model.clone(), step(0.0, 5.0, 1e-6), 15.0, -15.0);
        let options = method_options(method);
        let r = run(&circuit, &transient(20e-6, 0.1e-6), &options);
        let (t, v) = (&r.axis.values, real(&r, "V(out)"));
        let t1 = crossing(t, v, 1.0, 0.0);
        let t3 = crossing(t, v, 3.0, 0.0);
        let what = format!("{method:?} slope");
        assert_close(2.0 / (t3 - t1), expected, 1e-6, 0.0, &what);
        // The edge at 1 µs plus the slew time from 0 V.
        assert_close(t1, 1e-6 + 1.0 / expected, 1e-3, 0.0, "start of the ramp");
        // Settled at the input within the closed-loop error.
        let last = v[v.len() - 1];
        assert_close(last, 5.0, 1e-4, 0.0, &format!("{method:?} settles"));
    }
}

#[test]
fn small_steps_settle_with_the_closed_loop_time_constant() {
    // 1 mV is far below the slew onset (80 mV; tanh bends the response by
    // about (e/L)²/3 = 5·10⁻⁵ here): the follower settles as 1 − e^(−t/τc)
    // with τc = 1/(2π·GBW) (the loop gain is AOL/(1+AOL)).
    let model = gp();
    let circuit = follower(model.clone(), step(0.0, 0.001, 1e-6), 15.0, -15.0);
    let options = SimOptions {
        reltol: 1e-6,
        ..SimOptions::default()
    };
    let r = run(&circuit, &transient(3e-6, 1e-9), &options);
    let (t, v) = (&r.axis.values, real(&r, "V(out)"));
    let tau = model.aol / (2.0 * PI * model.gbw) / (1.0 + model.aol);
    let final_value = 0.001 * model.aol / (1.0 + model.aol);
    for dt in [0.1e-6, 0.3e-6, 1e-6] {
        let expected = final_value * (1.0 - (-dt / tau).exp());
        // The input rises in 1 ns, so the response lags the step by 0.5 ns.
        let at = 1e-6 + 0.5e-9 + dt;
        assert_close(at_time(t, v, at), expected, 1e-3, 0.0, "settling");
    }
}

/// Output of a clipped follower whose limit is `limit` and input `vin`:
/// the output drives rin back to the input through rout.
fn clipped(model: &OpAmpModel, limit: f64, vin: f64) -> f64 {
    let ratio = match model.kind {
        OpAmpKind::Ideal => 0.0,
        OpAmpKind::Macro => model.rout / model.rin,
    };
    (limit + ratio * vin) / (1.0 + ratio)
}

#[test]
fn outputs_clip_at_the_rails_with_and_without_headroom() {
    for (model, swing) in [(ideal(), 0.0), (gp(), 1.5), (rrio(), 0.05)] {
        for vin in [7.0, -7.0] {
            let circuit = follower(model.clone(), Source::dc(vin), 5.0, -5.0);
            let limit = vin.signum() * (5.0 - swing);
            let what = format!("{} at {vin} V", model.name);
            let expected = clipped(&model, limit, vin);
            assert_close(op_value(&circuit, "V(out)"), expected, 1e-9, 0.0, &what);
        }
    }
    // Asymmetric and single supplies: the limits follow the rails.
    let circuit = follower(gp(), Source::dc(20.0), 12.0, 0.0);
    let expected = clipped(&gp(), 10.5, 20.0);
    assert_close(
        op_value(&circuit, "V(out)"),
        expected,
        1e-9,
        0.0,
        "12 V supply",
    );
    let circuit = follower(rrio(), Source::dc(-1.0), 5.0, 0.0);
    let expected = clipped(&rrio(), 0.05, -1.0);
    assert_close(
        op_value(&circuit, "V(out)"),
        expected,
        1e-9,
        0.0,
        "ground rail",
    );
    // No room between the headrooms: the output sits mid-way, finite.
    let circuit = follower(gp(), Source::dc(1.0), 1.0, 0.0);
    assert_close(op_value(&circuit, "V(out)"), 0.5, 1e-3, 0.0, "no room");
}

#[test]
fn input_and_output_resistance_load_the_stage() {
    // Open loop at 1 V common mode with 10 µV of differential input: the
    // internal stage makes AOL·vd = 1 V, rout and a 75 Ω load halve it, and
    // vd/rin flows through the inputs.
    let model = gp();
    let mut c = Circuit::new();
    let (vp, vn) = supplies(&mut c, 15.0, -15.0);
    let p = c.node("p");
    let n = c.node("n");
    let out = c.node("out");
    c.add(Element::new(
        "VPOS",
        [p, GROUND],
        Device::VoltageSource(Source::dc(1.0 + 10e-6)),
    ));
    c.add(Element::new(
        "VNEG",
        [n, GROUND],
        Device::VoltageSource(Source::dc(1.0)),
    ));
    c.add(Element::new("RL", [out, GROUND], Device::resistor(75.0)));
    opamp(&mut c, [p, n, out, vp, vn], model.clone());
    let r = run(&c, &Analysis::OperatingPoint, &precise());
    let vd = (1.0 + 10e-6) - 1.0;
    let internal = model.aol * vd;
    assert_close(real(&r, "V(out)")[0], internal / 2.0, 1e-9, 0.0, "rout");
    assert_close(real(&r, "Ip(U1)")[0], vd / model.rin, 1e-9, 0.0, "Ip");
    assert_close(real(&r, "In(U1)")[0], -vd / model.rin, 1e-9, 0.0, "In");
    // The ideal op-amp draws no input current.
    c.elements.retain(|e| e.name != "U1");
    opamp(&mut c, [p, n, out, vp, vn], ideal());
    let r = run(&c, &Analysis::OperatingPoint, &precise());
    assert_eq!(real(&r, "Ip(U1)")[0], 0.0);
    assert_eq!(real(&r, "In(U1)")[0], 0.0);
}

/// Sum of the five recorded terminal currents of U1 in sample `k`.
fn kcl(r: &SimulationResult, k: usize) -> (f64, [f64; 5]) {
    let names = ["Ip(U1)", "In(U1)", "Io(U1)", "Isp(U1)", "Isn(U1)"];
    let currents = names.map(|n| real(r, n)[k]);
    (currents.iter().sum(), currents)
}

#[test]
fn supplies_carry_the_load_current_and_kcl_holds() {
    for model in [ideal(), gp()] {
        for vin in [0.1, -0.1, 0.0] {
            // Inverting gain −10 into 1 kΩ: ∓1 V, sourcing for a negative
            // input, sinking for a positive one.
            let mut circuit = stage(model.clone(), Source::dc(vin), 1e3, 10e3, true);
            let out = circuit.find_node("out").expect("out");
            circuit.add(Element::new("RL", [out, GROUND], Device::resistor(1e3)));
            let r = run(&circuit, &Analysis::OperatingPoint, &precise());
            let (sum, [_, _, io, isp, isn]) = kcl(&r, 0);
            assert!(sum.abs() <= 1e-15, "{} KCL {sum:e}", model.name);
            let i_out = -io;
            let v = real(&r, "V(out)")[0];
            let i_load = v / 1e3 + (v - real(&r, "V(n)")[0]) / 10e3;
            assert_close(i_out, i_load, 1e-9, 1e-18, "output current");
            // The rails see it: V+ while sourcing, V− while sinking.
            assert_close(-real(&r, "I(VP)")[0], isp, 1e-12, 1e-18, "VP");
            assert_close(-real(&r, "I(VN)")[0], isn, 1e-12, 1e-18, "VN");
            if vin < 0.0 {
                assert_close(isp, i_out, 1e-12, 0.0, "sourcing from V+");
                assert!(isn.abs() < 1e-15);
            } else if vin > 0.0 {
                assert_close(isn, i_out, 1e-12, 0.0, "sinking into V−");
                assert!(isp.abs() < 1e-15);
            } else {
                assert!(
                    isp.abs() < 1e-15 && isn.abs() < 1e-15,
                    "no quiescent current"
                );
            }
        }
    }
    // In a transient and in AC too.
    let mut circuit = stage(
        gp(),
        Source::with_waveform(Waveform::Sine {
            offset: 0.0,
            amplitude: 0.5,
            frequency: 10e3,
            delay: 0.0,
            damping: 0.0,
            phase: 0.0,
            cycles: None,
        }),
        1e3,
        10e3,
        true,
    );
    let out = circuit.find_node("out").expect("out");
    circuit.add(Element::new("RL", [out, GROUND], Device::resistor(1e3)));
    let r = run(&circuit, &transient(200e-6, 1e-6), &SimOptions::default());
    for k in 0..r.len() {
        let (sum, c) = kcl(&r, k);
        assert!(
            sum.abs() <= 1e-12 * c[2].abs().max(1e-9),
            "KCL at {k}: {sum:e}"
        );
    }
    let mut circuit = stage(gp(), Source::ac(0.0, 1.0, 0.0), 1e3, 10e3, true);
    let out = circuit.find_node("out").expect("out");
    circuit.add(Element::new("RL", [out, GROUND], Device::resistor(1e3)));
    let names = ["Ip(U1)", "In(U1)", "Io(U1)", "Isp(U1)", "Isn(U1)"];
    let spec = Analysis::Ac(AcSweep {
        scale: SweepScale::Linear,
        points: 1,
        start: 1e4,
        stop: 1e4,
    });
    let r = run(&circuit, &spec, &precise());
    let sum: Complex64 = names.iter().map(|n| phasor(&r, n)).sum();
    assert!(sum.norm() <= 1e-15, "AC KCL {sum}");
    // At the 0 V operating point the split is even.
    let io = phasor(&r, "Io(U1)");
    assert!((phasor(&r, "Isp(U1)") + io * 0.5).norm() <= 1e-12 * io.norm());
}

#[test]
fn integrator_ramps_with_every_method() {
    // Inverting integrator: 10 kΩ, 10 nF (RC = 100 µs), −0.1 V from 10 µs:
    // the output ramps up at r = 1 V/ms. The op-amp holds its input at
    // −(r/(2π·GBW) + V(out)/AOL), which slows the ramp by that over 0.1 V
    // (0.16 % for OA_GP at 1 kV/s, 5·10⁻⁶ for the ideal op-amp at the
    // ramp's mid-point of 0.5 V).
    for model in [gp(), ideal()] {
        for method in METHODS {
            let mut c = Circuit::new();
            let (vp, vn) = supplies(&mut c, 15.0, -15.0);
            let input = c.node("in");
            let n = c.node("n");
            let out = c.node("out");
            c.add(Element::new(
                "VIN",
                [input, GROUND],
                Device::VoltageSource(step(0.0, -0.1, 10e-6)),
            ));
            c.add(Element::new("R1", [input, n], Device::resistor(10e3)));
            c.add(Element::new("C1", [n, out], Device::capacitor(10e-9)));
            opamp(&mut c, [GROUND, n, out, vp, vn], model.clone());
            let options = method_options(method);
            let r = run(&c, &transient(1e-3, 1e-6), &options);
            let (t, v) = (&r.axis.values, real(&r, "V(out)"));
            let what = format!("{} {method:?}", model.name);
            let rise = at_time(t, v, 0.9e-3) - at_time(t, v, 0.1e-3);
            let lag = match model.kind {
                OpAmpKind::Ideal => 0.0,
                OpAmpKind::Macro => 1e3 / (2.0 * PI * model.gbw),
            };
            let expected = 0.8 * (1.0 - (lag + 0.5 / model.aol) / 0.1);
            assert_close(rise, expected, 1e-4, 0.0, &what);
        }
    }
}

#[test]
fn open_loop_comparators_switch_with_every_method() {
    for (model, limit) in [(ideal(), 5.0), (gp(), 3.5)] {
        for method in METHODS {
            let circuit = {
                let mut c = Circuit::new();
                let (vp, vn) = supplies(&mut c, 5.0, -5.0);
                let input = c.node("in");
                let out = c.node("out");
                c.add(Element::new(
                    "VIN",
                    [input, GROUND],
                    Device::VoltageSource(Source::with_waveform(Waveform::Sine {
                        offset: 0.0,
                        amplitude: 1.0,
                        frequency: 1e3,
                        delay: 0.0,
                        damping: 0.0,
                        phase: 0.0,
                        cycles: None,
                    })),
                ));
                c.add(Element::new("RL", [out, GROUND], Device::resistor(10e3)));
                opamp(&mut c, [input, GROUND, out, vp, vn], model.clone());
                c
            };
            let options = method_options(method);
            let r = run(&circuit, &transient(2e-3, 1e-6), &options);
            let (t, v) = (&r.axis.values, real(&r, "V(out)"));
            let what = format!("{} {method:?}", model.name);
            // rout drops the output by rout/(rout + RL).
            let drop = 1.0 / (1.0 + model.rout / 10e3);
            for (at, sign) in [(0.25e-3, 1.0), (0.75e-3, -1.0), (1.25e-3, 1.0)] {
                assert_close(at_time(t, v, at), sign * limit * drop, 1e-6, 0.0, &what);
            }
            assert!(
                v.iter().all(|v| v.abs() <= limit + 1e-9),
                "{what}: within the rails"
            );
            // The output crosses 0 V soon after the input does.
            let down = crossing(t, v, 0.0, 0.4e-3);
            assert!((0.5e-3..0.52e-3).contains(&down), "{what}: {down:e}");
        }
    }
}

#[test]
fn start_from_rest_holds_the_internal_state_at_zero() {
    let circuit = follower(gp(), Source::dc(1.0), 15.0, -15.0);
    let spec = Analysis::Transient(Transient {
        stop: 10e-6,
        step: 0.1e-6,
        start_save: 0.0,
        max_step: None,
        initial_state: InitialState::ZeroState,
    });
    let r = run(&circuit, &spec, &SimOptions::default());
    let (t, v) = (&r.axis.values, real(&r, "V(out)"));
    assert!(v[0].abs() < 1e-9, "{}", v[0]);
    // 1 V is above the slew onset: the output ramps at the slew rate first.
    assert_close(at_time(t, v, 1e-6), 0.5, 1e-3, 0.0, "slewing");
    assert_close(v[v.len() - 1], 1.0, 1e-4, 0.0, "settled");
}

#[test]
fn invalid_parameters_are_rejected() {
    for (field, value) in [
        ("aol", 0.0),
        ("gbw", -1.0),
        ("slew", 0.0),
        ("rin", f64::NAN),
        ("rout", 0.0),
        ("headroom_pos", -0.1),
        ("headroom_neg", f64::INFINITY),
    ] {
        let mut model = gp();
        match field {
            "aol" => model.aol = value,
            "gbw" => model.gbw = value,
            "slew" => model.slew = value,
            "rin" => model.rin = value,
            "rout" => model.rout = value,
            "headroom_pos" => model.headroom_pos = value,
            _ => model.headroom_neg = value,
        }
        let circuit = follower(model, Source::dc(1.0), 15.0, -15.0);
        let error = circuit.validate().expect_err(field);
        let parameter = field.to_uppercase();
        assert!(error.to_string().contains(&parameter), "{error}");
    }
    // The ideal op-amp ignores GBW and slew (infinite in the library).
    let mut model = ideal();
    model.gbw = 0.0;
    assert!(
        follower(model, Source::dc(1.0), 15.0, -15.0)
            .validate()
            .is_ok()
    );
}

#[test]
fn an_unconnected_supply_pin_is_named() {
    let mut c = Circuit::new();
    let vp = c.node("vp");
    let vn = c.node("vn");
    c.add(Element::new(
        "VP",
        [vp, GROUND],
        Device::VoltageSource(Source::dc(15.0)),
    ));
    let input = c.node("in");
    let out = c.node("out");
    c.add(Element::new(
        "VIN",
        [input, GROUND],
        Device::VoltageSource(Source::dc(1.0)),
    ));
    opamp(&mut c, [input, out, out, vp, vn], gp());
    let error = simulate(
        &c,
        &Analysis::OperatingPoint,
        &precise(),
        &SimControl::new(),
    )
    .expect_err("floating V−");
    assert!(error.to_string().contains("vn"), "{error}");
}

#[test]
fn macro_model_matches_ltspice_behavioural_sources() {
    // LTspice 24 batch run of tests/reference/devices_opamp_macro.cir: the
    // same equations built from B sources, R and C.
    let model = OpAmpModel {
        name: "OA_REF".into(),
        kind: OpAmpKind::Macro,
        aol: 2e5,
        gbw: 2e6,
        slew: 2e6,
        rin: 1e6,
        rout: 50.0,
        headroom_pos: 1.5,
        headroom_neg: 1.0,
    };
    let mut c = Circuit::new();
    let (vp, vn) = supplies(&mut c, 12.0, -10.0);
    let input = c.node("in");
    let n = c.node("inn");
    let out = c.node("out");
    c.add(Element::new(
        "VIN",
        [input, GROUND],
        Device::VoltageSource(Source::with_waveform(Waveform::Pulse {
            initial: -1.0,
            pulsed: 6.0,
            delay: 5e-6,
            rise: 1e-9,
            fall: 1e-9,
            width: 20e-6,
            period: 100e-6,
            cycles: None,
        })),
    ));
    c.add(Element::new("R1", [n, GROUND], Device::resistor(10e3)));
    c.add(Element::new("R2", [n, out], Device::resistor(10e3)));
    c.add(Element::new("RL", [out, GROUND], Device::resistor(2e3)));
    opamp(&mut c, [input, n, out, vp, vn], model);
    let options = SimOptions {
        reltol: 1e-9,
        ..SimOptions::default()
    };
    let spec = Analysis::Transient(Transient {
        stop: 50e-6,
        step: 10e-9,
        start_save: 0.0,
        max_step: Some(2e-9),
        initial_state: InitialState::OperatingPoint,
    });
    let r = run(&c, &spec, &options);
    let t = &r.axis.values;
    let v = real(&r, "V(out)");
    for (at, expected) in [
        (4e-6, -1.99997934746325),
        (8e-6, 3.83950446900072),
        (15e-6, 10.2189996610662),
        (28e-6, 4.82891079667266),
        (45e-6, -1.99997934746323),
    ] {
        let what = format!("V(out) at {at:e}");
        assert_close(at_time(t, v, at), expected, 1e-6, 0.0, &what);
    }
    let vp_current = real(&r, "I(VP)");
    let vn_current = real(&r, "I(VN)");
    assert_close(
        at_time(t, vp_current, 15e-6),
        -0.00562000677867622,
        1e-6,
        0.0,
        "I(VP) at 15 µs",
    );
    assert_close(
        at_time(t, vn_current, 45e-6),
        0.00109998863596733,
        1e-6,
        0.0,
        "I(VN) at 45 µs",
    );
    assert!(at_time(t, vp_current, 45e-6).abs() < 1e-12);
    assert_close(
        crossing(t, v, 4.0, 0.0),
        8.08245557715738e-06,
        1e-6,
        0.0,
        "rise",
    );
    assert_close(
        crossing(t, v, 4.0, 20e-6),
        2.84258580767088e-05,
        1e-6,
        0.0,
        "fall",
    );
}

#[test]
fn dc_sweeps_cross_the_limits_smoothly() {
    for model in [ideal(), gp()] {
        let circuit = follower(model.clone(), Source::dc(0.0), 5.0, -5.0);
        let spec = Analysis::DcSweep(DcSweep {
            source: "VIN".into(),
            start: -7.0,
            stop: 7.0,
            step: 0.05,
        });
        let r = run(&circuit, &spec, &precise());
        let (vin, v) = (&r.axis.values, real(&r, "V(out)"));
        // Non-decreasing: the ideal op-amp sits exactly at a rail beyond it.
        assert!(
            v.windows(2).all(|w| w[1] >= w[0]),
            "{}: monotonic",
            model.name
        );
        let swing = match model.kind {
            OpAmpKind::Ideal => 0.0,
            OpAmpKind::Macro => model.headroom_pos,
        };
        let last = v.len() - 1;
        let what = format!("{} ends", model.name);
        assert_close(v[0], clipped(&model, swing - 5.0, -7.0), 1e-9, 0.0, &what);
        assert_close(v[last], clipped(&model, 5.0 - swing, 7.0), 1e-9, 0.0, &what);
        // Inside the range the follower follows: V(out) = vin·A/(1 + A)
        // (rin carries nothing when in+ and in− are equal).
        let k = vin
            .iter()
            .position(|&x| (x - 1.0).abs() < 1e-9)
            .expect("1 V point");
        let expected = model.aol / (1.0 + model.aol);
        assert_close(v[k], expected, 1e-9, 0.0, &format!("{} at 1 V", model.name));
    }
}

#[test]
fn schmitt_triggers_switch_with_every_method() {
    // Inverting Schmitt trigger on ±5 V: in− follows a 1 V sine, in+ sits
    // at a tenth of the output, so the thresholds are ±limit/10.
    for (model, limit) in [(ideal(), 5.0), (gp(), 3.5)] {
        for method in METHODS {
            let mut c = Circuit::new();
            let (vp, vn) = supplies(&mut c, 5.0, -5.0);
            let input = c.node("in");
            let p = c.node("p");
            let out = c.node("out");
            c.add(Element::new(
                "VIN",
                [input, GROUND],
                Device::VoltageSource(Source::with_waveform(Waveform::Sine {
                    offset: 0.0,
                    amplitude: 1.0,
                    frequency: 1e3,
                    delay: 0.0,
                    damping: 0.0,
                    phase: 0.0,
                    cycles: None,
                })),
            ));
            c.add(Element::new("R1", [out, p], Device::resistor(90e3)));
            c.add(Element::new("R2", [p, GROUND], Device::resistor(10e3)));
            opamp(&mut c, [p, input, out, vp, vn], model.clone());
            let r = run(&c, &transient(2e-3, 1e-6), &method_options(method));
            let (t, v) = (&r.axis.values, real(&r, "V(out)"));
            let what = format!("{} {method:?}", model.name);
            // rout and the 100 kΩ divider: the output sits a hair inside
            // (and rin's microampere moves it by a few more microvolts).
            let drop = 1.0 / (1.0 + model.rout / 100e3);
            for (at, sign) in [(0.25e-3, -1.0), (0.75e-3, 1.0), (1.25e-3, -1.0)] {
                assert_close(at_time(t, v, at), sign * limit * drop, 1e-5, 0.0, &what);
            }
            // It switches up where the falling input crosses −limit/10 (the
            // ideal op-amp within the step across it, up to the 1 µs grid).
            let up = crossing(t, v, 0.0, 0.5e-3);
            let threshold = 0.5e-3 + (limit * drop / 10.0).asin() / (2.0 * PI * 1e3);
            let window = threshold - 1e-6..threshold + 20e-6;
            assert!(window.contains(&up), "{what}: {up:e}");
        }
    }
}

/// A pulse from `low` to `high` at `delay` with 1 ns edges, `width` long
/// (period 100 µs, or a single step when `width` is 1 s).
fn pulse(low: f64, high: f64, delay: f64, width: f64) -> Source {
    Source::with_waveform(Waveform::Pulse {
        initial: low,
        pulsed: high,
        delay,
        rise: 1e-9,
        fall: 1e-9,
        width,
        period: if width < 1.0 { 100e-6 } else { 2.0 },
        cycles: None,
    })
}

/// A rail pair `VP<k>`/`VN<k>`.
fn rails(c: &mut Circuit, k: usize, positive: f64, negative: f64) -> (NodeIndex, NodeIndex) {
    let vp = c.node(&format!("vp{k}"));
    let vn = c.node(&format!("vn{k}"));
    c.add(Element::new(
        format!("VP{k}"),
        [vp, GROUND],
        Device::VoltageSource(Source::dc(positive)),
    ));
    c.add(Element::new(
        format!("VN{k}"),
        [vn, GROUND],
        Device::VoltageSource(Source::dc(negative)),
    ));
    (vp, vn)
}

/// An inverting stage `U<k>` on the rails `VP<k>`/`VN<k>`: in+ grounded,
/// `r[0]` from `in<k>` to `n<k>`, `r[1]` from `n<k>` to `out<k>` and the
/// load `r[2]` to ground.
fn inverting(c: &mut Circuit, k: usize, model: OpAmpModel, source: Source, r: [f64; 3]) {
    let vp = c.find_node(&format!("vp{k}")).expect("vp");
    let vn = c.find_node(&format!("vn{k}")).expect("vn");
    let input = c.node(&format!("in{k}"));
    let n = c.node(&format!("n{k}"));
    let out = c.node(&format!("out{k}"));
    c.add(Element::new(
        format!("VIN{k}"),
        [input, GROUND],
        Device::VoltageSource(source),
    ));
    c.add(Element::new(
        format!("R1{k}"),
        [input, n],
        Device::resistor(r[0]),
    ));
    c.add(Element::new(
        format!("R2{k}"),
        [n, out],
        Device::resistor(r[1]),
    ));
    c.add(Element::new(
        format!("RL{k}"),
        [out, GROUND],
        Device::resistor(r[2]),
    ));
    c.add(Element::new(
        format!("U{k}"),
        [GROUND, n, out, vp, vn],
        Device::OpAmp { model },
    ));
}

#[test]
fn models_match_an_independently_built_ltspice_macro_model() {
    // LTspice 24 batch runs of tests/reference/devices_opamp_independent_*.cir:
    // the macro-model rebuilt from an E source, a 1 µF state capacitor and
    // B sources, separately from devices_opamp_macro.cir.

    // Stage A (_op, _ac): OA_GP inverting gain −10 on +12/−10 V into 2 kΩ.
    let mut a = Circuit::new();
    rails(&mut a, 1, 12.0, -10.0);
    inverting(&mut a, 1, gp(), Source::ac(0.1, 1.0, 0.0), [1e3, 10e3, 2e3]);
    let op = run(&a, &Analysis::OperatingPoint, &precise());
    for (name, expected) in [
        ("V(out1)", -0.999885010965372),
        ("V(n1)", 1.04487991484144e-05),
        ("I(VN1)", 0.000599932051459138),
    ] {
        assert_close(real(&op, name)[0], expected, 1e-6, 0.0, name);
    }
    assert!(
        real(&op, "I(VP1)")[0].abs() < 1e-15,
        "sinking: no V+ current"
    );
    // LTspice prints re() and im() as (dB, phase); re is negative (180°).
    let from_db = |db: f64| 10f64.powf(db / 20.0);
    for (f, re_db, im_db) in [
        (1.0, 19.999001159359, -78.7878696305311),
        (5e4, 17.519241594054, 12.7136425877404),
        (8e4, 14.673644979671, 13.9533662791577),
        (9e4, 13.6751054189069, 13.9791505797638),
        (9.1e4, 13.5752338585414, 13.9753922690007),
        (1e5, 12.6797916601959, 13.9004100788787),
        (1.2e5, 10.7356009756723, 13.5431416138853),
        (1e6, -23.2642908253985, -1.2683221448547),
    ] {
        let expected = Complex64::new(-from_db(re_db), from_db(im_db));
        let got = ac_at(&a, f, "V(out1)");
        assert!(
            (got - expected).norm() <= 1e-6 * expected.norm(),
            "AC {f} Hz: got {got}, expected {expected}"
        );
    }

    let (spec, options) = (
        Analysis::Transient(Transient {
            stop: 30e-6,
            step: 10e-9,
            start_save: 0.0,
            max_step: Some(2e-9),
            initial_state: InitialState::OperatingPoint,
        }),
        SimOptions {
            reltol: 1e-9,
            ..SimOptions::default()
        },
    );

    // _clip, stage B: OA_RRIO inverting gain −10 on +5/−10 V into 1 kΩ; a
    // 2 V step clips low. Stage C: OA_IDEAL gain-2 stages on a 5 V single
    // supply, 3 V into 500 Ω (clipped) and 1.2 V (linear).
    let mut b = Circuit::new();
    rails(&mut b, 3, 5.0, -10.0);
    inverting(
        &mut b,
        3,
        rrio(),
        pulse(0.0, 2.0, 1e-6, 1.0),
        [1e3, 10e3, 1e3],
    );
    let vp4 = b.node("vp4");
    b.add(Element::new(
        "VP4",
        [vp4, GROUND],
        Device::VoltageSource(Source::dc(5.0)),
    ));
    for (k, vin, load) in [(4, 3.0, Some(500.0)), (5, 1.2, None)] {
        let input = b.node(&format!("in{k}"));
        let n = b.node(&format!("n{k}"));
        let out = b.node(&format!("out{k}"));
        b.add(Element::new(
            format!("VIN{k}"),
            [input, GROUND],
            Device::VoltageSource(pulse(0.0, vin, 1e-6, 1.0)),
        ));
        b.add(Element::new(
            format!("R1{k}"),
            [n, GROUND],
            Device::resistor(1e3),
        ));
        b.add(Element::new(
            format!("R2{k}"),
            [n, out],
            Device::resistor(1e3),
        ));
        if let Some(load) = load {
            b.add(Element::new(
                format!("RL{k}"),
                [out, GROUND],
                Device::resistor(load),
            ));
        }
        b.add(Element::new(
            format!("U{k}"),
            [input, n, out, vp4, GROUND],
            Device::OpAmp { model: ideal() },
        ));
    }
    let r = run(&b, &spec, &options);
    let t = &r.axis.values;
    for (name, at, expected) in [
        ("V(out3)", 3e-6, -3.86787980739878),
        ("V(out3)", 30e-6, -9.6294016574021),
        ("V(n3)", 30e-6, 0.942695967875456),
        ("I(VN3)", 30e-6, 0.0106866114199299),
        ("V(out4)", 30e-6, 4.99998750003125),
        ("V(out5)", 30e-6, 2.39999520011469),
        ("I(VP4)", 30e-6, -0.0136999663501892),
    ] {
        let what = format!("{name} at {at:e}");
        assert_close(at_time(t, real(&r, name), at), expected, 1e-6, 0.0, &what);
    }
    assert!(at_time(t, real(&r, "I(VP3)"), 30e-6).abs() < 1e-15);
    let fall = crossing(t, real(&r, "V(out3)"), -5.0, 0.0);
    assert_close(
        fall,
        3.58458571348932e-06,
        1e-6,
        0.0,
        "B falls through −5 V",
    );

    // _tran, stage T1: OA_FAST inverting gain −2 on ±15 V into 2 kΩ, a
    // −7.5 V pulse: slews, clips and recovers. T2: an OA_GP follower on
    // ±5 V, −2 → 2 V and back. (Samples on the slewing edges close to their
    // targets are left out: there 1e-6 of the value is a fraction of a
    // picosecond, below LTspice's own time-step dependence at 2 ns.)
    let fast = library::opamp("OA_FAST").expect("OA_FAST");
    let mut c = Circuit::new();
    rails(&mut c, 1, 15.0, -15.0);
    inverting(
        &mut c,
        1,
        fast,
        pulse(0.0, -7.5, 1e-6, 4e-6),
        [1e3, 2e3, 2e3],
    );
    let (vp, vn) = rails(&mut c, 2, 5.0, -5.0);
    let input = c.node("in2");
    let out = c.node("out2");
    c.add(Element::new(
        "VIN2",
        [input, GROUND],
        Device::VoltageSource(pulse(-2.0, 2.0, 1e-6, 12e-6)),
    ));
    c.add(Element::new(
        "U2",
        [input, out, out, vp, vn],
        Device::OpAmp { model: gp() },
    ));
    let r = run(&c, &spec, &options);
    let t = &r.axis.values;
    for (name, at, expected) in [
        ("V(out1)", 1.6e-6, 5.63983955665636),
        ("V(out1)", 4e-6, 12.8400115122638),
        ("I(VP1)", 4e-6, -0.0131997697547248),
        ("V(out2)", 4e-6, -0.499893148265477),
        ("V(out2)", 8e-6, 1.50003157683693),
        ("V(out2)", 12e-6, 1.99997999994095),
        ("V(out2)", 28e-6, -1.99998000020002),
    ] {
        let what = format!("{name} at {at:e}");
        assert_close(at_time(t, real(&r, name), at), expected, 1e-6, 0.0, &what);
    }
    for (name, after, level, expected) in [
        ("V(out1)", 0.0, 6.0, 1.63751691280413e-06),
        ("V(out1)", 3e-6, 6.0, 5.77108924970513e-06),
        ("V(out2)", 0.0, 0.0, 4.99982378820502e-06),
        ("V(out2)", 13e-6, 0.0, 1.70008239754992e-05),
    ] {
        let got = crossing(t, real(&r, name), level, after);
        let what = format!("{name} crosses {level} V after {after:e}");
        assert_close(got, expected, 1e-6, 0.0, &what);
    }
    // T2 is mirror-symmetric, so its rise and fall take the same time.
    let (rise, fall) = (
        crossing(t, real(&r, "V(out2)"), 0.0, 0.0) - 1e-6,
        crossing(t, real(&r, "V(out2)"), 0.0, 13e-6) - 13.001e-6,
    );
    assert_close(fall, rise, 1e-9, 0.0, "symmetric slewing");
}

#[test]
fn integrators_without_dc_feedback_start_from_rest_and_saturate() {
    // −0.1 V into 10 kΩ and 10 nF from t = 0: the output ramps at 1 V/ms
    // from rest, clips and stays clipped while in− relaxes to the input.
    for (model, limit) in [(gp(), 13.5), (ideal(), 15.0)] {
        for method in METHODS {
            let mut c = Circuit::new();
            let (vp, vn) = supplies(&mut c, 15.0, -15.0);
            let input = c.node("in");
            let n = c.node("n");
            let out = c.node("out");
            c.add(Element::new(
                "VIN",
                [input, GROUND],
                Device::VoltageSource(Source::dc(-0.1)),
            ));
            c.add(Element::new("R1", [input, n], Device::resistor(10e3)));
            c.add(Element::new("C1", [n, out], Device::capacitor(10e-9)));
            opamp(&mut c, [GROUND, n, out, vp, vn], model.clone());
            let spec = Analysis::Transient(Transient {
                stop: 20e-3,
                step: 10e-6,
                start_save: 0.0,
                max_step: None,
                initial_state: InitialState::ZeroState,
            });
            let r = run(&c, &spec, &method_options(method));
            let (t, v) = (&r.axis.values, real(&r, "V(out)"));
            let what = format!("{} {method:?}", model.name);
            assert!(v[0].abs() < 1e-9, "{what}: starts at rest, {}", v[0]);
            assert_close(at_time(t, v, 5e-3), 5.0, 5e-3, 0.0, &what);
            let last = v.len() - 1;
            assert_close(v[last], limit, 1e-6, 0.0, &format!("{what} clipped"));
            // in− settles on the divider of R1 and rin (none for the ideal).
            let divider = match model.kind {
                OpAmpKind::Ideal => 1.0,
                OpAmpKind::Macro => model.rin / (model.rin + 10e3),
            };
            let n = real(&r, "V(n)")[last];
            assert_close(n, -0.1 * divider, 1e-6, 0.0, &what);
            // Newton accepts the limited output within reltol (10⁻³ for
            // backward Euler here: the ideal op-amp overshoots by 41 µV).
            let reltol = method_options(method).reltol;
            let within = v.iter().all(|&v| v <= limit * (1.0 + reltol));
            assert!(within, "{what}: within the rail");
            for k in 0..r.len() {
                let (sum, c) = kcl(&r, k);
                let scale = c.iter().fold(1e-9f64, |m, x| m.max(x.abs()));
                assert!(sum.abs() <= 1e-12 * scale, "{what}: KCL at {k}: {sum:e}");
            }
        }
    }
}

#[test]
fn open_loop_square_waves_switch_from_rest() {
    // ±0.5 V square wave at 50 kHz into an open-loop op-amp on ±5 V.
    let fast = library::opamp("OA_FAST").expect("OA_FAST");
    for (model, limit) in [(fast, 3.5), (ideal(), 5.0)] {
        for method in METHODS {
            let mut c = Circuit::new();
            let (vp, vn) = supplies(&mut c, 5.0, -5.0);
            let input = c.node("in");
            let out = c.node("out");
            c.add(Element::new(
                "VIN",
                [input, GROUND],
                Device::VoltageSource(Source::with_waveform(Waveform::Pulse {
                    initial: -0.5,
                    pulsed: 0.5,
                    delay: 1e-6,
                    rise: 10e-9,
                    fall: 10e-9,
                    width: 10e-6,
                    period: 20e-6,
                    cycles: None,
                })),
            ));
            c.add(Element::new("RL", [out, GROUND], Device::resistor(10e3)));
            opamp(&mut c, [input, GROUND, out, vp, vn], model.clone());
            let spec = Analysis::Transient(Transient {
                stop: 100e-6,
                step: 0.1e-6,
                start_save: 0.0,
                max_step: None,
                initial_state: InitialState::ZeroState,
            });
            let r = run(&c, &spec, &method_options(method));
            let (t, v) = (&r.axis.values, real(&r, "V(out)"));
            let what = format!("{} {method:?}", model.name);
            let drop = 1.0 / (1.0 + model.rout / 10e3);
            for (at, sign) in [(9e-6, 1.0), (19e-6, -1.0), (89e-6, 1.0), (99e-6, -1.0)] {
                assert_close(at_time(t, v, at), sign * limit * drop, 1e-6, 0.0, &what);
            }
            assert!(v.iter().all(|v| v.abs() <= limit + 1e-9), "{what}");
        }
    }
}

#[test]
fn bistable_and_open_loop_operating_points_converge() {
    // A Schmitt trigger's operating point inside its hysteresis band has
    // three solutions; outside it, one. Open-loop DC sweeps cross the
    // whole range in one step.
    for (model, limit) in [(ideal(), 5.0), (gp(), 3.5)] {
        for vin in [0.0, 0.2, -0.2, 1.0, -1.0] {
            let mut c = Circuit::new();
            let (vp, vn) = supplies(&mut c, 5.0, -5.0);
            let input = c.node("in");
            let p = c.node("p");
            let out = c.node("out");
            c.add(Element::new(
                "VIN",
                [input, GROUND],
                Device::VoltageSource(Source::dc(vin)),
            ));
            c.add(Element::new("R1", [out, p], Device::resistor(90e3)));
            c.add(Element::new("R2", [p, GROUND], Device::resistor(10e3)));
            opamp(&mut c, [p, input, out, vp, vn], model.clone());
            let r = run(&c, &Analysis::OperatingPoint, &precise());
            let v = real(&r, "V(out)")[0];
            let what = format!("{} at {vin}", model.name);
            if vin.abs() > 0.5 {
                let drop = 1.0 / (1.0 + model.rout / 100e3);
                assert_close(v, -vin.signum() * limit * drop, 1e-5, 0.0, &what);
            } else {
                assert!(v.abs() <= limit, "{what}: {v}");
            }
        }
        let mut c = Circuit::new();
        let (vp, vn) = supplies(&mut c, 5.0, -5.0);
        let input = c.node("in");
        let out = c.node("out");
        c.add(Element::new(
            "VIN",
            [input, GROUND],
            Device::VoltageSource(Source::dc(0.0)),
        ));
        opamp(&mut c, [input, GROUND, out, vp, vn], model.clone());
        let spec = Analysis::DcSweep(DcSweep {
            source: "VIN".into(),
            start: -1.0,
            stop: 1.0,
            step: 0.25,
        });
        let r = run(&c, &spec, &precise());
        let v = real(&r, "V(out)");
        assert!(v.windows(2).all(|w| w[1] >= w[0]), "{}", model.name);
        assert_close(v[0], -limit, 1e-9, 0.0, &model.name);
        assert_close(v[v.len() - 1], limit, 1e-9, 0.0, &model.name);
        assert!(v[4].abs() < 1e-12, "{}: {}", model.name, v[4]);
    }
}
