//! The main window (Q22): title bar with the logo and menus, circuit tabs
//! with the run controls at their right end (D20), the schematic editor with
//! its diagnostics dock, the waveform viewer once a circuit has results, and
//! the status bar.
//!
//! The workspace owns the open [`Project`]. Each circuit has a
//! [`CircuitTab`] with its own editor and viewer, so undo history, camera
//! and plot state survive tab switches (Q23, Q35).
//!
//! * [`circuit_tab`] — per-circuit views, result and run state.
//! * [`document`] — new, open, save, close and unsaved-changes checks.
//! * [`circuits`] — adding, renaming, deleting and switching circuits.
//! * [`events`] — reactions to editor and viewer events.
//! * [`run`] — starting, pausing and stopping simulations.
//! * [`commands`] — edit forwarding, appearance, dock and plot commands.
//! * [`recovery`] — crash-recovery snapshots and restore (Q41).
//! * rendering: [`render`], [`title_bar`], [`tab_strip`], [`run_controls`],
//!   [`main_area`], [`diagnostics_dock`], [`status_bar`].

mod circuit_tab;
mod circuits;
mod commands;
mod diagnostics_dock;
mod document;
mod events;
mod main_area;
mod recovery;
mod render;
mod run;
mod run_controls;
mod status_bar;
mod tab_strip;
mod title_bar;

#[cfg(test)]
mod motion_tests;
#[cfg(test)]
mod tests;

use crate::diagnostics::Diagnostics;
use crate::ids::TabIdAllocator;
use crate::prompts::{NativePrompts, Prompts};
use circuit_tab::CircuitTab;
use circuits::Rename;
use gpui_kit::{prelude::*, *};
use recovery::Recovery;
use std::path::PathBuf;
use std::rc::Rc;
use xsim_project::Project;
use xsim_project::appdata::{AppData, AppSettings};

/// Platform services the workspace uses, replaceable in tests.
pub struct Services {
    /// Settings, recent projects and crash recovery (never the real profile
    /// in tests).
    pub app_data: AppData,
    pub prompts: Rc<dyn Prompts>,
}

impl Services {
    /// The real data directory and native dialogs.
    pub fn native(app_data: AppData) -> Self {
        Self {
            app_data,
            prompts: Rc::new(NativePrompts),
        }
    }
}

/// The open project file and its state.
struct DocumentState {
    project: Project,
    /// File the project was opened from or saved to.
    path: Option<PathBuf>,
    /// For a project restored after a crash: the file it came from, offered
    /// by Save as.
    recovered_from: Option<PathBuf>,
    /// Unsaved changes (manual save only, Q40).
    dirty: bool,
    /// Written by a newer xsim (Q45): viewing only.
    read_only: bool,
}

impl DocumentState {
    fn untitled() -> Self {
        Self {
            project: Project::default(),
            path: None,
            recovered_from: None,
            dirty: false,
            read_only: false,
        }
    }
}

/// The main window's view.
pub struct Workspace {
    services: Services,
    settings: AppSettings,
    doc: DocumentState,
    /// One per circuit, in the order of `doc.project.circuits`.
    tabs: Vec<CircuitTab>,
    tab_ids: TabIdAllocator,
    diagnostics: Diagnostics,
    recent: Vec<PathBuf>,
    recovery: Recovery,
    renaming: Option<Rename>,
    /// Scroll position of the circuit tab strip, so a new circuit can be
    /// scrolled into view.
    tab_scroll: ScrollHandle,
    /// Whether the active circuit's tab still has to be scrolled into view
    /// after a project was shown.
    reveal_active_tab: bool,
    /// Counts started runs so a late outcome never lands in a newer run.
    run_generation: u64,
    focus: FocusHandle,
    /// The Run and analysis settings buttons' focus (D20: reached with Tab
    /// in the tab bar), kept here so it survives re-renders and tests can
    /// see it.
    run_focus: FocusHandle,
    settings_focus: FocusHandle,
    /// The window this workspace fills; viewer requests that need a file
    /// dialog are handled in it.
    window: AnyWindowHandle,
    _focus_return: Subscription,
}

impl Workspace {
    /// Opens a window on `path`, or on an empty project.
    pub fn new(
        path: Option<PathBuf>,
        services: Services,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let settings = services.app_data.settings();
        crate::appearance::apply(&settings, cx);
        let recovery = Recovery::start(&services.app_data);
        let focus = cx.focus_handle();
        let mut this = Self {
            recent: services.app_data.recent(),
            services,
            settings,
            doc: DocumentState::untitled(),
            tabs: Vec::new(),
            tab_ids: TabIdAllocator::default(),
            diagnostics: Diagnostics::default(),
            recovery,
            renaming: None,
            tab_scroll: ScrollHandle::new(),
            reveal_active_tab: true,
            run_generation: 0,
            run_focus: cx.focus_handle(),
            settings_focus: cx.focus_handle(),
            window: window.window_handle(),
            _focus_return: cx.on_focus(&focus, window, |this, window, cx| {
                this.return_focus_to_editor(window, cx)
            }),
            focus,
        };
        this.rebuild_tabs(cx);
        this.report_recovery_start();
        this.guard_window_close(window, cx);
        if let Some(path) = path {
            this.load_path(path, window, cx);
        }
        cx.defer_in(window, |this, window, cx| {
            this.focus_editor(window, cx);
            this.offer_orphans(window, cx);
        });
        this
    }

    /// Index of the circuit shown.
    fn active_index(&self) -> usize {
        self.doc
            .project
            .active_circuit
            .min(self.tabs.len().saturating_sub(1))
    }

    fn active_tab(&self) -> &CircuitTab {
        &self.tabs[self.active_index()]
    }

    /// Index of the tab with `id`, if it still exists.
    fn tab_index(&self, id: crate::ids::TabId) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.id == id)
    }

    /// Recreates every circuit tab from the project document.
    fn rebuild_tabs(&mut self, cx: &mut Context<Self>) {
        self.renaming = None;
        let read_only = self.doc.read_only;
        let tabs: Vec<CircuitTab> = self
            .doc
            .project
            .circuits
            .iter()
            .map(|doc| CircuitTab::new(self.tab_ids.allocate(), doc, read_only, cx))
            .collect();
        self.tabs = tabs;
        for index in 0..self.tabs.len() {
            self.restore_result(index, cx);
        }
        // The strip may still be scrolled for the previous project, or not
        // be laid out yet.
        self.reveal_active_tab = true;
    }

    /// Moves keyboard focus to the active schematic editor.
    fn focus_editor(&self, window: &mut Window, cx: &mut Context<Self>) {
        let handle = self.active_tab().editor.focus_handle(cx);
        handle.focus(window, cx);
    }

    /// Menus hand focus back to the workspace root when they run a command;
    /// pass it on to the schematic so its single-key shortcuts keep working.
    fn return_focus_to_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.focus.is_focused(window) {
            self.focus_editor(window, cx);
        }
    }

    /// Records an edit: marks the project unsaved and schedules a recovery
    /// snapshot. Read-only projects never become dirty.
    fn mark_dirty(&mut self, cx: &mut Context<Self>) {
        if self.doc.read_only {
            cx.notify();
            return;
        }
        self.doc.dirty = true;
        self.schedule_snapshot(cx);
        cx.notify();
    }

    /// Opens the dock when `is_error` (a new error was reported).
    fn reveal_errors(&mut self, is_error: bool) {
        if is_error {
            self.doc.project.layout.diagnostics_open = true;
        }
    }

    /// Asks before the window closes with unsaved changes, and releases the
    /// recovery session on a clean exit.
    fn guard_window_close(&self, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            this.update(cx, |this, cx| this.prepare_to_close(window, cx))
                .unwrap_or(true)
        });
    }
}

impl Focusable for Workspace {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
