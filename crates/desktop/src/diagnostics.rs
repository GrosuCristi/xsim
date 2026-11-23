//! The problems listed in the diagnostics dock (Q22): netlist errors,
//! simulation failures, file load warnings and recovery notices.
//!
//! Entries either belong to one circuit tab or to the whole project. The dock
//! shows the project entries plus those of the active circuit.

use crate::ids::TabId;

/// How serious a problem is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

/// What reported a problem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Converting the schematic to a circuit (Q44).
    Netlist,
    /// The simulator (D7: includes how far the run got).
    Simulation,
    /// Opening or saving a project file (Q45).
    File,
    /// Crash recovery (Q41).
    Recovery,
    /// Application settings and plot export.
    Application,
}

impl Source {
    /// Short name shown in the dock's source column.
    pub fn label(self) -> &'static str {
        match self {
            Self::Netlist => "Netlist",
            Self::Simulation => "Simulation",
            Self::File => "File",
            Self::Recovery => "Recovery",
            Self::Application => "Application",
        }
    }
}

/// One listed problem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub source: Source,
    pub message: String,
    /// The circuit the problem belongs to; `None` for project-wide entries.
    pub circuit: Option<TabId>,
}

impl Diagnostic {
    /// A project-wide entry.
    pub fn project(severity: Severity, source: Source, message: impl Into<String>) -> Self {
        Self {
            severity,
            source,
            message: message.into(),
            circuit: None,
        }
    }

    /// An entry that belongs to one circuit tab.
    pub fn circuit(
        tab: TabId,
        severity: Severity,
        source: Source,
        message: impl Into<String>,
    ) -> Self {
        Self {
            circuit: Some(tab),
            ..Self::project(severity, source, message)
        }
    }
}

/// Number of entries per severity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub errors: usize,
    pub warnings: usize,
    pub infos: usize,
}

impl Counts {
    /// All entries regardless of severity.
    pub fn total(self) -> usize {
        self.errors + self.warnings + self.infos
    }
}

/// All entries of one window.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Diagnostics {
    entries: Vec<Diagnostic>,
}

impl Diagnostics {
    /// Adds an entry. Returns `true` when it is an error, which opens the
    /// dock.
    pub fn push(&mut self, diagnostic: Diagnostic) -> bool {
        let is_error = diagnostic.severity == Severity::Error;
        self.entries.push(diagnostic);
        is_error
    }

    /// Removes the entries `source` reported for `circuit`, so a new run
    /// replaces the problems of the previous one.
    pub fn clear_source(&mut self, source: Source, circuit: Option<TabId>) {
        self.entries
            .retain(|d| !(d.source == source && d.circuit == circuit));
    }

    /// Removes every entry of a deleted circuit.
    pub fn clear_circuit(&mut self, circuit: TabId) {
        self.entries.retain(|d| d.circuit != Some(circuit));
    }

    /// Removes the entries visible while `active` is shown.
    pub fn clear_visible(&mut self, active: TabId) {
        self.entries
            .retain(|d| d.circuit.is_some() && d.circuit != Some(active));
    }

    /// Removes every entry (a different project was opened).
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Entries shown while `active` is the current circuit, most severe
    /// first, otherwise in the order they were reported.
    pub fn visible(&self, active: TabId) -> Vec<&Diagnostic> {
        let mut shown: Vec<&Diagnostic> = self
            .entries
            .iter()
            .filter(|d| d.circuit.is_none() || d.circuit == Some(active))
            .collect();
        shown.sort_by_key(|d| d.severity);
        shown
    }

    /// Counts of the entries [`Self::visible`] returns.
    pub fn counts(&self, active: TabId) -> Counts {
        self.visible(active)
            .into_iter()
            .fold(Counts::default(), |mut counts, d| {
                match d.severity {
                    Severity::Error => counts.errors += 1,
                    Severity::Warning => counts.warnings += 1,
                    Severity::Info => counts.infos += 1,
                }
                counts
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::TabIdAllocator;

    #[test]
    fn shows_project_entries_and_the_active_circuit_most_severe_first() {
        let mut ids = TabIdAllocator::default();
        let (a, b) = (ids.allocate(), ids.allocate());
        let mut diagnostics = Diagnostics::default();
        diagnostics.push(Diagnostic::project(
            Severity::Info,
            Source::File,
            "Migrated",
        ));
        assert!(diagnostics.push(Diagnostic::circuit(
            a,
            Severity::Error,
            Source::Netlist,
            "No ground"
        )));
        diagnostics.push(Diagnostic::circuit(
            b,
            Severity::Warning,
            Source::Simulation,
            "Cancelled",
        ));
        let shown: Vec<_> = diagnostics
            .visible(a)
            .iter()
            .map(|d| d.message.as_str())
            .collect();
        assert_eq!(shown, ["No ground", "Migrated"]);
        assert_eq!(
            diagnostics.counts(b),
            Counts {
                errors: 0,
                warnings: 1,
                infos: 1
            }
        );
    }

    #[test]
    fn a_new_run_replaces_only_its_own_source() {
        let mut ids = TabIdAllocator::default();
        let a = ids.allocate();
        let mut diagnostics = Diagnostics::default();
        diagnostics.push(Diagnostic::circuit(
            a,
            Severity::Error,
            Source::Netlist,
            "x",
        ));
        diagnostics.push(Diagnostic::circuit(
            a,
            Severity::Error,
            Source::Simulation,
            "y",
        ));
        diagnostics.clear_source(Source::Netlist, Some(a));
        assert_eq!(diagnostics.counts(a).errors, 1);
        diagnostics.clear_circuit(a);
        assert_eq!(diagnostics.counts(a).total(), 0);
    }

    #[test]
    fn clearing_the_visible_entries_keeps_other_circuits() {
        let mut ids = TabIdAllocator::default();
        let (a, b) = (ids.allocate(), ids.allocate());
        let mut diagnostics = Diagnostics::default();
        diagnostics.push(Diagnostic::project(Severity::Warning, Source::File, "w"));
        diagnostics.push(Diagnostic::circuit(
            a,
            Severity::Error,
            Source::Netlist,
            "a",
        ));
        diagnostics.push(Diagnostic::circuit(
            b,
            Severity::Error,
            Source::Netlist,
            "b",
        ));
        diagnostics.clear_visible(a);
        assert_eq!(diagnostics.counts(a).total(), 0);
        assert_eq!(diagnostics.counts(b).errors, 1);
    }
}
