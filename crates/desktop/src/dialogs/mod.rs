//! Modal dialogs of the workspace, each in a window of its own over the
//! main window (D22, `xsim_editor::dialog_window`).
//!
//! * [`analysis`] — analysis settings (Q75, Q76).
//! * [`settings`] — theme and interface scale (Q25, Q30).
//! * [`recovery`] — restoring work after a crash (Q41).

pub mod analysis;
pub mod recovery;
pub mod settings;
