//! Transient view state: cursors, selection, hover, drags and the geometry
//! of the last painted frame. None of it is persisted.

use crate::model::axis::AxisMap;
use crate::model::cursors::{CursorId, CursorTrace, Cursors};
use crate::model::heights::DividerDrag;
use crate::model::measurements::Measurement;
use crate::model::setup::MoveRefusal;
use crate::scene::lod::LodCache;
use gpui_kit::{Bounds, MouseButton, Pixels, Point, ScrollHandle, SharedString, Task};
use std::cell::RefCell;
use std::rc::Rc;
use xsim_project::PlotSetup;

use super::canvas::{CursorLines, Flags};
use super::trace_input::TraceInput;

/// Where a pane's data area was painted, in window coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaneGeometry {
    /// The whole pane canvas.
    pub frame: Bounds<Pixels>,
    /// The data area inside it.
    pub plot: Bounds<Pixels>,
    pub x: AxisMap,
    /// The left y axis.
    pub y: AxisMap,
    /// The right y axis, when the pane plots a second unit (D9). It spans
    /// the same pixels as `y`, so a pointer's fraction is the same on both.
    pub y2: Option<AxisMap>,
    /// The cursor lines painted over the plot (D19: vertical only).
    pub cursors: CursorLines,
    /// Their flags.
    pub flags: Flags,
}

/// Painted geometry per pane, written while painting and read by the mouse
/// handlers of the next events.
pub type Geometry = Rc<RefCell<Vec<Option<PaneGeometry>>>>;

/// The pointer over a pane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hover {
    pub pane: usize,
    pub position: Point<Pixels>,
}

/// A mouse gesture in progress.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Drag {
    /// Left button down, not moved far enough to be a drag yet.
    Pending { pane: usize, start: Point<Pixels> },
    /// Drag-box zoom (the magnifier tool).
    Zoom {
        pane: usize,
        start: Point<Pixels>,
        current: Point<Pixels>,
    },
    /// Left-drag (magnifier off), middle-drag or Space+drag pan.
    Pan { pane: usize, last: Point<Pixels> },
    /// Moving a cursor's line along x (D19); its y follows the trace.
    Cursor(CursorId),
    /// Moving the divider under pane `upper` (D23), pressed at window y
    /// `start`; [`ViewState::resize`] holds the heights it reaches.
    Divider { upper: usize, start: Pixels },
}

/// A divider drag in progress (D23). The panes are laid out with `live`
/// while it lasts; the setup changes only on release.
#[derive(Debug, Clone, PartialEq)]
pub struct PaneResize {
    pub drag: DividerDrag,
    /// The weights the panes are laid out with until the release.
    pub live: Vec<f64>,
    /// Whether the divider is away from where the press began.
    pub moved: bool,
}

/// A stored axis range; `None` = automatic.
pub type StoredRange = Option<[f64; 2]>;

/// How a drag began: the button that began it, so a second button cannot
/// take it over, and what it may change as it was then, so Escape can put
/// it back (D14): the stored axis ranges (`None` = automatic) and the
/// cursors.
#[derive(Debug, Clone, PartialEq)]
pub struct DragOrigin {
    /// The button whose press began the drag; only its release ends it.
    pub button: MouseButton,
    /// The shared x range.
    pub x: StoredRange,
    /// Each pane's left and right y ranges.
    pub y: Vec<(StoredRange, StoredRange)>,
    pub cursors: Cursors,
}

impl DragOrigin {
    pub fn of(button: MouseButton, setup: &PlotSetup, cursors: Cursors) -> Self {
        Self {
            button,
            x: setup.panes.first().and_then(|p| p.x_range),
            y: setup
                .panes
                .iter()
                .map(|p| (p.y_range, p.y2_range))
                .collect(),
            cursors,
        }
    }
}

/// The pane a dragged legend chip is over, and why it would refuse the
/// trace, if it would.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DropTarget {
    pub pane: usize,
    pub refusal: Option<MoveRefusal>,
}

/// Everything the viewer shows that is not part of the saved setup.
#[derive(Default)]
pub struct ViewState {
    pub cursors: Cursors,
    /// The traces cursor 1 and cursor 2 follow; `None` (or a trace that is
    /// gone or hidden) means the default: the selected trace, else the
    /// first visible trace of the active pane.
    pub cursor_traces: [Option<CursorTrace>; 2],
    /// `(pane, trace)` measured by the measurements panel, and the trace a
    /// cursor follows by default. The readout's y1, y2 and Δy come from the
    /// traces the cursors follow (`cursor_traces`), not from this.
    pub selected: Option<(usize, usize)>,
    pub measurements_open: bool,
    /// The magnifier tool: left-drag zooms to a box instead of panning. It
    /// stays on after a zoom until toggled off or Escape.
    pub magnifier: bool,
    /// Where a legend chip being dragged would land.
    pub drop_target: Option<DropTarget>,
    pub hover: Option<Hover>,
    pub drag: Option<Drag>,
    /// The view and cursors when `drag` began.
    pub drag_origin: Option<DragOrigin>,
    /// The pane heights of a divider drag in progress (D23).
    pub resize: Option<PaneResize>,
    /// The divider under the pointer, by the pane above it.
    pub divider_hover: Option<usize>,
    /// Cursor positions before a click moved one, so the double click that
    /// fits the view can put them back.
    pub cursors_before_click: Option<Cursors>,
    pub space_held: bool,
    /// The scroll position of the pane stack, which scrolls when there are
    /// more panes than fit (D22).
    pub panes_scroll: ScrollHandle,
    pub geometry: Geometry,
    pub lod: Rc<RefCell<LodCache>>,
    /// The add-trace field, created on first render (it needs a window).
    pub input: Option<TraceInput>,
    /// A short confirmation shown in the toolbar ("Copied").
    pub flash: Option<SharedString>,
    pub flash_task: Option<Task<()>>,
    /// The last measurements, keyed by trace id and window, so repaints
    /// during hover do not re-measure millions of samples.
    pub measured: RefCell<Option<(MeasureKey, Rc<Vec<Measurement>>)>>,
}

/// What a set of measurements depends on: `(PlotValues::id, from, to)`.
pub type MeasureKey = (u64, u64, u64);

impl ViewState {
    /// The painted geometry of pane `pane`, if it was painted.
    pub fn pane_geometry(&self, pane: usize) -> Option<PaneGeometry> {
        self.geometry.borrow().get(pane).copied().flatten()
    }

    /// Forgets gestures and per-result state when a new result arrives.
    pub fn reset_for_result(&mut self) {
        self.hover = None;
        self.drag = None;
        self.drag_origin = None;
        self.resize = None;
        self.divider_hover = None;
        self.drop_target = None;
        self.cursors.first = None;
        self.cursors.second = None;
        self.geometry.borrow_mut().clear();
    }
}
