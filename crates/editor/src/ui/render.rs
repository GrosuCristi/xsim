//! Composition of the editor view: toolbar on top, the canvas with its
//! overlays, and the status strip (Q22).

use crate::canvas::palette::Palette;
use crate::canvas::scene::{BoxSelection, Scene};
use crate::canvas::schematic_canvas;
use crate::model::geometry::{Rect, Vec2};
use crate::model::hit_test::BoxMode;
use crate::model::routing::Axis;
use crate::ui::actions::{self, CONTEXT};
use crate::ui::gesture::{Gesture, WireDrag};
use crate::ui::hover::card_element;
use crate::ui::label_editor::label_editor_element;
use crate::ui::status_bar::status_bar;
use crate::ui::toolbar::toolbar;
use crate::{SchematicEditor, Tool};
use gpui_kit::{component::ActiveTheme, prelude::*, *};

/// Offset of the inline label field from its grid point, in pixels.
const FIELD_OFFSET: Vec2 = Vec2::new(12.0, -14.0);

impl SchematicEditor {
    /// Everything the canvas paints this frame.
    fn scene(&mut self, cx: &App) -> Scene {
        let (schematic, connectivity) = match &self.interaction.gesture {
            Gesture::Moving(drag) => (drag.preview.clone(), drag.preview_connectivity.clone()),
            _ => (self.schematic.clone(), self.connectivity()),
        };
        Scene {
            schematic,
            connectivity,
            camera: self.camera(),
            auto_fit: self.auto_fit(),
            selection: self.selection.clone(),
            hovered: self.hover.object,
            net_highlight: self.hover.net.clone(),
            ghost: self.ghost(),
            wire_preview: self.wire_preview(),
            box_selection: self.box_selection(),
            probe_glyph: self.probe_glyph(),
            cursor: self.cursor_style(),
            palette: Palette::from_theme(cx.theme()),
        }
    }

    fn box_selection(&self) -> Option<BoxSelection> {
        match self.interaction.gesture {
            Gesture::BoxSelect { start, current, .. } => Some(BoxSelection {
                rect: Rect::from_corners(start, current),
                mode: BoxMode::for_drag(start, current),
            }),
            _ => None,
        }
    }

    /// Where the probe glyph is drawn: while probing, and while resting on
    /// a net a press would probe (not on a selected wire, which a press
    /// drags).
    pub(crate) fn probe_glyph(&self) -> Option<Vec2> {
        match &self.interaction.gesture {
            Gesture::Probing(drag) => Some(drag.at),
            Gesture::Idle
                if self.hover.net.is_some() && self.wire_drag_under_pointer().is_none() =>
            {
                self.interaction.pointer.map(|p| p.world)
            }
            _ => None,
        }
    }

    /// The mouse cursor, showing what a press would start: a resize arrow
    /// across a wire segment that drags (D14), the crosshair on a net that
    /// probes, and the wire tool's crosshair on a pin that starts a wire
    /// and while that wire is drawn (D20).
    pub(crate) fn cursor_style(&self) -> CursorStyle {
        let (wire_drag, wire_start) = match self.interaction.gesture {
            Gesture::Idle => (
                self.wire_drag_under_pointer(),
                self.wire_start_under_pointer(),
            ),
            _ => (None, None),
        };
        match &self.interaction.gesture {
            Gesture::Panning { .. } => CursorStyle::ClosedHand,
            Gesture::Probing(_) => CursorStyle::Crosshair,
            Gesture::Moving(drag) => drag
                .plan
                .segment_axis()
                .map_or(CursorStyle::Arrow, resize_cursor),
            _ if self.interaction.space_held => CursorStyle::OpenHand,
            _ if self.tool != Tool::Select || self.interaction.wire.is_some() => {
                CursorStyle::Crosshair
            }
            _ if wire_start.is_some() => CursorStyle::Crosshair,
            _ if wire_drag.is_some() => match wire_drag {
                Some(WireDrag::Segment(axis)) => resize_cursor(axis),
                _ => CursorStyle::Arrow,
            },
            _ if self.hover.net.is_some() => CursorStyle::Crosshair,
            _ => CursorStyle::Arrow,
        }
    }

    /// The canvas and the elements floating over it.
    fn canvas_area(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let scene = self.scene(cx);
        let empty = scene.schematic.is_empty() && self.tool == Tool::Select && !self.read_only;
        let canvas = schematic_canvas(scene, cx.weak_entity(), self.viewport.clone());
        let viewport = self.viewport.get();
        let card = self
            .hover
            .card
            .as_ref()
            .zip(viewport)
            .map(|(card, vp)| card_element(card, vp.origin, vp.size, cx));
        let field = self.label_field(cx);
        div()
            .debug_selector(|| "schematic-canvas".into())
            .relative()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .overflow_hidden()
            .bg(cx.theme().background)
            .child(canvas)
            .when(empty, |area| area.child(empty_state(cx)))
            .children(card)
            .children(field)
    }

    fn label_field(&self, cx: &App) -> Option<AnyElement> {
        let field = self.label_editor.as_ref()?;
        let viewport = self.viewport.get()?;
        let anchor = self.label_editor_anchor()?;
        let screen = self.camera().to_screen(&viewport, Vec2::from_grid(anchor));
        let at = screen - viewport.origin + FIELD_OFFSET;
        Some(label_editor_element(field, at, cx))
    }
}

/// The cursor of a segment drag moving along `normal`.
fn resize_cursor(normal: Axis) -> CursorStyle {
    match normal {
        Axis::Vertical => CursorStyle::ResizeUpDown,
        Axis::Horizontal => CursorStyle::ResizeLeftRight,
    }
}

/// What to do on an empty canvas.
fn empty_state(cx: &App) -> impl IntoElement + use<> {
    let theme = cx.theme();
    div()
        .debug_selector(|| "empty-state".into())
        .absolute()
        .inset_0()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_1()
        .child(
            div()
                .text_base()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.foreground)
                .child("Start your circuit"),
        )
        .child(
            div().text_sm().text_color(theme.muted_foreground).child(
                "Pick a part in the toolbar, or open the Library (P), then click to place it",
            ),
        )
}

impl Render for SchematicEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let root = div()
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(theme.background)
            .text_color(theme.foreground)
            .on_key_down(cx.listener(Self::key_down))
            .on_key_up(cx.listener(Self::key_up))
            .on_modifiers_changed(cx.listener(Self::modifiers_changed));
        let canvas = self.canvas_area(cx);
        actions::register(root, cx)
            .child(toolbar(self, cx))
            .child(canvas)
            .child(status_bar(self, cx))
    }
}
