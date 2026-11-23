//! Full-simulation tests of the junction diode: DC against the closed-form
//! Shockley equation and LTspice, breakdown, temperature, small-signal
//! capacitances, transient rectification and charge storage, and a hard
//! convergence case.

use xsim_spice_engine::*;

const BOLTZMANN: f64 = 1.380_649e-23;
const CHARGE: f64 = 1.602_176_634e-19;

fn thermal_voltage(celsius: f64) -> f64 {
    BOLTZMANN * (celsius + 273.15) / CHARGE
}

/// Tight solver tolerances so the comparisons test the model, not the
/// Newton stopping criterion.
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

fn op_value(result: &SimulationResult, name: &str) -> f64 {
    result
        .signal(name)
        .unwrap_or_else(|| panic!("no signal {name}"))
        .real()
        .unwrap()[0]
}

fn assert_close(actual: f64, expected: f64, rel: f64, abs: f64, what: &str) {
    let err = (actual - expected).abs();
    assert!(
        err <= rel * expected.abs() + abs,
        "{what}: got {actual:.12e}, expected {expected:.12e} (error {err:.3e})"
    );
}

/// Junction current `is·(e^(v/nVt) − 1) + gmin·v` as used by SPICE.
fn shockley(model: &DiodeModel, v: f64, celsius: f64, gmin: f64) -> f64 {
    model.is * (v / (model.n * thermal_voltage(celsius))).exp_m1() + gmin * v
}

/// Solves the monotonic equation `f(x) = 0` on `[lo, hi]` by bisection.
fn bisect(f: impl Fn(f64) -> f64, mut lo: f64, mut hi: f64) -> f64 {
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if f(mid) > 0.0 { hi = mid } else { lo = mid }
    }
    0.5 * (lo + hi)
}

fn library_diode(name: &str) -> DiodeModel {
    library::diode(name).unwrap_or_else(|| panic!("{name} missing from the library"))
}

/// A test signal diode (not a library part): series resistance, emission
/// coefficient above 1 and no breakdown, so the closed-form Shockley law
/// describes it exactly.
fn signal_diode() -> DiodeModel {
    DiodeModel {
        name: "DSIG_TEST".into(),
        is: 2e-9,
        n: 1.8,
        rs: 0.6,
        ..DiodeModel::default()
    }
}

/// A test rectifier (not a library part) with junction and diffusion
/// charge and no breakdown.
fn rectifier_diode() -> DiodeModel {
    DiodeModel {
        name: "DRECT_TEST".into(),
        is: 1e-8,
        n: 1.8,
        rs: 0.05,
        cjo: 20e-12,
        vj: 0.7,
        m: 0.4,
        tt: 2e-6,
        ..DiodeModel::default()
    }
}

/// `V1 → R → diode → ground`; returns the circuit and the diode node.
fn resistor_diode(supply: f64, resistance: f64, model: DiodeModel) -> Circuit {
    let mut c = Circuit::new();
    let a = c.node("a");
    let k = c.node("k");
    c.add(Element::new(
        "V1",
        [a, GROUND],
        Device::VoltageSource(Source::dc(supply)),
    ));
    c.add(Element::new("R1", [a, k], Device::resistor(resistance)));
    c.add(Element::new("D1", [k, GROUND], Device::diode(model)));
    c
}

#[test]
fn forward_drop_satisfies_shockley_and_ohms_law() {
    let model = signal_diode();
    let options = precise();
    for supply in [0.6, 1.0, 2.0, 5.0, 10.0, 20.0] {
        let r = run(
            &resistor_diode(supply, 1e3, model.clone()),
            &Analysis::OperatingPoint,
            &options,
        );
        let vk = op_value(&r, "V(k)");
        let current = (supply - vk) / 1e3;
        // The internal junction sits behind the series resistance.
        let junction = vk - current * model.rs;
        let expected = shockley(&model, junction, 27.0, options.gmin);
        assert_close(
            current,
            expected,
            1e-9,
            1e-18,
            &format!("diode current at {supply} V"),
        );
        assert_close(
            op_value(&r, "I(D1)"),
            current,
            1e-9,
            1e-18,
            "reported diode current",
        );
    }
}

#[test]
fn forward_drop_matches_ltspice() {
    // LTspice 24.1.9 batch run of tests/reference/devices_diode_dc.cir
    // (.meas vk_1 … vk_20), library D_SIGNAL parameters.
    let reference = [
        (1.0, 0.585303942338),
        (2.0, 0.634987681426),
        (5.0, 0.683809671602),
        (10.0, 0.717486288856),
        (20.0, 0.751653586736),
    ];
    for (supply, expected) in reference {
        let r = run(
            &resistor_diode(supply, 1e3, library_diode("D_SIGNAL")),
            &Analysis::OperatingPoint,
            &precise(),
        );
        assert_close(
            op_value(&r, "V(k)"),
            expected,
            0.0,
            1e-9,
            &format!("V(k) at {supply} V"),
        );
    }
}

/// The breakdown test diode of `devices_diode_dc.cir`.
fn breakdown_diode() -> DiodeModel {
    DiodeModel {
        name: "DZ".into(),
        is: 1e-14,
        n: 1.0,
        bv: 5.1,
        ibv: 1e-3,
        nbv: 1.0,
        ..DiodeModel::default()
    }
}

/// `V1 → 1 kΩ → cathode`, anode grounded (reverse-biased by a positive supply).
fn regulator(supply: f64, model: DiodeModel) -> Circuit {
    let mut c = Circuit::new();
    let a = c.node("a");
    let k = c.node("kz");
    c.add(Element::new(
        "V1",
        [a, GROUND],
        Device::VoltageSource(Source::dc(supply)),
    ));
    c.add(Element::new("R2", [a, k], Device::resistor(1e3)));
    c.add(Element::new("D2", [GROUND, k], Device::diode(model)));
    c
}

#[test]
fn breakdown_matches_ltspice() {
    // LTspice 24.1.9 batch run of tests/reference/devices_diode_dc.cir
    // (.meas vz_2 … vz_20). The residual differences (< 1e-9 V) are
    // LTspice's own Newton tolerance at reltol = 1e-9.
    let reference = [
        (2.0, 1.99999999799),
        (5.0, 4.98722402649),
        (6.0, 5.09735087869),
        (10.0, 5.14088871177),
        (20.0, 5.16974914539),
    ];
    for (supply, expected) in reference {
        let r = run(
            &regulator(supply, breakdown_diode()),
            &Analysis::OperatingPoint,
            &precise(),
        );
        assert_close(
            op_value(&r, "V(kz)"),
            expected,
            0.0,
            2e-9,
            &format!("V(kz) at {supply} V"),
        );
    }
}

#[test]
fn zener_with_soft_knee_matches_ltspice() {
    // LTspice 24.1.9 batch run of tests/reference/devices_diode_dc.cir
    // (.meas vz3_2 … vz3_20): the library DZ_5V1, whose NBV = 2.7 and
    // RS = 1 Ω shape the knee and the slope above it.
    let reference = [
        (2.0, 1.99999999799),
        (5.0, 4.8501830288),
        (6.0, 4.98468140692),
        (10.0, 5.09851183247),
        (20.0, 5.18566645279),
    ];
    for (supply, expected) in reference {
        let r = run(
            &regulator(supply, library_diode("DZ_5V1")),
            &Analysis::OperatingPoint,
            &precise(),
        );
        assert_close(
            op_value(&r, "V(kz)"),
            expected,
            0.0,
            2e-9,
            &format!("V(kz3) at {supply} V"),
        );
    }
}

#[test]
fn breakdown_current_at_bv_is_ibv() {
    // Forcing exactly IBV in reverse must produce exactly BV across the
    // junction (up to the gmin current, which shifts it by Vt·gmin·BV/IBV).
    let mut c = Circuit::new();
    let k = c.node("k");
    c.add(Element::new(
        "I1",
        [GROUND, k],
        Device::CurrentSource(Source::dc(1e-3)),
    ));
    c.add(Element::new(
        "D1",
        [GROUND, k],
        Device::diode(breakdown_diode()),
    ));
    let r = run(&c, &Analysis::OperatingPoint, &precise());
    assert_close(
        op_value(&r, "V(k)"),
        5.1,
        0.0,
        1e-8,
        "breakdown voltage at IBV",
    );
}

#[test]
fn zener_regulator_line_regulation() {
    // Above breakdown the output moves by only a few thermal voltages per
    // decade of current while the supply varies widely.
    let options = precise();
    let outputs: Vec<f64> = [8.0, 12.0, 20.0]
        .iter()
        .map(|&s| {
            op_value(
                &run(
                    &regulator(s, breakdown_diode()),
                    &Analysis::OperatingPoint,
                    &options,
                ),
                "V(kz)",
            )
        })
        .collect();
    let vt = thermal_voltage(27.0);
    for (s, v) in [8.0, 12.0, 20.0].iter().zip(&outputs) {
        // I = (S − V)/R and V = BV + Vt·ln(I/IBV) once the forward term is negligible.
        let current = (s - v) / 1e3;
        assert_close(
            *v,
            5.1 + vt * (current / 1e-3).ln(),
            0.0,
            1e-8,
            &format!("regulated output at {s} V"),
        );
    }
    assert!(outputs[2] - outputs[0] < 0.05);
}

#[test]
fn reverse_leakage_is_saturation_current_plus_gmin() {
    let model = DiodeModel {
        is: 2e-12,
        n: 1.5,
        ..DiodeModel::default()
    };
    let mut c = Circuit::new();
    let a = c.node("a");
    c.add(Element::new(
        "V1",
        [a, GROUND],
        Device::VoltageSource(Source::dc(-5.0)),
    ));
    c.add(Element::new(
        "D1",
        [a, GROUND],
        Device::diode(model.clone()),
    ));
    let options = precise();
    let r = run(&c, &Analysis::OperatingPoint, &options);
    assert_close(
        op_value(&r, "I(D1)"),
        shockley(&model, -5.0, 27.0, options.gmin),
        1e-12,
        0.0,
        "leakage",
    );
}

#[test]
fn forward_drop_follows_temperature_equations() {
    // Current-driven test diode at three temperatures against the SPICE3 law
    // IS(T) = IS·exp((T/Tnom − 1)·EG/(N·Vt) + XTI/N·ln(T/Tnom)).
    let model = signal_diode();
    let current = 1e-3;
    let mut previous = f64::INFINITY;
    for celsius in [-20.0, 27.0, 100.0] {
        let mut c = Circuit::new();
        let k = c.node("k");
        c.add(Element::new(
            "I1",
            [GROUND, k],
            Device::CurrentSource(Source::dc(current)),
        ));
        c.add(Element::new(
            "D1",
            [k, GROUND],
            Device::diode(model.clone()),
        ));
        let options = SimOptions {
            temperature: celsius,
            ..precise()
        };
        let v = op_value(&run(&c, &Analysis::OperatingPoint, &options), "V(k)");

        let (t, tnom) = (celsius + 273.15, 300.15);
        let nvt = model.n * thermal_voltage(celsius);
        let is = model.is
            * ((t / tnom - 1.0) * model.eg / nvt + model.xti / model.n * (t / tnom).ln()).exp();
        let junction = bisect(
            |vj| is * (vj / nvt).exp_m1() + options.gmin * vj - current,
            0.0,
            2.0,
        );
        assert_close(
            v,
            junction + current * model.rs,
            0.0,
            1e-9,
            &format!("V(k) at {celsius} °C"),
        );
        assert!(v < previous, "forward drop must fall with temperature");
        previous = v;
    }
}

#[test]
fn parallel_diodes_equal_one_diode_of_double_area() {
    let model = signal_diode();
    let options = precise();
    let single = {
        let mut c = resistor_diode(5.0, 100.0, model.clone());
        c.elements[2].device = Device::Diode {
            model: model.clone(),
            area: 2.0,
        };
        op_value(&run(&c, &Analysis::OperatingPoint, &options), "V(k)")
    };
    let pair = {
        let mut c = resistor_diode(5.0, 100.0, model.clone());
        let k = c.find_node("k").unwrap();
        c.add(Element::new("D2", [k, GROUND], Device::diode(model)));
        op_value(&run(&c, &Analysis::OperatingPoint, &options), "V(k)")
    };
    assert_close(single, pair, 0.0, 1e-10, "area scaling");
}

/// `V1 (DC bias + 1 V AC) → diode → ground`, returning the complex diode
/// current at `frequency`.
fn diode_admittance(model: DiodeModel, bias: f64, frequency: f64) -> (f64, f64) {
    let mut c = Circuit::new();
    let a = c.node("a");
    c.add(Element::new(
        "V1",
        [a, GROUND],
        Device::VoltageSource(Source::ac(bias, 1.0, 0.0)),
    ));
    c.add(Element::new("D1", [a, GROUND], Device::diode(model)));
    let spec = AcSweep {
        scale: SweepScale::Linear,
        points: 1,
        start: frequency,
        stop: frequency,
    };
    let r = run(&c, &Analysis::Ac(spec), &precise());
    match &r.signal("I(D1)").unwrap().data {
        SignalData::Complex { re, im } => (re[0], im[0]),
        SignalData::Real(_) => panic!("AC result must be complex"),
    }
}

#[test]
fn reverse_bias_admittance_is_depletion_capacitance() {
    let model = DiodeModel {
        cjo: 10e-12,
        vj: 0.7,
        m: 0.45,
        ..DiodeModel::default()
    };
    let f = 1e6;
    let (g, b) = diode_admittance(model, -5.0, f);
    let expected_c = 10e-12 * (1.0 + 5.0 / 0.7f64).powf(-0.45);
    assert_close(
        b / (2.0 * std::f64::consts::PI * f),
        expected_c,
        1e-9,
        0.0,
        "depletion capacitance",
    );
    assert_close(g, 1e-12, 1e-6, 0.0, "reverse conductance (gmin)");
}

#[test]
fn forward_bias_admittance_includes_diffusion_capacitance() {
    let model = DiodeModel {
        is: 1e-14,
        n: 1.0,
        tt: 20e-9,
        ..DiodeModel::default()
    };
    let (bias, f) = (0.62, 1e5);
    let (g, b) = diode_admittance(model, bias, f);
    let vt = thermal_voltage(27.0);
    let gd = 1e-14 * (bias / vt).exp() / vt;
    assert_close(g, gd + 1e-12, 1e-9, 0.0, "small-signal conductance");
    assert_close(
        b / (2.0 * std::f64::consts::PI * f),
        20e-9 * gd,
        1e-9,
        0.0,
        "diffusion capacitance",
    );
}

fn transient(stop: f64, step: f64, max_step: Option<f64>) -> Analysis {
    Analysis::Transient(Transient {
        stop,
        step,
        start_save: 0.0,
        max_step,
        initial_state: InitialState::ZeroState,
    })
}

#[test]
fn half_wave_rectifier_passes_positive_half_cycles() {
    let model = rectifier_diode();
    let (amplitude, load) = (10.0, 1e3);
    let mut c = Circuit::new();
    let a = c.node("a");
    let out = c.node("out");
    let sine = Waveform::Sine {
        offset: 0.0,
        amplitude,
        frequency: 50.0,
        delay: 0.0,
        damping: 0.0,
        phase: 0.0,
        cycles: None,
    };
    c.add(Element::new(
        "V1",
        [a, GROUND],
        Device::VoltageSource(Source::with_waveform(sine)),
    ));
    c.add(Element::new("D1", [a, out], Device::diode(model.clone())));
    c.add(Element::new("R1", [out, GROUND], Device::resistor(load)));
    let r = run(
        &c,
        &transient(40e-3, 20e-6, Some(20e-6)),
        &SimOptions::default(),
    );
    let v = r.signal("V(out)").unwrap().real().unwrap();
    let peak = v.iter().cloned().fold(f64::MIN, f64::max);
    let floor = v.iter().cloned().fold(f64::MAX, f64::min);

    // Quasi-static peak: solve amplitude = Vj + I·(RS + R) with the DC law.
    // At 50 Hz the junction charge shifts the peak by far less than 1 µV.
    let gmin = SimOptions::default().gmin;
    let current = |vj: f64| shockley(&model, vj, 27.0, gmin);
    let vj = bisect(
        |vj| vj + current(vj) * (model.rs + load) - amplitude,
        0.0,
        amplitude,
    );
    let expected_peak = current(vj) * load;
    // The output grid samples the crest within ±10 µs: cos(2π·50·10 µs)
    // lowers it by at most 2e-5 relative.
    assert_close(peak, expected_peak, 2e-5, 0.0, "rectified peak");
    // Reverse half cycles are blocked: removing the stored charge after each
    // conduction interval and the leakage leave less than 0.01 % of the drive.
    assert!(
        floor < 0.0 && floor > -1e-4 * amplitude,
        "reverse output {floor}"
    );
    // At the negative crest the charge is gone and dV/dt = 0, so only the
    // static leakage IS·(1 − e^(−V/nVt)) + gmin·V flows through the load.
    let crest = r
        .axis
        .values
        .iter()
        .position(|&t| (t - 15e-3).abs() < 1e-9)
        .expect("15 ms sample");
    let across = v[crest] + amplitude;
    let leakage = shockley(&model, -across, 27.0, gmin) * load;
    assert_close(v[crest], leakage, 1e-3, 0.0, "reverse leakage at the crest");
}

#[test]
fn reverse_recovery_follows_charge_control_storage_time() {
    // A diode carrying IF is switched to a reverse drive IR. Charge control
    // (q = TT·i) keeps the junction forward-biased for
    // ts = TT·ln(1 + IF/IR), during which the reverse current flows.
    let tt = 1e-6;
    // A small depletion capacitance keeps the end of storage, where the
    // diffusion capacitance collapses, from being an algebraic jump.
    let model = DiodeModel {
        is: 1e-14,
        n: 1.0,
        tt,
        cjo: 2e-12,
        ..DiodeModel::default()
    };
    let (drive, resistance) = (10.0, 1e3);
    let mut c = Circuit::new();
    let a = c.node("a");
    let k = c.node("k");
    let pulse = Waveform::Pulse {
        initial: drive,
        pulsed: -drive,
        delay: 1e-6,
        rise: 1e-9,
        fall: 1e-9,
        width: 1.0,
        period: 0.0,
        cycles: None,
    };
    c.add(Element::new(
        "V1",
        [a, GROUND],
        Device::VoltageSource(Source::with_waveform(pulse)),
    ));
    c.add(Element::new("R1", [a, k], Device::resistor(resistance)));
    c.add(Element::new("D1", [k, GROUND], Device::diode(model)));
    let spec = Analysis::Transient(Transient {
        stop: 4e-6,
        step: 1e-9,
        start_save: 0.0,
        max_step: Some(5e-9),
        initial_state: InitialState::OperatingPoint,
    });
    let r = run(&c, &spec, &SimOptions::default());
    let time = &r.axis.values;
    let vk = r.signal("V(k)").unwrap().real().unwrap();
    // The junction voltage crosses zero when the stored charge is gone.
    let switch = 1e-6 + 0.5e-9;
    let crossing = time
        .iter()
        .zip(vk)
        .find(|&(&t, &v)| t > switch && v < 0.0)
        .map(|(&t, _)| t)
        .expect("junction never recovered");
    let vd = vk[0];
    let forward = (drive - vd) / resistance;
    let reverse = (drive + vd) / resistance;
    let expected = tt * (1.0 + forward / reverse).ln();
    // The forward drop falls during storage, so IR grows slightly; 5% covers it.
    assert_close(crossing - switch, expected, 0.05, 0.0, "storage time");
}

#[test]
fn one_kilovolt_forward_drive_converges() {
    // A 1 kV source with 1 Ω drives about 540 A through the diode: plain
    // Newton from the SPICE starting point needs many limited steps, and the
    // solver must still land on the exact solution.
    let model = signal_diode();
    let options = precise();
    let r = run(
        &resistor_diode(1000.0, 1.0, model.clone()),
        &Analysis::OperatingPoint,
        &options,
    );
    let vk = op_value(&r, "V(k)");
    let current = 1000.0 - vk;
    let expected = shockley(&model, vk - current * model.rs, 27.0, options.gmin);
    assert_close(current, expected, 1e-9, 0.0, "kilovolt operating point");
}

#[test]
fn one_kilovolt_reverse_drive_converges() {
    // The test rectifier has no breakdown, so the junction blocks the full
    // kilovolt.
    let model = rectifier_diode();
    let options = precise();
    let r = run(
        &resistor_diode(-1000.0, 1e3, model.clone()),
        &Analysis::OperatingPoint,
        &options,
    );
    let vk = op_value(&r, "V(k)");
    assert!(
        vk < -999.9,
        "almost all of the supply must appear across the diode, got {vk}"
    );
    // The 1 kΩ drop is only ~15 µV, so compare the model current directly
    // and check KCL to the solver's voltage resolution (vntol + reltol·1 kV).
    let current = op_value(&r, "I(D1)");
    assert_close(
        current,
        shockley(&model, vk, 27.0, options.gmin),
        1e-6,
        0.0,
        "reverse current",
    );
    assert_close(
        current,
        (-1000.0 - vk) / 1e3,
        0.0,
        1e-11,
        "KCL at the diode",
    );
}

// --------------------------- junction exponents far above 80 (DCAC-2)

/// Forward voltage of `model` carrying `current` at `celsius`.
fn forward_at(model: DiodeModel, current: f64, celsius: f64) -> f64 {
    let mut c = Circuit::new();
    let a = c.node("a");
    c.add(Element::new(
        "I1",
        [GROUND, a],
        Device::CurrentSource(Source::dc(current)),
    ));
    c.add(Element::new("D1", [a, GROUND], Device::diode(model)));
    let options = SimOptions {
        temperature: celsius,
        ..precise()
    };
    op_value(&run(&c, &Analysis::OperatingPoint, &options), "V(a)")
}

/// `n·Vt·ln(I/IS(T) + 1) + I·RS` with the SPICE saturation-current law.
fn closed_form_forward(model: &DiodeModel, current: f64, celsius: f64) -> f64 {
    let (t, tnom) = (celsius + 273.15, model.tnom + 273.15);
    let vt = thermal_voltage(celsius);
    let factor = (t / tnom - 1.0) * model.eg / vt + model.xti * (t / tnom).ln();
    let is = model.is * (factor / model.n).exp();
    model.n * vt * (current / is).ln_1p() + current * model.rs
}

#[test]
fn tiny_saturation_currents_are_exact_far_above_exponent_80() {
    // A wide-bandgap style card: 20 mA puts the junction at an exponent of
    // 79 to 83, where the exponential used to continue as its tangent
    // (V 27.8 mV and 0.55 V too high). LTspice 24.1.9, I1 0 a 20m / D1 a 0
    // DX / .model DX D(IS=… N=1.5 RS=2), TEMP = TNOM = 26.9996851609185 and
    // the tight tolerances of tests/reference/devices_diode_dc.cir:
    // 3.18252821102 V and 3.25603439753 V.
    for (is, ltspice) in [
        (1e-36, None),
        (1.33e-37, Some(3.182_528_211_02)),
        (2e-38, Some(3.256_034_397_53)),
    ] {
        let model = DiodeModel {
            is,
            n: 1.5,
            rs: 2.0,
            ..DiodeModel::default()
        };
        let v = forward_at(model.clone(), 20e-3, 27.0);
        let exact = closed_form_forward(&model, 20e-3, 27.0);
        assert_close(v, exact, 1e-11, 0.0, &format!("IS = {is:e}"));
        if let Some(lt) = ltspice {
            assert_close(v, lt, 1e-9, 0.0, &format!("IS = {is:e} vs LTspice"));
        }
    }
}

#[test]
fn silicon_diode_is_exact_at_cryogenic_temperatures() {
    // IS = 20 fA falls to 5e-57 A at −200 °C, so 1 mA needs an exponent of
    // about 124: the old exponent cap gave a wrong 3.94 V at −170 °C and no
    // convergence at −200 °C. LTspice 24.1.9, I1 0 a 1m / D1 a 0 DS /
    // .model DS D(IS=2e-14 N=1.4 RS=8) with the options of
    // tests/reference/devices_diode_dc.cir and .temp −200.00007672703498
    // or −170.00010819403496 (scaled like TNOM for LTspice's kT/q):
    // 1.0915845713 V at −200 °C and 1.07158669944 V at −170 °C.
    let model = DiodeModel {
        is: 2e-14,
        n: 1.4,
        rs: 8.0,
        ..DiodeModel::default()
    };
    for (celsius, ltspice) in [
        (-200.0, 1.091_584_571_3),
        (-170.0, 1.071_586_699_44),
        (-150.0, f64::NAN),
    ] {
        let v = forward_at(model.clone(), 1e-3, celsius);
        let exact = closed_form_forward(&model, 1e-3, celsius);
        assert_close(v, exact, 1e-10, 0.0, &format!("{celsius} °C"));
        if ltspice.is_finite() {
            assert_close(v, ltspice, 1e-9, 0.0, &format!("{celsius} °C vs LTspice"));
        }
    }
}

#[test]
fn runaway_junction_voltages_stay_finite_and_converge() {
    // IS = 1e-45 with N = 2 behind 100 Ω from 5 V: the solution sits at an
    // exponent of about 186, and Newton must reach it from the usual start.
    let model = DiodeModel {
        is: 1e-45,
        n: 2.0,
        rs: 2.0,
        ..DiodeModel::default()
    };
    let mut c = Circuit::new();
    let (v, a) = (c.node("v"), c.node("a"));
    c.add(Element::new(
        "V1",
        [v, GROUND],
        Device::VoltageSource(Source::dc(5.0)),
    ));
    c.add(Element::new("R1", [v, a], Device::resistor(100.0)));
    c.add(Element::new(
        "D1",
        [a, GROUND],
        Device::diode(model.clone()),
    ));
    let r = run(&c, &Analysis::OperatingPoint, &precise());
    let (va, i) = (op_value(&r, "V(a)"), op_value(&r, "I(D1)"));
    assert_close(i, (5.0 - va) / 100.0, 1e-9, 0.0, "Ohm's law");
    assert_close(
        va,
        closed_form_forward(&model, i, 27.0),
        1e-9,
        0.0,
        "junction",
    );
}
