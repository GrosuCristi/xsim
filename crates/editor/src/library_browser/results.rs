//! The result list of the library window and its empty state. While a
//! search runs, each row names its category beside the part name (D12).

use super::catalog::CatalogEntry;
use super::tree::{listed_noun, root_label};
use super::view::LibraryView;
use crate::symbol_art::symbol_preview;
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _, h_flex, list::ListItem, v_flex,
};
use gpui_kit::{
    AnyElement, ClickEvent, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div, prelude::FluentBuilder as _, rems,
};

/// Suggestion shown when nothing matches.
const SEARCH_TIP: &str =
    "Try a part number, a description such as \u{201c}switching\u{201d}, or a manufacturer.";

impl LibraryView {
    pub(super) fn results_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let body = if self.browser.has_results() {
            self.result_list(cx)
        } else {
            self.no_results(cx)
        };
        div()
            .flex_1()
            .min_w_0()
            .border_1()
            .border_color(cx.theme().border)
            .rounded(cx.theme().radius)
            .overflow_hidden()
            .child(body)
    }

    fn result_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let rows: Vec<AnyElement> = self
            .browser
            .results()
            .map(|(ix, entry)| self.result_row(ix, entry, cx))
            .collect();
        v_flex()
            .id("library-results")
            .size_full()
            .p_1()
            .gap_px()
            .overflow_y_scroll()
            .track_scroll(&self.results_scroll)
            .children(rows)
            .into_any_element()
    }

    fn result_row(&self, ix: usize, entry: &CatalogEntry, cx: &mut Context<Self>) -> AnyElement {
        let selector = format!("library-item-{}", entry.name);
        let category_selector = format!("library-item-category-{}", entry.name);
        let category = self.browser.result_category(entry);
        let muted = cx.theme().muted_foreground;
        ListItem::new(("library-item", ix))
            .selected(self.browser.selected() == Some(ix))
            .py_1()
            .px_2()
            .rounded(cx.theme().radius)
            .debug_selector(move || selector)
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                this.click_result(ix, event.click_count(), window, cx)
            }))
            .child(
                h_flex()
                    .gap_3()
                    .child(
                        div()
                            .size(rems(1.75))
                            .flex_shrink_0()
                            .child(symbol_preview(entry.kind)),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                h_flex()
                                    .min_w_0()
                                    .gap_2()
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .text_sm()
                                            .font_medium()
                                            .child(entry.name.clone()),
                                    )
                                    .when_some(category, |line, category| {
                                        line.child(
                                            div()
                                                .min_w_0()
                                                .truncate()
                                                .text_xs()
                                                .text_color(muted)
                                                .debug_selector(move || category_selector)
                                                .child(category),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .truncate()
                                    .child(entry.description.clone()),
                            ),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(muted)
                            .child(entry.manufacturer.clone()),
                    ),
            )
            .into_any_element()
    }

    /// Empty state of the result list. When a search narrowed to a category
    /// finds nothing there, it says how many matches the other categories
    /// hold (the tree shows where).
    fn no_results(&self, cx: &mut Context<Self>) -> AnyElement {
        let query = self.browser.query().trim().to_string();
        let noun = listed_noun(self.filter);
        let everywhere = self.browser.count(&[]);
        let narrowed_to = self.browser.scope().last().filter(|_| everywhere > 0);
        let (title, tip, selector) = if query.is_empty() {
            (
                format!("The library has no {noun}"),
                None,
                "library-no-results-empty",
            )
        } else if let Some(category) = narrowed_to {
            let matches = if everywhere == 1 { "match" } else { "matches" };
            (
                format!("No {noun} in {category} match \u{201c}{query}\u{201d}"),
                Some(format!(
                    "{everywhere} {matches} in other categories. Select \u{201c}{}\u{201d} to see every match.",
                    root_label(self.filter)
                )),
                "library-no-results-elsewhere",
            )
        } else {
            (
                format!("No {noun} match \u{201c}{query}\u{201d}"),
                Some(SEARCH_TIP.to_string()),
                "library-no-results-anywhere",
            )
        };
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_2()
            .p_6()
            .debug_selector(|| "library-no-results".into())
            .child(
                Icon::new(IconName::Search)
                    .large()
                    .text_color(cx.theme().muted_foreground),
            )
            .child(
                div()
                    .font_medium()
                    .text_center()
                    .debug_selector(move || selector.into())
                    .child(title),
            )
            .when_some(tip, |state, tip| {
                state.child(
                    div()
                        .text_sm()
                        .text_center()
                        .text_color(cx.theme().muted_foreground)
                        .child(tip),
                )
            })
            .into_any_element()
    }
}
