//! The transient section of a source: waveform type selector, parameter
//! fields or the PWL point table, and the live preview.

use super::preview;
use super::sections::{compact_width, section};
use super::sparkline::sparkline;
use super::spec::FieldSpec;
use super::view::PropertiesView;
use super::waveform::WaveKind;
use crate::library_browser::pressable;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IconName, Selectable as _, Sizable as _, StyledExt as _,
    button::{Button, ButtonGroup, ButtonVariants as _},
    checkbox::Checkbox,
    h_flex, v_flex,
};
use gpui_kit::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement as _, Styled as _,
    div, prelude::FluentBuilder as _, rems,
};
use std::rc::Rc;

/// Sizes in rems, so they follow the UI scale. The preview takes the room
/// beside the fields, which is this wide in a dialog of the form's width.
const PREVIEW_WIDTH: f32 = 15.;
/// Height of the chart, which spans exactly the curve's value range.
const CHART_HEIGHT: f32 = 4.75;
/// Line height of the preview's title and axis labels. The value labels
/// stay within the chart's height: the largest one's top on the chart's
/// top edge, the smallest one's bottom on its bottom edge, so nothing
/// stands out above the curve to push it away from the title (D20).
const LABEL_LINE: f32 = 1.;
/// Gap between the preview's title, plot and time labels, and between the
/// value labels and the chart.
const PREVIEW_GAP: f32 = 0.25;
/// Gap between the waveform's fields and the preview.
const SIDE_GAP: f32 = 1.;
const ROW_ACTION_WIDTH: f32 = 1.75;

/// Width in rems of a source's form, set by its widest row: two compact
/// field columns beside the preview.
pub(super) const SOURCE_FORM_WIDTH: f32 = compact_width(2) + SIDE_GAP + PREVIEW_WIDTH;

impl PropertiesView {
    pub(super) fn transient_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let kind = self.form.wave_kind();
        let body = match kind {
            WaveKind::None => None,
            WaveKind::Pwl => Some(self.pwl_table(cx)),
            _ => Some(
                self.labelled_fields(2, &self.form.wave_fields(), cx)
                    .into_any_element(),
            ),
        };
        section("Transient", cx)
            .child(self.wave_selector(cx))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(kind.description()),
            )
            .when_some(body, |section, body| {
                section.child(
                    h_flex()
                        .items_start()
                        .gap(rems(SIDE_GAP))
                        .child(
                            div()
                                .flex_none()
                                .w(rems(compact_width(2)))
                                .track_focus(&self.wave_body)
                                .child(body),
                        )
                        .child(self.preview(cx)),
                )
            })
            .into_any_element()
    }

    /// The waveform types as one row of joined buttons. Each button handles
    /// its own click: the group's callback never sees Enter or Space on a
    /// focused button, and Enter presses the focused button instead of
    /// applying the dialog.
    fn wave_selector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.form.wave_kind();
        let buttons = WaveKind::ALL.into_iter().map(|kind| {
            pressable(Button::new(kind.label()))
                .label(kind.label())
                .selected(kind == current)
                .disabled(self.read_only)
                .debug_selector(move || format!("waveform-{}", kind.label()))
                .on_click(
                    cx.listener(move |this, _, window, cx| this.set_wave_kind(kind, window, cx)),
                )
        });
        ButtonGroup::new("waveform-kind")
            .small()
            .outline()
            .children(buttons)
    }

    /// Editable `(time, value)` rows with add/remove and the repeat option.
    fn pwl_table(&self, cx: &mut Context<Self>) -> AnyElement {
        let header = h_flex()
            .gap_2()
            .text_xs()
            .font_medium()
            .child(div().flex_1().child("Time"))
            .child(div().flex_1().child("Value"))
            .child(div().w(rems(ROW_ACTION_WIDTH)));
        let rows: Vec<AnyElement> = self
            .form
            .pwl_rows()
            .iter()
            .zip(self.form.pwl_fields())
            .enumerate()
            .map(|(n, (&row, [time, value]))| self.pwl_row(n, row, &time, &value, cx))
            .collect();
        v_flex()
            .gap_1()
            .child(header)
            .children(rows)
            .child(self.pwl_actions(cx))
            .into_any_element()
    }

    fn pwl_row(
        &self,
        n: usize,
        row: u64,
        time: &FieldSpec,
        value: &FieldSpec,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let remove = pressable(Button::new(("pwl-remove", n)))
            .ghost()
            .xsmall()
            .icon(IconName::Delete)
            .tooltip("Remove point")
            .disabled(self.read_only || !self.form.can_remove_pwl_row())
            .debug_selector(move || format!("pwl-remove-{n}"))
            .on_click(cx.listener(move |this, _, window, cx| this.remove_pwl_row(row, window, cx)));
        h_flex()
            .items_start()
            .gap_2()
            .child(div().flex_1().child(self.numeric_field(
                time,
                false,
                format!("pwl-time-{n}"),
                cx,
            )))
            .child(div().flex_1().child(self.numeric_field(
                value,
                false,
                format!("pwl-value-{n}"),
                cx,
            )))
            .child(div().w(rems(ROW_ACTION_WIDTH)).child(remove))
            .into_any_element()
    }

    /// Add point and Repeat forever, on two lines when one is too narrow,
    /// with the checkbox on the right either way. The repeat error takes a
    /// line of its own below, as wide as the column, so it wraps inside the
    /// narrow column (D20) and its appearing never moves the checkbox from
    /// under the pointer.
    fn pwl_actions(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let repeat_error = self.form.repeat_error();
        let actions = h_flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .child(
                pressable(Button::new("pwl-add"))
                    .small()
                    .outline()
                    .icon(IconName::Plus)
                    .label("Add point")
                    .disabled(self.read_only)
                    .debug_selector(|| "pwl-add".into())
                    .on_click(cx.listener(|this, _, window, cx| this.add_pwl_row(window, cx))),
            )
            .child(
                div().ml_auto().child(
                    Checkbox::new("pwl-repeat")
                        .label("Repeat forever")
                        .checked(self.form.pwl_repeat())
                        .disabled(self.read_only)
                        .debug_selector(|| "pwl-repeat".into())
                        .on_click(cx.listener(|this, checked: &bool, _, cx| {
                            this.set_pwl_repeat(*checked, cx)
                        })),
                ),
            );
        v_flex()
            .child(actions)
            .when_some(repeat_error, |column, error| {
                column.child(
                    div()
                        .text_xs()
                        .text_right()
                        .text_color(cx.theme().danger)
                        .debug_selector(|| "pwl-repeat-error".into())
                        .child(error),
                )
            })
    }

    /// A sparkline of one period of the waveform (or its whole shape) under
    /// the title "Preview", with its value range on the left and its start
    /// and end times below. The value labels hug the left padding and the
    /// curve reaches the right one, so the plot is balanced in its box; the
    /// curve's top is one gap under the title (D20).
    fn preview(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let body = match self.form.waveform() {
            Some(waveform) => {
                let window = preview::window(&waveform);
                let points = preview::samples(&waveform, &window);
                let labels = preview::labels(&window, &points, self.form.signal_unit());
                // As wide as the widest label and as tall as the chart: the
                // extremes' labels at its top and bottom edges, a flat
                // line's one label in the middle, level with the line.
                let values = v_flex()
                    .flex_none()
                    .h(rems(CHART_HEIGHT))
                    .items_end()
                    .map(|column| match &labels.min {
                        Some(_) => column.justify_between(),
                        None => column.justify_center(),
                    })
                    .child(value_label(labels.max, "waveform-preview-max"))
                    .children(
                        labels
                            .min
                            .map(|min| value_label(min, "waveform-preview-min")),
                    );
                let chart = div()
                    .w_full()
                    .h(rems(CHART_HEIGHT))
                    .debug_selector(|| "waveform-preview-chart".into())
                    .child(sparkline(Rc::new(points), theme.chart_1, theme.chart_grid));
                // The times line up with the curve's start and end.
                let times = h_flex()
                    .justify_between()
                    .gap_2()
                    .child(time_label(labels.start, "waveform-preview-start"))
                    .child(time_label(labels.end, "waveform-preview-end"));
                h_flex()
                    .items_start()
                    .gap(rems(PREVIEW_GAP))
                    .child(values)
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(rems(PREVIEW_GAP))
                            .child(chart)
                            .child(times),
                    )
                    .into_any_element()
            }
            // As tall as the plot, so fixing a field does not move it. A
            // column, so the note takes the box's width and wraps in it.
            None => v_flex()
                .h(rems(CHART_HEIGHT + PREVIEW_GAP + LABEL_LINE))
                .justify_center()
                .text_xs()
                .text_center()
                .child(
                    div()
                        .debug_selector(|| "waveform-preview-invalid".into())
                        .child("Fix the highlighted fields to see the preview."),
                )
                .into_any_element(),
        };
        v_flex()
            .flex_1()
            .min_w_0()
            .p_2()
            .gap(rems(PREVIEW_GAP))
            .rounded(theme.radius)
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
            .text_color(muted)
            .debug_selector(|| "waveform-preview".into())
            .child(
                div()
                    .text_xs()
                    .font_medium()
                    .line_height(rems(LABEL_LINE))
                    .debug_selector(|| "waveform-preview-title".into())
                    .child("Preview"),
            )
            .child(body)
    }
}

/// A value label, never cut: it sets the width of the label column.
fn value_label(text: String, selector: &'static str) -> impl IntoElement {
    div()
        .text_xs()
        .line_height(rems(LABEL_LINE))
        .whitespace_nowrap()
        .debug_selector(move || selector.into())
        .child(text)
}

/// A time label, cut short only when the two do not fit under the chart.
fn time_label(text: String, selector: &'static str) -> impl IntoElement {
    div()
        .min_w_0()
        .text_xs()
        .line_height(rems(LABEL_LINE))
        .truncate()
        .debug_selector(move || selector.into())
        .child(text)
}
