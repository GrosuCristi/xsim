//! Writes a [`Scene`] as a standalone SVG document (Q137). The raster
//! exports and the clipboard image rasterise this same document.

use crate::model::palette::{Color, PlotColors};
use crate::scene::layout::tick_mark_length;
use crate::scene::{Anchor, GridLine, Label, PaneScene, Polyline, Rect, Scene, Swatch};
use std::fmt::Write;

/// Font stack for exported labels; resvg picks the first installed family.
pub const FONT_FAMILY: &str = "Segoe UI, Helvetica Neue, Arial, sans-serif";

/// The SVG document for `scene`.
pub fn render(scene: &Scene) -> String {
    let mut svg = String::new();
    let (w, h) = (scene.width, scene.height);
    let _ = write!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}" font-family="{FONT_FAMILY}">"#
    );
    let _ = write!(
        svg,
        r#"<rect width="100%" height="100%" {}/>"#,
        paint("fill", scene.colors.background)
    );
    for (index, pane) in scene.panes.iter().enumerate() {
        pane_group(&mut svg, index, pane, &scene.colors);
    }
    svg.push_str("</svg>\n");
    svg
}

fn pane_group(svg: &mut String, index: usize, pane: &PaneScene, colors: &PlotColors) {
    let plot = pane.plot;
    let _ = write!(
        svg,
        r#"<g class="pane"><clipPath id="plot-{index}"><rect x="{}" y="{}" width="{}" height="{}"/></clipPath>"#,
        plot.x, plot.y, plot.w, plot.h
    );
    grid_lines(svg, pane, colors);
    frame(svg, plot, colors);
    right_ticks(svg, pane, colors);
    let _ = write!(
        svg,
        r#"<g clip-path="url(#plot-{index})" fill="none" stroke-linejoin="round" stroke-linecap="round">"#
    );
    for line in &pane.lines {
        polyline(svg, line);
    }
    svg.push_str("</g>");
    for swatch in &pane.swatches {
        legend_swatch(svg, swatch);
    }
    for label in &pane.labels {
        text(svg, label);
    }
    svg.push_str("</g>");
}

fn grid_lines(svg: &mut String, pane: &PaneScene, colors: &PlotColors) {
    let plot = pane.plot;
    for (major, color) in [(false, colors.grid_minor), (true, colors.grid)] {
        let mut d = String::new();
        for line in pane.x_grid.iter().filter(|l| l.major == major) {
            let _ = write!(d, "M{:.1} {:.1}V{:.1}", crisp(line), plot.y, plot.bottom());
        }
        for line in pane.y_grid.iter().filter(|l| l.major == major) {
            let _ = write!(d, "M{:.1} {:.1}H{:.1}", plot.x, crisp(line), plot.right());
        }
        if !d.is_empty() {
            let _ = write!(
                svg,
                r#"<path class="grid" d="{d}" {} stroke-width="1" fill="none"/>"#,
                paint("stroke", color)
            );
        }
    }
}

/// A 1px line at a half-pixel position renders sharp when rasterised.
fn crisp(line: &GridLine) -> f32 {
    line.pos.floor() + 0.5
}

fn frame(svg: &mut String, plot: Rect, colors: &PlotColors) {
    let _ = write!(
        svg,
        r#"<rect class="frame" x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" fill="none" {} stroke-width="1"/>"#,
        plot.x.floor() + 0.5,
        plot.y.floor() + 0.5,
        plot.w.round(),
        plot.h.round(),
        paint("stroke", colors.frame)
    );
}

/// Tick marks of the right y axis (D9), pointing inwards from the frame.
fn right_ticks(svg: &mut String, pane: &PaneScene, colors: &PlotColors) {
    if pane.y2_ticks.is_empty() {
        return;
    }
    // The right edge of the frame drawn by `frame`.
    let right = pane.plot.x.floor() + 0.5 + pane.plot.w.round();
    let mut d = String::new();
    for line in &pane.y2_ticks {
        let _ = write!(
            d,
            "M{right:.1} {:.1}h-{:.1}",
            crisp(line),
            tick_mark_length(pane, line)
        );
    }
    let _ = write!(
        svg,
        r#"<path class="tick" d="{d}" {} stroke-width="1" fill="none"/>"#,
        paint("stroke", colors.frame)
    );
}

fn polyline(svg: &mut String, line: &Polyline) {
    for run in &line.runs {
        if let [point] = run.as_slice() {
            let _ = write!(
                svg,
                r#"<circle class="dot" cx="{:.2}" cy="{:.2}" r="{:.2}" {}/>"#,
                point[0],
                point[1],
                line.width * 1.6,
                paint("fill", line.color)
            );
            continue;
        }
        let _ = write!(
            svg,
            r#"<polyline class="trace" {} stroke-width="{}" points=""#,
            paint("stroke", line.color),
            line.width
        );
        for (i, p) in run.iter().enumerate() {
            let separator = if i == 0 { "" } else { " " };
            let _ = write!(svg, "{separator}{:.2},{:.2}", p[0], p[1]);
        }
        svg.push_str("\"/>");
    }
}

fn legend_swatch(svg: &mut String, swatch: &Swatch) {
    let _ = write!(
        svg,
        r#"<path class="swatch" d="M{:.1} {:.1}h{:.1}" {} stroke-width="2.5" stroke-linecap="round"/>"#,
        swatch.x,
        swatch.y,
        swatch.width,
        paint("stroke", swatch.color)
    );
}

fn text(svg: &mut String, label: &Label) {
    let anchor = match label.anchor {
        Anchor::Start => "start",
        Anchor::Middle => "middle",
        Anchor::End => "end",
    };
    let weight = if label.strong {
        r#" font-weight="600""#
    } else {
        ""
    };
    // `y` is the vertical centre; the baseline sits about a third of the
    // font size below it.
    let _ = write!(
        svg,
        r#"<text x="{:.1}" y="{:.1}" font-size="{}" {} text-anchor="{anchor}"{weight}>{}</text>"#,
        label.x,
        label.y + label.size * 0.35,
        label.size,
        paint("fill", label.color),
        escape(&label.text)
    );
}

/// A colour attribute (`fill` or `stroke`) with its opacity when translucent.
fn paint(attribute: &str, color: Color) -> String {
    if color.a < 1.0 {
        format!(
            r#"{attribute}="{}" {attribute}-opacity="{:.3}""#,
            color.hex(),
            color.a
        )
    } else {
        format!(r#"{attribute}="{}""#, color.hex())
    }
}

/// XML-escapes text content.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene() -> Scene {
        let pane = PaneScene {
            plot: Rect::new(40.0, 20.0, 200.0, 100.0),
            x_grid: vec![GridLine {
                pos: 140.0,
                major: true,
            }],
            lines: vec![Polyline {
                color: Color::rgb(0x112233),
                width: 1.5,
                runs: vec![vec![[40.0, 120.0], [240.0, 20.0]], vec![[100.0, 50.0]]],
            }],
            labels: vec![Label {
                text: "V(a) & <b>".into(),
                x: 34.0,
                y: 20.0,
                anchor: Anchor::End,
                color: Color::rgb(0x737373),
                size: 12.0,
                strong: false,
            }],
            ..PaneScene::default()
        };
        Scene {
            width: 300.0,
            height: 160.0,
            colors: PlotColors::default(),
            panes: vec![pane],
        }
    }

    #[test]
    fn document_has_size_traces_dots_and_escaped_text() {
        let svg = render(&scene());
        assert!(
            svg.starts_with(r#"<svg xmlns="http://www.w3.org/2000/svg" width="300" height="160""#)
        );
        assert!(svg.contains(
            r##"stroke="#112233" stroke-width="1.5" points="40.00,120.00 240.00,20.00""##
        ));
        assert!(svg.contains(r#"<circle class="dot" cx="100.00" cy="50.00""#));
        assert!(svg.contains("V(a) &amp; &lt;b&gt;"));
        assert!(svg.contains(r#"text-anchor="end""#));
        assert!(svg.contains("M140.5 20.0V120.0"));
        assert!(svg.trim_end().ends_with("</svg>"));
    }

    #[test]
    fn translucent_colours_keep_their_opacity() {
        let mut scene = scene();
        scene.colors.grid_minor = Color::rgb(0x404040).with_alpha(0.45);
        scene.panes[0].x_grid.push(GridLine {
            pos: 90.0,
            major: false,
        });
        let svg = render(&scene);
        assert!(svg.contains(r##"stroke="#404040" stroke-opacity="0.450""##));
        assert!(!svg.contains(r##"fill="#ffffff" fill-opacity"##));
    }
}
