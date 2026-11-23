//! DC sweep: axis construction, linearity and validation.

use xsim_spice_engine::*;

fn sweep(
    circuit: &Circuit,
    source: &str,
    start: f64,
    stop: f64,
    step: f64,
) -> Result<SimulationResult, SimError> {
    let analysis = Analysis::DcSweep(DcSweep {
        source: source.into(),
        start,
        stop,
        step,
    });
    simulate(
        circuit,
        &analysis,
        &SimOptions::default(),
        &SimControl::new(),
    )
}

fn divider() -> Circuit {
    let mut c = Circuit::new();
    let (a, b) = (c.node("1"), c.node("2"));
    c.add(Element::new(
        "V1",
        [a, GROUND],
        Device::VoltageSource(Source::dc(1.0)),
    ));
    c.add(Element::new("R1", [a, b], Device::resistor(1e3)));
    c.add(Element::new("R2", [b, GROUND], Device::resistor(2e3)));
    c
}

fn samples<'a>(result: &'a SimulationResult, name: &str) -> &'a [f64] {
    result.signal(name).unwrap().real().unwrap()
}

#[test]
fn divider_sweep_obeys_ohms_law_and_source_polarity() {
    // Port of the prototype: a downward sweep through zero.
    let r = sweep(&divider(), "V1", 3.0, -3.0, -3.0).unwrap();
    assert_eq!(r.analysis, AnalysisKind::DcSweep);
    assert_eq!(r.axis.name, "V1");
    assert_eq!(r.axis.unit, "V");
    assert_eq!(r.axis.values, [3.0, 0.0, -3.0]);
    for (i, &v) in r.axis.values.iter().enumerate() {
        assert!((samples(&r, "V(2)")[i] - v * 2.0 / 3.0).abs() <= 1e-12);
        assert!((samples(&r, "I(V1)")[i] + v / 3000.0).abs() <= 1e-15);
        assert!((samples(&r, "I(R1)")[i] - v / 3000.0).abs() <= 1e-15);
    }
}

#[test]
fn sweeping_does_not_change_the_operating_point_value() {
    // The sweep varies a copy of the source; the circuit keeps its own DC value.
    let c = divider();
    sweep(&c, "V1", 0.0, 5.0, 1.0).unwrap();
    let op = simulate(
        &c,
        &Analysis::OperatingPoint,
        &SimOptions::default(),
        &SimControl::new(),
    )
    .unwrap();
    assert!((samples(&op, "V(2)")[0] - 2.0 / 3.0).abs() <= 1e-12);
}

#[test]
fn endpoints_are_included_even_for_a_partial_last_step() {
    let r = sweep(&divider(), "V1", 0.0, 1.0, 0.3).unwrap();
    assert_eq!(r.len(), 5);
    assert_eq!(r.axis.values[0], 0.0);
    assert_eq!(r.axis.values[4], 1.0);
    let tenths = sweep(&divider(), "V1", -1.0, 1.0, 0.1).unwrap();
    assert_eq!(tenths.len(), 21);
    assert_eq!(*tenths.axis.values.last().unwrap(), 1.0);
    assert_eq!(
        sweep(&divider(), "V1", 2.0, 2.0, 1.0).unwrap().axis.values,
        [2.0]
    );
}

#[test]
fn a_linear_network_responds_linearly() {
    // V(out) of a network with a fixed current source is affine in the
    // swept voltage, so its second differences vanish.
    let mut c = Circuit::new();
    let (a, b, d) = (c.node("a"), c.node("b"), c.node("d"));
    c.add(Element::new(
        "V1",
        [a, GROUND],
        Device::VoltageSource(Source::dc(0.0)),
    ));
    c.add(Element::new("R1", [a, b], Device::resistor(4.7e3)));
    c.add(Element::new("R2", [b, GROUND], Device::resistor(10e3)));
    c.add(Element::new("R3", [b, d], Device::resistor(2.2e3)));
    c.add(Element::new("R4", [d, GROUND], Device::resistor(1e3)));
    c.add(Element::new(
        "I1",
        [GROUND, d],
        Device::CurrentSource(Source::dc(1e-3)),
    ));
    let r = sweep(&c, "V1", -5.0, 5.0, 0.5).unwrap();
    let v = samples(&r, "V(d)");
    let slope = (v[1] - v[0]) / 0.5;
    for k in 1..v.len() - 1 {
        assert!((v[k + 1] - 2.0 * v[k] + v[k - 1]).abs() < 1e-12);
        assert!((v[k] - (v[0] + slope * 0.5 * k as f64)).abs() < 1e-12);
    }
}

#[test]
fn current_sources_sweep_in_amperes() {
    let mut c = Circuit::new();
    let n = c.node("n");
    c.add(Element::new(
        "I1",
        [GROUND, n],
        Device::CurrentSource(Source::dc(0.0)),
    ));
    c.add(Element::new("R1", [n, GROUND], Device::resistor(250.0)));
    let r = sweep(&c, "I1", 0.0, 4e-3, 1e-3).unwrap();
    assert_eq!(r.axis.unit, "A");
    for (i, &amps) in r.axis.values.iter().enumerate() {
        assert!((samples(&r, "V(n)")[i] - 250.0 * amps).abs() <= 1e-12);
    }
}

#[test]
fn progress_tracks_the_sweep() {
    let control = SimControl::new();
    let analysis = Analysis::DcSweep(DcSweep {
        source: "V1".into(),
        start: 0.0,
        stop: 1.0,
        step: 0.01,
    });
    simulate(&divider(), &analysis, &SimOptions::default(), &control).unwrap();
    assert_eq!(control.progress(), 1.0);
}

/// A diode fed through 1 kΩ by `V1`, with a current source `I1` into the
/// diode's node.
fn diode_circuit() -> Circuit {
    let mut c = Circuit::new();
    let (a, d) = (c.node("a"), c.node("d"));
    c.add(Element::new(
        "V1",
        [a, GROUND],
        Device::VoltageSource(Source::dc(0.0)),
    ));
    c.add(Element::new("R1", [a, d], Device::resistor(1e3)));
    c.add(Element::new(
        "D1",
        [d, GROUND],
        Device::diode(DiodeModel::default()),
    ));
    c.add(Element::new(
        "I1",
        [GROUND, d],
        Device::CurrentSource(Source::dc(0.0)),
    ));
    c
}

#[test]
fn a_failed_sweep_names_the_source_and_its_value_with_unit() {
    // One Newton iteration never confirms convergence of a nonlinear point.
    let options = SimOptions {
        dc_iterations: 1,
        ..SimOptions::default()
    };
    for (source, start, stop, step, point) in [
        ("V1", 2.5, 5.0, 2.5, "V1 = 2.5 V"),
        ("I1", 40e-15, 1e-3, 0.5e-3, "I1 = 40 fA"),
    ] {
        let analysis = Analysis::DcSweep(DcSweep {
            source: source.into(),
            start,
            stop,
            step,
        });
        let error =
            simulate(&diode_circuit(), &analysis, &options, &SimControl::new()).unwrap_err();
        let SimError::NoConvergence {
            at: Some(at),
            unknown: Some(unknown),
            ..
        } = &error
        else {
            panic!("expected non-convergence with a location, got {error:?}");
        };
        assert_eq!(at.quantity, source);
        assert_eq!(at.unit, if source == "V1" { "V" } else { "A" });
        assert!(unknown.starts_with("V("), "{error}");
        let text = error.to_string();
        assert!(
            text.starts_with(&format!(
                "Simulation did not converge at {point} near {unknown} (0.0% complete): "
            )),
            "{text}"
        );
    }
}

#[test]
fn a_sweep_with_no_operating_point_names_the_node_and_the_value() {
    let mut c = diode_circuit();
    let island = c.node("island");
    c.add(Element::new(
        "I2",
        [GROUND, island],
        Device::CurrentSource(Source::dc(1e-6)),
    ));
    let error = sweep(&c, "V1", 1.0, 2.0, 1.0).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Singular matrix at V(island) when V1 = 1 V (0.0% complete): node island has no DC \
         path to ground (only current source I2 feeds it, so its voltage is undefined)"
    );
}

#[test]
fn invalid_sweeps_are_rejected() {
    let c = divider();
    let invalid = |source: &str, start: f64, stop: f64, step: f64| {
        let error = sweep(&c, source, start, stop, step).unwrap_err();
        assert!(matches!(error, SimError::InvalidCircuit(_)), "{error}");
        error.to_string()
    };
    assert!(invalid("V1", 0.0, 1.0, 0.0).contains("step"));
    assert!(invalid("V1", 0.0, 1.0, -0.1).contains("moves away"));
    assert!(invalid("V1", 1.0, 0.0, 0.1).contains("moves away"));
    assert!(invalid("V9", 0.0, 1.0, 0.1).contains("V9"));
    assert!(invalid("R1", 0.0, 1.0, 0.1).contains("not an independent"));
    invalid("V1", 0.0, f64::INFINITY, 0.1);
    invalid("V1", 0.0, 1.0, 1e-9);
}
