//! Small building blocks shared by the workspace and its dialogs. Colours
//! always come from the active theme (Q21, Q25).

pub mod spinner;
pub mod split;

use crate::actions::PRESSABLE_CONTEXT;
use crate::diagnostics::Severity;
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable, button::Button, h_flex, v_flex};
use gpui_kit::{prelude::*, *};

/// Wraps `element` so headless tests can find it with
/// `VisualTestContext::debug_bounds(selector)`. A no-op in release builds.
pub fn testable(selector: &'static str, element: impl IntoElement) -> Div {
    div().debug_selector(|| selector.to_string()).child(element)
}

/// Lets Enter press `button` when it has the focus, as Space does, instead
/// of confirming the dialog it sits in.
pub fn pressable(button: Button) -> Button {
    button.key_context(PRESSABLE_CONTEXT)
}

/// A small round status dot.
pub fn dot(color: Hsla) -> Div {
    div()
        .size(rems(0.45))
        .flex_shrink_0()
        .rounded_full()
        .bg(color)
}

/// The icon for a diagnostic severity, in its theme colour.
pub fn severity_icon(severity: Severity, cx: &App) -> Icon {
    let theme = cx.theme();
    let (name, color) = match severity {
        Severity::Error => (IconName::CircleX, theme.danger),
        Severity::Warning => (IconName::TriangleAlert, theme.warning),
        Severity::Info => (IconName::Info, theme.info),
    };
    Icon::new(name).small().text_color(color)
}

/// A labelled form row: label on the left, control and unit on the right,
/// an inline validation message underneath.
pub fn form_row(
    label: &'static str,
    control: impl IntoElement,
    unit: &'static str,
    error: Option<&str>,
    cx: &App,
) -> Div {
    let theme = cx.theme();
    v_flex()
        .gap_1()
        .child(
            h_flex()
                .gap_3()
                .items_center()
                .child(
                    div()
                        .w(rems(9.))
                        .flex_shrink_0()
                        .text_sm()
                        .text_color(theme.foreground)
                        .child(label),
                )
                .child(div().flex_1().min_w_0().child(control))
                .child(
                    div()
                        .w(rems(2.5))
                        .flex_shrink_0()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(unit),
                ),
        )
        .when_some(error, |row, error| {
            row.child(
                div()
                    .pl(rems(9.75))
                    .text_xs()
                    .text_color(theme.danger)
                    .child(error.to_string()),
            )
        })
}

/// A small heading inside a dialog or panel.
pub fn section_title(title: &'static str, cx: &App) -> Div {
    div()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(cx.theme().muted_foreground)
        .child(title.to_uppercase())
}

/// Secondary explanatory text.
pub fn hint(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}
