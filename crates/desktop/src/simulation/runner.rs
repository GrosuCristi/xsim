//! Runs one simulation on its own thread (Q110, Q139).
//!
//! The UI keeps a [`SimulationRun`] while the engine works: it reads the
//! progress at render time, pauses, resumes or cancels through the shared
//! [`SimControl`], and awaits the outcome on a channel. Nothing here touches
//! GPUI, so the threading is tested without a window.

use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::time::{Duration, Instant};
use xsim_spice_engine::{Analysis, Circuit, SimControl, SimError, SimOptions, SimulationResult};

/// Everything the worker thread needs.
#[derive(Debug, Clone)]
pub struct RunRequest {
    pub circuit: Circuit,
    pub analysis: Analysis,
    pub options: SimOptions,
}

/// A successful run.
#[derive(Debug, Clone)]
pub struct CompletedRun {
    pub result: SimulationResult,
    /// Wall-clock time from start to finish, pauses included.
    pub elapsed: Duration,
}

/// Why a run produced no result.
#[derive(Debug, Clone, PartialEq)]
pub enum RunFailure {
    /// The engine reported an error; its message includes the progress (D7).
    Simulation(SimError),
    /// The worker could not start or stopped unexpectedly.
    Crashed(String),
}

impl RunFailure {
    /// `true` when the user stopped the run.
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Simulation(SimError::Cancelled { .. }))
    }

    /// The text listed in the diagnostics dock for a run that stopped at
    /// `progress` (`0.0..=1.0`). Engine errors already state how far the run
    /// got; a crash gets the percentage added, as D7 asks of every failure.
    pub fn message(&self, progress: f64) -> String {
        match self {
            Self::Simulation(error) => error.to_string(),
            Self::Crashed(message) => format!(
                "The simulator stopped unexpectedly ({:.1}% complete): {message}",
                progress.clamp(0.0, 1.0) * 100.0
            ),
        }
    }
}

impl fmt::Display for RunFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Simulation(error) => error.fmt(f),
            Self::Crashed(message) => write!(f, "The simulator stopped unexpectedly: {message}"),
        }
    }
}

/// What a finished run delivers.
pub type RunOutcome = Result<CompletedRun, RunFailure>;

/// A simulation in progress. Dropping it cancels the run.
#[derive(Debug)]
pub struct SimulationRun {
    control: Arc<SimControl>,
    outcome: async_channel::Receiver<RunOutcome>,
}

impl SimulationRun {
    /// Starts `request` on a new thread.
    pub fn start(request: RunRequest) -> Self {
        let control = Arc::new(SimControl::new());
        let (sender, outcome) = async_channel::bounded(1);
        let worker_control = Arc::clone(&control);
        let failure_sender = sender.clone();
        let spawned = std::thread::Builder::new()
            .name("xsim-simulation".into())
            .spawn(move || {
                let outcome = execute(&request, &worker_control);
                // The receiver is gone when the UI dropped the run; nothing
                // is waiting for the outcome then.
                let _ = sender.send_blocking(outcome);
            });
        if let Err(error) = spawned {
            let _ = failure_sender.send_blocking(Err(RunFailure::Crashed(error.to_string())));
        }
        Self { control, outcome }
    }

    /// Completed fraction in `0.0..=1.0`.
    pub fn progress(&self) -> f64 {
        self.control.progress()
    }

    /// Warnings the engine raised so far (D15), oldest first. A run that
    /// raised warnings can still deliver its result.
    pub fn warnings(&self) -> Vec<String> {
        self.control.warnings()
    }

    /// Whether a pause was requested and not yet resumed.
    pub fn is_paused(&self) -> bool {
        self.control.is_paused()
    }

    /// Pauses at the engine's next checkpoint.
    pub fn pause(&self) {
        self.control.pause();
    }

    /// Continues a paused run.
    pub fn resume(&self) {
        self.control.resume();
    }

    /// Stops the run; the outcome becomes [`SimError::Cancelled`].
    pub fn cancel(&self) {
        self.control.cancel();
    }

    /// Receives the outcome once the run ends.
    pub fn outcome(&self) -> async_channel::Receiver<RunOutcome> {
        self.outcome.clone()
    }

    /// Blocks until the run ends.
    #[cfg(test)]
    pub fn wait(&self) -> RunOutcome {
        self.outcome
            .recv_blocking()
            .unwrap_or_else(|_| Err(RunFailure::Crashed("no outcome".into())))
    }
}

impl Drop for SimulationRun {
    fn drop(&mut self) {
        self.control.cancel();
    }
}

/// Runs the engine, converting a panic into [`RunFailure::Crashed`] so a
/// bug in the simulator never takes the application down.
fn execute(request: &RunRequest, control: &SimControl) -> RunOutcome {
    let started = Instant::now();
    let run = catch_unwind(AssertUnwindSafe(|| {
        xsim_spice_engine::simulate(
            &request.circuit,
            &request.analysis,
            &request.options,
            control,
        )
    }));
    match run {
        Ok(Ok(result)) => Ok(CompletedRun {
            result,
            elapsed: started.elapsed(),
        }),
        Ok(Err(error)) => Err(RunFailure::Simulation(error)),
        Err(panic) => Err(RunFailure::Crashed(panic_message(panic.as_ref()))),
    }
}

fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    panic
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "internal error".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use xsim_spice_engine::{Device, Element, GROUND, InitialState, Source, Transient};

    fn rc_circuit() -> Circuit {
        let mut circuit = Circuit::new();
        let input = circuit.node("in");
        let output = circuit.node("out");
        circuit.add(Element::new(
            "V1",
            [input, GROUND],
            Device::VoltageSource(Source::dc(1.0)),
        ));
        circuit.add(Element::new("R1", [input, output], Device::resistor(1e3)));
        circuit.add(Element::new(
            "C1",
            [output, GROUND],
            Device::capacitor(1e-6),
        ));
        circuit
    }

    fn transient(stop: f64, step: f64) -> RunRequest {
        RunRequest {
            circuit: rc_circuit(),
            analysis: Analysis::Transient(Transient {
                stop,
                step,
                start_save: 0.0,
                max_step: Some(step),
                initial_state: InitialState::ZeroState,
            }),
            options: SimOptions::default(),
        }
    }

    #[test]
    fn a_run_delivers_its_result() {
        let run = SimulationRun::start(transient(5e-3, 1e-5));
        let completed = run.wait().expect("the RC transient succeeds");
        let out = completed.result.signal("V(out)").expect("V(out) recorded");
        let last = *out.real().expect("real samples").last().expect("samples");
        assert!(
            (last - (1.0 - (-5.0f64).exp())).abs() < 1e-3,
            "V(out) = {last}"
        );
        assert_eq!(run.progress(), 1.0);
    }

    #[test]
    fn cancelling_reports_cancelled() {
        let run = SimulationRun::start(transient(1.0, 1e-6));
        run.cancel();
        let failure = run.wait().expect_err("a cancelled run has no result");
        assert!(failure.is_cancelled(), "{failure}");
        assert!(failure.to_string().contains("cancelled"));
    }

    #[test]
    fn a_paused_run_waits_until_resumed() {
        let run = SimulationRun::start(transient(2e-2, 1e-6));
        run.pause();
        assert!(run.is_paused());
        std::thread::sleep(Duration::from_millis(30));
        let paused_at = run.progress();
        std::thread::sleep(Duration::from_millis(60));
        assert!(run.outcome().is_empty(), "a paused run must not finish");
        assert!(run.progress() - paused_at < 0.5);
        run.resume();
        assert!(!run.is_paused());
        assert!(run.wait().is_ok());
    }

    #[test]
    fn invalid_options_fail_without_crashing() {
        let mut request = transient(1e-3, 1e-5);
        request.options.reltol = -1.0;
        let failure = SimulationRun::start(request).wait().expect_err("invalid");
        assert!(matches!(
            failure,
            RunFailure::Simulation(SimError::InvalidCircuit(_))
        ));
    }

    #[test]
    fn dropping_a_run_cancels_it() {
        let run = SimulationRun::start(transient(1.0, 1e-6));
        let outcome = run.outcome();
        drop(run);
        let failure = outcome
            .recv_blocking()
            .expect("the worker reports")
            .expect_err("dropped runs are cancelled");
        assert!(failure.is_cancelled());
    }

    #[test]
    fn panics_become_crash_reports() {
        let message = panic_message(&"boom");
        assert_eq!(message, "boom");
        let failure = RunFailure::Crashed(message);
        assert!(failure.to_string().contains("stopped unexpectedly: boom"));
        assert!(!failure.is_cancelled());
    }

    /// INT-8 (D7): a crash states how far the run got, like every failure.
    #[test]
    fn crash_reports_state_the_completed_percentage() {
        let failure = RunFailure::Crashed("boom".into());
        assert_eq!(
            failure.message(0.41),
            "The simulator stopped unexpectedly (41.0% complete): boom"
        );
        assert!(failure.message(0.0).contains("(0.0% complete)"));
        let cancelled = RunFailure::Simulation(SimError::Cancelled { progress: 0.25 });
        assert_eq!(cancelled.message(0.9), cancelled.to_string());
    }

    /// A run without warnings reports none; they are read from the run's
    /// own control (D15).
    #[test]
    fn a_clean_run_has_no_warnings() {
        let run = SimulationRun::start(transient(1e-3, 1e-5));
        assert!(run.wait().is_ok());
        assert!(run.warnings().is_empty());
        assert_eq!(run.warnings(), run.control.warnings());
    }

    /// Two capacitors in series from a source to ground (D15): their middle
    /// node has no DC path, so the engine ties it to ground through gmin for
    /// the operating point, warns about it and still delivers the result.
    #[test]
    fn a_floating_capacitor_node_warns_and_still_runs() {
        let mut circuit = Circuit::new();
        let (input, middle) = (circuit.node("in"), circuit.node("mid"));
        circuit.add(Element::new(
            "V1",
            [input, GROUND],
            Device::VoltageSource(Source::dc(1.0)),
        ));
        circuit.add(Element::new("C1", [input, middle], Device::capacitor(1e-6)));
        circuit.add(Element::new(
            "C2",
            [middle, GROUND],
            Device::capacitor(1e-6),
        ));
        let run = SimulationRun::start(RunRequest {
            circuit,
            analysis: Analysis::OperatingPoint,
            options: SimOptions::default(),
        });
        let outcome = run.wait();
        assert!(outcome.is_ok(), "{:?}", outcome.err());
        let warnings = run.warnings();
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("mid") && w.contains("gmin")),
            "{warnings:?}"
        );
    }
}
