//! The add-trace field (Q132): any expression, with signal-name completion
//! and inline errors.
//!
//! The suggestion list and the error open below the field with a small gap,
//! so the typed formula stays visible (D14); when there is no room below
//! they open above it, again without covering it, and they follow the field
//! when it moves. The error shows while the field has focus (the field's
//! red border stays), so it never hides the legend below it.

use crate::WaveformViewer;
use crate::model::completion::{Completion, complete};
use crate::model::op_table::is_operating_point;
use gpui_kit::base::{Align, ElementExt, Placement, Positioner};
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState, MoveDown, MoveUp};
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable, v_flex};
use gpui_kit::{prelude::*, *};
use std::cell::Cell;
use std::rc::Rc;

/// Suggestions shown at once.
const MAX_SUGGESTIONS: usize = 8;
/// Width of the field, in rems (Q30).
const WIDTH: f32 = 17.5;
/// Gap between the field and its popups, in rems.
const POPUP_GAP: f32 = 0.25;
/// Least distance between a popup and the window edge, in rems.
const POPUP_MARGIN: f32 = 0.5;

/// State of the add-trace field.
pub struct TraceInput {
    pub state: Entity<InputState>,
    /// Why the last submitted expression was rejected.
    pub error: Option<SharedString>,
    /// The text that produced `error`; the error stays until it is edited.
    rejected: String,
    pub suggestions: Vec<Completion>,
    pub highlighted: usize,
    /// Where the field was painted last, in window coordinates: the popups
    /// are placed next to it.
    pub field_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    _subscription: Subscription,
}

impl TraceInput {
    pub fn new(window: &mut Window, cx: &mut Context<WaveformViewer>) -> Self {
        let state = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Add trace: V(out), V(out)-V(in), db(V(out))")
        });
        let subscription = cx.subscribe_in(&state, window, WaveformViewer::on_trace_input);
        Self {
            state,
            error: None,
            rejected: String::new(),
            suggestions: Vec::new(),
            highlighted: 0,
            field_bounds: Rc::default(),
            _subscription: subscription,
        }
    }
}

impl WaveformViewer {
    /// Creates the add-trace field on first render.
    pub(crate) fn ensure_trace_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.view.input.is_none() {
            self.view.input = Some(TraceInput::new(window, cx));
        }
    }

    fn on_trace_input(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change | InputEvent::Focus => self.refresh_suggestions(cx),
            InputEvent::PressEnter { .. } => self.submit_trace_input(window, cx),
            InputEvent::Blur => self.close_suggestions(cx),
        }
    }

    /// Recomputes the suggestions for the current text; editing the rejected
    /// text clears its error.
    fn refresh_suggestions(&mut self, cx: &mut Context<Self>) {
        let names: Vec<String> = self
            .result
            .as_ref()
            .map(|r| r.signals.iter().map(|s| s.name.clone()).collect())
            .unwrap_or_default();
        let Some(input) = self.view.input.as_mut() else {
            return;
        };
        let text = input.state.read(cx).value();
        input.suggestions = complete(&text, &names, MAX_SUGGESTIONS);
        input.highlighted = 0;
        if *text != input.rejected {
            input.error = None;
        }
        cx.notify();
    }

    fn close_suggestions(&mut self, cx: &mut Context<Self>) {
        if let Some(input) = self.view.input.as_mut() {
            input.suggestions.clear();
            cx.notify();
        }
    }

    /// Enter: accepts the highlighted suggestion, or adds the expression.
    fn submit_trace_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(input) = self.view.input.as_ref() else {
            return;
        };
        if !input.suggestions.is_empty() {
            let index = input.highlighted;
            self.accept_suggestion(index, window, cx);
            return;
        }
        let state = input.state.clone();
        let text = state.read(cx).value().to_string();
        if text.trim().is_empty() {
            return;
        }
        let outcome = self.try_add_trace(&text, cx);
        let Some(input) = self.view.input.as_mut() else {
            return;
        };
        match outcome {
            Ok(_) => {
                input.error = None;
                state.update(cx, |state, cx| state.set_value("", window, cx));
            }
            Err(error) => {
                input.error = Some(error.to_string().into());
                input.rejected = text;
            }
        }
        cx.notify();
    }

    /// Replaces the fragment being typed with suggestion `index`.
    fn accept_suggestion(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(input) = self.view.input.as_ref() else {
            return;
        };
        let Some(completion) = input.suggestions.get(index).cloned() else {
            return;
        };
        let state = input.state.clone();
        let text = completion.apply(&state.read(cx).value());
        state.update(cx, |state, cx| {
            state.set_value(text, window, cx);
            state.focus(window, cx);
        });
        self.refresh_suggestions(cx);
    }

    /// Moves the suggestion highlight; `false` when no list is open.
    fn move_highlight(&mut self, step: isize, cx: &mut Context<Self>) -> bool {
        let Some(input) = self.view.input.as_mut() else {
            return false;
        };
        let count = input.suggestions.len();
        if count == 0 {
            return false;
        }
        input.highlighted = (input.highlighted as isize + step).rem_euclid(count as isize) as usize;
        cx.notify();
        true
    }
}

/// The field with its suggestion list or error callout below it.
pub fn trace_input(
    viewer: &WaveformViewer,
    window: &Window,
    cx: &mut Context<WaveformViewer>,
) -> AnyElement {
    let Some(input) = viewer.view.input.as_ref() else {
        return div().into_any_element();
    };
    let field_bounds = input.field_bounds.clone();
    let painted = input.field_bounds.get();
    let rem = window.rem_size();
    let has_error = input.error.is_some();
    // The error callout lies over the top pane's legend, so it shows only
    // while the field has focus; the red border keeps the error visible.
    // Suggestions replace a stale error while the text is edited.
    let focused = input.state.read(cx).focus_handle(cx).is_focused(window);
    let error = input
        .error
        .clone()
        .filter(|_| focused && input.suggestions.is_empty());
    let suggesting = !input.suggestions.is_empty();
    let open = error.is_some() || suggesting;
    // An operating point has a table, not traces.
    let disabled = viewer.result.as_deref().is_some_and(is_operating_point);
    let field = Input::new(&input.state)
        .small()
        .disabled(disabled)
        .aria_label("Add trace expression")
        .prefix(
            Icon::new(IconName::Plus)
                .small()
                .text_color(cx.theme().muted_foreground),
        )
        .when(has_error, |field| field.border_color(cx.theme().danger));
    v_flex()
        .id("trace-input")
        .debug_selector(|| "trace-input".into())
        .w(rems(WIDTH))
        .capture_action(cx.listener(|this, _: &MoveDown, _, cx| {
            if this.move_highlight(1, cx) {
                cx.stop_propagation();
            }
        }))
        .capture_action(cx.listener(|this, _: &MoveUp, _, cx| {
            if this.move_highlight(-1, cx) {
                cx.stop_propagation();
            }
        }))
        .capture_action(cx.listener(|this, _: &Escape, _, cx| {
            let open = this
                .view
                .input
                .as_ref()
                .is_some_and(|i| !i.suggestions.is_empty());
            if open {
                this.close_suggestions(cx);
                cx.stop_propagation();
            }
        }))
        .child(div().child(field).on_prepaint(move |bounds, window, _| {
            // The popups are placed against the bounds of the frame before;
            // when the field moved (a resized window or split), place them
            // again on the next frame, so they never end up over it.
            if field_bounds.replace(Some(bounds)) != Some(bounds) && open {
                window.request_animation_frame();
            }
        }))
        .when_some(painted, |el, anchor| {
            el.when_some(error, |el, error| {
                el.child(popup(anchor, rem, error_callout(error, cx)))
            })
            .when(suggesting, |el| {
                el.child(popup(anchor, rem, suggestion_list(input, cx)))
            })
        })
        .into_any_element()
}

/// Paints `content` next to the field at `anchor`, above everything else:
/// below it when it fits, otherwise above it, never over it. It takes the
/// pointer over its area, so nothing hidden below it can be clicked.
fn popup(anchor: Bounds<Pixels>, rem: Pixels, content: impl IntoElement) -> impl IntoElement {
    deferred(
        Positioner::side(anchor)
            .placement(Placement::Bottom)
            .align(Align::Start)
            .offset(rem * POPUP_GAP)
            .margin(rem * POPUP_MARGIN)
            .occlude()
            .child(content),
    )
    .with_priority(1)
}

/// The error below the field; pressing it dismisses it (the focus moves to
/// the viewer), uncovering the legend below.
fn error_callout(error: SharedString, cx: &mut Context<WaveformViewer>) -> impl IntoElement {
    let theme = cx.theme().clone();
    div()
        .id("trace-input-error")
        .debug_selector(|| "trace-input-error".into())
        .w(rems(WIDTH))
        .px_2()
        .py_1()
        .rounded(theme.radius)
        .border_1()
        .border_color(theme.danger)
        .bg(theme.popover)
        .shadow_md()
        .text_xs()
        .text_color(theme.danger)
        .child(error)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| {
                this.focus.focus(window, cx);
                cx.stop_propagation();
            }),
        )
}

fn suggestion_list(input: &TraceInput, cx: &mut Context<WaveformViewer>) -> impl IntoElement {
    let theme = cx.theme().clone();
    v_flex()
        .id("trace-suggestions")
        .debug_selector(|| "trace-suggestions".into())
        .w(rems(WIDTH))
        .p_1()
        .rounded(theme.radius)
        .border_1()
        .border_color(theme.border)
        .bg(theme.popover)
        .shadow_md()
        .children(
            input
                .suggestions
                .iter()
                .enumerate()
                .map(|(index, completion)| {
                    let highlighted = index == input.highlighted;
                    div()
                        .id(("trace-suggestion", index))
                        .debug_selector(move || format!("trace-suggestion-{index}"))
                        .px_2()
                        .py_0p5()
                        .rounded(theme.radius)
                        .text_sm()
                        .font_family(theme.mono_font_family.clone())
                        .text_color(theme.popover_foreground)
                        .when(highlighted, |row| {
                            row.bg(theme.accent).text_color(theme.accent_foreground)
                        })
                        .hover(|row| row.bg(theme.accent))
                        .cursor_pointer()
                        .child(completion.label.clone())
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, window, cx| {
                                this.accept_suggestion(index, window, cx);
                                cx.stop_propagation();
                            }),
                        )
                }),
        )
}
