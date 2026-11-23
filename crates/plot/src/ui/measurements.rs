//! The measurements panel (Q135): statistics and timing of the selected
//! trace over the cursor window, or over the visible range.

use crate::WaveformViewer;
use crate::model::format;
use crate::model::measurements::{Measurement, measure_all};
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::{prelude::*, *};
use std::rc::Rc;

/// Width of the panel, in rems (Q30).
const WIDTH: f32 = 15.625;

/// What the panel measures: a trace over a window.
struct Subject {
    name: String,
    scope: &'static str,
    rows: Rc<Vec<Measurement>>,
}

/// The panel, when open.
pub fn panel(viewer: &WaveformViewer, cx: &App) -> Option<AnyElement> {
    if !viewer.view.measurements_open {
        return None;
    }
    let theme = cx.theme();
    let body = match subject(viewer) {
        Some(subject) => content(subject, cx).into_any_element(),
        None => div()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child("Select a trace in a legend to measure it.")
            .into_any_element(),
    };
    Some(
        v_flex()
            .id("measurements-panel")
            .debug_selector(|| "measurements-panel".into())
            .w(rems(WIDTH))
            .flex_shrink_0()
            .h_full()
            .overflow_y_scroll()
            .gap_2()
            .p_3()
            .border_l_1()
            .border_color(theme.border)
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Measurements"),
            )
            .child(body)
            .into_any_element(),
    )
}

/// The selected trace measured over the cursor window or the visible range.
fn subject(viewer: &WaveformViewer) -> Option<Subject> {
    let (pane, index) = viewer.view.selected?;
    let model = viewer.model()?;
    let p = viewer.setup.panes.get(pane)?;
    let trace = p.traces.get(index)?;
    let values = viewer.traces.values(&trace.expression, p.y_scale)?;
    let cursors = &viewer.view.cursors;
    let (window, scope) = match cursors.window().filter(|_| cursors.enabled) {
        Some(window) => (window, "Between cursors"),
        None => {
            let range = model.x_range();
            ((range.min, range.max), "Visible range")
        }
    };
    let axis = &model.result.axis;
    let key = (values.id, window.0.to_bits(), window.1.to_bits());
    let mut cache = viewer.view.measured.borrow_mut();
    let rows = match cache.as_ref() {
        Some((cached, rows)) if *cached == key => rows.clone(),
        _ => {
            let rows = Rc::new(measure_all(
                &axis.values,
                &values.y,
                Some(window),
                &axis.unit,
                &values.unit,
            ));
            *cache = Some((key, rows.clone()));
            rows
        }
    };
    Some(Subject {
        name: trace.expression.clone(),
        scope,
        rows,
    })
}

fn content(subject: Subject, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    v_flex()
        .gap_1()
        .child(
            v_flex()
                .child(
                    div()
                        .text_sm()
                        .font_family(theme.mono_font_family.clone())
                        .child(subject.name),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(subject.scope),
                ),
        )
        .children(subject.rows.iter().map(|row| measurement_row(row, cx)))
}

fn measurement_row(row: &Measurement, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let value = row
        .value
        .filter(|v| v.is_finite())
        .map(|v| format::value(v, &row.unit))
        .unwrap_or_else(|| "—".to_string());
    h_flex()
        .h(rems(1.375))
        .justify_between()
        .text_xs()
        .child(div().text_color(theme.muted_foreground).child(row.name))
        .child(
            div()
                .font_family(theme.mono_font_family.clone())
                .child(value),
        )
}
