//! Placing parts from the toolbar, the keyboard and the library.

use super::harness::{open, primary};
use crate::library_browser::LibraryItem;
use crate::ui::toolbar::TOOLBAR_PARTS;
use crate::{EditorEvent, Tool};
use gpui_kit::TestAppContext;
use xsim_project::{Part, PartKind, Point, Schematic};
use xsim_spice_engine::Polarity;

fn toolbar_selector(kind: PartKind) -> &'static str {
    match kind {
        PartKind::VoltageSource => "toolbar-part-v",
        PartKind::CurrentSource => "toolbar-part-i",
        PartKind::Resistor => "toolbar-part-r",
        PartKind::Inductor => "toolbar-part-l",
        PartKind::Capacitor => "toolbar-part-c",
        PartKind::Diode => "toolbar-part-d",
        PartKind::NpnBjt => "toolbar-part-q",
        PartKind::PnpBjt => "toolbar-part-qp",
        PartKind::Ground => "toolbar-part-gnd",
        PartKind::NJfet | PartKind::PJfet | PartKind::VoltageSwitch | PartKind::CurrentSwitch => {
            unreachable!("{kind:?} comes from the library (D6)")
        }
    }
}

#[gpui_kit::test]
fn every_toolbar_part_places_with_a_fresh_designator(cx: &mut TestAppContext) {
    let mut h = open(cx, Schematic::default());
    for (i, kind) in TOOLBAR_PARTS.into_iter().enumerate() {
        h.click_control(toolbar_selector(kind));
        assert_eq!(h.read(|e| e.tool()), Tool::Place(kind));
        let x = -16 + 4 * i as i32;
        h.click(x, 0);
        let s = h.schematic();
        let placed = s.components.last().expect("a part was placed");
        assert_eq!(placed.part.kind(), kind);
        assert_eq!(placed.position, Point::new(x, 0));
        assert!(placed.name.starts_with(kind.prefix()), "{}", placed.name);
    }
    assert_eq!(h.schematic().components.len(), TOOLBAR_PARTS.len());
    assert!(h.events().contains(&EditorEvent::Changed));
    h.close();
}

#[gpui_kit::test]
fn placement_repeats_until_escape(cx: &mut TestAppContext) {
    let mut h = open(cx, Schematic::default());
    h.keys("shift-r");
    h.click(0, 0);
    h.click(0, 4);
    let names: Vec<String> = h
        .schematic()
        .components
        .iter()
        .map(|c| c.name.clone())
        .collect();
    assert_eq!(names, ["R1", "R2"]);
    h.keys("escape");
    assert_eq!(h.read(|e| e.tool()), Tool::Select);
    h.click(0, 8);
    assert_eq!(h.schematic().components.len(), 2);
    h.close();
}

#[gpui_kit::test]
fn the_ghost_rotates_and_mirrors_before_placing(cx: &mut TestAppContext) {
    let mut h = open(cx, Schematic::default());
    h.keys("c");
    let p = h.at(2, 2);
    h.hover(p);
    h.keys("r");
    h.click(2, 2);
    h.keys("x");
    h.click(8, 2);
    let s = h.schematic();
    assert_eq!(s.components[0].orientation.rotation, 1);
    assert_eq!(
        s.components[0].pins(),
        vec![Point::new(2, 0), Point::new(2, 4)]
    );
    assert!(s.components[1].orientation.mirror_x);
    h.close();
}

#[gpui_kit::test]
fn sources_start_upright_with_plus_on_top(cx: &mut TestAppContext) {
    let mut h = open(cx, Schematic::default());
    h.keys("v");
    h.click(0, 0);
    let pins = h.schematic().components[0].pins();
    assert_eq!(pins, vec![Point::new(0, -2), Point::new(0, 2)]);
    h.close();
}

#[gpui_kit::test]
fn library_picks_place_the_chosen_model(cx: &mut TestAppContext) {
    let mut h = open(cx, Schematic::default());
    let item = LibraryItem::Bjt {
        polarity: Polarity::P,
        model: "PNP_SWITCH".into(),
    };
    h.update(|e, cx| e.place_library_item(item, cx));
    assert_eq!(h.read(|e| e.tool()), Tool::Place(PartKind::PnpBjt));
    h.click(0, 0);
    let part = h.schematic().components[0].part.clone();
    assert_eq!(
        part,
        Part::Bjt {
            polarity: Polarity::P,
            model: "PNP_SWITCH".into()
        }
    );
    h.close();
}

#[gpui_kit::test]
fn start_placing_uses_the_given_part(cx: &mut TestAppContext) {
    let mut h = open(cx, Schematic::default());
    let part = Part::Resistor { resistance: 47e3 };
    h.update(|e, cx| e.start_placing(part.clone(), cx));
    assert_eq!(h.read(|e| e.placing().cloned()), Some(part.clone()));
    h.click(4, 4);
    assert_eq!(h.schematic().components[0].part, part);
    h.keys(&primary("z"));
    assert!(h.schematic().components.is_empty());
    h.close();
}

#[gpui_kit::test]
fn a_click_right_after_a_shortcut_places_without_moving_the_mouse(cx: &mut TestAppContext) {
    let mut h = open(cx, Schematic::default());
    let p = h.at(4, 4);
    h.hover(p);
    h.keys("shift-r r");
    h.cx.simulate_click(p, gpui_kit::Modifiers::default());
    let s = h.schematic();
    assert_eq!(
        s.components.len(),
        1,
        "keyboard modality must not swallow the click"
    );
    assert_eq!(s.components[0].orientation.rotation, 1);
    h.close();
}

#[gpui_kit::test]
fn jfets_place_from_the_tool_with_their_default_models(cx: &mut TestAppContext) {
    let mut h = open(cx, Schematic::default());
    for (x, kind) in [(0, PartKind::NJfet), (6, PartKind::PJfet)] {
        h.update(|e, cx| e.set_tool(Tool::Place(kind), cx));
        h.click(x, 0);
    }
    let s = h.schematic();
    assert_eq!(s.components[0].part, Part::default_for(PartKind::NJfet));
    assert_eq!(s.components[1].part, Part::default_for(PartKind::PJfet));
    assert_eq!(
        s.components
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["J1", "J2"]
    );
    h.close();
}
