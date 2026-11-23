//! A horizontal split whose divider stores a width fraction, so the layout
//! is saved per project independently of the window size (Q39).

use crate::layout::{fraction_at, sanitize_fraction};
use gpui_kit::component::h_flex;
use gpui_kit::component::resizable::{HandleEdge, resize_handle};
use gpui_kit::{prelude::*, *};
use std::rc::Rc;

/// Drag payload of the divider.
#[derive(Clone)]
struct SplitDrag;

/// What follows the pointer while the divider is dragged: nothing; the
/// panels themselves resize.
struct DragGhost;

impl Render for DragGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

/// `left` and `right` side by side, `left` taking `fraction` of the width.
/// Dragging the divider calls `on_change` with the new fraction.
pub fn horizontal_split(
    id: impl Into<ElementId>,
    fraction: f32,
    left: impl IntoElement,
    right: impl IntoElement,
    on_change: impl Fn(f32, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let on_change = Rc::new(on_change);
    let handle = resize_handle::<SplitDrag, DragGhost>("split-handle", Axis::Horizontal)
        .inside(HandleEdge::Trailing)
        .on_drag(SplitDrag, |_, _, _, cx| cx.new(|_| DragGhost));
    h_flex()
        .id(id)
        .size_full()
        .on_drag_move(move |event: &DragMoveEvent<SplitDrag>, window, cx| {
            let fraction = fraction_at(
                f32::from(event.event.position.x),
                f32::from(event.bounds.left()),
                f32::from(event.bounds.size.width),
            );
            on_change(fraction, window, cx);
        })
        .child(
            div()
                .relative()
                .h_full()
                .min_w_0()
                .w(relative(sanitize_fraction(fraction)))
                .child(left)
                .child(handle),
        )
        .child(div().h_full().min_w_0().flex_1().child(right))
}
