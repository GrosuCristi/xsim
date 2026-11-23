//! A snapshot of everything the canvas paints in one frame.

use crate::camera::{Camera, Viewport};
use crate::canvas::palette::Palette;
use crate::model::geometry::{Rect, Vec2};
use crate::model::hit_test::BoxMode;
use crate::model::selection::{ObjectRef, Selection};
use gpui_kit::CursorStyle;
use std::collections::BTreeSet;
use std::sync::Arc;
use xsim_project::{Connectivity, Id, Orientation, PartKind, Point, Schematic};

/// A part following the mouse in placement mode.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ghost {
    pub kind: PartKind,
    pub position: Point,
    pub orientation: Orientation,
}

/// The net under the probe, highlighted while probing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NetHighlight {
    pub wires: BTreeSet<Id>,
    pub points: Vec<Point>,
}

/// The selection rectangle being dragged.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxSelection {
    pub rect: Rect,
    pub mode: BoxMode,
}

/// When the canvas should fit the camera to the drawing before painting.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AutoFit {
    /// Keep the camera.
    Off,
    /// Fit now (first show of a view the user never moved).
    Now(Option<Rect>),
    /// Fit whenever the canvas changes size (the user has not moved the view).
    OnResize(Option<Rect>),
}

/// Everything painted in one frame.
pub struct Scene {
    pub schematic: Arc<Schematic>,
    pub connectivity: Arc<Connectivity>,
    pub camera: Camera,
    pub auto_fit: AutoFit,
    pub selection: Selection,
    pub hovered: Option<ObjectRef>,
    pub net_highlight: Option<NetHighlight>,
    pub ghost: Option<Ghost>,
    /// Committed vertices plus the rubber-band route to the mouse.
    pub wire_preview: Vec<Point>,
    pub box_selection: Option<BoxSelection>,
    /// World position of the dragged probe glyph.
    pub probe_glyph: Option<Vec2>,
    pub cursor: CursorStyle,
    pub palette: Palette,
}

/// The camera and viewport of the frame being painted.
#[derive(Debug, Clone, Copy)]
pub struct Frame {
    pub camera: Camera,
    pub viewport: Viewport,
}

impl Frame {
    /// Screen position of a world point.
    pub fn screen(&self, world: Vec2) -> Vec2 {
        self.camera.to_screen(&self.viewport, world)
    }

    /// Screen position of a grid point.
    pub fn grid(&self, p: Point) -> Vec2 {
        self.screen(Vec2::from_grid(p))
    }

    /// Pixels per grid unit.
    pub fn scale(&self) -> f32 {
        self.camera.scale(&self.viewport)
    }

    /// Wire pen width in pixels.
    pub fn wire_width(&self) -> f32 {
        (self.scale() * 0.1).clamp(1.4, 3.0)
    }

    /// Junction dot radius in pixels.
    pub fn junction_radius(&self) -> f32 {
        (self.scale() * 0.2).clamp(2.6, 6.0)
    }

    /// Whether text is large enough to be worth drawing.
    pub fn shows_text(&self) -> bool {
        self.scale() >= 7.0
    }
}
