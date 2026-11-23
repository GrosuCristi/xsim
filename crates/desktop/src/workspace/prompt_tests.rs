//! D25: native questions are answered asynchronously, after their dialog
//! has closed and outside any UI callback.
//!
//! Holding a scripted question open stands in for a native dialog waiting
//! for the user: the event loop keeps running the application's tasks
//! meanwhile. The owner's crash was one of those tasks (the recovery
//! snapshot written 0.8 s after an edit) running inside a synchronous
//! dialog's nested event loop, where it found the App borrowed by the click
//! that opened the dialog, panicked, and so aborted the process.

use super::Workspace;
use crate::actions::*;
use crate::prompts::{Answer, NativePrompts, Prompts, UnsavedChoice};
use crate::test_support::{
    TestEnv, click, close, dialog, has_dialog, init, long_run_project, open, rc_project, wait_until,
};
use gpui_kit::{
    AnyWindowHandle, Entity, Focusable, TestAppContext, VisualContext as _, VisualTestContext,
};
use std::cell::RefCell;
use std::fs;
use std::rc::Rc;
use std::time::Duration;
use xsim_project::{CircuitDocument, Project};

fn names(workspace: &Entity<Workspace>, cx: &VisualTestContext) -> Vec<String> {
    workspace.read_with(cx, |w, _| {
        w.doc
            .project
            .circuits
            .iter()
            .map(|c| c.name.clone())
            .collect()
    })
}

/// Number of circuit tabs and the index of the one shown.
fn tabs(workspace: &Entity<Workspace>, cx: &VisualTestContext) -> (usize, usize) {
    workspace.read_with(cx, |w, _| (w.tabs.len(), w.active_index()))
}

fn asked(env: &TestEnv) -> Vec<&'static str> {
    env.prompts.asked.borrow().clone()
}

fn is_dirty(workspace: &Entity<Workspace>, cx: &VisualTestContext) -> bool {
    workspace.read_with(cx, |w, _| w.doc.dirty)
}

/// Whether the window of `cx` is still open.
fn is_open(handle: AnyWindowHandle, cx: &VisualTestContext) -> bool {
    cx.windows().contains(&handle)
}

/// Whether a recovery snapshot has been written in `env`'s data directory.
fn snapshot_written(env: &TestEnv) -> bool {
    fs::read_dir(env.data_dir().join("recovery"))
        .into_iter()
        .flatten()
        .flatten()
        .any(|session| session.path().join("snapshot.json").is_file())
}

/// Number of recovery sessions left in `env`'s data directory.
fn recovery_sessions(env: &TestEnv) -> usize {
    fs::read_dir(env.data_dir().join("recovery"))
        .map(|dir| dir.count())
        .unwrap_or(0)
}

/// Lets time pass while a question is open, so the application's timers
/// fire and their tasks run.
fn let_time_pass(cx: &mut VisualTestContext) {
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();
}

/// Closes the oldest open question and lets its answer apply.
fn answer(env: &TestEnv, cx: &mut VisualTestContext) {
    assert!(env.prompts.answer_held(), "a question is open");
    cx.run_until_parked();
}

/// The owner's report: in a project, add a circuit with the + or Ctrl+T,
/// then delete it with its tab's close button and confirm. The application
/// keeps running its tasks while the question is open, and the circuit is
/// deleted once it is answered.
#[gpui_kit::test]
fn deleting_a_new_circuit_waits_for_the_answer(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let path = env.write_project("rc.xsim_proj", &rc_project());
    let (workspace, mut window) = open(cx, &env, Some(&path));
    let first = names(&workspace, &window);
    env.prompts.hold.set(true);
    for with_shortcut in [false, true] {
        if with_shortcut {
            window.simulate_keystrokes(primary!("t"));
            window.run_until_parked();
        } else {
            click("add-circuit", &mut window);
        }
        assert_eq!(tabs(&workspace, &window), (2, 1), "the new circuit shows");

        env.prompts.delete.borrow_mut().push_back(true);
        click("delete-circuit-1", &mut window);
        assert_eq!(env.prompts.open_questions(), 1);
        assert_eq!(
            tabs(&workspace, &window),
            (2, 1),
            "nothing before the answer"
        );
        // The edit's recovery snapshot is written while the question is
        // open: the task that crashed inside the synchronous dialog.
        let_time_pass(&mut window);
        assert!(snapshot_written(&env), "the application kept running");

        answer(&env, &mut window);
        assert_eq!(tabs(&workspace, &window), (1, 0));
        assert_eq!(names(&workspace, &window), first);
        let editor_focused = workspace.update_in(&mut window, |w, window, cx| {
            w.active_tab().editor.focus_handle(cx).is_focused(window)
        });
        assert!(editor_focused, "the schematic has the keyboard again");
    }
    assert_eq!(asked(&env), ["delete", "delete"]);
    let_time_pass(&mut window);
    close(&mut window);
}

/// Declining keeps the circuit; the question can be asked again.
#[gpui_kit::test]
fn declining_the_deletion_keeps_the_circuit(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let (workspace, mut window) = open(cx, &env, None);
    window.dispatch_action(NewCircuit);
    env.prompts.hold.set(true);
    env.prompts.delete.borrow_mut().push_back(false);
    click("delete-circuit-1", &mut window);
    answer(&env, &mut window);
    assert_eq!(tabs(&workspace, &window), (2, 1));
    env.prompts.delete.borrow_mut().push_back(true);
    click("delete-circuit-0", &mut window);
    answer(&env, &mut window);
    assert_eq!(names(&workspace, &window), ["Circuit 2"]);
    close(&mut window);
}

/// While a question is open, the commands that would ask another one do
/// nothing: one dialog at a time, and a repeated click asks once.
#[gpui_kit::test]
fn a_question_is_asked_once_however_often_it_is_triggered(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let (workspace, mut window) = open(cx, &env, None);
    window.dispatch_action(NewCircuit);
    window.dispatch_action(NewCircuit);
    env.prompts.hold.set(true);
    env.prompts.delete.borrow_mut().push_back(true);
    click("delete-circuit-1", &mut window);
    click("delete-circuit-1", &mut window);
    click("delete-circuit-2", &mut window);
    window.dispatch_action(NewProject);
    window.dispatch_action(OpenProject);
    window.dispatch_action(SaveProject);
    window.dispatch_action(SaveProjectAs);
    window.dispatch_action(CloseProject);
    window.dispatch_action(ExitApplication);
    assert!(!window.simulate_close(), "the window stays while asking");
    assert_eq!(asked(&env), ["delete"]);
    assert!(is_open(window.window_handle(), &window));

    answer(&env, &mut window);
    assert_eq!(names(&workspace, &window), ["Circuit 1", "Circuit 3"]);
    assert_eq!(env.prompts.open_questions(), 0);
    click("delete-circuit-1", &mut window);
    assert_eq!(asked(&env), ["delete", "delete"], "answered, it asks again");
    answer(&env, &mut window);
    close(&mut window);
}

/// The answer applies to the circuit it was asked about: when the project
/// was replaced while the question was open, nothing is deleted.
#[gpui_kit::test]
fn a_late_answer_deletes_only_the_circuit_it_was_about(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let mut other = Project::new("Other");
    let second = other.next_circuit_name();
    other.circuits.push(CircuitDocument::new(second));
    let other = env.write_project("other.xsim_proj", &other);
    let (workspace, mut window) = open(cx, &env, None);
    window.dispatch_action(NewCircuit);
    env.prompts.hold.set(true);
    env.prompts.delete.borrow_mut().push_back(true);
    click("delete-circuit-1", &mut window);
    workspace.update_in(&mut window, |w, window, cx| w.load_path(other, window, cx));
    window.run_until_parked();
    let shown = names(&workspace, &window);
    answer(&env, &mut window);
    assert_eq!(names(&workspace, &window), shown, "nothing was deleted");
    assert_eq!(shown.len(), 2);
    close(&mut window);
}

/// Save in the unsaved-changes question saves first; a cancelled or failed
/// save keeps the project and its changes.
#[gpui_kit::test]
fn a_project_is_replaced_only_after_its_changes_are_saved(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let (workspace, mut window) = open(cx, &env, None);
    window.dispatch_action(NewCircuit);

    env.prompts
        .unsaved
        .borrow_mut()
        .push_back(UnsavedChoice::Save);
    window.dispatch_action(NewProject);
    assert_eq!(asked(&env), ["unsaved", "save"]);
    assert_eq!(tabs(&workspace, &window).0, 2, "Save as was cancelled");
    assert!(is_dirty(&workspace, &window));

    let unwritable = env.path("missing-folder").join("lost.xsim_proj");
    env.prompts
        .unsaved
        .borrow_mut()
        .push_back(UnsavedChoice::Save);
    env.prompts.save.borrow_mut().push_back(unwritable.clone());
    window.dispatch_action(NewProject);
    assert!(!unwritable.exists());
    assert_eq!(tabs(&workspace, &window).0, 2, "the save failed");
    assert!(is_dirty(&workspace, &window));

    let saved = env.path("kept.xsim_proj");
    env.prompts
        .unsaved
        .borrow_mut()
        .push_back(UnsavedChoice::Save);
    env.prompts.save.borrow_mut().push_back(saved.clone());
    window.dispatch_action(NewProject);
    let kept = Project::load(&saved)
        .expect("the changes were saved")
        .project;
    assert_eq!(kept.circuits.len(), 2);
    assert_eq!(
        tabs(&workspace, &window).0,
        1,
        "then the new project opened"
    );
    assert!(!is_dirty(&workspace, &window));
    close(&mut window);
}

/// File > Open picks the file first, then asks about unsaved changes.
#[gpui_kit::test]
fn open_asks_for_the_file_then_about_unsaved_changes(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let path = env.write_project("rc.xsim_proj", &rc_project());
    let (workspace, mut window) = open(cx, &env, None);
    window.dispatch_action(NewCircuit);
    env.prompts.hold.set(true);
    env.prompts.open.borrow_mut().push_back(path.clone());
    env.prompts
        .unsaved
        .borrow_mut()
        .push_back(UnsavedChoice::Discard);
    window.dispatch_action(OpenProject);
    assert_eq!(asked(&env), ["open"]);
    answer(&env, &mut window);
    assert_eq!(asked(&env), ["open", "unsaved"]);
    assert_eq!(
        tabs(&workspace, &window).0,
        2,
        "not before the second answer"
    );
    answer(&env, &mut window);
    workspace.read_with(&window, |w, _| {
        assert_eq!(w.doc.path.as_deref(), Some(path.as_path()));
        assert!(!w.doc.dirty);
    });
    close(&mut window);
}

/// The platform needs at once whether the window may close: with unsaved
/// changes it stays open while the question is asked (once, however often
/// close is pressed), Cancel keeps it, and Don't save closes it afterwards.
#[gpui_kit::test]
fn closing_with_unsaved_changes_closes_after_the_answer(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let (workspace, mut window) = open(cx, &env, None);
    let handle = window.window_handle();
    window.dispatch_action(NewCircuit);
    env.prompts.hold.set(true);
    env.prompts
        .unsaved
        .borrow_mut()
        .extend([UnsavedChoice::Cancel, UnsavedChoice::Discard]);

    assert!(!window.simulate_close());
    assert!(!window.simulate_close(), "a second close asks nothing more");
    window.dispatch_action(ExitApplication);
    assert_eq!(asked(&env), ["unsaved"]);
    answer(&env, &mut window);
    assert!(is_open(handle, &window), "Cancel keeps the window");
    assert!(is_dirty(&workspace, &window));

    assert!(!window.simulate_close());
    let_time_pass(&mut window);
    assert!(is_open(handle, &window), "open until answered");
    answer(&env, &mut window);
    assert!(!is_open(handle, &window), "Don't save closes the window");
    assert_eq!(recovery_sessions(&env), 0, "the session ended cleanly");
}

/// Save in the closing question closes the window once the file is written;
/// File > Exit asks the same way.
#[gpui_kit::test]
fn saving_on_close_closes_once_written(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let (_, mut window) = open(cx, &env, None);
    let handle = window.window_handle();
    window.dispatch_action(NewCircuit);
    let saved = env.path("closing.xsim_proj");
    env.prompts
        .unsaved
        .borrow_mut()
        .push_back(UnsavedChoice::Save);
    env.prompts.save.borrow_mut().push_back(saved.clone());
    window.dispatch_action(ExitApplication);
    assert_eq!(asked(&env), ["unsaved", "save"]);
    assert!(saved.is_file());
    assert!(!is_open(handle, &window));
    assert_eq!(recovery_sessions(&env), 0);
}

/// D22 with D25: closing the main window over a dialog window asks about
/// the unsaved changes; Cancel keeps both windows, Don't save closes the
/// main window and its dialog with it, so nothing is left open.
#[gpui_kit::test]
fn closing_over_a_dialog_window_closes_both_after_the_answer(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let path = env.write_project("rc.xsim_proj", &rc_project());
    let (_, mut window) = open(cx, &env, Some(&path));
    window.dispatch_action(NewCircuit);
    window.dispatch_action(OpenAnalysisSettings);
    window.run_until_parked();
    let analysis = dialog(&window);
    env.prompts
        .unsaved
        .borrow_mut()
        .extend([UnsavedChoice::Cancel, UnsavedChoice::Discard]);
    assert!(!window.simulate_close());
    window.run_until_parked();
    assert!(has_dialog(&window), "Cancel keeps the dialog");
    assert!(!window.simulate_close());
    window.run_until_parked();
    assert!(analysis.windows().is_empty(), "both windows closed");
    assert_eq!(asked(&env), ["unsaved", "unsaved"]);
}

/// A clean project closes at once, as before.
#[gpui_kit::test]
fn a_clean_project_closes_without_asking(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let path = env.write_project("rc.xsim_proj", &rc_project());
    let (_, mut window) = open(cx, &env, Some(&path));
    assert!(window.simulate_close());
    assert!(asked(&env).is_empty());
    close(&mut window);
}

/// Restoring a crashed session's work asks about unsaved changes first;
/// the snapshot stays offered when the question is cancelled.
#[gpui_kit::test]
fn restoring_after_a_crash_asks_about_unsaved_changes(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let (_, mut first) = open(cx, &env, None);
    first.dispatch_action(NewCircuit);
    let_time_pass(&mut first);
    leave_crashed_session(&env);

    let (restarted, mut second) = open(cx, &env, None);
    assert!(has_dialog(&second), "recovery is offered");
    second.dispatch_action(NewCircuit);
    second.dispatch_action(NewCircuit);
    click("recovery-restore-0", &mut dialog(&second));
    assert_eq!(asked(&env), ["unsaved"]);
    restarted.read_with(&second, |w, _| {
        assert_eq!(w.orphan_count(), 1, "cancelled: still offered");
        assert_eq!(w.tabs.len(), 3, "and the work in the window stays");
    });

    env.prompts
        .unsaved
        .borrow_mut()
        .push_back(UnsavedChoice::Discard);
    restarted.update_in(&mut second, |w, window, cx| w.restore_orphan(0, window, cx));
    second.run_until_parked();
    restarted.read_with(&second, |w, _| {
        assert_eq!(w.orphan_count(), 0);
        assert_eq!(w.tabs.len(), 2, "the recovered project has both circuits");
        assert!(w.doc.dirty);
    });
    close(&mut second);
    close(&mut first);
}

/// Copies the snapshot of the running session into a session that looks
/// crashed: its directory with an unlocked lock file.
fn leave_crashed_session(env: &TestEnv) {
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
}

/// Exports go to the circuit whose plot was shown when the file was asked
/// for; when that circuit is deleted meanwhile, nothing is written.
#[gpui_kit::test]
fn an_export_is_skipped_when_its_circuit_went_away(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let path = env.write_project("rc.xsim_proj", &rc_project());
    let (workspace, mut window) = open(cx, &env, Some(&path));
    window.simulate_keystrokes("f5");
    wait_until(&mut window, |cx| {
        workspace.read_with(cx, |w, _| w.active_tab().has_result())
    });
    window.dispatch_action(NewCircuit);
    click("circuit-tab-0", &mut window);
    env.prompts.hold.set(true);
    env.prompts.export.borrow_mut().push_back(env.path("plot"));
    window.dispatch_action(ExportCsv);
    workspace.update_in(&mut window, |w, window, cx| w.remove_circuit(0, window, cx));
    answer(&env, &mut window);
    assert!(!env.path("plot.csv").exists());
    close(&mut window);
}

/// Runs `answer` to completion on the test executor.
fn resolve<T: 'static>(answer: Answer<T>, cx: &mut VisualTestContext) -> T {
    let slot = Rc::new(RefCell::new(None));
    let filled = slot.clone();
    cx.foreground_executor()
        .spawn(async move {
            filled.replace(Some(answer.await));
        })
        .detach();
    cx.run_until_parked();
    slot.take().expect("the question was answered")
}

/// The native message boxes, through GPUI's test platform: their text, and
/// which button means what.
#[gpui_kit::test]
fn native_questions_name_their_buttons(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let (_, mut window) = open(cx, &env, None);

    for (button, choice) in [
        ("Save", UnsavedChoice::Save),
        ("Don't save", UnsavedChoice::Discard),
        ("Cancel", UnsavedChoice::Cancel),
    ] {
        let question =
            window.update(|window, cx| NativePrompts.unsaved_changes("RC filter", window, cx));
        let (message, detail) = window.pending_prompt().expect("a message box is open");
        assert!(message.contains("\"RC filter\""), "{message}");
        assert!(detail.contains("lost"), "{detail}");
        window.simulate_prompt_answer(button);
        assert_eq!(resolve(question, &mut window), choice, "{button}");
    }

    for (button, confirmed) in [("Delete", true), ("Cancel", false)] {
        let question =
            window.update(|window, cx| NativePrompts.confirm_delete_circuit("Filter", window, cx));
        let (message, _) = window.pending_prompt().expect("a message box is open");
        assert!(message.contains("\"Filter\""), "{message}");
        window.simulate_prompt_answer(button);
        assert_eq!(resolve(question, &mut window), confirmed, "{button}");
    }
    assert!(!window.has_pending_prompt());
    close(&mut window);
}

/// Deleting the circuit that is simulating: its run's progress ticks while
/// the question is open, the run stops with the circuit once the deletion
/// is confirmed, and its outcome lands nowhere.
#[gpui_kit::test]
fn deleting_a_running_circuit_stops_its_run_after_the_answer(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let path = env.write_project("long.xsim_proj", &long_run_project());
    let (workspace, mut window) = open(cx, &env, Some(&path));
    window.simulate_keystrokes("f5");
    window.run_until_parked();
    assert!(workspace.read_with(&window, |w, _| w.active_tab().is_running()));
    window.dispatch_action(NewCircuit);
    env.prompts.hold.set(true);
    env.prompts.delete.borrow_mut().push_back(true);
    click("delete-circuit-0", &mut window);
    let_time_pass(&mut window);
    assert!(
        workspace.read_with(&window, |w, _| w.tabs[0].is_running()),
        "the run goes on while the question is open"
    );

    answer(&env, &mut window);
    assert_eq!(names(&workspace, &window), ["Circuit 2"]);
    assert_eq!(tabs(&workspace, &window), (1, 0));
    // The cancelled run reports back on its own thread.
    for _ in 0..20 {
        let_time_pass(&mut window);
        std::thread::sleep(Duration::from_millis(10));
    }
    workspace.read_with(&window, |w, _| {
        let tab = w.active_tab();
        assert!(!tab.is_running() && !tab.has_result(), "no outcome landed");
    });
    close(&mut window);
}

/// Close, then Save: when the file dialog is cancelled or the file cannot
/// be written, the window stays open with its changes, and the next close
/// asks again.
#[gpui_kit::test]
fn closing_stays_open_when_the_save_is_cancelled_or_fails(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let (workspace, mut window) = open(cx, &env, None);
    let handle = window.window_handle();
    window.dispatch_action(NewCircuit);

    env.prompts
        .unsaved
        .borrow_mut()
        .push_back(UnsavedChoice::Save);
    assert!(!window.simulate_close());
    window.run_until_parked();
    assert_eq!(asked(&env), ["unsaved", "save"], "Save asks for a file");
    assert!(
        is_open(handle, &window),
        "a cancelled save keeps the window"
    );
    assert!(is_dirty(&workspace, &window));

    let unwritable = env.path("missing-folder").join("lost.xsim_proj");
    env.prompts
        .unsaved
        .borrow_mut()
        .push_back(UnsavedChoice::Save);
    env.prompts.save.borrow_mut().push_back(unwritable.clone());
    window.dispatch_action(ExitApplication);
    assert_eq!(asked(&env), ["unsaved", "save", "unsaved", "save"]);
    assert!(!unwritable.exists());
    assert!(is_open(handle, &window), "a failed save keeps the window");
    assert!(is_dirty(&workspace, &window));

    env.prompts
        .unsaved
        .borrow_mut()
        .push_back(UnsavedChoice::Discard);
    assert!(!window.simulate_close());
    window.run_until_parked();
    assert!(!is_open(handle, &window), "then Don't save closes it");
}

/// Every question, triggered twice while it is open, is asked once;
/// cancelling it changes nothing and leaves the next question free to be
/// asked.
#[gpui_kit::test]
fn each_question_is_asked_once_and_again_after_a_cancel(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let path = env.write_project("rc.xsim_proj", &rc_project());
    let (workspace, mut window) = open(cx, &env, Some(&path));
    let handle = window.window_handle();
    window.simulate_keystrokes("f5");
    wait_until(&mut window, |cx| {
        workspace.read_with(cx, |w, _| w.active_tab().has_result())
    });
    window.dispatch_action(NewCircuit);
    click("circuit-tab-0", &mut window);
    env.prompts.hold.set(true);

    /// A command and the question it asks.
    type Command = (&'static str, fn(&mut VisualTestContext));
    let commands: [Command; 7] = [
        ("open", |w| w.dispatch_action(OpenProject)),
        ("save", |w| w.dispatch_action(SaveProjectAs)),
        ("export", |w| w.dispatch_action(ExportCsv)),
        ("unsaved", |w| w.dispatch_action(NewProject)),
        ("unsaved", |w| w.dispatch_action(CloseProject)),
        ("unsaved", |w| w.dispatch_action(ExitApplication)),
        ("unsaved", |w| assert!(!w.simulate_close())),
    ];
    for (question, command) in commands.into_iter().chain([commands[0]]) {
        let before = asked(&env).len();
        command(&mut window);
        command(&mut window);
        window.run_until_parked();
        assert_eq!(&asked(&env)[before..], [question], "{question}");
        answer(&env, &mut window);
        assert_eq!(env.prompts.open_questions(), 0, "{question}");
        assert!(is_open(handle, &window), "{question}");
        workspace.read_with(&window, |w, _| {
            assert_eq!(w.tabs.len(), 2, "{question}");
            assert!(w.doc.dirty, "{question}");
            assert_eq!(w.doc.path.as_deref(), Some(path.as_path()), "{question}");
        });
    }
    let written: Vec<_> = fs::read_dir(env.path(""))
        .expect("test directory")
        .flatten()
        .map(|entry| entry.file_name())
        .filter(|name| name != "rc.xsim_proj" && name != "app-data")
        .collect();
    assert!(written.is_empty(), "nothing was written: {written:?}");
    close(&mut window);
}
