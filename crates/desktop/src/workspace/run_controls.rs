//! The run controls at the right end of the circuit tab bar (D20): the
//! out-of-date hint, the progress with Pause/Resume and Stop while the
//! active circuit runs, Run, and the analysis settings. The analysis that
//! Run executes is chosen in the analysis settings; the status bar names
//! it.

use super::Workspace;
use crate::actions::*;
use crate::dialogs::analysis::open_analysis_dialog;
use crate::simulation::summary::format_percent;
use crate::ui::testable;
use gpui_kit::component::{
    ActiveTheme, Disableable, IconName, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    separator::Separator,
    tag::Tag,
};
use gpui_kit::{prelude::*, *};

impl Workspace {
    /// The controls, as the tab bar's suffix: kept whole, the tabs scroll
    /// in the space left of them.
    pub(super) fn render_run_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .debug_selector(|| "run-controls".into())
            .flex_shrink_0()
            .items_center()
            .gap_1()
            .pl_2()
            .pr_1()
            .children(self.stale_hint())
            .children(self.run_progress(cx))
            .child(self.run_button(cx))
            .child(self.settings_button(cx))
    }

    fn run_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        testable(
            "run-button",
            Button::new("run")
                .track_focus(&self.run_focus)
                .small()
                .primary()
                .icon(IconName::Play)
                .label("Run")
                .disabled(self.active_tab().is_running() || self.doc.read_only)
                .tooltip_with_action("Run simulation", &RunSimulation, Some(WORKSPACE_CONTEXT))
                .on_click(cx.listener(|this, _, _, cx| this.run_simulation(cx))),
        )
    }

    fn settings_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        testable(
            "analysis-settings",
            Button::new("analysis-settings")
                .track_focus(&self.settings_focus)
                .small()
                .ghost()
                .icon(IconName::Settings2)
                .tooltip_with_action(
                    "Analysis settings",
                    &OpenAnalysisSettings,
                    Some(WORKSPACE_CONTEXT),
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    open_analysis_dialog(cx.entity(), this.analysis_dialog_input(), window, cx)
                })),
        )
    }

    /// Progress, Pause/Resume and Stop while the active circuit runs,
    /// then a separator before Run.
    fn run_progress(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(active) = &self.active_tab().run else {
            return Vec::new();
        };
        let paused = active.run.is_paused();
        let (icon, tip) = if paused {
            (IconName::Play, "Resume")
        } else {
            (IconName::Pause, "Pause")
        };
        vec![
            div()
                .debug_selector(|| "run-progress".into())
                // Room for "100%", so the buttons stay put while it counts.
                .min_w(rems(2.25))
                .flex()
                .justify_end()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(format_percent(active.run.progress()))
                .into_any_element(),
            testable(
                "pause-button",
                Button::new("pause")
                    .small()
                    .ghost()
                    .icon(icon)
                    .tooltip_with_action(tip, &TogglePause, Some(WORKSPACE_CONTEXT))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_pause(cx))),
            )
            .into_any_element(),
            testable(
                "stop-button",
                Button::new("stop")
                    .small()
                    .ghost()
                    .icon(gpui_kit::assets::IconName::Square)
                    .tooltip_with_action("Stop", &StopSimulation, Some(WORKSPACE_CONTEXT))
                    .on_click(cx.listener(|this, _, _, cx| this.stop_simulation(cx))),
            )
            .into_any_element(),
            Separator::vertical()
                .h(rems(1.25))
                .mx_1()
                .into_any_element(),
        ]
    }

    /// "Results out of date - press F5" once the schematic changed after a
    /// run, or for an opened result of another schematic (INT-1). Not
    /// while the circuit runs: the loader covers the old result then, Run
    /// is disabled, and the bar needs the room for the run's own controls.
    fn stale_hint(&self) -> Option<impl IntoElement> {
        let tab = self.active_tab();
        (tab.is_stale() && !tab.is_running()).then(|| {
            testable(
                "stale-results",
                Tag::warning()
                    .small()
                    .child(stale_hint_text(self.doc.read_only)),
            )
            .mr_1()
        })
    }
}

/// The out-of-date hint. A read-only project (from a newer xsim, Q45) can
/// show an out-of-date result since INT-1, but cannot run, so the hint does
/// not ask for F5 there.
pub(super) fn stale_hint_text(read_only: bool) -> &'static str {
    if read_only {
        "Results out of date"
    } else {
        "Results out of date - press F5"
    }
}

#[cfg(test)]
mod tests {
    use super::Workspace;
    use crate::actions::*;
    use crate::appearance::SCALE_PRESETS;
    use crate::test_support::{
        TestEnv, click, close, init, long_run_project, open, press, rc_project, wait_until,
    };
    use gpui_kit::component::WindowExt;
    use gpui_kit::{
        Bounds, Entity, Focusable, Keystroke, Pixels, TestAppContext, VisualTestContext, px,
    };
    use xsim_project::Part;

    fn bounds(selector: &'static str, cx: &mut VisualTestContext) -> Bounds<Pixels> {
        cx.debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} is not on screen"))
    }

    fn assert_near(actual: Pixels, expected: Pixels, what: &str) {
        assert!(
            (actual - expected).abs() <= px(0.5),
            "{what}: {actual:?}, expected {expected:?}"
        );
    }

    fn rem(cx: &mut VisualTestContext) -> Pixels {
        cx.update(|window, _| window.rem_size())
    }

    fn running(workspace: &Entity<Workspace>, cx: &VisualTestContext) -> bool {
        workspace.read_with(cx, |w, _| w.active_tab().is_running())
    }

    fn paused(workspace: &Entity<Workspace>, cx: &VisualTestContext) -> bool {
        workspace.read_with(cx, |w, _| {
            w.active_tab()
                .run
                .as_ref()
                .is_some_and(|r| r.run.is_paused())
        })
    }

    fn runs_started(workspace: &Entity<Workspace>, cx: &VisualTestContext) -> u64 {
        workspace.read_with(cx, |w, _| w.run_generation)
    }

    /// `element` lies in the circuit tab bar, centred on its middle line
    /// like the tab titles, the + and the close buttons.
    fn assert_in_bar(element: Bounds<Pixels>, bar: Bounds<Pixels>, what: &str) {
        assert!(
            element.left() >= bar.left()
                && element.right() <= bar.right()
                && element.top() >= bar.top()
                && element.bottom() <= bar.bottom(),
            "{what}: {element:?} inside the tab bar {bar:?}"
        );
        assert_near(
            element.center().y,
            bar.center().y,
            &format!("{what}: centred"),
        );
    }

    /// D20 while idle: Run and the analysis settings, in that order, alone
    /// at the right end of the tab bar (no analysis dropdown, no Copy
    /// plot), after the tabs and the +.
    fn assert_idle_controls(cx: &mut VisualTestContext, what: &str) {
        let rem = rem(cx);
        let (bar, controls) = (bounds("circuit-tabs", cx), bounds("run-controls", cx));
        let (run, settings) = (bounds("run-button", cx), bounds("analysis-settings", cx));
        assert_in_bar(run, bar, &format!("Run, {what}"));
        assert_in_bar(settings, bar, &format!("settings, {what}"));
        assert_near(
            bar.right() - settings.right(),
            rem * 0.25,
            &format!("settings at the right end, {what}"),
        );
        assert_near(
            settings.left() - run.right(),
            rem * 0.25,
            &format!("settings right after Run, {what}"),
        );
        assert_near(
            run.left() - controls.left(),
            rem * 0.5,
            &format!("nothing before Run, {what}"),
        );
        assert!(
            bounds("add-circuit", cx).right() <= controls.left(),
            "{what}: the + ends before the controls"
        );
    }

    /// D20: Run and the analysis settings sit at the right end of the
    /// circuit tab bar at every interface scale, and the tab bar follows
    /// the title bar directly: the command bar, with its analysis
    /// dropdown and Copy plot, is gone. A result (which enabled Copy plot
    /// there) adds nothing.
    #[gpui_kit::test]
    fn run_and_settings_sit_at_the_right_end_of_the_tab_bar(cx: &mut TestAppContext) {
        init(cx);
        let env = TestEnv::new();
        let path = env.write_project("rc.xsim_proj", &rc_project());
        let (workspace, mut window) = open(cx, &env, Some(&path));
        window.dispatch_action(NewCircuit);
        window.run_until_parked();
        for scale in SCALE_PRESETS {
            workspace.update_in(&mut window, |w, window, cx| {
                w.set_ui_scale(scale, window, cx)
            });
            window.run_until_parked();
            assert_idle_controls(&mut window, &format!("at {scale}"));
            assert_eq!(
                bounds("circuit-tabs", &mut window).top(),
                bounds("title-bar", &mut window).bottom(),
                "the tabs follow the title bar at {scale}"
            );
        }
        click("circuit-tab-0", &mut window);
        window.simulate_keystrokes("f5");
        wait_until(&mut window, |cx| !running(&workspace, cx));
        assert!(workspace.read_with(&window, |w, _| w.active_tab().has_result()));
        assert_idle_controls(&mut window, "with a result");
        close(&mut window);
    }

    /// The out-of-date hint moved along: it shows in the tab bar right
    /// before Run, and the bar keeps its height.
    #[gpui_kit::test]
    fn the_out_of_date_hint_sits_before_run(cx: &mut TestAppContext) {
        init(cx);
        let env = TestEnv::new();
        let path = env.write_project("rc.xsim_proj", &rc_project());
        let (workspace, mut window) = open(cx, &env, Some(&path));
        window.simulate_keystrokes("f5");
        wait_until(&mut window, |cx| !running(&workspace, cx));
        assert!(window.debug_bounds("stale-results").is_none());
        let height = bounds("circuit-tabs", &mut window).size.height;

        let editor = workspace.read_with(&window, |w, _| w.active_tab().editor.clone());
        editor
            .update(&mut window, |editor, cx| {
                editor.apply_component_edit(2, "R1".into(), Part::Resistor { resistance: 2e3 }, cx)
            })
            .expect("the editor applies the edit");
        window.run_until_parked();
        let (bar, hint) = (
            bounds("circuit-tabs", &mut window),
            bounds("stale-results", &mut window),
        );
        assert_in_bar(hint, bar, "the out-of-date hint");
        assert!(hint.right() <= bounds("run-button", &mut window).left());
        assert!(hint.left() >= bounds("run-controls", &mut window).left());
        assert_eq!(bar.size.height, height, "the bar keeps its height");
        assert_idle_controls_after_hint(&mut window);
        close(&mut window);
    }

    /// Run and the settings stay put at the right end beside the hint.
    fn assert_idle_controls_after_hint(cx: &mut VisualTestContext) {
        let rem = rem(cx);
        let (bar, run, settings) = (
            bounds("circuit-tabs", cx),
            bounds("run-button", cx),
            bounds("analysis-settings", cx),
        );
        assert_near(bar.right() - settings.right(), rem * 0.25, "settings");
        assert_near(settings.left() - run.right(), rem * 0.25, "Run");
    }

    /// With more circuits than fit, the tabs scroll under nothing: the
    /// shown tab and the + end before the controls.
    #[gpui_kit::test]
    fn the_controls_never_cover_the_tabs(cx: &mut TestAppContext) {
        init(cx);
        let env = TestEnv::new();
        let (_, mut window) = open(cx, &env, None);
        for _ in 0..29 {
            window.dispatch_action(NewCircuit);
        }
        window.run_until_parked();
        let controls = bounds("run-controls", &mut window);
        for shown in [
            bounds("circuit-tab-29", &mut window),
            bounds("add-circuit", &mut window),
        ] {
            assert!(
                shown.right() <= controls.left(),
                "{shown:?} before the controls {controls:?}"
            );
        }
        assert_idle_controls(&mut window, "with 30 circuits");
        close(&mut window);
    }

    /// While the circuit runs: the progress, Pause and Stop before Run,
    /// which is disabled; the buttons and F5, F6 and Shift+F5 work, and
    /// the run controls leave when the run ends.
    #[gpui_kit::test]
    fn a_run_shows_pause_and_stop_beside_a_disabled_run(cx: &mut TestAppContext) {
        init(cx);
        let env = TestEnv::new();
        let path = env.write_project("long.xsim_proj", &long_run_project());
        let (workspace, mut window) = open(cx, &env, Some(&path));
        assert!(window.debug_bounds("stop-button").is_none());

        click("run-button", &mut window);
        assert!(running(&workspace, &window), "Run starts the run");
        let started = runs_started(&workspace, &window);
        let bar = bounds("circuit-tabs", &mut window);
        let order = ["run-progress", "pause-button", "stop-button", "run-button"];
        let shown: Vec<_> = order.iter().map(|s| bounds(s, &mut window)).collect();
        for (pair, names) in shown.windows(2).zip(order.windows(2)) {
            assert!(pair[0].right() <= pair[1].left(), "{names:?} in order");
        }
        for (element, name) in shown.iter().zip(order) {
            assert_in_bar(*element, bar, name);
        }
        assert_near(
            bar.right() - bounds("analysis-settings", &mut window).right(),
            rem(&mut window) * 0.25,
            "the settings stay at the right end",
        );

        click("run-button", &mut window);
        window.simulate_keystrokes("f5");
        assert_eq!(
            runs_started(&workspace, &window),
            started,
            "Run is disabled"
        );

        click("pause-button", &mut window);
        assert!(paused(&workspace, &window), "Pause");
        window.simulate_keystrokes("f6");
        assert!(!paused(&workspace, &window), "F6 resumes");
        click("stop-button", &mut window);
        wait_until(&mut window, |cx| !running(&workspace, cx));
        for gone in ["run-progress", "pause-button", "stop-button"] {
            assert!(window.debug_bounds(gone).is_none(), "{gone} left");
        }
        assert_idle_controls(&mut window, "after Stop");

        window.simulate_keystrokes("f5");
        assert!(running(&workspace, &window), "F5 runs again");
        window.simulate_keystrokes("shift-f5");
        wait_until(&mut window, |cx| !running(&workspace, cx));
        close(&mut window);
    }

    /// The settings button opens the analysis settings, also for a
    /// read-only project, whose Run stays disabled.
    #[gpui_kit::test]
    fn the_settings_button_opens_the_analysis_settings(cx: &mut TestAppContext) {
        init(cx);
        let env = TestEnv::new();
        let path = env.write_project("rc.xsim_proj", &rc_project());
        let (workspace, mut window) = open(cx, &env, Some(&path));
        workspace.update(&mut window, |w, cx| {
            w.doc.read_only = true;
            cx.notify();
        });
        window.run_until_parked();
        click("run-button", &mut window);
        window.simulate_keystrokes("f5");
        assert!(
            !running(&workspace, &window),
            "a read-only project cannot run"
        );
        assert_eq!(runs_started(&workspace, &window), 0);

        click("analysis-settings", &mut window);
        assert!(window.update(|window, cx| window.has_active_dialog(cx)));
        assert!(window.debug_bounds("analysis-apply").is_some());
        close(&mut window);
    }

    /// Every control lies in the bar and the window, and the active
    /// circuit's tab is whole in view before the controls.
    fn assert_bar_fits(shown: &[&'static str], width: Pixels, cx: &mut VisualTestContext) {
        let (bar, controls) = (bounds("circuit-tabs", cx), bounds("run-controls", cx));
        assert!(bar.right() <= width, "the bar ends in the window: {bar:?}");
        for &name in shown {
            assert_in_bar(bounds(name, cx), bar, name);
        }
        let tab = bounds("circuit-tab-0", cx);
        assert!(
            tab.left() >= bar.left() && tab.right() <= controls.left(),
            "the active circuit's tab {tab:?} is in view before the controls {controls:?}"
        );
    }

    /// The fullest bar in the smallest window: at 200 % in a window of the
    /// minimum size, with five circuits and the active one's result out of
    /// date. While it runs, the out-of-date hint makes room for the run's
    /// controls (the loader covers the old result, and Run is disabled);
    /// it is back when the run stops. Either way everything fits and the
    /// circuit's tab stays in view.
    #[gpui_kit::test]
    fn the_fullest_bar_fits_the_smallest_window(cx: &mut TestAppContext) {
        init(cx);
        let env = TestEnv::new();
        let path = env.write_project("long.xsim_proj", &long_run_project());
        let (workspace, mut window) = open(cx, &env, Some(&path));
        for _ in 0..4 {
            window.dispatch_action(NewCircuit);
        }
        window.run_until_parked();
        click("circuit-tab-0", &mut window);
        workspace.update_in(&mut window, |w, window, cx| w.set_ui_scale(2.0, window, cx));
        let (width, height) = crate::MIN_WINDOW_SIZE;
        window.simulate_resize(gpui_kit::size(px(width), px(height)));
        // An opened result of another schematic (INT-1): out of date.
        workspace.update(&mut window, |w, cx| {
            let index = w.active_index();
            w.tabs[index].result_revision = Some(super::super::circuit_tab::OUTDATED);
            cx.notify();
        });
        window.run_until_parked();
        let idle = ["stale-results", "run-button", "analysis-settings"];
        assert_bar_fits(&idle, px(width), &mut window);

        window.simulate_keystrokes("f5");
        assert!(running(&workspace, &window));
        assert!(workspace.read_with(&window, |w, _| w.active_tab().is_stale()));
        assert!(
            window.debug_bounds("stale-results").is_none(),
            "no out-of-date hint during the run"
        );
        let running_controls = [
            "run-progress",
            "pause-button",
            "stop-button",
            "run-button",
            "analysis-settings",
        ];
        assert_bar_fits(&running_controls, px(width), &mut window);

        window.simulate_keystrokes("shift-f5");
        wait_until(&mut window, |cx| !running(&workspace, cx));
        assert!(workspace.read_with(&window, |w, _| w.active_tab().is_stale()));
        assert_bar_fits(&idle, px(width), &mut window);
        close(&mut window);
    }

    /// The controls follow the active circuit: a run of another circuit
    /// shows no progress, Pause or Stop, and leaves Run enabled. Clicks on
    /// the controls leave the focus in the schematic, also once the
    /// settings close.
    #[gpui_kit::test]
    fn the_controls_follow_the_active_circuit_and_keep_the_focus(cx: &mut TestAppContext) {
        init(cx);
        let env = TestEnv::new();
        let mut project = long_run_project();
        let mut second = project.circuits[0].clone();
        second.name = "Second".into();
        project.circuits.push(second);
        let path = env.write_project("two.xsim_proj", &project);
        let (workspace, mut window) = open(cx, &env, Some(&path));
        let run_controls = ["run-progress", "pause-button", "stop-button"];

        click("run-button", &mut window);
        assert!(running(&workspace, &window));
        // Paused, so circuit 0 is still running when it is shown again.
        click("pause-button", &mut window);
        assert!(paused(&workspace, &window));
        assert_eq!(focused_control(&workspace, &mut window), "schematic");
        click("circuit-tab-1", &mut window);
        for gone in run_controls {
            assert!(window.debug_bounds(gone).is_none(), "{gone} of circuit 0");
        }
        click("run-button", &mut window);
        assert!(running(&workspace, &window), "Run is enabled for circuit 1");
        assert_eq!(runs_started(&workspace, &window), 2);
        click("stop-button", &mut window);
        wait_until(&mut window, |cx| !running(&workspace, cx));
        assert_eq!(focused_control(&workspace, &mut window), "schematic");

        click("circuit-tab-0", &mut window);
        assert!(running(&workspace, &window), "circuit 0 still runs");
        assert!(paused(&workspace, &window), "and is still paused");
        for shown in run_controls {
            assert!(window.debug_bounds(shown).is_some(), "{shown} is back");
        }
        click("pause-button", &mut window);
        assert!(!paused(&workspace, &window), "resumed");
        assert_eq!(focused_control(&workspace, &mut window), "schematic");
        click("stop-button", &mut window);
        wait_until(&mut window, |cx| !running(&workspace, cx));
        assert_eq!(focused_control(&workspace, &mut window), "schematic");

        click("analysis-settings", &mut window);
        assert!(window.update(|window, cx| window.has_active_dialog(cx)));
        press("escape", &mut window);
        assert!(!window.update(|window, cx| window.has_active_dialog(cx)));
        assert_eq!(focused_control(&workspace, &mut window), "schematic");
        close(&mut window);
    }

    /// Which of the two buttons has the focus.
    fn focused_control(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> &'static str {
        workspace.update_in(cx, |w, window, cx| {
            if w.run_focus.is_focused(window) {
                "run"
            } else if w.settings_focus.is_focused(window) {
                "settings"
            } else if w
                .active_tab()
                .editor
                .focus_handle(cx)
                .contains_focused(window, cx)
            {
                "schematic"
            } else {
                "elsewhere"
            }
        })
    }

    /// D20: the moved buttons keep their keyboard access. Shift+Tab from
    /// the schematic reaches the settings, then Run, the bar's last two
    /// stops; Tab goes back from Run to the settings. Space on Run starts
    /// the run and Enter on the settings opens them.
    #[gpui_kit::test]
    fn run_and_settings_are_reached_and_pressed_from_the_keyboard(cx: &mut TestAppContext) {
        init(cx);
        let env = TestEnv::new();
        let path = env.write_project("long.xsim_proj", &long_run_project());
        let (workspace, mut window) = open(cx, &env, Some(&path));
        assert_eq!(focused_control(&workspace, &mut window), "schematic");

        let mut presses = 0;
        while focused_control(&workspace, &mut window) != "settings" {
            presses += 1;
            assert!(presses <= 6, "Shift+Tab never reaches the settings");
            press("shift-tab", &mut window);
        }
        press("shift-tab", &mut window);
        assert_eq!(focused_control(&workspace, &mut window), "run");
        press("tab", &mut window);
        assert_eq!(focused_control(&workspace, &mut window), "settings");
        press("shift-tab", &mut window);

        press("space", &mut window);
        assert!(running(&workspace, &window), "Space on Run starts the run");
        assert_eq!(runs_started(&workspace, &window), 1);
        window.simulate_keystrokes("shift-f5");
        wait_until(&mut window, |cx| !running(&workspace, cx));

        workspace.update_in(&mut window, |w, window, cx| {
            w.settings_focus.focus(window, cx)
        });
        press("enter", &mut window);
        assert!(window.update(|window, cx| window.has_active_dialog(cx)));
        assert!(window.debug_bounds("analysis-apply").is_some());
        close(&mut window);
    }

    /// The Simulate menu keeps Copy plot with its shortcut, which works
    /// while the schematic has the focus; only the command bar's button
    /// went. (Not pressed here: it would replace the system clipboard.)
    #[gpui_kit::test]
    fn copy_plot_keeps_its_shortcut(cx: &mut TestAppContext) {
        init(cx);
        let env = TestEnv::new();
        let (workspace, mut window) = open(cx, &env, None);
        let keys: Vec<Vec<Keystroke>> = workspace.update_in(&mut window, |w, window, cx| {
            let schematic = w.active_tab().editor.focus_handle(cx);
            window
                .bindings_for_action_in(&CopyPlot, &schematic)
                .iter()
                .map(|binding| {
                    binding
                        .keystrokes()
                        .iter()
                        .map(|k| k.inner().clone())
                        .collect()
                })
                .collect()
        });
        let expected = Keystroke::parse(primary!("shift-c")).expect("a keystroke");
        assert_eq!(keys, [vec![expected]]);
        close(&mut window);
    }
}
