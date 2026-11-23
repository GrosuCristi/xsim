//! Project files (Q31–Q46): new, open, save, save as, close and the
//! unsaved-changes check. Saving is manual only (Q40).
//!
//! D25: the questions these commands ask are answered asynchronously
//! ([`Workspace::ask`]), so each command takes what it does after the
//! answer as a continuation. While a question is open, the commands that
//! could ask another one do nothing.

use super::{DocumentState, Workspace};
use crate::diagnostics::{Diagnostic, Severity, Source};
use crate::files;
use crate::prompts::UnsavedChoice;
use gpui_kit::{prelude::*, *};
use std::path::{Path, PathBuf};
use xsim_project::{Compatibility, LoadedProject, Project, SCHEMA_VERSION};

/// What a command does once its question has been answered.
pub(super) trait Then:
    FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static
{
}

impl<F> Then for F where F: FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static {}

impl Workspace {
    /// File > New project.
    pub(super) fn new_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_asking() {
            return;
        }
        self.after_unsaved_check(
            |this, window, cx| {
                this.replace_document(DocumentState::untitled(), cx);
                this.focus_editor(window, cx);
            },
            window,
            cx,
        );
    }

    /// File > Open: picks a file, then checks for unsaved changes.
    pub(super) fn open_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_asking() {
            return;
        }
        self.ask(
            |prompts, window, cx| prompts.open_project(window, cx),
            |this, path, window, cx| {
                if let Some(path) = path {
                    this.open_path(path, window, cx);
                }
            },
            window,
            cx,
        );
    }

    /// Opens `path` after the unsaved-changes check (also used by Open
    /// recent).
    pub(super) fn open_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_asking() {
            return;
        }
        self.after_unsaved_check(
            move |this, window, cx| this.load_path(path, window, cx),
            window,
            cx,
        );
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

    /// File > Save; the first save asks for a path.
    pub(super) fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_asking() {
            self.save_then(|_, _, _| {}, window, cx);
        }
    }

    /// File > Save as. For a read-only project from a newer xsim this saves
    /// a copy in the current schema, which is then editable.
    pub(super) fn save_as(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_asking() {
            self.save_as_then(|_, _, _| {}, window, cx);
        }
    }

    /// Saves (asking for a path the first time), then runs `then` once the
    /// project is written; a failed or cancelled save skips it.
    fn save_then(&mut self, then: impl Then, window: &mut Window, cx: &mut Context<Self>) {
        match self.doc.path.clone() {
            Some(path) if !self.doc.read_only => {
                if self.write_to(path, window, cx) {
                    then(self, window, cx);
                }
            }
            _ => self.save_as_then(then, window, cx),
        }
    }

    /// Asks where to save, writes there, then runs `then`; a cancelled or
    /// failed save skips it.
    fn save_as_then(&mut self, then: impl Then, window: &mut Window, cx: &mut Context<Self>) {
        let suggested = self.suggested_save_path();
        self.ask(
            move |prompts, window, cx| prompts.save_project(&suggested, window, cx),
            move |this, path, window, cx| {
                let Some(path) = path else {
                    return;
                };
                if this.write_to(files::with_project_extension(path), window, cx) {
                    then(this, window, cx);
                }
            },
            window,
            cx,
        );
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

    /// File > Exit and the title bar's close button: closes the window
    /// (and with it the application) after the unsaved-changes check.
    pub(super) fn exit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.request_close(window, cx) {
            window.remove_window();
        }
    }

    /// The window is asked to close; `true` when it may close now. A clean
    /// project ends the session at once. With unsaved changes the window
    /// stays open while the question is asked: Save (once written) or Don't
    /// save then closes it, Cancel keeps it. While a question is open, a
    /// repeated close request is ignored.
    pub(super) fn request_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.is_asking() {
            return false;
        }
        if !self.doc.dirty {
            self.end_session();
            return true;
        }
        self.after_unsaved_check(
            |this, window, _| {
                this.end_session();
                window.remove_window();
            },
            window,
            cx,
        );
        false
    }

    /// A clean close: runs stop and the recovery session ends, deleting
    /// its snapshot.
    fn end_session(&mut self) {
        for tab in &mut self.tabs {
            tab.run = None;
        }
        self.end_recovery();
    }

    /// The unsaved-changes check: runs `then` once the current project may
    /// be replaced: at once when it has no unsaved changes, otherwise after
    /// the user saved it or chose not to. Cancel, or a save that is
    /// cancelled or fails, skips `then`.
    pub(super) fn after_unsaved_check(
        &mut self,
        then: impl Then,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.doc.dirty {
            then(self, window, cx);
            return;
        }
        let project = self.doc.project.name.clone();
        self.ask(
            move |prompts, window, cx| prompts.unsaved_changes(&project, window, cx),
            move |this, choice, window, cx| match choice {
                UnsavedChoice::Save => this.save_then(then, window, cx),
                UnsavedChoice::Discard => then(this, window, cx),
                UnsavedChoice::Cancel => {}
            },
            window,
            cx,
        );
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

    /// Lists a problem in the diagnostics dock and opens the dock on errors.
    pub(super) fn report(
        &mut self,
        severity: Severity,
        source: Source,
        message: String,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
