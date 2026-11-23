//! Hover feedback: the object under the mouse, the probed net, and the
//! measurement card shown after resting on a part (Q62).

use crate::SchematicEditor;
use crate::canvas::scene::NetHighlight;
use crate::model::geometry::Vec2;
use crate::model::hit_test::Hit;
use crate::model::measure::{Measurement, MeasurementKind, measure};
use crate::model::selection::ObjectRef;
use crate::ui::gesture::net_point;
use crate::ui::state::Pointer;
use gpui_kit::{component::ActiveTheme, prelude::*, *};
use std::time::Duration;
use xsim_project::{Id, PartKind, Point};
use xsim_spice_engine::units::format_value;

/// How long the mouse must rest on a part before its card appears.
pub const HOVER_DELAY: Duration = Duration::from_millis(300);
/// Card offset from the mouse, in pixels.
const CARD_OFFSET: f32 = 16.0;
/// Approximate card size, used to keep it inside the canvas.
const CARD_SIZE: (f32, f32) = (200.0, 104.0);

/// The measurement card of one part.
#[derive(Debug, Clone, PartialEq)]
pub struct HoverCard {
    pub component: Id,
    pub title: String,
    /// Mouse position in window pixels.
    pub at: Vec2,
    /// `None` when the result has no single values (sweeps).
    pub measurement: Option<Measurement>,
}

/// What the mouse is over.
#[derive(Default)]
pub struct Hover {
    pub object: Option<ObjectRef>,
    pub net: Option<NetHighlight>,
    /// The pin a press would start a wire from (D20). The cursor is
    /// computed live; a change here repaints it when the mouse moves from a
    /// part's body onto its pin within one grid cell.
    pub wire_start: Option<Point>,
    pub card: Option<HoverCard>,
    pending: Option<Id>,
    timer: Option<Task<()>>,
}

impl Hover {
    pub fn clear(&mut self) {
        *self = Hover::default();
    }
}

impl SchematicEditor {
    /// Updates hover feedback for the mouse at `pointer` (`None` outside).
    pub(crate) fn update_hover(&mut self, pointer: Option<Pointer>, cx: &mut Context<Self>) {
        let hit = pointer.and_then(|p| self.hit_at(p.world));
        // A wire started from a pin is drawn like with the wire tool.
        let selecting = self.tool == crate::Tool::Select && self.interaction.wire.is_none();
        let object = hit.filter(|_| selecting).map(Hit::object);
        let net = hit
            .filter(|_| selecting)
            .and_then(|h| self.net_highlight(h));
        let wire_start = pointer.and_then(|p| self.wire_start_at(p.world, hit));
        if object != self.hover.object
            || net != self.hover.net
            || wire_start != self.hover.wire_start
        {
            self.hover.object = object;
            self.hover.net = net;
            self.hover.wire_start = wire_start;
            cx.notify();
        }
        let target = match hit {
            Some(Hit::Component(id)) if self.probe.is_some() && selecting => Some(id),
            _ => None,
        };
        self.update_card(target, pointer, cx);
    }

    fn update_card(
        &mut self,
        target: Option<Id>,
        pointer: Option<Pointer>,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = target else {
            if self.hover.card.take().is_some() {
                cx.notify();
            }
            self.hover.pending = None;
            self.hover.timer = None;
            return;
        };
        if let (Some(card), Some(p)) = (&mut self.hover.card, pointer)
            && card.component == id
        {
            card.at = p.screen;
            cx.notify();
            return;
        }
        if self.hover.pending == Some(id) {
            return;
        }
        self.hover.card = None;
        self.hover.pending = Some(id);
        self.hover.timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(HOVER_DELAY).await;
            this.update(cx, |editor, cx| editor.show_card(id, cx)).ok();
        }));
    }

    fn show_card(&mut self, id: Id, cx: &mut Context<Self>) {
        if self.hover.pending != Some(id) {
            return;
        }
        let (Some(probe), Some(component), Some(pointer)) = (
            &self.probe,
            self.schematic.component(id),
            self.interaction.pointer,
        ) else {
            return;
        };
        if component.part.kind() == PartKind::Ground {
            return;
        }
        let summary = component.part.summary();
        let title = if summary.is_empty() {
            component.name.clone()
        } else {
            format!("{}  ·  {summary}", component.name)
        };
        self.hover.card = Some(HoverCard {
            component: id,
            title,
            at: pointer.screen,
            measurement: measure(component, &probe.netlist, &probe.result),
        });
        cx.notify();
    }

    /// The simulated net under a hit, while a probe context is set.
    fn net_highlight(&self, hit: Hit) -> Option<NetHighlight> {
        let probe = self.probe.as_ref()?;
        let point = net_point(&self.schematic, hit)?;
        let connectivity = &probe.netlist.connectivity;
        let net = connectivity.nets.get(connectivity.net_at(point)?)?;
        let points = net
            .pins
            .iter()
            .filter_map(|&(id, pin)| self.schematic.component(id)?.pins().get(pin).copied())
            .collect();
        Some(NetHighlight {
            wires: net.wires.iter().copied().collect(),
            points,
        })
    }
}

/// The card element, positioned next to the mouse inside the canvas whose
/// top-left corner is `origin` and size `size` (window pixels).
pub fn card_element(card: &HoverCard, origin: Vec2, size: Vec2, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let mut x = card.at.x - origin.x + CARD_OFFSET;
    let mut y = card.at.y - origin.y + CARD_OFFSET;
    if x + CARD_SIZE.0 > size.x {
        x = (card.at.x - origin.x - CARD_OFFSET - CARD_SIZE.0).max(4.0);
    }
    if y + CARD_SIZE.1 > size.y {
        y = (card.at.y - origin.y - CARD_OFFSET - CARD_SIZE.1).max(4.0);
    }
    let body = match &card.measurement {
        Some(m) => measurement_rows(m, cx).into_any_element(),
        None => div()
            .text_color(theme.muted_foreground)
            .child("Values need a transient or operating-point result")
            .into_any_element(),
    };
    div()
        .debug_selector(|| "hover-card".into())
        .absolute()
        .left(px(x))
        .top(px(y))
        .min_w(px(170.0))
        .max_w(px(260.0))
        .flex()
        .flex_col()
        .gap_1()
        .px_3()
        .py_2()
        .rounded(theme.radius)
        .border_1()
        .border_color(theme.border)
        .bg(theme.popover)
        .text_color(theme.popover_foreground)
        .shadow_md()
        .text_xs()
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .text_sm()
                .child(card.title.clone()),
        )
        .child(body)
        .into_any_element()
}

fn measurement_rows(m: &Measurement, cx: &App) -> impl IntoElement + use<> {
    let (rms, avg) = match m.kind {
        MeasurementKind::Transient => (" rms", " avg"),
        MeasurementKind::OperatingPoint => ("", ""),
    };
    div()
        .flex()
        .flex_col()
        .gap_0p5()
        .child(row(
            &format!("{}{rms}", m.voltage_label),
            m.voltage,
            "V",
            cx,
        ))
        .child(row(
            &format!("{}{rms}", m.current_label),
            m.current,
            "A",
            cx,
        ))
        .child(row(&format!("P{avg}"), m.power, "W", cx))
}

fn row(name: &str, value: Option<f64>, unit: &str, cx: &App) -> impl IntoElement + use<> {
    let text = value.map_or_else(|| "n/a".to_string(), |v| format_value(v, unit));
    div()
        .flex()
        .justify_between()
        .gap_4()
        .child(
            div()
                .text_color(cx.theme().muted_foreground)
                .child(name.to_string()),
        )
        .child(
            div()
                .font_family(cx.theme().mono_font_family.clone())
                .child(text),
        )
}
