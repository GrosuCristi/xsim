//! One pane's plot element: a GPUI Kit [`Plot`] that lays out its
//! [`ScenePane`] for the painted bounds and draws it with the kit's `Grid`
//! and `PlotLabel`, then the hover line, the cursor lines and the zoom-box
//! overlay. Traces are stroked with GPUI's `PathBuilder`, cut where they
//! turn straight back ([`stroke_pieces`]), and kept in the kit's
//! `PathCaches` across frames.

use super::state::{Geometry, PaneGeometry};
use super::theme::hsla;
use crate::model::axis::{AxisMap, AxisRange};
use crate::model::cursors::CursorId;
use crate::model::palette::{Color, PlotColors};
use crate::scene::layout::{PaneFrame, ScenePane, SceneX, Style, pane_scene, tick_mark_length};
use crate::scene::lod::LodCache;
use crate::scene::stroke::{path_batches, stroke_pieces};
use crate::scene::{Anchor, Label, PaneScene, Polyline, Rect};
use gpui_kit::base::plot::label::Text;
use gpui_kit::base::plot::{Grid, PathCaches, Plot, PlotElement, PlotLabel, ShapeKey};
use gpui_kit::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use xsim_spice_engine::SimulationResult;

/// The least space above the plot inside the canvas (the pane header above
/// it holds the legend chips).
pub const TOP_GUTTER: f32 = 10.0;

/// Space above the plot: at least [`TOP_GUTTER`], and room for the upper
/// half of a tick label at the plot's top edge, so a large UI scale does
/// not cut it off (labels are `font_size` high, centred on their tick).
pub fn top_gutter(font_size: f32) -> f32 {
    TOP_GUTTER.max((font_size * 0.6).ceil())
}
/// Width of a trace stroke on screen.
const LINE_WIDTH: f32 = 1.5;

/// One cursor as a pane draws it: only an x, as a cursor moves on the x
/// axis only (D19).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CursorMark {
    pub id: CursorId,
    /// Where the line is; it crosses every pane.
    pub x: f64,
}

/// A cursor's line as painted: vertical, one pixel wide, across the plot's
/// whole height. Cursor 1's is solid, cursor 2's dashed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CursorLine {
    pub id: CursorId,
    /// In window coordinates.
    pub bounds: Bounds<Pixels>,
}

/// The cursor lines painted over one pane, by cursor index.
pub type CursorLines = [Option<CursorLine>; 2];

/// A cursor's `1`/`2` flag as painted, at the top of its line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Flag {
    pub id: CursorId,
    /// In window coordinates.
    pub bounds: Bounds<Pixels>,
}

/// The flags painted over one pane: one per cursor line.
pub type Flags = [Option<Flag>; 2];

/// What is drawn over the traces.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Overlay {
    /// The placed cursors.
    pub cursors: Vec<CursorMark>,
    /// The x value under the pointer, shown in every pane.
    pub crosshair: Option<f64>,
    /// Opposite corners of a drag-box zoom, in window coordinates.
    pub zoom_box: Option<(Point<Pixels>, Point<Pixels>)>,
}

/// The plot of one pane.
pub struct PanePlot {
    pub index: usize,
    pub result: Arc<SimulationResult>,
    pub x_range: AxisRange,
    /// The x title under this pane: a sweep's source name, on the bottom
    /// pane only. Every pane shows the x tick labels (D20).
    pub x_title: Option<String>,
    pub pane: ScenePane,
    /// Shared left gutter, so all panes' x axes line up.
    pub left: f32,
    /// Shared right gutter (right-axis labels, D9).
    pub right: f32,
    pub colors: PlotColors,
    pub font_size: f32,
    pub overlay: Overlay,
    pub lod: Rc<RefCell<LodCache>>,
    pub geometry: Geometry,
}

impl IntoElement for PanePlot {
    type Element = PlotElement<Self>;

    fn into_element(self) -> Self::Element {
        PlotElement::new(self)
    }
}

impl Plot for PanePlot {
    fn id(&self) -> Option<ElementId> {
        Some(("waveform-pane-plot", self.index).into())
    }

    /// Hover, cursors and drags are the viewer's (they span all panes).
    fn interactive(&self) -> bool {
        false
    }

    fn paint(&mut self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        let scene = self.layout(bounds);
        let mut geometry = self.geometry_of(bounds, &scene);
        geometry.cursors = cursor_lines(&geometry, &self.overlay.cursors);
        geometry.flags = self.flags(&geometry);
        self.record(geometry);
        paint_grid(&scene, geometry.plot, &self.colors, window);
        window.paint_quad(outline(
            geometry.plot,
            hsla(self.colors.frame),
            BorderStyle::Solid,
        ));
        paint_tick_marks(&scene, bounds, &self.colors, window);
        window.with_content_mask(
            Some(ContentMask {
                bounds: geometry.plot,
            }),
            |window| {
                paint_lines(&scene.lines, bounds, window, cx);
            },
        );
        paint_labels(&scene.labels, bounds, window, cx);
        self.paint_overlay(&geometry, window, cx);
    }
}

impl PanePlot {
    /// The pane laid out at the element's size (scene coordinates are
    /// relative to the element's origin).
    pub(super) fn layout(&self, bounds: Bounds<Pixels>) -> PaneScene {
        let x = SceneX {
            values: &self.result.axis.values,
            range: self.x_range,
            unit: &self.result.axis.unit,
            title: self.x_title.clone().unwrap_or_default(),
        };
        let frame = PaneFrame {
            rect: Rect::new(
                0.0,
                0.0,
                bounds.size.width.as_f32(),
                bounds.size.height.as_f32(),
            ),
            top: top_gutter(self.font_size),
            left: self.left,
            right: self.right,
            x_title: self.x_title.is_some(),
            legend: false,
        };
        let style = Style {
            colors: &self.colors,
            font_size: self.font_size,
            line_width: LINE_WIDTH,
        };
        pane_scene(&x, &self.pane, &frame, &style, &mut self.lod.borrow_mut())
    }

    /// Where the data area is painted, without the cursor lines and flags
    /// yet.
    fn geometry_of(&self, bounds: Bounds<Pixels>, scene: &PaneScene) -> PaneGeometry {
        let origin = bounds.origin;
        let plot = Bounds::new(
            origin + point(px(scene.plot.x), px(scene.plot.y)),
            size(px(scene.plot.w), px(scene.plot.h)),
        );
        let (bottom, top) = (plot.bottom().as_f32(), plot.top().as_f32());
        PaneGeometry {
            frame: bounds,
            plot,
            x: AxisMap::new(self.x_range, plot.left().as_f32(), plot.right().as_f32()),
            y: AxisMap::new(self.pane.y, bottom, top),
            y2: self
                .pane
                .y2
                .as_ref()
                .map(|axis| AxisMap::new(axis.range, bottom, top)),
            cursors: [None; 2],
            flags: [None; 2],
        }
    }

    /// Stores what was painted for the mouse handlers.
    fn record(&self, geometry: PaneGeometry) {
        let mut all = self.geometry.borrow_mut();
        if all.len() <= self.index {
            all.resize(self.index + 1, None);
        }
        all[self.index] = Some(geometry);
    }

    /// Where the cursor flags of this pane go ([`flag_layout`]).
    fn flags(&self, geometry: &PaneGeometry) -> Flags {
        let lines: Vec<(CursorId, f32)> = geometry
            .cursors
            .iter()
            .flatten()
            .map(|line| (line.id, line.bounds.center().x.as_f32()))
            .collect();
        let side = self.flag_size();
        flag_layout(geometry.plot, side, flag_gap(side), &lines)
    }

    /// The hover line, then each cursor's vertical line (cursor 1 solid,
    /// cursor 2 dashed) and the `1`/`2` flags laid out by [`flag_layout`].
    fn paint_overlay(&self, geometry: &PaneGeometry, window: &mut Window, cx: &mut App) {
        let plot = geometry.plot;
        if let Some(x) = self.overlay.crosshair {
            dashed_line(plot, geometry.x.px(x), self.colors.crosshair, window);
        }
        let color = hsla(self.colors.cursor);
        for line in geometry.cursors.iter().flatten() {
            match line.id {
                CursorId::First => window.paint_quad(fill(line.bounds, color)),
                CursorId::Second => Grid::new()
                    .x([line.bounds.size.width / 2.0])
                    .stroke(color)
                    .dash_array(&DASH)
                    .paint(&line.bounds, window),
            }
        }
        for flag in geometry.flags.iter().flatten() {
            self.paint_flag(flag.bounds.origin, flag.id, window, cx);
        }
        if let Some((a, b)) = self.overlay.zoom_box {
            paint_zoom_box(plot, a, b, &self.colors, window);
        }
    }

    /// The side of a cursor flag: a little more than the label font, so it
    /// follows the UI scale.
    fn flag_size(&self) -> f32 {
        (self.font_size * 1.4).round()
    }

    /// A `1`/`2` tab with its top-left corner at `origin`.
    fn paint_flag(&self, origin: Point<Pixels>, id: CursorId, window: &mut Window, cx: &mut App) {
        let side = self.flag_size();
        let flag = Bounds::new(origin, size(px(side), px(side)));
        window.paint_quad(fill(flag, hsla(self.colors.cursor)).corner_radii(px(side / 5.0)));
        let label = Text::new(
            id.number(),
            point(px(side / 2.0), px(side / 2.0 - self.font_size / 2.0)),
            hsla(self.colors.background),
        )
        .font_size(px(self.font_size))
        .align(TextAlign::Center);
        PlotLabel::new(vec![label]).paint(&flag, window, cx);
    }
}

/// The dashes of a dashed line: 4 px on, 3 px off.
const DASH: [Pixels; 2] = [px(4.), px(3.)];

/// The lines of the placed cursors `marks` whose x is inside the plot, in
/// window coordinates: each vertical and one pixel wide across the plot's
/// whole height. No cursor has a horizontal line (D19).
pub(crate) fn cursor_lines(geometry: &PaneGeometry, marks: &[CursorMark]) -> CursorLines {
    let plot = geometry.plot;
    let inside = plot.left().as_f32()..=plot.right().as_f32();
    let mut lines: CursorLines = [None; 2];
    for mark in marks {
        let x = geometry.x.px(mark.x);
        if inside.contains(&x) {
            lines[mark.id.index()] = Some(CursorLine {
                id: mark.id,
                bounds: Bounds::new(
                    point(px(x - 0.5), plot.top()),
                    size(px(1.), plot.size.height),
                ),
            });
        }
    }
    lines
}

/// The space kept between neighbouring flags of side `side`: about an
/// eighth of a flag, so it follows the UI scale.
pub(crate) fn flag_gap(side: f32) -> f32 {
    (side / 8.0).round().max(1.0)
}

/// Lays out the cursor flags of one pane so that no two overlap, in window
/// coordinates: a flag centred on each cursor line (`lines`: cursor and
/// line x), at the top of `plot`. When the lines are closer than a flag,
/// the later cursor's flag goes one row lower, still centred on its line,
/// so the flags never cover each other or swap places. Neighbouring flags
/// keep `gap` pixels between them.
pub(crate) fn flag_layout(
    plot: Bounds<Pixels>,
    side: f32,
    gap: f32,
    lines: &[(CursorId, f32)],
) -> Flags {
    let mut flags: Flags = [None; 2];
    let mut placed: Vec<Bounds<Pixels>> = Vec::new();
    let blocker = |placed: &[Bounds<Pixels>], at: Bounds<Pixels>| {
        placed
            .iter()
            .filter(|other| touches(**other, at, gap))
            .max_by(|a, b| a.bottom().as_f32().total_cmp(&b.bottom().as_f32()))
            .copied()
    };
    let top = plot.top().as_f32();
    let wanted = lines
        .iter()
        .map(|&(id, x)| (id, square(x - side / 2.0, top, side)));
    for (slot, (id, mut at)) in wanted.take(flags.len()).enumerate() {
        // Each step moves the flag below one already placed, and never back
        // up, so it is clear after at most one step per placed flag.
        for _ in 0..placed.len() {
            let Some(other) = blocker(&placed, at) else {
                break;
            };
            at = square(at.left().as_f32(), other.bottom().as_f32() + gap, side);
        }
        placed.push(at);
        flags[slot] = Some(Flag { id, bounds: at });
    }
    flags
}

/// A `side`-square with its top-left corner at (`x`, `y`).
fn square(x: f32, y: f32, side: f32) -> Bounds<Pixels> {
    Bounds::new(point(px(x), px(y)), size(px(side), px(side)))
}

/// Whether `a` and `b` overlap or are closer than `gap` pixels.
fn touches(a: Bounds<Pixels>, b: Bounds<Pixels>, gap: f32) -> bool {
    let (a_left, a_right) = (a.left().as_f32(), a.right().as_f32());
    let (b_left, b_right) = (b.left().as_f32(), b.right().as_f32());
    let (a_top, a_bottom) = (a.top().as_f32(), a.bottom().as_f32());
    let (b_top, b_bottom) = (b.top().as_f32(), b.bottom().as_f32());
    a_left < b_right + gap
        && b_left < a_right + gap
        && a_top < b_bottom + gap
        && b_top < a_bottom + gap
}

fn paint_grid(scene: &PaneScene, plot: Bounds<Pixels>, colors: &PlotColors, window: &mut Window) {
    for (major, color) in [(false, colors.grid_minor), (true, colors.grid)] {
        let xs = scene
            .x_grid
            .iter()
            .filter(|l| l.major == major)
            .map(|l| px(l.pos - scene.plot.x));
        let ys = scene
            .y_grid
            .iter()
            .filter(|l| l.major == major)
            .map(|l| px(l.pos - scene.plot.y));
        Grid::new()
            .x(xs)
            .y(ys)
            .stroke(hsla(color))
            .paint(&plot, window);
    }
}

/// The right axis's tick marks (D9), pointing inwards from the frame.
fn paint_tick_marks(
    scene: &PaneScene,
    bounds: Bounds<Pixels>,
    colors: &PlotColors,
    window: &mut Window,
) {
    let right = scene.plot.right();
    for line in &scene.y2_ticks {
        let length = tick_mark_length(scene, line);
        let mark = Bounds::new(
            bounds.origin + point(px(right - length), px(line.pos.floor())),
            size(px(length), px(1.)),
        );
        window.paint_quad(fill(mark, hsla(colors.frame)));
    }
}

/// Every trace as stroked paths: its runs cut where the line turns straight
/// back, which GPUI's stroke would pinch to a hair ([`stroke_pieces`]), and
/// grouped into paths small enough to tessellate. The tessellations are
/// cached across frames while the points do not change.
fn paint_lines(lines: &[Polyline], bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
    let caches = PathCaches::for_paint("waveform-traces", window, cx);
    caches.update(cx, |caches, _| {
        let mut slot = 0;
        for line in lines {
            let color = hsla(line.color);
            let mut pieces = Vec::new();
            for run in &line.runs {
                if let [point] = run.as_slice() {
                    paint_dot(bounds, *point, line.width * 1.6, color, window);
                } else {
                    pieces.extend(stroke_pieces(run));
                }
            }
            for batch in path_batches(&pieces) {
                let key = stroke_key(batch, line.width);
                let path = caches
                    .slot(slot)
                    .get(key, bounds.origin, || stroke_path(batch, line.width));
                if let Some(path) = path {
                    window.paint_path(path, color);
                }
                slot += 1;
            }
        }
    });
}

/// One path stroking every piece of `pieces` (each its own sub-path), in
/// scene coordinates; the cache moves it to the element's origin.
fn stroke_path(pieces: &[Vec<[f32; 2]>], width: f32) -> Option<Path<Pixels>> {
    let mut builder = PathBuilder::stroke(px(width));
    for piece in pieces {
        let (first, rest) = piece.split_first()?;
        builder.move_to(point(px(first[0]), px(first[1])));
        for p in rest {
            builder.line_to(point(px(p[0]), px(p[1])));
        }
    }
    builder.build().ok()
}

/// What a stroked path depends on: the width and every piece's points.
fn stroke_key(pieces: &[Vec<[f32; 2]>], width: f32) -> u64 {
    let lengths: Vec<usize> = pieces.iter().map(Vec::len).collect();
    let mut key = ShapeKey::new((width.to_bits(), lengths));
    for p in pieces.iter().flatten() {
        key.point(point(px(p[0]), px(p[1])));
    }
    key.finish()
}

fn paint_dot(bounds: Bounds<Pixels>, at: [f32; 2], radius: f32, color: Hsla, window: &mut Window) {
    let dot = Bounds::new(
        bounds.origin + point(px(at[0] - radius), px(at[1] - radius)),
        size(px(2.0 * radius), px(2.0 * radius)),
    );
    window.paint_quad(fill(dot, color).corner_radii(px(radius)));
}

fn paint_labels(labels: &[Label], bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
    let texts = labels
        .iter()
        .map(|label| {
            let align = match label.anchor {
                Anchor::Start => TextAlign::Left,
                Anchor::Middle => TextAlign::Center,
                Anchor::End => TextAlign::Right,
            };
            Text::new(
                label.text.clone(),
                point(px(label.x), px(label.y - label.size / 2.0)),
                hsla(label.color),
            )
            .font_size(px(label.size))
            .align(align)
        })
        .collect();
    PlotLabel::new(texts).paint(&bounds, window, cx);
}

/// A dashed vertical line across the plot at window x `x`, when that is
/// inside it.
fn dashed_line(plot: Bounds<Pixels>, x: f32, color: Color, window: &mut Window) {
    let offset = x - plot.left().as_f32();
    if !(0.0..=plot.size.width.as_f32()).contains(&offset) {
        return;
    }
    Grid::new()
        .x([px(offset)])
        .stroke(hsla(color))
        .dash_array(&DASH)
        .paint(&plot, window);
}

fn paint_zoom_box(
    plot: Bounds<Pixels>,
    a: Point<Pixels>,
    b: Point<Pixels>,
    colors: &PlotColors,
    window: &mut Window,
) {
    let clamp = |p: Point<Pixels>| {
        point(
            p.x.clamp(plot.left(), plot.right()),
            p.y.clamp(plot.top(), plot.bottom()),
        )
    };
    let (a, b) = (clamp(a), clamp(b));
    let selection = Bounds::from_corners(
        point(a.x.min(b.x), a.y.min(b.y)),
        point(a.x.max(b.x), a.y.max(b.y)),
    );
    window.paint_quad(fill(selection, hsla(colors.selection)));
    window.paint_quad(outline(
        selection,
        hsla(colors.selection.with_alpha(0.8)),
        BorderStyle::Solid,
    ));
}

#[cfg(test)]
mod tests {
    use super::{CursorMark, Flag, Flags, cursor_lines, flag_gap, flag_layout, touches};
    use crate::model::axis::{AxisMap, AxisRange};
    use crate::model::cursors::CursorId;
    use crate::ui::state::PaneGeometry;
    use gpui_kit::{Bounds, Pixels, point, px, size};

    const SIDE: f32 = 17.0;
    const GAP: f32 = 2.0;

    fn plot() -> Bounds<Pixels> {
        Bounds::new(point(px(60.), px(40.)), size(px(600.), px(300.)))
    }

    fn placed(flags: &Flags) -> Vec<Flag> {
        flags.iter().flatten().copied().collect()
    }

    fn assert_apart(flags: &Flags) {
        let all = placed(flags);
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert!(
                    !touches(a.bounds, b.bounds, GAP - 1e-3),
                    "{a:?} overlaps {b:?}"
                );
            }
        }
    }

    fn centre_x(flag: &Flag) -> f32 {
        flag.bounds.center().x.as_f32()
    }

    #[test]
    fn far_apart_lines_keep_their_flags_on_top_and_centred() {
        let flags = flag_layout(
            plot(),
            SIDE,
            GAP,
            &[(CursorId::First, 200.0), (CursorId::Second, 400.0)],
        );
        let all = placed(&flags);
        assert_eq!(all.len(), 2);
        for (flag, x) in all.iter().zip([200.0, 400.0]) {
            assert_eq!(flag.bounds.top(), plot().top());
            assert_eq!(centre_x(flag), x);
        }
    }

    /// Lines on the same pixel, one pixel apart and just under a flag apart:
    /// cursor 2's flag drops a row and stays centred on its line.
    #[test]
    fn close_lines_stack_their_flags() {
        for (x1, x2) in [
            (300.0, 300.0),
            (300.0, 301.0),
            (301.0, 300.0),
            (300.0, 300.0 + SIDE + GAP - 0.5),
            // Against both edges of the plot.
            (60.0, 61.0),
            (660.0, 659.0),
        ] {
            let flags = flag_layout(
                plot(),
                SIDE,
                GAP,
                &[(CursorId::First, x1), (CursorId::Second, x2)],
            );
            assert_apart(&flags);
            let all = placed(&flags);
            assert_eq!((all[0].id, all[1].id), (CursorId::First, CursorId::Second));
            assert_eq!(all[0].bounds.top(), plot().top(), "cursor 1 stays on top");
            assert_eq!(
                all[1].bounds.top(),
                plot().top() + px(SIDE + GAP),
                "cursor 2 one row lower"
            );
            assert_eq!((centre_x(&all[0]), centre_x(&all[1])), (x1, x2));
        }
        // Exactly a flag and a gap apart: room for both on top.
        let flags = flag_layout(
            plot(),
            SIDE,
            GAP,
            &[
                (CursorId::First, 300.0),
                (CursorId::Second, 300.0 + SIDE + GAP),
            ],
        );
        assert_apart(&flags);
        assert!(
            placed(&flags)
                .iter()
                .all(|f| f.bounds.top() == plot().top())
        );
    }

    /// `plot()` showing x from 0 to 10 and y from 0 to 1.
    fn geometry() -> PaneGeometry {
        let plot = plot();
        let (left, right) = (plot.left().as_f32(), plot.right().as_f32());
        let (top, bottom) = (plot.top().as_f32(), plot.bottom().as_f32());
        let range = |min, max| AxisRange::new(min, max, false).expect("a range");
        PaneGeometry {
            frame: plot,
            plot,
            x: AxisMap::new(range(0.0, 10.0), left, right),
            y: AxisMap::new(range(0.0, 1.0), bottom, top),
            y2: None,
            cursors: [None; 2],
            flags: [None; 2],
        }
    }

    /// D19: a cursor is a vertical line only, one pixel wide at its x and
    /// across the plot's whole height; one outside the visible x range has
    /// no line (and so no flag).
    #[test]
    fn cursor_lines_are_vertical_only() {
        let geometry = geometry();
        let plot = geometry.plot;
        let marks = [
            CursorMark {
                id: CursorId::First,
                x: 2.5,
            },
            CursorMark {
                id: CursorId::Second,
                x: 7.5,
            },
        ];
        let lines = cursor_lines(&geometry, &marks);
        for (line, x) in lines.iter().zip([2.5, 7.5]) {
            let line = line.expect("a line inside the plot");
            assert_eq!(line.bounds.size.width, px(1.));
            assert_eq!(line.bounds.top(), plot.top());
            assert_eq!(line.bounds.size.height, plot.size.height);
            assert_eq!(line.bounds.center().x.as_f32(), geometry.x.px(x));
        }
        let (first, second) = (lines[0].unwrap(), lines[1].unwrap());
        assert_eq!((first.id, second.id), (CursorId::First, CursorId::Second));

        // Beyond the visible range: no line.
        let outside = [CursorMark {
            id: CursorId::Second,
            x: 12.0,
        }];
        assert_eq!(cursor_lines(&geometry, &outside), [None, None]);
        assert_eq!(cursor_lines(&geometry, &[]), [None, None]);
    }

    /// D20 on screen, end to end: the 40 ns spike in a 5 ms view, laid out
    /// as a pane canvas lays it out (in the top pane, which has its x
    /// labels too) and stroked into the very paths the canvas paints, is
    /// the trace's width from its tip to its foot.
    #[test]
    fn the_painted_spike_keeps_the_stroke_width() {
        use super::{LINE_WIDTH, Overlay, PanePlot, stroke_path};
        use crate::model::palette::PlotColors;
        use crate::model::traces::plot_values;
        use crate::model::units::YAxis;
        use crate::scene::Anchor;
        use crate::scene::layout::{RIGHT_GUTTER, ScenePane, SceneTrace};
        use crate::scene::stroke::tests::covered_width;
        use crate::scene::stroke::{path_batches, stroke_pieces};
        use std::rc::Rc;
        use std::sync::Arc;
        use xsim_project::YScale;
        use xsim_spice_engine::SignalData;
        use xsim_waveform::Series;

        let result = Arc::new(crate::test_fixtures::spike());
        let SignalData::Real(values) = &result.signals[0].data else {
            panic!("a real signal");
        };
        let colors = PlotColors::default();
        let y = AxisRange::new(-0.1, 2.4, false).expect("a range");
        let plot = PanePlot {
            index: 0,
            result: result.clone(),
            x_range: AxisRange::new(0.0, 5e-3, false).expect("a range"),
            x_title: None,
            pane: ScenePane {
                traces: vec![SceneTrace {
                    color: colors.series[0],
                    values: Arc::new(plot_values(
                        &Series::Real(values.clone()),
                        "V",
                        YScale::Linear,
                    )),
                    axis: YAxis::Left,
                }],
                y,
                y_unit: "V".into(),
                legend: Vec::new(),
                y2: None,
            },
            left: 60.0,
            right: RIGHT_GUTTER,
            colors: colors.clone(),
            font_size: 12.0,
            overlay: Overlay::default(),
            lod: Rc::default(),
            geometry: Rc::default(),
        };
        let bounds = Bounds::new(point(px(200.), px(100.)), size(px(1000.), px(700.)));
        let scene = plot.layout(bounds);
        assert!(
            scene
                .labels
                .iter()
                .any(|l| l.anchor == Anchor::Middle && l.text == "1 ms"),
            "the top pane labels x: {:?}",
            scene.labels
        );
        let line = &scene.lines[0];
        assert_eq!(line.width, LINE_WIDTH);
        let pieces: Vec<Vec<[f32; 2]>> = line.runs.iter().flat_map(|r| stroke_pieces(r)).collect();
        let batches = path_batches(&pieces);
        assert_eq!(batches.len(), 1);
        let path = stroke_path(batches[0], line.width).expect("the trace tessellates");
        let vertices: Vec<[f32; 2]> = path
            .vertices
            .iter()
            .map(|v| [v.xy_position.x.as_f32(), v.xy_position.y.as_f32()])
            .collect();
        let tip = pieces
            .iter()
            .flatten()
            .copied()
            .min_by(|a, b| a[1].total_cmp(&b[1]))
            .expect("a drawn trace");
        let foot = AxisMap::new(y, scene.plot.bottom(), scene.plot.y).px(0.3);
        for below in [0.5, 2.0, 20.0, 100.0, 300.0, foot - tip[1] - 2.0] {
            let width = covered_width(&vertices, tip[1] + below);
            assert!(
                (width - LINE_WIDTH).abs() < 0.02,
                "{width} px wide {below} px below the tip"
            );
        }
    }

    #[test]
    fn the_gap_follows_the_flag_size() {
        assert_eq!(flag_gap(17.0), 2.0);
        assert_eq!(flag_gap(34.0), 4.0);
        assert_eq!(flag_gap(4.0), 1.0);
    }
}
