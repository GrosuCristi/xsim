//! "Recover unsaved work" (Q41): lists the snapshots crashed instances left
//! behind, each with Restore and Discard.
//!
//! Every button is [`pressable`], so Enter on a focused button presses it
//! instead of the kit's dialog confirmation, which would only close the
//! dialog. Enter anywhere else closes it like "Decide later".

use crate::ui::{dialog_width, hint, pressable, testable};
use crate::workspace::Workspace;
use gpui_kit::component::{
    ActiveTheme, Icon, IconName, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    dialog::DialogFooter,
    h_flex, v_flex,
};
use gpui_kit::{prelude::*, *};

/// Opens the dialog for `workspace`'s orphaned snapshots, described by
/// `summaries` (project name and original file of each).
pub fn open_recovery_dialog(
    workspace: Entity<Workspace>,
    summaries: Vec<(String, Option<String>)>,
    window: &mut Window,
    cx: &mut App,
) {
    window.open_dialog(cx, move |dialog, window, cx| {
        let rows = summaries.iter().enumerate().map(|(index, (name, source))| {
            orphan_row(&workspace, index, name, source.as_deref(), cx)
        });
        dialog
            .title("Recover unsaved work")
            .w(dialog_width(32., window))
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
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                )),
            )
    });
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
                    window.close_dialog(cx);
                    discard.update(cx, |this, cx| this.discard_orphan(index, cx));
                    let remaining = discard.read(cx).orphan_summaries();
                    if !remaining.is_empty() {
                        open_recovery_dialog(discard.clone(), remaining, window, cx);
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
                    window.close_dialog(cx);
                    restore.update(cx, |this, cx| this.restore_orphan(index, window, cx));
                }),
        ))
}
