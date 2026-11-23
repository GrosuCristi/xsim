//! # xsim schematic editor
//!
//! A GPUI view that edits one [`xsim_project::Schematic`]: the schematic
//! toolbar (Q22), the grid canvas with modern CAD interactions (Q47–Q60),
//! part properties and the model browser (Q144), and post-simulation probing
//! (Q62, Q130).
//!
//! The desktop shell owns one [`SchematicEditor`] per circuit tab and reacts
//! to its [`EditorEvent`]s.
//!
//! * [`properties`] — the part properties dialog (values, source waveforms,
//!   models), opened by double-clicking a part.
//! * [`library_browser`] — the component library window (Q54, Q144): category
//!   tree, part list, preview with metadata, search.
//! * [`symbol_art`] — symbol artwork shared by the canvas and the library.
//!
//! Internally the crate is split into GPUI-free editing logic (`model`,
//! `camera`), the painted canvas (`canvas`) and the interactive view (`ui`).
//! Key bindings live in the `SchematicEditor` key context and are installed
//! by the first [`SchematicEditor::new`] (or [`init`]).
//!
//! ## Mouse gestures of the select tool
//!
//! * A click selects an object; Shift or Ctrl/Cmd toggles it. Dragging on
//!   empty canvas draws a selection box (Q47).
//! * Dragging a selected object moves the whole selection, and wires
//!   stretch to keep their connections (Q48, Q60). Arrow keys nudge it the
//!   same way.
//! * **Rubber-band pins (D20).** When a moved part's pin sits directly on a
//!   pin of a part that stays (they touch, with no wire between them), the
//!   move inserts an orthogonal wire between the two pins so the
//!   connection follows. It is part of the same undo step, also for arrow
//!   key nudges and when several parts move and only some partners stay.
//!   The inserted wire avoids every other pin, label and wire whenever a
//!   route with up to two bends allows it, so dragging a touching part past
//!   its partner shorts neither. Once inserted it is an ordinary wire: a
//!   later move or nudge stretches it like any other. Rotating and
//!   mirroring still disconnect (Q60).
//! * **Pins start wires (D20).** A press on a part's pin starts a wire
//!   there, exactly as the wire tool's first click does, whether the button
//!   is then released in place or dragged. Clicks add corners, a click on a
//!   pin, wire or label, a double-click or Enter finishes, Alt draws any
//!   angle and Esc cancels. The select tool stays active, the finished wire
//!   is one undo step, and the press neither selects nor moves the part (the
//!   selection is cleared). Releasing the button after a drag does nothing:
//!   the wire keeps following the mouse until the next click. The cursor
//!   is the wire tool's crosshair over such a pin and while drawing. A press
//!   on the part body still selects and moves the part. Shift or Ctrl/Cmd
//!   on a pin toggles the part's selection instead, a double-click on a pin
//!   opens the part's properties, and zoomed out a press more than 0.8 grid
//!   units from the pin acts on the part (so a ground symbol's bars move
//!   it). A double-click whose first click finishes the wire on a pin,
//!   wire or label only finishes it: its second press opens nothing. In a
//!   read-only editor, and while results are shown (below), a press on a
//!   pin never starts a wire; a wire already being drawn when results
//!   arrive is finished as usual.
//! * **Segment drag (D14).** Dragging a wire that is the only selected
//!   object moves the pressed straight segment sideways, in whole grid
//!   units, KiCad/LTspice style. Pressing an unselected wire selects it
//!   first, so a plain press-drag on any wire does this. The segment moves
//!   by the mouse's displacement rounded to whole units, so it does not
//!   jump when the press lands off its line. Neighbouring segments stretch. An
//!   end held by a pin gets a new bend. Wires that end on the segment
//!   follow it, and labels on it move with it, unless they also connect to
//!   a wire that stays (then a stub joins them). No connection is ever lost
//!   (the rules are in `model/segment.rs`). The drag is one undo step, Esc
//!   aborts it, and a middle press during it is ignored. The moved segment
//!   stays selected, even when cleanup joins its wire into a straight
//!   neighbour. A wire connected to nothing moves as a whole. While the
//!   mouse rests on the selected wire, the cursor shows the direction its
//!   segment drags.
//! * **While results are shown** (a [`ProbeContext`] is set), a press on a
//!   pin is the probe, not the start of a wire; the wire tool still draws
//!   from pins. Clicking a
//!   wire, pin or label probes its node voltage and selects it. Clicking a
//!   part probes its current and selects the part. Press-dragging from an
//!   *unselected* wire, a pin or a label to another node is the
//!   differential probe, which changes neither the schematic nor the
//!   selection. Press-dragging a *selected* wire still edits it, as above:
//!   a segment drag when it is the only selection, otherwise a move of the
//!   selection. A plain click on it still probes. So with results shown,
//!   click a wire, then drag it to realign it. In a read-only editor every
//!   press on a net is the probe.

extern crate gpui_kit as gpui;

mod camera;
mod canvas;
pub mod dialog_window;
pub mod library_browser;
mod model;
pub mod properties;
pub mod symbol_art;
mod ui;

// Interaction tests read element bounds, which only debug builds record.
#[cfg(all(test, debug_assertions))]
mod tests;

pub use model::selection::ObjectRef;

use canvas::ViewportCell;
use gpui_kit::{prelude::*, *};
use library_browser::LibraryItem;
use model::history::History;
use model::selection::Selection;
use std::sync::Arc;
use ui::hover::Hover;
use ui::label_editor::LabelEditor;
use ui::state::{Interaction, Placement, PropertiesOpener};
use xsim_project::{Connectivity, Id, Netlist, Part, PartKind, Point, Schematic, SchematicView};
use xsim_spice_engine::SimulationResult;

/// Active editing tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    /// Select, move, box-select; click a net or part to probe after a run.
    Select,
    /// Draw wires (orthogonal by default; hold Alt for any angle, Q49).
    Wire,
    /// Place a net label.
    Label,
    /// Place a part.
    Place(PartKind),
}

/// A trace the user asked for by probing the schematic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeRequest {
    /// Click on a wire or pin: node voltage to ground, e.g. `V(out)`.
    Voltage { node: String },
    /// Press on one node (not on a selected wire, which a press drags), drag
    /// the probe to another: `V(plus, minus)`.
    Differential { plus: String, minus: String },
    /// Click on a part body: its current, e.g. `I(R1)` or `Ic(Q1)`.
    Current { signal: String },
}

/// Simulation data the editor needs for probing and the hover box.
#[derive(Debug, Clone)]
pub struct ProbeContext {
    /// Netlist the result was computed from (maps grid points to nodes).
    pub netlist: Arc<Netlist>,
    /// The result whose traces probing requests and the hover box measures.
    pub result: Arc<SimulationResult>,
}

/// Events the editor emits to the shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorEvent {
    /// The schematic changed: an edit, undo or redo.
    Changed,
    /// The camera moved or zoomed.
    ViewChanged,
    /// The user probed the schematic (only while a probe context is set).
    Probe(ProbeRequest),
}

/// Edits one schematic.
pub struct SchematicEditor {
    schematic: Arc<Schematic>,
    view: SchematicView,
    tool: Tool,
    read_only: bool,
    probe: Option<ProbeContext>,
    focus: FocusHandle,
    selection: Selection,
    history: History,
    /// Connectivity of `schematic`, computed on demand after each change.
    connectivity: Option<Arc<Connectivity>>,
    placement: Option<Placement>,
    interaction: Interaction,
    hover: Hover,
    label_editor: Option<LabelEditor>,
    viewport: ViewportCell,
    /// Fit the drawing on the next frame (first show of an unset view).
    fit_pending: bool,
    properties_opener: Option<PropertiesOpener>,
    /// A transient message shown in the status strip.
    notice: Option<SharedString>,
}

impl EventEmitter<EditorEvent> for SchematicEditor {}

impl Focusable for SchematicEditor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// Installs the editor's key bindings. [`SchematicEditor::new`] does this
/// on first use, so calling it is optional.
pub fn init(cx: &mut App) {
    ui::actions::ensure_key_bindings(cx);
}

impl SchematicEditor {
    /// An editor for `schematic`, showing it with the saved camera `view`
    /// (fitted to the drawing when the user never moved it).
    pub fn new(schematic: Schematic, view: SchematicView, cx: &mut Context<Self>) -> Self {
        init(cx);
        Self {
            schematic: Arc::new(schematic),
            fit_pending: !view.user_set,
            view,
            tool: Tool::Select,
            read_only: false,
            probe: None,
            focus: cx.focus_handle(),
            selection: Selection::default(),
            history: History::default(),
            connectivity: None,
            placement: None,
            interaction: Interaction::default(),
            hover: Hover::default(),
            label_editor: None,
            viewport: ViewportCell::default(),
            properties_opener: None,
            notice: None,
        }
    }

    /// The schematic being edited.
    pub fn schematic(&self) -> &Schematic {
        &self.schematic
    }

    /// Current camera, for saving in the project (Q39).
    pub fn view(&self) -> SchematicView {
        self.view
    }

    /// The active tool.
    pub fn tool(&self) -> Tool {
        self.tool
    }

    /// Switches tool. `Tool::Place(kind)` places parts with default values;
    /// in read-only mode only [`Tool::Select`] is available.
    pub fn set_tool(&mut self, tool: Tool, cx: &mut Context<Self>) {
        self.switch_tool(tool, cx);
    }

    /// Read-only mode (projects from a newer xsim, Q45): viewing and probing
    /// stay possible, editing is disabled.
    pub fn set_read_only(&mut self, read_only: bool, cx: &mut Context<Self>) {
        self.read_only = read_only;
        self.hover.clear();
        if read_only {
            self.label_editor = None;
            self.switch_tool(Tool::Select, cx);
        }
        cx.notify();
    }

    /// Whether editing is disabled.
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Enables probing and hover measurements for `probe`, or disables them.
    pub fn set_probe_context(&mut self, probe: Option<ProbeContext>, cx: &mut Context<Self>) {
        self.probe = probe;
        self.hover.clear();
        cx.notify();
    }

    /// Whether [`Self::undo`] would change anything.
    pub fn can_undo(&self) -> bool {
        !self.read_only && self.history.can_undo()
    }

    /// Whether [`Self::redo`] would change anything.
    pub fn can_redo(&self) -> bool {
        !self.read_only && self.history.can_redo()
    }

    /// Reverts the last edit (Q57).
    pub fn undo(&mut self, cx: &mut Context<Self>) {
        self.step_history(true, cx);
    }

    /// Re-applies the last undone edit.
    pub fn redo(&mut self, cx: &mut Context<Self>) {
        self.step_history(false, cx);
    }

    /// Zooms and pans so the whole drawing is visible.
    pub fn fit_to_view(&mut self, cx: &mut Context<Self>) {
        self.fit_now(cx);
    }

    /// Enters placement mode for a specific part (e.g. a transistor with a
    /// model picked in the library browser). Each click places a copy.
    pub fn start_placing(&mut self, part: Part, cx: &mut Context<Self>) {
        self.begin_placement(part, cx);
    }

    /// Replaces the designator and parameters of component `id` as one
    /// undoable edit. Fails without changing anything, and with the reason
    /// as the error, if `name` is empty, holds anything but ASCII letters,
    /// digits and `_` (such names break probe expressions like `I(R1)`), or
    /// is used by another part, or if `part` has a different [`PartKind`].
    pub fn apply_component_edit(
        &mut self,
        id: Id,
        name: String,
        part: Part,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if self.read_only {
            return Err("The schematic is read-only".into());
        }
        let mut next = (*self.schematic).clone();
        if model::ops::edit_component(&mut next, id, &name, part)? {
            self.commit(next, cx);
        }
        Ok(())
    }

    /// Starts placing a part picked in the component library.
    pub fn place_library_item(&mut self, item: LibraryItem, cx: &mut Context<Self>) {
        self.begin_placement(item.to_part(), cx);
    }

    /// The part being placed, while [`Tool::Place`] is active.
    pub fn placing(&self) -> Option<&Part> {
        self.placement.as_ref().map(|p| &p.part)
    }

    /// The selected objects.
    pub fn selection(&self) -> Vec<ObjectRef> {
        self.selection.iter().collect()
    }

    /// Replaces the selection; objects that do not exist are ignored.
    pub fn set_selection(
        &mut self,
        objects: impl IntoIterator<Item = ObjectRef>,
        cx: &mut Context<Self>,
    ) {
        self.selection = objects.into_iter().collect();
        self.selection.retain_existing(&self.schematic);
        cx.notify();
    }

    /// Replaces what double-clicking a part opens (by default
    /// [`properties::open_properties`]).
    pub fn set_properties_opener(
        &mut self,
        opener: impl Fn(&Entity<SchematicEditor>, Id, &mut Window, &mut App) + 'static,
    ) {
        self.properties_opener = Some(std::rc::Rc::new(opener));
    }

    /// Window position of grid point `p`, once the canvas has been laid out.
    pub fn grid_to_window(&self, p: Point) -> Option<gpui::Point<Pixels>> {
        let viewport = self.viewport.get()?;
        let screen = self
            .camera()
            .to_screen(&viewport, model::geometry::Vec2::from_grid(p));
        Some(point(px(screen.x), px(screen.y)))
    }

    /// Opens the properties of part `id` after the current event.
    fn open_properties(&mut self, id: Id, window: &mut Window, cx: &mut Context<Self>) {
        let entity = cx.entity();
        let opener = self.properties_opener.clone();
        window.defer(cx, move |window, cx| match opener {
            Some(open) => open(&entity, id, window, cx),
            None => properties::open_properties(&entity, id, window, cx),
        });
    }
}
