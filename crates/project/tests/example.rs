//! The bundled example project, and end-to-end simulation of project files.
//!
//! `examples/RC low-pass.xsim_proj` is generated from [`rc_low_pass`] and
//! must always equal its canonical text. After changing the example or the
//! file format, regenerate it with
//!
//! ```text
//! cargo test -p xsim-project --test example -- --ignored regenerate_example
//! ```

mod common;

use common::{P, rc_low_pass_schematic, sample_at};
use std::path::PathBuf;
use xsim_project::{
    AnalysisChoice, CircuitDocument, Compatibility, Netlist, PlotPane, PlotSetup, Project,
    StoredResult, Trace, netlist,
};
use xsim_spice_engine::{InitialState, SimControl, SimulationResult, simulate};

/// Time constant of the example: 1 kΩ × 1 µF.
const TAU: f64 = 1e3 * 1e-6;

fn example_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/RC low-pass.xsim_proj")
}

/// The example: a 1 V step into an RC low-pass, 5 ms from rest.
fn rc_low_pass() -> Project {
    let mut project = Project::new("RC low-pass");
    let doc = &mut project.circuits[0];
    doc.name = "RC low-pass".into();
    doc.schematic = rc_low_pass_schematic();
    doc.simulation.selected = AnalysisChoice::Transient;
    doc.simulation.transient.stop = 5e-3;
    doc.simulation.transient.step = 10e-6;
    doc.simulation.transient.initial_state = InitialState::ZeroState;
    doc.plot = PlotSetup {
        panes: vec![PlotPane {
            traces: vec![Trace::new("V(out)")],
            ..PlotPane::default()
        }],
        active_pane: 0,
    };
    project
}

#[test]
fn example_file_is_the_canonical_text_of_the_example() {
    let on_disk = std::fs::read_to_string(example_path()).expect("the example file exists");
    assert!(
        on_disk == rc_low_pass().to_json(),
        "examples/RC low-pass.xsim_proj is stale; regenerate it (see the module docs)"
    );
}

#[test]
fn example_file_loads_as_a_current_project() {
    let loaded = Project::load(&example_path()).unwrap();
    assert_eq!(loaded.compatibility, Compatibility::Current);
    assert_eq!(loaded.project, rc_low_pass());
    assert!(loaded.project.circuits[0].result.is_none());
}

/// Netlists and simulates `doc` with its own settings.
fn run(doc: &CircuitDocument) -> (Netlist, SimulationResult) {
    let n = netlist(&doc.schematic).unwrap();
    let result = simulate(
        &n.circuit,
        &doc.simulation.analysis(),
        &doc.simulation.options,
        &SimControl::new(),
    )
    .unwrap();
    (n, result)
}

/// Simulates the active circuit and checks `V(<node at probe>)` against
/// the analytic step response `1 − e^(−t/τ)` at 1 ms and 5 ms.
fn assert_step_response(project: &Project, probe: xsim_project::Point) {
    let (n, result) = run(&project.circuits[project.active_circuit]);
    let node = n.node_name_at(probe).expect("the probe is on a net");
    let times = &result.axis.values;
    let v_out = result
        .signal(&format!("V({node})"))
        .and_then(|s| s.real())
        .expect("the output voltage is recorded");
    assert_eq!(times.first(), Some(&0.0));
    assert!(v_out[0].abs() <= 1e-12, "the transient starts from rest");
    for t in [1e-3, 5e-3] {
        let i = sample_at(times, t);
        let expected = 1.0 - (-times[i] / TAU).exp();
        assert!(
            (v_out[i] - expected).abs() <= 1e-6,
            "V({node}) at {t} s is {}, expected {expected}",
            v_out[i]
        );
    }
}

#[test]
fn example_simulates_to_the_analytic_step_response() {
    let project = Project::load(&example_path()).unwrap().project;
    assert_step_response(&project, P(12, 2));
}

#[test]
fn schema1_fixture_migrates_and_simulates_to_the_same_response() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/schema1_rc_lowpass.xsim_proj");
    let loaded = Project::load(&path).unwrap();
    assert_eq!(loaded.compatibility, Compatibility::Migrated { from: 1 });
    assert_step_response(&loaded.project, P(12, 2));
}

#[test]
fn a_simulated_result_is_stored_and_reloaded_exactly() {
    let mut project = rc_low_pass();
    let doc = &mut project.circuits[0];
    let (netlist, result) = run(doc);
    doc.result = Some(StoredResult {
        analysis: doc.simulation.analysis(),
        circuit_fingerprint: Some(StoredResult::fingerprint(&netlist.circuit)),
        result,
    });
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("simulated.xsim_proj");
    project.save(&path).unwrap();
    let saved = std::fs::read(&path).unwrap();
    let loaded = Project::load(&path).unwrap();
    assert_eq!(loaded.project, project);
    loaded.project.save(&path).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), saved);
}

#[test]
#[ignore = "rewrites examples/RC low-pass.xsim_proj; run explicitly after changing the example"]
fn regenerate_example() {
    rc_low_pass().save(&example_path()).unwrap();
}
