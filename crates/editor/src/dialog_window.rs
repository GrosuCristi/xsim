//! Modal dialogs in their own operating-system windows (D22, D24).
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
//!   padding. The width follows the UI scale (it is given in rems).
//! * Nothing animates (D19); only the operating system may animate the
//!   window as it appears.
//!
//! ## Its size and place (D24)
//!
//! A dialog that fits its content opens exactly as high as its content and
//! footer are, centred over its owner and inside the screen's work area,
//! [`SCREEN_MARGIN`] from its edges. GPUI cannot move a window once it is
//! open, and on Windows not even before a hidden window is first shown (it
//! then takes the bounds it was created with), so the window cannot open
//! at a guess and fit afterwards. Instead [`open`] first builds the content
//! in a hidden *probe* window, which GPUI lays out with the real fonts and
//! UI scale as it opens, reads the content's natural height there and
//! closes the probe, which is never shown; then it opens the dialog's
//! window at the measured size. The content is built twice for this: its
//! build function must not reach outside the window it is given (see
//! [`open`]).
//!
//! Content can change while the dialog is open: another waveform or
//! analysis type, more options, another UI scale. The window keeps fitting
//! it, growing or shrinking downward and to the right (its top left corner
//! stays). So that it stays on the screen as it grows, a dialog names the
//! states its content can reach ([`DialogOptions::reserve_states`],
//! [`DialogOptions::reserve_rem_size`]); the probe measures those too, and
//! the window opens just as far up and to the left of centred as the
//! largest of them needs to fit below and to the right of its corner. This
//! is the one place where a dialog may open off centre.
//!
//! Content taller than the work area, or grown past what was reserved,
//! scrolls; the footer with the dialog's buttons stays in view below it.
//!
//! On Windows the window's frame takes room from the size asked for: GPUI
//! creates a dialog window 0 × 0 and measures its frame then, as nothing,
//! so the frame (left, right and bottom; about 13 × 6.5 px at 200% display
//! scaling) comes out of the content area. A dialog that fits its content
//! therefore asks for its content's size plus the frame, which it reads as
//! the window's outer size less its content area. The frame is learnt from
//! the first such dialog at each display scale: that one opens a frame too
//! small and grows by it at once, the next ones open at their size.
//!
//! A resizable dialog (the library) opens at a given size, within the work
//! area, and its content fills the window.
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
    Decorations, DisplayId, Div, Element, ElementId, FocusHandle, Global, GlobalElementId,
    InspectorElementId, InteractiveElement as _, IntoElement, LayoutId, MouseButton, ParentElement,
    Pixels, PlatformDisplay, Render, ScrollHandle, SharedString, Size,
    StatefulInteractiveElement as _, Styled as _, Subscription, TitlebarOptions, Window,
    WindowBounds, WindowControlArea, WindowId, WindowKind, WindowOptions, canvas, div, point,
    prelude::FluentBuilder as _, px, size,
};
use std::cell::Cell;
use std::rc::Rc;

/// The kit's padding around a dialog's content.
pub const PADDING: Pixels = px(16.);

/// Key context of the dialog content: the kit's, whose bindings turn
/// Escape into [`Cancel`] and Enter into [`Confirm`].
const DIALOG_CONTEXT: &str = "Dialog";

/// Room kept between a dialog and the edges of the screen's work area.
pub const SCREEN_MARGIN: Pixels = px(16.);

/// Size asked for the hidden window that measures a dialog's content. It
/// does not matter, and is deliberately unlike any dialog's: the content is
/// laid out at the dialog's own width and its natural height whatever the
/// window's size (on Windows a hidden window has the system's default size,
/// not the one asked for).
const PROBE_SIZE: Size<Pixels> = Size {
    width: px(120.),
    height: px(80.),
};

/// Height of the title row: the kit's title bar at 100% UI scale, growing
/// with the scale beyond it.
fn title_bar_height(rem: Pixels) -> Pixels {
    TITLE_BAR_HEIGHT.max(rem * 2.125)
}

/// Whether xsim draws the title row in `window`, or in a dialog opened from
/// it: everywhere but where a Linux window manager decorates windows
/// itself.
fn draws_title_bar(window: &Window) -> bool {
    !(cfg!(target_os = "linux") && matches!(window.window_decorations(), Decorations::Server))
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

/// The work area dialogs are placed in, when tests set one.
struct WorkArea(Bounds<Pixels>);
impl Global for WorkArea {}

/// Places dialogs as on a screen whose work area (the screen less the
/// taskbar) is `area`, or again on the owner's display with `None`. For
/// tests: headless displays are 1920 × 1080 px with nothing taken.
#[doc(hidden)]
pub fn simulate_work_area(cx: &mut App, area: Option<Bounds<Pixels>>) {
    match area {
        Some(area) => cx.set_global(WorkArea(area)),
        None if cx.has_global::<WorkArea>() => {
            cx.remove_global::<WorkArea>();
        }
        None => {}
    }
}

/// The frame of a dialog window that fits its content, as learnt at each
/// display scale (see the [module](self)): what the platform takes from the
/// size asked for, as width and height.
#[derive(Default)]
struct Frames(Vec<(u32, Size<Pixels>)>);
impl Global for Frames {}

/// The frame a test sets in place of the window's own.
struct SimulatedFrame(Size<Pixels>);
impl Global for SimulatedFrame {}

/// Makes dialog windows take `frame` from the size they ask for, as Windows
/// does, or measure their own again with `None`. For tests: headless
/// windows have no frame. Only the measurement is simulated: a headless
/// window still shows the whole size asked for.
#[doc(hidden)]
pub fn simulate_window_frame(cx: &mut App, frame: Option<Size<Pixels>>) {
    match frame {
        Some(frame) => cx.set_global(SimulatedFrame(frame)),
        None if cx.has_global::<SimulatedFrame>() => {
            cx.remove_global::<SimulatedFrame>();
        }
        None => {}
    }
}

/// What the platform takes from the size `window` asked for: its outer size
/// less its content area. Only Windows has one (see the [module](self)); it
/// is nothing once GPUI measures the frame itself, as it does after a
/// change of display scale.
fn window_frame(window: &Window, cx: &App) -> Size<Pixels> {
    if let Some(simulated) = cx.try_global::<SimulatedFrame>() {
        return simulated.0;
    }
    if !cfg!(target_os = "windows") {
        return Size::default();
    }
    let outer = window.window_bounds().get_bounds().size;
    frame_between(outer, window.viewport_size(), window.scale_factor())
}

/// The frame between a window's `outer` size and its content area, `inner`,
/// at display scale `scale`, in whole device pixels. Both sizes are device
/// pixels divided by the scale, so their difference wavers in its last bits
/// from one window size to the next; a window asking for its content plus
/// that frame would then never settle.
fn frame_between(outer: Size<Pixels>, inner: Size<Pixels>, scale: f32) -> Size<Pixels> {
    let whole = |outer: Pixels, inner: Pixels| {
        px(((outer - inner).as_f32() * scale).round().max(0.) / scale)
    };
    size(
        whole(outer.width, inner.width),
        whole(outer.height, inner.height),
    )
}

/// The frame learnt for dialog windows at display scale `scale`; nothing
/// before the first one has opened there.
fn known_frame(scale: f32, cx: &App) -> Size<Pixels> {
    cx.try_global::<Frames>()
        .and_then(|frames| {
            frames
                .0
                .iter()
                .find(|(bits, _)| *bits == scale.to_bits())
                .map(|(_, frame)| *frame)
        })
        .unwrap_or_default()
}

/// Remembers `frame` for dialog windows at display scale `scale`.
fn learn_frame(scale: f32, frame: Size<Pixels>, cx: &mut App) {
    if !cx.has_global::<Frames>() {
        cx.set_global(Frames::default());
    }
    let frames = &mut cx.global_mut::<Frames>().0;
    match frames.iter_mut().find(|(bits, _)| *bits == scale.to_bits()) {
        Some(known) => known.1 = frame,
        None => frames.push((scale.to_bits(), frame)),
    }
}

/// Whether sizes `a` and `b` differ by less than half a device pixel at
/// display scale `scale` each way: asking for one after the other would
/// change nothing but rounding.
fn same_device_size(a: Size<Pixels>, b: Size<Pixels>, scale: f32) -> bool {
    let close = |a: Pixels, b: Pixels| ((a - b).abs() * scale) < px(0.5);
    close(a.width, b.width) && close(a.height, b.height)
}

/// `size` with `frame` added: the outer size of a window whose content area
/// is `size`.
fn with_frame(size: Size<Pixels>, frame: Size<Pixels>) -> Size<Pixels> {
    gpui_kit::size(size.width + frame.width, size.height + frame.height)
}

/// The work area of `display`, the screen a window at `owner` is on.
fn work_area(
    owner: Bounds<Pixels>,
    display: Option<&dyn PlatformDisplay>,
    cx: &App,
) -> Bounds<Pixels> {
    if let Some(area) = cx.try_global::<WorkArea>() {
        return area.0;
    }
    display.map_or(owner, |display| display.visible_bounds())
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
    /// Fits the content, measured before the window opens.
    Fit,
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
    title: SharedString,
    width: Width,
    height: Height,
    /// How many states of the content the window keeps room for, beyond
    /// the one it opens with (see [`Build::Reserved`]).
    reserved_states: usize,
    /// A rem size (UI scale) the window keeps room for.
    reserved_rem: Option<Pixels>,
}

impl DialogOptions {
    /// A dialog `width` rems wide (the whole window) whose height fits its
    /// content. `title` is the window's title until the content sets one.
    pub fn new(title: impl Into<SharedString>, width: f32) -> Self {
        Self {
            title: title.into(),
            width: Width::Window(width),
            height: Height::Fit,
            reserved_states: 0,
            reserved_rem: None,
        }
    }

    /// Makes the content `rems` wide, the window adding its padding.
    pub fn content_width(mut self, rems: f32) -> Self {
        self.width = Width::Content(rems);
        self
    }

    /// Keeps room on the screen for `states` other states of the content,
    /// which it can reach while the dialog is open (another type of
    /// waveform, say). Before the window opens, the build function is
    /// called once for each with [`Build::Reserved`], in the hidden window
    /// that measures the content, and the window opens where the largest of
    /// them fits too (see the [module](self)).
    pub fn reserve_states(mut self, states: usize) -> Self {
        self.reserved_states = states;
        self
    }

    /// Keeps room on the screen for the content at a rem size of `rem`: the
    /// largest UI scale that can be chosen while the dialog is open.
    pub fn reserve_rem_size(mut self, rem: Pixels) -> Self {
        self.reserved_rem = Some(rem);
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

/// Which content a dialog's build function makes (see [`open`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Build {
    /// The content the dialog opens with.
    Shown,
    /// The content put in its reserved state `n`, one of those of
    /// [`DialogOptions::reserve_states`]. It is only measured, in the
    /// hidden window.
    Reserved(usize),
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

    /// The row of buttons under the content. It stays in view when the
    /// content scrolls.
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
/// `build` returns the function that builds each frame of the content, for
/// the window it is given. Create the dialog's views there, with that
/// window: text inputs follow the focus of the window they were created
/// in. Focus the control that should have the keyboard there too;
/// otherwise the dialog itself takes it. A dialog that fits its content
/// calls it more than once: first in a hidden window that measures the
/// content and closes again (with [`Build::Shown`] and each reserved state,
/// and all of them again at a reserved rem size), then in the dialog's
/// window. So `build` must leave everything outside the window it is given
/// alone: no toasts, no changes to the application's entities, no focus
/// elsewhere.
pub fn open<R>(
    owner: &mut Window,
    cx: &mut App,
    options: DialogOptions,
    build: impl Fn(Build, &mut Window, &mut App) -> R + 'static,
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
    build: impl Fn(Build, &mut Window, &mut App) -> R + 'static,
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
    let display_id = display.as_ref().map(|display| display.id());
    let area = work_area(owner_bounds, display.as_deref(), cx);
    let title_bar = draws_title_bar(owner);
    let scale = owner.scale_factor();
    let (probed, frame) = match options.height {
        Height::Fit => {
            let place = Place {
                owner: owner_bounds,
                area,
                display: display_id,
                title_bar,
                frame: known_frame(scale, cx),
            };
            (probe(&options, rem, place, &build, cx), place.frame)
        }
        Height::Resizable { .. } => (None, Size::default()),
    };
    let geometry = Geometry::new(&options, rem, owner_bounds, area, title_bar, probed, frame);
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
        display_id,
        app_owns_titlebar_drag: true,
        ..WindowOptions::default()
    };
    let opened = gpui_kit::open_window(window_options, cx, move |window, cx| {
        let content = build(Build::Shown, window, cx);
        let view = cx.new(|cx| DialogWindow::new(options, geometry, Box::new(content), window, cx));
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

/// Where a dialog is about to open.
#[derive(Debug, Clone, Copy)]
struct Place {
    /// The owner's bounds.
    owner: Bounds<Pixels>,
    /// The work area of the owner's screen.
    area: Bounds<Pixels>,
    display: Option<DisplayId>,
    /// Whether the dialog will draw its title row.
    title_bar: bool,
    /// The window's frame, as far as known (see [`window_frame`]).
    frame: Size<Pixels>,
}

/// What the probe measured.
#[derive(Debug, Clone, Copy)]
struct Probed {
    /// Natural height of the content the dialog opens with, footer
    /// included.
    content: Pixels,
    /// The largest content area the window can need while the dialog is
    /// open, title row included: in any of its reserved states, at any
    /// reserved rem size, within the work area.
    reserve: Size<Pixels>,
}

/// Builds the content in a hidden window and measures it: as the dialog
/// opens, and in each reserved state at each reserved rem size. GPUI lays
/// a window out as it opens it, so the measurements are there once it is
/// open; a second frame catches anything that settles only on it. `None`
/// when nothing could be measured (in a test that skips drawing).
fn probe<R>(
    options: &DialogOptions,
    rem: Pixels,
    place: Place,
    build: &impl Fn(Build, &mut Window, &mut App) -> R,
    cx: &mut App,
) -> Option<Probed>
where
    R: Fn(DialogContent, &mut Window, &mut App) -> DialogContent + 'static,
{
    let mut rems = vec![rem];
    rems.extend(options.reserved_rem.filter(|reserved| *reserved != rem));
    let builds: Vec<Build> = std::iter::once(Build::Shown)
        .chain((0..options.reserved_states).map(Build::Reserved))
        .collect();
    let cases: Vec<(Build, Pixels)> = rems
        .iter()
        .flat_map(|&rem| builds.iter().map(move |&build| (build, rem)))
        .collect();
    let heights: Vec<Rc<Cell<Option<Pixels>>>> = cases.iter().map(|_| Rc::default()).collect();
    let window_options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: place.owner.origin,
            size: PROBE_SIZE,
        })),
        titlebar: None,
        focus: false,
        show: false,
        // Not a dialog: on Windows that would disable the active window.
        kind: WindowKind::Normal,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        display_id: place.display,
        ..WindowOptions::default()
    };
    let width = |rem: Pixels| {
        options
            .width
            .resolve(rem)
            .min(max_content(place.area, rem, place.frame).width)
    };
    let cells = heights.clone();
    let opened = gpui_kit::open_window(window_options, cx, |window, cx| {
        let entries = cases
            .iter()
            .zip(cells)
            .map(|(&(case, rem), height)| ProbeEntry {
                content: Box::new(build(case, window, cx)),
                rem,
                width: width(rem),
                height,
            })
            .collect();
        cx.new(|_| Probe { entries })
    });
    let (handle, view) = opened.ok()?;
    drop(view);
    handle
        .update(cx, |_, window, cx| {
            window.draw(cx).clear(cx);
            window.remove_window();
        })
        .ok();
    let content = heights[0].get()?;
    let mut reserve = Size::<Pixels>::default();
    for ((_, rem), height) in cases.iter().zip(&heights) {
        let Some(height) = height.get() else {
            continue;
        };
        let bar = if place.title_bar {
            title_bar_height(*rem)
        } else {
            px(0.)
        };
        let max = max_content(place.area, *rem, place.frame);
        reserve.width = reserve.width.max(width(*rem));
        reserve.height = reserve.height.max((bar + height).min(max.height));
    }
    Some(Probed { content, reserve })
}

/// One content the probe measures.
struct ProbeEntry {
    content: ContentFn,
    /// The rem size it is laid out at.
    rem: Pixels,
    /// The dialog's width at that rem size.
    width: Pixels,
    /// Its natural height, content and footer, once laid out.
    height: Rc<Cell<Option<Pixels>>>,
}

/// The root view of the probe: every content it measures, each at its
/// width and rem size and as high as it is, out of the window's way.
struct Probe {
    entries: Vec<ProbeEntry>,
}

impl Render for Probe {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let surfaces: Vec<AnyElement> = self
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let content = (entry.content)(DialogContent::default(), window, cx);
                let DialogContent {
                    children, footer, ..
                } = content;
                let height = entry.height.clone();
                let measure = canvas(
                    move |bounds, _, _| height.set(Some(bounds.size.height)),
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0();
                let surface = v_flex()
                    .relative()
                    .w_full()
                    .child(content_part(children))
                    .children(footer.map(footer_part))
                    .child(measure);
                let placed = div()
                    .id(("dialog-probe", index))
                    .absolute()
                    .top_0()
                    .left_0()
                    .w(entry.width)
                    .child(surface)
                    .into_any_element();
                RemScope {
                    rem: entry.rem,
                    child: placed,
                }
                .into_any_element()
            })
            .collect();
        div().size_full().overflow_hidden().children(surfaces)
    }
}

/// Lays its child out, and paints it, at another rem size: the probe's
/// view of content at another UI scale.
struct RemScope {
    rem: Pixels,
    child: AnyElement,
}

impl IntoElement for RemScope {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for RemScope {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let child = &mut self.child;
        let id = window.with_rem_size(Some(self.rem), |window| child.request_layout(window, cx));
        (id, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let child = &mut self.child;
        window.with_rem_size(Some(self.rem), |window| {
            child.prepaint(window, cx);
        });
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let child = &mut self.child;
        window.with_rem_size(Some(self.rem), |window| child.paint(window, cx));
    }
}

/// The content of a dialog with the padding around it; below it, the gap
/// to the footer. The same in the dialog's window and in the probe, which
/// must measure what the window shows.
fn content_part(children: Vec<AnyElement>) -> Div {
    v_flex().w_full().p(PADDING).children(children)
}

/// The footer of a dialog with the padding beside and below it.
fn footer_part(footer: AnyElement) -> Div {
    div()
        .flex_shrink_0()
        .w_full()
        .px(PADDING)
        .pb(PADDING)
        .child(footer)
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
    if let Some(dialog) = dialog_view(window, cx) {
        let focus = dialog.read(cx).focus.clone();
        focus.focus(window, cx);
    }
}

/// The dialog view at the root of `window`, if it shows a dialog.
fn dialog_view(window: &Window, cx: &App) -> Option<gpui_kit::Entity<DialogWindow>> {
    window
        .root::<Root>()
        .flatten()
        .and_then(|root| root.read(cx).view().clone().downcast::<DialogWindow>().ok())
}

/// How a dialog window is laid out, for tests.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DialogLayout {
    /// The window's bounds on the screen.
    pub bounds: Bounds<Pixels>,
    /// The size of the window's content area.
    pub viewport: Size<Pixels>,
    /// The height the title row, content and footer take without
    /// scrolling, as last laid out.
    pub natural_height: Pixels,
    /// The footer's bounds in the window, as last laid out.
    pub footer: Option<Bounds<Pixels>>,
    /// Where the window may be: the work area it was placed in, less
    /// [`SCREEN_MARGIN`] all round.
    pub room: Bounds<Pixels>,
    /// The frame a test simulates (see [`simulate_window_frame`]): what
    /// Windows would take from the content area, which a headless window
    /// does not.
    pub frame: Size<Pixels>,
}

impl DialogLayout {
    /// Checks the window against D24: it is inside the room; it shows all
    /// its content without scrolling, or, when that is taller than the
    /// room, it is as high as the room; and its footer is inside it. Each
    /// to within a pixel, for rounding.
    pub fn check(&self) -> Result<(), String> {
        let slack = px(1.);
        let (bounds, room) = (self.bounds, self.room);
        if bounds.left() < room.left() - slack
            || bounds.top() < room.top() - slack
            || bounds.right() > room.right() + slack
            || bounds.bottom() > room.bottom() + slack
        {
            return Err(format!("the window {bounds:?} leaves the room {room:?}"));
        }
        if (self.viewport.height - bounds.size.height).abs() > slack {
            return Err(format!(
                "the content area {:?} is not the window's {:?}",
                self.viewport, bounds.size
            ));
        }
        // What Windows would show inside the frame, and all of the content
        // unless the room inside the frame is smaller.
        let shown = self.viewport.height - self.frame.height;
        let needed = self
            .natural_height
            .min(room.size.height - self.frame.height);
        if shown + slack < needed {
            return Err(format!(
                "the window shows {shown:?} for {:?} of content in {:?} of room",
                self.natural_height, room.size.height
            ));
        }
        if let Some(footer) = self.footer
            && (footer.top() < px(0.)
                || footer.bottom() > self.viewport.height + slack
                || footer.right() > self.viewport.width + slack)
        {
            return Err(format!(
                "the footer {footer:?} is outside the window's {:?}",
                self.viewport
            ));
        }
        Ok(())
    }
}

/// The layout of the dialog in `window`, if it shows one that fits its
/// content. For tests.
#[doc(hidden)]
pub fn layout_of(window: AnyWindowHandle, cx: &mut App) -> Option<DialogLayout> {
    window
        .update(cx, |_, window, cx| {
            let dialog = dialog_view(window, cx)?;
            let dialog = dialog.read(cx);
            if !dialog.fits_content() {
                return None;
            }
            let bar = if draws_title_bar(window) {
                title_bar_height(window.rem_size())
            } else {
                px(0.)
            };
            let natural = dialog.natural_height()?;
            let area = dialog.area;
            Some(DialogLayout {
                bounds: window.bounds(),
                viewport: window.viewport_size(),
                natural_height: bar + natural,
                footer: dialog.footer_bounds.get(),
                frame: cx
                    .try_global::<SimulatedFrame>()
                    .map_or_else(Size::default, |simulated| simulated.0),
                room: Bounds {
                    origin: point(area.left() + SCREEN_MARGIN, area.top() + SCREEN_MARGIN),
                    size: size(
                        area.size.width - SCREEN_MARGIN * 2.,
                        area.size.height - SCREEN_MARGIN * 2.,
                    ),
                },
            })
        })
        .ok()
        .flatten()
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

impl Dialogs {
    /// Forgets the dialog in window `id`; returns its owner.
    fn remove(&mut self, id: WindowId) -> Option<AnyWindowHandle> {
        let index = self.open.iter().position(|d| d.window.window_id() == id)?;
        Some(self.open.remove(index).owner)
    }
}

/// The largest window that fits `area` (a work area) with
/// [`SCREEN_MARGIN`] all round, but no smaller than 10 rem square.
fn max_size(area: Bounds<Pixels>, rem: Pixels) -> Size<Pixels> {
    size(
        (area.size.width - SCREEN_MARGIN * 2.).max(rem * 10.),
        (area.size.height - SCREEN_MARGIN * 2.).max(rem * 10.),
    )
}

/// The largest content area of a window with `frame` that fits `area` as
/// [`max_size`] says, but no smaller than 10 rem square.
fn max_content(area: Bounds<Pixels>, rem: Pixels, frame: Size<Pixels>) -> Size<Pixels> {
    let max = max_size(area, rem);
    size(
        (max.width - frame.width).max(rem * 10.),
        (max.height - frame.height).max(rem * 10.),
    )
}

/// Where a dialog window opens.
#[derive(Debug, Clone, Copy)]
struct Geometry {
    /// The bounds asked for: the content area's and the frame's (see
    /// [`window_frame`]).
    bounds: Bounds<Pixels>,
    min_size: Option<Size<Pixels>>,
    /// The work area it is placed in.
    area: Bounds<Pixels>,
    limits: Limits,
}

/// How far right and down a dialog window's edges may go as its content
/// grows: to just inside the work area's.
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
        area: Bounds<Pixels>,
        title_bar: bool,
        probed: Option<Probed>,
        frame: Size<Pixels>,
    ) -> Self {
        let max = max_content(area, rem, frame);
        let width = options.width.resolve(rem).min(max.width);
        let (height, reserve, min_size) = match options.height {
            Height::Fit => {
                let bar = if title_bar {
                    title_bar_height(rem)
                } else {
                    px(0.)
                };
                match probed {
                    Some(probed) => ((bar + probed.content).min(max.height), probed.reserve, None),
                    // Nothing measured: as high as the screen allows, and
                    // the fit to the content follows.
                    None => (max.height, Size::default(), None),
                }
            }
            Height::Resizable {
                height,
                min_width,
                min_height,
            } => (
                (rem * height).min(max.height),
                Size::default(),
                Some(size(
                    (rem * min_width).min(width),
                    (rem * min_height).min(max.height),
                )),
            ),
        };
        Self {
            bounds: placed(
                owner,
                with_frame(size(width, height), frame),
                with_frame(reserve, frame),
                area,
            ),
            min_size,
            area,
            limits: Limits {
                right: area.right() - SCREEN_MARGIN,
                bottom: area.bottom() - SCREEN_MARGIN,
            },
        }
    }
}

/// Bounds of `size` centred over `owner`, then moved inside `area` (the
/// work area), [`SCREEN_MARGIN`] from its edges, where they would leave
/// it, and as far up and left as `reserve`, the largest size the window may
/// grow to from its top left corner, needs to stay inside it too (see
/// [`Limits`]).
fn placed(
    owner: Bounds<Pixels>,
    size: Size<Pixels>,
    reserve: Size<Pixels>,
    area: Bounds<Pixels>,
) -> Bounds<Pixels> {
    let room = size.max(&reserve);
    let center = owner.center();
    let x = (center.x - size.width / 2.)
        .min(area.right() - SCREEN_MARGIN - room.width)
        .max(area.left() + SCREEN_MARGIN);
    let y = (center.y - size.height / 2.)
        .min(area.bottom() - SCREEN_MARGIN - room.height)
        .max(area.top() + SCREEN_MARGIN);
    Bounds {
        origin: point(x, y),
        size,
    }
}

/// The root view of a dialog window: title row, content and footer.
struct DialogWindow {
    title: SharedString,
    /// The title last given to the operating system.
    shown_title: SharedString,
    width: Width,
    height: Height,
    content: ContentFn,
    focus: FocusHandle,
    scroll: ScrollHandle,
    /// Natural height of the content, padding included, measured on the
    /// last layout.
    content_height: Rc<Cell<Option<Pixels>>>,
    /// Bounds of the footer, padding included, in the window on the last
    /// layout; `None` without one.
    footer_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// The window size last asked for, frame included, so a size the
    /// platform rounds is not asked for again and again: at first the size
    /// the window opened at.
    requested: Option<Size<Pixels>>,
    /// Whether the window's frame has been measured and remembered for the
    /// dialogs opened after it (see [`window_frame`]).
    frame_learnt: bool,
    /// The UI scale and content area the size was last fitted at. A new
    /// content area means a new frame too, on Windows: after a change of
    /// display scale GPUI measures the frame itself (see [`window_frame`]).
    fitted: Option<(Pixels, Size<Pixels>)>,
    /// The work area the window was placed in.
    area: Bounds<Pixels>,
    limits: Limits,
    /// A press on the title row that may become a window move.
    should_move: bool,
    _focus_lost: Subscription,
}

impl DialogWindow {
    fn new(
        options: DialogOptions,
        geometry: Geometry,
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
            shown_title: SharedString::default(),
            title: options.title,
            width: options.width,
            height: options.height,
            content,
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            content_height: Rc::default(),
            footer_bounds: Rc::default(),
            requested: Some(geometry.bounds.size),
            frame_learnt: false,
            fitted: None,
            area: geometry.area,
            limits: geometry.limits,
            should_move: false,
            _focus_lost: focus_lost,
        }
    }

    fn fits_content(&self) -> bool {
        matches!(self.height, Height::Fit)
    }

    /// Natural height of the content and footer, as last laid out.
    fn natural_height(&self) -> Option<Pixels> {
        let content = self.content_height.get()?;
        let footer = self.footer_bounds.get().map(|footer| footer.size.height);
        Some(content + footer.unwrap_or_default())
    }

    /// Resizes the window to the measured content at the current UI scale,
    /// within the work area: the top left corner stays, so the window gets
    /// no wider or taller than the room left to the right and below. The
    /// window opened at the measured size, so this changes nothing until
    /// the content or the scale changes, unless the window's frame was not
    /// known yet (see [`window_frame`]): the first fit then makes room for
    /// it.
    fn fit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.fits_content() {
            return;
        }
        let frame = window_frame(window, cx);
        if !self.frame_learnt {
            self.frame_learnt = true;
            learn_frame(window.scale_factor(), frame, cx);
        }
        let Some(natural) = self.natural_height() else {
            return;
        };
        let rem = window.rem_size();
        let bar = if draws_title_bar(window) {
            title_bar_height(rem)
        } else {
            px(0.)
        };
        let min_height = bar + rem * 3.;
        let origin = window.bounds().origin;
        let height = (bar + natural)
            .min(self.limits.bottom - origin.y - frame.height)
            .max(min_height);
        let width = self
            .width
            .resolve(rem)
            .min(self.limits.right - origin.x - frame.width)
            .max(rem * 10.);
        let wanted = with_frame(size(width, height), frame);
        if self
            .requested
            .is_some_and(|requested| same_device_size(requested, wanted, window.scale_factor()))
        {
            return;
        }
        self.requested = Some(wanted);
        window.resize(wanted);
        // Headless windows report the new size only when told.
        window.bounds_changed(cx);
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

    /// The content and the footer under the title row. A dialog that fits
    /// its content is as high as they are, or shrinks to the window, its
    /// content scrolling above the footer; the content of a resizable one
    /// fills the window.
    fn body(&self, content: DialogContent, cx: &mut Context<Self>) -> AnyElement {
        let DialogContent {
            children, footer, ..
        } = content;
        let surface = v_flex()
            .debug_selector(|| "dialog-0".into())
            .w_full()
            .min_h_0();
        if !self.fits_content() {
            return surface
                .flex_1()
                .child(content_part(children).flex_1().min_h_0())
                .children(footer.map(footer_part))
                .into_any_element();
        }
        // Each part records its bounds on every layout and refits the window
        // when its height changed.
        let view = cx.weak_entity();
        let measure = move |record: Rc<dyn Fn(Bounds<Pixels>) -> bool>| {
            let view = view.clone();
            canvas(
                move |bounds, window, cx| {
                    if record(bounds) {
                        window.defer(cx, move |window, cx| {
                            view.update(cx, |this, cx| this.fit(window, cx)).ok();
                        });
                    }
                },
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0()
        };
        let content_height = self.content_height.clone();
        let content_measure = measure(Rc::new(move |bounds: Bounds<Pixels>| {
            content_height.replace(Some(bounds.size.height)) != Some(bounds.size.height)
        }));
        let footer = footer.map(|footer| {
            let footer_bounds = self.footer_bounds.clone();
            footer_part(footer)
                .relative()
                .debug_selector(|| "dialog-footer".into())
                .child(measure(Rc::new(move |bounds: Bounds<Pixels>| {
                    let old = footer_bounds.replace(Some(bounds));
                    old.map(|old| old.size.height) != Some(bounds.size.height)
                })))
        });
        if footer.is_none() {
            self.footer_bounds.set(None);
        }
        surface
            .flex_shrink_1()
            .child(
                div()
                    .id("dialog-scroll")
                    .flex_shrink_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(
                        content_part(children)
                            .relative()
                            .flex_shrink_0()
                            .debug_selector(|| "dialog-content".into())
                            .child(content_measure),
                    ),
            )
            .children(footer)
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
        let fitted = (rem, window.viewport_size());
        if self.fitted != Some(fitted) {
            self.fitted = Some(fitted);
            let view = cx.weak_entity();
            window.defer(cx, move |window, cx| {
                view.update(cx, |this, cx| this.fit(window, cx)).ok();
            });
        }
        let on_ok = content.on_ok.clone();
        let title_bar = draws_title_bar(window).then(|| self.title_bar(title, window, cx));
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
    use gpui_kit::component::Theme;
    use gpui_kit::{
        Entity, KeyDownEvent, KeyUpEvent, Keystroke, Modifiers, MouseDownEvent, MouseUpEvent,
        ScrollDelta, ScrollEvent, TestAppContext, VisualContext as _, VisualTestContext, rems,
    };
    use std::cell::RefCell;

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
        owner_window_at(OWNER, cx)
    }

    /// An owner window at `bounds`, the first window of the test.
    fn owner_window_at(
        bounds: Bounds<Pixels>,
        cx: &mut TestAppContext,
    ) -> (Entity<Owner>, VisualTestContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_reduce_motion(true);
            init(cx);
        });
        let (window, owner) = cx.update(|cx| second_owner(bounds, cx));
        let visual = VisualTestContext::from_window(window, cx);
        visual.run_until_parked();
        (owner, visual)
    }

    /// One more owner window at `bounds`, its content focused.
    fn second_owner(bounds: Bounds<Pixels>, cx: &mut App) -> (AnyWindowHandle, Entity<Owner>) {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
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
                DialogOptions::new("Test dialog", 0.).content_width(12.),
                move |_, _, _| {
                    let counter = counter.clone();
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

    /// Content of a sized test dialog: `height` high as it opens (a test may
    /// change it while the dialog is open), `reserved[n]` high in reserved
    /// state `n`, and a 2 rem footer.
    struct Sized {
        height: Rc<Cell<f32>>,
        /// Heights in rems, or in pixels when `in_rems` is false.
        in_rems: bool,
        reserved: Vec<f32>,
        /// The content each build was asked for, in order.
        builds: Rc<RefCell<Vec<Build>>>,
    }

    impl Sized {
        fn px(height: f32) -> Self {
            Self {
                height: Rc::new(Cell::new(height)),
                in_rems: false,
                reserved: Vec::new(),
                builds: Rc::default(),
            }
        }

        fn rems(height: f32) -> Self {
            Self {
                in_rems: true,
                ..Self::px(height)
            }
        }

        fn reserving(mut self, heights: &[f32]) -> Self {
            self.reserved = heights.to_vec();
            self
        }

        /// Opens the dialog with `options` over `cx`'s window; returns it.
        fn open(&self, options: DialogOptions, cx: &mut VisualTestContext) -> AnyWindowHandle {
            let (height, in_rems, reserved, builds) = (
                self.height.clone(),
                self.in_rems,
                self.reserved.clone(),
                self.builds.clone(),
            );
            let options = options.reserve_states(reserved.len());
            cx.update(|window, cx| {
                open(window, cx, options, move |build, _, _| {
                    builds.borrow_mut().push(build);
                    let fixed = match build {
                        Build::Shown => None,
                        Build::Reserved(n) => Some(reserved[n]),
                    };
                    let height = height.clone();
                    move |content: DialogContent, _: &mut Window, _: &mut App| {
                        let value = fixed.unwrap_or_else(|| height.get());
                        let body = if in_rems {
                            div().h(rems(value))
                        } else {
                            div().h(px(value))
                        };
                        content
                            .title("Sized")
                            .child(body.debug_selector(|| "test-body".into()))
                            .footer(div().h(rems(2.)).debug_selector(|| "test-footer".into()))
                    }
                })
            });
            cx.run_until_parked();
            let owner = cx.window_handle();
            read(cx, |cx| dialog_of(owner, cx)).expect("the dialog opens")
        }

        /// Changes the shown content's height and lets the dialog refit.
        fn set_height(&self, height: f32, dialog: AnyWindowHandle, cx: &mut VisualTestContext) {
            self.height.set(height);
            cx.cx
                .update(|cx| dialog.update(cx, |_, window, _| window.refresh()))
                .expect("the dialog is open");
            cx.run_until_parked();
        }
    }

    fn bounds_of(window: AnyWindowHandle, cx: &mut VisualTestContext) -> Bounds<Pixels> {
        cx.cx
            .update(|cx| window.update(cx, |_, window, _| window.bounds()))
            .expect("an open window")
    }

    fn layout(window: AnyWindowHandle, cx: &mut VisualTestContext) -> DialogLayout {
        cx.cx
            .update(|cx| layout_of(window, cx))
            .expect("a dialog that fits its content")
    }

    /// Checks `window` against D24 (see [`DialogLayout::check`]).
    fn assert_fully_shown(window: AnyWindowHandle, case: &str, cx: &mut VisualTestContext) {
        let layout = layout(window, cx);
        if let Err(problem) = layout.check() {
            panic!("{case}: {problem} ({layout:?})");
        }
        // Apart from the dialog's own measurements: content that fits is not
        // cut off, but between the title row and the footer.
        let mut dialog = VisualTestContext::from_window(window, &cx.cx);
        let content = dialog.debug_bounds("dialog-content").expect("the content");
        let bar = dialog
            .debug_bounds("dialog-title-bar")
            .map_or(px(0.), |bar| bar.bottom());
        let floor = dialog
            .debug_bounds("dialog-footer")
            .map_or(layout.viewport.height, |footer| footer.top());
        if layout.natural_height <= layout.room.size.height {
            assert!(
                content.top() >= bar - px(1.) && content.bottom() <= floor + px(1.),
                "{case}: the content {content:?} is cut off between {bar:?} and {floor:?}"
            );
        }
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

    fn set_font_size(font_size: f32, cx: &mut VisualTestContext) {
        cx.update(|_, cx| Theme::update(cx, |theme| theme.font_size = px(font_size)));
        cx.run_until_parked();
    }

    /// The owner's screen at 200% display scaling: a 1920 × 1030 px work
    /// area (D24), and a smaller one.
    const WORK_AREAS: [Bounds<Pixels>; 2] = [
        Bounds {
            origin: gpui_kit::Point {
                x: px(0.),
                y: px(0.),
            },
            size: Size {
                width: px(1920.),
                height: px(1030.),
            },
        },
        Bounds {
            origin: gpui_kit::Point {
                x: px(0.),
                y: px(0.),
            },
            size: Size {
                width: px(1280.),
                height: px(680.),
            },
        },
    ];

    /// D24: the dialog opens in its own window, at its measured height and
    /// centred over its owner, at 100% and 150% UI scale. The hidden window
    /// that measured it is gone, and the owner keeps its focus.
    #[gpui_kit::test]
    fn a_dialog_opens_in_its_own_window_centred_and_fitted(cx: &mut TestAppContext) {
        let (owner, mut main) = owner_window(cx);
        let before = main.windows().len();
        for font_size in [16., 24.] {
            set_font_size(font_size, &mut main);
            let (dialog, _) = open_test_dialog(&mut main, true);
            let dialog = dialog.expect("the dialog opens");
            assert_eq!(main.windows().len(), before + 1, "one more window");
            let active = active_window(&mut main);
            assert!(
                active.is_none_or(|active| active == dialog || active == main.window_handle()),
                "the hidden window was never activated: {active:?}"
            );
            assert!(
                main.update(|window, cx| owner.read(cx).focus.is_focused(window)),
                "the owner kept its focus"
            );
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
            let rem = px(font_size);
            assert_eq!(bounds.size.width, rem * 12. + PADDING * 2.);
            // Title row, padding, content, gap, footer, padding.
            let fitted = title_bar_height(rem) + PADDING * 3. + px(200.) + rem * 2.;
            assert_eq!(bounds.size.height, fitted, "fits its content");
            assert_eq!(bounds.center(), OWNER.center(), "centred");
            assert_fully_shown(dialog, "the test dialog", &mut main);
            let mut window = VisualTestContext::from_window(dialog, &main);
            assert_eq!(window.window_title().as_deref(), Some("Test dialog"));
            assert!(window.debug_bounds("dialog-title-bar").is_some());
            assert!(window.debug_bounds("modal-shield").is_none());
            assert!(main.debug_bounds("modal-shield").is_some());
            press("escape", &mut window);
            assert_eq!(main.windows().len(), before);
        }
    }

    /// D24: the content is built once to be measured and once to be shown,
    /// and once more for each reserved state, which is only measured.
    #[gpui_kit::test]
    fn the_content_is_measured_before_the_window_opens(cx: &mut TestAppContext) {
        let (_, mut main) = owner_window(cx);
        let plain = Sized::px(100.);
        let dialog = plain.open(DialogOptions::new("Plain", 20.), &mut main);
        assert_eq!(*plain.builds.borrow(), [Build::Shown, Build::Shown]);
        let mut window = VisualTestContext::from_window(dialog, &main);
        press("escape", &mut window);
        let reserving = Sized::px(100.).reserving(&[300., 200.]);
        let dialog = reserving.open(DialogOptions::new("Reserving", 20.), &mut main);
        assert_eq!(
            *reserving.builds.borrow(),
            [
                Build::Shown,
                Build::Reserved(0),
                Build::Reserved(1),
                Build::Shown
            ]
        );
        assert_eq!(main.windows().len(), 2, "the owner and the dialog");
        let rem = px(16.);
        let fitted = title_bar_height(rem) + PADDING * 3. + px(100.) + rem * 2.;
        assert_eq!(
            bounds_of(dialog, &mut main).size.height,
            fitted,
            "the window fits the content it shows"
        );
    }

    /// D24: text that wraps at the dialog's width is measured at that width,
    /// whatever the size of the hidden window that measures it: the window
    /// opens centred at the height it then keeps.
    #[gpui_kit::test]
    fn wrapped_text_is_measured_at_the_dialog_width(cx: &mut TestAppContext) {
        let (_, mut main) = owner_window(cx);
        let text = "A long explanation that wraps onto several lines in a narrow \
                    dialog, so the content's height depends on the dialog's width. "
            .repeat(2);
        main.update(|window, cx| {
            open(
                window,
                cx,
                DialogOptions::new("Wrapped", 16.),
                move |_, _, _| {
                    let text = text.clone();
                    move |content: DialogContent, _: &mut Window, _: &mut App| {
                        content
                            .child(
                                div()
                                    .debug_selector(|| "wrapped".into())
                                    .child(text.clone()),
                            )
                            .footer(div().h(rems(2.)))
                    }
                },
            )
        });
        main.run_until_parked();
        let dialog = read(&main, |cx| dialog_of(main.window_handle(), cx)).expect("open");
        let mut window = VisualTestContext::from_window(dialog, &main);
        let wrapped = window.debug_bounds("wrapped").expect("the text");
        assert!(
            wrapped.size.height > px(16.) * 4.,
            "several lines: {wrapped:?}"
        );
        let bounds = bounds_of(dialog, &mut main);
        assert_eq!(
            bounds.center(),
            OWNER.center(),
            "centred, so never refitted"
        );
        assert_fully_shown(dialog, "wrapped text", &mut main);
        let layout = layout(dialog, &mut main);
        assert_eq!(bounds.size.height, layout.natural_height);
    }

    /// D24: on the owner's screens a dialog opens fully on screen; one taller
    /// than the work area fills its height and scrolls its content, never
    /// its footer.
    #[gpui_kit::test]
    fn a_dialog_fits_the_work_area_and_keeps_its_footer_in_view(cx: &mut TestAppContext) {
        let (_, mut main) = owner_window_at(WORK_AREAS[1], cx);
        for area in WORK_AREAS {
            main.update(|_, cx| simulate_work_area(cx, Some(area)));
            for (height, case) in [(300., "short"), (2000., "taller than the screen")] {
                let sized = Sized::px(height);
                let dialog = sized.open(DialogOptions::new("Sized", 20.), &mut main);
                let case = format!("{case} on {:?}", area.size);
                assert_fully_shown(dialog, &case, &mut main);
                let mut window = VisualTestContext::from_window(dialog, &main);
                let footer = window.debug_bounds("test-footer").expect("the footer");
                let viewport = window.update(|window, _| window.viewport_size());
                assert!(footer.bottom() <= viewport.height, "{case}: {footer:?}");
                if height > area.size.height.as_f32() {
                    let bounds = bounds_of(dialog, &mut main);
                    assert_eq!(bounds.top(), area.top() + SCREEN_MARGIN, "{case}");
                    assert_eq!(bounds.bottom(), area.bottom() - SCREEN_MARGIN, "{case}");
                }
                press("escape", &mut window);
            }
        }
    }

    /// D24: content that can grow while open opens where its tallest
    /// reserved state fits below the window's top, and growing to it keeps
    /// the window on the screen with all of it shown. Without the reserve,
    /// the same growth would leave the screen's bottom.
    #[gpui_kit::test]
    fn a_dialog_opens_where_its_reserved_states_fit(cx: &mut TestAppContext) {
        let (_, mut main) = owner_window_at(WORK_AREAS[0], cx);
        let owner = main.update(|window, _| window.bounds());
        for area in WORK_AREAS {
            main.update(|_, cx| simulate_work_area(cx, Some(area)));
            let tall = area.size.height.as_f32() - 260.;
            for font_size in [16., 24.] {
                set_font_size(font_size, &mut main);
                let case = format!("{font_size} px on {:?}", area.size);
                let sized = Sized::px(200.).reserving(&[tall, 100.]);
                let dialog = sized.open(DialogOptions::new("Sized", 20.), &mut main);
                assert_fully_shown(dialog, &case, &mut main);
                let opened = bounds_of(dialog, &mut main);
                let rem = px(font_size);
                let reserve = title_bar_height(rem) + PADDING * 3. + px(tall) + rem * 2.;
                let centred = owner.center().y - opened.size.height / 2.;
                assert!(
                    centred > area.bottom() - SCREEN_MARGIN - reserve,
                    "{case}: the test needs the reserve to move the window"
                );
                assert_eq!(
                    opened.top(),
                    area.bottom() - SCREEN_MARGIN - reserve,
                    "{case}: as near centred as the reserve allows"
                );
                sized.set_height(tall, dialog, &mut main);
                assert_fully_shown(dialog, &format!("{case}, grown"), &mut main);
                let grown = bounds_of(dialog, &mut main);
                assert_eq!(grown.origin, opened.origin, "{case}: did not move");
                assert_eq!(grown.size.height, reserve, "{case}: grown to its content");
                let mut window = VisualTestContext::from_window(dialog, &main);
                press("escape", &mut window);

                let unreserved = Sized::px(200.);
                let dialog = unreserved.open(DialogOptions::new("Sized", 20.), &mut main);
                unreserved.set_height(tall, dialog, &mut main);
                assert!(
                    layout(dialog, &mut main).check().is_err(),
                    "{case}: without the reserve the grown window is cut off"
                );
                let mut window = VisualTestContext::from_window(dialog, &main);
                press("escape", &mut window);
            }
            set_font_size(16., &mut main);
        }
    }

    /// D24 on Windows, whose frame comes out of the size a dialog asks for
    /// (as GPUI measures it): the first dialog grows by its frame as soon as
    /// it opens, the frame learnt; the next ones open at their content's
    /// size plus the frame, centred. Either way the content area left inside
    /// the frame holds all the content, and the whole window stays inside
    /// the work area, as it grows to a reserved state too.
    #[gpui_kit::test]
    fn a_dialog_makes_room_for_the_window_frame(cx: &mut TestAppContext) {
        let (_, mut main) = owner_window_at(WORK_AREAS[0], cx);
        let owner = main.update(|window, _| window.bounds());
        // Windows' at 200% display scaling: 13 device px each side and at
        // the bottom.
        let frame = size(px(13.), px(6.5));
        let area = WORK_AREAS[0];
        let room_bottom = area.bottom() - SCREEN_MARGIN;
        main.update(|_, cx| {
            simulate_work_area(cx, Some(area));
            simulate_window_frame(cx, Some(frame));
        });
        let rem = px(16.);
        let fitted = title_bar_height(rem) + PADDING * 3. + px(300.) + rem * 2.;
        let width = rem * 20.;
        for pass in ["the first", "the next"] {
            let sized = Sized::px(300.);
            let dialog = sized.open(DialogOptions::new("Sized", 20.), &mut main);
            let bounds = bounds_of(dialog, &mut main);
            assert_eq!(
                bounds.size,
                size(width + frame.width, fitted + frame.height),
                "{pass}: the content's size and the frame's"
            );
            let layout = layout(dialog, &mut main);
            assert!(
                bounds.size.height - frame.height >= layout.natural_height,
                "{pass}: the content area {bounds:?} less {frame:?} holds {:?}",
                layout.natural_height
            );
            if pass == "the first" {
                assert_eq!(
                    bounds.top(),
                    owner.center().y - fitted / 2.,
                    "{pass}: opened as if it had no frame, grew below"
                );
            } else {
                assert_eq!(bounds.center(), owner.center(), "{pass}: centred");
            }
            let mut window = VisualTestContext::from_window(dialog, &main);
            press("escape", &mut window);
        }
        // Moved to a screen of another display scale, the window gets
        // another size from Windows, and GPUI then measures the frame
        // itself: the window fits its content again, with no frame added.
        let sized = Sized::px(300.);
        let dialog = sized.open(DialogOptions::new("Sized", 20.), &mut main);
        main.cx
            .update(|cx| {
                simulate_window_frame(cx, Some(Size::default()));
                dialog.update(cx, |_, window, cx| {
                    window.resize(size(width + px(20.), fitted + px(10.)));
                    window.bounds_changed(cx);
                })
            })
            .expect("the dialog is open");
        main.run_until_parked();
        assert_eq!(
            bounds_of(dialog, &mut main).size,
            size(width, fitted),
            "refitted, without a frame"
        );
        let mut window = VisualTestContext::from_window(dialog, &main);
        press("escape", &mut window);
        main.update(|_, cx| simulate_window_frame(cx, Some(frame)));
        // Taller than the screen: the frame stays inside it too.
        let tall = Sized::px(2000.);
        let dialog = tall.open(DialogOptions::new("Tall", 20.), &mut main);
        let bounds = bounds_of(dialog, &mut main);
        assert_eq!(bounds.top(), area.top() + SCREEN_MARGIN);
        assert_eq!(bounds.bottom(), room_bottom, "the frame included");
        let mut window = VisualTestContext::from_window(dialog, &main);
        press("escape", &mut window);
        // Room kept for a taller state keeps room for the frame below it.
        let reserve_content = 600.;
        let sized = Sized::px(200.).reserving(&[reserve_content]);
        let dialog = sized.open(DialogOptions::new("Sized", 20.), &mut main);
        let reserve = title_bar_height(rem) + PADDING * 3. + px(reserve_content) + rem * 2.;
        let opened = bounds_of(dialog, &mut main);
        let centred = owner.center().y - opened.size.height / 2.;
        assert!(
            centred > room_bottom - reserve - frame.height,
            "the test needs the reserve to move the window"
        );
        assert_eq!(opened.top(), room_bottom - reserve - frame.height);
        sized.set_height(reserve_content, dialog, &mut main);
        let grown = bounds_of(dialog, &mut main);
        assert_eq!(grown.origin, opened.origin, "did not move");
        assert_eq!(grown.size.height, reserve + frame.height, "grown to fit");
        assert_eq!(grown.bottom(), room_bottom, "and inside the work area");
    }

    /// The frame read off a Windows window, whose sizes are device pixels
    /// divided by the display scale, is the same to the bit whatever the
    /// window's size, so a window asking for its content plus that frame
    /// asks for the same size again after one resize: it settles.
    #[test]
    fn the_window_frame_is_the_same_at_every_window_size() {
        for scale in [1., 1.25, 1.5, 1.75, 2., 2.25] {
            for (fw, fh) in [(26, 13), (20, 10), (16, 8), (33, 17)] {
                let device = |pixels: i32| px(pixels as f32 / scale);
                let frame = size(device(fw), device(fh));
                for step in 0..400 {
                    // Content as tall and wide as a dialog's, in quarter
                    // pixels, as layouts give them.
                    let content =
                        size(px(300. + step as f32 * 0.25), px(150. + step as f32 * 1.75));
                    let asked = with_frame(content, frame);
                    let outer = asked.to_device_pixels(scale);
                    let outer_size = size(device(outer.width.0), device(outer.height.0));
                    let inner = size(device(outer.width.0 - fw), device(outer.height.0 - fh));
                    let read = frame_between(outer_size, inner, scale);
                    assert_eq!(
                        (
                            read.width.as_f32().to_bits(),
                            read.height.as_f32().to_bits()
                        ),
                        (
                            frame.width.as_f32().to_bits(),
                            frame.height.as_f32().to_bits()
                        ),
                        "at {scale} for {content:?}"
                    );
                    assert!(
                        same_device_size(asked, with_frame(content, read), scale),
                        "at {scale} for {content:?}: asks again for the same size"
                    );
                }
            }
        }
    }

    /// D24: a dialog that keeps room for a larger UI scale (the settings)
    /// opens where it fits at that scale too, and is fully shown at every
    /// scale up to it, wider and taller, without moving.
    #[gpui_kit::test]
    fn a_dialog_keeps_room_for_a_larger_ui_scale(cx: &mut TestAppContext) {
        let (_, mut main) = owner_window_at(WORK_AREAS[1], cx);
        main.update(|_, cx| simulate_work_area(cx, Some(WORK_AREAS[1])));
        let sized = Sized::rems(12.);
        let options = DialogOptions::new("Scaled", 30.).reserve_rem_size(px(32.));
        let dialog = sized.open(options, &mut main);
        assert_eq!(
            *sized.builds.borrow(),
            [Build::Shown, Build::Shown, Build::Shown]
        );
        let opened = bounds_of(dialog, &mut main);
        assert_fully_shown(dialog, "100%", &mut main);
        for font_size in [20., 24., 32.] {
            set_font_size(font_size, &mut main);
            let case = format!("{font_size} px");
            assert_fully_shown(dialog, &case, &mut main);
            let bounds = bounds_of(dialog, &mut main);
            assert_eq!(bounds.origin, opened.origin, "{case}: did not move");
            assert_eq!(bounds.size.width, px(30. * font_size), "{case}");
        }
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
                DialogOptions::new("Removable", 20.),
                move |_, window, cx| {
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
        // The last build is the dialog window's.
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

    /// A dialog that grows with the UI scale while it is open, without
    /// keeping room for it, stays on the screen: near its right edge the
    /// window gets no wider than the room left, as it gets no taller than
    /// the room below.
    #[gpui_kit::test]
    fn a_dialog_growing_with_the_scale_stays_on_the_screen(cx: &mut TestAppContext) {
        let (_, main) = owner_window(cx);
        // A second owner on the right of the 1920 × 1080 test screen.
        let bounds = Bounds {
            origin: point(px(1100.), px(100.)),
            size: size(px(800.), px(800.)),
        };
        let (right, _) = cx.update(|cx| second_owner(bounds, cx));
        let mut owner = VisualTestContext::from_window(right, &main);
        owner.run_until_parked();
        owner.update(|window, cx| {
            open(
                window,
                cx,
                DialogOptions::new("Wide", 0.).content_width(30.),
                |_, _, _| {
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
        set_font_size(32., &mut owner);
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
        let (right, _) = cx.update(|cx| second_owner(bounds, cx));
        let mut owner = VisualTestContext::from_window(right, &main);
        owner.run_until_parked();
        let (dialog, _) = open_test_dialog(&mut owner, true);
        let bounds = bounds_of(dialog.expect("the dialog opens"), &mut owner);
        let rem = px(16.);
        assert_eq!(bounds.size.width, rem * 12. + PADDING * 2., "its width");
        assert_eq!(bounds.right(), px(1920.) - SCREEN_MARGIN, "{bounds:?}");
    }
}
