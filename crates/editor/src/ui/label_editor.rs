//! The inline field that names a new net label or renames one (Q52).

use crate::SchematicEditor;
use crate::model::geometry::Vec2;
use crate::model::ops;
use gpui_kit::{
    component::{
        ActiveTheme, Sizable,
        input::{Input, InputEvent, InputState},
    },
    prelude::*,
    *,
};
use xsim_project::{Id, Point};

/// Width of the inline field in pixels.
const FIELD_WIDTH: f32 = 168.0;

/// What confirming the field does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelTarget {
    /// Adds a label at this grid point.
    New(Point),
    /// Renames an existing label.
    Rename(Id),
}

/// The open inline field.
pub struct LabelEditor {
    pub target: LabelTarget,
    pub input: Entity<InputState>,
    /// Why the last confirmed name was rejected.
    pub error: Option<SharedString>,
    /// The text that was rejected; editing it clears the error.
    rejected: Option<SharedString>,
    _subscription: Subscription,
}

impl SchematicEditor {
    /// Asks for the name of a new label at `at`.
    pub(crate) fn open_label_create(
        &mut self,
        at: Point,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_label_editor(LabelTarget::New(at), String::new(), window, cx);
    }

    /// Edits the name of label `id` in place.
    pub(crate) fn open_label_rename(
        &mut self,
        id: Id,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(name) = self.schematic.label(id).map(|l| l.name.clone()) else {
            return;
        };
        self.open_label_editor(LabelTarget::Rename(id), name, window, cx);
    }

    fn open_label_editor(
        &mut self,
        target: LabelTarget,
        initial: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only {
            return;
        }
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Net name, e.g. out")
                .default_value(initial)
        });
        let subscription =
            cx.subscribe_in(&input, window, |editor, _, event, window, cx| match event {
                InputEvent::PressEnter { .. } => editor.confirm_label(window, cx),
                InputEvent::Change => editor.label_text_changed(cx),
                _ => {}
            });
        // Focus after the current mouse event: the canvas takes focus on press.
        let field = input.clone();
        window.defer(cx, move |window, cx| {
            field.update(cx, |state, cx| {
                state.focus(window, cx);
                state.select_all(window, cx);
            });
        });
        self.label_editor = Some(LabelEditor {
            target,
            input,
            error: None,
            rejected: None,
            _subscription: subscription,
        });
        cx.notify();
    }

    /// Validates the typed name and applies it as one undoable step.
    pub(crate) fn confirm_label(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(field) = &mut self.label_editor else {
            return;
        };
        let text = field.input.read(cx).value();
        let name = match ops::validate_net_name(&text) {
            Ok(name) => name,
            Err(message) => {
                field.error = Some(message.into());
                field.rejected = Some(text);
                cx.notify();
                return;
            }
        };
        let target = field.target;
        self.close_label_editor(window, cx);
        self.edit(cx, |s| match target {
            LabelTarget::New(at) => {
                ops::add_label(s, name, at);
            }
            LabelTarget::Rename(id) => {
                ops::rename_label(s, id, name);
            }
        });
    }

    /// Clears the error once the rejected text has been edited.
    fn label_text_changed(&mut self, cx: &mut Context<Self>) {
        let Some(field) = &mut self.label_editor else {
            return;
        };
        if field.rejected.as_ref() != Some(&field.input.read(cx).value()) {
            field.error = None;
            field.rejected = None;
            cx.notify();
        }
    }

    /// Closes the field without applying it and gives focus back to the canvas.
    pub(crate) fn close_label_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.label_editor.take().is_some() {
            self.focus.focus(window, cx);
            cx.notify();
        }
    }

    /// Grid point the open field belongs to.
    pub(crate) fn label_editor_anchor(&self) -> Option<Point> {
        match self.label_editor.as_ref()?.target {
            LabelTarget::New(at) => Some(at),
            LabelTarget::Rename(id) => self.schematic.label(id).map(|l| l.position),
        }
    }
}

/// The field, its top-left corner at `at` relative to the canvas.
pub fn label_editor_element(field: &LabelEditor, at: Vec2, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let hint: SharedString = field
        .error
        .clone()
        .unwrap_or_else(|| "Enter to confirm · Esc to cancel".into());
    let hint_color = if field.error.is_some() {
        theme.danger
    } else {
        theme.muted_foreground
    };
    div()
        .id("label-editor")
        .debug_selector(|| "label-editor".into())
        .occlude()
        .absolute()
        .left(px(at.x))
        .top(px(at.y))
        .w(px(FIELD_WIDTH))
        .flex()
        .flex_col()
        .gap_1()
        .p_1p5()
        .rounded(theme.radius)
        .border_1()
        .border_color(theme.border)
        .bg(theme.popover)
        .shadow_md()
        .child(Input::new(&field.input).small())
        .child(div().text_xs().text_color(hint_color).child(hint))
        .into_any_element()
}
