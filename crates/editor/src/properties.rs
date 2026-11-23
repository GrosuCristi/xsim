//! Part properties dialog.
//!
//! One form per part kind, all values typed with SI suffixes and validated
//! while editing (Q75, Q76): resistance/capacitance/inductance and initial
//! conditions; independent sources with DC value, AC magnitude/phase and a
//! transient waveform (none, PULSE, SINE, EXP, PWL, SFFM; Q72); semiconductor
//! model choice through the [`crate::library_browser`]; switch models (D17):
//! RON, ROFF, threshold and hysteresis, checked as the engine checks them,
//! and a current switch's control source, picked from a drop-down of the
//! schematic's voltage sources. Applying calls
//! [`crate::SchematicEditor::apply_component_edit`].
//!
//! The form logic (part ↔ field texts, validation, waveform tables) lives in
//! GPUI-free modules: `spec`, `waveform`, `pwl`, `form` and `preview`. The
//! view, its sections (`sections`, `transient`), the field widget and the
//! dialog wiring build on them.

mod dialog;
mod field;
mod form;
mod preview;
mod pwl;
mod sections;
mod sparkline;
mod spec;
mod transient;
mod view;
mod waveform;

use crate::SchematicEditor;
use gpui_kit::{App, Entity, Window};
use xsim_project::Id;

/// Opens the properties dialog for component `id` of `editor`'s schematic.
///
/// OK, Apply and Enter write the form as one undoable edit through
/// [`SchematicEditor::apply_component_edit`] (nothing is written when no
/// value changed); invalid fields block applying and show their error.
/// Cancel and Escape discard the form. The dialog opens once the current
/// update finishes, so this may be called while `editor` is being updated
/// (e.g. from its double-click handler).
pub fn open_properties(
    editor: &Entity<SchematicEditor>,
    id: Id,
    window: &mut Window,
    cx: &mut App,
) {
    let editor = editor.clone();
    window.defer(cx, move |window, cx| dialog::open(editor, id, window, cx));
}
