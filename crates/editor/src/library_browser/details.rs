//! The bottom panel of the library window: symbol preview and metadata of
//! the selected entry.

use super::browser::PATH_SEPARATOR;
use super::catalog::CatalogEntry;
use super::view::LibraryView;
use crate::symbol_art::symbol_preview;
use gpui_kit::component::{
    ActiveTheme as _, Sizable as _, StyledExt as _, h_flex, tag::Tag, v_flex,
};
use gpui_kit::{
    AnyElement, App, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    Styled as _, div, rems,
};

/// Sizes in rems, so they follow the UI scale.
const DETAILS_HEIGHT: f32 = 9.25;
const PREVIEW_SIZE: f32 = 7.25;

impl LibraryView {
    /// Symbol preview and metadata of the selected entry.
    pub(super) fn details_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match self.browser.selected_entry() {
            Some(entry) => entry_details(entry, cx),
            None => div()
                .m_auto()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("Select a part to see its symbol and details.")
                .into_any_element(),
        };
        h_flex()
            .h(rems(DETAILS_HEIGHT))
            .flex_shrink_0()
            .p_3()
            .gap_4()
            .border_1()
            .border_color(cx.theme().border)
            .rounded(cx.theme().radius)
            .bg(cx.theme().muted)
            .debug_selector(|| "library-details".into())
            .child(content)
    }
}

fn entry_details(entry: &CatalogEntry, cx: &App) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    h_flex()
        .size_full()
        .gap_4()
        .child(
            div()
                .size(rems(PREVIEW_SIZE))
                .flex_shrink_0()
                .p_2()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().background)
                .debug_selector(|| "library-preview".into())
                .child(symbol_preview(entry.kind)),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .h_full()
                .gap_1()
                .child(
                    h_flex()
                        .gap_2()
                        .child(div().text_lg().font_semibold().child(entry.name.clone()))
                        .child(
                            Tag::secondary()
                                .small()
                                .child(entry.path.join(PATH_SEPARATOR)),
                        ),
                )
                .child(div().text_sm().child(entry.description.clone()))
                .child(parameter_grid(entry, cx))
                .child(
                    div()
                        .mt_auto()
                        .text_xs()
                        .text_color(muted)
                        .truncate()
                        .child(format!("Source: {}", entry.provenance)),
                ),
        )
        .into_any_element()
}

fn parameter_grid(entry: &CatalogEntry, cx: &App) -> impl IntoElement {
    let muted = cx.theme().muted_foreground;
    let pairs = std::iter::once(("Manufacturer", entry.manufacturer.clone()))
        .chain(entry.parameters.iter().cloned());
    h_flex()
        .flex_wrap()
        .gap_x_4()
        .gap_y_0p5()
        .text_xs()
        .children(pairs.map(|(label, value)| {
            h_flex()
                .gap_1()
                .child(div().text_color(muted).child(label))
                .child(div().font_medium().child(value))
        }))
}
