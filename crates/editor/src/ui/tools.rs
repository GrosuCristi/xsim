//! The drawing tools: part placement, wires (also started from a pin in
//! the select tool, D20) and net labels.

use crate::canvas::scene::Ghost;
use crate::library_browser::{self, LibraryFilter, LibraryItem};
use crate::model::geometry::{Vec2, on_segment};
use crate::model::hit_test::Hit;
use crate::model::ops;
use crate::model::routing::{Axis, first_leg, pin_axis, route};
use crate::model::transform::{Mirror, mirrored};
use crate::ui::gesture::Gesture;
use crate::ui::state::{Placement, Pointer, WireDraft};
use crate::{SchematicEditor, Tool};
use gpui_kit::{Context, Window};
use xsim_project::{Orientation, Part, PartKind, Point, Schematic};

/// Orientation of a freshly chosen part: sources stand upright with `+` on
/// top, everything else lies horizontally.
pub fn default_orientation(kind: PartKind) -> Orientation {
    match kind {
        PartKind::VoltageSource | PartKind::CurrentSource => Orientation {
            rotation: 1,
            ..Orientation::default()
        },
        _ => Orientation::default(),
    }
}

/// Whether a wire drawn to `p` connects to something there.
fn connects_at(schematic: &Schematic, p: Point) -> bool {
    schematic.components.iter().any(|c| c.pins().contains(&p))
        || schematic.labels.iter().any(|l| l.position == p)
        || schematic
            .wires
            .iter()
            .any(|w| w.points.windows(2).any(|s| on_segment(p, s[0], s[1])))
}

impl SchematicEditor {
    /// Switches tool, dropping unfinished work of the previous one. Only the
    /// select tool is available in read-only mode.
    pub(crate) fn switch_tool(&mut self, tool: Tool, cx: &mut Context<Self>) {
        let tool = if self.read_only { Tool::Select } else { tool };
        self.label_editor = None;
        self.interaction.wire = None;
        self.interaction.gesture = Gesture::Idle;
        self.placement = match tool {
            Tool::Place(kind) => Some(match self.placement.take() {
                Some(p) if p.part.kind() == kind => p,
                _ => Placement {
                    part: Part::default_for(kind),
                    orientation: default_orientation(kind),
                },
            }),
            _ => None,
        };
        if tool != Tool::Select {
            self.selection.clear();
        }
        self.tool = tool;
        cx.notify();
    }

    /// Enters placement mode for `part` (Q54).
    pub(crate) fn begin_placement(&mut self, part: Part, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        let kind = part.kind();
        self.switch_tool(Tool::Place(kind), cx);
        self.placement = Some(Placement {
            part,
            orientation: default_orientation(kind),
        });
    }

    /// Opens the component library; picking an item starts placing it.
    pub(crate) fn open_library(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        let editor = cx.weak_entity();
        let on_pick = move |item: LibraryItem, window: &mut Window, cx: &mut gpui_kit::App| {
            editor
                .update(cx, |editor, cx| {
                    editor.place_library_item(item, cx);
                    editor.focus.focus(window, cx);
                })
                .ok();
        };
        library_browser::open_library(LibraryFilter::All, on_pick, window, cx);
    }

    /// Rotates or mirrors the placement ghost. Returns `false` when not placing.
    pub(crate) fn transform_ghost(
        &mut self,
        mirror: Option<Mirror>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(placement) = &mut self.placement else {
            return false;
        };
        placement.orientation = match mirror {
            None => placement.orientation.rotated(),
            Some(m) => mirrored(placement.orientation, m),
        };
        cx.notify();
        true
    }

    /// The ghost of the part being placed, under the mouse.
    pub(crate) fn ghost(&self) -> Option<Ghost> {
        let placement = self.placement.as_ref()?;
        let pointer = self.interaction.pointer?;
        Some(Ghost {
            kind: placement.part.kind(),
            position: pointer.world.snapped(),
            orientation: placement.orientation,
        })
    }

    /// Places a copy of the current part at `at`; placement continues.
    pub(crate) fn place_at(&mut self, at: Point, cx: &mut Context<Self>) {
        let Some(placement) = self.placement.clone() else {
            return;
        };
        self.edit(cx, |s| {
            ops::place_component(s, placement.part, at, placement.orientation)
        });
    }

    /// A click with the wire tool: starts a wire, adds a vertex, or finishes
    /// on a pin, wire or label (or on a double-click).
    pub(crate) fn wire_click(
        &mut self,
        pointer: Pointer,
        click_count: usize,
        cx: &mut Context<Self>,
    ) {
        let at = pointer.world.snapped();
        let Some(draft) = &mut self.interaction.wire else {
            self.start_wire(at, cx);
            return;
        };
        let route = draft_route(draft, at, pointer.any_angle);
        draft.points.extend(route);
        draft.points.dedup();
        let started_here = draft.points.len() == 1;
        if click_count >= 2 || (!started_here && connects_at(&self.schematic, at)) {
            self.finish_wire(cx);
        } else {
            cx.notify();
        }
    }

    /// Starts a wire at `at`, leaving along the pin's axis when it is a pin.
    fn start_wire(&mut self, at: Point, cx: &mut Context<Self>) {
        let start_axis = self.pin_axis_at(at);
        self.interaction.wire = Some(WireDraft {
            points: vec![at],
            start_axis,
        });
        cx.notify();
    }

    /// A plain press on a pin in the select tool starts a wire there, as
    /// the wire tool's first click does (D20). The press neither selects
    /// nor moves the part; the selection is cleared, as switching to the
    /// wire tool clears it.
    pub(crate) fn start_pin_wire(&mut self, pin: Point, cx: &mut Context<Self>) {
        self.selection.clear();
        self.hover.clear();
        self.interaction.gesture = Gesture::Idle;
        self.start_wire(pin, cx);
    }

    /// A press in the select tool while a wire started from a pin is being
    /// drawn: the wire tool's click. A double-click on the pin the wire
    /// starts from, before any corner, drops the wire and opens the part's
    /// properties, as a double-click on a pin always did.
    pub(crate) fn pin_wire_press(
        &mut self,
        pointer: Pointer,
        hit: Option<Hit>,
        click_count: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let start = match self.interaction.wire.as_ref().map(|d| d.points.as_slice()) {
            Some([start]) => Some(*start),
            _ => None,
        };
        if click_count >= 2
            && let Some(Hit::Pin { point, .. }) = hit
            && start == Some(point)
        {
            self.interaction.wire = None;
            self.double_click(hit, window, cx);
            return;
        }
        self.wire_click(pointer, click_count, cx);
    }

    fn pin_axis_at(&self, p: Point) -> Option<Axis> {
        self.schematic.components.iter().find_map(|c| {
            let pin = c.pins().iter().position(|&q| q == p)?;
            pin_axis(c, pin)
        })
    }

    /// Commits the wire drawn so far (Enter, double-click, or a click on a
    /// connection point). The active tool stays: the wire tool, or the
    /// select tool for a wire started from a pin.
    pub(crate) fn finish_wire(&mut self, cx: &mut Context<Self>) {
        let Some(draft) = self.interaction.wire.take() else {
            return;
        };
        self.edit(cx, |s| ops::add_wire(s, draft.points));
        cx.notify();
    }

    /// The wire being drawn plus its rubber band to the mouse.
    pub(crate) fn wire_preview(&self) -> Vec<Point> {
        let Some(draft) = &self.interaction.wire else {
            return Vec::new();
        };
        let mut points = draft.points.clone();
        if let Some(pointer) = self.interaction.pointer {
            points.extend(draft_route(
                draft,
                pointer.world.snapped(),
                pointer.any_angle,
            ));
        }
        points
    }

    /// A click with the label tool asks for the new label's name.
    pub(crate) fn label_click(&mut self, at: Vec2, window: &mut Window, cx: &mut Context<Self>) {
        self.open_label_create(at.snapped(), window, cx);
    }
}

/// The route from the draft's last vertex to `to`.
fn draft_route(draft: &WireDraft, to: Point, any_angle: bool) -> Vec<Point> {
    let Some(&last) = draft.points.last() else {
        return Vec::new();
    };
    let previous = match draft.points.as_slice() {
        [.., a, b] => Some((*a, *b)),
        _ => None,
    };
    let start_axis = (draft.points.len() == 1)
        .then_some(draft.start_axis)
        .flatten();
    let axis = first_leg(previous, start_axis, to - last);
    route(last, to, axis, any_angle)
}
