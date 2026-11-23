//! One plot pane: a header (legend chips, axis menu, close) above the plot
//! canvas, which takes the pointer input. The whole pane is a drop target
//! for legend chips dragged from another pane (D14).
//!
//! The header has no axis titles (D20): the tick labels carry the units.
//! The chips of the left axis's traces start the row, above the left axis;
//! those of a right axis (D9) end it, set apart by a divider.
//!
//! The panes share the stack's height in proportion to their stored
//! weights (D23), none below [`MIN_PANE_HEIGHT`]; every pane but the first
//! carries the divider to the pane above it (`super::divider`), which
//! changes those weights.

use super::canvas::{CursorMark, Overlay, PanePlot};
use super::divider::divider;
use super::legend::{DraggedTrace, chips};
use super::state::{Drag, DropTarget};
use crate::WaveformViewer;
use crate::model::cursors::CursorId;
use crate::model::setup::{self, MoveRefusal};
use crate::model::units::YAxis;
use crate::view_model::PlotModel;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, IconName, Sizable, h_flex, v_flex};
use gpui_kit::{prelude::*, *};
use std::sync::Arc;
use xsim_project::YScale;

/// Height of a pane header, in rems (Q30).
const HEADER_HEIGHT: f32 = 1.875;
/// The smallest pane height, in rems: room for the legend row, a plot
/// with a few labelled y ticks, and the x tick labels under it. A divider
/// drag stops there, and with more panes than fit at it the stack scrolls.
pub const MIN_PANE_HEIGHT: f32 = 7.5;
/// Width of the bar left of the active pane's header, in pixels.
const ACTIVE_MARK: f32 = 2.0;

/// Layout shared by every pane of a frame.
#[derive(Clone, Copy)]
pub struct PaneLayout {
    /// Left gutter for y labels, shared so x axes line up.
    pub left: f32,
    /// Right gutter for right-axis labels (D9), shared like `left`.
    pub right: f32,
    pub font_size: f32,
    pub count: usize,
}

/// Pane `index` with its header and canvas, taking `grow` shares of the
/// stack's height (D23).
pub fn pane(
    viewer: &WaveformViewer,
    index: usize,
    layout: PaneLayout,
    grow: f32,
    cx: &mut Context<WaveformViewer>,
) -> AnyElement {
    let theme = cx.theme().clone();
    let last = index + 1 == layout.count;
    let drop = viewer.view.drop_target.filter(|t| t.pane == index);
    v_flex()
        .id(("pane", index))
        .debug_selector(move || format!("pane-{index}"))
        .relative()
        .flex_1()
        .flex_grow(grow)
        .min_h(rems(MIN_PANE_HEIGHT))
        .when(!last, |el| el.border_b_1().border_color(theme.border))
        .child(header(viewer, index, layout, cx))
        .child(canvas_area(viewer, index, layout, cx))
        .when_some(drop, |el, target| el.child(drop_highlight(target, cx)))
        // Last, so it lies above the pane above's plot and this header. Not
        // while a legend chip is dragged: the band hides both panes from the
        // pointer, so a chip dropped on it would be lost (D14).
        .when(index > 0 && !cx.has_active_drag(), |el| {
            el.child(divider(viewer, index - 1, cx))
        })
        .on_drag_move(
            cx.listener(move |this, event: &DragMoveEvent<DraggedTrace>, _, cx| {
                // `bounds` is the whole pane, also where the scrolled stack
                // hides it under the toolbar, the readout or the plus; only
                // the part in view takes the chip, as only it takes the drop.
                let position = event.event.position;
                let inside = event.bounds.contains(&position)
                    && this.view.panes_scroll.bounds().contains(&position);
                this.drag_over_pane(index, event.drag(cx).clone(), inside, cx);
            }),
        )
        .on_drop(cx.listener(move |this, dragged: &DraggedTrace, _, cx| {
            this.drop_trace(index, dragged, cx);
        }))
        .into_any_element()
}

/// The frame over a pane a legend chip is dragged onto: the ring colour
/// when the trace may move here, the danger colour and the reason when it
/// may not (D9).
fn drop_highlight(target: DropTarget, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let pane = target.pane;
    let (color, selector) = match target.refusal {
        None => (theme.ring, format!("pane-drop-{pane}")),
        Some(_) => (theme.danger, format!("pane-drop-refused-{pane}")),
    };
    div()
        .debug_selector(move || selector.clone())
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .border_2()
        .border_color(color)
        .bg(color.opacity(0.08))
        .when_some(target.refusal, |el, refusal| {
            el.child(
                div()
                    .px_2()
                    .py_1()
                    .rounded(theme.radius)
                    .bg(theme.popover)
                    .border_1()
                    .border_color(theme.danger)
                    .text_xs()
                    .text_color(theme.danger)
                    .child(refusal.reason()),
            )
        })
}

fn header(
    viewer: &WaveformViewer,
    index: usize,
    layout: PaneLayout,
    cx: &mut Context<WaveformViewer>,
) -> impl IntoElement {
    let theme = cx.theme().clone();
    let active = layout.count > 1 && setup::active_pane(&viewer.setup) == index;
    let pane = &viewer.setup.panes[index];
    let right_axis = viewer
        .model()
        .is_some_and(|m| m.units(pane).right.is_some());
    // The left axis's traces start the row at the plot's left edge, above
    // the left axis, at every UI scale; a right axis's traces end it, above
    // that axis (D9).
    h_flex()
        .id(("pane-header", index))
        .debug_selector(move || format!("pane-header-{index}"))
        .h(rems(HEADER_HEIGHT))
        .flex_shrink_0()
        .pl(px((layout.left - ACTIVE_MARK).max(0.0)))
        .pr_2()
        .gap_2()
        .border_l(px(ACTIVE_MARK))
        .border_color(if active {
            theme.ring
        } else {
            gpui_kit::transparent_black()
        })
        .child(
            h_flex()
                .debug_selector(move || format!("pane-left-axis-{index}"))
                .flex_1()
                .min_w_0()
                .gap_1()
                .overflow_x_hidden()
                .children(chips(viewer, index, YAxis::Left, cx)),
        )
        .when(right_axis, |el| {
            el.child(
                h_flex()
                    .debug_selector(move || format!("pane-right-axis-{index}"))
                    .min_w_0()
                    .gap_1()
                    .pl_2()
                    .border_l_1()
                    .border_color(theme.border)
                    .overflow_x_hidden()
                    .children(chips(viewer, index, YAxis::Right, cx)),
            )
        })
        .child(axis_menu(viewer, index, cx))
        .when(layout.count > 1, |el| el.child(close_button(index, cx)))
        .on_click(cx.listener(move |this, _, _, cx| {
            this.edit_setup(cx, |s| setup::set_active(s, index));
        }))
}

/// The y-scale menu, with the shared x-axis scale.
fn axis_menu(
    viewer: &WaveformViewer,
    index: usize,
    cx: &mut Context<WaveformViewer>,
) -> impl IntoElement {
    let scale = viewer.setup.panes[index].y_scale;
    let x_log = viewer.model().is_some_and(|m| m.x_log());
    let entity = cx.entity();
    div()
        .debug_selector(move || format!("pane-axis-menu-{index}"))
        .child(
            Button::new(("pane-axis-menu", index))
                .ghost()
                .xsmall()
                .label(scale_name(scale))
                .dropdown_caret(true)
                .tooltip("Axis scales")
                .dropdown_menu(move |mut menu, _, _| {
                    menu = menu.item(PopupMenuItem::label("Y axis"));
                    for option in [YScale::Linear, YScale::Log, YScale::Decibel, YScale::Phase] {
                        let entity = entity.clone();
                        menu = menu.item(
                            PopupMenuItem::new(scale_name(option))
                                .checked(option == scale)
                                .on_click(move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.set_y_scale(index, option, cx);
                                    });
                                }),
                        );
                    }
                    let entity = entity.clone();
                    menu.item(PopupMenuItem::separator())
                        .item(PopupMenuItem::label("X axis (all panes)"))
                        .item(PopupMenuItem::new("Logarithmic").checked(x_log).on_click(
                            move |_, _, cx| {
                                entity.update(cx, |this, cx| {
                                    this.edit_setup(cx, |s| setup::set_x_log(s, Some(!x_log)));
                                });
                            },
                        ))
                }),
        )
}

/// The label of a y scale.
pub fn scale_name(scale: YScale) -> &'static str {
    match scale {
        YScale::Linear => "Linear",
        YScale::Log => "Log",
        YScale::Decibel => "dB",
        YScale::Phase => "Phase",
    }
}

fn close_button(index: usize, cx: &mut Context<WaveformViewer>) -> impl IntoElement {
    div()
        .debug_selector(move || format!("pane-close-{index}"))
        .child(
            Button::new(("pane-close", index))
                .ghost()
                .xsmall()
                .icon(IconName::Close)
                .tooltip("Remove pane")
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.remove_pane(index, cx);
                    cx.stop_propagation();
                })),
        )
}

/// The plot with its pointer handlers, or a hint when the pane is empty.
fn canvas_area(
    viewer: &WaveformViewer,
    index: usize,
    layout: PaneLayout,
    cx: &mut Context<WaveformViewer>,
) -> impl IntoElement {
    let empty = viewer.setup.panes[index].traces.is_empty();
    div()
        .id(("pane-canvas", index))
        .debug_selector(move || format!("pane-canvas-{index}"))
        .relative()
        .flex_1()
        .min_h_0()
        .overflow_hidden()
        .cursor(pointer_style(viewer, index))
        .when_some(
            viewer.model().zip(viewer.result.clone()),
            |el, (model, result)| el.child(plot(viewer, model, result, index, layout)),
        )
        .when(empty, |el| el.child(empty_hint(cx)))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, e, window, cx| this.pointer_down(index, e, window, cx)),
        )
        .on_mouse_down(
            MouseButton::Middle,
            cx.listener(move |this, e, window, cx| this.pan_down(index, e, window, cx)),
        )
        .on_mouse_move(cx.listener(move |this, e, _, cx| this.hover(index, e, cx)))
        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
            if !hovered {
                this.hover_end(index, cx);
            }
        }))
        .on_scroll(cx.listener(move |this, e, _, cx| {
            this.scroll(index, e, cx);
            // The wheel over a plot zooms; it never also scrolls the stack.
            cx.stop_propagation();
        }))
}

pub(super) fn plot(
    viewer: &WaveformViewer,
    model: PlotModel,
    result: Arc<xsim_spice_engine::SimulationResult>,
    index: usize,
    layout: PaneLayout,
) -> PanePlot {
    let x_title = model.x().title;
    PanePlot {
        index,
        x_range: model.x_range(),
        // A sweep's source name goes under the bottom pane only.
        x_title: (index + 1 == layout.count && !x_title.is_empty()).then_some(x_title),
        pane: model.pane(index),
        result,
        left: layout.left,
        right: layout.right,
        colors: viewer.colors.clone(),
        font_size: layout.font_size,
        overlay: overlay(viewer, index),
        lod: viewer.view.lod.clone(),
        geometry: viewer.view.geometry.clone(),
    }
}

/// The cursors' lines and the hover line show in every pane, the same in
/// each: a cursor is only an x (D19). The zoom box shows only in the pane
/// being dragged.
pub(crate) fn overlay(viewer: &WaveformViewer, index: usize) -> Overlay {
    let placed = CursorId::ALL
        .into_iter()
        .filter_map(|id| viewer.cursor_point(id))
        .map(|point| CursorMark {
            id: point.id,
            x: point.x,
        })
        .collect();
    // No hover line while a divider is dragged: the pointer is resizing.
    let resizing = matches!(viewer.view.drag, Some(Drag::Divider { .. }));
    let crosshair = viewer.view.hover.filter(|_| !resizing).and_then(|hover| {
        let geometry = viewer.view.pane_geometry(hover.pane)?;
        geometry
            .plot
            .contains(&hover.position)
            .then(|| geometry.x.value(hover.position.x.as_f32()))
    });
    let zoom_box = match viewer.view.drag {
        Some(Drag::Zoom {
            pane,
            start,
            current,
        }) if pane == index => Some((start, current)),
        _ => None,
    };
    Overlay {
        cursors: placed,
        crosshair,
        zoom_box,
    }
}

/// The mouse pointer over pane `index`: a hand while panning, a left-right
/// arrow over (or while dragging) a cursor line, an up-down arrow while a
/// divider is dragged (D23), a crosshair for the magnifier's box, and the
/// arrow otherwise.
pub(crate) fn pointer_style(viewer: &WaveformViewer, index: usize) -> CursorStyle {
    let over_line = || {
        let hover = viewer.view.hover.filter(|h| h.pane == index)?;
        viewer.cursor_hit(index, hover.position)
    };
    match viewer.view.drag {
        Some(Drag::Pan { .. }) => CursorStyle::ClosedHand,
        Some(Drag::Cursor(_)) => CursorStyle::ResizeLeftRight,
        Some(Drag::Zoom { .. }) => CursorStyle::Crosshair,
        Some(Drag::Divider { .. }) => CursorStyle::ResizeRow,
        _ if viewer.view.space_held => CursorStyle::OpenHand,
        _ if over_line().is_some() => CursorStyle::ResizeLeftRight,
        _ if viewer.view.magnifier => CursorStyle::Crosshair,
        _ => CursorStyle::Arrow,
    }
}

fn empty_hint(cx: &App) -> impl IntoElement {
    div()
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .px_6()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child("Click a wire or pin in the schematic, or type an expression above, to add a trace.")
}

impl WaveformViewer {
    /// Removes pane `index` (the last pane stays).
    pub(crate) fn remove_pane(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.setup.panes.len() <= 1 || index >= self.setup.panes.len() {
            return;
        }
        self.view.selected = match self.view.selected {
            Some((p, _)) if p == index => None,
            Some((p, t)) if p > index => Some((p - 1, t)),
            other => other,
        };
        self.cursor_traces_after_pane_removal(index);
        self.view.geometry.borrow_mut().clear();
        self.view.divider_hover = None;
        self.edit_setup(cx, |s| setup::remove_pane(s, index));
    }

    /// A legend chip dragged over the viewer: pane `index` is the drop
    /// target while the pointer is `inside` it, unless the chip came from
    /// there.
    pub(crate) fn drag_over_pane(
        &mut self,
        index: usize,
        dragged: DraggedTrace,
        inside: bool,
        cx: &mut Context<Self>,
    ) {
        let target = match setup::can_move_trace(&self.setup, dragged.pane, dragged.trace, index) {
            _ if !inside || dragged.viewer != cx.entity_id() => None,
            Err(MoveRefusal::SamePane | MoveRefusal::Missing) => None,
            Err(refusal) => Some(DropTarget {
                pane: index,
                refusal: Some(refusal),
            }),
            Ok(()) => Some(DropTarget {
                pane: index,
                refusal: None,
            }),
        };
        let current = self.view.drop_target;
        let next = match target {
            Some(target) => Some(target),
            None if current.is_some_and(|t| t.pane == index) => None,
            None => current,
        };
        // The dividers make way for the chip (D23), and so does their grip:
        // a band left out while hovered never hears that the pointer left.
        let unhovered = self.view.divider_hover.take().is_some();
        if next != current || unhovered {
            self.view.drop_target = next;
            cx.notify();
        }
    }

    /// A legend chip dropped on pane `index`: moves its trace there when the
    /// pane accepts it (D9); the source pane and refusing panes keep
    /// everything as it is. The selection and the cursors follow the trace.
    pub(crate) fn drop_trace(
        &mut self,
        index: usize,
        dragged: &DraggedTrace,
        cx: &mut Context<Self>,
    ) {
        self.view.drop_target = None;
        cx.notify();
        if dragged.viewer != cx.entity_id() {
            return;
        }
        let (from, trace) = (dragged.pane, dragged.trace);
        let Some(expression) = self
            .setup
            .panes
            .get(from)
            .and_then(|p| p.traces.get(trace))
            .map(|t| t.expression.clone())
            .filter(|e| *e == dragged.expression)
        else {
            return;
        };
        if setup::can_move_trace(&self.setup, from, trace, index).is_err() {
            return;
        }
        // Before the edit, which re-pins cursors whose trace is gone.
        self.cursor_traces_after_move(from, index, &expression);
        let selected = self.view.selected;
        let mut moved = None;
        self.edit_setup(cx, |s| {
            moved = setup::move_trace(s, from, trace, index).ok();
            moved.is_some()
        });
        let Some(new_index) = moved else {
            return;
        };
        self.view.selected = match selected {
            Some((p, t)) if p == from && t == trace => Some((index, new_index)),
            Some((p, t)) if p == from && t > trace => Some((p, t - 1)),
            other => other,
        };
        self.validate_selection();
    }

    /// Switches the y scale of pane `index`. Traces that no longer fit (a
    /// third unit on a linear axis) move to other panes, so a selection in
    /// that pane is dropped rather than left on a shifted index.
    pub(crate) fn set_y_scale(&mut self, index: usize, scale: YScale, cx: &mut Context<Self>) {
        let count = self.setup.panes.get(index).map(|p| p.traces.len());
        let selected_here = self.view.selected.is_some_and(|(pane, _)| pane == index);
        self.edit_setup(cx, |s| setup::set_y_scale(s, index, scale));
        let moved = self.setup.panes.get(index).map(|p| p.traces.len()) != count;
        if moved && selected_here {
            self.view.selected = None;
            self.validate_selection();
        }
    }

    /// Adds an empty pane below the others and makes it active. When the
    /// stack scrolls, the new pane is scrolled into view (D22).
    pub(crate) fn add_pane(&mut self, cx: &mut Context<Self>) {
        self.edit_setup(cx, |s| {
            setup::add_pane(s);
            true
        });
        self.view.panes_scroll.scroll_to_bottom();
    }
}
