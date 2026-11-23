//! Modal dialogs in their own operating-system windows (D22).
//!
//! [`open`] shows a dialog in a new window centred over the window it was
//! opened from, its *owner*, and keeps the owner out of reach until the
//! dialog closes. The part properties, the component library, the
//! simulation settings, the settings and the crash recovery all open this
//! way. Menus, popovers, tooltips, toasts and the hover card stay inside
//! their window.
//!
//! ## The window
//!
//! * A title row drawn with the theme (the operating system's title bar
//!   would follow the system's light or dark mode instead): the application
//!   icon (see [`set_title_icon`]), the dialog's title, which is also the
//!   window's title for the operating system, and a close button, which is
//!   a window control and no tab stop. Dragging the row moves the window.
//!   There is no minimize or maximize button.
//! * The content and the footer below it, with the kit dialog's 16 px
//!   padding. The width follows the UI scale (it is given in rems). A dialog
//!   that fits its content measures it after each layout and resizes its
//!   window to match, keeping the top left corner where it is (GPUI cannot
//!   move a window once it is open) and the window on the screen. So its
//!   first height is an estimate, which the caller gives for the variant of
//!   its content (see [`DialogOptions::new`]'s `id`), and later openings of
//!   the same variant replace it with the measured one. When the content is
//!   taller than the screen allows, it scrolls, footer included.
//!   A resizable dialog (the library) starts at a given size and its content
//!   fills the window.
//! * Nothing animates (D19); only the operating system may animate the
//!   window as it appears.
//!
//! ## Modality
//!
//! Two layers:
//!
//! * **The operating system's.** The window is a [`WindowKind::Dialog`]. On
//!   Windows GPUI makes the *active* window its owner and disables that
//!   window until the dialog closes, so the dialog stays in front of it and
//!   a click on the owner brings the dialog back. On macOS it is a sheet of
//!   the main window, on X11 and Wayland a modal transient window. A dialog
//!   opened while xsim is in the background has no owner at this level.
//! * **xsim's own**, which never depends on the first. While a window owns
//!   an open dialog it is *blocked*: a layer over its whole content swallows
//!   presses and the wheel, and a press brings the dialog to the front; its
//!   keystrokes are dropped before any shortcut, key listener or text input
//!   sees them; and when it becomes the active window the dialog is
//!   activated instead. The layer is a Base root plugin, so it exists in
//!   every window opened after [`init`].
//!
//! A window owns at most one dialog: opening another from a blocked window
//! activates the open one instead. A dialog may own a dialog in turn (the
//! model library over the part properties); the chain closes from the top,
//! and the window below gets the keyboard back. Closing a window closes the
//! dialogs it owns.
//!
//! ## Keys
//!
//! The content sits in the kit's `Dialog` key context, so Escape closes the
//! dialog without applying anything and Enter that no focused control takes
//! calls the [`DialogContent::on_ok`] handler, which says whether to close
//! (no handler closes). The window's close button, Alt+F4 and the
//! operating system's close close it like Escape. When the focused control
//! leaves the content, the closest container still shown takes the
//! keyboard, so Escape and Enter keep working.

use gpui_kit::base::{Root, RootPlugin};
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, TITLE_BAR_HEIGHT,
    dialog::{Cancel, Confirm},
    h_flex, v_flex,
};
use gpui_kit::{
    AnyElement, AnyWindowHandle, App, AppContext as _, BorrowAppContext as _, Bounds, Context,
    Decorations, FocusHandle, Global, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement, Pixels, Render, ScrollHandle, SharedString, Size,
    StatefulInteractiveElement as _, Styled as _, Subscription, TitlebarOptions, Window,
    WindowBounds, WindowControlArea, WindowId, WindowKind, WindowOptions, canvas, div, point,
    prelude::FluentBuilder as _, px, size,
};
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

/// The kit's padding around a dialog's content.
pub const PADDING: Pixels = px(16.);

/// Key context of the dialog content: the kit's, whose bindings turn
/// Escape into [`Cancel`] and Enter into [`Confirm`].
const DIALOG_CONTEXT: &str = "Dialog";

/// Room kept between a dialog and the edges of the screen.
const SCREEN_MARGIN: Pixels = px(16.);

/// Height of the title row: the kit's title bar at 100% UI scale, growing
/// with the scale beyond it.
fn title_bar_height(rem: Pixels) -> Pixels {
    TITLE_BAR_HEIGHT.max(rem * 2.125)
}

/// Installs the modal layer in windows opened from now on, and the
/// keyboard and window-closing handling for every window. Call it once
/// before opening the application's windows; later calls do nothing.
/// [`open`] calls it too, which covers everything but the layer of windows
/// already open.
pub fn init(cx: &mut App) {
    if cx.has_global::<Dialogs>() {
        return;
    }
    cx.set_global(Dialogs::default());
    Root::register_plugin::<ModalShield>(cx, ModalShield::new);
    cx.intercept_keystrokes(|_, window, cx| {
        let handle = window.window_handle();
        if is_blocked(handle, cx) {
            cx.stop_propagation();
            bring_to_front(handle, cx);
        }
    })
    .detach();
    cx.on_window_closed(window_closed).detach();
}

/// Builds the title-row icon at a given row height (D20: the application
/// logo). The shell sets it; without one the row shows the title alone.
pub type TitleIcon = Rc<dyn Fn(Pixels, &mut Window, &mut App) -> AnyElement>;

struct TitleIconSlot(TitleIcon);
impl Global for TitleIconSlot {}

/// Sets the icon at the left of every dialog's title row.
pub fn set_title_icon(
    cx: &mut App,
    icon: impl Fn(Pixels, &mut Window, &mut App) -> AnyElement + 'static,
) {
    cx.set_global(TitleIconSlot(Rc::new(icon)));
}

/// Width of a dialog window.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Width {
    /// The whole window, in rems.
    Window(f32),
    /// The content, in rems; the window adds the padding on both sides.
    Content(f32),
}

impl Width {
    fn resolve(self, rem: Pixels) -> Pixels {
        match self {
            Self::Window(rems) => rem * rems,
            Self::Content(rems) => rem * rems + PADDING * 2.,
        }
    }
}

/// How a dialog window gets its height.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Height {
    /// Fits the content; the window opens `estimate` rems high until the
    /// content has been measured once.
    Fit { estimate: f32 },
    /// Starts `height` rems high and can be resized down to the minimum
    /// size, in rems; the content fills the window.
    Resizable {
        height: f32,
        min_width: f32,
        min_height: f32,
    },
}

/// What kind of dialog window to open.
#[derive(Debug, Clone)]
pub struct DialogOptions {
    id: SharedString,
    title: SharedString,
    width: Width,
    height: Height,
}

impl DialogOptions {
    /// A dialog `width` rems wide (the whole window) whose height fits its
    /// content. `id` names the kind of dialog, so a later opening can start
    /// at the height measured this time: give content of different heights
    /// (another waveform, another analysis) ids of their own. `title` is the
    /// window's title until the content sets one.
    pub fn new(id: impl Into<SharedString>, title: impl Into<SharedString>, width: f32) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            width: Width::Window(width),
            height: Height::Fit { estimate: 20. },
        }
    }

    /// Makes the content `rems` wide, the window adding its padding.
    pub fn content_width(mut self, rems: f32) -> Self {
        self.width = Width::Content(rems);
        self
    }

    /// The height, in rems, to open at before the content has been measured.
    pub fn estimated_height(mut self, rems: f32) -> Self {
        if let Height::Fit { estimate } = &mut self.height {
            *estimate = rems;
        }
        self
    }

    /// Makes the window resizable: it opens `height` rems high, its content
    /// fills it, and it can shrink to `min_width` × `min_height` rems.
    pub fn resizable(mut self, height: f32, min_width: f32, min_height: f32) -> Self {
        self.height = Height::Resizable {
            height,
            min_width,
            min_height,
        };
        self
    }
}

/// Handles Enter that no control took; returns whether the dialog closes.
type OkHandler = Rc<dyn Fn(&mut Window, &mut App) -> bool>;

/// One frame of a dialog's content, built by the dialog's render function:
/// a title, the content and an optional footer. Like the kit's dialog
/// builder.
#[derive(Default)]
pub struct DialogContent {
    title: Option<SharedString>,
    children: Vec<AnyElement>,
    footer: Option<AnyElement>,
    on_ok: Option<OkHandler>,
}

impl DialogContent {
    /// The title shown in the title row and given to the operating system.
    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// The row of buttons under the content.
    pub fn footer(mut self, footer: impl IntoElement) -> Self {
        self.footer = Some(footer.into_any_element());
        self
    }

    /// Handles Enter that no focused control takes; returns whether the
    /// dialog closes. Without a handler, Enter closes it.
    pub fn on_ok(mut self, handler: impl Fn(&mut Window, &mut App) -> bool + 'static) -> Self {
        self.on_ok = Some(Rc::new(handler));
        self
    }
}

impl ParentElement for DialogContent {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

/// Builds each frame of a dialog's content.
type ContentFn = Box<dyn Fn(DialogContent, &mut Window, &mut App) -> DialogContent>;

/// Opens a modal dialog over `owner`, the window it is opened from, once
/// the current update is over, so it may be called while the views it
/// shows are being updated (from their action handlers, say). Does nothing
/// but bring the open dialog to the front when `owner` already owns one
/// by then; [`dialog_of`] finds the new window.
///
/// `build` runs once in the new window and returns the function that
/// builds each frame of the content. Create the dialog's views there, with
/// the window it is given: text inputs follow the focus of the window they
/// were created in. Focus the control that should have the keyboard there
/// too; otherwise the dialog itself takes it.
pub fn open<R>(
    owner: &mut Window,
    cx: &mut App,
    options: DialogOptions,
    build: impl FnOnce(&mut Window, &mut App) -> R + 'static,
) where
    R: Fn(DialogContent, &mut Window, &mut App) -> DialogContent + 'static,
{
    init(cx);
    let owner = owner.window_handle();
    cx.defer(move |cx| {
        owner
            .update(cx, |_, owner, cx| open_now(owner, cx, options, build))
            .ok();
    });
}

/// [`open`], now; returns the dialog's window.
fn open_now<R>(
    owner: &mut Window,
    cx: &mut App,
    options: DialogOptions,
    build: impl FnOnce(&mut Window, &mut App) -> R,
) -> Option<AnyWindowHandle>
where
    R: Fn(DialogContent, &mut Window, &mut App) -> DialogContent + 'static,
{
    let owner_handle = owner.window_handle();
    if is_blocked(owner_handle, cx) {
        bring_to_front(owner_handle, cx);
        return None;
    }
    let rem = cx.theme().font_size;
    let owner_bounds = owner.bounds();
    let display = owner.display(cx);
    let screen = display.as_ref().map(|display| display.visible_bounds());
    let geometry = Geometry::new(&options, rem, owner_bounds, screen, cx);
    let window_options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(geometry.bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some(options.title.clone()),
            appears_transparent: true,
            traffic_light_position: None,
        }),
        kind: WindowKind::Dialog,
        is_resizable: geometry.min_size.is_some(),
        is_minimizable: false,
        window_min_size: geometry.min_size,
        display_id: display.map(|display| display.id()),
        app_owns_titlebar_drag: true,
        ..WindowOptions::default()
    };
    let limits = geometry.limits;
    let opened = gpui_kit::open_window(window_options, cx, move |window, cx| {
        let content = build(window, cx);
        let view = cx.new(|cx| DialogWindow::new(options, limits, Box::new(content), window, cx));
        if window.focused(cx).is_none() {
            view.read(cx).focus.clone().focus(window, cx);
        }
        // The operating system's close (the close control on Windows,
        // Alt+F4): close like Escape, once the platform's call is over.
        window.on_window_should_close(cx, |window, cx| {
            window.spawn(cx, async |cx| cx.update(close).ok()).detach();
            false
        });
        view
    });
    let (handle, _) = opened.ok()?;
    cx.update_global::<Dialogs, _>(|dialogs, _| {
        dialogs.open.push(OpenDialog {
            window: handle,
            owner: owner_handle,
        })
    });
    Some(handle)
}

/// Closes the dialog in `window` without applying anything and gives the
/// keyboard back to its owner; closes `window` whatever it is.
pub fn close(window: &mut Window, cx: &mut App) {
    let id = window.window_handle().window_id();
    let owner = cx.update_global::<Dialogs, _>(|dialogs, _| dialogs.remove(id));
    window.remove_window();
    if let Some(owner) = owner {
        activate_later(owner, cx);
    }
}

/// The window that owns the dialog in `window`, if it is one.
pub fn owner_of(window: AnyWindowHandle, cx: &App) -> Option<AnyWindowHandle> {
    cx.try_global::<Dialogs>()?
        .open
        .iter()
        .find(|dialog| dialog.window == window)
        .map(|dialog| dialog.owner)
}

/// The dialog `owner` owns, if one is open.
pub fn dialog_of(owner: AnyWindowHandle, cx: &App) -> Option<AnyWindowHandle> {
    cx.try_global::<Dialogs>()?
        .open
        .iter()
        .find(|dialog| dialog.owner == owner)
        .map(|dialog| dialog.window)
}

/// The dialog at the top of the chain `window` owns, or `window` itself
/// when it owns none: the window that has the keyboard.
pub fn topmost(window: AnyWindowHandle, cx: &App) -> AnyWindowHandle {
    let mut top = window;
    let depth = cx.try_global::<Dialogs>().map_or(0, |d| d.open.len());
    for _ in 0..depth {
        match dialog_of(top, cx) {
            Some(dialog) => top = dialog,
            None => break,
        }
    }
    top
}

/// Whether `window` owns an open dialog and so ignores input.
pub fn is_blocked(window: AnyWindowHandle, cx: &App) -> bool {
    dialog_of(window, cx).is_some()
}

/// Runs `f` in the window that owns the dialog in `window` (or in `window`
/// when it is no dialog) once the current update is over. Dialog buttons
/// use it for work that belongs to the main window, such as toasts or
/// focusing the editor. Call it before [`close`], which forgets the owner.
pub fn with_owner(window: &Window, cx: &mut App, f: impl FnOnce(&mut Window, &mut App) + 'static) {
    let handle = window.window_handle();
    let target = owner_of(handle, cx).unwrap_or(handle);
    cx.defer(move |cx| {
        target.update(cx, |_, window, cx| f(window, cx)).ok();
    });
}

/// Gives the keyboard to the dialog in `window` itself, as when it opened
/// without a control to focus: Tab then starts from its first control.
pub fn focus_dialog(window: &mut Window, cx: &mut App) {
    let dialog = window
        .root::<Root>()
        .flatten()
        .and_then(|root| root.read(cx).view().clone().downcast::<DialogWindow>().ok());
    if let Some(dialog) = dialog {
        let focus = dialog.read(cx).focus.clone();
        focus.focus(window, cx);
    }
}

/// Activates the dialog at the top of `window`'s chain, if `window` owns
/// one.
fn bring_to_front(window: AnyWindowHandle, cx: &mut App) {
    let top = topmost(window, cx);
    if top != window {
        activate_later(top, cx);
    }
}

fn activate_later(window: AnyWindowHandle, cx: &mut App) {
    cx.defer(move |cx| {
        window
            .update(cx, |_, window, _| window.activate_window())
            .ok();
    });
}

/// A closed window: forget it as a dialog, and close the dialogs it owned.
fn window_closed(cx: &mut App, id: WindowId) {
    if !cx.has_global::<Dialogs>() {
        return;
    }
    if let Some(owner) = cx.update_global::<Dialogs, _>(|dialogs, _| dialogs.remove(id)) {
        activate_later(owner, cx);
    }
    let owned: Vec<AnyWindowHandle> = cx
        .global::<Dialogs>()
        .open
        .iter()
        .filter(|dialog| dialog.owner.window_id() == id)
        .map(|dialog| dialog.window)
        .collect();
    for dialog in owned {
        cx.defer(move |cx| {
            dialog.update(cx, |_, window, cx| close(window, cx)).ok();
        });
    }
}

/// An open dialog and the window it blocks.
#[derive(Debug, Clone, Copy)]
struct OpenDialog {
    window: AnyWindowHandle,
    owner: AnyWindowHandle,
}

/// The open dialogs. Every window's modal layer observes it.
#[derive(Default)]
struct Dialogs {
    open: Vec<OpenDialog>,
}

impl Global for Dialogs {}

/// Height in rems that each kind of dialog fitted as it opened last time,
/// so the next one opens at it. Apart from [`Dialogs`], so that storing one
/// redraws no window.
#[derive(Default)]
struct FittedHeights(HashMap<SharedString, f32>);

impl Global for FittedHeights {}

impl Dialogs {
    /// Forgets the dialog in window `id`; returns its owner.
    fn remove(&mut self, id: WindowId) -> Option<AnyWindowHandle> {
        let index = self.open.iter().position(|d| d.window.window_id() == id)?;
        Some(self.open.remove(index).owner)
    }
}

/// Where a dialog window opens.
struct Geometry {
    bounds: Bounds<Pixels>,
    min_size: Option<Size<Pixels>>,
    limits: Limits,
}

/// How far right and down a dialog window's edges may go as its content
/// grows: to just inside the screen's.
#[derive(Debug, Clone, Copy)]
struct Limits {
    right: Pixels,
    bottom: Pixels,
}

impl Geometry {
    fn new(
        options: &DialogOptions,
        rem: Pixels,
        owner: Bounds<Pixels>,
        screen: Option<Bounds<Pixels>>,
        cx: &App,
    ) -> Self {
        let area = screen.unwrap_or(owner);
        let max_width = (area.size.width - SCREEN_MARGIN * 2.).max(rem * 10.);
        let max_height = (area.size.height - SCREEN_MARGIN * 2.).max(rem * 10.);
        let width = options.width.resolve(rem).min(max_width);
        let (height, min_size) = match options.height {
            Height::Fit { estimate } => {
                let rems = cx
                    .try_global::<FittedHeights>()
                    .and_then(|heights| heights.0.get(&options.id).copied())
                    .unwrap_or(estimate);
                (rem * rems, None)
            }
            Height::Resizable {
                height,
                min_width,
                min_height,
            } => (
                rem * height,
                Some(size(
                    (rem * min_width).min(width),
                    (rem * min_height).min(max_height),
                )),
            ),
        };
        let size = size(width, height.min(max_height));
        Self {
            bounds: centred(owner, size, area),
            min_size,
            limits: Limits {
                right: area.right() - SCREEN_MARGIN,
                bottom: area.bottom() - SCREEN_MARGIN,
            },
        }
    }
}

/// Bounds of `size` centred over `owner`, moved inside `area` (the screen),
/// [`SCREEN_MARGIN`] from its edges, where they would leave it: the room
/// the fit to the content keeps (see [`Limits`]).
fn centred(owner: Bounds<Pixels>, size: Size<Pixels>, area: Bounds<Pixels>) -> Bounds<Pixels> {
    let center = owner.center();
    let mut x = center.x - size.width / 2.;
    let mut y = center.y - size.height / 2.;
    x = x
        .min(area.right() - SCREEN_MARGIN - size.width)
        .max(area.left() + SCREEN_MARGIN);
    y = y
        .min(area.bottom() - SCREEN_MARGIN - size.height)
        .max(area.top() + SCREEN_MARGIN);
    Bounds {
        origin: point(x, y),
        size,
    }
}

/// The root view of a dialog window: title row, content and footer.
struct DialogWindow {
    id: SharedString,
    title: SharedString,
    /// The title last given to the operating system.
    shown_title: SharedString,
    width: Width,
    height: Height,
    content: ContentFn,
    focus: FocusHandle,
    scroll: ScrollHandle,
    /// Natural height of the content and footer, measured on the last
    /// layout.
    measured: Rc<Cell<Option<Pixels>>>,
    /// The window size last asked for, so a size the platform rounds is not
    /// asked for again and again.
    requested: Option<Size<Pixels>>,
    /// The UI scale the size was last fitted at.
    fitted_rem: Option<Pixels>,
    /// Whether the height of the content as it opened is remembered for
    /// the next dialog of its kind.
    remembered: bool,
    limits: Limits,
    /// A press on the title row that may become a window move.
    should_move: bool,
    _focus_lost: Subscription,
}

impl DialogWindow {
    fn new(
        options: DialogOptions,
        limits: Limits,
        content: ContentFn,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // When the focused control leaves the content (a field of an
        // analysis or waveform type no longer shown, say), nothing would
        // have the keyboard and Escape and Enter would do nothing: the
        // closest container still shown takes it, else the dialog itself.
        let focus_lost = cx.on_focus_lost(window, |this: &mut Self, window, cx| {
            let target = window
                .focus_lost_restore_target(cx)
                .unwrap_or_else(|| this.focus.clone());
            target.focus(window, cx);
        });
        Self {
            id: options.id,
            shown_title: SharedString::default(),
            title: options.title,
            width: options.width,
            height: options.height,
            content,
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            measured: Rc::default(),
            requested: None,
            fitted_rem: None,
            remembered: false,
            limits,
            should_move: false,
            _focus_lost: focus_lost,
        }
    }

    fn fits_content(&self) -> bool {
        matches!(self.height, Height::Fit { .. })
    }

    /// Resizes the window to the measured content at the current UI scale,
    /// within the screen: the top left corner stays, so the window gets no
    /// wider or taller than the room left to the right and below.
    fn fit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.fits_content() {
            return;
        }
        let Some(content) = self.measured.get() else {
            return;
        };
        let rem = window.rem_size();
        let bar = if self.draws_title_bar(window) {
            title_bar_height(rem)
        } else {
            px(0.)
        };
        let min_height = bar + rem * 3.;
        let origin = window.bounds().origin;
        let height = (bar + content)
            .min(self.limits.bottom - origin.y)
            .max(min_height);
        let width = self
            .width
            .resolve(rem)
            .min(self.limits.right - origin.x)
            .max(rem * 10.);
        let wanted = size(width, height);
        if self.requested == Some(wanted) {
            return;
        }
        self.requested = Some(wanted);
        if !self.remembered {
            self.remembered = true;
            cx.default_global::<FittedHeights>()
                .0
                .insert(self.id.clone(), height / rem);
        }
        window.resize(wanted);
        // Headless windows report the new size only when told.
        window.bounds_changed(cx);
    }

    /// Whether xsim draws the title row: everywhere but where a Linux
    /// window manager decorates the window itself.
    fn draws_title_bar(&self, window: &Window) -> bool {
        !(cfg!(target_os = "linux") && matches!(window.window_decorations(), Decorations::Server))
    }

    fn title_bar(
        &self,
        title: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let height = title_bar_height(window.rem_size());
        let icon = cx
            .try_global::<TitleIconSlot>()
            .map(|slot| slot.0.clone())
            .map(|icon| icon(height, window, cx));
        let theme = cx.theme();
        h_flex()
            .id("dialog-title-bar")
            .debug_selector(|| "dialog-title-bar".into())
            .flex_shrink_0()
            .w_full()
            .h(height)
            .bg(theme.title_bar)
            .border_b_1()
            .border_color(theme.title_bar_border)
            .child(
                h_flex()
                    .id("dialog-drag-area")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .pl_3()
                    .gap_2()
                    .window_control_area(WindowControlArea::Drag)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, _| this.should_move = true),
                    )
                    .on_mouse_down_out(cx.listener(|this, _, _, _| this.should_move = false))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, _, _| this.should_move = false),
                    )
                    .on_mouse_move(cx.listener(|this, _, window, _| {
                        if this.should_move {
                            this.should_move = false;
                            window.start_window_move();
                        }
                    }))
                    .children(icon)
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .text_color(theme.foreground)
                            .debug_selector(|| "dialog-title".into())
                            .child(title),
                    ),
            )
            .child(close_button(height, cx))
            .into_any_element()
    }

    fn body(&self, content: DialogContent, cx: &mut Context<Self>) -> AnyElement {
        let DialogContent {
            children, footer, ..
        } = content;
        let fill = !self.fits_content();
        let surface = v_flex()
            .debug_selector(|| "dialog-0".into())
            .w_full()
            .py(PADDING)
            .gap(PADDING)
            .child(
                v_flex()
                    .w_full()
                    .px(PADDING)
                    .when(fill, |this| this.flex_1().min_h_0())
                    .children(children),
            )
            .when_some(footer, |this, footer| {
                this.child(div().flex_shrink_0().w_full().px(PADDING).child(footer))
            });
        if fill {
            return surface.flex_1().min_h_0().into_any_element();
        }
        let measured = self.measured.clone();
        let view = cx.weak_entity();
        let measure = canvas(
            move |bounds, window, cx| {
                if measured.get() != Some(bounds.size.height) {
                    measured.set(Some(bounds.size.height));
                    window.defer(cx, move |window, cx| {
                        view.update(cx, |this, cx| this.fit(window, cx)).ok();
                    });
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0();
        div()
            .id("dialog-scroll")
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .child(surface.relative().flex_shrink_0().child(measure))
            .into_any_element()
    }
}

/// The title row's close button: closes like Escape. On Windows it is also
/// the window's close control for the operating system.
fn close_button(height: Pixels, cx: &mut Context<DialogWindow>) -> impl IntoElement {
    let theme = cx.theme();
    let (hover_bg, hover_fg, active_bg) =
        (theme.danger, theme.danger_foreground, theme.danger_active);
    div()
        .id("dialog-close")
        .debug_selector(|| "dialog-close".into())
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .w(height)
        .h_full()
        .text_color(theme.foreground)
        .hover(move |style| style.bg(hover_bg).text_color(hover_fg))
        .active(move |style| style.bg(active_bg).text_color(hover_fg))
        .when(cfg!(target_os = "windows"), |this| {
            this.window_control_area(WindowControlArea::Close)
        })
        .on_click(|_, window, cx| {
            // Handled here, so Windows does not close the window again.
            cx.stop_propagation();
            close(window, cx);
        })
        .child(Icon::new(IconName::WindowClose).small())
}

impl Render for DialogWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = (self.content)(DialogContent::default(), window, cx);
        let title = content.title.clone().unwrap_or_else(|| self.title.clone());
        if title != self.shown_title {
            window.set_window_title(&title);
            self.shown_title = title.clone();
        }
        let rem = window.rem_size();
        if self.fitted_rem != Some(rem) {
            self.fitted_rem = Some(rem);
            let view = cx.weak_entity();
            window.defer(cx, move |window, cx| {
                view.update(cx, |this, cx| this.fit(window, cx)).ok();
            });
        }
        let on_ok = content.on_ok.clone();
        let title_bar = self
            .draws_title_bar(window)
            .then(|| self.title_bar(title, window, cx));
        let theme = cx.theme();
        v_flex()
            .id("dialog-window")
            .key_context(DIALOG_CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .overflow_hidden()
            .bg(theme.background)
            .text_color(theme.foreground)
            .on_action(|_: &Cancel, window, cx| close(window, cx))
            .on_action(move |_: &Confirm, window, cx| {
                if on_ok.as_ref().is_none_or(|ok| ok(window, cx)) {
                    close(window, cx);
                }
            })
            .children(title_bar)
            .child(self.body(content, cx))
    }
}

/// The modal layer of a window: while the window owns a dialog, covers it
/// and takes its input. A Base root plugin, so every window has one.
///
/// A key that closes a dialog on its press (Enter, Escape) is released in
/// the owner. GPUI clicks a focused element only on a release whose press it
/// saw, so that release presses nothing there.
struct ModalShield {
    _activation: Subscription,
    _dialogs: Subscription,
}

impl ModalShield {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            // Brings the dialog back when its owner is activated, for
            // platforms that leave the owner reachable.
            _activation: cx.observe_window_activation(window, |_, window, cx| {
                if window.is_window_active() {
                    bring_to_front(window.window_handle(), cx);
                }
            }),
            _dialogs: cx.observe_global::<Dialogs>(|_, cx| cx.notify()),
        }
    }
}

impl Render for ModalShield {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !is_blocked(window.window_handle(), cx) {
            return div().into_any_element();
        }
        div()
            .id("modal-shield")
            .debug_selector(|| "modal-shield".into())
            .absolute()
            .inset_0()
            .occlude()
            .on_any_mouse_down(|_, window, cx| {
                cx.stop_propagation();
                bring_to_front(window.window_handle(), cx);
            })
            .on_scroll(|_, _, cx| cx.stop_propagation())
            .into_any_element()
    }
}

impl RootPlugin for ModalShield {
    /// Drops the key releases of a blocked window (and any press the
    /// keystroke interceptor let through) before a control sees them.
    fn decorate(
        &self,
        surface: AnyElement,
        _: &Root,
        _: &mut Window,
        _: &mut App,
    ) -> impl IntoElement {
        div()
            .size_full()
            .capture_key_down(|_, window, cx| {
                if is_blocked(window.window_handle(), cx) {
                    cx.stop_propagation();
                }
            })
            .capture_key_up(|_, window, cx| {
                if is_blocked(window.window_handle(), cx) {
                    cx.stop_propagation();
                }
            })
            .child(surface)
    }
}

#[cfg(all(test, debug_assertions))]
mod tests {
    use super::*;
    use gpui_kit::{
        Entity, KeyDownEvent, KeyUpEvent, Keystroke, Modifiers, MouseDownEvent, MouseUpEvent,
        ScrollDelta, ScrollEvent, TestAppContext, VisualContext as _, VisualTestContext, rems,
    };

    /// Window content that counts the input it gets.
    struct Owner {
        focus: FocusHandle,
        clicks: usize,
        keys: usize,
        /// Presses of any button, and turns of the wheel.
        presses: usize,
        scrolls: usize,
    }

    impl Render for Owner {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .id("owner")
                .debug_selector(|| "owner".into())
                .size_full()
                .track_focus(&self.focus)
                .on_click(cx.listener(|this, _, _, _| this.clicks += 1))
                .on_any_mouse_down(cx.listener(|this, _, _, _| this.presses += 1))
                .on_scroll(cx.listener(|this, _, _, _| this.scrolls += 1))
                .on_key_down(cx.listener(|this, _, _, _| this.keys += 1))
                .on_key_up(cx.listener(|this, _, _, _| this.keys += 1))
        }
    }

    const OWNER: Bounds<Pixels> = Bounds {
        origin: gpui_kit::Point {
            x: px(100.),
            y: px(50.),
        },
        size: Size {
            width: px(1000.),
            height: px(800.),
        },
    };

    fn owner_window(cx: &mut TestAppContext) -> (Entity<Owner>, VisualTestContext) {
        let (window, owner) = cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_reduce_motion(true);
            init(cx);
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(OWNER)),
                    ..WindowOptions::default()
                },
                cx,
                |window, cx| {
                    let owner = cx.new(|cx| Owner {
                        focus: cx.focus_handle(),
                        clicks: 0,
                        keys: 0,
                        presses: 0,
                        scrolls: 0,
                    });
                    owner.read(cx).focus.clone().focus(window, cx);
                    owner
                },
            )
            .expect("test window")
        });
        let visual = VisualTestContext::from_window(window, cx);
        visual.run_until_parked();
        (owner, visual)
    }

    /// How often Enter reached a test dialog.
    type Oks = Rc<Cell<usize>>;

    /// Opens a dialog of 12 rem × 200 px content over `cx`'s window, whose
    /// Enter handler counts and closes when `close_on_ok`.
    fn open_test_dialog(
        cx: &mut VisualTestContext,
        close_on_ok: bool,
    ) -> (Option<AnyWindowHandle>, Oks) {
        let oks = Oks::default();
        let counter = oks.clone();
        cx.update(|window, cx| {
            open(
                window,
                cx,
                DialogOptions::new("test", "Test dialog", 0.)
                    .content_width(12.)
                    .estimated_height(10.),
                move |_, _| {
                    move |content: DialogContent, _: &mut Window, _: &mut App| {
                        let counter = counter.clone();
                        content
                            .title("Test dialog")
                            .child(div().h(px(200.)).debug_selector(|| "test-body".into()))
                            .footer(div().h(rems(2.)).debug_selector(|| "test-footer".into()))
                            .on_ok(move |_, _| {
                                counter.set(counter.get() + 1);
                                close_on_ok
                            })
                    }
                },
            )
        });
        cx.run_until_parked();
        let owner = cx.window_handle();
        let dialog = read(cx, |cx| dialog_of(owner, cx));
        (dialog, oks)
    }

    fn bounds_of(window: AnyWindowHandle, cx: &mut VisualTestContext) -> Bounds<Pixels> {
        cx.cx
            .update(|cx| window.update(cx, |_, window, _| window.bounds()))
            .expect("an open window")
    }

    /// Reads the application state.
    fn read<R>(cx: &VisualTestContext, f: impl FnOnce(&App) -> R) -> R {
        cx.cx.read(f)
    }

    fn is_open(window: AnyWindowHandle, cx: &VisualTestContext) -> bool {
        cx.windows().contains(&window)
    }

    /// Presses and releases `key` in `cx`'s window; when the press closed
    /// that window, the release goes nowhere.
    fn press(key: &'static str, cx: &mut VisualTestContext) {
        let keystroke = Keystroke::parse(key).expect("a key");
        cx.simulate_event(KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        if is_open(cx.window_handle(), cx) {
            cx.simulate_event(KeyUpEvent { keystroke });
        }
        cx.run_until_parked();
    }

    fn click_at(selector: &'static str, cx: &mut VisualTestContext) {
        let center = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} is not shown"))
            .center();
        cx.simulate_mouse_move(center, None, Modifiers::default());
        cx.simulate_click(center, Modifiers::default());
        cx.run_until_parked();
    }

    fn active_window(cx: &mut VisualTestContext) -> Option<AnyWindowHandle> {
        cx.update(|_, cx| cx.active_window())
    }

    #[gpui_kit::test]
    fn a_dialog_opens_in_its_own_window_centred_and_fitted(cx: &mut TestAppContext) {
        let (_, mut main) = owner_window(cx);
        let before = main.windows().len();
        let (dialog, _) = open_test_dialog(&mut main, true);
        let dialog = dialog.expect("the dialog opens");
        assert_eq!(main.windows().len(), before + 1, "one more window");
        assert_eq!(
            read(&main, |cx| owner_of(dialog, cx)),
            Some(main.window_handle())
        );
        assert_eq!(
            read(&main, |cx| dialog_of(main.window_handle(), cx)),
            Some(dialog)
        );
        assert!(read(&main, |cx| is_blocked(main.window_handle(), cx)));
        let bounds = bounds_of(dialog, &mut main);
        let rem = px(16.);
        assert_eq!(bounds.size.width, rem * 12. + PADDING * 2.);
        assert_eq!(bounds.center().x, OWNER.center().x, "centred across");
        let estimate = rem * 10.;
        assert_eq!(
            bounds.origin.y,
            OWNER.center().y - estimate / 2.,
            "the top stays where the estimate centred it"
        );
        // Title row, padding, content, gap, footer, padding.
        let fitted = title_bar_height(rem) + PADDING * 3. + px(200.) + rem * 2.;
        assert_eq!(bounds.size.height, fitted, "fits its content");
        let mut window = VisualTestContext::from_window(dialog, &main);
        assert_eq!(window.window_title().as_deref(), Some("Test dialog"));
        assert!(window.debug_bounds("dialog-title-bar").is_some());
        assert!(window.debug_bounds("modal-shield").is_none());
        assert!(main.debug_bounds("modal-shield").is_some());

        // Measured once, the same dialog opens at its height, centred.
        press("escape", &mut window);
        let (again, _) = open_test_dialog(&mut main, true);
        let bounds = bounds_of(again.expect("opens again"), &mut main);
        assert_eq!(bounds.size.height, fitted);
        assert_eq!(bounds.center(), OWNER.center(), "centred");
    }

    #[gpui_kit::test]
    fn the_owner_ignores_input_while_the_dialog_is_open(cx: &mut TestAppContext) {
        let (owner, mut main) = owner_window(cx);
        click_at("owner", &mut main);
        press("a", &mut main);
        assert_eq!(owner.read_with(&main, |o, _| (o.clicks, o.keys)), (1, 2));
        let (dialog, _) = open_test_dialog(&mut main, true);
        let dialog = dialog.expect("the dialog opens");
        main.update(|window, _| window.activate_window());
        main.run_until_parked();
        assert_eq!(
            active_window(&mut main),
            Some(dialog),
            "activating the owner brings the dialog back"
        );
        click_at("owner", &mut main);
        press("a", &mut main);
        main.simulate_keystrokes("enter");
        main.simulate_input("typed");
        assert_eq!(
            owner.read_with(&main, |o, _| (o.clicks, o.keys)),
            (1, 2),
            "no click or key reached the owner"
        );
        assert_eq!(active_window(&mut main), Some(dialog));
        assert!(is_open(dialog, &main), "nothing closed the dialog");

        let mut window = VisualTestContext::from_window(dialog, &main);
        press("escape", &mut window);
        assert!(!is_open(dialog, &main));
        assert_eq!(
            active_window(&mut main),
            Some(main.window_handle()),
            "the owner is active again"
        );
        click_at("owner", &mut main);
        press("a", &mut main);
        assert_eq!(owner.read_with(&main, |o, _| (o.clicks, o.keys)), (2, 4));
    }

    /// Every mouse button and the wheel: none reaches the owner while the
    /// dialog is open, all do once it has closed.
    #[gpui_kit::test]
    fn the_owner_ignores_every_button_and_the_wheel(cx: &mut TestAppContext) {
        let (owner, mut main) = owner_window(cx);
        let centre = main.debug_bounds("owner").expect("the owner").center();
        let input = |main: &mut VisualTestContext| {
            for button in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
                main.simulate_mouse_move(centre, None, Modifiers::default());
                main.simulate_event(MouseDownEvent {
                    button,
                    position: centre,
                    modifiers: Modifiers::default(),
                    click_count: 1,
                    first_mouse: false,
                });
                main.simulate_event(MouseUpEvent {
                    button,
                    position: centre,
                    modifiers: Modifiers::default(),
                    click_count: 1,
                });
            }
            main.simulate_event(ScrollEvent {
                position: centre,
                delta: ScrollDelta::Lines(point(0., 3.)),
                ..ScrollEvent::default()
            });
            main.run_until_parked();
        };
        let (dialog, _) = open_test_dialog(&mut main, true);
        let dialog = dialog.expect("the dialog opens");
        input(&mut main);
        assert_eq!(
            owner.read_with(&main, |o, _| (o.presses, o.scrolls)),
            (0, 0),
            "nothing reached the owner"
        );
        assert!(is_open(dialog, &main));
        let mut window = VisualTestContext::from_window(dialog, &main);
        press("escape", &mut window);
        input(&mut main);
        assert_eq!(
            owner.read_with(&main, |o, _| (o.presses, o.scrolls)),
            (3, 1)
        );
    }

    /// Enter closes the dialog on its press; the release, in the owner,
    /// does not press the owner's focused button.
    #[gpui_kit::test]
    fn the_release_of_a_closing_key_presses_nothing_in_the_owner(cx: &mut TestAppContext) {
        let (owner, mut main) = owner_window(cx);
        let (dialog, oks) = open_test_dialog(&mut main, true);
        let dialog = dialog.expect("the dialog opens");
        let mut window = VisualTestContext::from_window(dialog, &main);
        let enter = Keystroke::parse("enter").expect("a key");
        window.simulate_event(KeyDownEvent {
            keystroke: enter.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        assert!(!is_open(dialog, &main), "Enter closed the dialog");
        assert_eq!(oks.get(), 1);
        main.simulate_event(KeyUpEvent { keystroke: enter });
        assert_eq!(owner.read_with(&main, |o, _| o.clicks), 0, "not pressed");
        press("enter", &mut main);
        assert_eq!(
            owner.read_with(&main, |o, _| o.clicks),
            1,
            "Enter pressed in the owner presses its focused element"
        );
    }

    #[gpui_kit::test]
    fn escape_the_close_button_and_the_system_close_cancel(cx: &mut TestAppContext) {
        let (_, mut main) = owner_window(cx);
        for how in ["escape", "close button", "system"] {
            let (dialog, oks) = open_test_dialog(&mut main, true);
            let dialog = dialog.expect("the dialog opens");
            let mut window = VisualTestContext::from_window(dialog, &main);
            match how {
                "escape" => press("escape", &mut window),
                "close button" => click_at("dialog-close", &mut window),
                _ => {
                    assert!(!window.simulate_close(), "the dialog closes itself");
                    window.run_until_parked();
                }
            }
            assert!(!is_open(dialog, &main), "{how} closes");
            assert_eq!(oks.get(), 0, "{how} is no Enter");
            assert!(
                !read(&main, |cx| is_blocked(main.window_handle(), cx)),
                "{how}"
            );
        }
    }

    #[gpui_kit::test]
    fn enter_asks_the_dialog_whether_to_close(cx: &mut TestAppContext) {
        let (_, mut main) = owner_window(cx);
        let (dialog, oks) = open_test_dialog(&mut main, false);
        let dialog = dialog.expect("the dialog opens");
        let mut window = VisualTestContext::from_window(dialog, &main);
        press("enter", &mut window);
        assert_eq!(oks.get(), 1);
        assert!(is_open(dialog, &main), "the handler kept it open");
        press("escape", &mut window);
        let (dialog, oks) = open_test_dialog(&mut main, true);
        let dialog = dialog.expect("the dialog opens");
        let mut window = VisualTestContext::from_window(dialog, &main);
        press("enter", &mut window);
        assert_eq!(oks.get(), 1);
        assert!(!is_open(dialog, &main), "the handler closed it");
    }

    #[gpui_kit::test]
    fn dialogs_stack_one_per_window(cx: &mut TestAppContext) {
        let (_, mut main) = owner_window(cx);
        let (first, _) = open_test_dialog(&mut main, true);
        let first = first.expect("the dialog opens");
        let count = main.windows().len();
        let (still, _) = open_test_dialog(&mut main, true);
        assert_eq!(still, Some(first), "one dialog per window");
        assert_eq!(main.windows().len(), count);

        let mut window = VisualTestContext::from_window(first, &main);
        let (second, _) = open_test_dialog(&mut window, true);
        let second = second.expect("a dialog over the dialog");
        assert_eq!(read(&main, |cx| topmost(main.window_handle(), cx)), second);
        assert!(
            read(&main, |cx| is_blocked(first, cx)),
            "the first dialog waits"
        );
        assert!(window.debug_bounds("modal-shield").is_some());
        click_at("test-body", &mut window);
        assert_eq!(active_window(&mut main), Some(second));
        // Escape in the first dialog does nothing while the second is open.
        press("escape", &mut window);
        assert!(is_open(first, &main) && is_open(second, &main));

        let mut top = VisualTestContext::from_window(second, &main);
        press("escape", &mut top);
        assert!(!is_open(second, &main) && is_open(first, &main));
        assert_eq!(active_window(&mut main), Some(first));
        assert_eq!(read(&main, |cx| topmost(main.window_handle(), cx)), first);
        press("escape", &mut window);
        assert!(!is_open(first, &main));
        assert!(!read(&main, |cx| is_blocked(main.window_handle(), cx)));
    }

    #[gpui_kit::test]
    fn closing_a_window_closes_its_dialogs(cx: &mut TestAppContext) {
        let (_, mut main) = owner_window(cx);
        let count = main.windows().len();
        let (first, _) = open_test_dialog(&mut main, true);
        let first = first.expect("the dialog opens");
        let mut window = VisualTestContext::from_window(first, &main);
        let (second, _) = open_test_dialog(&mut window, true);
        let second = second.expect("a dialog over the dialog");
        let handle = main.window_handle();
        main.update(|window, _| window.remove_window());
        window.run_until_parked();
        assert!(!is_open(handle, &window));
        assert!(!is_open(first, &window) && !is_open(second, &window));
        assert_eq!(window.windows().len(), count - 1);
        assert!(read(&window, |cx| cx.global::<Dialogs>().open.is_empty()));
    }

    /// Dialog content with a focusable field that can be taken away.
    struct Removable {
        focus: FocusHandle,
        field: FocusHandle,
        shown: bool,
    }

    impl Render for Removable {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .id("removable")
                .track_focus(&self.focus)
                .h(px(100.))
                .when(self.shown, |this| {
                    this.child(div().id("field").track_focus(&self.field).h(px(20.)))
                })
        }
    }

    /// When the focused control leaves the content, the closest container
    /// still shown takes the keyboard, so Escape still closes the dialog.
    #[gpui_kit::test]
    fn the_keyboard_stays_in_the_dialog_when_its_focused_control_goes(cx: &mut TestAppContext) {
        let (_, mut main) = owner_window(cx);
        let content: Rc<Cell<Option<Entity<Removable>>>> = Rc::default();
        let slot = content.clone();
        main.update(|window, cx| {
            open(
                window,
                cx,
                DialogOptions::new("removable", "Removable", 20.),
                move |window, cx| {
                    let view = cx.new(|cx| Removable {
                        focus: cx.focus_handle(),
                        field: cx.focus_handle(),
                        shown: true,
                    });
                    view.read(cx).field.clone().focus(window, cx);
                    slot.set(Some(view.clone()));
                    move |content: DialogContent, _: &mut Window, _: &mut App| {
                        content.child(view.clone())
                    }
                },
            )
        });
        main.run_until_parked();
        let dialog = read(&main, |cx| dialog_of(main.window_handle(), cx)).expect("open");
        let view = content.take().expect("the content");
        let mut window = VisualTestContext::from_window(dialog, &main);
        let focused = |field: bool, window: &mut VisualTestContext| {
            let view = view.clone();
            window.update(move |window, cx| {
                let view = view.read(cx);
                let handle = if field { &view.field } else { &view.focus };
                handle.is_focused(window)
            })
        };
        assert!(focused(true, &mut window), "the field has the keyboard");
        view.update(&mut window, |view, cx| {
            view.shown = false;
            cx.notify();
        });
        window.run_until_parked();
        assert!(focused(false, &mut window), "its container took it");
        press("escape", &mut window);
        assert!(!is_open(dialog, &main), "Escape closed the dialog");
    }

    /// A dialog that grows with the UI scale while it is open (the settings)
    /// stays on the screen: near its right edge the window gets no wider
    /// than the room left, as it gets no taller than the room below.
    #[gpui_kit::test]
    fn a_dialog_growing_with_the_scale_stays_on_the_screen(cx: &mut TestAppContext) {
        use gpui_kit::component::Theme;
        let (_, main) = owner_window(cx);
        // A second owner on the right of the 1920 × 1080 test screen.
        let bounds = Bounds {
            origin: point(px(1100.), px(100.)),
            size: size(px(800.), px(800.)),
        };
        let (right, _) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..WindowOptions::default()
                },
                cx,
                |_, cx| {
                    cx.new(|cx| Owner {
                        focus: cx.focus_handle(),
                        clicks: 0,
                        keys: 0,
                        presses: 0,
                        scrolls: 0,
                    })
                },
            )
            .expect("a second owner")
        });
        let mut owner = VisualTestContext::from_window(right, &main);
        owner.run_until_parked();
        owner.update(|window, cx| {
            open(
                window,
                cx,
                DialogOptions::new("wide", "Wide", 0.).content_width(30.),
                |_, _| {
                    |content: DialogContent, _: &mut Window, _: &mut App| {
                        content.child(div().h(px(100.)))
                    }
                },
            )
        });
        owner.run_until_parked();
        let dialog = read(&owner, |cx| dialog_of(right, cx)).expect("the dialog opens");
        let screen_right = px(1920.) - SCREEN_MARGIN;
        let before = bounds_of(dialog, &mut owner);
        assert!(before.right() <= screen_right, "{before:?}");
        owner.update(|_, cx| Theme::update(cx, |theme| theme.font_size = px(32.)));
        owner.run_until_parked();
        let after = bounds_of(dialog, &mut owner);
        assert!(after.size.width > before.size.width, "wider at 200%");
        assert!(
            after.right() <= screen_right,
            "{after:?} off the {screen_right:?} screen edge"
        );
        assert_eq!(after.origin, before.origin, "the window did not move");
    }

    /// A dialog opened over a window at the right edge of the screen moves
    /// inside the screen, as far as the room a growing dialog keeps, so its
    /// first fit keeps the width it was designed with.
    #[gpui_kit::test]
    fn a_dialog_opened_at_the_screen_edge_keeps_its_width(cx: &mut TestAppContext) {
        let (_, main) = owner_window(cx);
        // An owner whose centre is 20 px from the right of the test screen.
        let bounds = Bounds {
            origin: point(px(1500.), px(100.)),
            size: size(px(800.), px(800.)),
        };
        let (right, _) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..WindowOptions::default()
                },
                cx,
                |_, cx| {
                    cx.new(|cx| Owner {
                        focus: cx.focus_handle(),
                        clicks: 0,
                        keys: 0,
                        presses: 0,
                        scrolls: 0,
                    })
                },
            )
            .expect("a second owner")
        });
        let mut owner = VisualTestContext::from_window(right, &main);
        owner.run_until_parked();
        let (dialog, _) = open_test_dialog(&mut owner, true);
        let bounds = bounds_of(dialog.expect("the dialog opens"), &mut owner);
        let rem = px(16.);
        assert_eq!(bounds.size.width, rem * 12. + PADDING * 2., "its width");
        assert_eq!(bounds.right(), px(1920.) - SCREEN_MARGIN, "{bounds:?}");
    }
}
