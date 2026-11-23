//! Per-circuit views and runtime state.

use super::Workspace;
use crate::ids::TabId;
use crate::simulation::runner::SimulationRun;
use crate::simulation::summary::RunStatus;
use gpui_kit::{prelude::*, *};
use std::sync::Arc;
use xsim_editor::{ProbeContext, SchematicEditor};
use xsim_plot::WaveformViewer;
use xsim_project::{CircuitDocument, Netlist};
use xsim_spice_engine::Analysis;

/// A simulation in progress.
pub(super) struct ActiveRun {
    pub run: SimulationRun,
    pub generation: u64,
    pub analysis: Analysis,
    pub netlist: Arc<Netlist>,
    /// Schematic revision at start; edits during the run make it stale.
    pub revision: u64,
    /// Repaints the loader while the run progresses.
    pub _ticker: Task<()>,
}

/// One circuit tab.
pub(super) struct CircuitTab {
    pub id: TabId,
    pub editor: Entity<SchematicEditor>,
    pub viewer: Entity<WaveformViewer>,
    /// Incremented on every schematic edit.
    pub revision: u64,
    /// Schematic revision the shown result was computed from; `None` while
    /// the circuit has no result, [`OUTDATED`] for a stored result that
    /// belongs to an earlier version of the schematic.
    pub result_revision: Option<u64>,
    /// Whether the editor probes the shown result (the hover card and
    /// click-to-plot); only while the result is current. The editor keeps
    /// its probe context private, so the tests read this mirror of it.
    #[cfg(test)]
    pub probing: bool,
    pub run: Option<ActiveRun>,
    pub status: RunStatus,
    _subscriptions: [Subscription; 3],
}

/// The [`CircuitTab::result_revision`] of a stored result computed from a
/// schematic other than the one opened. Revisions count edits from 0 and
/// never reach it, so such a result shows as out of date until the next run.
pub(super) const OUTDATED: u64 = u64::MAX;

impl CircuitTab {
    /// Views for `doc`, subscribed to the workspace.
    pub fn new(
        id: TabId,
        doc: &CircuitDocument,
        read_only: bool,
        cx: &mut Context<Workspace>,
    ) -> Self {
        let (schematic, view) = (doc.schematic.clone(), doc.view);
        let editor = cx.new(|cx| {
            let mut editor = SchematicEditor::new(schematic, view, cx);
            editor.set_read_only(read_only, cx);
            editor
        });
        let viewer = cx.new(WaveformViewer::new);
        let subscriptions = [
            cx.subscribe(&editor, move |this, _, event, cx| {
                this.on_editor_event(id, event, cx)
            }),
            cx.subscribe(&viewer, move |this, _, event, cx| {
                this.on_viewer_event(id, event, cx)
            }),
            cx.subscribe(&viewer, move |this, _, request, cx| {
                this.on_viewer_request(id, request, cx)
            }),
        ];
        Self {
            id,
            editor,
            viewer,
            revision: 0,
            result_revision: None,
            #[cfg(test)]
            probing: false,
            run: None,
            status: RunStatus::Idle,
            _subscriptions: subscriptions,
        }
    }

    /// The schematic changed since the shown result was computed.
    pub fn is_stale(&self) -> bool {
        self.result_revision
            .is_some_and(|revision| revision != self.revision)
    }

    /// Whether the circuit has a result to show.
    pub fn has_result(&self) -> bool {
        self.result_revision.is_some()
    }

    /// Whether the results side of the split is shown: there is a result
    /// or a run in progress.
    pub fn shows_results(&self) -> bool {
        self.result_revision.is_some() || self.run.is_some()
    }

    /// Whether a simulation of this circuit is in progress.
    pub fn is_running(&self) -> bool {
        self.run.is_some()
    }

    /// Enables probing of a result in the editor, or disables it with
    /// `None`.
    pub fn set_probe_context(&mut self, probe: Option<ProbeContext>, cx: &mut Context<Workspace>) {
        #[cfg(test)]
        {
            self.probing = probe.is_some();
        }
        self.editor
            .update(cx, |editor, cx| editor.set_probe_context(probe, cx));
    }
}
