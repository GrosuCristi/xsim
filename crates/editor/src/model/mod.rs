//! Schematic editing logic without any UI: geometry, hit testing, routing,
//! moving selections and dragging wire segments, edit operations, undo
//! history, clipboard fragments, probing and measurements. Everything here
//! is covered by ordinary unit tests.

pub mod cleanup;
pub mod drag;
#[cfg(test)]
mod drag_fuzz;
pub mod fragment;
pub mod geometry;
pub mod history;
pub mod hit_test;
pub mod label_geometry;
pub mod measure;
pub mod ops;
pub mod probe;
pub mod routing;
pub mod segment;
#[cfg(test)]
mod segment_fuzz;
pub mod selection;
pub mod text_layout;
pub mod transform;
