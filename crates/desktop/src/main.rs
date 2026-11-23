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
