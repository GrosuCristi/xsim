//! The library window's view: owns the search field and the [`Browser`]
//! state, and reacts to typing, clicks and keyboard navigation. The panels
//! it renders are built in [`super::panels`].

use super::browser::Browser;
use super::catalog::Catalog;
use super::{LibraryFilter, LibraryItem};
use gpui_kit::component::{
    WindowExt as _,
    input::{InputEvent, InputState, MoveDown, MoveUp},
    v_flex,
};
use gpui_kit::{
    App, AppContext as _, Context, Entity, FocusHandle, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, ScrollHandle, Styled as _, Subscription, Window, rems,
};
use std::rc::Rc;

/// Room around the content for focus rings, which the kit draws 3 px
/// outside a control, in rems (4 px at 100% UI scale).
const FOCUS_RING_ROOM: f32 = 0.25;

/// Receives the picked item.
pub(crate) type PickHandler = Rc<dyn Fn(LibraryItem, &mut Window, &mut App)>;

/// Content of the library dialog.
pub(crate) struct LibraryView {
    pub(super) browser: Browser,
    pub(super) filter: LibraryFilter,
    pub(super) search: Entity<InputState>,
    pub(super) results_scroll: ScrollHandle,
    /// The category tree's keyboard focus (a tab stop after the search
    /// field) and scroll position.
    pub(super) tree_focus: FocusHandle,
    pub(super) tree_scroll: ScrollHandle,
    /// Tracks the whole content, so the dialog can tell a focused control of
    /// the window from its own close button.
    focus: FocusHandle,
    on_pick: PickHandler,
    _search_events: Subscription,
}

impl LibraryView {
    /// A browser over the built-in library restricted by `filter`, with
    /// `initial` selected when it is listed.
    pub(crate) fn new(
        filter: LibraryFilter,
        initial: Option<LibraryItem>,
        on_pick: PickHandler,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let catalog = Catalog::builtin().restricted(filter);
        let search = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Search by name, description, manufacturer or category")
        });
        let search_events = cx.subscribe(&search, Self::on_search_event);
        let view = Self {
            browser: Browser::new(catalog, initial.as_ref()),
            filter,
            search,
            results_scroll: ScrollHandle::new(),
            tree_focus: cx.focus_handle().tab_stop(true),
            tree_scroll: ScrollHandle::new(),
            focus: cx.focus_handle(),
            on_pick,
            _search_events: search_events,
        };
        view.scroll_to_selection();
        view
    }

    /// The pick callback and the selected item, if any.
    pub(crate) fn picked(&self) -> Option<(PickHandler, LibraryItem)> {
        let entry = self.browser.selected_entry()?;
        Some((self.on_pick.clone(), entry.item.clone()))
    }

    fn on_search_event(
        &mut self,
        search: Entity<InputState>,
        event: &InputEvent,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, InputEvent::Change) {
            let query = search.read(cx).value();
            self.browser.set_query(&query);
            self.scroll_to_selection();
            cx.notify();
        }
    }

    pub(super) fn scroll_to_selection(&self) {
        if let Some(position) = self.browser.selected_position() {
            self.results_scroll.scroll_to_item(position);
        }
    }

    fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        self.browser.move_selection(delta);
        self.scroll_to_selection();
        cx.notify();
    }

    /// Up/Down in the search field move through the results instead of the
    /// (single-line) text.
    fn on_move_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.move_selection(-1, cx);
    }

    fn on_move_down(&mut self, _: &MoveDown, _: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.move_selection(1, cx);
    }

    /// Shows category `path` and returns the keyboard to the search field,
    /// as after a click on its row.
    pub(super) fn select_category(
        &mut self,
        path: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.browser.select_category(path);
        self.scroll_to_selection();
        self.focus_search(window, cx);
        cx.notify();
    }

    /// A click on the tree row of category `path`: opens or closes it when
    /// `toggle` (it has sub-categories), and shows it.
    pub(super) fn click_category(
        &mut self,
        path: Vec<String>,
        toggle: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if toggle {
            self.browser.toggle_expanded(&path);
        }
        self.select_category(path, window, cx);
    }

    /// Selects result `ix`; a double click picks it.
    pub(super) fn click_result(
        &mut self,
        ix: usize,
        click_count: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.browser.select(ix);
        if click_count >= 2 {
            self.pick_and_close(window, cx);
        } else {
            self.focus_search(window, cx);
            cx.notify();
        }
    }

    /// Closes the dialog and reports the selected item.
    pub(super) fn pick_and_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((on_pick, item)) = self.picked() {
            window.close_dialog(cx);
            on_pick(item, window, cx);
        }
    }

    /// Whether the keyboard is on the dialog's own close button.
    pub(crate) fn on_close_button(&self, window: &Window, cx: &App) -> bool {
        super::keys::on_close_button(&self.focus, window, cx)
    }

    pub(crate) fn focus_search(&self, window: &mut Window, cx: &mut App) {
        self.search
            .update(cx, |search, cx| search.focus(window, cx));
    }
}

impl Render for LibraryView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rem = window.rem_size();
        let available = window.viewport_size().height - rem * 12.;
        let height = available.min(rem * 38.75).max(rem * 20.);
        v_flex()
            .id("library-browser")
            .debug_selector(|| "library-browser".into())
            .track_focus(&self.focus)
            .w_full()
            .h(height)
            // The dialog body clips at its top edge, right above the search
            // field: this keeps room for the focus rings drawn outside the
            // field and the buttons.
            .p(rems(FOCUS_RING_ROOM))
            .gap_3()
            .capture_action(cx.listener(Self::on_move_up))
            .capture_action(cx.listener(Self::on_move_down))
            .child(self.search_bar(cx))
            .child(self.browse_area(window, cx))
            .child(self.details_panel(cx))
            .child(self.footer(cx))
    }
}
