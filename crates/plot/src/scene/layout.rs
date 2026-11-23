//! Lays out one pane (on screen every pane is its own element) or a stack
//! of panes (image export).
//!
//! No axis has a title that only names a quantity and its unit ("Voltage
//! (V)", "Time (s)"): every tick label carries the unit (D20). Every pane
//! shows the x tick labels, so each one reads on its own; the panes share
//! the x range and the gutters, so their x axes line up. Only a sweep's x
//! axis keeps a title, the swept source's name, under the bottom pane.
//!
//! A pane whose traces have two units (D9) has a second, independently
//! scaled y axis on the right: its tick labels sit right of the plot, short
//! tick marks point at them from the plot edge (the grid follows the left
//! axis only), and on exports the legend of its traces ends above it, apart
//! from the left axis's legend, which starts above the left axis.

use super::clip::clip_polyline;
use super::columns::{Columns, upright_reversals};
use super::lod::{LodCache, LodKey};
use super::{
    Anchor, GridLine, Label, PaneScene, Polyline, Rect, Scene, Swatch, TICK_MARK, text_width,
};
use crate::model::axis::{AxisMap, AxisRange};
use crate::model::palette::{Color, PlotColors};
use crate::model::ticks::{Tick, axis_ticks};
use crate::model::traces::PlotValues;
use crate::model::units::YAxis;
use std::sync::Arc;

/// The smallest space right of the plot, so the last x label is not cut
/// off.
pub const RIGHT_GUTTER: f32 = 18.0;
/// Gap between a tick label and the plot frame.
const LABEL_GAP: f32 = 6.0;
/// Margin around an exported image.
const IMAGE_MARGIN: f32 = 8.0;
/// Length of a legend swatch.
const SWATCH: f32 = 16.0;
/// Space after a legend entry, in label font sizes.
const ENTRY_GAP: f32 = 1.5;
/// The least space between the left axis's legend and the right axis's,
/// in label font sizes, when they do not fit above their own axes.
const GROUP_GAP: f32 = 4.0;

/// The shared x axis.
#[derive(Debug, Clone)]
pub struct SceneX<'a> {
    /// Sample positions of every trace.
    pub values: &'a [f64],
    pub range: AxisRange,
    pub unit: &'a str,
    /// Title under the bottom pane: a sweep's source name (`"V1"`), or
    /// empty for time and frequency, whose tick labels carry the unit.
    pub title: String,
}

/// A plotted trace.
#[derive(Debug, Clone)]
pub struct SceneTrace {
    pub color: Color,
    pub values: Arc<PlotValues>,
    /// The y axis the trace is drawn against.
    pub axis: YAxis,
}

/// A legend entry of an exported pane.
#[derive(Debug, Clone, PartialEq)]
pub struct LegendEntry {
    pub label: String,
    pub color: Color,
    /// The y axis the trace is drawn against.
    pub axis: YAxis,
}

/// The right y axis of a pane that plots a second unit (D9).
#[derive(Debug, Clone, PartialEq)]
pub struct SceneAxis {
    pub range: AxisRange,
    /// Unit of the tick labels.
    pub unit: String,
}

/// One pane's content.
#[derive(Debug, Clone)]
pub struct ScenePane {
    /// Visible traces that could be evaluated, in drawing order.
    pub traces: Vec<SceneTrace>,
    /// The left y axis.
    pub y: AxisRange,
    /// Unit of the left y tick labels.
    pub y_unit: String,
    /// Drawn above the plot when the frame asks for a legend.
    pub legend: Vec<LegendEntry>,
    /// The right y axis, when the pane plots a second unit.
    pub y2: Option<SceneAxis>,
}

/// Fonts, strokes and colours.
#[derive(Debug, Clone, Copy)]
pub struct Style<'a> {
    pub colors: &'a PlotColors,
    /// Label size in pixels.
    pub font_size: f32,
    /// Trace stroke width in pixels.
    pub line_width: f32,
}

impl Style<'_> {
    /// Height above an exported plot, which holds its legend.
    pub fn top_gutter(&self) -> f32 {
        (self.font_size * 1.9).round()
    }

    /// Height below a plot: its x tick labels, then the x title when
    /// `title`.
    pub fn bottom_gutter(&self, title: bool) -> f32 {
        let labels = (self.font_size * 1.5 + LABEL_GAP).round();
        if title {
            labels + (self.font_size * 1.5).round()
        } else {
            labels
        }
    }

    fn x_spacing(&self) -> f32 {
        self.font_size * 7.0
    }

    fn y_spacing(&self) -> f32 {
        self.font_size * 3.2
    }
}

/// Where a pane goes and what it shows besides the traces.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaneFrame {
    pub rect: Rect,
    /// Height above the plot (the legend on exports).
    pub top: f32,
    /// Width left of the plot for y labels, shared by all panes so their x
    /// axes line up.
    pub left: f32,
    /// Width right of the plot: room for the last x label and for right-axis
    /// labels ([`y2_gutter`]), shared by all panes like `left`.
    pub right: f32,
    /// Make room for the x title under the x tick labels, and draw it when
    /// there is one: the bottom pane of a sweep.
    pub x_title: bool,
    /// Draw the legend above the plot (exports; the screen has chips).
    pub legend: bool,
}

/// The data area of `frame`.
pub fn plot_rect(frame: &PaneFrame, style: &Style) -> Rect {
    let top = frame.top;
    let bottom = style.bottom_gutter(frame.x_title);
    Rect::new(
        frame.rect.x + frame.left,
        frame.rect.y + top,
        (frame.rect.w - frame.left - frame.right).max(1.0),
        (frame.rect.h - top - bottom).max(1.0),
    )
}

/// The left gutter `pane` needs for its y labels when its plot is
/// `plot_height` pixels high.
pub fn y_gutter(pane: &ScenePane, plot_height: f32, style: &Style) -> f32 {
    label_gutter(pane.y, &pane.y_unit, plot_height, style)
}

/// The right gutter `pane` needs: room for its right-axis labels, or
/// [`RIGHT_GUTTER`] without a right axis.
pub fn y2_gutter(pane: &ScenePane, plot_height: f32, style: &Style) -> f32 {
    match &pane.y2 {
        Some(axis) => label_gutter(axis.range, &axis.unit, plot_height, style).max(RIGHT_GUTTER),
        None => RIGHT_GUTTER,
    }
}

fn label_gutter(range: AxisRange, unit: &str, plot_height: f32, style: &Style) -> f32 {
    let widest = axis_ticks(range, unit, plot_height, style.y_spacing())
        .iter()
        .filter_map(|t| t.label.as_deref())
        .map(|label| text_width(label, style.font_size))
        .fold(0.0, f32::max);
    (widest + LABEL_GAP + 8.0).max(style.font_size * 3.5).ceil()
}

/// Lays out one pane inside `frame`.
pub fn pane_scene(
    x: &SceneX,
    pane: &ScenePane,
    frame: &PaneFrame,
    style: &Style,
    lod: &mut LodCache,
) -> PaneScene {
    let plot = plot_rect(frame, style);
    let x_map = AxisMap::new(x.range, plot.x, plot.right());
    let y_map = AxisMap::new(pane.y, plot.bottom(), plot.y);
    let x_ticks = axis_ticks(x.range, x.unit, plot.w, style.x_spacing());
    let y_ticks = axis_ticks(pane.y, &pane.y_unit, plot.h, style.y_spacing());
    let mut scene = PaneScene {
        plot,
        x_grid: grid(&x_ticks, &x_map, plot.x, plot.right()),
        y_grid: grid(&y_ticks, &y_map, plot.y, plot.bottom()),
        tick_mark: (TICK_MARK * style.font_size).round(),
        ..PaneScene::default()
    };
    scene
        .labels
        .extend(y_labels(&y_ticks, &y_map, plot, style, YAxis::Left));
    let y2_map = pane.y2.as_ref().map(|axis| {
        let map = AxisMap::new(axis.range, plot.bottom(), plot.y);
        let ticks = axis_ticks(axis.range, &axis.unit, plot.h, style.y_spacing());
        scene.y2_ticks = grid(&ticks, &map, plot.y, plot.bottom());
        scene
            .labels
            .extend(y_labels(&ticks, &map, plot, style, YAxis::Right));
        map
    });
    scene
        .labels
        .extend(x_labels(&x_ticks, &x_map, frame.rect, plot, style));
    if frame.x_title && !x.title.is_empty() {
        scene.labels.push(x_title(x, plot, style));
    }
    if frame.legend {
        legend(&mut scene, pane, frame, style);
    }
    scene.lines = pane
        .traces
        .iter()
        .map(|trace| {
            let map = match (trace.axis, &y2_map) {
                (YAxis::Right, Some(map)) => map,
                _ => &y_map,
            };
            polyline(trace, x, &x_map, map, plot, style, lod)
        })
        .collect();
    scene
}

/// Panes stacked in a `width`×`height` image with legends, for export.
pub fn stacked_scene(
    x: &SceneX,
    panes: &[ScenePane],
    width: f32,
    height: f32,
    style: &Style,
    lod: &mut LodCache,
) -> Scene {
    let frames = stack_frames(panes, !x.title.is_empty(), width, height, style);
    Scene {
        width,
        height,
        colors: style.colors.clone(),
        panes: panes
            .iter()
            .zip(&frames)
            .map(|(pane, frame)| pane_scene(x, pane, frame, style, lod))
            .collect(),
    }
}

/// Frames splitting the image height so every plot gets the same height;
/// the bottom one also holds the x title when `x_title`.
fn stack_frames(
    panes: &[ScenePane],
    x_title: bool,
    width: f32,
    height: f32,
    style: &Style,
) -> Vec<PaneFrame> {
    let count = panes.len().max(1) as f32;
    let inner = Rect::new(
        IMAGE_MARGIN,
        IMAGE_MARGIN,
        width - 2.0 * IMAGE_MARGIN,
        height - 2.0 * IMAGE_MARGIN,
    );
    let gutters = count * (style.top_gutter() + style.bottom_gutter(false))
        + (style.bottom_gutter(x_title) - style.bottom_gutter(false));
    let plot_height = ((inner.h - gutters) / count).max(1.0);
    let left = panes
        .iter()
        .map(|pane| y_gutter(pane, plot_height, style))
        .fold(0.0, f32::max);
    let right = panes
        .iter()
        .map(|pane| y2_gutter(pane, plot_height, style))
        .fold(RIGHT_GUTTER, f32::max);
    let mut y = inner.y;
    (0..panes.len())
        .map(|index| {
            let title = x_title && index + 1 == panes.len();
            let h = style.top_gutter() + style.bottom_gutter(title) + plot_height;
            let frame = PaneFrame {
                rect: Rect::new(inner.x, y, inner.w, h),
                top: style.top_gutter(),
                left,
                right,
                x_title: title,
                legend: true,
            };
            y += h;
            frame
        })
        .collect()
}

/// Grid lines of the ticks that fall inside `start..=end`.
fn grid(ticks: &[Tick], map: &AxisMap, start: f32, end: f32) -> Vec<GridLine> {
    let (lo, hi) = (start.min(end) - 0.5, start.max(end) + 0.5);
    ticks
        .iter()
        .map(|t| GridLine {
            pos: map.px(t.value),
            major: t.major,
        })
        .filter(|line| (lo..=hi).contains(&line.pos))
        .collect()
}

/// Y tick labels: right-aligned left of the plot for the left axis,
/// left-aligned right of it for the right axis.
fn y_labels(ticks: &[Tick], map: &AxisMap, plot: Rect, style: &Style, axis: YAxis) -> Vec<Label> {
    let (x, anchor) = match axis {
        YAxis::Left => (plot.x - LABEL_GAP, Anchor::End),
        YAxis::Right => (plot.right() + LABEL_GAP, Anchor::Start),
    };
    ticks
        .iter()
        .filter_map(|t| Some((t.label.clone()?, map.px(t.value))))
        .filter(|(_, y)| (plot.y - 0.5..=plot.bottom() + 0.5).contains(y))
        .map(|(text, y)| Label {
            text,
            x,
            y,
            anchor,
            color: style.colors.muted,
            size: style.font_size,
            strong: false,
        })
        .collect()
}

/// X tick labels centred under their lines, skipping any that would overlap
/// the previous one or leave the frame.
fn x_labels(ticks: &[Tick], map: &AxisMap, frame: Rect, plot: Rect, style: &Style) -> Vec<Label> {
    let y = plot.bottom() + LABEL_GAP + style.font_size * 0.6;
    let mut right_edge = f32::NEG_INFINITY;
    let mut labels = Vec::new();
    for tick in ticks {
        let Some(text) = tick.label.clone() else {
            continue;
        };
        let x = map.px(tick.value);
        let half = text_width(&text, style.font_size) / 2.0;
        let inside = x - half >= frame.x && x + half <= frame.right();
        let in_plot = (plot.x - 0.5..=plot.right() + 0.5).contains(&x);
        if !inside || !in_plot || x - half < right_edge + style.font_size {
            continue;
        }
        right_edge = x + half;
        labels.push(Label {
            text,
            x,
            y,
            anchor: Anchor::Middle,
            color: style.colors.muted,
            size: style.font_size,
            strong: false,
        });
    }
    labels
}

/// The x title, centred under the x tick labels.
fn x_title(x: &SceneX, plot: Rect, style: &Style) -> Label {
    Label {
        text: x.title.clone(),
        x: plot.x + plot.w / 2.0,
        y: plot.bottom() + LABEL_GAP + style.font_size * 2.1,
        anchor: Anchor::Middle,
        color: style.colors.foreground,
        size: style.font_size,
        strong: false,
    }
}

/// The legend row above an exported plot: the left axis's traces from the
/// plot's left edge, above the left axis; a right axis's traces ending at
/// the plot's right edge, above that axis (D9). When the row is too narrow
/// for that, the right group follows the left one after a wide gap
/// ([`GROUP_GAP`]), so the two groups stay apart.
fn legend(scene: &mut PaneScene, pane: &ScenePane, frame: &PaneFrame, style: &Style) {
    let y = frame.rect.y + frame.top / 2.0;
    let on = |axis: YAxis| {
        pane.legend
            .iter()
            .filter(move |e| e.axis == axis || pane.y2.is_none())
    };
    let mut x = scene.plot.x;
    for entry in on(YAxis::Left) {
        x = legend_entry(scene, entry, x, y, style);
    }
    if pane.y2.is_none() {
        return;
    }
    let right: Vec<&LegendEntry> = on(YAxis::Right).collect();
    // Without the gap after the group's last entry.
    let width =
        right.iter().map(|e| entry_width(e, style)).sum::<f32>() - ENTRY_GAP * style.font_size;
    let after_left = x + (GROUP_GAP - ENTRY_GAP) * style.font_size;
    let mut x = (scene.plot.right() - width).max(after_left);
    for entry in right {
        x = legend_entry(scene, entry, x, y, style);
    }
}

/// Width of a legend entry including the gap after it.
fn entry_width(entry: &LegendEntry, style: &Style) -> f32 {
    SWATCH + 5.0 + text_width(&entry.label, style.font_size) + ENTRY_GAP * style.font_size
}

/// Draws a legend entry at `x`; returns where the next one starts.
fn legend_entry(scene: &mut PaneScene, entry: &LegendEntry, x: f32, y: f32, style: &Style) -> f32 {
    scene.swatches.push(Swatch {
        x,
        y,
        width: SWATCH,
        color: entry.color,
    });
    scene.labels.push(Label {
        text: entry.label.clone(),
        x: x + SWATCH + 5.0,
        y,
        anchor: Anchor::Start,
        color: style.colors.foreground,
        size: style.font_size,
        strong: false,
    });
    x + entry_width(entry, style)
}

/// The trace decimated to the plot width, mapped to pixels, with columns
/// that turn back drawn upright ([`super::columns`]), and clipped.
fn polyline(
    trace: &SceneTrace,
    x: &SceneX,
    x_map: &AxisMap,
    y_map: &AxisMap,
    plot: Rect,
    style: &Style,
    lod: &mut LodCache,
) -> Polyline {
    let key = LodKey {
        trace: trace.values.id,
        x_min: x.range.min,
        x_max: x.range.max,
        columns: plot.w.ceil() as usize,
        log_x: x.range.log,
    };
    let vertices = lod.vertices(key, x.values, &trace.values.y);
    let mut points: Vec<[f64; 2]> = vertices
        .iter()
        .map(|v| [x_map.px_f64(v.x), y_map.px_f64(v.y)])
        .collect();
    if let Some(columns) = Columns::new(x.range, key.columns.max(1)) {
        let column_of: Vec<Option<usize>> = vertices
            .iter()
            .map(|v| columns.index(v.x).filter(|_| !v.is_break()))
            .collect();
        upright_reversals(&mut points, &column_of);
    }
    // An upright column repeats a point where two samples are equal.
    points.dedup();
    Polyline {
        color: trace.color,
        width: style.line_width,
        runs: clip_polyline(&points, plot.outset(style.line_width)),
    }
}

/// The length of the right-axis tick mark `line` of `scene`.
pub fn tick_mark_length(scene: &PaneScene, line: &GridLine) -> f32 {
    if line.major {
        scene.tick_mark
    } else {
        scene.tick_mark / 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::traces::plot_values;
    use crate::test_fixtures;
    use xsim_project::YScale;
    use xsim_spice_engine::SignalData;
    use xsim_waveform::Series;

    fn style(colors: &PlotColors) -> Style<'_> {
        Style {
            colors,
            font_size: 12.0,
            line_width: 1.5,
        }
    }

    fn trace(values: Vec<f64>, unit: &str, axis: YAxis, color: u32) -> SceneTrace {
        SceneTrace {
            color: Color::rgb(color),
            values: Arc::new(plot_values(&Series::Real(values), unit, YScale::Linear)),
            axis,
        }
    }

    fn entry(label: &str, color: u32, axis: YAxis) -> LegendEntry {
        LegendEntry {
            label: label.into(),
            color: Color::rgb(color),
            axis,
        }
    }

    /// A voltage on the left axis (0…1 V) and a current on the right one
    /// (0…2 mA).
    fn mixed() -> ScenePane {
        ScenePane {
            traces: vec![
                trace(vec![0.0, 0.5, 1.0], "V", YAxis::Left, 0x111111),
                trace(vec![2e-3, 1e-3, 0.0], "A", YAxis::Right, 0x222222),
            ],
            y: AxisRange::new(0.0, 1.0, false).unwrap(),
            y_unit: "V".into(),
            legend: vec![
                entry("V(out)", 0x111111, YAxis::Left),
                entry("I(R1)", 0x222222, YAxis::Right),
            ],
            y2: Some(SceneAxis {
                range: AxisRange::new(0.0, 2e-3, false).unwrap(),
                unit: "A".into(),
            }),
        }
    }

    fn x_axis(values: &[f64]) -> SceneX<'_> {
        SceneX {
            values,
            range: AxisRange::new(0.0, 2.0, false).unwrap(),
            unit: "s",
            title: String::new(),
        }
    }

    fn frame(right: f32) -> PaneFrame {
        PaneFrame {
            rect: Rect::new(0.0, 0.0, 600.0, 300.0),
            top: 24.0,
            left: 60.0,
            right,
            x_title: false,
            legend: true,
        }
    }

    fn label<'a>(scene: &'a PaneScene, text: &str) -> &'a Label {
        scene
            .labels
            .iter()
            .find(|l| l.text == text)
            .unwrap_or_else(|| panic!("{text} is drawn: {:?}", scene.labels))
    }

    #[test]
    fn the_right_axis_has_its_own_unit_labelled_ticks() {
        let colors = PlotColors::default();
        let style = style(&colors);
        let pane = mixed();
        let right = y2_gutter(&pane, 200.0, &style);
        assert!(right > RIGHT_GUTTER, "room for the current labels: {right}");
        let xs = [0.0, 1.0, 2.0];
        let scene = pane_scene(
            &x_axis(&xs),
            &pane,
            &frame(right),
            &style,
            &mut LodCache::default(),
        );
        let plot = scene.plot;
        assert_eq!(plot.right(), 600.0 - right);
        let right_labels: Vec<&Label> = scene
            .labels
            .iter()
            .filter(|l| l.anchor == Anchor::Start && l.x > plot.right())
            .collect();
        assert!(right_labels.len() >= 2, "{:?}", scene.labels);
        assert!(right_labels.iter().all(|l| l.text.ends_with('A')));
        assert!(right_labels.iter().any(|l| l.text == "1 mA"));
        let left_labels: Vec<&Label> = scene
            .labels
            .iter()
            .filter(|l| l.anchor == Anchor::End && l.x < plot.x)
            .collect();
        assert!(left_labels.iter().all(|l| l.text.ends_with('V')));
        assert!(!scene.y2_ticks.is_empty());
        assert!(
            scene
                .y2_ticks
                .iter()
                .all(|t| (plot.y - 0.5..=plot.bottom() + 0.5).contains(&t.pos))
        );
        assert!(scene.y_grid.len() >= 2, "the grid follows the left axis");
    }

    #[test]
    fn right_axis_traces_are_scaled_by_the_right_axis() {
        let colors = PlotColors::default();
        let style = style(&colors);
        let pane = mixed();
        let xs = [0.0, 1.0, 2.0];
        let scene = pane_scene(
            &x_axis(&xs),
            &pane,
            &frame(60.0),
            &style,
            &mut LodCache::default(),
        );
        let plot = scene.plot;
        let close = |a: f32, b: f32| (a - b).abs() < 0.05;
        // 0 → 1 V rises across the full left axis; 2 → 0 mA falls across
        // the full right axis, so a milliamp trace is not a flat line.
        let voltage = &scene.lines[0].runs[0];
        assert!(close(voltage[0][1], plot.bottom()) && close(voltage[2][1], plot.y));
        let current = &scene.lines[1].runs[0];
        assert!(close(current[0][1], plot.y) && close(current[2][1], plot.bottom()));
    }

    /// D20: no label names only a quantity and its unit; every text is a
    /// tick label with its unit or a legend entry.
    #[test]
    fn no_axis_has_a_unit_title() {
        let colors = PlotColors::default();
        let style = style(&colors);
        let xs = [0.0, 1.0, 2.0];
        let scene = pane_scene(
            &x_axis(&xs),
            &mixed(),
            &frame(60.0),
            &style,
            &mut LodCache::default(),
        );
        for label in &scene.labels {
            let text = label.text.as_str();
            let tick = ['V', 'A', 's'].iter().any(|unit| text.ends_with(*unit))
                && text.starts_with(|c: char| c.is_ascii_digit() || c == '-');
            assert!(
                tick || text == "V(out)" || text == "I(R1)",
                "unexpected label {text:?}"
            );
            assert!(!text.contains(" ("), "a unit title: {text:?}");
        }
    }

    /// D9 without titles: the left axis's traces start above the left axis,
    /// the right axis's end above the right axis.
    #[test]
    fn the_legend_groups_traces_above_their_axes() {
        let colors = PlotColors::default();
        let style = style(&colors);
        let xs = [0.0, 1.0, 2.0];
        let scene = pane_scene(
            &x_axis(&xs),
            &mixed(),
            &frame(60.0),
            &style,
            &mut LodCache::default(),
        );
        let plot = scene.plot;
        assert_eq!(
            scene.swatches[0].x, plot.x,
            "the left group starts at the left axis"
        );
        let current = label(&scene, "I(R1)");
        let end = current.x + text_width("I(R1)", 12.0);
        assert!(
            (end - plot.right()).abs() < 0.01,
            "the right group ends at the right axis: {end} vs {}",
            plot.right()
        );
        assert!(label(&scene, "V(out)").x < scene.swatches[1].x);
    }

    /// A row too narrow for both groups above their axes keeps them apart.
    #[test]
    fn a_crowded_legend_keeps_the_groups_apart() {
        let colors = PlotColors::default();
        let style = style(&colors);
        let mut pane = mixed();
        pane.legend = vec![
            entry("V(a_long_node_name)", 0x111111, YAxis::Left),
            entry("V(another_long_node)", 0x333333, YAxis::Left),
            entry("I(R1)", 0x222222, YAxis::Right),
            entry("I(R2)", 0x444444, YAxis::Right),
        ];
        let xs = [0.0, 1.0, 2.0];
        let narrow = PaneFrame {
            rect: Rect::new(0.0, 0.0, 420.0, 300.0),
            ..frame(60.0)
        };
        let scene = pane_scene(
            &x_axis(&xs),
            &pane,
            &narrow,
            &style,
            &mut LodCache::default(),
        );
        let left_end = label(&scene, "V(another_long_node)").x
            + text_width("V(another_long_node)", style.font_size);
        let right_start = scene.swatches[2].x;
        assert!(
            right_start - left_end >= GROUP_GAP * style.font_size - 0.01,
            "{right_start} - {left_end}"
        );
        assert!(
            right_start + SWATCH > scene.plot.right(),
            "the row is crowded"
        );
        // Entries of one group keep the usual spacing.
        let gap = scene.swatches[3].x - (label(&scene, "I(R1)").x + text_width("I(R1)", 12.0));
        assert!((gap - ENTRY_GAP * style.font_size).abs() < 0.01, "{gap}");
    }

    #[test]
    fn panes_without_a_second_unit_keep_the_narrow_right_gutter() {
        let colors = PlotColors::default();
        let pane = ScenePane {
            y2: None,
            ..mixed()
        };
        assert_eq!(y2_gutter(&pane, 200.0, &style(&colors)), RIGHT_GUTTER);
    }

    fn x_labels_of(scene: &PaneScene) -> Vec<(String, f32, f32)> {
        scene
            .labels
            .iter()
            .filter(|l| l.anchor == Anchor::Middle)
            .map(|l| (l.text.clone(), l.x, l.y))
            .collect()
    }

    /// D20: every pane of an export shows the same x tick labels at the
    /// same x, each under its own plot.
    #[test]
    fn every_exported_pane_shows_the_x_labels() {
        let colors = PlotColors::default();
        let style = style(&colors);
        let xs = [0.0, 1.0, 2.0];
        let panes = [mixed(), mixed(), mixed()];
        let scene = stacked_scene(
            &x_axis(&xs),
            &panes,
            800.0,
            900.0,
            &style,
            &mut LodCache::default(),
        );
        let labels: Vec<Vec<(String, f32, f32)>> = scene.panes.iter().map(x_labels_of).collect();
        assert!(labels[0].len() >= 3, "{:?}", labels[0]);
        assert!(labels[0].iter().any(|(text, ..)| text == "1 s"));
        for (pane, (scene_pane, pane_labels)) in scene.panes.iter().zip(&labels).enumerate() {
            let texts_and_x = |l: &[(String, f32, f32)]| -> Vec<(String, f32)> {
                l.iter().map(|(t, x, _)| (t.clone(), *x)).collect()
            };
            assert_eq!(
                texts_and_x(pane_labels),
                texts_and_x(&labels[0]),
                "pane {pane}"
            );
            let plot = scene_pane.plot;
            assert_eq!(
                (plot.x, plot.w),
                (scene.panes[0].plot.x, scene.panes[0].plot.w)
            );
            for (_, _, y) in pane_labels {
                let below = y - plot.bottom();
                assert!(below > 0.0 && below + 0.6 * style.font_size <= style.bottom_gutter(false));
            }
        }
        // Every plot is as high as the others.
        let heights: Vec<f32> = scene.panes.iter().map(|p| p.plot.h).collect();
        assert!(
            heights.iter().all(|h| (h - heights[0]).abs() < 0.01),
            "{heights:?}"
        );
    }

    /// On screen each pane is laid out alone; the bottom one has no other
    /// x labels than the panes above it.
    #[test]
    fn panes_laid_out_alone_label_x_alike() {
        let colors = PlotColors::default();
        let style = style(&colors);
        let xs = [0.0, 1.0, 2.0];
        let lay_out = |frame: PaneFrame| {
            let scene = pane_scene(
                &x_axis(&xs),
                &mixed(),
                &frame,
                &style,
                &mut LodCache::default(),
            );
            let labels: Vec<(String, f32)> = x_labels_of(&scene)
                .into_iter()
                .map(|(t, x, _)| (t, x))
                .collect();
            labels
        };
        let top = lay_out(PaneFrame {
            legend: false,
            ..frame(60.0)
        });
        let bottom = lay_out(PaneFrame {
            rect: Rect::new(0.0, 300.0, 600.0, 200.0),
            legend: false,
            ..frame(60.0)
        });
        assert!(!top.is_empty());
        assert_eq!(top, bottom);
    }

    /// A sweep keeps its x title, the swept source's name, under the
    /// bottom pane only; time and frequency have none.
    #[test]
    fn only_a_sweep_has_an_x_title_under_the_bottom_pane() {
        let colors = PlotColors::default();
        let style = style(&colors);
        let xs = [0.0, 1.0, 2.0];
        let sweep = SceneX {
            unit: "V",
            title: "V1".into(),
            ..x_axis(&xs)
        };
        let panes = [mixed(), mixed()];
        let scene = stacked_scene(
            &sweep,
            &panes,
            800.0,
            600.0,
            &style,
            &mut LodCache::default(),
        );
        let titles = |p: &PaneScene| p.labels.iter().filter(|l| l.text == "V1").count();
        assert_eq!(titles(&scene.panes[0]), 0);
        assert_eq!(titles(&scene.panes[1]), 1);
        let title = label(&scene.panes[1], "V1");
        let tick_y = x_labels_of(&scene.panes[1])
            .iter()
            .filter(|(t, ..)| t != "V1")
            .map(|(_, _, y)| *y)
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(title.y > tick_y + style.font_size, "under the tick labels");
        assert!((scene.panes[0].plot.h - scene.panes[1].plot.h).abs() < 0.01);
        assert!(title.y + 0.6 * style.font_size < 600.0);
        // Without a title the bottom pane needs no more room than the others.
        let scene = stacked_scene(
            &x_axis(&xs),
            &panes,
            800.0,
            600.0,
            &style,
            &mut LodCache::default(),
        );
        let (top, bottom) = (scene.panes[0].plot, scene.panes[1].plot);
        assert!((top.h - bottom.h).abs() < 0.01);
        assert!(bottom.bottom() + style.bottom_gutter(false) <= 600.0 - IMAGE_MARGIN + 0.01);
    }

    /// D20: freed of the x title, the space under a plot holds only the
    /// tick labels.
    #[test]
    fn the_gutter_under_a_plot_fits_the_tick_labels() {
        let colors = PlotColors::default();
        for font_size in [10.0, 12.0, 24.0] {
            let style = Style {
                font_size,
                ..style(&colors)
            };
            let labels = style.bottom_gutter(false);
            // Label centre plus half a line, and a little room.
            assert!(labels >= LABEL_GAP + font_size * 1.2, "{labels}");
            assert!(labels <= LABEL_GAP + font_size * 1.6, "{labels}");
            assert!(style.bottom_gutter(true) > labels + font_size);
        }
    }

    /// D20: a 40 ns spike in a 5 ms view is drawn on one x, from the
    /// column's lowest to its highest sample, so its strokes up and down
    /// coincide.
    #[test]
    fn a_narrow_spike_is_drawn_upright() {
        let result = test_fixtures::spike();
        let SignalData::Real(values) = &result.signals[0].data else {
            panic!("a real signal");
        };
        let colors = PlotColors::default();
        let style = style(&colors);
        let x = SceneX {
            values: &result.axis.values,
            range: AxisRange::new(0.0, 5e-3, false).unwrap(),
            unit: "s",
            title: String::new(),
        };
        let pane = ScenePane {
            traces: vec![trace(values.clone(), "V", YAxis::Left, 0x111111)],
            y: AxisRange::new(-0.1, 2.4, false).unwrap(),
            y_unit: "V".into(),
            legend: Vec::new(),
            y2: None,
        };
        let frame = PaneFrame {
            rect: Rect::new(0.0, 0.0, 1000.0, 700.0),
            legend: false,
            ..frame(RIGHT_GUTTER)
        };
        let scene = pane_scene(&x, &pane, &frame, &style, &mut LodCache::default());
        let run = &scene.lines[0].runs[0];
        let top = run.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
        let tip = run
            .iter()
            .position(|p| p[1] == top)
            .expect("the tip is drawn");
        let column_x = run[tip][0];
        let plot = scene.plot;
        assert!((column_x - (plot.x + plot.w / 5.0)).abs() < 1.0, "at 1 ms");
        // Into the column at 0 V, up to the tip, down to 0.28 V: one x.
        assert_eq!(run[tip - 1][0], column_x);
        assert_eq!(run[tip + 1][0], column_x);
        let zero = AxisMap::new(pane.y, plot.bottom(), plot.y).px(0.0);
        assert!((run[tip - 1][1] - zero).abs() < 0.5);
        // Either side, the flat parts keep their own x.
        assert!(run[tip - 2][0] < column_x && run[tip + 2][0] > column_x);
        assert!(run.windows(2).all(|w| w[0] != w[1]), "no repeated point");
    }

    /// Review: zoomed in on y so the spike's tip is above the plot, the
    /// upright column is cut at the clip edge on both strokes: the run
    /// going up ends there and the run coming down starts there, on the
    /// column's one x.
    #[test]
    fn an_upright_spike_is_clipped_at_the_top_edge() {
        let result = test_fixtures::spike();
        let SignalData::Real(values) = &result.signals[0].data else {
            panic!("a real signal");
        };
        let colors = PlotColors::default();
        let style = style(&colors);
        let x = SceneX {
            values: &result.axis.values,
            range: AxisRange::new(0.0, 5e-3, false).unwrap(),
            unit: "s",
            title: String::new(),
        };
        let pane = ScenePane {
            traces: vec![trace(values.clone(), "V", YAxis::Left, 0x111111)],
            y: AxisRange::new(-0.1, 1.0, false).unwrap(),
            y_unit: "V".into(),
            legend: Vec::new(),
            y2: None,
        };
        let frame = PaneFrame {
            rect: Rect::new(0.0, 0.0, 1000.0, 700.0),
            legend: false,
            ..frame(RIGHT_GUTTER)
        };
        let scene = pane_scene(&x, &pane, &frame, &style, &mut LodCache::default());
        let plot = scene.plot;
        let clip = plot.outset(style.line_width);
        let runs = &scene.lines[0].runs;
        assert_eq!(runs.len(), 2, "cut once, at the tip: {runs:?}");
        assert!(
            runs.iter()
                .flatten()
                .all(|p| p[0].is_finite() && p[1].is_finite() && clip.contains(*p))
        );
        let (up, down) = (runs[0].last().unwrap(), runs[1].first().unwrap());
        assert_eq!(up[1], clip.y, "the up stroke ends at the edge");
        assert_eq!(down[1], clip.y, "the down stroke starts there");
        assert_eq!(up[0], down[0], "on the column's one x");
        assert!((up[0] - (plot.x + plot.w / 5.0)).abs() < 1.0, "at 1 ms");
        // Below the edge both strokes are upright.
        assert_eq!(runs[0][runs[0].len() - 2][0], up[0]);
        assert_eq!(runs[1][1][0], down[0]);
    }
}
