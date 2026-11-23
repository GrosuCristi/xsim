//! Schematic → engine circuit conversion and netlist errors.

mod common;

use common::{P, Sketch, rc_low_pass_schematic};
use xsim_project::{NetlistError, Part, PartKind, netlist};
use xsim_spice_engine::{Device, GROUND, Polarity, Source, library};

#[test]
fn rc_low_pass_becomes_a_three_element_circuit() {
    let schematic = rc_low_pass_schematic();
    let n = netlist(&schematic).unwrap();
    let names: Vec<_> = n.circuit.elements.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["V1", "R1", "C1"]);
    // Nodes are numbered in order of first use by the elements.
    assert_eq!(n.circuit.nodes, ["0", "in", "out"]);
    let terminals: Vec<_> = n.circuit.elements.iter().map(|e| e.nodes.clone()).collect();
    assert_eq!(terminals, [vec![1, GROUND], vec![1, 2], vec![2, GROUND]]);
    assert_eq!(
        n.circuit.elements[0].device,
        Device::VoltageSource(Source::dc(1.0))
    );
    assert_eq!(n.circuit.elements[1].device, Device::resistor(1e3));
    assert_eq!(n.circuit.elements[2].device, Device::capacitor(1e-6));
    n.circuit.validate().unwrap();

    // Ground symbols produce no element; ids map to element indices.
    let ids: Vec<_> = schematic.components.iter().map(|c| c.id).collect();
    assert_eq!(n.elements.len(), 3);
    assert_eq!(n.elements[&ids[0]], 0);
    assert_eq!(n.elements[&ids[2]], 2);
    assert!(!n.elements.contains_key(&ids[3]));

    // Every net maps to the engine node of the same name.
    for (net, &node) in n.connectivity.nets.iter().zip(&n.net_nodes) {
        assert_eq!(n.circuit.nodes[node], net.name);
    }
    assert_eq!(n.node_name_at(P(2, 0)), Some("in"));
    assert_eq!(n.node_name_at(P(12, 2)), Some("out"));
    assert_eq!(n.node_name_at(P(6, 8)), Some("0"));
    assert_eq!(n.node_name_at(P(6, 4)), None);
}

#[test]
fn unlabelled_nets_get_automatic_node_names() {
    let mut schematic = rc_low_pass_schematic();
    schematic.labels.clear();
    let n = netlist(&schematic).unwrap();
    // "in" starts at (0, 0), "out" at (8, 0); ground is "0".
    assert_eq!(n.circuit.nodes, ["0", "N001", "N002"]);
    assert_eq!(n.node_name_at(P(12, 2)), Some("N002"));
}

#[test]
fn part_parameters_reach_the_engine_devices() {
    let mut s = Sketch::new();
    let pulse = Source {
        dc: 0.0,
        ac_magnitude: 1.0,
        ac_phase: 45.0,
        waveform: Some(xsim_spice_engine::Waveform::Pwl {
            points: vec![(0.0, 0.0), (1e-3, 1.0)],
            repeat: false,
        }),
    };
    s.part(
        Part::Capacitor {
            capacitance: 2.2e-9,
            initial_voltage: Some(0.5),
        },
        (0, 0),
        0,
    );
    s.part(
        Part::Inductor {
            inductance: 4.7e-3,
            initial_current: Some(-1e-3),
        },
        (4, 0),
        0,
    );
    s.part(
        Part::CurrentSource {
            source: pulse.clone(),
        },
        (8, 0),
        0,
    );
    s.ground((-2, 0));
    s.ground((10, 0));
    let n = netlist(&s.schematic).unwrap();
    let devices: Vec<_> = n
        .circuit
        .elements
        .iter()
        .map(|e| e.device.clone())
        .collect();
    assert_eq!(
        devices,
        [
            Device::Capacitor {
                capacitance: 2.2e-9,
                initial_voltage: Some(0.5)
            },
            Device::Inductor {
                inductance: 4.7e-3,
                initial_current: Some(-1e-3)
            },
            Device::CurrentSource(pulse),
        ]
    );
    // Pins meeting at (2, 0) and (6, 0) connect without wires.
    assert_eq!(n.circuit.nodes, ["0", "N001", "N002"]);
}

#[test]
fn a_stray_wire_or_lone_label_does_not_change_the_circuit() {
    let reference = netlist(&rc_low_pass_schematic()).unwrap();
    let mut s = Sketch {
        schematic: rc_low_pass_schematic(),
    };
    s.wire(&[(30, 30), (40, 30)]);
    s.label("nowhere", (50, 50));
    let n = netlist(&s.schematic).unwrap();
    assert_eq!(n.circuit, reference.circuit);
    assert_eq!(n.net_nodes, reference.net_nodes);
    assert_eq!(n.node_name_at(P(35, 30)), None);
}

#[test]
fn a_schematic_without_parts_is_empty() {
    assert_eq!(netlist(&Default::default()), Err(NetlistError::Empty));
    let mut s = Sketch::new();
    s.ground((0, 0));
    s.wire(&[(0, 0), (5, 0)]);
    s.label("x", (5, 0));
    assert_eq!(netlist(&s.schematic), Err(NetlistError::Empty));
}

#[test]
fn a_circuit_needs_a_device_on_ground() {
    let mut s = Sketch::new();
    s.resistor((0, 0), 0);
    s.wire(&[(-2, 0), (-2, 4), (2, 4), (2, 0)]);
    assert_eq!(netlist(&s.schematic), Err(NetlistError::NoGround));
    // A ground symbol touching nothing does not count.
    s.ground((20, 20));
    assert_eq!(netlist(&s.schematic), Err(NetlistError::NoGround));
    // Neither does a ground on a stray wire.
    s.wire(&[(20, 20), (25, 20)]);
    assert_eq!(netlist(&s.schematic), Err(NetlistError::NoGround));
    s.ground((2, 4));
    assert!(netlist(&s.schematic).is_ok());
}

#[test]
fn a_label_named_0_grounds_its_net() {
    let mut s = Sketch::new();
    s.resistor((0, 0), 0);
    s.wire(&[(-2, 0), (-2, 4), (2, 4), (2, 0)]);
    s.label("0", (0, 4));
    let n = netlist(&s.schematic).unwrap();
    assert_eq!(n.circuit.elements[0].nodes, [GROUND, GROUND]);
}

#[test]
fn designators_must_be_unique_among_non_ground_parts() {
    let mut s = Sketch {
        schematic: rc_low_pass_schematic(),
    };
    let extra = s.ground((0, 10));
    s.schematic.component_mut(extra).unwrap().name = "GND1".into();
    // Ground symbols may share a name: they produce no element.
    assert!(netlist(&s.schematic).is_ok());
    let r2 = s.resistor((20, 0), 0);
    s.schematic.component_mut(r2).unwrap().name = "C1".into();
    assert_eq!(
        netlist(&s.schematic),
        Err(NetlistError::DuplicateName("C1".into()))
    );
}

#[test]
fn a_pin_touching_nothing_is_unconnected() {
    let mut s = Sketch {
        schematic: rc_low_pass_schematic(),
    };
    let floating = s.part(Part::default_for(PartKind::VoltageSource), (30, 0), 0);
    s.wire(&[(28, 0), (28, 8), (12, 8)]); // + pin wired to the ground rail
    assert_eq!(
        netlist(&s.schematic),
        Err(NetlistError::UnconnectedPin {
            component: "V2".into(),
            pin: "−".into(),
        })
    );
    // A label on the pin, a wire end or another pin all count as contact.
    s.label("probe", (32, 0));
    assert!(netlist(&s.schematic).is_ok());
    s.schematic.labels.pop();
    s.wire(&[(32, 0), (32, 4)]);
    let n = netlist(&s.schematic).unwrap();
    let index = n.elements[&floating];
    assert_eq!(n.circuit.elements[index].nodes[0], GROUND);
}

#[test]
fn errors_are_reported_in_a_fixed_order() {
    // Duplicate names come before unconnected pins, which come before a
    // missing ground.
    let mut s = Sketch::new();
    s.resistor((0, 0), 0);
    let r2 = s.resistor((10, 0), 0);
    s.schematic.component_mut(r2).unwrap().name = "R1".into();
    assert_eq!(
        netlist(&s.schematic),
        Err(NetlistError::DuplicateName("R1".into()))
    );
    s.schematic.component_mut(r2).unwrap().name = "R2".into();
    assert_eq!(
        netlist(&s.schematic),
        Err(NetlistError::UnconnectedPin {
            component: "R1".into(),
            pin: "1".into(),
        })
    );
    s.wire(&[(-2, 0), (-2, 2), (8, 2), (8, 0)]);
    s.wire(&[(2, 0), (2, -2), (12, -2), (12, 0)]);
    assert_eq!(netlist(&s.schematic), Err(NetlistError::NoGround));
}

#[test]
fn semiconductor_models_resolve_from_the_library() {
    let mut s = Sketch::new();
    let diode = s.part(Part::default_for(PartKind::Diode), (0, 0), 0);
    let npn = s.part(Part::default_for(PartKind::NpnBjt), (10, 0), 0);
    let pjfet = s.part(Part::default_for(PartKind::PJfet), (20, 0), 0);
    // Tie every terminal to ground so the circuit is well formed.
    for id in [diode, npn, pjfet] {
        for pin in s.component(id).pins() {
            s.label("0", (pin.x, pin.y));
        }
    }
    let n = netlist(&s.schematic).unwrap();
    let models: Vec<String> = n
        .circuit
        .elements
        .iter()
        .map(|e| match &e.device {
            Device::Diode { model, area } => {
                assert_eq!(*area, 1.0);
                model.name.clone()
            }
            Device::Bjt { model, .. } => model.name.clone(),
            Device::Jfet { model, .. } => model.name.clone(),
            other => panic!("unexpected device {other:?}"),
        })
        .collect();
    assert_eq!(
        models,
        [
            library::default_diode(),
            library::default_bjt(Polarity::N),
            library::default_jfet(Polarity::P),
        ]
    );
    let npn_model = library::bjt(library::default_bjt(Polarity::N)).unwrap();
    assert_eq!(n.circuit.elements[1].device, Device::bjt(npn_model));
}

#[test]
fn unknown_or_mismatched_models_are_reported() {
    let mut s = Sketch {
        schematic: rc_low_pass_schematic(),
    };
    let d = s.part(
        Part::Diode {
            model: "d_signal".into(), // names are case-sensitive
        },
        (30, 0),
        0,
    );
    s.label("0", (28, 0));
    s.label("0", (32, 0));
    assert_eq!(
        netlist(&s.schematic),
        Err(NetlistError::UnknownModel {
            component: "D1".into(),
            model: "d_signal".into(),
        })
    );
    // A PNP model on an NPN symbol is not an NPN model of that name.
    s.schematic.component_mut(d).unwrap().part = Part::Bjt {
        polarity: Polarity::N,
        model: library::default_bjt(Polarity::P).into(),
    };
    for pin in s.component(d).pins() {
        s.label("0", (pin.x, pin.y));
    }
    let error = netlist(&s.schematic).unwrap_err();
    assert_eq!(
        error,
        NetlistError::UnknownModel {
            component: "D1".into(),
            model: library::default_bjt(Polarity::P).into(),
        }
    );
    assert_eq!(
        error.to_string(),
        format!(
            "D1 uses model \"{}\", which is not in the model library",
            library::default_bjt(Polarity::P)
        )
    );
}
