//! # xsim project document
//!
//! A project is one self-contained `.xsim_proj` file (Q31, Q02) holding every
//! circuit, its schematic, simulation setup, plot configuration and the last
//! simulation result (Q35, Q38, Q39). The file is versioned JSON with stable
//! field order so it diffs cleanly in Git (Q34, Q42).
//!
//! * [`schematic`] — the editable drawing: parts, wires, net labels.
//! * [`symbols`] — pin geometry of every part (shared with the editor).
//! * [`netlist`](mod@netlist) — turns a schematic into an engine [`xsim_spice_engine::Circuit`].
//! * [`document`] — the project and per-circuit documents.
//! * [`io`] — load/save with schema migration and newer-version handling.
//! * [`appdata`] — recent files, app settings, crash-recovery snapshots.

pub mod appdata;
pub mod document;
mod durable;
pub mod error;
pub mod io;
pub mod netlist;
pub mod schematic;
pub mod symbols;

pub use document::{
    AnalysisChoice, CircuitDocument, PlotPane, PlotSetup, Project, SchematicView, SimulationSetup,
    StoredResult, Trace, WorkspaceLayout, YScale,
};
pub use error::ProjectError;
pub use io::{Compatibility, LoadedProject, PROJECT_EXTENSION, SCHEMA_VERSION, SYMBOL_EXTENSION};
pub use netlist::{Connectivity, Net, Netlist, NetlistError, connectivity, netlist};
pub use schematic::{Component, Id, NetLabel, Orientation, Part, PartKind, Point, Schematic, Wire};
