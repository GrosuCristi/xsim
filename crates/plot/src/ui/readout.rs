//! The cursor readout (Q131, D14, D19). The first block is the cursors: x1
//! and x2, and y1 and y2, the value of the trace each cursor follows at its
//! x, with that trace; then Δx = x2 − x1 and Δy = y2 − y1 (a dash when the
//! two traces' units differ) and 1/Δx on a time axis. A cursor moves along
//! x only, so its y always comes from its trace. The second block reads
//! every visible trace at both cursors' x; its `1`/`2` buttons choose the
//! trace each cursor follows.

use super::cursor::CursorPoint;
use super::theme::hsla;
use crate::WaveformViewer;
use crate::model::cursors::{CursorId, Readout, inverse_unit, x_delta};
use crate::model::format;
use crate::view_model::{PlotModel, x_title};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Selectable, Sizable, h_flex, v_flex};
use gpui_kit::{prelude::*, *};
use xsim_project::YScale;

/// Width of a value column, in rems (Q30).
const COLUMN: f32 = 6.5;

/// The readout table, when the cursors are on.
pub fn readout(viewer: &WaveformViewer, cx: &mut Context<WaveformViewer>) -> Option<AnyElement> {
    if !viewer.view.cursors.enabled {
        return None;
    }
    let model = viewer.model()?;
    let theme = cx.theme().clone();
    let traces = trace_rows(viewer, &model, cx);
    let cells = cursor_cells(viewer, &model);
    let table = v_flex()
        .id("cursor-readout")
        .debug_selector(|| "cursor-readout".into())
        // In a short dock the readout gives up height and scrolls, so the
        // "Add pane" strip under it stays inside the dock (D22).
        .min_h_0()
        .max_h(rems(13.0))
        .overflow_y_scroll()
        .px_3()
        .py_1p5()
        .border_t_1()
        .border_color(theme.border)
        .text_xs()
        .child(header(x_title(&model.result.axis), ["x", "y", ""], cx))
        .children(CursorId::ALL.map(|id| cursor_row(viewer, &cells, id, cx)))
        .child(delta_row(&cells, cx))
        .when(!traces.is_empty(), |el| {
            el.child(div().my_1().h(px(1.)).bg(theme.border))
                .child(header(String::new(), ["at x1", "at x2", "Δ (2 − 1)"], cx))
                .children(traces)
        });
    Some(table.into_any_element())
}

fn header(title: String, columns: [&str; 3], cx: &App) -> impl IntoElement {
    let muted = cx.theme().muted_foreground;
    row(
        div().text_color(muted).child(title),
        columns.map(str::to_string),
        String::new(),
        muted,
        cx,
    )
}

/// A `1`/`2` badge like the cursor's flag on the plot.
fn badge(id: CursorId, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .size(rems(1.0))
        .rounded(rems(0.2))
        .bg(theme.foreground)
        .text_color(theme.background)
        .font_weight(FontWeight::SEMIBOLD)
        .child(id.number())
}

/// The text of the cursor block's values; a dash where there is none.
#[derive(Debug, Clone, PartialEq)]
pub struct CursorCells {
    /// x1 and x2.
    pub x: [String; 2],
    /// y1 and y2: each cursor's trace at its x, in that trace's unit.
    pub y: [String; 2],
    /// x2 − x1.
    pub dx: String,
    /// y2 − y1, when both traces share a unit.
    pub dy: String,
    /// `1/Δx = …` on a time axis, else empty.
    pub inverse: String,
}

/// The values the cursor block shows.
pub(crate) fn cursor_cells(viewer: &WaveformViewer, model: &PlotModel) -> CursorCells {
    let unit = model.result.axis.unit.as_str();
    let points = CursorId::ALL.map(|id| viewer.cursor_point(id));
    let x = points.each_ref().map(|point| {
        point
            .as_ref()
            .map(|p| format::value(p.x, unit))
            .unwrap_or_else(dash)
    });
    let y = points.each_ref().map(|point| {
        point
            .as_ref()
            .and_then(|p| p.on.as_ref())
            .and_then(|on| Some(format::value(on.y?, &on.unit)))
            .unwrap_or_else(dash)
    });
    let delta = x_delta(&viewer.view.cursors);
    let dx = delta
        .map(|d| format::value(d.dx, unit))
        .unwrap_or_else(dash);
    let dy = match &points {
        [Some(first), Some(second)] => y_delta(first, second),
        _ => None,
    }
    .unwrap_or_else(dash);
    // 1/Δx is a frequency only on a time axis.
    let inverse = delta
        .and_then(|d| d.inverse)
        .filter(|_| unit == "s")
        .map(|v| format!("1/Δx = {}", format::value(v, &inverse_unit(unit))))
        .unwrap_or_default();
    CursorCells {
        x,
        y,
        dx,
        dy,
        inverse,
    }
}

/// Cursor `id`: its x, the value y of the trace it follows there, and that
/// trace.
fn cursor_row(
    viewer: &WaveformViewer,
    cells: &CursorCells,
    id: CursorId,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let point = viewer.cursor_point(id);
    let x = cells.x[id.index()].clone();
    let y = cells.y[id.index()].clone();
    let on = point.as_ref().and_then(|p| p.on.as_ref());
    let name = h_flex()
        .debug_selector(move || format!("cursor-row-{}", id.number()))
        .gap_1p5()
        .child(badge(id, cx))
        .map(|el| match on {
            Some(on) => el.child(swatch(hsla(on.color))).child(
                div()
                    .font_family(theme.mono_font_family.clone())
                    .whitespace_nowrap()
                    .child(trace_label(&on.expression, on.scale)),
            ),
            None => el.child(
                div()
                    .text_color(theme.muted_foreground)
                    .child("No trace to follow"),
            ),
        });
    row(
        name,
        [x, y, String::new()],
        String::new(),
        theme.foreground,
        cx,
    )
}

/// Δx = x2 − x1, Δy = y2 − y1 (only when both values share a unit) and
/// 1/Δx on a time axis.
fn delta_row(cells: &CursorCells, cx: &App) -> impl IntoElement {
    row(
        div()
            .debug_selector(|| "cursor-delta-row".into())
            .font_weight(FontWeight::MEDIUM)
            .child("Δ (2 − 1)"),
        [cells.dx.clone(), cells.dy.clone(), String::new()],
        cells.inverse.clone(),
        cx.theme().foreground,
        cx,
    )
}

/// `y2 − y1` with its unit, when both cursors have a value in one unit.
fn y_delta(first: &CursorPoint, second: &CursorPoint) -> Option<String> {
    let (a, b) = (first.on.as_ref()?, second.on.as_ref()?);
    (a.unit == b.unit).then_some(())?;
    Some(format::value(b.y? - a.y?, &a.unit))
}

/// Every visible trace at both cursors' x, with the buttons that make a
/// cursor follow it.
fn trace_rows(
    viewer: &WaveformViewer,
    model: &PlotModel,
    cx: &mut Context<WaveformViewer>,
) -> Vec<AnyElement> {
    let axis = &model.result.axis.values;
    let x_log = model.x_log();
    let followed = CursorId::ALL.map(|id| viewer.cursor_trace(id));
    let mut rows = Vec::new();
    for (pane_index, pane) in viewer.setup.panes.iter().enumerate() {
        for (index, trace) in pane.traces.iter().enumerate() {
            let Some(values) = trace
                .visible
                .then(|| viewer.traces.values(&trace.expression, pane.y_scale))
                .flatten()
            else {
                continue;
            };
            let y_log = pane.y_scale == YScale::Log;
            let readout = Readout::new(&viewer.view.cursors, axis, &values.y, x_log, y_log);
            let value = |v: Option<f64>| {
                v.map(|v| format::value(v, &values.unit))
                    .unwrap_or_else(dash)
            };
            let selected = viewer.view.selected == Some((pane_index, index));
            let theme = cx.theme().clone();
            let attach = CursorId::ALL.map(|id| {
                let on = followed[id.index()] == Some((pane_index, index));
                attach_button(id, pane_index, index, on, cx)
            });
            let name = h_flex()
                .gap_1p5()
                .child(swatch(hsla(viewer.colors.trace_color(trace, index))))
                .child(
                    div()
                        .font_family(theme.mono_font_family.clone())
                        .whitespace_nowrap()
                        .when(selected, |el| el.font_weight(FontWeight::SEMIBOLD))
                        .child(trace_label(&trace.expression, pane.y_scale)),
                )
                .children(attach);
            rows.push(
                row(
                    name,
                    [
                        value(readout.first),
                        value(readout.second),
                        value(readout.delta()),
                    ],
                    String::new(),
                    theme.foreground,
                    cx,
                )
                .into_any_element(),
            );
        }
    }
    rows
}

/// The `1`/`2` toggle that makes cursor `id` follow trace `trace` of pane
/// `pane`; selected while it does.
fn attach_button(
    id: CursorId,
    pane: usize,
    trace: usize,
    on: bool,
    cx: &mut Context<WaveformViewer>,
) -> AnyElement {
    let button_id = match id {
        CursorId::First => ("attach-cursor-1", pane * 1000 + trace),
        CursorId::Second => ("attach-cursor-2", pane * 1000 + trace),
    };
    let tooltip = match id {
        CursorId::First => "Cursor 1 follows this trace",
        CursorId::Second => "Cursor 2 follows this trace",
    };
    div()
        .debug_selector(move || format!("attach-cursor-{}-{pane}-{trace}", id.number()))
        .child(
            Button::new(button_id)
                .ghost()
                .xsmall()
                .label(id.number())
                .selected(on)
                .tooltip(tooltip)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.attach_cursor(id, pane, trace, cx);
                })),
        )
        .into_any_element()
}

fn swatch(color: Hsla) -> impl IntoElement {
    div()
        .flex_shrink_0()
        .w(rems(0.625))
        .h(rems(0.1875))
        .rounded_full()
        .bg(color)
}

/// The trace name, marking which Bode pane a value comes from.
fn trace_label(expression: &str, scale: YScale) -> String {
    match scale {
        YScale::Decibel => format!("{expression} (dB)"),
        YScale::Phase => format!("{expression} (phase)"),
        YScale::Linear | YScale::Log => expression.to_string(),
    }
}

fn row(
    name: impl IntoElement,
    values: [String; 3],
    extra: String,
    color: Hsla,
    cx: &App,
) -> impl IntoElement {
    let mono = cx.theme().mono_font_family.clone();
    let cell = move |value: String, width: f32| {
        div()
            .flex_shrink_0()
            .w(rems(width))
            .text_right()
            .font_family(mono.clone())
            .text_color(color)
            .child(value)
    };
    h_flex()
        .min_h(rems(1.375))
        .gap_2()
        .child(
            div()
                .flex_1()
                .min_w(rems(7.5))
                .overflow_x_hidden()
                .child(name),
        )
        .children(values.map(|value| cell(value, COLUMN)))
        .child(cell(extra, COLUMN * 1.3))
}

fn dash() -> String {
    "—".to_string()
}
