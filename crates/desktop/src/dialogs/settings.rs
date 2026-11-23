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

use crate::appearance::{SCALE_PRESETS, SCALE_RANGE, clamp_scale, scale_label};
use crate::ui::{form_row, hint, pressable, section_title, testable};
use crate::workspace::Workspace;
use gpui_kit::component::{
    ActiveTheme as _, Selectable, Sizable,
    button::{Button, ButtonGroup, ButtonVariants},
    dialog::DialogFooter,
    menu::{DropdownMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::{prelude::*, *};
use xsim_editor::dialog_window::{self, DialogContent, DialogOptions};
use xsim_project::appdata::ThemePreference;

/// Opens the settings dialog over `window`, once the current update is
/// over (it may be called from one of `workspace`'s). Its window keeps room
/// on the screen for the content at the largest interface scale, which can
/// be chosen in it (D24): the window grows with the scale and cannot move.
pub fn open_settings_dialog(workspace: Entity<Workspace>, window: &mut Window, cx: &mut App) {
    let owner = window.window_handle();
    cx.defer(move |cx| {
        owner
            .update(cx, |_, window, cx| open_now(workspace, window, cx))
            .ok();
    });
}

/// [`open_settings_dialog`], now.
fn open_now(workspace: Entity<Workspace>, window: &mut Window, cx: &mut App) {
    let scale = clamp_scale(workspace.read(cx).settings().ui_scale);
    let largest = cx.theme().font_size * (SCALE_RANGE.1 / scale);
    let options = DialogOptions::new("Settings", 29.).reserve_rem_size(largest);
    // The content only reads the workspace, so building it again for the
    // hidden window that measures it changes nothing.
    dialog_window::open(window, cx, options, move |_, _, _| {
        let workspace = workspace.clone();
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

#[cfg(all(test, debug_assertions))]
mod tests {
    use crate::actions::OpenSettings;
    use crate::appearance::SCALE_PRESETS;
    use crate::test_support::{TestEnv, close, dialog, init, open, press_in_dialog};
    use gpui_kit::{
        Bounds, Point, TestAppContext, VisualContext as _, VisualTestContext, px, size,
    };
    use xsim_editor::dialog_window;

    /// Checks the settings window against D24: inside the work area, all of
    /// it shown, Done inside the window.
    fn assert_fully_shown(case: &str, settings: &mut VisualTestContext) {
        let handle = settings.window_handle();
        let layout = settings
            .cx
            .update(|app| dialog_window::layout_of(handle, app))
            .unwrap_or_else(|| panic!("{case}: no dialog that fits its content"));
        if let Err(problem) = layout.check() {
            panic!("{case}: {problem} ({layout:?})");
        }
        // Apart from the dialog's own measurements: content that fits is not
        // cut off, but between the title row and the footer.
        if layout.natural_height <= layout.room.size.height {
            let content = settings
                .debug_bounds("dialog-content")
                .expect("the content");
            let bar = settings
                .debug_bounds("dialog-title-bar")
                .expect("the title");
            let footer = settings.debug_bounds("dialog-footer").expect("the footer");
            assert!(
                content.top() >= bar.bottom() - px(1.) && content.bottom() <= footer.top() + px(1.),
                "{case}: the content {content:?} is cut off between {bar:?} and {footer:?}"
            );
        }
        let done = settings
            .debug_bounds("settings-done")
            .unwrap_or_else(|| panic!("{case}: Done is not shown"));
        let viewport = layout.viewport;
        assert!(
            done.top() >= px(0.)
                && done.bottom() <= viewport.height + px(1.)
                && done.right() <= viewport.width + px(1.),
            "{case}: Done {done:?} outside {viewport:?}"
        );
    }

    /// D24: on the owner's screen at 200% display scaling (a 1920 × 1030 px
    /// work area) and on a smaller one, the settings window opened at 100%
    /// or 150% opens fully shown and stays so, without moving, at every
    /// interface scale chosen in it, up to the largest: it keeps room for
    /// that one.
    #[gpui_kit::test]
    fn the_settings_stay_fully_shown_at_every_scale(cx: &mut TestAppContext) {
        init(cx);
        let env = TestEnv::new();
        let (workspace, mut window) = open(cx, &env, None);
        for (width, height) in [(1920., 1030.), (1280., 680.)] {
            let area = Bounds::new(Point::default(), size(px(width), px(height)));
            for opened_at in [1.0, 1.5] {
                window.update(|window, cx| {
                    dialog_window::simulate_work_area(cx, Some(area));
                    window.resize(area.size);
                    window.bounds_changed(cx);
                });
                workspace.update_in(&mut window, |w, window, cx| {
                    w.set_ui_scale(opened_at, window, cx)
                });
                window.run_until_parked();
                window.dispatch_action(OpenSettings);
                window.run_until_parked();
                let mut settings = dialog(&window);
                let screen = format!("opened at {opened_at} on {width} × {height}");
                assert_fully_shown(&screen, &mut settings);
                let opened = settings.update(|window, _| window.bounds());
                for scale in SCALE_PRESETS {
                    workspace.update_in(&mut window, |w, window, cx| {
                        w.set_ui_scale(scale, window, cx)
                    });
                    window.run_until_parked();
                    let case = format!("{screen}, at {scale}");
                    assert_fully_shown(&case, &mut settings);
                    let bounds = settings.update(|window, _| window.bounds());
                    assert_eq!(bounds.origin, opened.origin, "{case}: did not move");
                }
                press_in_dialog("escape", &mut settings);
            }
        }
        close(&mut window);
    }
}
