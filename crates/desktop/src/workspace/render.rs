//! The window layout (Q22) and the action handlers behind every menu item
//! and shortcut.

use super::Workspace;
use crate::actions::*;
use crate::dialogs::analysis::{AnalysisDialogInput, open_analysis_dialog};
use crate::dialogs::settings::open_settings_dialog;
use crate::files::window_title;
use crate::prompts::ExportKind;
use gpui_kit::component::{ActiveTheme, v_flex};
use gpui_kit::{prelude::*, *};
use xsim_project::PartKind;
use xsim_project::appdata::{AppSettings, ThemePreference};

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.reveal_active_tab_once_laid_out(cx);
        window.set_window_title(&window_title(
            &self.doc.project.name,
            self.doc.dirty,
            self.doc.read_only,
        ));
        let root = v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .track_focus(&self.focus)
            .key_context(WORKSPACE_CONTEXT);
        self.on_actions(root, cx)
            .child(self.render_title_bar(window, cx))
            .child(self.render_tab_strip(window, cx))
            .child(self.render_main_area(cx))
            .child(self.render_status_bar(cx))
    }
}

impl Workspace {
    /// Registers the handlers of the [`crate::actions`].
    fn on_actions(&self, root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(cx.listener(|this, _: &NewProject, w, cx| this.new_project(w, cx)))
            .on_action(cx.listener(|this, _: &OpenProject, w, cx| this.open_project(w, cx)))
            .on_action(cx.listener(|this, _: &SaveProject, w, cx| {
                this.save(w, cx);
            }))
            .on_action(cx.listener(|this, _: &SaveProjectAs, w, cx| {
                this.save_as(w, cx);
            }))
            .on_action(cx.listener(|this, _: &CloseProject, w, cx| this.close_project(w, cx)))
            .on_action(cx.listener(|this, _: &ExitApplication, w, cx| this.exit(w, cx)))
            .on_action(
                cx.listener(|this, _: &ExportPng, w, cx| this.export_plot(ExportKind::Png, w, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ExportJpeg, w, cx| {
                    this.export_plot(ExportKind::Jpeg, w, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &ExportSvg, w, cx| this.export_plot(ExportKind::Svg, w, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ExportCsv, w, cx| this.export_plot(ExportKind::Csv, w, cx)),
            )
            .on_action(cx.listener(|this, _: &Undo, _, cx| this.undo(cx)))
            .on_action(cx.listener(|this, _: &Redo, _, cx| this.redo(cx)))
            .on_action(
                cx.listener(|this, _: &Cut, w, cx| this.forward_to_editor(primary!("x"), w, cx)),
            )
            .on_action(
                cx.listener(|this, _: &Copy, w, cx| this.forward_to_editor(primary!("c"), w, cx)),
            )
            .on_action(
                cx.listener(|this, _: &Paste, w, cx| this.forward_to_editor(primary!("v"), w, cx)),
            )
            .on_action(cx.listener(|this, _: &Duplicate, w, cx| {
                this.forward_to_editor(primary!("d"), w, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectAll, w, cx| {
                this.forward_to_editor(primary!("a"), w, cx)
            }))
            .on_action(
                cx.listener(|this, _: &Delete, w, cx| this.forward_to_editor("delete", w, cx)),
            )
            .on_action(cx.listener(|this, _: &UseLightTheme, w, cx| {
                this.set_theme(ThemePreference::Light, w, cx)
            }))
            .on_action(cx.listener(|this, _: &UseDarkTheme, w, cx| {
                this.set_theme(ThemePreference::Dark, w, cx)
            }))
            .on_action(
                cx.listener(|this, _: &ToggleDiagnostics, _, cx| this.toggle_diagnostics(cx)),
            )
            .on_action(cx.listener(|this, _: &ZoomToFit, _, cx| this.zoom_to_fit(cx)))
            .on_action(
                cx.listener(|_, _: &OpenSettings, w, cx| open_settings_dialog(cx.entity(), w, cx)),
            )
            .on_action(cx.listener(|this, _: &RunSimulation, _, cx| this.run_simulation(cx)))
            .on_action(cx.listener(|this, _: &TogglePause, _, cx| this.toggle_pause(cx)))
            .on_action(cx.listener(|this, _: &StopSimulation, _, cx| this.stop_simulation(cx)))
            .on_action(cx.listener(|this, _: &OpenAnalysisSettings, w, cx| {
                open_analysis_dialog(cx.entity(), this.analysis_dialog_input(), w, cx)
            }))
            .on_action(cx.listener(|this, _: &CopyPlot, w, cx| this.copy_plot(w, cx)))
            .on_action(cx.listener(|this, _: &NewCircuit, w, cx| this.add_circuit(w, cx)))
    }

    /// Current application settings.
    pub(crate) fn settings(&self) -> &AppSettings {
        &self.settings
    }

    /// What the analysis dialog edits for the active circuit.
    pub(super) fn analysis_dialog_input(&self) -> AnalysisDialogInput {
        let doc = &self.doc.project.circuits[self.active_index()];
        let sources = doc
            .schematic
            .components
            .iter()
            .filter(|c| {
                matches!(
                    c.part.kind(),
                    PartKind::VoltageSource | PartKind::CurrentSource
                )
            })
            .map(|c| c.name.clone())
            .collect();
        AnalysisDialogInput {
            setup: doc.simulation.clone(),
            sources,
            read_only: self.doc.read_only,
        }
    }
}
