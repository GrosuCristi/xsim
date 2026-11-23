//! INT-1: a stored result carries a fingerprint of the circuit it was
//! computed from, so a project opened later can tell whether the result
//! still belongs to its schematic.

mod common;

use common::rc_low_pass_schematic;
use xsim_project::{Part, Project, Schematic, StoredResult, netlist};
use xsim_spice_engine::{
    Analysis, AnalysisKind, Axis, Quantity, Signal, SignalData, SignalOrigin, SimulationResult,
    Source, Waveform,
};

fn fingerprint(schematic: &Schematic) -> String {
    StoredResult::fingerprint(&netlist(schematic).expect("the schematic netlists").circuit)
}

fn set_resistance(schematic: &mut Schematic, name: &str, resistance: f64) {
    let component = schematic
        .components
        .iter_mut()
        .find(|c| c.name == name)
        .expect("the part exists");
    component.part = Part::Resistor { resistance };
}

fn stored(circuit_fingerprint: Option<String>) -> StoredResult {
    StoredResult {
        analysis: Analysis::OperatingPoint,
        circuit_fingerprint,
        result: SimulationResult {
            analysis: AnalysisKind::OperatingPoint,
            axis: Axis {
                name: "point".into(),
                unit: String::new(),
                values: vec![0.0],
            },
            signals: vec![Signal {
                name: "V(out)".into(),
                quantity: Quantity::Voltage,
                origin: SignalOrigin::Node(1),
                data: SignalData::Real(vec![0.5]),
            }],
        },
    }
}

#[test]
fn the_same_circuit_always_has_the_same_fingerprint() {
    let schematic = rc_low_pass_schematic();
    let first = fingerprint(&schematic);
    assert_eq!(first, fingerprint(&schematic.clone()));
    assert_eq!(first, fingerprint(&rc_low_pass_schematic()));
    assert!(first.starts_with("fnv1a64-"), "{first}");
}

/// The INT-1 case: a value edit keeps every node name, so only the
/// fingerprint can tell that the result is out of date.
#[test]
fn a_value_edit_changes_the_fingerprint() {
    let before = rc_low_pass_schematic();
    let mut after = before.clone();
    set_resistance(&mut after, "R1", 2e3);
    let circuit = netlist(&after).expect("netlists").circuit;
    assert_eq!(
        netlist(&before).expect("netlists").circuit.nodes,
        circuit.nodes,
        "the same node names"
    );
    assert_ne!(fingerprint(&before), fingerprint(&after));

    let result = stored(Some(fingerprint(&before)));
    assert_eq!(result.computed_from(&circuit), Some(false));
    let original = netlist(&before).expect("netlists").circuit;
    assert_eq!(result.computed_from(&original), Some(true));
}

/// Replaces the part of component `name`.
fn set_part(schematic: &mut Schematic, name: &str, part: Part) {
    let component = schematic
        .components
        .iter_mut()
        .find(|c| c.name == name)
        .expect("the part exists");
    component.part = part;
}

/// Every electrical edit the netlist carries changes the fingerprint, not
/// only a value: an initial condition (D16), a source's AC setting or
/// waveform, an inductor in place of the capacitor, and a semiconductor
/// model, whose parameters come from the bundled library.
#[test]
fn every_electrical_edit_changes_the_fingerprint() {
    let base = rc_low_pass_schematic();
    let original = fingerprint(&base);
    let pulse = Source {
        waveform: Some(Waveform::Pulse {
            initial: 0.0,
            pulsed: 1.0,
            delay: 0.0,
            rise: 1e-6,
            fall: 1e-6,
            width: 1e-3,
            period: 2e-3,
            cycles: None,
        }),
        ..Source::dc(1.0)
    };
    let slower = Source {
        waveform: Some(Waveform::Pulse {
            initial: 0.0,
            pulsed: 1.0,
            delay: 0.0,
            rise: 1e-6,
            fall: 1e-6,
            width: 1e-3,
            period: 4e-3,
            cycles: None,
        }),
        ..Source::dc(1.0)
    };
    let edits: [(&str, &str, Part); 8] = [
        (
            "capacitor initial condition",
            "C1",
            Part::Capacitor {
                capacitance: 1e-6,
                initial_voltage: Some(0.5),
            },
        ),
        (
            "capacitor initial condition of 0 V",
            "C1",
            Part::Capacitor {
                capacitance: 1e-6,
                initial_voltage: Some(0.0),
            },
        ),
        (
            "inductor",
            "C1",
            Part::Inductor {
                inductance: 1e-3,
                initial_current: None,
            },
        ),
        (
            "inductor initial condition",
            "C1",
            Part::Inductor {
                inductance: 1e-3,
                initial_current: Some(1e-3),
            },
        ),
        (
            "AC amplitude",
            "V1",
            Part::VoltageSource {
                source: Source::ac(1.0, 1.0, 0.0),
            },
        ),
        (
            "pulse waveform",
            "V1",
            Part::VoltageSource { source: pulse },
        ),
        ("pulse period", "V1", Part::VoltageSource { source: slower }),
        (
            "diode",
            "R1",
            Part::Diode {
                model: "D_SIGNAL".into(),
            },
        ),
    ];
    let mut seen = vec![original.clone()];
    for (what, name, part) in edits {
        let mut edited = base.clone();
        set_part(&mut edited, name, part);
        let changed = fingerprint(&edited);
        assert!(!seen.contains(&changed), "{what}: {changed}");
        seen.push(changed);
    }
    // Another model of the same part: only its parameters differ.
    let mut fast = base.clone();
    set_part(
        &mut fast,
        "R1",
        Part::Diode {
            model: "D_FAST_1A".into(),
        },
    );
    assert!(!seen.contains(&fingerprint(&fast)), "another diode model");
}

/// Moving the whole drawing leaves the netlist as it was: the result is
/// still current.
#[test]
fn a_drawing_only_change_keeps_the_fingerprint() {
    let before = rc_low_pass_schematic();
    let mut after = before.clone();
    let shift = |p: &mut xsim_project::Point| *p = xsim_project::Point::new(p.x + 10, p.y - 4);
    after
        .components
        .iter_mut()
        .for_each(|c| shift(&mut c.position));
    after
        .wires
        .iter_mut()
        .for_each(|w| w.points.iter_mut().for_each(shift));
    after.labels.iter_mut().for_each(|l| shift(&mut l.position));
    assert_eq!(fingerprint(&before), fingerprint(&after));
}

/// Results written before the fingerprint existed have none, and loading
/// never invents one.
#[test]
fn results_without_a_fingerprint_load_without_one() {
    let mut project = Project::new("Legacy");
    project.circuits[0].schematic = rc_low_pass_schematic();
    project.circuits[0].result = Some(stored(None));
    let text = project.to_json();
    assert!(!text.contains("circuit_fingerprint"), "{text}");
    let loaded = Project::from_json(&text).expect("loads").project;
    let result = loaded.circuits[0].result.as_ref().expect("result kept");
    assert_eq!(result.circuit_fingerprint, None);
    let circuit = netlist(&loaded.circuits[0].schematic)
        .expect("netlists")
        .circuit;
    assert_eq!(result.computed_from(&circuit), None);
    assert_eq!(loaded.to_json(), text, "saved back byte-identically");
}

/// A fingerprint is saved and read back exactly; one of a scheme this
/// version does not know loads, and just never matches.
#[test]
fn fingerprints_round_trip_and_unknown_schemes_never_match() {
    let schematic = rc_low_pass_schematic();
    let mut project = Project::new("Current");
    project.circuits[0].schematic = schematic.clone();
    project.circuits[0].result = Some(stored(Some(fingerprint(&schematic))));
    let text = project.to_json();
    assert!(
        text.contains("\"circuit_fingerprint\": \"fnv1a64-"),
        "{text}"
    );
    let loaded = Project::from_json(&text).expect("loads").project;
    assert_eq!(loaded, project);

    let circuit = netlist(&schematic).expect("netlists").circuit;
    let later = stored(Some("sha256-0123abcd".into()));
    project.circuits[0].result = Some(later);
    let loaded = Project::from_json(&project.to_json())
        .expect("loads")
        .project;
    let result = loaded.circuits[0].result.as_ref().expect("result kept");
    assert_eq!(result.computed_from(&circuit), Some(false));
}
