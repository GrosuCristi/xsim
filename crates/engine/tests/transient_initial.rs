//! Capacitor and inductor initial conditions (D16, T5) and the `t = 0`
//! sample of a start from rest (T6). LTspice values come from batch runs of
//! `tests/reference/tran_ic_op.cir` (no `uic`) and `tran_ic_uic.cir` (LTspice
//! 24.1.9).

use xsim_spice_engine::*;

const METHODS: [IntegrationMethod; 4] = [
    IntegrationMethod::BackwardEuler,
    IntegrationMethod::Trapezoidal,
    IntegrationMethod::ModifiedTrapezoidal,
    IntegrationMethod::Gear2,
];

fn transient(stop: f64, step: f64, initial_state: InitialState) -> Analysis {
    Analysis::Transient(Transient {
        stop,
        step,
        start_save: 0.0,
        max_step: None,
        initial_state,
    })
}

fn options(method: IntegrationMethod) -> SimOptions {
    SimOptions {
        method,
        // Backward Euler needs ~1/reltol steps per time constant.
        reltol: if method == IntegrationMethod::BackwardEuler {
            1e-4
        } else {
            1e-6
        },
        ..SimOptions::default()
    }
}

fn samples<'a>(result: &'a SimulationResult, name: &str) -> &'a [f64] {
    result
        .signal(name)
        .unwrap_or_else(|| panic!("no signal {name}"))
        .real()
        .unwrap()
}

fn at(result: &SimulationResult, name: &str, t: f64) -> f64 {
    let k = result
        .axis
        .values
        .iter()
        .position(|&x| x == t)
        .unwrap_or_else(|| panic!("no sample at {t}"));
    samples(result, name)[k]
}

fn capacitor(capacitance: f64, initial_voltage: Option<f64>) -> Device {
    Device::Capacitor {
        capacitance,
        initial_voltage,
    }
}

fn inductor(inductance: f64, initial_current: Option<f64>) -> Device {
    Device::Inductor {
        inductance,
        initial_current,
    }
}

/// The T5 circuit (`tran_ic_op.cir`): 1 V → 1 kΩ → C1 (IC 2 V); C2 (IC 2 V)
/// ∥ 1 kΩ; L1 (IC 1 mA) ∥ 1 Ω. Every time constant is 1 ms.
fn precharged() -> Circuit {
    let mut c = Circuit::new();
    let (input, out, d, n) = (c.node("in"), c.node("out"), c.node("d"), c.node("n"));
    c.add(Element::new(
        "V1",
        [input, GROUND],
        Device::VoltageSource(Source::dc(1.0)),
    ));
    c.add(Element::new("R1", [input, out], Device::resistor(1e3)));
    c.add(Element::new(
        "C1",
        [out, GROUND],
        capacitor(1e-6, Some(2.0)),
    ));
    c.add(Element::new("R2", [d, GROUND], Device::resistor(1e3)));
    c.add(Element::new("C2", [d, GROUND], capacitor(1e-6, Some(2.0))));
    c.add(Element::new("L1", [n, GROUND], inductor(1e-3, Some(1e-3))));
    c.add(Element::new("R3", [n, GROUND], Device::resistor(1.0)));
    c
}

/// The exact response of [`precharged`] from its initial conditions.
fn precharged_exact(name: &str, t: f64) -> f64 {
    let decay = (-t / 1e-3).exp();
    match name {
        "V(in)" => 1.0,
        "V(out)" => 1.0 + decay,
        "V(d)" => 2.0 * decay,
        "V(n)" => -1e-3 * decay,
        "I(L1)" => 1e-3 * decay,
        "I(C1)" => -1e-3 * decay,
        "I(C2)" => -2e-3 * decay,
        _ => unreachable!(),
    }
}

#[test]
fn initial_conditions_constrain_the_operating_point_start() {
    // LTspice without `uic` (tran_ic_op.cir) at 1 ms and 2 ms. Its t = 0
    // values (V(out) = 1.999999000001, I(L1) = 0.999989999 mA) carry its
    // 1 mΩ / 100 kΩ initial-condition stiffness; xsim holds the ICs exactly.
    let ltspice = [
        ("V(out)", 1e-3, 1.36787944994492),
        ("V(d)", 1e-3, 0.735758899890065),
        ("I(L1)", 1e-3, 0.000367879449945088),
        ("V(out)", 2e-3, 1.13533527518613),
        ("V(d)", 2e-3, 0.27067055037255),
        ("I(L1)", 2e-3, 0.000135335275321652),
    ];
    let control = SimControl::new();
    for method in METHODS {
        let r = simulate(
            &precharged(),
            &transient(3e-3, 1e-4, InitialState::OperatingPoint),
            &options(method),
            &control,
        )
        .unwrap_or_else(|e| panic!("{method:?}: {e}"));
        assert!(control.warnings().is_empty(), "{:?}", control.warnings());
        for name in ["V(in)", "V(out)", "V(d)", "V(n)", "I(L1)", "I(C1)", "I(C2)"] {
            let value = samples(&r, name)[0];
            let exact = precharged_exact(name, 0.0);
            assert!(
                (value - exact).abs() <= 1e-9 * exact.abs(),
                "{method:?}: {name} at t = 0 is {value}, expected {exact}"
            );
        }
        let tolerance = if method == IntegrationMethod::BackwardEuler {
            1e-3
        } else {
            1e-6
        };
        for (name, t, lt) in ltspice {
            let value = at(&r, name, t);
            let exact = precharged_exact(name, t);
            assert!(
                (value - exact).abs() <= tolerance * exact.abs(),
                "{method:?}: {name} at {t} is {value}, expected {exact}"
            );
            assert!(
                (value - lt).abs() <= 1e-6 * lt.abs() + tolerance * exact.abs(),
                "{method:?}: {name} at {t} is {value}, LTspice {lt}"
            );
        }
    }
}

#[test]
fn an_initial_voltage_held_by_a_voltage_source_is_reported() {
    // C2 (IC 2 V) directly across a 1 V source: the source wins, as in
    // LTspice, and the run says so.
    let mut c = Circuit::new();
    let s = c.node("s");
    c.add(Element::new(
        "V2",
        [s, GROUND],
        Device::VoltageSource(Source::dc(1.0)),
    ));
    c.add(Element::new("C2", [s, GROUND], capacitor(1e-6, Some(2.0))));
    c.add(Element::new("R1", [s, GROUND], Device::resistor(1e3)));
    let control = SimControl::new();
    let r = simulate(
        &c,
        &transient(1e-3, 1e-4, InitialState::OperatingPoint),
        &SimOptions::default(),
        &control,
    )
    .unwrap();
    assert!(samples(&r, "V(s)").iter().all(|&v| (v - 1.0).abs() < 1e-12));
    let warnings = control.warnings();
    assert!(
        warnings.len() == 1 && warnings[0].contains("C2") && warnings[0].contains("2 V"),
        "{warnings:?}"
    );
    // The overridden hold is released before the start is recorded, so its
    // holding current (1 V / 1 pΩ = 1e12 A) shows nowhere: not in C2, not
    // in the source, which carries only R1's 1 mA.
    assert!(samples(&r, "I(C2)").iter().all(|i| i.abs() < 1e-9));
    assert!(
        samples(&r, "I(V2)").iter().all(|i| (i + 1e-3).abs() < 1e-9),
        "{:?}",
        &samples(&r, "I(V2)")[..2]
    );
}

#[test]
fn a_precharged_capacitor_shorted_by_an_inductor_is_released_with_a_warning() {
    // An LC tank whose capacitor alone has an initial voltage: at the
    // operating point the inductor shorts it. Its hold used to push
    // 1 V / 1 pΩ = 1e12 A into the inductor, which then rang at 3e13 V.
    // Now the run warns and starts from the short's 0 V and 0 A; from rest
    // the same tank rings at the initial voltage.
    let mut c = Circuit::new();
    let n = c.node("n");
    c.add(Element::new("L1", [n, GROUND], inductor(1e-3, None)));
    c.add(Element::new("C1", [n, GROUND], capacitor(1e-6, Some(1.0))));
    let control = SimControl::new();
    let r = simulate(
        &c,
        &transient(1e-3, 1e-5, InitialState::OperatingPoint),
        &SimOptions::default(),
        &control,
    )
    .unwrap();
    let worst = |name: &str| {
        samples(&r, name)
            .iter()
            .fold(0.0_f64, |worst, v| worst.max(v.abs()))
    };
    assert!(worst("V(n)") < 1e-9, "V(n) reaches {:e}", worst("V(n)"));
    assert!(worst("I(L1)") < 1e-9, "I(L1) reaches {:e}", worst("I(L1)"));
    let warnings = control.warnings();
    assert!(
        warnings.len() == 1
            && warnings[0].starts_with("C1: ")
            && warnings[0].contains("at 0 V")
            && warnings[0].contains("Start from rest"),
        "{warnings:?}"
    );
    let r = simulate(
        &c,
        &transient(1e-3, 1e-5, InitialState::ZeroState),
        &SimOptions::default(),
        &SimControl::new(),
    )
    .unwrap();
    // V(n) = cos(ω0·t) with ω0 = 1/√(LC).
    let w0 = 1.0 / (1e-3_f64 * 1e-6).sqrt();
    for (&t, &v) in r.axis.values.iter().zip(samples(&r, "V(n)")) {
        assert!((v - (w0 * t).cos()).abs() < 1e-5, "at {t}: {v}");
    }
}

#[test]
fn overridden_initial_conditions_are_released_one_at_a_time() {
    // C1 (IC 1 V) and C2 (IC 2 V) in series across 5 V: C1, the first in
    // element order, gives way and C2 keeps its 2 V. The holding current
    // (2 V / 2 pΩ) shows nowhere.
    let mut c = Circuit::new();
    let (a, b) = (c.node("a"), c.node("b"));
    c.add(Element::new(
        "V1",
        [a, GROUND],
        Device::VoltageSource(Source::dc(5.0)),
    ));
    c.add(Element::new("C1", [a, b], capacitor(1e-6, Some(1.0))));
    c.add(Element::new("C2", [b, GROUND], capacitor(1e-6, Some(2.0))));
    let control = SimControl::new();
    let r = simulate(
        &c,
        &transient(1e-3, 1e-4, InitialState::OperatingPoint),
        &SimOptions::default(),
        &control,
    )
    .unwrap();
    assert!((samples(&r, "V(b)")[0] - 2.0).abs() < 1e-9);
    assert!(samples(&r, "I(C1)")[0].abs() < 1e-9);
    let warnings = control.warnings();
    assert!(
        warnings.len() == 1 && warnings[0].starts_with("C1: ") && warnings[0].contains("at 3 V"),
        "{warnings:?}"
    );
    // L3 (IC 1 mA) in series with L4 (IC 2 mA) around C3 (IC 1 V): L3 gives
    // way to the 2 mA. Its gmin shunt used to carry the difference at
    // −5e8 V (LTspice: −49.5 V through its 100 kΩ shunts, 1.505 mA).
    let mut c = Circuit::new();
    let (p, q) = (c.node("p"), c.node("q"));
    c.add(Element::new("C3", [p, GROUND], capacitor(1e-6, Some(1.0))));
    c.add(Element::new("L3", [p, q], inductor(1e-3, Some(1e-3))));
    c.add(Element::new("L4", [q, GROUND], inductor(1e-3, Some(2e-3))));
    let control = SimControl::new();
    let r = simulate(
        &c,
        &transient(1e-3, 1e-5, InitialState::OperatingPoint),
        &SimOptions::default(),
        &control,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert!((samples(&r, "V(q)")[0] - 1.0).abs() < 1e-9);
    for name in ["I(L3)", "I(L4)"] {
        assert!((samples(&r, name)[0] - 2e-3).abs() < 1e-11, "{name}");
    }
    assert!(samples(&r, "V(q)").iter().all(|v| v.abs() < 2.0));
    let warnings = control.warnings();
    assert!(
        warnings.len() == 1 && warnings[0].starts_with("L3: ") && warnings[0].contains("2 mA"),
        "{warnings:?}"
    );
}

#[test]
fn an_initial_current_overridden_by_a_current_source_is_released() {
    // 1.1 mA from I1 through L1 (IC 1 mA) into 1 kΩ: the source wins. The
    // 0.1 mA difference used to flow through L1's gmin shunt at 1e8 V.
    let mut c = Circuit::new();
    let (a, b) = (c.node("a"), c.node("b"));
    c.add(Element::new(
        "I1",
        [GROUND, a],
        Device::CurrentSource(Source::dc(1.1e-3)),
    ));
    c.add(Element::new("L1", [a, b], inductor(1e-3, Some(1e-3))));
    c.add(Element::new("R1", [b, GROUND], Device::resistor(1e3)));
    let control = SimControl::new();
    let r = simulate(
        &c,
        &transient(1e-5, 1e-6, InitialState::OperatingPoint),
        &SimOptions::default(),
        &control,
    )
    .unwrap();
    assert!(samples(&r, "V(a)").iter().all(|v| (v - 1.1).abs() < 1e-9));
    assert!((samples(&r, "I(L1)")[0] - 1.1e-3).abs() < 1e-12);
    let warnings = control.warnings();
    assert!(
        warnings.len() == 1 && warnings[0].starts_with("L1: ") && warnings[0].contains("1.1 mA"),
        "{warnings:?}"
    );
    // The t = 0 sample of a start from rest holds an inductor without an IC
    // at 0 A; the same source overrides it there (it used to read 1.1e9 V).
    let mut c = c.clone();
    c.elements[1] = Element::new("L1", [a, b], inductor(1e-3, None));
    let r = simulate(
        &c,
        &transient(1e-5, 1e-6, InitialState::ZeroState),
        &SimOptions::default(),
        &SimControl::new(),
    )
    .unwrap();
    assert!((samples(&r, "V(a)")[0] - 1.1).abs() < 1e-9);
}

#[test]
fn a_start_from_rest_sample_shows_no_holding_current() {
    // C1 directly across V1 (supply decoupling) from rest: the t = 0 sample
    // used to hold C1 at 0 V against the source, 1e12 A in both. Released,
    // the source shows only R1's current into the discharged C2.
    let mut c = Circuit::new();
    let (s, o) = (c.node("s"), c.node("o"));
    c.add(Element::new(
        "V1",
        [s, GROUND],
        Device::VoltageSource(Source::dc(1.0)),
    ));
    c.add(Element::new("C1", [s, GROUND], capacitor(1e-6, None)));
    c.add(Element::new("R1", [s, o], Device::resistor(1e3)));
    c.add(Element::new("C2", [o, GROUND], capacitor(1e-6, None)));
    let control = SimControl::new();
    let r = simulate(
        &c,
        &transient(5e-3, 1e-4, InitialState::ZeroState),
        &SimOptions::default(),
        &control,
    )
    .unwrap();
    assert!(control.warnings().is_empty(), "{:?}", control.warnings());
    assert_eq!(samples(&r, "V(s)")[0], 1.0);
    assert!(samples(&r, "V(o)")[0].abs() < 1e-12);
    assert!(samples(&r, "I(C1)")[0].abs() < 1e-9);
    assert!((samples(&r, "I(V1)")[0] + 1e-3).abs() < 1e-9);
    let k = r.axis.values.iter().position(|&t| t == 1e-3).unwrap();
    let exact = -(-1.0_f64).exp_m1();
    assert!((samples(&r, "V(o)")[k] - exact).abs() < 1e-6);
}

#[test]
fn a_released_hold_leaves_a_nonlinear_operating_point_untouched() {
    // A diode biased from 5 V through 1 kΩ, with C1 (IC 3 V) across the
    // supply: after the release the start is the plain operating point, and
    // the only warning is C1's (no leftover from the conflicting solve).
    let mut c = Circuit::new();
    let (s, d) = (c.node("s"), c.node("d"));
    c.add(Element::new(
        "V1",
        [s, GROUND],
        Device::VoltageSource(Source::dc(5.0)),
    ));
    c.add(Element::new("C1", [s, GROUND], capacitor(1e-6, Some(3.0))));
    c.add(Element::new("R1", [s, d], Device::resistor(1e3)));
    c.add(Element::new(
        "D1",
        [d, GROUND],
        Device::diode(DiodeModel::default()),
    ));
    let op = simulate(
        &c,
        &Analysis::OperatingPoint,
        &SimOptions::default(),
        &SimControl::new(),
    )
    .unwrap();
    let control = SimControl::new();
    let r = simulate(
        &c,
        &transient(1e-4, 1e-5, InitialState::OperatingPoint),
        &SimOptions::default(),
        &control,
    )
    .unwrap();
    for name in ["V(s)", "V(d)", "I(V1)", "I(D1)"] {
        let (start, plain) = (samples(&r, name)[0], samples(&op, name)[0]);
        assert!(
            (start - plain).abs() <= 1e-9 * plain.abs() + 1e-15,
            "{name}: {start} vs {plain}"
        );
    }
    assert!(samples(&r, "I(C1)")[0].abs() < 1e-9);
    let warnings = control.warnings();
    assert!(
        warnings.len() == 1 && warnings[0].starts_with("C1: ") && warnings[0].contains("at 5 V"),
        "{warnings:?}"
    );
}

#[test]
fn an_initial_current_without_a_dc_path_is_a_precise_error() {
    // An LC tank whose inductor alone has an initial current: at the
    // operating point the capacitor is open, so the current has nowhere to
    // go. (LTspice starts it at V(n) = −99.99 V through its 100 kΩ shunt.)
    let mut c = Circuit::new();
    let n = c.node("n");
    c.add(Element::new("L1", [n, GROUND], inductor(1e-3, Some(1e-3))));
    c.add(Element::new("C1", [n, GROUND], capacitor(1e-6, None)));
    let error = simulate(
        &c,
        &transient(1e-3, 1e-4, InitialState::OperatingPoint),
        &SimOptions::default(),
        &SimControl::new(),
    )
    .unwrap_err();
    assert!(
        matches!(&error, SimError::InvalidCircuit(e) if e.message.starts_with("L1: ")),
        "{error}"
    );
    // From rest the same tank is well posed.
    let r = simulate(
        &c,
        &transient(1e-3, 1e-4, InitialState::ZeroState),
        &SimOptions::default(),
        &SimControl::new(),
    )
    .unwrap();
    assert!(samples(&r, "V(n)")[0].abs() < 1e-12);
    assert!((samples(&r, "I(L1)")[0] - 1e-3).abs() < 1e-15);
    assert!((samples(&r, "I(C1)")[0] + 1e-3).abs() < 1e-15);
}

#[test]
fn a_start_from_rest_shows_a_consistent_first_sample() {
    // The same circuit from rest: every state starts at its IC, so the
    // solution is the same as with the operating-point start, and the t = 0
    // sample shows it (LTspice uic shows these values from its first step
    // on, at 10 ps; its t = 0 row is all zero).
    for method in METHODS {
        let r = simulate(
            &precharged(),
            &transient(3e-3, 1e-4, InitialState::ZeroState),
            &options(method),
            &SimControl::new(),
        )
        .unwrap_or_else(|e| panic!("{method:?}: {e}"));
        for name in ["V(in)", "V(out)", "V(d)", "V(n)", "I(L1)", "I(C1)", "I(C2)"] {
            let value = samples(&r, name)[0];
            let exact = precharged_exact(name, 0.0);
            assert!(
                (value - exact).abs() <= 1e-9 * exact.abs(),
                "{method:?}: {name} at t = 0 is {value}, expected {exact}"
            );
        }
    }
    // A capacitor without an IC starts at 0 V, so the source-driven node
    // and the charging current show at t = 0.
    let mut c = Circuit::new();
    let (a, b) = (c.node("in"), c.node("out"));
    c.add(Element::new(
        "V1",
        [a, GROUND],
        Device::VoltageSource(Source::dc(1.0)),
    ));
    c.add(Element::new("R1", [a, b], Device::resistor(1e3)));
    c.add(Element::new("C1", [b, GROUND], capacitor(1e-6, None)));
    let r = simulate(
        &c,
        &transient(1e-3, 1e-4, InitialState::ZeroState),
        &SimOptions::default(),
        &SimControl::new(),
    )
    .unwrap();
    assert_eq!(samples(&r, "V(in)")[0], 1.0);
    assert!(samples(&r, "V(out)")[0].abs() < 1e-12);
    assert!((samples(&r, "I(C1)")[0] - 1e-3).abs() < 1e-12);
    assert!((samples(&r, "I(R1)")[0] - 1e-3).abs() < 1e-12);
}

#[test]
fn initial_conditions_leave_other_analyses_alone() {
    // The holding source of a capacitor with an initial voltage exists only
    // at the transient's start; the operating point and the AC response see
    // a plain capacitor.
    let op = simulate(
        &precharged(),
        &Analysis::OperatingPoint,
        &SimOptions::default(),
        &SimControl::new(),
    )
    .unwrap();
    assert!((samples(&op, "V(out)")[0] - 1.0).abs() < 1e-12);
    assert!(samples(&op, "V(d)")[0].abs() < 1e-12);
    assert_eq!(samples(&op, "I(C1)")[0], 0.0);
    let ac = simulate(
        &precharged(),
        &Analysis::Ac(AcSweep {
            scale: SweepScale::Decade,
            points: 5,
            start: 10.0,
            stop: 1e5,
        }),
        &SimOptions::default(),
        &SimControl::new(),
    );
    assert!(ac.is_ok(), "{ac:?}");
    // A start from rest with every initial condition at its default keeps
    // the plain zero start: nothing moves before the sources do.
    let mut c = Circuit::new();
    let n = c.node("n");
    c.add(Element::new("C1", [n, GROUND], capacitor(1e-6, Some(0.0))));
    c.add(Element::new("R1", [n, GROUND], Device::resistor(1e3)));
    let r = simulate(
        &c,
        &transient(1e-3, 1e-4, InitialState::OperatingPoint),
        &SimOptions::default(),
        &SimControl::new(),
    )
    .unwrap();
    assert!(samples(&r, "V(n)").iter().all(|v| v.abs() < 1e-15));
}

#[test]
fn a_zero_or_small_initial_current_with_a_voltage_across_it_is_well_posed() {
    // 5 V across L1 at the operating point: its gmin shunt carries 5 pA,
    // which is no reason to reject an initial current of 0 or 10 µA.
    for i0 in [0.0, 10e-6] {
        let mut c = Circuit::new();
        let (s, a) = (c.node("s"), c.node("a"));
        c.add(Element::new(
            "V1",
            [s, GROUND],
            Device::VoltageSource(Source::dc(5.0)),
        ));
        c.add(Element::new("R1", [s, a], Device::resistor(1e3)));
        c.add(Element::new("L1", [a, GROUND], inductor(1e-3, Some(i0))));
        let r = simulate(
            &c,
            &transient(5e-6, 1e-6, InitialState::OperatingPoint),
            &SimOptions::default(),
            &SimControl::new(),
        )
        .unwrap_or_else(|e| panic!("IC {i0}: {e}"));
        // i(t) = 5 mA − (5 mA − i0)·e^(−t/1 µs).
        let exact = |t: f64| 5e-3 - (5e-3 - i0) * (-t / 1e-6_f64).exp();
        for t in [0.0, 1e-6, 5e-6] {
            let value = at(&r, "I(L1)", t);
            assert!(
                (value - exact(t)).abs() < 1e-9,
                "IC {i0} at {t}: {value} vs {}",
                exact(t)
            );
        }
    }
}

#[test]
fn junction_charge_keeps_the_start_from_rest_dynamics() {
    // A diode with junction capacitance holds integrated charge the t = 0
    // solve cannot hold at rest, so the sample is the starting vector and
    // the run continues exactly as from rest.
    let mut c = Circuit::new();
    let (s, d) = (c.node("s"), c.node("d"));
    c.add(Element::new(
        "V1",
        [s, GROUND],
        Device::VoltageSource(Source::dc(1.0)),
    ));
    c.add(Element::new("R1", [s, d], Device::resistor(1e3)));
    let model = DiodeModel {
        cjo: 10e-12,
        ..DiodeModel::default()
    };
    c.add(Element::new("D1", [d, GROUND], Device::diode(model)));
    let r = simulate(
        &c,
        &transient(1e-6, 1e-7, InitialState::ZeroState),
        &SimOptions::default(),
        &SimControl::new(),
    )
    .unwrap();
    let v = samples(&r, "V(d)");
    assert_eq!(v[0], 0.0);
    // The junction charges through 1 kΩ: monotonic from 0.
    assert!(v.windows(2).all(|w| w[1] >= w[0] - 1e-12), "{v:?}");
    assert!(v[v.len() - 1] > 0.3);
}
