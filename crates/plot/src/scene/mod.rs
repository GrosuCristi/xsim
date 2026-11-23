//! The window-independent drawing of the plot panes.
//!
//! [`layout::pane_scene`] turns traces, ranges and a pixel frame into grid
//! lines, labels and clipped, decimated polylines. The GPUI painter
//! (`ui::canvas`) and the SVG exporter (`export::svg`) both draw these
//! scenes, so what is copied or exported matches what is on screen.
//!
//! [`columns`] draws decimated columns that turn back upright, so narrow
//! spikes keep the trace's width (D20); [`stroke`] cuts runs where they turn
//! straight back, which GPUI's stroke would otherwise pinch, for the screen.

pub mod clip;
pub mod columns;
pub mod layout;
pub mod lod;
pub mod stroke;

use crate::model::palette::{Color, PlotColors};

/// An axis-aligned rectangle in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    /// Whether `point` lies inside (edges included).
    pub fn contains(&self, point: [f32; 2]) -> bool {
        (self.x..=self.right()).contains(&point[0]) && (self.y..=self.bottom()).contains(&point[1])
    }

    /// The rectangle grown by `amount` on every side.
    pub fn outset(&self, amount: f32) -> Self {
        Self::new(
            self.x - amount,
            self.y - amount,
            self.w + 2.0 * amount,
            self.h + 2.0 * amount,
        )
    }
}

/// Horizontal text alignment relative to a label's `x`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    Start,
    Middle,
    End,
}

/// A single-line text label; `y` is its vertical centre.
#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub anchor: Anchor,
    pub color: Color,
    pub size: f32,
    pub strong: bool,
}

/// A grid line at pixel `pos` (x for vertical lines, y for horizontal ones).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridLine {
    pub pos: f32,
    pub major: bool,
}

/// A trace drawn as runs of connected points; one-point runs are dots.
#[derive(Debug, Clone, PartialEq)]
pub struct Polyline {
    pub color: Color,
    pub width: f32,
    pub runs: Vec<Vec<[f32; 2]>>,
}

/// A short coloured line in front of a legend label.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Swatch {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub color: Color,
}

/// One laid-out pane.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PaneScene {
    /// The data area; traces are clipped to it.
    pub plot: Rect,
    pub x_grid: Vec<GridLine>,
    /// Horizontal grid lines, at the left axis's ticks.
    pub y_grid: Vec<GridLine>,
    /// Tick marks of the right y axis (D9), drawn inwards from the right
    /// edge of the plot; the grid follows the left axis only.
    pub y2_ticks: Vec<GridLine>,
    /// Length of a major `y2_ticks` mark in pixels; minor ones are half.
    pub tick_mark: f32,
    pub lines: Vec<Polyline>,
    pub labels: Vec<Label>,
    pub swatches: Vec<Swatch>,
}

/// Length of a major right-axis tick mark in label font sizes, so it
/// follows the UI scale; minor marks are half as long.
pub const TICK_MARK: f32 = 0.4;

/// A full image: stacked panes on a background.
#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    pub width: f32,
    pub height: f32,
    /// Background, grid and frame colours.
    pub colors: PlotColors,
    pub panes: Vec<PaneScene>,
}

/// Approximate rendered width of `text` at `size` pixels in a proportional
/// UI font; used to reserve label space before any text is shaped.
pub fn text_width(text: &str, size: f32) -> f32 {
    text.chars().map(char_em).sum::<f32>() * size
}

fn char_em(c: char) -> f32 {
    match c {
        'i' | 'l' | 'j' | '.' | ',' | ':' | '\'' | '|' | '(' | ')' | ' ' => 0.3,
        'm' | 'M' | 'W' | 'w' => 0.85,
        '0'..='9' => 0.56,
        c if c.is_uppercase() => 0.66,
        _ => 0.55,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_edges_and_containment() {
        let rect = Rect::new(10.0, 20.0, 30.0, 40.0);
        assert_eq!((rect.right(), rect.bottom()), (40.0, 60.0));
        assert!(rect.contains([10.0, 60.0]));
        assert!(!rect.contains([9.0, 30.0]));
        assert_eq!(rect.outset(1.0), Rect::new(9.0, 19.0, 32.0, 42.0));
    }

    #[test]
    fn wider_text_measures_wider() {
        assert!(text_width("-1.25 ms", 12.0) > text_width("0", 12.0));
        assert_eq!(text_width("", 12.0), 0.0);
    }
}
