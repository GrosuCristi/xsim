//! Running simulations and presenting their results.
//!
//! * [`runner`] — the background thread and its controls.
//! * [`summary`] — labels for analyses, durations and run states.
//! * [`traces`] — default traces, probe expressions, result consistency.

pub mod runner;
pub mod summary;
pub mod traces;
