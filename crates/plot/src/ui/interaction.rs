//! Mouse and keyboard handling of the plot panes (Q131, D14, D19): wheel
//! zoom at the pointer, left-drag pan (or box zoom with the magnifier tool),
//! middle/Space-drag pan, double-click to fit, cursor placement and cursor
//! drags along x, and the synchronized hover line.
//!
//! Zooming out and panning never show more than the data: the x axis stops
//! at the full data extent and each y axis at its automatic range
//! ([`crate::model::viewport`]).
//!
//! The magnifier stays on after a box zoom, so several zooms in a row need
//! one click on the tool; Escape (or the tool again) turns it off. Escape
//! first cancels a drag in progress and puts back what it changed: a box
//! zoom is not applied, a pan returns to the view it started from (also an
//! automatic axis to automatic), and a cursor drag returns the cursor to
//! where it was.
//!
//! A drag belongs to the button that began it: pressing or releasing the
//! other button (left or middle) meanwhile does nothing, so the drag goes
//! on, ends with its own button's release, and Escape still knows where it
//! began.

use super::state::{Drag, DragOrigin, Hover, PaneGeometry};
use crate::WaveformViewer;
use crate::model::axis::AxisRange;
use crate::model::setup;
use crate::model::viewport::{bounded, pan_within, select, wheel_factor, zoom_within};
use gpui_kit::*;
use xsim_project::PlotSetup;

/// Pixels the pointer must travel before a press becomes a drag.
const DRAG_THRESHOLD: f32 = 4.0;
/// Pixels per scrolled line, for wheels that report lines.
const LINE_HEIGHT: f32 = 20.0;

/// A new range for one axis: `None` leaves it, `Some(None)` returns it to
/// its automatic range.
type AxisUpdate = Option<Option<AxisRange>>;

impl WaveformViewer {
    /// Left press on pane `pane`.
    pub(crate) fn pointer_down(
        &mut self,
        pane: usize,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.dragging_with_another_button(MouseButton::Left) {
            return;
        }
        // Space is only seen while the viewer has focus: a press arriving
        // from another view may follow a Space release the viewer missed.
        if !self.focus.contains_focused(window, cx) {
            self.view.space_held = false;
        }
        self.focus.focus(window, cx);
        self.edit_setup(cx, |s| setup::set_active(s, pane));
        if event.click_count >= 2 {
            // The first click of the pair may have moved a cursor; undo that.
            if let Some(cursors) = self.view.cursors_before_click.take() {
                self.view.cursors = cursors;
            }
            self.end_drag();
            self.zoom_to_fit(cx);
            return;
        }
        self.view.cursors_before_click = None;
        if self.view.pane_geometry(pane).is_none() {
            return;
        }
        // Cursor lines can be dragged whichever tool is active.
        let drag = if self.view.space_held {
            Drag::Pan {
                pane,
                last: event.position,
            }
        } else if let Some(id) = self.cursor_hit(pane, event.position) {
            Drag::Cursor(id)
        } else {
            Drag::Pending {
                pane,
                start: event.position,
            }
        };
        self.begin_drag(drag, MouseButton::Left);
        cx.notify();
    }

    /// Middle press: start panning pane `pane`.
    pub(crate) fn pan_down(
        &mut self,
        pane: usize,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.dragging_with_another_button(MouseButton::Middle) {
            return;
        }
        self.focus.focus(window, cx);
        self.begin_drag(
            Drag::Pan {
                pane,
                last: event.position,
            },
            MouseButton::Middle,
        );
    }

    /// Pointer movement over pane `pane`: the hover line follows it.
    pub(crate) fn hover(&mut self, pane: usize, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let hover = Some(Hover {
            pane,
            position: event.position,
        });
        if self.view.hover != hover {
            self.view.hover = hover;
            cx.notify();
        }
    }

    /// The pointer left pane `pane`.
    pub(crate) fn hover_end(&mut self, pane: usize, cx: &mut Context<Self>) {
        if self.view.hover.is_some_and(|h| h.pane == pane) {
            self.view.hover = None;
            cx.notify();
        }
    }

    /// Pointer movement anywhere over the viewer continues a drag. A left
    /// press that moves far enough pans, or draws a zoom box while the
    /// magnifier is on.
    pub(crate) fn drag_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(drag) = self.view.drag else {
            return;
        };
        if event.pressed_button.is_none() {
            self.end_drag();
            cx.notify();
            return;
        }
        let position = event.position;
        match drag {
            Drag::Pending { pane, start } if distance(start, position) >= DRAG_THRESHOLD => {
                if self.view.magnifier {
                    self.view.drag = Some(Drag::Zoom {
                        pane,
                        start,
                        current: position,
                    });
                } else {
                    self.pan_by(pane, start, position, cx);
                    self.view.drag = Some(Drag::Pan {
                        pane,
                        last: position,
                    });
                }
            }
            Drag::Pending { .. } => return,
            Drag::Zoom { pane, start, .. } => {
                self.view.drag = Some(Drag::Zoom {
                    pane,
                    start,
                    current: position,
                });
            }
            Drag::Pan { pane, last } => {
                self.pan_by(pane, last, position, cx);
                self.view.drag = Some(Drag::Pan {
                    pane,
                    last: position,
                });
            }
            Drag::Cursor(id) => self.move_cursor_x(id, position),
        }
        cx.notify();
    }

    /// Left release: finishes a box zoom, or treats a press without
    /// movement as a click that places the nearer cursor. Also ends a
    /// legend-chip drag that was not dropped on a pane. A middle-button pan
    /// goes on.
    pub(crate) fn pointer_up(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        if self.view.drop_target.take().is_some() {
            cx.notify();
        }
        if self.dragging_with_another_button(MouseButton::Left) {
            return;
        }
        let Some(drag) = self.end_drag() else {
            return;
        };
        match drag {
            Drag::Zoom { pane, start, .. } => self.box_zoom(pane, start, position, cx),
            Drag::Pending { pane, .. } => self.click(pane, position),
            Drag::Pan { .. } | Drag::Cursor(_) => {}
        }
        cx.notify();
    }

    /// Middle release: ends the middle-button pan where it is, keeping the
    /// view it reached. A drag the left button began goes on.
    pub(crate) fn cancel_drag(&mut self, cx: &mut Context<Self>) {
        if self.dragging_with_another_button(MouseButton::Middle) {
            return;
        }
        if self.end_drag().is_some() {
            cx.notify();
        }
    }

    /// Escape: cancels a drag in progress (a plot gesture or a legend chip)
    /// and puts back what it changed, otherwise turns the magnifier off.
    pub(crate) fn cancel_interaction(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let chip = cx.stop_active_drag(window);
        if chip {
            self.view.drop_target = None;
        }
        let origin = self.view.drag_origin.take();
        match self.end_drag() {
            Some(drag) => {
                if let Some(origin) = origin {
                    self.undo_drag(drag, origin, cx);
                }
                cx.notify();
            }
            None if chip => cx.notify(),
            None if self.view.magnifier => self.set_magnifier(false, cx),
            None => {}
        }
    }

    /// Puts back what `drag` changed since it began at `origin`.
    fn undo_drag(&mut self, drag: Drag, origin: DragOrigin, cx: &mut Context<Self>) {
        match drag {
            Drag::Pan { .. } => {
                self.edit_setup(cx, |s| restore_ranges(s, &origin));
            }
            Drag::Cursor(_) => {
                self.view.cursors.first = origin.cursors.first;
                self.view.cursors.second = origin.cursors.second;
            }
            // Nothing is applied before the release.
            Drag::Pending { .. } | Drag::Zoom { .. } => {}
        }
    }

    /// Starts `drag`, begun by a press of `button`, remembering what it may
    /// change.
    fn begin_drag(&mut self, drag: Drag, button: MouseButton) {
        self.view.drag = Some(drag);
        self.view.drag_origin = Some(DragOrigin::of(button, &self.setup, self.view.cursors));
    }

    /// Whether a drag that a button other than `button` began is in
    /// progress. Presses and releases of `button` then leave it alone, so
    /// a second button can neither take the drag over (and lose where it
    /// began) nor end it.
    fn dragging_with_another_button(&self, button: MouseButton) -> bool {
        self.view.drag.is_some()
            && self
                .view
                .drag_origin
                .as_ref()
                .is_some_and(|origin| origin.button != button)
    }

    /// Ends the drag in progress, if any, and forgets where it began.
    fn end_drag(&mut self) -> Option<Drag> {
        self.view.drag_origin = None;
        self.view.drag.take()
    }

    /// Turns the magnifier (box zoom) tool on or off.
    pub(crate) fn set_magnifier(&mut self, on: bool, cx: &mut Context<Self>) {
        self.view.magnifier = on;
        if !on && matches!(self.view.drag, Some(Drag::Zoom { .. })) {
            self.end_drag();
        }
        cx.notify();
    }

    /// Wheel over pane `pane`: zooms x at the pointer, or y with Shift/Ctrl.
    /// A y zoom over the plot scales both y axes of a mixed-unit pane (D9)
    /// about the pointer; over the left or right axis labels it scales only
    /// that axis. Zooming out stops at the data (D14).
    pub(crate) fn scroll(&mut self, pane: usize, event: &ScrollEvent, cx: &mut Context<Self>) {
        let (Some(geometry), Some(model)) = (self.view.pane_geometry(pane), self.model()) else {
            return;
        };
        let delta = event.delta.pixel_delta(px(LINE_HEIGHT));
        // Shift+wheel arrives as a horizontal scroll on some platforms.
        let amount = if delta.y != px(0.) { delta.y } else { delta.x };
        // A scroll event without movement (a trackpad gesture starting or
        // ending) zooms nothing and must not store a range.
        if amount == px(0.) {
            return;
        }
        let factor = wheel_factor(f64::from(amount.as_f32() / LINE_HEIGHT));
        let modifiers = event.modifiers;
        if modifiers.shift || modifiers.control || modifiers.platform {
            let anchor = geometry.y.fraction_at(event.position.y.as_f32());
            let x = event.position.x;
            let (left, right) = match geometry.y2 {
                None => (true, false),
                Some(_) if x < geometry.plot.left() => (true, false),
                Some(_) if x > geometry.plot.right() => (false, true),
                Some(_) => (true, true),
            };
            let y =
                left.then(|| zoom_within(model.y_range(pane), model.y_limit(pane), anchor, factor));
            let y2 = model
                .y2_range(pane)
                .zip(model.y2_limit(pane))
                .filter(|_| right)
                .map(|(range, limit)| zoom_within(range, limit, anchor, factor));
            self.edit_setup(cx, |s| set_y_ranges(s, pane, y, y2));
        } else {
            let anchor = geometry.x.fraction_at(event.position.x.as_f32());
            let range = zoom_within(model.x_range(), model.x_limit(), anchor, factor);
            self.edit_setup(cx, |s| setup::set_x_range(s, range.map(|r| r.to_array())));
        }
    }

    /// Space held: drags pan instead of zooming.
    pub(crate) fn key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.keystroke.key == "space" && self.focus.is_focused(window) {
            self.view.space_held = true;
            cx.stop_propagation();
        }
    }

    pub(crate) fn key_up(&mut self, event: &KeyUpEvent, _: &mut Window, _: &mut Context<Self>) {
        if event.keystroke.key == "space" {
            self.view.space_held = false;
        }
    }

    /// Moves the view of pane `pane` (and the shared x axis) with the
    /// pointer from `from` to `to`, stopping at the data on every axis.
    fn pan_by(
        &mut self,
        pane: usize,
        from: Point<Pixels>,
        to: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let (Some(geometry), Some(model)) = (self.view.pane_geometry(pane), self.model()) else {
            return;
        };
        let dx = geometry.x.fraction_at(from.x.as_f32()) - geometry.x.fraction_at(to.x.as_f32());
        let dy = geometry.y.fraction_at(from.y.as_f32()) - geometry.y.fraction_at(to.y.as_f32());
        let x = pan_within(model.x_range(), model.x_limit(), dx);
        let y = pan_within(model.y_range(pane), model.y_limit(pane), dy);
        // Both y axes move by the same fraction, so the traces move together
        // until one of them reaches its limit.
        let y2 = model
            .y2_range(pane)
            .zip(model.y2_limit(pane))
            .map(|(range, limit)| pan_within(range, limit, dy));
        self.edit_setup(cx, |s| {
            setup::set_x_range(s, x.map(|x| x.to_array())) | set_y_ranges(s, pane, Some(y), y2)
        });
    }

    fn box_zoom(
        &mut self,
        pane: usize,
        start: Point<Pixels>,
        end: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let (Some(geometry), Some(model)) = (self.view.pane_geometry(pane), self.model()) else {
            return;
        };
        let wide = (start.x - end.x).abs().as_f32() >= DRAG_THRESHOLD;
        let tall = (start.y - end.y).abs().as_f32() >= DRAG_THRESHOLD;
        // A box over all of an axis's data leaves that axis automatic.
        let x = wide
            .then(|| box_range(model.x_range(), &geometry, start.x, end.x, true))
            .flatten()
            .map(|x| bounded(x, model.x_limit()));
        let y = tall
            .then(|| box_range(model.y_range(pane), &geometry, start.y, end.y, false))
            .flatten()
            .map(|y| bounded(y, model.y_limit(pane)));
        // The box covers the same pixels on both y axes.
        let y2 = model
            .y2_range(pane)
            .zip(model.y2_limit(pane))
            .filter(|_| tall)
            .and_then(|(range, limit)| {
                box_range(range, &geometry, start.y, end.y, false).map(|y2| bounded(y2, limit))
            });
        self.edit_setup(cx, |s| {
            let x_changed = x.is_some_and(|x| setup::set_x_range(s, x.map(|x| x.to_array())));
            x_changed | set_y_ranges(s, pane, y, y2)
        });
    }

    /// A click without drag moves the nearer cursor there (cursors on).
    fn click(&mut self, pane: usize, position: Point<Pixels>) {
        let Some(geometry) = self.view.pane_geometry(pane) else {
            return;
        };
        if self.view.cursors.enabled && contains(&geometry, position) {
            self.view.cursors_before_click = Some(self.view.cursors);
            let id = self.view.cursors.nearest(&geometry.x, position.x.as_f32());
            self.move_cursor_x(id, position);
        }
    }
}

/// Puts back the stored ranges of `origin`; returns whether any changed.
fn restore_ranges(setup: &mut PlotSetup, origin: &DragOrigin) -> bool {
    let mut changed = setup::set_x_range(setup, origin.x);
    for (pane, &(y, y2)) in origin.y.iter().enumerate() {
        changed |= setup::set_y_range(setup, pane, y);
        changed |= setup::set_y2_range(setup, pane, y2);
    }
    changed
}

/// Stores the given left (`y`) and right (`y2`) axis ranges of pane `pane`.
fn set_y_ranges(setup: &mut PlotSetup, pane: usize, y: AxisUpdate, y2: AxisUpdate) -> bool {
    let array = |range: Option<AxisRange>| range.map(|r| r.to_array());
    let left = y.is_some_and(|y| setup::set_y_range(setup, pane, array(y)));
    let right = y2.is_some_and(|y2| setup::set_y2_range(setup, pane, array(y2)));
    left | right
}

/// The sub-range a drag box covers along one axis.
fn box_range(
    range: AxisRange,
    geometry: &PaneGeometry,
    a: Pixels,
    b: Pixels,
    horizontal: bool,
) -> Option<AxisRange> {
    let map = if horizontal { geometry.x } else { geometry.y };
    let fraction = |p: Pixels| map.fraction_at(p.as_f32()).clamp(0.0, 1.0);
    select(range, fraction(a), fraction(b))
}

fn contains(geometry: &PaneGeometry, position: Point<Pixels>) -> bool {
    geometry.plot.contains(&position)
}

fn distance(a: Point<Pixels>, b: Point<Pixels>) -> f32 {
    let d = a - b;
    d.x.as_f32().hypot(d.y.as_f32())
}
