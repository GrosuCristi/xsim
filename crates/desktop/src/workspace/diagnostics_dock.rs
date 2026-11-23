//! The closable dock under the schematic (Q22) listing warnings and errors
//! with their severity, message and source.

use super::Workspace;
use crate::actions::*;
use crate::diagnostics::{Counts, Diagnostic};
use crate::ui::{hint, severity_icon};
use gpui_kit::component::{
    ActiveTheme, Disableable, IconName, Sizable,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
};
use gpui_kit::{prelude::*, *};

impl Workspace {
    /// The dock with the problems of the project and the active circuit.
    pub(super) fn render_diagnostics_dock(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.active_tab().id;
        let entries: Vec<Diagnostic> = self
            .diagnostics
            .visible(active)
            .into_iter()
            .cloned()
            .collect();
        let counts = self.diagnostics.counts(active);
        let theme = cx.theme();
        v_flex()
            .id("diagnostics-dock")
            .debug_selector(|| "diagnostics-dock".into())
            .h(rems(11.))
            .flex_shrink_0()
            .border_t_1()
            .border_color(theme.border)
            .bg(theme.sidebar)
            .child(self.dock_header(counts, cx))
            .child(self.dock_body(&entries, cx))
    }

    fn dock_header(&self, counts: Counts, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        h_flex()
            .h(rems(2.))
            .px_2()
            .gap_2()
            .flex_shrink_0()
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("PROBLEMS"),
            )
            .child(hint(counts_text(counts), cx))
            .child(div().flex_1())
            .child(
                Button::new("diagnostics-clear")
                    .xsmall()
                    .ghost()
                    .label("Clear")
                    .disabled(counts.total() == 0)
                    .on_click(cx.listener(|this, _, _, cx| {
                        let active = this.active_tab().id;
                        this.diagnostics.clear_visible(active);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("diagnostics-close")
                    .xsmall()
                    .ghost()
                    .icon(IconName::Close)
                    .tooltip_with_action(
                        "Hide diagnostics",
                        &ToggleDiagnostics,
                        Some(WORKSPACE_CONTEXT),
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_diagnostics(cx))),
            )
    }

    fn dock_body(&self, entries: &[Diagnostic], cx: &mut Context<Self>) -> impl IntoElement {
        let body = v_flex()
            .id("diagnostics-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll();
        if entries.is_empty() {
            return body.p_3().child(hint(
                "No problems. Netlist errors, simulation failures and file warnings appear here.",
                cx,
            ));
        }
        body.children(entries.iter().map(|entry| self.dock_row(entry, cx)))
    }

    fn dock_row(&self, entry: &Diagnostic, cx: &App) -> impl IntoElement {
        let theme = cx.theme();
        let circuit = entry
            .circuit
            .and_then(|id| self.tab_index(id))
            .map(|index| self.doc.project.circuits[index].name.clone());
        let source = match circuit {
            Some(circuit) => format!("{} · {circuit}", entry.source.label()),
            None => entry.source.label().to_string(),
        };
        h_flex()
            .px_3()
            .py_1()
            .gap_2()
            .items_start()
            .border_b_1()
            .border_color(theme.border.opacity(0.5))
            .hover(|row| row.bg(theme.list_hover))
            .child(div().pt(px(2.)).child(severity_icon(entry.severity, cx)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .text_color(theme.foreground)
                    .child(entry.message.clone()),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(source),
            )
    }
}

/// `"1 error, 2 warnings"`.
pub(super) fn counts_text(counts: Counts) -> String {
    let plural = |n: usize, word: &str| {
        if n == 1 {
            format!("1 {word}")
        } else {
            format!("{n} {word}s")
        }
    };
    let mut parts = vec![
        plural(counts.errors, "error"),
        plural(counts.warnings, "warning"),
    ];
    if counts.infos > 0 {
        parts.push(plural(counts.infos, "message"));
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::counts_text;
    use crate::diagnostics::Counts;

    #[test]
    fn counts_read_naturally() {
        let counts = Counts {
            errors: 1,
            warnings: 2,
            infos: 0,
        };
        assert_eq!(counts_text(counts), "1 error, 2 warnings");
        assert_eq!(
            counts_text(Counts {
                infos: 1,
                ..Counts::default()
            }),
            "0 errors, 0 warnings, 1 message"
        );
    }
}
