//! The GPUI view: toolbar, stacked panes with the dividers between them
//! (D23), cursor readout, the "Add pane" strip at the bottom (D22),
//! measurements panel, operating-point table and empty state.

pub mod actions;
mod canvas;
mod cursor;
mod divider;
mod empty;
mod footer;
mod interaction;
mod legend;
mod measurements;
mod op_table;
mod pane;
mod readout;
pub mod state;
pub mod theme;
mod toolbar;
mod trace_input;

#[cfg(all(test, debug_assertions))]
mod tests;

use crate::WaveformViewer;
use crate::model::heights;
use crate::model::op_table::is_operating_point;
use crate::scene::layout::{RIGHT_GUTTER, Style, y_gutter, y2_gutter};
use actions::{CONTEXT, CancelInteraction, CopyPlot, RemoveSelectedTrace, ZoomToFit};
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::{prelude::*, *};
use pane::PaneLayout;

/// Plot height assumed for the y-label gutter before the first paint.
const DEFAULT_PLOT_HEIGHT: f32 = 200.0;

/// Renders the whole viewer.
pub fn render(
    viewer: &mut WaveformViewer,
    window: &mut Window,
    cx: &mut Context<WaveformViewer>,
) -> impl IntoElement {
    viewer.colors = theme::plot_colors(cx.theme());
    viewer.ensure_trace_input(window, cx);
    let theme = cx.theme().clone();
    v_flex()
        .id("waveform-viewer")
        .key_context(CONTEXT)
        .track_focus(&viewer.focus)
        .size_full()
        .min_w_0()
        .min_h_0()
        .bg(theme.background)
        .text_color(theme.foreground)
        .on_action(cx.listener(|this, _: &CopyPlot, _, cx| this.copy_plot(cx)))
        .on_action(cx.listener(|this, _: &ZoomToFit, _, cx| this.zoom_to_fit(cx)))
        .on_action(cx.listener(|this, _: &RemoveSelectedTrace, _, cx| this.remove_selected(cx)))
        .on_action(cx.listener(|this, _: &CancelInteraction, window, cx| {
            this.cancel_interaction(window, cx)
        }))
        .on_key_down(cx.listener(WaveformViewer::key_down))
        .on_key_up(cx.listener(WaveformViewer::key_up))
        .on_mouse_move(cx.listener(|this, e, _, cx| this.drag_move(e, cx)))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, e: &MouseUpEvent, _, cx| this.pointer_up(e.position, cx)),
        )
        .on_mouse_up_out(
            MouseButton::Left,
            cx.listener(|this, e: &MouseUpEvent, _, cx| this.pointer_up(e.position, cx)),
        )
        .on_mouse_up(
            MouseButton::Middle,
            cx.listener(|this, _, _, cx| this.cancel_drag(cx)),
        )
        .on_mouse_up_out(
            MouseButton::Middle,
            cx.listener(|this, _, _, cx| this.cancel_drag(cx)),
        )
        .child(toolbar::toolbar(viewer, window, cx))
        .child(body(viewer, window, cx))
        .when(
            matches!(viewer.view.drag, Some(state::Drag::Divider { .. })),
            |el| el.child(resize_pointer()),
        )
}

/// Keeps the row-resize pointer over the whole window while a divider is
/// dragged (D23), wherever the pointer goes.
fn resize_pointer() -> impl IntoElement {
    canvas(
        |_, _, _| {},
        |_, _, window, _| window.set_window_cursor_style(CursorStyle::ResizeRow),
    )
    .absolute()
    .size_0()
}

/// Empty state, operating-point table, or the plot panes.
fn body(viewer: &WaveformViewer, window: &Window, cx: &mut Context<WaveformViewer>) -> AnyElement {
    match viewer.result.as_deref() {
        None => empty::no_result(cx),
        Some(result) if is_operating_point(result) => op_table::table(result, cx),
        Some(_) => plot_area(viewer, window, cx),
    }
}

/// The pane stack, the cursor readout and the "Add pane" strip at the very
/// bottom (D22), with the measurements panel beside them.
fn plot_area(
    viewer: &WaveformViewer,
    window: &Window,
    cx: &mut Context<WaveformViewer>,
) -> AnyElement {
    let layout = pane_layout(viewer, window);
    h_flex()
        .flex_1()
        .min_h_0()
        .min_w_0()
        .items_stretch()
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .min_h_0()
                .child(pane_stack(viewer, layout, cx))
                .children(readout::readout(viewer, cx))
                .child(footer::footer(viewer, cx)),
        )
        .children(measurements::panel(viewer, cx))
        .into_any_element()
}

/// The panes, sharing the height left by the readout and the bottom strip
/// in proportion to their weights (D23), so a taller or shorter dock scales
/// them all alike. A pane never gets shorter than its minimum, so with more
/// panes than fit the stack scrolls (over a pane's header; the wheel over a
/// plot zooms).
fn pane_stack(
    viewer: &WaveformViewer,
    layout: PaneLayout,
    cx: &mut Context<WaveformViewer>,
) -> impl IntoElement {
    let grow = heights::grow_factors(&viewer.pane_weights());
    v_flex()
        .id("waveform-panes")
        .debug_selector(|| "waveform-panes".into())
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .track_scroll(&viewer.view.panes_scroll)
        .children((0..layout.count).map(|i| {
            let grow = grow.get(i).copied().unwrap_or(1.0);
            pane::pane(viewer, i, layout, grow, cx)
        }))
}

/// The y-label gutters every pane shares, so their x axes line up: left
/// for the left axes, right for the right axes of mixed-unit panes (D9).
fn pane_layout(viewer: &WaveformViewer, window: &Window) -> PaneLayout {
    let font_size = theme::label_size(window);
    let count = viewer.setup.panes.len();
    let (left, right) = viewer
        .model()
        .map_or((font_size * 4.0, RIGHT_GUTTER), |model| {
            let style = Style {
                colors: &viewer.colors,
                font_size,
                line_width: 1.0,
            };
            (0..count)
                .map(|i| {
                    let height = viewer
                        .view
                        .pane_geometry(i)
                        .map_or(DEFAULT_PLOT_HEIGHT, |g| g.plot.size.height.as_f32());
                    let pane = model.pane(i);
                    (
                        y_gutter(&pane, height, &style),
                        y2_gutter(&pane, height, &style),
                    )
                })
                .fold((0.0, RIGHT_GUTTER), |(l, r), (a, b)| (l.max(a), r.max(b)))
        });
    PaneLayout {
        left,
        right,
        font_size,
        count,
    }
}

impl WaveformViewer {
    /// Delete/Backspace: removes the selected trace.
    fn remove_selected(&mut self, cx: &mut Context<Self>) {
        if let Some((pane, trace)) = self.view.selected {
            self.remove_trace(pane, trace, cx);
        }
    }
}
