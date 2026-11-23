//! Op-amp parts (D26): kind, designator, default model and summary, the
//! five named pins in every orientation, netlisting with library models
//! (and unknown models), simulation of a netlisted stage, and project files
//! (round trips, older files).

use std::path::PathBuf;
use xsim_project::{
    Compatibility, Component, NetLabel, NetlistError, Orientation, Part, PartKind, Point, Project,
    Schematic, netlist, symbols,
};
use xsim_spice_engine::{
    Analysis, Device, OpAmpKind, SimControl, SimOptions, Source, library, simulate,
};

/// Adds `part` named `name` at `at` in `orientation`, with a label on each
/// pin naming its net (`"0"` is ground; empty leaves the pin open).
fn place(
    s: &mut Schematic,
    name: &str,
    part: Part,
    at: (i32, i32),
    orientation: Orientation,
    nets: &[&str],
) {
    let id = s.allocate_id();
    let component = Component {
        id,
        name: name.into(),
        position: Point::new(at.0, at.1),
        orientation,
        part,
    };
    let pins = component.pins();
    assert_eq!(pins.len(), nets.len(), "{name}");
    for (pin, net) in pins.into_iter().zip(nets) {
        if net.is_empty() {
            continue;
        }
        let id = s.allocate_id();
        s.labels.push(NetLabel {
            id,
            name: (*net).into(),
            position: pin,
        });
    }
    s.components.push(component);
}

fn upright() -> Orientation {
    Orientation::default()
}

fn two_pin(s: &mut Schematic, name: &str, part: Part, x: i32, nets: [&str; 2]) {
    place(s, name, part, (x, 40), upright(), &nets);
}

/// An inverting gain of −10 (`R1` 1 kΩ from `in`, `R2` 10 kΩ feedback) on
/// ±15 V with the op-amp `U1` in `orientation`, driven by 0.1 V.
fn inverting(orientation: Orientation, model: &str) -> Schematic {
    let mut s = Schematic::default();
    let volts = |v: f64| Part::VoltageSource {
        source: Source::dc(v),
    };
    let ohms = |r: f64| Part::Resistor { resistance: r };
    two_pin(&mut s, "VP", volts(15.0), 0, ["vp", "0"]);
    two_pin(&mut s, "VN", volts(-15.0), 20, ["vn", "0"]);
    two_pin(&mut s, "VIN", volts(0.1), 40, ["in", "0"]);
    two_pin(&mut s, "R1", ohms(1e3), 60, ["in", "n"]);
    two_pin(&mut s, "R2", ohms(10e3), 80, ["n", "out"]);
    place(
        &mut s,
        "U1",
        Part::OpAmp {
            model: model.into(),
        },
        (0, 0),
        orientation,
        &["0", "n", "out", "vp", "vn"],
    );
    place(&mut s, "GND1", Part::Ground, (100, 40), upright(), &["0"]);
    s
}

fn every_orientation() -> Vec<Orientation> {
    let mut all = Vec::new();
    for rotation in 0..4 {
        for (mirror_x, mirror_y) in [(false, false), (true, false), (false, true), (true, true)] {
            all.push(Orientation {
                rotation,
                mirror_x,
                mirror_y,
            });
        }
    }
    all
}

#[test]
fn the_opamp_kind_has_its_prefix_label_default_model_and_summary() {
    assert!(PartKind::ALL.contains(&PartKind::OpAmp));
    assert_eq!(PartKind::OpAmp.prefix(), "U");
    assert_eq!(PartKind::OpAmp.label(), "Op-amp");
    let part = Part::default_for(PartKind::OpAmp);
    assert_eq!(
        part,
        Part::OpAmp {
            model: "OA_GP".into()
        }
    );
    assert_eq!(part.kind(), PartKind::OpAmp);
    assert_eq!(part.summary(), "OA_GP");
    assert_eq!(library::default_opamp(), "OA_GP");
}

#[test]
fn the_opamp_has_five_named_pins_on_the_grid() {
    let g = symbols::geometry(PartKind::OpAmp);
    assert_eq!(g.pin_names, ["+", "−", "OUT", "V+", "V−"]);
    // Inputs on the left (− on top), output on the right, V+ on top and
    // V− at the bottom, at the D22 resolution.
    assert_eq!(
        g.pins,
        [
            Point::new(-4, 2),
            Point::new(-4, -2),
            Point::new(4, 0),
            Point::new(0, -4),
            Point::new(0, 4),
        ]
    );
    assert_eq!(
        (g.body_min, g.body_max),
        (Point::new(-4, -4), Point::new(4, 4))
    );
}

#[test]
fn opamps_netlist_with_their_library_model_and_terminal_order() {
    let schematic = inverting(upright(), "OA_RRIO");
    let n = netlist(&schematic).expect("netlists");
    let index = n
        .circuit
        .elements
        .iter()
        .position(|e| e.name == "U1")
        .expect("U1");
    let element = &n.circuit.elements[index];
    let Device::OpAmp { model } = &element.device else {
        panic!("not an op-amp: {:?}", element.device);
    };
    assert_eq!(Some(model.clone()), library::opamp("OA_RRIO"));
    let names: Vec<&str> = element
        .nodes
        .iter()
        .map(|&k| n.circuit.nodes[k].as_str())
        .collect();
    assert_eq!(names, ["0", "n", "out", "vp", "vn"]);
}

#[test]
fn rotated_and_mirrored_opamps_keep_their_terminals() {
    for orientation in every_orientation() {
        let n = netlist(&inverting(orientation, "OA_IDEAL")).expect("netlists");
        let element = n
            .circuit
            .elements
            .iter()
            .find(|e| e.name == "U1")
            .expect("U1");
        let names: Vec<&str> = element
            .nodes
            .iter()
            .map(|&k| n.circuit.nodes[k].as_str())
            .collect();
        assert_eq!(names, ["0", "n", "out", "vp", "vn"], "{orientation:?}");
        // And it amplifies: −10 × 0.1 V, within the ideal op-amp's 10⁻⁵.
        let r = simulate(
            &n.circuit,
            &Analysis::OperatingPoint,
            &SimOptions::default(),
            &SimControl::new(),
        )
        .expect("simulates");
        let v = r.signal("V(out)").and_then(|s| s.real()).expect("V(out)")[0];
        assert!((v + 1.0).abs() < 2e-5, "{orientation:?}: {v}");
    }
}

#[test]
fn an_unknown_opamp_model_names_the_part() {
    for model in ["OA_NONE", "oa_gp", "NPN_GP"] {
        assert_eq!(
            netlist(&inverting(upright(), model)).err(),
            Some(NetlistError::UnknownModel {
                component: "U1".into(),
                model: model.into(),
            }),
            "{model}"
        );
    }
}

#[test]
fn an_open_opamp_pin_is_named() {
    let mut s = inverting(upright(), "OA_GP");
    // Remove the V− label: the pin touches nothing.
    let pin = s
        .components
        .iter()
        .find(|c| c.name == "U1")
        .expect("U1")
        .pins()[4];
    s.labels.retain(|l| l.position != pin);
    assert_eq!(
        netlist(&s).err(),
        Some(NetlistError::UnconnectedPin {
            component: "U1".into(),
            pin: "V−".into(),
        })
    );
}

fn project_with(schematic: Schematic) -> Project {
    let mut project = Project::new("Op-amps");
    project.circuits[0].schematic = schematic;
    project
}

#[test]
fn projects_with_opamps_round_trip_byte_identically() {
    let mut schematic = inverting(
        Orientation {
            rotation: 1,
            mirror_x: true,
            mirror_y: false,
        },
        "OA_FAST",
    );
    place(
        &mut schematic,
        "U2",
        Part::OpAmp {
            model: "NOT_IN_LIBRARY".into(),
        },
        (0, 80),
        upright(),
        &["", "", "", "", ""],
    );
    let project = project_with(schematic);
    let text = project.to_json();
    let loaded = Project::from_json(&text).expect("loads");
    assert_eq!(loaded.compatibility, Compatibility::Current);
    assert_eq!(loaded.project, project);
    assert_eq!(loaded.project.to_json(), text);
    // Stored like the semiconductors: a tagged variant naming the model.
    let u1 = "\"part\": {\n              \"OpAmp\": {\n                \"model\": \"OA_FAST\"\n              }\n            }";
    assert!(text.contains(u1), "{text}");
    let temp = tempfile::tempdir().expect("temp dir");
    let path = temp.path().join("opamps.xsim_proj");
    project.save(&path).expect("saves");
    let reopened = Project::load(&path).expect("reopens");
    reopened.project.save(&path).expect("saves again");
    assert_eq!(std::fs::read_to_string(&path).expect("reads"), text);
}

#[test]
fn files_written_before_opamps_still_load_unchanged() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/RC low-pass.xsim_proj");
    let text = std::fs::read_to_string(path).expect("the example exists");
    let loaded = Project::from_json(&text).expect("loads");
    assert_eq!(loaded.compatibility, Compatibility::Current);
    assert_eq!(loaded.project.to_json(), text);
}

#[test]
fn every_library_opamp_netlists() {
    for entry in library::opamps() {
        let n = netlist(&inverting(upright(), &entry.model.name)).expect("netlists");
        assert!(n.circuit.validate().is_ok(), "{}", entry.model.name);
        let ideal = entry.model.kind == OpAmpKind::Ideal;
        assert_eq!(ideal, entry.model.name == "OA_IDEAL");
    }
}

#[test]
fn opamp_models_and_circuits_round_trip_through_json() {
    // The ideal op-amp's infinite parameters are stored as "inf".
    let model = library::opamp("OA_IDEAL").expect("OA_IDEAL");
    let json = serde_json::to_string(&model).expect("serialises");
    assert!(json.contains("\"gbw\":\"inf\""), "{json}");
    assert!(json.contains("\"kind\":\"Ideal\""), "{json}");
    let back: xsim_spice_engine::OpAmpModel = serde_json::from_str(&json).expect("reads");
    assert_eq!(back, model);
    // Missing fields take the defaults.
    let partial: xsim_spice_engine::OpAmpModel =
        serde_json::from_str(r#"{"name":"X","aol":2e5}"#).expect("reads");
    assert_eq!(partial.aol, 2e5);
    assert_eq!(partial.kind, OpAmpKind::Macro);
    assert_eq!(partial.gbw, xsim_spice_engine::OpAmpModel::default().gbw);
    // A netlisted circuit holding it round-trips exactly.
    let circuit = netlist(&inverting(upright(), "OA_IDEAL"))
        .expect("netlists")
        .circuit;
    let json = serde_json::to_string(&circuit).expect("serialises");
    let back: xsim_spice_engine::Circuit = serde_json::from_str(&json).expect("reads");
    assert_eq!(back, circuit);
}
