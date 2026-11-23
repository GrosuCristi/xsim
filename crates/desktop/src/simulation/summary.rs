//! Human-readable descriptions of analyses and runs for the run controls,
//! the analysis settings, the status bar and the loader.

use std::time::Duration;
use xsim_project::{AnalysisChoice, SimulationSetup};
use xsim_spice_engine::units::format_value;

/// Every analysis the Run button can execute, in menu order.
pub const ANALYSES: [AnalysisChoice; 4] = [
    AnalysisChoice::OperatingPoint,
    AnalysisChoice::DcSweep,
    AnalysisChoice::Ac,
    AnalysisChoice::Transient,
];

/// `"Transient"`, `"AC sweep"`, ...
pub fn analysis_label(choice: AnalysisChoice) -> &'static str {
    match choice {
        AnalysisChoice::OperatingPoint => "Operating point",
        AnalysisChoice::DcSweep => "DC sweep",
        AnalysisChoice::Ac => "AC sweep",
        AnalysisChoice::Transient => "Transient",
    }
}

/// The selected analysis with its key settings, e.g. `"Transient · 5 ms"`
/// or `"AC sweep · 1 Hz – 1 MHz"`.
pub fn analysis_summary(setup: &SimulationSetup) -> String {
    let label = analysis_label(setup.selected);
    match setup.selected {
        AnalysisChoice::OperatingPoint => label.to_string(),
        AnalysisChoice::DcSweep => format!(
            "{label} · {} {} – {}",
            setup.dc.source,
            format_value(setup.dc.start, ""),
            format_value(setup.dc.stop, "")
        ),
        AnalysisChoice::Ac => format!(
            "{label} · {} – {}",
            format_value(setup.ac.start, "Hz"),
            format_value(setup.ac.stop, "Hz")
        ),
        AnalysisChoice::Transient => {
            format!("{label} · {}", format_value(setup.transient.stop, "s"))
        }
    }
}

/// `"350 ms"`, `"2.4 s"`, `"1 min 5 s"`.
pub fn format_elapsed(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs_f64();
    if seconds < 1.0 {
        format!("{} ms", (seconds * 1000.0).round())
    } else if seconds < 60.0 {
        format!("{seconds:.1} s")
    } else {
        let whole = elapsed.as_secs();
        format!("{} min {} s", whole / 60, whole % 60)
    }
}

/// `"42%"` for a progress fraction.
pub fn format_percent(fraction: f64) -> String {
    format!("{:.0}%", (fraction.clamp(0.0, 1.0) * 100.0).floor())
}

/// What the last run of a circuit did, for the status bar.
#[derive(Debug, Clone, PartialEq)]
pub enum RunStatus {
    /// Nothing ran since the circuit was opened.
    Idle,
    Running {
        progress: f64,
        paused: bool,
    },
    Finished {
        elapsed: Duration,
    },
    Cancelled {
        progress: f64,
    },
    Failed,
}

impl RunStatus {
    /// Status-bar text.
    pub fn label(&self) -> String {
        match self {
            Self::Idle => "Ready".into(),
            Self::Running {
                progress,
                paused: false,
            } => format!("Simulating… {}", format_percent(*progress)),
            Self::Running {
                progress,
                paused: true,
            } => format!("Paused at {}", format_percent(*progress)),
            Self::Finished { elapsed } => format!("Finished in {}", format_elapsed(*elapsed)),
            Self::Cancelled { progress } => format!("Cancelled at {}", format_percent(*progress)),
            Self::Failed => "Simulation failed".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_read_naturally() {
        assert_eq!(format_elapsed(Duration::from_millis(350)), "350 ms");
        assert_eq!(format_elapsed(Duration::from_millis(2_440)), "2.4 s");
        assert_eq!(format_elapsed(Duration::from_secs(65)), "1 min 5 s");
    }

    #[test]
    fn progress_rounds_down() {
        assert_eq!(format_percent(0.4299), "42%");
        assert_eq!(format_percent(1.5), "100%");
        assert_eq!(format_percent(-1.0), "0%");
    }

    #[test]
    fn statuses_describe_the_run() {
        assert_eq!(RunStatus::Idle.label(), "Ready");
        assert_eq!(
            RunStatus::Running {
                progress: 0.5,
                paused: true
            }
            .label(),
            "Paused at 50%"
        );
        assert_eq!(
            RunStatus::Cancelled { progress: 0.25 }.label(),
            "Cancelled at 25%"
        );
        assert!(
            RunStatus::Finished {
                elapsed: Duration::from_millis(820)
            }
            .label()
            .starts_with("Finished in 820")
        );
    }

    #[test]
    fn summaries_name_the_analysis() {
        let mut setup = SimulationSetup::default();
        assert_eq!(analysis_summary(&setup), "Transient · 5 ms");
        setup.selected = AnalysisChoice::OperatingPoint;
        assert_eq!(analysis_summary(&setup), "Operating point");
        setup.selected = AnalysisChoice::Ac;
        assert_eq!(analysis_summary(&setup), "AC sweep · 1 Hz – 1 MHz");
        assert_eq!(ANALYSES.map(analysis_label)[1], "DC sweep");
    }
}
