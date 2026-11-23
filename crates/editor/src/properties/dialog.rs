//! Opens the properties dialog in its own window (D22) and builds its
//! footer.

use super::form::{taken_designators, voltage_sources};
use super::transient::SOURCE_FORM_WIDTH;
use super::view::PropertiesView;
use super::waveform::WaveKind;
use crate::SchematicEditor;
use crate::dialog_window::{self, Build, DialogContent, DialogOptions};
use crate::library_browser::{self, LibraryItem, pressable};
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
use xsim_spice_engine::library;

/// The window for a part of `kind`. A source's compact form gets exactly
/// its width (D20); other parts share a fixed width, in rems so it follows
/// the UI scale. It keeps room for `reserved` states of the form (D24, see
/// [`Reserved`]).
fn options(kind: PartKind, reserved: usize, title: String) -> DialogOptions {
    let options = match kind {
        PartKind::VoltageSource | PartKind::CurrentSource => {
            DialogOptions::new(title, 0.).content_width(SOURCE_FORM_WIDTH)
        }
        PartKind::Ground => DialogOptions::new(title, 26.),
        _ => DialogOptions::new(title, 32.5),
    };
    options.reserve_states(reserved)
}

/// A state the form can change to while the dialog is open, which makes it
/// taller: the window keeps room on the screen for it (D24). The dialog's
/// window opens where the tallest of them fits below its top, nearer the
/// top of the screen than centred if need be.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Reserved {
    /// A source showing another waveform type, as it is once chosen.
    Wave(WaveKind),
    /// A source's PWL table with [`RESERVED_PWL_POINTS`] points more than
    /// it shows once chosen.
    MorePwlPoints,
    /// A semiconductor showing the model of its family whose description
    /// is the longest, as when it is picked from the library.
    LongestModel,
}

/// How many points a PWL table can gain while the dialog is open and still
/// be shown whole. The table can grow without end, so the window cannot
/// keep room for all of it; with more points the form scrolls above the
/// footer.
const RESERVED_PWL_POINTS: usize = 4;

/// The states the form of `part` can reach: none in a read-only project,
/// whose form does not change.
fn reserved_states(part: &Part, read_only: bool) -> Vec<Reserved> {
    if read_only {
        return Vec::new();
    }
    match part {
        Part::VoltageSource { source } | Part::CurrentSource { source } => {
            let current = WaveKind::of(source.waveform.as_ref());
            WaveKind::ALL
                .into_iter()
                .filter(|kind| *kind != current)
                .map(Reserved::Wave)
                .chain([Reserved::MorePwlPoints])
                .collect()
        }
        Part::Diode { .. } | Part::Bjt { .. } | Part::Jfet { .. } => vec![Reserved::LongestModel],
        _ => Vec::new(),
    }
}

/// The library model of `part`'s family whose description is the longest,
/// as a library item.
fn longest_model(part: &Part) -> Option<LibraryItem> {
    fn longest<M>(
        entries: Vec<library::ModelEntry<M>>,
        admits: impl Fn(&M) -> bool,
    ) -> Option<library::ModelEntry<M>> {
        entries
            .into_iter()
            .filter(|entry| admits(&entry.model))
            .max_by_key(|entry| entry.description.len())
    }
    let part = match part {
        Part::Diode { .. } => Part::Diode {
            model: longest(library::diodes(), |_| true)?.model.name,
        },
        Part::Bjt { polarity, .. } => Part::Bjt {
            polarity: *polarity,
            model: longest(library::bjts(), |model| model.polarity == *polarity)?
                .model
                .name,
        },
        Part::Jfet { polarity, .. } => Part::Jfet {
            polarity: *polarity,
            model: longest(library::jfets(), |model| model.polarity == *polarity)?
                .model
                .name,
        },
        _ => return None,
    };
    LibraryItem::model_of(&part)
}

/// Puts the form of `part` in `state`. Only in the hidden window that
/// measures the dialog's content: whatever it focuses stays there.
fn enter(
    state: Reserved,
    part: &Part,
    view: &Entity<PropertiesView>,
    window: &mut Window,
    cx: &mut App,
) {
    let model = (state == Reserved::LongestModel)
        .then(|| longest_model(part))
        .flatten();
    view.update(cx, |view, cx| match state {
        Reserved::Wave(kind) => view.set_wave_kind(kind, window, cx),
        Reserved::MorePwlPoints => {
            view.set_wave_kind(WaveKind::Pwl, window, cx);
            for _ in 0..RESERVED_PWL_POINTS {
                view.add_pwl_row(window, cx);
            }
        }
        Reserved::LongestModel => {
            if let Some(model) = &model {
                view.set_model(model, cx);
            }
        }
    });
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
    let reserved = reserved_states(&component.part, read_only);
    let options = options(component.part.kind(), reserved.len(), title);
    // Built for the hidden window that measures the form, for each reserved
    // state, then for the dialog's window (D24). None of it reaches the
    // editor until Apply or OK.
    dialog_window::open(window, cx, options, move |build, window, cx| {
        let view = cx.new(|cx| {
            PropertiesView::new(
                editor.clone(),
                &component,
                taken.clone(),
                sources.clone(),
                read_only,
                window,
                cx,
            )
        });
        match build {
            Build::Shown => focus_initial_field(&view, window, cx),
            Build::Reserved(state) => enter(reserved[state], &component.part, &view, window, cx),
        }
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
