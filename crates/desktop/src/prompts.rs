//! Native file pickers and confirmations.
//!
//! D25: no question blocks inside a GPUI callback. A native dialog shown
//! synchronously runs a nested Windows message loop while GPUI's App is
//! borrowed by the callback; GPUI's queued tasks then run inside that loop,
//! the first one that touches the App panics, and a panic inside a window
//! procedure aborts the process. So every method here only starts its
//! dialog and returns an [`Answer`], which the workspace awaits in a task
//! and applies once the dialog has closed.
//!
//! * Message boxes are GPUI's platform prompts: on Windows a task dialog on
//!   a thread of its own, owned by the window that asks.
//! * File pickers are `rfd`'s asynchronous dialogs, which also run on a
//!   thread of their own (with COM set up there) and keep the file-type
//!   filters and titles GPUI's pickers cannot set.
//!
//! The workspace asks through [`Prompts`] so headless tests can script the
//! answers instead of opening a real dialog.

use gpui_kit::{App, PromptButton, PromptLevel, Window};
use rfd::AsyncFileDialog;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use xsim_plot::ImageFormat;
use xsim_project::PROJECT_EXTENSION;

/// The user's answer to a question, available once its dialog has closed.
pub type Answer<T> = Pin<Box<dyn Future<Output = T>>>;

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

/// Questions the workspace asks the user. Each method opens its dialog
/// over `window` without waiting for it and returns the answer to await.
pub trait Prompts {
    /// Asks whether to save `project` before it is closed or replaced.
    fn unsaved_changes(
        &self,
        project: &str,
        window: &mut Window,
        cx: &mut App,
    ) -> Answer<UnsavedChoice>;
    /// Picks a project file to open.
    fn open_project(&self, window: &mut Window, cx: &mut App) -> Answer<Option<PathBuf>>;
    /// Picks where to save a project, starting at `suggested`.
    fn save_project(
        &self,
        suggested: &Path,
        window: &mut Window,
        cx: &mut App,
    ) -> Answer<Option<PathBuf>>;
    /// Picks where to export the plot.
    fn export_plot(
        &self,
        kind: ExportKind,
        suggested: &str,
        window: &mut Window,
        cx: &mut App,
    ) -> Answer<Option<PathBuf>>;
    /// Confirms deleting circuit `name` and its results.
    fn confirm_delete_circuit(&self, name: &str, window: &mut Window, cx: &mut App)
    -> Answer<bool>;
}

/// The buttons of the unsaved-changes question, in [`UnsavedChoice`]
/// order.
const UNSAVED_BUTTONS: [&str; 3] = ["Save", "Don't save", "Cancel"];

/// The buttons of the delete question: confirm, then cancel.
const DELETE_BUTTONS: [&str; 2] = ["Delete", "Cancel"];

/// The text of the unsaved-changes question: message and detail.
fn unsaved_text(project: &str) -> (String, &'static str) {
    (
        format!("Do you want to save the changes to \"{project}\"?"),
        "Your changes are lost if you don't save them.",
    )
}

/// The text of the delete question: message and detail.
fn delete_text(name: &str) -> (String, &'static str) {
    (
        format!("Delete \"{name}\" from the project?"),
        "Its schematic, analysis settings, plots and results are removed. Undo cannot bring them back.",
    )
}

/// The choice behind the unsaved-changes button at `index`; a dialog
/// closed without a button (Escape, or its window going away) cancels.
fn unsaved_choice(index: Option<usize>) -> UnsavedChoice {
    match index {
        Some(0) => UnsavedChoice::Save,
        Some(1) => UnsavedChoice::Discard,
        _ => UnsavedChoice::Cancel,
    }
}

/// Buttons as GPUI prompt buttons: the first is the default, the last
/// answers Escape.
fn prompt_buttons(labels: &[&str]) -> Vec<PromptButton> {
    let last = labels.len().saturating_sub(1);
    labels
        .iter()
        .enumerate()
        .map(|(index, &label)| match index {
            0 => PromptButton::ok(label),
            index if index == last => PromptButton::cancel(label),
            _ => PromptButton::new(label),
        })
        .collect()
}

/// Opens a warning with `buttons` over `window`; the answer is the index
/// of the button pressed, `None` when the dialog went away without one.
fn ask_warning(
    message: &str,
    detail: &str,
    buttons: &[&str],
    window: &mut Window,
    cx: &mut App,
) -> Answer<Option<usize>> {
    let answer = window.prompt(
        PromptLevel::Warning,
        message,
        Some(detail),
        &prompt_buttons(buttons),
        cx,
    );
    Box::pin(async move { answer.await.ok() })
}

/// The operating system's dialogs.
pub struct NativePrompts;

impl Prompts for NativePrompts {
    fn unsaved_changes(
        &self,
        project: &str,
        window: &mut Window,
        cx: &mut App,
    ) -> Answer<UnsavedChoice> {
        let (message, detail) = unsaved_text(project);
        let answer = ask_warning(&message, detail, &UNSAVED_BUTTONS, window, cx);
        Box::pin(async move { unsaved_choice(answer.await) })
    }

    fn open_project(&self, window: &mut Window, _: &mut App) -> Answer<Option<PathBuf>> {
        let picked = AsyncFileDialog::new()
            .set_parent(window)
            .set_title("Open project")
            .add_filter("xsim project", &[PROJECT_EXTENSION])
            .pick_file();
        Box::pin(async move { picked.await.map(|file| file.path().to_path_buf()) })
    }

    fn save_project(
        &self,
        suggested: &Path,
        window: &mut Window,
        _: &mut App,
    ) -> Answer<Option<PathBuf>> {
        let picked = with_suggestion(AsyncFileDialog::new(), suggested)
            .set_parent(window)
            .set_title("Save project")
            .add_filter("xsim project", &[PROJECT_EXTENSION])
            .save_file();
        Box::pin(async move { picked.await.map(|file| file.path().to_path_buf()) })
    }

    fn export_plot(
        &self,
        kind: ExportKind,
        suggested: &str,
        window: &mut Window,
        _: &mut App,
    ) -> Answer<Option<PathBuf>> {
        let picked = AsyncFileDialog::new()
            .set_parent(window)
            .set_title("Export plot")
            .set_file_name(format!("{suggested}.{}", kind.extension()))
            .add_filter(kind.description(), &[kind.extension()])
            .save_file();
        Box::pin(async move { picked.await.map(|file| file.path().to_path_buf()) })
    }

    fn confirm_delete_circuit(
        &self,
        name: &str,
        window: &mut Window,
        cx: &mut App,
    ) -> Answer<bool> {
        let (message, detail) = delete_text(name);
        let answer = ask_warning(&message, detail, &DELETE_BUTTONS, window, cx);
        Box::pin(async move { answer.await == Some(0) })
    }
}

/// Starts the dialog in the folder of `suggested` with its file name filled
/// in.
fn with_suggestion(dialog: AsyncFileDialog, suggested: &Path) -> AsyncFileDialog {
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
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;

    /// Answers queued by a test; an empty queue answers "cancel".
    ///
    /// A question is answered as soon as its task runs, unless [`Self::hold`]
    /// is set: then it stays open, as a native dialog does until the user
    /// picks a button, until [`Self::answer_held`] closes it.
    #[derive(Default)]
    pub struct ScriptedPrompts {
        pub unsaved: RefCell<VecDeque<UnsavedChoice>>,
        pub open: RefCell<VecDeque<PathBuf>>,
        pub save: RefCell<VecDeque<PathBuf>>,
        pub export: RefCell<VecDeque<PathBuf>>,
        pub delete: RefCell<VecDeque<bool>>,
        /// How often each question was asked, in order.
        pub asked: RefCell<Vec<&'static str>>,
        /// Keeps the next questions open until the test answers them.
        pub hold: Cell<bool>,
        /// One per open question, oldest first.
        held: RefCell<VecDeque<async_channel::Sender<()>>>,
    }

    impl ScriptedPrompts {
        /// Records `question` and returns `answer`, now or once the test
        /// closes the question.
        fn ask<T: 'static>(&self, question: &'static str, answer: T) -> Answer<T> {
            self.asked.borrow_mut().push(question);
            if !self.hold.get() {
                return Box::pin(std::future::ready(answer));
            }
            let (close, closed) = async_channel::bounded::<()>(1);
            self.held.borrow_mut().push_back(close);
            Box::pin(async move {
                // A test that ends first closes the question too.
                closed.recv().await.ok();
                answer
            })
        }

        /// Closes the oldest open question with its queued answer; `false`
        /// when no question is open.
        pub fn answer_held(&self) -> bool {
            let close = self.held.borrow_mut().pop_front();
            close.is_some_and(|close| close.try_send(()).is_ok())
        }

        /// Questions asked and not answered yet.
        pub fn open_questions(&self) -> usize {
            self.held.borrow().len()
        }
    }

    impl Prompts for ScriptedPrompts {
        fn unsaved_changes(&self, _: &str, _: &mut Window, _: &mut App) -> Answer<UnsavedChoice> {
            let answer = self.unsaved.borrow_mut().pop_front();
            self.ask("unsaved", answer.unwrap_or(UnsavedChoice::Cancel))
        }

        fn open_project(&self, _: &mut Window, _: &mut App) -> Answer<Option<PathBuf>> {
            let answer = self.open.borrow_mut().pop_front();
            self.ask("open", answer)
        }

        fn save_project(&self, _: &Path, _: &mut Window, _: &mut App) -> Answer<Option<PathBuf>> {
            let answer = self.save.borrow_mut().pop_front();
            self.ask("save", answer)
        }

        fn export_plot(
            &self,
            _: ExportKind,
            _: &str,
            _: &mut Window,
            _: &mut App,
        ) -> Answer<Option<PathBuf>> {
            let answer = self.export.borrow_mut().pop_front();
            self.ask("export", answer)
        }

        fn confirm_delete_circuit(&self, _: &str, _: &mut Window, _: &mut App) -> Answer<bool> {
            let answer = self.delete.borrow_mut().pop_front();
            self.ask("delete", answer.unwrap_or(false))
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

    #[test]
    fn unsaved_buttons_map_to_choices() {
        let choices: Vec<UnsavedChoice> = (0..UNSAVED_BUTTONS.len())
            .map(|index| unsaved_choice(Some(index)))
            .collect();
        assert_eq!(
            choices,
            [
                UnsavedChoice::Save,
                UnsavedChoice::Discard,
                UnsavedChoice::Cancel
            ]
        );
        assert_eq!(unsaved_choice(None), UnsavedChoice::Cancel, "Escape");
    }

    #[test]
    fn the_first_button_confirms_and_the_last_cancels() {
        let buttons = prompt_buttons(&UNSAVED_BUTTONS);
        assert_eq!(buttons[0], PromptButton::ok("Save"));
        assert_eq!(buttons[1], PromptButton::new("Don't save"));
        assert_eq!(buttons[2], PromptButton::cancel("Cancel"));
        let buttons = prompt_buttons(&DELETE_BUTTONS);
        assert_eq!(buttons[0], PromptButton::ok("Delete"));
        assert_eq!(buttons[1], PromptButton::cancel("Cancel"));
    }
}
