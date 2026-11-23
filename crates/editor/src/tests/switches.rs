//! The voltage- and current-controlled switches (D17): placing them from
//! the component library, netlisting a placed circuit, probing their
//! current and the hover card of a simulated run.

use super::harness::{Harness, open};
use super::probing::probes;
use crate::library_browser::LibraryItem;
use crate::model::measure::MeasurementKind;
use crate::ui::hover::HOVER_DELAY;
use crate::{ObjectRef, ProbeContext, ProbeRequest, Tool};
use gpui_kit::TestAppContext;
use std::sync::Arc;
use xsim_project::{
    Component, NetLabel, Orientation, Part, PartKind, Point, Project, Schematic, netlist,
};
use xsim_spice_engine::{
    Analysis, CurrentSwitchModel, Device, InitialState, SimControl, SimOptions, Source,
    SwitchModel, Transient, Waveform, simulate,
};

/// Opens the component library from the toolbar, searches for `query` and
/// picks the best match with Enter. The library is a window of its own
/// (D22).
fn pick_from_library(h: &mut Harness, query: &str) {
    h.click_control("toolbar-library");
    h.cx.run_until_parked();
    let owner = h.cx.update(|window, _| window.window_handle());
    let dialog =
        h.cx.update(|_, cx| crate::dialog_window::dialog_of(owner, cx))
            .expect("the library opened in a window of its own");
    let mut library = gpui_kit::VisualTestContext::from_window(dialog, &h.cx);
    library.run_until_parked();
    assert!(
        library.debug_bounds("library-search").is_some(),
        "the library opened"
    );
    library.simulate_input(query);
    library.run_until_parked();
    library.simulate_keystrokes("enter");
    library.run_until_parked();
    h.cx.run_until_parked();
    let open =
        h.cx.update(|_, cx| crate::dialog_window::dialog_of(owner, cx));
    assert!(open.is_none(), "the pick closed it");
}

/// D17: both switches are placed from the library window, with their
/// default parameters, the next free S and W designators and the pins of
/// their symbols.
#[gpui_kit::test]
fn switches_are_placed_from_the_component_library(cx: &mut TestAppContext) {
    let mut h = open(cx, Schematic::default());
    pick_from_library(&mut h, "voltage-controlled");
    assert_eq!(h.read(|e| e.tool()), Tool::Place(PartKind::VoltageSwitch));
    h.click(0, 0);
    h.click(0, 12);
    h.keys("escape");
    pick_from_library(&mut h, "csw");
    assert_eq!(h.read(|e| e.tool()), Tool::Place(PartKind::CurrentSwitch));
    h.click(20, 0);
    h.keys("escape");
    let s = h.schematic();
    let summary: Vec<(&str, &Part)> = s
        .components
        .iter()
        .map(|c| (c.name.as_str(), &c.part))
        .collect();
    let voltage = Part::default_for(PartKind::VoltageSwitch);
    let current = Part::default_for(PartKind::CurrentSwitch);
    assert_eq!(
        summary,
        [("S1", &voltage), ("S2", &voltage), ("W1", &current)]
    );
    assert_eq!(
        s.components[0].pins(),
        [
            Point::new(4, -4),
            Point::new(4, 4),
            Point::new(-4, -2),
            Point::new(-4, 2)
        ]
    );
    assert_eq!(
        s.components[2].pins(),
        [Point::new(16, 0), Point::new(24, 0)]
    );
    assert!(h.read(|e| e.can_undo()));
    h.close();
}

/// A pulse from 0 to `high` that is low for the first millisecond, high for
/// the second, and so on.
fn square(high: f64) -> Source {
    Source {
        dc: 0.0,
        ac_magnitude: 0.0,
        ac_phase: 0.0,
        waveform: Some(Waveform::Pulse {
            initial: 0.0,
            pulsed: high,
            delay: 1e-3,
            rise: 1e-6,
            fall: 1e-6,
            width: 1e-3,
            period: 2e-3,
            cycles: None,
        }),
    }
}

/// One part of [`bench`]: designator, position, part and the nets of its
/// pins.
type BenchPart = (&'static str, (i32, i32), Part, &'static [&'static str]);

/// Positions of the switches in [`bench`].
const S1_AT: (i32, i32) = (20, -12);
const W1_AT: (i32, i32) = (20, 12);

/// Two switched loads, every pin joined by a net label:
///
/// * `Vctl` squares 0 / 1 V into S1's control (VT = 0.5 V);
/// * `Vdrv` squares 0 / 2 V through R2 (1 kΩ) into the 0 V `Vsense`, whose
///   current (0 / 2 mA, into its + pin) controls W1 (IT = 1 mA);
/// * each switch shunts a 1 kΩ load from the 1 V `Vdd`: `out` (S1) and
///   `out2` (W1) sit near 1 V while the switch is off and near 1 mV while
///   it is on (RON = 1 Ω, ROFF = 1 MΩ).
fn bench() -> Schematic {
    let resistor = Part::Resistor { resistance: 1e3 };
    let parts: [BenchPart; 9] = [
        (
            "Vdd",
            (-40, -12),
            Part::VoltageSource {
                source: Source::dc(1.0),
            },
            &["vdd", "0"],
        ),
        (
            "Vctl",
            (-20, -12),
            Part::VoltageSource {
                source: square(1.0),
            },
            &["ctl", "0"],
        ),
        ("R1", (0, -12), resistor.clone(), &["vdd", "out"]),
        (
            "S1",
            S1_AT,
            Part::default_for(PartKind::VoltageSwitch),
            &["out", "0", "ctl", "0"],
        ),
        (
            "Vdrv",
            (40, -12),
            Part::VoltageSource {
                source: square(2.0),
            },
            &["drv", "0"],
        ),
        ("R2", (-40, 12), resistor.clone(), &["drv", "sense"]),
        (
            "Vsense",
            (-20, 12),
            Part::VoltageSource {
                source: Source::dc(0.0),
            },
            &["sense", "0"],
        ),
        ("R3", (0, 12), resistor, &["vdd", "out2"]),
        ("W1", W1_AT, controlled_by("Vsense"), &["out2", "0"]),
    ];
    let mut s = Schematic::default();
    for (name, (x, y), part, nets) in parts {
        let id = s.allocate_id();
        // W1 stands upright, so the labels on its pins leave its body free.
        let rotation = u8::from(name == "W1");
        let c = Component {
            id,
            name: name.into(),
            position: Point::new(x, y),
            orientation: Orientation {
                rotation,
                ..Orientation::default()
            },
            part,
        };
        for (pin, net) in c.pins().into_iter().zip(nets) {
            let id = s.allocate_id();
            s.labels.push(NetLabel {
                id,
                name: net.to_string(),
                position: pin,
            });
        }
        s.components.push(c);
    }
    s
}

/// A current switch with the default model, controlled by `control`.
fn controlled_by(control: &str) -> Part {
    Part::CurrentSwitch {
        control: control.into(),
        model: CurrentSwitchModel {
            ron: 1.0,
            roff: 1e6,
            it: 1e-3,
            ih: 0.0,
            ..CurrentSwitchModel::default()
        },
    }
}

/// D17: a switch placed in the editor netlists to the engine's switch with
/// its pins on the right nets, and the circuit survives a save and reload
/// of the project unchanged.
#[gpui_kit::test]
fn a_placed_switch_circuit_netlists_and_round_trips(cx: &mut TestAppContext) {
    // The bench without its switches; they are placed where their pins
    // meet the labels left behind.
    let mut drawn = bench();
    drawn
        .components
        .retain(|c| !matches!(c.name.as_str(), "S1" | "W1"));
    let mut h = open(cx, drawn);
    h.update(|e, cx| e.place_library_item(LibraryItem::Part(PartKind::VoltageSwitch), cx));
    h.click(S1_AT.0, S1_AT.1);
    h.update(|e, cx| e.place_library_item(LibraryItem::Part(PartKind::CurrentSwitch), cx));
    let p = h.at(W1_AT.0, W1_AT.1);
    h.hover(p);
    h.keys("r");
    h.click(W1_AT.0, W1_AT.1);
    h.keys("escape");
    let w1 = h
        .schematic()
        .components
        .iter()
        .find(|c| c.name == "W1")
        .map(|c| c.id)
        .expect("W1 was placed");
    // Its control defaults to V1, which this circuit does not have.
    let error = netlist(&h.schematic()).expect_err("V1 is not in the circuit");
    assert!(
        error.to_string().contains("W1") && error.to_string().contains("\"V1\""),
        "{error}"
    );
    h.update(|e, cx| e.apply_component_edit(w1, "W1".into(), controlled_by("Vsense"), cx))
        .expect("the control is set");
    let placed = h.schematic();
    h.close();

    let n = netlist(&placed).expect("the placed circuit netlists");
    let node = |name: &str| n.circuit.find_node(name).expect("labelled net");
    let s1 = n.circuit.element("S1").expect("S1 is an element");
    assert_eq!(s1.nodes, [node("out"), 0, node("ctl"), 0]);
    assert_eq!(
        s1.device,
        Device::VoltageSwitch {
            model: SwitchModel {
                ron: 1.0,
                roff: 1e6,
                vt: 0.5,
                vh: 0.0,
                ..SwitchModel::default()
            }
        }
    );
    let w1 = n.circuit.element("W1").expect("W1 is an element");
    assert_eq!(w1.nodes, [node("out2"), 0]);
    let Device::CurrentSwitch { control, model } = &w1.device else {
        panic!("W1 is a current switch: {:?}", w1.device);
    };
    assert_eq!(
        (control.as_str(), model.it, model.ih),
        ("Vsense", 1e-3, 0.0)
    );

    let mut project = Project::new("Switches");
    project.circuits[0].schematic = placed.clone();
    let text = project.to_json();
    let reopened = Project::from_json(&text).expect("the project reloads");
    assert_eq!(reopened.project.circuits[0].schematic, placed);
    assert_eq!(reopened.project.to_json(), text, "byte for byte");
    let again = netlist(&reopened.project.circuits[0].schematic).expect("still netlists");
    assert_eq!(again.circuit, n.circuit);
}

/// The bench simulated for 4 ms: each switch is off for the first and
/// third millisecond and on for the second and fourth.
fn simulated_bench() -> ProbeContext {
    let netlist = netlist(&bench()).expect("the bench netlists");
    let analysis = Analysis::Transient(Transient {
        stop: 4e-3,
        step: 1e-6,
        start_save: 0.0,
        max_step: None,
        initial_state: InitialState::OperatingPoint,
    });
    let result = simulate(
        &netlist.circuit,
        &analysis,
        &SimOptions::default(),
        &SimControl::new(),
    )
    .expect("the bench simulates");
    ProbeContext {
        netlist: Arc::new(netlist),
        result: Arc::new(result),
    }
}

/// D17, Q62: clicking a switch probes `I(S1)` / `I(W1)`, the current into
/// P, and resting on it shows the RMS voltage across P–N, the RMS current
/// and the average power, as for any two-terminal part. The control inputs
/// of S1 carry no current, so its power is V(P, N)·I.
#[gpui_kit::test]
fn switches_probe_their_current_and_show_their_measurements(cx: &mut TestAppContext) {
    let context = simulated_bench();
    let schematic = bench();
    let s1 = schematic
        .components
        .iter()
        .find(|c| c.name == "S1")
        .map(|c| c.id)
        .expect("S1 is on the bench");
    let mut h = open(cx, schematic);
    h.update(|e, cx| e.set_probe_context(Some(context.clone()), cx));
    h.events();
    h.click(S1_AT.0, S1_AT.1);
    assert_eq!(h.read(|e| e.selection()), [ObjectRef::Component(s1)]);
    h.click(W1_AT.0, W1_AT.1);
    assert_eq!(
        probes(&mut h),
        [
            ProbeRequest::Current {
                signal: "I(S1)".into()
            },
            ProbeRequest::Current {
                signal: "I(W1)".into()
            },
        ]
    );
    for name in ["I(S1)", "I(W1)"] {
        assert!(context.result.signal(name).is_some(), "{name} is recorded");
    }

    // Off: about 1 V across and 1 µA; on: about 1 mV and 1 mA. Each half
    // of the run.
    let (v_off, i_on) = (1e6 / (1e6 + 1e3), 1.0 / (1e3 + 1.0));
    let (v_on, i_off) = (i_on * 1.0, 1.0 / (1e6 + 1e3));
    let v_rms = ((v_off * v_off + v_on * v_on) / 2.0f64).sqrt();
    let i_rms = ((i_on * i_on + i_off * i_off) / 2.0f64).sqrt();
    let p_avg = (v_off * i_off + v_on * i_on) / 2.0;
    let near = |a: Option<f64>, b: f64| a.is_some_and(|a| ((a - b) / b).abs() < 5e-3);
    for (x, y) in [S1_AT, W1_AT] {
        let p = h.at(x, y);
        h.hover(p);
        h.wait(HOVER_DELAY);
        assert!(h.control("hover-card").is_some(), "the card at x = {x}");
        let card = h.read(|e| e.hover.card.clone()).expect("a card");
        let m = card.measurement.expect("a transient has values");
        assert_eq!(m.kind, MeasurementKind::Transient);
        assert_eq!((m.voltage_label, m.current_label), ("V", "I"));
        assert!(near(m.voltage, v_rms), "x = {x}: V rms {:?}", m.voltage);
        assert!(near(m.current, i_rms), "x = {x}: I rms {:?}", m.current);
        assert!(near(m.power, p_avg), "x = {x}: P avg {:?}", m.power);
        let p = h.at(x, 0);
        h.hover(p);
        h.wait(HOVER_DELAY);
        assert!(h.read(|e| e.hover.card.is_none()), "off the part, no card");
    }
    h.close();
}

/// The engine runs the bench as built: each switch is off, then on, then
/// off and on again with its control.
#[test]
fn the_bench_switches_with_its_controls() {
    let context = simulated_bench();
    let result = &context.result;
    let at = |name: &str, t: f64| {
        let values = result
            .signal(name)
            .and_then(|s| s.real())
            .unwrap_or_else(|| panic!("{name} is recorded"));
        let k = result
            .axis
            .values
            .iter()
            .position(|&x| x >= t)
            .unwrap_or_else(|| panic!("the run reaches {t}"));
        values[k]
    };
    for out in ["V(out)", "V(out2)"] {
        for (t, high) in [
            (0.5e-3, true),
            (1.5e-3, false),
            (2.5e-3, true),
            (3.5e-3, false),
        ] {
            let v = at(out, t);
            let expected = if high {
                1e6 / (1e6 + 1e3)
            } else {
                1.0 / 1001.0
            };
            assert!((v - expected).abs() < 1e-4, "{out} at {t} s: {v}");
        }
    }
}
