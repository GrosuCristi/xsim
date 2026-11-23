//! Mouse and keyboard input on the canvas, routed to the active tool.
//!
//! Listeners are registered on the window while the canvas paints, so a
//! drag keeps tracking after the mouse leaves the canvas. Presses and hover
//! only count over the canvas hitbox (overlays such as the label field
//! occlude it).

use crate::model::geometry::Vec2;
use crate::ui::gesture::Gesture;
use crate::ui::state::Pointer;
use crate::{SchematicEditor, Tool};
use gpui_kit::{
    Context, DispatchPhase, Hitbox, KeyDownEvent, KeyUpEvent, Modifiers, ModifiersChangedEvent,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, ScrollEvent, WeakEntity,
    Window,
};

/// Registers this frame's mouse listeners for the canvas `hitbox`.
pub fn register_listeners(
    window: &mut Window,
    hitbox: &Hitbox,
    editor: WeakEntity<SchematicEditor>,
) {
    let (hit, e) = (hitbox.clone(), editor.clone());
    window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
        if phase == DispatchPhase::Bubble && hit.is_hovered(window) {
            e.update(cx, |editor, cx| editor.mouse_down(event, window, cx))
                .ok();
        }
    });
    let (hit, e) = (hitbox.clone(), editor.clone());
    window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
        if phase == DispatchPhase::Bubble {
            let over = hit.is_hovered(window);
            e.update(cx, |editor, cx| editor.mouse_move(event, over, cx))
                .ok();
        }
    });
    let e = editor.clone();
    window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
        if phase == DispatchPhase::Bubble {
            e.update(cx, |editor, cx| editor.mouse_up(event, cx)).ok();
        }
    });
    let hit = hitbox.clone();
    window.on_mouse_event(move |event: &ScrollEvent, phase, window, cx| {
        if phase == DispatchPhase::Bubble && hit.should_handle_scroll(window) {
            editor
                .update(cx, |editor, cx| editor.scroll(event, cx))
                .ok();
        }
    });
}

impl SchematicEditor {
    /// The pointer at a window position, if the canvas has been measured.
    pub(crate) fn pointer_at(
        &self,
        position: gpui_kit::Point<Pixels>,
        modifiers: Modifiers,
    ) -> Option<Pointer> {
        let viewport = self.viewport.get()?;
        let screen = Vec2::new(f32::from(position.x), f32::from(position.y));
        Some(Pointer {
            screen,
            world: self.camera().to_world(&viewport, screen),
            any_angle: modifiers.alt,
        })
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.label_editor.is_some() {
            self.confirm_label(window, cx);
            if self.label_editor.is_some() {
                self.close_label_editor(window, cx);
            }
        }
        self.focus.focus(window, cx);
        let Some(pointer) = self.pointer_at(event.position, event.modifiers) else {
            return;
        };
        self.interaction.pointer = Some(pointer);
        self.notice = None;
        // A middle press while the left button drags would silently drop
        // the drag (and its preview); it is ignored until the drag ends.
        let left_busy = !matches!(
            self.interaction.gesture,
            Gesture::Idle | Gesture::Panning { .. }
        );
        match event.button {
            MouseButton::Middle if left_busy => {}
            MouseButton::Middle => self.begin_pan(pointer),
            MouseButton::Left if self.interaction.space_held => self.begin_pan(pointer),
            MouseButton::Left => self.tool_press(pointer, event, window, cx),
            _ => {}
        }
        cx.notify();
    }

    fn tool_press(
        &mut self,
        pointer: Pointer,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.tool {
            Tool::Select => {
                self.select_press(pointer, event.modifiers, event.click_count, window, cx)
            }
            Tool::Wire => self.wire_click(pointer, event.click_count, cx),
            Tool::Label => self.label_click(pointer.world, window, cx),
            Tool::Place(_) => self.place_at(pointer.world.snapped(), cx),
        }
    }

    fn begin_pan(&mut self, pointer: Pointer) {
        self.interaction.gesture = Gesture::Panning {
            last: pointer.screen,
        };
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, over_canvas: bool, cx: &mut Context<Self>) {
        let pointer = self.pointer_at(event.position, event.modifiers);
        if let (Gesture::Panning { last }, Some(p)) = (&mut self.interaction.gesture, pointer) {
            let delta = p.screen - *last;
            *last = p.screen;
            self.pan_by(delta, cx);
            return;
        }
        let dragging = !matches!(self.interaction.gesture, Gesture::Idle);
        if dragging {
            if let Some(p) = pointer {
                self.interaction.pointer = Some(p);
                self.select_drag(p);
                if matches!(self.interaction.gesture, Gesture::Probing(_)) {
                    self.update_hover(over_canvas.then_some(p), cx);
                } else {
                    self.hover.clear();
                }
            }
            cx.notify();
            return;
        }
        let pointer = pointer.filter(|_| over_canvas);
        let moved_cell = pointer.map(|p| (p.world.snapped(), p.any_angle))
            != self
                .interaction
                .pointer
                .map(|p| (p.world.snapped(), p.any_angle));
        self.interaction.pointer = pointer;
        self.update_hover(pointer, cx);
        // The probe glyph follows the mouse exactly; everything else
        // (ghost, rubber band, coordinates) moves cell by cell.
        if moved_cell || self.hover.net.is_some() {
            cx.notify();
        }
    }

    fn mouse_up(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        let pointer = self.pointer_at(event.position, event.modifiers);
        match (&self.interaction.gesture, event.button) {
            (Gesture::Idle, _) => return,
            (Gesture::Panning { .. }, MouseButton::Middle | MouseButton::Left) => {
                self.interaction.gesture = Gesture::Idle;
            }
            (_, MouseButton::Left) => self.select_release(pointer, cx),
            _ => return,
        }
        self.update_hover(pointer, cx);
        cx.notify();
    }

    /// Space starts drag-panning; other keys pass through to the bindings.
    pub(crate) fn key_down(
        &mut self,
        event: &KeyDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.keystroke.key == "space" && self.label_editor.is_none() {
            self.interaction.space_held = true;
            cx.notify();
        }
    }

    /// Alt toggles any-angle wiring without moving the mouse.
    pub(crate) fn modifiers_changed(
        &mut self,
        event: &ModifiersChangedEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(pointer) = &mut self.interaction.pointer
            && pointer.any_angle != event.modifiers.alt
        {
            pointer.any_angle = event.modifiers.alt;
            cx.notify();
        }
    }

    pub(crate) fn key_up(&mut self, event: &KeyUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "space" {
            self.interaction.space_held = false;
            cx.notify();
        }
    }
}
