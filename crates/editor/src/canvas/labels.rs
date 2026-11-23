//! Net labels: a tag pointing at the labelled net, with the name inside.

use crate::canvas::primitives::{Align, Text, polygon, polyline, text, text_width};
use crate::canvas::scene::{Frame, Scene};
use crate::model::geometry::Vec2;
use crate::model::label_geometry::{TAG_HEIGHT, TAG_PADDING, TAG_POINT, TagSide, tag_side};
use crate::model::selection::ObjectRef;
use gpui_kit::{App, FontWeight, Hsla, Window};
use xsim_project::NetLabel;

/// Paints every net label.
pub fn paint_labels(window: &mut Window, cx: &mut App, frame: &Frame, scene: &Scene) {
    for label in &scene.schematic.labels {
        let object = ObjectRef::Label(label.id);
        let border = if scene.selection.contains(object) {
            scene.palette.selection
        } else if scene.hovered == Some(object) {
            scene.palette.selection.opacity(0.6)
        } else {
            scene.palette.label_border
        };
        let side = tag_side(&scene.schematic, label.position);
        paint_label(window, cx, frame, label, side, border, scene);
    }
}

/// Tag dimensions in pixels.
struct TagSize {
    /// Text width.
    text: f32,
    height: f32,
    /// Length of the pointed end.
    point: f32,
    padding: f32,
}

/// The tag outline and the centre of its text.
fn tag_outline(anchor: Vec2, side: TagSide, size: &TagSize) -> (Vec<Vec2>, Vec2) {
    let TagSize {
        text,
        height: h,
        point,
        padding: pad,
    } = *size;
    match side {
        TagSide::Right | TagSide::Left => {
            let dir = if side == TagSide::Right { 1.0 } else { -1.0 };
            let end = point + text + 2.0 * pad;
            let at = |dx: f32, dy: f32| anchor + Vec2::new(dx * dir, dy);
            let outline = vec![
                at(0.0, 0.0),
                at(point, -h / 2.0),
                at(end, -h / 2.0),
                at(end, h / 2.0),
                at(point, h / 2.0),
            ];
            (outline, at(point + pad + text / 2.0, 0.0))
        }
        TagSide::Above => {
            let half = text / 2.0 + pad;
            let top = -point - h;
            let at = |dx: f32, dy: f32| anchor + Vec2::new(dx, dy);
            let outline = vec![
                at(0.0, 0.0),
                at(point * 0.6, -point),
                at(half, -point),
                at(half, top),
                at(-half, top),
                at(-half, -point),
                at(-point * 0.6, -point),
            ];
            (outline, at(0.0, -point - h / 2.0))
        }
    }
}

fn paint_label(
    window: &mut Window,
    cx: &mut App,
    frame: &Frame,
    label: &NetLabel,
    side: TagSide,
    border: Hsla,
    scene: &Scene,
) {
    let scale = frame.scale();
    let name = Text {
        content: &label.name,
        size: (TAG_HEIGHT * scale * 0.62).clamp(8.0, 15.0),
        color: scene.palette.label_text,
        weight: FontWeight::MEDIUM,
    };
    let size = TagSize {
        text: text_width(window, &name),
        height: (TAG_HEIGHT * scale).max(name.size * 1.35),
        point: TAG_POINT * scale,
        padding: TAG_PADDING * scale,
    };
    let (outline, text_at) = tag_outline(frame.grid(label.position), side, &size);
    polygon(window, &outline, scene.palette.label_fill);
    let mut closed = outline.clone();
    closed.extend(outline.first().copied());
    polyline(window, &closed, 1.2, border);
    text(window, cx, &name, text_at, Align::Center);
}
