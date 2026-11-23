//! End-to-end tests of the GPUI-free pipeline: result + setup → evaluated
//! traces → scene → SVG / PNG / JPEG / CSV.

use crate::ImageFormat;
use crate::export;
use crate::model::palette::{Color, PlotColors};
use crate::model::ranges::AxisExtent;
use crate::model::setup;
use crate::model::traces::TraceStore;
use crate::model::units::YAxis;
use crate::scene::layout::{PaneFrame, RIGHT_GUTTER, Style, pane_scene, stacked_scene};
use crate::scene::lod::LodCache;
use crate::scene::{Anchor, Rect, Scene};
use crate::test_fixtures::{ac, ladder, spike, tiny, transient};
use crate::view_model::PlotModel;
use xsim_project::{PlotSetup, YScale};
use xsim_spice_engine::SimulationResult;

struct Fixture {
    result: SimulationResult,
    setup: PlotSetup,
    traces: TraceStore,
    colors: PlotColors,
}

impl Fixture {
    fn new(result: SimulationResult, expressions: &[&str]) -> Self {
        let mut setup = PlotSetup::default();
        setup::adapt_to_result(&mut setup, &result);
        let ac = result.analysis == xsim_spice_engine::AnalysisKind::Ac;
        for expression in expressions {
            setup::add_trace(&mut setup, expression, ac);
        }
        let mut traces = TraceStore::default();
        traces.sync(&setup, Some(&result));
        Self {
            result,
            setup,
            traces,
            colors: PlotColors::default(),
        }
    }

    fn model(&self) -> PlotModel<'_> {
        PlotModel {
            result: &self.result,
            x_extent: AxisExtent::of(&self.result.axis.values),
            setup: &self.setup,
            traces: &self.traces,
            colors: &self.colors,
        }
    }

    fn scene(&self, width: f32, height: f32) -> Scene {
        let model = self.model();
        let style = Style {
            colors: &self.colors,
            font_size: 12.0,
            line_width: 1.5,
        };
        stacked_scene(
            &model.x(),
            &model.panes(),
            width,
            height,
            &style,
            &mut LodCache::default(),
        )
    }
}

/// The `points` of every trace polyline in `svg`.
fn polylines(svg: &str) -> Vec<Vec<(f32, f32)>> {
    svg.split(r#"<polyline class="trace""#)
        .skip(1)
        .filter_map(|chunk| {
            let points = chunk.split(r#"points=""#).nth(1)?.split('"').next()?;
            Some(
                points
                    .split(' ')
                    .filter_map(|pair| {
                        let (x, y) = pair.split_once(',')?;
                        Some((x.parse().ok()?, y.parse().ok()?))
                    })
                    .collect(),
            )
        })
        .collect()
}

#[test]
fn tiny_result_draws_its_samples_across_the_plot() {
    let fixture = Fixture::new(tiny(), &["V(out)"]);
    let scene = fixture.scene(600.0, 300.0);
    let plot = scene.panes[0].plot;
    let lines = polylines(&export::svg::render(&scene));
    assert_eq!(lines.len(), 1);
    let points = &lines[0];
    assert_eq!(points.len(), 3, "{points:?}");
    let close = |a: f32, b: f32| (a - b).abs() < 0.02;
    assert!(close(points[0].0, plot.x));
    assert!(close(points[1].0, plot.x + plot.w / 2.0));
    assert!(close(points[2].0, plot.right()));
    // 0 → 0.5 → 1 V rises evenly up the padded y axis.
    assert!(points[0].1 > points[1].1 && points[1].1 > points[2].1);
    assert!(close(points[0].1 - points[1].1, points[1].1 - points[2].1));
}

/// Unit titles every export used to carry (D20 removes them).
const UNIT_TITLES: [&str; 8] = [
    "Time (s)",
    "Frequency (Hz)",
    "Voltage (V)",
    "Current (A)",
    "Power (W)",
    "Magnitude (dB)",
    "Phase (°)",
    "Value",
];

fn assert_no_unit_titles(svg: &str) {
    for title in UNIT_TITLES {
        assert!(!svg.contains(&format!(">{title}<")), "{title} is drawn");
    }
}

#[test]
fn svg_carries_tick_labels_and_legend_without_unit_titles() {
    let fixture = Fixture::new(tiny(), &["V(out)"]);
    let svg = export::svg::render(&fixture.scene(600.0, 300.0));
    for text in [">V(out)<", ">1 ms<", ">0 s<", ">1 V<"] {
        assert!(svg.contains(text), "missing {text} in {svg}");
    }
    assert_no_unit_titles(&svg);
    assert!(svg.contains(r#"class="swatch""#));
    assert!(svg.contains(&format!(
        r#"fill="{}""#,
        PlotColors::default().background.hex()
    )));
}

#[test]
fn panes_share_the_y_gutter_and_stack_to_the_image_height() {
    let fixture = Fixture::new(ac(), &["V(out)"]);
    assert_eq!(fixture.setup.panes.len(), 2);
    let scene = fixture.scene(800.0, 600.0);
    let (top, bottom) = (scene.panes[0].plot, scene.panes[1].plot);
    assert_eq!(top.x, bottom.x);
    assert_eq!(top.w, bottom.w);
    assert!((top.h - bottom.h).abs() < 0.5);
    assert!(bottom.bottom() < 600.0 && top.y > 0.0);
    let svg = export::svg::render(&scene);
    assert_no_unit_titles(&svg);
    assert!(
        svg.contains(" dB<") && svg.contains("°<"),
        "units on the ticks"
    );
    // Both panes label the shared, decade-labelled log x axis (D20).
    assert_eq!(
        svg.matches(">1 kHz<").count(),
        2,
        "every pane shows x labels"
    );
}

#[test]
fn bode_magnitude_and_phase_follow_the_low_pass() {
    let fixture = Fixture::new(ac(), &["V(out)"]);
    let magnitude = fixture.traces.values("V(out)", YScale::Decibel).unwrap();
    let phase = fixture.traces.values("V(out)", YScale::Phase).unwrap();
    // The 1 kHz corner is sample 30: −3 dB and −45°.
    assert!((magnitude.y[30] + 3.0103).abs() < 1e-3);
    assert!((phase.y[30] + 45.0).abs() < 1e-9);
    assert!(magnitude.y[0].abs() < 1e-3 && phase.y[60] < -89.0);
}

/// INT-9: on a phase pane a typed `ph(V(out))` is the same line as
/// `V(out)`, unwrapped past −180°, while `ph(V(out))` itself stays wrapped.
#[test]
fn typed_phase_traces_match_complex_traces_on_phase_panes() {
    let fixture = Fixture::new(ladder(), &["V(out)", "ph(V(out))", "ph(V(out))-ph(V(in))"]);
    let (_, phase_pane) = setup::bode_pair(&fixture.setup).expect("a Bode layout");
    let pane = &fixture.setup.panes[phase_pane];
    assert_eq!(pane.y_scale, YScale::Phase);
    assert_eq!(pane.traces.len(), 3, "{pane:?}");
    let complex = fixture.traces.values("V(out)", YScale::Phase).unwrap();
    for typed in ["ph(V(out))", "ph(V(out))-ph(V(in))"] {
        let values = fixture.traces.values(typed, YScale::Phase).unwrap();
        assert_eq!(values.unit, "°");
        assert_eq!(values.y.len(), complex.y.len());
        for (i, (a, b)) in values.y.iter().zip(&complex.y).enumerate() {
            assert!((a - b).abs() < 1e-9, "{typed} sample {i}: {a} vs {b}");
        }
        assert!(
            values.y.windows(2).all(|w| (w[1] - w[0]).abs() < 180.0),
            "{typed} is continuous"
        );
    }
    let last = *complex.y.last().unwrap();
    assert!((last + 269.544).abs() < 1e-2, "{last}");
    // The expression itself is wrapped, as LTspice's .meas reports it.
    let evaluated = fixture.traces.evaluated("ph(V(out))").unwrap();
    let xsim_waveform::Series::Real(raw) = &evaluated.series else {
        panic!("ph() is real");
    };
    assert!((raw.last().unwrap() - 90.456).abs() < 1e-2);
    assert_eq!(
        raw.windows(2)
            .filter(|w| (w[1] - w[0]).abs() > 180.0)
            .count(),
        1
    );
    // The scene draws the two traces on top of each other.
    let scene = fixture.scene(800.0, 600.0);
    let lines = &scene.panes[phase_pane].lines;
    assert!(lines.len() >= 2);
    assert_eq!(lines[0].runs, lines[1].runs);
}

#[test]
fn hidden_and_missing_traces_are_not_drawn() {
    let mut fixture = Fixture::new(transient(), &["V(out)", "V(gone)", "V(in)"]);
    setup::toggle_visible(&mut fixture.setup, 0, 2);
    fixture.traces.sync(&fixture.setup, Some(&fixture.result));
    assert!(fixture.traces.error("V(gone)").is_some());
    assert_eq!(fixture.setup.panes[0].traces.len(), 3);
    let pane = fixture.model().pane(0);
    assert_eq!(pane.traces.len(), 1);
    assert_eq!(pane.legend.len(), 1);
}

#[test]
fn zoomed_traces_are_clipped_to_the_plot() {
    let mut fixture = Fixture::new(transient(), &["V(out)"]);
    setup::set_x_range(&mut fixture.setup, Some([1e-3, 2e-3]));
    setup::set_y_range(&mut fixture.setup, 0, Some([0.7, 0.8]));
    let model = fixture.model();
    let style = Style {
        colors: &fixture.colors,
        font_size: 12.0,
        line_width: 1.5,
    };
    let frame = PaneFrame {
        rect: Rect::new(0.0, 0.0, 500.0, 300.0),
        top: 10.0,
        left: 60.0,
        right: RIGHT_GUTTER,
        x_title: false,
        legend: false,
    };
    let scene = pane_scene(
        &model.x(),
        &model.pane(0),
        &frame,
        &style,
        &mut LodCache::default(),
    );
    let bounds = scene.plot.outset(style.line_width + 0.01);
    let points: Vec<[f32; 2]> = scene.lines[0].runs.iter().flatten().copied().collect();
    assert!(!points.is_empty());
    assert!(points.iter().all(|p| bounds.contains(*p)), "{points:?}");
}

#[test]
fn raster_exports_decode_with_the_requested_size() {
    let fixture = Fixture::new(transient(), &["V(out)", "V(in)"]);
    let svg = export::svg::render(&fixture.scene(640.0, 360.0));
    for format in [ImageFormat::Png, ImageFormat::Jpeg] {
        let bytes = export::encode(&svg, format, 640, 360).unwrap();
        let image = image::load_from_memory(&bytes).unwrap();
        assert_eq!((image.width(), image.height()), (640, 360), "{format:?}");
    }
    let png = image::load_from_memory(&export::encode(&svg, ImageFormat::Png, 1280, 720).unwrap())
        .unwrap();
    assert_eq!((png.width(), png.height()), (1280, 720));
    let series = PlotColors::default().series[0];
    let rgba = png.to_rgba8();
    let traced = rgba
        .pixels()
        .filter(|p| p.0 == [series.r, series.g, series.b, 255])
        .count();
    assert!(
        traced > 50,
        "the first trace colour appears in the raster ({traced} px)"
    );
}

#[test]
fn clipboard_image_is_rgba_of_the_requested_size() {
    let fixture = Fixture::new(transient(), &["V(out)"]);
    let svg = export::svg::render(&fixture.scene(320.0, 200.0));
    let image = export::raster::clipboard_image(&svg, 320, 200).unwrap();
    assert_eq!((image.width, image.height), (320, 200));
    assert_eq!(image.bytes.len(), 320 * 200 * 4);
}

/// D9: a current next to a voltage gets its own axis on the right.
#[test]
fn a_current_next_to_a_voltage_is_scaled_on_the_right_axis() {
    let fixture = Fixture::new(transient(), &["V(out)", "I(R1)", "V(in)"]);
    assert_eq!(fixture.setup.panes.len(), 1);
    let model = fixture.model();
    let pane = model.pane(0);
    let axes: Vec<YAxis> = pane.traces.iter().map(|t| t.axis).collect();
    assert_eq!(axes, [YAxis::Left, YAxis::Right, YAxis::Left]);
    assert_eq!(pane.y_unit, "V");
    let right = pane.y2.as_ref().expect("a right axis");
    assert_eq!(right.unit, "A");
    // Each axis fits its own traces: 0…1 V on the left, 0…1 mA on the right.
    assert!(pane.y.min < 0.0 && pane.y.max > 1.0 && pane.y.max < 1.1);
    assert!(right.range.min < 0.0 && right.range.max > 1e-3 && right.range.max < 1.1e-3);
    let legend: Vec<(&str, YAxis)> = pane
        .legend
        .iter()
        .map(|e| (e.label.as_str(), e.axis))
        .collect();
    assert_eq!(
        legend,
        [
            ("V(out)", YAxis::Left),
            ("I(R1)", YAxis::Right),
            ("V(in)", YAxis::Left)
        ]
    );

    // The current fills the plot height instead of hugging zero.
    let scene = fixture.scene(800.0, 400.0);
    let plot = scene.panes[0].plot;
    let ys: Vec<f32> = scene.panes[0].lines[1]
        .runs
        .iter()
        .flatten()
        .map(|p| p[1])
        .collect();
    let span = ys.iter().copied().fold(f32::NEG_INFINITY, f32::max)
        - ys.iter().copied().fold(f32::INFINITY, f32::min);
    assert!(span > plot.h * 0.8, "{span} of {}", plot.h);
}

#[test]
fn svg_export_includes_the_right_axis() {
    let fixture = Fixture::new(transient(), &["V(out)", "I(R1)"]);
    let scene = fixture.scene(800.0, 400.0);
    let pane = &scene.panes[0];
    let right_ticks: Vec<&str> = pane
        .labels
        .iter()
        .filter(|l| l.anchor == Anchor::Start && l.x > pane.plot.right())
        .map(|l| l.text.as_str())
        .collect();
    assert!(right_ticks.len() >= 3, "{right_ticks:?}");
    assert!(
        right_ticks.iter().all(|t| t.ends_with('A')),
        "{right_ticks:?}"
    );
    assert!(!pane.y2_ticks.is_empty());
    let svg = export::svg::render(&scene);
    for text in right_ticks.iter().chain(&["I(R1)", "V(out)"]) {
        assert!(svg.contains(&format!(">{text}<")), "missing {text}");
    }
    assert_no_unit_titles(&svg);
    assert!(svg.contains(">1 mA<"), "{right_ticks:?}");
    assert!(
        svg.contains(r#"<path class="tick""#),
        "right-axis tick marks"
    );
    // The raster formats and the clipboard rasterise the same document.
    let png = export::encode(&svg, ImageFormat::Png, 800, 400).unwrap();
    let png = image::load_from_memory(&png).unwrap().to_rgba8();
    assert_eq!(png.dimensions(), (800, 400));
    // The right-axis trace keeps its pinned colour (slot 1) in the raster.
    let current = PlotColors::default().series[1];
    let traced = png
        .pixels()
        .filter(|p| p.0 == [current.r, current.g, current.b, 255])
        .count();
    assert!(traced > 50, "the current is drawn ({traced} px)");
    let jpeg = export::encode(&svg, ImageFormat::Jpeg, 800, 400).unwrap();
    assert_eq!(image::load_from_memory(&jpeg).unwrap().width(), 800);
    let clipboard = export::raster::clipboard_image(&svg, 800, 400).unwrap();
    assert_eq!((clipboard.width, clipboard.height), (800, 400));
}

#[test]
fn panes_share_the_right_gutter_when_one_has_a_right_axis() {
    let mut fixture = Fixture::new(transient(), &["V(out)", "I(R1)"]);
    setup::add_pane(&mut fixture.setup);
    setup::add_trace(&mut fixture.setup, "V(in)", false);
    fixture.traces.sync(&fixture.setup, Some(&fixture.result));
    let scene = fixture.scene(800.0, 600.0);
    let (top, bottom) = (scene.panes[0].plot, scene.panes[1].plot);
    assert_eq!((top.x, top.right()), (bottom.x, bottom.right()));
    assert!(
        800.0 - top.right() > RIGHT_GUTTER + 8.0,
        "room for current labels"
    );
    assert!(scene.panes[1].y2_ticks.is_empty());
}

/// How many pixels wide a trace of colour `trace` is on row `y` of
/// `image`, between columns `x0..x1`: each pixel counts by how far its
/// colour has moved from the background towards the trace's.
fn trace_width(image: &export::raster::Rgba8, trace: Color, y: u32, x0: u32, x1: u32) -> f32 {
    let background = PlotColors::default().background;
    let distance = |r: u8, g: u8, b: u8, to: Color| {
        f32::from(r.abs_diff(to.r)) + f32::from(g.abs_diff(to.g)) + f32::from(b.abs_diff(to.b))
    };
    let full = distance(trace.r, trace.g, trace.b, background);
    (x0..x1)
        .map(|x| {
            let i = ((y * image.width + x) * 4) as usize;
            let p = &image.data[i..i + 3];
            distance(p[0], p[1], p[2], background) / full
        })
        .sum()
}

/// D20: a 40 ns spike in an exported 5 ms view is as wide near its tip as
/// further down, the trace's own width, instead of thinning to a hair.
#[test]
fn an_exported_narrow_spike_keeps_the_trace_width() {
    let fixture = Fixture::new(spike(), &["V(sw)"]);
    let (width, height) = (2000, 1500);
    let model = fixture.model();
    let style = Style {
        colors: &fixture.colors,
        font_size: 24.0,
        line_width: 3.0,
    };
    let scene = stacked_scene(
        &model.x(),
        &model.panes(),
        width as f32,
        height as f32,
        &style,
        &mut LodCache::default(),
    );
    let run = &scene.panes[0].lines[0].runs[0];
    let tip = run
        .iter()
        .copied()
        .min_by(|a, b| a[1].total_cmp(&b[1]))
        .expect("a drawn trace");
    let image = export::raster::rasterize(&export::svg::render(&scene), width, height).unwrap();
    let color = PlotColors::default().series[0];
    let (x0, x1) = ((tip[0] - 10.0) as u32, (tip[0] + 10.0) as u32);
    let widths: Vec<f32> = [3.0, 10.0, 50.0, 200.0, 500.0, 900.0]
        .into_iter()
        .map(|below| trace_width(&image, color, (tip[1] + below) as u32, x0, x1))
        .collect();
    for (below, w) in [3.0, 10.0, 50.0, 200.0, 500.0, 900.0].iter().zip(&widths) {
        assert!(
            (w - 3.0).abs() < 0.15,
            "{w} px wide {below} px below the tip: {widths:?}"
        );
    }
}

#[test]
fn csv_columns_are_visible_unique_traces() {
    let fixture = Fixture::new(ac(), &["V(out)", "V(in)"]);
    let columns = export::table::trace_columns(&fixture.setup, &fixture.traces);
    let names: Vec<&str> = columns.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["V(out)", "V(in)"]);
    let csv = export::table::csv(&fixture.result, &columns);
    let header = csv.lines().next().unwrap();
    assert!(header.starts_with("frequency [Hz]"), "{header}");
    assert!(header.contains("V(out) magnitude"));
    assert_eq!(csv.lines().count(), fixture.result.len() + 1);
}
