//! Keyboard actions of the waveform viewer (Q26). Bindings use Ctrl, or Cmd
//! on macOS, and only apply while focus is inside the viewer.

use gpui_kit::{App, Global, KeyBinding};

/// Key context of the viewer root element.
pub const CONTEXT: &str = "WaveformViewer";

gpui_kit::actions!(
    waveform_viewer,
    [
        /// Copy the plot to the clipboard as an image.
        CopyPlot,
        /// Return every axis to its automatic range.
        ZoomToFit,
        /// Remove the selected trace.
        RemoveSelectedTrace,
        /// Cancel a drag in progress.
        CancelInteraction
    ]
);

struct Bound;

impl Global for Bound {}

/// Registers the viewer's key bindings. [`crate::WaveformViewer::new`]
/// calls it; calling it again does nothing.
pub fn init(cx: &mut App) {
    if cx.has_global::<Bound>() {
        return;
    }
    cx.set_global(Bound);
    let (copy, fit) = if cfg!(target_os = "macos") {
        ("cmd-c", "cmd-0")
    } else {
        ("ctrl-c", "ctrl-0")
    };
    cx.bind_keys([
        KeyBinding::new(copy, CopyPlot, Some(CONTEXT)),
        KeyBinding::new(fit, ZoomToFit, Some(CONTEXT)),
        KeyBinding::new("delete", RemoveSelectedTrace, Some(CONTEXT)),
        KeyBinding::new("backspace", RemoveSelectedTrace, Some(CONTEXT)),
        KeyBinding::new("escape", CancelInteraction, Some(CONTEXT)),
    ]);
}
