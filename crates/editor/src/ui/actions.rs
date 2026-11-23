//! Editor actions and their modern-editor key bindings (Q26).
//!
//! Bindings live in the `SchematicEditor` key context. Single keys and
//! editing chords are disabled while a text field inside the editor has
//! focus (`!Input`), so typing a net name never rotates or deletes anything.
//! The primary modifier is Cmd on macOS and Ctrl elsewhere.

use crate::model::transform::Mirror;
use crate::{SchematicEditor, Tool};
use gpui_kit::{App, Context, Div, Global, InteractiveElement, KeyBinding};
use xsim_project::PartKind;

/// The key context of the editor.
pub const CONTEXT: &str = "SchematicEditor";
/// Context predicate for bindings that must not fire while typing.
const NOT_TYPING: &str = "SchematicEditor && !Input";

gpui_kit::actions!(
    schematic_editor,
    [
        Undo,
        Redo,
        Copy,
        Cut,
        Paste,
        Duplicate,
        Delete,
        SelectAll,
        Cancel,
        Confirm,
        Rotate,
        MirrorHorizontal,
        MirrorVertical,
        ZoomToFit,
        ZoomIn,
        ZoomOut,
        NudgeLeft,
        NudgeRight,
        NudgeUp,
        NudgeDown,
        SelectTool,
        WireTool,
        LabelTool,
        OpenLibrary,
        PlaceVoltageSource,
        PlaceCurrentSource,
        PlaceResistor,
        PlaceInductor,
        PlaceCapacitor,
        PlaceDiode,
        PlaceNpn,
        PlacePnp,
        PlaceGround,
    ]
);

/// Marks the bindings as installed in this application.
struct KeysBound;
impl Global for KeysBound {}

/// Installs the editor key bindings once per application.
pub fn ensure_key_bindings(cx: &mut App) {
    if cx.has_global::<KeysBound>() {
        return;
    }
    cx.set_global(KeysBound);
    bind_keys(cx);
}

/// `key` with the primary modifier: Cmd on macOS, Ctrl elsewhere.
macro_rules! primary {
    ($key:literal) => {
        if cfg!(target_os = "macos") {
            concat!("cmd-", $key)
        } else {
            concat!("ctrl-", $key)
        }
    };
}

fn bind_keys(cx: &mut App) {
    let typing_safe = Some(NOT_TYPING);
    cx.bind_keys([
        KeyBinding::new(primary!("z"), Undo, typing_safe),
        KeyBinding::new(primary!("shift-z"), Redo, typing_safe),
        KeyBinding::new(primary!("y"), Redo, typing_safe),
        KeyBinding::new(primary!("c"), Copy, typing_safe),
        KeyBinding::new(primary!("x"), Cut, typing_safe),
        KeyBinding::new(primary!("v"), Paste, typing_safe),
        KeyBinding::new(primary!("d"), Duplicate, typing_safe),
        KeyBinding::new(primary!("a"), SelectAll, typing_safe),
        KeyBinding::new("delete", Delete, typing_safe),
        KeyBinding::new("backspace", Delete, typing_safe),
        KeyBinding::new("escape", Cancel, Some(CONTEXT)),
        KeyBinding::new("enter", Confirm, typing_safe),
        KeyBinding::new("r", Rotate, typing_safe),
        KeyBinding::new("x", MirrorHorizontal, typing_safe),
        KeyBinding::new("y", MirrorVertical, typing_safe),
        KeyBinding::new("f", ZoomToFit, typing_safe),
        KeyBinding::new("=", ZoomIn, typing_safe),
        KeyBinding::new("+", ZoomIn, typing_safe),
        KeyBinding::new("-", ZoomOut, typing_safe),
        KeyBinding::new("left", NudgeLeft, typing_safe),
        KeyBinding::new("right", NudgeRight, typing_safe),
        KeyBinding::new("up", NudgeUp, typing_safe),
        KeyBinding::new("down", NudgeDown, typing_safe),
        KeyBinding::new("s", SelectTool, typing_safe),
        KeyBinding::new("w", WireTool, typing_safe),
        KeyBinding::new("n", LabelTool, typing_safe),
        KeyBinding::new("p", OpenLibrary, typing_safe),
        KeyBinding::new("v", PlaceVoltageSource, typing_safe),
        KeyBinding::new("i", PlaceCurrentSource, typing_safe),
        KeyBinding::new("shift-r", PlaceResistor, typing_safe),
        KeyBinding::new("l", PlaceInductor, typing_safe),
        KeyBinding::new("c", PlaceCapacitor, typing_safe),
        KeyBinding::new("d", PlaceDiode, typing_safe),
        KeyBinding::new("q", PlaceNpn, typing_safe),
        KeyBinding::new("shift-q", PlacePnp, typing_safe),
        KeyBinding::new("g", PlaceGround, typing_safe),
    ]);
}

/// The action that places `kind`, for tooltips and the toolbar.
pub fn place_action(kind: PartKind) -> Option<Box<dyn gpui_kit::Action>> {
    let action: Box<dyn gpui_kit::Action> = match kind {
        PartKind::VoltageSource => Box::new(PlaceVoltageSource),
        PartKind::CurrentSource => Box::new(PlaceCurrentSource),
        PartKind::Resistor => Box::new(PlaceResistor),
        PartKind::Inductor => Box::new(PlaceInductor),
        PartKind::Capacitor => Box::new(PlaceCapacitor),
        PartKind::Diode => Box::new(PlaceDiode),
        PartKind::NpnBjt => Box::new(PlaceNpn),
        PartKind::PnpBjt => Box::new(PlacePnp),
        PartKind::Ground => Box::new(PlaceGround),
        // Placed from the component library only, not the toolbar (D6,
        // D17, D26).
        PartKind::NJfet
        | PartKind::PJfet
        | PartKind::VoltageSwitch
        | PartKind::CurrentSwitch
        | PartKind::OpAmp => {
            return None;
        }
    };
    Some(action)
}

/// Attaches every action handler to the editor's root element.
pub fn register(root: Div, cx: &mut Context<SchematicEditor>) -> Div {
    let root = register_editing(root, cx);
    let root = register_tools(root, cx);
    let root = register_parts(root, cx);
    register_view(root, cx)
}

fn register_editing(root: Div, cx: &mut Context<SchematicEditor>) -> Div {
    root.on_action(cx.listener(|e, _: &Undo, _, cx| e.undo(cx)))
        .on_action(cx.listener(|e, _: &Redo, _, cx| e.redo(cx)))
        .on_action(cx.listener(|e, _: &Copy, _, cx| e.copy(cx)))
        .on_action(cx.listener(|e, _: &Cut, _, cx| e.cut(cx)))
        .on_action(cx.listener(|e, _: &Paste, _, cx| e.paste(cx)))
        .on_action(cx.listener(|e, _: &Duplicate, _, cx| e.duplicate(cx)))
        .on_action(cx.listener(|e, _: &Delete, _, cx| e.delete_selection(cx)))
        .on_action(cx.listener(|e, _: &SelectAll, _, cx| e.select_all(cx)))
        .on_action(cx.listener(|e, _: &Rotate, _, cx| e.rotate(cx)))
        .on_action(cx.listener(|e, _: &MirrorHorizontal, _, cx| e.mirror(Mirror::Horizontal, cx)))
        .on_action(cx.listener(|e, _: &MirrorVertical, _, cx| e.mirror(Mirror::Vertical, cx)))
}

fn register_tools(root: Div, cx: &mut Context<SchematicEditor>) -> Div {
    root.on_action(cx.listener(|e, _: &Cancel, window, cx| e.cancel(window, cx)))
        .on_action(cx.listener(|e, _: &Confirm, _, cx| e.confirm(cx)))
        .on_action(cx.listener(|e, _: &SelectTool, _, cx| e.set_tool(Tool::Select, cx)))
        .on_action(cx.listener(|e, _: &WireTool, _, cx| e.set_tool(Tool::Wire, cx)))
        .on_action(cx.listener(|e, _: &LabelTool, _, cx| e.set_tool(Tool::Label, cx)))
        .on_action(cx.listener(|e, _: &OpenLibrary, window, cx| e.open_library(window, cx)))
}

fn register_parts(root: Div, cx: &mut Context<SchematicEditor>) -> Div {
    use PartKind::*;
    let place = |e: &mut SchematicEditor, kind, cx: &mut Context<SchematicEditor>| {
        e.set_tool(Tool::Place(kind), cx)
    };
    root.on_action(cx.listener(move |e, _: &PlaceVoltageSource, _, cx| place(e, VoltageSource, cx)))
        .on_action(cx.listener(move |e, _: &PlaceCurrentSource, _, cx| place(e, CurrentSource, cx)))
        .on_action(cx.listener(move |e, _: &PlaceResistor, _, cx| place(e, Resistor, cx)))
        .on_action(cx.listener(move |e, _: &PlaceInductor, _, cx| place(e, Inductor, cx)))
        .on_action(cx.listener(move |e, _: &PlaceCapacitor, _, cx| place(e, Capacitor, cx)))
        .on_action(cx.listener(move |e, _: &PlaceDiode, _, cx| place(e, Diode, cx)))
        .on_action(cx.listener(move |e, _: &PlaceNpn, _, cx| place(e, NpnBjt, cx)))
        .on_action(cx.listener(move |e, _: &PlacePnp, _, cx| place(e, PnpBjt, cx)))
        .on_action(cx.listener(move |e, _: &PlaceGround, _, cx| place(e, Ground, cx)))
}

fn register_view(root: Div, cx: &mut Context<SchematicEditor>) -> Div {
    root.on_action(cx.listener(|e, _: &ZoomToFit, _, cx| e.fit_to_view(cx)))
        .on_action(cx.listener(|e, _: &ZoomIn, _, cx| e.zoom_by(1.25, cx)))
        .on_action(cx.listener(|e, _: &ZoomOut, _, cx| e.zoom_by(0.8, cx)))
        .on_action(cx.listener(|e, _: &NudgeLeft, _, cx| e.nudge(-1, 0, cx)))
        .on_action(cx.listener(|e, _: &NudgeRight, _, cx| e.nudge(1, 0, cx)))
        .on_action(cx.listener(|e, _: &NudgeUp, _, cx| e.nudge(0, -1, cx)))
        .on_action(cx.listener(|e, _: &NudgeDown, _, cx| e.nudge(0, 1, cx)))
}
