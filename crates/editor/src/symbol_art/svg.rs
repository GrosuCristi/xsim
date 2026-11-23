//! SVG renderings of the symbol artwork, used as crisp toolbar icons.

use super::shapes::{SIGN_ARM, Shape, extent, shapes};
use crate::model::geometry::Vec2;
use std::fmt::Write;
use xsim_project::PartKind;

/// An SVG document of `kind`'s symbol in `currentColor`, square and padded,
/// suitable for [`gpui_kit::component::Icon::data`].
pub fn symbol_svg(kind: PartKind) -> String {
    let art = shapes(kind);
    let Some(r) = extent(&art) else {
        return String::from(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"/>"#);
    };
    let side = r.width().max(r.height()) * 1.12;
    let c = r.center();
    let (x0, y0) = (c.x - side / 2.0, c.y - side / 2.0);
    let stroke = side / 11.0;
    let mut svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="{x0:.3} {y0:.3} {side:.3} {side:.3}" fill="none" stroke="currentColor" stroke-width="{stroke:.3}" stroke-linecap="round" stroke-linejoin="round">"#
    );
    for shape in &art {
        push_shape(&mut svg, shape, stroke);
    }
    svg.push_str("</svg>");
    svg
}

fn points_attr(points: &[Vec2]) -> String {
    points
        .iter()
        .map(|p| format!("{:.3},{:.3}", p.x, p.y))
        .collect::<Vec<_>>()
        .join(" ")
}

fn push_shape(svg: &mut String, shape: &Shape, stroke: f32) {
    // Writing to a String cannot fail.
    let _ = match shape {
        Shape::Line(points) => write!(svg, r#"<polyline points="{}"/>"#, points_attr(points)),
        Shape::Fill(points) => write!(
            svg,
            r#"<polygon points="{}" fill="currentColor"/>"#,
            points_attr(points)
        ),
        Shape::Circle { center, radius } => write!(
            svg,
            r#"<circle cx="{:.3}" cy="{:.3}" r="{radius:.3}"/>"#,
            center.x, center.y
        ),
        Shape::Sign { at, plus } => {
            let arm = SIGN_ARM;
            let w = stroke * 0.8;
            let mut d = format!("M{:.3} {:.3}h{:.3}", at.x - arm, at.y, 2.0 * arm);
            if *plus {
                let _ = write!(d, "M{:.3} {:.3}v{:.3}", at.x, at.y - arm, 2.0 * arm);
            }
            write!(svg, r#"<path d="{d}" stroke-width="{w:.3}"/>"#)
        }
    };
}

#[cfg(test)]
mod tests {
    use super::symbol_svg;
    use xsim_project::PartKind;

    #[test]
    fn every_symbol_renders_to_a_complete_svg_document() {
        for kind in PartKind::ALL {
            let svg = symbol_svg(kind);
            assert!(
                svg.starts_with("<svg") && svg.ends_with("</svg>"),
                "{kind:?}"
            );
            assert!(svg.contains("<polyline"), "{kind:?} has no strokes");
            assert!(!svg.contains("NaN"), "{kind:?}");
        }
    }

    /// The switch icons carry every primitive of the canvas artwork: the
    /// `+` and `−` marks of the voltage switch, the current switch's arrow
    /// and the dashed links (one stroke per dash).
    #[test]
    fn switch_symbols_render_like_the_canvas_art() {
        let count = |svg: &str, element: &str| svg.matches(element).count();
        let voltage = symbol_svg(PartKind::VoltageSwitch);
        assert_eq!(count(&voltage, "<path"), 2, "{voltage}");
        assert!(count(&voltage, "<polyline") > 6, "{voltage}");
        let current = symbol_svg(PartKind::CurrentSwitch);
        assert_eq!(count(&current, "<polygon"), 1, "{current}");
        assert!(count(&current, "<polyline") > 4, "{current}");
    }
}
