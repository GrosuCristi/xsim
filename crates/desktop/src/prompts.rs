//! Native file pickers and confirmations.
//!
//! The workspace asks through [`Prompts`] so headless tests can script the
//! answers instead of blocking on a real dialog.

use gpui_kit::Window;
use rfd::{FileDialog, MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
use std::path::{Path, PathBuf};
use xsim_plot::ImageFormat;
use xsim_project::PROJECT_EXTENSION;

/// Answer to "save changes before closing?".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsavedChoice {
    Save,
    Discard,
    Cancel,
}

/// Plot export formats (Q137).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportKind {
    Png,
    Jpeg,
    Svg,
    Csv,
}

impl ExportKind {
    /// File extension without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Svg => "svg",
            Self::Csv => "csv",
        }
    }

    /// Name of the file type in the save dialog.
    pub fn description(self) -> &'static str {
        match self {
            Self::Png => "PNG image",
            Self::Jpeg => "JPEG image",
            Self::Svg => "SVG image",
            Self::Csv => "CSV table",
        }
    }

    /// The viewer's image format; `None` for CSV.
    pub fn image_format(self) -> Option<ImageFormat> {
        match self {
            Self::Png => Some(ImageFormat::Png),
            Self::Jpeg => Some(ImageFormat::Jpeg),
            Self::Svg => Some(ImageFormat::Svg),
            Self::Csv => None,
        }
    }
}

/// Questions the workspace asks the user.
pub trait Prompts {
    /// Asks whether to save `project` before it is closed or replaced.
    fn unsaved_changes(&self, project: &str, window: &Window) -> UnsavedChoice;
    /// Picks a project file to open.
    fn open_project(&self, window: &Window) -> Option<PathBuf>;
    /// Picks where to save a project, starting at `suggested`.
    fn save_project(&self, suggested: &Path, window: &Window) -> Option<PathBuf>;
    /// Picks where to export the plot.
    fn export_plot(&self, kind: ExportKind, suggested: &str, window: &Window) -> Option<PathBuf>;
    /// Confirms deleting circuit `name` and its results.
    fn confirm_delete_circuit(&self, name: &str, window: &Window) -> bool;
}

/// The operating system's dialogs.
pub struct NativePrompts;

impl Prompts for NativePrompts {
    fn unsaved_changes(&self, project: &str, window: &Window) -> UnsavedChoice {
        let answer = MessageDialog::new()
            .set_parent(window)
            .set_level(MessageLevel::Warning)
            .set_title("Unsaved changes")
            .set_description(format!(
                "Do you want to save the changes to \"{project}\"?\n\nYour changes are lost if you don't save them."
            ))
            .set_buttons(MessageButtons::YesNoCancel)
            .show();
        match answer {
            MessageDialogResult::Yes => UnsavedChoice::Save,
            MessageDialogResult::No => UnsavedChoice::Discard,
            _ => UnsavedChoice::Cancel,
        }
    }

    fn open_project(&self, window: &Window) -> Option<PathBuf> {
        FileDialog::new()
            .set_parent(window)
            .set_title("Open project")
            .add_filter("xsim project", &[PROJECT_EXTENSION])
            .pick_file()
    }

    fn save_project(&self, suggested: &Path, window: &Window) -> Option<PathBuf> {
        with_suggestion(FileDialog::new(), suggested)
            .set_parent(window)
            .set_title("Save project")
            .add_filter("xsim project", &[PROJECT_EXTENSION])
            .save_file()
    }

    fn export_plot(&self, kind: ExportKind, suggested: &str, window: &Window) -> Option<PathBuf> {
        FileDialog::new()
            .set_parent(window)
            .set_title("Export plot")
            .set_file_name(format!("{suggested}.{}", kind.extension()))
            .add_filter(kind.description(), &[kind.extension()])
            .save_file()
    }

    fn confirm_delete_circuit(&self, name: &str, window: &Window) -> bool {
        let answer = MessageDialog::new()
            .set_parent(window)
            .set_level(MessageLevel::Warning)
            .set_title("Delete circuit")
            .set_description(format!(
                "Delete \"{name}\" from the project?\n\nIts schematic, analysis settings, plots and results are removed. Undo cannot bring them back."
            ))
            .set_buttons(MessageButtons::OkCancel)
            .show();
        matches!(answer, MessageDialogResult::Ok | MessageDialogResult::Yes)
    }
}

/// Starts the dialog in the folder of `suggested` with its file name filled
/// in.
fn with_suggestion(dialog: FileDialog, suggested: &Path) -> FileDialog {
    let dialog = match suggested.parent().filter(|p| p.is_dir()) {
        Some(dir) => dialog.set_directory(dir),
        None => dialog,
    };
    match suggested.file_name() {
        Some(name) => dialog.set_file_name(name.to_string_lossy()),
        None => dialog,
    }
}

/// Scripted answers for headless tests.
#[cfg(test)]
pub mod scripted {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    /// Answers queued by a test; an empty queue answers "cancel".
    #[derive(Default)]
    pub struct ScriptedPrompts {
        pub unsaved: RefCell<VecDeque<UnsavedChoice>>,
        pub open: RefCell<VecDeque<PathBuf>>,
        pub save: RefCell<VecDeque<PathBuf>>,
        pub export: RefCell<VecDeque<PathBuf>>,
        pub delete: RefCell<VecDeque<bool>>,
        /// How often each question was asked, in order.
        pub asked: RefCell<Vec<&'static str>>,
    }

    impl ScriptedPrompts {
        fn ask(&self, question: &'static str) {
            self.asked.borrow_mut().push(question);
        }
    }

    impl Prompts for ScriptedPrompts {
        fn unsaved_changes(&self, _: &str, _: &Window) -> UnsavedChoice {
            self.ask("unsaved");
            self.unsaved
                .borrow_mut()
                .pop_front()
                .unwrap_or(UnsavedChoice::Cancel)
        }

        fn open_project(&self, _: &Window) -> Option<PathBuf> {
            self.ask("open");
            self.open.borrow_mut().pop_front()
        }

        fn save_project(&self, _: &Path, _: &Window) -> Option<PathBuf> {
            self.ask("save");
            self.save.borrow_mut().pop_front()
        }

        fn export_plot(&self, _: ExportKind, _: &str, _: &Window) -> Option<PathBuf> {
            self.ask("export");
            self.export.borrow_mut().pop_front()
        }

        fn confirm_delete_circuit(&self, _: &str, _: &Window) -> bool {
            self.ask("delete");
            self.delete.borrow_mut().pop_front().unwrap_or(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_kinds_know_their_files() {
        assert_eq!(ExportKind::Jpeg.extension(), "jpg");
        assert_eq!(ExportKind::Svg.image_format(), Some(ImageFormat::Svg));
        assert_eq!(ExportKind::Csv.image_format(), None);
        assert_eq!(ExportKind::Png.description(), "PNG image");
    }
}
