//! Headless UI tests of the part properties dialog: typing, validation,
//! applying as one undo step, cancelling, sources, PWL and models.
//!
//! The dialog is a window of its own (D22). The helpers keep the test's
//! `VisualTestContext` on the window that has the keyboard: the editor's,
//! the properties dialog over it, or the model library over that.
//!
//! They locate controls with debug selectors, which only debug builds record.
#![cfg(debug_assertions)]

use gpui_kit::component::Theme;
use gpui_kit::{
    AnyWindowHandle, AppContext as _, Bounds, Entity, Focusable as _, KeyDownEvent, KeyUpEvent,
    Keystroke, Modifiers, Pixels, Point, ScrollDelta, ScrollEvent, TestAppContext,
    VisualContext as _, VisualTestContext, WindowBounds, WindowOptions, px, size,
};
use xsim_editor::dialog_window;
use xsim_editor::library_browser::{LibraryFilter, open_library};
use xsim_editor::properties::open_properties;
use xsim_editor::{ObjectRef, SchematicEditor, Tool};
use xsim_project::{Component, Id, Part, PartKind, Point as GridPoint, Schematic, SchematicView};
use xsim_spice_engine::{Polarity, Source, Waveform, library, units::parse_value};

/// The value of `text` as the form parses it.
fn si(text: &str) -> f64 {
    parse_value(text).expect("valid SI value")
}

/// A schematic with one component per `(name, part)`, ids from 1.
fn schematic(parts: &[(&str, Part)]) -> Schematic {
    let mut schematic = Schematic::default();
    for (n, (name, part)) in parts.iter().enumerate() {
        let id = schematic.allocate_id();
        schematic.components.push(Component {
            id,
            name: name.to_string(),
            position: GridPoint::new(16 * n as i32, 0),
            orientation: Default::default(),
            part: part.clone(),
        });
    }
    schematic
}

/// Opens a headless 1200 × 900 window showing an editor of `schematic`.
fn editor_window(
    cx: &mut TestAppContext,
    schematic: Schematic,
) -> (Entity<SchematicEditor>, VisualTestContext) {
    editor_window_sized(cx, schematic, 1200., 900.)
}

/// A window tall enough for the whole source dialog at 200% UI scale, so
/// layout checks need not scroll.
fn tall_editor_window(
    cx: &mut TestAppContext,
    schematic: Schematic,
) -> (Entity<SchematicEditor>, VisualTestContext) {
    editor_window_sized(cx, schematic, 1200., 1800.)
}

fn editor_window_sized(
    cx: &mut TestAppContext,
    schematic: Schematic,
    width: f32,
    height: f32,
) -> (Entity<SchematicEditor>, VisualTestContext) {
    let (window, editor) = cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_reduce_motion(true);
        dialog_window::init(cx);
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Point::default(),
                    size(px(width), px(height)),
                ))),
                ..WindowOptions::default()
            },
            cx,
            |_, cx| cx.new(|cx| SchematicEditor::new(schematic, SchematicView::default(), cx)),
        )
        .expect("test window")
    });
    let visual = VisualTestContext::from_window(window, cx);
    visual.run_until_parked();
    (editor, visual)
}

/// The editor's window: the one that is no dialog.
fn main_window(cx: &VisualTestContext) -> AnyWindowHandle {
    cx.cx
        .read(|app| {
            app.windows()
                .into_iter()
                .find(|window| dialog_window::owner_of(*window, app).is_none())
        })
        .expect("the editor's window")
}

/// The window with the keyboard: the topmost dialog over the editor.
fn topmost(cx: &VisualTestContext) -> AnyWindowHandle {
    let main = main_window(cx);
    cx.cx.read(|app| dialog_window::topmost(main, app))
}

/// The open dialog over the editor's window, if any.
fn dialog(cx: &VisualTestContext) -> Option<AnyWindowHandle> {
    let main = main_window(cx);
    cx.cx.read(|app| dialog_window::dialog_of(main, app))
}

/// Points `cx` at `window`.
fn switch_to(window: AnyWindowHandle, cx: &mut VisualTestContext) {
    if cx.window_handle() != window {
        let next = VisualTestContext::from_window(window, &cx.cx);
        *cx = next;
    }
}

/// Points `cx` at the window with the keyboard, after dialogs opened or
/// closed.
fn follow(cx: &mut VisualTestContext) {
    let top = topmost(cx);
    switch_to(top, cx);
}

/// Opens the properties of `id` from the editor's window and points `cx`
/// at the dialog.
fn open(editor: &Entity<SchematicEditor>, id: Id, cx: &mut VisualTestContext) {
    let main = main_window(cx);
    switch_to(main, cx);
    cx.update(|window, cx| open_properties(editor, id, window, cx));
    cx.run_until_parked();
    follow(cx);
}

fn is_shown(selector: &'static str, cx: &mut VisualTestContext) -> bool {
    follow(cx);
    cx.debug_bounds(selector).is_some()
}

/// Scrolls the dialog's content until `selector` is in view: a dialog taller
/// than the screen scrolls (D22).
fn scroll_into_view(selector: &'static str, cx: &mut VisualTestContext) {
    let Some(bounds) = cx.debug_bounds(selector) else {
        return;
    };
    let viewport = cx.update(|window, _| window.viewport_size());
    let top = cx
        .debug_bounds("dialog-title-bar")
        .map_or(px(0.), |bar| bar.bottom());
    if bounds.top() >= top && bounds.bottom() <= viewport.height {
        return;
    }
    let middle = (top + viewport.height) / 2.;
    cx.simulate_event(ScrollEvent {
        position: gpui_kit::point(viewport.width / 2., middle),
        delta: ScrollDelta::Pixels(gpui_kit::point(px(0.), middle - bounds.center().y)),
        ..ScrollEvent::default()
    });
    cx.run_until_parked();
}

fn click(selector: &'static str, cx: &mut VisualTestContext) {
    follow(cx);
    scroll_into_view(selector, cx);
    let center = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} should be visible"))
        .center();
    cx.simulate_mouse_move(center, None, Modifiers::default());
    cx.simulate_click(center, Modifiers::default());
    cx.run_until_parked();
    follow(cx);
}

/// Types the space-separated `keystrokes` in the window with the keyboard.
fn keys(keystrokes: &str, cx: &mut VisualTestContext) {
    follow(cx);
    cx.simulate_keystrokes(keystrokes);
    cx.run_until_parked();
    follow(cx);
}

/// Replaces the text of the input with debug selector `selector`.
fn type_into(selector: &'static str, text: &str, cx: &mut VisualTestContext) {
    click(selector, cx);
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-a"
    } else {
        "ctrl-a"
    });
    cx.simulate_input(text);
    cx.run_until_parked();
}

fn part(editor: &Entity<SchematicEditor>, id: Id, cx: &mut VisualTestContext) -> Part {
    editor.read_with(cx, |editor, _| {
        editor
            .schematic()
            .component(id)
            .expect("component")
            .part
            .clone()
    })
}

/// Asserts that exactly one undo step was recorded: undoing it restores
/// `original` and leaves nothing to undo.
fn assert_one_undo_step(
    editor: &Entity<SchematicEditor>,
    original: &Schematic,
    cx: &mut VisualTestContext,
) {
    editor.update(cx, |editor, cx| {
        assert!(editor.can_undo(), "the edit should be undoable");
        editor.undo(cx);
        assert_eq!(
            editor.schematic(),
            original,
            "one undo restores the schematic"
        );
        assert!(!editor.can_undo(), "the edit should be a single undo step");
    });
}

fn resistor() -> Schematic {
    schematic(&[("R1", Part::default_for(PartKind::Resistor))])
}

#[gpui_kit::test]
fn typing_a_value_and_pressing_enter_applies_one_undoable_edit(cx: &mut TestAppContext) {
    let original = resistor();
    let (editor, mut cx) = editor_window(cx, original.clone());
    open(&editor, 1, &mut cx);
    assert!(is_shown("field-Resistance", &mut cx));
    type_into("field-Resistance", "4.7k", &mut cx);
    assert!(is_shown("field-Resistance-message", &mut cx));
    keys("enter", &mut cx);
    assert!(
        !is_shown("properties-ok", &mut cx),
        "Enter closes the dialog"
    );
    assert_eq!(
        part(&editor, 1, &mut cx),
        Part::Resistor {
            resistance: si("4.7k")
        }
    );
    assert_one_undo_step(&editor, &original, &mut cx);
}

#[gpui_kit::test]
fn the_value_field_has_the_keyboard_when_the_dialog_opens(cx: &mut TestAppContext) {
    let (editor, mut cx) = editor_window(cx, resistor());
    open(&editor, 1, &mut cx);
    cx.simulate_input("10M");
    keys("enter", &mut cx);
    assert_eq!(
        part(&editor, 1, &mut cx),
        Part::Resistor { resistance: 1e7 }
    );
}

#[gpui_kit::test]
fn invalid_input_shows_an_error_and_blocks_applying(cx: &mut TestAppContext) {
    let original = resistor();
    let (editor, mut cx) = editor_window(cx, original.clone());
    open(&editor, 1, &mut cx);
    type_into("field-Resistance", "abc", &mut cx);
    assert!(is_shown("field-Resistance-error", &mut cx));
    click("properties-ok", &mut cx);
    keys("enter", &mut cx);
    assert!(is_shown("properties-ok", &mut cx), "the dialog stays open");
    assert_eq!(
        editor.read_with(&cx, |e, _| e.schematic().clone()),
        original
    );
    assert!(!editor.read_with(&cx, |e, _| e.can_undo()));

    type_into("field-Resistance", "-5", &mut cx);
    assert!(
        is_shown("field-Resistance-error", &mut cx),
        "resistance must be positive"
    );
    type_into("field-Resistance", "2.2k", &mut cx);
    assert!(!is_shown("field-Resistance-error", &mut cx));
    click("properties-ok", &mut cx);
    assert!(!is_shown("properties-ok", &mut cx));
    assert_eq!(
        part(&editor, 1, &mut cx),
        Part::Resistor {
            resistance: si("2.2k")
        }
    );
}

#[gpui_kit::test]
fn a_duplicate_designator_is_rejected_inline(cx: &mut TestAppContext) {
    let parts = [
        ("R1", Part::default_for(PartKind::Resistor)),
        ("R2", Part::default_for(PartKind::Resistor)),
    ];
    let original = schematic(&parts);
    let (editor, mut cx) = editor_window(cx, original.clone());
    open(&editor, 2, &mut cx);
    type_into("field-Designator", "R1", &mut cx);
    assert!(is_shown("field-Designator-error", &mut cx));
    keys("enter", &mut cx);
    assert!(is_shown("properties-ok", &mut cx));
    assert_eq!(
        editor.read_with(&cx, |e, _| e.schematic().clone()),
        original
    );

    type_into("field-Designator", "Rload", &mut cx);
    assert!(!is_shown("field-Designator-error", &mut cx));
    click("properties-ok", &mut cx);
    let name = editor.read_with(&cx, |e, _| {
        e.schematic().component(2).map(|c| c.name.clone())
    });
    assert_eq!(name.as_deref(), Some("Rload"));
    assert_one_undo_step(&editor, &original, &mut cx);
}

#[gpui_kit::test]
fn escape_and_cancel_discard_the_form(cx: &mut TestAppContext) {
    let original = resistor();
    let (editor, mut cx) = editor_window(cx, original.clone());
    open(&editor, 1, &mut cx);
    type_into("field-Resistance", "1", &mut cx);
    keys("escape", &mut cx);
    assert!(
        !is_shown("properties-ok", &mut cx),
        "Escape closes the dialog"
    );

    open(&editor, 1, &mut cx);
    type_into("field-Resistance", "2", &mut cx);
    click("properties-cancel", &mut cx);
    assert!(!is_shown("properties-ok", &mut cx));
    assert_eq!(
        editor.read_with(&cx, |e, _| e.schematic().clone()),
        original
    );
    assert!(!editor.read_with(&cx, |e, _| e.can_undo()));
}

#[gpui_kit::test]
fn apply_keeps_the_dialog_open_and_unchanged_forms_add_no_step(cx: &mut TestAppContext) {
    let original = resistor();
    let (editor, mut cx) = editor_window(cx, original.clone());
    open(&editor, 1, &mut cx);
    click("properties-ok", &mut cx);
    assert!(!is_shown("properties-ok", &mut cx));
    assert!(
        !editor.read_with(&cx, |e, _| e.can_undo()),
        "OK without changes"
    );

    open(&editor, 1, &mut cx);
    type_into("field-Resistance", "330", &mut cx);
    click("properties-apply", &mut cx);
    assert!(
        is_shown("properties-ok", &mut cx),
        "Apply keeps the dialog open"
    );
    assert_eq!(
        part(&editor, 1, &mut cx),
        Part::Resistor { resistance: 330.0 }
    );
    click("properties-ok", &mut cx);
    assert!(!is_shown("properties-ok", &mut cx));
    assert_one_undo_step(&editor, &original, &mut cx);
}

#[gpui_kit::test]
fn capacitor_initial_voltage_is_optional(cx: &mut TestAppContext) {
    let original = schematic(&[("C1", Part::default_for(PartKind::Capacitor))]);
    let (editor, mut cx) = editor_window(cx, original);
    open(&editor, 1, &mut cx);
    type_into("field-Initial voltage", "2.5", &mut cx);
    type_into("field-Capacitance", "100n", &mut cx);
    click("properties-ok", &mut cx);
    assert_eq!(
        part(&editor, 1, &mut cx),
        Part::Capacitor {
            capacitance: si("100n"),
            initial_voltage: Some(2.5)
        }
    );
}

#[gpui_kit::test]
fn selecting_a_waveform_shows_its_fields_and_preview(cx: &mut TestAppContext) {
    let original = schematic(&[("V1", Part::default_for(PartKind::VoltageSource))]);
    let (editor, mut cx) = editor_window(cx, original.clone());
    open(&editor, 1, &mut cx);
    assert!(!is_shown("field-Pulsed value", &mut cx));
    click("waveform-PULSE", &mut cx);
    assert!(is_shown("field-Pulsed value", &mut cx));
    assert!(is_shown("waveform-preview", &mut cx));
    type_into("field-Pulsed value", "5", &mut cx);
    type_into("field-Period", "100u", &mut cx);
    assert!(
        is_shown("field-Period-error", &mut cx),
        "shorter than the pulse"
    );
    type_into("field-Period", "2m", &mut cx);
    type_into("field-AC magnitude", "1", &mut cx);
    click("properties-ok", &mut cx);
    let Part::VoltageSource { source } = part(&editor, 1, &mut cx) else {
        panic!("still a voltage source");
    };
    assert_eq!(source.dc, 1.0);
    assert_eq!(source.ac_magnitude, 1.0);
    assert_eq!(
        source.waveform,
        Some(Waveform::Pulse {
            initial: 0.0,
            pulsed: 5.0,
            delay: 0.0,
            rise: si("1n"),
            fall: si("1n"),
            width: si("500u"),
            period: si("2m"),
            cycles: None,
        })
    );
    assert_one_undo_step(&editor, &original, &mut cx);
}

#[gpui_kit::test]
fn pwl_points_can_be_added_removed_and_repeated(cx: &mut TestAppContext) {
    let original = schematic(&[("I1", Part::default_for(PartKind::CurrentSource))]);
    let (editor, mut cx) = editor_window(cx, original);
    open(&editor, 1, &mut cx);
    click("waveform-PWL", &mut cx);
    assert!(is_shown("pwl-time-1", &mut cx));
    click("pwl-add", &mut cx);
    assert!(is_shown("pwl-time-2", &mut cx));
    type_into("pwl-value-2", "-2m", &mut cx);
    click("pwl-remove-0", &mut cx);
    assert!(!is_shown("pwl-time-2", &mut cx), "two rows remain");
    click("pwl-repeat", &mut cx);
    click("properties-ok", &mut cx);
    let Part::CurrentSource { source } = part(&editor, 1, &mut cx) else {
        panic!("still a current source");
    };
    assert_eq!(
        source.waveform,
        Some(Waveform::Pwl {
            points: vec![(si("1m"), si("1m")), (si("2m"), si("-2m"))],
            repeat: true,
        })
    );
}

#[gpui_kit::test]
fn switching_back_to_no_waveform_removes_it(cx: &mut TestAppContext) {
    let sine = Source::with_waveform(Waveform::Sine {
        offset: 0.0,
        amplitude: 1.0,
        frequency: 50.0,
        delay: 0.0,
        damping: 0.0,
        phase: 0.0,
        cycles: None,
    });
    let original = schematic(&[("V1", Part::VoltageSource { source: sine })]);
    let (editor, mut cx) = editor_window(cx, original);
    open(&editor, 1, &mut cx);
    assert!(is_shown("field-Frequency", &mut cx));
    click("waveform-None", &mut cx);
    assert!(!is_shown("field-Frequency", &mut cx));
    click("properties-ok", &mut cx);
    let Part::VoltageSource { source } = part(&editor, 1, &mut cx) else {
        panic!("still a voltage source");
    };
    assert_eq!(source.waveform, None);
}

#[gpui_kit::test]
fn read_only_editors_show_the_values_without_editing(cx: &mut TestAppContext) {
    let original = resistor();
    let (editor, mut cx) = editor_window(cx, original.clone());
    editor.update(&mut cx, |editor, cx| editor.set_read_only(true, cx));
    open(&editor, 1, &mut cx);
    assert!(is_shown("field-Resistance", &mut cx));
    assert!(!is_shown("properties-ok", &mut cx));
    click("properties-close", &mut cx);
    assert!(!is_shown("properties-close", &mut cx));
    assert_eq!(
        editor.read_with(&cx, |e, _| e.schematic().clone()),
        original
    );
}

#[gpui_kit::test]
fn opening_from_inside_an_editor_update_is_deferred(cx: &mut TestAppContext) {
    let (editor, mut cx) = editor_window(cx, resistor());
    let handle = editor.clone();
    cx.update(|window, cx| {
        editor.update(cx, |_, cx| open_properties(&handle, 1, window, cx));
    });
    cx.run_until_parked();
    assert!(is_shown("field-Resistance", &mut cx));
}

#[gpui_kit::test]
fn unknown_components_open_nothing(cx: &mut TestAppContext) {
    let (editor, mut cx) = editor_window(cx, resistor());
    open(&editor, 99, &mut cx);
    assert!(!is_shown("properties-ok", &mut cx));
}

/// A built-in PNP model other than the default one.
fn other_pnp_model() -> String {
    let default = library::default_bjt(Polarity::P);
    library::bjts()
        .into_iter()
        .map(|entry| entry.model)
        .find(|model| model.polarity == Polarity::P && model.name != default)
        .map(|model| model.name)
        .expect("the library has several PNP models")
}

#[gpui_kit::test]
fn choosing_a_model_in_the_library_updates_the_part(cx: &mut TestAppContext) {
    let original = schematic(&[("Q1", Part::default_for(PartKind::PnpBjt))]);
    let (editor, mut cx) = editor_window(cx, original.clone());
    open(&editor, 1, &mut cx);
    assert!(is_shown("model-name", &mut cx));
    assert!(
        !is_shown("model-unknown", &mut cx),
        "the default model is listed"
    );
    click("choose-model", &mut cx);
    assert!(is_shown("library-search", &mut cx));
    assert!(
        !is_shown("library-item-Resistor", &mut cx),
        "only PNP models"
    );
    let model = other_pnp_model();
    cx.simulate_input(&model);
    cx.run_until_parked();
    click("library-pick", &mut cx);
    assert!(!is_shown("library-search", &mut cx), "the library closed");
    assert!(
        is_shown("properties-ok", &mut cx),
        "the properties stay open"
    );
    click("properties-ok", &mut cx);
    assert_eq!(
        part(&editor, 1, &mut cx),
        Part::Bjt {
            polarity: Polarity::P,
            model
        }
    );
    assert_one_undo_step(&editor, &original, &mut cx);
}

/// Asserts that every control in `selectors` lies inside the dialog surface:
/// across, inside the window's; down, inside the content, which may be
/// taller than the window and scroll (D24), or inside the footer.
fn assert_inside_dialog(selectors: &[&'static str], cx: &mut VisualTestContext) {
    let dialog = cx.debug_bounds("dialog-0").expect("an open dialog");
    let parts: Vec<Bounds<Pixels>> = ["dialog-content", "dialog-footer"]
        .into_iter()
        .filter_map(|part| cx.debug_bounds(part))
        .collect();
    for &selector in selectors {
        let bounds = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} should be visible"));
        let inside = bounds.left() >= dialog.left()
            && bounds.right() <= dialog.right()
            && parts
                .iter()
                .any(|part| bounds.top() >= part.top() && bounds.bottom() <= part.bottom());
        assert!(
            inside,
            "{selector} {bounds:?} leaves the dialog {dialog:?} ({parts:?})"
        );
    }
}

#[gpui_kit::test]
fn source_controls_stay_inside_the_dialog_at_every_ui_scale(cx: &mut TestAppContext) {
    let original = schematic(&[("V1", Part::default_for(PartKind::VoltageSource))]);
    let (editor, mut cx) = editor_window(cx, original);
    for font_size in [12., 16., 20.] {
        cx.update(|_, cx| Theme::update(cx, |theme| theme.font_size = px(font_size)));
        open(&editor, 1, &mut cx);
        click("waveform-PULSE", &mut cx);
        assert_inside_dialog(
            &[
                "field-Designator",
                "field-DC value",
                "field-AC phase",
                "waveform-None",
                "waveform-SFFM",
                "field-Pulsed value",
                "field-Cycles",
                "waveform-preview",
                "properties-cancel",
                "properties-apply",
                "properties-ok",
            ],
            &mut cx,
        );
        keys("escape", &mut cx);
    }
}

#[gpui_kit::test]
fn passive_and_model_controls_stay_inside_the_dialog(cx: &mut TestAppContext) {
    let parts = [
        ("C1", Part::default_for(PartKind::Capacitor)),
        ("D1", Part::default_for(PartKind::Diode)),
    ];
    let (editor, mut cx) = editor_window(cx, schematic(&parts));
    open(&editor, 1, &mut cx);
    assert_inside_dialog(
        &[
            "field-Capacitance",
            "field-Initial voltage",
            "properties-ok",
        ],
        &mut cx,
    );
    keys("escape", &mut cx);
    open(&editor, 2, &mut cx);
    assert_inside_dialog(&["model-name", "choose-model", "properties-ok"], &mut cx);
}

#[gpui_kit::test]
fn a_designator_refused_by_the_editor_is_shown_inline(cx: &mut TestAppContext) {
    let parts = [
        ("R1", Part::default_for(PartKind::Resistor)),
        ("R2", Part::default_for(PartKind::Resistor)),
    ];
    let (editor, mut cx) = editor_window(cx, schematic(&parts));
    open(&editor, 2, &mut cx);
    // R1 is renamed behind the dialog's back, so only the editor knows "Rx" is taken.
    editor
        .update(&mut cx, |editor, cx| {
            editor.apply_component_edit(1, "Rx".into(), Part::default_for(PartKind::Resistor), cx)
        })
        .expect("rename R1");
    let before = editor.read_with(&cx, |e, _| e.schematic().clone());
    type_into("field-Designator", "Rx", &mut cx);
    assert!(
        !is_shown("field-Designator-error", &mut cx),
        "the form cannot know yet"
    );
    click("properties-ok", &mut cx);
    assert!(
        is_shown("field-Designator-error", &mut cx),
        "the refusal is shown inline"
    );
    assert!(is_shown("properties-ok", &mut cx), "the dialog stays open");
    assert_eq!(editor.read_with(&cx, |e, _| e.schematic().clone()), before);
    type_into("field-Designator", "Ry", &mut cx);
    assert!(
        !is_shown("field-Designator-error", &mut cx),
        "editing clears the refusal"
    );
    click("properties-ok", &mut cx);
    let name = editor.read_with(&cx, |e, _| {
        e.schematic().component(2).map(|c| c.name.clone())
    });
    assert_eq!(name.as_deref(), Some("Ry"));
}

/// A click on the editor's window, under the dialog, neither closes the
/// dialog nor reaches the editor: it brings the dialog to the front.
#[gpui_kit::test]
fn clicking_outside_the_dialog_keeps_the_form(cx: &mut TestAppContext) {
    let (editor, mut cx) = editor_window(cx, resistor());
    open(&editor, 1, &mut cx);
    type_into("field-Resistance", "47", &mut cx);
    let mut main = VisualTestContext::from_window(main_window(&cx), &cx.cx);
    let outside = gpui_kit::point(px(60.), px(840.));
    main.simulate_mouse_move(outside, None, Modifiers::default());
    main.simulate_click(outside, Modifiers::default());
    main.run_until_parked();
    assert_eq!(
        editor.read_with(&main, |e, _| e.selection()),
        Vec::new(),
        "the editor got no click"
    );
    assert_eq!(
        main.update(|_, cx| cx.active_window()),
        dialog(&main),
        "the dialog came to the front"
    );
    assert!(is_shown("properties-ok", &mut cx), "the dialog stays open");
    click("properties-ok", &mut cx);
    assert_eq!(
        part(&editor, 1, &mut cx),
        Part::Resistor { resistance: 47.0 }
    );
}

#[gpui_kit::test]
fn enter_in_the_model_library_picks_and_returns_to_the_form(cx: &mut TestAppContext) {
    let original = schematic(&[("Q1", Part::default_for(PartKind::PnpBjt))]);
    let (editor, mut cx) = editor_window(cx, original);
    open(&editor, 1, &mut cx);
    click("choose-model", &mut cx);
    let model = other_pnp_model();
    cx.simulate_input(&model);
    keys("enter", &mut cx);
    assert!(
        !is_shown("library-search", &mut cx),
        "Enter closes only the library"
    );
    assert!(
        is_shown("properties-ok", &mut cx),
        "the properties stay open"
    );
    click("properties-ok", &mut cx);
    assert_eq!(
        part(&editor, 1, &mut cx),
        Part::Bjt {
            polarity: Polarity::P,
            model
        }
    );
}

fn voltage_source() -> Schematic {
    schematic(&[("V1", Part::default_for(PartKind::VoltageSource))])
}

/// Presses and releases each of the space-separated `keys`, as a keyboard
/// does: a focused button is pressed on the release.
/// Presses and releases each of the space-separated `keys` in the window
/// with the keyboard. A press that closes that window leaves the release
/// nowhere, as it is released in another window.
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

fn schematic_of(editor: &Entity<SchematicEditor>, cx: &mut VisualTestContext) -> Schematic {
    editor.read_with(cx, |e, _| e.schematic().clone())
}

/// Each waveform type button is a tab stop that Enter and Space press;
/// Enter on one switches the type instead of applying the dialog.
#[gpui_kit::test]
fn the_waveform_types_work_from_the_keyboard(cx: &mut TestAppContext) {
    let original = voltage_source();
    let (editor, mut cx) = editor_window(cx, original.clone());
    open(&editor, 1, &mut cx);
    // From the DC value: AC magnitude, AC phase, then None, PULSE, SINE.
    press("tab tab tab tab tab", &mut cx);
    press("enter", &mut cx);
    assert!(is_shown("field-Frequency", &mut cx), "SINE chosen");
    assert!(is_shown("properties-ok", &mut cx), "the dialog stays open");
    // The keyboard stays on SINE; EXP is next.
    press("tab", &mut cx);
    press("enter", &mut cx);
    assert!(is_shown("field-Rise time constant", &mut cx), "EXP chosen");
    assert!(!is_shown("field-Frequency", &mut cx));
    assert!(is_shown("properties-ok", &mut cx), "the dialog stays open");
    assert_eq!(schematic_of(&editor, &mut cx), original, "nothing applied");
    press("shift-tab", &mut cx);
    press("space", &mut cx);
    assert!(is_shown("field-Frequency", &mut cx), "SINE chosen again");
    press("tab tab", &mut cx);
    press("enter", &mut cx);
    assert!(is_shown("pwl-time-0", &mut cx), "PWL chosen");
    assert!(is_shown("properties-ok", &mut cx));
    assert!(!editor.read_with(&cx, |e, _| e.can_undo()));

    click("properties-ok", &mut cx);
    let Part::VoltageSource { source } = part(&editor, 1, &mut cx) else {
        panic!("still a voltage source");
    };
    assert!(matches!(source.waveform, Some(Waveform::Pwl { .. })));
}

/// Enter on a focused dialog button presses it rather than confirming the
/// dialog: Cancel discards a valid edit, Apply keeps the dialog open. The
/// window's close button is a window control, no tab stop (D22): Tab goes
/// from OK round to the designator, where Enter applies; a click on the
/// close button discards the edit.
#[gpui_kit::test]
fn enter_presses_the_focused_dialog_button(cx: &mut TestAppContext) {
    let original = resistor();
    let (editor, mut cx) = editor_window(cx, original.clone());
    open(&editor, 1, &mut cx);
    type_into("field-Resistance", "4.7k", &mut cx);
    // Resistance, then Cancel.
    press("tab", &mut cx);
    press("enter", &mut cx);
    assert!(!is_shown("properties-ok", &mut cx), "Cancel closed it");
    assert_eq!(schematic_of(&editor, &mut cx), original, "nothing applied");

    open(&editor, 1, &mut cx);
    type_into("field-Resistance", "4.7k", &mut cx);
    // Resistance, Cancel, then Apply.
    press("tab tab", &mut cx);
    press("enter", &mut cx);
    assert!(is_shown("properties-ok", &mut cx), "Apply keeps it open");
    assert_eq!(
        part(&editor, 1, &mut cx),
        Part::Resistor {
            resistance: si("4.7k")
        }
    );
    press("escape", &mut cx);
    assert_one_undo_step(&editor, &original, &mut cx);

    open(&editor, 1, &mut cx);
    type_into("field-Resistance", "2.2k", &mut cx);
    click("dialog-close", &mut cx);
    assert!(
        !is_shown("properties-ok", &mut cx),
        "the close button closed it"
    );
    assert_eq!(schematic_of(&editor, &mut cx), original, "nothing applied");

    open(&editor, 1, &mut cx);
    type_into("field-Resistance", "2.2k", &mut cx);
    // Resistance, Cancel, Apply, OK, then round to the designator.
    press("tab tab tab tab", &mut cx);
    press("enter", &mut cx);
    assert!(!is_shown("properties-ok", &mut cx), "Enter applied");
    assert_eq!(
        part(&editor, 1, &mut cx),
        Part::Resistor {
            resistance: si("2.2k")
        }
    );
}

/// Enter in a text field applies the form only when every field is valid.
#[gpui_kit::test]
fn enter_in_a_field_applies_only_a_valid_form(cx: &mut TestAppContext) {
    let original = voltage_source();
    let (editor, mut cx) = editor_window(cx, original.clone());
    open(&editor, 1, &mut cx);
    type_into("field-DC value", "5x", &mut cx);
    press("enter", &mut cx);
    assert!(is_shown("field-DC value-error", &mut cx));
    assert!(is_shown("properties-ok", &mut cx), "the dialog stays open");
    assert_eq!(schematic_of(&editor, &mut cx), original);

    // An invalid field elsewhere blocks Enter in a valid one too.
    type_into("field-DC value", "5", &mut cx);
    type_into("field-AC magnitude", "-1", &mut cx);
    click("field-DC value", &mut cx);
    press("enter", &mut cx);
    assert!(is_shown("field-AC magnitude-error", &mut cx));
    assert!(is_shown("properties-ok", &mut cx));
    assert_eq!(schematic_of(&editor, &mut cx), original);

    type_into("field-AC magnitude", "1", &mut cx);
    press("enter", &mut cx);
    assert!(!is_shown("properties-ok", &mut cx), "applied and closed");
    let Part::VoltageSource { source } = part(&editor, 1, &mut cx) else {
        panic!("still a voltage source");
    };
    assert_eq!((source.dc, source.ac_magnitude), (5.0, 1.0));
}

/// INT-4: a designator becomes a probe argument such as `I(R1)`, so only
/// letters, digits and `_` are accepted.
#[gpui_kit::test]
fn a_designator_with_other_characters_is_rejected_inline(cx: &mut TestAppContext) {
    let original = resistor();
    let (editor, mut cx) = editor_window(cx, original.clone());
    open(&editor, 1, &mut cx);
    for name in ["R(1)", "R1,2", "R-1"] {
        type_into("field-Designator", name, &mut cx);
        assert!(is_shown("field-Designator-error", &mut cx), "{name}");
        press("enter", &mut cx);
        assert!(is_shown("properties-ok", &mut cx), "{name}: stays open");
        assert_eq!(schematic_of(&editor, &mut cx), original, "{name}");
    }
    type_into("field-Designator", "R_load2", &mut cx);
    assert!(!is_shown("field-Designator-error", &mut cx));
    press("enter", &mut cx);
    let name = editor.read_with(&cx, |e, _| {
        e.schematic().component(1).map(|c| c.name.clone())
    });
    assert_eq!(name.as_deref(), Some("R_load2"));
}

/// D16: capacitors and inductors explain where their initial condition
/// applies; other parts have no such hint.
#[gpui_kit::test]
fn energy_storing_parts_explain_their_initial_condition(cx: &mut TestAppContext) {
    let parts = [
        ("C1", Part::default_for(PartKind::Capacitor)),
        ("L1", Part::default_for(PartKind::Inductor)),
        ("R1", Part::default_for(PartKind::Resistor)),
    ];
    let (editor, mut cx) = editor_window(cx, schematic(&parts));
    for (id, shown) in [(1, true), (2, true), (3, false)] {
        open(&editor, id, &mut cx);
        assert_eq!(is_shown("initial-condition-hint", &mut cx), shown, "{id}");
        if shown {
            assert_inside_dialog(&["initial-condition-hint"], &mut cx);
        }
        press("escape", &mut cx);
    }
}

/// D14, D20: the preview has just its "Preview" title (no caption naming
/// the span) and labels its time and value axes, all inside its box, at
/// 75%, 100%, 125% and 200% UI scale.
#[gpui_kit::test]
fn the_source_preview_labels_its_axes_under_a_bare_title(cx: &mut TestAppContext) {
    let (editor, mut cx) = tall_editor_window(cx, voltage_source());
    for font_size in SCALES {
        set_font_size(font_size, &mut cx);
        open(&editor, 1, &mut cx);
        assert!(!is_shown("waveform-preview", &mut cx), "no waveform yet");
        click("waveform-PULSE", &mut cx);
        let preview = cx.debug_bounds("waveform-preview").expect("the preview");
        let at = |selector: &'static str, cx: &mut VisualTestContext| {
            let bounds = shown(selector, cx);
            assert!(
                within(bounds, preview),
                "{selector} {bounds:?} leaves {preview:?}"
            );
            bounds
        };
        let title = at("waveform-preview-title", &mut cx);
        let chart = at("waveform-preview-chart", &mut cx);
        let (max, min) = (
            at("waveform-preview-max", &mut cx),
            at("waveform-preview-min", &mut cx),
        );
        let (start, end) = (
            at("waveform-preview-start", &mut cx),
            at("waveform-preview-end", &mut cx),
        );
        assert!(
            !is_shown("waveform-preview-caption", &mut cx),
            "{font_size}: no caption"
        );
        assert!(title.bottom() <= max.top(), "the title is on top");
        assert!(max.right() <= chart.left() && min.right() <= chart.left());
        assert!(max.top() < min.top(), "the largest value on top");
        assert!(start.top() >= min.bottom() && end.top() >= min.bottom());
        assert!(start.left() < end.left(), "start on the left");
        assert_inside_dialog(&["waveform-preview"], &mut cx);
        // A waveform the form cannot build has no chart, and keeps the
        // preview's height.
        type_into("field-Period", "abc", &mut cx);
        assert!(is_shown("field-Period-error", &mut cx));
        let placeholder = shown("waveform-preview", &mut cx);
        assert!(
            close(placeholder.size.height, preview.size.height),
            "{font_size}: {placeholder:?} vs {preview:?}"
        );
        assert!(!is_shown("waveform-preview-chart", &mut cx));
        assert!(!is_shown("waveform-preview-caption", &mut cx));
        assert!(is_shown("waveform-preview-title", &mut cx));
        // Its note wraps inside the box.
        let note = shown("waveform-preview-invalid", &mut cx);
        assert!(
            within(note, placeholder),
            "{font_size}: {note:?} leaves {placeholder:?}"
        );
        press("escape", &mut cx);
    }
}

/// Font sizes standing for 75%, 100%, 125% and 200% UI scale.
const SCALES: [f32; 4] = [12., 16., 20., 32.];

fn set_font_size(font_size: f32, cx: &mut VisualTestContext) {
    cx.update(|_, cx| Theme::update(cx, |theme| theme.font_size = px(font_size)));
    cx.run_until_parked();
}

/// The bounds of the element with debug selector `selector`, which must be
/// shown.
fn shown(selector: &str, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    follow(cx);
    let key: &'static str = Box::leak(selector.to_owned().into_boxed_str());
    cx.debug_bounds(key)
        .unwrap_or_else(|| panic!("{selector} should be visible"))
}

/// Whether `inner` lies inside `outer`, give or take a pixel of rounding.
fn within(inner: Bounds<Pixels>, outer: Bounds<Pixels>) -> bool {
    let slack = px(1.);
    inner.left() >= outer.left() - slack
        && inner.right() <= outer.right() + slack
        && inner.top() >= outer.top() - slack
        && inner.bottom() <= outer.bottom() + slack
}

/// Equal lengths, give or take a pixel of rounding.
fn close(a: Pixels, b: Pixels) -> bool {
    (a - b).abs() <= px(1.)
}

/// `length` in rems at `font_size`.
fn in_rems(length: Pixels, font_size: f32) -> f32 {
    f32::from(length) / font_size
}

/// A schematic with one source per waveform whose preview labels differ in
/// width: "1 V" / "0 V" (PULSE), "12.5 mA" / "-12.5 mA" (SINE) and a single
/// "2.5 V" (a flat PWL line).
fn preview_sources() -> Schematic {
    let sine = Source::with_waveform(Waveform::Sine {
        offset: 0.0,
        amplitude: 12.5e-3,
        frequency: 1e3,
        delay: 0.0,
        damping: 0.0,
        phase: 0.0,
        cycles: None,
    });
    let flat = Source::with_waveform(Waveform::Pwl {
        points: vec![(0.0, 2.5), (1e-3, 2.5)],
        repeat: false,
    });
    schematic(&[
        ("V1", Part::default_for(PartKind::VoltageSource)),
        ("I1", Part::CurrentSource { source: sine }),
        ("V2", Part::VoltageSource { source: flat }),
    ])
}

/// D20: the plot is balanced in the preview box. The widest value label
/// starts as far from the box's left edge as the curve ends from its right
/// edge; the start and end times line up with the curve's ends; the value
/// labels sit beside the curve's extremes, within its height; and nothing
/// but the usual gap separates the title from the curve's top, or the
/// curve's bottom from the times.
#[gpui_kit::test]
fn the_preview_plot_is_balanced_in_its_box(cx: &mut TestAppContext) {
    let (editor, mut cx) = tall_editor_window(cx, preview_sources());
    for font_size in SCALES {
        set_font_size(font_size, &mut cx);
        let gap = px(0.25 * font_size);
        for (id, extremes) in [(1, 2), (2, 2), (3, 1)] {
            open(&editor, id, &mut cx);
            if id == 1 {
                click("waveform-PULSE", &mut cx);
            }
            let case = format!("{font_size} px, part {id}");
            let preview = shown("waveform-preview", &mut cx);
            let title = shown("waveform-preview-title", &mut cx);
            let chart = shown("waveform-preview-chart", &mut cx);
            let max = shown("waveform-preview-max", &mut cx);
            let min = cx.debug_bounds("waveform-preview-min");
            assert_eq!(usize::from(min.is_some()) + 1, extremes, "{case}");
            let (start, end) = (
                shown("waveform-preview-start", &mut cx),
                shown("waveform-preview-end", &mut cx),
            );
            let labels_left = min.map_or(max.left(), |min| min.left().min(max.left()));
            let left_margin = labels_left - preview.left();
            let right_margin = preview.right() - chart.right();
            assert!(
                close(left_margin, right_margin),
                "{case}: {left_margin:?} left of the labels, {right_margin:?} right of the curve"
            );
            assert!(
                left_margin < px(font_size),
                "{case}: no empty space left of the labels ({left_margin:?})"
            );
            assert!(
                close(start.left(), chart.left()),
                "{case}: {start:?} {chart:?}"
            );
            assert!(
                close(end.right(), chart.right()),
                "{case}: {end:?} {chart:?}"
            );
            assert!(max.right() <= chart.left(), "{case}");
            match min {
                Some(min) => {
                    assert!(close(max.top(), chart.top()), "{case}: max at the top");
                    assert!(
                        close(min.bottom(), chart.bottom()),
                        "{case}: min at the bottom"
                    );
                    assert!(min.right() <= chart.left(), "{case}");
                }
                None => assert!(close(max.center().y, chart.center().y), "{case}: flat"),
            }
            // The title, the usual gap, then the curve's top: no label
            // stands out above it. Likewise below it, then the times.
            assert!(
                close(chart.top() - title.bottom(), gap),
                "{case}: {title:?} then {chart:?}"
            );
            assert!(
                close(start.top() - chart.bottom(), gap),
                "{case}: {chart:?} then {start:?}"
            );
            assert!(
                max.top() >= chart.top() - px(1.)
                    && min.is_none_or(|min| min.bottom() <= chart.bottom() + px(1.)),
                "{case}: the value labels stay within the chart's height"
            );
            assert!(
                close(title.left(), labels_left),
                "{case}: the labels line up with the title"
            );
            press("escape", &mut cx);
        }
    }
}

/// The source fields, at most half as wide as before D20 (12.5 to 13.5 rem,
/// 20.5 rem for the designator).
const SOURCE_FIELDS: [&str; 6] = [
    "field-Designator",
    "field-DC value",
    "field-AC magnitude",
    "field-AC phase",
    "field-Pulsed value",
    "field-Cycles",
];

/// D20: the source dialog is narrower, its input boxes half as wide, and
/// everything stays usable from 75% to 200% UI scale: controls inside the
/// dialog, units inside their boxes, the waveform types on one row.
#[gpui_kit::test]
fn the_source_dialog_is_narrow_with_half_width_fields(cx: &mut TestAppContext) {
    let (editor, mut cx) = tall_editor_window(cx, voltage_source());
    for font_size in SCALES {
        set_font_size(font_size, &mut cx);
        open(&editor, 1, &mut cx);
        click("waveform-PULSE", &mut cx);
        let dialog = shown("dialog-0", &mut cx);
        let width = in_rems(dialog.size.width, font_size);
        assert!(width < 33., "{font_size}: {width} rem, was 44");
        for field in SOURCE_FIELDS {
            let input = shown(field, &mut cx);
            let width = in_rems(input.size.width, font_size);
            assert!(
                (width - 6.5).abs() < 0.1,
                "{font_size} {field}: {width} rem"
            );
        }
        // A long value scrolls inside its box: the box keeps its width and
        // the unit stays visible.
        type_into("field-DC value", "123.456789k", &mut cx);
        for field in ["field-DC value", "field-Pulse width", "field-Period"] {
            let input = shown(field, &mut cx);
            let unit = shown(&format!("{field}-unit"), &mut cx);
            assert!(within(unit, input), "{font_size}: {field} unit");
            let width = in_rems(input.size.width, font_size);
            assert!(
                (width - 6.5).abs() < 0.1,
                "{font_size} {field}: {width} rem"
            );
        }
        let kinds = ["None", "PULSE", "SINE", "EXP", "PWL", "SFFM"].map(|kind| {
            let button = shown(&format!("waveform-{kind}"), &mut cx);
            assert!(within(button, dialog), "{font_size}: {kind}");
            button
        });
        assert!(
            kinds.iter().all(|b| close(b.top(), kinds[0].top())),
            "{font_size}: the waveform types share one row"
        );
        assert_inside_dialog(
            &[
                "field-Designator",
                "field-AC phase",
                "field-Pulsed value",
                "field-Cycles",
                "field-Cycles-message",
                "waveform-preview",
                "properties-cancel",
                "properties-apply",
                "properties-ok",
            ],
            &mut cx,
        );
        // The dialog fits its form exactly: two field columns, then the
        // 15 rem preview reaching the window's 16 px padding.
        let preview = shown("waveform-preview", &mut cx);
        assert!(
            close(dialog.right() - preview.right(), dialog_window::PADDING),
            "{font_size}: {preview:?} in {dialog:?}"
        );
        assert!(
            close(preview.size.width, px(15. * font_size)),
            "{preview:?}"
        );
        // The footer's input hint wraps beside the buttons.
        let hint = shown("properties-hint", &mut cx);
        assert!(
            hint.size.height > px(1.5 * font_size),
            "{font_size}: {hint:?}"
        );
        assert!(hint.right() <= shown("properties-cancel", &mut cx).left());
        click("waveform-PWL", &mut cx);
        assert_inside_dialog(
            &[
                "pwl-time-0",
                "pwl-value-1",
                "pwl-remove-1",
                "pwl-add",
                "pwl-repeat",
                "waveform-preview",
            ],
            &mut cx,
        );
        let add = shown("pwl-add", &mut cx);
        assert!(
            add.right() <= preview.left(),
            "{font_size}: the PWL table stays left of the preview"
        );
        press("escape", &mut cx);
    }
}

/// D20: in the narrow fields, long labels and validation messages wrap
/// inside their column, and the inputs of a row stay in line.
#[gpui_kit::test]
fn long_labels_and_messages_wrap_inside_their_column(cx: &mut TestAppContext) {
    let (editor, mut cx) = tall_editor_window(cx, voltage_source());
    for font_size in SCALES {
        set_font_size(font_size, &mut cx);
        open(&editor, 1, &mut cx);
        click("waveform-EXP", &mut cx);
        // "Rise delay" and "Rise time constant" share a row.
        let short = shown("field-Rise delay-label", &mut cx);
        let long = shown("field-Rise time constant-label", &mut cx);
        let input = shown("field-Rise time constant", &mut cx);
        assert!(
            long.size.height > short.size.height * 1.5,
            "{font_size}: wraps"
        );
        assert!(
            long.right() <= input.right() + px(1.),
            "{font_size}: in its column"
        );
        assert!(long.bottom() <= input.top(), "{font_size}: above its input");
        assert!(
            close(short.bottom(), long.bottom()),
            "{font_size}: each label sits on its input"
        );
        assert!(close(shown("field-Rise delay", &mut cx).top(), input.top()));

        click("waveform-SINE", &mut cx);
        // "Frequency" and "Delay", then "Damping" and "Phase". Measured from
        // the Frequency box, as the dialog may scroll to it at 200%.
        let below_frequency = |cx: &mut VisualTestContext| {
            shown("field-Damping", cx).top() - shown("field-Frequency", cx).top()
        };
        let next_row = below_frequency(&mut cx);
        // Typing passes through invalid text ("-" of "-5", "1e" of "1e-3"):
        // its one-line error does not move the rows below.
        type_into("field-Frequency", "1e", &mut cx);
        let error = shown("field-Frequency-error", &mut cx);
        assert!(
            close(error.size.height, px(font_size)),
            "{font_size}: one line"
        );
        assert!(close(below_frequency(&mut cx), next_row));
        // A longer error wraps inside its column and moves them down.
        type_into("field-Frequency", "0", &mut cx);
        let error = shown("field-Frequency-error", &mut cx);
        let input = shown("field-Frequency", &mut cx);
        assert!(
            error.size.height > px(1.5 * font_size),
            "{font_size}: wraps"
        );
        assert!(
            error.left() >= input.left() - px(1.) && error.right() <= input.right() + px(1.),
            "{font_size}: {error:?} under {input:?}"
        );
        assert!(close(shown("field-Delay", &mut cx).top(), input.top()));
        let next = shown("field-Damping-label", &mut cx);
        assert!(
            error.bottom() <= next.top(),
            "{font_size}: the next row moves down"
        );
        assert!(close(
            shown("field-Damping", &mut cx).top(),
            shown("field-Phase", &mut cx).top()
        ));

        // The longest message, a PULSE period shorter than the pulse.
        click("waveform-PULSE", &mut cx);
        type_into("field-Period", "100u", &mut cx);
        let error = shown("field-Period-error", &mut cx);
        let input = shown("field-Period", &mut cx);
        assert!(
            error.size.height > px(2.5 * font_size),
            "{font_size}: wraps"
        );
        assert!(
            error.right() <= input.right() + px(1.),
            "{font_size}: {error:?}"
        );
        assert!(close(shown("field-Cycles", &mut cx).top(), input.top()));
        assert_inside_dialog(&["field-Period-error", "field-Cycles-message"], &mut cx);
        press("escape", &mut cx);
    }
}

/// D20 narrows only the source dialog: the other parts keep their width
/// and fields that share it, one message line each.
#[gpui_kit::test]
fn other_part_dialogs_keep_their_layout(cx: &mut TestAppContext) {
    let parts = [
        ("R1", Part::default_for(PartKind::Resistor)),
        ("C1", Part::default_for(PartKind::Capacitor)),
        ("L1", Part::default_for(PartKind::Inductor)),
        ("D1", Part::default_for(PartKind::Diode)),
        ("Q1", Part::default_for(PartKind::NpnBjt)),
        ("J1", Part::default_for(PartKind::NJfet)),
    ];
    let fields: [&[&'static str]; 6] = [
        &["field-Designator", "field-Resistance"],
        &[
            "field-Designator",
            "field-Capacitance",
            "field-Initial voltage",
        ],
        &[
            "field-Designator",
            "field-Inductance",
            "field-Initial current",
        ],
        &["field-Designator", "model-name", "choose-model"],
        &["field-Designator", "model-name", "choose-model"],
        &["field-Designator", "model-name", "choose-model"],
    ];
    let (editor, mut cx) = tall_editor_window(cx, schematic(&parts));
    for font_size in SCALES {
        set_font_size(font_size, &mut cx);
        for (id, selectors) in (1..).zip(fields) {
            open(&editor, id, &mut cx);
            let case = format!("{font_size} px, part {id}");
            let dialog = shown("dialog-0", &mut cx);
            assert!(close(dialog.size.width, px(32.5 * font_size)), "{case}");
            // Two columns sharing the content's width (32.5 rem less the
            // window's 2 × 16 px padding) and a 0.75 rem gap.
            let column =
                (px(32.5 * font_size) - dialog_window::PADDING * 2. - px(0.75 * font_size)) / 2.;
            let designator = shown("field-Designator", &mut cx);
            assert!(
                close(designator.size.width, column),
                "{case}: {designator:?}"
            );
            let message = shown("field-Designator-message", &mut cx);
            assert!(
                close(message.size.height, px(font_size)),
                "{case}: one line"
            );
            // An invalid number keeps its message on one line, so the
            // dialog does not grow.
            if id == 1 {
                type_into("field-Resistance", "abc", &mut cx);
                let error = shown("field-Resistance-error", &mut cx);
                assert!(close(error.size.height, px(font_size)), "{case}: one line");
                let grown = shown("dialog-0", &mut cx);
                assert!(close(grown.size.height, dialog.size.height), "{case}");
            }
            assert_inside_dialog(selectors, &mut cx);
            let values: Vec<_> = selectors[1..]
                .iter()
                .filter(|s| s.starts_with("field-"))
                .map(|s| shown(s, &mut cx))
                .collect();
            for value in &values {
                assert!(close(value.size.width, column), "{case}: {value:?}");
                assert!(close(value.top(), values[0].top()), "{case}: one row");
            }
            press("escape", &mut cx);
        }
    }
}

/// `text` as a debug selector the test helpers take.
fn leaked(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

/// Whether `inner` lies between the left and right edges of `outer`, give
/// or take a pixel of rounding.
fn within_x(inner: Bounds<Pixels>, outer: Bounds<Pixels>) -> bool {
    inner.left() >= outer.left() - px(1.) && inner.right() <= outer.right() + px(1.)
}

/// A 0 → 1 V pulse train, so the transient section shows without a click
/// on a waveform button (which a scrolled or read-only dialog may not
/// allow).
fn pulse_source() -> Schematic {
    let pulse = Source::with_waveform(Waveform::Pulse {
        initial: 0.0,
        pulsed: 1.0,
        delay: 0.0,
        rise: 1e-9,
        fall: 1e-9,
        width: 500e-6,
        period: 1e-3,
        cycles: None,
    });
    schematic(&[("V1", Part::VoltageSource { source: pulse })])
}

/// The PULSE fields in form order.
const PULSE_FIELDS: [&str; 8] = [
    "Initial value",
    "Pulsed value",
    "Delay",
    "Rise time",
    "Fall time",
    "Pulse width",
    "Period",
    "Cycles",
];

/// D20 at 200%, with the dialog in a window of its own (D22) over the app's
/// smallest window (900 × 560, see crates/desktop) or a 1200 × 900 one: the
/// dialog keeps its form's width, stays on the 1920 × 1080 test screen and,
/// being taller than the screen, scrolls its content above the footer,
/// whose buttons stay in view (D24). The fields keep their width and
/// nothing is cut off at the sides.
fn assert_source_dialog_fits(width: f32, height: f32, cx: &mut TestAppContext) {
    let font_size = 32.;
    let (editor, mut cx) = editor_window_sized(cx, pulse_source(), width, height);
    set_font_size(font_size, &mut cx);
    open(&editor, 1, &mut cx);
    let case = format!("{width} × {height}");
    let (window, screen, viewport) = cx.update(|window, cx| {
        let screen = window.display(cx).map(|display| display.bounds());
        (window.bounds(), screen, window.viewport_size())
    });
    let screen = screen.expect("a screen");
    assert!(
        within(window, screen),
        "{case}: {window:?} leaves {screen:?}"
    );
    let dialog = shown("dialog-0", &mut cx);
    assert!(
        dialog.left() >= px(0.) && dialog.right() <= viewport.width,
        "{case}: {dialog:?} is cut off at the sides of {viewport:?}"
    );
    assert!(
        close(
            dialog.size.width,
            px(font_size * 29.75) + dialog_window::PADDING * 2.
        ),
        "{case}: the form's width {dialog:?}"
    );
    let content = shown("dialog-content", &mut cx);
    let footer = shown("dialog-footer", &mut cx);
    assert!(
        content.bottom() > footer.top(),
        "{case}: taller than the screen, so it scrolls above the footer"
    );
    assert!(
        within(
            dialog,
            Bounds::new(gpui_kit::point(px(0.), px(0.)), viewport)
        ),
        "{case}: {dialog:?} in {viewport:?}"
    );
    for button in ["properties-cancel", "properties-apply", "properties-ok"] {
        let bounds = shown(button, &mut cx);
        assert!(within(bounds, dialog), "{case}: {button} {bounds:?}");
        assert!(within(bounds, footer), "{case}: {button} {bounds:?}");
    }
    assert_fully_shown(&case, &mut cx);
    let hint = shown("properties-hint", &mut cx);
    assert!(within_x(hint, dialog), "{case}: {hint:?}");
    for kind in ["None", "PULSE", "SINE", "EXP", "PWL", "SFFM"] {
        let button = shown(&format!("waveform-{kind}"), &mut cx);
        assert!(within_x(button, dialog), "{case}: {kind} {button:?}");
    }
    let fields = SOURCE_FIELDS
        .into_iter()
        .map(str::to_owned)
        .chain(PULSE_FIELDS.map(|label| format!("field-{label}")));
    for field in fields {
        let input = shown(&field, &mut cx);
        assert!(within_x(input, dialog), "{case}: {field} {input:?}");
        let rems = in_rems(input.size.width, font_size);
        assert!((rems - 6.5).abs() < 0.1, "{case}: {field} {rems} rem");
    }
    let preview = shown("waveform-preview", &mut cx);
    assert!(within_x(preview, dialog), "{case}: {preview:?}");
    assert!(
        in_rems(preview.size.width, font_size) >= 8.,
        "{case}: the preview keeps room for its plot ({preview:?})"
    );
    for part in [
        "waveform-preview-chart",
        "waveform-preview-max",
        "waveform-preview-min",
        "waveform-preview-start",
        "waveform-preview-end",
    ] {
        let bounds = shown(part, &mut cx);
        assert!(within_x(bounds, preview), "{case}: {part} {bounds:?}");
    }
    assert!(
        shown("waveform-preview-start", &mut cx).right()
            < shown("waveform-preview-end", &mut cx).left(),
        "{case}: the times do not collide"
    );
    cx.simulate_event(ScrollEvent {
        position: gpui_kit::point(viewport.width / 2., viewport.height / 2.),
        delta: ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-4000.))),
        ..ScrollEvent::default()
    });
    cx.run_until_parked();
    let ok = shown("properties-ok", &mut cx);
    assert!(
        ok.top() >= px(0.) && ok.bottom() <= viewport.height,
        "{case}: scrolled into view {ok:?}"
    );
    click("properties-ok", &mut cx);
    assert!(!is_shown("properties-ok", &mut cx), "{case}: OK closes");
}

#[gpui_kit::test]
fn the_source_dialog_fits_the_smallest_window_at_200_percent(cx: &mut TestAppContext) {
    assert_source_dialog_fits(900., 560., cx);
}

#[gpui_kit::test]
fn the_source_dialog_fits_a_1200_by_900_window_at_200_percent(cx: &mut TestAppContext) {
    assert_source_dialog_fits(1200., 900., cx);
}

/// D20: the PWL table shares the fields' column beside the preview, so its
/// boxes are the narrowest of the dialog. Their units stay visible, and an
/// order error and the repeat error wrap inside the column.
#[gpui_kit::test]
fn the_pwl_table_stays_usable_in_the_narrow_column(cx: &mut TestAppContext) {
    let parts = [
        ("V1", Part::default_for(PartKind::VoltageSource)),
        ("I1", Part::default_for(PartKind::CurrentSource)),
    ];
    let (editor, mut cx) = tall_editor_window(cx, schematic(&parts));
    for font_size in SCALES {
        set_font_size(font_size, &mut cx);
        for id in [1, 2] {
            let case = format!("{font_size} px, part {id}");
            open(&editor, id, &mut cx);
            click("waveform-PWL", &mut cx);
            let preview = shown("waveform-preview", &mut cx);
            let dialog = shown("dialog-0", &mut cx);
            // The fields' 13.75 rem less the remove button and two gaps.
            for field in ["pwl-time-1", "pwl-value-1"] {
                let input = shown(field, &mut cx);
                let rems = in_rems(input.size.width, font_size);
                assert!((rems - 5.5).abs() < 0.1, "{case}: {field} {rems} rem");
                assert!(input.right() <= preview.left(), "{case}: {field}");
            }
            type_into("pwl-value-1", "-123.456789m", &mut cx);
            type_into("pwl-time-1", "0.00123456789", &mut cx);
            for field in ["pwl-time-1", "pwl-value-1"] {
                let input = shown(field, &mut cx);
                let unit = shown(&format!("{field}-unit"), &mut cx);
                assert!(within(unit, input), "{case}: {field} {unit:?} {input:?}");
                let rems = in_rems(input.size.width, font_size);
                assert!((rems - 5.5).abs() < 0.1, "{case}: {field} {rems} rem");
            }
            // The first point is at 0 s: a second one there is out of order.
            type_into("pwl-time-1", "0", &mut cx);
            let error = shown("pwl-time-1-error", &mut cx);
            let time = shown("pwl-time-1", &mut cx);
            assert!(
                error.size.height > px(1.5 * font_size),
                "{case}: wraps ({error:?})"
            );
            assert!(within_x(error, time), "{case}: {error:?} under {time:?}");
            assert!(close(shown("pwl-value-1", &mut cx).top(), time.top()));
            assert!(shown("pwl-add", &mut cx).top() >= error.bottom(), "{case}");
            assert_inside_dialog(&["pwl-time-1-error", "pwl-add", "pwl-repeat"], &mut cx);
            // One point cannot repeat.
            click("pwl-remove-1", &mut cx);
            let checkbox = shown("pwl-repeat", &mut cx);
            click("pwl-repeat", &mut cx);
            let error = shown("pwl-repeat-error", &mut cx);
            let moved = shown("pwl-repeat", &mut cx);
            assert!(
                close(moved.top(), checkbox.top()) && close(moved.left(), checkbox.left()),
                "{case}: the checkbox stays under the pointer ({checkbox:?} -> {moved:?})"
            );
            assert!(
                error.top() >= moved.bottom(),
                "{case}: the error is below it"
            );
            let column = shown("pwl-time-0", &mut cx);
            let column_right = column.left() + px(13.75 * font_size);
            assert!(
                close(moved.right(), column_right),
                "{case}: the checkbox keeps to the right ({moved:?})"
            );
            assert!(
                error.size.height > px(1.5 * font_size),
                "{case}: wraps ({error:?})"
            );
            assert!(
                error.left() >= column.left() - px(1.) && error.right() <= preview.left(),
                "{case}: {error:?} stays in the fields' column"
            );
            assert!(within(error, dialog), "{case}");
            press("escape", &mut cx);
        }
    }
}

/// D20: every waveform type's units stay visible in their half-width
/// boxes, including the widest ("1/s", "Hz", "°") with a long value typed.
#[gpui_kit::test]
fn every_unit_stays_inside_its_narrow_box(cx: &mut TestAppContext) {
    let kinds: [(&str, &[&str]); 4] = [
        ("PULSE", &PULSE_FIELDS[..7]),
        (
            "SINE",
            &[
                "Offset",
                "Amplitude",
                "Frequency",
                "Delay",
                "Damping",
                "Phase",
            ],
        ),
        (
            "EXP",
            &[
                "Initial value",
                "Pulsed value",
                "Rise delay",
                "Rise time constant",
                "Fall delay",
                "Fall time constant",
            ],
        ),
        (
            "SFFM",
            &[
                "Offset",
                "Amplitude",
                "Carrier frequency",
                "Signal frequency",
            ],
        ),
    ];
    let (editor, mut cx) = tall_editor_window(cx, voltage_source());
    for font_size in SCALES {
        set_font_size(font_size, &mut cx);
        open(&editor, 1, &mut cx);
        for (kind, labels) in kinds {
            click(leaked(format!("waveform-{kind}")), &mut cx);
            for label in labels {
                let case = format!("{font_size} px, {kind} {label}");
                let input = shown(&format!("field-{label}"), &mut cx);
                let unit = shown(&format!("field-{label}-unit"), &mut cx);
                assert!(within(unit, input), "{case}: {unit:?} {input:?}");
                let rems = in_rems(input.size.width, font_size);
                assert!((rems - 6.5).abs() < 0.1, "{case}: {rems} rem");
            }
        }
        click("waveform-SINE", &mut cx);
        for label in ["Frequency", "Damping", "Phase"] {
            let field = leaked(format!("field-{label}"));
            type_into(field, "-123.456789k", &mut cx);
            let input = shown(field, &mut cx);
            let unit = shown(&format!("{field}-unit"), &mut cx);
            assert!(within(unit, input), "{font_size} px, {label}: {unit:?}");
            let rems = in_rems(input.size.width, font_size);
            assert!((rems - 6.5).abs() < 0.1, "{font_size} px, {label}: {rems}");
        }
        press("escape", &mut cx);
    }
}

/// D20: the source's designator box is narrow too. A taken name's message
/// wraps under it, and a current source's dialog is as narrow as a
/// voltage source's.
#[gpui_kit::test]
fn a_taken_source_designator_wraps_under_its_narrow_box(cx: &mut TestAppContext) {
    let parts = [
        ("V1", Part::default_for(PartKind::VoltageSource)),
        ("V2", Part::default_for(PartKind::VoltageSource)),
        ("I1", Part::default_for(PartKind::CurrentSource)),
    ];
    let original = schematic(&parts);
    let (editor, mut cx) = tall_editor_window(cx, original.clone());
    for font_size in SCALES {
        set_font_size(font_size, &mut cx);
        open(&editor, 3, &mut cx);
        let current = shown("dialog-0", &mut cx);
        press("escape", &mut cx);
        open(&editor, 2, &mut cx);
        let dialog = shown("dialog-0", &mut cx);
        assert!(
            close(current.size.width, dialog.size.width),
            "{font_size}: {current:?} vs {dialog:?}"
        );
        type_into("field-Designator", "V1", &mut cx);
        let error = shown("field-Designator-error", &mut cx);
        let input = shown("field-Designator", &mut cx);
        assert!(
            error.size.height > px(1.5 * font_size),
            "{font_size}: wraps ({error:?})"
        );
        assert!(within_x(error, input), "{font_size}: {error:?} {input:?}");
        assert_inside_dialog(
            &["field-Designator-error", "field-DC value", "properties-ok"],
            &mut cx,
        );
        click("properties-ok", &mut cx);
        assert!(
            is_shown("properties-ok", &mut cx),
            "{font_size}: stays open"
        );
        assert_eq!(schematic_of(&editor, &mut cx), original, "{font_size}");
        press("escape", &mut cx);
    }
}

/// D20 in a read-only project: the source dialog keeps its narrow layout,
/// with the read-only note beside Close and every control inside.
#[gpui_kit::test]
fn the_read_only_source_dialog_is_narrow_too(cx: &mut TestAppContext) {
    let (editor, mut cx) = tall_editor_window(cx, pulse_source());
    editor.update(&mut cx, |editor, cx| editor.set_read_only(true, cx));
    for font_size in SCALES {
        set_font_size(font_size, &mut cx);
        open(&editor, 1, &mut cx);
        let dialog = shown("dialog-0", &mut cx);
        let width = in_rems(dialog.size.width - dialog_window::PADDING * 2., font_size);
        assert!((width - 29.75).abs() < 0.1, "{font_size}: {width} rem");
        for field in ["field-Designator", "field-DC value", "field-Period"] {
            let rems = in_rems(shown(field, &mut cx).size.width, font_size);
            assert!((rems - 6.5).abs() < 0.1, "{font_size}: {field} {rems}");
        }
        assert_inside_dialog(
            &[
                "field-Designator",
                "field-AC phase",
                "field-Cycles",
                "waveform-SFFM",
                "waveform-preview",
                "properties-hint",
                "properties-close",
            ],
            &mut cx,
        );
        let hint = shown("properties-hint", &mut cx);
        assert!(hint.right() <= shown("properties-close", &mut cx).left());
        press("escape", &mut cx);
        assert!(!is_shown("properties-close", &mut cx), "{font_size}");
    }
}

/// The field grid lays out labels, boxes and messages in rows of their
/// own, yet Tab still walks the boxes in form order: along a row, then on
/// to the next.
#[gpui_kit::test]
fn tab_walks_the_source_fields_row_by_row(cx: &mut TestAppContext) {
    let (editor, mut cx) = editor_window(cx, pulse_source());
    for steps in 1..PULSE_FIELDS.len() {
        open(&editor, 1, &mut cx);
        click("field-Initial value", &mut cx);
        press(&vec!["tab"; steps].join(" "), &mut cx);
        cx.simulate_input("x");
        cx.run_until_parked();
        for (n, label) in PULSE_FIELDS.into_iter().enumerate() {
            assert_eq!(
                is_shown(leaked(format!("field-{label}-error")), &mut cx),
                n == steps,
                "{steps} tabs from the initial value: {label}"
            );
        }
        press("escape", &mut cx);
    }
}

/// A click on the dialog's title row or on its margin leaves the keyboard
/// on the dialog itself, not on a control: Enter still applies the form
/// there.
#[gpui_kit::test]
fn enter_after_a_click_on_the_dialog_title_still_applies(cx: &mut TestAppContext) {
    let (editor, mut cx) = editor_window(cx, resistor());
    open(&editor, 1, &mut cx);
    type_into("field-Resistance", "330", &mut cx);
    click("dialog-title", &mut cx);
    let dialog = cx.debug_bounds("dialog-0").expect("an open dialog");
    let margin = gpui_kit::point(dialog.left() + px(4.), dialog.top() + px(4.));
    cx.simulate_mouse_move(margin, None, Modifiers::default());
    cx.simulate_click(margin, Modifiers::default());
    cx.run_until_parked();
    press("enter", &mut cx);
    assert!(!is_shown("properties-ok", &mut cx), "applied and closed");
    assert_eq!(
        part(&editor, 1, &mut cx),
        Part::Resistor { resistance: 330.0 }
    );
}

/// A click on a waveform type button takes no focus, so a field of the
/// previous type could keep the keyboard while it is no longer shown: Enter
/// and Escape then reached nothing. The form takes the keyboard instead.
#[gpui_kit::test]
fn enter_applies_after_a_click_hides_the_field_being_typed_in(cx: &mut TestAppContext) {
    let original = voltage_source();
    let (editor, mut cx) = editor_window(cx, original.clone());
    open(&editor, 1, &mut cx);
    click("waveform-SINE", &mut cx);
    type_into("field-Frequency", "2k", &mut cx);
    click("waveform-PULSE", &mut cx);
    assert!(!is_shown("field-Frequency", &mut cx));
    press("enter", &mut cx);
    assert!(!is_shown("properties-ok", &mut cx), "applied and closed");
    let Part::VoltageSource { source } = part(&editor, 1, &mut cx) else {
        panic!("still a voltage source");
    };
    assert!(matches!(source.waveform, Some(Waveform::Pulse { .. })));
    assert_one_undo_step(&editor, &original, &mut cx);
}

#[gpui_kit::test]
fn escape_closes_after_a_click_hides_the_field_being_typed_in(cx: &mut TestAppContext) {
    let original = voltage_source();
    let (editor, mut cx) = editor_window(cx, original.clone());
    open(&editor, 1, &mut cx);
    click("waveform-PWL", &mut cx);
    type_into("pwl-value-1", "3", &mut cx);
    click("waveform-None", &mut cx);
    assert!(!is_shown("pwl-value-1", &mut cx));
    press("escape", &mut cx);
    assert!(!is_shown("properties-ok", &mut cx), "closed");
    assert_eq!(schematic_of(&editor, &mut cx), original, "nothing applied");
}

/// The same for a button of the previous type that has the keyboard (the
/// PWL table's Add point, reached with Tab).
#[gpui_kit::test]
fn enter_and_escape_work_after_a_click_hides_the_focused_button(cx: &mut TestAppContext) {
    let original = voltage_source();
    let (editor, mut cx) = editor_window(cx, original.clone());
    open(&editor, 1, &mut cx);
    click("waveform-PWL", &mut cx);
    click("pwl-value-1", &mut cx);
    // The last row's value, its Remove button, then Add point.
    press("tab tab", &mut cx);
    click("waveform-SINE", &mut cx);
    assert!(!is_shown("pwl-add", &mut cx));
    press("escape", &mut cx);
    assert!(!is_shown("properties-ok", &mut cx), "Escape closes");
    assert_eq!(schematic_of(&editor, &mut cx), original);

    open(&editor, 1, &mut cx);
    click("waveform-PWL", &mut cx);
    click("pwl-value-1", &mut cx);
    press("tab tab", &mut cx);
    click("waveform-SINE", &mut cx);
    press("enter", &mut cx);
    assert!(!is_shown("properties-ok", &mut cx), "applied and closed");
    let Part::VoltageSource { source } = part(&editor, 1, &mut cx) else {
        panic!("still a voltage source");
    };
    assert!(matches!(source.waveform, Some(Waveform::Sine { .. })));
}

/// A field that stays shown keeps the keyboard when the type changes.
#[gpui_kit::test]
fn a_shown_field_keeps_the_keyboard_when_the_type_changes(cx: &mut TestAppContext) {
    let (editor, mut cx) = editor_window(cx, voltage_source());
    open(&editor, 1, &mut cx);
    click("waveform-SINE", &mut cx);
    type_into("field-DC value", "2", &mut cx);
    click("waveform-EXP", &mut cx);
    cx.simulate_input("5");
    cx.run_until_parked();
    press("enter", &mut cx);
    let Part::VoltageSource { source } = part(&editor, 1, &mut cx) else {
        panic!("still a voltage source");
    };
    assert_eq!(source.dc, 25.0, "typed on in the DC field");
}

/// Read-only: Tab reaches Close past the (selectable) fields, and Enter
/// and Escape close the dialog. Space presses only a focused button, so it
/// shows where the keyboard is.
#[gpui_kit::test]
fn the_read_only_dialog_closes_from_the_keyboard(cx: &mut TestAppContext) {
    let original = voltage_source();
    let (editor, mut cx) = editor_window(cx, original.clone());
    editor.update(&mut cx, |editor, cx| editor.set_read_only(true, cx));
    open(&editor, 1, &mut cx);
    press("space", &mut cx);
    assert!(is_shown("properties-close", &mut cx), "Space in a field");
    press("escape", &mut cx);
    // From the DC value: AC magnitude, AC phase, then Close (the waveform
    // types are disabled).
    for keys in ["escape", "enter", "tab tab tab space"] {
        open(&editor, 1, &mut cx);
        assert!(is_shown("properties-close", &mut cx), "{keys}");
        press(keys, &mut cx);
        assert!(!is_shown("properties-close", &mut cx), "{keys} closes");
        assert_eq!(schematic_of(&editor, &mut cx), original, "{keys}");
    }
}

/// Enter and Escape keep working after a PWL point is added or removed
/// with the keyboard in its row.
#[gpui_kit::test]
fn the_keyboard_survives_adding_and_removing_pwl_points(cx: &mut TestAppContext) {
    let original = voltage_source();
    let (editor, mut cx) = editor_window(cx, original.clone());
    open(&editor, 1, &mut cx);
    click("waveform-PWL", &mut cx);
    click("pwl-add", &mut cx);
    cx.simulate_input("3m");
    cx.run_until_parked();
    // The new row's time, its value, then its Remove button.
    press("tab tab", &mut cx);
    press("enter", &mut cx);
    assert!(
        !is_shown("pwl-time-2", &mut cx),
        "removed from the keyboard"
    );
    assert!(is_shown("properties-ok", &mut cx), "the dialog stays open");
    press("escape", &mut cx);
    assert!(!is_shown("properties-ok", &mut cx), "Escape closes");
    assert_eq!(schematic_of(&editor, &mut cx), original);

    open(&editor, 1, &mut cx);
    click("waveform-PWL", &mut cx);
    type_into("pwl-time-1", "2m", &mut cx);
    click("pwl-remove-1", &mut cx);
    press("enter", &mut cx);
    assert!(!is_shown("properties-ok", &mut cx), "applied and closed");
    let Part::VoltageSource { source } = part(&editor, 1, &mut cx) else {
        panic!("still a voltage source");
    };
    assert_eq!(
        source.waveform,
        Some(Waveform::Pwl {
            points: vec![(0.0, 0.0)],
            repeat: false
        })
    );
}

/// Back from the model library, the keyboard is in the form again: Enter
/// applies the chosen model and Escape closes, without a click first.
#[gpui_kit::test]
fn the_keyboard_returns_to_the_form_after_the_model_library(cx: &mut TestAppContext) {
    let original = schematic(&[("Q1", Part::default_for(PartKind::PnpBjt))]);
    let (editor, mut cx) = editor_window(cx, original.clone());
    open(&editor, 1, &mut cx);
    click("choose-model", &mut cx);
    let model = other_pnp_model();
    cx.simulate_input(&model);
    cx.run_until_parked();
    press("enter", &mut cx);
    assert!(!is_shown("library-search", &mut cx), "picked");
    assert!(is_shown("properties-ok", &mut cx));
    assert_eq!(
        cx.update(|_, cx| cx.active_window()),
        dialog(&cx),
        "the properties window is active again"
    );
    press("enter", &mut cx);
    assert!(!is_shown("properties-ok", &mut cx), "applied and closed");
    assert_eq!(
        part(&editor, 1, &mut cx),
        Part::Bjt {
            polarity: Polarity::P,
            model
        }
    );

    open(&editor, 1, &mut cx);
    click("choose-model", &mut cx);
    press("escape", &mut cx);
    assert!(!is_shown("library-search", &mut cx), "the library closed");
    assert!(is_shown("properties-ok", &mut cx), "the properties did not");
    press("escape", &mut cx);
    assert!(!is_shown("properties-ok", &mut cx), "then they close too");
}

/// The field selectors of a switch's model, for the given threshold and
/// hysteresis names (VT/VH or IT/IH).
const VOLTAGE_SWITCH_FIELDS: [&str; 4] = [
    "field-On resistance (RON)",
    "field-Off resistance (ROFF)",
    "field-Threshold (VT)",
    "field-Hysteresis (VH)",
];
const CURRENT_SWITCH_FIELDS: [&str; 4] = [
    "field-On resistance (RON)",
    "field-Off resistance (ROFF)",
    "field-Threshold (IT)",
    "field-Hysteresis (IH)",
];

/// D17: RON, ROFF, VT and VH are typed with SI suffixes and applied as one
/// undoable edit; the model card's name is kept.
#[gpui_kit::test]
fn a_voltage_switch_edits_its_model_and_undoes_in_one_step(cx: &mut TestAppContext) {
    let original = schematic(&[("S1", Part::default_for(PartKind::VoltageSwitch))]);
    let (editor, mut cx) = editor_window(cx, original.clone());
    open(&editor, 1, &mut cx);
    for selector in VOLTAGE_SWITCH_FIELDS {
        assert!(is_shown(selector, &mut cx), "{selector}");
    }
    assert!(is_shown("switch-hint", &mut cx));
    assert!(!is_shown("switch-control", &mut cx), "no control source");
    // RON has the keyboard, its text selected.
    cx.simulate_input("10m");
    type_into("field-Off resistance (ROFF)", "1G", &mut cx);
    type_into("field-Threshold (VT)", "2.5", &mut cx);
    type_into("field-Hysteresis (VH)", "-500m", &mut cx);
    assert!(is_shown("field-Hysteresis (VH)-message", &mut cx));
    press("enter", &mut cx);
    assert!(
        !is_shown("properties-ok", &mut cx),
        "Enter applied and closed"
    );
    assert_eq!(
        part(&editor, 1, &mut cx),
        Part::VoltageSwitch {
            model: xsim_spice_engine::SwitchModel {
                ron: si("10m"),
                roff: 1e9,
                vt: 2.5,
                vh: si("-500m"),
                ..xsim_spice_engine::SwitchModel::default()
            }
        }
    );
    assert_one_undo_step(&editor, &original, &mut cx);
}

/// The engine refuses a zero or negative resistance, so the form does:
/// Enter and OK keep the dialog open with the error under the field.
#[gpui_kit::test]
fn an_invalid_switch_value_blocks_applying(cx: &mut TestAppContext) {
    let original = schematic(&[("W1", Part::default_for(PartKind::CurrentSwitch))]);
    let (editor, mut cx) = editor_window(cx, original.clone());
    open(&editor, 1, &mut cx);
    for selector in CURRENT_SWITCH_FIELDS {
        assert!(is_shown(selector, &mut cx), "{selector}");
    }
    type_into("field-On resistance (RON)", "0", &mut cx);
    assert!(is_shown("field-On resistance (RON)-error", &mut cx));
    press("enter", &mut cx);
    click("properties-ok", &mut cx);
    assert!(is_shown("properties-ok", &mut cx), "the dialog stays open");
    assert_eq!(schematic_of(&editor, &mut cx), original);
    type_into("field-On resistance (RON)", "-1", &mut cx);
    type_into("field-Threshold (IT)", "x", &mut cx);
    assert!(is_shown("field-Threshold (IT)-error", &mut cx));
    type_into("field-On resistance (RON)", "2", &mut cx);
    type_into("field-Threshold (IT)", "-2m", &mut cx);
    click("properties-ok", &mut cx);
    assert!(
        !is_shown("properties-ok", &mut cx),
        "a negative IT is legal"
    );
    let Part::CurrentSwitch { model, .. } = part(&editor, 1, &mut cx) else {
        panic!("still a current switch");
    };
    assert_eq!((model.ron, model.it), (2.0, si("-2m")));
}

/// A schematic with two voltage sources, a current source and a current
/// switch controlled by V1 (ids 1–4).
fn current_switch_circuit() -> Schematic {
    schematic(&[
        ("V1", Part::default_for(PartKind::VoltageSource)),
        ("Vsense", Part::default_for(PartKind::VoltageSource)),
        ("I1", Part::default_for(PartKind::CurrentSource)),
        ("W1", Part::default_for(PartKind::CurrentSwitch)),
    ])
}

fn control_of(editor: &Entity<SchematicEditor>, id: Id, cx: &mut VisualTestContext) -> String {
    match part(editor, id, cx) {
        Part::CurrentSwitch { control, .. } => control,
        other => panic!("not a current switch: {other:?}"),
    }
}

/// D17: the control is chosen from a drop-down of the schematic's voltage
/// sources (not its current sources). Picking one with the mouse or the
/// keyboard changes the form only; OK applies it as one undo step.
#[gpui_kit::test]
fn the_control_source_is_picked_from_a_drop_down(cx: &mut TestAppContext) {
    let original = current_switch_circuit();
    let (editor, mut cx) = editor_window(cx, original.clone());
    open(&editor, 4, &mut cx);
    assert!(is_shown("switch-control-label", &mut cx));
    assert!(is_shown("switch-control-message", &mut cx), "V1 is valid");
    click("switch-control", &mut cx);
    assert!(is_shown("switch-control-V1", &mut cx));
    assert!(is_shown("switch-control-Vsense", &mut cx));
    assert!(
        !is_shown("switch-control-I1", &mut cx),
        "a current source has no branch current to sense"
    );
    click("switch-control-Vsense", &mut cx);
    assert!(!is_shown("switch-control-V1", &mut cx), "the menu closed");
    assert!(is_shown("properties-ok", &mut cx), "the dialog stays open");
    assert_eq!(
        schematic_of(&editor, &mut cx),
        original,
        "nothing applied yet"
    );
    click("properties-ok", &mut cx);
    assert!(!is_shown("properties-ok", &mut cx));
    assert_eq!(control_of(&editor, 4, &mut cx), "Vsense");
    assert_one_undo_step(&editor, &original, &mut cx);

    // From the keyboard: RON has the keyboard on opening; Shift-Tab reaches
    // the drop-down, where Enter opens the menu instead of applying, and
    // Down and Enter pick a source.
    open(&editor, 4, &mut cx);
    press("shift-tab", &mut cx);
    press("enter", &mut cx);
    assert!(
        is_shown("switch-control-Vsense", &mut cx),
        "the menu opened"
    );
    assert!(is_shown("properties-ok", &mut cx), "Enter did not apply");
    press("down down", &mut cx);
    press("enter", &mut cx);
    assert!(
        !is_shown("switch-control-Vsense", &mut cx),
        "the menu closed"
    );
    assert!(is_shown("properties-ok", &mut cx), "the dialog stays open");
    assert_eq!(
        schematic_of(&editor, &mut cx),
        original,
        "nothing applied yet"
    );
    // Escape in an open menu closes only the menu.
    press("enter", &mut cx);
    assert!(
        is_shown("switch-control-V1", &mut cx),
        "the menu opened again"
    );
    press("escape", &mut cx);
    assert!(!is_shown("switch-control-V1", &mut cx));
    assert!(
        is_shown("properties-ok", &mut cx),
        "Escape closed only the menu"
    );
    // Enter in a field applies the valid form.
    click("field-On resistance (RON)", &mut cx);
    press("enter", &mut cx);
    assert!(!is_shown("properties-ok", &mut cx), "applied and closed");
    assert_eq!(control_of(&editor, 4, &mut cx), "Vsense");
    assert_one_undo_step(&editor, &original, &mut cx);
}

/// With no voltage source in the schematic the dialog says what to add
/// instead of showing an empty list, and the other values still apply.
#[gpui_kit::test]
fn a_schematic_without_voltage_sources_explains_the_control(cx: &mut TestAppContext) {
    let original = schematic(&[
        ("I1", Part::default_for(PartKind::CurrentSource)),
        ("W1", Part::default_for(PartKind::CurrentSwitch)),
    ]);
    let (editor, mut cx) = editor_window(cx, original);
    open(&editor, 2, &mut cx);
    assert!(!is_shown("switch-control", &mut cx), "nothing to choose");
    assert!(is_shown("switch-control-error", &mut cx));
    assert_inside_dialog(&["switch-control-error", "properties-ok"], &mut cx);
    type_into("field-Threshold (IT)", "2m", &mut cx);
    press("enter", &mut cx);
    assert!(!is_shown("properties-ok", &mut cx), "applied and closed");
    let Part::CurrentSwitch { control, model } = part(&editor, 2, &mut cx) else {
        panic!("still a current switch");
    };
    assert_eq!((control.as_str(), model.it), ("V1", si("2m")));
}

/// When the control source is renamed (or deleted) the switch keeps the
/// old name: its dialog says it is gone, and the netlist names both the
/// switch and the missing control.
#[gpui_kit::test]
fn a_renamed_control_source_is_reported(cx: &mut TestAppContext) {
    // Every pin labelled, so the netlist gets as far as the control.
    let mut circuit = current_switch_circuit();
    let nets = [["in", "0"], ["in", "mid"], ["mid", "0"], ["mid", "0"]];
    for (component, nets) in circuit.components.clone().iter().zip(nets) {
        for (pin, net) in component.pins().into_iter().zip(nets) {
            let id = circuit.allocate_id();
            circuit.labels.push(xsim_project::NetLabel {
                id,
                name: net.into(),
                position: pin,
            });
        }
    }
    assert!(xsim_project::netlist(&circuit).is_ok(), "a valid circuit");
    let (editor, mut cx) = editor_window(cx, circuit);
    open(&editor, 1, &mut cx);
    type_into("field-Designator", "Vin", &mut cx);
    press("enter", &mut cx);
    assert!(!is_shown("properties-ok", &mut cx));
    assert_eq!(control_of(&editor, 4, &mut cx), "V1", "the switch keeps V1");

    open(&editor, 4, &mut cx);
    assert!(is_shown("switch-control-error", &mut cx), "V1 is gone");
    assert!(
        is_shown("switch-control", &mut cx),
        "Vin and Vsense to choose"
    );
    press("escape", &mut cx);

    let error = xsim_project::netlist(&schematic_of(&editor, &mut cx))
        .err()
        .map(|e| e.to_string())
        .unwrap_or_default();
    assert!(
        error.contains("W1") && error.contains("\"V1\""),
        "the netlist names the switch and its control: {error}"
    );
}

/// Both switch dialogs keep their controls inside the dialog at every UI
/// scale, with RON beside ROFF and the threshold beside the hysteresis.
#[gpui_kit::test]
fn switch_dialogs_keep_their_layout_at_every_ui_scale(cx: &mut TestAppContext) {
    let parts = [
        ("V1", Part::default_for(PartKind::VoltageSource)),
        ("S1", Part::default_for(PartKind::VoltageSwitch)),
        ("W1", Part::default_for(PartKind::CurrentSwitch)),
    ];
    let (editor, mut cx) = tall_editor_window(cx, schematic(&parts));
    for font_size in SCALES {
        set_font_size(font_size, &mut cx);
        for (id, fields) in [(2, VOLTAGE_SWITCH_FIELDS), (3, CURRENT_SWITCH_FIELDS)] {
            open(&editor, id, &mut cx);
            let case = format!("{font_size} px, part {id}");
            let mut selectors = vec!["field-Designator", "switch-hint", "properties-ok"];
            selectors.extend(fields);
            if id == 3 {
                selectors.extend(["switch-control", "switch-control-message"]);
            }
            assert_inside_dialog(&selectors, &mut cx);
            let bounds: Vec<_> = fields.iter().map(|s| shown(s, &mut cx)).collect();
            assert!(close(bounds[0].top(), bounds[1].top()), "{case}: RON, ROFF");
            assert!(close(bounds[2].top(), bounds[3].top()), "{case}: threshold");
            assert!(bounds[2].top() > bounds[0].bottom(), "{case}: two rows");
            assert!(close(bounds[0].size.width, bounds[3].size.width), "{case}");
            press("escape", &mut cx);
        }
    }
}

/// The editor's window, seen by a test context of its own.
fn editor_cx(cx: &VisualTestContext) -> VisualTestContext {
    VisualTestContext::from_window(main_window(cx), &cx.cx)
}

/// Clicks the editor's canvas far from every part, which gives the editor
/// the keyboard, then selects R1.
fn focus_editor_and_select_r1(editor: &Entity<SchematicEditor>, cx: &mut VisualTestContext) {
    let empty = gpui_kit::point(px(1000.), px(700.));
    cx.simulate_mouse_move(empty, None, Modifiers::default());
    cx.simulate_click(empty, Modifiers::default());
    cx.run_until_parked();
    editor.update(cx, |editor, cx| {
        editor.set_selection([ObjectRef::Component(1)], cx)
    });
    assert!(
        cx.update(|window, cx| editor.read(cx).focus_handle(cx).is_focused(window)),
        "the editor has the keyboard"
    );
}

/// D22: the properties open in a window of their own, centred across the
/// editor's window and titled like the dialog. The editor's window ignores
/// clicks and keys meanwhile: a click on the canvas does not clear the
/// selection and Delete does not delete it. Escape closes the window
/// without applying, and the editor's window works again.
#[gpui_kit::test]
fn the_properties_open_in_a_window_of_their_own(cx: &mut TestAppContext) {
    let original = resistor();
    let (editor, mut cx) = editor_window(cx, original.clone());
    let mut main = editor_cx(&cx);
    focus_editor_and_select_r1(&editor, &mut main);
    let windows = cx.windows().len();
    let owner = main.update(|window, _| window.bounds());
    open(&editor, 1, &mut cx);
    assert_eq!(cx.windows().len(), windows + 1, "a window of its own");
    assert_ne!(cx.window_handle(), main.window_handle());
    assert_eq!(cx.window_title().as_deref(), Some("R1 \u{b7} Resistor"));
    let bounds = cx.update(|window, _| window.bounds());
    assert!(close(bounds.center().x, owner.center().x), "{bounds:?}");
    assert!(
        (bounds.center().y - owner.center().y).abs() <= px(8.),
        "centred down, within half a rem: {bounds:?} over {owner:?}"
    );
    assert!(within(bounds, owner), "{bounds:?} inside {owner:?}");

    type_into("field-Resistance", "2k", &mut cx);
    let empty = gpui_kit::point(px(1000.), px(700.));
    main.simulate_mouse_move(empty, None, Modifiers::default());
    main.simulate_click(empty, Modifiers::default());
    main.simulate_keystrokes("delete r");
    main.run_until_parked();
    editor.read_with(&main, |editor, _| {
        assert_eq!(editor.selection(), [ObjectRef::Component(1)], "no click");
        assert_eq!(editor.schematic(), &original, "no Delete");
        assert_eq!(editor.tool(), Tool::Select, "no tool shortcut");
    });
    assert!(is_shown("properties-ok", &mut cx), "the dialog stays open");

    press("escape", &mut cx);
    assert_eq!(cx.windows().len(), windows, "Escape closed the window");
    assert_eq!(schematic_of(&editor, &mut cx), original, "nothing applied");
    assert_eq!(
        main.update(|_, cx| cx.active_window()),
        Some(main.window_handle()),
        "the editor's window is active again"
    );
    main.simulate_keystrokes("delete");
    main.run_until_parked();
    assert!(
        editor.read_with(&main, |editor, _| editor.schematic().components.is_empty()),
        "the editor has the keyboard back"
    );
}

/// D22: OK in the window applies the form to the editor in the main window
/// as one undo step and closes the window.
#[gpui_kit::test]
fn ok_in_the_properties_window_applies_to_the_editor(cx: &mut TestAppContext) {
    let original = resistor();
    let (editor, mut cx) = editor_window(cx, original.clone());
    let windows = cx.windows().len();
    open(&editor, 1, &mut cx);
    type_into("field-Resistance", "330", &mut cx);
    click("properties-ok", &mut cx);
    assert_eq!(cx.windows().len(), windows);
    assert_eq!(
        part(&editor, 1, &mut cx),
        Part::Resistor { resistance: 330.0 }
    );
    assert_one_undo_step(&editor, &original, &mut cx);
}

/// D22: the model library opens over the properties, in a third window
/// that the properties window owns and waits for. The properties ignore
/// clicks and keys meanwhile; the pick fills their form and gives them the
/// keyboard back.
#[gpui_kit::test]
fn the_model_library_stacks_over_the_properties(cx: &mut TestAppContext) {
    let original = schematic(&[("Q1", Part::default_for(PartKind::PnpBjt))]);
    let (editor, mut cx) = editor_window(cx, original);
    let windows = cx.windows().len();
    open(&editor, 1, &mut cx);
    let properties = cx.window_handle();
    click("choose-model", &mut cx);
    assert_eq!(cx.windows().len(), windows + 2);
    let library = cx.window_handle();
    assert_ne!(library, properties);
    assert_eq!(
        cx.cx.read(|app| dialog_window::owner_of(library, app)),
        Some(properties)
    );
    assert!(cx.cx.read(|app| dialog_window::is_blocked(properties, app)));
    assert!(
        cx.cx
            .read(|app| dialog_window::is_blocked(main_window(&cx), app))
    );
    let lib_bounds = cx.update(|window, _| window.bounds());
    let mut form = VisualTestContext::from_window(properties, &cx.cx);
    let form_bounds = form.update(|window, _| window.bounds());
    assert!(
        close(lib_bounds.center().x, form_bounds.center().x),
        "centred over the properties"
    );
    // A click and Escape in the properties window do nothing.
    let cancel = form
        .debug_bounds("properties-cancel")
        .expect("the properties' Cancel")
        .center();
    form.simulate_mouse_move(cancel, None, Modifiers::default());
    form.simulate_click(cancel, Modifiers::default());
    form.simulate_keystrokes("escape");
    form.run_until_parked();
    assert_eq!(cx.windows().len(), windows + 2, "both stay open");
    assert_eq!(
        cx.update(|_, cx| cx.active_window()),
        Some(library),
        "the library came to the front"
    );

    let model = other_pnp_model();
    cx.simulate_input(&model);
    cx.run_until_parked();
    click("library-pick", &mut cx);
    assert_eq!(cx.window_handle(), properties, "back in the properties");
    assert_eq!(cx.windows().len(), windows + 1);
    assert_eq!(
        cx.update(|_, cx| cx.active_window()),
        Some(properties),
        "the properties are active again"
    );
    assert!(is_shown("model-name", &mut cx));
    press("enter", &mut cx);
    assert_eq!(cx.windows().len(), windows, "Enter applied and closed");
    assert_eq!(
        part(&editor, 1, &mut cx),
        Part::Bjt {
            polarity: Polarity::P,
            model
        }
    );
}

/// D22: closing the editor's window closes the dialogs over it, the model
/// library over the properties too.
#[gpui_kit::test]
fn closing_the_editor_window_closes_its_dialogs(cx: &mut TestAppContext) {
    let original = schematic(&[("Q1", Part::default_for(PartKind::PnpBjt))]);
    let (editor, mut cx) = editor_window(cx, original);
    open(&editor, 1, &mut cx);
    click("choose-model", &mut cx);
    assert_eq!(cx.windows().len(), 3);
    let mut main = editor_cx(&cx);
    main.update(|window, _| window.remove_window());
    main.run_until_parked();
    assert!(cx.windows().is_empty(), "every window closed");
}

/// D24: the window of each part kind opens at the height its content
/// measures, centred over the editor's window, and shows all of it with
/// its buttons.
#[gpui_kit::test]
fn every_part_window_opens_centred_over_the_editor(cx: &mut TestAppContext) {
    let parts = every_part();
    let (editor, mut cx) = editor_window(cx, schematic(&parts));
    for (n, kind) in PartKind::ALL.into_iter().enumerate() {
        assert_opens_centred(&editor, n as Id + 1, &format!("{kind:?}"), &mut cx);
    }
}

/// One part of each kind, with its default parameters, ids from 1.
fn every_part() -> Vec<(&'static str, Part)> {
    let names = [
        "R1", "C1", "L1", "V1", "I1", "D1", "Q1", "Q2", "J1", "J2", "GND", "S1", "W1",
    ];
    names
        .into_iter()
        .zip(PartKind::ALL)
        .map(|(name, kind)| (name, Part::default_for(kind)))
        .collect()
}

/// Opens part `id` from the editor's window and checks that the window is
/// centred over it, to a pixel, and fully shown.
fn assert_opens_centred(
    editor: &Entity<SchematicEditor>,
    id: Id,
    case: &str,
    cx: &mut VisualTestContext,
) {
    let owner = editor_cx(cx).update(|window, _| window.bounds());
    open(editor, id, cx);
    assert_fully_shown(case, cx);
    let (bounds, rem) = cx.update(|window, _| (window.bounds(), window.rem_size()));
    let off = (bounds.center().y - owner.center().y).abs();
    assert!(
        off <= px(1.),
        "{case}: {off:?} off centre, {} rem high",
        bounds.size.height / rem
    );
    assert!(close(bounds.center().x, owner.center().x), "{case}");
    press("escape", cx);
    assert!(dialog(cx).is_none(), "{case}: closed");
}

/// Checks the dialog with the keyboard against D24: inside the screen's
/// work area, showing all of its content without scrolling unless that is
/// taller than the work area, and its footer, with OK and Cancel (or
/// Close), inside the window.
fn assert_fully_shown(case: &str, cx: &mut VisualTestContext) {
    follow(cx);
    let handle = cx.window_handle();
    let layout = cx
        .cx
        .update(|app| dialog_window::layout_of(handle, app))
        .unwrap_or_else(|| panic!("{case}: no dialog that fits its content"));
    if let Err(problem) = layout.check() {
        panic!("{case}: {problem} ({layout:?})");
    }
    assert_not_cut_off(&layout, case, cx);
    let viewport = layout.viewport;
    let mut buttons = 0;
    for button in ["properties-ok", "properties-cancel", "properties-close"] {
        let Some(bounds) = cx.debug_bounds(button) else {
            continue;
        };
        buttons += 1;
        assert!(
            bounds.top() >= px(0.)
                && bounds.bottom() <= viewport.height + px(1.)
                && bounds.right() <= viewport.width + px(1.),
            "{case}: {button} {bounds:?} is outside the window's {viewport:?}"
        );
    }
    assert!(buttons > 0, "{case}: no OK, Cancel or Close");
}

/// Checks, apart from the dialog's own measurements, that content which
/// fits the room is not cut off: it lies between the title row and the
/// footer, unscrolled.
fn assert_not_cut_off(
    layout: &dialog_window::DialogLayout,
    case: &str,
    cx: &mut VisualTestContext,
) {
    if layout.natural_height > layout.room.size.height {
        return;
    }
    let content = cx.debug_bounds("dialog-content").expect("the content");
    let bar = cx
        .debug_bounds("dialog-title-bar")
        .map_or(px(0.), |bar| bar.bottom());
    let floor = cx
        .debug_bounds("dialog-footer")
        .map_or(layout.viewport.height, |footer| footer.top());
    assert!(
        content.top() >= bar - px(1.) && content.bottom() <= floor + px(1.),
        "{case}: the content {content:?} is cut off between {bar:?} and {floor:?}"
    );
}

/// The owner's screen at 200% display scaling: a 1920 × 1030 px work area
/// (D24); and a smaller one.
const WORK_AREAS: [(f32, f32); 2] = [(1920., 1030.), (1280., 680.)];

/// Simulates a screen whose work area is `width` × `height` and makes the
/// editor's window fill it, as when maximized.
fn simulate_screen(width: f32, height: f32, cx: &mut VisualTestContext) {
    let area = Bounds::new(Point::default(), size(px(width), px(height)));
    let mut main = editor_cx(cx);
    main.update(|window, cx| {
        dialog_window::simulate_work_area(cx, Some(area));
        window.resize(area.size);
        window.bounds_changed(cx);
    });
    main.run_until_parked();
}

/// The model of `part`'s family with the longest description, which takes
/// the most lines.
fn longest_model(part: &Part) -> Option<String> {
    fn longest<M>(entries: Vec<library::ModelEntry<M>>, admits: impl Fn(&M) -> bool) -> M {
        entries
            .into_iter()
            .filter(|entry| admits(&entry.model))
            .max_by_key(|entry| entry.description.len())
            .expect("a model of the family")
            .model
    }
    match part {
        Part::Diode { .. } => Some(longest(library::diodes(), |_| true).name),
        Part::Bjt { polarity, .. } => {
            Some(longest(library::bjts(), |model| model.polarity == *polarity).name)
        }
        Part::Jfet { polarity, .. } => {
            Some(longest(library::jfets(), |model| model.polarity == *polarity).name)
        }
        _ => None,
    }
}

/// D24: on a screen whose work area is `width` × `height`, at `font_size`
/// px (the UI scale), the window of every part opens fully shown, OK and
/// Cancel without scrolling, and stays so through every change its form
/// allows while open: each waveform type of a source, four more PWL points,
/// and the library model with the longest description.
fn assert_every_part_stays_fully_shown(
    screen: (f32, f32),
    font_size: f32,
    cx: &mut TestAppContext,
) {
    assert_every_part_stays_fully_shown_in_a_frame(screen, font_size, None, cx);
}

/// [`assert_every_part_stays_fully_shown`], with windows that lose `frame`
/// (width, height) of their size to the operating system's frame, as on
/// Windows.
fn assert_every_part_stays_fully_shown_in_a_frame(
    (width, height): (f32, f32),
    font_size: f32,
    frame: Option<(f32, f32)>,
    cx: &mut TestAppContext,
) {
    let parts = every_part();
    let (editor, mut cx) = editor_window(cx, schematic(&parts));
    simulate_screen(width, height, &mut cx);
    set_font_size(font_size, &mut cx);
    if let Some((frame_width, frame_height)) = frame {
        cx.update(|_, cx| {
            dialog_window::simulate_window_frame(cx, Some(size(px(frame_width), px(frame_height))))
        });
    }
    for (n, (_, part)) in parts.iter().enumerate() {
        let case = format!(
            "{:?} at {font_size} px on {width} × {height}, frame {frame:?}",
            part.kind()
        );
        open(&editor, n as Id + 1, &mut cx);
        assert_fully_shown(&case, &mut cx);
        if matches!(
            part.kind(),
            PartKind::VoltageSource | PartKind::CurrentSource
        ) {
            for kind in [
                "waveform-PULSE",
                "waveform-SINE",
                "waveform-EXP",
                "waveform-SFFM",
                "waveform-None",
                "waveform-PWL",
            ] {
                click(kind, &mut cx);
                assert_fully_shown(&format!("{case}, {kind}"), &mut cx);
            }
            for point in 1..=4 {
                click("pwl-add", &mut cx);
                assert_fully_shown(&format!("{case}, {point} more points"), &mut cx);
            }
        }
        if let Some(model) = longest_model(part) {
            click("choose-model", &mut cx);
            cx.simulate_input(&model);
            cx.run_until_parked();
            click("library-pick", &mut cx);
            assert!(is_shown("model-name", &mut cx), "{case}: back in the form");
            assert_fully_shown(&format!("{case}, {model}"), &mut cx);
        }
        press("escape", &mut cx);
        assert!(dialog(&cx).is_none(), "{case}: closed");
    }
}

/// D24 on the owner's screen at 200% display scaling, at 100% UI scale.
#[gpui_kit::test]
fn every_part_window_stays_fully_shown_on_the_owners_screen(cx: &mut TestAppContext) {
    assert_every_part_stays_fully_shown(WORK_AREAS[0], 16., cx);
}

/// D24 on the owner's screen at 200% display scaling, at 150% UI scale.
#[gpui_kit::test]
fn every_part_window_stays_fully_shown_on_the_owners_screen_at_150_percent(
    cx: &mut TestAppContext,
) {
    assert_every_part_stays_fully_shown(WORK_AREAS[0], 24., cx);
}

/// D24 on a smaller screen, at 100% UI scale.
#[gpui_kit::test]
fn every_part_window_stays_fully_shown_on_a_small_screen(cx: &mut TestAppContext) {
    assert_every_part_stays_fully_shown(WORK_AREAS[1], 16., cx);
}

/// D24 on a smaller screen, at 150% UI scale, where the tallest forms are
/// taller than the screen and scroll above their buttons.
#[gpui_kit::test]
fn every_part_window_stays_fully_shown_on_a_small_screen_at_150_percent(cx: &mut TestAppContext) {
    assert_every_part_stays_fully_shown(WORK_AREAS[1], 24., cx);
}

/// D24 with Windows' frame at 200% display scaling (13 × 6.5 px taken from
/// the size asked for), on the owner's screen at 100% UI scale and on a
/// smaller one at 150%: the windows make room for it.
#[gpui_kit::test]
fn every_part_window_stays_fully_shown_inside_the_windows_frame(cx: &mut TestAppContext) {
    assert_every_part_stays_fully_shown_in_a_frame(WORK_AREAS[0], 16., Some((13., 6.5)), cx);
}

#[gpui_kit::test]
fn every_part_window_stays_fully_shown_inside_the_windows_frame_on_a_small_screen(
    cx: &mut TestAppContext,
) {
    assert_every_part_stays_fully_shown_in_a_frame(WORK_AREAS[1], 24., Some((13., 6.5)), cx);
}

/// D24 in a read-only project, whose forms cannot change, so their windows
/// keep no room for other states: on both screens, at 100% and 150%.
#[gpui_kit::test]
fn every_read_only_part_window_is_fully_shown(cx: &mut TestAppContext) {
    let parts = every_part();
    let (editor, mut cx) = editor_window(cx, schematic(&parts));
    editor.update(&mut cx, |editor, cx| editor.set_read_only(true, cx));
    for (width, height) in WORK_AREAS {
        simulate_screen(width, height, &mut cx);
        for font_size in [16., 24.] {
            set_font_size(font_size, &mut cx);
            for (n, (_, part)) in parts.iter().enumerate() {
                let case = format!(
                    "read-only {:?} at {font_size} px on {width} × {height}",
                    part.kind()
                );
                assert_opens_where_it_fits(&editor, n as Id + 1, &case, &mut cx);
            }
        }
    }
}

/// Opens part `id` and checks that it is fully shown and, keeping no room
/// for other states, centred over the editor's window but for what keeps
/// it on the screen.
fn assert_opens_where_it_fits(
    editor: &Entity<SchematicEditor>,
    id: Id,
    case: &str,
    cx: &mut VisualTestContext,
) {
    let owner = editor_cx(cx).update(|window, _| window.bounds());
    open(editor, id, cx);
    assert_fully_shown(case, cx);
    let handle = cx.window_handle();
    let layout = cx
        .cx
        .update(|app| dialog_window::layout_of(handle, app))
        .expect("a fitted dialog");
    let (bounds, room) = (layout.bounds, layout.room);
    let centred = (owner.center().y - bounds.size.height / 2.)
        .min(room.bottom() - bounds.size.height)
        .max(room.top());
    assert!(
        (bounds.top() - centred).abs() <= px(1.),
        "{case}: {bounds:?} not centred over {owner:?} in {room:?}"
    );
    press("escape", cx);
}

/// D24: a source's window opens centred whatever its waveform: each type,
/// PWL tables of several lengths, current sources and read-only projects;
/// the same source opened again opens the same way.
#[gpui_kit::test]
fn every_source_window_opens_centred_over_the_editor(cx: &mut TestAppContext) {
    let pulse = Waveform::Pulse {
        initial: 0.0,
        pulsed: 1.0,
        delay: 0.0,
        rise: 1e-6,
        fall: 1e-6,
        width: 1e-3,
        period: 2e-3,
        cycles: None,
    };
    let sine = Waveform::Sine {
        offset: 0.0,
        amplitude: 1.0,
        frequency: 50.0,
        delay: 0.0,
        damping: 0.0,
        phase: 0.0,
        cycles: None,
    };
    let exp = Waveform::Exp {
        initial: 0.0,
        pulsed: 1.0,
        rise_delay: 0.0,
        rise_tau: 1e-3,
        fall_delay: 2e-3,
        fall_tau: 1e-3,
    };
    let sffm = Waveform::Sffm {
        offset: 0.0,
        amplitude: 1.0,
        carrier: 1e3,
        index: 1.0,
        signal: 50.0,
    };
    let pwl = |points: usize| Waveform::Pwl {
        points: (0..points).map(|i| (i as f64 * 1e-3, i as f64)).collect(),
        repeat: false,
    };
    let mut parts: Vec<Part> = [
        pulse.clone(),
        sine,
        exp,
        sffm,
        pwl(1),
        pwl(2),
        pwl(3),
        pwl(6),
    ]
    .into_iter()
    .map(|waveform| Part::VoltageSource {
        source: Source::with_waveform(waveform),
    })
    .collect();
    parts.insert(0, Part::default_for(PartKind::VoltageSource));
    parts.push(Part::default_for(PartKind::CurrentSource));
    parts.push(Part::CurrentSource {
        source: Source::with_waveform(pulse),
    });
    let named: Vec<(String, Part)> = parts
        .iter()
        .enumerate()
        .map(|(n, part)| (format!("X{n}"), part.clone()))
        .collect();
    let refs: Vec<(&str, Part)> = named.iter().map(|(n, p)| (n.as_str(), p.clone())).collect();
    let (editor, mut cx) = editor_window(cx, schematic(&refs));
    for (n, part) in parts.iter().enumerate() {
        assert_opens_centred(&editor, n as Id + 1, &format!("{part:?}"), &mut cx);
    }
    // The DC source after the others, and the pulse after it.
    for id in [1, 2] {
        let case = format!("again: {:?}", parts[id as usize - 1]);
        assert_opens_centred(&editor, id, &case, &mut cx);
    }
    editor.update(&mut cx, |editor, cx| editor.set_read_only(true, cx));
    for id in [1, 2, 8] {
        let case = format!("read-only: {:?}", parts[id as usize - 1]);
        assert_opens_centred(&editor, id, &case, &mut cx);
    }
}

/// D22: the window of a part with a model opens centred whichever model it
/// uses (its description takes one line or two, or it is not in the
/// library), and so do those of the ground, a resistor and a current
/// switch, also in a read-only project.
#[gpui_kit::test]
fn every_model_window_opens_centred_over_the_editor(cx: &mut TestAppContext) {
    /// The models of a family with the shortest and the longest description.
    fn extremes<M>(entries: Vec<library::ModelEntry<M>>) -> Vec<library::ModelEntry<M>> {
        let mut entries = entries;
        entries.sort_by_key(|entry| entry.description.len());
        let longest = entries.pop();
        entries.truncate(1);
        entries.extend(longest);
        entries
    }
    let mut parts: Vec<Part> = Vec::new();
    parts.extend(
        extremes(library::diodes())
            .into_iter()
            .map(|entry| Part::Diode {
                model: entry.model.name,
            }),
    );
    parts.extend(
        extremes(library::bjts())
            .into_iter()
            .map(|entry| Part::Bjt {
                polarity: entry.model.polarity,
                model: entry.model.name,
            }),
    );
    parts.extend(
        extremes(library::jfets())
            .into_iter()
            .map(|entry| Part::Jfet {
                polarity: entry.model.polarity,
                model: entry.model.name,
            }),
    );
    parts.push(Part::Diode {
        model: "NOT_IN_THE_LIBRARY".into(),
    });
    parts.extend(
        [
            PartKind::Ground,
            PartKind::Resistor,
            PartKind::CurrentSwitch,
            PartKind::VoltageSource,
        ]
        .map(Part::default_for),
    );
    let named: Vec<(String, Part)> = parts
        .iter()
        .enumerate()
        .map(|(n, part)| (format!("X{n}"), part.clone()))
        .collect();
    let refs: Vec<(&str, Part)> = named.iter().map(|(n, p)| (n.as_str(), p.clone())).collect();
    let (editor, mut cx) = editor_window(cx, schematic(&refs));
    let models = parts.len() - 4;
    for (n, part) in parts.iter().take(parts.len() - 1).enumerate() {
        assert_opens_centred(&editor, n as Id + 1, &format!("{part:?}"), &mut cx);
    }
    editor.update(&mut cx, |editor, cx| editor.set_read_only(true, cx));
    for n in [0, models, models + 1] {
        let case = format!("read-only: {:?}", parts[n]);
        assert_opens_centred(&editor, n as Id + 1, &case, &mut cx);
    }
}

/// D22: text typed into a blocked window reaches none of its controls. With
/// a library open over the properties, typing and Backspace in the
/// properties leave their focused value alone; once the library has closed,
/// the value takes what is typed.
#[gpui_kit::test]
fn typing_into_the_blocked_properties_changes_nothing(cx: &mut TestAppContext) {
    let (editor, mut cx) = editor_window(cx, resistor());
    open(&editor, 1, &mut cx);
    let properties = cx.window_handle();
    cx.update(|window, cx| open_library(LibraryFilter::All, |_, _, _| {}, window, cx));
    cx.run_until_parked();
    follow(&mut cx);
    assert_ne!(cx.window_handle(), properties, "the library is open");
    let mut form = VisualTestContext::from_window(properties, &cx.cx);
    form.simulate_input("47");
    form.simulate_keystrokes("backspace");
    form.run_until_parked();
    press("escape", &mut cx);
    assert_eq!(cx.window_handle(), properties, "back in the properties");
    cx.simulate_input("47");
    press("enter", &mut cx);
    assert!(dialog(&cx).is_none(), "Enter applied and closed");
    assert_eq!(
        part(&editor, 1, &mut cx),
        Part::Resistor { resistance: 47.0 }
    );
}

/// D22: the menus of a dialog stay inside its window, which is smaller than
/// the main window: a current switch's control menu with many voltage
/// sources scrolls within the window, so every source can be reached.
#[gpui_kit::test]
fn a_long_control_menu_fits_the_properties_window(cx: &mut TestAppContext) {
    let names: Vec<String> = (1..=40).map(|n| format!("V{n}")).collect();
    let mut parts = vec![("W1", Part::default_for(PartKind::CurrentSwitch))];
    parts.extend(
        names
            .iter()
            .map(|name| (name.as_str(), Part::default_for(PartKind::VoltageSource))),
    );
    let (editor, mut cx) = editor_window(cx, schematic(&parts));
    open(&editor, 1, &mut cx);
    click("switch-control", &mut cx);
    let viewport = cx.update(|window, _| window.viewport_size());
    let first = cx
        .debug_bounds("switch-control-V1")
        .expect("the menu is open");
    assert!(first.top() >= px(0.), "{first:?}");
    // Scroll the menu to its end.
    for _ in 0..20 {
        cx.simulate_event(ScrollEvent {
            position: first.center(),
            delta: ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-200.))),
            ..ScrollEvent::default()
        });
    }
    cx.run_until_parked();
    let last = cx
        .debug_bounds("switch-control-V40")
        .expect("the last source is in the menu");
    assert!(
        last.top() >= px(0.) && last.bottom() <= viewport.height,
        "{last:?} in a {viewport:?} window"
    );
    click("switch-control-V40", &mut cx);
    press("enter", &mut cx);
    assert!(
        matches!(part(&editor, 1, &mut cx), Part::CurrentSwitch { control, .. } if control == "V40")
    );
}
