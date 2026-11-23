//! Plot colours, independent of GPUI so the SVG exporter and the tests can
//! use them. The viewer snapshots them from the active theme on every frame
//! (see `ui::theme`); [`PlotColors::default`] mirrors the default light theme.

use xsim_project::Trace;

/// An sRGB colour with straight (non-premultiplied) alpha.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    /// Opacity from 0 (transparent) to 1 (opaque).
    pub a: f32,
}

impl Color {
    /// An opaque colour from `0xRRGGBB`.
    pub const fn rgb(hex: u32) -> Self {
        Self {
            r: (hex >> 16) as u8,
            g: (hex >> 8) as u8,
            b: hex as u8,
            a: 1.0,
        }
    }

    /// The same colour with opacity `alpha`.
    pub fn with_alpha(self, alpha: f32) -> Self {
        Self {
            a: alpha.clamp(0.0, 1.0),
            ..self
        }
    }

    /// `0xRRGGBB`, dropping the alpha.
    pub fn to_u32(self) -> u32 {
        (u32::from(self.r) << 16) | (u32::from(self.g) << 8) | u32::from(self.b)
    }

    /// `#rrggbb` for SVG attributes.
    pub fn hex(self) -> String {
        format!("#{:06x}", self.to_u32())
    }
}

/// Every colour the plot draws with.
#[derive(Debug, Clone, PartialEq)]
pub struct PlotColors {
    pub background: Color,
    pub foreground: Color,
    /// Tick labels and secondary text.
    pub muted: Color,
    pub grid: Color,
    pub grid_minor: Color,
    /// Plot frame and axis lines.
    pub frame: Color,
    /// Cursor lines and their flags.
    pub cursor: Color,
    /// The hover crosshair.
    pub crosshair: Color,
    /// Fill of the drag-to-zoom box.
    pub selection: Color,
    /// Traces whose signals are missing from the result.
    pub unavailable: Color,
    /// Trace colours, indexed by a trace's palette slot (D11).
    pub series: Vec<Color>,
}

impl Default for PlotColors {
    fn default() -> Self {
        Self {
            background: Color::rgb(0xffffff),
            foreground: Color::rgb(0x0a0a0a),
            muted: Color::rgb(0x737373),
            grid: Color::rgb(0xe5e5e5),
            grid_minor: Color::rgb(0xf3f3f3),
            frame: Color::rgb(0xd4d4d4),
            cursor: Color::rgb(0x0a0a0a),
            crosshair: Color::rgb(0xa3a3a3),
            selection: Color::rgb(0x3b82f6).with_alpha(0.12),
            unavailable: Color::rgb(0xa3a3a3),
            series: [0x3b82f6, 0xdc2626, 0x16a34a, 0x9333ea, 0xca8a04, 0x0891b2]
                .into_iter()
                .map(Color::rgb)
                .collect(),
        }
    }
}

impl PlotColors {
    /// The colour of `trace`: its explicit colour when set, otherwise the
    /// palette entry of its pinned slot (D11), so removing another trace
    /// never changes it. `position` (the trace's index in its pane) stands
    /// in for a slot that was not assigned yet; the viewer assigns slots
    /// whenever it receives a setup.
    pub fn trace_color(&self, trace: &Trace, position: usize) -> Color {
        let slot = trace
            .slot
            .and_then(|slot| usize::try_from(slot).ok())
            .unwrap_or(position);
        match trace.color {
            Some(hex) => Color::rgb(hex),
            None if self.series.is_empty() => self.foreground,
            None => self.series[slot % self.series.len()],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips() {
        let color = Color::rgb(0x1677cf);
        assert_eq!((color.r, color.g, color.b), (0x16, 0x77, 0xcf));
        assert_eq!(color.hex(), "#1677cf");
        assert_eq!(color.to_u32(), 0x1677cf);
        assert_eq!(color.with_alpha(2.0).a, 1.0);
    }

    #[test]
    fn traces_cycle_through_the_palette_unless_coloured() {
        let colors = PlotColors::default();
        let trace = Trace::new("V(out)");
        assert_eq!(colors.trace_color(&trace, 0), colors.series[0]);
        assert_eq!(
            colors.trace_color(&trace, colors.series.len() + 1),
            colors.series[1]
        );
        let custom = Trace {
            color: Some(0x123456),
            ..trace.clone()
        };
        assert_eq!(colors.trace_color(&custom, 3), Color::rgb(0x123456));
    }

    #[test]
    fn a_pinned_slot_wins_over_the_position() {
        let colors = PlotColors::default();
        let pinned = Trace {
            slot: Some(2),
            ..Trace::new("V(out)")
        };
        assert_eq!(colors.trace_color(&pinned, 0), colors.series[2]);
        assert_eq!(colors.trace_color(&pinned, 5), colors.series[2]);
        let wrapped = Trace {
            slot: Some(7),
            ..Trace::new("V(out)")
        };
        assert_eq!(colors.trace_color(&wrapped, 0), colors.series[1]);
        let custom = Trace {
            color: Some(0xabcdef),
            ..pinned
        };
        assert_eq!(colors.trace_color(&custom, 0), Color::rgb(0xabcdef));
    }
}
