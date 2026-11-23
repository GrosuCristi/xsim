//! Cuts trace runs where the line turns straight back, for the screen (D20).
//!
//! GPUI strokes paths with lyon. Lyon's miter join between two segments
//! that point almost exactly opposite ways (within about 0.6°) gets a zero
//! join vector, which passes the miter limit, so the join collapses to a
//! single point and both strokes taper to nothing towards it. A spike drawn
//! as one polyline is then a long thin wedge: a hair near its tip. Ending
//! one piece and starting the next at such a vertex gives each piece its
//! own butt cap there, so the line keeps its full width up to the tip.
//!
//! The SVG exports are rasterised by resvg with round joins, which have no
//! such fault, so they draw the runs uncut.

/// Points closer than this to the last kept one are dropped, so the
/// tessellator, which merges points closer than about 0.07 px, sees exactly
/// the joins checked here.
pub const MERGE: f32 = 0.1;
/// Below this length of the sum of a join's two unit directions the line
/// turns back (lyon collapses joins below 0.01).
pub const TURN_BACK: f32 = 0.05;
/// The most points one GPUI path gets: lyon numbers its vertices with
/// 16 bits, and a stroke needs a few vertices per point.
pub const PATH_POINTS: usize = 8192;

/// `run` cut into pieces at every vertex where the line turns back, each
/// piece starting where the previous one ends. Pieces have between two and
/// [`PATH_POINTS`] points; a run that is a single point (after dropping
/// points closer than [`MERGE`]) has none.
pub fn stroke_pieces(run: &[[f32; 2]]) -> Vec<Vec<[f32; 2]>> {
    let mut pieces = Vec::new();
    let mut piece: Vec<[f32; 2]> = Vec::new();
    // The direction of the segment that reached the last kept point.
    let mut incoming: Option<[f32; 2]> = None;
    for &point in run {
        let Some(&last) = piece.last() else {
            piece.push(point);
            continue;
        };
        let (dx, dy) = (point[0] - last[0], point[1] - last[1]);
        let length = dx.hypot(dy);
        if length.is_nan() || length < MERGE {
            continue;
        }
        let direction = [dx / length, dy / length];
        let turns_back =
            incoming.is_some_and(|t| (t[0] + direction[0]).hypot(t[1] + direction[1]) < TURN_BACK);
        if turns_back || piece.len() == PATH_POINTS {
            pieces.push(std::mem::replace(&mut piece, vec![last]));
        }
        piece.push(point);
        incoming = Some(direction);
    }
    if piece.len() > 1 {
        pieces.push(piece);
    }
    pieces
}

/// `pieces` grouped, in order, into paths of at most [`PATH_POINTS`]
/// points each.
pub fn path_batches(pieces: &[Vec<[f32; 2]>]) -> Vec<&[Vec<[f32; 2]>]> {
    let mut batches = Vec::new();
    let (mut start, mut points) = (0, 0);
    for (index, piece) in pieces.iter().enumerate() {
        if points + piece.len() > PATH_POINTS && index > start {
            batches.push(&pieces[start..index]);
            (start, points) = (index, 0);
        }
        points += piece.len();
    }
    if start < pieces.len() {
        batches.push(&pieces[start..]);
    }
    batches
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use gpui::{PathBuilder, point, px};

    /// The triangles GPUI's stroke tessellation makes of `pieces`, each
    /// piece a sub-path, as the canvas paints them.
    fn tessellate(pieces: &[Vec<[f32; 2]>], width: f32) -> Vec<[f32; 2]> {
        let mut builder = PathBuilder::stroke(px(width));
        for piece in pieces {
            builder.move_to(point(px(piece[0][0]), px(piece[0][1])));
            for p in &piece[1..] {
                builder.line_to(point(px(p[0]), px(p[1])));
            }
        }
        let path = builder.build().expect("the stroke tessellates");
        path.vertices
            .iter()
            .map(|v| [v.xy_position.x.as_f32(), v.xy_position.y.as_f32()])
            .collect()
    }

    /// How wide the triangles `vertices` cover the horizontal line `y`,
    /// counting overlaps once.
    pub(crate) fn covered_width(vertices: &[[f32; 2]], y: f32) -> f32 {
        let mut spans: Vec<(f32, f32)> = vertices
            .as_chunks::<3>()
            .0
            .iter()
            .filter_map(|t| {
                let crossings: Vec<f32> = [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])]
                    .into_iter()
                    .filter(|(a, b)| a[1] != b[1] && (a[1] - y) * (b[1] - y) <= 0.0)
                    .map(|(a, b)| a[0] + (y - a[1]) / (b[1] - a[1]) * (b[0] - a[0]))
                    .collect();
                let low = crossings.iter().copied().fold(f32::INFINITY, f32::min);
                let high = crossings.iter().copied().fold(f32::NEG_INFINITY, f32::max);
                (low < high).then_some((low, high))
            })
            .collect();
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut width = 0.0;
        let mut current: Option<(f32, f32)> = None;
        for (low, high) in spans {
            current = match current {
                Some((a, b)) if low <= b => Some((a, b.max(high))),
                Some((a, b)) => {
                    width += b - a;
                    Some((low, high))
                }
                None => Some((low, high)),
            };
        }
        width + current.map_or(0.0, |(a, b)| b - a)
    }

    /// A 40 ns spike in a 5 ms view as the scene lays it out on screen:
    /// flat, up 589 px within a third of a pixel, down to 565 px.
    const SPIKE: [[f32; 2]; 6] = [
        [243.11, 630.16],
        [244.03, 630.16],
        [244.40, 41.16],
        [244.95, 565.17],
        [245.14, 565.42],
        [245.88, 565.56],
    ];
    const TIP: [f32; 2] = SPIKE[2];

    /// Why the runs are cut: drawn whole, GPUI's stroke collapses at the
    /// tip. If a newer GPUI or lyon no longer does, this test fails and the
    /// cutting can go.
    #[test]
    fn a_whole_spike_tapers_to_a_hair() {
        let vertices = tessellate(&[SPIKE.to_vec()], 1.5);
        assert!(covered_width(&vertices, TIP[1] + 2.0) < 0.1);
        assert!(covered_width(&vertices, TIP[1] + 100.0) < 0.75);
    }

    #[test]
    fn cut_at_the_tip_the_spike_keeps_its_width() {
        let pieces = stroke_pieces(&SPIKE);
        assert_eq!(pieces.len(), 2);
        assert_eq!(pieces[0].last(), Some(&TIP));
        assert_eq!(pieces[1].first(), Some(&TIP));
        let vertices = tessellate(&pieces, 1.5);
        for below in [0.5, 2.0, 20.0, 100.0, 300.0, 500.0] {
            let width = covered_width(&vertices, TIP[1] + below);
            // At least the stroke width; wider further down, where the
            // strokes up and down part (see `super::super::columns`).
            assert!(
                (1.45..2.5).contains(&width),
                "{width} px wide {below} px below the tip"
            );
        }
    }

    /// An exactly upright spike (as [`super::super::columns`] draws one).
    #[test]
    fn an_upright_spike_keeps_its_width() {
        let run = [
            [90.0, 600.0],
            [100.0, 600.0],
            [100.0, 50.0],
            [100.0, 580.0],
            [110.0, 580.0],
        ];
        let vertices = tessellate(&[run.to_vec()], 1.5);
        assert!(
            covered_width(&vertices, 52.0) < 0.1,
            "drawn whole it collapses"
        );
        let pieces = stroke_pieces(&run);
        assert_eq!(pieces.len(), 2);
        let vertices = tessellate(&pieces, 1.5);
        for y in [50.5, 52.0, 100.0, 400.0, 570.0] {
            let width = covered_width(&vertices, y);
            assert!((width - 1.5).abs() < 0.01, "{width} px wide at y = {y}");
        }
    }

    #[test]
    fn smooth_lines_and_corners_stay_whole() {
        let sine: Vec<[f32; 2]> = (0..200)
            .map(|i| {
                let x = i as f32;
                [x, 100.0 + 80.0 * (x / 9.0).sin()]
            })
            .collect();
        assert_eq!(stroke_pieces(&sine), vec![sine.clone()]);
        // A square wave turns by right angles, never back.
        let square = [
            [0.0, 10.0],
            [5.0, 10.0],
            [5.0, 90.0],
            [10.0, 90.0],
            [10.0, 10.0],
        ];
        assert_eq!(stroke_pieces(&square).len(), 1);
    }

    #[test]
    fn points_closer_than_the_merge_distance_are_dropped() {
        let run = [[0.0, 0.0], [0.05, 0.0], [10.0, 0.0], [10.0, 0.04]];
        assert_eq!(stroke_pieces(&run), vec![vec![[0.0, 0.0], [10.0, 0.0]]]);
        assert!(stroke_pieces(&[[3.0, 3.0], [3.0, 3.05]]).is_empty());
        assert!(stroke_pieces(&[]).is_empty());
    }

    #[test]
    fn long_runs_are_split_into_paths_lyon_can_number() {
        let run: Vec<[f32; 2]> = (0..20_000).map(|i| [i as f32 * 0.5, 0.0]).collect();
        let pieces = stroke_pieces(&run);
        assert!(pieces.iter().all(|p| p.len() <= PATH_POINTS));
        assert_eq!(
            pieces.iter().map(Vec::len).sum::<usize>(),
            20_000 + pieces.len() - 1
        );
        let batches = path_batches(&pieces);
        assert_eq!(batches.len(), 3);
        for batch in &batches {
            assert!(batch.iter().map(Vec::len).sum::<usize>() <= PATH_POINTS);
        }
        // Small pieces share a path.
        let small = vec![vec![[0.0, 0.0], [1.0, 1.0]]; 10];
        assert_eq!(path_batches(&small), vec![&small[..]]);
        assert!(path_batches(&[]).is_empty());
    }

    /// The worst cases for lyon's 16-bit vertex numbers still tessellate
    /// (a failed tessellation would leave the trace undrawn): a zigzag that
    /// turns back at every vertex, so every piece has its own caps, and a
    /// sawtooth whose every join is cut back by the miter limit.
    #[test]
    fn every_path_batch_tessellates() {
        let zigzag: Vec<[f32; 2]> = (0..20_000)
            .map(|i| [i as f32 * 0.3, if i % 2 == 0 { 0.0 } else { 300.0 }])
            .collect();
        let sawtooth: Vec<[f32; 2]> = (0..PATH_POINTS)
            .map(|i| [i as f32 * 3.0, if i % 2 == 0 { 0.0 } else { 40.0 }])
            .collect();
        for (run, cut) in [(zigzag, true), (sawtooth, false)] {
            let pieces = stroke_pieces(&run);
            assert_eq!(pieces.len() > 1, cut);
            let batches = path_batches(&pieces);
            for batch in batches {
                let vertices = tessellate(batch, 1.5);
                assert!(vertices.len() > batch.len());
            }
        }
    }

    /// The cut keeps spikes at least the stroke's width below the tip, for
    /// every base width: narrow ones are cut, wider ones are joined by
    /// lyon as usual.
    #[test]
    fn spikes_of_every_width_keep_the_stroke_width() {
        for height in [30.0f32, 500.0] {
            for (left, right) in [
                (0.0, 0.0),
                (0.0, 0.3),
                (0.4, 0.0),
                (0.05, 0.05),
                (0.2, 0.2),
                (0.5, 0.5),
                (1.0, 1.0),
                (2.0, 3.0),
                (5.0, 5.0),
                (8.0, 12.0),
                (12.0, 13.0),
                (20.0, 20.0),
                (40.0, 40.0),
            ] {
                let (x, base) = (300.0, 600.0);
                let tip = [x, base - height];
                let run = [
                    [x - left - 20.0, base],
                    [x - left, base],
                    tip,
                    [x + right, base],
                    [x + right + 20.0, base],
                ];
                let vertices = tessellate(&stroke_pieces(&run), 1.5);
                for below in [1.0, 2.0, height / 2.0] {
                    let width = covered_width(&vertices, tip[1] + below);
                    assert!(
                        width >= 1.45,
                        "{height} px high, base {left}+{right}: {width} px wide {below} px below the tip"
                    );
                }
            }
        }
    }

    /// From result to tessellated stroke: the fixture's 40 ns spike in a
    /// 5 ms view is drawn upright by the scene and cut at the tip, so it is
    /// the stroke's width from tip to foot.
    #[test]
    fn the_scene_spike_is_stroked_at_full_width() {
        use crate::model::axis::{AxisMap, AxisRange};
        use crate::model::palette::PlotColors;
        use crate::model::traces::plot_values;
        use crate::model::units::YAxis;
        use crate::scene::Rect;
        use crate::scene::layout::{
            PaneFrame, RIGHT_GUTTER, ScenePane, SceneTrace, SceneX, Style, pane_scene,
        };
        use crate::scene::lod::LodCache;
        use std::sync::Arc;
        use xsim_project::YScale;
        use xsim_spice_engine::SignalData;
        use xsim_waveform::Series;

        let result = crate::test_fixtures::spike();
        let SignalData::Real(values) = &result.signals[0].data else {
            panic!("a real signal");
        };
        let colors = PlotColors::default();
        let x = SceneX {
            values: &result.axis.values,
            range: AxisRange::new(0.0, 5e-3, false).expect("a range"),
            unit: "s",
            title: String::new(),
        };
        let pane = ScenePane {
            traces: vec![SceneTrace {
                color: colors.series[0],
                values: Arc::new(plot_values(
                    &Series::Real(values.clone()),
                    "V",
                    YScale::Linear,
                )),
                axis: YAxis::Left,
            }],
            y: AxisRange::new(-0.1, 2.4, false).expect("a range"),
            y_unit: "V".into(),
            legend: Vec::new(),
            y2: None,
        };
        let frame = PaneFrame {
            rect: Rect::new(0.0, 0.0, 1000.0, 700.0),
            top: 10.0,
            left: 60.0,
            right: RIGHT_GUTTER,
            x_title: false,
            legend: false,
        };
        let style = Style {
            colors: &colors,
            font_size: 12.0,
            line_width: 1.5,
        };
        let scene = pane_scene(&x, &pane, &frame, &style, &mut LodCache::default());
        let pieces: Vec<Vec<[f32; 2]>> = scene.lines[0]
            .runs
            .iter()
            .flat_map(|run| stroke_pieces(run))
            .collect();
        let tip = pieces
            .iter()
            .flatten()
            .copied()
            .min_by(|a, b| a[1].total_cmp(&b[1]))
            .expect("a drawn trace");
        let foot = AxisMap::new(pane.y, scene.plot.bottom(), scene.plot.y).px(0.3);
        let vertices = tessellate(&pieces, 1.5);
        for below in [0.5, 2.0, 20.0, 100.0, 300.0, foot - tip[1] - 2.0] {
            let width = covered_width(&vertices, tip[1] + below);
            assert!(
                (width - 1.5).abs() < 0.02,
                "{width} px wide {below} px below the tip"
            );
        }
    }
}
