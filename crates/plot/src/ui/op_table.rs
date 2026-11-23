//! Operating-point results: a table of node voltages and element currents
//! instead of a plot.

use crate::model::op_table::{OpRow, OpTable, op_table};
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::{prelude::*, *};
use xsim_spice_engine::SimulationResult;

/// Width of the table, in rems (Q30).
const WIDTH: f32 = 28.75;

/// The scrollable table.
pub fn table(result: &SimulationResult, cx: &App) -> AnyElement {
    let OpTable { voltages, currents } = op_table(result);
    v_flex()
        .id("op-table")
        .debug_selector(|| "op-table".into())
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .items_center()
        .p_4()
        .child(
            v_flex()
                .w(rems(WIDTH))
                .max_w_full()
                .gap_4()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Operating point"),
                )
                .child(section("Node voltages", &voltages, cx))
                .child(section("Currents", &currents, cx)),
        )
        .into_any_element()
}

fn section(title: &'static str, rows: &[OpRow], cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    v_flex()
        .rounded(theme.radius)
        .border_1()
        .border_color(theme.border)
        .overflow_hidden()
        .child(
            h_flex()
                .px_3()
                .h(rems(1.75))
                .bg(theme.table_head)
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.table_head_foreground)
                .child(title),
        )
        .when(rows.is_empty(), |el| {
            el.child(
                div()
                    .px_3()
                    .py_2()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("None recorded"),
            )
        })
        .children(
            rows.iter()
                .enumerate()
                .map(|(i, row)| table_row(i, row, cx)),
        )
}

fn table_row(index: usize, row: &OpRow, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    h_flex()
        .px_3()
        .h(rems(1.625))
        .justify_between()
        .border_t_1()
        .border_color(theme.table_row_border)
        .when(index % 2 == 1, |el| el.bg(theme.table_even))
        .text_sm()
        .font_family(theme.mono_font_family.clone())
        .child(div().child(row.name.clone()))
        .child(div().child(row.text.clone()))
}
