//! Opens the library in its own resizable window (D22).

use super::keys::ensure_key_bindings;
use super::view::{LibraryView, PickHandler};
use super::{LibraryFilter, LibraryItem};
use crate::dialog_window::{self, DialogContent, DialogOptions};
use gpui_kit::{App, AppContext as _, Entity, ParentElement as _, Window};
use xsim_spice_engine::Polarity::{N, P};

/// Window width, height and smallest size in rems, so they follow the UI
/// scale.
const WIDTH: f32 = 58.75;
const HEIGHT: f32 = 44.;
const MIN_WIDTH: f32 = 44.;
const MIN_HEIGHT: f32 = 30.;

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

/// Opens the library over `window` with `initial` selected. Enter, a double
/// click or the primary button pick the selection; Escape or Cancel close
/// it. Enter on a focused button presses it, and in the category tree it
/// opens or closes the highlighted category. The pick reaches `on_pick` in
/// `window`, once the library has closed.
pub(crate) fn open(
    filter: LibraryFilter,
    initial: Option<LibraryItem>,
    on_pick: PickHandler,
    window: &mut Window,
    cx: &mut App,
) {
    ensure_key_bindings(cx);
    let options = DialogOptions::new("library", title(filter), WIDTH)
        .resizable(HEIGHT, MIN_WIDTH, MIN_HEIGHT);
    dialog_window::open(window, cx, options, move |window, cx| {
        let view = cx.new(|cx| LibraryView::new(filter, initial, on_pick, window, cx));
        focus_search(&view, window, cx);
        move |content: DialogContent, _: &mut Window, _: &mut App| {
            let confirm_view = view.clone();
            content
                .title(title(filter))
                .child(view.clone())
                .on_ok(move |window, cx| confirm(&confirm_view, window, cx))
        }
    });
}

fn focus_search(view: &Entity<LibraryView>, window: &mut Window, cx: &mut App) {
    let search = view.read(cx).search.clone();
    search.update(cx, |search, cx| search.focus(window, cx));
}

/// Enter that no focused control took: reports the selection and closes
/// the dialog, or keeps it open when nothing is selected.
fn confirm(view: &Entity<LibraryView>, window: &mut Window, cx: &mut App) -> bool {
    let Some((on_pick, item)) = view.read(cx).picked() else {
        return false;
    };
    deliver(on_pick, item, window, cx);
    true
}

/// Hands `item` to `on_pick` in the window the library was opened from:
/// placing a part focuses the editor there.
pub(super) fn deliver(on_pick: PickHandler, item: LibraryItem, window: &Window, cx: &mut App) {
    dialog_window::with_owner(window, cx, move |window, cx| on_pick(item, window, cx));
}
