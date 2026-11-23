//! The sections of the properties form: general, value, a switch's control
//! source and model, DC & AC and semiconductor model, plus the shared
//! section/field-grid builders. The transient section has its own module.

use super::field::{Field, FieldParts};
use super::form::FieldStatus;
use super::spec::{self, FieldId, FieldSpec};
use super::view::PropertiesView;
use crate::library_browser::pressable;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _, StyledExt as _,
    button::Button,
    h_flex,
    menu::{DropdownMenu as _, PopupMenuItem},
    v_flex,
};
use gpui_kit::{
    AnyElement, App, Context, Div, InteractiveElement as _, IntoElement, ParentElement as _,
    Styled as _, div, prelude::FluentBuilder as _, rems,
};
use xsim_project::PartKind;

/// Width in rems of one field column of a source's compact form: half the
/// width its fields had while they shared the dialog's width (D20).
pub(super) const COMPACT_FIELD_WIDTH: f32 = 6.5;
/// Gap between field columns in rems.
const COLUMN_GAP: f32 = 0.75;

/// Width in rems of `columns` compact field columns and the gaps between
/// them.
pub(super) const fn compact_width(columns: u16) -> f32 {
    columns as f32 * COMPACT_FIELD_WIDTH + columns.saturating_sub(1) as f32 * COLUMN_GAP
}

/// A titled group of fields.
pub(super) fn section(title: &'static str, cx: &App) -> Div {
    v_flex().gap_2().child(
        div()
            .text_xs()
            .font_semibold()
            .text_color(cx.theme().muted_foreground)
            .child(title),
    )
}

/// `fields` in `columns` equally wide columns: [`COMPACT_FIELD_WIDTH`] each
/// when `compact`, else sharing the full width. Labels, inputs and messages
/// take a grid row each, so the inputs of a row stay in line when a label
/// or a message wraps, and every label sits right above its input.
pub(super) fn field_grid(columns: u16, compact: bool, fields: Vec<FieldParts>) -> Div {
    let count = usize::from(columns.max(1));
    let blanks = |n: usize| (0..n).map(|_| div().into_any_element());
    let mut cells = Vec::new();
    let mut fields = fields.into_iter();
    loop {
        let row: Vec<FieldParts> = fields.by_ref().take(count).collect();
        if row.is_empty() {
            break;
        }
        let missing = count - row.len();
        let labelled = row.iter().any(|field| field.label.is_some());
        let mut labels = Vec::with_capacity(count);
        let mut inputs = Vec::with_capacity(count);
        let mut messages = Vec::with_capacity(count);
        for field in row {
            labels.push(
                v_flex()
                    .min_w_0()
                    .justify_end()
                    .children(field.label)
                    .into_any_element(),
            );
            inputs.push(field.input);
            messages.push(field.message);
        }
        if labelled {
            cells.extend(labels);
            cells.extend(blanks(missing));
        }
        cells.extend(inputs);
        cells.extend(blanks(missing));
        cells.extend(messages);
        cells.extend(blanks(missing));
    }
    div()
        .grid()
        .grid_cols(columns)
        .gap_x(rems(COLUMN_GAP))
        .gap_y_1()
        .when(compact, |grid| grid.w(rems(compact_width(columns))))
        .children(cells)
}

/// Debug selector of a labelled field, e.g. `field-Resistance`.
pub(super) fn selector(label: &str) -> String {
    format!("field-{label}")
}

impl PropertiesView {
    /// Whether the form uses narrow field columns: a source's does, so its
    /// dialog is narrower (D20); other parts share the dialog's width.
    pub(super) fn compact(&self) -> bool {
        self.form.is_source()
    }

    /// A numeric field of the form with its unit and status, labelled when
    /// `label`; `None` while it has no input.
    fn numeric(&self, spec: &FieldSpec, label: bool, selector: String) -> Option<Field> {
        self.input(spec.id)?;
        Some(Field {
            label: label.then_some(spec.label),
            selector,
            unit: spec.unit.symbol(),
            status: self.form.status(spec),
            disabled: self.read_only,
        })
    }

    /// A numeric field rendered as one column (in the PWL table's rows).
    pub(super) fn numeric_field(
        &self,
        spec: &FieldSpec,
        label: bool,
        selector: String,
        cx: &App,
    ) -> AnyElement {
        match (self.numeric(spec, label, selector), self.input(spec.id)) {
            (Some(field), Some(input)) => field.render(input, cx),
            _ => div().into_any_element(),
        }
    }

    /// `specs` as labelled fields, in [`field_grid`] columns.
    pub(super) fn labelled_fields(
        &self,
        columns: u16,
        specs: &[FieldSpec],
        cx: &App,
    ) -> impl IntoElement {
        let fields = specs
            .iter()
            .filter_map(|spec| {
                let field = self.numeric(spec, true, selector(spec.label))?;
                Some(field.parts(self.input(spec.id)?, cx))
            })
            .collect();
        field_grid(columns, self.compact(), fields)
    }

    pub(super) fn general_section(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let designator = self.input(FieldId::Designator).map(|input| {
            Field {
                label: Some("Designator"),
                selector: selector("Designator"),
                unit: "",
                status: self.designator_status(),
                disabled: self.read_only,
            }
            .parts(input, cx)
        });
        section("General", cx).child(field_grid(
            2,
            self.compact(),
            designator.into_iter().collect(),
        ))
    }

    pub(super) fn value_section(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let specs = self.form.value_fields();
        if specs.is_empty() {
            return None;
        }
        let hint = spec::initial_condition_hint(self.form.kind()).map(|hint| {
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .debug_selector(|| "initial-condition-hint".into())
                .child(hint)
        });
        Some(
            section("Value", cx)
                .child(self.labelled_fields(2, &specs, cx))
                .children(hint)
                .into_any_element(),
        )
    }

    /// The voltage source whose current switches a current switch (D17): a
    /// drop-down of the schematic's voltage sources, or why there is none
    /// to choose. Not [`pressable`]: the kit's drop-down takes Enter and
    /// Space on its button to open the menu, where the arrow keys and
    /// Enter choose, so Enter there never applies the dialog.
    pub(super) fn control_section(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.form.has_control() {
            return None;
        }
        let theme = cx.theme();
        let (message, color, message_selector) = match self.form.control_status() {
            FieldStatus::Valid(text) => (text, theme.muted_foreground, "switch-control-message"),
            FieldStatus::Empty(text) => (
                text.to_string(),
                theme.muted_foreground,
                "switch-control-message",
            ),
            FieldStatus::Invalid(text) => (text, theme.danger, "switch-control-error"),
        };
        let message = div()
            .min_w_0()
            .text_xs()
            .text_color(color)
            .debug_selector(move || message_selector.into())
            .child(message);
        let label = div()
            .text_xs()
            .font_medium()
            .debug_selector(|| "switch-control-label".into())
            .child("Control source");
        let sources = self.form.control_sources().to_vec();
        let section = section("Control", cx);
        if sources.is_empty() {
            return Some(section.child(label).child(message).into_any_element());
        }
        let current = self.form.control().to_string();
        let shown = if current.trim().is_empty() {
            "Choose\u{2026}".to_string()
        } else {
            current.clone()
        };
        let view = cx.weak_entity();
        let menu = Button::new("switch-control")
            .small()
            .outline()
            .label(shown)
            .dropdown_caret(true)
            .disabled(self.read_only)
            .debug_selector(|| "switch-control".into())
            .dropdown_menu(move |menu, _, _| {
                sources.iter().fold(menu, |menu, source| {
                    let (view, chosen) = (view.clone(), source.clone());
                    let (name, selector) = (source.clone(), format!("switch-control-{source}"));
                    menu.item(
                        PopupMenuItem::element(move |_, _| {
                            let selector = selector.clone();
                            div().debug_selector(move || selector).child(name.clone())
                        })
                        .checked(*source == current)
                        .on_click(move |_, _, cx| {
                            // The dialog may have closed meanwhile.
                            let _ =
                                view.update(cx, |view, cx| view.set_control(chosen.clone(), cx));
                        }),
                    )
                })
            });
        let field = FieldParts {
            label: Some(label.into_any_element()),
            input: div().min_w_0().child(menu).into_any_element(),
            message: message.into_any_element(),
        };
        Some(
            section
                .child(field_grid(2, self.compact(), vec![field]))
                .into_any_element(),
        )
    }

    /// The model of a voltage or current switch (D17): RON and ROFF, the
    /// threshold and the hysteresis, and how the last two act.
    pub(super) fn switch_section(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let specs = self.form.switch_fields();
        let hint = spec::switch_hint(self.form.kind())?;
        let hint = div()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .debug_selector(|| "switch-hint".into())
            .child(hint);
        Some(
            section("Switch model", cx)
                .child(self.labelled_fields(2, &specs, cx))
                .child(hint)
                .into_any_element(),
        )
    }

    pub(super) fn source_sections(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if !self.form.is_source() {
            return Vec::new();
        }
        vec![
            section("DC & AC analysis", cx)
                .child(self.labelled_fields(3, &self.form.source_fields(), cx))
                .into_any_element(),
            self.transient_section(cx),
        ]
    }

    /// The chosen model with its library description and a Choose button.
    pub(super) fn model_section(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.form.has_model() {
            return None;
        }
        let theme = cx.theme();
        let name = self.form.model().trim().to_string();
        let details = match &self.model {
            Some(summary) => v_flex()
                .child(div().text_sm().child(summary.description.clone()))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(summary.manufacturer.clone()),
                ),
            None => v_flex().child(
                div()
                    .text_sm()
                    .text_color(theme.danger)
                    .debug_selector(|| "model-unknown".into())
                    .child("Not in the built-in library. Choose a listed model."),
            ),
        };
        let card = h_flex()
            .gap_3()
            .p_3()
            .rounded(theme.radius)
            .border_1()
            .border_color(theme.border)
            .bg(theme.muted)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        div()
                            .font_semibold()
                            .debug_selector(|| "model-name".into())
                            .child(if name.is_empty() {
                                "No model".into()
                            } else {
                                name
                            }),
                    )
                    .child(details),
            )
            .child(
                pressable(Button::new("choose-model"))
                    .small()
                    .label("Choose\u{2026}")
                    .tooltip("Browse the component library")
                    .disabled(self.read_only)
                    .debug_selector(|| "choose-model".into())
                    .on_click(cx.listener(|this, _, window, cx| this.choose_model(window, cx))),
            );
        Some(section("Model", cx).child(card).into_any_element())
    }

    pub(super) fn ground_note(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        (self.form.kind() == PartKind::Ground).then(|| {
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("Ground is the 0 V reference node and has no electrical parameters.")
                .into_any_element()
        })
    }
}
