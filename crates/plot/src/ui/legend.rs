//! Legend chips: colour swatch, expression, visibility toggle and remove.
//! Clicking a chip selects the trace for measurements; dragging it onto
//! another pane moves the trace there (D14); traces whose signals are
//! missing from the result are greyed with the reason in a tooltip.

use super::theme::hsla;
use crate::WaveformViewer;
use crate::model::setup;
use crate::model::units::{PaneUnits, YAxis};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, IconName, Sizable, h_flex};
use gpui_kit::{prelude::*, *};
use xsim_project::Trace;

/// A legend chip being dragged to another pane.
#[derive(Debug, Clone, PartialEq)]
pub struct DraggedTrace {
    /// The viewer the chip belongs to; other viewers ignore it.
    pub viewer: EntityId,
    pub pane: usize,
    pub trace: usize,
    pub expression: String,
    pub color: Hsla,
}

/// The chip that follows the pointer while it is dragged.
pub struct TraceDragPreview {
    trace: DraggedTrace,
}

impl Render for TraceDragPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        h_flex()
            .h(rems(1.375))
            .px_1p5()
            .gap_1()
            .rounded(theme.radius)
            .border_1()
            .border_color(theme.ring)
            .bg(theme.popover)
            .shadow_md()
            .opacity(0.92)
            .child(
                div()
                    .w(rems(0.75))
                    .h(rems(0.1875))
                    .rounded_full()
                    .bg(self.trace.color),
            )
            .child(
                div()
                    .text_xs()
                    .font_family(theme.mono_font_family.clone())
                    .text_color(theme.popover_foreground)
                    .child(self.trace.expression.clone()),
            )
    }
}

/// The chips of the traces of pane `pane` drawn against `axis` (D9), in
/// pane order; the header groups the right axis's chips at its right end.
pub fn chips(
    viewer: &WaveformViewer,
    pane: usize,
    axis: YAxis,
    cx: &mut Context<WaveformViewer>,
) -> Vec<AnyElement> {
    let Some(p) = viewer.setup.panes.get(pane) else {
        return Vec::new();
    };
    let units = PaneUnits::of(p, |expression| viewer.traces.unit(expression));
    let on_axis = |trace: &Trace| {
        units
            .axis(viewer.traces.unit(&trace.expression))
            .unwrap_or(YAxis::Left)
            == axis
    };
    (0..p.traces.len())
        .filter(|&trace| on_axis(&p.traces[trace]))
        .map(|trace| chip(viewer, pane, trace, cx).into_any_element())
        .collect()
}

fn chip(
    viewer: &WaveformViewer,
    pane: usize,
    index: usize,
    cx: &mut Context<WaveformViewer>,
) -> impl IntoElement {
    let theme = cx.theme().clone();
    let trace = &viewer.setup.panes[pane].traces[index];
    let error = viewer
        .traces
        .error(&trace.expression)
        .map(|e| e.to_string());
    let available = viewer.result.is_none() || error.is_none();
    let selected = viewer.view.selected == Some((pane, index));
    let color = if available && trace.visible {
        hsla(viewer.colors.trace_color(trace, index))
    } else {
        theme.muted_foreground.opacity(0.6)
    };
    let text_color = if available && trace.visible {
        theme.foreground
    } else {
        theme.muted_foreground
    };
    let dragged = DraggedTrace {
        viewer: cx.entity_id(),
        pane,
        trace: index,
        expression: trace.expression.clone(),
        color,
    };
    h_flex()
        .id(("trace-chip", pane * 1000 + index))
        .debug_selector(move || format!("trace-chip-{pane}-{index}"))
        .h(rems(1.375))
        .pl_1p5()
        .gap_1()
        .rounded(theme.radius)
        .border_1()
        .border_color(if selected {
            theme.ring
        } else {
            gpui_kit::transparent_black()
        })
        .when(selected, |el| el.bg(theme.accent))
        .hover(|el| el.bg(theme.accent))
        .cursor_pointer()
        .child(div().w(px(12.)).h(px(3.)).rounded_full().bg(color))
        .child(
            div()
                .text_xs()
                .font_family(theme.mono_font_family.clone())
                .text_color(text_color)
                .when(!available, |el| el.line_through())
                .child(trace.expression.clone()),
        )
        .child(visibility_button(pane, index, trace.visible, cx))
        .child(remove_button(pane, index, cx))
        // Focus the viewer, so Delete and Escape act on the chip's trace.
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| this.focus.focus(window, cx)),
        )
        .on_click(cx.listener(move |this, _, _, cx| this.select_trace(pane, index, cx)))
        .on_drag(dragged, |dragged, _, _, cx| {
            cx.new(|_| TraceDragPreview {
                trace: dragged.clone(),
            })
        })
        .when_some(error, |el, error| {
            el.tooltip(move |window, cx| Tooltip::new(error.clone()).build(window, cx))
        })
}

fn visibility_button(
    pane: usize,
    index: usize,
    visible: bool,
    cx: &mut Context<WaveformViewer>,
) -> impl IntoElement {
    let icon = if visible {
        IconName::Eye
    } else {
        IconName::EyeOff
    };
    let tooltip = if visible { "Hide trace" } else { "Show trace" };
    div()
        .debug_selector(move || format!("trace-visibility-{pane}-{index}"))
        .on_mouse_down(MouseButton::Left, cx.listener(press_on_chip_button))
        .child(
            Button::new(("trace-visibility", pane * 1000 + index))
                .ghost()
                .xsmall()
                .icon(icon)
                .tooltip(tooltip)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.edit_setup(cx, |s| setup::toggle_visible(s, pane, index));
                    cx.stop_propagation();
                })),
        )
}

fn remove_button(pane: usize, index: usize, cx: &mut Context<WaveformViewer>) -> impl IntoElement {
    div()
        .debug_selector(move || format!("trace-remove-{pane}-{index}"))
        .on_mouse_down(MouseButton::Left, cx.listener(press_on_chip_button))
        .child(
            Button::new(("trace-remove", pane * 1000 + index))
                .ghost()
                .xsmall()
                .icon(IconName::Close)
                .tooltip("Remove trace")
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.remove_trace(pane, index, cx);
                    cx.stop_propagation();
                })),
        )
}

/// A press on a chip's hide or remove button belongs to the button: it
/// focuses the viewer like a press on the chip, but it does not reach the
/// chip, so moving the pointer with the button held never drags the chip
/// (the button's own click still fires on release over it).
fn press_on_chip_button(
    viewer: &mut WaveformViewer,
    _: &MouseDownEvent,
    window: &mut Window,
    cx: &mut Context<WaveformViewer>,
) {
    viewer.focus.focus(window, cx);
    cx.stop_propagation();
}

impl WaveformViewer {
    /// Selects a trace for the measurements panel and the Δy readout.
    pub(crate) fn select_trace(&mut self, pane: usize, trace: usize, cx: &mut Context<Self>) {
        self.edit_setup(cx, |s| setup::set_active(s, pane));
        // Shown cursors stay on the trace they follow (D14).
        self.pin_cursor_traces();
        if self.plottable(pane, trace) {
            self.view.selected = Some((pane, trace));
        }
        cx.notify();
    }

    /// Removes a trace; the selection moves to a remaining one.
    pub(crate) fn remove_trace(&mut self, pane: usize, trace: usize, cx: &mut Context<Self>) {
        self.view.selected = match self.view.selected {
            Some((p, t)) if p == pane && t == trace => None,
            Some((p, t)) if p == pane && t > trace => Some((p, t - 1)),
            other => other,
        };
        self.edit_setup(cx, |s| setup::remove_trace(s, pane, trace));
    }
}
