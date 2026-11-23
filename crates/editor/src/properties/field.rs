//! The field widget of the properties form: optional label, input with unit
//! suffix, and a message line that shows the parsed value or the
//! validation error. The message keeps at least one line, so a short
//! message never shifts the layout; a long one wraps in narrow fields.

use super::form::FieldStatus;
use gpui_kit::component::{
    ActiveTheme as _, Sizable as _, StyledExt as _,
    input::{Input, InputState},
    v_flex,
};
use gpui_kit::{
    AnyElement, App, Entity, InteractiveElement as _, IntoElement, ParentElement as _, Styled as _,
    div, prelude::FluentBuilder as _, rems,
};

/// Height of one message line in rems.
const MESSAGE_LINE: f32 = 1.;

/// How one field is presented.
pub(super) struct Field {
    /// Label above the input; `None` inside tables with column headers.
    pub label: Option<&'static str>,
    /// Debug selector of the input; the message line gets `-message`, or
    /// `-error` while the field is invalid, the label `-label` and the
    /// unit `-unit`.
    pub selector: String,
    /// Unit symbol after the input (empty for none).
    pub unit: &'static str,
    pub status: FieldStatus,
    pub disabled: bool,
}

/// The rendered parts of a field, for a layout that lines up the inputs of
/// several fields (see [`super::sections::field_grid`]).
pub(super) struct FieldParts {
    pub label: Option<AnyElement>,
    pub input: AnyElement,
    pub message: AnyElement,
}

impl Field {
    pub(super) fn parts(self, input: &Entity<InputState>, cx: &App) -> FieldParts {
        let theme = cx.theme();
        let (message, color, invalid) = match self.status {
            FieldStatus::Valid(text) => (text, theme.muted_foreground, false),
            FieldStatus::Empty(meaning) => (meaning.to_string(), theme.muted_foreground, false),
            FieldStatus::Invalid(error) => (error, theme.danger, true),
        };
        let input_selector = self.selector.clone();
        let label_selector = format!("{}-label", self.selector);
        let unit_selector = format!("{}-unit", self.selector);
        let message_selector = format!(
            "{}-{}",
            self.selector,
            if invalid { "error" } else { "message" }
        );
        let unit = self.unit;
        let input = Input::new(input)
            .small()
            .disabled(self.disabled)
            .when(!unit.is_empty(), |input| {
                input.suffix(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .debug_selector(move || unit_selector)
                        .child(unit),
                )
            })
            .when(invalid, |input| input.border_color(theme.danger));
        FieldParts {
            label: self.label.map(|label| {
                div()
                    .text_xs()
                    .font_medium()
                    .debug_selector(move || label_selector)
                    .child(label)
                    .into_any_element()
            }),
            input: div()
                .min_w_0()
                .debug_selector(move || input_selector)
                .child(input)
                .into_any_element(),
            message: div()
                .min_w_0()
                .min_h(rems(MESSAGE_LINE))
                .line_height(rems(MESSAGE_LINE))
                .text_xs()
                .text_color(color)
                .debug_selector(move || message_selector)
                .child(message)
                .into_any_element(),
        }
    }

    /// The label, input and message stacked in one column.
    pub(super) fn render(self, input: &Entity<InputState>, cx: &App) -> AnyElement {
        let parts = self.parts(input, cx);
        v_flex()
            .min_w_0()
            .gap_1()
            .children(parts.label)
            .child(parts.input)
            .child(parts.message)
            .into_any_element()
    }
}
