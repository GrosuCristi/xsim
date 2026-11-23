//! Edit, view and plot commands.

use super::Workspace;
use crate::appearance;
use crate::diagnostics::{Severity, Source};
use crate::files;
use crate::ids::TabId;
use crate::prompts::ExportKind;
use gpui_kit::{prelude::*, *};
use std::path::PathBuf;
use xsim_project::appdata::ThemePreference;

/// Size of exported raster images, in pixels.
const EXPORT_SIZE: (u32, u32) = (1600, 1000);

impl Workspace {
    /// Edit > Undo in the active schematic.
    pub(super) fn undo(&mut self, cx: &mut Context<Self>) {
        let editor = self.active_tab().editor.clone();
        editor.update(cx, |editor, cx| {
            if editor.can_undo() {
                editor.undo(cx);
            }
        });
    }

    /// Edit > Redo in the active schematic.
    pub(super) fn redo(&mut self, cx: &mut Context<Self>) {
        let editor = self.active_tab().editor.clone();
        editor.update(cx, |editor, cx| {
            if editor.can_redo() {
                editor.redo(cx);
            }
        });
    }

    /// Sends an edit shortcut (copy, paste, delete, ...) to the schematic
    /// editor, which owns the selection and knows whether it is read-only.
    /// When the editor already has focus it did not handle the keys itself,
    /// so nothing is forwarded.
    pub(super) fn forward_to_editor(
        &mut self,
        keystroke: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let handle = self.active_tab().editor.focus_handle(cx);
        if handle.contains_focused(window, cx) {
            return;
        }
        handle.focus(window, cx);
        window.defer(cx, move |window, cx| {
            if let Ok(keystroke) = Keystroke::parse(keystroke) {
                window.dispatch_keystroke(keystroke, cx);
            }
        });
    }

    /// View > Zoom to fit: shows the whole drawing.
    pub(super) fn zoom_to_fit(&mut self, cx: &mut Context<Self>) {
        let editor = self.active_tab().editor.clone();
        editor.update(cx, |editor, cx| editor.fit_to_view(cx));
    }

    /// Opens or closes the diagnostics dock (navigation only).
    pub(super) fn toggle_diagnostics(&mut self, cx: &mut Context<Self>) {
        let layout = &mut self.doc.project.layout;
        layout.diagnostics_open = !layout.diagnostics_open;
        cx.notify();
    }

    /// The split was dragged (navigation only: not an unsaved change).
    pub(super) fn set_schematic_fraction(&mut self, fraction: f32, cx: &mut Context<Self>) {
        self.doc.project.layout.schematic_fraction = fraction;
        cx.notify();
    }

    /// View > Light/Dark theme (Q25), applied now and remembered.
    pub(crate) fn set_theme(
        &mut self,
        theme: ThemePreference,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings.theme = theme;
        self.apply_settings(window, cx);
    }

    /// Settings > Interface scale (Q30), applied now and remembered.
    pub(crate) fn set_ui_scale(&mut self, scale: f32, window: &mut Window, cx: &mut Context<Self>) {
        self.settings.ui_scale = appearance::clamp_scale(scale);
        self.apply_settings(window, cx);
    }

    fn apply_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        appearance::apply(&self.settings, cx);
        if let Err(error) = self.services.app_data.save_settings(&self.settings) {
            let message = format!("Settings could not be saved: {error}");
            self.report(Severity::Warning, Source::Application, message, window, cx);
        }
        cx.notify();
    }

    /// Copies the active plot to the clipboard as an image (Q07).
    pub(super) fn copy_plot(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.active_tab().has_result() {
            return;
        }
        let _ = self.active_tab().viewer.read(cx).copy_image_to_clipboard();
    }

    /// File > Export plot as PNG, JPG, SVG or CSV (Q137): asks for a file,
    /// then writes the plot of the circuit shown when it was asked.
    pub(super) fn export_plot(
        &mut self,
        kind: ExportKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let index = self.active_index();
        if self.is_asking() || !self.tabs[index].has_result() {
            return;
        }
        let id = self.tabs[index].id;
        let suggested = files::safe_file_stem(&format!(
            "{} - {}",
            self.doc.project.name, self.doc.project.circuits[index].name
        ));
        self.ask(
            move |prompts, window, cx| prompts.export_plot(kind, &suggested, window, cx),
            move |this, path, _, cx| {
                if let Some(path) = path {
                    this.write_export(id, kind, path, cx);
                }
            },
            window,
            cx,
        );
    }

    /// Writes the plot of tab `id` to `path`, unless the circuit or its
    /// result went away while the file was being chosen.
    fn write_export(&mut self, id: TabId, kind: ExportKind, path: PathBuf, cx: &mut Context<Self>) {
        let Some(index) = self
            .tab_index(id)
            .filter(|&index| self.tabs[index].has_result())
        else {
            return;
        };
        let path = files::with_extension(path, kind.extension());
        let viewer = self.tabs[index].viewer.read(cx);
        let _ = match kind.image_format() {
            Some(format) => viewer.export_image(&path, format, EXPORT_SIZE.0, EXPORT_SIZE.1),
            None => viewer.export_csv(&path),
        };
    }
}
