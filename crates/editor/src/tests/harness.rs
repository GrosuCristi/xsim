//! A headless window hosting one or more editors, with helpers that drive
//! them through real mouse and keyboard events.

use crate::{EditorEvent, SchematicEditor};
use gpui_kit::{
    AppContext, Bounds, Entity, Focusable, KeyDownEvent, KeyUpEvent, Keystroke, Modifiers,
    MouseButton, MouseDownEvent, Pixels, Point, ScrollDelta, ScrollEvent, Subscription,
    TestAppContext, VisualTestContext, WindowBounds, WindowOptions, point, px, size,
};
use std::cell::RefCell;
use std::rc::Rc;
use xsim_project::{Point as GridPoint, Schematic, SchematicView};

/// A camera every test can rely on: origin centred, 20 px per grid unit.
pub fn fixed_view() -> SchematicView {
    SchematicView {
        center: [0.0, 0.0],
        zoom: 20.0,
        user_set: true,
    }
}

/// One editor in its own headless window.
pub struct Harness {
    pub cx: VisualTestContext,
    pub editor: Entity<SchematicEditor>,
    events: Rc<RefCell<Vec<EditorEvent>>>,
    _subscription: Subscription,
}

/// Opens `schematic` with the fixed camera.
pub fn open(cx: &mut TestAppContext, schematic: Schematic) -> Harness {
    open_with_view(cx, schematic, fixed_view())
}

/// Opens `schematic` with `view`.
pub fn open_with_view(
    cx: &mut TestAppContext,
    schematic: Schematic,
    view: SchematicView,
) -> Harness {
    open_sized(cx, schematic, view, size(px(1100.0), px(720.0)))
}

/// Opens `schematic` with `view` in a window of `window_size`.
pub fn open_sized(
    cx: &mut TestAppContext,
    schematic: Schematic,
    view: SchematicView,
    window_size: gpui_kit::Size<Pixels>,
) -> Harness {
    let (window, editor) = cx.update(|cx| {
        if !cx.has_global::<gpui_kit::component::Theme>() {
            gpui_kit::init(cx);
        }
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Point::default(),
                    window_size,
                ))),
                ..WindowOptions::default()
            },
            cx,
            |_, cx| cx.new(|cx| SchematicEditor::new(schematic, view, cx)),
        )
        .expect("open a test window")
    });
    let mut visual = VisualTestContext::from_window(window, cx);
    visual.run_until_parked();
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink = events.clone();
    let subscription = visual.update(|_, cx| {
        cx.subscribe(&editor, move |_, event: &EditorEvent, _| {
            sink.borrow_mut().push(event.clone())
        })
    });
    let mut harness = Harness {
        cx: visual,
        editor,
        events,
        _subscription: subscription,
    };
    harness.focus();
    harness
}

impl Harness {
    /// Gives keyboard focus to the editor.
    pub fn focus(&mut self) {
        let editor = self.editor.clone();
        self.cx.update(|window, cx| {
            let handle = editor.focus_handle(cx);
            handle.focus(window, cx);
        });
        self.cx.run_until_parked();
    }

    /// The schematic as the editor holds it now.
    pub fn schematic(&mut self) -> Schematic {
        self.editor
            .read_with(&self.cx, |e, _| e.schematic().clone())
    }

    /// Reads editor state.
    pub fn read<R>(&mut self, f: impl FnOnce(&SchematicEditor) -> R) -> R {
        self.editor.read_with(&self.cx, |e, _| f(e))
    }

    /// Updates editor state.
    pub fn update<R>(
        &mut self,
        f: impl FnOnce(&mut SchematicEditor, &mut gpui_kit::Context<SchematicEditor>) -> R,
    ) -> R {
        let result = self.editor.update(&mut self.cx, f);
        self.cx.run_until_parked();
        result
    }

    /// Takes the events emitted so far.
    pub fn events(&mut self) -> Vec<EditorEvent> {
        std::mem::take(&mut *self.events.borrow_mut())
    }

    /// Window position of a grid point, slightly off the exact dot so that
    /// snapping is exercised.
    pub fn at(&mut self, x: i32, y: i32) -> Point<Pixels> {
        let p = self
            .read(|e| e.grid_to_window(GridPoint::new(x, y)))
            .expect("the canvas has been laid out");
        p + point(px(1.5), px(-1.5))
    }

    /// Window position of a fractional world point.
    pub fn at_f(&mut self, x: f32, y: f32) -> Point<Pixels> {
        let origin = self.at(0, 0) - point(px(1.5), px(-1.5));
        let unit = self.at(1, 0) - self.at(0, 0);
        origin + point(unit.x * x, unit.x * y)
    }

    /// Centre of an element with a debug selector.
    pub fn control(&mut self, selector: &'static str) -> Option<Point<Pixels>> {
        self.cx.debug_bounds(selector).map(|b| b.center())
    }

    /// Clicks an element with a debug selector.
    pub fn click_control(&mut self, selector: &'static str) {
        let p = self
            .control(selector)
            .unwrap_or_else(|| panic!("{selector} is not rendered"));
        self.cx.simulate_mouse_move(p, None, Modifiers::default());
        self.cx.simulate_click(p, Modifiers::default());
    }

    /// Moves the mouse to a window position.
    pub fn hover(&mut self, p: Point<Pixels>) {
        self.cx.simulate_mouse_move(p, None, Modifiers::default());
    }

    /// Clicks at a window position.
    pub fn click_px(&mut self, p: Point<Pixels>, modifiers: Modifiers) {
        self.cx.simulate_mouse_move(p, None, modifiers);
        self.cx.simulate_click(p, modifiers);
    }

    /// Clicks at a grid point.
    pub fn click(&mut self, x: i32, y: i32) {
        let p = self.at(x, y);
        self.click_px(p, Modifiers::default());
    }

    /// Clicks at a grid point with modifiers held.
    pub fn click_with(&mut self, x: i32, y: i32, modifiers: Modifiers) {
        let p = self.at(x, y);
        self.click_px(p, modifiers);
    }

    /// Double-clicks at a window position.
    pub fn double_click_px(&mut self, p: Point<Pixels>) {
        self.click_px(p, Modifiers::default());
        self.cx.simulate_event(MouseDownEvent {
            position: p,
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count: 2,
            first_mouse: false,
        });
        self.cx
            .simulate_mouse_up(p, MouseButton::Left, Modifiers::default());
    }

    /// Drags with `button` from one window position to another in steps.
    pub fn drag_px(
        &mut self,
        from: Point<Pixels>,
        to: Point<Pixels>,
        button: MouseButton,
        modifiers: Modifiers,
    ) {
        self.cx.simulate_mouse_move(from, None, modifiers);
        self.cx.simulate_mouse_down(from, button, modifiers);
        for step in 1..=4 {
            let t = step as f32 / 4.0;
            let p = from + (to - from) * t;
            self.cx.simulate_mouse_move(p, Some(button), modifiers);
        }
        self.cx.simulate_mouse_up(to, button, modifiers);
    }

    /// Left-drags between two grid points.
    pub fn drag(&mut self, from: (i32, i32), to: (i32, i32)) {
        let (a, b) = (self.at(from.0, from.1), self.at(to.0, to.1));
        self.drag_px(a, b, MouseButton::Left, Modifiers::default());
    }

    /// Types key strokes, e.g. `"r"` or `"ctrl-z"`.
    pub fn keys(&mut self, keystrokes: &str) {
        self.cx.simulate_keystrokes(keystrokes);
    }

    /// Types text into the focused input.
    pub fn type_text(&mut self, text: &str) {
        self.cx.simulate_input(text);
    }

    /// Presses or releases the space bar.
    pub fn space(&mut self, down: bool) {
        let keystroke = Keystroke::parse("space").expect("valid keystroke");
        if down {
            self.cx.simulate_event(KeyDownEvent {
                keystroke,
                is_held: false,
                prefer_character_input: false,
            });
        } else {
            self.cx.simulate_event(KeyUpEvent { keystroke });
        }
    }

    /// Scrolls the wheel by `lines` at a window position.
    pub fn wheel(&mut self, at: Point<Pixels>, lines: f32, modifiers: Modifiers) {
        self.scroll(at, ScrollDelta::Lines(point(0.0, lines)), modifiers);
    }

    /// Turns the wheel by `notches` (negative: towards the user), one event
    /// per notch, as the platform reports them (in lines).
    pub fn wheel_notches(&mut self, at: Point<Pixels>, notches: i32) {
        let lines = crate::ui::navigation::LINES_PER_NOTCH * notches.signum() as f32;
        for _ in 0..notches.unsigned_abs() {
            self.wheel(at, lines, Modifiers::default());
        }
    }

    /// Sends one scroll event at a window position.
    pub fn scroll(&mut self, at: Point<Pixels>, delta: ScrollDelta, modifiers: Modifiers) {
        self.cx.simulate_mouse_move(at, None, modifiers);
        self.cx.simulate_event(ScrollEvent {
            position: at,
            delta,
            modifiers,
            ..ScrollEvent::default()
        });
    }

    /// Lets `duration` of (fake) time pass, firing due timers.
    pub fn wait(&mut self, duration: std::time::Duration) {
        self.cx.executor().advance_clock(duration);
        self.cx.run_until_parked();
    }

    /// Closes the window.
    pub fn close(mut self) {
        self.cx.update(|window, _| window.remove_window());
        self.cx.run_until_parked();
    }
}

/// Shift held.
pub fn shift() -> Modifiers {
    Modifiers {
        shift: true,
        ..Modifiers::default()
    }
}

/// Alt held.
pub fn alt() -> Modifiers {
    Modifiers {
        alt: true,
        ..Modifiers::default()
    }
}

/// The primary shortcut modifier (Cmd on macOS, Ctrl elsewhere) as a key
/// prefix for [`Harness::keys`].
pub fn primary(key: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("cmd-{key}")
    } else {
        format!("ctrl-{key}")
    }
}
