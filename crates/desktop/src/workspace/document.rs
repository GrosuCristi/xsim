//! Project files (Q31–Q46): new, open, save, save as, close and the
//! unsaved-changes check. Saving is manual only (Q40).

use super::{DocumentState, Workspace};
use crate::diagnostics::{Diagnostic, Severity, Source};
use crate::files;
use crate::prompts::UnsavedChoice;
use crate::ui::toast;
use gpui_kit::component::notification::Notification;
use gpui_kit::{prelude::*, *};
use std::path::{Path, PathBuf};
use xsim_project::{Compatibility, LoadedProject, Project, SCHEMA_VERSION};

impl Workspace {
    /// File > New project.
    pub(super) fn new_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.confirm_discard(window, cx) {
            self.replace_document(DocumentState::untitled(), cx);
            self.focus_editor(window, cx);
        }
    }

    /// File > Open.
    pub(super) fn open_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let prompts = self.services.prompts.clone();
        if let Some(path) = prompts.open_project(window) {
            self.open_path(path, window, cx);
        }
    }

    /// Opens `path` after the unsaved-changes check (also used by Open
    /// recent).
    pub(super) fn open_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self.confirm_discard(window, cx) {
            self.load_path(path, window, cx);
        }
    }

    /// Loads `path` into the window, reporting failures in the dock.
    pub(super) fn load_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        match Project::load(&path) {
            Ok(loaded) => {
                self.install_loaded(loaded, path, cx);
                self.focus_editor(window, cx);
            }
            Err(error) => {
                let message = format!("Could not open {}: {error}", path.display());
                self.report(Severity::Error, Source::File, message, window, cx);
            }
        }
    }

    /// Shows a freshly loaded project and reports its compatibility (Q45).
    fn install_loaded(&mut self, loaded: LoadedProject, path: PathBuf, cx: &mut Context<Self>) {
        let read_only = matches!(loaded.compatibility, Compatibility::Newer { .. });
        self.remember_recent(&path);
        self.replace_document(
            DocumentState {
                project: loaded.project,
                path: Some(path),
                recovered_from: None,
                dirty: false,
                read_only,
            },
            cx,
        );
        if let Some(diagnostic) = compatibility_notice(loaded.compatibility) {
            let important = diagnostic.severity != Severity::Info;
            self.diagnostics.push(diagnostic);
            self.reveal_errors(important);
        }
    }

    /// Replaces the open project: running simulations stop (their tabs are
    /// dropped), problems of the old project are cleared and the recovery
    /// snapshot is reset.
    pub(super) fn replace_document(&mut self, doc: DocumentState, cx: &mut Context<Self>) {
        self.doc = doc;
        let circuits = self.doc.project.circuits.len();
        if circuits == 0 {
            self.doc.project.circuits = Project::default().circuits;
        }
        self.doc.project.active_circuit = self.doc.project.active_circuit.min(circuits.max(1) - 1);
        self.diagnostics.clear();
        self.rebuild_tabs(cx);
        self.clear_snapshot();
        cx.notify();
    }

    /// File > Save; the first save asks for a path. Returns whether the
    /// project was written.
    pub(super) fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        match self.doc.path.clone() {
            Some(path) if !self.doc.read_only => self.write_to(path, window, cx),
            _ => self.save_as(window, cx),
        }
    }

    /// File > Save as. For a read-only project from a newer xsim this saves
    /// a copy in the current schema, which is then editable.
    pub(super) fn save_as(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let suggested = self.suggested_save_path();
        let prompts = self.services.prompts.clone();
        match prompts.save_project(&suggested, window) {
            Some(path) => self.write_to(files::with_project_extension(path), window, cx),
            None => false,
        }
    }

    /// Where Save as starts: the current file, the file a recovered project
    /// came from, or `<name>.xsim_proj`.
    fn suggested_save_path(&self) -> PathBuf {
        self.doc
            .path
            .clone()
            .or_else(|| self.doc.recovered_from.clone())
            .unwrap_or_else(|| {
                let stem = files::safe_file_stem(&self.doc.project.name);
                files::with_project_extension(PathBuf::from(stem))
            })
    }

    /// Writes the project atomically to `path` and makes it the current
    /// file.
    fn write_to(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let mut project = self.doc.project.clone();
        project.schema_version = SCHEMA_VERSION;
        if let Some(name) = files::project_name_from_path(&path) {
            project.name = name;
        }
        if let Err(error) = project.save(&path) {
            let message = format!("Could not save {}: {error}", path.display());
            self.report(Severity::Error, Source::File, message, window, cx);
            return false;
        }
        self.doc.project = project;
        self.doc.path = Some(path.clone());
        self.doc.recovered_from = None;
        self.doc.dirty = false;
        self.set_read_only(false, cx);
        self.diagnostics.clear_source(Source::File, None);
        self.remember_recent(&path);
        self.clear_snapshot();
        toast(
            Notification::success(format!("Saved {}", self.doc.project.name)),
            window,
            cx,
        );
        cx.notify();
        true
    }

    /// Switches every editor between viewing and editing.
    fn set_read_only(&mut self, read_only: bool, cx: &mut Context<Self>) {
        if self.doc.read_only == read_only {
            return;
        }
        self.doc.read_only = read_only;
        for tab in &self.tabs {
            tab.editor
                .update(cx, |editor, cx| editor.set_read_only(read_only, cx));
        }
    }

    /// File > Close project: back to an empty project.
    pub(super) fn close_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.new_project(window, cx);
    }

    /// File > Exit: closes the window (and the application) after the
    /// unsaved-changes check.
    pub(super) fn exit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.prepare_to_close(window, cx) {
            window.remove_window();
        }
    }

    /// Called before the window closes: `false` keeps it open. On a clean
    /// close the recovery session ends, deleting its snapshot.
    pub(super) fn prepare_to_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.confirm_discard(window, cx) {
            return false;
        }
        for tab in &mut self.tabs {
            tab.run = None;
        }
        self.end_recovery();
        true
    }

    /// The unsaved-changes check: `true` when the current project may be
    /// replaced (it was saved, the user discarded it, or it is clean).
    pub(super) fn confirm_discard(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.doc.dirty {
            return true;
        }
        let prompts = self.services.prompts.clone();
        match prompts.unsaved_changes(&self.doc.project.name, window) {
            UnsavedChoice::Save => self.save(window, cx),
            UnsavedChoice::Discard => true,
            UnsavedChoice::Cancel => false,
        }
    }

    /// Moves `path` to the top of the recent list (Q46).
    fn remember_recent(&mut self, path: &Path) {
        let app_data = &self.services.app_data;
        if let Err(error) = app_data.push_recent(path) {
            self.diagnostics.push(Diagnostic::project(
                Severity::Warning,
                Source::Application,
                format!("The recent projects list could not be updated: {error}"),
            ));
        }
        self.recent = app_data.recent();
    }

    /// Lists a problem and shows it as a toast.
    pub(super) fn report(
        &mut self,
        severity: Severity,
        source: Source,
        message: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let note = match severity {
            Severity::Error => Notification::error(message.clone()),
            Severity::Warning => Notification::warning(message.clone()),
            Severity::Info => Notification::info(message.clone()),
        };
        toast(note, window, cx);
        let opened = self
            .diagnostics
            .push(Diagnostic::project(severity, source, message));
        self.reveal_errors(opened);
        cx.notify();
    }
}

/// The dock entry for a migrated or newer file.
fn compatibility_notice(compatibility: Compatibility) -> Option<Diagnostic> {
    match compatibility {
        Compatibility::Current => None,
        Compatibility::Migrated { from } => Some(Diagnostic::project(
            Severity::Info,
            Source::File,
            format!(
                "This project was upgraded from file format {from}. Saving writes format {SCHEMA_VERSION}, which older xsim versions cannot open."
            ),
        )),
        Compatibility::Newer { version } => Some(Diagnostic::project(
            Severity::Warning,
            Source::File,
            format!(
                "This project was written by a newer xsim (file format {version}; this version reads {SCHEMA_VERSION}). It is open read-only and some features may not work. Use Save as to keep an editable copy."
            ),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::compatibility_notice;
    use crate::diagnostics::Severity;
    use xsim_project::Compatibility;

    #[test]
    fn compatibility_is_explained() {
        assert!(compatibility_notice(Compatibility::Current).is_none());
        let migrated = compatibility_notice(Compatibility::Migrated { from: 1 }).expect("notice");
        assert_eq!(migrated.severity, Severity::Info);
        assert!(migrated.message.contains("upgraded from file format 1"));
        let newer = compatibility_notice(Compatibility::Newer { version: 9 }).expect("notice");
        assert_eq!(newer.severity, Severity::Warning);
        assert!(newer.message.contains("read-only"));
        assert!(newer.message.contains("some features may not work"));
    }
}
