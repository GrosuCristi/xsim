//! Plot colours and sizes from the active GPUI Kit theme (Q21, Q25).

use crate::model::palette::{Color, PlotColors};
use gpui_kit::component::Theme;
use gpui_kit::{Hsla, Pixels, Rgba, Window};

/// Snapshot of the theme's colours for the plot and the exporters.
pub fn plot_colors(theme: &Theme) -> PlotColors {
    let dark = theme.is_dark();
    PlotColors {
        background: color(theme.background),
        foreground: color(theme.foreground),
        muted: color(theme.muted_foreground),
        grid: color(theme.border).with_alpha(if dark { 0.9 } else { 1.0 }),
        grid_minor: color(theme.border).with_alpha(0.45),
        frame: color(theme.border),
        cursor: color(theme.foreground),
        crosshair: color(theme.muted_foreground),
        selection: color(theme.chart_2).with_alpha(0.14),
        unavailable: color(theme.muted_foreground),
        series: [
            theme.chart_2,
            theme.red,
            theme.green,
            theme.magenta,
            theme.yellow,
            theme.cyan,
        ]
        .into_iter()
        .map(color)
        .collect(),
    }
}

/// A GPUI colour as a plot colour.
pub fn color(hsla: Hsla) -> Color {
    let rgba = Rgba::from(hsla);
    let channel = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color {
        r: channel(rgba.r),
        g: channel(rgba.g),
        b: channel(rgba.b),
        a: rgba.a,
    }
}

/// A plot colour as a GPUI colour.
pub fn hsla(color: Color) -> Hsla {
    Rgba {
        r: f32::from(color.r) / 255.0,
        g: f32::from(color.g) / 255.0,
        b: f32::from(color.b) / 255.0,
        a: color.a,
    }
    .into()
}

/// Tick label size: three quarters of the rem, so it follows the UI scale
/// setting (Q30).
pub fn label_size(window: &Window) -> f32 {
    let rem: Pixels = window.rem_size();
    (rem.as_f32() * 0.75).round()
}
