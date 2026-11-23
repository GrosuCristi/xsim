//! The frame of the library window: search bar, the browse area (category
//! tree beside the result list), and the footer with keyboard hints and
//! buttons. The tree, list and details panels have their own modules.

use super::LibraryFilter;
use super::keys::pressable;
use super::view::LibraryView;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::Input,
    kbd::Kbd,
};
use gpui_kit::{
    Context, InteractiveElement as _, IntoElement, Keystroke, ParentElement as _, Styled as _,
    Window, div,
};

/// Verb of the primary button: placing a new part or choosing a model.
pub(super) fn action_label(filter: LibraryFilter) -> &'static str {
    if filter == LibraryFilter::All {
        "Place"
    } else {
        "Choose"
    }
}

impl LibraryView {
    pub(super) fn search_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div().debug_selector(|| "library-search".into()).child(
            Input::new(&self.search)
                .prefix(
                    Icon::new(IconName::Search)
                        .small()
                        .text_color(cx.theme().muted_foreground),
                )
                .cleanable(true)
                .aria_label("Search the component library"),
        )
    }

    /// Category tree and result list side by side.
    pub(super) fn browse_area(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .flex_1()
            .min_h_0()
            .gap_3()
            .items_stretch()
            .child(self.tree_panel(window, cx))
            .child(self.results_panel(cx))
    }

    /// Keyboard hints, Cancel and the primary Place/Choose button.
    pub(super) fn footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let picked = self.browser.selected_entry().is_some();
        h_flex()
            .justify_between()
            .items_center()
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_4()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(hint(&["up", "down"], "Navigate"))
                    .child(hint(&["enter"], action_label(self.filter)))
                    .child(hint(&["escape"], "Close")),
            )
            .child(
                h_flex()
                    .flex_shrink_0()
                    .gap_2()
                    .child(
                        pressable(Button::new("library-cancel"))
                            .label("Cancel")
                            .debug_selector(|| "library-cancel".into())
                            .on_click(|_, window, cx| crate::dialog_window::close(window, cx)),
                    )
                    .child(
                        pressable(Button::new("library-pick"))
                            .primary()
                            .label(action_label(self.filter))
                            .disabled(!picked)
                            .debug_selector(|| "library-pick".into())
                            .on_click(
                                cx.listener(|this, _, window, cx| this.pick_and_close(window, cx)),
                            ),
                    ),
            )
    }
}

fn hint(keys: &'static [&'static str], action: &'static str) -> impl IntoElement {
    h_flex()
        .gap_1()
        .children(
            keys.iter()
                .filter_map(|key| Keystroke::parse(key).ok())
                .map(Kbd::new),
        )
        .child(action)
}
