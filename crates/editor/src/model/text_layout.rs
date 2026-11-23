//! Placement of a part's designator and value text.
//!
//! Text is always drawn horizontally, so it is never upside down whatever
//! the rotation or mirroring. It goes on the first side of the body, in the
//! order right, left, above, below, that has no pin in the middle of it:
//! above a horizontal resistor, right of a vertical one, right of a
//! transistor whose base faces left.

use crate::model::geometry::Vec2;
use xsim_project::{Component, Point};

/// Gap between the body and its text, in grid units.
const GAP: f32 = 0.45;

/// Side of the body the text block sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextSide {
    Right,
    Left,
    Above,
    Below,
}

/// Where a part's text block goes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextPlacement {
    pub side: TextSide,
    /// The point the block attaches to: its left-centre for `Right`,
    /// right-centre for `Left`, bottom-centre for `Above`, top-centre for
    /// `Below`.
    pub anchor: Vec2,
}

/// Places the text of `component`.
pub fn place_text(component: &Component) -> TextPlacement {
    let (min, max) = component.bounds();
    let pins = component.pins();
    let blocked = |side: TextSide| pins.iter().any(|&p| on_face_middle(p, min, max, side));
    let side = [
        TextSide::Right,
        TextSide::Left,
        TextSide::Above,
        TextSide::Below,
    ]
    .into_iter()
    .find(|&s| !blocked(s))
    .unwrap_or(TextSide::Right);
    let (min, max) = (Vec2::from_grid(min), Vec2::from_grid(max));
    let mid = Vec2::new((min.x + max.x) * 0.5, (min.y + max.y) * 0.5);
    let anchor = match side {
        TextSide::Right => Vec2::new(max.x + GAP, mid.y),
        TextSide::Left => Vec2::new(min.x - GAP, mid.y),
        TextSide::Above => Vec2::new(mid.x, min.y - GAP * 0.5),
        TextSide::Below => Vec2::new(mid.x, max.y + GAP * 0.5),
    };
    TextPlacement { side, anchor }
}

/// Whether `p` lies on the given face of the box, away from its corners.
fn on_face_middle(p: Point, min: Point, max: Point, side: TextSide) -> bool {
    let inside_x = p.x > min.x && p.x < max.x;
    let inside_y = p.y > min.y && p.y < max.y;
    match side {
        TextSide::Right => p.x == max.x && inside_y,
        TextSide::Left => p.x == min.x && inside_y,
        TextSide::Above => p.y == min.y && inside_x,
        TextSide::Below => p.y == max.y && inside_x,
    }
}

#[cfg(test)]
mod tests {
    use super::{TextSide, place_text};
    use xsim_project::{Component, Orientation, Part, Point};
    use xsim_spice_engine::Polarity;

    fn component(part: Part, rotation: u8, mirror_x: bool) -> Component {
        Component {
            id: 1,
            name: "X1".into(),
            position: Point::new(0, 0),
            orientation: Orientation {
                rotation,
                mirror_x,
                mirror_y: false,
            },
            part,
        }
    }

    #[test]
    fn two_pin_parts_label_the_side_without_leads() {
        let r = Part::Resistor { resistance: 1.0 };
        assert_eq!(
            place_text(&component(r.clone(), 0, false)).side,
            TextSide::Above
        );
        assert_eq!(
            place_text(&component(r.clone(), 1, false)).side,
            TextSide::Right
        );
        assert_eq!(place_text(&component(r, 3, true)).side, TextSide::Right);
    }

    #[test]
    fn transistors_label_the_side_opposite_the_base() {
        let q = Part::Bjt {
            polarity: Polarity::N,
            model: "M".into(),
        };
        assert_eq!(
            place_text(&component(q.clone(), 0, false)).side,
            TextSide::Right
        );
        assert_eq!(place_text(&component(q, 0, true)).side, TextSide::Left);
    }

    #[test]
    fn text_sits_outside_the_body() {
        let p = place_text(&component(Part::Resistor { resistance: 1.0 }, 0, false));
        assert!(p.anchor.y < -1.0);
        assert_eq!(p.anchor.x, 0.0);
    }
}
