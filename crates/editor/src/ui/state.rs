//! Transient interaction state of the editor (never saved).

use crate::SchematicEditor;
use crate::model::geometry::Vec2;
use crate::model::routing::Axis;
use crate::ui::gesture::Gesture;
use gpui_kit::{App, Entity, Window};
use std::rc::Rc;
use xsim_project::{Id, Orientation, Part, Point};

/// A part being placed: each click drops a copy.
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    pub part: Part,
    pub orientation: Orientation,
}

/// A wire being drawn with the wire tool.
#[derive(Debug, Clone, PartialEq)]
pub struct WireDraft {
    /// Vertices placed so far; the first is the start point.
    pub points: Vec<Point>,
    /// Natural direction of the first leg when the wire starts at a pin.
    pub start_axis: Option<Axis>,
}

/// The mouse over the canvas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pointer {
    /// Window position in pixels.
    pub screen: Vec2,
    /// World position in grid units.
    pub world: Vec2,
    /// Alt held: wires may take any angle (Q49).
    pub any_angle: bool,
}

/// Mouse and keyboard state between events.
#[derive(Default)]
pub struct Interaction {
    pub gesture: Gesture,
    pub wire: Option<WireDraft>,
    /// `None` while the mouse is outside the canvas.
    pub pointer: Option<Pointer>,
    /// Space held: dragging pans the view.
    pub space_held: bool,
    /// The last left press in the select tool ended a wire started from a
    /// pin (D20): it finished the wire, or a double-click on its start pin
    /// dropped it. The further presses of that multi-click (the second
    /// press of a double-click) belong to it and are ignored, so they do
    /// not open the properties of the pin's part or rename the label the
    /// wire ended on.
    pub wire_finished: bool,
}

/// Opens the properties of a part; replaceable for embedding and tests.
pub type PropertiesOpener = Rc<dyn Fn(&Entity<SchematicEditor>, Id, &mut Window, &mut App)>;
