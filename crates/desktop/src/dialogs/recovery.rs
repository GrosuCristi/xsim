//! "Recover unsaved work" (Q41): lists the snapshots crashed instances left
//! behind, each with Restore and Discard. A modal window of its own over the
//! main window (D22).
//!
//! The list follows the workspace: Discard removes a row in place, and the
//! dialog closes with the last one. Restore closes it and restores in the
//! main window.
//!
//! Every button is [`pressable`], so Enter on a focused button presses it
//! instead of the dialog's confirmation, which would only close the dialog.
//! Enter anywhere else closes it like "Decide later".

use crate::ui::{hint, pressable, testable};
use crate::workspace::Workspace;
use gpui_kit::component::{
    ActiveTheme, Icon, IconName, Sizable,
    button::{Button, ButtonVariants},
    dialog::DialogFooter,
    h_flex, v_flex,
};
use gpui_kit::{prelude::*, *};
use xsim_editor::dialog_window::{self, DialogContent, DialogOptions};

/// Opens the dialog for `workspace`'s orphaned snapshots, `rows` of them
/// now, over `window`.
pub fn open_recovery_dialog(
    workspace: Entity<Workspace>,
    rows: usize,
    window: &mut Window,
    cx: &mut App,
) {
    // Opens at the height its rows are expected to take, so that it opens
    // centred (D22); each number of rows is a dialog kind of its own.
    let options = DialogOptions::new(format!("recovery-{rows}"), "Recover unsaved work", 32.)
        .estimated_height(11. + 4. * rows as f32);
    dialog_window::open(window, cx, options, move |_, _| {
        move |content: DialogContent, _: &mut Window, cx: &mut App| {
            recovery_content(content, &workspace, cx)
        }
    });
}

/// The dialog's content: one row per snapshot still offered.
fn recovery_content(
    content: DialogContent,
    workspace: &Entity<Workspace>,
    cx: &App,
) -> DialogContent {
    let summaries = workspace.read(cx).orphan_summaries();
    let rows = summaries
        .iter()
        .enumerate()
        .map(|(index, (name, source))| orphan_row(workspace, index, name, source.as_deref(), cx));
    content
        .title("Recover unsaved work")
        .child(
            v_flex()
                .gap_3()
                .child(hint(
                    "xsim closed unexpectedly while these projects had unsaved changes. \
                     Restore one to continue where you left off.",
                    cx,
                ))
                .child(v_flex().gap_2().children(rows)),
        )
        .footer(
            DialogFooter::new().child(testable(
                "recovery-later",
                pressable(Button::new("recovery-later"))
                    .label("Decide later")
                    .on_click(|_, window, cx| dialog_window::close(window, cx)),
            )),
        )
}

/// One snapshot: name, original file, Restore and Discard.
fn orphan_row(
    workspace: &Entity<Workspace>,
    index: usize,
    name: &str,
    source: Option<&str>,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let restore = workspace.clone();
    let discard = workspace.clone();
    h_flex()
        .gap_3()
        .p_2()
        .rounded(theme.radius)
        .border_1()
        .border_color(theme.border)
        .child(Icon::new(IconName::File).text_color(theme.muted_foreground))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child(name.to_string()),
                )
                .child(hint(source.unwrap_or("Never saved").to_string(), cx)),
        )
        .child(testable(
            if index == 0 {
                "recovery-discard-0"
            } else {
                "recovery-discard"
            },
            pressable(Button::new(("recovery-discard", index)))
                .small()
                .ghost()
                .label("Discard")
                .on_click(move |_, window, cx| {
                    discard.update(cx, |this, cx| this.discard_orphan(index, cx));
                    if discard.read(cx).orphan_summaries().is_empty() {
                        dialog_window::close(window, cx);
                    } else {
                        // The rows below moved up: start again from the top.
                        dialog_window::focus_dialog(window, cx);
                    }
                }),
        ))
        .child(testable(
            if index == 0 {
                "recovery-restore-0"
            } else {
                "recovery-restore"
            },
            pressable(Button::new(("recovery-restore", index)))
                .small()
                .primary()
                .label("Restore")
                .on_click(move |_, window, cx| {
                    let restore = restore.clone();
                    dialog_window::with_owner(window, cx, move |window, cx| {
                        restore.update(cx, |this, cx| this.restore_orphan(index, window, cx));
                    });
                    dialog_window::close(window, cx);
                }),
        ))
}
