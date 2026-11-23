//! Crash recovery (Q41). While the project has unsaved changes a snapshot
//! is rewritten shortly after each edit, off the UI thread. Saving clears
//! it; a clean exit deletes the session. Snapshots left by crashed
//! instances are offered at startup.

use super::{DocumentState, Workspace};
use crate::diagnostics::{Diagnostic, Severity, Source};
use crate::dialogs::recovery::open_recovery_dialog;
use gpui_kit::{prelude::*, *};
use std::sync::Arc;
use std::time::Duration;
use xsim_project::appdata::{AppData, Orphan, RecoverySession, RecoverySnapshot};

/// Delay between the last edit and writing the snapshot.
const SNAPSHOT_DELAY: Duration = Duration::from_millis(800);

/// This instance's recovery state.
pub(super) struct Recovery {
    session: Option<Arc<RecoverySession>>,
    /// Why recovery is unavailable, reported once at startup.
    start_error: Option<String>,
    /// Pending debounced write.
    debounce: Option<Task<()>>,
    /// Write in progress.
    write: Option<Task<()>>,
    /// Snapshots of crashed instances not yet restored or discarded.
    pub orphans: Vec<Orphan>,
}

impl Recovery {
    /// Starts this instance's session and collects crashed instances'
    /// snapshots.
    pub fn start(app_data: &AppData) -> Self {
        let (session, start_error) = match app_data.start_recovery() {
            Ok(session) => (Some(Arc::new(session)), None),
            Err(error) => (None, Some(error.to_string())),
        };
        Self {
            session,
            start_error,
            debounce: None,
            write: None,
            orphans: app_data.orphans(),
        }
    }
}

impl Workspace {
    /// Reports a failure to start the recovery session.
    pub(super) fn report_recovery_start(&mut self) {
        if let Some(error) = self.recovery.start_error.take() {
            self.diagnostics.push(Diagnostic::project(
                Severity::Warning,
                Source::Recovery,
                format!("Crash recovery is unavailable: {error}"),
            ));
        }
    }

    /// Writes a snapshot [`SNAPSHOT_DELAY`] after the latest edit.
    pub(super) fn schedule_snapshot(&mut self, cx: &mut Context<Self>) {
        if self.recovery.session.is_none() || !self.doc.dirty {
            return;
        }
        self.recovery.debounce = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SNAPSHOT_DELAY).await;
            this.update(cx, |this, cx| this.write_snapshot(cx)).ok();
        }));
    }

    /// Serializes and writes the current project on a background thread.
    fn write_snapshot(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.recovery.session.clone() else {
            return;
        };
        let snapshot = self.snapshot();
        let write = cx.background_spawn(async move { session.write(&snapshot) });
        self.recovery.write = Some(cx.spawn(async move |this, cx| {
            if let Err(error) = write.await {
                this.update(cx, |this, cx| this.recovery_failed(error, cx))
                    .ok();
            }
        }));
    }

    fn snapshot(&self) -> RecoverySnapshot {
        RecoverySnapshot {
            source: self
                .doc
                .path
                .clone()
                .or_else(|| self.doc.recovered_from.clone()),
            project: self.doc.project.clone(),
        }
    }

    fn recovery_failed(&mut self, error: std::io::Error, cx: &mut Context<Self>) {
        self.diagnostics.clear_source(Source::Recovery, None);
        self.diagnostics.push(Diagnostic::project(
            Severity::Warning,
            Source::Recovery,
            format!("Unsaved changes could not be backed up for crash recovery: {error}"),
        ));
        cx.notify();
    }

    /// Removes the snapshot: the project on disk is up to date.
    pub(super) fn clear_snapshot(&mut self) {
        self.recovery.debounce = None;
        self.recovery.write = None;
        if let Some(session) = &self.recovery.session
            && let Err(error) = session.clear()
        {
            self.diagnostics.push(Diagnostic::project(
                Severity::Warning,
                Source::Recovery,
                format!("The crash-recovery snapshot could not be removed: {error}"),
            ));
        }
    }

    /// Ends the session on a clean exit, deleting its directory.
    pub(super) fn end_recovery(&mut self) {
        self.recovery.debounce = None;
        self.recovery.write = None;
        self.recovery.session = None;
    }

    /// Offers the snapshots of crashed instances, if any.
    pub(super) fn offer_orphans(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.recovery.orphans.is_empty() {
            open_recovery_dialog(cx.entity(), self.recovery.orphans.len(), window, cx);
        }
    }

    /// Summaries of the orphans for the recovery dialog: project name and
    /// the file it came from.
    pub(crate) fn orphan_summaries(&self) -> Vec<(String, Option<String>)> {
        self.recovery
            .orphans
            .iter()
            .map(|orphan| {
                let snapshot = &orphan.snapshot;
                let source = snapshot.source.as_ref().map(|p| p.display().to_string());
                (snapshot.project.name.clone(), source)
            })
            .collect()
    }

    /// Opens orphan `index` as an unsaved project and takes it over: it is
    /// snapshotted in this session before the crashed session is deleted.
    pub(crate) fn restore_orphan(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if index >= self.recovery.orphans.len() || !self.confirm_discard(window, cx) {
            return;
        }
        let orphan = self.recovery.orphans.remove(index);
        let source = orphan.snapshot.source.clone();
        let name = orphan.snapshot.project.name.clone();
        self.replace_document(
            DocumentState {
                project: orphan.snapshot.project.clone(),
                path: None,
                recovered_from: source,
                dirty: true,
                read_only: false,
            },
            cx,
        );
        self.take_over(&orphan, cx);
        self.diagnostics.push(Diagnostic::project(
            Severity::Info,
            Source::Recovery,
            format!("Recovered the unsaved changes of \"{name}\". Save the project to keep them."),
        ));
        self.focus_editor(window, cx);
        cx.notify();
    }

    /// Copies the restored project into this session, then deletes the
    /// crashed one.
    fn take_over(&mut self, orphan: &Orphan, cx: &mut Context<Self>) {
        let saved = match &self.recovery.session {
            Some(session) => session.write(&self.snapshot()),
            None => Ok(()),
        };
        let discarded = saved.and_then(|()| self.services.app_data.discard_orphan(orphan));
        if let Err(error) = discarded {
            self.recovery_failed(error, cx);
        }
    }

    /// Deletes orphan `index` without restoring it.
    pub(crate) fn discard_orphan(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.recovery.orphans.len() {
            return;
        }
        let orphan = self.recovery.orphans.remove(index);
        if let Err(error) = self.services.app_data.discard_orphan(&orphan) {
            self.recovery_failed(error, cx);
        }
        cx.notify();
    }

    /// Number of snapshots still offered.
    #[cfg(test)]
    pub(crate) fn orphan_count(&self) -> usize {
        self.recovery.orphans.len()
    }
}
