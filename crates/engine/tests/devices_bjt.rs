//! Full-simulation tests of the Gummel–Poon BJT: closed-form Ebers–Moll and
//! Early-effect checks, base resistance, temperature, small-signal gain,
//! excess phase, polarity symmetry, KCL and LTspice cross-checks.

use std::f64::consts::PI;
use xsim_spice_engine::*;

const BOLTZMANN: f64 = 1.380_649e-23;
const CHARGE: f64 = 1.602_176_634e-19;

fn thermal_voltage(celsius: f64) -> f64 {
    BOLTZMANN * (celsius + 273.15) / CHARGE
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

fn op(circuit: &Circuit) -> SimulationResult {
    run(circuit, &Analysis::OperatingPoint, &precise())
}

fn value(result: &SimulationResult, name: &str) -> f64 {
    result
        .signal(name)
        .unwrap_or_else(|| panic!("no signal {name}"))
        .real()
        .unwrap()[0]
}

fn complex(result: &SimulationResult, name: &str, index: usize) -> (f64, f64) {
    match &result
        .signal(name)
        .unwrap_or_else(|| panic!("no signal {name}"))
        .data
    {
        SignalData::Complex { re, im } => (re[index], im[index]),
        SignalData::Real(_) => panic!("{name} is not complex"),
    }
}

fn assert_close(actual: f64, expected: f64, rel: f64, abs: f64, what: &str) {
    let err = (actual - expected).abs();
    assert!(
        err <= rel * expected.abs() + abs,
        "{what}: got {actual:.12e}, expected {expected:.12e} (error {err:.3e})"
    );
}

fn dc(value: f64) -> Device {
    Device::VoltageSource(Source::dc(value))
}

/// A transistor whose Gummel–Poon model reduces to transport Ebers–Moll.
fn ideal(polarity: Polarity) -> BjtModel {
    BjtModel {
        polarity,
        is: 2e-15,
        bf: 150.0,
        br: 4.0,
        ..BjtModel::default()
    }
}

/// Transistor with its base-emitter and collector-emitter voltages forced
/// by sources `VBE` and `VCE` (emitter grounded).
fn forced(model: BjtModel, vbe: f64, vce: f64) -> Circuit {
    let mut c = Circuit::new();
    let b = c.node("b");
    let col = c.node("c");
    c.add(Element::new("VBE", [b, GROUND], dc(vbe)));
    c.add(Element::new("VCE", [col, GROUND], dc(vce)));
    c.add(Element::new("Q1", [col, b, GROUND], Device::bjt(model)));
    c
}

#[test]
fn ideal_transistor_follows_ebers_moll_in_every_region() {
    let vt = thermal_voltage(27.0);
    let gmin = precise().gmin;
    for &(vbe, vce) in &[
        (0.65, 5.0),
        (0.7, 0.1),
        (0.72, 0.02),
        (0.3, 3.0),
        (-0.5, 2.0),
        (0.6, -0.3),
    ] {
        let r = op(&forced(ideal(Polarity::N), vbe, vce));
        let vbc = vbe - vce;
        let fwd = 2e-15 * (vbe / vt).exp_m1();
        let rev = 2e-15 * (vbc / vt).exp_m1();
        let ic = fwd - rev - rev / 4.0 - gmin * vbc;
        let ib = fwd / 150.0 + rev / 4.0 + gmin * (vbe + vbc);
        assert_close(
            value(&r, "Ic(Q1)"),
            ic,
            1e-9,
            1e-20,
            &format!("Ic at vbe={vbe} vce={vce}"),
        );
        assert_close(
            value(&r, "Ib(Q1)"),
            ib,
            1e-9,
            1e-20,
            &format!("Ib at vbe={vbe} vce={vce}"),
        );
        assert_close(value(&r, "Ie(Q1)"), -(ic + ib), 1e-9, 1e-20, "Ie");
    }
}

#[test]
fn early_effect_scales_transport_current() {
    let model = BjtModel {
        vaf: 60.0,
        ..ideal(Polarity::N)
    };
    let vt = thermal_voltage(27.0);
    let vbe = 0.66;
    for vce in [1.0, 5.0, 20.0] {
        let r = op(&forced(model.clone(), vbe, vce));
        let vbc = vbe - vce;
        let transport = 2e-15 * ((vbe / vt).exp_m1() - (vbc / vt).exp_m1());
        let expected = transport * (1.0 - vbc / 60.0)
            - 2e-15 * (vbc / vt).exp_m1() / 4.0
            - precise().gmin * vbc;
        assert_close(
            value(&r, "Ic(Q1)"),
            expected,
            1e-9,
            0.0,
            &format!("Ic at vce={vce}"),
        );
    }
}

#[test]
fn base_resistance_adds_ohmic_drop() {
    // Base current forced by a source, collector held at 5 V: the external
    // base voltage is the junction voltage plus Ib·RB (RBM = RB: constant).
    let model = BjtModel {
        rb: 250.0,
        rbm: 250.0,
        ..ideal(Polarity::N)
    };
    let ib = 5e-6;
    let mut c = Circuit::new();
    let b = c.node("b");
    let col = c.node("c");
    c.add(Element::new(
        "IB",
        [GROUND, b],
        Device::CurrentSource(Source::dc(ib)),
    ));
    c.add(Element::new("VCE", [col, GROUND], dc(5.0)));
    c.add(Element::new("Q1", [col, b, GROUND], Device::bjt(model)));
    let r = op(&c);
    let vt = thermal_voltage(27.0);
    let gmin = precise().gmin;
    // Solve ib = is/bf·(e^(v/Vt) − 1) + gmin·v + (B-C leakage) for the junction.
    let vb = value(&r, "V(b)");
    let junction = vb - ib * 250.0;
    let vbc = junction - 5.0;
    let base = 2e-15 / 150.0 * (junction / vt).exp_m1()
        + gmin * junction
        + 2e-15 / 4.0 * (vbc / vt).exp_m1()
        + gmin * vbc;
    assert_close(base, ib, 1e-9, 0.0, "base current through RB");
}

#[test]
fn collector_current_follows_saturation_current_temperature_law() {
    let model = BjtModel {
        xti: 3.0,
        eg: 1.11,
        ..ideal(Polarity::N)
    };
    let vbe = 0.6;
    let at = |celsius: f64| {
        let options = SimOptions {
            temperature: celsius,
            ..precise()
        };
        value(
            &run(
                &forced(model.clone(), vbe, 5.0),
                &Analysis::OperatingPoint,
                &options,
            ),
            "Ic(Q1)",
        )
    };
    let (t, tnom) = (373.15, 300.15);
    let vt = thermal_voltage(100.0);
    let is = 2e-15 * ((t / tnom - 1.0) * 1.11 / vt + 3.0 * (t / tnom).ln()).exp();
    let vbc = vbe - 5.0;
    let expected = is * ((vbe / vt).exp_m1() - (vbc / vt).exp_m1())
        - is * (vbc / vt).exp_m1() / 4.0
        - precise().gmin * vbc;
    assert_close(at(100.0), expected, 1e-9, 0.0, "Ic at 100 °C");
    // At fixed VBE the IS increase outweighs the smaller VBE/Vt: roughly ×90.
    assert!(at(100.0) > 50.0 * at(27.0));
}

#[test]
fn pnp_mirrors_npn() {
    let npn = BjtModel {
        vaf: 80.0,
        ikf: 0.05,
        rb: 20.0,
        rc: 1.0,
        re: 0.5,
        ..ideal(Polarity::N)
    };
    let pnp = BjtModel {
        polarity: Polarity::P,
        ..npn.clone()
    };
    let rn = op(&forced(npn, 0.7, 4.0));
    let rp = op(&forced(pnp, -0.7, -4.0));
    for name in ["Ic(Q1)", "Ib(Q1)", "Ie(Q1)"] {
        assert_close(value(&rp, name), -value(&rn, name), 1e-12, 0.0, name);
    }
}

#[test]
fn terminal_currents_obey_kcl_across_a_dc_sweep() {
    let model = extended_npn();
    let mut c = forced(model, 0.0, 5.0);
    c.elements[2].nodes = vec![c.find_node("c").unwrap(), c.find_node("b").unwrap(), GROUND];
    let sweep = Analysis::DcSweep(DcSweep {
        source: "VBE".into(),
        start: 0.3,
        stop: 0.9,
        step: 0.05,
    });
    let r = run(&c, &sweep, &precise());
    let ic = r.signal("Ic(Q1)").unwrap().real().unwrap();
    let ib = r.signal("Ib(Q1)").unwrap().real().unwrap();
    let ie = r.signal("Ie(Q1)").unwrap().real().unwrap();
    for k in 0..r.len() {
        let sum = ic[k] + ib[k] + ie[k];
        assert!(
            sum.abs() <= 1e-12 * ie[k].abs() + 1e-24,
            "KCL at point {k}: {sum:e}"
        );
    }
    // The sweep covers cut-off to high injection.
    assert!(ic[0] < 1e-6 && ic[r.len() - 1] > 1e-2);
}

/// Common-emitter stage of `forced` with a collector resistor and an AC
/// base drive; returns the collector voltage at `frequency`.
fn ce_gain(model: BjtModel, vbe: f64, frequency: f64) -> ((f64, f64), f64) {
    let mut c = Circuit::new();
    let b = c.node("b");
    let col = c.node("c");
    let vcc = c.node("vcc");
    c.add(Element::new(
        "VBE",
        [b, GROUND],
        Device::VoltageSource(Source::ac(vbe, 1.0, 0.0)),
    ));
    c.add(Element::new("VCC", [vcc, GROUND], dc(10.0)));
    c.add(Element::new("RC", [vcc, col], Device::resistor(2e3)));
    c.add(Element::new(
        "Q1",
        [col, b, GROUND],
        Device::bjt(model.clone()),
    ));
    let ic = value(&op(&c), "Ic(Q1)");
    let spec = AcSweep {
        scale: SweepScale::Linear,
        points: 1,
        start: frequency,
        stop: frequency,
    };
    (
        complex(&run(&c, &Analysis::Ac(spec), &precise()), "V(c)", 0),
        ic,
    )
}

#[test]
fn small_signal_gain_is_minus_gm_rc() {
    // Ideal transistor: gm = Ic/Vt, no output conductance, no capacitances.
    let ((re, im), ic) = ce_gain(ideal(Polarity::N), 0.62, 1e3);
    let gm = (ic + 2e-15 + precise().gmin * (0.62 - 10.0 + ic * 2e3)) / thermal_voltage(27.0);
    assert_close(re, -gm * 2e3, 1e-6, 0.0, "CE gain");
    assert!(im.abs() < 1e-12);
}

#[test]
fn excess_phase_delays_transconductance() {
    // With PTF the AC transconductance is gm·e^(−jω·ptf·tf); TF alone only
    // loads the base, so the collector current carries just the rotation.
    let model = BjtModel {
        tf: 1e-9,
        ptf: 40.0,
        ..ideal(Polarity::N)
    };
    let f = 1e8;
    let ((re, im), _) = ce_gain(model, 0.62, f);
    let expected_phase = PI - 2.0 * PI * f * 40f64.to_radians() * 1e-9;
    assert_close(im.atan2(re), expected_phase, 1e-9, 0.0, "excess phase");
    let ((re0, im0), _) = ce_gain(
        BjtModel {
            tf: 1e-9,
            ..ideal(Polarity::N)
        },
        0.62,
        f,
    );
    assert_close(
        re.hypot(im),
        re0.hypot(im0),
        1e-9,
        0.0,
        "excess phase keeps |gm|",
    );
}

/// Collector admittance of a cut-off transistor with forced terminal
/// voltages, measured at `frequency` with an AC drive on the collector.
fn collector_admittance(model: BjtModel, vbe: f64, vce: f64, frequency: f64) -> (f64, f64) {
    let mut c = Circuit::new();
    let b = c.node("b");
    let col = c.node("c");
    c.add(Element::new("VBE", [b, GROUND], dc(vbe)));
    c.add(Element::new(
        "VCE",
        [col, GROUND],
        Device::VoltageSource(Source::ac(vce, 1.0, 0.0)),
    ));
    c.add(Element::new("Q1", [col, b, GROUND], Device::bjt(model)));
    let spec = AcSweep {
        scale: SweepScale::Linear,
        points: 1,
        start: frequency,
        stop: frequency,
    };
    let (re, im) = complex(&run(&c, &Analysis::Ac(spec), &precise()), "I(VCE)", 0);
    // I(VCE) flows into the source; the transistor draws the opposite.
    (-re, -im)
}

#[test]
fn junction_capacitances_load_the_collector() {
    // In cut-off the collector sees CJS to the substrate (ground), the
    // (1 − XCJC) part of CJC to the external base (AC ground) and the XCJC
    // part to the internal base, which reaches ground through RB.
    let (cjc, vjc, mjc, xcjc, cjs, vjs, mjs, rb) = (6e-12, 0.6, 0.4, 0.3, 3e-12, 0.7, 0.5, 200.0);
    let npn = BjtModel {
        cjc,
        vjc,
        mjc,
        xcjc,
        cjs,
        vjs,
        mjs,
        rb,
        rbm: rb,
        ..ideal(Polarity::N)
    };
    let (vbe, vce, f) = (-1.0, 5.0, 2e7);
    let w = 2.0 * PI * f;
    let gmin = precise().gmin;
    let cj = |c0: f64, v: f64, vj: f64, m: f64| c0 * (1.0 - v / vj).powf(-m);
    let vbc = vbe - vce;
    let c_inner = cj(cjc * xcjc, vbc, vjc, mjc);
    let c_outer = cj(cjc * (1.0 - xcjc), vbc, vjc, mjc);
    let c_substrate = cj(cjs, -vce, vjs, mjs);
    // The inner part (gmin + jωC towards c') in series with the internal
    // base's path to ground (1/RB, plus gmin across the reverse-biased B-E
    // junction to the grounded emitter).
    let (yr, yi) = (gmin, w * c_inner);
    let yb = 1.0 / rb + gmin;
    let den = (yr + yb).powi(2) + yi * yi;
    let series_re = (yr * yb * (yr + yb) + yi * yi * yb) / den;
    let series_im = yi * yb * yb / den;
    let expected_im = series_im + w * (c_outer + c_substrate);
    for (model, sign) in [
        (npn.clone(), 1.0),
        (
            BjtModel {
                polarity: Polarity::P,
                ..npn
            },
            -1.0,
        ),
    ] {
        let (re, im) = collector_admittance(model, sign * vbe, sign * vce, f);
        assert_close(im, expected_im, 1e-9, 0.0, "collector susceptance");
        assert_close(re, series_re, 1e-6, 0.0, "collector conductance");
    }
}

/// Relative agreement required with the LTspice BJT references.
///
/// The NPN_GP and PNP_GP stages agree to about 1e-9. The QN_EXT and QP_EXT
/// stages differ by up to 7e-8 at 10 MHz (Re V(c4); Re V(c3) 4.5e-8) and by
/// up to 3e-9 at the operating point. With IRB removed (constant RB) QN_EXT
/// agrees to 1.1e-9 at 10 MHz, so the residual comes from the IRB
/// base-resistance formula; the constants inside LTspice's closed
/// implementation of it could not be checked. 1e-7 still checks every
/// quantity to far better than six decimal places (Q153).
const BJT_LTSPICE_TOLERANCE: f64 = 1e-7;

/// The `QN_EXT` test transistor of `tests/reference/devices_bjt_*.cir` (not a
/// library part): the `NPN_GP` parameters plus a current-dependent base
/// resistance (IRB, RBM < RB), a split B-C capacitance (XCJC < 1), a
/// collector-substrate capacitance, non-ideal emission coefficients, reverse
/// Early and high-injection corners, B-C leakage and a bias-dependent transit
/// time.
fn extended_npn() -> BjtModel {
    BjtModel {
        name: "QN_EXT".into(),
        nf: 1.01,
        nr: 1.02,
        var: 20.0,
        ikr: 0.2,
        isc: 1e-13,
        nc: 2.0,
        rb: 60.0,
        irb: 100e-6,
        rbm: 8.0,
        xcjc: 0.6,
        cjs: 2e-12,
        vjs: 0.6,
        mjs: 0.4,
        xtf: 2.0,
        vtf: 10.0,
        itf: 50e-3,
        ..library::bjt("NPN_GP").unwrap()
    }
}

/// The `QP_EXT` test transistor of `tests/reference/devices_bjt_*.cir`: the
/// `PNP_GP` parameters extended like [`extended_npn`], without CJS.
fn extended_pnp() -> BjtModel {
    BjtModel {
        name: "QP_EXT".into(),
        nf: 1.01,
        nr: 1.02,
        var: 15.0,
        ikr: 0.15,
        isc: 1e-13,
        nc: 2.0,
        rb: 60.0,
        irb: 100e-6,
        rbm: 8.0,
        xcjc: 0.7,
        xtf: 2.0,
        vtf: 8.0,
        itf: 50e-3,
        ..library::bjt("PNP_GP").unwrap()
    }
}

/// The common-emitter stages of `tests/reference/devices_bjt_*.cir`.
fn reference_amplifier() -> Circuit {
    let mut c = Circuit::new();
    let [vcc, b, col, e, input, e2, b2, c2] =
        ["vcc", "b", "c", "e", "in", "e2", "b2", "c2"].map(|n| c.node(n));
    c.add(Element::new("VCC", [vcc, GROUND], dc(12.0)));
    c.add(Element::new("R1", [vcc, b], Device::resistor(47e3)));
    c.add(Element::new("R2", [b, GROUND], Device::resistor(10e3)));
    c.add(Element::new("RC", [vcc, col], Device::resistor(2.2e3)));
    c.add(Element::new("RE", [e, GROUND], Device::resistor(1e3)));
    c.add(Element::new("CE", [e, GROUND], Device::capacitor(100e-6)));
    c.add(Element::new("Cin", [input, b], Device::capacitor(10e-6)));
    c.add(Element::new(
        "Vin",
        [input, GROUND],
        Device::VoltageSource(Source::ac(0.0, 1.0, 0.0)),
    ));
    c.add(Element::new(
        "Q1",
        [col, b, e],
        Device::bjt(library::bjt("NPN_GP").unwrap()),
    ));
    c.add(Element::new("RE2", [vcc, e2], Device::resistor(1e3)));
    c.add(Element::new("R3", [vcc, b2], Device::resistor(10e3)));
    c.add(Element::new("R4", [b2, GROUND], Device::resistor(47e3)));
    c.add(Element::new("RC2", [c2, GROUND], Device::resistor(2.2e3)));
    c.add(Element::new(
        "Q2",
        [c2, b2, e2],
        Device::bjt(library::bjt("PNP_GP").unwrap()),
    ));
    // The test transistors split CJC (XCJC < 1) behind a current-dependent
    // base resistance, which exercises the external-base capacitance.
    let [b3, c3, e3, b4, c4, e4] = ["b3", "c3", "e3", "b4", "c4", "e4"].map(|n| c.node(n));
    c.add(Element::new("R5", [vcc, b3], Device::resistor(47e3)));
    c.add(Element::new("R6", [b3, GROUND], Device::resistor(10e3)));
    c.add(Element::new("RC3", [vcc, c3], Device::resistor(2.2e3)));
    c.add(Element::new("RE3", [e3, GROUND], Device::resistor(1e3)));
    c.add(Element::new("CE3", [e3, GROUND], Device::capacitor(100e-6)));
    c.add(Element::new("Cin3", [input, b3], Device::capacitor(10e-6)));
    c.add(Element::new(
        "Q3",
        [c3, b3, e3],
        Device::bjt(extended_npn()),
    ));
    c.add(Element::new("RE4", [vcc, e4], Device::resistor(1e3)));
    c.add(Element::new("CE4", [e4, vcc], Device::capacitor(100e-6)));
    c.add(Element::new("R7", [vcc, b4], Device::resistor(10e3)));
    c.add(Element::new("R8", [b4, GROUND], Device::resistor(47e3)));
    c.add(Element::new("RC4", [c4, GROUND], Device::resistor(2.2e3)));
    c.add(Element::new("Cin4", [input, b4], Device::capacitor(10e-6)));
    c.add(Element::new(
        "Q4",
        [c4, b4, e4],
        Device::bjt(extended_pnp()),
    ));
    c
}

#[test]
fn amplifier_operating_point_matches_ltspice() {
    // LTspice 24.1.9 batch run of tests/reference/devices_bjt_op.cir; LTspice
    // reports currents into the terminals, like xsim.
    let reference = [
        ("V(b)", 2.04892168706),
        ("V(c)", 8.95599006262),
        ("V(e)", 1.39047378241),
        ("Ic(Q1)", 0.00138364088063),
        ("Ib(Q1)", 6.83290178242e-06),
        ("Ie(Q1)", -0.00139047378241),
        ("V(b2)", 9.95660684328),
        ("V(c2)", 3.03147293816),
        ("V(e2)", 10.6145543723),
        ("Ic(Q2)", -0.00137794224462),
        ("Ib(Q2)", -7.50338312137e-06),
        ("Ie(Q2)", 0.00138544562774),
        ("V(b3)", 2.04667153115),
        ("V(c3)", 8.97764441779),
        ("V(e3)", 1.38090377597),
        ("Ic(Q3)", 0.00137379799191),
        ("Ib(Q3)", 7.10579303126e-06),
        ("Ie(Q3)", -0.00138090377597),
        ("V(b4)", 9.95978520632),
        ("V(c4)", 3.00699352145),
        ("V(e4)", 10.6252959188),
        ("Ic(Q4)", -0.00136681523702),
        ("Ib(Q4)", -7.88884417038e-06),
        ("Ie(Q4)", 0.00137470408119),
    ];
    let r = op(&reference_amplifier());
    for (name, expected) in reference {
        assert_close(
            value(&r, name),
            expected,
            BJT_LTSPICE_TOLERANCE,
            1e-15,
            name,
        );
    }
}

/// Converts an LTspice `re()`/`im()` measurement printed as `(dB, 0°|180°)`.
fn from_db(db: f64, degrees: f64) -> f64 {
    10f64.powf(db / 20.0) * if degrees == 180.0 { -1.0 } else { 1.0 }
}

#[test]
fn amplifier_small_signal_response_matches_ltspice() {
    // LTspice 24.1.9 batch run of tests/reference/devices_bjt_ac.cir.
    let reference = [
        (
            "V(c)",
            1e3,
            from_db(40.6649428412, 180.0),
            from_db(19.2393140572, 180.0),
        ),
        (
            "V(c)",
            1e7,
            from_db(35.8047160449, 180.0),
            from_db(34.9307067473, 0.0),
        ),
        (
            "V(c3)",
            1e3,
            from_db(40.4705328807, 180.0),
            from_db(18.8726478242, 180.0),
        ),
        (
            "V(c3)",
            1e7,
            from_db(35.5019524859, 180.0),
            from_db(34.972086172, 0.0),
        ),
        (
            "V(c4)",
            1e3,
            from_db(40.3544926664, 180.0),
            from_db(18.6939688999, 180.0),
        ),
        (
            "V(c4)",
            1e7,
            from_db(32.1482769992, 180.0),
            from_db(34.6871559366, 0.0),
        ),
    ];
    let circuit = reference_amplifier();
    for (node, f, re, im) in reference {
        let spec = AcSweep {
            scale: SweepScale::Linear,
            points: 1,
            start: f,
            stop: f,
        };
        let (got_re, got_im) = complex(&run(&circuit, &Analysis::Ac(spec), &precise()), node, 0);
        assert_close(
            got_re,
            re,
            BJT_LTSPICE_TOLERANCE,
            0.0,
            &format!("Re {node} at {f} Hz"),
        );
        assert_close(
            got_im,
            im,
            BJT_LTSPICE_TOLERANCE,
            0.0,
            &format!("Im {node} at {f} Hz"),
        );
    }
}

#[test]
fn saturated_switch_transient_keeps_kcl() {
    // An inverter driven hard into saturation and back: the junction
    // charges (CJE, the XCJC-split CJC behind RB, TF, TR) are integrated and
    // the terminal currents, charging currents included, still sum to zero
    // (CJS = 0, so no current leaves through the substrate).
    let mut c = Circuit::new();
    let [vcc, input, b, col] = ["vcc", "in", "b", "c"].map(|n| c.node(n));
    let pulse = Waveform::Pulse {
        initial: 0.0,
        pulsed: 5.0,
        delay: 0.2e-6,
        rise: 10e-9,
        fall: 10e-9,
        width: 1e-6,
        period: 0.0,
        cycles: None,
    };
    c.add(Element::new("VCC", [vcc, GROUND], dc(5.0)));
    c.add(Element::new(
        "VIN",
        [input, GROUND],
        Device::VoltageSource(Source::with_waveform(pulse)),
    ));
    c.add(Element::new("RB", [input, b], Device::resistor(1e3)));
    c.add(Element::new("RC", [vcc, col], Device::resistor(500.0)));
    c.add(Element::new(
        "Q1",
        [col, b, GROUND],
        Device::bjt(BjtModel {
            cjs: 0.0,
            ..extended_npn()
        }),
    ));
    let spec = Analysis::Transient(Transient {
        stop: 2e-6,
        step: 5e-9,
        start_save: 0.0,
        max_step: Some(5e-9),
        initial_state: InitialState::OperatingPoint,
    });
    let r = run(&c, &spec, &SimOptions::default());
    let ic = r.signal("Ic(Q1)").unwrap().real().unwrap();
    let ib = r.signal("Ib(Q1)").unwrap().real().unwrap();
    let ie = r.signal("Ie(Q1)").unwrap().real().unwrap();
    let vc = r.signal("V(c)").unwrap().real().unwrap();
    for k in 0..r.len() {
        let scale = ic[k].abs().max(ib[k].abs()).max(ie[k].abs());
        assert!(
            (ic[k] + ib[k] + ie[k]).abs() <= 1e-9 * scale + 1e-15,
            "KCL at sample {k}"
        );
    }
    let at = |t: f64| vc[r.axis.values.iter().position(|&s| s >= t).unwrap()];
    assert!(at(0.1e-6) > 4.99, "off before the pulse");
    assert!(at(1.0e-6) < 0.3, "saturated during the pulse");
    assert!(at(1.99e-6) > 4.9, "off again after storage and fall");
}

#[test]
fn tiny_saturation_current_is_exact_far_above_exponent_80() {
    // IS = 1e-38 puts the B-E exponent near 81 at IC = 1 mA, where the old
    // exponent cap raised V(b) by 5.5 mV. LTspice 24.1.9 batch run of
    // tests/reference/devices_bjt_lowis.cir: V(b) = 2.08446673612 V,
    // Ic = 0.999999884021 mA.
    let model = BjtModel {
        is: 1e-38,
        bf: 100.0,
        ..BjtModel::default()
    };
    let mut c = Circuit::new();
    let (b, col) = (c.node("b"), c.node("c"));
    c.add(Element::new(
        "I1",
        [GROUND, b],
        Device::CurrentSource(Source::dc(10e-6)),
    ));
    c.add(Element::new("VC", [col, GROUND], dc(3.0)));
    c.add(Element::new("Q1", [col, b, GROUND], Device::bjt(model)));
    let r = op(&c);
    assert_close(value(&r, "V(b)"), 2.084_466_736_12, 1e-10, 0.0, "V(b)");
    assert_close(value(&r, "Ic(Q1)"), 0.999_999_884_021e-3, 1e-9, 0.0, "Ic");
    // Closed form: IB = IS/BF·(e^(vbe/Vt) − 1) plus gmin and the reverse
    // biased B-C junction, so IC = IS·(e^(vbe/Vt) − 1) − (1 + 1/BR)·cbc.
    let vt = thermal_voltage(27.0);
    let vbe = value(&r, "V(b)");
    let gmin = SimOptions::default().gmin;
    let ib = 1e-38 / 100.0 * (vbe / vt).exp_m1() + gmin * vbe + gmin * (vbe - 3.0);
    assert_close(ib, 10e-6, 1e-9, 0.0, "base current");
}

/// Substrate capacitance of a transistor with only CJS (collector at 5 V,
/// base and emitter grounded), from the collector's AC current at 1 MHz.
fn substrate_capacitance(celsius: f64) -> f64 {
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
    let analysis = Analysis::Ac(AcSweep {
        scale: SweepScale::Linear,
        points: 1,
        start: f,
        stop: f,
    });
    let options = SimOptions {
        temperature: celsius,
        ..precise()
    };
    let (_, im) = complex(&run(&c, &analysis, &options), "Ic(Q1)", 0);
    im / (2.0 * PI * f)
}

#[test]
fn substrate_capacitance_keeps_its_model_values_at_every_temperature() {
    // SPICE3 and LTspice do not temperature-adjust CJS and VJS. LTspice
    // 24.1.9, VC c 0 DC 5 AC 1 / Q1 c 0 0 QS / .model QS NPN(CJS=2p VJS=0.7
    // MJS=0.3) at 0, 27 and 100 °C (scaled for its kT/q): im(Ic) =
    // 6.6984885496e-6 A at 1 MHz every time, i.e. cjs·(1 + 5/vjs)^(−mjs).
    let expected = 2e-12 * (1.0 + 5.0 / 0.7_f64).powf(-0.3);
    assert_close(
        expected * 2.0 * PI * 1e6,
        6.698_488_549_6e-6,
        1e-10,
        0.0,
        "LTspice",
    );
    for celsius in [0.0, 27.0, 100.0] {
        let c = substrate_capacitance(celsius);
        assert_close(c, expected, 1e-12, 0.0, &format!("CJS at {celsius} °C"));
    }
}
