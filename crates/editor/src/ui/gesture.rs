//! Select-tool mouse gestures: click selection, box selection, moving the
//! selection, dragging a wire segment, starting a wire from a pin, panning,
//! and probing after a simulation (Q47, Q48, Q62, D14, D20). The rules are
//! listed in the crate docs.

use crate::model::drag::MovePlan;
use crate::model::geometry::{Rect, Vec2};
use crate::model::hit_test::{BoxMode, Hit, hit_test, objects_in_box};
use crate::model::probe;
use crate::model::routing::Axis;
use crate::model::segment::{self, SegmentPlan};
use crate::model::selection::{ObjectRef, Selection};
use crate::ui::state::Pointer;
use crate::{EditorEvent, SchematicEditor, Tool};
use gpui_kit::{Context, Modifiers, Window};
use std::sync::Arc;
use xsim_project::{Connectivity, Point, Schematic, connectivity};

/// Pixels the mouse must travel before a press becomes a drag.
const DRAG_THRESHOLD: f32 = 4.0;
/// Hit tolerance in pixels.
pub const HIT_TOLERANCE_PX: f32 = 7.0;
/// Farthest a press may land from a pin, in grid units, to start a wire
/// from it in the select tool (D20). Zoomed out, the pixel hit tolerance
/// covers most of a small part, which must stay movable: a press on the pin
/// beyond this radius acts on the part, as on its body. It is shorter than
/// the ground symbol's lead (0.9), so its bars always move it.
pub const PIN_WIRE_RADIUS: f32 = 0.8;

/// What the left (or middle) button is doing.
#[derive(Default)]
pub enum Gesture {
    #[default]
    Idle,
    /// Pressed, not yet dragged past the threshold.
    Pressed(Press),
    /// Dragging the selection or a wire segment.
    Moving(Box<MoveDrag>),
    /// Dragging a selection rectangle (world corners).
    BoxSelect {
        start: Vec2,
        current: Vec2,
        additive: bool,
    },
    /// Dragging the view (middle button or Space + left button).
    Panning { last: Vec2 },
    /// Holding the probe pressed on a net.
    Probing(ProbeDrag),
}

/// A press waiting to become a click or a drag.
pub struct Press {
    pub screen: Vec2,
    pub world: Vec2,
    pub hit: Option<Hit>,
    /// Shift or the primary modifier: toggle instead of replace.
    pub toggle: bool,
    /// The pressed object was already selected before the press.
    pub was_selected: bool,
    /// The node a click probes: set when the press landed on a selected
    /// wire while results are shown, where a drag edits instead.
    pub probe: Option<String>,
}

/// How a drag changes the schematic.
pub enum DragPlan {
    /// The selection moves; wires stretch (Q48, Q60).
    Selection(MovePlan),
    /// One straight run of the only selected wire moves sideways (D14).
    Segment(SegmentPlan),
}

impl DragPlan {
    fn apply(&self, delta: Point) -> Schematic {
        match self {
            DragPlan::Selection(plan) => plan.apply(delta),
            DragPlan::Segment(plan) => plan.apply(delta),
        }
    }

    /// The axis a segment drag moves along.
    pub fn segment_axis(&self) -> Option<Axis> {
        match self {
            DragPlan::Segment(plan) => Some(plan.normal()),
            DragPlan::Selection(_) => None,
        }
    }
}

/// A drag in progress; the preview is recomputed from the plan for the
/// total displacement.
pub struct MoveDrag {
    pub plan: DragPlan,
    /// Where the press landed (world units).
    pub press: Vec2,
    pub delta: Point,
    pub preview: Arc<Schematic>,
    pub preview_connectivity: Arc<Connectivity>,
}

/// The probe held down on the net `plus`.
pub struct ProbeDrag {
    pub plus: String,
    pub start: Vec2,
    pub at: Vec2,
    pub dragged: bool,
    /// What was pressed: a click (no drag) selects it.
    pub object: ObjectRef,
    /// Shift or the primary modifier: a click toggles the selection.
    pub toggle: bool,
}

/// What a left press on a selected wire starts when dragged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireDrag {
    /// A segment drag along this axis.
    Segment(Axis),
    /// Moving the whole selection.
    Selection,
}

/// The grid point identifying the net of a pin, wire or label hit.
pub fn net_point(schematic: &Schematic, hit: Hit) -> Option<Point> {
    match hit {
        Hit::Pin { point, .. } => Some(point),
        Hit::Wire { wire, segment } => schematic.wire(wire)?.points.get(segment).copied(),
        Hit::Label(id) => schematic.label(id).map(|l| l.position),
        Hit::Component(_) => None,
    }
}

impl SchematicEditor {
    /// World tolerance for hit testing at the current zoom.
    pub(crate) fn hit_tolerance(&self) -> f32 {
        let scale = self
            .viewport
            .get()
            .map_or(self.view.zoom, |vp| self.camera().scale(&vp));
        HIT_TOLERANCE_PX / scale.max(0.1)
    }

    /// The object under `world`.
    pub(crate) fn hit_at(&self, world: Vec2) -> Option<Hit> {
        hit_test(&self.schematic, world, self.hit_tolerance())
    }

    /// Left press with the select tool.
    pub(crate) fn select_press(
        &mut self,
        pointer: Pointer,
        modifiers: Modifiers,
        click_count: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if click_count >= 2 && self.interaction.wire_finished {
            return;
        }
        self.interaction.wire_finished = false;
        let hit = self.hit_at(pointer.world);
        if self.interaction.wire.is_some() {
            self.pin_wire_press(pointer, hit, click_count, window, cx);
            self.interaction.wire_finished = self.interaction.wire.is_none();
            return;
        }
        if click_count >= 2 {
            self.double_click(hit, window, cx);
            return;
        }
        let toggle = modifiers.shift || modifiers.secondary(cx);
        if !toggle && let Some(pin) = self.wire_start_at(pointer.world, hit) {
            self.start_pin_wire(pin, cx);
            return;
        }
        let node = hit.and_then(|h| self.probe_node(h));
        // While results are shown, a press on a selected wire still edits
        // it; anywhere else on a net it is the probe.
        let edits = !toggle && self.wire_drag_at(hit).is_some();
        if let (Some(plus), Some(hit), false) = (&node, hit, edits) {
            self.interaction.gesture = Gesture::Probing(ProbeDrag {
                plus: plus.clone(),
                start: pointer.screen,
                at: pointer.world,
                dragged: false,
                object: hit.object(),
                toggle,
            });
            return;
        }
        let object = hit.map(Hit::object);
        let was_selected = object.is_some_and(|o| self.selection.contains(o));
        match object {
            Some(o) if toggle => self.selection.toggle(o),
            Some(o) if !was_selected => self.selection = [o].into_iter().collect(),
            None if !toggle => self.selection.clear(),
            _ => {}
        }
        self.interaction.gesture = Gesture::Pressed(Press {
            screen: pointer.screen,
            world: pointer.world,
            hit,
            toggle,
            was_selected,
            probe: node,
        });
    }

    /// The simulated node of a pin, wire or label hit, while results are
    /// shown.
    fn probe_node(&self, hit: Hit) -> Option<String> {
        let probe = self.probe.as_ref()?;
        let point = net_point(&self.schematic, hit)?;
        probe.netlist.node_name_at(point).map(str::to_string)
    }

    /// What dragging from `hit` would do when it is a selected wire of an
    /// editable schematic.
    pub(crate) fn wire_drag_at(&self, hit: Option<Hit>) -> Option<WireDrag> {
        let Some(Hit::Wire { wire, segment }) = hit else {
            return None;
        };
        if self.read_only || !self.selection.has_wire(wire) {
            return None;
        }
        let only = self.selection.len() == 1;
        match segment::drag_axis(&self.schematic, wire, segment) {
            Some(axis) if only => Some(WireDrag::Segment(axis)),
            _ => Some(WireDrag::Selection),
        }
    }

    /// The pin a plain left press at `world` (on `hit`) starts a wire from
    /// in the select tool (D20): only in an editable schematic, while no
    /// results are shown (a press on a pin then probes) and no wire is
    /// being drawn, and only within [`PIN_WIRE_RADIUS`] of the pin.
    pub(crate) fn wire_start_at(&self, world: Vec2, hit: Option<Hit>) -> Option<Point> {
        let Some(Hit::Pin { point, .. }) = hit else {
            return None;
        };
        let idle = self.tool == Tool::Select && self.interaction.wire.is_none();
        if !idle || self.read_only || self.probe.is_some() {
            return None;
        }
        let radius = self.hit_tolerance().min(PIN_WIRE_RADIUS);
        (world.distance(Vec2::from_grid(point)) <= radius).then_some(point)
    }

    /// The pin a plain left press at the mouse would start a wire from, for
    /// the cursor. Computed when painting, so the cursor is right after an
    /// edit, Esc or new results even before the mouse moves.
    pub(crate) fn wire_start_under_pointer(&self) -> Option<Point> {
        if self.interaction.space_held {
            return None;
        }
        let pointer = self.interaction.pointer?;
        self.wire_start_at(pointer.world, self.hit_at(pointer.world))
    }

    /// What a left press at the mouse would start on a selected wire, for
    /// the cursor and the probe glyph.
    pub(crate) fn wire_drag_under_pointer(&self) -> Option<WireDrag> {
        if self.tool != Tool::Select
            || self.interaction.space_held
            || self.interaction.wire.is_some()
        {
            return None;
        }
        let pointer = self.interaction.pointer?;
        self.wire_drag_at(self.hit_at(pointer.world))
    }

    pub(crate) fn double_click(
        &mut self,
        hit: Option<Hit>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.interaction.gesture = Gesture::Idle;
        if self.read_only {
            return;
        }
        match hit {
            Some(Hit::Component(id) | Hit::Pin { component: id, .. }) => {
                self.selection = [ObjectRef::Component(id)].into_iter().collect();
                self.open_properties(id, window, cx);
            }
            Some(Hit::Label(id)) => {
                self.selection = [ObjectRef::Label(id)].into_iter().collect();
                self.open_label_rename(id, window, cx);
            }
            _ => {}
        }
    }

    /// Mouse moved with the left button down in the select tool.
    pub(crate) fn select_drag(&mut self, pointer: Pointer) {
        let gesture = std::mem::take(&mut self.interaction.gesture);
        self.interaction.gesture = match gesture {
            Gesture::Pressed(press) if press.screen.distance(pointer.screen) >= DRAG_THRESHOLD => {
                self.begin_drag(press, pointer)
            }
            Gesture::Moving(mut drag) => {
                self.update_move(&mut drag, pointer);
                Gesture::Moving(drag)
            }
            Gesture::BoxSelect {
                start, additive, ..
            } => Gesture::BoxSelect {
                start,
                current: pointer.world,
                additive,
            },
            Gesture::Probing(mut drag) => {
                drag.dragged |= drag.start.distance(pointer.screen) >= DRAG_THRESHOLD;
                drag.at = pointer.world;
                Gesture::Probing(drag)
            }
            other => other,
        };
    }

    fn begin_drag(&mut self, press: Press, pointer: Pointer) -> Gesture {
        match press.hit {
            None => Gesture::BoxSelect {
                start: press.world,
                current: pointer.world,
                additive: press.toggle,
            },
            Some(_) if self.read_only || press.toggle || self.selection.is_empty() => Gesture::Idle,
            Some(hit) => {
                let mut drag = Box::new(MoveDrag {
                    plan: self.drag_plan(hit),
                    press: press.world,
                    delta: Point::default(),
                    preview: self.schematic.clone(),
                    preview_connectivity: self.connectivity(),
                });
                self.update_move(&mut drag, pointer);
                Gesture::Moving(drag)
            }
        }
    }

    /// A segment drag when the pressed wire is the only selected object
    /// (and is connected to something), else moving the whole selection.
    fn drag_plan(&self, hit: Hit) -> DragPlan {
        if let Hit::Wire { wire, segment } = hit
            && self.selection.len() == 1
            && self.selection.has_wire(wire)
            && let Some(plan) = SegmentPlan::new(&self.schematic, wire, segment)
        {
            return DragPlan::Segment(plan);
        }
        DragPlan::Selection(MovePlan::new(&self.schematic, &self.selection))
    }

    fn update_move(&self, drag: &mut MoveDrag, pointer: Pointer) {
        let delta = match drag.plan {
            // The pressed segment lies on a grid line, but the press may be
            // nearer the next line (the hit tolerance exceeds half a unit
            // at most zooms): the mouse's own displacement, rounded, moves
            // it, so it neither jumps at the start nor lags behind.
            DragPlan::Segment(_) => (pointer.world - drag.press).snapped(),
            // Objects keep their grid offset from the snapped mouse.
            DragPlan::Selection(_) => pointer.world.snapped() - drag.press.snapped(),
        };
        if delta == drag.delta {
            return;
        }
        drag.delta = delta;
        let preview = drag.plan.apply(delta);
        drag.preview_connectivity = Arc::new(connectivity(&preview));
        drag.preview = Arc::new(preview);
    }

    /// Left button released in the select tool.
    pub(crate) fn select_release(&mut self, pointer: Option<Pointer>, cx: &mut Context<Self>) {
        match std::mem::take(&mut self.interaction.gesture) {
            Gesture::Pressed(press) => self.click(press, cx),
            Gesture::Moving(drag) => self.finish_move(*drag, cx),
            Gesture::BoxSelect {
                start,
                current,
                additive,
            } => self.finish_box(start, current, additive),
            Gesture::Probing(drag) => self.finish_probe(drag, pointer, cx),
            Gesture::Idle | Gesture::Panning { .. } => {}
        }
    }

    /// Commits a drag as one undo step.
    fn finish_move(&mut self, drag: MoveDrag, cx: &mut Context<Self>) {
        let MoveDrag {
            plan,
            delta,
            preview,
            ..
        } = drag;
        if delta == Point::default() || !self.commit(Arc::unwrap_or_clone(preview), cx) {
            return;
        }
        // Cleanup may have joined the dragged wire into a straight
        // neighbour: keep the moved run selected.
        if let DragPlan::Segment(plan) = &plan
            && let Some(id) = plan.dragged_wire(&self.schematic, delta)
        {
            self.selection = [ObjectRef::Wire(id)].into_iter().collect();
        }
    }

    fn click(&mut self, press: Press, cx: &mut Context<Self>) {
        let Some(hit) = press.hit else {
            return;
        };
        if press.was_selected && !press.toggle {
            self.selection = [hit.object()].into_iter().collect();
        }
        if let Some(request) = press.probe.as_deref().and_then(probe::voltage) {
            cx.emit(EditorEvent::Probe(request));
        }
        if let Hit::Component(id) = hit
            && let Some(probe) = &self.probe
            && probe.netlist.elements.contains_key(&id)
            && let Some(signal) = self.schematic.component(id).and_then(probe::current_signal)
        {
            cx.emit(EditorEvent::Probe(crate::ProbeRequest::Current { signal }));
        }
    }

    fn finish_box(&mut self, start: Vec2, current: Vec2, additive: bool) {
        let rect = Rect::from_corners(start, current);
        let mode = BoxMode::for_drag(start, current);
        let picked = objects_in_box(&self.schematic, rect, mode);
        if additive {
            self.selection.extend(picked);
        } else {
            self.selection = picked.into_iter().collect::<Selection>();
        }
    }

    fn finish_probe(&mut self, drag: ProbeDrag, pointer: Option<Pointer>, cx: &mut Context<Self>) {
        let request = if drag.dragged {
            let minus = pointer.and_then(|p| self.node_under(p.world));
            minus.and_then(|minus| probe::differential(&drag.plus, &minus))
        } else {
            // A click also selects what it probed, as a click on a part
            // does, so a wire can then be dragged (see the crate docs).
            if drag.toggle {
                self.selection.toggle(drag.object);
            } else {
                self.selection = [drag.object].into_iter().collect();
            }
            probe::voltage(&drag.plus)
        };
        if let Some(request) = request {
            cx.emit(EditorEvent::Probe(request));
        }
    }

    /// The simulated node under a world position, while probing.
    pub(crate) fn node_under(&self, world: Vec2) -> Option<String> {
        let probe = self.probe.as_ref()?;
        let point = net_point(&self.schematic, self.hit_at(world)?)?;
        probe.netlist.node_name_at(point).map(str::to_string)
    }
}
