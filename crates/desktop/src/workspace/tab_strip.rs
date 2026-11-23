//! Circuit tabs (Q23, Q35, D14): the kit's browser-style folder tabs. Click
//! to switch, double-click to rename, the close button deletes after
//! confirmation, and the + right after the last tab adds a circuit. The run
//! controls sit at the bar's right end (D20, [`super::run_controls`]).

use super::Workspace;
use crate::actions::*;
use crate::ui::testable;
use gpui_kit::component::{
    ActiveTheme, Disableable, IconName, Sizable, Size,
    button::{Button, ButtonVariants},
    input::Input,
    tab::{Tab, TabBar},
};
use gpui_kit::{prelude::*, *};

/// Interface size up to which the strip uses the kit's small tabs.
const SMALL_TABS_UP_TO: Pixels = px(16.5);

/// Height of a tab in root ems: the kit's small tab (24 px) at 100 %.
const TAB_HEIGHT_REMS: f32 = 1.5;

/// The kit's left padding of a folder tab's label box when the tab has a
/// prefix; the box stays empty here, as the title is the prefix.
const EMPTY_LABEL_PADDING: Pixels = px(4.);

/// The kit's folder-tab geometry at the size the strip uses (mirrors its
/// private `FolderTabMetrics`), with the tab height in rems.
///
/// The kit sizes its folder tabs in fixed pixels while their title and
/// buttons follow the interface scale, so above 100 % the title would be
/// clipped and the tallest button would lift the tabs off the content. The
/// strip therefore gives every tab a minimum height of
/// [`TAB_HEIGHT_REMS`] (the kit never sets one, so it wins over the kit's
/// fixed height) and shows the title in the tab's prefix, outside the
/// kit's fixed-height label box.
#[derive(Clone, Copy)]
struct FolderGeometry {
    size: Size,
    /// The kit's fixed tab height at [`Self::size`].
    kit_height: Pixels,
    /// Height of one tab at the current scale.
    tab_height: Pixels,
    /// Bar space above the tabs. Tab titles sit this much above the middle
    /// of the row the tabs and the + share.
    top_padding: Pixels,
    /// Padding on both sides inside a tab.
    padding_x: Pixels,
}

impl FolderGeometry {
    /// Small tabs up to 100 %, medium tabs (wider padding, rounder corners)
    /// above; either way at least [`TAB_HEIGHT_REMS`] high.
    fn for_rem_size(rem: Pixels) -> Self {
        let (size, kit_height, padding_x) = if rem <= SMALL_TABS_UP_TO {
            (Size::Small, px(24.), px(8.))
        } else {
            (Size::Medium, px(32.), px(12.))
        };
        Self {
            size,
            kit_height,
            tab_height: kit_height.max(rem * TAB_HEIGHT_REMS),
            top_padding: px(4.),
            padding_x,
        }
    }

    /// How far the selected tab's bottom curve reaches past its sides (the
    /// kit derives it from its own fixed height).
    fn curve(&self, cx: &App) -> Pixels {
        let radius = match self.size {
            Size::XSmall | Size::Small => cx.theme().radius,
            _ => cx.theme().radius_lg,
        };
        radius.min(self.kit_height / 3.)
    }
}

/// How far the active circuit's tab still has to be scrolled into view
/// (see [`Workspace::reveal_active_tab_once_laid_out`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TabReveal {
    /// Nothing to scroll.
    Done,
    /// It has to be scrolled into view.
    Pending,
    /// The + after the last tab was scrolled into view on the frame
    /// before; the tab follows.
    PlusShown,
}

/// Asks for one more frame from inside `render`. GPUI ignores a notify
/// while the window draws, so the notify waits until this frame is done.
fn render_again(cx: &mut Context<Workspace>) {
    let this = cx.weak_entity();
    cx.defer(move |cx| {
        this.update(cx, |_, cx| cx.notify()).ok();
    });
}

/// `content` (the title or the rename box) in a tab's prefix. The kit puts
/// its empty label box between the prefix and the close button, which adds
/// a gap and the box's padding; the negative margin takes both back, so the
/// close button sits the kit's 0.25 rem after the content, as after a
/// label.
fn tab_content(content: impl IntoElement, rem: Pixels) -> Div {
    div()
        .flex_shrink_0()
        .mr(-(rem * 0.25 + EMPTY_LABEL_PADDING))
        .child(content)
}

impl Workspace {
    /// One folder tab per circuit, then the + button.
    pub(super) fn render_tab_strip(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let rem = window.rem_size();
        let geometry = FolderGeometry::for_rem_size(rem);
        let tabs: Vec<Tab> = (0..self.tabs.len())
            .map(|index| self.circuit_tab(index, geometry, rem, cx))
            .collect();
        testable(
            "circuit-tabs",
            TabBar::new("circuit-tabs")
                .folder()
                .with_size(geometry.size)
                .selected_index(self.active_index())
                .track_scroll(&self.tab_scroll)
                .children(tabs)
                // The kit puts this slot inside the scrolling strip, right
                // after the last tab, but only for a bar with a suffix.
                .last_empty_space(self.add_button(geometry, rem, cx))
                // D20: Run and the analysis settings at the bar's right end.
                .suffix(self.render_run_controls(window, cx)),
        )
    }

    /// Scrolls the active circuit's tab into view after a project was shown
    /// or Run changed width. GPUI drops a scroll request made before the
    /// strip's first layout, so until then this asks for one more frame.
    /// The last circuit's tab shows with the + after it, as a new circuit
    /// does: GPUI applies one scroll request a frame, so the + is scrolled
    /// into view first and the tab on the next frame, which wins where the
    /// strip cannot hold both.
    pub(super) fn reveal_active_tab_once_laid_out(&mut self, cx: &mut Context<Self>) {
        if self.reveal_active_tab == TabReveal::Done {
            return;
        }
        if self.tab_scroll.children_count() == 0 {
            cx.notify();
            return;
        }
        let active = self.active_index();
        if active + 1 == self.tabs.len() && self.reveal_active_tab == TabReveal::Pending {
            self.reveal_active_tab = TabReveal::PlusShown;
            // The strip's children are the tabs and then the +.
            self.tab_scroll.scroll_to_item(self.tabs.len());
            render_again(cx);
            return;
        }
        self.reveal_active_tab = TabReveal::Done;
        self.tab_scroll.scroll_to_item(active);
    }

    /// The + button, like a browser's new-tab button.
    fn add_button(&self, geometry: FolderGeometry, rem: Pixels, cx: &mut Context<Self>) -> Div {
        div()
            .flex_shrink_0()
            // Clear of the selected tab's curve, which reaches past the tab.
            .pl(geometry.curve(cx))
            .pr(rem * 0.25)
            // Level with the tab titles, which sit above the row's middle.
            .pb(geometry.top_padding)
            .child(testable(
                "add-circuit",
                Button::new("add-circuit")
                    .xsmall()
                    .ghost()
                    .icon(IconName::Plus)
                    .disabled(self.doc.read_only)
                    .tooltip_with_action("New circuit", &NewCircuit, Some(WORKSPACE_CONTEXT))
                    .on_click(cx.listener(|this, _, window, cx| this.add_circuit(window, cx))),
            ))
    }

    /// One tab: its name (or the rename box) and a delete button.
    fn circuit_tab(
        &self,
        index: usize,
        geometry: FolderGeometry,
        rem: Pixels,
        cx: &mut Context<Self>,
    ) -> Tab {
        let tab = &self.tabs[index];
        let name = self.doc.project.circuits[index].name.clone();
        let renaming = self.renaming.as_ref().filter(|r| r.tab == tab.id);
        let base = Tab::new()
            .min_h(geometry.tab_height)
            .aria_label(name.clone())
            .debug_selector(move || format!("circuit-tab-{index}"))
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                // Clicks on the tab that holds the rename box belong to the
                // box: they move its caret or select a word, and must not
                // take its focus or start the rename over.
                if this.renaming_index() == Some(index) {
                    return;
                }
                if event.click_count() >= 2 {
                    this.start_rename(index, window, cx);
                } else {
                    this.switch_to(index, window, cx);
                }
            }));
        // Both in the prefix: the kit's label box has a fixed pixel height
        // and clips what it holds, the rename box and, above 100 %, the
        // title too.
        let content = match renaming {
            Some(rename) => div()
                .debug_selector(|| "circuit-rename".into())
                .w(rems(9.))
                .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    if event.keystroke.key == "escape" {
                        this.cancel_rename(window, cx);
                    }
                }))
                .child(Input::new(&rename.input).xsmall()),
            None => div()
                .debug_selector(move || format!("circuit-title-{index}"))
                // The kit's label line height.
                .line_height(relative(1.25))
                .whitespace_nowrap()
                .child(name),
        };
        let base = base.prefix(tab_content(content, rem));
        if self.tabs.len() > 1 && !self.doc.read_only {
            base.suffix(self.delete_button(index, geometry, rem, cx))
        } else {
            base
        }
    }

    /// The close (X) button. The kit leaves a 0.25 rem gap before a tab's
    /// suffix but its full padding after it, so the button is pulled right
    /// until the space after it equals the gap before it: the X then sits
    /// evenly between the title and the tab's edge at every scale.
    fn delete_button(
        &self,
        index: usize,
        geometry: FolderGeometry,
        rem: Pixels,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        Button::new(("delete-circuit", index))
            .xsmall()
            .ghost()
            .icon(IconName::Close)
            .tooltip("Delete circuit")
            .mr(rem * 0.25 - geometry.padding_x)
            .debug_selector(move || format!("delete-circuit-{index}"))
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.delete_circuit(index, window, cx);
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::Workspace;
    use crate::actions::{NewCircuit, primary};
    use crate::test_support::{TestEnv, click, close, double_click, init, open};
    use gpui_kit::{
        Bounds, Entity, Focusable, Pixels, ScaledPixels, TestAppContext, VisualTestContext, px,
    };
    use xsim_project::{CircuitDocument, Project};

    fn bounds(selector: String, cx: &mut VisualTestContext) -> Bounds<Pixels> {
        let selector: &'static str = Box::leak(selector.into_boxed_str());
        cx.debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} is not on screen"))
    }

    fn tab(index: usize, cx: &mut VisualTestContext) -> Bounds<Pixels> {
        bounds(format!("circuit-tab-{index}"), cx)
    }

    fn close_button(index: usize, cx: &mut VisualTestContext) -> Bounds<Pixels> {
        bounds(format!("delete-circuit-{index}"), cx)
    }

    fn plus(cx: &mut VisualTestContext) -> Bounds<Pixels> {
        bounds("add-circuit".into(), cx)
    }

    fn strip(cx: &mut VisualTestContext) -> Bounds<Pixels> {
        bounds("circuit-tabs".into(), cx)
    }

    fn rem(cx: &mut VisualTestContext) -> Pixels {
        cx.update(|window, _| window.rem_size())
    }

    fn assert_near(actual: Pixels, expected: Pixels, what: &str) {
        assert!(
            (actual - expected).abs() <= px(0.5),
            "{what}: {actual:?}, expected {expected:?}"
        );
    }

    fn add_circuits(count: usize, cx: &mut VisualTestContext) {
        for _ in 0..count {
            cx.dispatch_action(NewCircuit);
        }
        cx.run_until_parked();
    }

    /// D14: the kit's folder tabs, small at 100 %: 24 px tabs under 4 px of
    /// bar, 4 px apart.
    #[gpui_kit::test]
    fn circuit_tabs_use_the_folder_design(cx: &mut TestAppContext) {
        init(cx);
        let env = TestEnv::new();
        let (_, mut window) = open(cx, &env, None);
        add_circuits(1, &mut window);
        let (first, second, bar) = (tab(0, &mut window), tab(1, &mut window), strip(&mut window));
        assert_eq!(first.size.height, px(24.));
        assert_eq!(bar.size.height, px(28.));
        assert_eq!(
            first.bottom(),
            bar.bottom(),
            "tabs stand on the bar's bottom"
        );
        assert_eq!(second.left() - first.right(), px(4.));
        close(&mut window);
    }

    /// D14: the + follows the last tab inside the strip, level with the
    /// titles, like a browser's new-tab button, and moves along when a
    /// circuit is added.
    #[gpui_kit::test]
    fn the_plus_button_follows_the_last_tab(cx: &mut TestAppContext) {
        init(cx);
        let env = TestEnv::new();
        let (workspace, mut window) = open(cx, &env, None);
        let only = tab(0, &mut window);
        let button = plus(&mut window);
        let gap = button.left() - only.right();
        assert!(
            gap > px(0.) && gap <= px(16.),
            "the + starts {gap:?} after the tab"
        );
        assert!(
            button.right() < strip(&mut window).center().x,
            "the + is not pushed to the end of the bar"
        );
        assert_near(button.center().y, only.center().y - px(2.), "+ level");

        add_circuits(1, &mut window);
        let last = tab(1, &mut window);
        let moved = plus(&mut window);
        assert_near(
            moved.left() - last.right(),
            gap,
            "same gap after the new tab",
        );
        assert_near(
            moved.center().y,
            close_button(1, &mut window).center().y,
            "+ level with the close buttons",
        );

        click_plus(&mut window);
        assert_eq!(workspace.read_with(&window, |w, _| w.tabs.len()), 3);
        assert!(tab(2, &mut window).left() < plus(&mut window).left());
        close(&mut window);
    }

    fn click_plus(cx: &mut VisualTestContext) {
        crate::test_support::click("add-circuit", cx);
    }

    fn title(index: usize, cx: &mut VisualTestContext) -> Bounds<Pixels> {
        bounds(format!("circuit-title-{index}"), cx)
    }

    fn set_scale(scale: f32, workspace: &Entity<Workspace>, cx: &mut VisualTestContext) {
        workspace.update_in(cx, |w, window, cx| w.set_ui_scale(scale, window, cx));
        cx.run_until_parked();
    }

    /// Every interface scale the settings offer.
    const SCALES: [f32; 8] = crate::appearance::SCALE_PRESETS;

    /// D14: the X sits as far from the tab's edge as from the title, inside
    /// the tab, at every scale.
    #[gpui_kit::test]
    fn the_close_button_has_balanced_margins(cx: &mut TestAppContext) {
        init(cx);
        let env = TestEnv::new();
        let (workspace, mut window) = open(cx, &env, None);
        add_circuits(1, &mut window);
        for scale in SCALES {
            set_scale(scale, &workspace, &mut window);
            let gap = rem(&mut window) * 0.25;
            for index in 0..2 {
                let (tab, close) = (tab(index, &mut window), close_button(index, &mut window));
                assert_near(
                    tab.right() - close.right(),
                    gap,
                    &format!("space after the X at {scale}"),
                );
                assert_near(
                    close.left() - title(index, &mut window).right(),
                    gap,
                    &format!("space before the X at {scale}"),
                );
                assert!(
                    close.top() >= tab.top() && close.bottom() <= tab.bottom(),
                    "the X fits in the tab at {scale}: {close:?} in {tab:?}"
                );
            }
            assert_near(
                plus(&mut window).center().y,
                close_button(1, &mut window).center().y,
                &format!("+ level with the X at {scale}"),
            );
        }
        close(&mut window);
    }

    /// A tab without a close button (the only circuit) pads its title
    /// evenly on both sides, as the kit pads a label.
    #[gpui_kit::test]
    fn a_lone_tab_pads_its_title_evenly(cx: &mut TestAppContext) {
        init(cx);
        let env = TestEnv::new();
        let (workspace, mut window) = open(cx, &env, None);
        for scale in [1.0, 2.0] {
            set_scale(scale, &workspace, &mut window);
            assert!(window.debug_bounds("delete-circuit-0").is_none());
            let (tab, title) = (tab(0, &mut window), title(0, &mut window));
            assert_near(
                tab.right() - title.right(),
                title.left() - tab.left(),
                &format!("padding at {scale}"),
            );
        }
        close(&mut window);
    }

    /// The tabs grow with the interface (the kit's are fixed pixels): at
    /// every scale the title is whole inside its tab, the title, X and +
    /// share one centre line (the middle of the bar), and every tab stands
    /// on the bar's bottom, so the selected one joins the content below.
    #[gpui_kit::test]
    fn the_strip_scales_with_the_interface(cx: &mut TestAppContext) {
        init(cx);
        let env = TestEnv::new();
        let (workspace, mut window) = open(cx, &env, None);
        add_circuits(2, &mut window);
        // A tab that is neither first nor last is selected.
        click("circuit-tab-1", &mut window);
        for scale in SCALES {
            set_scale(scale, &workspace, &mut window);
            let rem = rem(&mut window);
            let (bar, plus) = (strip(&mut window), plus(&mut window));
            let middle = bar.center().y;
            assert_near(plus.center().y, middle, &format!("+ centred at {scale}"));
            for index in 0..3 {
                let (tab, title, close) = (
                    tab(index, &mut window),
                    title(index, &mut window),
                    close_button(index, &mut window),
                );
                let what = |part: &str| format!("{part} of tab {index} at {scale}");
                assert_near(tab.bottom(), bar.bottom(), &what("bottom"));
                assert!(
                    tab.size.height >= rem * 1.5 - px(0.5),
                    "{}: {tab:?}",
                    what("height")
                );
                assert_near(
                    bar.size.height - tab.size.height,
                    px(4.),
                    &what("bar above"),
                );
                assert!(
                    title.top() >= tab.top() && title.bottom() <= tab.bottom(),
                    "{}: {title:?} in {tab:?}",
                    what("title")
                );
                // A whole line of small text (0.875 rem at 1.25 lines).
                assert!(
                    title.size.height >= rem * 0.875 * 1.25 - px(0.5),
                    "{}: {title:?}",
                    what("title line")
                );
                assert_near(title.center().y, middle, &what("title centre"));
                assert_near(close.center().y, middle, &what("X centre"));
            }
        }
        close(&mut window);
    }

    /// The kit's separator between two unselected tabs, in window pixels:
    /// the one-pixel-wide quad painted in the gap between `left` and `right`.
    fn separator_between(
        left: Bounds<Pixels>,
        right: Bounds<Pixels>,
        cx: &mut VisualTestContext,
    ) -> Option<Bounds<Pixels>> {
        let (scale, quads) = cx.update(|window, _| (window.scale_factor(), window.painted_quads()));
        quads
            .iter()
            .map(|quad| {
                let b = quad.bounds;
                Bounds::new(
                    gpui_kit::point(px(b.origin.x.0 / scale), px(b.origin.y.0 / scale)),
                    gpui_kit::size(px(b.size.width.0 / scale), px(b.size.height.0 / scale)),
                )
            })
            .find(|b| {
                (b.size.width - px(1.)).abs() <= px(0.1)
                    && b.left() >= left.right() - px(0.5)
                    && b.right() <= right.left() + px(0.5)
                    && b.top() >= left.top()
                    && b.bottom() <= left.bottom()
            })
    }

    /// The separator between two unselected tabs is centred on their titles
    /// up to 125 %. Above, the kit places it from its own fixed tab height,
    /// which the strip cannot reach: measured 2 px (150 %), 4.75 px (175 %)
    /// and 8 px (200 %) above the titles' centre. It still stays beside the
    /// titles, inside their line.
    #[gpui_kit::test]
    fn the_separators_sit_beside_the_titles(cx: &mut TestAppContext) {
        init(cx);
        let env = TestEnv::new();
        let (workspace, mut window) = open(cx, &env, None);
        // Circuit 3 is selected: the only separator is between 1 and 2.
        add_circuits(2, &mut window);
        for scale in SCALES {
            set_scale(scale, &workspace, &mut window);
            let (first, second) = (tab(0, &mut window), tab(1, &mut window));
            let separator = separator_between(first, second, &mut window)
                .unwrap_or_else(|| panic!("no separator between the tabs at {scale}"));
            let title = title(0, &mut window);
            if scale <= 1.25 {
                assert_near(
                    separator.center().y,
                    title.center().y,
                    &format!("separator centre at {scale}"),
                );
            }
            assert!(
                separator.top() >= title.top() && separator.bottom() <= title.bottom(),
                "separator beside the title at {scale}: {separator:?}, {title:?}"
            );
            assert!(
                title.center().y - separator.center().y <= px(8.5),
                "separator offset at {scale}: {separator:?}, {title:?}"
            );
        }
        close(&mut window);
    }

    /// The rename box sits in the tab without being clipped.
    #[gpui_kit::test]
    fn the_rename_box_fits_in_the_tab(cx: &mut TestAppContext) {
        init(cx);
        let env = TestEnv::new();
        let (workspace, mut window) = open(cx, &env, None);
        add_circuits(1, &mut window);
        // Every preset, ending at 100 %, where the paint is checked below.
        let mut scales: Vec<f32> = SCALES.into_iter().filter(|&s| s != 1.0).collect();
        scales.push(1.0);
        for scale in scales {
            set_scale(scale, &workspace, &mut window);
            double_click("circuit-tab-1", &mut window);
            assert!(workspace.read_with(&window, |w, _| w.renaming.is_some()));
            let (tab, input, close) = (
                tab(1, &mut window),
                bounds("circuit-rename".into(), &mut window),
                close_button(1, &mut window),
            );
            assert!(
                input.top() >= tab.top() && input.bottom() <= tab.bottom(),
                "{input:?} in {tab:?} at {scale}"
            );
            // The same gap as after a title, not three times as much.
            assert_near(
                close.left() - input.right(),
                rem(&mut window) * 0.25,
                &format!("rename box to X at {scale}"),
            );
            if scale != 1.0 {
                window.simulate_keystrokes("escape");
                window.run_until_parked();
            }
        }
        let input = bounds("circuit-rename".into(), &mut window);
        // The input's bordered box is painted whole, not cut by a clip: the
        // clips it is painted under (the focus ring paints its sides
        // separately) together reach its top and bottom edges.
        let (scale, quads) =
            window.update(|window, _| (window.scale_factor(), window.painted_quads()));
        let input = input.scale(scale);
        let masks: Vec<_> = quads
            .iter()
            .filter(|quad| quad.bounds == input && quad.border_widths.top > ScaledPixels::from(0.))
            .map(|quad| quad.content_mask.bounds)
            .collect();
        assert!(!masks.is_empty(), "the input's border is painted");
        assert!(
            masks.iter().any(|mask| mask.top() <= input.top()),
            "the input's top edge is clipped: {masks:?}"
        );
        assert!(
            masks.iter().any(|mask| mask.bottom() >= input.bottom()),
            "the input's bottom edge is clipped: {masks:?}"
        );
        close(&mut window);
    }

    /// With more circuits than fit, the strip scrolls; a new circuit and
    /// the + are scrolled into view.
    #[gpui_kit::test]
    fn a_new_circuit_is_scrolled_into_view(cx: &mut TestAppContext) {
        init(cx);
        let env = TestEnv::new();
        let (workspace, mut window) = open(cx, &env, None);
        add_circuits(29, &mut window);
        let count = workspace.read_with(&window, |w, _| w.tabs.len());
        assert_eq!(count, 30);
        let bar = strip(&mut window);
        assert!(
            tab(0, &mut window).right() < bar.left(),
            "the first tabs scrolled out"
        );
        // Shown left of the run controls at the bar's right end (D20).
        let end = bounds("run-controls".into(), &mut window).left();
        for shown in [tab(count - 1, &mut window), plus(&mut window)] {
            assert!(
                shown.left() >= bar.left() && shown.right() <= end,
                "{shown:?} inside {bar:?}, before {end:?}"
            );
        }
        assert_eq!(
            workspace.read_with(&window, |w, _| w.active_index()),
            count - 1
        );
        close(&mut window);
    }

    /// Starts renaming circuit `index` and types `name` over its old name.
    fn type_new_name(index: usize, name: &str, cx: &mut VisualTestContext) {
        let selector: &'static str = Box::leak(format!("circuit-tab-{index}").into_boxed_str());
        double_click(selector, cx);
        cx.simulate_keystrokes(primary!("a"));
        cx.simulate_input(name);
        cx.run_until_parked();
    }

    fn names(workspace: &Entity<Workspace>, cx: &VisualTestContext) -> Vec<String> {
        workspace.read_with(cx, |w, _| {
            w.doc
                .project
                .circuits
                .iter()
                .map(|c| c.name.clone())
                .collect()
        })
    }

    /// Clicks in the rename box move the caret or select a word; they
    /// neither end the rename nor start it over.
    #[gpui_kit::test]
    fn clicks_in_the_rename_box_keep_the_typed_name(cx: &mut TestAppContext) {
        init(cx);
        let env = TestEnv::new();
        let (workspace, mut window) = open(cx, &env, None);
        add_circuits(1, &mut window);
        type_new_name(1, "Filter", &mut window);
        // The typed text, while the rename box keeps the keyboard.
        let typed = |window: &mut VisualTestContext| {
            workspace.update_in(window, |w, window, cx| {
                w.renaming
                    .as_ref()
                    .filter(|rename| rename.input.focus_handle(cx).is_focused(window))
                    .map(|rename| rename.input.read(cx).value().to_string())
            })
        };
        click("circuit-rename", &mut window);
        assert_eq!(
            typed(&mut window).as_deref(),
            Some("Filter"),
            "after a click"
        );
        double_click("circuit-rename", &mut window);
        assert_eq!(
            typed(&mut window).as_deref(),
            Some("Filter"),
            "after a double-click"
        );
        window.simulate_keystrokes("enter");
        assert_eq!(names(&workspace, &window), ["Circuit 1", "Filter"]);
        close(&mut window);
    }

    /// Escape abandons the rename and hands the keyboard back to the
    /// schematic.
    #[gpui_kit::test]
    fn escape_abandons_a_rename(cx: &mut TestAppContext) {
        init(cx);
        let env = TestEnv::new();
        let (workspace, mut window) = open(cx, &env, None);
        add_circuits(1, &mut window);
        type_new_name(1, "Filter", &mut window);
        window.simulate_keystrokes("escape");
        assert_eq!(names(&workspace, &window), ["Circuit 1", "Circuit 2"]);
        workspace.update_in(&mut window, |w, window, cx| {
            assert!(w.renaming.is_none());
            assert!(w.active_tab().editor.focus_handle(cx).is_focused(window));
        });
        close(&mut window);
    }

    /// Leaving the rename box for another tab, the + or Ctrl+T applies the
    /// typed name, as clicking into the schematic does.
    #[gpui_kit::test]
    fn switching_circuits_applies_a_typed_name(cx: &mut TestAppContext) {
        init(cx);
        let env = TestEnv::new();
        let (workspace, mut window) = open(cx, &env, None);
        add_circuits(1, &mut window);

        type_new_name(1, "Filter", &mut window);
        click("circuit-tab-0", &mut window);
        assert_eq!(names(&workspace, &window), ["Circuit 1", "Filter"]);
        assert_eq!(workspace.read_with(&window, |w, _| w.active_index()), 0);

        type_new_name(0, "Amplifier", &mut window);
        click_plus(&mut window);
        let shown = names(&workspace, &window);
        assert_eq!(shown.len(), 3);
        assert_eq!(shown[..2], ["Amplifier", "Filter"]);

        type_new_name(2, "Output", &mut window);
        window.simulate_keystrokes(primary!("t"));
        window.run_until_parked();
        let shown = names(&workspace, &window);
        assert_eq!((shown.len(), shown[2].as_str()), (4, "Output"));
        assert!(workspace.read_with(&window, |w, _| w.renaming.is_none()));
        close(&mut window);
    }

    /// The close button deletes its own circuit after confirmation and
    /// leaves the shown circuit alone.
    #[gpui_kit::test]
    fn the_close_button_asks_before_deleting_its_circuit(cx: &mut TestAppContext) {
        init(cx);
        let env = TestEnv::new();
        let (workspace, mut window) = open(cx, &env, None);
        add_circuits(2, &mut window);
        let active =
            |window: &VisualTestContext| workspace.read_with(window, |w, _| w.active_index());
        assert_eq!(active(&window), 2);

        env.prompts.delete.borrow_mut().push_back(false);
        click("delete-circuit-0", &mut window);
        assert_eq!(names(&workspace, &window).len(), 3, "declined");
        assert_eq!(active(&window), 2, "the click did not switch circuits");

        env.prompts.delete.borrow_mut().push_back(true);
        click("delete-circuit-0", &mut window);
        assert_eq!(names(&workspace, &window), ["Circuit 2", "Circuit 3"]);
        assert_eq!(active(&window), 1, "Circuit 3 is still shown");
        close(&mut window);
    }

    /// A project opened with its circuit far along the strip shows that
    /// tab, wherever the previous project left the strip scrolled.
    #[gpui_kit::test]
    fn an_opened_project_shows_its_active_circuit(cx: &mut TestAppContext) {
        init(cx);
        let env = TestEnv::new();
        let mut project = Project::new("Many circuits");
        for _ in 1..30 {
            let name = project.next_circuit_name();
            project.circuits.push(CircuitDocument::new(name));
        }
        project.active_circuit = 29;
        let late = env.write_project("late.xsim_proj", &project);
        project.active_circuit = 0;
        let early = env.write_project("early.xsim_proj", &project);

        let (workspace, mut window) = open(cx, &env, Some(&late));
        for index in [29, 0] {
            let (shown, bar) = (tab(index, &mut window), strip(&mut window));
            let end = bounds("run-controls".into(), &mut window).left();
            assert!(
                shown.left() >= bar.left() && shown.right() <= end,
                "circuit {index}: {shown:?} inside {bar:?}, before {end:?}"
            );
            let path = early.clone();
            workspace.update_in(&mut window, |w, window, cx| w.load_path(path, window, cx));
            window.run_until_parked();
        }
        close(&mut window);
    }

    /// Read-only projects show no close buttons and a disabled +.
    #[gpui_kit::test]
    fn read_only_tabs_cannot_be_changed(cx: &mut TestAppContext) {
        init(cx);
        let env = TestEnv::new();
        let (workspace, mut window) = open(cx, &env, None);
        add_circuits(1, &mut window);
        workspace.update(&mut window, |w, cx| {
            w.doc.read_only = true;
            cx.notify();
        });
        window.run_until_parked();
        assert!(window.debug_bounds("delete-circuit-0").is_none());
        click_plus(&mut window);
        assert_eq!(workspace.read_with(&window, |w, _| w.tabs.len()), 2);
        close(&mut window);
    }
}
