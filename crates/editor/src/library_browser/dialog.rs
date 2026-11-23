//! Opens the library window as a modal dialog on the window root.

use super::keys::ensure_key_bindings;
use super::view::{LibraryView, PickHandler};
use super::{LibraryFilter, LibraryItem};
use gpui_kit::component::WindowExt as _;
use gpui_kit::{App, AppContext as _, Entity, ParentElement as _, Window};
use std::cell::Cell;
use xsim_spice_engine::Polarity::{N, P};

/// Dialog width in rems, so it follows the UI scale.
const DIALOG_WIDTH: f32 = 58.75;

/// Dialog title for `filter`.
fn title(filter: LibraryFilter) -> &'static str {
    match filter {
        LibraryFilter::All => "Component Library",
        LibraryFilter::Diodes => "Choose Diode Model",
        LibraryFilter::Bjts(N) => "Choose NPN Transistor Model",
        LibraryFilter::Bjts(P) => "Choose PNP Transistor Model",
        LibraryFilter::Jfets(N) => "Choose N-Channel JFET Model",
        LibraryFilter::Jfets(P) => "Choose P-Channel JFET Model",
    }
}

/// Opens the dialog with `initial` selected. Enter, a double click or the
/// primary button pick the selection; Escape or Cancel close it. Enter on a
/// focused button presses it, and in the category tree it opens or closes
/// the highlighted category.
pub(crate) fn open(
    filter: LibraryFilter,
    initial: Option<LibraryItem>,
    on_pick: PickHandler,
    window: &mut Window,
    cx: &mut App,
) {
    ensure_key_bindings(cx);
    let view = cx.new(|cx| LibraryView::new(filter, initial, on_pick, window, cx));
    let first_build = Cell::new(true);
    window.open_dialog(cx, move |dialog, window, cx| {
        if first_build.replace(false) {
            focus_search(&view, window, cx);
        }
        let confirm_view = view.clone();
        dialog
            .title(title(filter))
            .w(window.rem_size() * DIALOG_WIDTH)
            .child(view.clone())
            .on_ok(move |_, window, cx| confirm(&confirm_view, window, cx))
    });
}

fn focus_search(view: &Entity<LibraryView>, window: &mut Window, cx: &mut App) {
    let search = view.read(cx).search.clone();
    search.update(cx, |search, cx| search.focus(window, cx));
}

/// Enter that no focused control took: reports the selection and closes
/// the dialog, or keeps it open when nothing is selected. On the kit's
/// close button it closes without picking, as that button does.
fn confirm(view: &Entity<LibraryView>, window: &mut Window, cx: &mut App) -> bool {
    if view.read(cx).on_close_button(window, cx) {
        return true;
    }
    let Some((on_pick, item)) = view.read(cx).picked() else {
        return false;
    };
    on_pick(item, window, cx);
    true
}
