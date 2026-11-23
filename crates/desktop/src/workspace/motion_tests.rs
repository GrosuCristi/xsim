//! D19: panels open and close without animation. Without the test clock
//! moving, each panel is painted where it stays on the first frame after it
//! opens, stops asking for frames after at most one more, and is gone on
//! the first frame after it closes, by a button, Escape or Enter. The dock,
//! the plot's side panels and the kit's menus have never animated; their
//! tests keep it so. Toasts keep coming and going, and the run loader keeps
//! turning.
//!
//! The confirmations (unsaved changes, deleting a circuit) are the
//! operating system's own dialogs and are not drawn by the kit.

use super::tests::leave_crashed_session;
use crate::actions::*;
use crate::test_support::{
    TestEnv, click, close, init, long_run_project, open, press, rc_project, wait_until,
};
use gpui_kit::component::WindowExt;
use gpui_kit::component::notification::Notification;
use gpui_kit::{
    Background, Bounds, Hsla, Modifiers, Pixels, ScaledPixels, TestAppContext, VisualTestContext,
};
use std::time::Duration;
use xsim_project::{Component, Part, PartKind, Point as GridPoint};

/// The surface of the first dialog open.
const DIALOG: &str = "dialog-0";

/// The surface of a dialog opened over the first.
const STACKED_DIALOG: &str = "dialog-1";

/// Lets the clock run well past every kit transition and paints a fresh
/// frame: the test window repaints only when something marks it dirty.
fn much_later(window: &mut VisualTestContext) {
    window.executor().advance_clock(Duration::from_secs(2));
    window.update(|window, _| window.refresh());
    window.run_until_parked();
}

/// Where `selector` is painted on the first frame after it opened, which
/// must be where it stays. Some panels (the part properties, the library,
/// the measurements) were seen to ask for one more frame, on which nothing
/// moves; none may ask for frames after that, as a running animation does.
/// Nor has the panel moved once every transition would be over. Sliding in
/// from the top (the kit's dialogs) or growing (a dock) fails here.
fn assert_opened_at_rest(
    selector: &'static str,
    what: &str,
    window: &mut VisualTestContext,
) -> Bounds<Pixels> {
    let first = window
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{what}: not shown on its first frame"));
    for frame in 1..=3 {
        let asked = window.update(|window, cx| window.simulate_next_frame(cx));
        window.run_until_parked();
        assert_eq!(
            window.debug_bounds(selector),
            Some(first),
            "{what}: moved on frame {frame}"
        );
        if frame > 1 {
            assert_eq!(asked, 0, "{what}: still asks for frames on frame {frame}");
        }
    }
    much_later(window);
    assert_eq!(
        window.debug_bounds(selector),
        Some(first),
        "{what}: moved after its first frame"
    );
    first
}

/// No dialog is painted any more, not even one fading out, and none is
/// left to finish closing.
fn assert_dialog_gone(what: &str, window: &mut VisualTestContext) {
    assert!(
        window.debug_bounds("dialog-layer").is_none(),
        "{what}: a dialog is still painted"
    );
    assert!(
        !window.update(|window, cx| window.has_active_dialog(cx)),
        "{what}: a dialog is still open"
    );
}

/// Closes the open dialog with a click on `selector`, or with the key it
/// names: Escape, or Enter, which goes through the kit's confirmation and
/// its own way of closing.
fn close_with(selector: &'static str, window: &mut VisualTestContext) {
    match selector {
        "escape" | "enter" => press(selector, window),
        _ => click(selector, window),
    }
}

#[gpui_kit::test]
fn the_analysis_settings_open_and_close_at_once(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let path = env.write_project("rc.xsim_proj", &rc_project());
    let (_, mut window) = open(cx, &env, Some(&path));
    for (closer, how) in [
        ("analysis-apply", "Apply"),
        ("analysis-cancel", "Cancel"),
        ("escape", "Escape"),
        ("enter", "Enter"),
    ] {
        window.dispatch_action(OpenAnalysisSettings);
        window.run_until_parked();
        assert_opened_at_rest(DIALOG, "analysis settings", &mut window);
        close_with(closer, &mut window);
        assert_dialog_gone(how, &mut window);
    }
    close(&mut window);
}

#[gpui_kit::test]
fn the_settings_open_and_close_at_once(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let (_, mut window) = open(cx, &env, None);
    for (closer, how) in [
        ("settings-done", "Done"),
        ("escape", "Escape"),
        ("enter", "Enter"),
    ] {
        window.dispatch_action(OpenSettings);
        window.run_until_parked();
        assert_opened_at_rest(DIALOG, "settings", &mut window);
        close_with(closer, &mut window);
        assert_dialog_gone(how, &mut window);
    }
    close(&mut window);
}

#[gpui_kit::test]
fn the_recovery_dialog_opens_and_closes_at_once(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let (_, mut window) = open(cx, &env, None);
    window.dispatch_action(NewCircuit);
    leave_crashed_session(&env, &mut window);
    let (_, mut second) = open(cx, &env, None);
    assert_opened_at_rest(DIALOG, "recovery", &mut second);
    click("recovery-later", &mut second);
    assert_dialog_gone("Decide later", &mut second);
    close(&mut second);
    close(&mut window);
}

#[gpui_kit::test]
fn the_part_properties_open_and_close_at_once(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let path = env.write_project("rc.xsim_proj", &rc_project());
    let (workspace, mut window) = open(cx, &env, Some(&path));
    let editor = workspace.read_with(&window, |w, _| w.active_tab().editor.clone());
    for (closer, how) in [
        ("properties-cancel", "Cancel"),
        ("properties-ok", "OK"),
        ("escape", "Escape"),
        ("enter", "Enter"),
    ] {
        // R1 of the RC circuit.
        window
            .update(|window, cx| xsim_editor::properties::open_properties(&editor, 2, window, cx));
        window.run_until_parked();
        assert_opened_at_rest(DIALOG, "part properties", &mut window);
        close_with(closer, &mut window);
        assert_dialog_gone(how, &mut window);
    }
    close(&mut window);
}

#[gpui_kit::test]
fn the_component_library_opens_and_closes_at_once(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let (_, mut window) = open(cx, &env, None);
    // The schematic has the keyboard; P opens the library.
    press("p", &mut window);
    assert_opened_at_rest(DIALOG, "component library", &mut window);
    press("escape", &mut window);
    assert_dialog_gone("Escape", &mut window);
    close(&mut window);
}

/// The model library opened from the part properties is a second dialog
/// over the first. It opens and closes at once, and the properties below
/// neither move nor play their entrance again.
#[gpui_kit::test]
fn the_model_library_over_the_part_properties_opens_and_closes_at_once(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let mut project = rc_project();
    let schematic = &mut project.circuits[0].schematic;
    let transistor = schematic.allocate_id();
    schematic.components.push(Component {
        id: transistor,
        name: "Q1".into(),
        position: GridPoint::new(16, 0),
        orientation: Default::default(),
        part: Part::default_for(PartKind::PnpBjt),
    });
    let path = env.write_project("bjt.xsim_proj", &project);
    let (workspace, mut window) = open(cx, &env, Some(&path));
    let editor = workspace.read_with(&window, |w, _| w.active_tab().editor.clone());
    window.update(|window, cx| {
        xsim_editor::properties::open_properties(&editor, transistor, window, cx)
    });
    window.run_until_parked();
    let properties = assert_opened_at_rest(DIALOG, "part properties", &mut window);

    click("choose-model", &mut window);
    assert_opened_at_rest(STACKED_DIALOG, "model library", &mut window);
    assert_eq!(
        window.debug_bounds(DIALOG),
        Some(properties),
        "the properties moved under the library"
    );
    press("escape", &mut window);
    assert!(
        window.debug_bounds(STACKED_DIALOG).is_none(),
        "the library is gone on the first frame"
    );
    assert_eq!(
        assert_opened_at_rest(DIALOG, "the properties left open", &mut window),
        properties
    );
    press("escape", &mut window);
    assert_dialog_gone("Escape", &mut window);
    close(&mut window);
}

#[gpui_kit::test]
fn the_diagnostics_dock_opens_and_closes_at_once(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let (_, mut window) = open(cx, &env, None);
    window.simulate_keystrokes(primary!("j"));
    let dock = assert_opened_at_rest("diagnostics-dock", "diagnostics dock", &mut window);
    let rem = window.update(|window, _| window.rem_size());
    assert_eq!(dock.size.height, rem * 11., "at its full height at once");
    window.simulate_keystrokes(primary!("j"));
    assert!(
        window.debug_bounds("diagnostics-dock").is_none(),
        "the dock is gone on the first frame"
    );
    close(&mut window);
}

/// The plot's side panels: the measurements and the cursor readout.
#[gpui_kit::test]
fn the_plot_panels_open_and_close_at_once(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let path = env.write_project("rc.xsim_proj", &rc_project());
    let (workspace, mut window) = open(cx, &env, Some(&path));
    window.simulate_keystrokes("f5");
    wait_until(&mut window, |cx| {
        workspace.read_with(cx, |w, _| {
            !w.active_tab().is_running() && w.active_tab().has_result()
        })
    });
    for (toggle, panel) in [
        ("toggle-measurements", "measurements-panel"),
        ("toggle-cursors", "cursor-readout"),
    ] {
        click(toggle, &mut window);
        assert_opened_at_rest(panel, panel, &mut window);
        click(toggle, &mut window);
        assert!(
            window.debug_bounds(panel).is_none(),
            "{panel}: gone on the first frame"
        );
    }
    close(&mut window);
}

/// The quads a frame painted, comparable from frame to frame.
type Painted = Vec<(Bounds<ScaledPixels>, Background, Hsla)>;

fn painted(window: &mut VisualTestContext) -> Painted {
    window.update(|window, _| {
        window
            .painted_quads()
            .iter()
            .map(|quad| (quad.bounds, quad.background, quad.border_color))
            .collect()
    })
}

/// What `now` paints that `before` did not.
fn added(before: &Painted, now: Painted) -> Painted {
    now.into_iter()
        .filter(|quad| !before.contains(quad))
        .collect()
}

/// A menu opened from `trigger` since `before` was painted: on its first
/// frame it is painted below the trigger, and from then on it neither
/// moves nor fades, and stops asking for frames after at most one more.
/// Returns the bounds of its surface, the largest quad it added.
fn assert_menu_opened_at_rest(
    trigger: &'static str,
    before: &Painted,
    window: &mut VisualTestContext,
) -> Bounds<ScaledPixels> {
    let first = added(before, painted(window));
    let area =
        |bounds: &Bounds<ScaledPixels>| bounds.size.width.as_f32() * bounds.size.height.as_f32();
    let surface = first
        .iter()
        .map(|quad| quad.0)
        .max_by(|a, b| area(a).total_cmp(&area(b)))
        .unwrap_or_else(|| panic!("{trigger}: no menu on its first frame"));
    let scale = window.update(|window, _| window.scale_factor());
    let trigger_bounds = window
        .debug_bounds(trigger)
        .unwrap_or_else(|| panic!("{trigger} is not on screen"))
        .scale(scale);
    assert!(
        surface.origin.y >= trigger_bounds.bottom() - ScaledPixels(1.),
        "{trigger}: the largest new quad {surface:?} is not a menu below {trigger_bounds:?}"
    );
    for frame in 1..=3 {
        let asked = window.update(|window, cx| window.simulate_next_frame(cx));
        window.run_until_parked();
        assert_eq!(
            added(before, painted(window)),
            first,
            "{trigger}: the menu changed on frame {frame}"
        );
        if frame > 1 {
            assert_eq!(
                asked, 0,
                "{trigger}: still asks for frames on frame {frame}"
            );
        }
    }
    much_later(window);
    assert_eq!(
        added(before, painted(window)),
        first,
        "{trigger}: the menu changed after its first frame"
    );
    surface
}

/// No quad is painted where the menu surface was.
fn assert_menu_gone(trigger: &str, surface: Bounds<ScaledPixels>, window: &mut VisualTestContext) {
    assert!(
        !painted(window).iter().any(|quad| quad.0 == surface),
        "{trigger}: the menu is still painted"
    );
}

/// A title-bar menu, and the interface-scale dropdown inside a dialog.
#[gpui_kit::test]
fn menus_open_and_close_at_once(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let (_, mut window) = open(cx, &env, None);
    let file = window
        .debug_bounds("menu-file")
        .expect("the File menu is on screen")
        .center();
    window.simulate_mouse_move(file, None, Modifiers::default());
    window.run_until_parked();
    let before = painted(&mut window);
    click("menu-file", &mut window);
    let surface = assert_menu_opened_at_rest("menu-file", &before, &mut window);
    press("escape", &mut window);
    assert_menu_gone("menu-file", surface, &mut window);

    window.dispatch_action(OpenSettings);
    window.run_until_parked();
    // Light, Dark, then the interface scale.
    for _ in 0..3 {
        press("tab", &mut window);
    }
    let dialog = window.debug_bounds(DIALOG).expect("the settings are open");
    let before = painted(&mut window);
    press("enter", &mut window);
    let surface = assert_menu_opened_at_rest("ui-scale", &before, &mut window);
    press("escape", &mut window);
    assert_menu_gone("ui-scale", surface, &mut window);
    assert_eq!(
        window.debug_bounds(DIALOG),
        Some(dialog),
        "the settings stay as they were"
    );
    close(&mut window);
}

/// Toasts appear and leave without sliding, but keep their lifecycle: shown
/// for their timeout, then gone.
#[gpui_kit::test]
fn toasts_still_hide_themselves_after_their_timeout(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let (_, mut window) = open(cx, &env, None);
    let toasts =
        |window: &mut VisualTestContext| window.update(|window, cx| window.notifications(cx).len());
    window.update(|window, cx| crate::ui::toast(Notification::info("Saved"), window, cx));
    window.run_until_parked();
    assert_eq!(toasts(&mut window), 1, "the toast is shown");
    let step = Duration::from_millis(250);
    let mut shown = Duration::ZERO;
    while toasts(&mut window) > 0 {
        assert!(shown < Duration::from_secs(10), "the toast never left");
        window.executor().advance_clock(step);
        window.run_until_parked();
        shown += step;
    }
    assert!(
        shown >= Duration::from_secs(5),
        "left after {shown:?}, before its timeout"
    );
    close(&mut window);
}

/// The run loader's spinner is shown and asks for the next frame, which the
/// kit's spinner no longer does with motion reduced; so this fails if the
/// loader goes back to it. That the angle advances is checked in
/// [`crate::ui::spinner`].
#[gpui_kit::test]
fn the_run_loader_spinner_keeps_turning(cx: &mut TestAppContext) {
    init(cx);
    let env = TestEnv::new();
    let path = env.write_project("long.xsim_proj", &long_run_project());
    let (workspace, mut window) = open(cx, &env, Some(&path));
    window.dispatch_action(RunSimulation);
    assert!(
        window.debug_bounds("run-spinner").is_some(),
        "the loader shows the spinner"
    );
    for _ in 0..3 {
        let frames = window.update(|window, cx| window.simulate_next_frame(cx));
        assert!(frames > 0, "the spinner stopped asking for frames");
        window.run_until_parked();
    }
    window.dispatch_action(StopSimulation);
    wait_until(&mut window, |cx| {
        workspace.read_with(cx, |w, _| !w.active_tab().is_running())
    });
    assert!(window.debug_bounds("run-spinner").is_none());
    close(&mut window);
}
