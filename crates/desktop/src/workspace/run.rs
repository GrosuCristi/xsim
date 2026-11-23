//! Running simulations (Q110, Q139) and showing their results (Q38, Q136).

use super::Workspace;
use super::circuit_tab::{ActiveRun, OUTDATED};
use crate::diagnostics::{Diagnostic, Severity, Source};
use crate::ids::TabId;
use crate::simulation::runner::{CompletedRun, RunFailure, RunOutcome, RunRequest, SimulationRun};
use crate::simulation::summary::RunStatus;
use crate::simulation::traces::{default_traces, is_empty, stored_result_is_current, with_traces};
use gpui_kit::Context;
use std::sync::Arc;
use std::time::Duration;
use xsim_editor::ProbeContext;
use xsim_project::{Netlist, SimulationSetup, StoredResult};
use xsim_spice_engine::{Analysis, Circuit, Device, SimulationResult};

/// Warning for an AC analysis that nothing excites.
const NO_AC_SOURCE: &str = "No source has an AC amplitude, so every AC response is zero. Set an AC amplitude on a source (for example AC 1) in its properties.";

/// Whether any independent source has a nonzero AC amplitude.
fn has_ac_excitation(circuit: &Circuit) -> bool {
    circuit
        .elements
        .iter()
        .any(|element| match &element.device {
            Device::VoltageSource(source) | Device::CurrentSource(source) => {
                source.ac_magnitude != 0.0
            }
            _ => false,
        })
}

/// How often the loader repaints while a run progresses.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

impl Workspace {
    /// Netlists the active circuit and simulates it in the background. A
    /// netlist error goes to the diagnostics dock instead (Q44).
    pub(super) fn run_simulation(&mut self, cx: &mut Context<Self>) {
        let index = self.active_index();
        if self.doc.read_only || self.tabs[index].is_running() {
            return;
        }
        let id = self.tabs[index].id;
        self.diagnostics.clear_source(Source::Netlist, Some(id));
        self.diagnostics.clear_source(Source::Simulation, Some(id));
        let doc = &self.doc.project.circuits[index];
        let netlist = match xsim_project::netlist(&doc.schematic) {
            Ok(netlist) => Arc::new(netlist),
            Err(error) => {
                let opened = self.diagnostics.push(Diagnostic::circuit(
                    id,
                    Severity::Error,
                    Source::Netlist,
                    error.to_string(),
                ));
                self.reveal_errors(opened);
                self.tabs[index].status = RunStatus::Failed;
                cx.notify();
                return;
            }
        };
        let request = RunRequest {
            circuit: netlist.circuit.clone(),
            analysis: doc.simulation.analysis(),
            options: doc.simulation.options.clone(),
        };
        if matches!(request.analysis, Analysis::Ac(_)) && !has_ac_excitation(&request.circuit) {
            self.diagnostics.push(Diagnostic::circuit(
                id,
                Severity::Warning,
                Source::Simulation,
                NO_AC_SOURCE.to_string(),
            ));
        }
        self.start_run(index, request, netlist, cx);
    }

    fn start_run(
        &mut self,
        index: usize,
        request: RunRequest,
        netlist: Arc<Netlist>,
        cx: &mut Context<Self>,
    ) {
        self.run_generation += 1;
        let generation = self.run_generation;
        let tab = &mut self.tabs[index];
        let id = tab.id;
        let analysis = request.analysis.clone();
        let run = SimulationRun::start(request);
        let outcome = run.outcome();
        cx.spawn(async move |this, cx| {
            if let Ok(outcome) = outcome.recv().await {
                this.update(cx, |this, cx| this.finish_run(id, generation, outcome, cx))
                    .ok();
            }
        })
        .detach();
        let ticker = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(PROGRESS_INTERVAL).await;
                let running = this.update(cx, |this, cx| this.tick_run(id, cx));
                if !matches!(running, Ok(true)) {
                    break;
                }
            }
        });
        tab.status = RunStatus::Running {
            progress: 0.0,
            paused: false,
        };
        tab.run = Some(ActiveRun {
            run,
            generation,
            analysis,
            netlist,
            revision: tab.revision,
            _ticker: ticker,
        });
        cx.notify();
    }

    /// Refreshes the progress shown; `false` once the run is over.
    fn tick_run(&mut self, id: TabId, cx: &mut Context<Self>) -> bool {
        let Some(tab) = self.tab_index(id).map(|i| &mut self.tabs[i]) else {
            return false;
        };
        let Some(active) = &tab.run else {
            return false;
        };
        tab.status = RunStatus::Running {
            progress: active.run.progress(),
            paused: active.run.is_paused(),
        };
        cx.notify();
        true
    }

    /// Handles the outcome of run `generation` of tab `id`, unless the tab
    /// was closed or started another run since.
    fn finish_run(
        &mut self,
        id: TabId,
        generation: u64,
        outcome: RunOutcome,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.tab_index(id) else {
            return;
        };
        let tab = &mut self.tabs[index];
        if tab.run.as_ref().map(|r| r.generation) != Some(generation) {
            return;
        }
        let Some(active) = tab.run.take() else {
            return;
        };
        let progress = active.run.progress();
        let warnings = active.run.warnings();
        match outcome {
            Ok(completed) => self.store_result(index, active, completed, cx),
            Err(failure) => self.report_failure(index, &failure, progress, cx),
        }
        self.report_warnings(index, warnings);
        cx.notify();
    }

    /// Stores a finished result in the document (Q38) and shows it with the
    /// circuit's plot setup (Q136), starting with default traces the first
    /// time.
    fn store_result(
        &mut self,
        index: usize,
        active: ActiveRun,
        completed: CompletedRun,
        cx: &mut Context<Self>,
    ) {
        let doc = &mut self.doc.project.circuits[index];
        let result = Arc::new(completed.result);
        if is_empty(&doc.plot) {
            doc.plot = with_traces(doc.plot.clone(), default_traces(&result, &doc.schematic));
        }
        // The circuit actually simulated, even when the schematic changed
        // during the run: opening the project later compares it with the
        // schematic of that time (INT-1).
        doc.result = Some(StoredResult {
            analysis: active.analysis,
            circuit_fingerprint: Some(StoredResult::fingerprint(&active.netlist.circuit)),
            result: SimulationResult::clone(&result),
        });
        let setup = doc.plot.clone();
        let tab = &mut self.tabs[index];
        tab.status = RunStatus::Finished {
            elapsed: completed.elapsed,
        };
        let fresh = active.revision == tab.revision;
        tab.result_revision = Some(active.revision);
        let probe = fresh.then(|| ProbeContext {
            netlist: active.netlist,
            result: Arc::clone(&result),
        });
        tab.viewer
            .update(cx, |viewer, cx| viewer.set_result(Some(result), setup, cx));
        tab.set_probe_context(probe, cx);
        self.mark_dirty(cx);
    }

    /// Lists a failure; a cancelled run is a warning, anything else an
    /// error that opens the dock (D7: the message carries the progress).
    pub(super) fn report_failure(
        &mut self,
        index: usize,
        failure: &RunFailure,
        progress: f64,
        cx: &mut Context<Self>,
    ) {
        let tab = &mut self.tabs[index];
        let (severity, status) = if failure.is_cancelled() {
            (Severity::Warning, RunStatus::Cancelled { progress })
        } else {
            (Severity::Error, RunStatus::Failed)
        };
        tab.status = status;
        let id = tab.id;
        let opened = self.diagnostics.push(Diagnostic::circuit(
            id,
            severity,
            Source::Simulation,
            failure.message(progress),
        ));
        self.reveal_errors(opened);
        cx.notify();
    }

    /// Lists the warnings a run raised (D15), whether or not it produced a
    /// result. Warnings do not open the dock; the status bar counts them.
    pub(super) fn report_warnings(&mut self, index: usize, warnings: Vec<String>) {
        let id = self.tabs[index].id;
        for warning in warnings {
            self.diagnostics.push(Diagnostic::circuit(
                id,
                Severity::Warning,
                Source::Simulation,
                warning,
            ));
        }
    }

    /// Shows the result stored in circuit `index` (after opening or
    /// recovering a project, Q07: no re-run needed). A result the schematic
    /// no longer netlists to shows as out of date, without probing, exactly
    /// as after an edit in the session (INT-1).
    pub(super) fn restore_result(&mut self, index: usize, cx: &mut Context<Self>) {
        let doc = &self.doc.project.circuits[index];
        let Some(stored) = &doc.result else {
            return;
        };
        let result = Arc::new(stored.result.clone());
        let netlist = xsim_project::netlist(&doc.schematic)
            .ok()
            .filter(|netlist| stored_result_is_current(stored, netlist))
            .map(Arc::new);
        let setup = doc.plot.clone();
        let tab = &mut self.tabs[index];
        tab.result_revision = Some(match netlist {
            Some(_) => tab.revision,
            None => OUTDATED,
        });
        let probe = netlist.map(|netlist| ProbeContext {
            netlist,
            result: Arc::clone(&result),
        });
        tab.viewer
            .update(cx, |viewer, cx| viewer.set_result(Some(result), setup, cx));
        tab.set_probe_context(probe, cx);
        // The viewer may adapt the stored setup to the result (an AC result
        // gets its Bode panes). That is not an edit: take the adapted setup
        // before its SetupChanged arrives, so opening a project never marks
        // it unsaved.
        let adapted = tab.viewer.read(cx).setup().clone();
        self.doc.project.circuits[index].plot = adapted;
    }

    /// Pauses or resumes the active circuit's run.
    pub(super) fn toggle_pause(&mut self, cx: &mut Context<Self>) {
        let index = self.active_index();
        let Some(active) = &self.tabs[index].run else {
            return;
        };
        if active.run.is_paused() {
            active.run.resume();
        } else {
            active.run.pause();
        }
        let id = self.tabs[index].id;
        self.tick_run(id, cx);
    }

    /// Cancels the active circuit's run; the outcome arrives as Cancelled.
    pub(super) fn stop_simulation(&mut self, cx: &mut Context<Self>) {
        if let Some(active) = &self.active_tab().run {
            active.run.cancel();
            cx.notify();
        }
    }

    /// Stores the analysis dialog's settings for the active circuit.
    pub(crate) fn apply_simulation_setup(
        &mut self,
        setup: SimulationSetup,
        cx: &mut Context<Self>,
    ) {
        let index = self.active_index();
        if self.doc.read_only || self.doc.project.circuits[index].simulation == setup {
            return;
        }
        self.doc.project.circuits[index].simulation = setup;
        self.mark_dirty(cx);
    }
}
