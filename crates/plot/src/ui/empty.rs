//! The empty state shown before any simulation has run.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{ActiveTheme, Icon, v_flex};
use gpui_kit::{prelude::*, *};

/// "No results yet" with what to do about it.
pub fn no_result(cx: &App) -> AnyElement {
    let theme = cx.theme();
    v_flex()
        .id("waveform-empty")
        .debug_selector(|| "waveform-empty".into())
        .flex_1()
        .items_center()
        .justify_center()
        .gap_2()
        .p_6()
        .child(
            Icon::new(Lucide::ChartLine)
                .size(rems(2.))
                .text_color(theme.muted_foreground),
        )
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .child("No simulation results yet"),
        )
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child("Run a simulation (F5) to plot its waveforms here."),
        )
        .into_any_element()
}
