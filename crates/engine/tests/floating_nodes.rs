//! Nodes without a DC path to ground (D15, SR-6, INT-3): a node reached only
//! through capacitors is tied to ground through gmin for every operating
//! point (plain OP, DC sweep points, the AC linearisation point and the
//! transient's starting point) with a warning naming it, while a node fed
//! only by a current source is an error that names the node and the reason.
//!
//! LTspice 24.1.9 values come from the decks in `tests/reference/`
//! (`floating_*.cir`). LTspice keeps its gmin leak during the transient too,
//! which costs it 2.5e-7 V at 1 ms in the capacitive divider; D15 ties the
//! node for the operating point only, so xsim's transient is exactly 0.5.

use xsim_spice_engine::*;

fn pulse(delay: f64, period: f64) -> Source {
    let mut s = Source::with_waveform(Waveform::Pulse {
        initial: 0.0,
        pulsed: 1.0,
        delay,
        rise: 1e-6,
        fall: 1e-6,
        width: 1.0,
        period,
        cycles: None,
    });
    s.ac_magnitude = 1.0;
    s
}

/// `V1 a 0`, `C1 a m 1n`, `C2 m 0 1n`, `R1 a 0 1k`: node m is reached only
/// through capacitors.
fn capacitive_divider(source: Source) -> Circuit {
    let mut c = Circuit::new();
    let (a, m) = (c.node("a"), c.node("m"));
    c.add(Element::new(
        "V1",
        [a, GROUND],
        Device::VoltageSource(source),
    ));
    c.add(Element::new("C1", [a, m], Device::capacitor(1e-9)));
    c.add(Element::new("C2", [m, GROUND], Device::capacitor(1e-9)));
    c.add(Element::new("R1", [a, GROUND], Device::resistor(1e3)));
    c
}

fn tied_warning(node: &str) -> String {
    format!(
        "Node {node} has no DC path to ground (it is reached only through capacitors); it was \
         tied to ground through gmin = 1 pS for the operating point"
    )
}

fn run(
    circuit: &Circuit,
    analysis: &Analysis,
) -> (Result<SimulationResult, SimError>, Vec<String>) {
    let control = SimControl::new();
    let result = simulate(circuit, analysis, &SimOptions::default(), &control);
    (result, control.warnings())
}

fn transient(stop: f64, step: f64, max_step: Option<f64>, initial_state: InitialState) -> Analysis {
    Analysis::Transient(Transient {
        stop,
        step,
        start_save: 0.0,
        max_step,
        initial_state,
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

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "expected {expected}, got {actual} (tolerance {tolerance})"
    );
}

#[test]
fn a_capacitive_divider_has_an_operating_point_with_a_warning() {
    let mut source = Source::dc(1.0);
    source.ac_magnitude = 1.0;
    for solver in [SolverKind::Dense, SolverKind::Sparse] {
        let control = SimControl::new();
        let options = SimOptions {
            solver,
            ..SimOptions::default()
        };
        let r = simulate(
            &capacitive_divider(source.clone()),
            &Analysis::OperatingPoint,
            &options,
            &control,
        )
        .unwrap();
        // LTspice: "WARNING: Node m is floating.", V(m) = 9.99999998e-10.
        close(r.signal("V(m)").unwrap().real().unwrap()[0], 0.0, 1e-9);
        assert_eq!(control.warnings(), [tied_warning("m")]);
    }
}

#[test]
fn a_capacitive_divider_has_an_ac_response() {
    // The AC linearisation point ties m; the AC system itself is untouched.
    let mut source = Source::dc(1.0);
    source.ac_magnitude = 1.0;
    let analysis = Analysis::Ac(AcSweep {
        scale: SweepScale::Decade,
        points: 1,
        start: 1.0,
        stop: 1e6,
    });
    let (r, warnings) = run(&capacitive_divider(source), &analysis);
    let r = r.unwrap();
    let SignalData::Complex { re, im } = &r.signal("V(m)").unwrap().data else {
        panic!("AC data is complex");
    };
    // LTspice: −6.02059991327962 dB at 0° at 1 Hz, 1 kHz and 1 MHz.
    let ltspice = 10f64.powf(-6.020_599_913_279_62 / 20.0);
    for (k, &f) in r.axis.values.iter().enumerate() {
        close(re[k].hypot(im[k]), ltspice, 1e-12);
        close(im[k], 0.0, 1e-15);
        assert!(f > 0.0);
    }
    assert_eq!(warnings, [tied_warning("m")]);
}

#[test]
fn a_capacitive_divider_runs_a_transient_from_its_operating_point() {
    let analysis = transient(1e-3, 1e-5, None, InitialState::OperatingPoint);
    let (r, warnings) = run(&capacitive_divider(pulse(0.0, 2.0)), &analysis);
    let r = r.unwrap();
    // LTspice: V(m) = 0 at t = 0 and 0.499999750125063 at 1 ms.
    assert_eq!(at(&r, "V(m)", 0.0), 0.0);
    close(at(&r, "V(m)", 1e-3), 0.499_999_750_125_063, 1e-6);
    close(at(&r, "V(m)", 1e-3), 0.5, 1e-12);
    assert_eq!(warnings, [tied_warning("m")]);
}

#[test]
fn series_capacitors_run_a_transient_from_their_operating_point() {
    // INT-3: V1 a 0 PULSE(0 1 0.1m 1u 1u 1 0); R1 a 0 1k; C1 f 0 1u; C2 f a 1u.
    let mut c = Circuit::new();
    let (a, f) = (c.node("a"), c.node("f"));
    c.add(Element::new(
        "V1",
        [a, GROUND],
        Device::VoltageSource(pulse(0.1e-3, 0.0)),
    ));
    c.add(Element::new("R1", [a, GROUND], Device::resistor(1e3)));
    c.add(Element::new("C1", [f, GROUND], Device::capacitor(1e-6)));
    c.add(Element::new("C2", [f, a], Device::capacitor(1e-6)));
    let analysis = transient(1e-3, 1e-6, Some(1e-6), InitialState::OperatingPoint);
    let (r, warnings) = run(&c, &analysis);
    let r = r.unwrap();
    // LTspice: 0 at t = 0, 0.499999999900117 at 0.5 ms, 0.499999999775107 at 1 ms.
    assert_eq!(at(&r, "V(f)", 0.0), 0.0);
    close(at(&r, "V(f)", 0.5e-3), 0.499_999_999_900_117, 1e-9);
    close(at(&r, "V(f)", 1e-3), 0.499_999_999_775_107, 1e-9);
    assert_eq!(warnings, [tied_warning("f")]);
}

#[test]
fn a_capacitor_initial_condition_holds_the_node_at_the_transient_start() {
    // D16: C1's initial voltage constrains the starting operating point, so
    // m is not floating there: V(m) = V(a) − 0.5 V, and nothing is tied. The
    // plain operating point ignores initial conditions and ties m.
    let mut c = capacitive_divider(Source::dc(1.0));
    c.elements[1].device = Device::Capacitor {
        capacitance: 1e-9,
        initial_voltage: Some(0.5),
    };
    let analysis = transient(1e-6, 1e-7, None, InitialState::OperatingPoint);
    let (r, warnings) = run(&c, &analysis);
    let r = r.unwrap();
    close(at(&r, "V(m)", 0.0), 0.5, 1e-6);
    assert!(
        warnings.iter().all(|w| !w.contains("Node m")),
        "{warnings:?}"
    );
    let (r, warnings) = run(&c, &Analysis::OperatingPoint);
    assert_eq!(r.unwrap().signal("V(m)").unwrap().real().unwrap()[0], 0.0);
    assert_eq!(warnings, [tied_warning("m")]);
}

#[test]
fn every_dc_sweep_point_ties_the_node_and_warns_once() {
    let sweep = Analysis::DcSweep(DcSweep {
        source: "V1".into(),
        start: -1.0,
        stop: 1.0,
        step: 0.5,
    });
    let (r, warnings) = run(&capacitive_divider(Source::dc(0.0)), &sweep);
    let r = r.unwrap();
    assert_eq!(r.signal("V(m)").unwrap().real().unwrap(), [0.0; 5]);
    assert_eq!(
        r.signal("V(a)").unwrap().real().unwrap(),
        [-1.0, -0.5, 0.0, 0.5, 1.0]
    );
    assert_eq!(warnings, [tied_warning("m")]);
}

#[test]
fn the_tie_uses_the_gmin_option() {
    let options = SimOptions {
        gmin: 1e-9,
        ..SimOptions::default()
    };
    let control = SimControl::new();
    simulate(
        &capacitive_divider(Source::dc(1.0)),
        &Analysis::OperatingPoint,
        &options,
        &control,
    )
    .unwrap();
    assert!(
        control.warnings()[0].contains("gmin = 1 nS"),
        "{:?}",
        control.warnings()
    );
}

/// `V1 a 0 1`, `R1 a 0 1k`, `I1 0 → n 1 mA`, `C1 n 0 1µ`: node n is fed only
/// by a current source.
fn current_fed_capacitor() -> Circuit {
    let mut c = Circuit::new();
    let (a, n) = (c.node("a"), c.node("n"));
    c.add(Element::new(
        "V1",
        [a, GROUND],
        Device::VoltageSource(Source::dc(1.0)),
    ));
    c.add(Element::new("R1", [a, GROUND], Device::resistor(1e3)));
    c.add(Element::new(
        "I1",
        [GROUND, n],
        Device::CurrentSource(Source::dc(1e-3)),
    ));
    c.add(Element::new("C1", [n, GROUND], Device::capacitor(1e-6)));
    c
}

#[test]
fn a_node_fed_only_by_a_current_source_is_an_error_everywhere_it_needs_an_operating_point() {
    let reason = "node n has no DC path to ground (only current source I1 feeds it, so its \
                  voltage is undefined)";
    let ac = Analysis::Ac(AcSweep {
        scale: SweepScale::Decade,
        points: 1,
        start: 1.0,
        stop: 10.0,
    });
    let sweep = Analysis::DcSweep(DcSweep {
        source: "V1".into(),
        start: 2.0,
        stop: 3.0,
        step: 1.0,
    });
    let tran = transient(1e-3, 1e-5, None, InitialState::OperatingPoint);
    for (analysis, at) in [
        (Analysis::OperatingPoint, None),
        (ac, None),
        (sweep, Some(FailurePoint::sweep("V1", 2.0, "V"))),
        (tran, Some(FailurePoint::time(0.0))),
    ] {
        let (error, warnings) = run(&current_fed_capacitor(), &analysis);
        assert_eq!(
            error.unwrap_err(),
            SimError::SingularMatrix {
                unknown: "V(n)".into(),
                reason: reason.into(),
                at,
                progress: 0.0,
            },
            "{analysis:?}"
        );
        // Tying n to ground would turn 1 mA into a 1e9 V operating point.
        assert!(warnings.is_empty());
    }
    let (error, _) = run(
        &current_fed_capacitor(),
        &transient(1e-3, 1e-5, None, InitialState::OperatingPoint),
    );
    assert_eq!(
        error.unwrap_err().to_string(),
        format!("Singular matrix at V(n) when t = 0 s (0.0% complete): {reason}")
    );
}

#[test]
fn a_node_floating_even_through_capacitors_is_named_with_the_time() {
    // Starting from rest skips the operating point; the first step then
    // finds node n with no path to ground at all.
    let mut c = current_fed_capacitor();
    c.elements.pop();
    let (error, _) = run(&c, &transient(1e-3, 1e-5, None, InitialState::ZeroState));
    let error = error.unwrap_err();
    let SimError::SingularMatrix {
        unknown,
        reason,
        at: Some(at),
        ..
    } = &error
    else {
        panic!("expected a singular matrix with a time, got {error:?}");
    };
    assert_eq!(unknown, "V(n)");
    assert_eq!(
        reason,
        "node n has no path to ground, not even through capacitors (only current source I1 \
         feeds it, so its voltage is undefined)"
    );
    assert_eq!((at.quantity.as_str(), at.unit.as_str()), ("t", "s"));
    assert!(at.value > 0.0 && at.value < 1e-3, "{error}");
}
