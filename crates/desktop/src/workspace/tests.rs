//! Headless UI integration tests of the main window. Each test uses its own
//! data directory and scripted dialogs; simulations run on real threads.

use super::Workspace;
use crate::actions::*;
use crate::diagnostics::{Severity, Source};
use crate::prompts::UnsavedChoice;
use crate::simulation::summary::RunStatus;
use crate::test_support::{
    TestEnv, click, close, dialog, double_click, has_dialog, init, long_run_project, open, press,
    press_in_dialog, rc_project, rc_schematic, series_capacitor_project, switch_project,
    wait_until,
};
use gpui_kit::component::ActiveTheme;
use gpui_kit::{Entity, Focusable, TestAppContext, VisualTestContext};
use std::fs;
use xsim_editor::{EditorEvent, ProbeRequest};
use xsim_project::appdata::ThemePreference;
use xsim_project::{AnalysisChoice, Part, SCHEMA_VERSION};

/// Runs the active circuit with F5 and waits for the outcome.
fn run_and_wait(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("f5");
    wait_until(cx, |cx| {
        workspace.read_with(cx, |w, _| !w.active_tab().is_running())
    });
}

/// The diagnostics shown for the active circuit as `(severity, source,
/// message)`.
fn shown_problems(
    workspace: &Entity<Workspace>,
    cx: &VisualTestContext,
) -> Vec<(Severity, Source, String)> {
    workspace.read_with(cx, |w, _| {
        w.diagnostics
            .visible(w.active_tab().id)
            .into_iter()
            .map(|d| (d.severity, d.source, d.message.clone()))
            .collect()
    })
}

/// Q07: create, simulate, plot V(out), save, reopen with the traces shown
/// immediately.
#[gpui_kit::test]
fn rc_workflow_runs_saves_and_reopens_with_results(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let drawn = env.write_project("drawn.xsim_proj", &rc_project());
    let (workspace, mut window) = open(cx, &env, None);
    window.dispatch_action(NewProject);
    env.prompts.open.borrow_mut().push_back(drawn.clone());
    window.dispatch_action(OpenProject);
    window.run_until_parked();

    run_and_wait(&workspace, &mut window);
    workspace.read_with(&window, |w, cx| {
        let doc = &w.doc.project.circuits[0];
        let stored = doc
            .result
            .as_ref()
            .expect("the result is stored in the project");
        let out = stored.result.signal("V(out)").expect("V(out) recorded");
        let last = *out.real().expect("real").last().expect("samples");
        assert!(
            (last - (1.0 - (-5.0f64).exp())).abs() < 1e-3,
            "V(out) = {last}"
        );
        assert_eq!(doc.plot.panes[0].traces[0].expression, "V(out)");
        let viewer = w.active_tab().viewer.read(cx);
        assert!(viewer.result().is_some(), "the viewer shows the result");
        assert_eq!(viewer.setup(), &doc.plot);
        assert!(matches!(w.active_tab().status, RunStatus::Finished { .. }));
        assert!(w.doc.dirty, "a new result is an unsaved change");
    });
    assert!(
        window.debug_bounds("waveform-viewer").is_some(),
        "the split opened"
    );

    let saved = env.path("saved.xsim_proj");
    env.prompts.save.borrow_mut().push_back(env.path("saved"));
    window.dispatch_action(SaveProjectAs);
    workspace.read_with(&window, |w, _| {
        assert!(!w.doc.dirty);
        assert_eq!(w.doc.path.as_deref(), Some(saved.as_path()));
        assert_eq!(w.doc.project.name, "saved");
        assert_eq!(w.recent.first(), Some(&saved));
    });
    close(&mut window);

    let (reopened, mut window) = open(cx, &env, Some(&saved));
    reopened.read_with(&window, |w, cx| {
        let tab = w.active_tab();
        assert!(!tab.is_running(), "nothing re-runs on open");
        assert!(tab.has_result());
        assert!(!tab.is_stale());
        assert!(
            !w.doc.dirty,
            "opening a project with results is not an edit"
        );
        let viewer = tab.viewer.read(cx);
        assert!(viewer.result().is_some(), "traces are shown immediately");
        assert_eq!(viewer.setup().panes[0].traces[0].expression, "V(out)");
    });
    assert!(window.debug_bounds("waveform-viewer").is_some());
    close(&mut window);
}

#[gpui_kit::test]
fn editing_after_a_run_marks_the_results_out_of_date(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let path = env.write_project("rc.xsim_proj", &rc_project());
    let (workspace, mut window) = open(cx, &env, Some(&path));
    run_and_wait(&workspace, &mut window);
    assert!(window.debug_bounds("stale-results").is_none());
    let editor = workspace.read_with(&window, |w, _| w.active_tab().editor.clone());
    editor
        .update(&mut window, |editor, cx| {
            editor.apply_component_edit(2, "R1".into(), Part::Resistor { resistance: 2e3 }, cx)
        })
        .expect("the editor applies the edit");
    window.run_until_parked();
    workspace.read_with(&window, |w, _| {
        assert!(w.active_tab().is_stale());
        assert!(w.active_tab().has_result(), "the old result stays visible");
        assert!(w.doc.dirty);
    });
    assert!(window.debug_bounds("stale-results").is_some());
    close(&mut window);
}

#[gpui_kit::test]
fn each_circuit_tab_keeps_its_own_editor_and_undo_history(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let path = env.write_project("rc.xsim_proj", &rc_project());
    let (workspace, mut window) = open(cx, &env, Some(&path));
    window.dispatch_action(NewCircuit);
    window.run_until_parked();
    let (first, second) = workspace.read_with(&window, |w, _| {
        assert_eq!(w.doc.project.circuits[1].name, "Circuit 2");
        assert_eq!(w.active_index(), 1);
        (w.tabs[0].editor.clone(), w.tabs[1].editor.clone())
    });
    assert_ne!(first.entity_id(), second.entity_id());

    click("circuit-tab-0", &mut window);
    first
        .update(&mut window, |editor, cx| {
            editor.apply_component_edit(2, "R1".into(), Part::Resistor { resistance: 2e3 }, cx)
        })
        .expect("the editor applies the edit");
    window.run_until_parked();
    click("circuit-tab-1", &mut window);
    click("circuit-tab-0", &mut window);
    workspace.read_with(&window, |w, _| {
        assert_eq!(w.active_index(), 0);
        assert_eq!(
            w.tabs[0].editor.entity_id(),
            first.entity_id(),
            "same editor after switching"
        );
    });
    let resistance = |w: &Workspace| {
        w.doc.project.circuits[0].schematic.components[1]
            .part
            .clone()
    };
    assert_eq!(
        workspace.read_with(&window, |w, _| resistance(w)),
        Part::Resistor { resistance: 2e3 }
    );
    assert!(
        first.read_with(&window, |e, _| e.can_undo()),
        "undo survives the switch"
    );
    assert!(!second.read_with(&window, |e, _| e.can_undo()));
    window.simulate_keystrokes(primary!("z"));
    assert_eq!(
        workspace.read_with(&window, |w, _| resistance(w)),
        Part::Resistor { resistance: 1e3 }
    );
    window.simulate_keystrokes(primary!("shift-z"));
    assert_eq!(
        workspace.read_with(&window, |w, _| resistance(w)),
        Part::Resistor { resistance: 2e3 }
    );
    close(&mut window);
}

#[gpui_kit::test]
fn a_circuit_without_ground_lists_a_netlist_error(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let mut project = rc_project();
    project.circuits[0].schematic = rc_schematic(false);
    let path = env.write_project("floating.xsim_proj", &project);
    let (workspace, mut window) = open(cx, &env, Some(&path));
    assert!(window.debug_bounds("diagnostics-dock").is_none());
    run_and_wait(&workspace, &mut window);
    let problems = shown_problems(&workspace, &window);
    assert_eq!(problems.len(), 1, "{problems:?}");
    let (severity, source, message) = &problems[0];
    assert_eq!((*severity, *source), (Severity::Error, Source::Netlist));
    assert!(message.contains("ground"), "{message}");
    workspace.read_with(&window, |w, _| {
        assert!(
            w.doc.project.layout.diagnostics_open,
            "errors open the dock"
        );
        assert_eq!(w.active_tab().status, RunStatus::Failed);
        assert!(!w.active_tab().has_result());
    });
    assert!(window.debug_bounds("diagnostics-dock").is_some());
    close(&mut window);
}

#[gpui_kit::test]
fn stop_cancels_a_long_run_and_reports_it(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let path = env.write_project("long.xsim_proj", &long_run_project());
    let (workspace, mut window) = open(cx, &env, Some(&path));
    window.dispatch_action(RunSimulation);
    assert!(workspace.read_with(&window, |w, _| w.active_tab().is_running()));
    assert!(
        window.debug_bounds("stop-button").is_some(),
        "Stop shows while running"
    );

    window.dispatch_action(TogglePause);
    let paused = workspace.read_with(&window, |w, _| {
        w.active_tab()
            .run
            .as_ref()
            .is_some_and(|r| r.run.is_paused())
    });
    assert!(paused);
    window.dispatch_action(TogglePause);
    window.dispatch_action(StopSimulation);
    wait_until(&mut window, |cx| {
        workspace.read_with(cx, |w, _| !w.active_tab().is_running())
    });
    workspace.read_with(&window, |w, _| {
        assert!(matches!(w.active_tab().status, RunStatus::Cancelled { .. }));
        assert!(w.doc.project.circuits[0].result.is_none());
        assert!(!w.doc.dirty);
    });
    let problems = shown_problems(&workspace, &window);
    assert!(
        problems.iter().any(|(s, src, m)| *s == Severity::Warning
            && *src == Source::Simulation
            && m.contains("cancelled")),
        "{problems:?}"
    );
    close(&mut window);
}

#[gpui_kit::test]
fn projects_from_a_newer_xsim_open_read_only(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let mut project = rc_project();
    project.schema_version = SCHEMA_VERSION + 1;
    let path = env.path("future.xsim_proj");
    fs::write(&path, project.to_json()).expect("write the future project");
    let original = fs::read(&path).expect("read back");

    let (workspace, mut window) = open(cx, &env, Some(&path));
    workspace.read_with(&window, |w, cx| {
        assert!(w.doc.read_only);
        assert!(w.active_tab().editor.read(cx).is_read_only());
        assert!(w.doc.project.layout.diagnostics_open);
    });
    let problems = shown_problems(&workspace, &window);
    assert!(problems.iter().any(|(s, src, m)| *s == Severity::Warning
        && *src == Source::File
        && m.contains("some features may not work")));

    window.dispatch_action(RunSimulation);
    window.dispatch_action(NewCircuit);
    window.dispatch_action(SaveProject);
    workspace.read_with(&window, |w, _| {
        assert!(
            !w.active_tab().is_running(),
            "read-only projects do not run"
        );
        assert_eq!(w.tabs.len(), 1);
        assert!(!w.doc.dirty);
    });
    assert_eq!(
        env.prompts.asked.borrow().as_slice(),
        ["save"],
        "Save offers Save as"
    );
    assert_eq!(fs::read(&path).expect("unchanged"), original);
    close(&mut window);
}

#[gpui_kit::test]
fn migrated_projects_explain_the_upgrade(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let path = env.path("old.xsim_proj");
    fs::write(
        &path,
        include_str!("../../../project/tests/fixtures/schema1_rc_lowpass.xsim_proj"),
    )
    .expect("write the schema-1 project");
    let (workspace, mut window) = open(cx, &env, Some(&path));
    assert!(!workspace.read_with(&window, |w, _| w.doc.read_only));
    let problems = shown_problems(&workspace, &window);
    assert!(problems.iter().any(|(s, src, m)| *s == Severity::Info
        && *src == Source::File
        && m.contains("upgraded from file format 1")));
    close(&mut window);
}

#[gpui_kit::test]
fn unsaved_changes_are_confirmed_before_replacing_the_project(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let (workspace, mut window) = open(cx, &env, None);
    window.dispatch_action(NewCircuit);
    assert!(workspace.read_with(&window, |w, _| w.doc.dirty));
    env.prompts
        .unsaved
        .borrow_mut()
        .push_back(UnsavedChoice::Cancel);
    window.dispatch_action(NewProject);
    assert_eq!(
        workspace.read_with(&window, |w, _| w.tabs.len()),
        2,
        "cancel keeps the work"
    );
    env.prompts
        .unsaved
        .borrow_mut()
        .push_back(UnsavedChoice::Discard);
    window.dispatch_action(NewProject);
    workspace.read_with(&window, |w, _| {
        assert_eq!(w.tabs.len(), 1);
        assert!(!w.doc.dirty);
    });
    assert_eq!(
        env.prompts.asked.borrow().as_slice(),
        ["unsaved", "unsaved"]
    );
    close(&mut window);
}

#[gpui_kit::test]
fn circuits_are_renamed_by_double_click_and_deleted_after_confirmation(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let (workspace, mut window) = open(cx, &env, None);
    window.dispatch_action(NewCircuit);
    window.run_until_parked();

    double_click("circuit-tab-0", &mut window);
    assert!(workspace.read_with(&window, |w, _| w.renaming.is_some()));
    window.simulate_keystrokes(primary!("a"));
    window.simulate_input("Filter");
    window.simulate_keystrokes("enter");
    workspace.read_with(&window, |w, _| {
        assert!(w.renaming.is_none());
        assert_eq!(w.doc.project.circuits[0].name, "Filter");
    });

    env.prompts.delete.borrow_mut().push_back(false);
    workspace.update_in(&mut window, |w, window, cx| w.delete_circuit(0, window, cx));
    // D25: the answer applies once the question's task has run.
    window.run_until_parked();
    assert_eq!(workspace.read_with(&window, |w, _| w.tabs.len()), 2);
    env.prompts.delete.borrow_mut().push_back(true);
    workspace.update_in(&mut window, |w, window, cx| w.delete_circuit(0, window, cx));
    window.run_until_parked();
    workspace.read_with(&window, |w, _| {
        assert_eq!(w.tabs.len(), 1);
        assert_eq!(w.doc.project.circuits[0].name, "Circuit 2");
    });
    close(&mut window);
}

#[gpui_kit::test]
fn edits_are_snapshotted_and_offered_after_a_crash(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let (_, mut window) = open(cx, &env, None);
    window.dispatch_action(NewCircuit);
    window
        .executor()
        .advance_clock(std::time::Duration::from_secs(2));
    window.run_until_parked();
    let recovery = env.data_dir().join("recovery");
    let snapshot = fs::read_dir(&recovery)
        .expect("recovery directory")
        .flatten()
        .map(|entry| entry.path().join("snapshot.json"))
        .find(|path| path.is_file())
        .expect("a snapshot is written after an edit");

    // A crashed instance leaves its directory with an unlocked lock file.
    let crashed = recovery.join("session-crashed");
    fs::create_dir_all(&crashed).expect("crashed session");
    fs::copy(&snapshot, crashed.join("snapshot.json")).expect("copy the snapshot");
    fs::write(crashed.join("lock"), b"").expect("lock file");

    let (restarted, mut second) = open(cx, &env, None);
    assert_eq!(restarted.read_with(&second, |w, _| w.orphan_count()), 1);
    assert!(has_dialog(&second), "recovery is offered");
    click("recovery-restore-0", &mut dialog(&second));
    restarted.read_with(&second, |w, _| {
        assert_eq!(w.tabs.len(), 2, "the recovered project has both circuits");
        assert!(w.doc.dirty, "recovered work is unsaved");
        assert_eq!(w.orphan_count(), 0);
    });
    assert!(!crashed.exists(), "the crashed session is cleaned up");
    close(&mut second);
    close(&mut window);
}

#[gpui_kit::test]
fn settings_apply_immediately_and_persist(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let (workspace, mut window) = open(cx, &env, None);
    window.dispatch_action(UseDarkTheme);
    assert!(window.update(|_, cx| cx.theme().is_dark()));
    workspace.update_in(&mut window, |w, window, cx| {
        w.set_ui_scale(1.25, window, cx)
    });
    let stored = env.app_data().settings();
    assert_eq!(stored.theme, ThemePreference::Dark);
    assert_eq!(stored.ui_scale, 1.25);
    close(&mut window);

    let (reopened, mut window) = open(cx, &env, None);
    assert_eq!(
        reopened.read_with(&window, |w, _| w.settings().ui_scale),
        1.25
    );
    window.dispatch_action(UseLightTheme);
    assert!(!window.update(|_, cx| cx.theme().is_dark()));
    close(&mut window);
}

#[gpui_kit::test]
fn the_analysis_dialog_opens_and_applies(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let path = env.write_project("rc.xsim_proj", &rc_project());
    let (workspace, mut window) = open(cx, &env, Some(&path));
    window.dispatch_action(OpenAnalysisSettings);
    window.run_until_parked();
    assert!(has_dialog(&window));
    click("analysis-apply", &mut dialog(&window));
    assert!(!has_dialog(&window));
    assert!(
        !workspace.read_with(&window, |w, _| w.doc.dirty),
        "nothing changed"
    );
    close(&mut window);
}

/// D14: the analysis is chosen with the source-type style buttons in the
/// real dialog, and Apply stores the choice.
#[gpui_kit::test]
fn the_analysis_dialog_switches_analysis_with_its_buttons(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let path = env.write_project("rc.xsim_proj", &rc_project());
    let (workspace, mut window) = open(cx, &env, Some(&path));
    window.dispatch_action(OpenAnalysisSettings);
    window.run_until_parked();
    let mut settings = dialog(&window);
    click("analysis-kind-ac", &mut settings);
    click("analysis-apply", &mut settings);
    workspace.read_with(&window, |w, _| {
        assert_eq!(
            w.doc.project.circuits[0].simulation.selected,
            AnalysisChoice::Ac
        );
        assert!(w.doc.dirty);
    });
    close(&mut window);
}

#[gpui_kit::test]
fn missing_files_are_reported_without_closing_the_project(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let (workspace, mut window) = open(cx, &env, Some(&env.path("missing.xsim_proj")));
    let problems = shown_problems(&workspace, &window);
    assert_eq!(problems[0].0, Severity::Error);
    assert!(problems[0].2.contains("Could not open"));
    assert_eq!(workspace.read_with(&window, |w, _| w.tabs.len()), 1);
    close(&mut window);
}

#[gpui_kit::test]
fn schematic_probes_add_traces_to_the_plot(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let path = env.write_project("rc.xsim_proj", &rc_project());
    let (workspace, mut window) = open(cx, &env, Some(&path));
    run_and_wait(&workspace, &mut window);
    let (editor, viewer) = workspace.read_with(&window, |w, _| {
        (w.active_tab().editor.clone(), w.active_tab().viewer.clone())
    });
    editor.update(&mut window, |_, cx| {
        cx.emit(EditorEvent::Probe(ProbeRequest::Differential {
            plus: "N001".into(),
            minus: "out".into(),
        }))
    });
    window.run_until_parked();
    let plotted: Vec<String> = viewer.read_with(&window, |v, _| {
        v.setup().panes[0]
            .traces
            .iter()
            .map(|t| t.expression.clone())
            .collect()
    });
    assert!(plotted.contains(&"V(N001,out)".to_string()), "{plotted:?}");
    workspace.read_with(&window, |w, cx| {
        assert_eq!(&w.doc.project.circuits[0].plot, viewer.read(cx).setup());
        assert!(w.doc.dirty);
    });
    close(&mut window);
}

#[gpui_kit::test]
fn plots_export_to_the_chosen_file(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let path = env.write_project("rc.xsim_proj", &rc_project());
    let (workspace, mut window) = open(cx, &env, Some(&path));
    window.dispatch_action(ExportCsv);
    assert!(
        env.prompts.asked.borrow().is_empty(),
        "nothing to export before a run"
    );
    run_and_wait(&workspace, &mut window);
    env.prompts.export.borrow_mut().push_back(env.path("plot"));
    window.dispatch_action(ExportCsv);
    assert_eq!(env.prompts.asked.borrow().as_slice(), ["export"]);
    assert!(env.path("plot.csv").is_file(), "the extension is added");
    close(&mut window);
}

#[gpui_kit::test]
fn the_diagnostics_dock_toggles_from_the_keyboard(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let (workspace, mut window) = open(cx, &env, None);
    assert!(window.debug_bounds("diagnostics-dock").is_none());
    window.simulate_keystrokes(primary!("j"));
    assert!(window.debug_bounds("diagnostics-dock").is_some());
    assert!(
        !workspace.read_with(&window, |w, _| w.doc.dirty),
        "layout changes are not edits"
    );
    window.simulate_keystrokes(primary!("j"));
    assert!(window.debug_bounds("diagnostics-dock").is_none());
    close(&mut window);
}

/// Q07 with the bundled example: the RC output reaches 1 − e⁻⁵ after 5 ms.
#[gpui_kit::test]
fn the_bundled_rc_example_simulates(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let example = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/RC low-pass.xsim_proj"
    );
    let path = env.path("RC low-pass.xsim_proj");
    fs::copy(example, &path).expect("copy the example");
    let (workspace, mut window) = open(cx, &env, Some(&path));
    run_and_wait(&workspace, &mut window);
    workspace.read_with(&window, |w, _| {
        let problems: Vec<_> = w.diagnostics.visible(w.active_tab().id);
        assert!(problems.is_empty(), "{problems:?}");
        let stored = w.doc.project.circuits[0].result.as_ref().expect("result");
        let out = stored.result.signal("V(out)").expect("V(out)");
        let last = *out.real().expect("real").last().expect("samples");
        assert!((last - 0.993262).abs() < 1e-5, "V(out) = {last}");
    });
    close(&mut window);
}

#[gpui_kit::test]
fn edit_commands_reach_the_schematic_editor(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let path = env.write_project("rc.xsim_proj", &rc_project());
    let (workspace, mut window) = open(cx, &env, Some(&path));
    // As after a menu command: focus on the workspace root, not the editor.
    for keys in [primary!("a"), "delete"] {
        workspace.update_in(&mut window, |w, window, cx| {
            w.focus.focus(window, cx);
            w.forward_to_editor(keys, window, cx);
        });
        window.run_until_parked();
    }
    let empty = |w: &Workspace| w.doc.project.circuits[0].schematic.is_empty();
    assert!(
        workspace.read_with(&window, |w, _| empty(w)),
        "Select all + Delete clear the drawing"
    );
    window.dispatch_action(Undo);
    window.run_until_parked();
    assert!(
        !workspace.read_with(&window, |w, _| empty(w)),
        "undo restores it"
    );
    close(&mut window);
}

#[gpui_kit::test]
fn focus_returns_to_the_schematic_after_a_menu_command(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let (workspace, mut window) = open(cx, &env, None);
    // Focus events are only delivered to an active window.
    window.update(|window, _| window.activate_window());
    workspace.update_in(&mut window, |w, window, cx| w.focus.focus(window, cx));
    window.run_until_parked();
    let editor_focused = workspace.update_in(&mut window, |w, window, cx| {
        w.active_tab()
            .editor
            .focus_handle(cx)
            .contains_focused(window, cx)
    });
    assert!(editor_focused);
    close(&mut window);
}

#[gpui_kit::test]
fn closing_the_window_asks_about_unsaved_changes(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let (_, mut window) = open(cx, &env, None);
    window.dispatch_action(NewCircuit);
    env.prompts
        .unsaved
        .borrow_mut()
        .push_back(UnsavedChoice::Cancel);
    // D25: the window stays open while the question is asked, and closes
    // itself after Don't save.
    assert!(!window.simulate_close(), "the window waits for the answer");
    window.run_until_parked();
    assert!(!window.windows().is_empty(), "cancel keeps the window open");
    env.prompts
        .unsaved
        .borrow_mut()
        .push_back(UnsavedChoice::Discard);
    assert!(!window.simulate_close(), "the window waits for the answer");
    window.run_until_parked();
    assert!(window.windows().is_empty(), "discard closes it");
    let sessions = fs::read_dir(env.data_dir().join("recovery"))
        .map(|dir| dir.count())
        .unwrap_or(0);
    assert_eq!(sessions, 0, "a clean exit leaves no recovery session");
}

#[gpui_kit::test]
fn the_schematic_keeps_focus_after_run_control_clicks(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let path = env.write_project("rc.xsim_proj", &rc_project());
    let (workspace, mut window) = open(cx, &env, Some(&path));
    click("run-button", &mut window);
    wait_until(&mut window, |cx| {
        workspace.read_with(cx, |w, _| !w.active_tab().is_running())
    });
    let editor_focused = workspace.update_in(&mut window, |w, window, cx| {
        w.active_tab()
            .editor
            .focus_handle(cx)
            .contains_focused(window, cx)
    });
    assert!(editor_focused, "single-key editor shortcuts keep working");
    close(&mut window);
}

/// The plot toolbar's Export menu asks for a file like File > Export plot.
#[gpui_kit::test]
fn the_plot_toolbar_exports_through_the_shell(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let path = env.write_project("rc.xsim_proj", &rc_project());
    let (workspace, mut window) = open(cx, &env, Some(&path));
    run_and_wait(&workspace, &mut window);
    env.prompts.export.borrow_mut().push_back(env.path("plot"));
    let viewer = workspace.read_with(&window, |w, _| w.active_tab().viewer.clone());
    viewer.update(&mut window, |_, cx| {
        cx.emit(xsim_plot::ViewerRequest::ExportImage(
            xsim_plot::ImageFormat::Svg,
        ))
    });
    window.run_until_parked();
    assert_eq!(env.prompts.asked.borrow().as_slice(), ["export"]);
    let svg = fs::read_to_string(env.path("plot.svg")).expect("the SVG is written");
    assert!(svg.contains("<svg"), "{svg:.80}");
    close(&mut window);
}

/// A stored AC result whose plot is a single linear pane is shown with the
/// Bode layout; adapting the plot on open is not an edit.
#[gpui_kit::test]
fn opening_an_ac_result_adapts_the_plot_without_an_edit(cx: &mut TestAppContext) {
    use xsim_project::{PlotPane, PlotSetup, StoredResult, Trace};
    init(cx);
    let env = TestEnv::new();
    let mut project = rc_project();
    let doc = &mut project.circuits[0];
    doc.simulation.selected = AnalysisChoice::Ac;
    set_v1(doc, xsim_spice_engine::Source::ac(0.0, 1.0, 0.0));
    let analysis = doc.simulation.analysis();
    let netlist = xsim_project::netlist(&doc.schematic).expect("the RC circuit netlists");
    let result = xsim_spice_engine::simulate(
        &netlist.circuit,
        &analysis,
        &doc.simulation.options,
        &xsim_spice_engine::SimControl::new(),
    )
    .expect("the AC analysis runs");
    doc.result = Some(StoredResult {
        circuit_fingerprint: Some(StoredResult::fingerprint(&netlist.circuit)),
        analysis,
        result,
    });
    doc.plot = PlotSetup {
        panes: vec![PlotPane {
            traces: vec![Trace::new("V(out)")],
            ..PlotPane::default()
        }],
        active_pane: 0,
    };
    let path = env.write_project("ac.xsim_proj", &project);
    let (workspace, mut window) = open(cx, &env, Some(&path));
    window.run_until_parked();
    workspace.read_with(&window, |w, cx| {
        assert!(!w.doc.dirty, "opening a project is not an edit");
        let shown = w.active_tab().viewer.read(cx).setup().clone();
        assert_eq!(shown, w.doc.project.circuits[0].plot);
        assert_eq!(shown.panes.len(), 2, "magnitude and phase panes");
    });
    close(&mut window);
}

/// Real windows are active: the schematic's shortcuts must work there too.
#[gpui_kit::test]
fn schematic_shortcuts_work_in_an_active_window(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let path = env.write_project("rc.xsim_proj", &rc_project());
    let (workspace, mut window) = open(cx, &env, Some(&path));
    window.update(|window, _| window.activate_window());
    window.run_until_parked();
    let stack = window.update(|window, _| format!("{:?}", window.context_stack()));
    window.simulate_keystrokes(primary!("a"));
    window.simulate_keystrokes("delete");
    workspace.read_with(&window, |w, _| {
        let schematic = &w.doc.project.circuits[0].schematic;
        assert!(
            schematic.components.is_empty(),
            "Ctrl+A, Delete left {} parts; context stack {stack}",
            schematic.components.len()
        );
    });
    close(&mut window);
}

/// Replaces the source of V1.
fn set_v1(doc: &mut xsim_project::CircuitDocument, source: xsim_spice_engine::Source) {
    let v1 = doc
        .schematic
        .components
        .iter_mut()
        .find(|c| c.name == "V1")
        .expect("the RC circuit has V1");
    v1.part = Part::VoltageSource { source };
}

/// An AC run of the RC low-pass matches |H| = 1/√(1 + (f/fc)²) and
/// ∠H = −atan(f/fc) at every frequency to six significant digits (Q114).
#[gpui_kit::test]
fn an_ac_run_matches_the_analytic_low_pass(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let mut project = rc_project();
    let doc = &mut project.circuits[0];
    doc.simulation.selected = AnalysisChoice::Ac;
    set_v1(doc, xsim_spice_engine::Source::ac(0.0, 1.0, 0.0));
    let path = env.write_project("ac.xsim_proj", &project);
    let (workspace, mut window) = open(cx, &env, Some(&path));
    run_and_wait(&workspace, &mut window);
    assert!(shown_problems(&workspace, &window).is_empty());
    workspace.read_with(&window, |w, _| {
        let stored = w.doc.project.circuits[0].result.as_ref().expect("a result");
        let frequencies = &stored.result.axis.values;
        let out = stored.result.signal("V(out)").expect("V(out)");
        let xsim_spice_engine::SignalData::Complex { re, im } = &out.data else {
            panic!("AC data is complex");
        };
        let fc = 1.0 / (2.0 * std::f64::consts::PI * 1e3 * 1e-6);
        assert!(frequencies.len() > 10);
        for (k, &f) in frequencies.iter().enumerate() {
            let ratio = f / fc;
            let magnitude = re[k].hypot(im[k]);
            let expected = 1.0 / (1.0 + ratio * ratio).sqrt();
            assert!(
                ((magnitude - expected) / expected).abs() < 1e-6,
                "|H({f})| = {magnitude}, expected {expected}"
            );
            let phase = im[k].atan2(re[k]);
            assert!((phase + ratio.atan()).abs() < 1e-6, "∠H({f}) = {phase}");
        }
    });
    close(&mut window);
}

/// An AC run with no AC amplitude anywhere warns instead of plotting
/// silence.
#[gpui_kit::test]
fn an_ac_run_without_an_ac_source_warns(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let mut project = rc_project();
    project.circuits[0].simulation.selected = AnalysisChoice::Ac;
    let path = env.write_project("ac.xsim_proj", &project);
    let (workspace, mut window) = open(cx, &env, Some(&path));
    run_and_wait(&workspace, &mut window);
    let problems = shown_problems(&workspace, &window);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].0, Severity::Warning);
    assert!(problems[0].2.contains("AC amplitude"), "{}", problems[0].2);
    close(&mut window);
}

/// Whether a dialog window is open over the window.
fn dialog_open(cx: &mut VisualTestContext) -> bool {
    has_dialog(cx)
}

/// D14, in the real dialog: Tab reaches the analysis buttons, and Enter or
/// Space chooses the focused one without closing the dialog or applying.
#[gpui_kit::test]
fn the_analysis_buttons_work_from_the_keyboard_in_the_dialog(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let path = env.write_project("rc.xsim_proj", &rc_project());
    let (workspace, mut window) = open(cx, &env, Some(&path));
    let applied = |window: &VisualTestContext| {
        workspace.read_with(window, |w, _| {
            (w.doc.project.circuits[0].simulation.selected, w.doc.dirty)
        })
    };
    // Tab order: operating point, DC sweep, AC sweep.
    for (key, tabs, expected) in [
        ("enter", 3, AnalysisChoice::Ac),
        ("space", 1, AnalysisChoice::OperatingPoint),
    ] {
        window.dispatch_action(OpenAnalysisSettings);
        window.run_until_parked();
        let mut settings = dialog(&window);
        let before = applied(&window);
        for _ in 0..tabs {
            press("tab", &mut settings);
        }
        press(key, &mut settings);
        assert!(
            dialog_open(&mut window),
            "{key} on a button closed the dialog"
        );
        assert_eq!(applied(&window), before, "{key} applied the dialog");
        click("analysis-apply", &mut settings);
        assert_eq!(applied(&window), (expected, true), "{key} chose a button");
    }
    close(&mut window);
}

/// Enter in one of the dialog's boxes applies the settings like the Apply
/// button; an invalid value keeps the dialog open with its message.
#[gpui_kit::test]
fn enter_in_the_analysis_dialog_applies_it(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let path = env.write_project("rc.xsim_proj", &rc_project());
    let (workspace, mut window) = open(cx, &env, Some(&path));
    let stop = |window: &VisualTestContext| {
        workspace.read_with(window, |w, _| {
            w.doc.project.circuits[0].simulation.transient.stop
        })
    };
    window.dispatch_action(OpenAnalysisSettings);
    window.run_until_parked();
    let mut settings = dialog(&window);
    // The four analysis buttons, then the transient's stop time.
    for _ in 0..5 {
        press("tab", &mut settings);
    }
    settings.simulate_keystrokes(primary!("a"));
    settings.simulate_input("soon");
    press_in_dialog("enter", &mut settings);
    assert!(dialog_open(&mut window), "an invalid value is not applied");
    assert_eq!(stop(&window), 5e-3);

    settings.simulate_keystrokes(primary!("a"));
    settings.simulate_input("7m");
    press_in_dialog("enter", &mut settings);
    assert!(!dialog_open(&mut window), "Enter applies and closes");
    assert!((stop(&window) - 7e-3).abs() < 1e-12, "{}", stop(&window));
    assert!(workspace.read_with(&window, |w, _| w.doc.dirty));
    close(&mut window);
}

/// Enter that no button takes applies the dialog, so a choice made with
/// the mouse is kept. On a focused footer button it does what that button
/// does. The window's close button (D22) is no tab stop; a click on it
/// closes without applying.
#[gpui_kit::test]
fn enter_applies_the_analysis_dialog_except_on_close_and_cancel(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let path = env.write_project("rc.xsim_proj", &rc_project());
    let (workspace, mut window) = open(cx, &env, Some(&path));
    let applied = |window: &VisualTestContext| {
        workspace.read_with(window, |w, _| {
            (w.doc.project.circuits[0].simulation.selected, w.doc.dirty)
        })
    };
    let unchanged = (AnalysisChoice::Transient, false);
    window.dispatch_action(OpenAnalysisSettings);
    window.run_until_parked();
    let mut settings = dialog(&window);
    click("analysis-kind-ac", &mut settings);
    click("dialog-close", &mut settings);
    assert!(!dialog_open(&mut window), "the close button closes");
    assert_eq!(applied(&window), unchanged, "the close button");
    // Backwards from the form: Apply, Cancel.
    for (choose, back, expected) in [
        ("analysis-kind-ac", 2, unchanged),
        ("analysis-kind-ac", 1, (AnalysisChoice::Ac, true)),
        (
            "analysis-kind-op",
            0,
            (AnalysisChoice::OperatingPoint, true),
        ),
    ] {
        window.dispatch_action(OpenAnalysisSettings);
        window.run_until_parked();
        let mut settings = dialog(&window);
        click(choose, &mut settings);
        for _ in 0..back {
            press("shift-tab", &mut settings);
        }
        press_in_dialog("enter", &mut settings);
        assert!(!dialog_open(&mut window), "{back} back: Enter closes");
        assert_eq!(applied(&window), expected, "{back} back");
        keyboard_back_after_enter(&mut window);
    }
    close(&mut window);
}

/// Lets time pass after Enter closed a dialog, as a user would before the
/// next key. (The kit's in-window dialogs handed the keyboard back after a
/// fixed 250 ms; a dialog window gives it back at once, D22.)
fn keyboard_back_after_enter(window: &mut VisualTestContext) {
    window
        .executor()
        .advance_clock(std::time::Duration::from_millis(300));
    window.run_until_parked();
}

/// Changes R1 of the active circuit to `resistance` through its editor, as
/// the properties dialog does.
fn set_r1(workspace: &Entity<Workspace>, resistance: f64, window: &mut VisualTestContext) {
    let editor = workspace.read_with(window, |w, _| w.active_tab().editor.clone());
    editor
        .update(window, |editor, cx| {
            editor.apply_component_edit(2, "R1".into(), Part::Resistor { resistance }, cx)
        })
        .expect("the editor applies the edit");
    window.run_until_parked();
}

/// The active circuit shows its result as out of date, without probing,
/// the way an edit in the session does.
fn assert_out_of_date(workspace: &Entity<Workspace>, window: &mut VisualTestContext, what: &str) {
    workspace.read_with(window, |w, cx| {
        let tab = w.active_tab();
        assert!(tab.has_result(), "{what}: the old result stays visible");
        assert!(tab.is_stale(), "{what}: the result is out of date");
        assert!(!tab.probing, "{what}: probing and the hover card are off");
        assert!(tab.viewer.read(cx).result().is_some(), "{what}: plotted");
    });
    assert!(
        window.debug_bounds("stale-results").is_some(),
        "{what}: the out-of-date hint shows"
    );
}

/// The active circuit shows its result as current, with probing.
fn assert_current(workspace: &Entity<Workspace>, window: &mut VisualTestContext, what: &str) {
    workspace.read_with(window, |w, _| {
        let tab = w.active_tab();
        assert!(tab.has_result(), "{what}: has a result");
        assert!(!tab.is_stale(), "{what}: the result is current");
        assert!(tab.probing, "{what}: the result can be probed");
    });
    assert!(window.debug_bounds("stale-results").is_none(), "{what}");
}

/// INT-1: a result made out of date by an edit stays out of date after
/// Save and Open, although its node names still match; a new run makes it
/// current again, also after the next reopen.
#[gpui_kit::test]
fn an_out_of_date_result_stays_out_of_date_after_reopening(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let path = env.write_project("rc.xsim_proj", &rc_project());
    let original = xsim_project::netlist(&rc_schematic(true)).expect("netlists");
    let (workspace, mut window) = open(cx, &env, Some(&path));
    run_and_wait(&workspace, &mut window);
    assert_current(&workspace, &mut window, "after the run");
    set_r1(&workspace, 2e3, &mut window);
    assert_out_of_date(&workspace, &mut window, "after the edit");
    let saved = env.path("edited.xsim_proj");
    env.prompts.save.borrow_mut().push_back(saved.clone());
    window.dispatch_action(SaveProjectAs);
    workspace.read_with(&window, |w, _| {
        let stored = w.doc.project.circuits[0].result.as_ref().expect("stored");
        assert_eq!(
            stored.computed_from(&original.circuit),
            Some(true),
            "the fingerprint is the simulated circuit's"
        );
    });
    close(&mut window);

    let (reopened, mut window) = open(cx, &env, Some(&saved));
    assert_out_of_date(&reopened, &mut window, "after reopening");
    assert!(!reopened.read_with(&window, |w, _| w.doc.dirty));
    run_and_wait(&reopened, &mut window);
    assert_current(&reopened, &mut window, "after the new run");
    window.dispatch_action(SaveProject);
    close(&mut window);
    let (again, mut window) = open(cx, &env, Some(&saved));
    assert_current(&again, &mut window, "after saving the new run");
    close(&mut window);
}

/// Simulates the first circuit of `project` and stores the result in it,
/// with or without the circuit fingerprint (files written before it
/// existed have none).
fn store_operating_point(project: &mut xsim_project::Project, fingerprint: bool) {
    use xsim_project::StoredResult;
    let doc = &mut project.circuits[0];
    doc.simulation.selected = AnalysisChoice::OperatingPoint;
    let analysis = doc.simulation.analysis();
    let netlist = xsim_project::netlist(&doc.schematic).expect("the RC circuit netlists");
    let result = xsim_spice_engine::simulate(
        &netlist.circuit,
        &analysis,
        &doc.simulation.options,
        &xsim_spice_engine::SimControl::new(),
    )
    .expect("the operating point solves");
    doc.result = Some(StoredResult {
        analysis,
        circuit_fingerprint: fingerprint.then(|| StoredResult::fingerprint(&netlist.circuit)),
        result,
    });
}

/// Sets R1 of the first circuit of `project`.
fn with_r1(mut project: xsim_project::Project, resistance: f64) -> xsim_project::Project {
    let r1 = project.circuits[0]
        .schematic
        .components
        .iter_mut()
        .find(|c| c.name == "R1")
        .expect("R1");
    r1.part = Part::Resistor { resistance };
    project
}

/// INT-1, files written before the fingerprint: their result is current
/// while it records exactly the voltages of the schematic's nodes (a value
/// edit cannot be told apart then), and out of date otherwise.
#[gpui_kit::test]
fn results_without_a_fingerprint_are_judged_by_their_nodes(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let mut legacy = rc_project();
    store_operating_point(&mut legacy, false);
    let edited = with_r1(legacy.clone(), 2e3);
    let mut renamed = legacy.clone();
    renamed.circuits[0].schematic.labels[0].name = "vout".into();
    for (name, project, current) in [
        ("legacy", &legacy, true),
        ("legacy-value-edit", &edited, true),
        ("legacy-renamed-node", &renamed, false),
    ] {
        let path = env.write_project(&format!("{name}.xsim_proj"), project);
        let text = fs::read_to_string(&path).expect("saved");
        assert!(!text.contains("circuit_fingerprint"), "{name}");
        let (workspace, mut window) = open(cx, &env, Some(&path));
        if current {
            assert_current(&workspace, &mut window, name);
        } else {
            assert_out_of_date(&workspace, &mut window, name);
        }
        close(&mut window);
    }
}

/// INT-1 with a fingerprint: a value changed outside xsim (a merge, a
/// checkout) is caught too.
#[gpui_kit::test]
fn a_fingerprinted_result_of_another_schematic_is_out_of_date(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let mut project = rc_project();
    store_operating_point(&mut project, true);
    let path = env.write_project("same.xsim_proj", &project);
    let (workspace, mut window) = open(cx, &env, Some(&path));
    assert_current(&workspace, &mut window, "the same schematic");
    close(&mut window);

    let path = env.write_project("changed.xsim_proj", &with_r1(project, 2e3));
    let (workspace, mut window) = open(cx, &env, Some(&path));
    assert_out_of_date(&workspace, &mut window, "a changed value");
    close(&mut window);
}

/// INT-1 with Q45: a project from a newer xsim opens read-only. Its result
/// shows as out of date when its fingerprint belongs to another schematic or
/// is one this version cannot read (the file still opens), and the hint does
/// not ask for F5, which a read-only project cannot run.
#[gpui_kit::test]
fn a_newer_project_shows_an_out_of_date_result_read_only(cx: &mut TestAppContext) {
    use super::run_controls::stale_hint_text;
    init(cx);
    let env = TestEnv::new();
    let mut project = rc_project();
    store_operating_point(&mut project, true);
    let edited = with_r1(project.clone(), 2e3);
    for (name, fingerprint) in [
        ("other-schematic", None),
        ("unreadable-fingerprint", Some("sha3:0123abcd")),
    ] {
        let mut future = edited.clone();
        if let Some(fingerprint) = fingerprint {
            future = project.clone();
            let stored = future.circuits[0].result.as_mut().expect("stored");
            stored.circuit_fingerprint = Some(fingerprint.into());
        }
        future.schema_version = SCHEMA_VERSION + 1;
        let path = env.path(&format!("{name}.xsim_proj"));
        fs::write(&path, future.to_json()).expect("write the future project");
        let (workspace, mut window) = open(cx, &env, Some(&path));
        assert!(
            workspace.read_with(&window, |w, _| w.doc.read_only),
            "{name}: read-only"
        );
        assert_out_of_date(&workspace, &mut window, name);
        close(&mut window);
    }
    assert_eq!(
        stale_hint_text(false, false),
        "Results out of date - press F5"
    );
    assert!(!stale_hint_text(true, false).contains("F5"));
}

/// Lets `window`'s session write its recovery snapshot and copies it into
/// a crashed session (a directory with an unlocked lock file), which the
/// next instance offers to restore. Returns that directory.
pub(super) fn leave_crashed_session(
    env: &TestEnv,
    window: &mut VisualTestContext,
) -> std::path::PathBuf {
    window
        .executor()
        .advance_clock(std::time::Duration::from_secs(2));
    window.run_until_parked();
    let recovery = env.data_dir().join("recovery");
    let snapshot = fs::read_dir(&recovery)
        .expect("recovery directory")
        .flatten()
        .map(|entry| entry.path().join("snapshot.json"))
        .find(|path| path.is_file())
        .expect("a snapshot is written after an edit");
    let crashed = recovery.join("session-crashed");
    fs::create_dir_all(&crashed).expect("crashed session");
    fs::copy(&snapshot, crashed.join("snapshot.json")).expect("copy the snapshot");
    fs::write(crashed.join("lock"), b"").expect("lock file");
    crashed
}

/// Leaves `window`'s work as a crashed session, starts a second instance
/// and restores the work there.
fn crash_and_restore(
    env: &TestEnv,
    cx: &mut TestAppContext,
    window: &mut VisualTestContext,
) -> (Entity<Workspace>, VisualTestContext) {
    leave_crashed_session(env, window);
    let (restarted, second) = open(cx, env, None);
    click("recovery-restore-0", &mut dialog(&second));
    assert_eq!(restarted.read_with(&second, |w, _| w.orphan_count()), 0);
    (restarted, second)
}

/// INT-1 after a crash: the recovered result is out of date when the
/// schematic was edited after the run, and current when it was not.
#[gpui_kit::test]
fn recovered_results_keep_their_staleness(cx: &mut TestAppContext) {
    init(cx);
    for edit in [true, false] {
        let env = TestEnv::new();
        let path = env.write_project("rc.xsim_proj", &rc_project());
        let (workspace, mut window) = open(cx, &env, Some(&path));
        run_and_wait(&workspace, &mut window);
        if edit {
            set_r1(&workspace, 2e3, &mut window);
        }
        let (restored, mut second) = crash_and_restore(&env, cx, &mut window);
        if edit {
            assert_out_of_date(&restored, &mut second, "recovered after an edit");
        } else {
            assert_current(&restored, &mut second, "recovered without an edit");
        }
        close(&mut second);
        close(&mut window);
    }
}

/// INT-8 (D7): a crashed run still says how far it got.
#[gpui_kit::test]
fn a_crashed_run_reports_its_progress(cx: &mut TestAppContext) {
    use crate::simulation::runner::RunFailure;
    init(cx);
    let env = TestEnv::new();
    let (workspace, mut window) = open(cx, &env, None);
    workspace.update(&mut window, |w, cx| {
        let index = w.active_index();
        let failure = RunFailure::Crashed("index out of bounds".into());
        w.report_failure(index, &failure, 0.41, cx);
    });
    let problems = shown_problems(&workspace, &window);
    assert_eq!(problems.len(), 1, "{problems:?}");
    let (severity, source, message) = &problems[0];
    assert_eq!((*severity, *source), (Severity::Error, Source::Simulation));
    assert!(message.contains("(41.0% complete)"), "{message}");
    assert!(message.contains("index out of bounds"), "{message}");
    workspace.read_with(&window, |w, _| {
        assert!(
            w.doc.project.layout.diagnostics_open,
            "errors open the dock"
        );
        assert!(matches!(w.active_tab().status, RunStatus::Failed));
    });
    close(&mut window);
}

/// D15: the warnings a run raised are listed for its circuit, without
/// opening the dock; the status bar counts them.
#[gpui_kit::test]
fn run_warnings_are_listed_without_opening_the_dock(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let (workspace, mut window) = open(cx, &env, None);
    window.dispatch_action(NewCircuit);
    window.run_until_parked();
    let warning = "Node mid has no DC path to ground; it was tied to ground through gmin \
                   for the operating point.";
    workspace.update(&mut window, |w, cx| {
        let index = w.active_index();
        w.report_warnings(index, vec![warning.to_string()]);
        cx.notify();
    });
    window.run_until_parked();
    let problems = shown_problems(&workspace, &window);
    assert_eq!(
        problems,
        [(Severity::Warning, Source::Simulation, warning.to_string())]
    );
    workspace.read_with(&window, |w, _| {
        assert!(
            !w.doc.project.layout.diagnostics_open,
            "warnings do not open the dock"
        );
        let counts = w.diagnostics.counts(w.active_tab().id);
        assert_eq!((counts.errors, counts.warnings), (0, 1), "status bar count");
    });
    // The warning belongs to its circuit.
    click("circuit-tab-0", &mut window);
    assert!(shown_problems(&workspace, &window).is_empty());
    close(&mut window);
}

/// D15 end to end: the middle of two series capacitors is tied to ground
/// for the operating point, the run succeeds, and the engine's warning
/// naming the node is listed without opening the dock.
#[gpui_kit::test]
fn series_capacitors_run_with_a_warning(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    for analysis in [AnalysisChoice::OperatingPoint, AnalysisChoice::Transient] {
        let mut project = series_capacitor_project();
        project.circuits[0].simulation.selected = analysis;
        project.circuits[0].simulation.transient.initial_state =
            xsim_spice_engine::InitialState::OperatingPoint;
        let path = env.write_project("series.xsim_proj", &project);
        let (workspace, mut window) = open(cx, &env, Some(&path));
        run_and_wait(&workspace, &mut window);
        workspace.read_with(&window, |w, _| {
            assert!(
                matches!(w.active_tab().status, RunStatus::Finished { .. }),
                "{analysis:?}: {:?}",
                w.active_tab().status
            );
            assert!(!w.doc.project.layout.diagnostics_open, "{analysis:?}");
            let counts = w.diagnostics.counts(w.active_tab().id);
            assert_eq!(counts.errors, 0, "{analysis:?}");
            assert!(counts.warnings >= 1, "{analysis:?}: the status bar count");
        });
        let problems = shown_problems(&workspace, &window);
        assert!(
            problems.iter().all(|(severity, source, _)| {
                (*severity, *source) == (Severity::Warning, Source::Simulation)
            }),
            "{analysis:?}: {problems:?}"
        );
        assert!(
            problems
                .iter()
                .any(|(_, _, message)| message.contains("mid") && message.contains("gmin")),
            "{analysis:?}: {problems:?}"
        );
        // A new run replaces the warnings of the previous one.
        run_and_wait(&workspace, &mut window);
        assert_eq!(
            shown_problems(&workspace, &window),
            problems,
            "{analysis:?}"
        );
        close(&mut window);
    }
}

/// D15: a run that ends without a result still lists the warnings it
/// raised before it stopped, next to its own failure.
#[gpui_kit::test]
fn a_stopped_run_still_lists_its_warnings(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let mut project = series_capacitor_project();
    let long = long_run_project().circuits[0].simulation.transient.clone();
    let simulation = &mut project.circuits[0].simulation;
    simulation.selected = AnalysisChoice::Transient;
    simulation.transient = xsim_spice_engine::Transient {
        initial_state: xsim_spice_engine::InitialState::OperatingPoint,
        ..long
    };
    let path = env.write_project("series-long.xsim_proj", &project);
    let (workspace, mut window) = open(cx, &env, Some(&path));
    window.dispatch_action(RunSimulation);
    // Past the operating point, into the transient.
    wait_until(&mut window, |cx| {
        workspace.read_with(cx, |w, _| {
            w.active_tab()
                .run
                .as_ref()
                .is_some_and(|active| active.run.progress() > 0.0)
        })
    });
    window.dispatch_action(StopSimulation);
    wait_until(&mut window, |cx| {
        workspace.read_with(cx, |w, _| !w.active_tab().is_running())
    });
    let problems = shown_problems(&workspace, &window);
    assert!(
        problems.iter().all(|(severity, source, _)| {
            *severity == Severity::Warning && *source == Source::Simulation
        }),
        "{problems:?}"
    );
    assert!(
        problems.iter().any(|(_, _, m)| m.contains("cancelled")),
        "{problems:?}"
    );
    assert!(
        problems.iter().any(|(_, _, m)| m.contains("mid")),
        "{problems:?}"
    );
    close(&mut window);
}

/// D14: the theme is chosen with the analysis dialog's joined row of small
/// outline buttons.
#[gpui_kit::test]
fn the_theme_buttons_match_the_analysis_buttons(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let (_, mut window) = open(cx, &env, None);
    window.dispatch_action(OpenSettings);
    window.run_until_parked();
    let mut settings = dialog(&window);
    let rem = settings.update(|window, _| window.rem_size());
    let light = settings
        .debug_bounds("theme-light")
        .expect("Light is shown");
    let dark = settings.debug_bounds("theme-dark").expect("Dark is shown");
    assert_eq!(light.top(), dark.top(), "one row");
    assert_eq!(light.right(), dark.left(), "joined, no gap");
    for button in [light, dark] {
        assert_eq!(button.size.height, rem * 1.5, "a small button");
    }
    close(&mut window);
}

/// Clicking Dark or Light switches the theme at once, remembers it and
/// keeps the dialog open.
#[gpui_kit::test]
fn clicking_a_theme_button_switches_the_theme(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let (workspace, mut window) = open(cx, &env, None);
    window.dispatch_action(OpenSettings);
    window.run_until_parked();
    let mut settings = dialog(&window);
    for (button, expected) in [
        ("theme-dark", ThemePreference::Dark),
        ("theme-light", ThemePreference::Light),
        ("theme-dark", ThemePreference::Dark),
    ] {
        click(button, &mut settings);
        assert_eq!(
            workspace.read_with(&window, |w, _| w.settings().theme),
            expected,
            "{button}"
        );
        assert_eq!(
            window.update(|_, cx| cx.theme().is_dark()),
            expected == ThemePreference::Dark,
            "{button}"
        );
        assert!(dialog_open(&mut window), "{button} closed the dialog");
    }
    assert_eq!(env.app_data().settings().theme, ThemePreference::Dark);
    close(&mut window);
}

/// What is painted for the element at `selector`: the background and
/// border colour of every quad with exactly its bounds.
fn painted_look(
    selector: &'static str,
    window: &mut VisualTestContext,
) -> Vec<(gpui_kit::Background, gpui_kit::Hsla)> {
    let bounds = window
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is not shown"));
    let (scale, quads) = window.update(|window, _| (window.scale_factor(), window.painted_quads()));
    let bounds = bounds.scale(scale);
    let look: Vec<_> = quads
        .iter()
        .filter(|quad| quad.bounds == bounds)
        .map(|quad| (quad.background, quad.border_color))
        .collect();
    assert!(!look.is_empty(), "nothing painted at {selector}");
    look
}

/// The chosen theme's button looks selected and the other does not, also
/// after switching back and forth, in both themes. The dialog's first frame
/// is final (D19), so its colours are read without waiting.
#[gpui_kit::test]
fn the_chosen_theme_button_looks_selected(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let (_, mut window) = open(cx, &env, None);
    window.dispatch_action(OpenSettings);
    window.run_until_parked();
    let mut settings = dialog(&window);
    let mut looks = Vec::new();
    for (chosen, other) in [
        ("theme-dark", "theme-light"),
        ("theme-light", "theme-dark"),
        ("theme-dark", "theme-light"),
    ] {
        // No forced repaint: the click itself must repaint the buttons.
        click(chosen, &mut settings);
        let (selected, unselected) = (
            painted_look(chosen, &mut settings),
            painted_look(other, &mut settings),
        );
        assert_ne!(selected, unselected, "{chosen} looks like {other}");
        looks.push((selected, unselected));
    }
    assert_eq!(looks[0], looks[2], "the same look after switching back");
    assert_ne!(looks[0], looks[1], "each theme paints its own colours");
    close(&mut window);
}

/// Tab reaches the theme buttons; Enter or Space chooses the focused one
/// and keeps the dialog open. Enter on Done closes it, and Enter that no
/// button takes closes it like Done. The window's close button (D22) is no
/// tab stop: Tab goes from Done round to Light.
#[gpui_kit::test]
fn the_settings_dialog_works_from_the_keyboard(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let (workspace, mut window) = open(cx, &env, None);
    let theme = |window: &VisualTestContext| workspace.read_with(window, |w, _| w.settings().theme);
    window.dispatch_action(OpenSettings);
    window.run_until_parked();
    let mut settings = dialog(&window);
    // Tab order: Light, Dark, interface scale, Done.
    press("tab", &mut settings);
    press("tab", &mut settings);
    press("enter", &mut settings);
    assert_eq!(theme(&window), ThemePreference::Dark, "Enter chose Dark");
    assert!(window.update(|_, cx| cx.theme().is_dark()));
    assert!(
        dialog_open(&mut window),
        "Enter on a theme closed the dialog"
    );
    press("shift-tab", &mut settings);
    press("space", &mut settings);
    assert_eq!(theme(&window), ThemePreference::Light, "Space chose Light");
    assert!(dialog_open(&mut window));
    press("enter", &mut settings);
    assert_eq!(theme(&window), ThemePreference::Light);
    assert!(
        dialog_open(&mut window),
        "Enter on the chosen theme keeps it"
    );
    for _ in 0..3 {
        press("tab", &mut settings);
    }
    press_in_dialog("enter", &mut settings);
    assert!(!dialog_open(&mut window), "Enter on Done closes");

    for tabs in [0, 4] {
        window.dispatch_action(OpenSettings);
        window.run_until_parked();
        let mut settings = dialog(&window);
        for _ in 0..tabs {
            press("tab", &mut settings);
        }
        press_in_dialog("enter", &mut settings);
        assert!(!dialog_open(&mut window), "{tabs} tabs: Enter closes");
        assert_eq!(theme(&window), ThemePreference::Light, "{tabs} tabs");
    }

    // Done, then round to Light, where Enter chooses Light again.
    window.dispatch_action(OpenSettings);
    window.run_until_parked();
    let mut settings = dialog(&window);
    for _ in 0..5 {
        press("tab", &mut settings);
    }
    press("enter", &mut settings);
    assert!(dialog_open(&mut window), "Enter on Light keeps the dialog");
    press_in_dialog("escape", &mut settings);
    assert!(!dialog_open(&mut window));
    close(&mut window);
}

/// Enter on the interface scale opens its menu instead of closing the
/// dialog, and the arrow keys and Enter choose a scale there.
#[gpui_kit::test]
fn the_interface_scale_menu_opens_from_the_keyboard(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let (workspace, mut window) = open(cx, &env, None);
    let scale =
        |window: &VisualTestContext| workspace.read_with(window, |w, _| w.settings().ui_scale);
    window.dispatch_action(OpenSettings);
    window.run_until_parked();
    let mut settings = dialog(&window);
    for _ in 0..3 {
        press("tab", &mut settings);
    }
    press("enter", &mut settings);
    assert!(
        dialog_open(&mut window),
        "Enter on the scale closed the dialog"
    );
    assert_eq!(scale(&window), 1.0);
    press("down", &mut settings);
    press("enter", &mut settings);
    assert!(
        dialog_open(&mut window),
        "choosing a scale keeps the dialog"
    );
    assert_ne!(scale(&window), 1.0, "a scale was chosen from the menu");
    close(&mut window);
}

/// Enter on the focused Restore, Discard or "Decide later" does what that
/// button does, not the kit's confirmation, which only closed the dialog.
#[gpui_kit::test]
fn enter_presses_the_focused_recovery_button(cx: &mut TestAppContext) {
    init(cx);
    // Tab order: Discard, Restore, Decide later.
    for (tabs, button) in [(2, "Restore"), (1, "Discard"), (3, "Decide later")] {
        let env = TestEnv::new();
        let (_, mut window) = open(cx, &env, None);
        window.dispatch_action(NewCircuit);
        let crashed = leave_crashed_session(&env, &mut window);
        let (restarted, mut second) = open(cx, &env, None);
        assert!(dialog_open(&mut second), "{button}: recovery is offered");
        let mut recovery = dialog(&second);
        for _ in 0..tabs {
            press("tab", &mut recovery);
        }
        press_in_dialog("enter", &mut recovery);
        assert!(!dialog_open(&mut second), "{button}: the dialog closed");
        let (circuits, orphans) =
            restarted.read_with(&second, |w, _| (w.tabs.len(), w.orphan_count()));
        match button {
            "Restore" => {
                assert_eq!((circuits, orphans), (2, 0), "{button}");
                assert!(!crashed.exists(), "{button}");
            }
            "Discard" => {
                assert_eq!((circuits, orphans), (1, 0), "{button}");
                assert!(!crashed.exists(), "{button}");
            }
            _ => {
                assert_eq!((circuits, orphans), (1, 1), "{button}");
                assert!(crashed.exists(), "{button}");
            }
        }
        close(&mut second);
        close(&mut window);
    }
}

/// Escape with the interface-scale menu open closes the menu only; the
/// next Escape closes the dialog. The scale stays as it was.
#[gpui_kit::test]
fn escape_closes_the_scale_menu_before_the_settings(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let (workspace, mut window) = open(cx, &env, None);
    let scale =
        |window: &VisualTestContext| workspace.read_with(window, |w, _| w.settings().ui_scale);
    window.dispatch_action(OpenSettings);
    window.run_until_parked();
    let mut settings = dialog(&window);
    for _ in 0..3 {
        press("tab", &mut settings);
    }
    press("enter", &mut settings);
    press("down", &mut settings);
    press("escape", &mut settings);
    assert!(
        dialog_open(&mut window),
        "Escape in the menu closed the dialog"
    );
    assert_eq!(scale(&window), 1.0, "Escape chose a scale");
    press_in_dialog("escape", &mut settings);
    assert!(
        !dialog_open(&mut window),
        "the second Escape closes the dialog"
    );
    assert_eq!(scale(&window), 1.0);
    close(&mut window);
}

/// With two crashed sessions, Enter on the first Discard discards it and
/// leaves the other one in the dialog, which keeps the keyboard from its
/// top: Tab and Enter there restore it.
#[gpui_kit::test]
fn the_recovery_dialog_works_from_the_keyboard_with_two_snapshots(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let (_, mut window) = open(cx, &env, None);
    window.dispatch_action(NewCircuit);
    let crashed = leave_crashed_session(&env, &mut window);
    let other = crashed.with_file_name("session-crashed-2");
    fs::create_dir_all(&other).expect("second crashed session");
    for file in ["snapshot.json", "lock"] {
        fs::copy(crashed.join(file), other.join(file)).expect("copy the session");
    }
    let (restarted, mut second) = open(cx, &env, None);
    assert_eq!(restarted.read_with(&second, |w, _| w.orphan_count()), 2);
    let mut recovery = dialog(&second);
    // D22: centred with two rows too.
    let owner = second.update(|window, _| window.bounds());
    let (bounds, rem) = recovery.update(|window, _| (window.bounds(), window.rem_size()));
    assert!(
        (bounds.center().y - owner.center().y).abs() <= rem * 0.5,
        "centred: {bounds:?} over {owner:?}"
    );
    // Tab order: Discard, Restore of each snapshot, then Decide later.
    press("tab", &mut recovery);
    press("enter", &mut recovery);
    second.run_until_parked();
    assert!(
        dialog_open(&mut second),
        "the other snapshot is offered again"
    );
    assert_eq!(restarted.read_with(&second, |w, _| w.orphan_count()), 1);
    assert!(recovery.debug_bounds("recovery-restore-0").is_some());
    assert!(
        recovery.debug_bounds("recovery-restore").is_none(),
        "one row"
    );
    press("tab", &mut recovery);
    press("tab", &mut recovery);
    press_in_dialog("enter", &mut recovery);
    assert!(!dialog_open(&mut second), "Restore closed the dialog");
    let (circuits, orphans) = restarted.read_with(&second, |w, _| (w.tabs.len(), w.orphan_count()));
    assert_eq!((circuits, orphans), (2, 0), "restored");
    assert!(!crashed.exists() && !other.exists());
    close(&mut second);
    close(&mut window);
}

/// The sample of `name` in `result` at the first time point at or after
/// `t`.
fn sample_at(result: &xsim_spice_engine::SimulationResult, name: &str, t: f64) -> f64 {
    let values = result
        .signal(name)
        .and_then(|s| s.real())
        .unwrap_or_else(|| panic!("{name} is recorded"));
    let k = result
        .axis
        .values
        .iter()
        .position(|&x| x >= t)
        .unwrap_or_else(|| panic!("the run reaches {t} s"));
    values[k]
}

/// D17: F5 runs a circuit with a voltage- and a current-controlled switch.
/// Each load is near 1 V while its switch is off (the first and third
/// millisecond) and near 1 mV while its control holds it on; probing a
/// switch on the schematic plots its current.
#[gpui_kit::test]
fn both_switches_turn_on_and_off_in_a_run(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let path = env.write_project("switches.xsim_proj", &switch_project());
    let (workspace, mut window) = open(cx, &env, Some(&path));
    run_and_wait(&workspace, &mut window);
    assert!(
        shown_problems(&workspace, &window).is_empty(),
        "{:?}",
        shown_problems(&workspace, &window)
    );
    workspace.read_with(&window, |w, _| {
        assert!(matches!(w.active_tab().status, RunStatus::Finished { .. }));
        let stored = w.doc.project.circuits[0].result.as_ref().expect("a result");
        let result = &stored.result;
        let off = 1e6 / (1e6 + 1e3);
        let on = 1.0 / (1e3 + 1.0);
        for out in ["V(out)", "V(out2)"] {
            for (t, expected) in [(0.5e-3, off), (1.5e-3, on), (2.5e-3, off), (3.5e-3, on)] {
                // On and off differ a thousandfold; this checks the state,
                // not the solver's precision.
                let v = sample_at(result, out, t);
                assert!((v - expected).abs() < 1e-4, "{out} at {t} s: {v}");
            }
        }
        for (current, t, expected) in [
            ("I(S1)", 0.5e-3, 1.0 / (1e6 + 1e3)),
            ("I(S1)", 1.5e-3, on),
            ("I(W1)", 0.5e-3, 1.0 / (1e6 + 1e3)),
            ("I(W1)", 3.5e-3, on),
            ("I(Vsense)", 1.5e-3, 2e-3),
        ] {
            let i = sample_at(result, current, t);
            assert!(
                ((i - expected) / expected).abs() < 1e-3,
                "{current} at {t} s: {i}"
            );
        }
    });

    let (editor, viewer) = workspace.read_with(&window, |w, _| {
        (w.active_tab().editor.clone(), w.active_tab().viewer.clone())
    });
    for signal in ["I(S1)", "I(W1)"] {
        editor.update(&mut window, |_, cx| {
            cx.emit(EditorEvent::Probe(ProbeRequest::Current {
                signal: signal.into(),
            }))
        });
    }
    window.run_until_parked();
    let plotted: Vec<String> = viewer.read_with(&window, |v, _| {
        v.setup()
            .panes
            .iter()
            .flat_map(|pane| pane.traces.iter().map(|t| t.expression.clone()))
            .collect()
    });
    for signal in ["I(S1)", "I(W1)"] {
        assert!(plotted.iter().any(|t| t == signal), "{signal}: {plotted:?}");
    }
    close(&mut window);
}

/// D17: renaming the voltage source that controls a current switch leaves
/// the switch on the old name, and F5 lists a netlist error naming the
/// switch and the missing source.
#[gpui_kit::test]
fn a_renamed_control_source_fails_the_run_naming_it(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let path = env.write_project("switches.xsim_proj", &switch_project());
    let (workspace, mut window) = open(cx, &env, Some(&path));
    let editor = workspace.read_with(&window, |w, _| w.active_tab().editor.clone());
    let (vsense, part) = editor.read_with(&window, |e, _| {
        let c = e
            .schematic()
            .components
            .iter()
            .find(|c| c.name == "Vsense")
            .expect("the sense source");
        (c.id, c.part.clone())
    });
    editor
        .update(&mut window, |e, cx| {
            e.apply_component_edit(vsense, "Vs".into(), part, cx)
        })
        .expect("the source is renamed");
    run_and_wait(&workspace, &mut window);
    let problems = shown_problems(&workspace, &window);
    assert_eq!(problems.len(), 1, "{problems:?}");
    let (severity, source, message) = &problems[0];
    assert_eq!((*severity, *source), (Severity::Error, Source::Netlist));
    assert!(
        message.contains("W1") && message.contains("\"Vsense\""),
        "{message}"
    );
    workspace.read_with(&window, |w, _| {
        assert_eq!(w.active_tab().status, RunStatus::Failed);
    });
    close(&mut window);
}

/// D22: opens the dialog `which` names over the main window.
fn open_workspace_dialog(which: &str, window: &mut VisualTestContext) {
    match which {
        "analysis" => window.dispatch_action(OpenAnalysisSettings),
        _ => window.dispatch_action(OpenSettings),
    }
    window.run_until_parked();
}

/// D22: the analysis settings and the settings open in windows of their
/// own, titled and centred over the main window. The main window ignores a
/// click on Run and the dock's shortcut meanwhile, and a second Settings
/// opens nothing. Escape closes the window without applying anything.
#[gpui_kit::test]
fn workspace_dialogs_open_in_windows_of_their_own(cx: &mut TestAppContext) {
    use gpui_kit::VisualContext as _;
    init(cx);
    let env = TestEnv::new();
    let path = env.write_project("rc.xsim_proj", &rc_project());
    let (workspace, mut window) = open(cx, &env, Some(&path));
    let owner = window.update(|window, _| window.bounds());
    let state = |window: &VisualTestContext| {
        workspace.read_with(window, |w, _| {
            (
                w.doc.dirty,
                w.doc.project.layout.diagnostics_open,
                w.active_tab().is_running(),
                w.settings().theme,
            )
        })
    };
    let before = state(&window);
    for (which, title) in [("analysis", "Analysis settings"), ("settings", "Settings")] {
        open_workspace_dialog(which, &mut window);
        assert_eq!(window.windows().len(), 2, "{which}: a window of its own");
        let mut settings = dialog(&window);
        assert_eq!(settings.window_title().as_deref(), Some(title));
        assert!(
            settings.debug_bounds("dialog-logo").is_some(),
            "{which}: the logo in the title row (D20)"
        );
        assert_eq!(
            settings
                .cx
                .read(|app| xsim_editor::dialog_window::owner_of(settings.window_handle(), app)),
            Some(window.window_handle()),
            "{which}: over the main window"
        );
        let (bounds, rem) = settings.update(|window, _| (window.bounds(), window.rem_size()));
        assert!(
            (bounds.center().x - owner.center().x).abs() < gpui_kit::px(1.),
            "{which}: centred across {bounds:?} over {owner:?}"
        );
        assert!(
            (bounds.center().y - owner.center().y).abs() <= rem * 0.5,
            "{which}: centred down {bounds:?} over {owner:?}"
        );
        assert!(
            bounds.top() >= owner.top() && bounds.bottom() <= owner.bottom(),
            "{which}: over the main window {bounds:?} {owner:?}"
        );

        click("run-button", &mut window);
        window.simulate_keystrokes(primary!("j"));
        window.dispatch_action(OpenSettings);
        window.run_until_parked();
        assert_eq!(state(&window), before, "{which}: the main window reacted");
        assert_eq!(window.windows().len(), 2, "{which}: one dialog at a time");
        assert_eq!(
            window.update(|_, cx| cx.active_window()),
            Some(settings.window_handle()),
            "{which}: the dialog came to the front"
        );

        press_in_dialog("escape", &mut settings);
        assert!(!has_dialog(&window), "{which}: Escape closed it");
        assert_eq!(state(&window), before, "{which}: nothing applied");
    }
    window.simulate_keystrokes(primary!("j"));
    assert!(
        workspace.read_with(&window, |w, _| w.doc.project.layout.diagnostics_open) != before.1,
        "the main window has the keyboard again"
    );
    close(&mut window);
}

/// D22: a change of interface scale in the settings window applies to
/// every window, and the settings window keeps fitting its content at the
/// new scale.
#[gpui_kit::test]
fn the_settings_window_follows_the_interface_scale(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let (workspace, mut window) = open(cx, &env, None);
    open_workspace_dialog("settings", &mut window);
    let mut settings = dialog(&window);
    let width = |cx: &mut VisualTestContext| {
        cx.update(|window, _| (window.bounds().size.width, window.rem_size()))
    };
    let (before, rem) = width(&mut settings);
    assert_eq!(before, rem * 29., "29 rem wide");
    // Light, Dark, the interface scale; its menu; its fifth scale.
    for key in [
        "tab", "tab", "tab", "enter", "down", "down", "down", "down", "down",
    ] {
        press(key, &mut settings);
    }
    press("enter", &mut settings);
    let scale = workspace.read_with(&window, |w, _| w.settings().ui_scale);
    assert_eq!(scale, 1.25, "125% was chosen");
    let (after, rem) = width(&mut settings);
    assert_eq!(
        rem,
        gpui_kit::px(16. * scale),
        "the settings window's scale"
    );
    assert_eq!(after, rem * 29., "still 29 rem wide");
    assert_eq!(
        window.update(|window, _| window.rem_size()),
        rem,
        "the main window's scale"
    );
    let content = settings.debug_bounds("dialog-0").expect("the content");
    let viewport = settings.update(|window, _| window.viewport_size());
    assert!(
        (content.bottom() - viewport.height).abs() < gpui_kit::px(1.),
        "fits its content: {content:?} in {viewport:?}"
    );
    close(&mut window);
}

/// D22: Apply in the analysis settings window stores the settings of the
/// main window's circuit; closing the main window closes the dialog.
#[gpui_kit::test]
fn the_analysis_window_applies_and_closes_with_the_main_window(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let path = env.write_project("rc.xsim_proj", &rc_project());
    let (workspace, mut window) = open(cx, &env, Some(&path));
    open_workspace_dialog("analysis", &mut window);
    let mut settings = dialog(&window);
    click("analysis-kind-dc", &mut settings);
    click("analysis-apply", &mut settings);
    assert!(!has_dialog(&window), "Apply closes");
    assert_eq!(
        workspace.read_with(&window, |w, _| w.doc.project.circuits[0]
            .simulation
            .selected),
        AnalysisChoice::DcSweep
    );
    open_workspace_dialog("analysis", &mut window);
    assert_eq!(window.windows().len(), 2);
    close(&mut window);
    assert!(
        settings.windows().is_empty(),
        "closing the main window closed its dialog"
    );
}

/// D22: the crash recovery opens at start in a window of its own, centred
/// over the new main window, which ignores clicks and keys until the work
/// is restored there. Escape closes the window and keeps the snapshot for
/// later.
#[gpui_kit::test]
fn the_recovery_opens_in_a_window_over_the_new_main_window(cx: &mut TestAppContext) {
    use gpui_kit::VisualContext as _;
    init(cx);
    let env = TestEnv::new();
    let (_, mut window) = open(cx, &env, None);
    window.dispatch_action(NewCircuit);
    let crashed = leave_crashed_session(&env, &mut window);
    let (_, mut later) = open(cx, &env, None);
    press_in_dialog("escape", &mut dialog(&later));
    assert!(!has_dialog(&later), "Escape closed the recovery");
    assert!(crashed.exists(), "and kept the snapshot");
    close(&mut later);

    let (restarted, mut second) = open(cx, &env, None);
    assert_eq!(second.windows().len(), 3, "two main windows and the dialog");
    let mut recovery = dialog(&second);
    assert_eq!(
        recovery.window_title().as_deref(),
        Some("Recover unsaved work")
    );
    let owner = second.update(|window, _| window.bounds());
    let (bounds, rem) = recovery.update(|window, _| (window.bounds(), window.rem_size()));
    assert!(
        (bounds.center().x - owner.center().x).abs() < gpui_kit::px(1.)
            && (bounds.center().y - owner.center().y).abs() <= rem * 0.5,
        "centred: {bounds:?} over {owner:?}"
    );
    assert!(!has_dialog(&window), "the first window is free");
    second.simulate_keystrokes(primary!("t"));
    click("add-circuit", &mut second);
    second.run_until_parked();
    assert_eq!(
        restarted.read_with(&second, |w, _| w.tabs.len()),
        1,
        "the blocked window ignored New circuit"
    );
    click("recovery-restore-0", &mut recovery);
    assert!(!has_dialog(&second));
    assert_eq!(restarted.read_with(&second, |w, _| w.tabs.len()), 2);
    assert_eq!(
        second.update(|_, cx| cx.active_window()),
        Some(second.window_handle()),
        "the restored window is active"
    );
    close(&mut second);
    close(&mut window);
}

/// D22: Escape and the window's close button close the analysis settings
/// without applying what was changed in them: another analysis and a typed
/// stop time.
#[gpui_kit::test]
fn closing_the_analysis_window_applies_nothing(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let path = env.write_project("rc.xsim_proj", &rc_project());
    let (workspace, mut window) = open(cx, &env, Some(&path));
    let stored = |window: &VisualTestContext| {
        workspace.read_with(window, |w, _| {
            (w.doc.project.circuits[0].simulation.clone(), w.doc.dirty)
        })
    };
    let before = stored(&window);
    for how in ["escape", "close button"] {
        window.dispatch_action(OpenAnalysisSettings);
        window.run_until_parked();
        let mut settings = dialog(&window);
        // The four analysis buttons, then the transient's stop time.
        for _ in 0..5 {
            press("tab", &mut settings);
        }
        settings.simulate_keystrokes(primary!("a"));
        settings.simulate_input("7m");
        click("analysis-kind-dc", &mut settings);
        match how {
            "escape" => press_in_dialog("escape", &mut settings),
            _ => click("dialog-close", &mut settings),
        }
        assert!(!has_dialog(&window), "{how} closed the window");
        assert_eq!(stored(&window), before, "{how} applied nothing");
    }
    close(&mut window);
}

/// D22: the operating system asking the main window to close while a
/// dialog is open over it asks about unsaved changes as usual; once the
/// main window closes, the dialog closes with it and nothing is left open,
/// so the application quits (it quits when its last window closes).
#[gpui_kit::test]
fn closing_the_main_window_over_a_dialog_closes_both(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let (_, mut window) = open(cx, &env, None);
    window.dispatch_action(NewCircuit);
    window.dispatch_action(OpenSettings);
    window.run_until_parked();
    let settings = dialog(&window);
    env.prompts
        .unsaved
        .borrow_mut()
        .push_back(UnsavedChoice::Cancel);
    assert!(
        !window.simulate_close(),
        "Cancel keeps the main window open"
    );
    window.run_until_parked();
    assert!(has_dialog(&window), "and its dialog");
    env.prompts
        .unsaved
        .borrow_mut()
        .push_back(UnsavedChoice::Discard);
    // D25: the main window closes itself once Don't save is answered.
    assert!(
        !window.simulate_close(),
        "the main window waits for the answer"
    );
    window.run_until_parked();
    assert!(settings.windows().is_empty(), "the dialog closed with it");
}

/// D22: the menus of a dialog stay inside its window, which is smaller than
/// the main window: the DC sweep's source menu of a schematic with many
/// sources scrolls within the analysis window, so every source can be
/// chosen.
#[gpui_kit::test]
fn a_long_source_menu_fits_the_analysis_window(cx: &mut TestAppContext) {
    use gpui_kit::{Modifiers, ScrollDelta, ScrollEvent, point, px};
    init(cx);
    let env = TestEnv::new();
    let mut project = rc_project();
    let circuit = &mut project.circuits[0];
    for n in 2..=40 {
        let id = circuit.schematic.allocate_id();
        circuit.schematic.components.push(xsim_project::Component {
            id,
            name: format!("V{n}"),
            position: xsim_project::Point::new(40 + 8 * n, 40),
            orientation: Default::default(),
            part: Part::default_for(xsim_project::PartKind::VoltageSource),
        });
    }
    circuit.simulation.selected = AnalysisChoice::DcSweep;
    let path = env.write_project("sources.xsim_proj", &project);
    let (workspace, mut window) = open(cx, &env, Some(&path));
    window.dispatch_action(OpenAnalysisSettings);
    window.run_until_parked();
    let mut settings = dialog(&window);
    click("dc-source", &mut settings);
    let viewport = settings.update(|window, _| window.viewport_size());
    let first = settings
        .debug_bounds("dc-source-V1")
        .expect("the menu is open");
    for _ in 0..20 {
        settings.simulate_event(ScrollEvent {
            position: first.center(),
            delta: ScrollDelta::Pixels(point(px(0.), px(-200.))),
            ..ScrollEvent::default()
        });
    }
    settings.run_until_parked();
    let last = settings
        .debug_bounds("dc-source-V40")
        .expect("the last source is in the menu");
    assert!(
        last.top() >= px(0.) && last.bottom() <= viewport.height,
        "{last:?} in a {viewport:?} window"
    );
    settings.simulate_mouse_move(last.center(), None, Modifiers::default());
    settings.simulate_click(last.center(), Modifiers::default());
    settings.run_until_parked();
    click("analysis-apply", &mut settings);
    assert!(!has_dialog(&window), "applied");
    let source = workspace.read_with(&window, |w, _| {
        w.doc.project.circuits[0].simulation.dc.source.clone()
    });
    assert_eq!(source, "V40");
    close(&mut window);
}
