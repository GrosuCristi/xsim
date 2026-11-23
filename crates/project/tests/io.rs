//! Project files: canonical JSON, lossless round trips, atomic saves, schema
//! migration and newer-schema handling.

mod common;

use common::{Sketch, rc_low_pass_schematic};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use xsim_project::{
    AnalysisChoice, CircuitDocument, Compatibility, Orientation, Part, PartKind, PlotPane,
    PlotSetup, Project, ProjectError, SCHEMA_VERSION, SchematicView, StoredResult, Trace,
    WorkspaceLayout, YScale,
};
use xsim_spice_engine::{
    Analysis, AnalysisKind, Axis, InitialState, IntegrationMethod, Polarity, Quantity, Signal,
    SignalData, SignalOrigin, SimulationResult, Source, Transient, Waveform,
};

const FIXTURE: &str = include_str!("fixtures/schema1_rc_lowpass.xsim_proj");

/// A project exercising every document feature and awkward float values.
fn rich_project() -> Project {
    let mut project = Project::new("Rich \"quoted\" ünïcode");
    project.layout = WorkspaceLayout {
        diagnostics_open: true,
        schematic_fraction: 0.37,
    };
    let mut s = Sketch {
        schematic: rc_low_pass_schematic(),
    };
    let sources = [
        Waveform::Pulse {
            initial: 0.0,
            pulsed: 5.0,
            delay: 1e-9,
            rise: 2e-9,
            fall: 3e-9,
            width: 1e-6,
            period: 2e-6,
            cycles: Some(3),
        },
        Waveform::Sine {
            offset: 0.1 + 0.2,
            amplitude: 1.0 / 3.0,
            frequency: 1e3,
            delay: 0.0,
            damping: 10.0,
            phase: -90.0,
            cycles: Some(2.5),
        },
        Waveform::Exp {
            initial: -1.0,
            pulsed: 1.0,
            rise_delay: 1e-3,
            rise_tau: 1e-4,
            fall_delay: 5e-3,
            fall_tau: 2e-4,
        },
        Waveform::Pwl {
            points: vec![(0.0, 0.0), (1e-300, 5e-324), (1.0, f64::MAX)],
            repeat: true,
        },
        Waveform::Sffm {
            offset: 0.0,
            amplitude: 1.0,
            carrier: 1e6,
            index: 5.0,
            signal: 1e3,
        },
    ];
    for (i, waveform) in sources.into_iter().enumerate() {
        let source = Source {
            dc: -0.0,
            ac_magnitude: 1.0,
            ac_phase: 12.5,
            waveform: Some(waveform),
        };
        let part = if i % 2 == 0 {
            Part::VoltageSource { source }
        } else {
            Part::CurrentSource { source }
        };
        let x = 40 + 12 * i32::try_from(i).unwrap();
        s.part(part, (x, 0), u8::try_from(i % 4).unwrap());
    }
    for kind in [
        PartKind::Diode,
        PartKind::PnpBjt,
        PartKind::NJfet,
        PartKind::Inductor,
    ] {
        let id = s.part(Part::default_for(kind), (0, -40), 2);
        s.schematic.component_mut(id).unwrap().orientation = Orientation {
            rotation: 3,
            mirror_x: true,
            mirror_y: true,
        };
    }
    s.part(
        Part::Capacitor {
            capacitance: f64::MIN_POSITIVE,
            initial_voltage: Some(-1.5),
        },
        (-60, -60),
        0,
    );
    s.wire(&[(-200_000, 14), (6, 199_998), (-10, -10)]);
    s.label("bus[0]", (-14, 6));

    let doc = &mut project.circuits[0];
    doc.name = "Main".into();
    doc.schematic = s.schematic;
    doc.view = SchematicView {
        center: [1.5, -2.25],
        zoom: 33.3,
        user_set: true,
    };
    doc.simulation.selected = AnalysisChoice::Ac;
    doc.simulation.transient.max_step = Some(1e-7);
    doc.simulation.transient.initial_state = InitialState::ZeroState;
    doc.simulation.options.method = IntegrationMethod::Gear2;
    doc.simulation.options.temperature = -40.0;
    doc.plot = PlotSetup {
        panes: vec![
            PlotPane {
                traces: vec![
                    Trace::new("V(out)"),
                    Trace {
                        expression: "I(R1)*1k".into(),
                        color: Some(0xff8800),
                        visible: false,
                        slot: Some(3),
                    },
                ],
                x_range: Some([0.0, 1e-3]),
                y_range: Some([-0.5, 1.5]),
                x_log: Some(true),
                y_scale: YScale::Decibel,
                y2_range: Some([-2e-3, 2.5e-3]),
                height: Some(1.0 / 3.0),
            },
            PlotPane::default(),
        ],
        active_pane: 1,
    };
    doc.result = Some(StoredResult {
        circuit_fingerprint: Some("fnv1a64-0123456789abcdef".into()),
        analysis: Analysis::Transient(Transient {
            stop: 2e-5,
            step: 1e-5,
            start_save: 0.0,
            max_step: None,
            initial_state: InitialState::OperatingPoint,
        }),
        result: SimulationResult {
            analysis: AnalysisKind::Transient,
            axis: Axis {
                name: "time".into(),
                unit: "s".into(),
                values: vec![0.0, 1e-5, 2e-5],
            },
            signals: vec![
                Signal {
                    name: "V(out)".into(),
                    quantity: Quantity::Voltage,
                    origin: SignalOrigin::Node(2),
                    data: SignalData::Real(vec![0.0, 0.1 + 0.7, 1.0 - 1e-16]),
                },
                Signal {
                    name: "I(V1)".into(),
                    quantity: Quantity::Current,
                    origin: SignalOrigin::TerminalCurrent {
                        element: 0,
                        terminal: 0,
                    },
                    data: SignalData::Complex {
                        re: vec![1.0, 2.0, 3.0],
                        im: vec![-1e-200, 0.0, 7e100],
                    },
                },
            ],
        },
    });
    let mut bias = CircuitDocument::new("Bias");
    bias.simulation.selected = AnalysisChoice::OperatingPoint;
    bias.plot.panes.clear();
    project.circuits.push(bias);
    project.active_circuit = 1;
    project
}

fn files_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn load_value(value: &Value) -> Result<xsim_project::LoadedProject, ProjectError> {
    Project::from_json(&serde_json::to_string_pretty(value).unwrap())
}

#[test]
fn round_trip_is_lossless_and_byte_identical() {
    let project = rich_project();
    let text = project.to_json();
    let loaded = Project::from_json(&text).unwrap();
    assert_eq!(loaded.compatibility, Compatibility::Current);
    assert_eq!(loaded.project, project);
    assert_eq!(loaded.project.to_json(), text);
}

/// D9, D11 and D23 added `PlotPane::y2_range`, `Trace::slot` and
/// `PlotPane::height`. Files written before them have none of these keys:
/// they load with the defaults and save back byte-identically, because
/// unset values are not written.
#[test]
fn files_without_right_axis_ranges_trace_slots_or_pane_heights_round_trip_unchanged() {
    let mut project = rich_project();
    for pane in &mut project.circuits[0].plot.panes {
        pane.y2_range = None;
        pane.height = None;
        for trace in &mut pane.traces {
            trace.slot = None;
        }
    }
    let text = project.to_json();
    assert!(!text.contains("\"slot\"") && !text.contains("\"y2_range\""));
    assert!(!text.contains("\"height\""));
    let loaded = Project::from_json(&text).unwrap();
    assert_eq!(loaded.compatibility, Compatibility::Current);
    assert_eq!(loaded.project, project);
    assert_eq!(loaded.project.to_json(), text);

    // Set values are written and read back.
    let rich = rich_project().to_json();
    assert!(rich.contains("\"slot\": 3") && rich.contains("\"y2_range\": ["));
    // The height is the last key of its pane; the default pane has none.
    let height = rich
        .find("\"height\": 0.33333334\n")
        .expect("the height is written");
    assert!(
        rich[height..]
            .split('\n')
            .nth(1)
            .is_some_and(|l| l.trim() == "},"),
        "the height is the last key of its pane"
    );
    assert_eq!(
        rich.matches("\"height\"").count(),
        1,
        "the default pane has none"
    );
    let panes = &Project::from_json(&rich).unwrap().project.circuits[0]
        .plot
        .panes;
    let pane = &panes[0];
    assert_eq!(pane.y2_range, Some([-2e-3, 2.5e-3]));
    assert_eq!(pane.traces[1].slot, Some(3));
    assert_eq!(
        pane.height,
        Some(1.0 / 3.0),
        "the height round-trips exactly"
    );
    assert_eq!(panes[1].height, None);
}

/// Every schema-2 fixture, written before pane heights existed (D23),
/// opens without heights; resaved in the current format it has no height
/// keys either.
#[test]
fn older_fixtures_have_no_pane_heights() {
    for text in [
        include_str!("fixtures/schema2_rc_lowpass.xsim_proj"),
        include_str!("fixtures/schema2_mixed.xsim_proj"),
        FIXTURE,
    ] {
        let project = Project::from_json(text).unwrap().project;
        let panes = project.circuits.iter().flat_map(|c| &c.plot.panes);
        assert!(panes.into_iter().all(|p| p.height.is_none()));
        assert!(!project.to_json().contains("\"height\""));
    }
}

/// A stored pane height is a relative weight (D23): it must be a positive
/// finite number. A number too small for `f32` reads as zero; one too large
/// for it is a format error, as for any other `f32` of the file.
#[test]
fn pane_heights_must_be_positive_and_finite() {
    let good = serde_json::to_value(rich_project()).unwrap();
    for bad in [json!(0.0), json!(-1.0), json!(-0.0), json!(1e-50)] {
        let mut value = good.clone();
        value["circuits"][0]["plot"]["panes"][1]["height"] = bad.clone();
        match load_value(&value) {
            Err(ProjectError::Invalid(text)) => {
                assert!(text.contains("plot pane 2 has the height"), "{bad}: {text}");
                assert!(text.contains("positive finite"), "{bad}: {text}");
            }
            other => panic!("{bad}: expected Invalid, got {other:?}"),
        }
    }
    // A height that is not a number, or does not fit `f32`, is a format
    // error, like any other wrongly typed value of the current schema.
    for bad in [json!("tall"), json!(1e39)] {
        let mut value = good.clone();
        value["circuits"][0]["plot"]["panes"][1]["height"] = bad.clone();
        let result = load_value(&value);
        assert!(matches!(result, Err(ProjectError::Format(_))), "{bad}");
    }
    // Tiny and huge weights are kept as they are.
    for fine in [f32::MIN_POSITIVE, 1e-3, 2.5, 1e30] {
        let mut value = good.clone();
        value["circuits"][0]["plot"]["panes"][1]["height"] = json!(fine);
        let project = load_value(&value).unwrap().project;
        assert_eq!(project.circuits[0].plot.panes[1].height, Some(fine));
    }
    // Saving refuses a height the file could not hold.
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("heights.xsim_proj");
    for bad in [0.0, -2.0, f32::INFINITY, f32::NAN] {
        let mut project = rich_project();
        project.circuits[0].plot.panes[0].height = Some(bad);
        let error = project.save(&path).unwrap_err().to_string();
        assert!(
            error.contains("plot pane 1 has the height"),
            "{bad}: {error}"
        );
    }
    assert!(!path.exists());
}

/// Q45: a newer xsim may store pane heights this version cannot use; the
/// file still opens (read-only), with those panes at the default height
/// and the other heights kept.
#[test]
fn newer_schemas_drop_unusable_pane_heights() {
    // Numbers this version cannot use, and values that are not numbers.
    for bad in [
        json!(0.0),
        json!(-3.0),
        json!(1e39),
        json!("tall"),
        json!({"rows": 3}),
    ] {
        let mut value = serde_json::to_value(rich_project()).unwrap();
        value["schema_version"] = json!(SCHEMA_VERSION + 1);
        value["circuits"][0]["plot"]["panes"][1]["height"] = bad.clone();
        let loaded = load_value(&value).unwrap_or_else(|e| panic!("{bad}: {e}"));
        assert_eq!(
            loaded.compatibility,
            Compatibility::Newer {
                version: SCHEMA_VERSION + 1
            }
        );
        let panes = &loaded.project.circuits[0].plot.panes;
        assert_eq!(panes.len(), 2, "{bad}: the pane is kept");
        assert_eq!(panes[1].height, None, "{bad}");
        assert_eq!(panes[0].height, Some(1.0 / 3.0), "{bad}: others are kept");
        assert_eq!(panes[0].y2_range, Some([-2e-3, 2.5e-3]), "{bad}");
    }
}

#[test]
fn missing_trace_and_pane_keys_take_their_defaults() {
    let mut value = serde_json::to_value(rich_project()).unwrap();
    let pane = value["circuits"][0]["plot"]["panes"][0]
        .as_object_mut()
        .unwrap();
    pane.remove("y2_range");
    for trace in pane["traces"].as_array_mut().unwrap() {
        trace.as_object_mut().unwrap().remove("slot");
    }
    let project = load_value(&value).unwrap().project;
    let pane = &project.circuits[0].plot.panes[0];
    assert_eq!(pane.y2_range, None);
    assert!(pane.traces.iter().all(|t| t.slot.is_none()));
    assert_eq!(pane.y_range, Some([-0.5, 1.5]), "other fields are kept");
}

#[test]
fn canonical_text_uses_struct_order_two_space_indent_and_lf() {
    let text = rich_project().to_json();
    assert!(text.ends_with("}\n") && !text.contains('\r'));
    let top_level: Vec<&str> = text
        .lines()
        .filter(|l| l.starts_with("  \"") && !l.starts_with("   "))
        .map(|l| l.trim_start().split('"').nth(1).unwrap())
        .collect();
    assert_eq!(
        top_level,
        [
            "schema_version",
            "name",
            "circuits",
            "active_circuit",
            "layout"
        ]
    );
    let r1 = "{\n            \"id\": 2,\n            \"name\": \"R1\",\n            \"position\": {\n              \"x\": 12,\n              \"y\": 0\n            },\n            \"orientation\": {\n              \"rotation\": 0,\n              \"mirror_x\": false,\n              \"mirror_y\": false\n            },\n            \"part\": {\n              \"Resistor\": {\n                \"resistance\": 1000.0\n              }\n            }\n          }";
    assert!(text.contains(r1), "{text}");
}

#[test]
fn save_writes_the_canonical_text_and_reopens_identically() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("bench.xsim_proj");
    let project = rich_project();
    project.save(&path).unwrap();
    let first = fs::read(&path).unwrap();
    assert_eq!(first, project.to_json().as_bytes());
    let reopened = Project::load(&path).unwrap();
    assert_eq!(reopened.compatibility, Compatibility::Current);
    assert_eq!(reopened.project, project);
    reopened.project.save(&path).unwrap();
    assert_eq!(fs::read(&path).unwrap(), first);
    assert_eq!(files_in(temp.path()), ["bench.xsim_proj"]);
}

#[test]
fn a_failed_save_never_touches_the_previous_file() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("previous.xsim_proj");
    let mut project = rich_project();
    project.save(&path).unwrap();
    let previous = fs::read(&path).unwrap();

    // JSON cannot represent NaN or infinity: such projects are refused.
    project.circuits[0].schematic.components[1].part = Part::Resistor {
        resistance: f64::NAN,
    };
    assert!(matches!(project.save(&path), Err(ProjectError::Invalid(_))));
    project = rich_project();
    project.circuits[0].plot.panes[0].x_range = Some([0.0, f64::INFINITY]);
    let error = project.save(&path).unwrap_err().to_string();
    assert!(error.contains("plot range"), "{error}");
    project = rich_project();
    project.circuits[0].plot.panes[0].y2_range = Some([f64::NEG_INFINITY, 1.0]);
    let error = project.save(&path).unwrap_err().to_string();
    assert!(error.contains("plot range"), "{error}");
    assert_eq!(fs::read(&path).unwrap(), previous);

    // A target that cannot be replaced fails after writing the temporary
    // file, which must be cleaned up.
    let blocked = temp.path().join("blocked.xsim_proj");
    fs::create_dir(&blocked).unwrap();
    assert!(matches!(
        rich_project().save(&blocked),
        Err(ProjectError::Io(_))
    ));
    assert_eq!(
        files_in(temp.path()),
        ["blocked.xsim_proj", "previous.xsim_proj"]
    );
    assert_eq!(fs::read(&path).unwrap(), previous);
}

#[test]
fn saving_into_a_missing_directory_is_an_io_error() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("missing").join("p.xsim_proj");
    assert!(matches!(
        Project::new("P").save(&path),
        Err(ProjectError::Io(_))
    ));
}

#[test]
fn loading_garbage_fails_with_a_clear_error() {
    let temp = tempfile::tempdir().unwrap();
    assert!(matches!(
        Project::load(&temp.path().join("absent.xsim_proj")),
        Err(ProjectError::Io(_))
    ));
    for (text, expect_format) in [
        ("not json", true),
        ("{\"schema_version\": 3", true),
        ("[1, 2]", false),
        ("{}", false),
        ("{\"name\": \"x\"}", false),
        ("{\"schema_version\": \"2\"}", false),
    ] {
        match Project::from_json(text) {
            Err(ProjectError::Format(_)) => assert!(expect_format, "{text}"),
            Err(ProjectError::Invalid(_)) => assert!(!expect_format, "{text}"),
            other => panic!("{text}: unexpected {other:?}"),
        }
    }
}

#[test]
fn structurally_inconsistent_projects_are_rejected() {
    let good = serde_json::to_value(rich_project()).unwrap();
    let cases: [(&str, Value, &str); 12] = [
        ("/active_circuit", json!(2), "active circuit"),
        ("/circuits", json!([]), "no circuits"),
        (
            "/circuits/0/schematic/components/1/id",
            json!(1),
            "used more than once",
        ),
        (
            "/circuits/0/schematic/labels/0/id",
            json!(5),
            "used more than once",
        ),
        (
            "/circuits/0/schematic/wires/0/points",
            json!([{"x": 0, "y": 0}]),
            "fewer than two points",
        ),
        (
            "/circuits/0/schematic/wires/0/points",
            json!([{"x": 0, "y": 0}, {"x": 0, "y": 0}]),
            "repeats a point",
        ),
        (
            "/circuits/0/schematic/components/0/orientation/rotation",
            json!(4),
            "rotation",
        ),
        (
            "/circuits/0/schematic/components/0/position/x",
            json!(2_000_000_000),
            "outside",
        ),
        ("/circuits/0/plot/active_pane", json!(2), "plot pane"),
        (
            "/circuits/0/result/result/signals/0/data/Real",
            json!([0.0]),
            "samples",
        ),
        (
            "/circuits/0/result/circuit_fingerprint",
            json!("not a fingerprint"),
            "malformed circuit fingerprint",
        ),
        (
            "/circuits/0/result/circuit_fingerprint",
            json!(""),
            "malformed circuit fingerprint",
        ),
    ];
    for (pointer, value, message) in cases {
        let mut bad = good.clone();
        *bad.pointer_mut(pointer).unwrap() = value;
        match load_value(&bad) {
            Err(ProjectError::Invalid(text)) => {
                assert!(text.contains(message), "{pointer}: {text}")
            }
            other => panic!("{pointer}: expected Invalid, got {other:?}"),
        }
    }
}

#[test]
fn newer_schemas_open_best_effort_and_cannot_be_saved_over() {
    let mut value = serde_json::to_value(rich_project()).unwrap();
    value["schema_version"] = json!(SCHEMA_VERSION + 1);
    value["future_asset"] = json!({"payload": [1, 2, 3]});
    value["circuits"][0]["schematic"]["components"][0]["future_flag"] = json!(true);
    let loaded = load_value(&value).unwrap();
    assert_eq!(
        loaded.compatibility,
        Compatibility::Newer {
            version: SCHEMA_VERSION + 1
        }
    );
    let mut expected = rich_project();
    expected.schema_version = SCHEMA_VERSION + 1;
    assert_eq!(loaded.project, expected);

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("future.xsim_proj");
    let original = serde_json::to_vec_pretty(&value).unwrap();
    fs::write(&path, &original).unwrap();
    let project = Project::load(&path).unwrap().project;
    let error = project.save(&path).unwrap_err().to_string();
    assert!(error.contains("newer version"), "{error}");
    assert_eq!(fs::read(&path).unwrap(), original);
    assert_eq!(files_in(temp.path()), ["future.xsim_proj"]);

    // Saving a copy in the current format is a deliberate choice.
    let mut copy = project;
    copy.schema_version = SCHEMA_VERSION;
    copy.save(&temp.path().join("copy.xsim_proj")).unwrap();
}

/// INT-1 and Q45: a newer xsim may fingerprint its results in a way this
/// version cannot even read (other characters, a longer digest). The file
/// still opens read-only; the result is kept but shows as out of date, as
/// any fingerprint this version did not write does, and never falls back
/// to the weaker node-name check of files without a fingerprint.
#[test]
fn newer_schemas_with_an_unreadable_fingerprint_open_with_an_out_of_date_result() {
    for fingerprint in [
        json!("sha3:0123abcd"),
        json!("blake3_0123"),
        json!("x".repeat(300)),
        json!(""),
    ] {
        let mut value = serde_json::to_value(rich_project()).unwrap();
        value["schema_version"] = json!(SCHEMA_VERSION + 1);
        value["circuits"][0]["result"]["circuit_fingerprint"] = fingerprint.clone();
        let loaded = load_value(&value).unwrap_or_else(|e| panic!("{fingerprint}: {e}"));
        assert_eq!(
            loaded.compatibility,
            Compatibility::Newer {
                version: SCHEMA_VERSION + 1
            },
            "{fingerprint}"
        );
        let doc = &loaded.project.circuits[0];
        let stored = doc.result.as_ref().expect("the result is kept");
        let circuit = xsim_project::netlist(&doc.schematic).map(|n| n.circuit);
        let circuit = circuit.unwrap_or_default();
        assert_eq!(stored.computed_from(&circuit), Some(false), "{fingerprint}");
        // A copy saved in the current format opens normally.
        let mut copy = loaded.project.clone();
        copy.schema_version = SCHEMA_VERSION;
        let reopened = Project::from_json(&copy.to_json()).unwrap().project;
        let stored = reopened.circuits[0].result.as_ref().expect("kept");
        assert_eq!(stored.computed_from(&circuit), Some(false), "{fingerprint}");
    }
    // A fingerprint that is not even text leaves the whole result
    // unreadable, like a result of an unknown analysis: it is left out.
    let mut value = serde_json::to_value(rich_project()).unwrap();
    value["schema_version"] = json!(SCHEMA_VERSION + 1);
    value["circuits"][0]["result"]["circuit_fingerprint"] = json!({"digest": "0123"});
    let loaded = load_value(&value).unwrap();
    assert_eq!(loaded.project.circuits[0].result, None);
    // The current format still rejects a malformed fingerprint.
    let mut current = serde_json::to_value(rich_project()).unwrap();
    current["circuits"][0]["result"]["circuit_fingerprint"] = json!("sha3:0123abcd");
    assert!(matches!(
        load_value(&current),
        Err(ProjectError::Invalid(_))
    ));
}

#[test]
fn newer_schemas_skip_what_this_version_cannot_read() {
    let mut value = serde_json::to_value(rich_project()).unwrap();
    value["schema_version"] = json!(99);
    let circuit = &mut value["circuits"][0];
    let components = circuit["schematic"]["components"].as_array_mut().unwrap();
    let total = components.len();
    components[1]["part"] = json!({"Mosfet": {"model": "IRF540"}});
    circuit["schematic"]["wires"][0]["points"] = json!("spline");
    circuit["simulation"]["selected"] = json!("Noise");
    circuit["plot"]["panes"][0]["y_scale"] = json!("Smith");
    circuit["plot"]["panes"][0]["traces"][1]["expression"] = json!({"ast": []});
    circuit["result"]["analysis"] = json!({"Noise": {}});
    value["circuits"]
        .as_array_mut()
        .unwrap()
        .push(json!("a circuit kind from the future"));

    let loaded = load_value(&value).unwrap();
    assert_eq!(loaded.compatibility, Compatibility::Newer { version: 99 });
    let project = loaded.project;
    assert_eq!(project.circuits.len(), 2);
    assert_eq!(project.active_circuit, 1);
    let doc = &project.circuits[0];
    let reference = &rich_project().circuits[0];
    assert_eq!(doc.schematic.components.len(), total - 1);
    assert!(doc.schematic.components.iter().all(|c| c.name != "R1"));
    assert_eq!(
        doc.schematic.wires.len(),
        reference.schematic.wires.len() - 1
    );
    assert_eq!(doc.schematic.labels, reference.schematic.labels);
    assert_eq!(doc.simulation.selected, AnalysisChoice::Transient);
    assert_eq!(doc.simulation.ac, reference.simulation.ac);
    let pane = &doc.plot.panes[0];
    assert_eq!(pane.y_scale, YScale::Linear);
    assert_eq!(pane.x_range, reference.plot.panes[0].x_range);
    assert_eq!(pane.traces, [Trace::new("V(out)")]);
    assert_eq!(doc.result, None);
    assert_eq!(doc.view, reference.view);

    // Nothing readable at all is still an error.
    let unreadable = json!({"schema_version": 99, "circuits": [1, 2]});
    assert!(matches!(
        load_value(&unreadable),
        Err(ProjectError::Invalid(_))
    ));
}

#[test]
fn schema1_fixture_migrates_to_the_current_document() {
    let loaded = Project::from_json(FIXTURE).unwrap();
    assert_eq!(loaded.compatibility, Compatibility::Migrated { from: 1 });
    let project = loaded.project;
    assert_eq!(project.schema_version, SCHEMA_VERSION);
    assert_eq!(project.name, "RC low-pass");
    assert_eq!(project.layout, WorkspaceLayout::default());
    let doc = &project.circuits[0];
    assert_eq!(doc.name, "RC low-pass");

    let s = &doc.schematic;
    let summary: Vec<_> = s
        .components
        .iter()
        .map(|c| (c.id, c.name.as_str(), c.position, c.orientation.rotation))
        .collect();
    assert_eq!(
        summary,
        [
            (1, "V1", xsim_project::Point::new(0, 8), 1),
            (2, "R1", xsim_project::Point::new(12, 0), 0),
            (3, "C1", xsim_project::Point::new(24, 8), 1),
            (4, "GND1", xsim_project::Point::new(12, 16), 0),
        ]
    );
    assert_eq!(
        s.components[0].part,
        Part::VoltageSource {
            source: Source::ac(1.0, 1.0, 0.0)
        }
    );
    assert_eq!(s.components[1].part, Part::Resistor { resistance: 1000.0 });
    assert_eq!(
        s.components[2].part,
        Part::Capacitor {
            capacitance: 1e-6,
            initial_voltage: None
        }
    );
    assert_eq!(s.components[3].part, Part::Ground);
    assert_eq!(s.wires.len(), 4);
    assert_eq!(s.wires[2].points.len(), 3);
    assert!(s.labels.is_empty());
    assert_eq!(s.next_id, 9);

    assert_eq!(doc.simulation.selected, AnalysisChoice::Transient);
    assert_eq!(
        doc.simulation.transient,
        Transient {
            stop: 0.005,
            step: 0.00001,
            start_save: 0.0,
            max_step: None,
            initial_state: InitialState::ZeroState,
        }
    );
    // Plotted node indices and results are dropped.
    assert_eq!(doc.plot.panes, [PlotPane::default()]);
    assert_eq!(doc.result, None);
    // The prototype stored no view: the default one, fitted on opening.
    assert_eq!(doc.view, SchematicView::default());

    // A migrated project saves and reloads as the current schema.
    let reloaded = Project::from_json(&project.to_json()).unwrap();
    assert_eq!(reloaded.compatibility, Compatibility::Current);
    assert_eq!(reloaded.project, project);
}

#[test]
fn schema1_analyses_and_quirks_migrate() {
    let circuit = |analysis: Value| {
        json!({
            "name": "c",
            "schematic": {
                "components": [{
                    "id": 7, "name": "I1", "kind": "CurrentSource",
                    "position": {"x": -3, "y": 2}, "rotation": 2,
                    "mirror_x": true, "mirror_y": false, "value": -0.002,
                    "disconnected": [1]
                }],
                "wires": [{"id": 9, "points": [{"x": 0, "y": 0}, {"x": 0, "y": 0}, {"x": 4, "y": 0}]}]
            },
            "analysis": analysis,
            "plot": {"nodes": [1, 5], "x_range": [0.0, 2.0], "y_range": null},
            "result": {"axis": [0.0], "voltages": [[0.0]], "currents": [[0.0]],
                       "voltage_phase_deg": null, "current_phase_deg": null}
        })
    };
    let file = json!({
        "schema_version": 1,
        "name": "quirks",
        "circuits": [
            circuit(json!("OperatingPoint")),
            circuit(json!({"DcSweep": {"source": "I1", "start": 0.0, "stop": 1.0, "points": 11}})),
            circuit(json!({"AcSweep": {"start_hz": 10.0, "stop_hz": 1e5, "points": 41}})),
        ],
        "active_circuit": 2,
        "layout": {"dark": true, "scale": 1.25, "diagnostics_open": true}
    });
    let project = load_value(&file).unwrap().project;
    assert_eq!(project.active_circuit, 2);
    assert!(project.layout.diagnostics_open);
    let selected: Vec<_> = project
        .circuits
        .iter()
        .map(|c| c.simulation.selected)
        .collect();
    assert_eq!(
        selected,
        [
            AnalysisChoice::OperatingPoint,
            AnalysisChoice::DcSweep,
            AnalysisChoice::Ac
        ]
    );
    let dc = &project.circuits[1].simulation.dc;
    assert_eq!(
        (dc.source.as_str(), dc.start, dc.stop, dc.step),
        ("I1", 0.0, 1.0, 0.1)
    );
    let ac = &project.circuits[2].simulation.ac;
    assert_eq!((ac.start, ac.stop, ac.points), (10.0, 1e5, 10));
    // Analyses that were not selected keep the defaults.
    assert_eq!(
        project.circuits[0].simulation.transient.initial_state,
        InitialState::OperatingPoint
    );

    let doc = &project.circuits[0];
    let component = &doc.schematic.components[0];
    // Schema-1 coordinates are schema-2 units, doubled on the way (D22).
    assert_eq!(component.position, xsim_project::Point::new(-6, 4));
    assert_eq!(
        component.orientation,
        Orientation {
            rotation: 2,
            mirror_x: true,
            mirror_y: false
        }
    );
    assert_eq!(
        component.part,
        Part::CurrentSource {
            source: Source::ac(-0.002, 0.002, 180.0)
        }
    );
    assert_eq!(
        doc.schematic.wires[0].points,
        [
            xsim_project::Point::new(0, 0),
            xsim_project::Point::new(8, 0)
        ]
    );
    assert_eq!(doc.plot.panes[0].x_range, Some([0.0, 2.0]));
    assert!(doc.plot.panes[0].traces.is_empty());
    assert_eq!(doc.result, None);
    assert_eq!(doc.schematic.next_id, 10);
}

#[test]
fn malformed_schema1_files_are_errors() {
    let mut value: Value = serde_json::from_str(FIXTURE).unwrap();
    value["circuits"][0]["schematic"]["components"][0]["kind"] = json!("Transistor");
    assert!(matches!(load_value(&value), Err(ProjectError::Format(_))));
    let mut value: Value = serde_json::from_str(FIXTURE).unwrap();
    value["circuits"][0]["schematic"]["components"][0]["rotation"] = json!(7);
    assert!(matches!(load_value(&value), Err(ProjectError::Invalid(_))));
}

#[test]
fn semiconductor_parts_serialise_with_polarity_and_model() {
    let mut project = Project::new("Semis");
    let mut s = Sketch::new();
    s.part(
        Part::Jfet {
            polarity: Polarity::P,
            model: "Custom".into(),
        },
        (0, 0),
        0,
    );
    project.circuits[0].schematic = s.schematic;
    let text = project.to_json();
    assert!(text.contains("\"Jfet\": {\n"), "{text}");
    assert!(text.contains("\"polarity\": \"P\",\n"), "{text}");
    // A project can be opened even when a model is unknown (Q44).
    assert_eq!(Project::from_json(&text).unwrap().project, project);
}
