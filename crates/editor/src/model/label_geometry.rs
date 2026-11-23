//! Where a net label's tag is drawn, shared by painting and hit testing.

use crate::model::geometry::{Rect, Vec2};
use xsim_project::{NetLabel, Point, Schematic};

/// Height of a label tag in grid units.
pub const TAG_HEIGHT: f32 = 1.1;
/// Approximate advance of one label character in grid units, for hit testing.
const CHAR_WIDTH: f32 = 0.5;
/// Space between the tag's point and its text, in grid units.
pub const TAG_POINT: f32 = 0.45;
/// Horizontal text padding inside the tag, in grid units.
pub const TAG_PADDING: f32 = 0.3;

/// The side of its anchor a label tag extends to. Text always stays
/// horizontal, so it is never upside down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagSide {
    Right,
    Left,
    /// Above the anchor, for labels in the middle of a horizontal wire.
    Above,
}

/// Chooses the side with no wire leaving the anchor.
pub fn tag_side(schematic: &Schematic, position: Point) -> TagSide {
    let mut right = false;
    let mut left = false;
    for w in &schematic.wires {
        for (i, &p) in w.points.iter().enumerate() {
            if p != position {
                continue;
            }
            let neighbours = [i.checked_sub(1), Some(i + 1)];
            for q in neighbours
                .into_iter()
                .flatten()
                .filter_map(|j| w.points.get(j))
            {
                right |= q.x > p.x && q.y == p.y;
                left |= q.x < p.x && q.y == p.y;
            }
        }
    }
    match (right, left) {
        (true, true) => TagSide::Above,
        (true, false) => TagSide::Left,
        _ => TagSide::Right,
    }
}

/// Approximate world extent of a label tag with text `width` grid units wide.
pub fn tag_rect(position: Point, side: TagSide, text_width: f32) -> Rect {
    let anchor = Vec2::from_grid(position);
    let length = TAG_POINT + text_width + 2.0 * TAG_PADDING;
    let half = TAG_HEIGHT * 0.5;
    match side {
        TagSide::Right => Rect::from_corners(
            anchor + Vec2::new(0.0, -half),
            anchor + Vec2::new(length, half),
        ),
        TagSide::Left => Rect::from_corners(
            anchor + Vec2::new(-length, -half),
            anchor + Vec2::new(0.0, half),
        ),
        TagSide::Above => {
            let w = text_width * 0.5 + TAG_PADDING;
            Rect::from_corners(
                anchor + Vec2::new(-w, -TAG_POINT - TAG_HEIGHT),
                anchor + Vec2::new(w, 0.0),
            )
        }
    }
}

/// Approximate text width of a label name in grid units.
pub fn estimated_text_width(name: &str) -> f32 {
    name.chars().count() as f32 * CHAR_WIDTH
}

/// The label's clickable area.
pub fn label_hit_rect(schematic: &Schematic, label: &NetLabel) -> Rect {
    let side = tag_side(schematic, label.position);
    tag_rect(label.position, side, estimated_text_width(&label.name))
}

#[cfg(test)]
mod tests {
    use super::{TagSide, tag_side};
    use xsim_project::{Point, Schematic, Wire};

    fn with_wire(points: &[(i32, i32)]) -> Schematic {
        Schematic {
            wires: vec![Wire {
                id: 1,
                points: points.iter().map(|&(x, y)| Point::new(x, y)).collect(),
            }],
            ..Schematic::default()
        }
    }

    #[test]
    fn tags_point_away_from_the_wire() {
        let s = with_wire(&[(0, 0), (4, 0)]);
        assert_eq!(tag_side(&s, Point::new(0, 0)), TagSide::Left);
        assert_eq!(tag_side(&s, Point::new(4, 0)), TagSide::Right);
        let s = with_wire(&[(0, 0), (2, 0), (4, 0)]);
        assert_eq!(tag_side(&s, Point::new(2, 0)), TagSide::Above);
        let s = with_wire(&[(0, 0), (0, 4)]);
        assert_eq!(tag_side(&s, Point::new(0, 0)), TagSide::Right);
    }
}
