//! The viewer toolbar: add trace, cursors, measurements, the magnifier (box
//! zoom) tool, zoom to fit, copy image and the export menu. Every button has
//! a tooltip with its shortcut where there is one. "Add pane" is the plus
//! at the bottom of the dock instead (D22, `footer`).

use super::actions::{CONTEXT, CopyPlot, ZoomToFit};
use super::trace_input::trace_input;
use crate::model::op_table::is_operating_point;
use crate::{ImageFormat, ViewerRequest, WaveformViewer};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, Disableable, IconName, Selectable, Sizable, h_flex};
use gpui_kit::{prelude::*, *};

/// Height of the toolbar, in rems so it follows the UI scale (Q30).
const HEIGHT: f32 = 2.5;

/// The toolbar row.
pub fn toolbar(
    viewer: &WaveformViewer,
    window: &Window,
    cx: &mut Context<WaveformViewer>,
) -> impl IntoElement {
    let theme = cx.theme().clone();
    let plotting = viewer
        .result
        .as_deref()
        .is_some_and(|r| !is_operating_point(r));
    h_flex()
        .id("waveform-toolbar")
        .debug_selector(|| "waveform-toolbar".into())
        .h(rems(HEIGHT))
        .flex_shrink_0()
        .px_2()
        .gap_1()
        .border_b_1()
        .border_color(theme.border)
        .child(trace_input(viewer, window, cx))
        .child(divider(cx))
        .child(cursors_button(viewer, plotting, cx))
        .child(measurements_button(viewer, plotting, cx))
        .child(magnifier_button(viewer, plotting, cx))
        .child(zoom_button(plotting, cx))
        .child(div().flex_1())
        .when_some(viewer.view.flash.clone(), |el, flash| {
            el.child(
                div()
                    .id("waveform-flash")
                    .debug_selector(|| "waveform-flash".into())
                    .px_2()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(flash),
            )
        })
        .child(copy_button(plotting, cx))
        .child(export_menu(viewer, cx))
}

fn divider(cx: &App) -> impl IntoElement {
    div().mx_1().w(px(1.)).h(rems(1.125)).bg(cx.theme().border)
}

/// An icon button wrapped so tests can find it by `selector`.
fn tool(selector: &'static str, button: impl IntoElement) -> impl IntoElement {
    div().debug_selector(move || selector.into()).child(button)
}

fn cursors_button(
    viewer: &WaveformViewer,
    enabled: bool,
    cx: &mut Context<WaveformViewer>,
) -> impl IntoElement {
    let on = viewer.view.cursors.enabled;
    tool(
        "toggle-cursors",
        Button::new("toggle-cursors")
            .ghost()
            .small()
            .icon(Lucide::Crosshair)
            .selected(on)
            .toggled(on)
            .tooltip(if on { "Hide cursors" } else { "Show cursors" })
            .disabled(!enabled)
            .on_click(cx.listener(move |this, _, _, cx| this.set_cursors_visible(!on, cx))),
    )
}

fn measurements_button(
    viewer: &WaveformViewer,
    enabled: bool,
    cx: &mut Context<WaveformViewer>,
) -> impl IntoElement {
    let open = viewer.view.measurements_open;
    tool(
        "toggle-measurements",
        Button::new("toggle-measurements")
            .ghost()
            .small()
            .icon(Lucide::Sigma)
            .selected(open)
            .toggled(open)
            .tooltip(if open {
                "Hide measurements"
            } else {
                "Show measurements"
            })
            .disabled(!enabled)
            .on_click(cx.listener(move |this, _, _, cx| this.set_measurements_visible(!open, cx))),
    )
}

/// The magnifier: while it is selected a left-drag zooms to the dragged
/// box; otherwise a left-drag pans (D14).
fn magnifier_button(
    viewer: &WaveformViewer,
    enabled: bool,
    cx: &mut Context<WaveformViewer>,
) -> impl IntoElement {
    let on = viewer.view.magnifier;
    tool(
        "toggle-magnifier",
        Button::new("toggle-magnifier")
            .ghost()
            .small()
            .icon(Lucide::ZoomIn)
            .selected(on)
            .toggled(on)
            .tooltip("Zoom to area")
            .disabled(!enabled)
            .on_click(cx.listener(move |this, _, window, cx| {
                // Focus the viewer so Escape reaches it straight away.
                this.focus.focus(window, cx);
                this.set_magnifier(!on, cx);
            })),
    )
}

fn zoom_button(enabled: bool, cx: &mut Context<WaveformViewer>) -> impl IntoElement {
    tool(
        "zoom-to-fit",
        Button::new("zoom-to-fit")
            .ghost()
            .small()
            .icon(Lucide::Scan)
            .tooltip_with_action(
                "Zoom to fit (or double-click the plot)",
                &ZoomToFit,
                Some(CONTEXT),
            )
            .disabled(!enabled)
            .on_click(cx.listener(|this, _, _, cx| this.zoom_to_fit(cx))),
    )
}

fn copy_button(enabled: bool, cx: &mut Context<WaveformViewer>) -> impl IntoElement {
    tool(
        "copy-plot",
        Button::new("copy-plot")
            .ghost()
            .small()
            .icon(IconName::Copy)
            .tooltip_with_action("Copy plot image", &CopyPlot, Some(CONTEXT))
            .disabled(!enabled)
            .on_click(cx.listener(|this, _, _, cx| this.copy_plot(cx))),
    )
}

fn export_menu(viewer: &WaveformViewer, cx: &mut Context<WaveformViewer>) -> impl IntoElement {
    let has_result = viewer.result.is_some();
    let plotting = viewer
        .result
        .as_deref()
        .is_some_and(|r| !is_operating_point(r));
    let entity = cx.entity();
    let request = move |request: ViewerRequest| {
        let entity = entity.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            entity.update(cx, |_, cx| cx.emit(request.clone()));
        }
    };
    tool(
        "export-menu",
        Button::new("export-menu")
            .ghost()
            .small()
            .icon(Lucide::Download)
            .label("Export")
            .dropdown_caret(true)
            .tooltip("Export the plot as an image or the traces as CSV")
            .disabled(!has_result)
            .dropdown_menu(move |menu, _, _| {
                let image = |label: &'static str, format| {
                    PopupMenuItem::new(label)
                        .disabled(!plotting)
                        .on_click(request(ViewerRequest::ExportImage(format)))
                };
                menu.item(PopupMenuItem::label("Image"))
                    .item(image("PNG image…", ImageFormat::Png))
                    .item(image("JPEG image…", ImageFormat::Jpeg))
                    .item(image("SVG image…", ImageFormat::Svg))
                    .item(PopupMenuItem::separator())
                    .item(PopupMenuItem::label("Data"))
                    .item(PopupMenuItem::new("CSV…").on_click(request(ViewerRequest::ExportCsv)))
            }),
    )
}

impl WaveformViewer {
    /// Copies the plot image and confirms it in the toolbar.
    pub(crate) fn copy_plot(&mut self, cx: &mut Context<Self>) {
        let (flash, status) = match self.copy_image_to_clipboard() {
            Ok(()) => (
                "Copied to clipboard".into(),
                "Plot copied to clipboard".to_string(),
            ),
            Err(error) => ("Copy failed".into(), error),
        };
        cx.emit(ViewerRequest::Status(status));
        self.flash(flash, cx);
    }

    /// Shows `text` in the toolbar for two seconds.
    fn flash(&mut self, text: SharedString, cx: &mut Context<Self>) {
        self.view.flash = Some(text);
        self.view.flash_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(2))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.view.flash = None;
                cx.notify();
            });
        }));
        cx.notify();
    }
}
