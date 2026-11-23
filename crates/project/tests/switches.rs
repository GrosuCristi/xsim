//! Voltage- and current-controlled switch parts (D17): symbols and pins,
//! netlisting with their own models and the control source, netlist errors,
//! project files (round trips, older files, structural checks) and circuit
//! fingerprints.

mod common;

use common::{P, Sketch};
use serde_json::{Value, json};
use std::path::PathBuf;
use xsim_project::{
    Compatibility, Component, NetLabel, NetlistError, Orientation, Part, PartKind, Point, Project,
    ProjectError, Schematic, StoredResult, netlist, symbols,
};
use xsim_spice_engine::{
    Analysis, CurrentSwitchModel, Device, SimControl, SimOptions, Source, SwitchModel, simulate,
};

/// Adds `part` named `name` at `at`, with a label on each pin naming its
/// net (`"0"` is ground).
fn place(s: &mut Schematic, name: &str, part: Part, at: (i32, i32), nets: &[&str]) {
    let id = s.allocate_id();
    let component = Component {
        id,
        name: name.into(),
        position: Point::new(at.0, at.1),
        orientation: Orientation::default(),
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

fn voltage(volts: f64) -> Part {
    Part::VoltageSource {
        source: Source::dc(volts),
    }
}

fn current_switch(control: &str) -> Part {
    match Part::default_for(PartKind::CurrentSwitch) {
        Part::CurrentSwitch { model, .. } => Part::CurrentSwitch {
            control: control.into(),
            model,
        },
        other => panic!("not a current switch: {other:?}"),
    }
}

/// `V1 (5 V)` feeds `R1 → o` and `R2 → p`. `S1` (default model, VT = 0.5 V)
/// switches `o` to ground, controlled by `VG` on its control pins; `W1`
/// (default model, IT = 1 mA, controlled by `VS`) switches `p` to ground.
/// `I1` pushes `sense` amperes through the 0 V source `VS`. `W1` comes
/// before its control source in schematic order.
fn switches(gate: f64, sense: f64) -> Schematic {
    let mut s = Schematic::default();
    let nets = |a, b| [a, b];
    place(&mut s, "V1", voltage(5.0), (0, 0), &nets("vcc", "0"));
    place(
        &mut s,
        "R1",
        Part::Resistor { resistance: 1e3 },
        (10, 0),
        &nets("vcc", "o"),
    );
    place(
        &mut s,
        "S1",
        Part::default_for(PartKind::VoltageSwitch),
        (20, 0),
        &["o", "0", "g", "0"],
    );
    place(&mut s, "VG", voltage(gate), (30, 0), &nets("g", "0"));
    place(
        &mut s,
        "R2",
        Part::Resistor { resistance: 1e3 },
        (40, 0),
        &nets("vcc", "p"),
    );
    place(&mut s, "W1", current_switch("VS"), (50, 0), &nets("p", "0"));
    place(
        &mut s,
        "I1",
        Part::CurrentSource {
            source: Source::dc(sense),
        },
        (60, 0),
        &nets("0", "x"),
    );
    place(&mut s, "VS", voltage(0.0), (70, 0), &nets("x", "0"));
    place(&mut s, "GND1", Part::Ground, (80, 0), &["0"]);
    s
}

fn component<'a>(s: &'a mut Schematic, name: &str) -> &'a mut Component {
    s.components
        .iter_mut()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("no part {name}"))
}

// ------------------------------------------------------------ the parts

#[test]
fn switch_kinds_have_prefixes_labels_and_defaults() {
    let mut s = Schematic::default();
    assert_eq!(s.next_designator(PartKind::VoltageSwitch), "S1");
    assert_eq!(s.next_designator(PartKind::CurrentSwitch), "W1");
    let mut sketch = Sketch::new();
    sketch.part(Part::default_for(PartKind::CurrentSwitch), (0, 0), 0);
    s = sketch.schematic;
    assert_eq!(s.next_designator(PartKind::CurrentSwitch), "W2");
    assert_eq!(PartKind::VoltageSwitch.label(), "Voltage-controlled switch");
    assert_eq!(PartKind::CurrentSwitch.label(), "Current-controlled switch");
    assert!(PartKind::ALL.contains(&PartKind::VoltageSwitch));
    assert!(PartKind::ALL.contains(&PartKind::CurrentSwitch));
    for kind in PartKind::ALL {
        assert_eq!(Part::default_for(kind).kind(), kind);
    }
    assert_eq!(
        Part::default_for(PartKind::VoltageSwitch),
        Part::VoltageSwitch {
            model: SwitchModel {
                name: "SW".into(),
                ron: 1.0,
                roff: 1e6,
                vt: 0.5,
                vh: 0.0,
            },
        }
    );
    assert_eq!(
        Part::default_for(PartKind::CurrentSwitch),
        Part::CurrentSwitch {
            control: "V1".into(),
            model: CurrentSwitchModel {
                name: "CSW".into(),
                ron: 1.0,
                roff: 1e6,
                it: 1e-3,
                ih: 0.0,
            },
        }
    );
}

#[test]
fn the_voltage_switch_has_four_named_pins_and_the_current_switch_two() {
    let s = symbols::geometry(PartKind::VoltageSwitch);
    assert_eq!(
        s.pins,
        [
            Point::new(2, -2),
            Point::new(2, 2),
            Point::new(-2, -1),
            Point::new(-2, 1)
        ]
    );
    assert_eq!(s.pin_names, ["P", "N", "C+", "C−"]);
    let w = symbols::geometry(PartKind::CurrentSwitch);
    assert_eq!(w.pins, [Point::new(-2, 0), Point::new(2, 0)]);
    assert_eq!(w.pin_names, ["P", "N"]);
    // Pins follow the placement transform like every other symbol.
    let placed = Component {
        id: 1,
        name: "S1".into(),
        position: Point::new(10, 20),
        orientation: Orientation {
            rotation: 1,
            ..Orientation::default()
        },
        part: Part::default_for(PartKind::VoltageSwitch),
    };
    assert_eq!(
        placed.pins(),
        [
            Point::new(12, 22),
            Point::new(8, 22),
            Point::new(11, 18),
            Point::new(9, 18)
        ]
    );
    assert_eq!(placed.bounds(), (Point::new(8, 18), Point::new(12, 22)));
}

// ------------------------------------------------------------ netlisting

#[test]
fn switches_netlist_with_their_own_models_and_control() {
    let n = netlist(&switches(1.0, 2e-3)).expect("netlists");
    n.circuit.validate().expect("valid circuit");
    let node = |name: &str| n.circuit.find_node(name).expect("node exists");
    let s1 = n.circuit.element("S1").expect("S1");
    assert_eq!(s1.nodes, [node("o"), 0, node("g"), 0]);
    let Part::VoltageSwitch { model } = Part::default_for(PartKind::VoltageSwitch) else {
        unreachable!()
    };
    assert_eq!(s1.device, Device::VoltageSwitch { model });
    let w1 = n.circuit.element("W1").expect("W1");
    assert_eq!(w1.nodes, [node("p"), 0]);
    let Part::CurrentSwitch { model, .. } = current_switch("VS") else {
        unreachable!()
    };
    assert_eq!(
        w1.device,
        Device::CurrentSwitch {
            control: "VS".into(),
            model
        }
    );
    // The control is the element of that designator, a voltage source.
    assert!(matches!(
        n.circuit.element("VS").map(|e| &e.device),
        Some(Device::VoltageSource(_))
    ));
}

#[test]
fn netlisted_switches_follow_their_controls() {
    // S1: VT = 0.5 V; W1: IT = 1 mA through VS. Ron = 1 Ω, Roff = 1 MΩ,
    // each behind 1 kΩ from 5 V.
    let (on, off) = (5.0 * 1.0 / 1001.0, 5.0 * 1e6 / (1e6 + 1e3));
    for (gate, sense) in [(1.0, 2e-3), (0.0, 0.0), (1.0, 0.0), (0.0, 2e-3)] {
        let n = netlist(&switches(gate, sense)).expect("netlists");
        let r = simulate(
            &n.circuit,
            &Analysis::OperatingPoint,
            &SimOptions::default(),
            &SimControl::new(),
        )
        .expect("simulates");
        let v = |name: &str| r.signal(name).expect(name).real().expect("real")[0];
        let expect = |closed: bool| if closed { on } else { off };
        let what = format!("gate {gate} V, sense {sense} A");
        assert!(
            (v("V(o)") - expect(gate > 0.5)).abs() < 1e-9,
            "V(o), {what}"
        );
        assert!(
            (v("V(p)") - expect(sense > 1e-3)).abs() < 1e-9,
            "V(p), {what}"
        );
        assert!((v("I(VS)") - sense).abs() < 1e-15, "I(VS), {what}");
    }
}

/// The documented placement transform (mirror, then quarter turns
/// clockwise, `(x, y) → (−y, x)`), written out independently of
/// [`Orientation::apply`].
fn oriented(local: Point, orientation: Orientation) -> Point {
    let (mut x, mut y) = (local.x, local.y);
    if orientation.mirror_x {
        x = -x;
    }
    if orientation.mirror_y {
        y = -y;
    }
    for _ in 0..orientation.rotation {
        (x, y) = (-y, x);
    }
    Point::new(x, y)
}

#[test]
fn rotated_and_mirrored_switches_keep_their_terminals() {
    // Hand-computed for one turn of each mirror: mirror x then three
    // quarter turns, and mirror y then one, both put P at (−2, 2), N at
    // (2, 2), C+ at (−1, −2) and C− at (1, −2).
    for orientation in [
        Orientation {
            rotation: 3,
            mirror_x: true,
            mirror_y: false,
        },
        Orientation {
            rotation: 1,
            mirror_x: false,
            mirror_y: true,
        },
    ] {
        let local = symbols::geometry(PartKind::VoltageSwitch).pins;
        let placed: Vec<Point> = local.iter().map(|&p| oriented(p, orientation)).collect();
        assert_eq!(
            placed,
            [P(-2, 2), P(2, 2), P(-1, -2), P(1, -2)],
            "{orientation:?}"
        );
    }
    // In every orientation the netlist maps P, N, C+, C− in that order, so
    // S1 turns on only for a positive V(C+) − V(C−) and conducts from P to
    // N; W1 conducts from P to N too.
    let gate = |volts: f64| Part::VoltageSource {
        source: Source::dc(volts),
    };
    for rotation in 0..4 {
        for (mirror_x, mirror_y) in [(false, false), (true, false), (false, true), (true, true)] {
            let orientation = Orientation {
                rotation,
                mirror_x,
                mirror_y,
            };
            for drive in [1.0, -1.0] {
                let mut s = Schematic::default();
                place(&mut s, "V1", voltage(5.0), (0, 0), &["vcc", "0"]);
                place(
                    &mut s,
                    "R1",
                    Part::Resistor { resistance: 1e3 },
                    (10, 0),
                    &["vcc", "o"],
                );
                place(
                    &mut s,
                    "R2",
                    Part::Resistor { resistance: 1e3 },
                    (20, 0),
                    &["m", "0"],
                );
                // C− sits at 2 V, C+ at 2 V + drive: the control is the
                // difference, whatever the voltages to ground.
                place(&mut s, "VG", gate(drive), (30, 0), &["g", "h"]);
                place(&mut s, "VH", gate(2.0), (40, 0), &["h", "0"]);
                place(
                    &mut s,
                    "R3",
                    Part::Resistor { resistance: 1e3 },
                    (50, 0),
                    &["vcc", "q"],
                );
                place(
                    &mut s,
                    "R4",
                    Part::Resistor { resistance: 1e3 },
                    (60, 0),
                    &["r", "0"],
                );
                place(
                    &mut s,
                    "I1",
                    Part::CurrentSource {
                        source: Source::dc(2e-3),
                    },
                    (70, 0),
                    &["0", "x"],
                );
                place(&mut s, "VS", voltage(0.0), (80, 0), &["x", "0"]);
                place(&mut s, "GND1", Part::Ground, (90, 0), &["0"]);
                for (name, part, at, nets) in [
                    (
                        "S1",
                        Part::default_for(PartKind::VoltageSwitch),
                        P(0, 20),
                        &["o", "m", "g", "h"][..],
                    ),
                    ("W1", current_switch("VS"), P(20, 20), &["q", "r"][..]),
                ] {
                    let pins = symbols::geometry(part.kind()).pins;
                    for (&pin, net) in pins.iter().zip(nets) {
                        let id = s.allocate_id();
                        s.labels.push(NetLabel {
                            id,
                            name: (*net).into(),
                            position: at + oriented(pin, orientation),
                        });
                    }
                    let id = s.allocate_id();
                    s.components.push(Component {
                        id,
                        name: name.into(),
                        position: at,
                        orientation,
                        part,
                    });
                }
                let what = format!("{orientation:?}, drive {drive} V");
                let n = netlist(&s).unwrap_or_else(|e| panic!("{what}: {e}"));
                let node = |name: &str| n.circuit.find_node(name).expect("node");
                let s1 = n.circuit.element("S1").expect("S1");
                assert_eq!(
                    s1.nodes,
                    [node("o"), node("m"), node("g"), node("h")],
                    "{what}"
                );
                let w1 = n.circuit.element("W1").expect("W1");
                assert_eq!(w1.nodes, [node("q"), node("r")], "{what}");
                let r = simulate(
                    &n.circuit,
                    &Analysis::OperatingPoint,
                    &SimOptions::default(),
                    &SimControl::new(),
                )
                .unwrap_or_else(|e| panic!("{what}: {e}"));
                let value = |name: &str| r.signal(name).expect(name).real().expect("real")[0];
                // On: 5 V across 1 kΩ + 1 Ω + 1 kΩ; off: across 1 MΩ more.
                let (on, off) = (5.0 / 2001.0, 5.0 / (1e6 + 2e3));
                let expected = if drive > 0.0 { on } else { off };
                assert!(
                    (value("I(S1)") / expected - 1.0).abs() < 1e-9,
                    "I(S1), {what}: {}",
                    value("I(S1)")
                );
                assert!(
                    (value("I(W1)") / on - 1.0).abs() < 1e-9,
                    "I(W1), {what}: {}",
                    value("I(W1)")
                );
            }
        }
    }
}

#[test]
fn a_current_switch_must_name_a_voltage_source() {
    for control in ["V9", "R1", "I1", "vs", "S1", "W1", "VS "] {
        let mut s = switches(1.0, 2e-3);
        component(&mut s, "W1").part = current_switch(control);
        let error = netlist(&s).err();
        assert_eq!(
            error,
            Some(NetlistError::UnknownControl {
                component: "W1".into(),
                control: control.into()
            }),
            "{control:?}"
        );
        let message = error.map(|e| e.to_string()).unwrap_or_default();
        assert_eq!(
            message,
            format!(
                "W1 is controlled by the current through \"{control}\", which is not a voltage \
                 source in this circuit"
            )
        );
    }
    let mut s = switches(1.0, 2e-3);
    component(&mut s, "W1").part = current_switch("");
    let message = netlist(&s).err().map(|e| e.to_string()).unwrap_or_default();
    assert_eq!(
        message,
        "W1 has no control source: name the voltage source whose current switches it"
    );
}

#[test]
fn unconnected_switch_pins_are_named_and_reported_before_the_control() {
    let mut s = switches(1.0, 2e-3);
    // Remove the label on S1's C+ pin and break W1's control.
    let c_plus = s
        .components
        .iter()
        .find(|c| c.name == "S1")
        .map(|c| c.pins()[2])
        .expect("S1");
    s.labels.retain(|l| l.position != c_plus);
    component(&mut s, "W1").part = current_switch("V9");
    assert_eq!(
        netlist(&s).err(),
        Some(NetlistError::UnconnectedPin {
            component: "S1".into(),
            pin: "C+".into()
        })
    );
}

#[test]
fn a_switch_model_error_names_the_switch() {
    let mut s = switches(1.0, 2e-3);
    component(&mut s, "S1").part = Part::VoltageSwitch {
        model: SwitchModel {
            ron: 0.0,
            ..SwitchModel::default()
        },
    };
    let n = netlist(&s).expect("the netlister does not check model values");
    let error = n.circuit.validate().expect_err("RON = 0 is rejected");
    assert!(
        error
            .message
            .starts_with("S1: model SW parameter RON must be"),
        "{}",
        error.message
    );
}

// ------------------------------------------------------------ project files

fn project_with(schematic: Schematic) -> Project {
    let mut project = Project::new("Switches");
    project.circuits[0].schematic = schematic;
    project
}

#[test]
fn projects_with_switches_round_trip_byte_identically() {
    let mut s = switches(1.0, 2e-3);
    let mut sketch = Sketch { schematic: s };
    sketch.part(
        Part::VoltageSwitch {
            model: SwitchModel {
                name: "SWX".into(),
                ron: 0.1 + 0.2,
                roff: f64::MAX,
                vt: -2.5,
                vh: -0.125,
            },
        },
        (0, 40),
        3,
    );
    sketch.part(
        Part::CurrentSwitch {
            control: String::new(),
            model: CurrentSwitchModel {
                name: "CSWX".into(),
                ron: 5e-324,
                roff: 1e12,
                it: -1.0 / 3.0,
                ih: 1e-300,
            },
        },
        (10, 40),
        2,
    );
    s = sketch.schematic;
    let project = project_with(s);
    let text = project.to_json();
    let loaded = Project::from_json(&text).expect("loads");
    assert_eq!(loaded.compatibility, Compatibility::Current);
    assert_eq!(loaded.project, project);
    assert_eq!(loaded.project.to_json(), text);
    // The parts serialise like the others: a tagged variant with the model
    // inline.
    let s1 = "\"part\": {\n              \"VoltageSwitch\": {\n                \"model\": {\n                  \"name\": \"SW\",\n                  \"ron\": 1.0,\n                  \"roff\": 1000000.0,\n                  \"vt\": 0.5,\n                  \"vh\": 0.0\n                }\n              }\n            }";
    assert!(text.contains(s1), "{text}");
    let w1 = "\"part\": {\n              \"CurrentSwitch\": {\n                \"control\": \"VS\",\n                \"model\": {\n                  \"name\": \"CSW\",\n                  \"ron\": 1.0,\n                  \"roff\": 1000000.0,\n                  \"it\": 0.001,\n                  \"ih\": 0.0\n                }\n              }\n            }";
    assert!(text.contains(w1), "{text}");
    // Saved and reopened, the file is unchanged.
    let temp = tempfile::tempdir().expect("temp dir");
    let path = temp.path().join("switches.xsim_proj");
    project.save(&path).expect("saves");
    let reopened = Project::load(&path).expect("reopens");
    reopened.project.save(&path).expect("saves again");
    assert_eq!(std::fs::read_to_string(&path).expect("reads"), text);
}

#[test]
fn files_written_before_switches_still_load_unchanged() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/RC low-pass.xsim_proj");
    let text = std::fs::read_to_string(path).expect("the example exists");
    let loaded = Project::from_json(&text).expect("loads");
    assert_eq!(loaded.compatibility, Compatibility::Current);
    assert_eq!(loaded.project.to_json(), text);
}

#[test]
fn a_dangling_switch_control_still_opens() {
    // Like an unknown model (Q44), a control naming no voltage source is an
    // electrical problem reported when simulating, not a broken file.
    for control in ["V9", "R1", ""] {
        let mut s = switches(1.0, 2e-3);
        component(&mut s, "W1").part = current_switch(control);
        let project = project_with(s);
        let loaded = Project::from_json(&project.to_json()).expect("opens");
        assert_eq!(loaded.project, project, "{control:?}");
    }
}

#[test]
fn switch_models_missing_fields_take_their_defaults() {
    let project = project_with(switches(1.0, 2e-3));
    let mut value = serde_json::to_value(&project).expect("serialises");
    let components = value["circuits"][0]["schematic"]["components"]
        .as_array_mut()
        .expect("components");
    let s1 = components
        .iter_mut()
        .find(|c| c["name"] == "S1")
        .expect("S1");
    s1["part"] = json!({"VoltageSwitch": {"model": {"ron": 2.0}}});
    let text = serde_json::to_string_pretty(&value).expect("text");
    let loaded = Project::from_json(&text).expect("loads");
    let part = &loaded.project.circuits[0]
        .schematic
        .components
        .iter()
        .find(|c| c.name == "S1")
        .expect("S1")
        .part;
    assert_eq!(
        *part,
        Part::VoltageSwitch {
            model: SwitchModel {
                ron: 2.0,
                ..SwitchModel::default()
            }
        }
    );
}

#[test]
fn non_finite_switch_parameters_are_refused() {
    // The engine's model serde reads "inf" and "nan", but a project only
    // holds numbers JSON can represent, as for every other part value.
    let project = project_with(switches(1.0, 2e-3));
    let good = serde_json::to_value(&project).expect("serialises");
    for (name, field) in [("S1", "roff"), ("S1", "vh"), ("W1", "it"), ("W1", "ron")] {
        let mut bad = good.clone();
        let components = bad["circuits"][0]["schematic"]["components"]
            .as_array_mut()
            .expect("components");
        let part = components
            .iter_mut()
            .find(|c| c["name"] == name)
            .expect("part");
        let variant = if name == "S1" {
            "VoltageSwitch"
        } else {
            "CurrentSwitch"
        };
        part["part"][variant]["model"][field] = Value::String("inf".into());
        match Project::from_json(&serde_json::to_string(&bad).expect("text")) {
            Err(ProjectError::Invalid(message)) => assert!(
                message.contains(name) && message.contains("not a finite number"),
                "{name}.{field}: {message}"
            ),
            other => panic!("{name}.{field}: expected Invalid, got {other:?}"),
        }
    }
    let mut project = project;
    component(&mut project.circuits[0].schematic, "W1").part = Part::CurrentSwitch {
        control: "VS".into(),
        model: CurrentSwitchModel {
            ih: f64::NAN,
            ..CurrentSwitchModel::default()
        },
    };
    let temp = tempfile::tempdir().expect("temp dir");
    let path = temp.path().join("bad.xsim_proj");
    assert!(matches!(project.save(&path), Err(ProjectError::Invalid(_))));
    assert!(!path.exists());
}

// ------------------------------------------------------------ fingerprints

#[test]
fn switch_edits_change_the_circuit_fingerprint() {
    let fingerprint =
        |s: &Schematic| StoredResult::fingerprint(&netlist(s).expect("netlists").circuit);
    let base = switches(1.0, 2e-3);
    let reference = fingerprint(&base);
    assert_eq!(reference, fingerprint(&switches(1.0, 2e-3)));
    let mut edits: Vec<Schematic> = Vec::new();
    let mut s = base.clone();
    component(&mut s, "W1").part = current_switch("V1");
    edits.push(s);
    let mut s = base.clone();
    if let Part::CurrentSwitch { model, .. } = &mut component(&mut s, "W1").part {
        model.ih = 1e-4;
    }
    edits.push(s);
    let mut s = base.clone();
    if let Part::VoltageSwitch { model } = &mut component(&mut s, "S1").part {
        model.vt = 0.6;
    }
    edits.push(s);
    for (k, edited) in edits.iter().enumerate() {
        assert_ne!(fingerprint(edited), reference, "edit {k}");
    }
}
