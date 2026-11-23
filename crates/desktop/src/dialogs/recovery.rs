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
///
/// The window opens at the height of all the rows (D24), and Discard only
/// takes rows away, so it keeps no room for more. The number of rows is
/// left to the window to measure.
pub fn open_recovery_dialog(
    workspace: Entity<Workspace>,
    rows: usize,
    window: &mut Window,
    cx: &mut App,
) {
    let _ = rows;
    let options = DialogOptions::new("Recover unsaved work", 32.);
    // The content only reads the workspace, so building it again for the
    // hidden window that measures it changes nothing.
    dialog_window::open(window, cx, options, move |_, _, _| {
        let workspace = workspace.clone();
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

#[cfg(all(test, debug_assertions))]
mod tests {
    use crate::actions::NewCircuit;
    use crate::test_support::{TestEnv, click, close, dialog, init, open, press_in_dialog};
    use gpui_kit::component::Theme;
    use gpui_kit::{
        Bounds, Point, TestAppContext, VisualContext as _, VisualTestContext, px, size,
    };
    use std::fs;
    use xsim_editor::dialog_window;

    /// Leaves `rows` crashed sessions with `window`'s work behind, as
    /// instances that crashed with unsaved changes would.
    fn leave_crashed_sessions(rows: usize, env: &TestEnv, window: &mut VisualTestContext) {
        window
            .executor()
            .advance_clock(std::time::Duration::from_secs(2));
        window.run_until_parked();
        let recovery = env.data_dir().join("recovery");
        let snapshot = fs::read_dir(&recovery)
            .expect("recovery directory")
            .flatten()
            .map(|entry| entry.path().join("snapshot.json"))
            .find(|path| path.is_file())
            .expect("a snapshot is written after an edit");
        for row in 0..rows {
            let crashed = recovery.join(format!("session-crashed-{row}"));
            fs::create_dir_all(&crashed).expect("crashed session");
            fs::copy(&snapshot, crashed.join("snapshot.json")).expect("copy the snapshot");
            fs::write(crashed.join("lock"), b"").expect("lock file");
        }
    }

    /// Checks the recovery window against D24: inside the work area, all of
    /// it shown unless taller than the work area, "Decide later" inside the
    /// window.
    fn assert_fully_shown(case: &str, recovery: &mut VisualTestContext) {
        let handle = recovery.window_handle();
        let layout = recovery
            .cx
            .update(|app| dialog_window::layout_of(handle, app))
            .unwrap_or_else(|| panic!("{case}: no dialog that fits its content"));
        if let Err(problem) = layout.check() {
            panic!("{case}: {problem} ({layout:?})");
        }
        // Apart from the dialog's own measurements: rows that fit are not
        // cut off, but between the title row and the footer.
        if layout.natural_height <= layout.room.size.height {
            let content = recovery.debug_bounds("dialog-content").expect("the rows");
            let bar = recovery
                .debug_bounds("dialog-title-bar")
                .expect("the title");
            let footer = recovery.debug_bounds("dialog-footer").expect("the footer");
            assert!(
                content.top() >= bar.bottom() - px(1.) && content.bottom() <= footer.top() + px(1.),
                "{case}: the rows {content:?} are cut off between {bar:?} and {footer:?}"
            );
        }
        let later = recovery
            .debug_bounds("recovery-later")
            .unwrap_or_else(|| panic!("{case}: Decide later is not shown"));
        let viewport = layout.viewport;
        assert!(
            later.top() >= px(0.)
                && later.bottom() <= viewport.height + px(1.)
                && later.right() <= viewport.width + px(1.),
            "{case}: Decide later {later:?} outside {viewport:?}"
        );
    }

    /// D24: with one snapshot to recover and with many, on the owner's
    /// screen at 200% display scaling (a 1920 × 1030 px work area) and on a
    /// smaller one, at 100% and 150% UI scale, the recovery window opens
    /// fully shown, centred over the main window unless the screen's edge
    /// is in the way, and stays so as Discard takes rows away.
    #[gpui_kit::test]
    fn the_recovery_window_is_fully_shown_with_one_row_or_many(cx: &mut TestAppContext) {
        init(cx);
        for rows in [1, 12] {
            let env = TestEnv::new();
            let (_, mut first) = open(cx, &env, None);
            first.dispatch_action(NewCircuit);
            leave_crashed_sessions(rows, &env, &mut first);
            for (width, height) in [(1920., 1030.), (1280., 680.)] {
                let area = Bounds::new(Point::default(), size(px(width), px(height)));
                for font_size in [16., 24.] {
                    first.update(|_, cx| {
                        dialog_window::simulate_work_area(cx, Some(area));
                        Theme::update(cx, |theme| theme.font_size = px(font_size));
                    });
                    let (_, mut second) = open(cx, &env, None);
                    let case = format!("{rows} rows at {font_size} px on {width} × {height}");
                    let mut recovery = dialog(&second);
                    assert_fully_shown(&case, &mut recovery);
                    let owner = second.update(|window, _| window.bounds());
                    let bounds = recovery.update(|window, _| window.bounds());
                    let room_bottom = area.bottom() - dialog_window::SCREEN_MARGIN;
                    let centred = (owner.center().y - bounds.size.height / 2.)
                        .min(room_bottom - bounds.size.height)
                        .max(area.top() + dialog_window::SCREEN_MARGIN);
                    assert!(
                        (bounds.top() - centred).abs() <= px(1.),
                        "{case}: {bounds:?} not centred over {owner:?}"
                    );
                    if rows > 1 {
                        click("recovery-discard-0", &mut recovery);
                        second.run_until_parked();
                        assert_fully_shown(&format!("{case}, one discarded"), &mut recovery);
                    }
                    press_in_dialog("escape", &mut recovery);
                    close(&mut second);
                }
            }
            close(&mut first);
        }
    }
}
