//! Headless UI tests of the viewer (Q157): real pointer and keyboard events
//! dispatched to a test window.

use super::canvas::Overlay;
use super::cursor::CursorPoint;
use super::readout::CursorCells;
use super::state::{Drag, PaneGeometry};
use crate::model::cursors::{CursorId, Cursors};
use crate::model::format;
use crate::model::units::YAxis;
use crate::test_fixtures::{TAU, ac, operating_point, transient};
use crate::{ImageFormat, ViewerEvent, WaveformViewer};
use gpui_kit::{
    AppContext, Bounds, Context, CursorStyle, Entity, InteractiveElement, IntoElement, Modifiers,
    MouseButton, MouseDownEvent, MouseUpEvent, ParentElement, Pixels, Point, Render, ScrollDelta,
    ScrollEvent, Styled, Subscription, TestAppContext, VisualTestContext, Window, WindowBounds,
    WindowOptions, div, point, px, size,
};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use xsim_project::{PlotPane, PlotSetup, Trace, YScale};
use xsim_spice_engine::SimulationResult;

struct Harness {
    viewer: Entity<WaveformViewer>,
    cx: VisualTestContext,
    changes: Rc<Cell<usize>>,
    _subscription: Subscription,
}

fn options() -> WindowOptions {
    let bounds = Bounds::new(Point::default(), size(px(1100.), px(WINDOW_HEIGHT)));
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        ..WindowOptions::default()
    }
}

fn open(cx: &mut TestAppContext) -> Harness {
    let (window, viewer) = cx.update(|cx| {
        gpui_kit::init(cx);
        gpui_kit::open_window(options(), cx, |_, cx| cx.new(WaveformViewer::new))
            .expect("open a test window")
    });
    finish(cx, window, viewer)
}

/// The test window's height.
const WINDOW_HEIGHT: f32 = 760.0;

/// A viewer docked in the bottom `height` pixels of the window, as when
/// the shell's window is short or the plot gets a short split: its toolbar
/// sits low and its bottom is the window's.
struct Docked {
    viewer: Entity<WaveformViewer>,
    height: f32,
}

impl Render for Docked {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(div().h(px(WINDOW_HEIGHT - self.height)).flex_shrink_0())
            .child(div().h(px(self.height)).flex().child(self.viewer.clone()))
    }
}

/// A viewer in the bottom `height` pixels of the window, and the dock, so
/// a test can change its height.
fn open_docked(cx: &mut TestAppContext, height: f32) -> (Harness, Entity<Docked>) {
    let (window, viewer, dock) = cx.update(|cx| {
        gpui_kit::init(cx);
        let mut built = None;
        let (window, dock) = gpui_kit::open_window(options(), cx, |_, cx| {
            let inner = cx.new(WaveformViewer::new);
            built = Some(inner.clone());
            cx.new(|_| Docked {
                viewer: inner,
                height,
            })
        })
        .expect("open a test window");
        (window, built.expect("the viewer was built"), dock)
    });
    (finish(cx, window, viewer), dock)
}

/// A viewer squeezed into the bottom 80 pixels of the window: its toolbar
/// sits near the window's edge.
fn open_squeezed(cx: &mut TestAppContext) -> Harness {
    open_docked(cx, 80.0).0
}

fn finish(
    cx: &mut TestAppContext,
    window: gpui_kit::AnyWindowHandle,
    viewer: Entity<WaveformViewer>,
) -> Harness {
    let mut cx = VisualTestContext::from_window(window, cx);
    let changes = Rc::new(Cell::new(0));
    let counter = changes.clone();
    let subscription = cx.update(|_, app| {
        app.subscribe(&viewer, move |_, event: &ViewerEvent, _| {
            if *event == ViewerEvent::SetupChanged {
                counter.set(counter.get() + 1);
            }
        })
    });
    cx.run_until_parked();
    Harness {
        viewer,
        cx,
        changes,
        _subscription: subscription,
    }
}

impl Harness {
    fn show(&mut self, result: SimulationResult, setup: PlotSetup) {
        self.viewer.update(&mut self.cx, |v, cx| {
            v.set_result(Some(Arc::new(result)), setup, cx)
        });
        self.cx.run_until_parked();
    }

    fn add(&mut self, expression: &str) {
        self.viewer
            .update(&mut self.cx, |v, cx| v.add_trace(expression, cx));
        self.cx.run_until_parked();
    }

    fn setup(&mut self) -> PlotSetup {
        self.viewer.read_with(&self.cx, |v, _| v.setup().clone())
    }

    fn cursors(&mut self) -> Cursors {
        self.viewer.read_with(&self.cx, |v, _| v.cursors())
    }

    fn bounds(&mut self, selector: &'static str) -> Bounds<Pixels> {
        self.cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} was not rendered"))
    }

    fn rendered(&mut self, selector: &'static str) -> bool {
        self.cx.debug_bounds(selector).is_some()
    }

    fn click(&mut self, selector: &'static str) {
        let center = self.bounds(selector).center();
        self.click_at(center);
    }

    fn click_at(&mut self, position: Point<Pixels>) {
        self.cx
            .simulate_mouse_move(position, None, Modifiers::default());
        self.cx.simulate_click(position, Modifiers::default());
        self.cx.run_until_parked();
    }

    /// The data area of pane `pane` as last painted.
    fn plot(&mut self, pane: usize) -> Bounds<Pixels> {
        self.plot_geometry(pane).plot
    }

    /// Pane `pane`'s geometry as last painted.
    fn plot_geometry(&mut self, pane: usize) -> PaneGeometry {
        self.viewer
            .read_with(&self.cx, |v, _| v.view.pane_geometry(pane))
            .unwrap_or_else(|| panic!("pane {pane} was not painted"))
    }

    /// What pane `pane` draws over its traces.
    fn overlay(&mut self, pane: usize) -> Overlay {
        self.viewer
            .read_with(&self.cx, |v, _| super::pane::overlay(v, pane))
    }

    /// Where cursor `id` is.
    fn cursor_point(&mut self, id: CursorId) -> CursorPoint {
        self.viewer
            .read_with(&self.cx, |v, _| v.cursor_point(id))
            .expect("the cursor is placed")
    }

    /// The window x of cursor `id`'s line.
    fn line_x(&mut self, id: CursorId) -> Pixels {
        let x = self.cursor_point(id).x;
        px(self.plot_geometry(0).x.px(x))
    }

    /// Where cursor `id`'s trace is drawn at the cursor's x, on the trace's
    /// own y axis (D9): the point D14's horizontal line went through.
    fn on_trace(&mut self, id: CursorId) -> Point<Pixels> {
        let cursor = self.cursor_point(id);
        let on = cursor.on.expect("the cursor follows a trace");
        let geometry = self.plot_geometry(on.pane);
        let axis = self.viewer.read_with(&self.cx, |v, _| {
            let model = v.model().expect("a result");
            let pane = &v.setup.panes[on.pane];
            model.axis_of(&model.units(pane), &pane.traces[on.trace])
        });
        let map = match axis {
            YAxis::Left => geometry.y,
            YAxis::Right => geometry.y2.expect("a right axis"),
        };
        let y = map.px(on.y.expect("a value"));
        point(px(geometry.x.px(cursor.x)), px(y))
    }

    /// The values the readout's cursor block shows.
    fn cells(&mut self) -> CursorCells {
        self.viewer.read_with(&self.cx, |v, _| {
            super::readout::cursor_cells(v, &v.model().expect("a result"))
        })
    }

    /// The mouse pointer pane `pane` shows.
    fn pointer(&mut self, pane: usize) -> CursorStyle {
        self.viewer
            .read_with(&self.cx, |v, _| super::pane::pointer_style(v, pane))
    }

    /// Presses on `chip`, drags it to `to` and releases it there; `during`
    /// runs while the chip hovers over `to`.
    fn drag_chip(&mut self, chip: &'static str, to: Point<Pixels>, during: impl FnOnce(&mut Self)) {
        // Press on the swatch end, away from the chip's buttons.
        let bounds = self.bounds(chip);
        let from = point(bounds.left() + px(10.), bounds.center().y);
        let none = Modifiers::default();
        self.cx.simulate_mouse_move(from, None, none);
        self.cx.simulate_mouse_down(from, MouseButton::Left, none);
        let start = point(from.x + px(8.), from.y + px(8.));
        self.cx.simulate_mouse_move(start, MouseButton::Left, none);
        self.cx.simulate_mouse_move(to, MouseButton::Left, none);
        self.cx.run_until_parked();
        during(self);
        self.cx.simulate_mouse_up(to, MouseButton::Left, none);
        self.cx.run_until_parked();
    }

    /// The point at fractions `fx` (from the left) and `fy` (from the top)
    /// of pane `pane`'s data area.
    fn at(&mut self, pane: usize, fx: f32, fy: f32) -> Point<Pixels> {
        let plot = self.plot(pane);
        point(
            plot.left() + plot.size.width * fx,
            plot.top() + plot.size.height * fy,
        )
    }

    fn drag(&mut self, button: MouseButton, from: Point<Pixels>, to: Point<Pixels>) {
        self.cx
            .simulate_mouse_move(from, None, Modifiers::default());
        self.cx
            .simulate_mouse_down(from, button, Modifiers::default());
        self.cx
            .simulate_mouse_move(to, button, Modifiers::default());
        self.cx.simulate_mouse_up(to, button, Modifiers::default());
        self.cx.run_until_parked();
    }

    fn scroll(&mut self, position: Point<Pixels>, lines: f32, modifiers: Modifiers) {
        self.cx
            .simulate_mouse_move(position, None, Modifiers::default());
        self.cx.simulate_event(ScrollEvent {
            position,
            delta: ScrollDelta::Lines(point(0., lines)),
            modifiers,
            ..ScrollEvent::default()
        });
        self.cx.run_until_parked();
    }

    fn type_trace(&mut self, text: &str) {
        self.click("trace-input");
        self.cx.simulate_input(text);
        self.cx.simulate_keystrokes("enter");
        self.cx.run_until_parked();
    }

    fn input_text(&mut self) -> String {
        self.viewer.read_with(&self.cx, |v, cx| {
            v.view
                .input
                .as_ref()
                .map(|i| i.state.read(cx).value().to_string())
                .unwrap_or_default()
        })
    }
}

fn expressions(setup: &PlotSetup, pane: usize) -> Vec<String> {
    setup.panes[pane]
        .traces
        .iter()
        .map(|t| t.expression.clone())
        .collect()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1e-6)
}

#[gpui_kit::test]
fn empty_viewer_explains_what_to_do(cx: &mut TestAppContext) {
    let mut h = open(cx);
    assert!(h.rendered("waveform-empty"));
    assert!(!h.rendered("pane-canvas-0"));
}

#[gpui_kit::test]
fn typing_an_expression_adds_a_trace(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    assert!(h.rendered("pane-canvas-0"));
    h.type_trace("V(out)-V(in)");
    assert_eq!(expressions(&h.setup(), 0), ["V(out)-V(in)"]);
    assert_eq!(h.input_text(), "", "the field clears after adding");
    assert!(h.changes.get() >= 1);
    assert!(h.rendered("trace-chip-0-0"));
}

#[gpui_kit::test]
fn invalid_expressions_show_an_inline_error(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.type_trace("V(nope)");
    assert!(h.setup().panes[0].traces.is_empty());
    assert!(h.rendered("trace-input-error"));
    let error = h.viewer.read_with(&h.cx, |v, _| {
        v.view.input.as_ref().and_then(|i| i.error.clone())
    });
    assert_eq!(
        error.as_deref(),
        Some("No signal named V(nope) in the result")
    );
    h.cx.simulate_input("x");
    h.cx.run_until_parked();
    assert!(!h.rendered("trace-input-error"), "editing clears the error");
}

#[gpui_kit::test]
fn autocomplete_completes_signal_names(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.click("trace-input");
    h.cx.simulate_input("V(o");
    h.cx.run_until_parked();
    assert!(h.rendered("trace-suggestions"));
    h.cx.simulate_keystrokes("enter");
    h.cx.run_until_parked();
    assert_eq!(h.input_text(), "V(out)");
    h.cx.simulate_keystrokes("enter");
    h.cx.run_until_parked();
    assert_eq!(expressions(&h.setup(), 0), ["V(out)"]);
}

#[gpui_kit::test]
fn wheel_zoom_stores_the_x_range_and_emits_setup_changed(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    let before = h.changes.get();
    let center = h.at(0, 0.5, 0.5);
    h.scroll(center, 2.0, Modifiers::default());
    let setup = h.setup();
    let [min, max] = setup.panes[0].x_range.expect("zoom stores an x range");
    let factor = 0.85f64.powi(2);
    assert!(close(min, 2.5e-3 - 2.5e-3 * factor), "{min}");
    assert!(close(max, 2.5e-3 + 2.5e-3 * factor), "{max}");
    assert_eq!(setup.panes[0].y_range, None);
    assert!(h.changes.get() > before);
}

#[gpui_kit::test]
fn shift_wheel_zooms_the_y_axis_of_that_pane(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    let fitted = h.plot_geometry(0).y.range();
    let center = h.at(0, 0.5, 0.5);
    h.scroll(center, 2.0, Modifiers::shift());
    let pane = h.setup().panes[0].clone();
    assert_eq!(pane.x_range, None);
    let [min, max] = pane.y_range.expect("shift+wheel stores a y range");
    assert!(
        max - min < (fitted.max - fitted.min) * 0.8,
        "scrolling up zooms in: {min}..{max}"
    );
    // Scrolling down zooms out again, but never past the fitted range.
    h.scroll(center, -1.0, Modifiers::shift());
    let [min2, max2] = h.setup().panes[0].y_range.expect("still zoomed in");
    assert!(max2 - min2 > max - min);
    h.scroll(center, -10.0, Modifiers::shift());
    assert_eq!(h.setup().panes[0].y_range, None, "back to the fitted range");
}

/// D14: the wheel never zooms out past the data, on either axis.
#[gpui_kit::test]
fn wheel_zoom_out_stops_at_the_data(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    let full = h.plot_geometry(0);
    let center = h.at(0, 0.5, 0.5);

    // From the full view, zooming out changes nothing and reports nothing.
    let before = h.changes.get();
    h.scroll(center, -3.0, Modifiers::default());
    h.scroll(center, -3.0, Modifiers::shift());
    let pane = h.setup().panes[0].clone();
    assert_eq!((pane.x_range, pane.y_range), (None, None));
    assert_eq!(h.changes.get(), before, "no setup change");
    let shown = h.plot_geometry(0);
    assert_eq!(
        shown.x.range(),
        full.x.range(),
        "traces keep the plot's width"
    );
    assert_eq!(shown.y.range(), full.y.range(), "and its height");

    // Zoom in at the right end, then out about the left edge: the view
    // grows back inside the data and ends at the full extent.
    let right = h.at(0, 0.95, 0.5);
    h.scroll(right, 4.0, Modifiers::default());
    let left = h.at(0, 0.0, 0.5);
    h.scroll(left, -1.0, Modifiers::default());
    let [min, max] = h.setup().panes[0].x_range.expect("still zoomed in");
    assert!(
        min >= 0.0 && max <= 5e-3 + 1e-12,
        "{min}..{max} inside the data"
    );
    assert!(close(max, 5e-3), "pushed back against the end: {max}");
    h.scroll(left, -20.0, Modifiers::default());
    assert_eq!(h.setup().panes[0].x_range, None, "fully out is automatic");
}

/// D14 with D9: zooming out over an axis's labels stops at that axis's
/// fitted range.
#[gpui_kit::test]
fn axis_label_zoom_out_stops_at_the_fitted_range(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    h.add("I(R1)");
    let plot = h.plot(0);
    let over_right_axis = point(plot.right() + px(12.), plot.center().y);
    h.scroll(over_right_axis, -5.0, Modifiers::shift());
    assert_eq!(h.setup().panes[0].y2_range, None);
    h.scroll(over_right_axis, 3.0, Modifiers::shift());
    assert!(h.setup().panes[0].y2_range.is_some());
    h.scroll(over_right_axis, -30.0, Modifiers::shift());
    let pane = h.setup().panes[0].clone();
    assert_eq!((pane.y_range, pane.y2_range), (None, None));
}

/// D14: with the magnifier off, a left-drag pans, and panning stops at the
/// data; an axis showing all of it does not move.
#[gpui_kit::test]
fn left_drag_pans_within_the_data(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    assert!(!h.viewer.read_with(&h.cx, |v, _| v.view.magnifier));

    // Fully zoomed out: a drag moves nothing.
    let before = h.changes.get();
    let (from, to) = (h.at(0, 0.5, 0.5), h.at(0, 0.8, 0.2));
    h.drag(MouseButton::Left, from, to);
    let pane = h.setup().panes[0].clone();
    assert_eq!((pane.x_range, pane.y_range), (None, None));
    assert_eq!(h.changes.get(), before);

    // Zoomed in on x: a drag to the right shows earlier times; y, still
    // fitted, stays put however far the pointer moves vertically.
    let center = h.at(0, 0.5, 0.5);
    h.scroll(center, 4.0, Modifiers::default());
    let [min, max] = h.setup().panes[0].x_range.expect("zoomed in");
    let (from, to) = (h.at(0, 0.5, 0.5), h.at(0, 0.6, 0.1));
    h.drag(MouseButton::Left, from, to);
    let pane = h.setup().panes[0].clone();
    let [min2, max2] = pane.x_range.expect("panned");
    let shift = (max - min) * 0.1;
    // Pointer positions are pixels, so allow for their rounding.
    let near = |a: f64, b: f64| (a - b).abs() <= (max - min) * 1e-4;
    assert!(
        near(min2, min - shift) && near(max2, max - shift),
        "{min2}..{max2}"
    );
    assert_eq!(pane.y_range, None, "the fitted y axis does not drift");

    // Dragging far stops at the start of the data.
    let (from, to) = (h.at(0, 0.02, 0.5), h.at(0, 0.98, 0.5));
    h.drag(MouseButton::Left, from, to);
    let [min3, max3] = h.setup().panes[0].x_range.expect("still zoomed");
    assert_eq!(min3, 0.0);
    assert!(near(max3 - min3, max - min), "the span is kept");
}

/// D14: box zoom needs the magnifier, which stays on until Escape.
#[gpui_kit::test]
fn the_magnifier_box_zooms_until_turned_off(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    h.click("toggle-magnifier");
    assert!(h.viewer.read_with(&h.cx, |v, _| v.view.magnifier));
    let (from, to) = (h.at(0, 0.2, 0.2), h.at(0, 0.6, 0.7));
    h.drag(MouseButton::Left, from, to);
    let [min, max] = h.setup().panes[0].x_range.expect("box zoom sets x");
    assert!(
        (min - 1e-3).abs() < 2e-5 && (max - 3e-3).abs() < 2e-5,
        "{min}..{max}"
    );
    assert!(
        h.viewer.read_with(&h.cx, |v, _| v.view.magnifier),
        "the magnifier stays on for the next zoom"
    );
    let (from, to) = (h.at(0, 0.0, 0.0), h.at(0, 0.5, 1.0));
    h.drag(MouseButton::Left, from, to);
    let [min2, max2] = h.setup().panes[0].x_range.expect("zoomed again");
    assert!(
        close(min2, min) && close(max2, (min + max) / 2.0),
        "{min2}..{max2}"
    );

    // Escape turns it off; a drag then pans instead of zooming.
    let center = h.at(0, 0.5, 0.5);
    h.click_at(center);
    h.cx.simulate_keystrokes("escape");
    h.cx.run_until_parked();
    assert!(!h.viewer.read_with(&h.cx, |v, _| v.view.magnifier));
    let (from, to) = (h.at(0, 0.5, 0.5), h.at(0, 0.6, 0.5));
    h.drag(MouseButton::Left, from, to);
    let [min3, max3] = h.setup().panes[0].x_range.expect("panned");
    assert!(
        (max3 - min3 - (max2 - min2)).abs() <= (max2 - min2) * 1e-4,
        "a pan keeps the span"
    );
    assert!(min3 < min2);

    // The tool button toggles it too.
    h.click("toggle-magnifier");
    h.click("toggle-magnifier");
    assert!(!h.viewer.read_with(&h.cx, |v, _| v.view.magnifier));
}

#[gpui_kit::test]
fn drag_box_zooms_and_double_click_fits(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    h.click("toggle-magnifier");
    let (from, to) = (h.at(0, 0.2, 0.2), h.at(0, 0.6, 0.7));
    h.drag(MouseButton::Left, from, to);
    let pane = h.setup().panes[0].clone();
    let [min, max] = pane.x_range.expect("box zoom sets x");
    assert!(
        (min - 1e-3).abs() < 2e-5 && (max - 3e-3).abs() < 2e-5,
        "{min}..{max}"
    );
    assert!(pane.y_range.is_some(), "box zoom sets y");

    // A double click arrives as a second press with `click_count: 2`.
    let position = h.at(0, 0.5, 0.5);
    for click_count in [1, 2] {
        h.cx.simulate_event(MouseDownEvent {
            position,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
            click_count,
            first_mouse: false,
        });
        h.cx.simulate_event(MouseUpEvent {
            position,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
            click_count,
        });
    }
    h.cx.run_until_parked();
    let pane = &h.setup().panes[0];
    assert_eq!((pane.x_range, pane.y_range), (None, None));
}

#[gpui_kit::test]
fn escape_cancels_a_box_zoom(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    h.click("toggle-magnifier");
    let (from, to) = (h.at(0, 0.2, 0.2), h.at(0, 0.6, 0.7));
    h.cx.simulate_mouse_move(from, None, Modifiers::default());
    h.cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::default());
    h.cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::default());
    h.cx.simulate_keystrokes("escape");
    h.cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::default());
    h.cx.run_until_parked();
    assert_eq!(h.setup().panes[0].x_range, None);
    assert!(
        h.viewer.read_with(&h.cx, |v, _| v.view.magnifier),
        "the first Escape only cancels the box"
    );
}

#[gpui_kit::test]
fn middle_drag_pans_both_axes(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    // Zoom in on both axes first: a fully zoomed-out axis cannot pan.
    let center = h.at(0, 0.5, 0.5);
    h.scroll(center, 4.0, Modifiers::default());
    h.scroll(center, 4.0, Modifiers::shift());
    let zoomed = h.setup().panes[0].clone();
    let ([x0, x1], [y0, y1]) = (zoomed.x_range.unwrap(), zoomed.y_range.unwrap());
    let (from, to) = (h.at(0, 0.5, 0.5), h.at(0, 0.6, 0.6));
    h.drag(MouseButton::Middle, from, to);
    let pane = h.setup().panes[0].clone();
    let [min, max] = pane.x_range.expect("pan stores an x range");
    // Pointer positions are pixels, so allow for their rounding.
    let near = |a: f64, b: f64, span: f64| (a - b).abs() <= span * 1e-4;
    let shift = (x1 - x0) * 0.1;
    assert!(
        near(min, x0 - shift, x1 - x0) && near(max, x1 - shift, x1 - x0),
        "{min}..{max}"
    );
    let [low, high] = pane.y_range.expect("and a y range");
    let rise = (y1 - y0) * 0.1;
    assert!(
        near(low, y0 + rise, y1 - y0) && near(high, y1 + rise, y1 - y0),
        "{low}..{high}"
    );
}

#[gpui_kit::test]
fn cursors_snap_and_report_deltas(cx: &mut TestAppContext) {
    let mut h = open(cx);
    let result = transient();
    let axis = result.axis.values.clone();
    h.show(result, PlotSetup::default());
    h.add("V(out)");
    h.click("toggle-cursors");
    let cursors = h.cursors();
    assert!(cursors.enabled);
    assert_eq!(
        (cursors.first, cursors.second),
        (Some(axis[25]), Some(axis[75]))
    );
    assert!(h.rendered("cursor-readout"));

    // A click moves the nearer cursor to the sample under the pointer.
    let target = h.at(0, 0.1, 0.5);
    h.click_at(target);
    assert_eq!(h.cursors().first, Some(axis[10]));

    // Dragging a cursor line moves that cursor.
    let (grab, drop) = (h.at(0, 0.75, 0.5), h.at(0, 0.6, 0.5));
    h.drag(MouseButton::Left, grab, drop);
    let cursors = h.cursors();
    assert_eq!(cursors.second, Some(axis[60]));
    let delta = crate::model::cursors::x_delta(&cursors).unwrap();
    assert!(close(delta.dx, axis[60] - axis[10]));
    assert!(close(delta.inverse.unwrap(), 1.0 / (axis[60] - axis[10])));
    assert_eq!(h.setup().panes[0].x_range, None, "cursor drags do not zoom");

    h.click("toggle-cursors");
    assert!(!h.cursors().enabled);
    assert!(!h.rendered("cursor-readout"));
}

#[gpui_kit::test]
fn measurements_panel_measures_the_selected_trace(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(in)");
    h.add("V(out)");
    h.click("trace-chip-0-1");
    h.click("toggle-measurements");
    assert!(h.rendered("measurements-panel"));
    let rows = h.viewer.read_with(&h.cx, |v, _| {
        assert_eq!(v.view.selected, Some((0, 1)));
        v.view
            .measured
            .borrow()
            .as_ref()
            .map(|(_, rows)| (**rows).clone())
    });
    let rows = rows.expect("the panel measured the selection");
    let maximum = rows.iter().find(|r| r.name == "Maximum").unwrap();
    assert!((maximum.value.unwrap() - (1.0 - (-5.0f64).exp())).abs() < 1e-9);
    assert_eq!(maximum.unit, "V");
}

#[gpui_kit::test]
fn ac_results_get_bode_panes_and_probes_go_to_both(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(ac(), PlotSetup::default());
    let setup = h.setup();
    let scales: Vec<YScale> = setup.panes.iter().map(|p| p.y_scale).collect();
    assert_eq!(scales, [YScale::Decibel, YScale::Phase]);
    assert!(h.changes.get() >= 1, "adapting the setup is reported");
    h.add("V(out)");
    let setup = h.setup();
    assert_eq!(expressions(&setup, 0), ["V(out)"]);
    assert_eq!(expressions(&setup, 1), ["V(out)"]);
    assert!(h.rendered("pane-canvas-1"));
    let log = h.viewer.read_with(&h.cx, |v, _| {
        v.view.pane_geometry(0).map(|g| g.x.range().log)
    });
    assert_eq!(log, Some(true), "AC frequency axes are logarithmic");
}

#[gpui_kit::test]
fn operating_points_show_a_table(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(operating_point(), PlotSetup::default());
    assert!(h.rendered("op-table"));
    assert!(!h.rendered("pane-canvas-0"));
    let csv = h.viewer.read_with(&h.cx, |v, _| v.csv()).unwrap();
    assert!(csv.lines().next().unwrap().contains("V(out)"));
    let image = h.viewer.read_with(&h.cx, |v, _| v.svg(400, 300));
    assert!(image.is_err(), "an operating point has no plot");
}

#[gpui_kit::test]
fn missing_signals_stay_listed_as_unavailable(cx: &mut TestAppContext) {
    let mut h = open(cx);
    let mut setup = PlotSetup::default();
    setup.panes[0].traces = vec![Trace::new("V(gone)"), Trace::new("V(out)")];
    h.show(transient(), setup);
    assert_eq!(expressions(&h.setup(), 0), ["V(gone)", "V(out)"]);
    let error = h.viewer.read_with(&h.cx, |v, _| {
        v.trace_error("V(gone)").map(ToString::to_string)
    });
    assert_eq!(
        error.as_deref(),
        Some("No signal named V(gone) in the result")
    );
    assert!(h.rendered("trace-chip-0-0"));
    let selected = h.viewer.read_with(&h.cx, |v, _| v.view.selected);
    assert_eq!(selected, Some((0, 1)), "the available trace is selected");
}

#[gpui_kit::test]
fn legend_chips_hide_and_remove_traces(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(in)");
    h.add("V(out)");
    h.click("trace-visibility-0-0");
    assert!(!h.setup().panes[0].traces[0].visible);
    h.click("trace-visibility-0-0");
    assert!(h.setup().panes[0].traces[0].visible);
    h.click("trace-remove-0-0");
    assert_eq!(expressions(&h.setup(), 0), ["V(out)"]);
}

#[gpui_kit::test]
fn delete_removes_the_selected_trace(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(in)");
    h.add("V(out)");
    let inside = h.at(0, 0.5, 0.5);
    h.click_at(inside);
    h.click("trace-chip-0-1");
    h.cx.simulate_keystrokes("delete");
    h.cx.run_until_parked();
    assert_eq!(expressions(&h.setup(), 0), ["V(in)"]);
}

#[gpui_kit::test]
fn the_plus_adds_and_the_close_button_removes_panes(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    h.click("add-pane");
    let setup = h.setup();
    assert_eq!((setup.panes.len(), setup.active_pane), (2, 1));
    h.add("V(in)");
    assert_eq!(expressions(&h.setup(), 1), ["V(in)"]);
    let (top, bottom) = (h.plot(0), h.plot(1));
    assert_eq!(top.left(), bottom.left(), "panes share the x axis");
    assert_eq!(top.right(), bottom.right());
    h.click("pane-close-1");
    assert_eq!(h.setup().panes.len(), 1);
}

/// Sets the UI scale: the rem size in pixels (16 = 100 %).
fn set_rem(h: &mut Harness, rem: f32) {
    h.cx.update(|_, app| {
        gpui_kit::component::Theme::update(app, |theme| theme.font_size = px(rem));
    });
    h.viewer.update(&mut h.cx, |_, cx| cx.notify());
    h.cx.run_until_parked();
}

/// Whether `inner` lies within `outer`, up to the paint snap.
fn inside(outer: &Bounds<Pixels>, inner: &Bounds<Pixels>) -> bool {
    let snap = px(SNAP);
    inner.left() >= outer.left() - snap
        && inner.right() <= outer.right() + snap
        && inner.top() >= outer.top() - snap
        && inner.bottom() <= outer.bottom() + snap
}

/// The window's bottom edge: the viewer fills the test window.
fn window_bottom(h: &mut Harness) -> Pixels {
    h.cx.update(|window, _| window.viewport_size().height)
}

/// D22 at 75 %, 100 % and 200 % UI scale, with the cursor readout hidden
/// and shown: the "Add pane" plus is in a strip at the very bottom of the
/// dock, below the last pane (so under its x labels, never over them) and
/// below the readout, and not in the toolbar.
#[gpui_kit::test]
fn the_add_pane_plus_sits_at_the_very_bottom_of_the_dock(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    h.click("add-pane");
    h.add("V(in)");
    let last = 1;
    for rem in [12.0, 16.0, 32.0] {
        set_rem(&mut h, rem);
        for cursors in [false, true] {
            // At 200 % with the readout open two panes do not fit: show
            // the last one, as adding it does.
            h.viewer.update(&mut h.cx, |v, cx| {
                v.set_cursors_visible(cursors, cx);
                v.view.panes_scroll.scroll_to_bottom();
            });
            h.cx.run_until_parked();
            let case = format!("{rem} px rem, cursors {cursors}");
            let plus = h.bounds("add-pane");
            let strip = h.bounds("waveform-footer");
            assert!(
                plus.size.width > px(0.) && plus.size.height > px(0.),
                "{case}"
            );
            assert!(inside(&strip, &plus), "{case}: {plus:?} in {strip:?}");
            assert!(near(strip.bottom(), window_bottom(&mut h)), "{case}");
            // Below the last pane and every one of its x labels.
            let (geometry, scene) = screen_scene(&mut h, last);
            assert!(plus.top() >= geometry.frame.bottom(), "{case}");
            let x_labels: Vec<_> = scene
                .labels
                .iter()
                .filter(|l| l.anchor == crate::scene::Anchor::Middle)
                .collect();
            assert!(!x_labels.is_empty(), "{case}");
            for label in x_labels {
                let bottom = geometry.frame.top() + px(label.y + label.size * 0.6);
                assert!(bottom <= plus.top(), "{case}: {label:?}");
            }
            assert!(
                inside(&h.bounds("waveform-panes"), &geometry.frame),
                "{case}"
            );
            if cursors {
                assert!(plus.top() >= h.bounds("cursor-readout").bottom(), "{case}");
            }
            // The toolbar has no add-pane control any more: only the
            // divider separates the trace field from the cursors button,
            // where the old control was.
            let toolbar = h.bounds("waveform-toolbar");
            assert!(!toolbar.intersects(&plus), "{case}");
            assert!(plus.top() >= toolbar.bottom(), "{case}");
            let gap = h.bounds("toggle-cursors").left() - h.bounds("trace-input").right();
            let rem_size = h.cx.update(|window, _| window.rem_size());
            assert!(gap < rem_size * 1.5, "{case}: {gap:?}");
            // The plus keeps the toolbar's left inset.
            assert!(near(plus.left(), h.bounds("trace-input").left()), "{case}");
        }
    }
}

/// D22: with more panes than fit, the plus stays in view at every UI scale
/// and keeps adding panes, each scrolled into view at the bottom of the
/// stack, which scrolls instead of pushing the plus off the window.
#[gpui_kit::test]
fn the_plus_stays_in_view_with_more_panes_than_fit(cx: &mut TestAppContext) {
    let mut h = open(cx);
    for rem in [12.0, 16.0, 32.0] {
        h.show(transient(), PlotSetup::default());
        h.add("V(out)");
        set_rem(&mut h, rem);
        for added in 1..=8 {
            let case = format!("{rem} px rem, pane {added}");
            let plus = h.bounds("add-pane");
            let strip = h.bounds("waveform-footer");
            assert!(inside(&strip, &plus), "{case}");
            assert!(near(strip.bottom(), window_bottom(&mut h)), "{case}");
            // Clicked again and again without moving: the plus never moves
            // under the pointer, so no click is lost.
            h.click("add-pane");
            assert_eq!(h.setup().panes.len(), added + 1, "{case}");
            assert_eq!(h.bounds("add-pane").origin.x, plus.origin.x, "{case}");
            // The new pane is in view, right above the strip.
            let stack = h.bounds("waveform-panes");
            let new = h.plot_geometry(added).frame;
            assert!(inside(&stack, &new), "{case}: {new:?} in {stack:?}");
            let strip = h.bounds("waveform-footer");
            assert!(stack.bottom() <= strip.top() + px(SNAP), "{case}");
        }
        // Nine panes do not fit: the first one has scrolled out of view,
        // and no pane got shorter than its minimum.
        let stack = h.bounds("waveform-panes");
        assert!(h.bounds("pane-0").top() < stack.top(), "{rem}: scrolled");
        let min_height = h.cx.update(|window, _| window.rem_size()) * 7.5;
        let pane = h.bounds("pane-0");
        assert!(pane.size.height >= min_height - px(SNAP), "{rem}: {pane:?}");
    }
}

/// The wheel over a plot zooms without scrolling the stack; over a pane's
/// header it scrolls the stack and zooms nothing. The plus stays put.
#[gpui_kit::test]
fn the_wheel_scrolls_the_stack_only_over_a_pane_header(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    for _ in 0..8 {
        h.click("add-pane");
    }
    let last = 8;
    let plus = h.bounds("add-pane");
    let before = h.plot_geometry(last).frame;
    let center = h.plot(last).center();
    h.scroll(center, 3.0, Modifiers::default());
    assert!(h.setup().panes[0].x_range.is_some(), "the wheel zoomed");
    assert_eq!(
        h.plot_geometry(last).frame,
        before,
        "the stack did not move"
    );

    let zoomed = h.setup();
    let header = h.bounds("pane-header-8");
    let over_header = point(header.left() + px(4.), header.center().y);
    h.scroll(over_header, 3.0, Modifiers::default());
    let after = h.plot_geometry(last).frame;
    assert!(after.top() > before.top(), "the stack scrolled: {after:?}");
    assert_eq!(h.setup(), zoomed, "nothing zoomed");
    assert_eq!(h.bounds("add-pane"), plus, "the plus stays put");
}

fn press_key(h: &mut Harness, key: &'static str) {
    let keystroke = gpui_kit::Keystroke::parse(key).expect("a keystroke");
    h.cx.simulate_event(gpui_kit::KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    h.cx.simulate_event(gpui_kit::KeyUpEvent { keystroke });
    h.cx.run_until_parked();
}

/// D22: the plus is a tab stop, and Enter or Space on it adds a pane; it
/// keeps the focus, so repeated presses go on adding panes.
#[gpui_kit::test]
fn enter_or_space_on_the_focused_plus_adds_a_pane(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    let focus = h.viewer.read_with(&h.cx, |v, _| v.add_pane_focus.clone());
    h.cx.update(|window, app| focus.focus(window, app));
    h.cx.run_until_parked();
    for (key, panes) in [("enter", 2), ("space", 3), ("enter", 4)] {
        press_key(&mut h, key);
        assert_eq!(h.setup().panes.len(), panes, "{key}");
        assert!(h.cx.update(|window, _| focus.is_focused(window)), "{key}");
    }
    // Tab order reaches it: away and back again.
    h.cx.update(|window, app| window.focus_prev(app));
    assert!(!h.cx.update(|window, _| focus.is_focused(window)));
    h.cx.update(|window, app| window.focus_next(app));
    assert!(h.cx.update(|window, _| focus.is_focused(window)));
}

/// No plus in the empty state or for an operating point, which have no
/// panes to add to.
#[gpui_kit::test]
fn there_is_no_plus_without_panes(cx: &mut TestAppContext) {
    let mut h = open(cx);
    assert!(!h.rendered("add-pane"));
    h.show(operating_point(), PlotSetup::default());
    assert!(!h.rendered("add-pane"));
    h.show(transient(), PlotSetup::default());
    assert!(h.rendered("add-pane"));
}

/// The bounds of the element tagged `selector`, for selectors built at run
/// time (`pane-3`).
fn bounds_of(h: &mut Harness, selector: String) -> Bounds<Pixels> {
    h.bounds(Box::leak(selector.into_boxed_str()))
}

/// D22 in a short window: with the cursor readout open and full of trace
/// rows, the readout gives up height (and scrolls) before the plus does, so
/// the plus stays at the dock's bottom edge and keeps adding panes, at 75 %
/// to 200 % UI scale and down to a dock barely taller than the toolbar and
/// the strip.
#[gpui_kit::test]
fn the_plus_stays_in_a_short_dock_with_the_readout_open(cx: &mut TestAppContext) {
    let (mut h, dock) = open_docked(cx, 560.0);
    h.show(transient(), PlotSetup::default());
    for expression in [
        "V(in)",
        "V(out)",
        "I(R1)",
        "V(out)-V(in)",
        "2*V(out)",
        "V(in)/2",
    ] {
        h.add(expression);
    }
    h.viewer
        .update(&mut h.cx, |v, cx| v.set_cursors_visible(true, cx));
    h.cx.run_until_parked();
    for rem in [12.0, 16.0, 24.0, 32.0] {
        set_rem(&mut h, rem);
        // The toolbar (2.5 rem) and the strip (2 rem) never shrink.
        let rem_size = h.cx.update(|window, _| window.rem_size());
        let fixed = rem_size.as_f32() * 4.5;
        let mut row_heights = None;
        for height in [560.0, 400.0, 300.0, fixed + 16.0] {
            dock.update(&mut h.cx, |dock, cx| {
                dock.height = height;
                cx.notify();
            });
            h.cx.run_until_parked();
            let case = format!("{rem} px rem, {height} px dock");
            let strip = h.bounds("waveform-footer");
            let plus = h.bounds("add-pane");
            assert!(near(strip.bottom(), px(WINDOW_HEIGHT)), "{case}: {strip:?}");
            assert!(inside(&strip, &plus), "{case}: {plus:?} in {strip:?}");
            let toolbar = h.bounds("waveform-toolbar");
            assert!(near(toolbar.top(), px(WINDOW_HEIGHT - height)), "{case}");
            let readout = h.bounds("cursor-readout");
            assert!(readout.top() >= toolbar.bottom() - px(SNAP), "{case}");
            assert!(readout.bottom() <= strip.top() + px(SNAP), "{case}");
            if height == 560.0 && rem <= 24.0 {
                // With room, the readout is at its 13 rem cap: these rows
                // fill it.
                assert!(near(readout.size.height, rem_size * 13.0), "{case}");
            }
            // A shorter readout scrolls its rows; it never squashes them.
            let rows = ["cursor-row-1", "cursor-delta-row"].map(|row| h.bounds(row).size.height);
            let full = *row_heights.get_or_insert(rows);
            for (row, full) in rows.into_iter().zip(full) {
                assert!(near(row, full), "{case}: {row:?} of {full:?}");
            }
            let panes = h.setup().panes.len();
            h.click("add-pane");
            assert_eq!(h.setup().panes.len(), panes + 1, "{case}");
        }
    }
}

/// D22: nothing in the toolbar adds a pane any more. Its controls sit side
/// by side with nothing between them (only the divider after the trace
/// field, where the old control was), and clicks all along its empty
/// middle add no pane.
#[gpui_kit::test]
fn the_toolbar_has_no_add_pane_control(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    let rem = h.cx.update(|window, _| window.rem_size());
    let (inset, gap) = (rem * 0.5, rem * 0.25);
    let toolbar = h.bounds("waveform-toolbar");
    let field = h.bounds("trace-input");
    assert!(near(field.left(), toolbar.left() + inset));
    // The divider: a 1 px line with a 0.25 rem margin each side.
    let cursors = h.bounds("toggle-cursors");
    assert!(
        near(cursors.left() - field.right(), gap * 4.0 + px(1.)),
        "{:?}",
        cursors.left() - field.right()
    );
    for pair in [
        ["toggle-cursors", "toggle-measurements"],
        ["toggle-measurements", "toggle-magnifier"],
        ["toggle-magnifier", "zoom-to-fit"],
        ["copy-plot", "export-menu"],
    ] {
        let (left, right) = (h.bounds(pair[0]), h.bounds(pair[1]));
        assert!(near(right.left() - left.right(), gap), "{pair:?}");
    }
    assert!(near(
        h.bounds("export-menu").right(),
        toolbar.right() - inset
    ));
    // The empty middle. The copy and export buttons are left alone: one
    // writes to the system clipboard, the other opens a menu.
    let (from, to) = (
        h.bounds("zoom-to-fit").right(),
        h.bounds("copy-plot").left(),
    );
    let mut x = from + px(2.);
    while x < to - px(2.) {
        h.click_at(point(x, toolbar.center().y));
        x += px(6.);
    }
    for selector in [
        "toggle-cursors",
        "toggle-measurements",
        "toggle-magnifier",
        "zoom-to-fit",
    ] {
        h.click(selector);
    }
    assert_eq!(h.setup().panes.len(), 1);
}

/// A chip dragged over the toolbar or the plus, where a pane scrolled out
/// of view runs on hidden under them, targets no pane: only the visible
/// part of a pane takes a dropped chip, and only it lights up (D14, D22).
#[gpui_kit::test]
fn a_chip_over_the_toolbar_or_the_plus_targets_no_hidden_pane(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    for _ in 0..8 {
        h.click("add-pane");
    }
    h.add("V(in)");
    let before = h.setup();
    let target = |h: &mut Harness| h.viewer.read_with(&h.cx, |v, _| v.view.drop_target);
    // Scrolled to the bottom: an earlier pane runs on under the toolbar.
    let toolbar = h.bounds("waveform-toolbar");
    let over_toolbar = point(
        h.bounds("zoom-to-fit").right() + px(60.),
        toolbar.center().y,
    );
    let under = (0..9).find(|&i| bounds_of(&mut h, format!("pane-{i}")).contains(&over_toolbar));
    assert!(under.is_some(), "a pane lies under the toolbar");
    h.drag_chip("trace-chip-8-0", over_toolbar, |h| {
        assert_eq!(target(h), None, "no hidden pane is the target");
    });
    assert_eq!(h.setup(), before, "nothing moved");
    assert_eq!(target(&mut h), None);

    // Scrolled to the top: a later pane runs on under the plus.
    h.viewer.update(&mut h.cx, |v, cx| {
        v.view.panes_scroll.set_offset(point(px(0.), px(0.)));
        cx.notify();
    });
    h.cx.run_until_parked();
    let over_plus = h.bounds("add-pane").center();
    let under = (0..9).find(|&i| bounds_of(&mut h, format!("pane-{i}")).contains(&over_plus));
    assert!(under.is_some(), "a pane lies under the plus");
    h.drag_chip("trace-chip-0-0", over_plus, |h| {
        assert_eq!(target(h), None, "no hidden pane is the target");
    });
    assert_eq!(h.setup(), before, "nothing moved");
    assert_eq!(target(&mut h), None);
    // A visible pane still takes the chip.
    let onto = h.at(1, 0.5, 0.5);
    h.drag_chip("trace-chip-0-0", onto, |h| {
        assert_eq!(target(h).map(|t| t.pane), Some(1));
    });
    assert_eq!(expressions(&h.setup(), 1), ["V(out)"]);
}

/// Closing panes while the stack is scrolled to its end: the panes left
/// fill the stack again, with no blank gap under the last one.
#[gpui_kit::test]
fn closing_panes_while_scrolled_leaves_no_gap(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    for _ in 0..8 {
        h.click("add-pane");
    }
    while h.setup().panes.len() > 1 {
        let last = h.setup().panes.len() - 1;
        let close = bounds_of(&mut h, format!("pane-close-{last}"));
        h.click_at(close.center());
        assert_eq!(h.setup().panes.len(), last, "pane {last} closed");
        let stack = h.bounds("waveform-panes");
        let bottom = bounds_of(&mut h, format!("pane-{}", last - 1));
        assert!(
            near(bottom.bottom(), stack.bottom()),
            "{last} left: {bottom:?} ends the stack {stack:?}"
        );
        assert!(h.bounds("pane-0").top() <= stack.top() + px(SNAP), "{last}");
    }
}

#[gpui_kit::test]
fn exports_render_the_current_plot(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    let png = h
        .viewer
        .read_with(&h.cx, |v, _| v.image_bytes(ImageFormat::Png, 800, 450))
        .unwrap();
    let decoded = image::load_from_memory(&png).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (800, 450));
    let jpeg = h
        .viewer
        .read_with(&h.cx, |v, _| v.image_bytes(ImageFormat::Jpeg, 640, 480))
        .unwrap();
    let decoded = image::load_from_memory(&jpeg).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (640, 480));
    let (image, size) = h
        .viewer
        .read_with(&h.cx, |v, _| (v.clipboard_image(), v.default_image_size()));
    let image = image.unwrap();
    assert_eq!((image.width as u32, image.height as u32), size);
    assert_eq!(image.bytes.len(), image.width * image.height * 4);

    let path = std::env::temp_dir().join(format!("xsim-plot-test-{}.csv", std::process::id()));
    h.viewer
        .read_with(&h.cx, |v, _| v.export_csv(&path))
        .unwrap();
    let csv = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(csv.lines().next(), Some("time [s],V(out)"));
    assert_eq!(csv.lines().count(), 102);
}

#[gpui_kit::test]
fn probing_an_unknown_signal_lists_it_as_unavailable(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(n007)");
    h.add("V(out");
    assert_eq!(
        expressions(&h.setup(), 0),
        ["V(n007)"],
        "syntax errors are ignored"
    );
    let error = h.viewer.read_with(&h.cx, |v, _| {
        v.trace_error("V(n007)").map(ToString::to_string)
    });
    assert_eq!(
        error.as_deref(),
        Some("No signal named V(n007) in the result")
    );
    assert!(h.rendered("trace-chip-0-0"));
}

/// D20: a sweep's bottom pane keeps room under its x labels for the swept
/// source's name; a transient's panes all have the same room, for the
/// labels alone.
#[gpui_kit::test]
fn only_a_sweeps_bottom_pane_has_room_for_an_x_title(cx: &mut TestAppContext) {
    let mut h = open(cx);
    let under = |h: &mut Harness, pane| {
        let geometry = h.plot_geometry(pane);
        (geometry.frame.bottom() - geometry.plot.bottom()).as_f32()
    };
    let mut sweep = transient();
    sweep.analysis = xsim_spice_engine::AnalysisKind::DcSweep;
    sweep.axis.name = "V1".into();
    sweep.axis.unit = "V".into();
    for (result, titled) in [(transient(), false), (sweep, true)] {
        h.show(result, PlotSetup::default());
        h.add("V(out)");
        h.viewer.update(&mut h.cx, |v, cx| v.add_pane(cx));
        h.cx.run_until_parked();
        h.add("V(in)");
        assert_eq!(h.setup().panes.len(), 2);
        let (top, bottom) = (under(&mut h, 0), under(&mut h, 1));
        assert!(top > 12.0, "room for the x labels: {top}");
        if titled {
            assert!(bottom > top + 10.0, "room for the title: {top} vs {bottom}");
        } else {
            assert_eq!(top, bottom);
        }
    }
}

/// Pane `pane` as its canvas lays it out on screen (scene coordinates are
/// relative to the canvas), with where it was painted.
fn screen_scene(h: &mut Harness, pane: usize) -> (PaneGeometry, crate::scene::PaneScene) {
    let geometry = h.plot_geometry(pane);
    let viewer = h.viewer.clone();
    let scene = h.cx.update(|window, app| {
        let v = viewer.read(app);
        let layout = super::pane_layout(v, window);
        let model = v.model().expect("a result");
        let result = v.result.clone().expect("a result");
        super::pane::plot(v, model, result, pane, layout).layout(geometry.frame)
    });
    (geometry, scene)
}

/// Review of D20 at 75 %, 100 % and 200 % UI scale (12, 16 and 32 px rem):
/// a mixed-unit pane and a plain one both show the same x tick labels at
/// the same x under their plots, keep every label inside their canvas,
/// line up, and start their legend row at the plot's left edge, above the
/// left axis.
#[gpui_kit::test]
fn every_pane_shows_the_x_labels_at_every_ui_scale(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    h.add("I(R1)");
    h.viewer.update(&mut h.cx, |v, cx| v.add_pane(cx));
    h.cx.run_until_parked();
    h.add("V(in)");
    assert_eq!(expressions(&h.setup(), 0), ["V(out)", "I(R1)"]);
    assert_eq!(expressions(&h.setup(), 1), ["V(in)"]);
    for rem in [12.0, 16.0, 32.0] {
        h.cx.update(|_, app| {
            gpui_kit::component::Theme::update(app, |theme| theme.font_size = px(rem));
        });
        h.viewer.update(&mut h.cx, |_, cx| cx.notify());
        h.cx.run_until_parked();
        let panes: Vec<_> = (0..2).map(|pane| screen_scene(&mut h, pane)).collect();
        let x_labels = |scene: &crate::scene::PaneScene| -> Vec<(String, f32)> {
            scene
                .labels
                .iter()
                .filter(|l| l.anchor == crate::scene::Anchor::Middle)
                .map(|l| (l.text.clone(), l.x))
                .collect()
        };
        let top_labels = x_labels(&panes[0].1);
        assert!(top_labels.len() >= 3, "{rem}: {top_labels:?}");
        assert!(top_labels.iter().all(|(text, _)| text.ends_with('s')));
        assert_eq!(top_labels, x_labels(&panes[1].1), "{rem}: same x labels");
        for (pane, (geometry, scene)) in panes.iter().enumerate() {
            // The scene is what was painted.
            let plot = geometry.plot;
            let origin = geometry.frame.origin;
            assert!(near(origin.x + px(scene.plot.x), plot.left()), "{rem}");
            assert!(near(origin.y + px(scene.plot.y), plot.top()), "{rem}");
            assert!(near(px(scene.plot.w), plot.size.width), "{rem}");
            // Every label lies inside the canvas, so none is clipped.
            let height = geometry.frame.size.height.as_f32();
            for label in &scene.labels {
                let (top, bottom) = (label.y - label.size / 2.0, label.y + label.size * 0.6);
                assert!(
                    top >= 0.0 && bottom <= height,
                    "{rem}, pane {pane}: {label:?} outside 0..{height}"
                );
            }
            // The x labels sit under the plot.
            for label in scene
                .labels
                .iter()
                .filter(|l| l.anchor == crate::scene::Anchor::Middle)
            {
                assert!(label.y > scene.plot.bottom(), "{rem}: {label:?}");
            }
        }
        let (top, bottom) = (&panes[0].0, &panes[1].0);
        assert_eq!(top.plot.left(), bottom.plot.left(), "{rem}: x aligned");
        assert_eq!(top.plot.right(), bottom.plot.right(), "{rem}: x aligned");
        let under = |g: &PaneGeometry| g.frame.bottom() - g.plot.bottom();
        assert_eq!(under(top), under(bottom), "{rem}: same room for labels");
        // The legend row starts above the left axis.
        for (pane, chip) in [(0, "trace-chip-0-0"), (1, "trace-chip-1-0")] {
            let chip = h.bounds(chip);
            let plot = h.plot(pane);
            assert!(
                near(chip.left(), plot.left()),
                "{rem}, pane {pane}: chip at {:?}, plot at {:?}",
                chip.left(),
                plot.left()
            );
        }
        // The right axis's chip ends the row of the mixed pane.
        let (left_group, right_group) =
            (h.bounds("pane-left-axis-0"), h.bounds("pane-right-axis-0"));
        assert!(right_group.left() >= left_group.right(), "{rem}");
        assert!(right_group.contains(&h.bounds("trace-chip-0-1").center()));
    }
    // Zooming x in one pane relabels both alike: the panes stay in step.
    let x_labels = |h: &mut Harness, pane| -> Vec<(String, f32)> {
        screen_scene(h, pane)
            .1
            .labels
            .iter()
            .filter(|l| l.anchor == crate::scene::Anchor::Middle)
            .map(|l| (l.text.clone(), l.x))
            .collect()
    };
    let before = x_labels(&mut h, 1);
    let center = h.plot(1).center();
    h.scroll(center, 3.0, Modifiers::default());
    assert!(h.setup().panes[0].x_range.is_some(), "x zoomed");
    let (top, bottom) = (x_labels(&mut h, 0), x_labels(&mut h, 1));
    assert_ne!(bottom, before);
    assert!(!top.is_empty());
    assert_eq!(top, bottom);
}

/// D9: a second unit gets a right axis in the same pane, a third unit a new
/// pane below.
#[gpui_kit::test]
fn a_current_gets_the_right_axis_and_a_third_unit_a_new_pane(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    h.add("I(R1)");
    assert_eq!(expressions(&h.setup(), 0), ["V(out)", "I(R1)"]);
    let geometry = h
        .viewer
        .read_with(&h.cx, |v, _| v.view.pane_geometry(0))
        .expect("pane 0 was painted");
    let right = geometry.y2.expect("a right axis").range();
    assert!(
        right.max > 1e-3 && right.max < 1.1e-3,
        "the current axis fits the current: {right:?}"
    );
    assert!(
        geometry.y.range().max > 1.0,
        "the left axis fits the voltage"
    );

    // The current's chip ends the header, in the right axis's group; the
    // voltage's starts it, above the left axis, with no title before it
    // (D20).
    assert!(h.rendered("pane-right-axis-0"));
    let group = h.bounds("pane-right-axis-0");
    let left_group = h.bounds("pane-left-axis-0");
    let (voltage, current) = (h.bounds("trace-chip-0-0"), h.bounds("trace-chip-0-1"));
    assert!(group.contains(&current.center()));
    assert!(!group.contains(&voltage.center()));
    assert!(left_group.contains(&voltage.center()));
    assert!(current.left() > voltage.right());
    assert!(
        group.left() >= left_group.right(),
        "the groups do not overlap"
    );
    assert!(
        voltage.left() <= h.plot(0).left() + px(2.),
        "the first chip starts the row: {voltage:?} vs {:?}",
        h.plot(0)
    );

    // Shift+wheel over the right axis labels zooms only that axis …
    let plot = h.plot(0);
    let over_right_axis = point(plot.right() + px(12.), plot.center().y);
    h.scroll(over_right_axis, 1.0, Modifiers::shift());
    let pane = h.setup().panes[0].clone();
    let [min, max] = pane.y2_range.expect("the right axis zoomed");
    assert!(max - min < 1.1e-3 * 0.9, "{min}..{max}");
    assert_eq!(pane.y_range, None);
    // … and over the plot it zooms both.
    h.scroll(plot.center(), 1.0, Modifiers::shift());
    let pane = h.setup().panes[0].clone();
    assert!(pane.y_range.is_some());
    assert_ne!(pane.y2_range, Some([min, max]));

    // Power is a third unit: a new pane below, the active pane stays.
    h.add("V(out)*I(R1)");
    let setup = h.setup();
    assert_eq!(setup.panes.len(), 2);
    assert_eq!(expressions(&setup, 0), ["V(out)", "I(R1)"]);
    assert_eq!(expressions(&setup, 1), ["V(out)*I(R1)"]);
    assert_eq!(setup.active_pane, 0);
    let (top, bottom) = (h.plot(0), h.plot(1));
    assert_eq!(top.left(), bottom.left(), "the panes' x axes line up");
    assert_eq!(top.right(), bottom.right());
    // Every pane keeps the same room under its plot for the x labels
    // (D20), not only the bottom one.
    let under = |h: &mut Harness, pane| {
        let geometry = h.plot_geometry(pane);
        (geometry.frame.bottom() - geometry.plot.bottom()).as_f32()
    };
    let (under_top, under_bottom) = (under(&mut h, 0), under(&mut h, 1));
    assert_eq!(under_top, under_bottom);
    assert!(under_top > 12.0, "room for a line of labels: {under_top}");
    // The power pane has a single unit: no right axis group.
    assert!(!h.rendered("pane-right-axis-1"));

    // A double click fits every axis again.
    let position = h.at(0, 0.5, 0.5);
    for click_count in [1, 2] {
        h.cx.simulate_event(MouseDownEvent {
            position,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
            click_count,
            first_mouse: false,
        });
        h.cx.simulate_event(MouseUpEvent {
            position,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
            click_count,
        });
    }
    h.cx.run_until_parked();
    let pane = &h.setup().panes[0];
    assert_eq!((pane.y_range, pane.y2_range), (None, None));
}

/// D9: a box zoom covers the same pixels on both y axes.
#[gpui_kit::test]
fn box_zoom_sets_both_y_axes(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    h.add("I(R1)");
    let geometry = h
        .viewer
        .read_with(&h.cx, |v, _| v.view.pane_geometry(0))
        .expect("pane 0 was painted");
    let fitted = |range: crate::model::axis::AxisRange| (range.min + range.max) / 2.0;
    let (middle, middle2) = (
        fitted(geometry.y.range()),
        fitted(geometry.y2.expect("a right axis").range()),
    );
    h.click("toggle-magnifier");
    let (from, to) = (h.at(0, 0.2, 0.25), h.at(0, 0.6, 0.75));
    h.drag(MouseButton::Left, from, to);
    let pane = h.setup().panes[0].clone();
    let [low, high] = pane.y_range.expect("the left axis zoomed");
    let [low2, high2] = pane.y2_range.expect("the right axis zoomed");
    // Both axes keep the middle half of their fitted ranges.
    let span = geometry.y.range().max - geometry.y.range().min;
    let span2 = geometry.y2.map_or(0.0, |m| m.range().max - m.range().min);
    assert!(close(high - low, span / 2.0), "{low}..{high}");
    assert!(close(high2 - low2, span2 / 2.0), "{low2}..{high2}");
    assert!(close((low + high) / 2.0, middle));
    assert!(close((low2 + high2) / 2.0, middle2));
}

/// Measurements of a right-axis trace are in its own unit.
#[gpui_kit::test]
fn right_axis_traces_are_measured_in_their_own_unit(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    h.add("I(R1)");
    h.click("trace-chip-0-1");
    h.click("toggle-measurements");
    let rows = h.viewer.read_with(&h.cx, |v, _| {
        assert_eq!(v.view.selected, Some((0, 1)));
        v.view
            .measured
            .borrow()
            .as_ref()
            .map(|(_, rows)| (**rows).clone())
    });
    let rows = rows.expect("the panel measured the current");
    let maximum = rows.iter().find(|r| r.name == "Maximum").unwrap();
    assert_eq!(maximum.unit, "A");
    assert!((maximum.value.unwrap() - 1e-3).abs() < 1e-12);
}

/// D11: a trace keeps its colour whatever happens to the others.
#[gpui_kit::test]
fn removing_a_trace_recolours_no_other_trace(cx: &mut TestAppContext) {
    let mut h = open(cx);
    let mut setup = PlotSetup::default();
    setup.panes[0].traces = vec![Trace::new("V(in)"), Trace::new("V(out)")];
    h.show(transient(), setup);
    // Traces from older files get their slots when the setup arrives.
    let slots: Vec<Option<u32>> = h.setup().panes[0].traces.iter().map(|t| t.slot).collect();
    assert_eq!(slots, [Some(0), Some(1)]);
    assert!(h.changes.get() >= 1, "pinning the slots is reported");
    h.add("V(out)-V(in)");

    let colors = |h: &mut Harness| {
        h.viewer.read_with(&h.cx, |v, _| {
            v.setup.panes[0]
                .traces
                .iter()
                .enumerate()
                .map(|(i, t)| (t.expression.clone(), v.colors.trace_color(t, i)))
                .collect::<Vec<_>>()
        })
    };
    let before = colors(&mut h);
    h.click("trace-remove-0-0");
    let after = colors(&mut h);
    assert_eq!(after.len(), 2);
    assert_eq!(
        after[..],
        before[1..],
        "the remaining traces keep their colours"
    );

    // The freed slot goes to the next trace.
    h.add("V(in)");
    let setup = h.setup();
    let slots: Vec<Option<u32>> = setup.panes[0].traces.iter().map(|t| t.slot).collect();
    assert_eq!(slots, [Some(1), Some(2), Some(0)]);
}

#[gpui_kit::test]
fn double_click_fits_without_moving_cursors(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    h.click("toggle-cursors");
    let before = h.cursors();
    let center = h.at(0, 0.5, 0.5);
    h.scroll(center, 1.0, Modifiers::default());
    let position = h.at(0, 0.1, 0.5);
    for click_count in [1, 2] {
        h.cx.simulate_event(MouseDownEvent {
            position,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
            click_count,
            first_mouse: false,
        });
        h.cx.simulate_event(MouseUpEvent {
            position,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
            click_count,
        });
    }
    h.cx.run_until_parked();
    assert_eq!(h.cursors(), before);
    assert_eq!(h.setup().panes[0].x_range, None);
}

/// D19: a cursor moves along x only. Dragging its line, also up or down,
/// moves x to the sample under the pointer; y follows the trace the cursor
/// follows, in the readout too, with Δx, Δy and 1/Δx.
#[gpui_kit::test]
fn cursors_move_along_x_and_read_their_trace(cx: &mut TestAppContext) {
    let mut h = open(cx);
    let axis = transient().axis.values;
    h.show(transient(), PlotSetup::default());
    h.add("V(in)");
    h.add("V(out)");
    h.click("trace-chip-0-1");
    h.click("toggle-cursors");
    let v_out = |t: f64| 1.0 - (-t / TAU).exp();

    // Both cursors follow the selected trace, at its value.
    for id in CursorId::ALL {
        let cursor = h.cursor_point(id);
        let on = cursor.on.expect("the cursor follows a trace");
        assert_eq!((on.pane, on.trace, on.unit.as_str()), (0, 1, "V"));
        assert!(close(on.y.expect("a value"), v_out(cursor.x)));
    }
    // Selecting another trace does not move them.
    h.click("trace-chip-0-0");
    assert_eq!(h.cursor_point(CursorId::First).on.map(|o| o.trace), Some(1));

    // Cursor 1's line, grabbed low and dragged up and to the right: only x
    // moves, to a sample, and y follows the trace.
    let from = point(h.line_x(CursorId::First), h.at(0, 0.5, 0.95).y);
    let to = h.at(0, 0.6, 0.1);
    h.drag(MouseButton::Left, from, to);
    let first = h.cursor_point(CursorId::First);
    assert_eq!(first.x, axis[60]);
    let y1 = first.on.as_ref().and_then(|on| on.y).expect("a value");
    assert!(close(y1, v_out(axis[60])));
    assert_eq!(h.setup().panes[0].x_range, None, "cursor drags do not pan");
    assert_eq!(h.setup().panes[0].y_range, None);

    // The readout shows x1, y1, x2, y2, Δx, Δy and 1/Δx.
    assert!(h.rendered("cursor-row-1") && h.rendered("cursor-row-2"));
    assert!(h.rendered("cursor-delta-row"));
    let x2 = h.cursor_point(CursorId::Second).x;
    assert_eq!(x2, axis[75]);
    let cells = h.cells();
    assert_eq!(
        cells.x,
        [format::value(axis[60], "s"), format::value(axis[75], "s")]
    );
    assert_eq!(
        cells.y,
        [
            format::value(v_out(axis[60]), "V"),
            format::value(v_out(axis[75]), "V")
        ]
    );
    assert_eq!(cells.dx, format::value(axis[75] - axis[60], "s"));
    assert_eq!(
        cells.dy,
        format::value(v_out(axis[75]) - v_out(axis[60]), "V")
    );
    let frequency = format::value(1.0 / (axis[75] - axis[60]), "Hz");
    assert_eq!(cells.inverse, format!("1/Δx = {frequency}"));

    // Cursor 2 dragged left past cursor 1: Δx and Δy turn negative.
    let from = point(h.line_x(CursorId::Second), h.at(0, 0.5, 0.5).y);
    let to = h.at(0, 0.2, 0.5);
    h.drag(MouseButton::Left, from, to);
    let cells = h.cells();
    assert_eq!(
        cells.y[1],
        format::value(v_out(axis[20]), "V"),
        "y2 follows the trace"
    );
    assert_eq!(cells.dx, format::value(axis[20] - axis[60], "s"));
    assert_eq!(
        cells.dy,
        format::value(v_out(axis[20]) - v_out(axis[60]), "V")
    );
}

/// D19 with D9: a cursor on a right-axis trace reads that trace in its own
/// unit as it moves along x; the readout's buttons choose the trace each
/// cursor follows, and Δy is a dash between amperes and volts.
#[gpui_kit::test]
fn cursors_read_right_axis_traces_in_their_unit(cx: &mut TestAppContext) {
    let mut h = open(cx);
    let axis = transient().axis.values;
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    h.add("I(R1)");
    h.click("toggle-cursors");
    assert_eq!(h.cursor_point(CursorId::First).on.map(|o| o.trace), Some(0));
    h.click("attach-cursor-1-0-1");
    let current = |t: f64| (-t / TAU).exp() / 1e3;
    let first = h.cursor_point(CursorId::First);
    let on = first.on.expect("a trace");
    assert_eq!((on.pane, on.trace, on.unit.as_str()), (0, 1, "A"));
    assert!(close(on.y.expect("a value"), current(first.x)));
    assert_eq!(
        h.cursor_point(CursorId::Second).on.map(|o| o.trace),
        Some(0),
        "cursor 2 stays on the voltage"
    );

    // Dragged along x, the value follows the current.
    let from = point(h.line_x(CursorId::First), h.at(0, 0.5, 0.95).y);
    let to = h.at(0, 0.4, 0.95);
    h.drag(MouseButton::Left, from, to);
    let first = h.cursor_point(CursorId::First);
    assert_eq!(first.x, axis[40]);
    let y = first.on.and_then(|on| on.y).expect("a value");
    assert!(close(y, current(axis[40])), "{y}");
    let cells = h.cells();
    assert_eq!(cells.y[0], format::value(current(axis[40]), "A"));
    assert_eq!(cells.dy, "—", "Δy has no meaning between amperes and volts");
    assert_ne!(cells.dx, "—");

    // The per-trace table reads every trace with its buttons.
    for button in [
        "attach-cursor-1-0-0",
        "attach-cursor-2-0-0",
        "attach-cursor-1-0-1",
        "attach-cursor-2-0-1",
    ] {
        assert!(h.rendered(button), "{button}");
    }
}

/// D19: pane `pane` paints each cursor as one vertical line across the plot
/// at the cursor's x, with one flag at its top, and nothing else: no
/// horizontal line, no flag at a line's end and no ring. The cursors are
/// far enough apart for both flags to sit on top.
fn assert_vertical_lines_only(h: &mut Harness, pane: usize) {
    let geometry = h.plot_geometry(pane);
    let plot = geometry.plot;
    let lines: Vec<_> = geometry.cursors.iter().flatten().copied().collect();
    assert_eq!(lines.len(), 2, "pane {pane}: one line per cursor");
    for (line, id) in lines.iter().zip(CursorId::ALL) {
        let x = geometry.x.px(h.cursor_point(id).x);
        assert_eq!(line.id, id);
        assert_eq!(line.bounds.size.width, px(1.), "pane {pane}: {line:?}");
        assert_eq!(line.bounds.top(), plot.top(), "pane {pane}: {line:?}");
        assert_eq!(
            line.bounds.size.height, plot.size.height,
            "pane {pane}: {line:?}"
        );
        assert!((line.bounds.center().x.as_f32() - x).abs() < 0.01);
    }
    let flags: Vec<_> = geometry.flags.iter().flatten().copied().collect();
    assert_eq!(flags.len(), 2, "pane {pane}: one flag per cursor");
    for (flag, line) in flags.iter().zip(&lines) {
        assert_eq!(flag.id, line.id);
        assert_eq!(flag.bounds.top(), plot.top(), "pane {pane}: {flag:?}");
        let offset = flag.bounds.center().x - line.bounds.center().x;
        assert!(offset.as_f32().abs() < 0.01, "pane {pane}: {flag:?}");
    }
}

/// D19: no cursor has a horizontal line. Every pane, the followed traces'
/// panes included, paints the same vertical lines and nothing else, and a
/// press on a trace where D14's horizontal line was grabs no cursor: the
/// pointer stays an arrow and the drag pans like any other, moving no
/// cursor, also on a right axis.
#[gpui_kit::test]
fn no_horizontal_cursor_line_is_drawn_or_draggable(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    h.add("I(R1)");
    h.click("add-pane");
    h.add("V(in)");
    h.click("toggle-cursors");
    h.click("attach-cursor-2-0-1");
    let marks = |h: &mut Harness, pane: usize| h.overlay(pane).cursors;
    let (top, bottom) = (marks(&mut h, 0), marks(&mut h, 1));
    assert_eq!(top.len(), 2);
    assert_eq!(top, bottom, "a cursor is only an x, the same in every pane");
    for pane in [0, 1] {
        assert_vertical_lines_only(&mut h, pane);
    }
    let centres = |h: &mut Harness, pane: usize| {
        let lines = h.plot_geometry(pane).cursors;
        lines.map(|line| line.map(|l| l.bounds.center().x))
    };
    assert_eq!(
        centres(&mut h, 0),
        centres(&mut h, 1),
        "the lines line up across the panes"
    );

    let before = h.cursors();
    for id in CursorId::ALL {
        // On the trace, away from the vertical line (left axis for cursor
        // 1's voltage, right axis for cursor 2's current).
        let on = h.on_trace(id);
        let grab = point(on.x + px(80.), on.y);
        let plot = h.plot(0);
        assert!(plot.contains(&grab), "{grab:?} inside {plot:?}");
        h.cx.simulate_mouse_move(grab, None, Modifiers::default());
        h.cx.run_until_parked();
        let hit = h.viewer.read_with(&h.cx, |v, _| v.cursor_hit(0, grab));
        assert_eq!(hit, None, "{id:?}: nothing to grab there");
        assert_eq!(h.pointer(0), CursorStyle::Arrow, "{id:?}");

        // Pressed there and moved up or down: a pan, not a cursor drag.
        let shift = if grab.y < plot.center().y { 40. } else { -40. };
        let to = point(grab.x, grab.y + px(shift));
        press_and_move(&mut h, MouseButton::Left, grab, to);
        let drag = h.viewer.read_with(&h.cx, |v, _| v.view.drag);
        assert!(matches!(drag, Some(Drag::Pan { .. })), "{id:?}: {drag:?}");
        assert_eq!(h.cursors(), before, "{id:?}: no cursor moves");
        h.cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::default());
        h.cx.run_until_parked();
        assert_eq!(h.cursors(), before, "{id:?}: no cursor moved");
    }

    // On the line itself the pointer offers a left-right drag.
    let line = point(h.line_x(CursorId::First), h.at(0, 0.5, 0.5).y);
    h.cx.simulate_mouse_move(line, None, Modifiers::default());
    h.cx.run_until_parked();
    assert_eq!(h.pointer(0), CursorStyle::ResizeLeftRight);
}

/// What the last frame painted in the cursor colour over pane `pane`'s data
/// area, in window pixels: the quads filled with it, and how many quads
/// have it as their border (as D14's rings on the trace did).
fn painted_in_cursor_colour(h: &mut Harness, pane: usize) -> (Vec<Bounds<Pixels>>, usize) {
    let plot = h.plot(pane);
    let color = h
        .viewer
        .read_with(&h.cx, |v, _| super::theme::hsla(v.colors.cursor));
    let (quads, scale) =
        h.cx.update(|window, _| (window.painted_quads(), window.scale_factor()));
    let mut filled = Vec::new();
    let mut rings = 0;
    for quad in quads {
        let b = quad.bounds;
        let bounds = Bounds::new(
            point(px(b.origin.x.0 / scale), px(b.origin.y.0 / scale)),
            size(px(b.size.width.0 / scale), px(b.size.height.0 / scale)),
        );
        if !bounds.intersects(&plot) {
            continue;
        }
        if quad.background.as_solid() == Some(color) {
            filled.push(bounds);
        }
        let w = quad.border_widths;
        if quad.border_color == color
            && [w.top, w.right, w.bottom, w.left].iter().any(|w| w.0 > 0.0)
        {
            rings += 1;
        }
    }
    (filled, rings)
}

/// Painting snaps quads to device pixels, so a painted quad may sit up to
/// half a pixel from where it was laid out.
const SNAP: f32 = 0.5;

fn near(a: Pixels, b: Pixels) -> bool {
    (a - b).abs().as_f32() <= SNAP
}

fn same_bounds(a: &Bounds<Pixels>, b: &Bounds<Pixels>) -> bool {
    near(a.left(), b.left())
        && near(a.top(), b.top())
        && near(a.size.width, b.size.width)
        && near(a.size.height, b.size.height)
}

/// D19, as painted: over every pane the cursors leave only their vertical
/// lines, cursor 1's one solid stroke and cursor 2's dashed, and the two
/// flags at the top, in the cursor colour. Nothing runs across the plot and
/// nothing rings the point on the trace, also for a right-axis trace.
#[gpui_kit::test]
fn the_painted_cursors_are_vertical_lines_and_flags_only(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    h.add("I(R1)");
    h.click("add-pane");
    h.add("V(in)");
    h.click("toggle-cursors");
    h.click("attach-cursor-2-0-1");
    let xs = CursorId::ALL.map(|id| h.cursor_point(id).x);
    for pane in [0, 1] {
        let geometry = h.plot_geometry(pane);
        let plot = geometry.plot;
        let (quads, rings) = painted_in_cursor_colour(&mut h, pane);
        assert_eq!(rings, 0, "pane {pane}: no ring on a trace");

        let flags: Vec<_> = geometry.flags.iter().flatten().map(|f| f.bounds).collect();
        assert_eq!(flags.len(), 2, "pane {pane}");
        for flag in &flags {
            let painted = quads.iter().filter(|q| same_bounds(q, flag)).count();
            assert_eq!(painted, 1, "pane {pane}: the flag {flag:?} is painted");
        }
        let strokes: Vec<_> = quads
            .iter()
            .filter(|q| !flags.iter().any(|f| same_bounds(q, f)))
            .copied()
            .collect();
        for stroke in &strokes {
            assert!(
                stroke.size.width <= px(1.0 + SNAP),
                "pane {pane}: a stroke across the plot: {stroke:?}"
            );
        }
        let on_line = |x: f64| -> Vec<Bounds<Pixels>> {
            let x = geometry.x.px(x);
            strokes
                .iter()
                .filter(|q| near(q.center().x, px(x)))
                .copied()
                .collect()
        };
        let (solid, dashed) = (on_line(xs[0]), on_line(xs[1]));
        assert_eq!(
            solid.len() + dashed.len(),
            strokes.len(),
            "pane {pane}: every stroke is on a cursor line"
        );

        // Cursor 1: one stroke over the plot's whole height.
        assert_eq!(solid.len(), 1, "pane {pane}: {solid:?}");
        assert!(near(solid[0].top(), plot.top()), "pane {pane}: {solid:?}");
        assert!(near(solid[0].size.height, plot.size.height), "pane {pane}");

        // Cursor 2: 4 px dashes 3 px apart, from the top to the bottom.
        assert!(dashed.len() > 10, "pane {pane}: {dashed:?}");
        assert!(dashed.iter().all(|d| d.size.height <= px(4.0 + SNAP)));
        let top = dashed.iter().map(|d| d.top()).min().expect("dashes");
        let bottom = dashed.iter().map(|d| d.bottom()).max().expect("dashes");
        assert!(near(top, plot.top()), "pane {pane}: {top:?}");
        let end = plot.bottom() + px(SNAP);
        assert!(bottom <= end && bottom > end - px(7.0 + SNAP), "{bottom:?}");
        let covered: f32 = dashed.iter().map(|d| d.size.height.as_f32()).sum();
        let height = plot.size.height.as_f32();
        assert!(
            (covered - height * 4.0 / 7.0).abs() <= 4.0,
            "pane {pane}: {covered} of {height} px dashed"
        );
    }
}

/// D19: a cursor's line is grabbed in any pane, not only in the pane of the
/// trace it follows. Moved straight up or down it keeps its sample and pans
/// nothing; moved sideways it takes the sample under the pointer in every
/// pane at once, and its y follows its trace in the other pane.
#[gpui_kit::test]
fn a_cursor_line_drags_along_x_from_any_pane(cx: &mut TestAppContext) {
    let mut h = open(cx);
    let axis = transient().axis.values;
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    h.click("add-pane");
    h.add("V(in)");
    h.click("toggle-cursors");
    h.click("attach-cursor-1-0-0");
    let on = h.cursor_point(CursorId::First).on.expect("a trace");
    assert_eq!((on.pane, on.expression.as_str()), (0, "V(out)"));
    let before = h.cursors();
    let views = |h: &mut Harness| {
        let setup = h.setup();
        let ranges: Vec<_> = setup
            .panes
            .iter()
            .map(|p| (p.x_range, p.y_range, p.y2_range))
            .collect();
        ranges
    };
    let view = views(&mut h);

    // Straight down the line in the lower pane: a cursor drag that keeps x.
    let x = h.line_x(CursorId::First);
    let (from, to) = (point(x, h.at(1, 0.5, 0.2).y), point(x, h.at(1, 0.5, 0.9).y));
    press_and_move(&mut h, MouseButton::Left, from, to);
    let drag = h.viewer.read_with(&h.cx, |v, _| v.view.drag);
    assert_eq!(drag, Some(Drag::Cursor(CursorId::First)));
    assert_eq!(h.pointer(1), CursorStyle::ResizeLeftRight);
    assert_eq!(
        h.cursors(),
        before,
        "moving along the line keeps the sample"
    );
    h.cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::default());
    h.cx.run_until_parked();
    assert_eq!(h.cursors(), before);
    assert_eq!(views(&mut h), view, "nothing panned");

    // Sideways: the sample under the pointer, in both panes.
    let from = point(h.line_x(CursorId::First), h.at(1, 0.5, 0.5).y);
    let to = h.at(1, 0.4, 0.7);
    h.drag(MouseButton::Left, from, to);
    let first = h.cursor_point(CursorId::First);
    assert_eq!(first.x, axis[40]);
    assert_eq!(h.cursors().second, before.second, "cursor 2 stays");
    for pane in [0, 1] {
        let geometry = h.plot_geometry(pane);
        let line = geometry.cursors[0].expect("cursor 1's line");
        let x = geometry.x.px(axis[40]);
        assert!(
            (line.bounds.center().x.as_f32() - x).abs() < 0.01,
            "pane {pane}"
        );
    }
    let v_out = 1.0 - (-axis[40] / TAU).exp();
    assert_eq!(
        h.cells().y[0],
        format::value(v_out, "V"),
        "y1 follows V(out) in the pane above"
    );
    assert_eq!(views(&mut h), view, "nothing panned");
}

/// The measurements panel measures the selected trace between the cursors
/// (Q135), and follows a cursor line dragged along x.
#[gpui_kit::test]
fn measurements_follow_a_dragged_cursor(cx: &mut TestAppContext) {
    let mut h = open(cx);
    let axis = transient().axis.values;
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    h.click("trace-chip-0-0");
    h.click("toggle-measurements");
    h.click("toggle-cursors");
    let v_out = |t: f64| 1.0 - (-t / TAU).exp();
    let measured = |h: &mut Harness, name: &str| {
        h.viewer.read_with(&h.cx, |v, _| {
            let measured = v.view.measured.borrow();
            let (_, rows) = measured.as_ref().expect("the panel measured");
            rows.iter()
                .find(|r| r.name == name)
                .and_then(|r| r.value)
                .expect("a value")
        })
    };
    assert!(close(measured(&mut h, "Minimum"), v_out(axis[25])));
    assert!(close(measured(&mut h, "Maximum"), v_out(axis[75])));

    let from = point(h.line_x(CursorId::Second), h.at(0, 0.5, 0.5).y);
    let to = h.at(0, 0.5, 0.3);
    h.drag(MouseButton::Left, from, to);
    assert_eq!(h.cursors().second, Some(axis[50]));
    assert!(close(measured(&mut h, "Minimum"), v_out(axis[25])));
    assert!(close(measured(&mut h, "Maximum"), v_out(axis[50])));
}

/// D14: a legend chip dragged onto another pane moves its trace there,
/// keeping its colour; the source pane, a refusing pane (D9) and empty
/// space change nothing.
#[gpui_kit::test]
fn dragging_a_chip_moves_the_trace_to_another_pane(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    h.add("I(R1)");
    h.click("add-pane");
    h.add("V(out)*I(R1)");
    assert_eq!(expressions(&h.setup(), 1), ["V(out)*I(R1)"]);
    h.click("trace-chip-0-1");
    h.click("toggle-cursors");
    h.click("attach-cursor-2-0-1");
    let slot = h.setup().panes[0].traces[1].slot;
    let before = h.changes.get();

    // The current moves below: a second unit there.
    let below = h.at(1, 0.5, 0.5);
    h.drag_chip("trace-chip-0-1", below, |h| {
        assert!(h.rendered("pane-drop-1"), "the target pane is highlighted");
        assert!(!h.rendered("pane-drop-refused-1"));
    });
    let setup = h.setup();
    assert_eq!(expressions(&setup, 0), ["V(out)"]);
    assert_eq!(expressions(&setup, 1), ["V(out)*I(R1)", "I(R1)"]);
    assert_eq!(setup.panes[1].traces[1].slot, slot, "the colour is kept");
    assert!(h.changes.get() > before, "the move is saved");
    assert!(
        !h.rendered("pane-drop-1"),
        "the highlight ends with the drop"
    );
    let selected = h.viewer.read_with(&h.cx, |v, _| v.view.selected);
    assert_eq!(selected, Some((1, 1)), "the selection follows the trace");
    let on = h.cursor_point(CursorId::Second).on.expect("a trace");
    assert_eq!((on.pane, on.trace), (1, 1), "and so does cursor 2");

    // The voltage would be a third unit below: refused, with feedback.
    let before = h.setup();
    h.drag_chip("trace-chip-0-0", below, |h| {
        assert!(h.rendered("pane-drop-refused-1"), "a not-allowed highlight");
        assert!(!h.rendered("pane-drop-1"));
    });
    assert_eq!(h.setup(), before, "a refused drop changes nothing");
    assert!(!h.rendered("pane-drop-refused-1"));

    // Back onto its own pane, or onto the toolbar: nothing happens.
    h.drag_chip("trace-chip-1-0", below, |h| {
        assert!(!h.rendered("pane-drop-1") && !h.rendered("pane-drop-refused-1"));
    });
    let toolbar = h.bounds("zoom-to-fit").center();
    h.drag_chip("trace-chip-1-0", toolbar, |h| {
        // D23: the dividers make way while a chip is dragged.
        assert!(!h.rendered("pane-divider-0"));
    });
    assert_eq!(h.setup(), before);
    let target = h.viewer.read_with(&h.cx, |v, _| v.view.drop_target);
    assert_eq!(target, None);
    assert!(h.rendered("pane-divider-0"), "and are back after it");

    // A click without a drag still selects.
    h.click("trace-chip-0-0");
    let selected = h.viewer.read_with(&h.cx, |v, _| v.view.selected);
    assert_eq!(selected, Some((0, 0)));
}

/// D14: the completion list opens below the field, never over it.
#[gpui_kit::test]
fn suggestions_open_below_the_field(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.click("trace-input");
    h.cx.simulate_input("V(");
    h.cx.run_until_parked();
    let (field, list) = (h.bounds("trace-input"), h.bounds("trace-suggestions"));
    assert!(list.top() > field.bottom(), "{list:?} below {field:?}");
    assert!(!list.intersects(&field));
    assert_eq!(list.left(), field.left());
    // The error callout is placed the same way.
    h.cx.simulate_keystrokes("escape");
    h.cx.simulate_input("nope)");
    h.cx.simulate_keystrokes("enter");
    h.cx.run_until_parked();
    let error = h.bounds("trace-input-error");
    assert!(error.top() > field.bottom() && !error.intersects(&field));
}

/// D14: without room below, the list opens above the field, still without
/// covering it.
#[gpui_kit::test]
fn suggestions_open_above_without_room_below(cx: &mut TestAppContext) {
    let mut h = open_squeezed(cx);
    h.show(transient(), PlotSetup::default());
    h.click("trace-input");
    h.cx.simulate_input("V(");
    h.cx.run_until_parked();
    let (field, list) = (h.bounds("trace-input"), h.bounds("trace-suggestions"));
    assert!(field.top() > px(600.), "the field sits low: {field:?}");
    assert!(list.bottom() < field.top(), "{list:?} above {field:?}");
    assert!(!list.intersects(&field));
}

/// A suggestion clicked in the repositioned list is accepted.
#[gpui_kit::test]
fn clicking_a_suggestion_accepts_it(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.click("trace-input");
    h.cx.simulate_input("V(o");
    h.cx.run_until_parked();
    assert!(h.rendered("trace-suggestion-0"));
    h.click("trace-suggestion-0");
    assert_eq!(h.input_text(), "V(out)");
    assert!(
        h.setup().panes[0].traces.is_empty(),
        "nothing plotted by a click"
    );
}

/// Escape right after choosing the magnifier turns it off again.
#[gpui_kit::test]
fn escape_after_choosing_the_magnifier_turns_it_off(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    h.click("toggle-magnifier");
    assert!(h.viewer.read_with(&h.cx, |v, _| v.view.magnifier));
    h.cx.simulate_keystrokes("escape");
    h.cx.run_until_parked();
    assert!(!h.viewer.read_with(&h.cx, |v, _| v.view.magnifier));
}

/// Escape while a legend chip is dragged cancels the move.
#[gpui_kit::test]
fn escape_cancels_a_chip_drag(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    h.click("add-pane");
    h.add("V(in)");
    let before = h.setup();
    let below = h.at(1, 0.5, 0.5);
    h.drag_chip("trace-chip-0-0", below, |h| {
        assert!(h.rendered("pane-drop-1"));
        h.cx.simulate_keystrokes("escape");
        h.cx.run_until_parked();
        assert!(!h.rendered("pane-drop-1"), "the highlight goes away");
        // D23: the divider, left out while the chip was dragged, is back.
        assert!(h.rendered("pane-divider-0"), "the divider is back");
    });
    assert_eq!(h.setup(), before, "the trace stays where it was");
    let target = h.viewer.read_with(&h.cx, |v, _| v.view.drop_target);
    assert_eq!(target, None);
    assert!(h.rendered("pane-divider-0"));
}

/// A cursor keeps following its trace when a pane above it is removed.
#[gpui_kit::test]
fn cursors_keep_their_trace_when_a_pane_above_goes(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    h.click("add-pane");
    h.add("V(in)");
    h.click("toggle-cursors");
    h.click("attach-cursor-2-1-0");
    let on = h.cursor_point(CursorId::Second).on.expect("a trace");
    assert_eq!((on.pane, on.expression.as_str()), (1, "V(in)"));
    h.click("pane-close-0");
    assert_eq!(h.setup().panes.len(), 1);
    let on = h.cursor_point(CursorId::Second).on.expect("a trace");
    assert_eq!((on.pane, on.trace, on.expression.as_str()), (0, 0, "V(in)"));
}

/// Cursors work on Bode panes: y is the magnitude in decibels, and both
/// panes draw the same vertical lines and nothing else (D19).
#[gpui_kit::test]
fn cursors_work_on_bode_panes(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(ac(), PlotSetup::default());
    h.add("V(out)");
    h.click("toggle-cursors");
    let cursor = h.cursor_point(CursorId::First);
    let on = cursor.on.expect("a trace");
    assert_eq!((on.pane, on.unit.as_str()), (0, "dB"));
    // |H| of a 1 kHz first-order low-pass, interpolated in decades.
    let expected = -10.0 * (1.0 + (cursor.x / 1e3).powi(2)).log10();
    assert!((on.y.expect("a value") - expected).abs() < 0.05, "{on:?}");
    let (magnitude, phase) = (h.overlay(0), h.overlay(1));
    assert_eq!(magnitude.cursors.len(), 2);
    assert_eq!(magnitude.cursors, phase.cursors);
    for pane in [0, 1] {
        assert_vertical_lines_only(&mut h, pane);
    }
    assert_eq!(h.cells().inverse, "", "no 1/Δx on a frequency axis");

    // Cursor 1's line dragged in the phase pane: x moves in decades to a
    // sample, and y, still the magnitude it follows, is −3 dB at 1 kHz.
    let axis = ac().axis.values;
    let from = point(h.line_x(CursorId::First), h.at(1, 0.5, 0.5).y);
    let to = h.at(1, 0.5, 0.8);
    h.drag(MouseButton::Left, from, to);
    let first = h.cursor_point(CursorId::First);
    assert_eq!(first.x, axis[30]);
    let on = first.on.expect("a trace");
    assert_eq!((on.pane, on.unit.as_str()), (0, "dB"));
    let y = on.y.expect("a value");
    assert!((y + 10.0 * 2f64.log10()).abs() < 1e-6, "{y}");
    for pane in [0, 1] {
        assert_vertical_lines_only(&mut h, pane);
    }

    // Cursor 2 on the phase reads degrees; Δy has no meaning across them.
    h.click("attach-cursor-2-1-0");
    let second = h.cursor_point(CursorId::Second);
    let on = second.on.expect("the phase");
    assert_eq!((on.pane, on.unit.as_str()), (1, "°"));
    let phase = -(second.x / 1e3).atan().to_degrees();
    let read = on.y.expect("a value");
    assert!((read - phase).abs() < 1e-6, "{on:?}");
    let cells = h.cells();
    assert_eq!(cells.y[1], format::value(read, "°"));
    assert_eq!(cells.dy, "—", "no Δy between decibels and degrees");
}

/// A viewer below a spacer whose height a test changes, as when the shell's
/// window or split is resized while the completion list is open.
struct Shifting {
    viewer: Entity<WaveformViewer>,
    top: Rc<Cell<f32>>,
}

impl Render for Shifting {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(div().h(px(self.top.get())).flex_shrink_0())
            .child(div().flex_1().flex().child(self.viewer.clone()))
    }
}

/// D14: the completion list follows the field when the layout moves it, so
/// it never ends up over the typed formula.
#[gpui_kit::test]
fn the_completion_list_follows_a_moving_field(cx: &mut TestAppContext) {
    let top = Rc::new(Cell::new(0.0f32));
    let spacer = top.clone();
    let (window, viewer, root) = cx.update(|cx| {
        gpui_kit::init(cx);
        let mut built = None;
        let window = gpui_kit::open_window(options(), cx, |_, cx| {
            let viewer = cx.new(WaveformViewer::new);
            let root = cx.new(|_| Shifting {
                viewer: viewer.clone(),
                top: spacer,
            });
            built = Some((viewer, root.clone()));
            root
        })
        .expect("open a test window")
        .0;
        let (viewer, root) = built.expect("the viewer was built");
        (window, viewer, root)
    });
    let mut h = finish(cx, window, viewer);
    h.show(transient(), PlotSetup::default());
    h.click("trace-input");
    h.cx.simulate_input("V(");
    h.cx.run_until_parked();
    let list = h.bounds("trace-suggestions");
    assert!(list.top() > h.bounds("trace-input").bottom());

    // The field moves down by 200 px while the list is open.
    top.set(200.0);
    root.update(&mut h.cx, |_, cx| cx.notify());
    h.cx.run_until_parked();
    // Deliver the frame the moved field asks for.
    h.cx.update(|window, cx| window.simulate_next_frame(cx));
    h.cx.run_until_parked();
    let (field, list) = (h.bounds("trace-input"), h.bounds("trace-suggestions"));
    assert!(field.top() > px(200.), "{field:?}");
    assert!(list.top() > field.bottom(), "{list:?} below {field:?}");
    assert!(!list.intersects(&field));
}

/// The error callout lies over the top pane's legend; it shows only while
/// the field has focus, so the chips below stay usable.
#[gpui_kit::test]
fn the_error_shows_only_while_the_field_has_focus(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(in)");
    h.add("V(out)");
    h.type_trace("V(nope)");
    let error = h.bounds("trace-input-error");
    let chip = h.bounds("trace-chip-0-1");
    assert!(error.intersects(&chip), "the callout covers a chip");

    // A press on the callout reaches nothing below it (here the hidden
    // remove button of a chip) and dismisses it.
    let remove = h.bounds("trace-remove-0-0").center();
    assert!(error.contains(&remove));
    h.click_at(remove);
    assert_eq!(expressions(&h.setup(), 0), ["V(in)", "V(out)"]);
    assert!(!h.rendered("trace-input-error"));

    // Back in the field it shows again; clicking the plot moves the focus
    // away, and the chip under the callout takes a click.
    h.click("trace-input");
    assert!(h.rendered("trace-input-error"));
    let center = h.at(0, 0.5, 0.5);
    h.click_at(center);
    assert!(!h.rendered("trace-input-error"));
    let under = point(chip.left() + px(10.), chip.center().y);
    assert!(error.contains(&under));
    h.click_at(under);
    let selected = h.viewer.read_with(&h.cx, |v, _| v.view.selected);
    assert_eq!(selected, Some((0, 1)));

    // Back in the field, the error shows again until the text is edited.
    h.click("trace-input");
    assert!(h.rendered("trace-input-error"));
    h.cx.simulate_input("x");
    h.cx.run_until_parked();
    assert!(!h.rendered("trace-input-error"));
}

/// D14: shown cursors stay on their trace when the selection changes, also
/// when they were turned on before any trace was plotted or their trace
/// was removed.
#[gpui_kit::test]
fn cursors_stay_on_their_trace_whatever_is_selected(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(in)");
    h.click("trace-remove-0-0");
    h.click("toggle-cursors");
    h.add("V(in)");
    h.add("V(out)");
    h.add("I(R1)");
    let on = |h: &mut Harness| h.cursor_point(CursorId::First).on.map(|o| o.trace);
    assert_eq!(on(&mut h), Some(0), "on the first trace plotted");
    h.click("trace-chip-0-1");
    assert_eq!(on(&mut h), Some(0), "selecting another trace moves nothing");

    // Its trace removed, the cursor takes the selected one and keeps it.
    h.click("trace-remove-0-0");
    let expression = h.cursor_point(CursorId::First).on.map(|o| o.expression);
    assert_eq!(expression.as_deref(), Some("V(out)"));
    h.click("trace-chip-0-1");
    let expression = h.cursor_point(CursorId::First).on.map(|o| o.expression);
    assert_eq!(expression.as_deref(), Some("V(out)"));
}

/// A pane whose last trace is dragged away forgets its y zoom, so the next
/// trace there, of another unit, is not drawn off screen.
#[gpui_kit::test]
fn a_pane_emptied_by_a_chip_drag_keeps_no_y_range(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    h.add("I(R1)");
    h.click("add-pane");
    h.add("V(in)");
    let center = h.at(1, 0.5, 0.5);
    h.scroll(center, 3.0, Modifiers::shift());
    assert!(h.setup().panes[1].y_range.is_some(), "zoomed on volts");
    let above = h.at(0, 0.5, 0.5);
    h.drag_chip("trace-chip-1-0", above, |_| {});
    let setup = h.setup();
    assert!(setup.panes[1].traces.is_empty());
    assert_eq!(setup.panes[1].y_range, None);
    // The current dropped there is fitted.
    let below = h.at(1, 0.5, 0.5);
    h.drag_chip("trace-chip-0-1", below, |_| {});
    assert_eq!(expressions(&h.setup(), 1), ["I(R1)"]);
    assert_eq!(h.setup().panes[1].y_range, None);
}

/// A wheel event without movement and a box over all the data leave the
/// axes automatic and the project unchanged.
#[gpui_kit::test]
fn gestures_that_show_everything_keep_the_axes_automatic(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    let before = h.changes.get();
    let center = h.at(0, 0.5, 0.5);
    h.scroll(center, 0.0, Modifiers::default());
    h.scroll(center, 0.0, Modifiers::shift());
    h.click("toggle-magnifier");
    let plot = h.plot(0);
    let (from, to) = (
        point(plot.left() - px(10.), plot.top() - px(5.)),
        point(plot.right() + px(10.), plot.bottom() + px(5.)),
    );
    h.drag(MouseButton::Left, from, to);
    let pane = h.setup().panes[0].clone();
    assert_eq!((pane.x_range, pane.y_range), (None, None));
    assert_eq!(h.changes.get(), before, "nothing to save");
}

/// The smallest plottable result (two samples) takes every gesture.
#[gpui_kit::test]
fn a_two_sample_result_takes_every_gesture(cx: &mut TestAppContext) {
    let mut h = open(cx);
    let mut result = crate::test_fixtures::tiny();
    result.axis.values.truncate(2);
    if let xsim_spice_engine::SignalData::Real(values) = &mut result.signals[0].data {
        values.truncate(2);
    }
    h.show(result, PlotSetup::default());
    h.add("V(out)");
    h.click("toggle-cursors");
    let cursors = h.cursors();
    assert_eq!((cursors.first, cursors.second), (Some(0.0), Some(1e-3)));
    let center = h.at(0, 0.5, 0.5);
    h.scroll(center, 5.0, Modifiers::default());
    h.scroll(center, -50.0, Modifiers::default());
    assert_eq!(h.setup().panes[0].x_range, None);
    let (from, to) = (h.at(0, 0.3, 0.3), h.at(0, 0.7, 0.7));
    h.drag(MouseButton::Left, from, to);
    // Cursor 2's line, at the plot's right edge, dragged to 30 %: it snaps
    // to the nearer sample, and y follows.
    let from = point(h.line_x(CursorId::Second), h.at(0, 0.5, 0.5).y);
    let to = h.at(0, 0.3, 0.2);
    h.drag(MouseButton::Left, from, to);
    let second = h.cursor_point(CursorId::Second);
    assert_eq!(second.x, 0.0);
    let y = second.on.as_ref().and_then(|on| on.y).expect("a value");
    assert_eq!(y, 0.0, "on the line: {second:?}");
}

/// Presses `button` at `from` and moves to `to` with it held, without
/// releasing it.
fn press_and_move(h: &mut Harness, button: MouseButton, from: Point<Pixels>, to: Point<Pixels>) {
    let none = Modifiers::default();
    h.cx.simulate_mouse_move(from, None, none);
    h.cx.simulate_mouse_down(from, button, none);
    let step = point(
        from.x + (to.x - from.x) * 0.5,
        from.y + (to.y - from.y) * 0.5,
    );
    h.cx.simulate_mouse_move(step, button, none);
    h.cx.simulate_mouse_move(to, button, none);
    h.cx.run_until_parked();
}

/// Presses Escape, moves further and releases `button`.
fn escape_then_release(h: &mut Harness, button: MouseButton, at: Point<Pixels>) {
    let none = Modifiers::default();
    h.cx.simulate_keystrokes("escape");
    h.cx.run_until_parked();
    let further = point(at.x + px(30.), at.y + px(20.));
    h.cx.simulate_mouse_move(further, button, none);
    h.cx.simulate_mouse_up(further, button, none);
    h.cx.run_until_parked();
}

/// The stored ranges of pane 0: x, y and right y.
type Ranges = (Option<[f64; 2]>, Option<[f64; 2]>, Option<[f64; 2]>);

fn ranges(h: &mut Harness) -> Ranges {
    let pane = h.setup().panes[0].clone();
    (pane.x_range, pane.y_range, pane.y2_range)
}

/// Presses (`down`) or releases the Space bar.
fn space(h: &mut Harness, down: bool) {
    let keystroke = gpui_kit::Keystroke::parse("space").expect("a keystroke");
    if down {
        h.cx.simulate_event(gpui_kit::KeyDownEvent {
            keystroke,
            is_held: false,
            prefer_character_input: false,
        });
    } else {
        h.cx.simulate_event(gpui_kit::KeyUpEvent { keystroke });
    }
    h.cx.run_until_parked();
}

/// D14 review: Escape during a pan (left-drag, middle-drag and Space+drag)
/// returns to the view the pan started from, like it cancels a box zoom;
/// the rest of the gesture then changes nothing.
#[gpui_kit::test]
fn escape_during_a_pan_restores_the_view(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    h.add("I(R1)");
    let center = h.at(0, 0.5, 0.5);
    h.scroll(center, 4.0, Modifiers::default());
    h.scroll(center, 4.0, Modifiers::shift());
    let before = ranges(&mut h);
    assert!(before.0.is_some() && before.1.is_some() && before.2.is_some());

    for (button, space_held) in [
        (MouseButton::Left, false),
        (MouseButton::Middle, false),
        (MouseButton::Left, true),
    ] {
        let label = format!("{button:?}, Space {space_held}");
        if space_held {
            space(&mut h, true);
            assert!(h.viewer.read_with(&h.cx, |v, _| v.view.space_held));
        }
        let (from, to) = (h.at(0, 0.5, 0.5), h.at(0, 0.62, 0.65));
        press_and_move(&mut h, button, from, to);
        let panned = ranges(&mut h);
        assert_ne!(panned, before, "{label}: the pan moves the view");
        let changes = h.changes.get();
        escape_then_release(&mut h, button, to);
        assert_eq!(ranges(&mut h), before, "{label}: Escape restores it");
        assert!(h.changes.get() > changes, "{label}: the restore is saved");
        assert_eq!(h.viewer.read_with(&h.cx, |v, _| v.view.drag), None);
        if space_held {
            space(&mut h, false);
        }
    }

    // A fitted y axis stays automatic: only x moves, and comes back.
    h.viewer.update(&mut h.cx, |v, cx| {
        v.edit_setup(cx, |s| crate::model::setup::set_y_range(s, 0, None));
    });
    h.cx.run_until_parked();
    let before = ranges(&mut h);
    assert_eq!(before.1, None);
    let (from, to) = (h.at(0, 0.5, 0.5), h.at(0, 0.7, 0.2));
    press_and_move(&mut h, MouseButton::Left, from, to);
    assert_ne!(ranges(&mut h).0, before.0);
    escape_then_release(&mut h, MouseButton::Left, to);
    assert_eq!(ranges(&mut h), before);

    // Without Escape a pan stays where it was released, also a middle one.
    let (from, to) = (h.at(0, 0.5, 0.5), h.at(0, 0.6, 0.5));
    h.drag(MouseButton::Middle, from, to);
    assert_ne!(ranges(&mut h), before);
}

/// D14 review: Escape during a cursor drag puts the cursor back.
#[gpui_kit::test]
fn escape_during_a_cursor_drag_restores_the_cursor(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    h.click("toggle-cursors");
    let before = h.cursors();

    // The line of cursor 2, grabbed low on the plot.
    let from = point(h.line_x(CursorId::Second), h.at(0, 0.5, 0.95).y);
    let to = h.at(0, 0.4, 0.95);
    press_and_move(&mut h, MouseButton::Left, from, to);
    assert_ne!(h.cursors(), before, "the drag moves the cursor");
    escape_then_release(&mut h, MouseButton::Left, to);
    assert_eq!(h.cursors(), before, "Escape puts it back");
    assert_eq!(h.setup().panes[0].x_range, None, "the view is untouched");

    // A drag that is not cancelled keeps its result.
    let from = point(h.line_x(CursorId::Second), h.at(0, 0.5, 0.95).y);
    let to = h.at(0, 0.4, 0.95);
    h.drag(MouseButton::Left, from, to);
    assert_ne!(h.cursors(), before);
}

/// D14 review: pressing a chip's hide or remove button and moving away with
/// the button held does not drag the chip; nothing changes.
#[gpui_kit::test]
fn a_press_on_a_chip_button_never_drags_the_chip(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    h.add("V(in)");
    h.click("add-pane");
    h.add("I(R1)");
    let before = h.setup();
    let below = h.at(1, 0.5, 0.5);
    for button in ["trace-visibility-0-0", "trace-remove-0-0"] {
        // Focus elsewhere first, so the press must focus the viewer.
        h.click("trace-input");
        let from = h.bounds(button).center();
        press_and_move(&mut h, MouseButton::Left, from, below);
        let dragging = h.cx.update(|_, app| app.has_active_drag());
        assert!(!dragging, "{button}: no chip drag starts");
        assert!(!h.rendered("pane-drop-1") && !h.rendered("pane-drop-refused-1"));
        let viewer = h.viewer.clone();
        let focused =
            h.cx.update(|window, app| viewer.read(app).focus.is_focused(window));
        assert!(focused, "{button}: the press focuses the viewer");
        h.cx.simulate_mouse_up(below, MouseButton::Left, Modifiers::default());
        h.cx.run_until_parked();
        assert_eq!(h.setup(), before, "{button}: nothing moved or changed");
    }

    // The buttons still work, and the chip itself still drags.
    h.click("trace-visibility-0-0");
    assert!(!h.setup().panes[0].traces[0].visible);
    h.click("trace-visibility-0-0");
    h.drag_chip("trace-chip-0-1", below, |_| {});
    assert_eq!(expressions(&h.setup(), 1), ["I(R1)", "V(in)"]);
    h.click("trace-remove-0-0");
    assert!(h.setup().panes[0].traces.is_empty());
}

/// The flags at the top of the cursor lines as last painted.
fn top_flags(h: &mut Harness) -> Vec<super::canvas::Flag> {
    h.plot_geometry(0).flags.iter().flatten().copied().collect()
}

fn place(h: &mut Harness, first: f64, second: f64) {
    h.viewer.update(&mut h.cx, |v, cx| {
        v.view.cursors.set(CursorId::First, first);
        v.view.cursors.set(CursorId::Second, second);
        cx.notify();
    });
    h.cx.run_until_parked();
}

/// The top flags of the cursors never overlap, however close the lines
/// are, and each stays centred on its own line; far-apart lines keep both
/// on top.
#[gpui_kit::test]
fn close_cursor_flags_never_overlap(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    h.click("toggle-cursors");
    assert_flags_apart(&mut h);
}

/// Review: at the smallest and largest UI scale (75 % and 200 % of the
/// 16 px rem) the flags grow with the labels and still never overlap.
#[gpui_kit::test]
fn cursor_flags_follow_the_ui_scale(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    h.click("toggle-cursors");
    let mut sides = Vec::new();
    for rem in [12.0, 32.0] {
        // As the shell's UI scale does: the kit's root takes the rem from
        // the theme's font size every frame.
        h.cx.update(|_, app| {
            gpui_kit::component::Theme::update(app, |theme| theme.font_size = px(rem));
        });
        h.viewer.update(&mut h.cx, |_, cx| cx.notify());
        h.cx.run_until_parked();
        assert_flags_apart(&mut h);
        let side = top_flags(&mut h)[0].bounds.size;
        assert_eq!(side.width, side.height, "{rem}: square");
        sides.push(side.width.as_f32());
    }
    // A flag is 1.4 labels wide, and a label three quarters of the rem.
    assert_eq!(sides, [13.0, 34.0]);
}

/// The flag invariants of [`close_cursor_flags_never_overlap`] for cursor
/// pairs from the same sample to far apart, at the current UI scale.
fn assert_flags_apart(h: &mut Harness) {
    let axis = transient().axis.values;
    let top = h.plot(0).top();
    for (a, b) in [(40, 40), (40, 41), (41, 40), (0, 1), (100, 99), (20, 80)] {
        place(h, axis[a], axis[b]);
        let geometry = h.plot_geometry(0);
        let flags = top_flags(h);
        assert_eq!(flags.len(), 2, "{a}, {b}: {flags:?}");
        let (one, two) = (flags[0], flags[1]);
        assert_eq!((one.id, two.id), (CursorId::First, CursorId::Second));
        assert!(
            !one.bounds.intersects(&two.bounds),
            "{a}, {b}: {one:?} and {two:?}"
        );
        for (flag, x) in [(one, axis[a]), (two, axis[b])] {
            let centre = flag.bounds.center().x.as_f32();
            assert!(
                (centre - geometry.x.px(x)).abs() < 0.01,
                "{a}, {b}: centred"
            );
        }
        assert_eq!(one.bounds.top(), top, "the flag of cursor 1 stays on top");
        let far = a.abs_diff(b) > 10;
        assert_eq!(two.bounds.top() == top, far, "{a}, {b}: {two:?}");
    }
}

/// Exports show the plot without the cursors: neither the docs (Q137,
/// Q07) nor D14 ask for them in images, so a shown cursor changes nothing.
#[gpui_kit::test]
fn exports_leave_the_cursors_out(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    let without = h.viewer.read_with(&h.cx, |v, _| v.svg(800, 450)).unwrap();
    h.click("toggle-cursors");
    assert!(h.cursors().enabled);
    let with = h.viewer.read_with(&h.cx, |v, _| v.svg(800, 450)).unwrap();
    assert_eq!(with, without);
}

/// Review: a second mouse button pressed or released during a drag changes
/// nothing. The drag the first button began goes on, ends only with that
/// button's release, and Escape still returns to where it began.
#[gpui_kit::test]
fn a_second_button_mid_drag_is_ignored(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    let center = h.at(0, 0.5, 0.5);
    h.scroll(center, 2.0, Modifiers::default());
    h.click("toggle-cursors");
    let none = Modifiers::default();
    let view = ranges(&mut h);
    assert!(view.0.is_some(), "zoomed in, so a pan would show");
    let before = h.cursors();

    // A cursor drag: Middle pressed and released on the way neither pans
    // nor ends it.
    let from = point(h.line_x(CursorId::Second), h.at(0, 0.5, 0.95).y);
    let mid = point(from.x - px(60.), from.y);
    press_and_move(&mut h, MouseButton::Left, from, mid);
    let moved = h.cursors();
    assert_ne!(moved, before, "the drag moves the cursor");
    h.cx.simulate_mouse_down(mid, MouseButton::Middle, none);
    let further = point(mid.x - px(60.), mid.y - px(10.));
    h.cx.simulate_mouse_move(further, MouseButton::Left, none);
    h.cx.simulate_mouse_up(further, MouseButton::Middle, none);
    let last = point(further.x - px(40.), further.y);
    h.cx.simulate_mouse_move(last, MouseButton::Left, none);
    h.cx.run_until_parked();
    assert_eq!(ranges(&mut h), view, "no pan");
    let dragged = h.cursors();
    assert!(
        dragged.second < moved.second,
        "the cursor drag goes on: {dragged:?} after {moved:?}"
    );
    escape_then_release(&mut h, MouseButton::Left, last);
    assert_eq!(h.cursors(), before, "Escape returns the cursor");
    assert_eq!(ranges(&mut h), view);

    // A left pan: Middle pressed on the way does not restart it, so Escape
    // returns to the view before the left press.
    let (from, mid) = (h.at(0, 0.4, 0.5), h.at(0, 0.5, 0.5));
    press_and_move(&mut h, MouseButton::Left, from, mid);
    assert_ne!(ranges(&mut h), view, "the pan moves the view");
    h.cx.simulate_mouse_down(mid, MouseButton::Middle, none);
    let further = h.at(0, 0.6, 0.5);
    h.cx.simulate_mouse_move(further, MouseButton::Left, none);
    h.cx.run_until_parked();
    escape_then_release(&mut h, MouseButton::Left, further);
    h.cx.simulate_mouse_up(further, MouseButton::Middle, none);
    h.cx.run_until_parked();
    assert_eq!(ranges(&mut h), view, "Escape returns to the first view");
    assert_eq!(h.cursors(), before);

    // A middle pan: a left click on the way places no cursor and does not
    // end the pan, which stays where Middle is released.
    let (from, mid) = (h.at(0, 0.45, 0.5), h.at(0, 0.5, 0.5));
    press_and_move(&mut h, MouseButton::Middle, from, mid);
    let panned = ranges(&mut h);
    assert_ne!(panned, view, "the pan moves the view");
    h.cx.simulate_mouse_down(mid, MouseButton::Left, none);
    h.cx.simulate_mouse_up(mid, MouseButton::Left, none);
    let further = h.at(0, 0.55, 0.5);
    h.cx.simulate_mouse_move(further, MouseButton::Middle, none);
    h.cx.run_until_parked();
    assert_eq!(h.cursors(), before, "the left click places no cursor");
    assert_ne!(ranges(&mut h), panned, "the pan goes on");
    h.cx.simulate_mouse_up(further, MouseButton::Middle, none);
    h.cx.run_until_parked();
    assert_eq!(h.viewer.read_with(&h.cx, |v, _| v.view.drag), None);
    let kept = ranges(&mut h);
    assert_ne!(kept, view, "the released pan is kept");
}

/// Review, pinning today's rule: with both cursors on one sample a press on
/// the line grabs cursor 1 (its flag is the top one); once it is dragged
/// away, cursor 2's line is free to grab. The flags themselves are not
/// grab targets.
#[gpui_kit::test]
fn coincident_cursors_separate_by_dragging_cursor_one(cx: &mut TestAppContext) {
    let mut h = open(cx);
    let axis = transient().axis.values;
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    h.click("toggle-cursors");
    place(&mut h, axis[40], axis[40]);
    let line = h.plot_geometry(0).x.px(axis[40]);
    let low = h.at(0, 0.5, 0.95).y;
    h.drag(
        MouseButton::Left,
        point(px(line), low),
        point(px(line) - px(100.), low),
    );
    let cursors = h.cursors();
    assert!(
        cursors.first < Some(axis[40]),
        "cursor 1 moved: {cursors:?}"
    );
    assert_eq!(cursors.second, Some(axis[40]), "cursor 2 stayed");
    h.drag(
        MouseButton::Left,
        point(px(line), low),
        point(px(line) + px(100.), low),
    );
    let cursors = h.cursors();
    assert!(cursors.second > Some(axis[40]), "now cursor 2: {cursors:?}");
}

/// The viewer beside another focusable view, as in the shell, where the
/// schematic editor sits next to the plot.
struct Beside {
    viewer: Entity<WaveformViewer>,
    other: gpui_kit::FocusHandle,
}

impl Render for Beside {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .child(div().track_focus(&self.other).w(px(100.)).h_full())
            .child(div().flex_1().h_full().child(self.viewer.clone()))
    }
}

/// Review: Space pressed in the viewer and released after focus moved to
/// another view leaves no stale Space+drag behind; the next press on a
/// cursor line drags the cursor instead of panning.
#[gpui_kit::test]
fn space_released_elsewhere_does_not_stick(cx: &mut TestAppContext) {
    let mut other = None;
    let (window, viewer) = cx.update(|cx| {
        gpui_kit::init(cx);
        let mut viewer = None;
        let (window, _) = gpui_kit::open_window(options(), cx, |_, cx| {
            let inner = cx.new(WaveformViewer::new);
            viewer = Some(inner.clone());
            let handle = cx.focus_handle();
            other = Some(handle.clone());
            cx.new(|_| Beside {
                viewer: inner,
                other: handle,
            })
        })
        .expect("open a test window");
        (window, viewer.expect("the viewer was built"))
    });
    let other = other.expect("the other view was built");
    let mut h = finish(cx, window, viewer);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    h.click("toggle-cursors");
    // Space held while the viewer has focus.
    h.viewer
        .update_in(&mut h.cx, |v, window, cx| v.focus.focus(window, cx));
    h.cx.run_until_parked();
    space(&mut h, true);
    assert!(h.viewer.read_with(&h.cx, |v, _| v.view.space_held));
    // Focus moves to the other view and Space is released there.
    h.cx.update(|window, cx| other.focus(window, cx));
    h.cx.run_until_parked();
    space(&mut h, false);

    let before = h.cursors();
    let from = point(h.line_x(CursorId::Second), h.at(0, 0.5, 0.95).y);
    h.drag(MouseButton::Left, from, point(from.x - px(80.), from.y));
    assert_ne!(h.cursors(), before, "the press drags the cursor line");
    assert!(!h.viewer.read_with(&h.cx, |v, _| v.view.space_held));
}

// D23: the dividers between panes.

/// The bounds of pane `pane`.
fn pane_bounds(h: &mut Harness, pane: usize) -> Bounds<Pixels> {
    bounds_of(h, format!("pane-{pane}"))
}

/// Every pane's height, top to bottom.
fn pane_heights(h: &mut Harness) -> Vec<f32> {
    let count = h.setup().panes.len();
    (0..count)
        .map(|i| pane_bounds(h, i).size.height.as_f32())
        .collect()
}

/// The band of the divider under pane `upper`.
fn divider_bounds(h: &mut Harness, upper: usize) -> Bounds<Pixels> {
    bounds_of(h, format!("pane-divider-{upper}"))
}

/// Every pane's stored height.
fn stored_heights(h: &mut Harness) -> Vec<Option<f32>> {
    h.setup().panes.iter().map(|p| p.height).collect()
}

/// Panes with `heights`, each plotting `V(out)`.
fn panes_of(heights: &[Option<f32>]) -> PlotSetup {
    PlotSetup {
        panes: heights
            .iter()
            .map(|&height| PlotPane {
                traces: vec![Trace::new("V(out)")],
                height,
                ..PlotPane::default()
            })
            .collect(),
        active_pane: 0,
    }
}

fn rem_px(h: &mut Harness) -> f32 {
    h.cx.update(|window, _| window.rem_size()).as_f32()
}

/// Whether `a` and `b` differ by at most `tolerance` pixels.
fn within(a: f32, b: f32, tolerance: f32) -> bool {
    (a - b).abs() <= tolerance
}

/// Presses on the middle of the divider under pane `upper`, moves the
/// pointer `dy` pixels down in `steps` steps (laid out again after each),
/// runs `during` and releases there.
fn drag_divider(
    h: &mut Harness,
    upper: usize,
    dy: f32,
    steps: u32,
    during: impl FnOnce(&mut Harness),
) {
    let from = divider_bounds(h, upper).center();
    let none = Modifiers::default();
    h.cx.simulate_mouse_move(from, None, none);
    h.cx.simulate_mouse_down(from, MouseButton::Left, none);
    h.cx.run_until_parked();
    for step in 1..=steps {
        let to = point(from.x, from.y + px(dy * step as f32 / steps as f32));
        h.cx.simulate_mouse_move(to, MouseButton::Left, none);
        h.cx.run_until_parked();
    }
    during(h);
    let to = point(from.x, from.y + px(dy));
    h.cx.simulate_mouse_up(to, MouseButton::Left, none);
    h.cx.run_until_parked();
}

/// A press and release with `click_count` at `position`, as the second
/// half of a double click arrives.
fn click_with_count(h: &mut Harness, position: Point<Pixels>, click_count: usize) {
    h.cx.simulate_event(MouseDownEvent {
        position,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
        click_count,
        first_mouse: false,
    });
    h.cx.simulate_event(MouseUpEvent {
        position,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
        click_count,
    });
    h.cx.run_until_parked();
}

/// D23: dragging the divider between two panes moves height from one to
/// the other, in small steps or one jump, also next to a right axis (D9).
/// The other panes, the stack and the plus stay where they were, nothing
/// pans, zooms or changes the active pane, and the setup changes once, on
/// release.
#[gpui_kit::test]
fn dragging_a_divider_moves_height_between_its_panes(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    h.add("I(R1)");
    h.click("add-pane");
    h.add("V(in)");
    h.click("add-pane");
    h.add("V(out)");
    assert!(h.plot_geometry(0).y2.is_some(), "pane 0 has a right axis");
    let before = pane_heights(&mut h);
    assert!(
        before.iter().all(|&x| within(x, before[0], 1.0)),
        "{before:?}"
    );
    let (stack, plus) = (h.bounds("waveform-panes"), h.bounds("add-pane"));
    let plot = h.plot(0);
    let setup = h.setup();
    let changes = h.changes.get();
    drag_divider(&mut h, 0, 60.0, 6, |h| {
        // The panes follow the pointer; the setup changes on release.
        let live = pane_heights(h);
        assert!(within(live[0], before[0] + 60.0, 1.0), "{live:?}");
        assert_eq!(h.changes.get(), changes, "nothing reported mid-drag");
        assert_eq!(h.setup(), setup, "nothing stored mid-drag");
        for pane in 0..3 {
            assert_eq!(h.pointer(pane), CursorStyle::ResizeRow, "{pane}");
        }
    });
    assert_eq!(h.changes.get(), changes + 1, "one SetupChanged per drag");
    let after = pane_heights(&mut h);
    assert!(within(after[0], before[0] + 60.0, 1.0), "{after:?}");
    assert!(within(after[1], before[1] - 60.0, 1.0), "{after:?}");
    assert!(within(after[2], before[2], 1.0), "{after:?}");
    let grown = h.plot(0).size.height.as_f32();
    assert!(
        within(grown, plot.size.height.as_f32() + 60.0, 1.0),
        "{grown}"
    );
    let stored = stored_heights(&mut h);
    assert!(stored[0].is_some_and(|w| w > 1.0), "{stored:?}");
    assert!(stored[1].is_some_and(|w| w < 1.0), "{stored:?}");
    assert_eq!(stored[2], None, "the third pane is untouched");
    let ranges = |s: &PlotSetup| -> Vec<_> {
        s.panes
            .iter()
            .map(|p| (p.x_range, p.y_range, p.y2_range, p.traces.clone()))
            .collect()
    };
    let now = h.setup();
    assert_eq!(ranges(&now), ranges(&setup), "no pan, zoom or trace change");
    assert_eq!(now.active_pane, setup.active_pane);
    assert_eq!(h.bounds("waveform-panes"), stack);
    assert_eq!(h.bounds("add-pane"), plus);
    // The divider sits on the new boundary.
    let line = pane_bounds(&mut h, 1).top().as_f32();
    let middle = divider_bounds(&mut h, 0).center().y.as_f32();
    assert!(within(middle, line, 1.0), "{middle} on {line}");

    // The other divider, upwards in one jump.
    drag_divider(&mut h, 1, -40.0, 1, |_| {});
    assert_eq!(h.changes.get(), changes + 2);
    let last = pane_heights(&mut h);
    assert!(within(last[0], after[0], 1.0), "{last:?}");
    assert!(within(last[1], after[1] - 40.0, 1.0), "{last:?}");
    assert!(within(last[2], after[2] + 40.0, 1.0), "{last:?}");

    // In 1 px steps the pointer stays on the band, which passes the moves
    // on.
    drag_divider(&mut h, 1, 12.0, 12, |_| {});
    assert_eq!(h.changes.get(), changes + 3);
    let fine = pane_heights(&mut h);
    assert!(within(fine[1], last[1] + 12.0, 1.0), "{fine:?}");
    assert!(within(fine[2], last[2] - 12.0, 1.0), "{fine:?}");
    let last = fine;

    // Closing a pane leaves the others in proportion; a new pane gets an
    // average share.
    let close = bounds_of(&mut h, "pane-close-2".into());
    h.click_at(close.center());
    let two = pane_heights(&mut h);
    let ratio = |v: &[f32]| v[0] / v[1];
    assert!(
        (ratio(&two) - ratio(&last)).abs() < 0.02,
        "{two:?} vs {last:?}"
    );
    h.click("add-pane");
    let three = pane_heights(&mut h);
    let total: f32 = three.iter().sum();
    assert!(within(three[2], total / 3.0, 1.5), "{three:?}");
    assert!((ratio(&three) - ratio(&last)).abs() < 0.02, "{three:?}");

    // What the drags stored is a valid project setup: it saves, reads back
    // the same and saves again byte for byte.
    let mut project = xsim_project::Project::new("Resized");
    project.circuits[0].plot = h.setup();
    let text = project.to_json();
    let loaded = xsim_project::Project::from_json(&text).expect("a valid project");
    assert_eq!(loaded.project.circuits[0].plot, h.setup());
    assert_eq!(loaded.project.to_json(), text);
    assert!(text.contains("\"height\": "), "{text}");
}

/// D23 at 75 %, 100 % and 150 % UI scale: a divider stops where the pane
/// it shrinks reaches the minimum height of 7.5 rem, in both directions,
/// and the pair keeps its total height.
#[gpui_kit::test]
fn a_divider_stops_at_the_minimum_pane_height(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), panes_of(&[None, None]));
    for rem in [12.0, 16.0, 24.0] {
        set_rem(&mut h, rem);
        let min = rem_px(&mut h) * super::pane::MIN_PANE_HEIGHT;
        let total: f32 = pane_heights(&mut h).iter().sum();
        // Down to the window's bottom edge, over the plus.
        let from = divider_bounds(&mut h, 0).center().y.as_f32();
        drag_divider(&mut h, 0, WINDOW_HEIGHT - 2.0 - from, 4, |_| {});
        let low = pane_heights(&mut h);
        assert!(within(low[1], min, 1.0), "{rem}: {low:?}, min {min}");
        assert!(within(low[0] + low[1], total, 1.0), "{rem}: {low:?}");
        // Up to the window's top edge, over the toolbar.
        let from = divider_bounds(&mut h, 0).center().y.as_f32();
        drag_divider(&mut h, 0, 2.0 - from, 4, |_| {});
        let high = pane_heights(&mut h);
        assert!(within(high[0], min, 1.0), "{rem}: {high:?}");
        assert!(within(high[0] + high[1], total, 1.0), "{rem}: {high:?}");
        // The small pane still has its plot and every x label.
        let (geometry, scene) = screen_scene(&mut h, 0);
        assert!(geometry.plot.size.height > px(2.0 * rem), "{rem}");
        for label in &scene.labels {
            let bottom = geometry.frame.top() + px(label.y + label.size * 0.6);
            assert!(
                bottom <= geometry.frame.bottom() + px(SNAP),
                "{rem}: {label:?}"
            );
        }
    }
}

/// D23: with more panes than fit, every pane is at its minimum and the
/// stack scrolls; a divider then has no room to move and changes nothing.
#[gpui_kit::test]
fn a_scrolling_stack_has_no_room_to_resize(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), panes_of(&[None; 9]));
    let before = pane_heights(&mut h);
    let min = rem_px(&mut h) * super::pane::MIN_PANE_HEIGHT;
    assert!(before.iter().all(|&x| within(x, min, 1.0)), "{before:?}");
    let (setup, changes) = (h.setup(), h.changes.get());
    drag_divider(&mut h, 0, 50.0, 2, |_| {});
    assert_eq!(pane_heights(&mut h), before);
    assert_eq!((h.setup(), h.changes.get()), (setup, changes));
}

/// D23: a double click on a divider makes its two panes equally high,
/// keeping their total, and leaves the other panes alone; the first click
/// of the pair changes nothing. Already equal, a double click reports
/// nothing.
#[gpui_kit::test]
fn double_clicking_a_divider_makes_its_panes_equal(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), panes_of(&[Some(2.0), None, None]));
    let before = pane_heights(&mut h);
    assert!(within(before[0], 2.0 * before[1], 2.0), "{before:?}");
    assert!(within(before[2], before[1], 1.0), "{before:?}");
    let changes = h.changes.get();
    let double_click = |h: &mut Harness, upper: usize| {
        let position = divider_bounds(h, upper).center();
        h.cx.simulate_mouse_move(position, None, Modifiers::default());
        click_with_count(h, position, 1);
        let single = h.changes.get();
        click_with_count(h, position, 2);
        single
    };
    let single = double_click(&mut h, 0);
    assert_eq!(single, changes, "the first click changes nothing");
    assert_eq!(h.changes.get(), changes + 1);
    let after = pane_heights(&mut h);
    assert!(within(after[0], after[1], 1.0), "{after:?}");
    assert!(within(after[0] + after[1], before[0] + before[1], 1.0));
    assert!(within(after[2], before[2], 1.0), "{after:?}");
    assert_eq!(stored_heights(&mut h), [Some(1.5), Some(1.5), None]);
    double_click(&mut h, 0);
    assert_eq!(h.changes.get(), changes + 1, "already equal");
    double_click(&mut h, 1);
    assert_eq!(h.changes.get(), changes + 2);
    assert_eq!(stored_heights(&mut h), [Some(1.5), Some(1.25), Some(1.25)]);
}

/// D23: Escape during a divider drag puts the heights back and reports
/// nothing; a press without movement changes nothing either, not even the
/// active pane. A release the viewer missed (outside the window) keeps the
/// heights the drag reached, as a release does.
#[gpui_kit::test]
fn escape_or_a_click_on_a_divider_changes_nothing(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), panes_of(&[None, None]));
    let before = pane_heights(&mut h);
    let (setup, changes) = (h.setup(), h.changes.get());
    drag_divider(&mut h, 0, 80.0, 4, |h| {
        assert!(within(pane_heights(h)[0], before[0] + 80.0, 1.0));
        h.cx.simulate_keystrokes("escape");
        h.cx.run_until_parked();
        assert_eq!(pane_heights(h), before, "put back at once");
        let still = point(px(300.), px(500.));
        h.cx.simulate_mouse_move(still, MouseButton::Left, Modifiers::default());
        h.cx.run_until_parked();
        assert_eq!(pane_heights(h), before, "the drag is over");
    });
    assert_eq!(pane_heights(&mut h), before);
    assert_eq!((h.setup(), h.changes.get()), (setup.clone(), changes));

    let band = divider_bounds(&mut h, 0);
    h.click_at(band.center());
    assert_eq!((h.setup(), h.changes.get()), (setup, changes));
    assert_eq!(h.setup().active_pane, 0, "the lower pane did not activate");

    // Moved, and then the release is missed: the next move without a
    // button ends the drag where it was.
    let from = band.center();
    let none = Modifiers::default();
    h.cx.simulate_mouse_move(from, None, none);
    h.cx.simulate_mouse_down(from, MouseButton::Left, none);
    let to = point(from.x, from.y - px(50.));
    h.cx.simulate_mouse_move(to, MouseButton::Left, none);
    h.cx.run_until_parked();
    h.cx.simulate_mouse_move(to, None, none);
    h.cx.run_until_parked();
    assert_eq!(h.changes.get(), changes + 1);
    let after = pane_heights(&mut h);
    assert!(within(after[0], before[0] - 50.0, 1.0), "{after:?}");
    assert!(stored_heights(&mut h).iter().all(Option::is_some));
}

/// D23 with the cursors, their readout and the magnifier on: a divider
/// drag neither pans nor zooms nor moves a cursor, shows no hover line or
/// zoom box, and leaves the readout and the plus in place.
#[gpui_kit::test]
fn a_divider_drag_never_pans_zooms_or_moves_a_cursor(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), PlotSetup::default());
    h.add("V(out)");
    h.add("I(R1)");
    h.click("add-pane");
    h.add("V(in)");
    h.click("toggle-cursors");
    // Zoomed in on every axis, so a pan would move them.
    let center = h.at(0, 0.5, 0.5);
    h.scroll(center, 3.0, Modifiers::default());
    h.scroll(center, 3.0, Modifiers::shift());
    let zoomed = h.setup();
    assert!(zoomed.panes[0].x_range.is_some() && zoomed.panes[0].y_range.is_some());
    let cursors = h.cursors();
    let readout = h.bounds("cursor-readout");
    for (magnifier, dy) in [(false, 40.0), (true, -70.0)] {
        if magnifier {
            h.click("toggle-magnifier");
        }
        drag_divider(&mut h, 0, dy, 5, |h| {
            for pane in 0..2 {
                let overlay = h.overlay(pane);
                assert_eq!(overlay.zoom_box, None, "{magnifier}");
                assert_eq!(overlay.crosshair, None, "{magnifier}");
            }
            let drag = h.viewer.read_with(&h.cx, |v, _| v.view.drag);
            assert!(
                matches!(drag, Some(Drag::Divider { upper: 0, .. })),
                "{drag:?}"
            );
        });
        let setup = h.setup();
        for (pane, (now, then)) in setup.panes.iter().zip(&zoomed.panes).enumerate() {
            assert_eq!(
                (now.x_range, now.y_range, now.y2_range),
                (then.x_range, then.y_range, then.y2_range),
                "{magnifier}: pane {pane}"
            );
        }
        assert_eq!(h.cursors(), cursors, "{magnifier}");
        assert_eq!(h.bounds("cursor-readout"), readout, "{magnifier}");
        assert!(near(
            h.bounds("waveform-footer").bottom(),
            window_bottom(&mut h)
        ));
        let on = h.viewer.read_with(&h.cx, |v, _| v.view.magnifier);
        assert_eq!(on, magnifier);
    }
}

/// The divider bands hide the pointer from the viewer behind them, so they
/// pass on the moves of other drags: a pan and a cursor drag that cross a
/// divider keep following the pointer.
#[gpui_kit::test]
fn a_pan_or_cursor_drag_across_a_divider_keeps_going(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), panes_of(&[None, None]));
    let center = h.at(0, 0.5, 0.5);
    h.scroll(center, 4.0, Modifiers::default());
    let zoomed = h.setup().panes[0].x_range;
    let none = Modifiers::default();
    let band = divider_bounds(&mut h, 0);
    let from = h.at(0, 0.5, 0.5);
    h.cx.simulate_mouse_move(from, None, none);
    h.cx.simulate_mouse_down(from, MouseButton::Left, none);
    let started = point(from.x - px(10.), from.y);
    h.cx.simulate_mouse_move(started, MouseButton::Left, none);
    h.cx.run_until_parked();
    let began = h.setup().panes[0].x_range;
    assert_ne!(began, zoomed, "the pan began");
    let over = point(from.x - px(60.), band.center().y);
    h.cx.simulate_mouse_move(over, MouseButton::Left, none);
    h.cx.run_until_parked();
    let on_band = h.setup().panes[0].x_range;
    assert_ne!(on_band, began, "the pan follows over the divider");
    let beyond = point(from.x - px(100.), h.at(1, 0.5, 0.5).y);
    h.cx.simulate_mouse_move(beyond, MouseButton::Left, none);
    h.cx.simulate_mouse_up(beyond, MouseButton::Left, none);
    h.cx.run_until_parked();
    assert_ne!(h.setup().panes[0].x_range, on_band);
    assert!(
        stored_heights(&mut h).iter().all(Option::is_none),
        "no resize"
    );

    // A cursor line dragged down across the divider.
    h.click("toggle-cursors");
    let x = h.line_x(CursorId::First);
    let grab = point(x, h.at(0, 0.5, 0.8).y);
    let before = h.cursors().first;
    h.cx.simulate_mouse_move(grab, None, none);
    h.cx.simulate_mouse_down(grab, MouseButton::Left, none);
    let band = divider_bounds(&mut h, 0);
    let over = point(x + px(80.), band.center().y);
    h.cx.simulate_mouse_move(over, MouseButton::Left, none);
    h.cx.run_until_parked();
    assert_ne!(h.cursors().first, before, "the cursor follows over it");
    h.cx.simulate_mouse_up(over, MouseButton::Left, none);
    h.cx.run_until_parked();
    assert!(
        stored_heights(&mut h).iter().all(Option::is_none),
        "no resize"
    );
}

/// D23 at 75 %, 100 % and 200 % UI scale: a divider is a band 0.5 rem high
/// across the pane, centred on the line between its panes, one between
/// each pair. A grip shows on the line under the pointer and grows while
/// the divider is dragged.
#[gpui_kit::test]
fn the_divider_band_straddles_the_line_with_a_grip(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), panes_of(&[None, None, None]));
    assert!(h.rendered("pane-divider-0") && h.rendered("pane-divider-1"));
    assert!(!h.rendered("pane-divider-2"), "none under the last pane");
    for rem in [12.0, 16.0, 32.0] {
        set_rem(&mut h, rem);
        let rem_size = rem_px(&mut h);
        for upper in 0..2 {
            let band = divider_bounds(&mut h, upper);
            let below = pane_bounds(&mut h, upper + 1);
            let case = format!("{rem} px rem, divider {upper}");
            let hit = rem_size * super::divider::HIT_HEIGHT;
            assert!(
                within(band.size.height.as_f32(), hit, 1.0),
                "{case}: {band:?}"
            );
            let middle = band.center().y.as_f32();
            assert!(within(middle, below.top().as_f32(), 1.0), "{case}");
            assert!(near(band.left(), below.left()), "{case}");
            assert!(near(band.right(), below.right()), "{case}");
        }
        // Away from it, no grip; over it, a short one.
        let away = h.at(0, 0.5, 0.5);
        h.cx.simulate_mouse_move(away, None, Modifiers::default());
        h.cx.run_until_parked();
        assert!(!h.rendered("pane-divider-grip-0"), "{rem}");
        let band = divider_bounds(&mut h, 0);
        h.cx.simulate_mouse_move(band.center(), None, Modifiers::default());
        h.cx.run_until_parked();
        assert!(h.rendered("pane-divider-grip-0"), "{rem}");
        assert!(!h.rendered("pane-divider-grip-1"), "{rem}");
        let grip = h.bounds("pane-divider-grip-0");
        assert!(
            within(grip.size.width.as_f32(), rem_size * 1.25, 1.0),
            "{rem}"
        );
        assert!(within(
            grip.center().y.as_f32(),
            band.center().y.as_f32(),
            1.0
        ));
        assert!(within(
            grip.center().x.as_f32(),
            band.center().x.as_f32(),
            1.0
        ));
        drag_divider(&mut h, 0, 10.0, 1, |h| {
            let grip = h.bounds("pane-divider-grip-0");
            let width = grip.size.width.as_f32();
            assert!(within(width, rem_size * 2.75, 1.0), "{rem}: {width}");
        });
        h.cx.simulate_mouse_move(away, None, Modifiers::default());
        h.cx.run_until_parked();
        assert!(!h.rendered("pane-divider-grip-0"), "{rem}");
    }
}

/// D23: stored heights are relative, so the panes keep their proportion as
/// the dock gets taller or shorter, and weights summing below one still
/// fill the stack. Showing a setup never rewrites its heights.
#[gpui_kit::test]
fn pane_heights_keep_their_proportion_as_the_dock_resizes(cx: &mut TestAppContext) {
    let (mut h, dock) = open_docked(cx, 560.0);
    h.show(transient(), panes_of(&[Some(2.0), None]));
    for height in [560.0, 700.0, 480.0] {
        dock.update(&mut h.cx, |dock, cx| {
            dock.height = height;
            cx.notify();
        });
        h.cx.run_until_parked();
        let heights = pane_heights(&mut h);
        let case = format!("{height} px dock: {heights:?}");
        assert!(within(heights[0], 2.0 * heights[1], 2.0), "{case}");
        let stack = h.bounds("waveform-panes").size.height.as_f32();
        assert!(within(heights.iter().sum(), stack, 1.0), "{case}");
    }
    assert_eq!(stored_heights(&mut h), [Some(2.0), None]);
    h.show(transient(), panes_of(&[Some(0.2), Some(0.3)]));
    let heights = pane_heights(&mut h);
    let stack = h.bounds("waveform-panes").size.height.as_f32();
    assert!(within(heights.iter().sum(), stack, 1.0), "{heights:?}");
    assert!(within(heights[0] * 1.5, heights[1], 2.0), "{heights:?}");
    assert_eq!(stored_heights(&mut h), [Some(0.2), Some(0.3)]);
}

/// D23: after the dock got shorter, a pane is held at its minimum height
/// (taller than its weight's share). Dragging its divider still moves the
/// divider with the pointer and leaves the third pane where it was.
#[gpui_kit::test]
fn a_pane_held_at_its_minimum_resizes_smoothly(cx: &mut TestAppContext) {
    let (mut h, _dock) = open_docked(cx, 700.0);
    h.show(transient(), panes_of(&[Some(0.3), None, None]));
    let min = rem_px(&mut h) * super::pane::MIN_PANE_HEIGHT;
    let before = pane_heights(&mut h);
    assert!(within(before[0], min, 1.0), "held: {before:?}");
    assert!(within(before[1], before[2], 1.0), "{before:?}");
    drag_divider(&mut h, 0, 20.0, 2, |_| {});
    let after = pane_heights(&mut h);
    assert!(within(after[0], before[0] + 20.0, 1.0), "{after:?}");
    assert!(within(after[1], before[1] - 20.0, 1.0), "{after:?}");
    assert!(within(after[2], before[2], 1.0), "{after:?}");

    // A double click there too: the two become equal, the third stays.
    h.show(transient(), panes_of(&[Some(0.3), None, None]));
    assert_eq!(pane_heights(&mut h), before);
    let changes = h.changes.get();
    let position = divider_bounds(&mut h, 0).center();
    h.cx.simulate_mouse_move(position, None, Modifiers::default());
    click_with_count(&mut h, position, 1);
    click_with_count(&mut h, position, 2);
    assert_eq!(h.changes.get(), changes + 1);
    let equal = pane_heights(&mut h);
    assert!(within(equal[0], equal[1], 1.0), "{equal:?}");
    assert!(within(equal[0] + equal[1], before[0] + before[1], 1.0));
    assert!(within(equal[2], before[2], 1.0), "{equal:?}");

    // Review: the other divider's panes are painted by their weights, so
    // dragging it changes only their heights; the held pane keeps its own,
    // and with it its share of a taller dock.
    h.show(transient(), panes_of(&[Some(0.3), None, None]));
    drag_divider(&mut h, 1, 20.0, 2, |_| {});
    let after = pane_heights(&mut h);
    assert!(within(after[0], before[0], 1.0), "{after:?}");
    assert!(within(after[1], before[1] + 20.0, 1.0), "{after:?}");
    assert!(within(after[2], before[2] - 20.0, 1.0), "{after:?}");
    assert_eq!(stored_heights(&mut h)[0], Some(0.3));
}

/// The `(y, height)` of each pane's plot in an exported SVG.
fn exported_plots(svg: &str) -> Vec<(f32, f32)> {
    let attribute = |text: &str, name: &str| -> f32 {
        let key = format!(" {name}=\"");
        let start = text.find(&key).expect("the attribute") + key.len();
        let end = start + text[start..].find('"').expect("a closing quote");
        text[start..end].parse().expect("a number")
    };
    svg.split("<clipPath id=\"plot-")
        .skip(1)
        .map(|rest| {
            let rect = &rest[..rest.find("/>").expect("the clip rect")];
            (attribute(rect, "y"), attribute(rect, "height"))
        })
        .collect()
}

/// The ratio of the first exported pane's height to the second's: the
/// space between two plots holds one pane's gutters.
fn exported_ratio(svg: &str) -> f32 {
    let plots = exported_plots(svg);
    assert_eq!(plots.len(), 2);
    let gutters = plots[1].0 - (plots[0].0 + plots[0].1);
    (plots[0].1 + gutters) / (plots[1].1 + gutters)
}

/// D23: exports (SVG, and the PNG, JPEG and clipboard images drawn from it)
/// keep the panes' relative heights, also after a drag on screen.
#[gpui_kit::test]
fn exports_keep_the_relative_pane_heights(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), panes_of(&[Some(3.0), None]));
    for (width, height) in [(800, 900), (1600, 1200)] {
        let svg = h
            .viewer
            .read_with(&h.cx, |v, _| v.svg(width, height))
            .unwrap();
        let ratio = exported_ratio(&svg);
        assert!((ratio - 3.0).abs() < 0.01, "{width}×{height}: {ratio}");
    }
    drag_divider(&mut h, 0, -100.0, 2, |_| {});
    let weights = stored_heights(&mut h);
    let (Some(top), Some(bottom)) = (weights[0], weights[1]) else {
        panic!("both heights are stored: {weights:?}");
    };
    let svg = h.viewer.read_with(&h.cx, |v, _| v.svg(800, 900)).unwrap();
    let ratio = exported_ratio(&svg);
    assert!(
        (ratio - top / bottom).abs() < 0.01,
        "{ratio} vs {top} / {bottom}"
    );
}

/// Review of D23 with D14: a divider band lies over the edges of both its
/// panes. A legend chip dropped there lands on the pane highlighted under
/// the pointer, above the line or below it, as anywhere else on the pane,
/// and the dividers are back once the chip is dropped.
#[gpui_kit::test]
fn a_chip_dropped_on_a_divider_lands_on_the_highlighted_pane(cx: &mut TestAppContext) {
    let mut h = open(cx);
    let setup = PlotSetup {
        panes: vec![
            PlotPane {
                traces: vec![Trace::new("V(out)"), Trace::new("V(in)")],
                ..PlotPane::default()
            },
            PlotPane {
                traces: vec![Trace::new("I(R1)")],
                ..PlotPane::default()
            },
        ],
        active_pane: 0,
    };
    h.show(transient(), setup);
    let band = divider_bounds(&mut h, 0);
    let quarter = band.size.height / 4.0;
    // Below the line: the top of the lower pane's header.
    let below = point(band.center().x, band.center().y + quarter);
    assert!(pane_bounds(&mut h, 1).contains(&below));
    h.drag_chip("trace-chip-0-1", below, |h| {
        assert!(h.rendered("pane-drop-1"), "the lower pane is the target");
    });
    let moved = h.setup();
    assert_eq!(expressions(&moved, 0), ["V(out)"]);
    assert_eq!(expressions(&moved, 1), ["I(R1)", "V(in)"]);
    assert!(
        !h.rendered("pane-drop-1"),
        "the highlight ends with the drop"
    );
    assert!(h.rendered("pane-divider-0"), "the divider is back");

    // Above the line: the bottom of the upper pane's x labels.
    let band = divider_bounds(&mut h, 0);
    let above = point(band.center().x, band.center().y - quarter);
    assert!(pane_bounds(&mut h, 0).contains(&above));
    h.drag_chip("trace-chip-1-0", above, |h| {
        assert!(h.rendered("pane-drop-0"), "the upper pane is the target");
    });
    let moved = h.setup();
    assert_eq!(expressions(&moved, 0), ["V(out)", "I(R1)"]);
    assert_eq!(expressions(&moved, 1), ["V(in)"]);
    assert!(
        stored_heights(&mut h).iter().all(Option::is_none),
        "no resize"
    );

    // The divider still drags afterwards.
    let before = pane_heights(&mut h);
    drag_divider(&mut h, 0, 30.0, 3, |_| {});
    let after = pane_heights(&mut h);
    assert!(within(after[0], before[0] + 30.0, 1.0), "{after:?}");

    // The band was under the pointer when a chip drag began (the move that
    // starts it can land there), so its grip shows; left out during the
    // drag, the band never hears the pointer leave, and the grip must not
    // outlive the drag.
    let chip = h.bounds("trace-chip-0-1");
    h.cx.simulate_mouse_move(chip.center(), None, Modifiers::default());
    h.cx.run_until_parked();
    h.viewer.update(&mut h.cx, |v, cx| {
        v.view.divider_hover = Some(0);
        cx.notify();
    });
    h.cx.run_until_parked();
    assert!(h.rendered("pane-divider-grip-0"));
    let target = h.at(1, 0.5, 0.5);
    h.drag_chip("trace-chip-0-1", target, |_| {});
    assert_eq!(expressions(&h.setup(), 1), ["V(in)", "I(R1)"]);
    assert!(!h.rendered("pane-divider-grip-0"), "no stale grip");
}

/// Review of D23: with more panes than fit every pane is at its minimum,
/// so a double click on a divider moves nothing on screen. It makes only
/// its own two panes' stored heights equal, so they are equal once the
/// dock is tall enough, and keeps every other pane's height.
#[gpui_kit::test]
fn a_double_click_in_a_scrolling_stack_keeps_the_other_heights(cx: &mut TestAppContext) {
    let mut h = open(cx);
    let mut heights = vec![None; 9];
    heights[0] = Some(2.0);
    heights[3] = Some(0.5);
    h.show(transient(), panes_of(&heights));
    let before = pane_heights(&mut h);
    let min = rem_px(&mut h) * super::pane::MIN_PANE_HEIGHT;
    assert!(before.iter().all(|&x| within(x, min, 1.0)), "{before:?}");
    let changes = h.changes.get();
    let double_click = |h: &mut Harness, upper: usize| {
        let position = divider_bounds(h, upper).center();
        h.cx.simulate_mouse_move(position, None, Modifiers::default());
        click_with_count(h, position, 1);
        click_with_count(h, position, 2);
    };
    // Panes 1 and 2 already weigh the same: nothing to do.
    double_click(&mut h, 1);
    assert_eq!(stored_heights(&mut h), heights);
    assert_eq!(h.changes.get(), changes, "nothing changed");
    // Panes 0 and 1 share their weights; the others keep theirs.
    double_click(&mut h, 0);
    let mut expected = heights.clone();
    expected[0] = Some(1.5);
    expected[1] = Some(1.5);
    assert_eq!(stored_heights(&mut h), expected);
    assert_eq!(h.changes.get(), changes + 1);
    assert_eq!(pane_heights(&mut h), before, "nothing moves on screen");
}

/// Review of D23: the middle button on a divider band neither pans nor
/// starts a resize, and pressing or releasing it during a divider drag
/// leaves that drag alone, as for the plot's other drags (D14).
#[gpui_kit::test]
fn the_middle_button_on_a_divider_does_nothing(cx: &mut TestAppContext) {
    let mut h = open(cx);
    h.show(transient(), panes_of(&[None, None]));
    let center = h.at(0, 0.5, 0.5);
    h.scroll(center, 3.0, Modifiers::default());
    let zoomed = h.setup();
    let changes = h.changes.get();
    let none = Modifiers::default();
    let from = divider_bounds(&mut h, 0).center();
    let to = point(from.x + px(80.), from.y + px(40.));
    h.drag(MouseButton::Middle, from, to);
    assert_eq!(h.setup(), zoomed, "no pan and no resize");
    assert_eq!(h.changes.get(), changes);
    assert_eq!(h.viewer.read_with(&h.cx, |v, _| v.view.drag), None);

    // A left drag of the divider, with the middle button pressed and
    // released on the way.
    let before = pane_heights(&mut h);
    h.cx.simulate_mouse_move(from, None, none);
    h.cx.simulate_mouse_down(from, MouseButton::Left, none);
    let mid = point(from.x, from.y + px(20.));
    h.cx.simulate_mouse_move(mid, MouseButton::Left, none);
    h.cx.simulate_mouse_down(mid, MouseButton::Middle, none);
    h.cx.simulate_mouse_up(mid, MouseButton::Middle, none);
    let end = point(from.x, from.y + px(45.));
    h.cx.simulate_mouse_move(end, MouseButton::Left, none);
    h.cx.run_until_parked();
    let drag = h.viewer.read_with(&h.cx, |v, _| v.view.drag);
    assert!(
        matches!(drag, Some(Drag::Divider { upper: 0, .. })),
        "{drag:?}"
    );
    h.cx.simulate_mouse_up(end, MouseButton::Left, none);
    h.cx.run_until_parked();
    let after = pane_heights(&mut h);
    assert!(within(after[0], before[0] + 45.0, 1.0), "{after:?}");
    assert_eq!(h.changes.get(), changes + 1, "one change, on the release");
    let now = h.setup();
    for (now, then) in now.panes.iter().zip(&zoomed.panes) {
        assert_eq!((now.x_range, now.y_range), (then.x_range, then.y_range));
    }
}
