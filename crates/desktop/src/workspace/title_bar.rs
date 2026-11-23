//! The title bar: the xsim logo (D20), File / Edit / View / Simulate menus
//! and the project name with its unsaved and read-only markers.

use super::Workspace;
use crate::actions::*;
use crate::files::recent_label;
use crate::logo::logo_for;
use crate::ui::{dot, testable};
use gpui_kit::component::{
    ActiveTheme, Sizable, TITLE_BAR_HEIGHT, TitleBar,
    button::{Button, ButtonVariants},
    h_flex,
    menu::{DropdownMenu, PopupMenu, PopupMenuItem},
    tag::Tag,
};
use gpui_kit::{prelude::*, *};
use std::path::PathBuf;
use xsim_project::appdata::ThemePreference;

/// Height of the logo: 1.5 rem, but never more than the kit's title bar
/// (a fixed height) leaves with 4 px above and below.
pub(super) fn logo_height(rem: Pixels) -> Pixels {
    (rem * 1.5).min(TITLE_BAR_HEIGHT - px(8.))
}

/// Menu state captured at render time; the menu builders run later.
#[derive(Clone)]
struct MenuState {
    focus: FocusHandle,
    workspace: WeakEntity<Workspace>,
    recent: Vec<PathBuf>,
    read_only: bool,
    has_plot: bool,
    can_undo: bool,
    can_redo: bool,
    running: bool,
    paused: bool,
    dark: bool,
    dock_open: bool,
}

impl Workspace {
    /// The kit title bar with the logo, the menus and the project name.
    pub(super) fn render_title_bar(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let state = self.menu_state(cx.weak_entity(), cx);
        let close = cx.weak_entity();
        testable(
            "title-bar",
            TitleBar::new()
                .on_close_window(move |_, window, cx| {
                    close.update(cx, |this, cx| this.exit(window, cx)).ok();
                })
                .child(
                    h_flex()
                        .gap_1()
                        .child(app_logo(window, cx))
                        .child(file_menu(state.clone()))
                        .child(edit_menu(state.clone()))
                        .child(view_menu(state.clone()))
                        .child(simulate_menu(state)),
                )
                .child(self.project_title(cx))
                .child(div().w(rems(12.))),
        )
        .flex_shrink_0()
    }

    fn menu_state(&self, workspace: WeakEntity<Workspace>, cx: &App) -> MenuState {
        let tab = self.active_tab();
        let editor = tab.editor.read(cx);
        MenuState {
            focus: self.focus.clone(),
            workspace,
            recent: self.recent.clone(),
            read_only: self.doc.read_only,
            has_plot: tab.has_result(),
            can_undo: editor.can_undo(),
            can_redo: editor.can_redo(),
            running: tab.is_running(),
            paused: tab.run.as_ref().is_some_and(|r| r.run.is_paused()),
            dark: self.settings.theme == ThemePreference::Dark,
            dock_open: self.doc.project.layout.diagnostics_open,
        }
    }

    /// Project name, unsaved dot and read-only badge, centred.
    fn project_title(&self, cx: &App) -> impl IntoElement {
        let theme = cx.theme();
        h_flex()
            .gap_2()
            .items_center()
            .text_sm()
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.foreground)
                    .child(self.doc.project.name.clone()),
            )
            .when(self.doc.dirty, |this| {
                this.child(dot(theme.muted_foreground).debug_selector(|| "unsaved-marker".into()))
            })
            .when(self.doc.read_only, |this| {
                this.child(Tag::warning().small().child("Read-only"))
            })
    }
}

/// The xsim logo in the corner, in the variant for the theme: white
/// letters on a dark title bar, dark letters on a light one.
fn app_logo(window: &Window, cx: &App) -> impl IntoElement {
    let dark = cx.theme().is_dark();
    let height = logo_height(window.rem_size());
    let logo = logo_for(dark, f32::from(height) * window.scale_factor());
    let variant = if dark {
        "app-logo-dark"
    } else {
        "app-logo-light"
    };
    testable(
        "app-logo",
        testable(
            variant,
            img(logo.image.clone())
                .id("app-logo")
                .role(Role::Image)
                .aria_label("xsim")
                .h(height)
                .w(logo.width_at(height))
                .flex_shrink_0(),
        ),
    )
    .px_1()
    .flex_shrink_0()
}

/// A title-bar menu button.
fn menu_button(id: &'static str, label: &'static str) -> Button {
    Button::new(id).ghost().small().label(label)
}

fn file_menu(state: MenuState) -> impl IntoElement {
    testable(
        "menu-file",
        menu_button("menu-file", "File").dropdown_menu(move |menu, window, cx| {
            let recent = state.recent.clone();
            let workspace = state.workspace.clone();
            let no_plot = !state.has_plot;
            menu.action_context(state.focus.clone())
                .menu("New project", Box::new(NewProject))
                .menu("Open…", Box::new(OpenProject))
                .submenu("Open recent", window, cx, move |menu, _, _| {
                    recent_items(menu, &recent, &workspace)
                })
                .separator()
                .menu_with_disabled("Save", Box::new(SaveProject), state.read_only)
                .menu("Save as…", Box::new(SaveProjectAs))
                .separator()
                .submenu("Export plot", window, cx, move |menu, _, _| {
                    menu.menu_with_disabled("PNG image…", Box::new(ExportPng), no_plot)
                        .menu_with_disabled("JPEG image…", Box::new(ExportJpeg), no_plot)
                        .menu_with_disabled("SVG image…", Box::new(ExportSvg), no_plot)
                        .menu_with_disabled("CSV table…", Box::new(ExportCsv), no_plot)
                })
                .separator()
                .menu("Close project", Box::new(CloseProject))
                .menu("Exit", Box::new(ExitApplication))
        }),
    )
}

/// One entry per recent project (Q46), newest first.
fn recent_items(
    menu: PopupMenu,
    recent: &[PathBuf],
    workspace: &WeakEntity<Workspace>,
) -> PopupMenu {
    if recent.is_empty() {
        return menu.item(PopupMenuItem::new("No recent projects").disabled(true));
    }
    recent.iter().fold(menu, |menu, path| {
        let (path, workspace) = (path.clone(), workspace.clone());
        menu.item(
            PopupMenuItem::new(recent_label(&path)).on_click(move |_, window, cx| {
                workspace
                    .update(cx, |this, cx| this.open_path(path.clone(), window, cx))
                    .ok();
            }),
        )
    })
}

fn edit_menu(state: MenuState) -> impl IntoElement {
    menu_button("menu-edit", "Edit").dropdown_menu(move |menu, _, _| {
        let locked = state.read_only;
        menu.action_context(state.focus.clone())
            .menu_with_disabled("Undo", Box::new(Undo), !state.can_undo)
            .menu_with_disabled("Redo", Box::new(Redo), !state.can_redo)
            .separator()
            .menu_with_disabled("Cut", Box::new(Cut), locked)
            .menu("Copy", Box::new(Copy))
            .menu_with_disabled("Paste", Box::new(Paste), locked)
            .menu_with_disabled("Duplicate", Box::new(Duplicate), locked)
            .menu_with_disabled("Delete", Box::new(Delete), locked)
            .separator()
            .menu("Select all", Box::new(SelectAll))
    })
}

fn view_menu(state: MenuState) -> impl IntoElement {
    menu_button("menu-view", "View").dropdown_menu(move |menu, _, _| {
        menu.action_context(state.focus.clone())
            .menu_with_check("Light theme", !state.dark, Box::new(UseLightTheme))
            .menu_with_check("Dark theme", state.dark, Box::new(UseDarkTheme))
            .separator()
            .menu_with_check(
                "Diagnostics panel",
                state.dock_open,
                Box::new(ToggleDiagnostics),
            )
            .menu("Zoom to fit", Box::new(ZoomToFit))
            .separator()
            .menu("Settings…", Box::new(OpenSettings))
    })
}

fn simulate_menu(state: MenuState) -> impl IntoElement {
    menu_button("menu-simulate", "Simulate").dropdown_menu(move |menu, _, _| {
        let pause_label = if state.paused { "Resume" } else { "Pause" };
        menu.action_context(state.focus.clone())
            .menu_with_disabled(
                "Run",
                Box::new(RunSimulation),
                state.running || state.read_only,
            )
            .menu_with_disabled(pause_label, Box::new(TogglePause), !state.running)
            .menu_with_disabled("Stop", Box::new(StopSimulation), !state.running)
            .separator()
            .menu("Analysis settings…", Box::new(OpenAnalysisSettings))
            .menu_with_disabled("Copy plot", Box::new(CopyPlot), !state.has_plot)
            .separator()
            .menu_with_disabled("New circuit", Box::new(NewCircuit), state.read_only)
    })
}

#[cfg(test)]
mod tests {
    use super::logo_height;
    use crate::actions::{UseDarkTheme, UseLightTheme};
    use crate::appearance::SCALE_PRESETS;
    use crate::logo::logo_for;
    use crate::test_support::{TestEnv, close, init, open};
    use gpui_kit::component::{ActiveTheme, TITLE_BAR_HEIGHT};
    use gpui_kit::{
        Action, Bounds, DevicePixels, Pixels, TestAppContext, VisualTestContext, px, size,
    };

    fn bounds(selector: &'static str, cx: &mut VisualTestContext) -> Bounds<Pixels> {
        cx.debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} is not on screen"))
    }

    fn assert_near(actual: Pixels, expected: Pixels, what: &str) {
        assert!(
            (actual - expected).abs() <= px(0.5),
            "{what}: {actual:?}, expected {expected:?}"
        );
    }

    fn use_theme(action: impl Action, cx: &mut VisualTestContext) {
        cx.dispatch_action(action);
        cx.run_until_parked();
    }

    /// The logo sits at the left of the title bar, before the menus,
    /// inside the bar, as high as [`logo_height`] and as wide as its
    /// proportions make it.
    fn assert_logo_in_place(logo: Bounds<Pixels>, cx: &mut VisualTestContext, what: &str) {
        let (title, file) = (bounds("title-bar", cx), bounds("menu-file", cx));
        let rem = cx.update(|window, _| window.rem_size());
        assert!(
            logo.top() >= title.top() && logo.bottom() <= title.bottom(),
            "{what}: {logo:?} inside the title bar {title:?}"
        );
        assert!(logo.right() <= file.left(), "{what}: before the File menu");
        // First in the bar: after the kit's left padding (room for the
        // window buttons on macOS) and the logo's own 0.25 rem.
        let kit_padding = px(if cfg!(target_os = "macos") { 80. } else { 12. });
        assert_near(
            logo.left() - title.left(),
            kit_padding + rem * 0.25,
            &format!("{what}: at the left end"),
        );
        assert_near(
            logo.size.height,
            logo_height(rem),
            &format!("{what}: height"),
        );
        let aspect = logo.size.width / logo.size.height;
        assert!(
            (aspect - 1.915).abs() < 0.03,
            "{what}: proportions {aspect}"
        );
        assert_near(
            logo.center().y,
            title.center().y,
            &format!("{what}: centred"),
        );
    }

    /// D20: the owner's logo replaces the "xsim" text at the left of the
    /// title bar, with white letters on the dark theme's bar and dark
    /// letters on the light one, decoded from its PNG with a transparent
    /// background.
    #[gpui_kit::test]
    fn the_title_bar_shows_the_logo_for_the_theme(cx: &mut TestAppContext) {
        init(cx);
        let env = TestEnv::new();
        let (_, mut window) = open(cx, &env, None);
        for dark in [true, false, true] {
            if dark {
                use_theme(UseDarkTheme, &mut window);
            } else {
                use_theme(UseLightTheme, &mut window);
            }
            assert_eq!(window.update(|_, cx| cx.theme().is_dark()), dark);
            let (shown, hidden) = if dark {
                ("app-logo-dark", "app-logo-light")
            } else {
                ("app-logo-light", "app-logo-dark")
            };
            assert!(window.debug_bounds(hidden).is_none(), "{hidden} hidden");
            let logo = bounds(shown, &mut window);
            let slot = bounds("app-logo", &mut window);
            assert!(
                logo.left() >= slot.left()
                    && logo.right() <= slot.right()
                    && logo.top() >= slot.top()
                    && logo.bottom() <= slot.bottom(),
                "{shown} in its slot: {logo:?} in {slot:?}"
            );
            assert_logo_in_place(logo, &mut window, shown);

            // GPUI decoded the rendering it paints: the PNG for the
            // logo's height in device pixels, transparent around the
            // drawing.
            let scale = window.update(|window, _| window.scale_factor());
            let chosen = logo_for(dark, f32::from(logo.size.height) * scale);
            let decoded = window
                .update(|window, cx| chosen.image.clone().get_render_image(window, cx))
                .unwrap_or_else(|| panic!("{shown} is decoded"));
            assert_eq!(
                decoded.size(0),
                size(
                    DevicePixels(chosen.width as i32),
                    DevicePixels(chosen.height as i32)
                ),
                "{shown}"
            );
            let bgra = decoded.as_bytes(0).expect("one frame");
            assert_eq!(bgra[3], 0, "{shown}: the corner is transparent");
            let opaque = bgra.chunks(4).filter(|px| px[3] == 255).count();
            assert!(opaque > bgra.len() / 4 / 10, "{shown}: the logo is drawn");
        }
        close(&mut window);
    }

    /// The logo grows with the interface but stays inside the kit's
    /// title bar, which keeps a fixed height, at every scale.
    #[gpui_kit::test]
    fn the_logo_fits_the_title_bar_at_every_scale(cx: &mut TestAppContext) {
        init(cx);
        let env = TestEnv::new();
        let (workspace, mut window) = open(cx, &env, None);
        for scale in SCALE_PRESETS {
            workspace.update_in(&mut window, |w, window, cx| {
                w.set_ui_scale(scale, window, cx)
            });
            window.run_until_parked();
            // The light theme is the default.
            let logo = bounds("app-logo-light", &mut window);
            assert_logo_in_place(logo, &mut window, &format!("{scale}"));
            assert!(logo.size.height <= TITLE_BAR_HEIGHT - px(8.));
        }
        assert_eq!(logo_height(px(16.)), px(24.), "1.5 rem at 100 %");
        assert_eq!(logo_height(px(32.)), TITLE_BAR_HEIGHT - px(8.));
        close(&mut window);
    }

    /// At 100 % the logo is drawn texel for texel: a rendering exactly
    /// its height in device pixels, as wide as its PNG, on whole device
    /// pixels, so the GPU does not resample it.
    #[gpui_kit::test]
    fn at_100_percent_the_logo_is_drawn_texel_for_texel(cx: &mut TestAppContext) {
        init(cx);
        let env = TestEnv::new();
        let (_, mut window) = open(cx, &env, None);
        for (dark, shown) in [(false, "app-logo-light"), (true, "app-logo-dark")] {
            if dark {
                use_theme(UseDarkTheme, &mut window);
            }
            let logo = bounds(shown, &mut window);
            let scale = window.update(|window, _| window.scale_factor());
            let device = |value: Pixels| f32::from(value) * scale;
            let chosen = logo_for(dark, device(logo.size.height));
            assert_eq!(device(logo.size.height), chosen.height as f32, "{shown}");
            assert_eq!(device(logo.size.width), chosen.width as f32, "{shown}");
            for edge in [logo.left(), logo.top()] {
                assert_eq!(device(edge).fract(), 0.0, "{shown}: {logo:?}");
            }
        }
        close(&mut window);
    }
}
