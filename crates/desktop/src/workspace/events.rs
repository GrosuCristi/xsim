//! Reactions to the editor and viewer of each circuit.

use super::Workspace;
use crate::ids::TabId;
use crate::prompts::ExportKind;
use crate::simulation::traces::probe_expression;
use gpui_kit::{AppContext, Context};
use xsim_editor::EditorEvent;
use xsim_plot::{ImageFormat, ViewerEvent, ViewerRequest};

impl Workspace {
    /// Mirrors editor changes into the project document.
    pub(super) fn on_editor_event(
        &mut self,
        id: TabId,
        event: &EditorEvent,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.tab_index(id) else {
            return;
        };
        match event {
            EditorEvent::Changed => self.schematic_changed(index, cx),
            EditorEvent::ViewChanged => {
                let view = self.tabs[index].editor.read(cx).view();
                self.doc.project.circuits[index].view = view;
            }
            EditorEvent::Probe(probe) => {
                let expression = probe_expression(probe);
                self.tabs[index]
                    .viewer
                    .update(cx, |viewer, cx| viewer.add_trace(&expression, cx));
            }
        }
    }

    /// Copies plot changes (traces, panes, ranges) into the document; they
    /// are kept across runs (Q136).
    pub(super) fn on_viewer_event(
        &mut self,
        id: TabId,
        event: &ViewerEvent,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.tab_index(id) else {
            return;
        };
        match event {
            ViewerEvent::SetupChanged => {
                let setup = self.tabs[index].viewer.read(cx).setup().clone();
                if self.doc.project.circuits[index].plot != setup {
                    self.doc.project.circuits[index].plot = setup;
                    self.mark_dirty(cx);
                }
            }
        }
    }

    /// The plot toolbar's Export menu: ask for a file in this window and
    /// write it, like File > Export plot. Status messages are already shown
    /// in the viewer's own toolbar.
    pub(super) fn on_viewer_request(
        &mut self,
        id: TabId,
        request: &ViewerRequest,
        cx: &mut Context<Self>,
    ) {
        let kind = match request {
            ViewerRequest::ExportImage(ImageFormat::Png) => ExportKind::Png,
            ViewerRequest::ExportImage(ImageFormat::Jpeg) => ExportKind::Jpeg,
            ViewerRequest::ExportImage(ImageFormat::Svg) => ExportKind::Svg,
            ViewerRequest::ExportCsv => ExportKind::Csv,
            ViewerRequest::Status(_) => return,
        };
        if self.tab_index(id) != Some(self.active_index()) {
            return;
        }
        let workspace = cx.entity();
        let window = self.window;
        cx.defer(move |cx| {
            // The window is gone only while it closes; nothing to export then.
            let _ = cx.update_window(window, |_, window, cx| {
                workspace.update(cx, |this, cx| this.export_plot(kind, window, cx));
            });
        });
    }

    /// An edit, undo or redo: the document follows the editor and the shown
    /// result becomes out of date, so probing it is switched off.
    fn schematic_changed(&mut self, index: usize, cx: &mut Context<Self>) {
        let tab = &mut self.tabs[index];
        tab.revision += 1;
        let schematic = tab.editor.read(cx).schematic().clone();
        if tab.has_result() {
            tab.set_probe_context(None, cx);
        }
        self.doc.project.circuits[index].schematic = schematic;
        self.mark_dirty(cx);
    }
}
