//! DC operating point: analytic circuits, Kirchhoff's laws on random
//! networks, singular-matrix diagnostics and input validation.

use proptest::prelude::*;
use proptest::test_runner::RngSeed;
use xsim_spice_engine::*;

fn op(circuit: &Circuit) -> Result<SimulationResult, SimError> {
    simulate(
        circuit,
        &Analysis::OperatingPoint,
        &SimOptions::default(),
        &SimControl::new(),
    )
}

fn op_with(circuit: &Circuit, solver: SolverKind) -> Result<SimulationResult, SimError> {
    let options = SimOptions {
        solver,
        ..SimOptions::default()
    };
    simulate(
        circuit,
        &Analysis::OperatingPoint,
        &options,
        &SimControl::new(),
    )
}

fn value(result: &SimulationResult, name: &str) -> f64 {
    result
        .signal(name)
        .unwrap_or_else(|| panic!("no signal {name}"))
        .real()
        .unwrap()[0]
}

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "expected {expected}, got {actual} (tolerance {tolerance})"
    );
}

fn voltage_source(name: &str, p: NodeIndex, n: NodeIndex, volts: f64) -> Element {
    Element::new(name, [p, n], Device::VoltageSource(Source::dc(volts)))
}

fn current_source(name: &str, p: NodeIndex, n: NodeIndex, amps: f64) -> Element {
    Element::new(name, [p, n], Device::CurrentSource(Source::dc(amps)))
}

fn resistor(name: &str, p: NodeIndex, n: NodeIndex, ohms: f64) -> Element {
    Element::new(name, [p, n], Device::resistor(ohms))
}

#[test]
fn voltage_divider_obeys_ohms_law_and_the_sign_convention() {
    let mut c = Circuit::new();
    let (a, b) = (c.node("in"), c.node("out"));
    c.add(voltage_source("V1", a, GROUND, 10.0));
    c.add(resistor("R1", a, b, 1e3));
    c.add(resistor("R2", b, GROUND, 3e3));
    let r = op(&c).unwrap();
    assert_eq!(r.analysis, AnalysisKind::OperatingPoint);
    assert_eq!(
        (
            r.axis.name.as_str(),
            r.axis.unit.as_str(),
            r.axis.values.as_slice()
        ),
        ("point", "", &[0.0][..])
    );
    let names: Vec<&str> = r.signals.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["V(in)", "V(out)", "I(V1)", "I(R1)", "I(R2)"]);
    close(value(&r, "V(in)"), 10.0, 1e-12);
    close(value(&r, "V(out)"), 7.5, 1e-12);
    // V1 delivers power, so the current into its positive terminal is negative.
    close(value(&r, "I(V1)"), -2.5e-3, 1e-15);
    close(value(&r, "I(R1)"), 2.5e-3, 1e-15);
    close(value(&r, "I(R2)"), 2.5e-3, 1e-15);
    let s = r.signal("I(R2)").unwrap();
    assert_eq!(s.quantity, Quantity::Current);
    assert_eq!(s.unit(), "A");
    assert_eq!(
        s.origin,
        SignalOrigin::TerminalCurrent {
            element: 2,
            terminal: 0
        }
    );
    assert_eq!(r.signal("V(out)").unwrap().origin, SignalOrigin::Node(b));
}

#[test]
fn current_source_drives_a_resistor() {
    // Port of the prototype: 2 mA into 600 Ω in parallel with 1 µF.
    let mut c = Circuit::new();
    let n = c.node("n");
    c.add(current_source("I1", GROUND, n, 2e-3));
    c.add(resistor("R1", n, GROUND, 600.0));
    c.add(Element::new("C1", [n, GROUND], Device::capacitor(1e-6)));
    let r = op(&c).unwrap();
    close(value(&r, "V(n)"), 1.2, 1e-13);
    close(value(&r, "I(I1)"), 2e-3, 1e-18);
    close(value(&r, "I(C1)"), 0.0, 0.0);
}

#[test]
fn superposition_holds() {
    let build = |volts: f64, amps: f64| {
        let mut c = Circuit::new();
        let (a, b) = (c.node("a"), c.node("b"));
        c.add(voltage_source("V1", a, GROUND, volts));
        c.add(resistor("R1", a, b, 1e3));
        c.add(current_source("I1", GROUND, b, amps));
        c.add(resistor("R2", b, GROUND, 2e3));
        value(&op(&c).unwrap(), "V(b)")
    };
    let both = build(5.0, 1e-3);
    // V(b) = 5·2/3 + 1 mA·(1k ∥ 2k).
    close(both, 5.0 * 2.0 / 3.0 + 1e-3 * 2e3 / 3.0, 1e-12);
    close(both, build(5.0, 0.0) + build(0.0, 1e-3), 1e-12);
}

#[test]
fn inductors_short_and_capacitors_open_at_dc() {
    let mut c = Circuit::new();
    let (input, a, b) = (c.node("in"), c.node("a"), c.node("b"));
    c.add(voltage_source("V1", input, GROUND, 5.0));
    c.add(resistor("R1", input, a, 1e3));
    c.add(Element::new("L1", [a, b], Device::inductor(1e-3)));
    c.add(resistor("R2", b, GROUND, 1e3));
    c.add(Element::new("C1", [b, GROUND], Device::capacitor(1e-6)));
    let r = op(&c).unwrap();
    close(value(&r, "V(a)"), 2.5, 1e-12);
    close(value(&r, "V(b)"), 2.5, 1e-12);
    close(value(&r, "I(L1)"), 2.5e-3, 1e-15);
    assert_eq!(value(&r, "I(C1)"), 0.0);
}

#[test]
fn rl_operating_point_tracks_the_branch_current() {
    // Port of the prototype: 1 V, 1 kΩ, 1 H to ground.
    let mut c = Circuit::new();
    let (a, b) = (c.node("1"), c.node("2"));
    c.add(voltage_source("V1", a, GROUND, 1.0));
    c.add(resistor("R1", a, b, 1e3));
    c.add(Element::new("L1", [b, GROUND], Device::inductor(1.0)));
    let r = op(&c).unwrap();
    close(value(&r, "V(2)"), 0.0, 1e-13);
    close(value(&r, "I(L1)"), 1e-3, 1e-14);
}

#[test]
fn a_floating_node_is_named() {
    // D15: a node reached only through capacitors is tied to ground through
    // gmin for the operating point, and the run warns, naming the node.
    let mut c = Circuit::new();
    let (a, n1) = (c.node("a"), c.node("n1"));
    c.add(voltage_source("V1", a, GROUND, 1.0));
    c.add(resistor("R1", a, GROUND, 1e3));
    // Only a capacitor touches n1, and capacitors are open at DC.
    c.add(Element::new("C1", [n1, GROUND], Device::capacitor(1e-6)));
    for solver in [SolverKind::Dense, SolverKind::Sparse] {
        let control = SimControl::new();
        let options = SimOptions {
            solver,
            ..SimOptions::default()
        };
        let r = simulate(&c, &Analysis::OperatingPoint, &options, &control).unwrap();
        assert_eq!(value(&r, "V(n1)"), 0.0);
        close(value(&r, "V(a)"), 1.0, 1e-15);
        assert_eq!(
            control.warnings(),
            [
                "Node n1 has no DC path to ground (it is reached only through capacitors); it was \
              tied to ground through gmin = 1 pS for the operating point"
            ]
        );
    }
    // A node connected to nothing at all is still an error that says so.
    let mut c = Circuit::new();
    let a = c.node("a");
    c.add(voltage_source("V1", a, GROUND, 1.0));
    c.node("lonely");
    for solver in [SolverKind::Dense, SolverKind::Sparse] {
        let error = op_with(&c, solver).unwrap_err();
        assert_eq!(
            error,
            SimError::SingularMatrix {
                unknown: "V(lonely)".into(),
                reason: "node lonely has no DC path to ground (nothing connects it to the rest \
                         of the circuit)"
                    .into(),
                at: None,
                progress: 0.0
            }
        );
        assert_eq!(
            error.to_string(),
            "Singular matrix at V(lonely) (0.0% complete): node lonely has no DC path to ground \
             (nothing connects it to the rest of the circuit)"
        );
    }
}

#[test]
fn a_current_source_into_a_capacitor_has_no_operating_point() {
    let mut c = Circuit::new();
    let n = c.node("n");
    c.add(current_source("I1", GROUND, n, 1e-3));
    c.add(Element::new("C1", [n, GROUND], Device::capacitor(1e-6)));
    let control = SimControl::new();
    let error = simulate(
        &c,
        &Analysis::OperatingPoint,
        &SimOptions::default(),
        &control,
    )
    .unwrap_err();
    assert!(
        matches!(&error, SimError::SingularMatrix { unknown, .. } if unknown == "V(n)"),
        "{error}"
    );
    assert!(
        error.to_string().ends_with(
            "node n has no DC path to ground (only current source I1 feeds it, so its voltage \
             is undefined)"
        ),
        "{error}"
    );
    // The node is not tied to ground: that would turn 1 mA into ~1e9 V.
    assert!(control.warnings().is_empty());
}

#[test]
fn a_voltage_source_loop_names_the_redundant_source() {
    let mut c = Circuit::new();
    let n = c.node("n");
    c.add(voltage_source("V1", n, GROUND, 1.0));
    c.add(voltage_source("V2", n, GROUND, 2.0));
    for solver in [SolverKind::Dense, SolverKind::Sparse] {
        let error = op_with(&c, solver).unwrap_err();
        assert_eq!(
            error,
            SimError::SingularMatrix {
                unknown: "I(V2)".into(),
                reason: "V1 and V2 form a loop of voltage sources and inductors (shorts at DC), \
                         so the loop current is undefined"
                    .into(),
                at: None,
                progress: 0.0
            }
        );
    }
    // An inductor is a short at DC, so it closes a loop as well.
    let mut c = Circuit::new();
    let (a, b) = (c.node("a"), c.node("b"));
    c.add(voltage_source("V1", a, GROUND, 1.0));
    c.add(Element::new("L1", [a, b], Device::inductor(1e-3)));
    c.add(Element::new("L2", [b, GROUND], Device::inductor(1e-3)));
    let error = op(&c).unwrap_err();
    assert!(
        error.to_string().ends_with(
            ": L1, V1 and L2 form a loop of voltage sources and inductors (shorts at DC), so \
             the loop current is undefined"
        ),
        "{error}"
    );
    assert!(
        matches!(&error, SimError::SingularMatrix { unknown, .. } if unknown == "I(L2)"),
        "{error}"
    );
}

#[test]
fn invalid_circuits_and_options_are_rejected() {
    let base = || {
        let mut c = Circuit::new();
        let (a, b) = (c.node("a"), c.node("b"));
        c.add(voltage_source("V1", a, GROUND, 1.0));
        c.add(resistor("R1", a, b, 1e3));
        c.add(resistor("R2", b, GROUND, 1e3));
        c
    };
    let mut cases = Vec::new();
    let mut c = base();
    c.elements[1].device = Device::resistor(f64::NAN);
    cases.push(c);
    let mut c = base();
    c.elements[1].device = Device::resistor(-5.0);
    cases.push(c);
    let mut c = base();
    c.elements[1].nodes[1] = 99;
    cases.push(c);
    let mut c = base();
    c.elements[1].nodes.push(0);
    cases.push(c);
    let mut c = base();
    c.elements[2].name = "R1".into();
    cases.push(c);
    let mut c = base();
    c.elements[2].name.clear();
    cases.push(c);
    let mut c = base();
    c.elements[0].device = Device::VoltageSource(Source::dc(f64::INFINITY));
    cases.push(c);
    for circuit in cases {
        assert!(
            matches!(op(&circuit), Err(SimError::InvalidCircuit(_))),
            "{:?}",
            circuit.elements
        );
    }
    let options = SimOptions {
        reltol: 0.0,
        ..SimOptions::default()
    };
    let error = simulate(
        &base(),
        &Analysis::OperatingPoint,
        &options,
        &SimControl::new(),
    )
    .unwrap_err();
    assert!(matches!(error, SimError::InvalidCircuit(_)));
    assert_eq!(error.progress(), 0.0);
}

#[test]
fn absurd_inputs_are_invalid_rather_than_singular() {
    // SR-5: these used to come back as "Singular matrix ... floating nodes".
    let base = || {
        let mut c = Circuit::new();
        let (a, b) = (c.node("a"), c.node("b"));
        c.add(voltage_source("V1", a, GROUND, 1.0));
        c.add(resistor("R1", a, b, 1e3));
        c.add(Element::new("C1", [b, GROUND], Device::capacitor(1e-6)));
        c.add(Element::new("L1", [b, GROUND], Device::inductor(1e-3)));
        c
    };
    let invalid = |circuit: &Circuit, options: &SimOptions, expected: &str| {
        let analysis = Analysis::Transient(Transient {
            stop: 1e-3,
            step: 1e-5,
            start_save: 0.0,
            max_step: None,
            initial_state: InitialState::ZeroState,
        });
        let error = simulate(circuit, &analysis, options, &SimControl::new()).unwrap_err();
        let SimError::InvalidCircuit(e) = &error else {
            panic!("expected an invalid circuit, got {error}");
        };
        assert_eq!(e.message, expected);
    };
    let defaults = SimOptions::default();
    for v in [f64::NAN, f64::INFINITY, 1e300] {
        let mut c = base();
        c.elements[2].device = Device::Capacitor {
            capacitance: 1e-6,
            initial_voltage: Some(v),
        };
        invalid(
            &c,
            &defaults,
            &format!("C1: initial voltage must be finite and at most 1e30 in magnitude, got {v:e}"),
        );
        let mut c = base();
        c.elements[3].device = Device::Inductor {
            inductance: 1e-3,
            initial_current: Some(-v),
        };
        invalid(
            &c,
            &defaults,
            &format!(
                "L1: initial current must be finite and at most 1e30 in magnitude, got {:e}",
                -v
            ),
        );
    }
    let mut c = base();
    c.nodes.push("a".into());
    invalid(&c, &defaults, "nodes 1 and 3 are both named a");
    let mut c = base();
    c.nodes[2] = "0".into();
    invalid(&c, &defaults, "node 2 is named \"0\", the name of ground");
    for t in [-273.15, -300.0, f64::NAN] {
        let options = SimOptions {
            temperature: t,
            ..SimOptions::default()
        };
        invalid(
            &base(),
            &options,
            &format!("option temperature must be above absolute zero (-273.15 °C), got {t} °C"),
        );
    }
    // Just above absolute zero is allowed.
    assert!(
        SimOptions {
            temperature: -273.0,
            ..SimOptions::default()
        }
        .validate()
        .is_ok()
    );
}

#[test]
fn an_overflowing_solution_is_reported_as_such() {
    // 1e200 V across 1e-200 Ω: the current overflows.
    let mut c = Circuit::new();
    let a = c.node("a");
    c.add(voltage_source("V1", a, GROUND, 1e200));
    c.add(resistor("R1", a, GROUND, 1e-200));
    let error = op(&c).unwrap_err();
    assert!(
        matches!(&error, SimError::NumericalOverflow { unknown, at: None, progress } if unknown == "I(V1)" && *progress == 0.0),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "The solution overflowed or became NaN at I(V1) (0.0% complete): check for extreme \
         element values or initial conditions"
    );
}

#[test]
fn a_failed_operating_point_names_the_worst_node() {
    // SR-4: one Newton iteration never confirms convergence of a diode.
    let mut c = Circuit::new();
    let (a, d) = (c.node("a"), c.node("d"));
    c.add(voltage_source("V1", a, GROUND, 5.0));
    c.add(resistor("R1", a, d, 1e3));
    c.add(Element::new(
        "D1",
        [d, GROUND],
        Device::diode(DiodeModel::default()),
    ));
    let options = SimOptions {
        dc_iterations: 1,
        ..SimOptions::default()
    };
    let error = simulate(&c, &Analysis::OperatingPoint, &options, &SimControl::new()).unwrap_err();
    let SimError::NoConvergence {
        at: None,
        unknown: Some(unknown),
        ..
    } = &error
    else {
        panic!("expected non-convergence naming an unknown, got {error:?}");
    };
    assert!(
        error.to_string().starts_with(&format!(
            "Simulation did not converge near {unknown} (0.0% complete): Newton–Raphson did not \
             converge in 1 iteration; GMIN stepping"
        )),
        "{error}"
    );
    // A transient's starting point is located at t = 0 s.
    let analysis = Analysis::Transient(Transient {
        stop: 1e-3,
        step: 1e-5,
        start_save: 0.0,
        max_step: None,
        initial_state: InitialState::OperatingPoint,
    });
    let error = simulate(&c, &analysis, &options, &SimControl::new()).unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with("Simulation did not converge at t = 0 s near "),
        "{error}"
    );
}

#[test]
fn an_empty_circuit_has_an_empty_operating_point() {
    let r = op(&Circuit::new()).unwrap();
    assert!(r.signals.is_empty());
    assert_eq!(r.len(), 1);
}

#[test]
fn progress_reaches_one_after_success() {
    let mut c = Circuit::new();
    let a = c.node("a");
    c.add(voltage_source("V1", a, GROUND, 1.0));
    c.add(resistor("R1", a, GROUND, 1.0));
    let control = SimControl::new();
    simulate(
        &c,
        &Analysis::OperatingPoint,
        &SimOptions::default(),
        &control,
    )
    .unwrap();
    assert_eq!(control.progress(), 1.0);
}

/// A random connected resistor network with independent sources.
#[derive(Debug, Clone)]
struct Network {
    nodes: usize,
    /// `(a, b, ohms)` with node 0 as ground.
    resistors: Vec<(usize, usize, f64)>,
    /// `(node, amps)`: current injected into the node from ground.
    currents: Vec<(usize, f64)>,
    /// `(node, volts)`: at most one per node, always to ground.
    voltages: Vec<(usize, f64)>,
}

impl Network {
    fn circuit(&self) -> Circuit {
        let mut c = Circuit::new();
        for i in 1..=self.nodes {
            c.node(&format!("n{i}"));
        }
        for (k, &(a, b, ohms)) in self.resistors.iter().enumerate() {
            c.add(resistor(&format!("R{k}"), a, b, ohms));
        }
        for (k, &(n, amps)) in self.currents.iter().enumerate() {
            c.add(current_source(&format!("I{k}"), GROUND, n, amps));
        }
        for (k, &(n, volts)) in self.voltages.iter().enumerate() {
            c.add(voltage_source(&format!("V{k}"), n, GROUND, volts));
        }
        c
    }
}

fn network() -> impl Strategy<Value = Network> {
    (2_usize..14).prop_flat_map(|nodes| {
        let ohms = || (0.0..6.0_f64).prop_map(|e| 10f64.powf(e));
        let tree = prop::collection::vec((any::<prop::sample::Index>(), ohms()), nodes);
        let extra = prop::collection::vec((0..=nodes, 0..=nodes, ohms()), 0..2 * nodes);
        let currents = prop::collection::vec((1..=nodes, -1e-2..1e-2_f64), 0..4);
        let voltages = prop::collection::btree_map(1..=nodes, -10.0..10.0_f64, 0..3);
        (tree, extra, currents, voltages).prop_map(move |(tree, extra, currents, voltages)| {
            // Node i hangs off an earlier node or ground, so every node has a
            // DC path to ground.
            let mut resistors: Vec<(usize, usize, f64)> = tree
                .iter()
                .enumerate()
                .map(|(i, (parent, r))| (i + 1, parent.index(i + 1), *r))
                .collect();
            resistors.extend(extra.into_iter().filter(|(a, b, _)| a != b));
            Network {
                nodes,
                resistors,
                currents,
                voltages: voltages.into_iter().collect(),
            }
        })
    })
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 128,
        failure_persistence: None,
        rng_seed: RngSeed::Fixed(0x6b63_6c00),
        ..ProptestConfig::default()
    })]

    /// Kirchhoff's current law holds at every node, every resistor obeys
    /// Ohm's law, and the dense and sparse solvers agree.
    #[test]
    fn random_networks_satisfy_kirchhoff(net in network()) {
        let circuit = net.circuit();
        let dense = op_with(&circuit, SolverKind::Dense).unwrap();
        let sparse = op_with(&circuit, SolverKind::Sparse).unwrap();
        for (d, s) in dense.signals.iter().zip(&sparse.signals) {
            let (d, s) = (d.real().unwrap()[0], s.real().unwrap()[0]);
            prop_assert!((d - s).abs() <= 1e-10 * d.abs().max(1e-3), "{d} vs {s}");
        }
        for result in [&dense, &sparse] {
            let voltage = |n: usize| if n == 0 { 0.0 } else { value(result, &format!("V(n{n})")) };
            let mut leaving = vec![0.0; net.nodes + 1];
            let mut scale = 1e-9_f64;
            for (element, signal) in circuit.elements.iter().zip(&result.signals[net.nodes..]) {
                let i = signal.real().unwrap()[0];
                leaving[element.nodes[0]] += i;
                leaving[element.nodes[1]] -= i;
                scale = scale.max(i.abs());
                if let Device::Resistor { resistance } = element.device {
                    let ohm = (voltage(element.nodes[0]) - voltage(element.nodes[1])) / resistance;
                    prop_assert!((i - ohm).abs() <= 1e-12 * ohm.abs().max(1e-9));
                }
            }
            for (node, residual) in leaving.iter().enumerate().skip(1) {
                prop_assert!(residual.abs() <= 1e-9 * scale, "KCL residual {residual} at n{node}");
            }
        }
    }
}
