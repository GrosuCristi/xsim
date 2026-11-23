//! The slim strip under the canvas: what the active tool expects next, and
//! the cursor position and zoom.

use crate::{SchematicEditor, Tool};
use gpui_kit::{component::ActiveTheme, prelude::*, *};

impl SchematicEditor {
    /// The hint for the current tool and state.
    pub(crate) fn hint(&self) -> SharedString {
        if let Some(notice) = &self.notice {
            return notice.clone();
        }
        let probing = self.probe.is_some();
        let text = match self.tool {
            _ if self.read_only && probing => {
                "Read-only · Click a wire or pin to plot its voltage · Click a part for its current"
            }
            _ if self.read_only => "Read-only schematic · Wheel to zoom · Middle-drag to pan",
            Tool::Select if self.interaction.wire.is_some() => {
                "Click to add a corner · Click a pin or wire to finish · Enter finishes · Alt for any angle · Esc cancels"
            }
            Tool::Select if probing => {
                "Click a wire or pin to plot its voltage · Drag the probe to another net for the difference · Click a part for its current"
            }
            Tool::Select if self.schematic.is_empty() => {
                "Pick a part in the toolbar, then click the grid to place it"
            }
            Tool::Select if !self.selection.is_empty() => {
                "Drag to move · R rotate · X/Y mirror · Delete removes"
            }
            Tool::Select => {
                "Click to select · Click a pin to start a wire · Drag to box-select · Wheel to zoom · Middle-drag or Space+drag to pan"
            }
            Tool::Wire if self.interaction.wire.is_some() => {
                "Click to add a corner · Click a pin or wire to finish · Enter finishes · Alt for any angle · Esc cancels"
            }
            Tool::Wire => "Click a pin or grid point to start a wire",
            Tool::Label => "Click a wire or pin to name its net",
            Tool::Place(_) => "Click to place · R rotate · X/Y mirror · Esc to finish",
        };
        text.into()
    }

    /// `x 12  y −4` of the grid point under the mouse.
    fn cursor_text(&self) -> Option<String> {
        let p = self.interaction.pointer?.world.snapped();
        Some(format!("x {}   y {}", p.x, p.y))
    }
}

/// The status strip element.
pub fn status_bar(editor: &SchematicEditor, cx: &App) -> impl IntoElement + use<> {
    let theme = cx.theme();
    let zoom = format!(
        "{:.0}%",
        editor.view.zoom / xsim_project::SchematicView::default().zoom * 100.0
    );
    div()
        .debug_selector(|| "schematic-status".into())
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .h(px(26.0))
        .px_3()
        .flex_shrink_0()
        .border_t_1()
        .border_color(theme.border)
        .text_xs()
        .text_color(theme.muted_foreground)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(editor.hint()),
        )
        .child(
            div()
                .flex()
                .gap_3()
                .flex_shrink_0()
                .font_family(theme.mono_font_family.clone())
                .children(editor.cursor_text())
                .child(zoom),
        )
}
