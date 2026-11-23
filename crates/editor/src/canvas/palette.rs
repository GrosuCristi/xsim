//! Canvas colours, all taken from the active GPUI Kit theme (Q21, Q25).

use gpui_kit::{Hsla, component::Theme};

/// Colours used to paint the schematic.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    /// Grid dots, every one a snap point (D22).
    pub grid: Hsla,
    pub wire: Hsla,
    pub symbol: Hsla,
    pub pin_marker: Hsla,
    pub designator: Hsla,
    pub value: Hsla,
    pub label_text: Hsla,
    pub label_border: Hsla,
    pub label_fill: Hsla,
    pub selection: Hsla,
    pub selection_halo: Hsla,
    pub hover_halo: Hsla,
    pub ghost: Hsla,
    pub probe: Hsla,
    pub probe_halo: Hsla,
    pub box_fill: Hsla,
}

impl Palette {
    pub fn from_theme(theme: &Theme) -> Self {
        let accent = theme.blue;
        Self {
            grid: theme.muted_foreground.opacity(0.5),
            wire: theme.green,
            symbol: theme.foreground,
            pin_marker: theme.muted_foreground,
            designator: theme.foreground,
            value: theme.muted_foreground,
            label_text: theme.foreground,
            label_border: theme.muted_foreground,
            label_fill: theme.background,
            selection: accent,
            selection_halo: accent.opacity(0.18),
            hover_halo: accent.opacity(0.12),
            ghost: accent.opacity(0.75),
            probe: theme.yellow,
            probe_halo: theme.yellow.opacity(0.28),
            box_fill: accent.opacity(0.08),
        }
    }
}
