//! Headless UI tests of the component library window: search across every
//! category (D12), categories, keyboard navigation, picking and family
//! filters.
//!
//! The library is a window of its own (D22). The helpers keep the test's
//! `VisualTestContext` on the window that has the keyboard: the library, or
//! the host window once it closed.
//!
//! They locate controls with debug selectors, which only debug builds record.
#![cfg(debug_assertions)]

use gpui_kit::component::Theme;
use gpui_kit::{
    AnyWindowHandle, AppContext as _, Bounds, Context, Entity, Focusable as _, IntoElement,
    KeyDownEvent, KeyUpEvent, Keystroke, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent,
    ParentElement as _, Pixels, Point, Render, Styled as _, TestAppContext, VisualContext as _,
    VisualTestContext, Window, WindowBounds, WindowOptions, div, point, px, size,
};
use std::{cell::RefCell, rc::Rc};
use xsim_editor::library_browser::{LibraryFilter, LibraryItem, open_library, open_library_at};
use xsim_editor::{SchematicEditor, Tool, dialog_window};
use xsim_project::{PartKind, Schematic, SchematicView};
use xsim_spice_engine::{Polarity, library};

/// An empty window content for the dialog to sit on.
struct Host;

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child("host")
    }
}

type Picks = Rc<RefCell<Vec<LibraryItem>>>;

fn host_window(cx: &mut TestAppContext) -> VisualTestContext {
    let (window, _) = cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_reduce_motion(true);
        dialog_window::init(cx);
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Point::default(),
                    size(px(1280.), px(900.)),
                ))),
                ..WindowOptions::default()
            },
            cx,
            |_, cx| cx.new(|_| Host),
        )
        .expect("test window")
    });
    let visual = VisualTestContext::from_window(window, cx);
    visual.run_until_parked();
    visual
}

/// The host window: the one that is no dialog.
fn host(cx: &VisualTestContext) -> AnyWindowHandle {
    cx.cx
        .read(|app| {
            app.windows()
                .into_iter()
                .find(|window| dialog_window::owner_of(*window, app).is_none())
        })
        .expect("the host window")
}

/// The library's window, while it is open.
fn library(cx: &VisualTestContext) -> Option<AnyWindowHandle> {
    let host = host(cx);
    cx.cx.read(|app| dialog_window::dialog_of(host, app))
}

/// Points `cx` at `window`.
fn switch_to(window: AnyWindowHandle, cx: &mut VisualTestContext) {
    if cx.window_handle() != window {
        let next = VisualTestContext::from_window(window, &cx.cx);
        *cx = next;
    }
}

/// Points `cx` at the window with the keyboard: the library while it is
/// open, the host window after.
fn follow(cx: &mut VisualTestContext) {
    let top = library(cx).unwrap_or_else(|| host(cx));
    switch_to(top, cx);
}

/// Types the space-separated `keystrokes` in the window with the keyboard.
fn keys(keystrokes: &str, cx: &mut VisualTestContext) {
    follow(cx);
    cx.simulate_keystrokes(keystrokes);
    cx.run_until_parked();
    follow(cx);
}

/// Opens the library over the host window with `filter`, points `cx` at
/// it and records every pick.
fn open(filter: LibraryFilter, selected: Option<LibraryItem>, cx: &mut VisualTestContext) -> Picks {
    let picks = Picks::default();
    let record = picks.clone();
    let host = host(cx);
    switch_to(host, cx);
    cx.update(|window, cx| {
        open_library_at(
            filter,
            selected,
            move |item, _, _| record.borrow_mut().push(item),
            window,
            cx,
        )
    });
    cx.run_until_parked();
    follow(cx);
    picks
}

fn is_shown(selector: &'static str, cx: &mut VisualTestContext) -> bool {
    follow(cx);
    cx.debug_bounds(selector).is_some()
}

fn click_with_count(selector: &'static str, clicks: usize, cx: &mut VisualTestContext) {
    follow(cx);
    let position = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} should be visible"))
        .center();
    cx.simulate_mouse_move(position, None, Modifiers::default());
    cx.simulate_event(MouseDownEvent {
        position,
        modifiers: Modifiers::default(),
        button: MouseButton::Left,
        click_count: clicks,
        first_mouse: false,
    });
    cx.simulate_event(MouseUpEvent {
        position,
        modifiers: Modifiers::default(),
        button: MouseButton::Left,
        click_count: clicks,
    });
    cx.run_until_parked();
    follow(cx);
}

fn click(selector: &'static str, cx: &mut VisualTestContext) {
    click_with_count(selector, 1, cx);
}

fn type_query(text: &str, cx: &mut VisualTestContext) {
    follow(cx);
    cx.simulate_input(text);
    cx.run_until_parked();
}

/// Deletes the last `chars` characters of the query.
fn erase(chars: usize, cx: &mut VisualTestContext) {
    keys(&vec!["backspace"; chars].join(" "), cx);
}

/// Presses and releases each of the space-separated `keys`, as a keyboard
/// does: a focused button is pressed on the release. A press that closes
/// the library leaves the release nowhere, as it is released in another
/// window.
fn press(keys: &str, cx: &mut VisualTestContext) {
    for key in keys.split(' ') {
        follow(cx);
        let keystroke = cx
            .update(|_, cx| Keystroke::parse_with_os(key, cx.platform_info().os))
            .unwrap_or_else(|_| panic!("{key} is not a key"));
        cx.simulate_event(KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        if cx.windows().contains(&cx.window_handle()) {
            cx.simulate_event(KeyUpEvent { keystroke });
        }
        cx.run_until_parked();
    }
    follow(cx);
}

/// A debug selector built at run time.
fn selector(text: String) -> &'static str {
    text.leak()
}

fn bjt(polarity: Polarity, model: &str) -> LibraryItem {
    LibraryItem::Bjt {
        polarity,
        model: model.into(),
    }
}

#[gpui_kit::test]
fn the_window_shows_search_tree_list_and_preview(cx: &mut TestAppContext) {
    let mut cx = host_window(cx);
    open(LibraryFilter::All, None, &mut cx);
    for selector in [
        "library-search",
        "library-category-All parts",
        "library-category-Basic parts",
        "library-category-Sources",
        "library-category-Switches",
        "library-category-Diodes",
        "library-category-Bipolar transistors",
        "library-category-JFETs",
        "library-item-Resistor",
        "library-details",
        "library-preview",
        "library-pick",
    ] {
        assert!(is_shown(selector, &mut cx), "{selector}");
    }
}

#[gpui_kit::test]
fn searching_narrows_the_list_and_enter_picks_the_best_match(cx: &mut TestAppContext) {
    let mut cx = host_window(cx);
    let picks = open(LibraryFilter::All, None, &mut cx);
    type_query("capac", &mut cx);
    assert!(is_shown("library-item-Capacitor", &mut cx));
    assert!(!is_shown("library-item-Resistor", &mut cx));
    keys("enter", &mut cx);
    assert_eq!(*picks.borrow(), [LibraryItem::Part(PartKind::Capacitor)]);
    assert!(
        !is_shown("library-search", &mut cx),
        "picking closes the window"
    );
}

#[gpui_kit::test]
fn up_and_down_move_the_selection_from_the_search_field(cx: &mut TestAppContext) {
    let mut cx = host_window(cx);
    let picks = open(LibraryFilter::All, None, &mut cx);
    cx.simulate_keystrokes("down down down up");
    keys("enter", &mut cx);
    assert_eq!(*picks.borrow(), [LibraryItem::Part(PartKind::Inductor)]);
}

#[gpui_kit::test]
fn double_clicking_an_item_picks_it(cx: &mut TestAppContext) {
    let mut cx = host_window(cx);
    let picks = open(LibraryFilter::All, None, &mut cx);
    click("library-item-Ground", &mut cx);
    assert!(picks.borrow().is_empty(), "a single click only selects");
    assert!(is_shown("library-search", &mut cx));
    click_with_count("library-item-Ground", 2, &mut cx);
    assert_eq!(*picks.borrow(), [LibraryItem::Part(PartKind::Ground)]);
    assert!(!is_shown("library-search", &mut cx));
}

#[gpui_kit::test]
fn the_place_button_picks_the_selection(cx: &mut TestAppContext) {
    let mut cx = host_window(cx);
    let picks = open(LibraryFilter::All, None, &mut cx);
    click("library-item-Voltage source", &mut cx);
    click("library-pick", &mut cx);
    assert_eq!(
        *picks.borrow(),
        [LibraryItem::Part(PartKind::VoltageSource)]
    );
}

/// D17: the Switches category lists both switches, a search finds them
/// from every category (D12) and by their SPICE model names, and picking
/// one returns it.
#[gpui_kit::test]
fn the_switches_are_listed_searchable_and_picked(cx: &mut TestAppContext) {
    let mut cx = host_window(cx);
    let picks = open(LibraryFilter::All, None, &mut cx);
    click("library-category-Switches", &mut cx);
    assert!(is_shown("library-selected-category-Switches", &mut cx));
    assert!(is_shown("library-item-Voltage-controlled switch", &mut cx));
    assert!(is_shown("library-item-Current-controlled switch", &mut cx));
    assert!(!is_shown("library-item-Voltage source", &mut cx));
    click("library-item-Current-controlled switch", &mut cx);
    assert!(is_shown("library-preview", &mut cx));
    click("library-pick", &mut cx);
    assert_eq!(
        *picks.borrow(),
        [LibraryItem::Part(PartKind::CurrentSwitch)]
    );

    let picks = open(LibraryFilter::All, None, &mut cx);
    click("library-category-Diodes", &mut cx);
    type_query("voltage-controlled", &mut cx);
    assert!(
        is_shown("library-selected-category-All parts", &mut cx),
        "typing searches every category"
    );
    assert!(is_shown(
        "library-item-category-Voltage-controlled switch",
        &mut cx
    ));
    press("enter", &mut cx);
    assert_eq!(
        *picks.borrow(),
        [LibraryItem::Part(PartKind::VoltageSwitch)]
    );

    let picks = open(LibraryFilter::All, None, &mut cx);
    type_query("csw", &mut cx);
    assert!(is_shown("library-item-Current-controlled switch", &mut cx));
    assert!(!is_shown("library-item-Voltage-controlled switch", &mut cx));
    press("enter", &mut cx);
    assert_eq!(
        *picks.borrow(),
        [LibraryItem::Part(PartKind::CurrentSwitch)]
    );
}

#[gpui_kit::test]
fn escape_and_cancel_close_without_picking(cx: &mut TestAppContext) {
    let mut cx = host_window(cx);
    let picks = open(LibraryFilter::All, None, &mut cx);
    keys("escape", &mut cx);
    assert!(!is_shown("library-search", &mut cx));
    let more = open(LibraryFilter::All, None, &mut cx);
    click("library-cancel", &mut cx);
    assert!(!is_shown("library-search", &mut cx));
    let searching = open(LibraryFilter::All, None, &mut cx);
    type_query("res", &mut cx);
    press("escape", &mut cx);
    assert!(
        !is_shown("library-search", &mut cx),
        "escape closes while searching too"
    );
    assert!(picks.borrow().is_empty() && more.borrow().is_empty());
    assert!(searching.borrow().is_empty());
}

#[gpui_kit::test]
fn categories_restrict_the_list(cx: &mut TestAppContext) {
    let mut cx = host_window(cx);
    open(LibraryFilter::All, None, &mut cx);
    click("library-category-Sources", &mut cx);
    assert!(is_shown("library-item-Voltage source", &mut cx));
    assert!(is_shown("library-item-Current source", &mut cx));
    assert!(!is_shown("library-item-Resistor", &mut cx));
    click("library-category-All parts", &mut cx);
    assert!(is_shown("library-item-Resistor", &mut cx));
}

#[gpui_kit::test]
fn typing_searches_every_category_and_clearing_returns_to_the_selected_one(
    cx: &mut TestAppContext,
) {
    let mut cx = host_window(cx);
    let picks = open(LibraryFilter::All, None, &mut cx);
    click("library-category-Sources", &mut cx);
    assert!(is_shown("library-selected-category-Sources", &mut cx));
    assert!(!is_shown("library-item-Resistor", &mut cx));
    assert!(
        !is_shown("library-item-category-Voltage source", &mut cx),
        "browsing shows no category labels"
    );

    type_query("resistor", &mut cx);
    assert!(
        is_shown("library-item-Resistor", &mut cx),
        "found outside Sources"
    );
    assert!(is_shown("library-item-category-Resistor", &mut cx));
    assert!(is_shown("library-selected-category-All parts", &mut cx));
    assert!(!is_shown("library-no-results", &mut cx));
    assert!(
        is_shown("library-preview", &mut cx),
        "the best match is previewed"
    );

    erase("resistor".len(), &mut cx);
    assert!(is_shown("library-selected-category-Sources", &mut cx));
    assert!(is_shown("library-item-Voltage source", &mut cx));
    assert!(!is_shown("library-item-Resistor", &mut cx));
    assert!(!is_shown("library-item-category-Voltage source", &mut cx));
    press("enter", &mut cx);
    assert_eq!(
        *picks.borrow(),
        [LibraryItem::Part(PartKind::VoltageSource)]
    );
}

#[gpui_kit::test]
fn a_category_clicked_while_searching_narrows_the_matches(cx: &mut TestAppContext) {
    let mut cx = host_window(cx);
    let picks = open(LibraryFilter::All, None, &mut cx);
    type_query("current", &mut cx);
    assert!(is_shown("library-item-Current source", &mut cx));
    assert!(
        is_shown("library-item-Inductor", &mut cx),
        "its description mentions the initial current"
    );

    click("library-category-Basic parts", &mut cx);
    assert!(is_shown("library-selected-category-Basic parts", &mut cx));
    assert!(is_shown("library-item-Inductor", &mut cx));
    assert!(!is_shown("library-item-Current source", &mut cx));

    erase(1, &mut cx);
    assert!(
        is_shown("library-item-Current source", &mut cx),
        "editing the query searches every category again"
    );
    assert!(is_shown("library-selected-category-All parts", &mut cx));

    erase("curren".len(), &mut cx);
    assert!(
        is_shown("library-selected-category-Basic parts", &mut cx),
        "clearing returns to the category selected last"
    );
    assert!(is_shown("library-item-Resistor", &mut cx));
    assert!(!is_shown("library-item-Current source", &mut cx));
    press("enter", &mut cx);
    assert_eq!(*picks.borrow(), [LibraryItem::Part(PartKind::Inductor)]);
}

#[gpui_kit::test]
fn arrows_move_through_the_matches_of_every_category(cx: &mut TestAppContext) {
    let mut cx = host_window(cx);
    let picks = open(LibraryFilter::All, None, &mut cx);
    click("library-category-Sources", &mut cx);
    type_query("current", &mut cx);
    // Current source, then the current switch (Switches), then the inductor
    // (Basic parts, whose description mentions its initial current).
    press("down down", &mut cx);
    press("enter", &mut cx);
    assert_eq!(*picks.borrow(), [LibraryItem::Part(PartKind::Inductor)]);
}

#[gpui_kit::test]
fn nothing_matching_anywhere_shows_an_empty_state(cx: &mut TestAppContext) {
    let mut cx = host_window(cx);
    let picks = open(LibraryFilter::All, None, &mut cx);
    click("library-category-Diodes", &mut cx);
    type_query("zzqx", &mut cx);
    assert!(is_shown("library-no-results", &mut cx));
    assert!(is_shown("library-no-results-anywhere", &mut cx));
    assert!(!is_shown("library-no-results-elsewhere", &mut cx));
    assert!(
        !is_shown("library-search-everywhere", &mut cx),
        "searching already covers every category"
    );
    assert!(is_shown("library-details", &mut cx));
    press("enter", &mut cx);
    assert!(picks.borrow().is_empty(), "nothing to pick");
    assert!(is_shown("library-search", &mut cx), "the window stays open");
}

#[gpui_kit::test]
fn a_narrowed_search_without_matches_points_to_the_other_categories(cx: &mut TestAppContext) {
    let mut cx = host_window(cx);
    open(LibraryFilter::All, None, &mut cx);
    type_query("resistor", &mut cx);
    click("library-category-Sources", &mut cx);
    assert!(is_shown("library-no-results-elsewhere", &mut cx));
    assert!(!is_shown("library-search-everywhere", &mut cx));
    click("library-category-All parts", &mut cx);
    assert!(is_shown("library-item-Resistor", &mut cx));
}

#[gpui_kit::test]
fn the_preselected_item_survives_a_search_across_categories(cx: &mut TestAppContext) {
    let mut cx = host_window(cx);
    let pnp = bjt(Polarity::P, library::default_bjt(Polarity::P));
    let picks = open(LibraryFilter::All, Some(pnp.clone()), &mut cx);
    let prefix: String = library::default_bjt(Polarity::P).chars().take(3).collect();
    type_query(&prefix, &mut cx);
    press("enter", &mut cx);
    assert_eq!(*picks.borrow(), [pnp]);
}

#[gpui_kit::test]
fn family_filters_list_only_matching_models(cx: &mut TestAppContext) {
    let mut cx = host_window(cx);
    let picks = open(LibraryFilter::Bjts(Polarity::P), None, &mut cx);
    assert!(!is_shown("library-item-Resistor", &mut cx));
    assert!(
        !is_shown("library-item-NPN_GP", &mut cx),
        "NPN is filtered out"
    );
    assert!(is_shown("library-item-PNP_GP", &mut cx));
    assert!(is_shown("library-category-All PNP transistors", &mut cx));
    type_query(library::default_bjt(Polarity::N), &mut cx);
    assert!(
        is_shown("library-no-results-anywhere", &mut cx),
        "searching every category stays within the family"
    );
    erase(library::default_bjt(Polarity::N).len(), &mut cx);
    type_query("PNP_GP", &mut cx);
    keys("enter", &mut cx);
    assert_eq!(
        *picks.borrow(),
        [LibraryItem::Bjt {
            polarity: Polarity::P,
            model: "PNP_GP".into()
        }]
    );
}

#[gpui_kit::test]
fn a_filtered_search_covers_every_category_of_the_family(cx: &mut TestAppContext) {
    let mut cx = host_window(cx);
    let models: Vec<_> = library::bjts()
        .into_iter()
        .filter(|entry| entry.model.polarity == Polarity::P)
        .collect();
    let leaf = |category: &[&'static str]| category.last().copied().unwrap_or_default();
    let current = models
        .iter()
        .find(|entry| entry.model.name == library::default_bjt(Polarity::P))
        .expect("the default PNP model");
    let other = models
        .iter()
        .find(|entry| leaf(entry.category) != leaf(current.category))
        .expect("PNP models in two categories");
    let (current_name, other_name) = (current.model.name.clone(), other.model.name.clone());
    let picks = open(
        LibraryFilter::Bjts(Polarity::P),
        Some(bjt(Polarity::P, &current_name)),
        &mut cx,
    );

    click(
        selector(format!("library-category-{}", leaf(current.category))),
        &mut cx,
    );
    assert!(is_shown(
        selector(format!("library-item-{current_name}")),
        &mut cx
    ));
    let other_row = selector(format!("library-item-{other_name}"));
    assert!(!is_shown(other_row, &mut cx), "in another category");

    type_query(&other_name, &mut cx);
    assert!(is_shown(other_row, &mut cx));
    assert!(is_shown(
        selector(format!("library-item-category-{other_name}")),
        &mut cx
    ));
    assert!(is_shown(
        "library-selected-category-All PNP transistors",
        &mut cx
    ));
    assert!(!is_shown("library-item-Resistor", &mut cx));
    press("enter", &mut cx);
    assert_eq!(*picks.borrow(), [bjt(Polarity::P, &other_name)]);
}

#[gpui_kit::test]
fn every_builtin_model_is_listed(cx: &mut TestAppContext) {
    let mut cx = host_window(cx);
    let picks = open(LibraryFilter::Diodes, None, &mut cx);
    let diodes = library::diodes();
    assert!(!diodes.is_empty());
    let first = diodes[0].model.name.clone();
    type_query(&first, &mut cx);
    keys("enter", &mut cx);
    assert_eq!(*picks.borrow(), [LibraryItem::Diode { model: first }]);
}

#[gpui_kit::test]
fn the_current_model_is_preselected(cx: &mut TestAppContext) {
    let mut cx = host_window(cx);
    let current = LibraryItem::Jfet {
        polarity: Polarity::N,
        model: library::default_jfet(Polarity::N).into(),
    };
    let picks = open(
        LibraryFilter::Jfets(Polarity::N),
        Some(current.clone()),
        &mut cx,
    );
    keys("enter", &mut cx);
    assert_eq!(*picks.borrow(), [current]);
}

#[gpui_kit::test]
fn open_library_lists_everything(cx: &mut TestAppContext) {
    let mut cx = host_window(cx);
    let picks = Picks::default();
    let record = picks.clone();
    cx.update(|window, cx| {
        open_library(
            LibraryFilter::All,
            move |item, _, _| record.borrow_mut().push(item),
            window,
            cx,
        )
    });
    cx.run_until_parked();
    keys("enter", &mut cx);
    assert_eq!(*picks.borrow(), [LibraryItem::Part(PartKind::Resistor)]);
}

#[gpui_kit::test]
fn the_layout_stays_inside_the_dialog_at_every_ui_scale(cx: &mut TestAppContext) {
    let mut cx = host_window(cx);
    for font_size in [16., 20.] {
        cx.update(|_, cx| Theme::update(cx, |theme| theme.font_size = px(font_size)));
        open(LibraryFilter::All, None, &mut cx);
        let dialog = cx.debug_bounds("dialog-0").expect("an open dialog");
        for selector in [
            "library-search",
            "library-category-All parts",
            "library-item-Resistor",
            "library-details",
            "library-preview",
            "library-cancel",
            "library-pick",
        ] {
            let bounds = cx
                .debug_bounds(selector)
                .unwrap_or_else(|| panic!("{selector} should be visible"));
            let inside = bounds.left() >= dialog.left()
                && bounds.right() <= dialog.right()
                && bounds.top() >= dialog.top()
                && bounds.bottom() <= dialog.bottom();
            assert!(inside, "{selector} {bounds:?} leaves the dialog {dialog:?}");
        }
        keys("escape", &mut cx);
    }
}

/// Asserts that `inner` lies inside `outer`.
fn assert_within(inner: Bounds<Pixels>, outer: Bounds<Pixels>, what: &str) {
    let inside = inner.left() >= outer.left()
        && inner.right() <= outer.right()
        && inner.top() >= outer.top()
        && inner.bottom() <= outer.bottom();
    assert!(inside, "{what}: {inner:?} leaves {outer:?}");
}

/// How far the kit's focus ring reaches outside a control's border box: a
/// 3 px band at every UI scale.
const FOCUS_RING_REACH: f32 = 3.;

/// `bounds` grown by the focus ring around it.
fn with_focus_ring(bounds: Bounds<Pixels>) -> Bounds<Pixels> {
    let reach = px(FOCUS_RING_REACH);
    Bounds::new(
        point(bounds.left() - reach, bounds.top() - reach),
        size(
            bounds.size.width + reach * 2.,
            bounds.size.height + reach * 2.,
        ),
    )
}

/// D14: the dialog body clips at the top edge of the library's content,
/// right under the title, and 16 px beside it. The search field (focused on
/// opening), the category tree and the footer buttons keep their focus
/// rings inside that content at 75%, 100%, 150% and 200% UI scale.
#[gpui_kit::test]
fn focus_rings_are_not_clipped_at_any_ui_scale(cx: &mut TestAppContext) {
    let mut cx = host_window(cx);
    for font_size in [12., 16., 24., 32.] {
        cx.update(|_, cx| Theme::update(cx, |theme| theme.font_size = px(font_size)));
        open(LibraryFilter::All, None, &mut cx);
        let content = cx.debug_bounds("library-browser").expect("the content");
        let dialog = cx.debug_bounds("dialog-0").expect("an open dialog");
        assert_within(content, dialog, "the content");
        let search = cx.debug_bounds("library-search").expect("the search field");
        assert_within(
            with_focus_ring(search),
            content,
            &format!("the search field's ring at {font_size} px"),
        );
        press("tab", &mut cx);
        let tree = cx.debug_bounds("library-tree").expect("the tree");
        assert_within(
            with_focus_ring(tree),
            content,
            &format!("the tree's ring at {font_size} px"),
        );
        for button in ["library-cancel", "library-pick"] {
            let bounds = cx.debug_bounds(button).expect("a footer button");
            assert_within(
                with_focus_ring(bounds),
                content,
                &format!("the ring of {button} at {font_size} px"),
            );
        }
        press("escape", &mut cx);
    }
}

#[gpui_kit::test]
fn clicking_a_category_row_opens_and_closes_it(cx: &mut TestAppContext) {
    let mut cx = host_window(cx);
    let picks = open(LibraryFilter::All, None, &mut cx);
    assert!(
        is_shown("library-collapse-Diodes", &mut cx),
        "open at first"
    );
    assert!(is_shown("library-category-Switching", &mut cx));

    click("library-category-Diodes", &mut cx);
    assert!(
        !is_shown("library-category-Switching", &mut cx),
        "a click on the label closes it"
    );
    assert!(is_shown("library-expand-Diodes", &mut cx));
    assert!(
        is_shown("library-selected-category-Diodes", &mut cx),
        "and shows it"
    );
    assert!(!is_shown("library-item-Resistor", &mut cx));

    click("library-category-Diodes", &mut cx);
    assert!(is_shown("library-category-Switching", &mut cx), "reopened");
    assert!(is_shown("library-selected-category-Diodes", &mut cx));

    // The arrow is part of the row: one click toggles once.
    click("library-collapse-Diodes", &mut cx);
    assert!(!is_shown("library-category-Switching", &mut cx));
    click("library-expand-Diodes", &mut cx);
    assert!(is_shown("library-category-Switching", &mut cx));

    // A category without sub-categories is only shown.
    click("library-category-Sources", &mut cx);
    assert!(is_shown("library-selected-category-Sources", &mut cx));
    assert!(is_shown("library-category-Switching", &mut cx), "unchanged");

    // The keyboard is back in the search field after a click.
    type_query("zzqx", &mut cx);
    assert!(is_shown("library-no-results", &mut cx));
    assert!(picks.borrow().is_empty());
}

/// The tree is a tab stop after the search field. Up/Down walk it,
/// Left/Right close and open categories or go up and down a level, and
/// Enter or Space open and close a category without picking anything.
#[gpui_kit::test]
fn the_category_tree_works_from_the_keyboard(cx: &mut TestAppContext) {
    let mut cx = host_window(cx);
    let picks = open(LibraryFilter::All, None, &mut cx);
    press("tab", &mut cx);
    press("down", &mut cx);
    assert!(is_shown("library-selected-category-Basic parts", &mut cx));
    press("down down down", &mut cx);
    assert!(is_shown("library-selected-category-Diodes", &mut cx));

    press("left", &mut cx);
    assert!(
        !is_shown("library-category-Switching", &mut cx),
        "Left closes"
    );
    assert!(is_shown("library-selected-category-Diodes", &mut cx));
    press("right", &mut cx);
    assert!(
        is_shown("library-category-Switching", &mut cx),
        "Right opens"
    );
    press("right", &mut cx);
    assert!(
        is_shown("library-selected-category-LED", &mut cx),
        "Right again enters the first sub-category"
    );
    press("left", &mut cx);
    assert!(
        is_shown("library-selected-category-Diodes", &mut cx),
        "Left goes back up"
    );

    press("enter", &mut cx);
    assert!(
        !is_shown("library-category-Switching", &mut cx),
        "Enter closes"
    );
    assert!(is_shown("library-search", &mut cx), "the window stays open");
    press("space", &mut cx);
    assert!(
        is_shown("library-category-Switching", &mut cx),
        "Space opens"
    );
    press("enter", &mut cx);
    press("enter", &mut cx);
    assert!(is_shown("library-category-Switching", &mut cx));
    assert!(picks.borrow().is_empty(), "nothing was picked");

    press("up up up up", &mut cx);
    assert!(is_shown("library-selected-category-All parts", &mut cx));
    press("up", &mut cx);
    assert!(is_shown("library-selected-category-All parts", &mut cx));

    // A category without sub-categories has nothing to open: Enter picks
    // the selection, as everywhere in the window.
    press("down down", &mut cx);
    assert!(is_shown("library-selected-category-Sources", &mut cx));
    press("enter", &mut cx);
    assert_eq!(
        *picks.borrow(),
        [LibraryItem::Part(PartKind::VoltageSource)]
    );
    assert!(!is_shown("library-search", &mut cx));
}

/// Enter on a focused button presses that button: Cancel closes the window
/// without picking, Place picks. The window's close button is a window
/// control, no tab stop (D22): a click on it closes without picking, and
/// Tab goes from Place round to the search field, where Enter picks.
#[gpui_kit::test]
fn enter_presses_the_focused_button(cx: &mut TestAppContext) {
    let mut cx = host_window(cx);
    let picks = open(LibraryFilter::All, None, &mut cx);
    // Search field, category tree, Cancel.
    press("tab tab", &mut cx);
    press("enter", &mut cx);
    assert!(!is_shown("library-search", &mut cx), "Cancel closed it");
    assert!(picks.borrow().is_empty(), "without picking");

    let more = open(LibraryFilter::All, None, &mut cx);
    click("dialog-close", &mut cx);
    assert!(
        !is_shown("library-search", &mut cx),
        "the close button closed it"
    );
    assert!(more.borrow().is_empty(), "without picking");

    let round = open(LibraryFilter::All, None, &mut cx);
    // Search field, category tree, Cancel, Place, then the search field.
    press("tab tab tab tab", &mut cx);
    press("enter", &mut cx);
    assert!(!is_shown("library-search", &mut cx), "Enter picked");
    assert_eq!(*round.borrow(), [LibraryItem::Part(PartKind::Resistor)]);

    let placed = open(LibraryFilter::All, None, &mut cx);
    press("tab tab tab", &mut cx);
    press("enter", &mut cx);
    assert!(!is_shown("library-search", &mut cx));
    assert_eq!(*placed.borrow(), [LibraryItem::Part(PartKind::Resistor)]);
}

/// A click on the dialog's margin leaves the keyboard on the dialog itself,
/// not on a control: Enter still places the selection there.
#[gpui_kit::test]
fn enter_after_a_click_on_the_dialog_margin_still_places(cx: &mut TestAppContext) {
    let mut cx = host_window(cx);
    let picks = open(LibraryFilter::All, None, &mut cx);
    let dialog = cx.debug_bounds("dialog-0").expect("an open dialog");
    let content = cx.debug_bounds("library-browser").expect("the content");
    let margin = point(dialog.left() + px(8.), content.center().y);
    assert!(margin.x < content.left(), "beside the content");
    cx.simulate_mouse_move(margin, None, Modifiers::default());
    cx.simulate_click(margin, Modifiers::default());
    cx.run_until_parked();
    press("enter", &mut cx);
    assert_eq!(*picks.borrow(), [LibraryItem::Part(PartKind::Resistor)]);
    assert!(!is_shown("library-search", &mut cx));
}

/// A double click is a press, release, press and release; the second
/// release reports two clicks.
fn double_click(selector: &'static str, cx: &mut VisualTestContext) {
    follow(cx);
    let position = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} should be visible"))
        .center();
    cx.simulate_mouse_move(position, None, Modifiers::default());
    for click_count in [1, 2] {
        cx.simulate_event(MouseDownEvent {
            position,
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            position,
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count,
        });
    }
    cx.run_until_parked();
    follow(cx);
}

/// A double click on a category row opens or closes it once, rather than
/// twice (and so not at all).
#[gpui_kit::test]
fn double_clicking_a_category_row_toggles_it_once(cx: &mut TestAppContext) {
    let mut cx = host_window(cx);
    let picks = open(LibraryFilter::All, None, &mut cx);
    double_click("library-category-Diodes", &mut cx);
    assert!(!is_shown("library-category-Switching", &mut cx), "closed");
    assert!(is_shown("library-selected-category-Diodes", &mut cx));
    double_click("library-category-Diodes", &mut cx);
    assert!(is_shown("library-category-Switching", &mut cx), "reopened");
    // A single click still toggles right after.
    click("library-category-Diodes", &mut cx);
    assert!(!is_shown("library-category-Switching", &mut cx));
    assert!(picks.borrow().is_empty(), "nothing is picked");
}

/// Pressing a category row leaves the keyboard in the search field: the
/// tree takes no focus on the press (its ring would flash until the
/// release), and a press dragged off the row changes nothing.
#[gpui_kit::test]
fn pressing_a_category_row_keeps_the_keyboard_in_the_search_field(cx: &mut TestAppContext) {
    let mut cx = host_window(cx);
    let picks = open(LibraryFilter::All, None, &mut cx);
    let row = cx
        .debug_bounds("library-category-Sources")
        .expect("the Sources row")
        .center();
    let away = cx
        .debug_bounds("library-details")
        .expect("the details")
        .center();
    cx.simulate_mouse_move(row, None, Modifiers::default());
    cx.simulate_event(MouseDownEvent {
        position: row,
        modifiers: Modifiers::default(),
        button: MouseButton::Left,
        click_count: 1,
        first_mouse: false,
    });
    cx.run_until_parked();
    // Down in the tree would show the next category; in the search field
    // it moves through the list.
    press("down", &mut cx);
    assert!(
        is_shown("library-selected-category-All parts", &mut cx),
        "the press left the keyboard in the search field"
    );
    cx.simulate_mouse_move(away, Some(MouseButton::Left), Modifiers::default());
    cx.simulate_event(MouseUpEvent {
        position: away,
        modifiers: Modifiers::default(),
        button: MouseButton::Left,
        click_count: 1,
    });
    cx.run_until_parked();
    assert!(
        is_shown("library-selected-category-All parts", &mut cx),
        "released off the row: no category shown"
    );
    assert!(
        is_shown("library-category-Switching", &mut cx),
        "none toggled"
    );
    press("down", &mut cx);
    press("enter", &mut cx);
    assert_eq!(
        *picks.borrow(),
        [LibraryItem::Part(PartKind::Inductor)],
        "both Downs moved through the list"
    );
}

/// Escape closes the window after a click on a result or a category, and
/// Enter picks after either.
#[gpui_kit::test]
fn enter_and_escape_work_after_clicks_in_the_lists(cx: &mut TestAppContext) {
    let mut cx = host_window(cx);
    let picks = open(LibraryFilter::All, None, &mut cx);
    click("library-item-Capacitor", &mut cx);
    press("escape", &mut cx);
    assert!(
        !is_shown("library-search", &mut cx),
        "closed after a result"
    );
    let more = open(LibraryFilter::All, None, &mut cx);
    click("library-category-Diodes", &mut cx);
    press("escape", &mut cx);
    assert!(
        !is_shown("library-search", &mut cx),
        "closed after a category"
    );
    assert!(picks.borrow().is_empty() && more.borrow().is_empty());

    let placed = open(LibraryFilter::All, None, &mut cx);
    let diode = library::diodes()[0].model.name.clone();
    click("library-category-Diodes", &mut cx);
    click(selector(format!("library-item-{diode}")), &mut cx);
    press("enter", &mut cx);
    assert_eq!(*placed.borrow(), [LibraryItem::Diode { model: diode }]);
}

/// A host window showing a schematic editor that has the keyboard.
fn editor_window(cx: &mut TestAppContext) -> (Entity<SchematicEditor>, VisualTestContext) {
    let (window, editor) = cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_reduce_motion(true);
        dialog_window::init(cx);
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(40.), px(30.)),
                    size(px(1280.), px(900.)),
                ))),
                ..WindowOptions::default()
            },
            cx,
            |window, cx| {
                let editor = cx.new(|cx| {
                    SchematicEditor::new(Schematic::default(), SchematicView::default(), cx)
                });
                editor.read(cx).focus_handle(cx).focus(window, cx);
                editor
            },
        )
        .expect("test window")
    });
    let visual = VisualTestContext::from_window(window, cx);
    visual.run_until_parked();
    (editor, visual)
}

/// D22: P in the editor opens the library in a window of its own, centred
/// over the editor's window, which ignores keys meanwhile. The pick starts
/// placing the part in the editor of the main window, which has the
/// keyboard again.
#[gpui_kit::test]
fn a_library_pick_places_the_part_in_the_main_window(cx: &mut TestAppContext) {
    let (editor, mut cx) = editor_window(cx);
    let main = cx.window_handle();
    let owner = cx.update(|window, _| window.bounds());
    keys("p", &mut cx);
    assert_eq!(cx.windows().len(), 2, "the library has a window of its own");
    let library = cx.window_handle();
    assert_ne!(library, main);
    assert_eq!(cx.window_title().as_deref(), Some("Component Library"));
    let bounds = cx.update(|window, _| window.bounds());
    assert_eq!(bounds.center(), owner.center(), "centred over the editor");
    assert!(cx.update(|window, _| window.is_resizable()), "resizable");

    // Keys and clicks in the editor's window reach neither the editor nor
    // the library; a click brings the library to the front.
    let mut editor_cx = VisualTestContext::from_window(main, &cx.cx);
    editor_cx.simulate_keystrokes("w escape");
    editor_cx.run_until_parked();
    let resistor = editor_cx
        .debug_bounds("toolbar-part-r")
        .expect("the toolbar's resistor")
        .center();
    editor_cx.simulate_mouse_move(resistor, None, Modifiers::default());
    editor_cx.simulate_click(resistor, Modifiers::default());
    editor_cx.run_until_parked();
    assert_eq!(editor.read_with(&cx, |e, _| e.tool()), Tool::Select);
    assert_eq!(
        cx.update(|_, cx| cx.active_window()),
        Some(library),
        "the library came to the front"
    );
    assert!(is_shown("library-search", &mut cx), "still open");

    type_query("capacitor", &mut cx);
    press("enter", &mut cx);
    assert_eq!(cx.windows(), vec![main], "the pick closed the library");
    assert_eq!(
        editor.read_with(&cx, |e, _| e.tool()),
        Tool::Place(PartKind::Capacitor),
        "placing in the main window's editor"
    );
    assert!(
        cx.update(|window, cx| editor.read(cx).focus_handle(cx).is_focused(window)),
        "the editor has the keyboard"
    );
    keys("escape", &mut cx);
    assert_eq!(editor.read_with(&cx, |e, _| e.tool()), Tool::Select);
}

/// D22: the toolbar's Library button opens the library in a window of its
/// own over the editor, whose window ignores the wheel meanwhile: the view
/// does not move. A double click on a result places it in the editor of the
/// main window, which has the keyboard again.
#[gpui_kit::test]
fn the_toolbar_opens_the_library_and_a_double_click_places(cx: &mut TestAppContext) {
    use gpui_kit::{ScrollDelta, ScrollEvent};
    let (editor, mut cx) = editor_window(cx);
    let main = cx.window_handle();
    click("toolbar-library", &mut cx);
    assert_eq!(cx.windows().len(), 2, "the library has a window of its own");
    assert_ne!(cx.window_handle(), main);
    assert_eq!(cx.window_title().as_deref(), Some("Component Library"));

    let mut editor_cx = VisualTestContext::from_window(main, &cx.cx);
    let view = |cx: &VisualTestContext| {
        editor.read_with(cx, |e, _| e.grid_to_window(xsim_project::Point::new(0, 0)))
    };
    let before = view(&cx);
    assert!(before.is_some(), "the canvas is laid out");
    let middle = editor_cx.update(|window, _| {
        let size = window.viewport_size();
        point(size.width / 2., size.height / 2.)
    });
    editor_cx.simulate_mouse_move(middle, None, Modifiers::default());
    for delta in [
        ScrollDelta::Lines(point(0., 3.)),
        ScrollDelta::Pixels(point(px(40.), px(-120.))),
    ] {
        for modifiers in [Modifiers::default(), Modifiers::control()] {
            editor_cx.simulate_event(ScrollEvent {
                position: middle,
                delta,
                modifiers,
                ..ScrollEvent::default()
            });
        }
    }
    editor_cx.run_until_parked();
    assert_eq!(view(&cx), before, "the wheel moved nothing");

    double_click("library-item-Capacitor", &mut cx);
    assert_eq!(cx.windows(), vec![main], "the pick closed the library");
    assert_eq!(
        editor.read_with(&cx, |e, _| e.tool()),
        Tool::Place(PartKind::Capacitor),
        "placing in the main window's editor"
    );
    assert!(
        cx.update(|window, cx| editor.read(cx).focus_handle(cx).is_focused(window)),
        "the editor has the keyboard"
    );
    // The wheel works again.
    cx.simulate_event(ScrollEvent {
        position: middle,
        delta: ScrollDelta::Lines(point(0., 3.)),
        ..ScrollEvent::default()
    });
    cx.run_until_parked();
    assert_ne!(view(&cx), before, "the wheel moves the view again");
}

/// D24: on the owner's screen at 200% display scaling (a 1920 × 1030 px
/// work area) and on a smaller one, at 100% and 150% UI scale, the library
/// opens inside the work area with Cancel and the pick button in view.
#[gpui_kit::test]
fn the_library_window_fits_the_owners_screens(cx: &mut TestAppContext) {
    let (_, mut cx) = editor_window(cx);
    let main = cx.window_handle();
    for (width, height) in [(1920., 1030.), (1280., 680.)] {
        let area = Bounds::new(point(px(0.), px(0.)), size(px(width), px(height)));
        for font_size in [16., 24.] {
            cx.update(|window, cx| {
                dialog_window::simulate_work_area(cx, Some(area));
                Theme::update(cx, |theme| theme.font_size = px(font_size));
                window.resize(area.size);
                window.bounds_changed(cx);
            });
            cx.run_until_parked();
            keys("p", &mut cx);
            let case = format!("{font_size} px on {width} × {height}");
            assert_ne!(cx.window_handle(), main, "{case}: the library opened");
            let (bounds, viewport) =
                cx.update(|window, _| (window.bounds(), window.viewport_size()));
            let margin = dialog_window::SCREEN_MARGIN;
            assert!(
                bounds.left() >= margin
                    && bounds.top() >= margin
                    && bounds.right() <= area.right() - margin
                    && bounds.bottom() <= area.bottom() - margin,
                "{case}: {bounds:?} leaves the work area"
            );
            for button in ["library-cancel", "library-pick"] {
                let shown = cx
                    .debug_bounds(button)
                    .unwrap_or_else(|| panic!("{case}: {button} is shown"));
                assert!(
                    shown.left() >= px(0.)
                        && shown.top() >= px(0.)
                        && shown.right() <= viewport.width
                        && shown.bottom() <= viewport.height,
                    "{case}: {button} {shown:?} outside {viewport:?}"
                );
            }
            press("escape", &mut cx);
            assert_eq!(cx.window_handle(), main, "{case}: closed");
        }
    }
}
