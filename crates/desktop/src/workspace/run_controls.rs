//! The run controls at the right end of the circuit tab bar (D20): the
//! out-of-date hint, the progress with Pause/Resume and Stop while the
//! active circuit runs, Run, and the analysis settings. The analysis that
//! Run executes is chosen in the analysis settings; Run names it after its
//! own label (D23), and so does the status bar.

use super::Workspace;
use super::tab_strip::TabReveal;
use crate::actions::*;
use crate::dialogs::analysis::open_analysis_dialog;
use crate::simulation::summary::{analysis_label, format_percent};
use crate::ui::testable;
use gpui_kit::component::{
    ActiveTheme, Disableable, IconName, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    separator::Separator,
    tag::Tag,
};
use gpui_kit::{prelude::*, *};
use xsim_project::AnalysisChoice;

/// Below this window width, in root ems, the controls use their compact
/// wording (see [`run_analysis_text`] and [`stale_hint_text`]). The tab
/// bar spans the window and everything in it scales with the root em, so
/// the width in ems decides what fits. With the longest analysis name and
/// the out-of-date hint, the full wording leaves the active circuit's tab
/// and the + in view down to about 37 em. The compact one leaves the tab in
/// view down to about 27 em, below the smallest window at 200 % (28 em),
/// and the + down to about 29 em. (Measured in the headless tests, whose
/// glyphs are wider than real ones.) The smallest window is compact from
/// 150 % up.
const COMPACT_BELOW_REMS: f32 = 38.;

/// Opacity of the analysis name on an enabled Run button: the button's own
/// text colour, faded, reads as secondary text on it in either theme.
const ANALYSIS_NAME_OPACITY: f32 = 0.7;

impl Workspace {
    /// The controls, as the tab bar's suffix: kept whole, the tabs scroll
    /// in the space left of them.
    pub(super) fn render_run_controls(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let compact = is_compact(window.viewport_size().width, window.rem_size());
        h_flex()
            .debug_selector(|| "run-controls".into())
            .flex_shrink_0()
            .items_center()
            .gap_1()
            .pl_2()
            .pr_1()
            .children(self.stale_hint(compact))
            .children(self.run_progress(cx))
            .child(self.run_button(compact, cx))
            .child(self.settings_button(cx))
    }

    /// The analysis the active circuit's Run executes.
    fn selected_analysis(&self) -> AnalysisChoice {
        self.doc.project.circuits[self.active_index()]
            .simulation
            .selected
    }

    /// Run's width follows its analysis name, which changes with Apply in
    /// the analysis settings, with a switch to a circuit with another
    /// analysis and with the compact wording. The strip keeps its scroll
    /// offset when its room changes, so the active circuit's tab, shown at
    /// the strip's right end, would slide under the controls as Run grows:
    /// it is scrolled back into view on the next frame (the last circuit's
    /// with the + after it, as when it was added). Not on this one, as the
    /// strip scrolls an item into the room it had on the frame before.
    pub(super) fn reveal_active_tab_when_run_resizes(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let compact = is_compact(window.viewport_size().width, window.rem_size());
        let shown = run_analysis_text(self.selected_analysis(), compact);
        let before = self.run_analysis_shown.replace(shown);
        if before.is_some_and(|before| before != shown) {
            let this = cx.weak_entity();
            cx.defer(move |cx| {
                this.update(cx, |this, cx| {
                    this.reveal_active_tab = TabReveal::Pending;
                    cx.notify();
                })
                .ok();
            });
        }
    }

    /// "Run", then the analysis it executes in secondary text (D23), for
    /// example "Run  Transient".
    fn run_button(&self, compact: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let analysis = self.selected_analysis();
        let disabled = self.active_tab().is_running() || self.doc.read_only;
        testable(
            "run-button",
            Button::new("run")
                .track_focus(&self.run_focus)
                .small()
                .primary()
                .icon(IconName::Play)
                .label("Run")
                .child(analysis_name(
                    run_analysis_text(analysis, compact),
                    disabled,
                ))
                .accessibility_label(format!("Run {}", analysis_label(analysis)))
                .disabled(disabled)
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
    fn stale_hint(&self, compact: bool) -> Option<impl IntoElement> {
        let tab = self.active_tab();
        (tab.is_stale() && !tab.is_running()).then(|| {
            testable(
                "stale-results",
                Tag::warning()
                    .small()
                    .child(stale_hint_text(self.doc.read_only, compact)),
            )
            .mr_1()
        })
    }
}

/// Whether a window `width` wide, at root em `rem`, shows the compact
/// wording.
fn is_compact(width: Pixels, rem: Pixels) -> bool {
    width < rem * COMPACT_BELOW_REMS
}

/// The analysis name Run shows after its label: the name the analysis
/// settings use, with "Operating point", the longest, shortened in a
/// compact bar.
fn run_analysis_text(analysis: AnalysisChoice, compact: bool) -> &'static str {
    match analysis {
        AnalysisChoice::OperatingPoint if compact => "Op. point",
        _ => analysis_label(analysis),
    }
}

/// The analysis name inside Run, in secondary text. It inherits the
/// button's text colour, so it follows hover, press and both themes, and
/// fades it while Run is enabled. The disabled colour is faint already and
/// stays as it is, to keep the name readable while the circuit runs.
fn analysis_name(text: &'static str, disabled: bool) -> Div {
    div()
        .debug_selector(move || analysis_name_selector(text))
        .flex_shrink_0()
        .whitespace_nowrap()
        // With the button's gap, as far from "Run" as two spaces.
        .ml(rems(0.25))
        .when(!disabled, |name| name.opacity(ANALYSIS_NAME_OPACITY))
        .child(text)
}

/// The debug selector of Run's analysis name showing `text`.
fn analysis_name_selector(text: &str) -> String {
    format!("run-analysis {text}")
}

/// The out-of-date hint. A read-only project (from a newer xsim, Q45) can
/// show an out-of-date result since INT-1, but cannot run, so the hint does
/// not ask for F5 there; nor does a compact bar, to leave Run room for its
/// analysis name (Run's tooltip still names F5).
pub(super) fn stale_hint_text(read_only: bool, compact: bool) -> &'static str {
    if read_only || compact {
        "Results out of date"
    } else {
        "Results out of date - press F5"
    }
}

#[cfg(test)]
mod tests {
    use super::super::tab_strip::TabReveal;
    use super::Workspace;
    use super::{
        COMPACT_BELOW_REMS, analysis_name, analysis_name_selector, run_analysis_text,
        stale_hint_text,
    };
    use crate::actions::*;
    use crate::appearance::SCALE_PRESETS;
    use crate::simulation::summary::{ANALYSES, analysis_label};
    use crate::test_support::{
        TestEnv, click, close, dialog, has_dialog, init, long_run_project, open, press,
        press_in_dialog, rc_project, wait_until,
    };
    use gpui_kit::{
        Bounds, Entity, Focusable, Keystroke, Modifiers, Pixels, ScaledPixels, Styled,
        TestAppContext, VisualTestContext, px,
    };
    use std::time::Duration;
    use xsim_project::{AnalysisChoice, Part};

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
        assert!(has_dialog(&window));
        assert!(dialog(&window).debug_bounds("analysis-apply").is_some());
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
        assert!(has_dialog(&window));
        press_in_dialog("escape", &mut dialog(&window));
        assert!(!has_dialog(&window));
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
        assert!(has_dialog(&window));
        assert!(dialog(&window).debug_bounds("analysis-apply").is_some());
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

    /// D23: Run names the analysis as the analysis settings' selector does.
    /// A compact bar shortens only "Operating point", and its out-of-date
    /// hint leaves out F5.
    #[test]
    fn run_names_the_analysis_as_the_settings_do() {
        for analysis in ANALYSES {
            assert_eq!(run_analysis_text(analysis, false), analysis_label(analysis));
        }
        assert_eq!(
            ANALYSES.map(|analysis| run_analysis_text(analysis, false)),
            ["Operating point", "DC sweep", "AC sweep", "Transient"]
        );
        assert_eq!(
            ANALYSES.map(|analysis| run_analysis_text(analysis, true)),
            ["Op. point", "DC sweep", "AC sweep", "Transient"]
        );
        assert_eq!(
            stale_hint_text(false, false),
            "Results out of date - press F5"
        );
        for read_only in [false, true] {
            assert_eq!(stale_hint_text(read_only, true), "Results out of date");
        }
    }

    /// The name is secondary text in the button's own colour, so it
    /// follows the theme and the button's hover and press: faded while Run
    /// is enabled, and in the disabled colour as it is while Run is
    /// disabled, as that colour is faint already.
    #[test]
    fn the_analysis_name_is_secondary_text_in_the_buttons_colour() {
        let mut enabled = analysis_name("Transient", false);
        let opacity = enabled.style().opacity;
        assert!(
            opacity.is_some_and(|o| o > 0.5 && o < 1.),
            "faded while enabled: {opacity:?}"
        );
        assert_eq!(enabled.style().text.color, None, "no colour of its own");
        let mut disabled = analysis_name("Transient", true);
        assert_eq!(disabled.style().opacity, None, "not faded while disabled");
        assert_eq!(disabled.style().text.color, None, "no colour of its own");
    }

    /// Every name Run can show.
    fn every_name() -> Vec<&'static str> {
        let mut names: Vec<_> = ANALYSES
            .into_iter()
            .flat_map(|analysis| [false, true].map(|compact| run_analysis_text(analysis, compact)))
            .collect();
        names.dedup();
        names
    }

    /// Where Run's analysis name shows `text`, if it does.
    fn analysis_name_bounds(text: &str, cx: &mut VisualTestContext) -> Option<Bounds<Pixels>> {
        // The test context looks selectors up by `&'static str`.
        let selector: &'static str = analysis_name_selector(text).leak();
        cx.debug_bounds(selector)
    }

    /// Run shows `text`, and no other name, after its label: the name has
    /// the width of `text` in the headless text system (0.6 em a character
    /// at the button's 0.875 rem; layout rounds it to whole pixels), lies
    /// right of "Run" and ends the button, which grew to hold it.
    fn assert_run_names(text: &str, cx: &mut VisualTestContext) {
        let name =
            analysis_name_bounds(text, cx).unwrap_or_else(|| panic!("Run does not show {text:?}"));
        let (rem, run) = (rem(cx), bounds("run-button", cx));
        let text_width = |text: &str| rem * 0.875 * 0.6 * text.len() as f32;
        let within_a_pixel = |actual: Pixels, expected: Pixels, what: &str| {
            assert!(
                (actual - expected).abs() <= px(1.),
                "{text:?}: {what} {actual:?}, expected {expected:?}"
            );
        };
        within_a_pixel(name.size.width, text_width(text), "width");
        // The padding, "Run" and the two gaps before the name, at least.
        assert!(
            name.left() >= run.left() + rem + text_width("Run"),
            "{text:?} at {name:?} after the label of Run {run:?}"
        );
        within_a_pixel(
            name.right(),
            run.right() - rem * 0.5,
            "end, before the padding",
        );
        assert_near(
            name.center().y,
            run.center().y,
            &format!("{text:?} on the line of Run"),
        );
        for other in every_name().into_iter().filter(|&other| other != text) {
            assert!(
                analysis_name_bounds(other, cx).is_none(),
                "Run shows {other:?} too"
            );
        }
    }

    /// Selects `analysis` for the active circuit, as Apply in the analysis
    /// settings does.
    fn select_analysis(
        analysis: AnalysisChoice,
        workspace: &Entity<Workspace>,
        cx: &mut VisualTestContext,
    ) {
        workspace.update(cx, |w, cx| {
            let index = w.active_index();
            w.doc.project.circuits[index].simulation.selected = analysis;
            cx.notify();
        });
        cx.run_until_parked();
    }

    /// Selects `analysis` for every circuit.
    fn select_everywhere(
        analysis: AnalysisChoice,
        workspace: &Entity<Workspace>,
        cx: &mut VisualTestContext,
    ) {
        workspace.update(cx, |w, cx| {
            for circuit in &mut w.doc.project.circuits {
                circuit.simulation.selected = analysis;
            }
            cx.notify();
        });
        cx.run_until_parked();
    }

    /// D23: Run shows the active circuit's analysis after "Run", for each
    /// analysis, with the controls laid out as before; also while it is
    /// disabled, during a run and in a read-only project.
    #[gpui_kit::test]
    fn run_shows_the_selected_analysis(cx: &mut TestAppContext) {
        init(cx);
        let env = TestEnv::new();
        let path = env.write_project("long.xsim_proj", &long_run_project());
        let (workspace, mut window) = open(cx, &env, Some(&path));
        assert_run_names("Transient", &mut window);
        for analysis in ANALYSES {
            select_analysis(analysis, &workspace, &mut window);
            let name = analysis_label(analysis);
            assert_run_names(name, &mut window);
            assert_idle_controls(&mut window, name);
        }

        select_analysis(AnalysisChoice::Transient, &workspace, &mut window);
        window.simulate_keystrokes("f5");
        assert!(running(&workspace, &window));
        assert_run_names("Transient", &mut window);
        window.simulate_keystrokes("shift-f5");
        wait_until(&mut window, |cx| !running(&workspace, cx));

        workspace.update(&mut window, |w, cx| {
            w.doc.read_only = true;
            cx.notify();
        });
        window.run_until_parked();
        assert_run_names("Transient", &mut window);
        close(&mut window);
    }

    /// The name follows the analysis: at once when the analysis settings
    /// apply another one (not before), and per circuit when switching
    /// circuits.
    #[gpui_kit::test]
    fn the_analysis_name_follows_apply_and_the_active_circuit(cx: &mut TestAppContext) {
        init(cx);
        let env = TestEnv::new();
        let mut project = rc_project();
        let mut second = project.circuits[0].clone();
        second.name = "Second".into();
        second.simulation.selected = AnalysisChoice::DcSweep;
        project.circuits.push(second);
        let path = env.write_project("two.xsim_proj", &project);
        let (_, mut window) = open(cx, &env, Some(&path));
        assert_run_names("Transient", &mut window);

        click("analysis-settings", &mut window);
        let mut settings = dialog(&window);
        click("analysis-kind-ac", &mut settings);
        assert_run_names("Transient", &mut window);
        click("analysis-apply", &mut settings);
        assert!(!has_dialog(&window));
        assert_run_names("AC sweep", &mut window);

        click("circuit-tab-1", &mut window);
        assert_run_names("DC sweep", &mut window);
        click("analysis-settings", &mut window);
        let mut settings = dialog(&window);
        click("analysis-kind-op", &mut settings);
        click("analysis-apply", &mut settings);
        assert_run_names("Operating point", &mut window);

        click("circuit-tab-0", &mut window);
        assert_run_names("AC sweep", &mut window);
        click("circuit-tab-1", &mut window);
        assert_run_names("Operating point", &mut window);
        close(&mut window);
    }

    /// When Run grows, the active circuit's tab at the strip's right end
    /// stays whole in view: after a click on a circuit with a longer
    /// analysis name, and after Apply selects a longer one.
    #[gpui_kit::test]
    fn the_active_tab_stays_in_view_as_run_grows(cx: &mut TestAppContext) {
        init(cx);
        let env = TestEnv::new();
        let path = env.write_project("long.xsim_proj", &long_run_project());
        let (workspace, mut window) = open(cx, &env, Some(&path));
        for _ in 0..11 {
            window.dispatch_action(NewCircuit);
        }
        window.run_until_parked();
        select_everywhere(AnalysisChoice::Ac, &workspace, &mut window);
        // The strip scrolls: the new last circuit and the + are in view.
        assert!(
            bounds("circuit-tab-0", &mut window).left()
                < bounds("circuit-tabs", &mut window).left()
        );
        assert_before_controls("add-circuit", "a new circuit", &mut window);

        click("circuit-tab-10", &mut window);
        workspace.update(&mut window, |w, cx| {
            w.doc.project.circuits[11].simulation.selected = AnalysisChoice::OperatingPoint;
            cx.notify();
        });
        click("circuit-tab-11", &mut window);
        assert_run_names("Operating point", &mut window);
        assert_before_controls("circuit-tab-11", "after the click", &mut window);

        click("circuit-tab-10", &mut window);
        assert_run_names("AC sweep", &mut window);
        click("analysis-settings", &mut window);
        let mut settings = dialog(&window);
        click("analysis-kind-op", &mut settings);
        click("analysis-apply", &mut settings);
        assert_run_names("Operating point", &mut window);
        assert_before_controls("circuit-tab-10", "after Apply", &mut window);
        close(&mut window);
    }

    /// Shows circuit `index` with its tab scrolled into view, as a click on
    /// it does: the last circuit's with the + after it, as far as the room
    /// allows; the tab wins where both do not fit.
    fn show_circuit(index: usize, workspace: &Entity<Workspace>, cx: &mut VisualTestContext) {
        workspace.update_in(cx, |w, window, cx| {
            w.switch_to(index, window, cx);
            if index + 1 == w.tabs.len() {
                w.tab_scroll.scroll_to_item(w.tabs.len());
            }
        });
        cx.run_until_parked();
        workspace.update(cx, |w, cx| {
            w.tab_scroll.scroll_to_item(index);
            cx.notify();
        });
        cx.run_until_parked();
    }

    /// `shown` ends before the run controls, and starts in the bar.
    fn assert_before_controls(shown: &'static str, what: &str, cx: &mut VisualTestContext) {
        let (bar, controls, shown_bounds) = (
            bounds("circuit-tabs", cx),
            bounds("run-controls", cx),
            bounds(shown, cx),
        );
        assert!(
            shown_bounds.left() >= bar.left() && shown_bounds.right() <= controls.left(),
            "{what}: {shown} {shown_bounds:?} in view before the controls {controls:?}"
        );
    }

    /// D23 at every interface scale, in the smallest window and in the
    /// narrowest one with the full wording, with five circuits that all
    /// show an out-of-date result. For each analysis, Run holds its name
    /// and every control lies in the bar with the active circuit's tab in
    /// view before them; with the longest name, also for the last circuit,
    /// and the + after it in a window at least 29 em wide. The compact
    /// wording applies exactly below the threshold, and the hint then
    /// leaves out " - press F5". While a circuit runs, its longest name
    /// fits as well.
    #[gpui_kit::test]
    fn the_analysis_name_fits_the_bar_at_every_scale(cx: &mut TestAppContext) {
        init(cx);
        let env = TestEnv::new();
        let path = env.write_project("long.xsim_proj", &long_run_project());
        let (workspace, mut window) = open(cx, &env, Some(&path));
        for _ in 0..4 {
            window.dispatch_action(NewCircuit);
        }
        window.run_until_parked();
        let (min_width, height) = crate::MIN_WINDOW_SIZE;
        let idle = ["stale-results", "run-button", "analysis-settings"];
        let running_controls = [
            "run-progress",
            "pause-button",
            "stop-button",
            "run-button",
            "analysis-settings",
        ];
        let mut compact_seen = [false; 2];
        for scale in SCALE_PRESETS {
            workspace.update_in(&mut window, |w, window, cx| {
                w.set_ui_scale(scale, window, cx);
                for tab in &mut w.tabs {
                    tab.result_revision = Some(super::super::circuit_tab::OUTDATED);
                }
            });
            window.run_until_parked();
            let rem = rem(&mut window);
            let threshold = f32::from(rem) * COMPACT_BELOW_REMS;
            let full_from = threshold.ceil().max(min_width);
            let mut hint_widths = Vec::new();
            for width in [min_width, full_from] {
                window.simulate_resize(gpui_kit::size(px(width), px(height)));
                window.run_until_parked();
                let compact = width < threshold;
                compact_seen[usize::from(compact)] = true;
                let what = format!("{width} px at {scale}");
                show_circuit(0, &workspace, &mut window);
                for analysis in ANALYSES {
                    select_analysis(analysis, &workspace, &mut window);
                    assert_run_names(run_analysis_text(analysis, compact), &mut window);
                    assert_bar_fits(&idle, px(width), &mut window);
                }
                hint_widths.push(bounds("stale-results", &mut window).size.width);

                select_everywhere(AnalysisChoice::OperatingPoint, &workspace, &mut window);
                show_circuit(4, &workspace, &mut window);
                assert_run_names(
                    run_analysis_text(AnalysisChoice::OperatingPoint, compact),
                    &mut window,
                );
                let bar = bounds("circuit-tabs", &mut window);
                for name in idle {
                    assert_in_bar(bounds(name, &mut window), bar, name);
                }
                assert_before_controls("circuit-tab-4", &what, &mut window);
                // Only the smallest window at 200 % (28 em) is too narrow for
                // the + after the last tab, which scrolls on into view.
                if width >= f32::from(rem) * 29. {
                    assert_before_controls("add-circuit", &what, &mut window);
                }
            }
            if full_from > min_width {
                // " - press F5" in the headless text system, at the small
                // tag's 0.625 rem.
                let left_out = rem * 0.625 * 0.6 * " - press F5".len() as f32;
                assert_near(
                    hint_widths[1] - hint_widths[0],
                    left_out,
                    &format!("the compact hint at {scale}"),
                );
            } else {
                assert_eq!(hint_widths[0], hint_widths[1]);
            }

            // A run of the first circuit, paused while its longest name
            // shows in the smallest window.
            window.simulate_resize(gpui_kit::size(px(min_width), px(height)));
            show_circuit(0, &workspace, &mut window);
            select_analysis(AnalysisChoice::Transient, &workspace, &mut window);
            window.simulate_keystrokes("f5");
            assert!(running(&workspace, &window));
            click("pause-button", &mut window);
            select_analysis(AnalysisChoice::OperatingPoint, &workspace, &mut window);
            let compact = min_width < threshold;
            assert_run_names(
                run_analysis_text(AnalysisChoice::OperatingPoint, compact),
                &mut window,
            );
            assert_bar_fits(&running_controls, px(min_width), &mut window);
            window.simulate_keystrokes("shift-f5");
            wait_until(&mut window, |cx| !running(&workspace, cx));
        }
        assert_eq!(compact_seen, [true, true], "both wordings were checked");
        close(&mut window);
    }

    /// D23 keeps Run's shortcut and tooltip: F5 runs from the schematic,
    /// and hovering Run shows its tooltip below it, wide enough for "Run
    /// simulation" (headless glyphs, small text) with the shortcut after
    /// it.
    #[gpui_kit::test]
    fn run_keeps_its_shortcut_and_tooltip(cx: &mut TestAppContext) {
        init(cx);
        let env = TestEnv::new();
        let (workspace, mut window) = open(cx, &env, None);
        let keys: Vec<Vec<Keystroke>> = workspace.update_in(&mut window, |w, window, cx| {
            let schematic = w.active_tab().editor.focus_handle(cx);
            window
                .bindings_for_action_in(&RunSimulation, &schematic)
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
        assert_eq!(keys, [vec![Keystroke::parse("f5").expect("a keystroke")]]);

        let (rem, run) = (rem(&mut window), bounds("run-button", &mut window));
        let painted = |window: &mut VisualTestContext| {
            window.update(|window, _| {
                let scale = window.scale_factor();
                window
                    .painted_quads()
                    .into_iter()
                    .filter(|quad| quad.border_widths.top > ScaledPixels::from(0.))
                    .map(|quad| quad.bounds.map(|v| px(v.0 / scale)))
                    .collect::<Vec<_>>()
            })
        };
        let before = painted(&mut window);
        window.simulate_mouse_move(run.center(), None, Modifiers::default());
        window.run_until_parked();
        window.executor().advance_clock(Duration::from_secs(2));
        window.run_until_parked();
        let tooltip = painted(&mut window)
            .into_iter()
            .find(|quad| {
                !before.contains(quad)
                    && quad.top() >= run.bottom()
                    && quad.size.height < rem * 2.
                    && quad.left() < run.right()
                    && quad.right() > run.left()
            })
            .unwrap_or_else(|| panic!("no tooltip below Run {run:?}"));
        // The tooltip's padding, "Run simulation" and the gap before the
        // shortcut, at the least.
        let text = rem * 0.875 * 0.6 * "Run simulation".len() as f32;
        assert!(
            tooltip.size.width > rem * (2. * 0.5 + 0.75) + text + px(2.),
            "the tooltip {tooltip:?} holds the text and the shortcut"
        );
        close(&mut window);
    }

    /// When a new circuit makes Run wider (its transient after an AC sweep),
    /// the new circuit's tab and the + after it stay whole in view, as
    /// before D23: with the +, with Ctrl+T and from the menu's action, at
    /// 100 % and 200 %.
    #[gpui_kit::test]
    fn a_new_circuit_keeps_the_plus_in_view_as_run_grows(cx: &mut TestAppContext) {
        init(cx);
        let env = TestEnv::new();
        let (workspace, mut window) = open(cx, &env, None);
        for _ in 0..29 {
            window.dispatch_action(NewCircuit);
        }
        window.run_until_parked();
        for scale in [1.0, 2.0] {
            workspace.update_in(&mut window, |w, window, cx| {
                w.set_ui_scale(scale, window, cx)
            });
            window.run_until_parked();
            for how in ["the +", "Ctrl+T", "the action"] {
                select_everywhere(AnalysisChoice::Ac, &workspace, &mut window);
                assert_run_names("AC sweep", &mut window);
                match how {
                    "the +" => click("add-circuit", &mut window),
                    "Ctrl+T" => press(primary!("t"), &mut window),
                    _ => {
                        window.dispatch_action(NewCircuit);
                        window.run_until_parked();
                    }
                }
                assert_run_names("Transient", &mut window);
                let last = workspace.read_with(&window, |w, _| w.tabs.len() - 1);
                let tab: &'static str = format!("circuit-tab-{last}").leak();
                let what = format!("a circuit added with {how} at {scale}");
                assert_before_controls(tab, &what, &mut window);
                assert_before_controls("add-circuit", &what, &mut window);
                assert_revealed(&workspace, &what, &window);
            }
        }
        close(&mut window);
    }

    /// The reveal of the active tab has run to its end.
    fn assert_revealed(workspace: &Entity<Workspace>, what: &str, cx: &VisualTestContext) {
        let reveal = workspace.read_with(cx, |w, _| w.reveal_active_tab);
        assert_eq!(reveal, TabReveal::Done, "{what}: the reveal finished");
    }

    /// Where the strip cannot hold the last tab and the + after it, the tab
    /// wins: in the smallest window at 200 % with the out-of-date hint, the
    /// last circuit's Run grows from "AC sweep" to "Op. point" on Apply, and
    /// its tab stays whole in view.
    #[gpui_kit::test]
    fn the_last_tab_wins_over_the_plus_as_run_grows(cx: &mut TestAppContext) {
        init(cx);
        let env = TestEnv::new();
        let path = env.write_project("long.xsim_proj", &long_run_project());
        let (workspace, mut window) = open(cx, &env, Some(&path));
        for _ in 0..4 {
            window.dispatch_action(NewCircuit);
        }
        window.run_until_parked();
        let (width, height) = crate::MIN_WINDOW_SIZE;
        workspace.update_in(&mut window, |w, window, cx| {
            w.set_ui_scale(2.0, window, cx);
            for tab in &mut w.tabs {
                tab.result_revision = Some(super::super::circuit_tab::OUTDATED);
            }
        });
        window.simulate_resize(gpui_kit::size(px(width), px(height)));
        window.run_until_parked();
        select_everywhere(AnalysisChoice::Ac, &workspace, &mut window);
        show_circuit(4, &workspace, &mut window);
        assert_before_controls("add-circuit", "with AC sweep", &mut window);

        click("analysis-settings", &mut window);
        let mut settings = dialog(&window);
        click("analysis-kind-op", &mut settings);
        click("analysis-apply", &mut settings);
        assert_run_names("Op. point", &mut window);
        assert_before_controls("circuit-tab-4", "with Op. point", &mut window);
        let (tab, plus, controls) = (
            bounds("circuit-tab-4", &mut window),
            bounds("add-circuit", &mut window),
            bounds("run-controls", &mut window),
        );
        assert_eq!(
            tab.left(),
            bounds("circuit-tabs", &mut window).left(),
            "the tab starts the strip"
        );
        assert!(
            plus.right() > controls.left(),
            "the + does not fit after it here: {plus:?}, {controls:?}"
        );
        assert_revealed(&workspace, "after Apply", &window);
        close(&mut window);
    }

    /// Many circuits with the out-of-date hint, in the smallest window at
    /// every interface scale: for each analysis, with the first, a middle
    /// and the last circuit shown, Run holds its name, every control lies
    /// in the bar and the shown circuit's tab is whole in view before them.
    #[gpui_kit::test]
    fn many_circuits_and_the_hint_fit_the_smallest_window(cx: &mut TestAppContext) {
        init(cx);
        let env = TestEnv::new();
        let path = env.write_project("long.xsim_proj", &long_run_project());
        let (workspace, mut window) = open(cx, &env, Some(&path));
        for _ in 0..29 {
            window.dispatch_action(NewCircuit);
        }
        window.run_until_parked();
        let (width, height) = crate::MIN_WINDOW_SIZE;
        window.simulate_resize(gpui_kit::size(px(width), px(height)));
        let idle = ["stale-results", "run-button", "analysis-settings"];
        for scale in SCALE_PRESETS {
            workspace.update_in(&mut window, |w, window, cx| {
                w.set_ui_scale(scale, window, cx);
                for tab in &mut w.tabs {
                    tab.result_revision = Some(super::super::circuit_tab::OUTDATED);
                }
            });
            window.run_until_parked();
            let compact = width < f32::from(rem(&mut window)) * COMPACT_BELOW_REMS;
            for analysis in ANALYSES {
                select_everywhere(analysis, &workspace, &mut window);
                for index in [0, 15, 29] {
                    show_circuit(index, &workspace, &mut window);
                    assert_run_names(run_analysis_text(analysis, compact), &mut window);
                    let bar = bounds("circuit-tabs", &mut window);
                    for name in idle {
                        assert_in_bar(bounds(name, &mut window), bar, name);
                    }
                    let tab: &'static str = format!("circuit-tab-{index}").leak();
                    let what = format!("{analysis:?}, circuit {index} at {scale}");
                    assert_before_controls(tab, &what, &mut window);
                }
            }
        }
        close(&mut window);
    }
}
