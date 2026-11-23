//! The status bar: run state, current analysis, problem counts and the
//! read-only badge.

use super::Workspace;
use crate::appearance::scale_label;
use crate::diagnostics::Severity;
use crate::simulation::summary::{RunStatus, analysis_summary};
use crate::ui::{dot, severity_icon};
use gpui_kit::component::{
    ActiveTheme, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    status_bar::StatusBar,
    tag::Tag,
};
use gpui_kit::{prelude::*, *};

impl Workspace {
    /// The bar at the bottom of the window.
    pub(super) fn render_status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = self.active_tab();
        let setup = &self.doc.project.circuits[self.active_index()].simulation;
        let theme = cx.theme();
        StatusBar::new()
            .text_xs()
            .text_color(theme.muted_foreground)
            .left(status_text(&tab.status, cx))
            .left(div().child(analysis_summary(setup)))
            .right(self.problem_counts(cx))
            .when(self.doc.read_only, |bar| {
                bar.right(Tag::warning().xsmall().child("Read-only"))
            })
            .right(div().child(scale_label(self.settings.ui_scale)))
    }

    /// Error and warning counts; clicking toggles the dock.
    fn problem_counts(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let counts = self.diagnostics.counts(self.active_tab().id);
        Button::new("status-problems")
            .xsmall()
            .ghost()
            .child(
                h_flex()
                    .gap_1()
                    .child(severity_icon(Severity::Error, cx))
                    .child(counts.errors.to_string())
                    .child(severity_icon(Severity::Warning, cx))
                    .child(counts.warnings.to_string()),
            )
            .tooltip("Show problems")
            .on_click(cx.listener(|this, _, _, cx| this.toggle_diagnostics(cx)))
    }
}

/// The run state with a coloured dot.
fn status_text(status: &RunStatus, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let color = match status {
        RunStatus::Idle => theme.muted_foreground,
        RunStatus::Running { .. } => theme.info,
        RunStatus::Finished { .. } => theme.success,
        RunStatus::Cancelled { .. } => theme.warning,
        RunStatus::Failed => theme.danger,
    };
    h_flex()
        .gap_1p5()
        .items_center()
        .child(dot(color))
        .child(status.label())
}
