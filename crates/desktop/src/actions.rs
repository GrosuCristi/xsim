//! Window commands and their shortcuts (Q26: conventional modern-editor
//! keys; Cmd instead of Ctrl on macOS).
//!
//! Menus dispatch these actions to the workspace, which handles them in one
//! place; the bindings give the menus and tooltips their shortcut hints.

use gpui_kit::{App, KeyBinding, NoAction};

/// Key context of the workspace root element.
pub const WORKSPACE_CONTEXT: &str = "Workspace";

/// Key context of a dialog button that Enter presses, as Space does. In
/// the kit's dialogs Enter otherwise confirms the dialog, whatever has the
/// focus. See [`crate::ui::pressable`].
pub const PRESSABLE_CONTEXT: &str = "PressableButton";

gpui_kit::actions!(
    xsim,
    [
        NewProject,
        OpenProject,
        SaveProject,
        SaveProjectAs,
        CloseProject,
        ExitApplication,
        ExportPng,
        ExportJpeg,
        ExportSvg,
        ExportCsv,
        Undo,
        Redo,
        Cut,
        Copy,
        Paste,
        Duplicate,
        Delete,
        SelectAll,
        UseLightTheme,
        UseDarkTheme,
        ToggleDiagnostics,
        ZoomToFit,
        OpenSettings,
        RunSimulation,
        TogglePause,
        StopSimulation,
        OpenAnalysisSettings,
        CopyPlot,
        NewCircuit,
    ]
);

/// A shortcut with the platform's command modifier: `primary!("s")` is
/// `"ctrl-s"`, or `"cmd-s"` on macOS.
macro_rules! primary {
    ($keys:literal) => {
        if cfg!(target_os = "macos") {
            concat!("cmd-", $keys)
        } else {
            concat!("ctrl-", $keys)
        }
    };
}
pub(crate) use primary;

/// Registers every workspace shortcut.
pub fn bind_keys(cx: &mut App) {
    let context = Some(WORKSPACE_CONTEXT);
    cx.bind_keys([
        KeyBinding::new(primary!("n"), NewProject, context),
        KeyBinding::new(primary!("o"), OpenProject, context),
        KeyBinding::new(primary!("s"), SaveProject, context),
        KeyBinding::new(primary!("shift-s"), SaveProjectAs, context),
        KeyBinding::new(primary!("z"), Undo, context),
        KeyBinding::new(primary!("shift-z"), Redo, context),
        KeyBinding::new(primary!("y"), Redo, context),
        KeyBinding::new(primary!("x"), Cut, context),
        KeyBinding::new(primary!("c"), Copy, context),
        KeyBinding::new(primary!("v"), Paste, context),
        KeyBinding::new(primary!("d"), Duplicate, context),
        KeyBinding::new(primary!("a"), SelectAll, context),
        KeyBinding::new(primary!("j"), ToggleDiagnostics, context),
        KeyBinding::new(primary!(","), OpenSettings, context),
        KeyBinding::new(primary!("shift-c"), CopyPlot, context),
        KeyBinding::new(primary!("t"), NewCircuit, context),
        KeyBinding::new("delete", Delete, context),
        KeyBinding::new("backspace", Delete, context),
        KeyBinding::new("f5", RunSimulation, context),
        KeyBinding::new("f6", TogglePause, context),
        KeyBinding::new("shift-f5", StopSimulation, context),
        // Leaves Enter to the focused button itself.
        KeyBinding::new("enter", NoAction, Some(PRESSABLE_CONTEXT)),
    ]);
}

#[cfg(test)]
mod tests {
    #[test]
    fn shortcuts_use_the_platform_modifier() {
        let expected = if cfg!(target_os = "macos") {
            "cmd-shift-s"
        } else {
            "ctrl-shift-s"
        };
        assert_eq!(primary!("shift-s"), expected);
    }
}
