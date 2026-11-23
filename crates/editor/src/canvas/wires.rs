//! Wires, junction dots and the probed net's highlight.

use crate::canvas::primitives::{disc, polyline};
use crate::canvas::scene::{Frame, Scene};
use crate::model::geometry::Vec2;
use crate::model::selection::ObjectRef;
use gpui_kit::Window;
use xsim_project::Wire;

fn screen_points(frame: &Frame, wire: &Wire) -> Vec<Vec2> {
    wire.points.iter().map(|&p| frame.grid(p)).collect()
}

/// Translucent halos under selected, hovered and probed wires.
pub fn paint_wire_halos(window: &mut Window, frame: &Frame, scene: &Scene) {
    let width = (frame.scale() * 1.1).clamp(7.0, 16.0);
    for wire in &scene.schematic.wires {
        let object = ObjectRef::Wire(wire.id);
        let color = if scene.selection.contains(object) {
            scene.palette.selection_halo
        } else if scene
            .net_highlight
            .as_ref()
            .is_some_and(|n| n.wires.contains(&wire.id))
        {
            scene.palette.probe_halo
        } else if scene.hovered == Some(object) {
            scene.palette.hover_halo
        } else {
            continue;
        };
        polyline(window, &screen_points(frame, wire), width, color);
    }
}

/// The wires themselves.
pub fn paint_wires(window: &mut Window, frame: &Frame, scene: &Scene) {
    let width = frame.wire_width();
    for wire in &scene.schematic.wires {
        let color = if scene.selection.has_wire(wire.id) {
            scene.palette.selection
        } else {
            scene.palette.wire
        };
        polyline(window, &screen_points(frame, wire), width, color);
    }
}

/// A dot wherever three or more branches meet (Q50).
pub fn paint_junctions(window: &mut Window, frame: &Frame, scene: &Scene) {
    let radius = frame.junction_radius();
    for &p in &scene.connectivity.junctions {
        disc(window, frame.grid(p), radius, scene.palette.wire);
    }
}

/// Marks the pins of the probed net.
pub fn paint_net_highlight(window: &mut Window, frame: &Frame, scene: &Scene) {
    let Some(net) = &scene.net_highlight else {
        return;
    };
    let radius = (frame.scale() * 0.64).clamp(4.0, 9.0);
    for &p in &net.points {
        disc(window, frame.grid(p), radius, scene.palette.probe_halo);
    }
}
