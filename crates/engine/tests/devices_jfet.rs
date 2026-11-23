//! Full-simulation tests of the Shichman–Hodges JFET: closed-form channel
//! currents in every region, the doping-tail parameter, temperature, small-
//! signal transconductance and gate capacitance, polarity symmetry, KCL and
//! LTspice cross-checks.

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

fn complex(result: &SimulationResult, name: &str) -> (f64, f64) {
    match &result
        .signal(name)
        .unwrap_or_else(|| panic!("no signal {name}"))
        .data
    {
        SignalData::Complex { re, im } => (re[0], im[0]),
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

/// JFET with gate-source and drain-source voltages forced (source grounded).
fn forced(model: JfetModel, vgs: Source, vds: f64) -> Circuit {
    let mut c = Circuit::new();
    let g = c.node("g");
    let d = c.node("d");
    c.add(Element::new("VGS", [g, GROUND], Device::VoltageSource(vgs)));
    c.add(Element::new("VDS", [d, GROUND], dc(vds)));
    c.add(Element::new("J1", [d, g, GROUND], Device::jfet(model)));
    c
}

/// Gate-junction current `is·(e^(v/Vt) − 1) + gmin·v`.
fn gate_diode(is: f64, v: f64) -> f64 {
    is * (v / thermal_voltage(27.0)).exp_m1() + precise().gmin * v
}

/// Classic Shichman–Hodges drain current for `vds ≥ 0`.
fn square_law(m: &JfetModel, vgs: f64, vds: f64) -> f64 {
    let vgst = vgs - m.vto;
    let beta = m.beta * (1.0 + m.lambda * vds);
    if vgst <= 0.0 {
        0.0
    } else if vds < vgst {
        beta * vds * (2.0 * vgst - vds)
    } else {
        beta * vgst * vgst
    }
}

fn library_jfet(name: &str) -> JfetModel {
    library::jfet(name).unwrap_or_else(|| panic!("{name} missing from the library"))
}

/// A test JFET (not a library part) for the closed-form checks: no series
/// resistances, so the terminal voltages are the channel voltages.
fn test_jfet() -> JfetModel {
    JfetModel {
        name: "JTEST".into(),
        vto: -3.0,
        beta: 1.2e-3,
        lambda: 0.015,
        cgs: 2e-12,
        cgd: 1e-12,
        pb: 0.9,
        is: 1e-14,
        ..JfetModel::default()
    }
}

#[test]
fn drain_current_follows_square_law_in_every_region() {
    let m = test_jfet();
    for &(vgs, vds) in &[
        (-1.0, 10.0),
        (-2.5, 6.0),
        (0.0, 0.5),
        (-1.5, 1.0),
        (-4.0, 5.0),
        (0.2, 3.0),
    ] {
        let r = op(&forced(m.clone(), Source::dc(vgs), vds));
        let vgd = vgs - vds;
        let expected = square_law(&m, vgs, vds) - gate_diode(m.is, vgd);
        assert_close(
            value(&r, "Id(J1)"),
            expected,
            1e-9,
            1e-20,
            &format!("Id at vgs={vgs} vds={vds}"),
        );
        let gate = gate_diode(m.is, vgs) + gate_diode(m.is, vgd);
        assert_close(
            value(&r, "Ig(J1)"),
            gate,
            1e-9,
            1e-22,
            &format!("Ig at vgs={vgs} vds={vds}"),
        );
    }
}

#[test]
fn reversed_drain_and_source_mirror_the_current() {
    // With vds < 0 the drain acts as source: Id(vgs, vds) = −F(vgd, −vds).
    // The gate sits below both channel ends so neither junction conducts.
    let m = test_jfet();
    let (vgs, vds) = (-2.5, -2.0);
    let r = op(&forced(m.clone(), Source::dc(vgs), vds));
    let vgd = vgs - vds;
    let expected = -square_law(&m, vgd, -vds) - gate_diode(m.is, vgd);
    assert_close(
        value(&r, "Id(J1)"),
        expected,
        1e-9,
        0.0,
        "inverse-mode drain current",
    );
}

#[test]
fn doping_tail_parameter_shapes_saturation_current() {
    let m = JfetModel {
        vto: -2.0,
        beta: 1e-3,
        lambda: 0.01,
        pb: 0.8,
        b: 0.7,
        ..JfetModel::default()
    };
    let (vgs, vds) = (-0.8, 8.0);
    let r = op(&forced(m.clone(), Source::dc(vgs), vds));
    let vgst = vgs + 2.0;
    let tail = (1.0 - 0.7) / (0.8 + 2.0);
    let channel = 1e-3 * (1.0 + 0.01 * vds) * vgst * vgst * (0.7 + tail * vgst);
    assert_close(
        value(&r, "Id(J1)"),
        channel - gate_diode(m.is, vgs - vds),
        1e-9,
        0.0,
        "doping-tail current",
    );
}

#[test]
fn temperature_shifts_threshold_and_beta() {
    let m = JfetModel {
        vtotc: -2.5e-3,
        betatce: -0.5,
        ..test_jfet()
    };
    let celsius = 85.0;
    let options = SimOptions {
        temperature: celsius,
        ..precise()
    };
    let r = run(
        &forced(m.clone(), Source::dc(-1.0), 10.0),
        &Analysis::OperatingPoint,
        &options,
    );
    let dt = celsius - 27.0;
    let hot = JfetModel {
        vto: m.vto + m.vtotc * dt,
        beta: m.beta * 1.01f64.powf(m.betatce * dt),
        ..m.clone()
    };
    // Gate leakage follows the PSpice law with EG = 1.11 eV and XTI = 3.
    let (t, tnom) = (celsius + 273.15, 300.15);
    let vt = thermal_voltage(celsius);
    let is = m.is * ((t / tnom - 1.0) * 1.11 / vt + 3.0 * (t / tnom).ln()).exp();
    let vgd = -11.0;
    let leakage = is * (vgd / vt).exp_m1() + options.gmin * vgd;
    assert_close(
        value(&r, "Id(J1)"),
        square_law(&hot, -1.0, 10.0) - leakage,
        1e-9,
        0.0,
        "Id at 85 °C",
    );
}

#[test]
fn p_channel_mirrors_n_channel() {
    let n = test_jfet();
    let p = JfetModel {
        polarity: Polarity::P,
        ..n.clone()
    };
    let rn = op(&forced(n, Source::dc(-1.2), 7.0));
    let rp = op(&forced(p, Source::dc(1.2), -7.0));
    for name in ["Id(J1)", "Ig(J1)", "Is(J1)"] {
        assert_close(value(&rp, name), -value(&rn, name), 1e-12, 0.0, name);
    }
}

#[test]
fn terminal_currents_obey_kcl_with_series_resistances() {
    let m = JfetModel {
        rd: 15.0,
        rs: 22.0,
        ..test_jfet()
    };
    let sweep = Analysis::DcSweep(DcSweep {
        source: "VGS".into(),
        start: -4.0,
        stop: 0.5,
        step: 0.25,
    });
    let r = run(&forced(m, Source::dc(0.0), 10.0), &sweep, &precise());
    let id = r.signal("Id(J1)").unwrap().real().unwrap();
    let ig = r.signal("Ig(J1)").unwrap().real().unwrap();
    let is = r.signal("Is(J1)").unwrap().real().unwrap();
    for k in 0..r.len() {
        let sum = id[k] + ig[k] + is[k];
        assert!(
            sum.abs() <= 1e-12 * id[k].abs().max(ig[k].abs()) + 1e-24,
            "KCL at point {k}: {sum:e}"
        );
    }
}

#[test]
fn small_signal_transconductance_and_gate_capacitance() {
    // AC drive on the gate with the drain held: the drain-source current is
    // gm·vgs with gm = 2β(1 + λ·vds)(vgs − vto); the gate draws
    // jω(Cgs + Cgd) plus the junction conductances.
    let m = JfetModel {
        cgs: 3e-12,
        cgd: 1.5e-12,
        ..test_jfet()
    };
    let (vgs, vds, f) = (-1.5, 10.0, 1e6);
    let spec = Analysis::Ac(AcSweep {
        scale: SweepScale::Linear,
        points: 1,
        start: f,
        stop: f,
    });
    let r = run(
        &forced(m.clone(), Source::ac(vgs, 1.0, 0.0), vds),
        &spec,
        &precise(),
    );
    let gm = 2.0 * m.beta * (1.0 + m.lambda * vds) * (vgs - m.vto);
    let vt = thermal_voltage(27.0);
    let ggd = m.is * ((vgs - vds) / vt).exp() / vt + precise().gmin;
    let cgd = 1.5e-12 / (1.0 - (vgs - vds) / m.pb).sqrt();
    let cgs = 3e-12 / (1.0 - vgs / m.pb).sqrt();
    let w = 2.0 * PI * f;
    // VDS supplies the drain current: I(VDS) = −(gm − ggd − jω·cgd)·vgs.
    let (re, im) = complex(&r, "I(VDS)");
    assert_close(re, -(gm - ggd), 1e-9, 0.0, "drain transconductance");
    assert_close(im, w * cgd, 1e-9, 0.0, "gate-drain feedthrough");
    let ggs = m.is * (vgs / vt).exp() / vt + precise().gmin;
    let (re, im) = complex(&r, "I(VGS)");
    assert_close(re, -(ggs + ggd), 1e-9, 0.0, "gate conductance");
    assert_close(im, -w * (cgs + cgd), 1e-9, 0.0, "gate capacitance");
}

/// The self-biased stages of `tests/reference/devices_jfet_*.cir`.
fn reference_stages() -> Circuit {
    let mut c = Circuit::new();
    let [dd, d, s, g, input, ss, d2, s2, g2] =
        ["dd", "d", "s", "g", "in", "ss", "d2", "s2", "g2"].map(|n| c.node(n));
    c.add(Element::new("VDD", [dd, GROUND], dc(15.0)));
    c.add(Element::new("RD", [dd, d], Device::resistor(2.2e3)));
    c.add(Element::new("RS", [s, GROUND], Device::resistor(1e3)));
    c.add(Element::new("CS", [s, GROUND], Device::capacitor(100e-6)));
    c.add(Element::new("RG", [g, GROUND], Device::resistor(1e6)));
    c.add(Element::new("Cin", [input, g], Device::capacitor(1e-6)));
    c.add(Element::new(
        "Vin",
        [input, GROUND],
        Device::VoltageSource(Source::ac(0.0, 1.0, 0.0)),
    ));
    c.add(Element::new(
        "J1",
        [d, g, s],
        Device::jfet(library_jfet("NJF_MID")),
    ));
    c.add(Element::new("VSS", [ss, GROUND], dc(-15.0)));
    c.add(Element::new("RD2", [d2, ss], Device::resistor(2.2e3)));
    c.add(Element::new("RS2", [s2, GROUND], Device::resistor(1e3)));
    c.add(Element::new("RG2", [g2, GROUND], Device::resistor(1e6)));
    c.add(Element::new(
        "J2",
        [d2, g2, s2],
        Device::jfet(library_jfet("PJF_MID")),
    ));
    // NJF_HIGH has drain and source resistances.
    let [d3, s3, g3] = ["d3", "s3", "g3"].map(|n| c.node(n));
    c.add(Element::new("RD3", [dd, d3], Device::resistor(1e3)));
    c.add(Element::new("RS3", [s3, GROUND], Device::resistor(220.0)));
    c.add(Element::new("CS3", [s3, GROUND], Device::capacitor(100e-6)));
    c.add(Element::new("RG3", [g3, GROUND], Device::resistor(1e6)));
    c.add(Element::new("Cin3", [input, g3], Device::capacitor(1e-6)));
    c.add(Element::new(
        "J3",
        [d3, g3, s3],
        Device::jfet(library_jfet("NJF_HIGH")),
    ));
    c
}

#[test]
fn self_biased_stages_match_ltspice() {
    // LTspice 24.1.9 batch run of tests/reference/devices_jfet_op.cir.
    let reference = [
        ("V(s)", 1.08406515104),
        ("V(d)", 12.6150566375),
        ("V(g)", 1.37190943504e-05),
        ("Id(J1)", 0.00108406516475),
        ("Ig(J1)", -1.37190943504e-11),
        ("Is(J1)", -0.00108406515104),
        ("V(s2)", -1.08406516486),
        ("V(d2)", -12.6150566071),
        ("V(g2)", -1.37390942938e-05),
        ("Id(J2)", -0.0010840651786),
        ("Ig(J2)", 1.37390942938e-11),
        ("Is(J2)", 0.00108406516486),
        ("V(s3)", 1.33230213254),
        ("V(d3)", 8.94408120533),
        ("V(g3)", 1.03763625851e-05),
        ("Id(J3)", 0.00605591879467),
        ("Ig(J3)", -1.03763625851e-11),
        ("Is(J3)", -0.00605591878429),
    ];
    let r = op(&reference_stages());
    for (name, expected) in reference {
        assert_close(value(&r, name), expected, 2e-9, 1e-15, name);
    }
}

/// Converts an LTspice `re()`/`im()` measurement printed as `(dB, 0°|180°)`.
fn from_db(db: f64, degrees: f64) -> f64 {
    10f64.powf(db / 20.0) * if degrees == 180.0 { -1.0 } else { 1.0 }
}

#[test]
fn common_source_response_matches_ltspice() {
    // LTspice 24.1.9 batch run of tests/reference/devices_jfet_ac.cir.
    let reference = [
        (
            "V(d)",
            1e3,
            from_db(14.0022212098, 180.0),
            from_db(-34.3860735347, 180.0),
        ),
        (
            "V(d)",
            1e7,
            from_db(13.977661777, 180.0),
            from_db(-10.6799525386, 0.0),
        ),
        (
            "V(d3)",
            1e3,
            from_db(16.0482080529, 180.0),
            from_db(-23.5888443246, 180.0),
        ),
        (
            "V(d3)",
            1e7,
            from_db(16.033045128, 180.0),
            from_db(-10.3792756601, 0.0),
        ),
    ];
    let circuit = reference_stages();
    for (node, f, re, im) in reference {
        let spec = Analysis::Ac(AcSweep {
            scale: SweepScale::Linear,
            points: 1,
            start: f,
            stop: f,
        });
        let (got_re, got_im) = complex(&run(&circuit, &spec, &precise()), node);
        assert_close(got_re, re, 1e-8, 0.0, &format!("Re {node} at {f} Hz"));
        assert_close(got_im, im, 1e-8, 0.0, &format!("Im {node} at {f} Hz"));
    }
}
