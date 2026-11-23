//! xsim desktop application: bootstraps GPUI Kit and opens the main
//! window, optionally on the project given as the first argument.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

// GPUI's derive macros emit `gpui::` paths.
extern crate gpui_kit as gpui;

mod actions;
mod analysis_form;
mod appearance;
mod diagnostics;
mod dialogs;
mod files;
mod ids;
mod layout;
mod logo;
mod motion;
mod prompts;
mod simulation;
mod ui;
mod workspace;

#[cfg(test)]
mod test_support;

use gpui_kit::component::TitleBar;
use gpui_kit::*;
use std::path::PathBuf;
use workspace::{Services, Workspace};
use xsim_project::appdata::AppData;

/// Initial window size.
const WINDOW_SIZE: (f32, f32) = (1360.0, 860.0);

/// Smallest usable window size.
const MIN_WINDOW_SIZE: (f32, f32) = (900.0, 560.0);

fn main() {
    crash_log::install(AppData::locate().root());
    let path = std::env::args_os().nth(1).map(PathBuf::from);
    // Q14: Wayland only on Linux.
    #[cfg(target_os = "linux")]
    let app = gpui_kit::platform::linux(WindowingModes::WAYLAND);
    #[cfg(not(target_os = "linux"))]
    let app = gpui_kit::application();
    app.with_assets(gpui_kit::assets::AllAssets)
        .with_quit_mode(QuitMode::LastWindowClosed)
        .run(move |cx| {
            setup(cx);
            let services = Services::native(AppData::locate());
            let opened = gpui_kit::open_window(window_options(cx), cx, |window, cx| {
                cx.new(|cx| Workspace::new(path, services, window, cx))
            });
            match opened {
                Ok(_) => cx.activate(true),
                Err(error) => {
                    eprintln!("xsim could not open its window: {error}");
                    cx.quit();
                }
            }
        });
}

/// Initializes the kit, motion (D19), the shortcuts and the dialog windows
/// (D22: their modal layer goes into every window opened afterwards, and
/// their title row shows the logo). The headless tests start the same way,
/// so they run the application's setup.
fn setup(cx: &mut App) {
    gpui_kit::init(cx);
    motion::init(cx);
    actions::bind_keys(cx);
    xsim_editor::dialog_window::init(cx);
    xsim_editor::dialog_window::set_title_icon(cx, workspace::dialog_logo);
}

/// A centred window with the kit's custom title bar.
fn window_options(cx: &App) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(WINDOW_SIZE.0), px(WINDOW_SIZE.1)),
            cx,
        ))),
        window_min_size: Some(size(px(MIN_WINDOW_SIZE.0), px(MIN_WINDOW_SIZE.1))),
        ..TitleBar::window_options()
    }
}

/// D25: every panic is recorded in `crash.log` in the data directory
/// (`$XSIM_DATA_DIR` when set) and on stderr.
///
/// A panic inside a window callback cannot unwind out of the window
/// procedure, so the process aborts (exit code 0xc0000409 on Windows)
/// without a message the user can see; this file keeps what happened.
/// Panics of a simulation run are recorded too, with the name of their
/// thread; the runner still catches them and reports the crash (D7).
mod crash_log {
    use std::backtrace::Backtrace;
    use std::fs::{self, OpenOptions};
    use std::io::{self, Write};
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// The log's name in the data directory.
    pub const FILE_NAME: &str = "crash.log";

    /// Size above which the log starts over; the previous one is kept as
    /// `crash.log.old`.
    const MAX_LEN: u64 = 1 << 20;

    /// Records every panic before the previous hook (which prints Rust's
    /// usual message) runs.
    pub fn install(data_dir: &Path) {
        let path = data_dir.join(FILE_NAME);
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // `thread::current` only fails while a thread's locals are
            // being destroyed, which no xsim thread panics in.
            let thread = std::thread::current();
            let entry = report(
                SystemTime::now(),
                thread.name().unwrap_or("unnamed"),
                &info
                    .location()
                    .map_or_else(|| "unknown".into(), ToString::to_string),
                info.payload_as_str().unwrap_or("(no message)"),
                &Backtrace::force_capture().to_string(),
            );
            // Nothing here may panic: a panic in the hook aborts at once.
            let _ = io::stderr().write_all(entry.as_bytes());
            let _ = append(&path, &entry);
            previous(info);
        }));
    }

    /// One log entry.
    pub fn report(
        time: SystemTime,
        thread: &str,
        location: &str,
        message: &str,
        backtrace: &str,
    ) -> String {
        format!(
            "=== xsim {} panicked at {} ===\nthread: {thread}\nlocation: {location}\nmessage: {message}\nbacktrace:\n{}\n\n",
            env!("CARGO_PKG_VERSION"),
            utc_timestamp(time),
            backtrace.trim_end(),
        )
    }

    /// Appends `entry` to the log at `path`, creating its directory; a log
    /// grown past [`MAX_LEN`] is moved aside first. When it cannot be moved
    /// (another xsim has it open), the entry is still appended.
    pub fn append(path: &Path, entry: &str) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        if fs::metadata(path).is_ok_and(|meta| meta.len() > MAX_LEN) {
            let _ = fs::rename(path, path.with_extension("log.old"));
        }
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?
            .write_all(entry.as_bytes())
    }

    /// `time` in UTC as `YYYY-MM-DDTHH:MM:SSZ`.
    pub fn utc_timestamp(time: SystemTime) -> String {
        let seconds = time
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        let (year, month, day) = civil_date(seconds / 86_400);
        let second_of_day = seconds % 86_400;
        format!(
            "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
            second_of_day / 3600,
            second_of_day % 3600 / 60,
            second_of_day % 60
        )
    }

    /// The calendar date `days` after 1970-01-01 (Howard Hinnant's
    /// `civil_from_days`).
    fn civil_date(days: u64) -> (u64, u64, u64) {
        let shifted = days + 719_468;
        let era = shifted / 146_097;
        let day_of_era = shifted % 146_097;
        let year_of_era =
            (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let month_index = (5 * day_of_year + 2) / 153;
        let day = day_of_year - (153 * month_index + 2) / 5 + 1;
        let month = if month_index < 10 {
            month_index + 3
        } else {
            month_index - 9
        };
        let year = year_of_era + era * 400 + u64::from(month <= 2);
        (year, month, day)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::time::Duration;

        fn at(seconds: u64) -> SystemTime {
            UNIX_EPOCH + Duration::from_secs(seconds)
        }

        #[test]
        fn timestamps_are_utc_dates_and_times() {
            assert_eq!(utc_timestamp(UNIX_EPOCH), "1970-01-01T00:00:00Z");
            assert_eq!(utc_timestamp(at(951_782_400)), "2000-02-29T00:00:00Z");
            assert_eq!(utc_timestamp(at(1_000_000_000)), "2001-09-09T01:46:40Z");
            assert_eq!(utc_timestamp(at(1_798_761_599)), "2026-12-31T23:59:59Z");
        }

        #[test]
        fn an_entry_says_when_where_and_what() {
            let entry = report(
                at(1_000_000_000),
                "main",
                "src/workspace/circuits.rs:64:9",
                "RefCell already borrowed",
                "   0: xsim::main\n",
            );
            assert!(entry.starts_with("=== xsim "), "{entry}");
            assert!(entry.contains("panicked at 2001-09-09T01:46:40Z ==="));
            assert!(entry.contains("\nthread: main\n"));
            assert!(entry.contains("\nlocation: src/workspace/circuits.rs:64:9\n"));
            assert!(entry.contains("\nmessage: RefCell already borrowed\n"));
            assert!(
                entry.ends_with("backtrace:\n   0: xsim::main\n\n"),
                "{entry:?}"
            );
        }

        #[test]
        fn entries_are_appended_in_a_new_data_directory() {
            let dir = tempfile::tempdir().expect("temporary directory");
            let path = dir.path().join("app-data").join(FILE_NAME);
            append(&path, "first\n").expect("append to a new log");
            append(&path, "second\n").expect("append again");
            assert_eq!(fs::read_to_string(&path).expect("log"), "first\nsecond\n");
        }

        #[test]
        fn a_full_log_is_moved_aside() {
            let dir = tempfile::tempdir().expect("temporary directory");
            let path = dir.path().join(FILE_NAME);
            let full = "x".repeat(MAX_LEN as usize + 1);
            fs::write(&path, &full).expect("a full log");
            append(&path, "fresh\n").expect("append");
            assert_eq!(fs::read_to_string(&path).expect("log"), "fresh\n");
            let old = fs::read_to_string(dir.path().join("crash.log.old")).expect("old log");
            assert_eq!(old.len(), full.len());
        }

        /// Set in a child run of this test binary: the data directory the
        /// child's hook logs to. The hook is process-wide, so it is only
        /// installed in a process of its own.
        const CHILD_DATA_DIR: &str = "XSIM_CRASH_LOG_TEST_DATA_DIR";

        /// Runs test `name` of this binary alone in a child process that logs
        /// to `data_dir`, which is also its `$XSIM_DATA_DIR`.
        fn run_child(name: &str, data_dir: &Path) -> std::process::Output {
            let exe = std::env::current_exe().expect("the test binary");
            std::process::Command::new(exe)
                .args([
                    &format!("crash_log::tests::{name}"),
                    "--exact",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env(CHILD_DATA_DIR, data_dir)
                .env(xsim_project::appdata::DATA_DIR_ENV, data_dir)
                .output()
                .expect("run the test binary")
        }

        /// What the owner saw: a panic that cannot unwind out of a window
        /// procedure aborts the process. The entry is in the log anyway, in
        /// `$XSIM_DATA_DIR` as `main` installs it.
        #[test]
        fn a_panic_that_aborts_the_process_is_logged() {
            const MESSAGE: &str = "crash-log test: a panic inside a window procedure";
            if std::env::var_os(CHILD_DATA_DIR).is_some() {
                install(xsim_project::appdata::AppData::locate().root());
                extern "C" fn window_procedure() {
                    panic!("{MESSAGE}");
                }
                window_procedure();
                return;
            }
            let dir = tempfile::tempdir().expect("temporary directory");
            let data_dir = dir.path().join("app-data");
            let output = run_child("a_panic_that_aborts_the_process_is_logged", &data_dir);
            assert!(!output.status.success(), "the child aborts");
            if cfg!(windows) {
                assert_eq!(
                    output.status.code(),
                    Some(0xc000_0409_u32 as i32),
                    "as in the owner's report"
                );
            }
            let log = fs::read_to_string(data_dir.join(FILE_NAME)).expect("crash.log");
            assert!(log.starts_with("=== xsim "), "{log}");
            assert!(log.contains(&format!("\nmessage: {MESSAGE}\n")), "{log}");
            assert!(
                log.contains("\nlocation: ") && log.contains("main.rs:"),
                "{log}"
            );
            assert!(log.contains("\nbacktrace:\n"), "{log}");
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(stderr.contains(MESSAGE), "also on stderr: {stderr}");
        }

        /// A panic that is caught (as the simulation runner catches its
        /// own) still unwinds after it is logged; a log that cannot be
        /// written is skipped, and the hook installed before still runs.
        #[test]
        fn a_logged_panic_still_unwinds_even_when_the_log_cannot_be_written() {
            const MESSAGE: &str = "crash-log test: a caught panic";
            if let Some(data_dir) = std::env::var_os(CHILD_DATA_DIR) {
                let data_dir = Path::new(&data_dir);
                install(data_dir);
                // A data directory that cannot be created: its parent is a
                // file.
                let blocked = data_dir.join("blocked");
                fs::write(&blocked, "").expect("a file");
                install(&blocked.join("app-data"));
                let caught = std::panic::catch_unwind(|| panic!("{MESSAGE}"));
                let payload = caught.expect_err("the panic unwinds");
                assert_eq!(
                    payload.downcast_ref::<String>().map(String::as_str),
                    Some(MESSAGE)
                );
                return;
            }
            let dir = tempfile::tempdir().expect("temporary directory");
            let data_dir = dir.path().join("app-data");
            fs::create_dir_all(&data_dir).expect("data directory");
            let output = run_child(
                "a_logged_panic_still_unwinds_even_when_the_log_cannot_be_written",
                &data_dir,
            );
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(output.status.success(), "the child ran on: {stderr}");
            let log = fs::read_to_string(data_dir.join(FILE_NAME)).expect("crash.log");
            assert_eq!(log.matches("=== xsim ").count(), 1, "{log}");
            assert!(log.contains(&format!("\nmessage: {MESSAGE}\n")), "{log}");
            assert!(
                !data_dir.join("blocked").join("app-data").exists(),
                "nothing was written below a file"
            );
            assert!(stderr.matches(MESSAGE).count() >= 2, "both hooks: {stderr}");
        }
    }
}
