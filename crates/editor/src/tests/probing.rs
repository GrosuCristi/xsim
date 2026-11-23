//! Probing after a simulation (Q62, Q130) and the hover measurement card.

use super::fixtures::rc_low_pass;
use super::harness::{Harness, open};
use crate::model::measure::{MeasurementKind, measure};
use crate::ui::hover::HOVER_DELAY;
use crate::{EditorEvent, ObjectRef, ProbeContext, ProbeRequest};
use gpui_kit::TestAppContext;
use std::sync::Arc;
use xsim_project::{Schematic, netlist};
use xsim_spice_engine::{
    AnalysisKind, Axis, Quantity, Signal, SignalData, SignalOrigin, SimulationResult,
};

const TIME: [f64; 3] = [0.0, 1e-3, 2e-3];
const V_IN: [f64; 3] = [1.0, 1.0, 1.0];
const V_OUT: [f64; 3] = [0.0, 0.5, 0.8];

fn signal(name: &str, quantity: Quantity, origin: SignalOrigin, data: &[f64]) -> Signal {
    Signal {
        name: name.into(),
        quantity,
        origin,
        data: SignalData::Real(data.to_vec()),
    }
}

/// A hand-built transient result for [`rc_low_pass`]: `in` held at 1 V,
/// `out` charging, and R1's current `(in − out) / 1 kΩ`.
pub(super) fn probe_context(s: &Schematic, analysis: AnalysisKind) -> ProbeContext {
    let netlist = netlist(s).expect("the fixture is a valid circuit");
    let node = |name: &str| netlist.circuit.find_node(name).expect("labelled node");
    let r1 = *netlist.elements.get(&2).expect("R1 is an element");
    let current: Vec<f64> = V_IN.iter().zip(V_OUT).map(|(a, b)| (a - b) / 1e3).collect();
    let result = SimulationResult {
        analysis,
        axis: Axis {
            name: "time".into(),
            unit: "s".into(),
            values: TIME.to_vec(),
        },
        signals: vec![
            signal(
                "V(in)",
                Quantity::Voltage,
                SignalOrigin::Node(node("in")),
                &V_IN,
            ),
            signal(
                "V(out)",
                Quantity::Voltage,
                SignalOrigin::Node(node("out")),
                &V_OUT,
            ),
            signal(
                "I(R1)",
                Quantity::Current,
                SignalOrigin::TerminalCurrent {
                    element: r1,
                    terminal: 0,
                },
                &current,
            ),
        ],
    };
    ProbeContext {
        netlist: Arc::new(netlist),
        result: Arc::new(result),
    }
}

fn probed(cx: &mut TestAppContext) -> Harness {
    let mut h = open(cx, rc_low_pass());
    let context = probe_context(&rc_low_pass(), AnalysisKind::Transient);
    h.update(|e, cx| e.set_probe_context(Some(context), cx));
    h.events();
    h
}

pub(super) fn probes(h: &mut Harness) -> Vec<ProbeRequest> {
    h.events()
        .into_iter()
        .filter_map(|e| match e {
            EditorEvent::Probe(request) => Some(request),
            _ => None,
        })
        .collect()
}

fn voltage(node: &str) -> ProbeRequest {
    ProbeRequest::Voltage { node: node.into() }
}

#[gpui_kit::test]
fn clicking_a_wire_or_pin_probes_its_voltage(cx: &mut TestAppContext) {
    let mut h = probed(cx);
    h.click(-4, -4);
    h.click(2, -4);
    assert_eq!(probes(&mut h), vec![voltage("in"), voltage("out")]);
    h.click(-2, 4);
    assert!(probes(&mut h).is_empty(), "ground has no voltage trace");
    h.close();
}

#[gpui_kit::test]
fn dragging_the_probe_between_nets_measures_the_difference(cx: &mut TestAppContext) {
    let mut h = probed(cx);
    h.drag((-4, -4), (4, -4));
    assert_eq!(
        probes(&mut h),
        vec![ProbeRequest::Differential {
            plus: "in".into(),
            minus: "out".into()
        }]
    );
    h.drag((-4, -4), (-6, -3));
    assert_eq!(
        probes(&mut h),
        vec![voltage("in")],
        "same net: plain voltage"
    );
    h.drag((4, -4), (-2, 4));
    assert_eq!(
        probes(&mut h),
        vec![voltage("out")],
        "to ground: plain voltage"
    );
    assert_eq!(h.schematic(), rc_low_pass(), "probing never edits");
    h.close();
}

#[gpui_kit::test]
fn holding_the_probe_shows_the_glyph_at_the_cursor(cx: &mut TestAppContext) {
    let mut h = probed(cx);
    let (a, b) = (h.at(-4, -4), h.at(0, 0));
    h.cx.simulate_mouse_move(a, None, gpui_kit::Modifiers::default());
    h.cx.simulate_mouse_down(
        a,
        gpui_kit::MouseButton::Left,
        gpui_kit::Modifiers::default(),
    );
    h.cx.simulate_mouse_move(
        b,
        Some(gpui_kit::MouseButton::Left),
        gpui_kit::Modifiers::default(),
    );
    assert!(
        matches!(h.read(|e| e.probe_glyph()), Some(p) if p.snapped() == xsim_project::Point::new(0, 0))
    );
    h.cx.simulate_mouse_up(
        b,
        gpui_kit::MouseButton::Left,
        gpui_kit::Modifiers::default(),
    );
    assert!(h.read(|e| e.probe_glyph()).is_none());
    h.close();
}

#[gpui_kit::test]
fn clicking_a_part_selects_it_and_probes_its_current(cx: &mut TestAppContext) {
    let mut h = probed(cx);
    h.click(0, -4);
    assert_eq!(
        probes(&mut h),
        vec![ProbeRequest::Current {
            signal: "I(R1)".into()
        }]
    );
    assert_eq!(h.read(|e| e.selection()), vec![ObjectRef::Component(2)]);
    h.drag((0, -4), (0, -6));
    assert!(probes(&mut h).is_empty(), "dragging a part moves it");
    assert_eq!(
        h.schematic().components[1].position,
        xsim_project::Point::new(0, -6)
    );
    h.close();
}

#[gpui_kit::test]
fn hovering_a_wire_highlights_its_net(cx: &mut TestAppContext) {
    let mut h = probed(cx);
    let p = h.at(-4, -4);
    h.hover(p);
    let net = h
        .read(|e| e.hover.net.clone())
        .expect("the net is highlighted");
    assert!(net.wires.contains(&5));
    assert_eq!(net.points.len(), 2, "V1+ and R1 pin 1");
    h.close();
}

#[gpui_kit::test]
fn resting_on_a_part_shows_rms_and_average_power(cx: &mut TestAppContext) {
    let mut h = probed(cx);
    let p = h.at(0, -4);
    h.hover(p);
    assert!(
        h.read(|e| e.hover.card.is_none()),
        "the card waits for the mouse to rest"
    );
    h.wait(HOVER_DELAY);
    assert!(
        h.control("hover-card").is_some(),
        "the card appears after the delay"
    );
    let m = h
        .read(|e| e.hover.card.clone())
        .and_then(|c| c.measurement)
        .expect("transient results have values");
    assert_eq!(m.kind, MeasurementKind::Transient);
    let close =
        |a: Option<f64>, b: f64| a.is_some_and(|a| (a - b).abs() <= 1e-9 * b.abs().max(1.0));
    // V = [1, 0.5, 0.2] interpolated linearly: ∫V² over each 1 ms step is
    // (a² + ab + b²) / 3 · 1 ms, divided by the 2 ms run.
    let v_rms = (((1.0 + 0.5 + 0.25) / 3.0 + (0.25 + 0.1 + 0.04) / 3.0) / 2.0f64).sqrt();
    assert!(close(m.voltage, v_rms), "{:?}", m.voltage);
    assert!(close(m.current, v_rms / 1e3), "{:?}", m.current);
    assert!(close(m.power, 0.385e-3), "{:?}", m.power);
    let p = h.at(0, 4);
    h.hover(p);
    h.wait(HOVER_DELAY);
    assert!(h.control("hover-card").is_none(), "ground has no card");
    h.close();
}

#[gpui_kit::test]
fn sweeps_show_the_card_without_values(cx: &mut TestAppContext) {
    let mut h = open(cx, rc_low_pass());
    let context = probe_context(&rc_low_pass(), AnalysisKind::DcSweep);
    h.update(|e, cx| e.set_probe_context(Some(context), cx));
    let p = h.at(0, -4);
    h.hover(p);
    h.wait(HOVER_DELAY);
    let card = h.read(|e| e.hover.card.clone()).expect("card shown");
    assert!(card.measurement.is_none());
    h.close();
}

#[gpui_kit::test]
fn read_only_editors_still_probe(cx: &mut TestAppContext) {
    let mut h = probed(cx);
    h.update(|e, cx| e.set_read_only(true, cx));
    h.click(2, -4);
    h.drag((0, -4), (0, -6));
    assert!(probes(&mut h).contains(&voltage("out")));
    assert_eq!(h.schematic(), rc_low_pass());
    h.update(|e, cx| e.set_probe_context(None, cx));
    h.click(2, -4);
    assert!(probes(&mut h).is_empty(), "no probing without a result");
    h.close();
}

/// Runs the real engine on the RC fixture, started from rest so that the
/// capacitor charges through R1.
fn simulated_context() -> ProbeContext {
    use xsim_spice_engine::{Analysis, InitialState, SimControl, SimOptions, Transient, simulate};
    let netlist = netlist(&rc_low_pass()).expect("valid circuit");
    let analysis = Analysis::Transient(Transient {
        stop: 5e-3,
        step: 1e-5,
        start_save: 0.0,
        max_step: None,
        initial_state: InitialState::ZeroState,
    });
    let result = simulate(
        &netlist.circuit,
        &analysis,
        &SimOptions::default(),
        &SimControl::new(),
    )
    .expect("the RC circuit simulates");
    ProbeContext {
        netlist: Arc::new(netlist),
        result: Arc::new(result),
    }
}

#[gpui_kit::test]
fn probes_and_the_hover_card_agree_with_a_real_simulation(cx: &mut TestAppContext) {
    let context = simulated_context();
    let mut h = open(cx, rc_low_pass());
    h.update(|e, cx| e.set_probe_context(Some(context.clone()), cx));
    h.events();
    h.click(2, -4);
    h.click(0, -4);
    let requests = probes(&mut h);
    assert_eq!(
        requests,
        vec![
            voltage("out"),
            ProbeRequest::Current {
                signal: "I(R1)".into()
            }
        ]
    );
    for name in ["V(out)", "V(in)", "I(R1)"] {
        assert!(
            context.result.signal(name).is_some(),
            "the result has {name}"
        );
    }
    let schematic = rc_low_pass();
    let power = |id: u64| {
        let c = schematic.component(id).expect("fixture part");
        measure(c, &context.netlist, &context.result).and_then(|m| m.power)
    };
    assert!(
        power(2).is_some_and(|p| p > 0.0),
        "R1 dissipates: {:?}",
        power(2)
    );
    assert!(
        power(1).is_some_and(|p| p < 0.0),
        "V1 delivers: {:?}",
        power(1)
    );
    h.close();
}
