//! Built-in model library (D13): naming and metadata rules, default models,
//! parameter sanity, a bias-point smoke test of every model, and every
//! documented design target checked by simulation.
//!
//! The targets below are the ones each entry's `source` string states. They
//! are checked at 27 °C with the default simulator options; tolerances allow
//! for the rounding of the derived parameters.

use std::collections::BTreeSet;
use std::f64::consts::PI;
use xsim_spice_engine::library::{self, ModelEntry};
use xsim_spice_engine::*;

const D13_PREFIX: &str = "xsim generic model (D13): ";
const D13_SUFFIX: &str = "not a model of any specific part";

/// Upper-case identifier starting with one of `prefixes`.
fn assert_generic_name(name: &str, prefixes: &[&str]) {
    assert!(
        name.chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'),
        "{name}: names are upper-case identifiers"
    );
    assert!(
        prefixes
            .iter()
            .any(|p| name.starts_with(p) && name.len() > p.len()),
        "{name}: must start with one of {prefixes:?}"
    );
}

fn check_metadata<M>(entries: &[ModelEntry<M>], name: impl Fn(&M) -> &str, root: &str) {
    let mut names = BTreeSet::new();
    for e in entries {
        let n = name(&e.model);
        assert!(names.insert(n.to_string()), "duplicate model name {n}");
        assert!(e.description.starts_with("Generic "), "{n}: description");
        assert_eq!(e.manufacturer, "Generic", "{n}: manufacturer");
        assert!(
            e.source.starts_with(D13_PREFIX) && e.source.ends_with(D13_SUFFIX),
            "{n}: source must state the D13 design targets: {}",
            e.source
        );
        assert!(!e.source.contains("http"), "{n}: no external source");
        assert_eq!(
            e.category.first(),
            Some(&root),
            "{n}: category {:?}",
            e.category
        );
        assert!(
            e.category.len() >= 2 && e.category.iter().all(|c| !c.is_empty()),
            "{n}: category {:?}",
            e.category
        );
    }
}

#[test]
fn every_entry_is_a_generic_model_with_complete_metadata() {
    check_metadata(&library::diodes(), |m| &m.name, "Diodes");
    check_metadata(&library::bjts(), |m| &m.name, "Transistors");
    check_metadata(&library::jfets(), |m| &m.name, "Transistors");
    check_metadata(&library::opamps(), |m| &m.name, "Op-amps");
}

#[test]
fn names_start_with_the_device_type() {
    for e in library::diodes() {
        assert_generic_name(&e.model.name, &["D_", "DZ_", "LED_"]);
    }
    for e in library::bjts() {
        let prefix = match e.model.polarity {
            Polarity::N => "NPN_",
            Polarity::P => "PNP_",
        };
        assert_generic_name(&e.model.name, &[prefix]);
    }
    for e in library::jfets() {
        let prefix = match e.model.polarity {
            Polarity::N => "NJF_",
            Polarity::P => "PJF_",
        };
        assert_generic_name(&e.model.name, &[prefix]);
    }
    for e in library::opamps() {
        assert_generic_name(&e.model.name, &["OA_"]);
    }
}

#[test]
fn default_models_resolve_with_matching_polarity() {
    assert_eq!(library::default_diode(), "D_SIGNAL");
    assert!(library::diode(library::default_diode()).is_some());
    for (polarity, bjt, jfet) in [
        (Polarity::N, "NPN_GP", "NJF_MID"),
        (Polarity::P, "PNP_GP", "PJF_MID"),
    ] {
        assert_eq!(library::default_bjt(polarity), bjt);
        assert_eq!(library::default_jfet(polarity), jfet);
        let model = library::bjt(bjt).expect("default BJT");
        assert_eq!(model.polarity, polarity);
        let model = library::jfet(jfet).expect("default JFET");
        assert_eq!(model.polarity, polarity);
    }
}

#[test]
fn lookup_is_exact_and_case_sensitive() {
    assert_eq!(
        library::diode("D_SIGNAL").map(|m| m.name),
        Some("D_SIGNAL".to_string())
    );
    assert!(library::diode("d_signal").is_none());
    assert!(library::bjt("npn_gp").is_none());
    assert!(library::bjt("NPN_GP ").is_none());
    assert!(library::jfet("no such part").is_none());
    assert!(library::opamp("oa_gp").is_none());
    assert_eq!(library::default_opamp(), "OA_GP");
    assert_eq!(
        library::opamp(library::default_opamp()).map(|m| m.kind),
        Some(OpAmpKind::Macro)
    );
}

fn leaf<M>(entry: &ModelEntry<M>) -> &'static str {
    entry.category.last().copied().unwrap_or_default()
}

#[test]
fn library_covers_the_required_families() {
    let diodes = library::diodes();
    let kinds: BTreeSet<_> = diodes.iter().map(leaf).collect();
    for kind in ["Switching", "Rectifiers", "Schottky", "Zener", "LED"] {
        assert!(kinds.contains(kind), "no {kind} diode");
    }
    let bjts = library::bjts();
    for polarity in [Polarity::N, Polarity::P] {
        let family: Vec<_> = bjts
            .iter()
            .filter(|e| e.model.polarity == polarity)
            .collect();
        assert!(family.len() >= 5, "{polarity:?}: {}", family.len());
        // The library window's search tests (D12) need each polarity in
        // more than one leaf category.
        let leaves: BTreeSet<_> = family.iter().map(|e| leaf(e)).collect();
        assert!(leaves.len() >= 2, "{polarity:?}: {leaves:?}");
    }
    let jfets = library::jfets();
    let n = jfets
        .iter()
        .filter(|e| e.model.polarity == Polarity::N)
        .count();
    assert!(
        n >= 3 && jfets.len() - n >= 2,
        "{n} N, {} P",
        jfets.len() - n
    );
}

#[test]
fn polarity_matches_category() {
    for e in library::bjts() {
        let expected = if e.model.polarity == Polarity::N {
            "NPN"
        } else {
            "PNP"
        };
        assert!(
            e.category.contains(&expected),
            "{}: {:?}",
            e.model.name,
            e.category
        );
    }
    for e in library::jfets() {
        let expected = if e.model.polarity == Polarity::N {
            "N-channel"
        } else {
            "P-channel"
        };
        assert!(
            e.category.contains(&expected),
            "{}: {:?}",
            e.model.name,
            e.category
        );
    }
}

fn positive(name: &str, what: &str, v: f64) {
    assert!(
        v.is_finite() && v > 0.0,
        "{name}: {what} must be positive and finite, got {v}"
    );
}

fn non_negative(name: &str, what: &str, v: f64) {
    assert!(
        v.is_finite() && v >= 0.0,
        "{name}: {what} must be finite and non-negative, got {v}"
    );
}

/// Positive, possibly infinite ("no effect") corner parameters.
fn positive_or_infinite(name: &str, what: &str, v: f64) {
    assert!(
        v > 0.0 && !v.is_nan(),
        "{name}: {what} must be positive or infinite, got {v}"
    );
}

#[test]
fn diode_parameters_are_physical() {
    for e in library::diodes() {
        let m = &e.model;
        let n = m.name.as_str();
        for (what, v) in [
            ("is", m.is),
            ("n", m.n),
            ("vj", m.vj),
            ("eg", m.eg),
            ("ibv", m.ibv),
            ("nbv", m.nbv),
        ] {
            positive(n, what, v);
        }
        for (what, v) in [
            ("rs", m.rs),
            ("cjo", m.cjo),
            ("m", m.m),
            ("tt", m.tt),
            ("xti", m.xti),
            ("fc", m.fc),
        ] {
            non_negative(n, what, v);
        }
        positive_or_infinite(n, "bv", m.bv);
        assert!(m.fc < 1.0 && m.m < 1.0, "{n}: fc, m");
    }
}

#[test]
fn bjt_parameters_are_physical() {
    for e in library::bjts() {
        let m = &e.model;
        let n = m.name.as_str();
        for (what, v) in [
            ("is", m.is),
            ("bf", m.bf),
            ("br", m.br),
            ("nf", m.nf),
            ("nr", m.nr),
            ("ne", m.ne),
            ("nc", m.nc),
            ("vje", m.vje),
            ("vjc", m.vjc),
            ("vjs", m.vjs),
            ("eg", m.eg),
        ] {
            positive(n, what, v);
        }
        for (what, v) in [
            ("ise", m.ise),
            ("isc", m.isc),
            ("rb", m.rb),
            ("rbm", m.rbm),
            ("re", m.re),
            ("rc", m.rc),
            ("cje", m.cje),
            ("cjc", m.cjc),
            ("cjs", m.cjs),
            ("tf", m.tf),
            ("tr", m.tr),
            ("xtf", m.xtf),
            ("itf", m.itf),
            ("xti", m.xti),
            ("xtb", m.xtb),
        ] {
            non_negative(n, what, v);
        }
        for (what, v) in [
            ("vaf", m.vaf),
            ("var", m.var),
            ("ikf", m.ikf),
            ("ikr", m.ikr),
            ("irb", m.irb),
            ("vtf", m.vtf),
        ] {
            positive_or_infinite(n, what, v);
        }
        // Library rule: a set RB is a constant base resistance.
        assert_eq!(m.rbm, m.rb, "{n}: rbm must equal rb");
        assert!((0.0..=1.0).contains(&m.xcjc), "{n}: xcjc");
    }
}

#[test]
fn jfet_parameters_are_physical() {
    for e in library::jfets() {
        let m = &e.model;
        let n = m.name.as_str();
        for (what, v) in [
            ("beta", m.beta),
            ("pb", m.pb),
            ("is", m.is),
            ("b", m.b),
            ("lambda", m.lambda),
        ] {
            positive(n, what, v);
        }
        for (what, v) in [("rd", m.rd), ("rs", m.rs), ("cgs", m.cgs), ("cgd", m.cgd)] {
            non_negative(n, what, v);
        }
        // Every bundled JFET is a depletion-mode device.
        assert!(m.vto.is_finite() && m.vto < 0.0, "{n}: vto {}", m.vto);
    }
}

// ------------------------------------------------------------------ helpers

/// `+1` for N-type and `−1` for P-type devices.
fn sign(polarity: Polarity) -> f64 {
    match polarity {
        Polarity::N => 1.0,
        Polarity::P => -1.0,
    }
}

fn simulate_ok(circuit: &Circuit, analysis: &Analysis) -> SimulationResult {
    simulate(
        circuit,
        analysis,
        &SimOptions::default(),
        &SimControl::new(),
    )
    .expect("simulation failed")
}

fn operating_point(circuit: &Circuit) -> SimulationResult {
    simulate_ok(circuit, &Analysis::OperatingPoint)
}

fn value(result: &SimulationResult, name: &str) -> f64 {
    result
        .signal(name)
        .unwrap_or_else(|| panic!("no signal {name}"))
        .real()
        .unwrap_or_else(|| panic!("{name} is not real"))[0]
}

fn phasor(result: &SimulationResult, name: &str) -> (f64, f64) {
    match &result
        .signal(name)
        .unwrap_or_else(|| panic!("no signal {name}"))
        .data
    {
        SignalData::Complex { re, im } => (re[0], im[0]),
        SignalData::Real(_) => panic!("{name} is not complex"),
    }
}

fn ac_at(frequency: f64) -> Analysis {
    Analysis::Ac(AcSweep {
        scale: SweepScale::Linear,
        points: 1,
        start: frequency,
        stop: frequency,
    })
}

fn assert_within(actual: f64, target: f64, rel: f64, what: &str) {
    assert!(
        (actual - target).abs() <= rel * target.abs(),
        "{what}: {actual:.6e}, target {target:.6e} ± {:.1} %",
        rel * 100.0
    );
}

fn library_diode(name: &str) -> DiodeModel {
    library::diode(name).unwrap_or_else(|| panic!("{name} missing from the library"))
}

fn library_bjt(name: &str) -> BjtModel {
    library::bjt(name).unwrap_or_else(|| panic!("{name} missing from the library"))
}

fn library_jfet(name: &str) -> JfetModel {
    library::jfet(name).unwrap_or_else(|| panic!("{name} missing from the library"))
}

// ------------------------------------------------------------ smoke tests

#[test]
fn every_model_passes_circuit_validation() {
    let check = |device: Device, nodes: usize| {
        let mut c = Circuit::new();
        let ids: Vec<_> = (0..nodes).map(|i| c.node(&format!("n{i}"))).collect();
        c.add(Element::new("X1", ids, device));
        c.validate().expect("library model rejected");
    };
    for e in library::diodes() {
        check(Device::diode(e.model), 2);
    }
    for e in library::bjts() {
        check(Device::bjt(e.model), 3);
    }
    for e in library::jfets() {
        check(Device::jfet(e.model), 3);
    }
}

#[test]
fn every_diode_conducts_one_milliamp() {
    for e in library::diodes() {
        let v = forward_voltage(&e.model, 1e-3);
        assert!(
            v > 0.05 && v < 2.5,
            "{}: forward drop {v} V at 1 mA",
            e.model.name
        );
    }
}

#[test]
fn every_bjt_amplifies_in_forward_active() {
    for e in library::bjts() {
        let s = sign(e.model.polarity);
        // Power transistors are characterised at far higher currents.
        let ib = if e.category.contains(&"Power") {
            1e-3
        } else {
            10e-6
        };
        let (ic, _) = bjt_bias(&e.model, ib, 5.0);
        assert!(
            ic > 10.0 * ib,
            "{}: Ic = {ic} A for Ib = {ib} A (sign {s})",
            e.model.name
        );
    }
}

#[test]
fn every_jfet_conducts_at_zero_gate_bias() {
    for e in library::jfets() {
        let idss = jfet_drain_current(&e.model, 0.0, 10.0);
        // IDSS = β·VTO²·(1 + λ·VDS) for the classic model.
        let m = &e.model;
        let expected = m.beta * m.vto * m.vto * (1.0 + m.lambda * 10.0);
        if m.rd == 0.0 && m.rs == 0.0 {
            assert!(
                (idss - expected).abs() < 1e-3 * expected,
                "{}: IDSS {idss} vs {expected}",
                m.name
            );
        } else {
            // Source resistance degenerates the channel slightly.
            assert!(
                idss < expected && idss > 0.9 * expected,
                "{}: IDSS {idss} vs {expected}",
                m.name
            );
        }
    }
}

// ----------------------------------------------------------- diode targets

/// Terminal voltage of `model` carrying the forward `current`.
fn forward_voltage(model: &DiodeModel, current: f64) -> f64 {
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
    value(&operating_point(&c), "V(k)")
}

/// Reverse voltage across `model` carrying the reverse `current`.
fn reverse_voltage(model: &DiodeModel, current: f64) -> f64 {
    let mut c = Circuit::new();
    let k = c.node("k");
    c.add(Element::new(
        "I1",
        [GROUND, k],
        Device::CurrentSource(Source::dc(current)),
    ));
    c.add(Element::new(
        "D1",
        [GROUND, k],
        Device::diode(model.clone()),
    ));
    value(&operating_point(&c), "V(k)")
}

/// Current into the anode at the terminal voltage `v`.
fn diode_current(model: &DiodeModel, v: f64) -> f64 {
    let mut c = Circuit::new();
    let a = c.node("a");
    c.add(Element::new(
        "V1",
        [a, GROUND],
        Device::VoltageSource(Source::dc(v)),
    ));
    c.add(Element::new(
        "D1",
        [a, GROUND],
        Device::diode(model.clone()),
    ));
    value(&operating_point(&c), "I(D1)")
}

/// Small-signal capacitance at the terminal voltage `v` (1 MHz).
fn diode_capacitance(model: &DiodeModel, v: f64) -> f64 {
    let mut c = Circuit::new();
    let a = c.node("a");
    c.add(Element::new(
        "V1",
        [a, GROUND],
        Device::VoltageSource(Source::ac(v, 1.0, 0.0)),
    ));
    c.add(Element::new(
        "D1",
        [a, GROUND],
        Device::diode(model.clone()),
    ));
    let f = 1e6;
    let (_, im) = phasor(&simulate_ok(&c, &ac_at(f)), "I(D1)");
    im / (2.0 * PI * f)
}

/// Storage time of the diode switched from `drive` to `−drive` through
/// `resistance`: the time from the switch until the junction voltage crosses
/// zero, and the charge-control expectation `TT·ln(1 + IF/IR)`.
fn storage_time(model: &DiodeModel, drive: f64, resistance: f64) -> (f64, f64) {
    let tt = model.tt;
    let mut c = Circuit::new();
    let a = c.node("a");
    let k = c.node("k");
    let delay = tt;
    let pulse = Waveform::Pulse {
        initial: drive,
        pulsed: -drive,
        delay,
        rise: tt * 1e-3,
        fall: tt * 1e-3,
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
    c.add(Element::new(
        "D1",
        [k, GROUND],
        Device::diode(model.clone()),
    ));
    let spec = Analysis::Transient(Transient {
        stop: delay + 4.0 * tt,
        step: tt / 200.0,
        start_save: 0.0,
        max_step: Some(tt / 200.0),
        initial_state: InitialState::OperatingPoint,
    });
    let r = simulate_ok(&c, &spec);
    let time = &r.axis.values;
    let vk = r.signal("V(k)").and_then(Signal::real).expect("V(k)");
    let switch = delay + 0.5 * tt * 1e-3;
    let crossing = time
        .iter()
        .zip(vk)
        .find(|&(&t, &v)| t > switch && v < 0.0)
        .map(|(&t, _)| t)
        .expect("junction never recovered");
    let vd = vk[0];
    let forward = (drive - vd) / resistance;
    let reverse = (drive + vd) / resistance;
    (crossing - switch, tt * (1.0 + forward / reverse).ln())
}

/// Forward voltage targets `(current, volts)`; checked to ±10 mV.
const FORWARD_TARGETS: &[(&str, &[(f64, f64)])] = &[
    ("D_SIGNAL", &[(1e-3, 0.62), (10e-3, 0.72), (100e-3, 0.85)]),
    ("D_RECT_1A", &[(10e-3, 0.62), (100e-3, 0.74), (1.0, 0.95)]),
    ("D_FAST_1A", &[(10e-3, 0.66), (100e-3, 0.79), (1.0, 1.05)]),
    (
        "D_SCHOTTKY_1A",
        &[(10e-3, 0.23), (100e-3, 0.30), (1.0, 0.45)],
    ),
    ("DZ_3V3", &[(10e-3, 0.72), (100e-3, 0.87)]),
    ("DZ_5V1", &[(10e-3, 0.72), (100e-3, 0.87)]),
    ("DZ_12V", &[(10e-3, 0.72), (100e-3, 0.87)]),
    ("LED_RED", &[(1e-3, 1.80), (10e-3, 1.94), (20e-3, 2.00)]),
    ("LED_GREEN", &[(1e-3, 2.00), (10e-3, 2.15), (20e-3, 2.20)]),
];

/// Reverse breakdown targets `(current, volts)`; checked to ±1 %.
const BREAKDOWN_TARGETS: &[(&str, f64, f64)] = &[
    ("D_SIGNAL", 100e-6, 100.0),
    ("D_RECT_1A", 10e-6, 1000.0),
    ("D_FAST_1A", 10e-6, 600.0),
    ("D_SCHOTTKY_1A", 0.5e-3, 40.0),
    ("DZ_3V3", 5e-3, 3.3),
    ("DZ_5V1", 5e-3, 5.1),
    ("DZ_12V", 5e-3, 12.0),
    ("LED_RED", 10e-6, 5.0),
    ("LED_GREEN", 10e-6, 5.0),
];

/// Junction capacitance targets `(terminal volts, farads)`; checked to ±5 %.
const CAPACITANCE_TARGETS: &[(&str, f64, f64)] = &[
    ("D_SIGNAL", 0.0, 2e-12),
    ("D_SIGNAL", -10.0, 1e-12),
    ("D_RECT_1A", -4.0, 9e-12),
    ("D_FAST_1A", -4.0, 7e-12),
    ("D_SCHOTTKY_1A", -4.0, 50e-12),
    ("DZ_3V3", 0.0, 150e-12),
    ("DZ_5V1", 0.0, 100e-12),
    ("DZ_12V", 0.0, 40e-12),
    ("LED_RED", 0.0, 20e-12),
    ("LED_GREEN", 0.0, 15e-12),
];

/// Storage-time targets at IF ≈ IR (`drive`, `resistance`, seconds);
/// checked to ±15 % against the target and ±10 % against charge control.
const STORAGE_TARGETS: &[(&str, f64, f64, f64)] = &[
    ("D_SIGNAL", 50.0, 5e3, 3.5e-9),
    ("D_RECT_1A", 100.0, 100.0, 2.1e-6),
    ("D_FAST_1A", 100.0, 100.0, 140e-9),
];

#[test]
fn every_diode_has_forward_targets() {
    let named: BTreeSet<_> = FORWARD_TARGETS.iter().map(|(n, _)| *n).collect();
    for e in library::diodes() {
        assert!(named.contains(e.model.name.as_str()), "{}", e.model.name);
    }
}

#[test]
fn diodes_meet_their_forward_voltage_targets() {
    for (name, points) in FORWARD_TARGETS {
        let model = library_diode(name);
        for &(current, target) in *points {
            let v = forward_voltage(&model, current);
            assert!(
                (v - target).abs() <= 0.01,
                "{name}: VF = {v:.4} V at {current} A, target {target} V"
            );
        }
    }
}

#[test]
fn diodes_meet_their_breakdown_targets() {
    for &(name, current, target) in BREAKDOWN_TARGETS {
        let v = reverse_voltage(&library_diode(name), current);
        assert_within(
            v,
            target,
            0.01,
            &format!("{name}: reverse V at {current} A"),
        );
    }
}

#[test]
fn leds_drop_their_colour_voltage_at_ten_milliamps() {
    for (name, lo, hi) in [("LED_RED", 1.8, 2.0), ("LED_GREEN", 2.0, 2.2)] {
        let v = forward_voltage(&library_diode(name), 10e-3);
        assert!(v > lo && v < hi, "{name}: VF = {v} V at 10 mA");
    }
}

#[test]
fn zeners_meet_their_dynamic_resistance_and_knee_targets() {
    for (name, vz, rz) in [
        ("DZ_3V3", 3.3, 30.0),
        ("DZ_5V1", 5.1, 15.0),
        ("DZ_12V", 12.0, 15.0),
    ] {
        let model = library_diode(name);
        let measured = (reverse_voltage(&model, 5.5e-3) - reverse_voltage(&model, 4.5e-3)) / 1e-3;
        assert_within(measured, rz, 0.1, &format!("{name}: rz at 5 mA"));
        // Below the knee the current is a small fraction of IZT.
        let knee = -diode_current(&model, -0.9 * vz);
        assert!(knee > 0.0 && knee < 1e-3, "{name}: {knee} A at 0.9·VZ");
        // The breakdown branch must not conduct at zero bias.
        assert!(diode_current(&model, 0.0).abs() < 1e-12, "{name}: I(0 V)");
    }
}

#[test]
fn zeners_conduct_forward_like_a_silicon_diode() {
    // The series resistance acts in both directions: forward conduction
    // must stay below 1 V at 100 mA (the knee is shaped by NBV, not RS).
    for name in ["DZ_3V3", "DZ_5V1", "DZ_12V"] {
        let v = forward_voltage(&library_diode(name), 100e-3);
        assert!(v < 1.0, "{name}: VF = {v} V at 100 mA");
    }
}

/// Forward-voltage temperature coefficient targets `(current, V/K)`,
/// measured from 0 °C to 50 °C; checked to ±10 %.
const TEMPCO_TARGETS: &[(&str, f64, f64)] =
    &[("LED_RED", 10e-3, -2.0e-3), ("LED_GREEN", 10e-3, -2.5e-3)];

/// Forward voltage of `model` carrying `current` at `celsius`.
fn forward_voltage_at(model: &DiodeModel, current: f64, celsius: f64) -> f64 {
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
        ..SimOptions::default()
    };
    let r = simulate(&c, &Analysis::OperatingPoint, &options, &SimControl::new())
        .unwrap_or_else(|e| panic!("{}: {e}", model.name));
    value(&r, "V(k)")
}

#[test]
fn leds_meet_their_forward_voltage_temperature_coefficients() {
    for &(name, current, target) in TEMPCO_TARGETS {
        let model = library_diode(name);
        let cold = forward_voltage_at(&model, current, 0.0);
        let hot = forward_voltage_at(&model, current, 50.0);
        let tempco = (hot - cold) / 50.0;
        assert_within(tempco, target, 0.1, &format!("{name}: dVF/dT"));
        // Real LEDs lie between about −1.5 and −3 mV/K.
        assert!((-3e-3..=-1.5e-3).contains(&tempco), "{name}: {tempco} V/K");
    }
}

#[test]
fn diodes_meet_their_capacitance_targets() {
    for &(name, v, target) in CAPACITANCE_TARGETS {
        let c = diode_capacitance(&library_diode(name), v);
        assert_within(c, target, 0.05, &format!("{name}: C at {v} V"));
    }
}

#[test]
fn diodes_meet_their_storage_time_targets() {
    for &(name, drive, resistance, target) in STORAGE_TARGETS {
        let (ts, charge_control) = storage_time(&library_diode(name), drive, resistance);
        assert_within(ts, target, 0.15, &format!("{name}: storage time"));
        assert_within(ts, charge_control, 0.1, &format!("{name}: charge control"));
    }
}

// ------------------------------------------------------------- BJT targets

/// Design targets of one BJT (magnitudes; VCE = 5 V unless noted).
struct BjtTargets {
    name: &'static str,
    /// `(IC, VBE)`, checked to ±10 mV.
    vbe: (f64, f64),
    /// `(IC, β)` at the nominal, low and high current, checked to ±5 %.
    beta: [(f64, f64); 3],
    /// Early voltage (V), checked to ±3 %.
    vaf: f64,
    /// `(IC, fT)`, checked to ±10 %.
    ft: (f64, f64),
    /// Zero-bias CJE and CJC (F), checked to ±3 %.
    cje: f64,
    cjc: f64,
    /// `(IC, IB, VCE(sat))`, checked to ±15 %.
    saturation: (f64, f64, f64),
}

const BJT_TARGETS: &[BjtTargets] = &[
    BjtTargets {
        name: "NPN_GP",
        vbe: (1e-3, 0.65),
        beta: [(2e-3, 200.0), (10e-6, 150.0), (100e-3, 100.0)],
        vaf: 100.0,
        ft: (10e-3, 250e6),
        cje: 10e-12,
        cjc: 4e-12,
        saturation: (10e-3, 1e-3, 0.06),
    },
    BjtTargets {
        name: "NPN_LOWNOISE",
        vbe: (1e-3, 0.63),
        beta: [(1e-3, 450.0), (10e-6, 380.0), (50e-3, 250.0)],
        vaf: 80.0,
        ft: (10e-3, 200e6),
        cje: 12e-12,
        cjc: 5e-12,
        saturation: (10e-3, 0.5e-3, 0.075),
    },
    BjtTargets {
        name: "NPN_SWITCH",
        vbe: (1e-3, 0.66),
        beta: [(10e-3, 100.0), (100e-6, 70.0), (200e-3, 50.0)],
        vaf: 60.0,
        ft: (10e-3, 500e6),
        cje: 5e-12,
        cjc: 2.5e-12,
        saturation: (100e-3, 10e-3, 0.2),
    },
    BjtTargets {
        name: "NPN_HV",
        vbe: (10e-3, 0.68),
        beta: [(10e-3, 80.0), (100e-6, 50.0), (100e-3, 40.0)],
        vaf: 500.0,
        ft: (10e-3, 60e6),
        cje: 30e-12,
        cjc: 3e-12,
        saturation: (20e-3, 2e-3, 0.37),
    },
    BjtTargets {
        name: "NPN_1A",
        vbe: (100e-3, 0.70),
        beta: [(150e-3, 150.0), (5e-3, 100.0), (1.0, 80.0)],
        vaf: 100.0,
        ft: (50e-3, 150e6),
        cje: 60e-12,
        cjc: 20e-12,
        saturation: (1.0, 0.1, 0.36),
    },
    BjtTargets {
        name: "NPN_POWER",
        vbe: (1.0, 0.80),
        beta: [(1.0, 70.0), (50e-3, 50.0), (5.0, 40.0)],
        vaf: 100.0,
        ft: (0.5, 3e6),
        cje: 600e-12,
        cjc: 250e-12,
        saturation: (5.0, 0.5, 0.5),
    },
    BjtTargets {
        name: "PNP_GP",
        vbe: (1e-3, 0.65),
        beta: [(2e-3, 180.0), (10e-6, 130.0), (100e-3, 90.0)],
        vaf: 80.0,
        ft: (10e-3, 200e6),
        cje: 12e-12,
        cjc: 6e-12,
        saturation: (10e-3, 1e-3, 0.065),
    },
    BjtTargets {
        name: "PNP_LOWNOISE",
        vbe: (1e-3, 0.63),
        beta: [(1e-3, 400.0), (10e-6, 330.0), (50e-3, 220.0)],
        vaf: 60.0,
        ft: (10e-3, 150e6),
        cje: 12e-12,
        cjc: 7e-12,
        saturation: (10e-3, 0.5e-3, 0.08),
    },
    BjtTargets {
        name: "PNP_SWITCH",
        vbe: (1e-3, 0.66),
        beta: [(10e-3, 90.0), (100e-6, 60.0), (200e-3, 45.0)],
        vaf: 50.0,
        ft: (10e-3, 400e6),
        cje: 6e-12,
        cjc: 3e-12,
        saturation: (100e-3, 10e-3, 0.2),
    },
    BjtTargets {
        name: "PNP_1A",
        vbe: (100e-3, 0.70),
        beta: [(150e-3, 130.0), (5e-3, 90.0), (1.0, 70.0)],
        vaf: 80.0,
        ft: (50e-3, 120e6),
        cje: 70e-12,
        cjc: 30e-12,
        saturation: (1.0, 0.1, 0.4),
    },
    BjtTargets {
        name: "PNP_POWER",
        vbe: (1.0, 0.80),
        beta: [(1.0, 60.0), (50e-3, 45.0), (5.0, 35.0)],
        vaf: 80.0,
        ft: (0.5, 3e6),
        cje: 700e-12,
        cjc: 350e-12,
        saturation: (5.0, 0.5, 0.55),
    },
];

/// Transistor with base current `ib` (magnitude) and VCE forced to `vce`
/// (magnitude), emitter grounded. Returns the collector current and VBE
/// magnitudes.
fn bjt_bias(model: &BjtModel, ib: f64, vce: f64) -> (f64, f64) {
    let r = operating_point(&bjt_circuit(model, Source::dc(ib), vce));
    let s = sign(model.polarity);
    (s * value(&r, "Ic(Q1)"), s * value(&r, "V(b)"))
}

fn bjt_circuit(model: &BjtModel, base: Source, vce: f64) -> Circuit {
    let s = sign(model.polarity);
    let base = Source {
        dc: s * base.dc,
        ..base
    };
    let mut c = Circuit::new();
    let b = c.node("b");
    let col = c.node("c");
    c.add(Element::new("IB", [GROUND, b], Device::CurrentSource(base)));
    c.add(Element::new(
        "VCE",
        [col, GROUND],
        Device::VoltageSource(Source::dc(s * vce)),
    ));
    c.add(Element::new(
        "Q1",
        [col, b, GROUND],
        Device::bjt(model.clone()),
    ));
    c
}

/// Base current (magnitude) giving the collector current `ic` at `vce`.
fn base_current_for(model: &BjtModel, ic: f64, vce: f64) -> f64 {
    let mut ib = ic / 100.0;
    for _ in 0..60 {
        let (got, _) = bjt_bias(model, ib, vce);
        let ratio = ic / got;
        ib *= ratio;
        if (ratio - 1.0).abs() < 1e-10 {
            return ib;
        }
    }
    panic!("{}: no base current gives IC = {ic} A", model.name)
}

fn beta_at(model: &BjtModel, ic: f64) -> f64 {
    ic / base_current_for(model, ic, 5.0)
}

/// Extrapolated Early voltage plus VBE (≈ VAF) from the collector currents
/// at VCE = 5 V and 10 V with the base current fixed.
fn early_voltage(model: &BjtModel, ic: f64) -> f64 {
    let ib = base_current_for(model, ic, 5.0);
    let (ic1, vbe) = bjt_bias(model, ib, 5.0);
    let (ic2, _) = bjt_bias(model, ib, 10.0);
    ic1 * 5.0 / (ic2 - ic1) - 5.0 + vbe
}

/// `f·|h21|` at a frequency a decade below the target fT.
fn transition_frequency(model: &BjtModel, ic: f64, ft: f64) -> f64 {
    let ib = base_current_for(model, ic, 5.0);
    let c = bjt_circuit(model, Source::ac(ib, 1.0, 0.0), 5.0);
    let f = ft / 10.0;
    let (re, im) = phasor(&simulate_ok(&c, &ac_at(f)), "I(VCE)");
    f * re.hypot(im)
}

/// Zero-bias capacitance seen at `driven` (`"c"` or `"e"`) with the other two
/// terminals at AC ground.
fn junction_capacitance(model: &BjtModel, driven: &str) -> f64 {
    let mut c = Circuit::new();
    let b = c.node("b");
    let col = c.node("c");
    let e = c.node("e");
    let drive = |name: &str| {
        if name == driven {
            Source::ac(0.0, 1.0, 0.0)
        } else {
            Source::dc(0.0)
        }
    };
    c.add(Element::new(
        "VB",
        [b, GROUND],
        Device::VoltageSource(drive("b")),
    ));
    c.add(Element::new(
        "VC",
        [col, GROUND],
        Device::VoltageSource(drive("c")),
    ));
    c.add(Element::new(
        "VE",
        [e, GROUND],
        Device::VoltageSource(drive("e")),
    ));
    c.add(Element::new("Q1", [col, b, e], Device::bjt(model.clone())));
    let f = 1e6;
    let source = if driven == "c" { "I(VC)" } else { "I(VE)" };
    let (_, im) = phasor(&simulate_ok(&c, &ac_at(f)), source);
    -im / (2.0 * PI * f)
}

/// VCE (magnitude) with the collector current and base current forced.
fn saturation_voltage(model: &BjtModel, ic: f64, ib: f64) -> f64 {
    let s = sign(model.polarity);
    let mut c = Circuit::new();
    let b = c.node("b");
    let col = c.node("c");
    c.add(Element::new(
        "IB",
        [GROUND, b],
        Device::CurrentSource(Source::dc(s * ib)),
    ));
    c.add(Element::new(
        "IC",
        [GROUND, col],
        Device::CurrentSource(Source::dc(s * ic)),
    ));
    c.add(Element::new(
        "Q1",
        [col, b, GROUND],
        Device::bjt(model.clone()),
    ));
    s * value(&operating_point(&c), "V(c)")
}

#[test]
fn every_bjt_has_targets() {
    let named: BTreeSet<_> = BJT_TARGETS.iter().map(|t| t.name).collect();
    for e in library::bjts() {
        assert!(named.contains(e.model.name.as_str()), "{}", e.model.name);
    }
}

#[test]
fn bjts_meet_their_dc_targets() {
    for t in BJT_TARGETS {
        let m = library_bjt(t.name);
        let (ic, vbe) = t.vbe;
        let (_, got) = bjt_bias(&m, base_current_for(&m, ic, 5.0), 5.0);
        assert!(
            (got - vbe).abs() <= 0.01,
            "{}: VBE = {got:.4} V at {ic} A, target {vbe} V",
            t.name
        );
        for (ic, beta) in t.beta {
            assert_within(
                beta_at(&m, ic),
                beta,
                0.05,
                &format!("{}: β at {ic} A", t.name),
            );
        }
        assert_within(
            early_voltage(&m, t.vbe.0),
            t.vaf,
            0.03,
            &format!("{}: Early voltage", t.name),
        );
        let (ic, ib, vce) = t.saturation;
        assert_within(
            saturation_voltage(&m, ic, ib),
            vce,
            0.15,
            &format!("{}: VCE(sat)", t.name),
        );
    }
}

#[test]
fn bjts_meet_their_small_signal_targets() {
    for t in BJT_TARGETS {
        let m = library_bjt(t.name);
        let (ic, ft) = t.ft;
        assert_within(
            transition_frequency(&m, ic, ft),
            ft,
            0.1,
            &format!("{}: fT at {ic} A", t.name),
        );
        assert_within(
            junction_capacitance(&m, "e"),
            t.cje,
            0.03,
            &format!("{}: CJE", t.name),
        );
        assert_within(
            junction_capacitance(&m, "c"),
            t.cjc,
            0.03,
            &format!("{}: CJC", t.name),
        );
    }
}

// ------------------------------------------------------------ JFET targets

/// Design targets of one JFET (magnitudes, VDS = 10 V).
struct JfetTargets {
    name: &'static str,
    /// Checked to ±3 %.
    idss: f64,
    /// Pinch-off voltage magnitude: VGS where ID falls to 1 nA, ±20 mV.
    vp: f64,
    /// Channel-length modulation (1/V) at VGS = 0.75·VP, ±5 %.
    lambda: f64,
    /// Transconductance at VGS = 0, ±5 %.
    gfs: f64,
    /// CGS + CGD at zero bias, ±3 %.
    ciss: f64,
}

const JFET_TARGETS: &[JfetTargets] = &[
    JfetTargets {
        name: "NJF_LOW",
        idss: 1e-3,
        vp: 0.8,
        lambda: 0.01,
        gfs: 2.5e-3,
        ciss: 4.5e-12,
    },
    JfetTargets {
        name: "NJF_MID",
        idss: 5e-3,
        vp: 2.0,
        lambda: 0.02,
        gfs: 5e-3,
        ciss: 5.5e-12,
    },
    JfetTargets {
        name: "NJF_HIGH",
        idss: 20e-3,
        vp: 3.0,
        lambda: 0.025,
        gfs: 13e-3,
        ciss: 8.5e-12,
    },
    JfetTargets {
        name: "PJF_MID",
        idss: 5e-3,
        vp: 2.0,
        lambda: 0.02,
        gfs: 5e-3,
        ciss: 8.5e-12,
    },
    JfetTargets {
        name: "PJF_HIGH",
        idss: 15e-3,
        vp: 3.0,
        lambda: 0.025,
        gfs: 9.5e-3,
        ciss: 14e-12,
    },
];

/// Drain current magnitude at the gate-source and drain-source voltage
/// magnitudes `vgs` (N-type orientation: negative closes the channel) and
/// `vds`.
fn jfet_drain_current(model: &JfetModel, vgs: f64, vds: f64) -> f64 {
    let s = sign(model.polarity);
    let mut c = Circuit::new();
    let g = c.node("g");
    let d = c.node("d");
    c.add(Element::new(
        "VGS",
        [g, GROUND],
        Device::VoltageSource(Source::dc(s * vgs)),
    ));
    c.add(Element::new(
        "VDS",
        [d, GROUND],
        Device::VoltageSource(Source::dc(s * vds)),
    ));
    c.add(Element::new(
        "J1",
        [d, g, GROUND],
        Device::jfet(model.clone()),
    ));
    s * value(&operating_point(&c), "Id(J1)")
}

/// Gate-source voltage (N-type orientation) where the drain current falls to
/// `current` at VDS = 10 V.
fn cutoff_voltage(model: &JfetModel, current: f64) -> f64 {
    let (mut lo, mut hi) = (-10.0, 0.0);
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        if jfet_drain_current(model, mid, 10.0) > current {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    0.5 * (lo + hi)
}

/// CGS + CGD seen at the gate with drain and source at AC ground.
fn input_capacitance(model: &JfetModel) -> f64 {
    let mut c = Circuit::new();
    let g = c.node("g");
    let d = c.node("d");
    c.add(Element::new(
        "VG",
        [g, GROUND],
        Device::VoltageSource(Source::ac(0.0, 1.0, 0.0)),
    ));
    c.add(Element::new(
        "VD",
        [d, GROUND],
        Device::VoltageSource(Source::dc(0.0)),
    ));
    c.add(Element::new(
        "J1",
        [d, g, GROUND],
        Device::jfet(model.clone()),
    ));
    let f = 1e6;
    let (_, im) = phasor(&simulate_ok(&c, &ac_at(f)), "I(VG)");
    -im / (2.0 * PI * f)
}

#[test]
fn every_jfet_has_targets() {
    let named: BTreeSet<_> = JFET_TARGETS.iter().map(|t| t.name).collect();
    for e in library::jfets() {
        assert!(named.contains(e.model.name.as_str()), "{}", e.model.name);
    }
}

#[test]
fn jfets_meet_their_targets() {
    for t in JFET_TARGETS {
        let m = library_jfet(t.name);
        let n = t.name;
        assert_within(
            jfet_drain_current(&m, 0.0, 10.0),
            t.idss,
            0.03,
            &format!("{n}: IDSS"),
        );
        let vp = -cutoff_voltage(&m, 1e-9);
        assert!(
            (vp - t.vp).abs() <= 0.02,
            "{n}: VP = {vp} V, target {}",
            t.vp
        );
        let vgs = -0.75 * t.vp;
        let ratio = jfet_drain_current(&m, vgs, 15.0) / jfet_drain_current(&m, vgs, 5.0);
        let lambda = (ratio - 1.0) / (15.0 - 5.0 * ratio);
        assert_within(lambda, t.lambda, 0.05, &format!("{n}: λ"));
        let gfs = (jfet_drain_current(&m, 5e-3, 10.0) - jfet_drain_current(&m, -5e-3, 10.0)) / 1e-2;
        assert_within(gfs, t.gfs, 0.05, &format!("{n}: gfs"));
        assert_within(input_capacitance(&m), t.ciss, 0.03, &format!("{n}: Ciss"));
    }
}

// ---------------------------------------------------------------------------
// Op-amps (D26): each entry's targets by simulation.

/// Supplies ±15 V and the op-amp `U1` on `[in+, in−, out]`.
fn opamp_circuit(model: OpAmpModel, input: Source, follower: bool) -> Circuit {
    let mut c = Circuit::new();
    let vp = c.node("vp");
    let vn = c.node("vn");
    let p = c.node("p");
    let out = c.node("out");
    c.add(Element::new(
        "VP",
        [vp, GROUND],
        Device::VoltageSource(Source::dc(15.0)),
    ));
    c.add(Element::new(
        "VN",
        [vn, GROUND],
        Device::VoltageSource(Source::dc(-15.0)),
    ));
    c.add(Element::new(
        "VIN",
        [p, GROUND],
        Device::VoltageSource(input),
    ));
    let n = if follower { out } else { GROUND };
    c.add(Element::new(
        "U1",
        [p, n, out, vp, vn],
        Device::OpAmp { model },
    ));
    c
}

#[test]
fn every_opamp_meets_its_gain_bandwidth_slew_and_swing_targets() {
    let entries = library::opamps();
    let kinds: BTreeSet<_> = entries.iter().map(leaf).collect();
    for kind in ["Ideal", "General purpose", "Fast", "Rail-to-rail"] {
        assert!(kinds.contains(kind), "no {kind} op-amp");
    }
    // Targets: [AOL, GBW, slew, RIN, ROUT, headroom] as the sources state.
    let targets = [
        (
            "OA_IDEAL",
            [1e6, f64::INFINITY, f64::INFINITY, f64::INFINITY, 1e-3, 0.0],
        ),
        ("OA_GP", [1e5, 1e6, 0.5e6, 2e6, 75.0, 1.5]),
        ("OA_FAST", [1e5, 10e6, 10e6, 1e6, 50.0, 1.5]),
        ("OA_RRIO", [3.16e5, 3e6, 2e6, 10e6, 30.0, 0.05]),
    ];
    assert_eq!(entries.len(), targets.len());
    for (name, [aol, gbw, slew, rin, rout, headroom]) in targets {
        let model = library::opamp(name).expect(name);
        let params = [model.aol, model.gbw, model.slew, model.rin, model.rout];
        assert_eq!(params, [aol, gbw, slew, rin, rout], "{name}");
        assert_eq!([model.headroom_pos, model.headroom_neg], [headroom; 2]);

        // Open-loop DC gain: 10 µV in.
        let r = operating_point(&opamp_circuit(model.clone(), Source::dc(10e-6), false));
        assert_within(
            value(&r, "V(out)") / 10e-6,
            aol,
            1e-9,
            &format!("{name} AOL"),
        );

        // Output swing: overdriven both ways, no load.
        let ideal = model.kind == OpAmpKind::Ideal;
        let swing = if ideal { 0.0 } else { headroom };
        for (drive, limit) in [(1.0, 15.0 - swing), (-1.0, swing - 15.0)] {
            let r = operating_point(&opamp_circuit(model.clone(), Source::dc(drive), false));
            assert_within(value(&r, "V(out)"), limit, 1e-9, &format!("{name} swing"));
        }
        if ideal {
            assert!(model.gbw.is_infinite() && model.slew.is_infinite());
            continue;
        }

        // Unity-gain crossing of the open-loop gain at GBW.
        let c = opamp_circuit(model.clone(), Source::ac(0.0, 1.0, 0.0), false);
        let (re, im) = phasor(&simulate_ok(&c, &ac_at(gbw)), "V(out)");
        assert_within(re.hypot(im), 1.0, 1e-6, &format!("{name} |A(GBW)|"));

        // Slew: a follower driven by a 10 V step ramps at `slew` (less the
        // rout/rin leak to the input), timed between 2 V and 6 V.
        let step = Source::with_waveform(Waveform::Pulse {
            initial: 0.0,
            pulsed: 10.0,
            delay: 0.0,
            rise: 1e-9,
            fall: 1e-9,
            width: 1.0,
            period: 2.0,
            cycles: None,
        });
        let duration = 20.0 / slew;
        let spec = Analysis::Transient(Transient {
            stop: duration,
            step: duration / 200.0,
            start_save: 0.0,
            max_step: None,
            initial_state: InitialState::OperatingPoint,
        });
        let r = simulate_ok(&opamp_circuit(model.clone(), step, true), &spec);
        let t = &r.axis.values;
        let v = r.signal("V(out)").and_then(Signal::real).expect("V(out)");
        let when = |level: f64| {
            let k = v.iter().position(|&x| x >= level).expect("crossing");
            t[k - 1] + (level - v[k - 1]) / (v[k] - v[k - 1]) * (t[k] - t[k - 1])
        };
        let measured = 4.0 / (when(6.0) - when(2.0));
        assert_within(
            measured * (1.0 + rout / rin),
            slew,
            1e-6,
            &format!("{name} slew"),
        );
    }
}
