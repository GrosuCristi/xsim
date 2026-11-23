//! Application settings (Q25, Q30): theme and interface scale. Changes
//! apply immediately, to every window, and are remembered for the next
//! start. The dialog is a modal window of its own over the main window
//! (D22); its size follows the interface scale chosen in it.
//!
//! Enter on a focused button presses it, as Space does, instead of the
//! dialog's confirmation: the buttons are [`pressable`], and the scale
//! drop-down opens its menu. Enter anywhere else confirms the dialog like
//! Done: the settings already apply as they are chosen, so there is nothing
//! left to apply.

use crate::appearance::{SCALE_PRESETS, scale_label};
use crate::ui::{form_row, hint, pressable, section_title, testable};
use crate::workspace::Workspace;
use gpui_kit::component::{
    Selectable, Sizable,
    button::{Button, ButtonGroup, ButtonVariants},
    dialog::DialogFooter,
    menu::{DropdownMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::{prelude::*, *};
use xsim_editor::dialog_window::{self, DialogContent, DialogOptions};
use xsim_project::appdata::ThemePreference;

/// Opens the settings dialog over `window`.
pub fn open_settings_dialog(workspace: Entity<Workspace>, window: &mut Window, cx: &mut App) {
    let options = DialogOptions::new("settings", "Settings", 29.).estimated_height(15.5);
    dialog_window::open(window, cx, options, move |_, _| {
        move |content: DialogContent, _: &mut Window, cx: &mut App| {
            settings_content(content, &workspace, cx)
        }
    });
}

/// The dialog's content, rebuilt on every frame from the workspace's
/// settings.
fn settings_content(
    content: DialogContent,
    workspace: &Entity<Workspace>,
    cx: &App,
) -> DialogContent {
    content
        .title("Settings")
        .child(
            v_flex()
                .gap_4()
                .child(section_title("Appearance", cx))
                .child(form_row("Theme", theme_choice(workspace, cx), "", None, cx))
                .child(form_row(
                    "Interface scale",
                    scale_choice(workspace, cx),
                    "",
                    None,
                    cx,
                ))
                .child(hint(
                    "Changes apply immediately and are kept for the next start.",
                    cx,
                )),
        )
        // Enter that no button took: everything is applied already.
        .on_ok(|_, _| true)
        .footer(
            DialogFooter::new().child(testable(
                "settings-done",
                pressable(Button::new("settings-done"))
                    .primary()
                    .label("Done")
                    .on_click(|_, window, cx| dialog_window::close(window, cx)),
            )),
        )
}

/// Element id and debug selector of the button for `theme`.
fn theme_button_selector(theme: ThemePreference) -> &'static str {
    match theme {
        ThemePreference::Light => "theme-light",
        ThemePreference::Dark => "theme-dark",
    }
}

/// Light / Dark: the small outline button group of the analysis dialog
/// (D14). Each button handles its own click, so Tab reaches it and Enter or
/// Space chooses it.
fn theme_choice(workspace: &Entity<Workspace>, cx: &App) -> impl IntoElement {
    let current = workspace.read(cx).settings().theme;
    let buttons = [
        (ThemePreference::Light, "Light"),
        (ThemePreference::Dark, "Dark"),
    ]
    .map(|(theme, label)| {
        let workspace = workspace.clone();
        pressable(Button::new(theme_button_selector(theme)))
            .label(label)
            .selected(theme == current)
            .debug_selector(move || theme_button_selector(theme).into())
            .on_click(move |_, window, cx| {
                // In the main window, where a failure to save is reported.
                let workspace = workspace.clone();
                dialog_window::with_owner(window, cx, move |window, cx| {
                    workspace.update(cx, |this, cx| this.set_theme(theme, window, cx));
                });
            })
    });
    ButtonGroup::new("theme-choice")
        .small()
        .outline()
        .children(buttons)
}

/// Drop-down of the scale presets. Not [`pressable`]: the kit's drop-down
/// already takes Enter and Space on its button to open the menu, and the
/// pressable binding would hide them from it.
fn scale_choice(workspace: &Entity<Workspace>, cx: &App) -> impl IntoElement {
    let current = workspace.read(cx).settings().ui_scale;
    let workspace = workspace.clone();
    Button::new("ui-scale")
        .small()
        .outline()
        .debug_selector(|| "ui-scale".into())
        .label(scale_label(current))
        .dropdown_caret(true)
        .dropdown_menu(move |menu, _, _| {
            SCALE_PRESETS.iter().fold(menu, |menu, &scale| {
                let workspace = workspace.clone();
                menu.item(
                    PopupMenuItem::new(scale_label(scale))
                        .checked((scale - current).abs() < 0.001)
                        .on_click(move |_, window, cx| {
                            let workspace = workspace.clone();
                            dialog_window::with_owner(window, cx, move |window, cx| {
                                workspace
                                    .update(cx, |this, cx| this.set_ui_scale(scale, window, cx));
                            });
                        }),
                )
            })
        })
}
