//! The category tree of the library window: an "all" row, then the
//! categories with expand/collapse chevrons and match counts. Clicking a
//! row selects its category and opens or closes it (D14). The highlighted
//! row is the category the list is drawn from, so it moves to the "all" row
//! while a search covers every category (D12).
//!
//! The tree is one tab stop. With the focus on it, Up and Down show the
//! previous or next category, Right opens the highlighted category (or goes
//! to its first sub-category), Left closes it (or goes to its parent), and
//! Enter or Space open and close it. Enter on a category without
//! sub-categories picks the selection, as elsewhere in the window.

use super::LibraryFilter;
use super::categories::TreeRow;
use super::keys::{
    CollapseCategory, ExpandCategory, NextCategory, PreviousCategory, TREE_CONTEXT, ToggleCategory,
};
use super::view::LibraryView;
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _, ThemeStyled as _, h_flex,
    list::ListItem, v_flex,
};
use gpui_kit::{
    AnyElement, ClickEvent, Context, ElementId, Hsla, InteractiveElement as _, IntoElement,
    MouseButton, ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _,
    Window, div, prelude::FluentBuilder as _, rems,
};

/// Sizes in rems, so they follow the UI scale.
const TREE_WIDTH: f32 = 14.75;
const INDENT: f32 = 0.875;

/// What the window lists under `filter`, as a plural noun for messages.
pub(super) fn listed_noun(filter: LibraryFilter) -> &'static str {
    use xsim_spice_engine::Polarity::{N, P};
    match filter {
        LibraryFilter::All => "parts",
        LibraryFilter::Diodes => "diodes",
        LibraryFilter::Bjts(N) => "NPN transistors",
        LibraryFilter::Bjts(P) => "PNP transistors",
        LibraryFilter::Jfets(N) => "N-channel JFETs",
        LibraryFilter::Jfets(P) => "P-channel JFETs",
    }
}

/// Label of the tree row that shows every entry the filter admits.
pub(super) fn root_label(filter: LibraryFilter) -> &'static str {
    use xsim_spice_engine::Polarity::{N, P};
    match filter {
        LibraryFilter::All => "All parts",
        LibraryFilter::Diodes => "All diodes",
        LibraryFilter::Bjts(N) => "All NPN transistors",
        LibraryFilter::Bjts(P) => "All PNP transistors",
        LibraryFilter::Jfets(N) => "All N-channel JFETs",
        LibraryFilter::Jfets(P) => "All P-channel JFETs",
    }
}

impl LibraryView {
    /// The tree in a bordered frame that takes the keyboard focus and shows
    /// the kit's focus ring while it has it. The frame does not clip, so
    /// the ring outside it stays whole; the rows scroll inside it.
    pub(super) fn tree_panel(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let root = self.category_row(Vec::new(), root_label(self.filter).into(), 0, None, cx);
        let rows: Vec<AnyElement> = self
            .browser
            .tree_rows()
            .into_iter()
            .map(|row| self.tree_row(row, cx))
            .collect();
        let focused = self.tree_focus.is_focused(window);
        v_flex()
            .id("library-tree")
            .debug_selector(|| "library-tree".into())
            .track_focus(&self.tree_focus)
            .key_context(TREE_CONTEXT)
            .on_action(cx.listener(Self::on_previous_category))
            .on_action(cx.listener(Self::on_next_category))
            .on_action(cx.listener(Self::on_collapse_category))
            .on_action(cx.listener(Self::on_expand_category))
            .on_action(cx.listener(Self::on_toggle_category))
            .w(rems(TREE_WIDTH))
            .flex_shrink_0()
            .border_1()
            .border_color(cx.theme().border)
            .rounded(cx.theme().radius)
            .when(focused, |frame| frame.focus_ring_style(window, cx))
            .child(
                v_flex()
                    .id("library-tree-rows")
                    .flex_1()
                    .min_h_0()
                    .p_1()
                    .gap_px()
                    .overflow_y_scroll()
                    .track_scroll(&self.tree_scroll)
                    .child(root)
                    .children(rows),
            )
    }

    fn on_previous_category(
        &mut self,
        _: &PreviousCategory,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.browser.step_category(-1);
        self.tree_key_handled(cx);
    }

    fn on_next_category(&mut self, _: &NextCategory, _: &mut Window, cx: &mut Context<Self>) {
        self.browser.step_category(1);
        self.tree_key_handled(cx);
    }

    fn on_collapse_category(
        &mut self,
        _: &CollapseCategory,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.browser.collapse_highlighted();
        self.tree_key_handled(cx);
    }

    fn on_expand_category(&mut self, _: &ExpandCategory, _: &mut Window, cx: &mut Context<Self>) {
        self.browser.expand_highlighted();
        self.tree_key_handled(cx);
    }

    /// Enter or Space; on a category without sub-categories Enter goes on
    /// to the dialog, which picks the selection.
    fn on_toggle_category(&mut self, _: &ToggleCategory, _: &mut Window, cx: &mut Context<Self>) {
        if self.browser.toggle_highlighted() {
            self.tree_key_handled(cx);
        } else {
            cx.propagate();
        }
    }

    /// Keeps the highlighted row and the selected entry in view.
    fn tree_key_handled(&mut self, cx: &mut Context<Self>) {
        if let Some(row) = self.browser.highlighted_row() {
            self.tree_scroll.scroll_to_item(row);
        }
        self.scroll_to_selection();
        cx.notify();
    }

    fn tree_row(&self, row: TreeRow, cx: &mut Context<Self>) -> AnyElement {
        let label: SharedString = row.label().to_string().into();
        let disclosure = row.has_children.then_some(row.expanded);
        self.category_row(row.path, label, row.depth + 1, disclosure, cx)
    }

    /// One selectable tree row; `depth` 0 is the "all" row. `disclosure` is
    /// whether a category with sub-categories is expanded, `None` without
    /// sub-categories. A click anywhere on the row selects the category and
    /// opens or closes it (D14); the second click of a double click only
    /// selects, so a double click opens or closes the category once. The
    /// press takes no focus: the click hands the keyboard to the search
    /// field, and the tree's focus ring would only flash in between.
    fn category_row(
        &self,
        path: Vec<String>,
        label: SharedString,
        depth: usize,
        disclosure: Option<bool>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.browser.scope() == path.as_slice();
        let count = self.browser.count(&path);
        let muted = cx.theme().muted_foreground;
        let id = ElementId::Name(format!("category:{}", path.join("/")).into());
        let selector = format!("library-category-{label}");
        let selected_selector = format!("library-selected-category-{label}");
        let slot = div()
            .w(rems(INDENT))
            .flex_shrink_0()
            .children(disclosure.map(|expanded| chevron(&label, expanded, muted)));
        let toggles = disclosure.is_some();
        ListItem::new(id)
            .selected(selected)
            .py_0p5()
            .pl(rems(0.25 + INDENT * depth.saturating_sub(1) as f32))
            .pr_2()
            .rounded(cx.theme().radius)
            .debug_selector(move || selector)
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                let toggle = toggles && event.click_count() == 1;
                this.click_category(path.clone(), toggle, window, cx)
            }))
            .child(
                h_flex()
                    .gap_1()
                    .text_sm()
                    .when(selected, |row| {
                        row.debug_selector(move || selected_selector)
                    })
                    .when(depth == 0, |row| row.font_medium())
                    .when(count == 0, |row| row.text_color(muted))
                    .child(slot)
                    .child(div().flex_1().min_w_0().truncate().child(label))
                    .child(div().text_xs().text_color(muted).child(count.to_string())),
            )
            .into_any_element()
    }
}

/// The disclosure arrow of a category with sub-categories: a plain icon,
/// as the whole row opens and closes the category.
fn chevron(label: &str, expanded: bool, color: Hsla) -> impl IntoElement {
    let icon = if expanded {
        IconName::ChevronDown
    } else {
        IconName::ChevronRight
    };
    let selector = format!(
        "library-{}-{label}",
        if expanded { "collapse" } else { "expand" }
    );
    div()
        .flex()
        .debug_selector(move || selector)
        .child(Icon::new(icon).xsmall().text_color(color))
}

#[cfg(test)]
mod tests {
    use super::*;
    use xsim_spice_engine::Polarity;

    #[test]
    fn the_all_row_names_what_the_filter_lists() {
        let filters = [
            LibraryFilter::All,
            LibraryFilter::Diodes,
            LibraryFilter::Bjts(Polarity::N),
            LibraryFilter::Bjts(Polarity::P),
            LibraryFilter::Jfets(Polarity::N),
            LibraryFilter::Jfets(Polarity::P),
        ];
        for filter in filters {
            assert_eq!(
                root_label(filter),
                format!("All {}", listed_noun(filter)),
                "{filter:?}"
            );
        }
        assert_eq!(listed_noun(LibraryFilter::All), "parts");
    }
}
