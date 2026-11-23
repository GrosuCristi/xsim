//! Opens the properties dialog on the window root and builds its footer.

use super::form::{taken_designators, voltage_sources};
use super::transient::SOURCE_FORM_WIDTH;
use super::view::PropertiesView;
use crate::SchematicEditor;
use crate::library_browser::{self, pressable};
use gpui_kit::component::{
    ActiveTheme as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
};
use gpui_kit::{
    AnyElement, App, AppContext as _, Entity, Focusable as _, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, Styled as _, Window, div, px,
};
use std::cell::Cell;
use xsim_project::{Id, PartKind};

/// What the kit's dialog surface adds to the width of its content: 16 px
/// of padding on either side inside a 1 px border.
const DIALOG_SIDES: Pixels = px(34.);

/// Dialog width for a part kind at `rem` (so it follows the UI scale). A
/// source's compact form gets exactly its width (D20); other parts share
/// a fixed width.
fn width(kind: PartKind, rem: Pixels) -> Pixels {
    match kind {
        PartKind::VoltageSource | PartKind::CurrentSource => rem * SOURCE_FORM_WIDTH + DIALOG_SIDES,
        PartKind::Ground => rem * 26.,
        _ => rem * 32.5,
    }
}

/// Opens the dialog for component `id`; does nothing if it does not exist.
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
    let kind = component.part.kind();
    let view =
        cx.new(|cx| PropertiesView::new(editor, &component, taken, sources, read_only, window, cx));
    let first_build = Cell::new(true);
    window.open_dialog(cx, move |dialog, window, cx| {
        if first_build.replace(false) {
            focus_initial_field(&view, window, cx);
        }
        let confirm = view.clone();
        dialog
            .title(view.read(cx).title())
            .w(width(kind, window.rem_size()))
            // A stray click outside must not discard typed values.
            .overlay_closable(false)
            .child(view.clone())
            .footer(footer(&view, read_only, cx))
            .on_ok(move |_, window, cx| confirm_from_keyboard(&confirm, window, cx))
    });
}

/// Enter that no focused control took, as the kit confirms a dialog with
/// it; whether the dialog closes. In the form (a field, the checkbox, a
/// blank spot or the dialog's title and margins) it applies like OK and
/// keeps the dialog open on errors. On the kit's close button it closes
/// without applying, as that button does.
fn confirm_from_keyboard(view: &Entity<PropertiesView>, window: &mut Window, cx: &mut App) -> bool {
    if library_browser::on_close_button(&view.focus_handle(cx), window, cx) {
        return true;
    }
    view.update(cx, |view, cx| view.apply(window, cx))
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
                .on_click(|_, window, cx| window.close_dialog(cx))
                .into_any_element(),
        ];
    }
    let apply_view = view.clone();
    let ok_view = view.clone();
    vec![
        pressable(Button::new("properties-cancel"))
            .label("Cancel")
            .debug_selector(|| "properties-cancel".into())
            .on_click(|_, window, cx| window.close_dialog(cx))
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
                    window.close_dialog(cx);
                }
            })
            .into_any_element(),
    ]
}
