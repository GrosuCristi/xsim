//! Opens the properties dialog in its own window (D22) and builds its
//! footer.

use super::form::{taken_designators, voltage_sources};
use super::transient::SOURCE_FORM_WIDTH;
use super::view::PropertiesView;
use super::waveform::WaveKind;
use crate::SchematicEditor;
use crate::dialog_window::{self, DialogContent, DialogOptions};
use crate::library_browser::{self, pressable};
use gpui_kit::component::{
    ActiveTheme as _,
    button::{Button, ButtonVariants as _},
    h_flex,
};
use gpui_kit::{
    AnyElement, App, AppContext as _, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Styled as _, Window, div,
};
use xsim_project::{Id, Part, PartKind};
use xsim_spice_engine::{Source, Waveform};

/// The window for `part`. A source's compact form gets exactly its width
/// (D20); other parts share a fixed width, in rems so it follows the UI
/// scale.
///
/// The window opens at the height its form is expected to measure, so that
/// it opens centred over the editor (D22): once open it can only grow or
/// shrink downward. That height depends on more than the part's kind (see
/// [`variant`]), and each variant is a dialog kind of its own, so a later
/// opening of the same variant starts at the height measured this time.
fn options(part: &Part, read_only: bool, title: String) -> DialogOptions {
    let kind = part.kind();
    let access = if read_only { "-read-only" } else { "" };
    let id = format!("properties-{kind:?}{}{access}", variant(part));
    let options = match kind {
        PartKind::VoltageSource | PartKind::CurrentSource => {
            DialogOptions::new(id, title, 0.).content_width(SOURCE_FORM_WIDTH)
        }
        PartKind::Ground => DialogOptions::new(id, title, 26.),
        _ => DialogOptions::new(id, title, 32.5),
    };
    options.estimated_height(estimated_height(part, read_only))
}

/// What sets the height of `part`'s form beyond its kind: a source's
/// waveform type and number of PWL points, and the model of a
/// semiconductor, whose description takes one line or two.
fn variant(part: &Part) -> String {
    match part {
        Part::VoltageSource { source } | Part::CurrentSource { source } => {
            let wave = WaveKind::of(source.waveform.as_ref());
            match &source.waveform {
                Some(Waveform::Pwl { points, .. }) => format!("-{wave:?}-{}", points.len()),
                _ => format!("-{wave:?}"),
            }
        }
        Part::Diode { model } | Part::Bjt { model, .. } | Part::Jfet { model, .. } => {
            format!("-{model}")
        }
        _ => String::new(),
    }
}

/// The height in rems the window opens at, before its content has been
/// measured: what the form of `part` measures at 100% UI scale.
fn estimated_height(part: &Part, read_only: bool) -> f32 {
    // A read-only footer has a single button, so its hint takes one line.
    let footer = if read_only { -0.4375 } else { 0. };
    let form = match part {
        Part::Resistor { .. } => 20.75,
        Part::Capacitor { .. } | Part::Inductor { .. } => 24.75,
        Part::VoltageSource { source } | Part::CurrentSource { source } => source_height(source),
        // The narrower window wraps the ground's hint differently.
        Part::Ground if read_only => return 17.125,
        Part::Ground => 20.,
        // Between a model description of one line and one of two.
        Part::Diode { .. } | Part::Bjt { .. } => 23.25,
        Part::Jfet { .. } => 22.5,
        Part::VoltageSwitch { .. } => 29.25,
        Part::CurrentSwitch { .. } => 38.75,
    };
    form + footer
}

/// The height in rems of a source's form: its waveform's fields and
/// preview, or its PWL table, three rems per point.
fn source_height(source: &Source) -> f32 {
    match &source.waveform {
        None => 26.625,
        Some(Waveform::Pulse { .. } | Waveform::Sine { .. }) => 44.75,
        Some(Waveform::Exp { .. } | Waveform::Sffm { .. }) => 42.75,
        Some(Waveform::Pwl { points, .. }) if points.len() <= 1 => 35.5,
        Some(Waveform::Pwl { points, .. }) => 31.75 + 3. * points.len() as f32,
    }
}

/// Opens the dialog for component `id` over `window`; does nothing if it
/// does not exist.
pub(super) fn open(editor: Entity<SchematicEditor>, id: Id, window: &mut Window, cx: &mut App) {
    library_browser::ensure_key_bindings(cx);
    let (component, taken, sources, read_only) = {
        let state = editor.read(cx);
        let Some(component) = state.schematic().component(id).cloned() else {
            return;
        };
        let taken = taken_designators(state.schematic(), id);
        let sources = voltage_sources(state.schematic());
        (component, taken, sources, state.is_read_only())
    };
    let title = format!(
        "{} \u{b7} {}",
        component.name,
        component.part.kind().label()
    );
    let options = options(&component.part, read_only, title);
    dialog_window::open(window, cx, options, move |window, cx| {
        let view = cx.new(|cx| {
            PropertiesView::new(editor, &component, taken, sources, read_only, window, cx)
        });
        focus_initial_field(&view, window, cx);
        move |content: DialogContent, _: &mut Window, cx: &mut App| {
            let confirm = view.clone();
            content
                .title(view.read(cx).title())
                .child(view.clone())
                .footer(footer(&view, read_only, cx))
                .on_ok(move |window, cx| confirm.update(cx, |view, cx| view.apply(window, cx)))
        }
    });
}

/// Focuses the main value field with its text selected, so typing replaces it.
fn focus_initial_field(view: &Entity<PropertiesView>, window: &mut Window, cx: &mut App) {
    if let Some(input) = view.read(cx).initial_focus() {
        input.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
    }
}

/// Input hint on the left; Cancel, Apply and OK (or Close when read-only) on
/// the right.
fn footer(view: &Entity<PropertiesView>, read_only: bool, cx: &App) -> AnyElement {
    let hint = if read_only {
        "This project is read-only."
    } else {
        "Values accept SI prefixes: f p n u m k M G T (m = milli, M = mega)."
    };
    h_flex()
        .w_full()
        .justify_between()
        .items_center()
        .gap_4()
        .child(
            // Wraps in the narrow source dialog.
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .debug_selector(|| "properties-hint".into())
                .child(hint),
        )
        .child(
            h_flex()
                .flex_shrink_0()
                .gap_2()
                .children(buttons(view, read_only)),
        )
        .into_any_element()
}

fn buttons(view: &Entity<PropertiesView>, read_only: bool) -> Vec<AnyElement> {
    if read_only {
        return vec![
            pressable(Button::new("properties-close"))
                .primary()
                .label("Close")
                .debug_selector(|| "properties-close".into())
                .on_click(|_, window, cx| dialog_window::close(window, cx))
                .into_any_element(),
        ];
    }
    let apply_view = view.clone();
    let ok_view = view.clone();
    vec![
        pressable(Button::new("properties-cancel"))
            .label("Cancel")
            .debug_selector(|| "properties-cancel".into())
            .on_click(|_, window, cx| dialog_window::close(window, cx))
            .into_any_element(),
        pressable(Button::new("properties-apply"))
            .label("Apply")
            .tooltip("Apply without closing")
            .debug_selector(|| "properties-apply".into())
            .on_click(move |_, window, cx| {
                apply_view.update(cx, |view, cx| view.apply(window, cx));
            })
            .into_any_element(),
        pressable(Button::new("properties-ok"))
            .primary()
            .label("OK")
            .debug_selector(|| "properties-ok".into())
            .on_click(move |_, window, cx| {
                if ok_view.update(cx, |view, cx| view.apply(window, cx)) {
                    dialog_window::close(window, cx);
                }
            })
            .into_any_element(),
    ]
}
