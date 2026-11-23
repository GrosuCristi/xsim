//! GPUI-free viewer logic: axes, ticks, ranges, traces, setup edits, pane
//! heights, cursors, measurements, completion and colours. Everything here
//! is plain data and functions so it is tested without a window.

pub mod axis;
pub mod completion;
pub mod cursors;
pub mod format;
pub mod heights;
pub mod measurements;
pub mod op_table;
pub mod palette;
pub mod ranges;
pub mod setup;
pub mod ticks;
pub mod traces;
pub mod units;
pub mod viewport;
