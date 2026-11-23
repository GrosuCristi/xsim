//! Editing commands: every model change goes through [`SchematicEditor::edit`]
//! or [`SchematicEditor::commit`], which record one undo step (Q57), clean
//! up the wires and emit [`EditorEvent::Changed`].

use crate::model::cleanup::normalize;
use crate::model::drag::MovePlan;
use crate::model::fragment;
use crate::model::ops;
use crate::model::selection::Selection;
use crate::model::transform::{Mirror, mirror_selection, rotate_selection};
use crate::ui::gesture::Gesture;
use crate::{EditorEvent, SchematicEditor};
use gpui_kit::{ClipboardItem, Context, SharedString, Window};
use std::sync::Arc;
use xsim_project::{Connectivity, Point, Schematic, connectivity};

/// Offset of duplicated objects, in grid units.
const DUPLICATE_OFFSET: Point = Point::new(2, 2);

impl SchematicEditor {
    /// Applies `edit` to a copy of the schematic and commits the result as
    /// one undoable step. Does nothing in read-only mode.
    pub(crate) fn edit<R>(
        &mut self,
        cx: &mut Context<Self>,
        edit: impl FnOnce(&mut Schematic) -> R,
    ) -> Option<R> {
        if self.read_only {
            return None;
        }
        let mut next = (*self.schematic).clone();
        let result = edit(&mut next);
        normalize(&mut next);
        self.commit(next, cx);
        Some(result)
    }

    /// Replaces the schematic with `next` as one undoable step. Returns
    /// whether anything changed.
    pub(crate) fn commit(&mut self, next: Schematic, cx: &mut Context<Self>) -> bool {
        if self.read_only || next == *self.schematic {
            return false;
        }
        let before = std::mem::replace(&mut self.schematic, Arc::new(next));
        self.history.record(before);
        self.model_changed(cx);
        true
    }

    fn model_changed(&mut self, cx: &mut Context<Self>) {
        self.connectivity = None;
        self.selection.retain_existing(&self.schematic);
        self.hover.clear();
        cx.emit(EditorEvent::Changed);
        cx.notify();
    }

    /// Connectivity of the current schematic, computed once per change.
    pub(crate) fn connectivity(&mut self) -> Arc<Connectivity> {
        self.connectivity
            .get_or_insert_with(|| Arc::new(connectivity(&self.schematic)))
            .clone()
    }

    /// Undo (`back`) or redo one step.
    pub(crate) fn step_history(&mut self, back: bool, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        let current = self.schematic.clone();
        let restored = if back {
            self.history.undo(current)
        } else {
            self.history.redo(current)
        };
        if let Some(state) = restored {
            self.schematic = state;
            self.interaction.gesture = Gesture::Idle;
            self.interaction.wire = None;
            self.model_changed(cx);
        }
    }

    /// Deletes the selection (Delete, Backspace).
    pub(crate) fn delete_selection(&mut self, cx: &mut Context<Self>) {
        let selection = std::mem::take(&mut self.selection);
        if self.edit(cx, |s| ops::delete_selection(s, &selection)) != Some(true) {
            self.selection = selection;
        }
    }

    /// Rotates the placement ghost, or the selection (disconnecting it, Q60).
    pub(crate) fn rotate(&mut self, cx: &mut Context<Self>) {
        if self.transform_ghost(None, cx) {
            return;
        }
        let selection = self.selection.clone();
        self.edit(cx, |s| rotate_selection(s, &selection));
    }

    /// Mirrors the placement ghost or the selection on screen.
    pub(crate) fn mirror(&mut self, mirror: Mirror, cx: &mut Context<Self>) {
        if self.transform_ghost(Some(mirror), cx) {
            return;
        }
        let selection = self.selection.clone();
        self.edit(cx, |s| mirror_selection(s, &selection, mirror));
    }

    /// Selects everything (Ctrl+A).
    pub(crate) fn select_all(&mut self, cx: &mut Context<Self>) {
        self.switch_tool(crate::Tool::Select, cx);
        self.selection = Selection::all(&self.schematic);
        cx.notify();
    }

    /// Copies the selection to the system clipboard (Q58).
    pub(crate) fn copy(&mut self, cx: &mut Context<Self>) {
        if self.selection.is_empty() {
            return;
        }
        let fragment = fragment::extract(&self.schematic, &self.selection);
        if let Some(text) = fragment::encode(&fragment) {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            self.notify_status(format!("Copied {}", describe(&self.selection)), cx);
        }
    }

    /// Copies, then deletes the selection.
    pub(crate) fn cut(&mut self, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        self.copy(cx);
        self.delete_selection(cx);
    }

    /// Pastes clipboard objects centred under the mouse (or the view centre).
    pub(crate) fn paste(&mut self, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        let text = cx.read_from_clipboard().and_then(|item| item.text());
        let Some(fragment) = text.as_deref().and_then(fragment::decode) else {
            self.notify_status("The clipboard holds no schematic objects", cx);
            return;
        };
        let target = self
            .interaction
            .pointer
            .map(|p| p.world.snapped())
            .unwrap_or_else(|| self.camera().center.snapped());
        let offset = fragment::center(&fragment).map_or(Point::default(), |c| target - c);
        self.insert_fragment(&fragment, offset, cx);
    }

    /// Duplicates the selection next to itself (Ctrl+D).
    pub(crate) fn duplicate(&mut self, cx: &mut Context<Self>) {
        if self.selection.is_empty() {
            return;
        }
        let fragment = fragment::extract(&self.schematic, &self.selection);
        self.insert_fragment(&fragment, DUPLICATE_OFFSET, cx);
    }

    fn insert_fragment(&mut self, fragment: &Schematic, offset: Point, cx: &mut Context<Self>) {
        self.switch_tool(crate::Tool::Select, cx);
        if let Some(inserted) = self.edit(cx, |s| fragment::insert(s, fragment, offset)) {
            self.selection = inserted;
            self.selection.retain_existing(&self.schematic);
            cx.notify();
        }
    }

    /// Arrow keys: move the selection one grid step (wires stretch as when
    /// dragging), or scroll the view when nothing is selected.
    pub(crate) fn nudge(&mut self, dx: i32, dy: i32, cx: &mut Context<Self>) {
        const PAN_STEP: f32 = 64.0;
        if self.selection.is_empty() || self.read_only {
            let delta = crate::model::geometry::Vec2::new(-dx as f32, -dy as f32) * PAN_STEP;
            self.pan_by(delta, cx);
            return;
        }
        let moved = MovePlan::new(&self.schematic, &self.selection).apply(Point::new(dx, dy));
        self.commit(moved, cx);
    }

    /// Esc: closes the label editor, aborts a drag, cancels the wire being
    /// drawn, leaves the current tool, then clears the selection.
    pub(crate) fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.label_editor.is_some() {
            self.close_label_editor(window, cx);
        } else if !matches!(self.interaction.gesture, Gesture::Idle) {
            self.interaction.gesture = Gesture::Idle;
        } else if self.interaction.wire.is_some() {
            self.interaction.wire = None;
        } else if self.tool != crate::Tool::Select {
            self.switch_tool(crate::Tool::Select, cx);
        } else {
            self.selection.clear();
        }
        cx.notify();
    }

    /// Enter: finishes the wire being drawn.
    pub(crate) fn confirm(&mut self, cx: &mut Context<Self>) {
        self.finish_wire(cx);
    }

    /// Shows a short message in the status strip.
    pub(crate) fn notify_status(
        &mut self,
        message: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        self.notice = Some(message.into());
        cx.notify();
    }
}

/// "2 parts and 3 wires" style summary of a selection.
fn describe(selection: &Selection) -> String {
    use crate::ObjectRef::*;
    let count = |f: fn(&crate::ObjectRef) -> bool| selection.iter().filter(f).count();
    let parts = [
        (count(|o| matches!(o, Component(_))), "part", "parts"),
        (count(|o| matches!(o, Wire(_))), "wire", "wires"),
        (count(|o| matches!(o, Label(_))), "label", "labels"),
    ];
    let words: Vec<String> = parts
        .into_iter()
        .filter(|(n, ..)| *n > 0)
        .map(|(n, one, many)| format!("{n} {}", if n == 1 { one } else { many }))
        .collect();
    match words.as_slice() {
        [] => "nothing".into(),
        [only] => only.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::describe;
    use crate::ObjectRef;
    use crate::model::selection::Selection;

    #[test]
    fn selections_are_described_in_words() {
        let s: Selection = [
            ObjectRef::Component(1),
            ObjectRef::Component(2),
            ObjectRef::Wire(3),
        ]
        .into_iter()
        .collect();
        assert_eq!(describe(&s), "2 parts and 1 wire");
        assert_eq!(describe(&Selection::default()), "nothing");
    }
}
