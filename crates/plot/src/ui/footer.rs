//! The strip at the very bottom of the plot dock (D22): the "Add pane"
//! plus. It sits below the last pane and the cursor readout, outside the
//! scrolling pane stack, so it never covers a pane's x labels and stays in
//! view however many panes there are.
//!
//! The plus keeps the toolbar's left inset rather than following the
//! panes' y-label gutter: the gutter changes width as panes are added, and
//! a plus that moved under the pointer would swallow the next click.

use crate::WaveformViewer;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, IconName, Sizable, h_flex};
use gpui_kit::{prelude::*, *};

/// Height of the strip, in rems so it follows the UI scale (Q30).
const HEIGHT: f32 = 2.0;

/// The strip with the plus.
pub fn footer(viewer: &WaveformViewer, cx: &mut Context<WaveformViewer>) -> impl IntoElement {
    h_flex()
        .id("waveform-footer")
        .debug_selector(|| "waveform-footer".into())
        .h(rems(HEIGHT))
        .flex_shrink_0()
        .px_2()
        .border_t_1()
        .border_color(cx.theme().border)
        .child(add_pane_button(viewer, cx))
}

/// The plus: a ghost icon button like the toolbar's, focusable, and
/// pressed with Enter or Space as well as the mouse.
fn add_pane_button(viewer: &WaveformViewer, cx: &mut Context<WaveformViewer>) -> impl IntoElement {
    div().debug_selector(|| "add-pane".into()).child(
        Button::new("add-pane")
            .ghost()
            .small()
            .icon(IconName::Plus)
            .tooltip("Add pane")
            .accessibility_label("Add pane")
            .track_focus(&viewer.add_pane_focus)
            .on_click(cx.listener(|this, _, _, cx| this.add_pane(cx))),
    )
}
