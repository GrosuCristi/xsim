//! Small-signal AC analysis against closed-form transfer functions.

use num_complex::Complex64;
use std::f64::consts::{FRAC_1_SQRT_2, PI, TAU};
use xsim_spice_engine::*;

fn ac(
    circuit: &Circuit,
    scale: SweepScale,
    points: usize,
    start: f64,
    stop: f64,
) -> SimulationResult {
    let analysis = Analysis::Ac(AcSweep {
        scale,
        points,
        start,
        stop,
    });
    simulate(
        circuit,
        &analysis,
        &SimOptions::default(),
        &SimControl::new(),
    )
    .unwrap()
}

fn phasors(result: &SimulationResult, name: &str) -> Vec<Complex64> {
    match &result
        .signal(name)
        .unwrap_or_else(|| panic!("no signal {name}"))
        .data
    {
        SignalData::Complex { re, im } => re
            .iter()
            .zip(im)
            .map(|(&r, &i)| Complex64::new(r, i))
            .collect(),
        SignalData::Real(_) => panic!("{name} is not complex"),
    }
}

fn assert_relative(actual: Complex64, expected: Complex64, tolerance: f64) {
    let error = (actual - expected).norm() / expected.norm();
    assert!(
        error <= tolerance,
        "expected {expected}, got {actual} (relative error {error:e})"
    );
}

/// 1 V AC source, 1 kΩ, 1 µF low-pass.
fn rc_low_pass() -> Circuit {
    let mut c = Circuit::new();
    let (a, b) = (c.node("in"), c.node("out"));
    c.add(Element::new(
        "V1",
        [a, GROUND],
        Device::VoltageSource(Source::ac(0.0, 1.0, 0.0)),
    ));
    c.add(Element::new("R1", [a, b], Device::resistor(1e3)));
    c.add(Element::new("C1", [b, GROUND], Device::capacitor(1e-6)));
    c
}

#[test]
fn rc_low_pass_matches_its_transfer_function() {
    let r = ac(&rc_low_pass(), SweepScale::Decade, 10, 1.0, 1e6);
    assert_eq!(r.analysis, AnalysisKind::Ac);
    assert_eq!(
        (r.axis.name.as_str(), r.axis.unit.as_str()),
        ("frequency", "Hz")
    );
    assert_eq!(r.len(), 61);
    let (out, cap, source) = (
        phasors(&r, "V(out)"),
        phasors(&r, "I(C1)"),
        phasors(&r, "I(V1)"),
    );
    for (k, &f) in r.axis.values.iter().enumerate() {
        let jwc = Complex64::new(0.0, TAU * f * 1e-6);
        let h = 1.0 / (1.0 + jwc * 1e3);
        assert_relative(out[k], h, 1e-9);
        assert_relative(cap[k], jwc * h, 1e-9);
        // The source delivers the capacitor current: passive sign convention.
        assert_relative(source[k], -jwc * h, 1e-9);
        let magnitude_db = 20.0 * out[k].norm().log10();
        let expected_db = -10.0 * (1.0 + (TAU * f * 1e-3).powi(2)).log10();
        assert!((magnitude_db - expected_db).abs() < 1e-8);
        let phase = out[k].arg().to_degrees();
        assert!((phase + (TAU * f * 1e-3).atan().to_degrees()).abs() < 1e-8);
    }
}

#[test]
fn rc_cutoff_has_half_power_and_minus_45_degrees() {
    // Port of the prototype: one point per decade around the cutoff.
    let cutoff = 1.0 / (TAU * 1e-3);
    let r = ac(
        &rc_low_pass(),
        SweepScale::Decade,
        1,
        cutoff / 10.0,
        cutoff * 10.0,
    );
    assert_eq!(r.len(), 3);
    assert!((r.axis.values[1] - cutoff).abs() <= 1e-12 * cutoff);
    let v = phasors(&r, "V(out)")[1];
    assert!((v.norm() - FRAC_1_SQRT_2).abs() < 1e-12);
    assert!((v.arg().to_degrees() + 45.0).abs() < 1e-10);
    let i = phasors(&r, "I(C1)")[1];
    assert!((i.norm() - FRAC_1_SQRT_2 / 1000.0).abs() < 1e-14);
    assert!((i.arg().to_degrees() - 45.0).abs() < 1e-10);
}

#[test]
fn randomized_rc_high_pass_filters_match_their_response() {
    // Port of the prototype; a fixed-seed LCG keeps failures reproducible.
    let mut seed = 3181_u64;
    let mut sample = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        (seed >> 11) as f64 / (1_u64 << 53) as f64
    };
    for _ in 0..64 {
        let r = 1e2 * 10_f64.powf(4.0 * sample());
        let c = 1e-12 * 10_f64.powf(6.0 * sample());
        let hz = 1.0 / (TAU * r * c) * 10_f64.powf(6.0 * sample() - 3.0);
        let mut circuit = Circuit::new();
        let (a, b) = (circuit.node("1"), circuit.node("2"));
        circuit.add(Element::new(
            "V",
            [a, GROUND],
            Device::VoltageSource(Source::ac(0.0, 1.0, 0.0)),
        ));
        circuit.add(Element::new("C", [a, b], Device::capacitor(c)));
        circuit.add(Element::new("R", [b, GROUND], Device::resistor(r)));
        let result = ac(&circuit, SweepScale::Linear, 1, hz, hz);
        let v = phasors(&result, "V(2)")[0];
        let x = TAU * hz * r * c;
        assert!((v.norm() - x / x.hypot(1.0)).abs() <= 1e-9 * v.norm());
        assert!((v.arg().to_degrees() - (1.0 / x).atan().to_degrees()).abs() < 1e-9);
    }
}

#[test]
fn series_rlc_resonates_at_one_over_root_lc() {
    let (l, c, r): (f64, f64, f64) = (10e-3, 100e-9, 50.0);
    let f0 = 1.0 / (TAU * (l * c).sqrt());
    let mut circuit = Circuit::new();
    let (a, b, d) = (circuit.node("a"), circuit.node("b"), circuit.node("d"));
    circuit.add(Element::new(
        "V1",
        [a, GROUND],
        Device::VoltageSource(Source::ac(0.0, 1.0, 0.0)),
    ));
    circuit.add(Element::new("L1", [a, b], Device::inductor(l)));
    circuit.add(Element::new("C1", [b, d], Device::capacitor(c)));
    circuit.add(Element::new("R1", [d, GROUND], Device::resistor(r)));

    // At resonance the reactances cancel: the whole source appears on R.
    let at_f0 = ac(&circuit, SweepScale::Linear, 1, f0, f0);
    assert_relative(phasors(&at_f0, "V(d)")[0], Complex64::new(1.0, 0.0), 1e-9);
    assert_relative(
        phasors(&at_f0, "I(L1)")[0],
        Complex64::new(1.0 / r, 0.0),
        1e-9,
    );

    // A fine sweep peaks at the sample nearest f0 and follows |Z| everywhere.
    let sweep = ac(&circuit, SweepScale::Decade, 200, f0 / 3.0, f0 * 3.0);
    let current = phasors(&sweep, "I(R1)");
    let peak = (0..current.len())
        .max_by(|&i, &j| current[i].norm().total_cmp(&current[j].norm()))
        .unwrap();
    let nearest = (0..current.len())
        .min_by(|&i, &j| {
            (sweep.axis.values[i] / f0)
                .ln()
                .abs()
                .total_cmp(&(sweep.axis.values[j] / f0).ln().abs())
        })
        .unwrap();
    assert_eq!(peak, nearest);
    for (k, &f) in sweep.axis.values.iter().enumerate() {
        let w = TAU * f;
        let z = Complex64::new(r, w * l - 1.0 / (w * c));
        assert_relative(current[k], 1.0 / z, 1e-9);
    }
}

#[test]
fn source_phase_and_current_sources_are_honoured() {
    let mut c = Circuit::new();
    let (a, b) = (c.node("a"), c.node("b"));
    c.add(Element::new(
        "V1",
        [a, GROUND],
        Device::VoltageSource(Source::ac(3.0, 2.0, 30.0)),
    ));
    c.add(Element::new("R1", [a, GROUND], Device::resistor(1e3)));
    c.add(Element::new(
        "I1",
        [GROUND, b],
        Device::CurrentSource(Source::ac(1.0, 1e-3, -90.0)),
    ));
    c.add(Element::new("L1", [b, GROUND], Device::inductor(1e-3)));
    c.add(Element::new("R2", [b, GROUND], Device::resistor(10.0)));
    let f = 1e3;
    let r = ac(&c, SweepScale::Octave, 3, f, f);
    assert_relative(
        phasors(&r, "V(a)")[0],
        Complex64::from_polar(2.0, PI / 6.0),
        1e-12,
    );
    // DC values do not enter the small-signal solution.
    let injected = Complex64::from_polar(1e-3, -PI / 2.0);
    let z = 1.0 / (1.0 / Complex64::new(0.0, TAU * f * 1e-3) + 1.0 / 10.0);
    assert_relative(phasors(&r, "V(b)")[0], injected * z, 1e-12);
    assert_relative(phasors(&r, "I(I1)")[0], injected, 1e-12);
    // The inductor's branch current and the resistor's share add up.
    let total = phasors(&r, "I(L1)")[0] + phasors(&r, "I(R2)")[0];
    assert_relative(total, injected, 1e-12);
}

#[test]
fn frequency_axis_follows_the_requested_spacing() {
    let decade = ac(&rc_low_pass(), SweepScale::Decade, 5, 10.0, 1e4);
    assert_eq!(decade.len(), 16);
    assert_eq!(decade.axis.values[5], 100.0);
    assert_eq!(decade.axis.values[15], 1e4);
    let octave = ac(&rc_low_pass(), SweepScale::Octave, 1, 100.0, 1600.0);
    assert_eq!(octave.axis.values, [100.0, 200.0, 400.0, 800.0, 1600.0]);
    let linear = ac(&rc_low_pass(), SweepScale::Linear, 4, 1.0, 4.0);
    assert_eq!(linear.axis.values, [1.0, 2.0, 3.0, 4.0]);
}

#[test]
fn invalid_sweeps_are_rejected() {
    for (scale, points, start, stop) in [
        (SweepScale::Decade, 10, 0.0, 1.0),
        (SweepScale::Decade, 10, -1.0, 1.0),
        (SweepScale::Decade, 10, 10.0, 1.0),
        (SweepScale::Decade, 0, 1.0, 10.0),
        (SweepScale::Linear, 1, 1.0, 10.0),
        (SweepScale::Octave, 10, 1.0, f64::NAN),
    ] {
        let analysis = Analysis::Ac(AcSweep {
            scale,
            points,
            start,
            stop,
        });
        let error = simulate(
            &rc_low_pass(),
            &analysis,
            &SimOptions::default(),
            &SimControl::new(),
        )
        .unwrap_err();
        assert!(matches!(error, SimError::InvalidCircuit(_)), "{error}");
    }
}

#[test]
fn ac_reports_a_singular_operating_point() {
    let mut c = rc_low_pass();
    let n = c.node("float");
    // A current source into a capacitor has no operating point. (A node
    // reached only through capacitors is tied to ground instead, D15.)
    c.add(Element::new("C9", [n, GROUND], Device::capacitor(1e-9)));
    c.add(Element::new(
        "I9",
        [GROUND, n],
        Device::CurrentSource(Source::dc(1e-6)),
    ));
    let analysis = Analysis::Ac(AcSweep {
        scale: SweepScale::Decade,
        points: 1,
        start: 1.0,
        stop: 10.0,
    });
    let error = simulate(&c, &analysis, &SimOptions::default(), &SimControl::new()).unwrap_err();
    assert!(
        matches!(&error, SimError::SingularMatrix { unknown, .. } if unknown == "V(float)"),
        "{error}"
    );
}

// ------------------------------------------- terminal currents (DCAC-1)

/// `circuit` with a 0 V ammeter `AM<k>` between terminal `k` of element
/// `name` and its node, so `I(AM<k>)` is the current into that terminal.
/// The element's terminals then sit on distinct private nodes: an oracle
/// for terminals that share a node or are grounded.
fn with_ammeters(circuit: &Circuit, name: &str) -> Circuit {
    let mut c = circuit.clone();
    let index = c
        .elements
        .iter()
        .position(|e| e.name == name)
        .unwrap_or_else(|| panic!("no element {name}"));
    let nodes = c.elements[index].nodes.clone();
    for (k, &node) in nodes.iter().enumerate() {
        let private = c.node(&format!("{name}:am{k}"));
        c.elements[index].nodes[k] = private;
        c.add(Element::new(
            format!("AM{k}"),
            [node, private],
            Device::VoltageSource(Source::dc(0.0)),
        ));
    }
    c
}

/// The JFET `JM` of `tests/reference/devices_ac_currents.cir`.
fn jfet_model() -> JfetModel {
    JfetModel {
        vto: -2.0,
        beta: 1.3e-3,
        lambda: 0.02,
        rd: 10.0,
        rs: 5.0,
        cgs: 3e-12,
        cgd: 1.5e-12,
        pb: 0.8,
        b: 0.8,
        ..JfetModel::default()
    }
}

/// The NPN `QM` of `tests/reference/devices_ac_currents.cir`.
fn npn_model() -> BjtModel {
    BjtModel {
        is: 5e-15,
        bf: 180.0,
        br: 4.0,
        vaf: 60.0,
        cje: 10e-12,
        cjc: 4e-12,
        tf: 300e-12,
        cjs: 2e-12,
        vjs: 0.7,
        mjs: 0.33,
        ..BjtModel::default()
    }
}

/// The circuits of `tests/reference/devices_ac_currents.cir`: JA (gate tied
/// to source), JB (gate and source grounded), the mirror QM1/QM2 with a
/// diode-connected input and the grounded-emitter stage QE with CJS.
fn shared_terminal_circuit() -> Circuit {
    let mut c = Circuit::new();
    let (da, sa, db) = (c.node("da"), c.node("sa"), c.node("db"));
    c.add(Element::new(
        "VDA",
        [da, GROUND],
        Device::VoltageSource(Source::ac(5.0, 1.0, 0.0)),
    ));
    c.add(Element::new("JA", [da, sa, sa], Device::jfet(jfet_model())));
    c.add(Element::new("RSA", [sa, GROUND], Device::resistor(100.0)));
    c.add(Element::new(
        "VDB",
        [db, GROUND],
        Device::VoltageSource(Source::ac(5.0, 1.0, 0.0)),
    ));
    c.add(Element::new(
        "JB",
        [db, GROUND, GROUND],
        Device::jfet(jfet_model()),
    ));
    let (m, out) = (c.node("m"), c.node("out"));
    c.add(Element::new(
        "I1",
        [GROUND, m],
        Device::CurrentSource(Source::ac(100e-6, 1e-6, 0.0)),
    ));
    c.add(Element::new(
        "QM1",
        [m, m, GROUND],
        Device::bjt(npn_model()),
    ));
    c.add(Element::new(
        "QM2",
        [out, m, GROUND],
        Device::bjt(npn_model()),
    ));
    c.add(Element::new(
        "VOUT",
        [out, GROUND],
        Device::VoltageSource(Source::dc(5.0)),
    ));
    let (vcc, col) = (c.node("vcc"), c.node("c"));
    let (base, input) = (c.node("b"), c.node("in"));
    c.add(Element::new(
        "VCC",
        [vcc, GROUND],
        Device::VoltageSource(Source::dc(12.0)),
    ));
    c.add(Element::new("RC", [vcc, col], Device::resistor(2e3)));
    c.add(Element::new("RB", [vcc, base], Device::resistor(1e6)));
    c.add(Element::new("CIN", [input, base], Device::capacitor(1e-6)));
    c.add(Element::new(
        "VIN",
        [input, GROUND],
        Device::VoltageSource(Source::ac(0.0, 1.0, 0.0)),
    ));
    c.add(Element::new(
        "QE",
        [col, base, GROUND],
        Device::bjt(npn_model()),
    ));
    c
}

/// A PNP with base, collector and emitter resistances and CJS, diode
/// connected from the supply, and a closed switch whose positive terminal
/// is grounded.
fn resistive_terminal_circuit() -> Circuit {
    let pnp = BjtModel {
        polarity: Polarity::P,
        rb: 50.0,
        rbm: 50.0,
        rc: 5.0,
        re: 1.0,
        ..npn_model()
    };
    let mut c = Circuit::new();
    let (vcc, m) = (c.node("vcc"), c.node("m"));
    c.add(Element::new(
        "VCC",
        [vcc, GROUND],
        Device::VoltageSource(Source::ac(5.0, 1.0, 0.0)),
    ));
    c.add(Element::new("QP", [m, m, vcc], Device::bjt(pnp)));
    c.add(Element::new("RM", [m, GROUND], Device::resistor(20e3)));
    let (a, o, ctl) = (c.node("a"), c.node("o"), c.node("ctl"));
    c.add(Element::new(
        "VC",
        [ctl, GROUND],
        Device::VoltageSource(Source::dc(2.0)),
    ));
    c.add(Element::new(
        "V1",
        [a, GROUND],
        Device::VoltageSource(Source::ac(-5.0, 1.0, 0.0)),
    ));
    c.add(Element::new("R1", [a, o], Device::resistor(1e3)));
    c.add(Element::new(
        "S1",
        [GROUND, o, ctl, GROUND],
        Device::VoltageSwitch {
            model: SwitchModel {
                ron: 10.0,
                roff: 1e6,
                vt: 1.0,
                vh: 0.0,
                ..SwitchModel::default()
            },
        },
    ));
    c
}

fn at(result: &SimulationResult, name: &str) -> Complex64 {
    phasors(result, name)[0]
}

/// The recorded terminal currents of `element` (`prefixes`) agree with the
/// ammeters of the same circuit at `frequency`.
fn assert_matches_ammeters(circuit: &Circuit, element: &str, prefixes: &[&str], frequency: f64) {
    let direct = ac(circuit, SweepScale::Linear, 1, frequency, frequency);
    let metered = ac(
        &with_ammeters(circuit, element),
        SweepScale::Linear,
        1,
        frequency,
        frequency,
    );
    let currents: Vec<Complex64> = prefixes
        .iter()
        .map(|p| at(&direct, &format!("{p}({element})")))
        .collect();
    let scale = currents.iter().map(|i| i.norm()).fold(0.0, f64::max);
    for (k, (prefix, &i)) in prefixes.iter().zip(&currents).enumerate() {
        let expected = at(&metered, &format!("I(AM{k})"));
        assert!(
            (i - expected).norm() <= 1e-9 * scale + 1e-20,
            "{prefix}({element}) at {frequency} Hz: {i} vs ammeter {expected}"
        );
    }
}

#[test]
fn terminal_currents_of_shared_and_grounded_terminals_match_ammeters() {
    let shared = shared_terminal_circuit();
    let resistive = resistive_terminal_circuit();
    let jfet = ["Id", "Ig", "Is"];
    let bjt = ["Ic", "Ib", "Ie"];
    for f in [1e3, 100e6, 1e9] {
        assert_matches_ammeters(&shared, "JA", &jfet, f);
        assert_matches_ammeters(&shared, "JB", &jfet, f);
        assert_matches_ammeters(&shared, "QM1", &bjt, f);
        assert_matches_ammeters(&shared, "QM2", &bjt, f);
        assert_matches_ammeters(&shared, "QE", &bjt, f);
        assert_matches_ammeters(&resistive, "QP", &bjt, f);
        assert_matches_ammeters(&resistive, "S1", &["I"], f);
    }
}

#[test]
fn grounded_switch_terminal_carries_the_switch_current() {
    // V1 (1 V AC) drives R1 (1 kΩ) into the closed 10 Ω switch, whose
    // positive terminal is grounded: the AC current 1/(1010 Ω) leaves the
    // switch through that terminal.
    let r = ac(
        &resistive_terminal_circuit(),
        SweepScale::Linear,
        1,
        1e3,
        1e3,
    );
    assert_relative(at(&r, "I(S1)"), Complex64::new(-1.0 / 1010.0, 0.0), 1e-12);
    assert_relative(at(&r, "I(R1)"), Complex64::new(1.0 / 1010.0, 0.0), 1e-12);
}

#[test]
fn substrate_current_stays_in_the_collector_current() {
    // A transistor with only CJS, emitter and base grounded: the collector's
    // AC current is the substrate capacitance's, and none of it appears in
    // the grounded emitter or base.
    let model = BjtModel {
        cjs: 2e-12,
        vjs: 0.7,
        mjs: 0.3,
        ..BjtModel::default()
    };
    let mut c = Circuit::new();
    let col = c.node("c");
    c.add(Element::new(
        "VC",
        [col, GROUND],
        Device::VoltageSource(Source::ac(5.0, 1.0, 0.0)),
    ));
    c.add(Element::new(
        "Q1",
        [col, GROUND, GROUND],
        Device::bjt(model),
    ));
    let f = 1e6;
    let r = ac(&c, SweepScale::Linear, 1, f, f);
    // The collector is 5 V above the substrate: the junction is reverse
    // biased by 5 V, C = cjs·(1 + 5/vjs)^(−mjs).
    // Only the B-C junction's gmin (1 pS) adds a real part.
    let ic = at(&r, "Ic(Q1)");
    let ccs = 2e-12 * (1.0 + 5.0 / 0.7_f64).powf(-0.3);
    assert!((ic.im / (TAU * f * ccs) - 1.0).abs() < 1e-12, "{ic}");
    assert!((ic.re - 1e-12).abs() < 1e-18, "{ic}");
    let ib = at(&r, "Ib(Q1)");
    assert!((ib + Complex64::new(1e-12, 0.0)).norm() < 1e-18, "{ib}");
    assert!(at(&r, "Ie(Q1)").norm() < 1e-18);
}

#[test]
fn terminal_currents_match_ltspice() {
    // LTspice 24.1.9 batch run of tests/reference/devices_ac_currents.cir
    // (.meas values converted from dB).
    let expected: &[(&str, f64, f64, f64)] = &[
        ("Ig(JA)", 1e3, -9.9191905776e-13, -3.6575925882e-9),
        ("Is(JA)", 1e3, -9.1745606065e-5, 4.5101387954e-11),
        ("Id(JA)", 1e3, 9.1745607057e-5, 3.6124912002e-9),
        ("Ig(JB)", 100e6, -1.2448850693e-6, -3.5214634200e-4),
        ("Is(JB)", 100e6, -9.2535567901e-5, 1.1591553864e-6),
        ("Id(JB)", 100e6, 9.3780452970e-5, 3.5098718661e-4),
        ("Ib(QM1)", 100e6, 4.3540863866e-7, 7.2411795293e-8),
        ("Ic(QM1)", 100e6, 7.0538514859e-8, -1.5470121969e-7),
        ("Ie(QM1)", 100e6, -4.6269257439e-7, 8.9574929732e-8),
        ("Ib(QE)", 1e9, 5.9626364569e-2, 2.7165164808e-1),
        ("Ic(QE)", 1e9, -2.6067636359e-4, -2.5344878888e-3),
        ("Ie(QE)", 1e9, -8.7679345026e-2, -2.6620505273e-1),
    ];
    let circuit = shared_terminal_circuit();
    for &(name, f, re, im) in expected {
        let r = ac(&circuit, SweepScale::Linear, 1, f, f);
        // The element's largest terminal current sets the scale, so a
        // terminal carrying almost nothing is compared absolutely.
        let element = &name[name.find('(').unwrap_or(0)..];
        let scale = ["Id", "Ig", "Is", "Ic", "Ib", "Ie"]
            .iter()
            .filter(|p| r.signal(&format!("{p}{element}")).is_some())
            .map(|p| at(&r, &format!("{p}{element}")).norm())
            .fold(0.0, f64::max);
        let got = at(&r, name);
        assert!(
            (got - Complex64::new(re, im)).norm() <= 1e-6 * scale,
            "{name} at {f} Hz: xsim {got}, LTspice {re}{im:+}i"
        );
    }
}
