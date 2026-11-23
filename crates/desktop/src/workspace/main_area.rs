//! The main area (Q22): the schematic fills the window until the circuit
//! has results; then schematic and waveforms share a resizable split.

use super::Workspace;
use super::circuit_tab::ActiveRun;
use crate::simulation::summary::{analysis_label, format_percent};
use crate::ui::{spinner::LoaderSpinner, split::horizontal_split, testable};
use gpui_kit::component::{
    ActiveTheme, IconName, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    progress::Progress,
    v_flex,
};
use gpui_kit::{prelude::*, *};

impl Workspace {
    /// Schematic alone, or schematic and results side by side.
    pub(super) fn render_main_area(&self, cx: &mut Context<Self>) -> AnyElement {
        let container = div().flex_1().min_h_0().w_full();
        let schematic = self.render_schematic_frame(cx);
        if !self.active_tab().shows_results() {
            return container.child(schematic).into_any_element();
        }
        let this = cx.weak_entity();
        container
            .child(horizontal_split(
                "main-split",
                self.doc.project.layout.schematic_fraction,
                schematic,
                self.render_results_side(cx),
                move |fraction, _, cx| {
                    this.update(cx, |this, cx| this.set_schematic_fraction(fraction, cx))
                        .ok();
                },
            ))
            .into_any_element()
    }

    /// The editor with the diagnostics dock underneath.
    fn render_schematic_frame(&self, cx: &mut Context<Self>) -> Div {
        v_flex()
            .size_full()
            .child(
                testable("schematic-frame", self.active_tab().editor.clone())
                    .flex_1()
                    .min_h_0()
                    .w_full(),
            )
            .when(self.doc.project.layout.diagnostics_open, |frame| {
                frame.child(self.render_diagnostics_dock(cx))
            })
    }

    /// The viewer, covered by the loader while a run is in progress (Q139:
    /// progress only, no live traces).
    fn render_results_side(&self, cx: &mut Context<Self>) -> Div {
        let tab = self.active_tab();
        div()
            .relative()
            .size_full()
            .border_l_1()
            .border_color(cx.theme().border)
            .child(testable("waveform-viewer", tab.viewer.clone()).size_full())
            .when_some(tab.run.as_ref(), |side, run| {
                side.child(self.render_loader(run, cx))
            })
    }

    /// Centred card with spinner, percentage, progress bar, Pause and Stop.
    fn render_loader(&self, active: &ActiveRun, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let progress = active.run.progress();
        let paused = active.run.is_paused();
        let analysis = self.doc.project.circuits[self.active_index()]
            .simulation
            .selected;
        let title = if paused {
            "Simulation paused".to_string()
        } else {
            format!("Simulating {}", analysis_label(analysis).to_lowercase())
        };
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme.background.opacity(0.82))
            .child(
                v_flex()
                    .w(rems(18.))
                    .p_4()
                    .gap_3()
                    .rounded(theme.radius_lg)
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.popover)
                    .shadow_lg()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .when(!paused, |row| {
                                row.child(testable(
                                    "run-spinner",
                                    LoaderSpinner::new("run-spinner"),
                                ))
                            })
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(title))
                            .child(div().flex_1())
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme.muted_foreground)
                                    .child(format_percent(progress)),
                            ),
                    )
                    .child(Progress::new("run-progress").value((progress * 100.0) as f32))
                    .child(self.loader_buttons(paused, cx)),
            )
    }

    fn loader_buttons(&self, paused: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let (icon, label) = if paused {
            (IconName::Play, "Resume")
        } else {
            (IconName::Pause, "Pause")
        };
        h_flex()
            .gap_2()
            .justify_end()
            .child(
                Button::new("loader-pause")
                    .small()
                    .outline()
                    .icon(icon)
                    .label(label)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_pause(cx))),
            )
            .child(
                Button::new("loader-stop")
                    .small()
                    .danger()
                    .label("Stop")
                    .on_click(cx.listener(|this, _, _, cx| this.stop_simulation(cx))),
            )
    }
}
