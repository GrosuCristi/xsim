//! The schematic toolbar (Q22, D6): tools, common parts, the component
//! library, transforms, history and view controls. Every button shows its
//! shortcut in the tooltip; buttons that cannot act are disabled, and the
//! editing groups are hidden in read-only mode.

use crate::model::transform::Mirror;
use crate::ui::actions::{self, CONTEXT};
use crate::ui::icons::EditorIcon;
use crate::{SchematicEditor, Tool};
use gpui_kit::{
    component::{
        ActiveTheme, Disableable, Selectable, Sizable,
        button::{Button, ButtonVariants},
    },
    prelude::*,
    *,
};
use std::rc::Rc;
use xsim_project::PartKind;

/// Height of a toolbar row in rems, so it follows the UI scale (Q30). It is
/// the waveform viewer toolbar's height (`HEIGHT` in
/// crates/plot/src/ui/toolbar.rs), so the two toolbars line up side by side
/// (D14). A narrow editor wraps the buttons onto more rows.
pub(crate) const TOOLBAR_HEIGHT: f32 = 2.5;
/// Height of a group separator in rems, as in the waveform viewer toolbar.
const SEPARATOR_HEIGHT: f32 = 1.125;

/// Parts offered directly on the toolbar, in display order (D6).
pub const TOOLBAR_PARTS: [PartKind; 9] = [
    PartKind::VoltageSource,
    PartKind::CurrentSource,
    PartKind::Resistor,
    PartKind::Inductor,
    PartKind::Capacitor,
    PartKind::Diode,
    PartKind::NpnBjt,
    PartKind::PnpBjt,
    PartKind::Ground,
];

type Command = Rc<dyn Fn(&mut SchematicEditor, &mut Window, &mut Context<SchematicEditor>)>;

/// One toolbar button.
struct ToolButton {
    id: SharedString,
    icon: EditorIcon,
    tooltip: SharedString,
    action: Box<dyn Action>,
    selected: bool,
    enabled: bool,
    run: Command,
}

impl ToolButton {
    fn new(
        id: impl Into<SharedString>,
        icon: EditorIcon,
        tooltip: impl Into<SharedString>,
        action: Box<dyn Action>,
        run: impl Fn(&mut SchematicEditor, &mut Window, &mut Context<SchematicEditor>) + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            icon,
            tooltip: tooltip.into(),
            action,
            selected: false,
            enabled: true,
            run: Rc::new(run),
        }
    }

    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    fn render(self, cx: &mut Context<SchematicEditor>) -> AnyElement {
        let run = self.run;
        let selector = format!("toolbar-{}", self.id);
        Button::new(ElementId::Name(self.id))
            .ghost()
            .small()
            .icon(self.icon.icon())
            .selected(self.selected)
            .disabled(!self.enabled)
            .tooltip_with_action(self.tooltip, self.action.as_ref(), Some(CONTEXT))
            .on_click(cx.listener(move |editor, _, window, cx| {
                run(editor, window, cx);
                editor.focus.focus(window, cx);
            }))
            .debug_selector(move || selector)
            .into_any_element()
    }
}

fn separator(cx: &App) -> impl IntoElement + use<> {
    div()
        .w(px(1.0))
        .h(rems(SEPARATOR_HEIGHT))
        .mx_1()
        .flex_shrink_0()
        .bg(cx.theme().border)
}

fn group(buttons: Vec<ToolButton>, cx: &mut Context<SchematicEditor>) -> impl IntoElement + use<> {
    div()
        .flex()
        .items_center()
        .gap_0p5()
        .flex_shrink_0()
        .children(buttons.into_iter().map(|b| b.render(cx)))
}

fn tool_buttons(editor: &SchematicEditor) -> Vec<ToolButton> {
    vec![
        ToolButton::new(
            "select",
            EditorIcon::Select,
            "Select",
            Box::new(actions::SelectTool),
            |e, _, cx| e.set_tool(Tool::Select, cx),
        )
        .selected(editor.tool == Tool::Select),
        ToolButton::new(
            "wire",
            EditorIcon::Wire,
            "Wire",
            Box::new(actions::WireTool),
            |e, _, cx| e.set_tool(Tool::Wire, cx),
        )
        .selected(editor.tool == Tool::Wire),
        ToolButton::new(
            "label",
            EditorIcon::Label,
            "Net label",
            Box::new(actions::LabelTool),
            |e, _, cx| e.set_tool(Tool::Label, cx),
        )
        .selected(editor.tool == Tool::Label),
    ]
}

fn part_buttons(editor: &SchematicEditor) -> Vec<ToolButton> {
    TOOLBAR_PARTS
        .into_iter()
        .filter_map(|kind| {
            let action = actions::place_action(kind)?;
            let id = format!("part-{}", kind.prefix().to_lowercase());
            let id = match kind {
                PartKind::PnpBjt => "part-qp".to_string(),
                _ => id,
            };
            Some(
                ToolButton::new(
                    id,
                    EditorIcon::Part(kind),
                    kind.label(),
                    action,
                    move |e, _, cx| e.set_tool(Tool::Place(kind), cx),
                )
                .selected(editor.tool == Tool::Place(kind)),
            )
        })
        .collect()
}

fn library_button(cx: &mut Context<SchematicEditor>) -> impl IntoElement + use<> {
    Button::new("library")
        .ghost()
        .small()
        // The parts group's spacing, so it does not touch the Ground button.
        .ml_0p5()
        .icon(EditorIcon::Library.icon())
        .label("Library…")
        .tooltip_with_action("Component library", &actions::OpenLibrary, Some(CONTEXT))
        .on_click(cx.listener(|editor, _, window, cx| editor.open_library(window, cx)))
        .debug_selector(|| "toolbar-library".into())
}

fn edit_buttons(editor: &SchematicEditor) -> Vec<ToolButton> {
    let transformable = editor.placement.is_some() || !editor.selection.is_empty();
    vec![
        ToolButton::new(
            "rotate",
            EditorIcon::Rotate,
            "Rotate 90°",
            Box::new(actions::Rotate),
            |e, _, cx| e.rotate(cx),
        )
        .enabled(transformable),
        ToolButton::new(
            "mirror-h",
            EditorIcon::MirrorHorizontal,
            "Mirror horizontally",
            Box::new(actions::MirrorHorizontal),
            |e, _, cx| e.mirror(Mirror::Horizontal, cx),
        )
        .enabled(transformable),
        ToolButton::new(
            "mirror-v",
            EditorIcon::MirrorVertical,
            "Mirror vertically",
            Box::new(actions::MirrorVertical),
            |e, _, cx| e.mirror(Mirror::Vertical, cx),
        )
        .enabled(transformable),
        ToolButton::new(
            "delete",
            EditorIcon::Delete,
            "Delete",
            Box::new(actions::Delete),
            |e, _, cx| e.delete_selection(cx),
        )
        .enabled(!editor.selection.is_empty()),
    ]
}

fn history_buttons(editor: &SchematicEditor) -> Vec<ToolButton> {
    vec![
        ToolButton::new(
            "undo",
            EditorIcon::Undo,
            "Undo",
            Box::new(actions::Undo),
            |e, _, cx| e.undo(cx),
        )
        .enabled(editor.can_undo()),
        ToolButton::new(
            "redo",
            EditorIcon::Redo,
            "Redo",
            Box::new(actions::Redo),
            |e, _, cx| e.redo(cx),
        )
        .enabled(editor.can_redo()),
    ]
}

fn view_buttons() -> Vec<ToolButton> {
    vec![ToolButton::new(
        "fit",
        EditorIcon::ZoomToFit,
        "Zoom to fit",
        Box::new(actions::ZoomToFit),
        |e, _, cx| e.fit_to_view(cx),
    )]
}

fn read_only_badge(cx: &App) -> impl IntoElement + use<> {
    let theme = cx.theme();
    div()
        .debug_selector(|| "read-only-badge".into())
        .px_2()
        .py_0p5()
        .rounded(theme.radius)
        .bg(theme.muted)
        .text_xs()
        .text_color(theme.muted_foreground)
        .child("Read-only")
}

/// The toolbar element.
pub fn toolbar(
    editor: &SchematicEditor,
    cx: &mut Context<SchematicEditor>,
) -> impl IntoElement + use<> {
    let border = cx.theme().border;
    // One row is exactly TOOLBAR_HEIGHT tall (border included), with the
    // viewer toolbar's small ghost buttons and side padding; wrapped rows
    // add their own height. Buttons sit 0.125 rem apart, half the viewer's
    // spacing, so the row still fits the editor's half of the default
    // window when results are shown.
    let bar = div()
        .debug_selector(|| "schematic-toolbar".into())
        .flex()
        .flex_wrap()
        .items_center()
        .content_center()
        .min_h(rems(TOOLBAR_HEIGHT))
        .gap_y_0p5()
        .px_2()
        .py_1()
        .flex_shrink_0()
        .border_b_1()
        .border_color(border);
    if editor.read_only {
        return bar
            .child(read_only_badge(cx))
            .child(separator(cx))
            .child(group(view_buttons(), cx));
    }
    bar.child(group(tool_buttons(editor), cx))
        .child(separator(cx))
        .child(group(part_buttons(editor), cx))
        .child(library_button(cx))
        .child(separator(cx))
        .child(group(edit_buttons(editor), cx))
        .child(separator(cx))
        .child(group(history_buttons(editor), cx))
        .child(separator(cx))
        .child(group(view_buttons(), cx))
}
