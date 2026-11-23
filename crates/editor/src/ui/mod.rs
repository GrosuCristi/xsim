//! The interactive editor view: actions and shortcuts, mouse gestures, the
//! drawing tools, editing commands, hover feedback, the inline label field,
//! the toolbar and the status strip.

pub(crate) mod actions;
mod commands;
pub(crate) mod gesture;
pub(crate) mod hover;
mod icons;
pub(crate) mod input;
pub(crate) mod label_editor;
pub(crate) mod navigation;
mod render;
pub(crate) mod state;
mod status_bar;
pub(crate) mod toolbar;
mod tools;
