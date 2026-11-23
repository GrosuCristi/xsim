//! The dividers between plot panes (D23). Dragging one moves height from
//! one of its panes to the other; a double click makes the two equally
//! high. The heights are relative weights stored with the plot setup
//! ([`crate::model::heights`]).
//!
//! A divider is a band 0.5 rem high centred on the line between two panes,
//! with a row-resize pointer. At rest only that 1 px line shows; under the
//! pointer a small grip appears on it, and while it is dragged the grip
//! grows and darkens, in the theme's muted foreground colour, as on the
//! kit's own resize handles. The band is drawn at the top of the lower
//! pane, after its other children, so it lies above the upper pane's plot
//! and the lower pane's header: a press on it never reaches the plot, so
//! it never pans, zooms or moves a cursor, and it never makes the lower
//! pane the active one. While a legend chip is dragged (D14) the bands are
//! left out, so the chip can be dropped on either pane right up to the
//! line, as the drop highlight shows.
//!
//! While a divider is dragged the panes are laid out with the dragged
//! heights, and the setup is changed once, on release, so
//! [`crate::ViewerEvent::SetupChanged`] is emitted once per drag. A press
//! without movement, or Escape during the drag, changes nothing.

use super::pane::MIN_PANE_HEIGHT;
use super::state::{Drag, PaneResize};
use crate::WaveformViewer;
use crate::model::heights::{self, DividerDrag};
use gpui_kit::component::ActiveTheme;
use gpui_kit::{prelude::*, *};

/// Height of a divider's band, in rems.
pub const HIT_HEIGHT: f32 = 0.5;
/// Length of the grip under the pointer, and while dragged, in rems.
const GRIP_LENGTH: [f32; 2] = [1.25, 2.75];
/// Opacity of the grip under the pointer, and while dragged.
const GRIP_OPACITY: [f32; 2] = [0.35, 0.9];
/// Thickness of the grip, in pixels.
const GRIP_THICKNESS: f32 = 3.0;

/// The divider between pane `upper` and the pane below it, as a child of
/// the lower pane.
pub fn divider(
    viewer: &WaveformViewer,
    upper: usize,
    cx: &mut Context<WaveformViewer>,
) -> impl IntoElement {
    let dragged = matches!(viewer.view.drag, Some(Drag::Divider { upper: u, .. }) if u == upper);
    let hovered = viewer.view.divider_hover == Some(upper);
    let grip = (dragged || hovered).then(|| grip(upper, usize::from(dragged), cx));
    div()
        .id(("pane-divider", upper))
        .debug_selector(move || format!("pane-divider-{upper}"))
        .absolute()
        .left_0()
        .right_0()
        .top(rems(-HIT_HEIGHT / 2.0))
        .h(rems(HIT_HEIGHT))
        .flex()
        .items_center()
        .justify_center()
        // Above the plot of the upper pane and the header of the lower one:
        // neither sees the press, the hover or the pointer style. The wheel
        // still reaches them.
        .block_mouse_except_scroll()
        .cursor(CursorStyle::ResizeRow)
        .children(grip)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event, window, cx| {
                this.divider_down(upper, event, window, cx);
                cx.stop_propagation();
            }),
        )
        // The band hides the pointer from the viewer behind it, so it
        // forwards the moves of any drag that passes over it.
        .on_mouse_move(cx.listener(|this, event, _, cx| this.drag_move(event, cx)))
        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
            this.divider_hovered(upper, *hovered, cx);
        }))
}

/// The grip on the line: `level` 0 under the pointer, 1 while dragged.
fn grip(upper: usize, level: usize, cx: &App) -> impl IntoElement {
    div()
        .debug_selector(move || format!("pane-divider-grip-{upper}"))
        .flex_none()
        .h(px(GRIP_THICKNESS))
        .w(rems(GRIP_LENGTH[level]))
        .rounded_full()
        .bg(cx.theme().muted_foreground.opacity(GRIP_OPACITY[level]))
}

impl WaveformViewer {
    /// The weights the panes are laid out with: those of a divider drag in
    /// progress, else the stored ones.
    pub(crate) fn pane_weights(&self) -> Vec<f64> {
        match &self.view.resize {
            Some(resize) if resize.live.len() == self.setup.panes.len() => resize.live.clone(),
            _ => heights::weights(&self.setup),
        }
    }

    /// The painted height of every pane, when every pane was painted.
    fn painted_heights(&self) -> Option<Vec<f64>> {
        (0..self.setup.panes.len())
            .map(|i| {
                let bounds = self.view.panes_scroll.bounds_for_item(i)?;
                Some(f64::from(bounds.size.height.as_f32()))
            })
            .collect()
    }

    /// Left press on the divider under pane `upper`: starts dragging it,
    /// or on a double click makes its two panes equally high.
    pub(crate) fn divider_down(
        &mut self,
        upper: usize,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.dragging_with_another_button(MouseButton::Left) {
            return;
        }
        // Focused, so that Escape reaches the viewer.
        self.focus.focus(window, cx);
        let min = f64::from((window.rem_size() * MIN_PANE_HEIGHT).as_f32());
        let weights = heights::weights(&self.setup);
        let drag = self
            .painted_heights()
            .and_then(|painted| DividerDrag::start(&weights, &painted, upper, min));
        if event.click_count >= 2 {
            // From the weights a drag would start from, so a pane held at
            // its minimum does not make a third pane move.
            self.end_drag();
            self.edit_setup(cx, |s| match &drag {
                Some(drag) => heights::store(s, &drag.equalized()),
                None => heights::equalize(s, upper),
            });
            cx.notify();
            return;
        }
        let Some(drag) = drag else {
            return;
        };
        self.begin_drag(
            Drag::Divider {
                upper,
                start: event.position.y,
            },
            MouseButton::Left,
        );
        self.view.resize = Some(PaneResize {
            live: drag.start_weights().to_vec(),
            drag,
            moved: false,
        });
        cx.notify();
    }

    /// The divider drag in progress follows the pointer, `dy` below the
    /// press.
    pub(super) fn move_divider(&mut self, dy: Pixels) {
        if let Some(resize) = &mut self.view.resize {
            let (live, moved) = resize.drag.weights_at(f64::from(dy.as_f32()));
            resize.live = live;
            resize.moved = moved;
        }
    }

    /// Ends a divider drag by storing the heights it reached, when the
    /// divider moved: one setup change per drag. A drag outlived by its
    /// panes (a probe added one meanwhile) stores nothing.
    pub(super) fn commit_resize(&mut self, cx: &mut Context<Self>) {
        if let Some(resize) = self.view.resize.take()
            && resize.moved
            && resize.live.len() == self.setup.panes.len()
        {
            self.edit_setup(cx, |s| heights::store(s, &resize.live));
        }
    }

    /// The pointer entered or left the divider under pane `upper`.
    fn divider_hovered(&mut self, upper: usize, hovered: bool, cx: &mut Context<Self>) {
        let next = match self.view.divider_hover {
            _ if hovered => Some(upper),
            Some(current) if current == upper => None,
            current => current,
        };
        if next != self.view.divider_hover {
            self.view.divider_hover = next;
            cx.notify();
        }
    }
}
